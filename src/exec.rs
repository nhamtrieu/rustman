//! Full execution of one request: pre-request scripts → resolve variables → send →
//! post-response scripts. Used by the request tabs and the collection runner.

use std::sync::Arc;

use crate::http::{self, Prepared, ResponseData};
use crate::model::{Auth, AuthKind, AuthOwner, Collection, OAuthToken, Request};
use crate::oauth2;
use crate::script::{self, Info, Net, Phase, RequestJs, ResponseJs, ScriptCtx, ScriptEvent, Scopes, TestResult, VarOp};

/// Scripts of one level (collection, folders, then request), outermost first.
#[derive(Clone, Debug)]
pub struct ScriptSet {
    /// "Collection", "Folder 'Users'" or "Request", used in error messages.
    pub label: String,
    pub pre: String,
    pub post: String,
}

impl ScriptSet {
    pub fn new(label: impl Into<String>, pre: &str, post: &str) -> Self {
        Self { label: label.into(), pre: pre.to_string(), post: post.to_string() }
    }
}

/// Scripts and inherited auth for `request` in `collection` (if it is saved in one): the
/// collection's scripts, then each enclosing folder's, then the request's own.
pub fn context(collection: Option<&Collection>, request: &Request) -> (Vec<ScriptSet>, Option<Auth>, AuthOwner) {
    let mut scripts = Vec::new();
    let mut auth = None;
    let mut owner = AuthOwner::Collection;
    if let Some(c) = collection {
        scripts.push(ScriptSet::new("Collection", &c.pre_request_script, &c.post_response_script));
        for f in c.ancestors(request.id) {
            scripts.push(ScriptSet::new(format!("Folder '{}'", f.name), &f.pre_request_script, &f.post_response_script));
        }
        let (o, a) = c.inherited_auth(request.id);
        (owner, auth) = (o, Some(a.clone()));
    }
    scripts.push(ScriptSet::new("Request", &request.pre_request_script, &request.post_response_script));
    (scripts, auth, owner)
}

pub struct Job {
    pub request: Request,
    pub scripts: Vec<ScriptSet>,
    /// The auth a request with `Inherit` uses, and where it comes from.
    pub auth: Option<Auth>,
    pub auth_owner: AuthOwner,
    pub scopes: Scopes,
    pub environment_name: Option<String>,
    pub info: Info,
}

#[derive(Debug, Default)]
pub struct Report {
    pub tests: Vec<TestResult>,
    /// Script errors, e.g. `Post-response script (Request): TypeError: … (line 3)`.
    pub errors: Vec<String>,
    /// Every variable change, in order (including local ones).
    pub ops: Vec<VarOp>,
    pub next_request: Option<Option<String>>,
    pub pre_events: Vec<ScriptEvent>,
    pub post_events: Vec<ScriptEvent>,
    /// At least one script ran.
    pub ran_scripts: bool,
    /// An expired OAuth 2.0 token was refreshed before sending; the app stores it.
    pub token_update: Option<TokenUpdate>,
}

#[derive(Debug, Clone)]
pub struct TokenUpdate {
    /// The request that was sent.
    pub request: uuid::Uuid,
    /// The inherited auth the token belongs to; `None` for the request's own auth.
    pub owner: Option<AuthOwner>,
    pub token: OAuthToken,
}

impl Report {
    pub fn passed_tests(&self) -> usize {
        self.tests.iter().filter(|t| t.passed && !t.skipped).count()
    }
    /// Variable changes to persist in the workspace (not local / iteration data).
    pub fn persistent_ops(&self) -> impl Iterator<Item = &VarOp> {
        self.ops.iter().filter(|o| !matches!(o.scope(), script::Scope::Local | script::Scope::Data))
    }
}

pub struct Executed {
    /// The request as sent (`None` if it never got that far).
    pub prepared: Option<Prepared>,
    pub result: Result<ResponseData, String>,
    pub report: Report,
    /// Variables after all scripts, carried to the next request of a run.
    pub scopes: Scopes,
}

/// Runs a request with its scripts. `on_send` is called right before the HTTP call with the
/// prepared request and the pre-request console events, so the UI can log it as pending.
pub async fn run(job: Job, jar: Arc<reqwest::cookie::Jar>, on_send: impl FnOnce(&Prepared, &[ScriptEvent]) + Send) -> Executed {
    let Job { mut request, scripts, mut auth, auth_owner, mut scopes, environment_name, info } = job;
    let mut report = Report::default();
    let net = Net { handle: tokio::runtime::Handle::current(), jar: jar.clone() };

    // 1. Pre-request scripts (they may change the request and variables).
    let pres: Vec<(String, String)> = scripts.iter().filter(|s| !s.pre.trim().is_empty()).map(|s| (s.label.clone(), s.pre.clone())).collect();
    if !pres.is_empty() {
        report.ran_scripts = true;
        let (req, sc, env, inf, net) = (request.clone(), scopes.clone(), environment_name.clone(), info.clone(), net.clone());
        let outcome = tokio::task::spawn_blocking(move || run_pre(pres, req, sc, env.as_deref(), &inf, net)).await;
        let pre = match outcome {
            Ok(p) => p,
            Err(e) => PreOutcome { error: Some(format!("Pre-request script crashed: {e}")), ..PreOutcome::new(request.clone(), scopes.clone()) },
        };
        request = pre.request;
        scopes = pre.scopes;
        report.ops.extend(pre.ops);
        report.tests.extend(pre.tests);
        report.pre_events = pre.events;
        report.next_request = pre.next_request;
        if let Some(e) = pre.error {
            report.errors.push(e.clone());
            return Executed { prepared: None, result: Err(e), report, scopes };
        }
        if pre.skip {
            return Executed { prepared: None, result: Err("Request skipped by pm.execution.skipRequest()".into()), report, scopes };
        }
    }

    // 2. Refresh an expired OAuth 2.0 token.
    let inherited = request.auth.kind == AuthKind::Inherit;
    let target = if inherited { auth.as_mut() } else { Some(&mut request.auth) };
    if let Some(a) = target
        && a.kind == AuthKind::OAuth2
        && a.oauth2.should_auto_refresh(oauth2::now())
    {
        let cfg = a.oauth2.resolved(&scopes.resolved());
        let out = oauth2::refresh(&cfg, &request.settings, jar.clone()).await;
        report.pre_events.extend(out.exchanges.into_iter().map(|x| ScriptEvent::Http(Box::new(x))));
        match out.token {
            Ok(t) => {
                a.oauth2.set_token(&t);
                report.token_update = Some(TokenUpdate { request: request.id, owner: inherited.then_some(auth_owner), token: t });
            }
            Err(e) => report.pre_events.push(ScriptEvent::Log(script::LogLevel::Warn, format!("OAuth 2.0 token refresh failed: {e}"))),
        }
    }

    // 3. Resolve and send.
    let prepared = match http::prepare(&request, &scopes.resolved(), auth.as_ref()) {
        Ok(p) => p,
        Err(e) => return Executed { prepared: None, result: Err(e), report, scopes },
    };
    on_send(&prepared, &report.pre_events);
    let result = http::execute(prepared.clone(), jar).await;

    // 4. Post-response scripts (only when a response arrived, like Postman).
    let posts: Vec<(String, String)> = scripts.iter().filter(|s| !s.post.trim().is_empty()).map(|s| (s.label.clone(), s.post.clone())).collect();
    if let Ok(resp) = &result
        && !posts.is_empty()
    {
        report.ran_scripts = true;
        let req_js = RequestJs::from_prepared(&request.name, &prepared);
        let res_js = ResponseJs::from_response(resp);
        let (sc, env, inf) = (scopes.clone(), environment_name.clone(), info.clone());
        let outcome = tokio::task::spawn_blocking(move || run_post(posts, req_js, res_js, sc, env.as_deref(), &inf, net)).await;
        match outcome {
            Ok(post) => {
                scopes = post.scopes;
                report.ops.extend(post.ops);
                report.tests.extend(post.tests);
                report.post_events = post.events;
                report.errors.extend(post.errors);
                if post.next_request.is_some() {
                    report.next_request = post.next_request;
                }
            }
            Err(e) => report.errors.push(format!("Post-response script crashed: {e}")),
        }
    }

    Executed { prepared: Some(prepared), result, report, scopes }
}

struct PreOutcome {
    request: Request,
    scopes: Scopes,
    ops: Vec<VarOp>,
    tests: Vec<TestResult>,
    events: Vec<ScriptEvent>,
    next_request: Option<Option<String>>,
    skip: bool,
    error: Option<String>,
}

impl PreOutcome {
    fn new(request: Request, scopes: Scopes) -> Self {
        Self { request, scopes, ops: Vec::new(), tests: Vec::new(), events: Vec::new(), next_request: None, skip: false, error: None }
    }
}

fn run_pre(scripts: Vec<(String, String)>, request: Request, scopes: Scopes, env: Option<&str>, info: &Info, net: Net) -> PreOutcome {
    let mut o = PreOutcome::new(request, scopes);
    for (label, src) in scripts {
        let before = RequestJs::from_request(&o.request);
        let c = ScriptCtx { phase: Phase::PreRequest, scopes: &o.scopes, environment_name: env, request: &before, response: None, info };
        let r = script::run(&src, &c, Some(net.clone()));
        o.scopes.apply(&r.ops);
        o.ops.extend(r.ops);
        o.tests.extend(r.tests);
        o.events.extend(r.events);
        if r.next_request.is_some() {
            o.next_request = r.next_request;
        }
        if let Some(after) = &r.request {
            after.apply_to(&before, &mut o.request);
        }
        if let Some(e) = r.error {
            o.error = Some(format!("Error in pre-request script ({label}): {e}"));
            return o;
        }
        if r.skip {
            o.skip = true;
            return o;
        }
    }
    o
}

struct PostOutcome {
    scopes: Scopes,
    ops: Vec<VarOp>,
    tests: Vec<TestResult>,
    events: Vec<ScriptEvent>,
    errors: Vec<String>,
    next_request: Option<Option<String>>,
}

fn run_post(scripts: Vec<(String, String)>, req: RequestJs, res: ResponseJs, scopes: Scopes, env: Option<&str>, info: &Info, net: Net) -> PostOutcome {
    let mut o = PostOutcome { scopes, ops: Vec::new(), tests: Vec::new(), events: Vec::new(), errors: Vec::new(), next_request: None };
    // Unlike pre-request scripts, an error in one does not stop the next one (Postman behaviour).
    for (label, src) in scripts {
        let c = ScriptCtx { phase: Phase::Test, scopes: &o.scopes, environment_name: env, request: &req, response: Some(&res), info };
        let r = script::run(&src, &c, Some(net.clone()));
        o.scopes.apply(&r.ops);
        o.ops.extend(r.ops);
        o.tests.extend(r.tests);
        o.events.extend(r.events);
        if r.next_request.is_some() {
            o.next_request = r.next_request;
        }
        if let Some(e) = r.error {
            o.errors.push(format!("Error in post-response script ({label}): {e}"));
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyValue;

    #[test]
    fn pre_request_error_stops_and_vars_flow() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let jar = Arc::new(reqwest::cookie::Jar::default());
        let job = |pre: &str| Job {
            request: Request { name: "r".into(), url: "{{host}}/x".into(), ..Default::default() },
            scripts: vec![
                ScriptSet::new("Collection", "pm.variables.set('host', 'http://127.0.0.1:9');", ""),
                ScriptSet::new("Request", pre, "pm.test('never', () => {})"),
            ],
            auth: None,
            auth_owner: AuthOwner::Collection,
            scopes: Scopes::new(&[], &[KeyValue::new("host", "http://unused")], None, &[]),
            environment_name: None,
            info: Info::default(),
        };

        let ex = rt.block_on(run(job("throw new Error('nope')"), jar.clone(), |_, _| panic!("must not send")));
        assert!(ex.prepared.is_none());
        assert_eq!(ex.result.unwrap_err(), "Error in pre-request script (Request): Error: nope (line 1)");

        // Port 9 (discard) refuses connections: the request is sent, fails, tests are skipped.
        let mut seen = None;
        let ex = rt.block_on(run(job("pm.request.headers.add({key: 'X-Host', value: pm.variables.get('host')})"), jar, |p, _| seen = Some(p.url.clone())));
        assert_eq!(seen.as_deref(), Some("http://127.0.0.1:9/x"));
        let p = ex.prepared.unwrap();
        assert!(p.headers.iter().any(|(k, v)| k == "X-Host" && v == "http://127.0.0.1:9"));
        assert!(ex.result.is_err());
        assert!(ex.report.tests.is_empty());
        assert_eq!(ex.report.persistent_ops().count(), 0);
    }

    #[test]
    fn context_orders_scripts_and_picks_folder_auth() {
        use crate::model::{Folder, Item};
        let req = Request { pre_request_script: "r".into(), ..Default::default() };
        let mut f = Folder::new("Users");
        f.pre_request_script = "f".into();
        f.auth = Auth { kind: AuthKind::Bearer, bearer_token: "F".into(), ..Default::default() };
        let fid = f.id;
        f.items.push(Item::Request(req.clone()));
        let c = Collection { pre_request_script: "c".into(), items: vec![Item::Folder(f)], ..Default::default() };

        let (scripts, auth, owner) = context(Some(&c), &req);
        let got: Vec<(&str, &str)> = scripts.iter().map(|s| (s.label.as_str(), s.pre.as_str())).collect();
        assert_eq!(got, vec![("Collection", "c"), ("Folder 'Users'", "f"), ("Request", "r")]);
        assert_eq!((auth.unwrap().bearer_token, owner), ("F".to_string(), AuthOwner::Folder(fid)));

        let (scripts, auth, _) = context(None, &req);
        assert_eq!(scripts.len(), 1);
        assert!(auth.is_none());
    }

    /// Tiny HTTP server: `/login` returns a token, any other path echoes the request headers.
    async fn serve(listener: tokio::net::TcpListener) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = if path == "/login" {
                    r#"{"token":"T-123"}"#.to_string()
                } else {
                    let auth = req.lines().find_map(|l| l.strip_prefix("Authorization: ").or_else(|| l.strip_prefix("authorization: "))).unwrap_or("");
                    serde_json::json!({ "path": path, "auth": auth }).to_string()
                };
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    }

    #[test]
    fn end_to_end_send_request_and_tests() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(serve(listener));

        let pre = r#"
            const res = await pm.sendRequest({ url: pm.variables.replaceIn("{{base}}/login"), method: "POST", body: { mode: "raw", raw: "{}" } });
            pm.environment.set("token", res.json().token);
            pm.request.headers.upsert({ key: "Authorization", value: "Bearer {{token}}" });
        "#;
        let post = r#"
            pm.test("status", () => pm.response.to.have.status(200));
            pm.test("token sent", () => pm.expect(pm.response.json().auth).to.equal("Bearer " + pm.environment.get("token")));
            pm.test("request seen by script", () => pm.expect(pm.request.url.getPath()).to.equal("/me"));
            pm.test("fails on purpose", () => pm.expect(pm.response.json().path).to.equal("/other"));
            pm.collectionVariables.set("last", pm.response.json().path);
        "#;
        let job = Job {
            request: Request { name: "Me".into(), url: "{{base}}/me".into(), pre_request_script: pre.into(), post_response_script: post.into(), ..Default::default() },
            scripts: vec![ScriptSet::new("Request", pre, post)],
            auth: None,
            auth_owner: AuthOwner::Collection,
            scopes: Scopes::new(&[], &[KeyValue::new("base", &base)], Some(&[]), &[]),
            environment_name: Some("Dev".into()),
            info: Info::default(),
        };
        let ex = rt.block_on(run(job, Arc::new(reqwest::cookie::Jar::default()), |_, _| {}));
        assert!(ex.report.errors.is_empty(), "{:?}", ex.report.errors);
        assert_eq!(ex.result.as_ref().unwrap().status, 200);
        let results: Vec<(&str, bool)> = ex.report.tests.iter().map(|t| (t.name.as_str(), t.passed)).collect();
        assert_eq!(results, vec![("status", true), ("token sent", true), ("request seen by script", true), ("fails on purpose", false)]);
        let sends = ex.report.pre_events.iter().filter(|e| matches!(e, ScriptEvent::Http(_))).count();
        assert_eq!(sends, 1, "pm.sendRequest is reported for the console");
        let persisted: Vec<&VarOp> = ex.report.persistent_ops().collect();
        assert_eq!(persisted.len(), 2);
        assert_eq!(ex.scopes.environment.as_ref().unwrap().get("token").map(String::as_str), Some("T-123"));
        assert_eq!(ex.scopes.collection.get("last").map(String::as_str), Some("/me"));
    }
}
