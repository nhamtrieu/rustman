//! Pre-request and post-response scripts.
//!
//! Scripts are JavaScript run by QuickJS in a fresh sandbox, one per script, with a
//! Postman-compatible `pm` API defined in `script/prelude.js`. Data crosses the boundary as
//! JSON: Rust passes the variable scopes, request, response and run info in, and gets back the
//! variable changes, test results, console logs and request changes. The only native call is
//! `pm.sendRequest`, which blocks on the tokio runtime, so scripts must run on a blocking thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::{CaughtError, Context, Function, Runtime};
use serde::{Deserialize, Serialize};

use crate::http::{self, Prepared, PreparedBody, PreparedPart, ResponseData};
use crate::model::{KeyValue, Method, Request, RequestSettings};
use crate::vars::{VarMap, build_vars};

const PRELUDE: &str = include_str!("script/prelude.js");
/// `require()`, Node-style modules and the legacy globals (`CryptoJS`, `_`, `xml2Json`, ...).
const REQUIRE: &str = include_str!("script/require.js");

/// Libraries for `require()`, the ones the Postman sandbox offers (licenses in `script/lib/LICENSES.md`).
/// Evaluated only when a script requires them.
const LIBRARIES: &[(&str, &str)] = &[
    ("crypto-js", include_str!("script/lib/crypto-js.js")),
    ("lodash", include_str!("script/lib/lodash.min.js")),
    ("moment", include_str!("script/lib/moment.min.js")),
    ("tv4", include_str!("script/lib/tv4.js")),
    ("ajv", include_str!("script/lib/ajv.min.js")),
    ("chai", include_str!("script/lib/chai.js")),
];

/// Source of a bundled library.
fn library(name: &str) -> Option<&'static str> {
    LIBRARIES.iter().find(|(n, _)| *n == name).map(|(_, src)| *src)
}

/// Random bytes for `crypto.getRandomValues` (crypto-js, uuid).
fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n.min(65536)];
    if getrandom::fill(&mut buf).is_err() {
        // Practically unreachable; keep scripts running with weaker randomness.
        for chunk in buf.chunks_mut(16) {
            let u = uuid::Uuid::new_v4();
            chunk.copy_from_slice(&u.as_bytes()[..chunk.len()]);
        }
    }
    buf
}

/// Requests made by `pm.sendRequest` during one script run.
type Sent = Rc<RefCell<Vec<(Prepared, Result<ResponseData, String>)>>>;
/// Wall-clock limit of one script, excluding nothing: `pm.sendRequest` time counts too.
pub const TIMEOUT: Duration = Duration::from_secs(30);
const MEMORY_LIMIT: usize = 128 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Variables
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Globals,
    Collection,
    Environment,
    Data,
    Local,
}

/// A variable change made by a script, applied to the workspace afterwards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum VarOp {
    Set { scope: Scope, key: String, value: String },
    Unset { scope: Scope, key: String },
    Clear { scope: Scope },
}

impl VarOp {
    pub fn scope(&self) -> Scope {
        match self {
            VarOp::Set { scope, .. } | VarOp::Unset { scope, .. } | VarOp::Clear { scope } => *scope,
        }
    }

    /// Applies the change to a workspace variable list (keeps order; re-enables a disabled entry).
    pub fn apply_to(&self, list: &mut Vec<KeyValue>) {
        match self {
            VarOp::Set { key, value, .. } => {
                let pos = list.iter().position(|kv| kv.enabled && kv.key == *key).or_else(|| list.iter().position(|kv| kv.key == *key));
                match pos {
                    Some(i) => {
                        list[i].value = value.clone();
                        list[i].enabled = true;
                    }
                    None => list.push(KeyValue::new(key.clone(), value.clone())),
                }
            }
            VarOp::Unset { key, .. } => list.retain(|kv| kv.key != *key),
            VarOp::Clear { .. } => list.clear(),
        }
    }
}

/// Every variable scope visible to a script, as plain maps.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Scopes {
    pub globals: VarMap,
    pub collection: VarMap,
    /// `None` when no environment is active.
    pub environment: Option<VarMap>,
    /// Current dataset row of a collection run.
    pub data: VarMap,
    /// `pm.variables.set(...)`: lives for one request execution.
    pub local: VarMap,
}

impl Scopes {
    pub fn new(globals: &[KeyValue], collection: &[KeyValue], environment: Option<&[KeyValue]>, data: &[KeyValue]) -> Self {
        Self {
            globals: build_vars(&[globals]),
            collection: build_vars(&[collection]),
            environment: environment.map(|e| build_vars(&[e])),
            data: build_vars(&[data]),
            local: VarMap::new(),
        }
    }

    /// Variables used for `{{...}}` substitution: globals < collection < environment < data < local.
    pub fn resolved(&self) -> VarMap {
        let mut map = self.globals.clone();
        map.extend(self.collection.clone());
        if let Some(env) = &self.environment {
            map.extend(env.clone());
        }
        map.extend(self.data.clone());
        map.extend(self.local.clone());
        map
    }

    pub fn apply(&mut self, ops: &[VarOp]) {
        for op in ops {
            let map = match op.scope() {
                Scope::Globals => &mut self.globals,
                Scope::Collection => &mut self.collection,
                Scope::Environment => match &mut self.environment {
                    Some(e) => e,
                    None => continue,
                },
                Scope::Data => &mut self.data,
                Scope::Local => &mut self.local,
            };
            match op {
                VarOp::Set { key, value, .. } => {
                    map.insert(key.clone(), value.clone());
                }
                VarOp::Unset { key, .. } => {
                    map.remove(key);
                }
                VarOp::Clear { .. } => map.clear(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Data passed to / returned from the sandbox
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Phase {
    #[serde(rename = "prerequest")]
    PreRequest,
    #[serde(rename = "test")]
    Test,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderJs {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BodyJs {
    pub mode: String,
    pub raw: String,
}

/// `pm.request` as seen by a script.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestJs {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderJs>,
    pub body: BodyJs,
}

impl RequestJs {
    /// The editable request, before variables are resolved (pre-request phase).
    pub fn from_request(r: &Request) -> Self {
        Self {
            name: r.name.clone(),
            method: r.method.as_str().into(),
            url: r.url.clone(),
            headers: r.headers.iter().filter(|h| !h.key.is_empty()).map(|h| HeaderJs { key: h.key.clone(), value: h.value.clone(), disabled: !h.enabled }).collect(),
            body: BodyJs { mode: body_mode_name(r), raw: if r.body.mode == crate::model::BodyMode::Raw { r.body.raw.clone() } else { String::new() } },
        }
    }

    /// The request as it was sent (post-response phase).
    pub fn from_prepared(name: &str, p: &Prepared) -> Self {
        let raw = match &p.body {
            PreparedBody::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        };
        let mode = match &p.body {
            PreparedBody::None => "none",
            PreparedBody::Bytes(_) => "raw",
            PreparedBody::File(_) => "file",
            PreparedBody::Multipart(_) => "formdata",
        };
        Self {
            name: name.into(),
            method: p.method.as_str().into(),
            url: p.url.clone(),
            headers: p.headers.iter().map(|(k, v)| HeaderJs { key: k.clone(), value: v.clone(), disabled: false }).collect(),
            body: BodyJs { mode: mode.into(), raw },
        }
    }

    /// Writes the changes a pre-request script made back into the request.
    pub fn apply_to(&self, original: &RequestJs, r: &mut Request) {
        if self.method != original.method
            && let Some(m) = Method::parse(&self.method)
        {
            r.method = m;
        }
        if self.url != original.url {
            r.url = self.url.clone();
            r.sync_params_from_url();
        }
        if self.headers != original.headers {
            let mut headers: Vec<KeyValue> = self
                .headers
                .iter()
                .map(|h| {
                    let description = r.headers.iter().find(|o| o.key == h.key).map(|o| o.description.clone()).unwrap_or_default();
                    KeyValue { enabled: !h.disabled, key: h.key.clone(), value: h.value.clone(), description }
                })
                .collect();
            // Keep blank rows the script never saw.
            headers.extend(r.headers.iter().filter(|h| h.key.is_empty()).cloned());
            r.headers = headers;
        }
        if self.body.raw != original.body.raw {
            if r.body.mode == crate::model::BodyMode::None {
                r.body.mode = crate::model::BodyMode::Raw;
            }
            if r.body.mode == crate::model::BodyMode::Raw {
                r.body.raw = self.body.raw.clone();
            }
        }
    }
}

fn body_mode_name(r: &Request) -> String {
    use crate::model::BodyMode;
    match r.body.mode {
        BodyMode::None => "none",
        BodyMode::Raw => "raw",
        BodyMode::UrlEncoded => "urlencoded",
        BodyMode::FormData => "formdata",
        BodyMode::Binary => "file",
        BodyMode::GraphQl => "graphql",
    }
    .into()
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseJs {
    pub code: u16,
    pub status: String,
    pub headers: Vec<HeaderJs>,
    pub body: String,
    pub response_time: u64,
    pub response_size: usize,
    pub cookies: Vec<CookieJs>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CookieJs {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

impl ResponseJs {
    pub fn from_response(r: &ResponseData) -> Self {
        Self {
            code: r.status,
            status: r.status_text.clone(),
            headers: r.headers.iter().map(|(k, v)| HeaderJs { key: k.clone(), value: v.clone(), disabled: false }).collect(),
            body: r.text(),
            response_time: r.duration.as_millis() as u64,
            response_size: r.body.len(),
            cookies: r.cookies.iter().map(|c| CookieJs { name: c.name.clone(), value: c.value.clone(), domain: c.domain.clone(), path: c.path.clone() }).collect(),
        }
    }
}

/// `pm.info`.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub request_name: String,
    pub request_id: String,
    /// 0-based.
    pub iteration: u32,
    pub iteration_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Input<'a> {
    phase: Phase,
    scopes: &'a Scopes,
    environment_name: Option<&'a str>,
    request: &'a RequestJs,
    response: Option<&'a ResponseJs>,
    info: &'a Info,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Log,
    Info,
    Warn,
    Error,
}

/// Something a script did that is shown in the console, in order.
#[derive(Clone, Debug)]
pub enum ScriptEvent {
    Log(LogLevel, String),
    /// A `pm.sendRequest` call.
    Http(Box<(Prepared, Result<ResponseData, String>)>),
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum EventJs {
    Log { level: LogLevel, text: String },
    Http { index: usize },
}

#[derive(Deserialize)]
struct NextRequestJs {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct OutputJs {
    ops: Vec<VarOp>,
    tests: Vec<TestResult>,
    events: Vec<EventJs>,
    request: Option<RequestJs>,
    next_request: Option<NextRequestJs>,
    skip: bool,
    error: Option<String>,
}

/// What one script run produced.
#[derive(Debug, Default)]
pub struct ScriptRun {
    pub ops: Vec<VarOp>,
    pub tests: Vec<TestResult>,
    pub events: Vec<ScriptEvent>,
    /// Pre-request phase: the request after the script's changes.
    pub request: Option<RequestJs>,
    /// `Some(None)`: stop the run; `Some(Some(name))`: jump to that request.
    pub next_request: Option<Option<String>>,
    /// `pm.execution.skipRequest()` was called.
    pub skip: bool,
    pub error: Option<String>,
}

/// Lets `pm.sendRequest` reach the network.
#[derive(Clone)]
pub struct Net {
    pub handle: tokio::runtime::Handle,
    pub jar: Arc<reqwest::cookie::Jar>,
}

pub struct ScriptCtx<'a> {
    pub phase: Phase,
    pub scopes: &'a Scopes,
    pub environment_name: Option<&'a str>,
    pub request: &'a RequestJs,
    pub response: Option<&'a ResponseJs>,
    pub info: &'a Info,
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/// Runs one script. Blocks (call from a blocking thread when `net` is set).
pub fn run(source: &str, c: &ScriptCtx, net: Option<Net>) -> ScriptRun {
    run_with_timeout(source, c, net, TIMEOUT)
}

fn run_with_timeout(source: &str, c: &ScriptCtx, net: Option<Net>, timeout: Duration) -> ScriptRun {
    let input = Input { phase: c.phase, scopes: c.scopes, environment_name: c.environment_name, request: c.request, response: c.response, info: c.info };
    let input_json = match serde_json::to_string(&input) {
        Ok(s) => s,
        Err(e) => return ScriptRun { error: Some(format!("Cannot prepare script input: {e}")), ..Default::default() },
    };
    match run_inner(source, &input_json, net, timeout) {
        Ok(r) => r,
        Err(e) => ScriptRun { error: Some(e), ..Default::default() },
    }
}

fn run_inner(source: &str, input_json: &str, net: Option<Net>, timeout: Duration) -> Result<ScriptRun, String> {
    let rt = Runtime::new().map_err(|e| format!("Cannot start the script engine: {e}"))?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(1024 * 1024);
    let started = Instant::now();
    let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        // Interrupt once at the deadline, then allow a short grace period so `__finish` can run.
        let timed_out = timed_out.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            let elapsed = started.elapsed();
            if elapsed < timeout {
                return false;
            }
            let first = !timed_out.swap(true, std::sync::atomic::Ordering::Relaxed);
            first || elapsed > timeout + Duration::from_secs(2)
        })));
    }
    let ctx = Context::full(&rt).map_err(|e| format!("Cannot start the script engine: {e}"))?;
    let sent: Sent = Rc::default();

    let out_json: String = ctx.with(|ctx| -> Result<String, String> {
        let globals = ctx.globals();
        globals.set("__input", input_json).map_err(|e| e.to_string())?;
        let sent_in = sent.clone();
        let send = Function::new(ctx.clone(), move |payload: String| -> String { send_request(&payload, net.as_ref(), &sent_in) }).map_err(|e| e.to_string())?;
        globals.set("__send", send).map_err(|e| e.to_string())?;
        let lib = Function::new(ctx.clone(), |name: String| -> Option<String> { library(&name).map(str::to_string) }).map_err(|e| e.to_string())?;
        globals.set("__lib", lib).map_err(|e| e.to_string())?;
        let random = Function::new(ctx.clone(), |n: usize| -> Vec<u8> { random_bytes(n) }).map_err(|e| e.to_string())?;
        globals.set("__random", random).map_err(|e| e.to_string())?;

        let mut opts = EvalOptions::default();
        opts.filename = Some("rustman".into());
        let r: rquickjs::Result<()> = ctx.eval_with_options(PRELUDE, opts);
        r.map_err(|e| format!("Script engine error: {}", describe(CaughtError::from_error(&ctx, e))))?;
        let mut opts = EvalOptions::default();
        opts.filename = Some("rustman-require".into());
        let r: rquickjs::Result<()> = ctx.eval_with_options(REQUIRE, opts);
        r.map_err(|e| format!("Script engine error: {}", describe(CaughtError::from_error(&ctx, e))))?;

        // The script body stays on line 1 so reported line numbers match the editor.
        let wrapped = format!("__run(async function () {{{source}\n}});");
        let mut opts = EvalOptions::default();
        opts.filename = Some("script".into());
        opts.strict = false;
        let r: rquickjs::Result<()> = ctx.eval_with_options(wrapped, opts);
        let syntax_error = r.err().map(|e| describe(CaughtError::from_error(&ctx, e)));
        while ctx.execute_pending_job() {}

        let finish: Function = globals.get("__finish").map_err(|e| e.to_string())?;
        let out: rquickjs::Result<String> = finish.call(());
        let out = out.map_err(|e| describe(CaughtError::from_error(&ctx, e)))?;
        match syntax_error {
            // Keep the logs and patch the error in.
            Some(err) => {
                let mut v: serde_json::Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
                v["error"] = serde_json::Value::String(err);
                Ok(v.to_string())
            }
            None => Ok(out),
        }
    })?;

    let out: OutputJs = serde_json::from_str(&out_json).map_err(|e| format!("Invalid script output: {e}"))?;
    let mut sent: Vec<Option<(Prepared, Result<ResponseData, String>)>> = sent.take().into_iter().map(Some).collect();
    let events = out
        .events
        .into_iter()
        .filter_map(|e| match e {
            EventJs::Log { level, text } => Some(ScriptEvent::Log(level, text)),
            EventJs::Http { index } => sent.get_mut(index).and_then(Option::take).map(|x| ScriptEvent::Http(Box::new(x))),
        })
        .collect();
    let mut error = out.error;
    if timed_out.load(std::sync::atomic::Ordering::Relaxed) {
        error = Some(format!("Script timed out after {} s", timeout.as_secs_f32()));
    }
    Ok(ScriptRun {
        ops: out.ops,
        tests: out.tests,
        events,
        request: out.request,
        next_request: out.next_request.map(|n| n.name),
        skip: out.skip,
        error,
    })
}

/// `Name: message (line N)` from a QuickJS error.
fn describe(e: CaughtError) -> String {
    match e {
        CaughtError::Exception(ex) => {
            let name: String = ex.get("name").unwrap_or_else(|_| "Error".into());
            let message = ex.message().unwrap_or_default();
            let line = ex.stack().as_deref().and_then(script_line).or_else(|| ex.get::<_, i32>("lineNumber").ok().filter(|n| *n > 0).map(|n| n as u32));
            match line {
                Some(n) => format!("{name}: {message} (line {n})"),
                None => format!("{name}: {message}"),
            }
        }
        CaughtError::Value(v) => format!("Uncaught {v:?}"),
        CaughtError::Error(e) => e.to_string(),
    }
}

fn script_line(stack: &str) -> Option<u32> {
    let i = stack.find("script:")?;
    stack[i + 7..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

#[derive(Deserialize)]
struct SendSpec {
    url: String,
    method: String,
    #[serde(default)]
    headers: Vec<HeaderJs>,
    #[serde(default)]
    body: Option<SendBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendBody {
    raw: Option<String>,
    urlencoded: Option<Vec<(String, String)>>,
    formdata: Option<Vec<(String, String)>>,
    content_type: Option<String>,
}

/// Native side of `pm.sendRequest`: returns `{index, error, response}` as JSON.
fn send_request(payload: &str, net: Option<&Net>, sent: &Sent) -> String {
    let reply = |index: Option<usize>, result: &Result<ResponseData, String>| {
        let (error, response) = match result {
            Ok(r) => (None, Some(ResponseJs::from_response(r))),
            Err(e) => (Some(e.clone()), None),
        };
        serde_json::json!({ "index": index, "error": error, "response": response }).to_string()
    };
    let spec: SendSpec = match serde_json::from_str(payload) {
        Ok(s) => s,
        Err(e) => return reply(None, &Err(format!("Invalid pm.sendRequest options: {e}"))),
    };
    let Some(net) = net else { return reply(None, &Err("pm.sendRequest is not available here".into())) };
    let prepared = match prepare_send(spec) {
        Ok(p) => p,
        Err(e) => return reply(None, &Err(e)),
    };
    let result = net.handle.block_on(http::execute(prepared.clone(), net.jar.clone()));
    let out = reply(Some(sent.borrow().len()), &result);
    sent.borrow_mut().push((prepared, result));
    out
}

fn prepare_send(spec: SendSpec) -> Result<Prepared, String> {
    let method = Method::parse(&spec.method).ok_or_else(|| format!("Unsupported method '{}'", spec.method))?;
    let mut url = spec.url.trim().to_string();
    if url.is_empty() {
        return Err("pm.sendRequest: URL is empty".into());
    }
    if !url.contains("://") {
        url = format!("http://{url}");
    }
    let mut headers: Vec<(String, String)> = spec.headers.into_iter().filter(|h| !h.disabled && !h.key.is_empty()).map(|h| (h.key, h.value)).collect();
    let has = |hs: &[(String, String)], n: &str| hs.iter().any(|(k, _)| k.eq_ignore_ascii_case(n));
    let body = match spec.body {
        None => PreparedBody::None,
        Some(SendBody { formdata: Some(parts), .. }) => {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
            PreparedBody::Multipart(parts.into_iter().map(|(name, value)| PreparedPart { name, value, is_file: false }).collect())
        }
        Some(SendBody { urlencoded: Some(pairs), .. }) => {
            if !has(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
            }
            let s: Vec<String> = pairs.iter().map(|(k, v)| format!("{}={}", http::encode_form(k), http::encode_form(v))).collect();
            PreparedBody::Bytes(s.join("&").into_bytes())
        }
        Some(SendBody { raw, content_type, .. }) => {
            if let Some(ct) = content_type
                && !has(&headers, "content-type")
            {
                headers.push(("Content-Type".into(), ct));
            }
            match raw {
                Some(r) if !r.is_empty() => PreparedBody::Bytes(r.into_bytes()),
                _ => PreparedBody::None,
            }
        }
    };
    if !has(&headers, "user-agent") {
        headers.push(("User-Agent".into(), concat!("Rustman/", env!("CARGO_PKG_VERSION")).into()));
    }
    Ok(Prepared { method, url, headers, body, settings: RequestSettings::default(), digest: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_run(phase: Phase, src: &str, scopes: &Scopes, response: Option<&ResponseJs>) -> ScriptRun {
        let request = RequestJs {
            name: "Get user".into(),
            method: "GET".into(),
            url: "{{base}}/users?id=1".into(),
            headers: vec![HeaderJs { key: "Accept".into(), value: "*/*".into(), disabled: false }],
            body: BodyJs { mode: "raw".into(), raw: "{}".into() },
        };
        let info = Info { request_name: "Get user".into(), request_id: "r1".into(), iteration: 0, iteration_count: 1 };
        run(src, &ScriptCtx { phase, scopes, environment_name: Some("Dev"), request: &request, response, info: &info }, None)
    }

    fn scopes() -> Scopes {
        Scopes::new(&[KeyValue::new("g", "1")], &[KeyValue::new("base", "http://x.test")], Some(&[KeyValue::new("token", "abc")]), &[])
    }

    fn response() -> ResponseJs {
        ResponseJs {
            code: 200,
            status: "OK".into(),
            headers: vec![HeaderJs { key: "Content-Type".into(), value: "application/json".into(), disabled: false }],
            body: r#"{"id": 7, "name": "Nham", "tags": ["a", "b"], "nested": {"x": [1, {"y": true}]}}"#.into(),
            response_time: 42,
            response_size: 60,
            cookies: vec![],
        }
    }

    #[test]
    fn variables_and_request_changes() {
        let s = scopes();
        let r = ctx_run(
            Phase::PreRequest,
            r#"
            pm.environment.set("ts", 5);
            pm.collectionVariables.set("obj", {a: 1});
            pm.globals.unset("g");
            pm.variables.set("local", "L");
            console.log("token", pm.environment.get("token"), pm.variables.get("base"), pm.environment.name);
            pm.request.headers.add({key: "X-Sig", value: pm.variables.replaceIn("{{token}}-{{local}}")});
            pm.request.headers.upsert({key: "accept", value: "application/json"});
            pm.request.url.query.upsert({key: "id", value: 2});
            pm.request.method = "post";
            "#,
            &s,
            None,
        );
        assert_eq!(r.error, None);
        assert_eq!(
            r.ops,
            vec![
                VarOp::Set { scope: Scope::Environment, key: "ts".into(), value: "5".into() },
                VarOp::Set { scope: Scope::Collection, key: "obj".into(), value: r#"{"a":1}"#.into() },
                VarOp::Unset { scope: Scope::Globals, key: "g".into() },
                VarOp::Set { scope: Scope::Local, key: "local".into(), value: "L".into() },
            ]
        );
        match &r.events[0] {
            ScriptEvent::Log(LogLevel::Log, t) => assert_eq!(t, "token abc http://x.test Dev"),
            e => panic!("unexpected event {e:?}"),
        }
        let req = r.request.unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(req.url, "{{base}}/users?id=2");
        assert_eq!(req.headers[0].value, "application/json");
        assert_eq!(req.headers[1], HeaderJs { key: "X-Sig".into(), value: "abc-L".into(), disabled: false });

        let mut s2 = s.clone();
        s2.apply(&r.ops);
        assert_eq!(s2.resolved().get("local").map(String::as_str), Some("L"));
        assert!(!s2.globals.contains_key("g"));
    }

    /// Runs a post-response script and expects every `pm.test` in it to pass.
    fn all_pass(src: &str) -> ScriptRun {
        let s = scopes();
        let r = ctx_run(Phase::Test, src, &s, Some(&response()));
        assert_eq!(r.error, None, "script error");
        let failed: Vec<_> = r.tests.iter().filter(|t| !t.passed).map(|t| format!("{}: {:?}", t.name, t.error)).collect();
        assert!(failed.is_empty(), "failed tests: {failed:#?}");
        assert!(!r.tests.is_empty());
        r
    }

    #[test]
    fn require_crypto_js() {
        // Expected values computed with Python's hmac/hashlib.
        all_pass(r#"
            const CryptoJS = require('crypto-js');
            pm.test('hmac base64', () => pm.expect(CryptoJS.HmacSHA256('message', 'secret').toString(CryptoJS.enc.Base64)).to.equal('i19IcCmVwVmMVz2x4hhmqbgl1KeU0WnXBgoDYFeWNgs='));
            pm.test('hmac hex', () => pm.expect(CryptoJS.HmacSHA256('message', 'secret').toString()).to.equal('8b5f48702995c1598c573db1e21866a9b825d4a794d169d7060a03605796360b'));
            pm.test('md5', () => pm.expect(CryptoJS.MD5('abc').toString()).to.equal('900150983cd24fb0d6963f7d28e17f72'));
            pm.test('sha512', () => pm.expect(CryptoJS.SHA512('abc').toString().slice(0, 32)).to.equal('ddaf35a193617abacc417349ae204131'));
            pm.test('utf8 base64', () => pm.expect(CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('Xin chào 👋'))).to.equal('WGluIGNow6BvIPCfkYs='));
            // AES with a passphrase needs a random salt (crypto.getRandomValues).
            const enc = CryptoJS.AES.encrypt('top secret', 'pass').toString();
            pm.test('aes round trip', () => pm.expect(CryptoJS.AES.decrypt(enc, 'pass').toString(CryptoJS.enc.Utf8)).to.equal('top secret'));
            pm.test('random salt', () => pm.expect(CryptoJS.AES.encrypt('top secret', 'pass').toString()).to.not.equal(enc));
            pm.test('legacy global', () => pm.expect(CryptoJS).to.equal(require('crypto-js')));
            pm.test('global without require', () => pm.expect(globalThis.CryptoJS.SHA1('abc').toString()).to.equal('a9993e364706816aba3e25717850c26c9cd0d89d'));
        "#);
    }

    #[test]
    fn require_lodash_moment_and_validators() {
        all_pass(r#"
            const _ = require('lodash');
            const body = pm.response.json();
            pm.test('lodash', () => {
                pm.expect(_.chunk([1, 2, 3, 4, 5], 2)).to.eql([[1, 2], [3, 4], [5]]);
                pm.expect(_.get(body, 'nested.x[1].y')).to.equal(true);
                pm.expect(_.map(_.sortBy([{n: 2}, {n: 1}], 'n'), 'n')).to.eql([1, 2]);
            });
            pm.test('lodash global', () => pm.expect(globalThis._.VERSION).to.equal('4.17.21'));
            const moment = require('moment');
            pm.test('moment', () => {
                pm.expect(moment.utc('2024-01-31T10:00:00Z').add(1, 'month').format('YYYY-MM-DD')).to.equal('2024-02-29');
                pm.expect(moment('2024-03-01', 'YYYY-MM-DD').isValid()).to.equal(true);
                pm.expect(moment().year()).to.be.above(2000);
            });
            const schema = { type: 'object', required: ['id', 'name'], properties: { id: { type: 'integer' }, name: { type: 'string' } } };
            pm.test('tv4', () => {
                pm.expect(tv4.validate(body, schema)).to.equal(true);
                pm.expect(tv4.validate({ id: 'x' }, schema)).to.equal(false);
            });
            const Ajv = require('ajv');
            pm.test('ajv', () => {
                const ajv = new Ajv({ allErrors: true });
                pm.expect(ajv.validate(schema, body)).to.equal(true);
                pm.expect(ajv.validate(schema, { id: 1.5 })).to.equal(false);
                pm.expect(ajv.errors.length).to.equal(2);
            });
            const { expect } = require('chai');
            pm.test('chai', () => {
                expect(body).to.have.property('name', 'Nham');
                expect(() => expect(1).to.equal(2)).to.throw('expected 1 to equal 2');
            });
            pm.test('second require is cached', () => pm.expect(require('lodash')).to.equal(_));
        "#);
    }

    #[test]
    fn require_node_style_modules() {
        all_pass(r#"
            const { Buffer } = require('buffer');
            pm.test('buffer utf8/base64/hex', () => {
                pm.expect(Buffer.from('Xin chào 👋').toString('base64')).to.equal('WGluIGNow6BvIPCfkYs=');
                pm.expect(Buffer.from('WGluIGNow6BvIPCfkYs=', 'base64').toString()).to.equal('Xin chào 👋');
                pm.expect(Buffer.from('Xin chào 👋').toString('hex')).to.equal('58696e206368c3a06f20f09f918b');
                pm.expect(Buffer.from('58696e', 'hex').toString('utf8')).to.equal('Xin');
                pm.expect(Buffer.from('user:pàss').toString('base64')).to.equal('dXNlcjpww6Bzcw==');
                pm.expect(Buffer.concat([Buffer.from('a'), Buffer.from('bc')]).toString()).to.equal('abc');
                pm.expect(Buffer.byteLength('👋')).to.equal(4);
                pm.expect(globalThis.Buffer).to.equal(Buffer);
            });
            pm.test('uuid', () => {
                const re = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
                const uuid = require('uuid');
                pm.expect(uuid.v4()).to.match(re);
                pm.expect(uuid()).to.match(re);
                pm.expect(uuid.v4()).to.not.equal(uuid.v4());
            });
            const qs = require('querystring');
            pm.test('querystring', () => {
                pm.expect(qs.parse('a=1&b=x%20y&a=2&c')).to.eql({ a: ['1', '2'], b: 'x y', c: '' });
                pm.expect(qs.stringify({ q: 'a b&c', n: [1, 2] })).to.equal('q=a%20b%26c&n=1&n=2');
            });
            pm.test('util.format', () => pm.expect(require('util').format('%s has %d items: %j', 'cart', 2, { a: 1 })).to.equal('cart has 2 items: {"a":1}'));
            pm.test('TextEncoder', () => pm.expect(new TextDecoder().decode(new TextEncoder().encode('ồ'))).to.equal('ồ'));
            pm.test('node: prefix', () => pm.expect(require('node:buffer').Buffer).to.equal(Buffer));
        "#);
    }

    #[test]
    fn xml2js_and_xml2json() {
        all_pass(r#"
            const xml = '<?xml version="1.0"?><!-- c --><root a="1"><item>x</item><item>y</item><name>N &amp; M</name><empty/><p id="7">Hi <b>there</b> you</p><raw><![CDATA[<b>&</b>]]></raw></root>';
            pm.test('xml2Json (Postman options)', () => {
                pm.expect(xml2Json(xml)).to.eql({ root: { $: { a: '1' }, item: ['x', 'y'], name: 'N & M', empty: '', p: { $: { id: '7' }, _: 'Hi  you', b: 'there' }, raw: '<b>&</b>' } });
            });
            const xml2js = require('xml2js');
            let parsed;
            xml2js.parseString('<r><a x="1">v</a><a>w</a><n>&#65;&#x42;&lt;</n></r>', (err, res) => { parsed = res; });
            pm.test('parseString defaults (explicitArray)', () => pm.expect(parsed).to.eql({ r: { a: [{ $: { x: '1' }, _: 'v' }, 'w'], n: ['AB<'] } }));
            pm.test('mergeAttrs, explicitRoot', () => {
                new xml2js.Parser({ mergeAttrs: true, explicitArray: false, explicitRoot: false }).parseString('<r><a x="1">v</a></r>', (e, res) => {
                    pm.expect(res).to.eql({ a: { x: '1', _: 'v' } });
                });
            });
            pm.test('namespaced SOAP', () => {
                const soap = '<soap:Envelope xmlns:soap="http://s"><soap:Body><m:Resp><m:Code>0</m:Code></m:Resp></soap:Body></soap:Envelope>';
                pm.expect(xml2Json(soap)['soap:Envelope']['soap:Body']['m:Resp']['m:Code']).to.equal('0');
            });
            pm.test('errors', () => {
                let error;
                xml2js.parseString('<a><b></a>', (e) => { error = e; });
                pm.expect(error.message).to.include('Unexpected end tag </a>');
                pm.expect(() => xml2Json('not xml')).to.throw();
            });
        "#);
    }

    #[test]
    fn require_unknown_module() {
        let s = scopes();
        let r = ctx_run(Phase::Test, "require('cheerio')", &s, Some(&response()));
        let e = r.error.unwrap();
        assert!(e.contains("Cannot find module 'cheerio'") && e.contains("crypto-js, lodash, moment"), "{e}");
    }

    #[test]
    fn tests_and_assertions() {
        let res = response();
        let r = ctx_run(
            Phase::Test,
            r#"
            pm.test("status", () => pm.response.to.have.status(200));
            pm.test("ok", () => { pm.response.to.be.ok; pm.response.to.be.success; pm.response.to.not.be.error; });
            pm.test("header", () => pm.response.to.have.header("content-type", "application/json"));
            pm.test("json", function () {
                const j = pm.response.json();
                pm.expect(j.id).to.eql(7);
                pm.expect(j).to.have.property("name").that.is.a("string").and.equal("Nham");
                pm.expect(j.tags).to.have.lengthOf(2).and.include("b");
                pm.expect(j.tags).to.have.length.above(1);
                pm.expect(j).to.include.keys("id", "tags");
                pm.expect(j).to.have.nested.property("nested.x[1].y", true);
                pm.expect(j).to.deep.include({tags: ["a", "b"]});
                pm.expect(j.tags).to.have.members(["b", "a"]);
                pm.expect(pm.response.responseTime).to.be.below(100);
                pm.expect([]).to.be.empty;
                pm.expect(null).to.be.null;
                pm.expect("abc").to.match(/b/);
                pm.expect(3).to.be.oneOf([1, 2, 3]);
                pm.response.to.have.jsonBody("nested.x[0]", 1);
                pm.response.to.have.jsonSchema({type: "object", required: ["id", "name"], properties: {id: {type: "integer"}, tags: {type: "array", items: {type: "string"}}}});
            });
            pm.test("fails", () => pm.expect(pm.response.code).to.equal(201));
            pm.test("bad schema", () => pm.response.to.have.jsonSchema({type: "object", properties: {id: {type: "string"}}}));
            pm.test("throws", () => { undefinedFn(); });
            pm.test.skip("later");
            tests["legacy"] = responseCode.code === 200;
            "#,
            &scopes(),
            Some(&res),
        );
        assert_eq!(r.error, None);
        let by_name = |n: &str| r.tests.iter().find(|t| t.name == n).unwrap_or_else(|| panic!("no test {n}"));
        for n in ["status", "ok", "header", "json", "legacy"] {
            assert!(by_name(n).passed, "{n}: {:?}", by_name(n).error);
        }
        assert_eq!(by_name("fails").error.as_deref(), Some("AssertionError: expected 200 to equal 201 (line 23)"));
        assert!(by_name("bad schema").error.as_deref().unwrap().contains("data.id should be string but is number"));
        assert!(by_name("throws").error.as_deref().unwrap().starts_with("ReferenceError"));
        assert!(by_name("later").skipped);
    }

    #[test]
    fn errors_report_line_and_keep_logs() {
        let r = ctx_run(Phase::PreRequest, "console.log('before');\nlet x = 1;\nnotDefined.foo();", &scopes(), None);
        assert_eq!(r.error.as_deref(), Some("ReferenceError: notDefined is not defined (line 3)"));
        assert_eq!(r.events.len(), 1);

        let r = ctx_run(Phase::PreRequest, "let a = ;", &scopes(), None);
        assert!(r.error.as_deref().unwrap().starts_with("SyntaxError"), "{:?}", r.error);

        let r = ctx_run(Phase::PreRequest, "pm.execution.setNextRequest(null); pm.execution.skipRequest();", &scopes(), None);
        assert_eq!(r.next_request, Some(None));
        assert!(r.skip);
    }

    #[test]
    fn no_environment_and_send_without_network() {
        let s = Scopes::new(&[], &[], None, &[KeyValue::new("row", "r1")]);
        let r = ctx_run(
            Phase::PreRequest,
            r#"pm.environment.set("a", "b");
               pm.expect(pm.iterationData.get("row")).to.equal("r1");
               pm.sendRequest("http://x.test", (err, res) => { console.log(err.message); });
               btoa(atob(btoa("hi")))"#,
            &s,
            None,
        );
        assert_eq!(r.error, None);
        assert!(r.ops.is_empty());
        let logs: Vec<String> = r.events.iter().filter_map(|e| if let ScriptEvent::Log(_, t) = e { Some(t.clone()) } else { None }).collect();
        assert!(logs[0].contains("No environment is selected"));
        assert_eq!(logs[1], "pm.sendRequest is not available here");
    }

    #[test]
    fn infinite_loop_times_out() {
        let s = scopes();
        let req = RequestJs::default();
        let info = Info::default();
        let c = ScriptCtx { phase: Phase::Test, scopes: &s, environment_name: None, request: &req, response: None, info: &info };
        let started = Instant::now();
        let r = run_with_timeout("console.log('start'); while (true) {}", &c, None, Duration::from_millis(300));
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(r.error.as_deref().unwrap().starts_with("Script timed out"), "{:?}", r.error);
        assert_eq!(r.events.len(), 1, "logs before the timeout are kept");
    }

    #[test]
    fn apply_request_changes() {
        let mut req = Request { url: "{{base}}/users?id=1".into(), ..Default::default() };
        req.headers = vec![KeyValue { description: "d".into(), ..KeyValue::new("Accept", "*/*") }, KeyValue::default()];
        let before = RequestJs::from_request(&req);
        let mut after = before.clone();
        after.url = "{{base}}/users?id=2".into();
        after.headers.push(HeaderJs { key: "X".into(), value: "1".into(), disabled: false });
        after.apply_to(&before, &mut req);
        assert_eq!(req.params[0].value, "2");
        assert_eq!(req.headers.len(), 3);
        assert_eq!(req.headers[0].description, "d");
        assert!(req.headers[2].key.is_empty());

        let mut list = vec![KeyValue { enabled: false, ..KeyValue::new("a", "1") }];
        VarOp::Set { scope: Scope::Environment, key: "a".into(), value: "2".into() }.apply_to(&mut list);
        VarOp::Set { scope: Scope::Environment, key: "b".into(), value: "3".into() }.apply_to(&mut list);
        assert_eq!(list.iter().map(|k| (k.key.as_str(), k.value.as_str(), k.enabled)).collect::<Vec<_>>(), vec![("a", "2", true), ("b", "3", true)]);
    }
}
