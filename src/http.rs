//! Turning a `Request` into a concrete HTTP call and executing it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;

use crate::model::{ApiKeyLocation, Auth, AuthKind, BodyMode, FormFieldKind, Method, Request, RequestSettings, TokenPlacement, NetSettings, ClientCert};
use crate::vars::{VarMap, substitute};

/// A request with all variables resolved and auth applied.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: PreparedBody,
    pub settings: RequestSettings,
}

#[derive(Clone, Debug)]
pub enum PreparedBody {
    None,
    Bytes(Vec<u8>),
    /// Read from disk at send time.
    File(String),
    Multipart(Vec<PreparedPart>),
}

#[derive(Clone, Debug)]
pub struct PreparedPart {
    pub name: String,
    pub value: String,
    pub is_file: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ResponseData {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<CookieInfo>,
    pub body: Vec<u8>,
    pub duration: Duration,
    pub remote_addr: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: String,
    pub http_only: bool,
    pub secure: bool,
}

impl ResponseData {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn content_type(&self) -> String {
        self.header("content-type").unwrap_or("").to_ascii_lowercase()
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
}

/// Effective auth: a request with `Inherit` uses its collection's auth.
pub fn effective_auth<'a>(req: &'a Auth, inherited: Option<&'a Auth>) -> Option<&'a Auth> {
    match req.kind {
        AuthKind::Inherit => inherited.filter(|a| !matches!(a.kind, AuthKind::Inherit | AuthKind::None)),
        AuthKind::None => None,
        _ => Some(req),
    }
}

pub fn prepare(req: &Request, vars: &VarMap, inherited_auth: Option<&Auth>) -> Result<Prepared, String> {
    let sub = |s: &str| substitute(s, vars);

    let mut url = sub(req.url.trim());
    if url.is_empty() {
        return Err("URL is empty".into());
    }
    if !url.contains("://") {
        url = format!("http://{url}");
    }

    let mut headers: Vec<(String, String)> = req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.key.trim().is_empty())
        .map(|h| (sub(h.key.trim()), sub(&h.value)))
        .collect();

    if let Some(auth) = effective_auth(&req.auth, inherited_auth) {
        match auth.kind {
            AuthKind::Bearer if !has_header(&headers, "authorization") => {
                headers.push(("Authorization".into(), format!("Bearer {}", sub(&auth.bearer_token))));
            }
            AuthKind::Basic if !has_header(&headers, "authorization") => {
                let raw = format!("{}:{}", sub(&auth.basic_username), sub(&auth.basic_password));
                let enc = base64::engine::general_purpose::STANDARD.encode(raw);
                headers.push(("Authorization".into(), format!("Basic {enc}")));
            }
            AuthKind::ApiKey if !auth.api_key_name.is_empty() => {
                let (k, v) = (sub(&auth.api_key_name), sub(&auth.api_key_value));
                match auth.api_key_in {
                    ApiKeyLocation::Header => headers.push((k, v)),
                    ApiKeyLocation::Query => {
                        let sep = if url.contains('?') { '&' } else { '?' };
                        url = format!("{url}{sep}{}={}", encode_component(&k), encode_component(&v));
                    }
                }
            }
            AuthKind::OAuth2 if !auth.oauth2.access_token.is_empty() => {
                let o = &auth.oauth2;
                let token = sub(&o.access_token);
                match o.add_to {
                    TokenPlacement::Header if !has_header(&headers, "authorization") => {
                        let prefix = sub(&o.header_prefix);
                        let value = if prefix.trim().is_empty() { token } else { format!("{} {token}", prefix.trim()) };
                        headers.push(("Authorization".into(), value));
                    }
                    TokenPlacement::Header => {}
                    TokenPlacement::Query => {
                        let sep = if url.contains('?') { '&' } else { '?' };
                        url = format!("{url}{sep}access_token={}", encode_component(&token));
                    }
                }
            }
            _ => {}
        }
    }

    let body = match req.body.mode {
        BodyMode::None => PreparedBody::None,
        BodyMode::Raw => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), req.body.raw_lang.content_type().into()));
            }
            PreparedBody::Bytes(sub(&req.body.raw).into_bytes())
        }
        BodyMode::UrlEncoded => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
            }
            let encoded: Vec<String> = req
                .body
                .urlencoded
                .iter()
                .filter(|kv| kv.enabled && !kv.key.is_empty())
                .map(|kv| format!("{}={}", encode_form(&sub(&kv.key)), encode_form(&sub(&kv.value))))
                .collect();
            PreparedBody::Bytes(encoded.join("&").into_bytes())
        }
        BodyMode::FormData => {
            // reqwest generates Content-Type with the boundary; a user-provided one would break it.
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
            PreparedBody::Multipart(
                req.body
                    .form_data
                    .iter()
                    .filter(|f| f.enabled && !f.key.is_empty())
                    .map(|f| PreparedPart {
                        name: sub(&f.key),
                        value: sub(&f.value),
                        is_file: f.kind == FormFieldKind::File,
                    })
                    .collect(),
            )
        }
        BodyMode::Binary => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/octet-stream".into()));
            }
            PreparedBody::File(sub(&req.body.binary_path))
        }
        BodyMode::GraphQl => {
            if !has_header(&headers, "content-type") {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            let variables_src = sub(&req.body.graphql_variables);
            let variables: serde_json::Value = if variables_src.trim().is_empty() {
                serde_json::Value::Object(Default::default())
            } else {
                serde_json::from_str(&variables_src).map_err(|e| format!("GraphQL variables are not valid JSON: {e}"))?
            };
            let payload = serde_json::json!({ "query": sub(&req.body.graphql_query), "variables": variables });
            PreparedBody::Bytes(serde_json::to_vec(&payload).unwrap_or_default())
        }
    };

    if !has_header(&headers, "user-agent") {
        headers.push(("User-Agent".into(), concat!("Rustman/", env!("CARGO_PKG_VERSION")).into()));
    }
    if !has_header(&headers, "accept") {
        headers.push(("Accept".into(), "*/*".into()));
    }

    Ok(Prepared { method: req.method, url, headers, body, settings: req.settings.clone() })
}

/// Percent-encode for query components.
pub fn encode_component(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20")
}

/// application/x-www-form-urlencoded encoding (space -> '+').
pub fn encode_form(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn to_reqwest_method(m: Method) -> reqwest::Method {
    match m {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
        Method::Head => reqwest::Method::HEAD,
        Method::Options => reqwest::Method::OPTIONS,
    }
}

/// Network settings for every request (proxy, certificates). Set by the app from the workspace.
static NETWORK: std::sync::RwLock<Option<Arc<NetSettings>>> = std::sync::RwLock::new(None);

pub fn set_network(n: &NetSettings) {
    if let Ok(mut g) = NETWORK.write() {
        *g = Some(Arc::new(n.clone()));
    }
}

fn network() -> Arc<NetSettings> {
    NETWORK.read().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Proxy URL from the custom settings (`proxy.corp`, `proxy.corp:3128` or a full URL).
pub fn proxy_url(n: &NetSettings) -> Option<String> {
    let host = n.proxy_host.trim().trim_end_matches('/');
    if host.is_empty() {
        return None;
    }
    let with_scheme = if host.contains("://") { host.to_string() } else { format!("http://{host}") };
    let after_scheme = with_scheme.split_once("://").map(|(_, r)| r).unwrap_or("");
    let has_port = after_scheme.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let port = n.proxy_port.trim();
    Some(if has_port || port.is_empty() { with_scheme } else { format!("{with_scheme}:{port}") })
}

/// `*.example.com` → `.example.com` (reqwest's form for "this domain and its subdomains").
fn bypass_list(s: &str) -> String {
    s.split([',', ';', '\n'])
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(|h| h.strip_prefix('*').filter(|r| r.starts_with('.')).unwrap_or(h))
        .collect::<Vec<_>>()
        .join(",")
}

/// Applies proxy and certificate settings for a request to `url`.
fn apply_network(mut b: reqwest::ClientBuilder, n: &NetSettings, url: &str) -> Result<reqwest::ClientBuilder, String> {
    // Proxy: the custom one replaces the system one.
    let custom = proxy_url(n).filter(|_| n.custom_proxy && (n.proxy_http || n.proxy_https));
    if let Some(proxy) = custom {
        let no_proxy = reqwest::NoProxy::from_string(&bypass_list(&n.proxy_bypass));
        let make = |http: bool| -> Result<reqwest::Proxy, String> {
            let p = if http { reqwest::Proxy::http(&proxy) } else { reqwest::Proxy::https(&proxy) };
            let mut p = p.map_err(|e| format!("Invalid proxy '{proxy}': {e}"))?;
            if n.proxy_auth {
                p = p.basic_auth(&n.proxy_username, &n.proxy_password);
            }
            Ok(p.no_proxy(no_proxy.clone()))
        };
        if n.proxy_http {
            b = b.proxy(make(true)?);
        }
        if n.proxy_https {
            b = b.proxy(make(false)?);
        }
    } else if !n.system_proxy {
        b = b.no_proxy();
    }

    // Extra CA certificates.
    if n.ca_enabled && !n.ca_path.trim().is_empty() {
        let path = n.ca_path.trim();
        let pem = std::fs::read(path).map_err(|e| format!("Cannot read CA certificates '{path}': {e}"))?;
        let certs = reqwest::Certificate::from_pem_bundle(&pem).map_err(|e| format!("Invalid CA certificates in '{path}': {e}"))?;
        if certs.is_empty() {
            return Err(format!("No certificate found in '{path}' (PEM expected)"));
        }
        b = b.tls_certs_merge(certs);
    }

    // Client certificate for this host.
    if let Ok(u) = url::Url::parse(url)
        && u.scheme() == "https"
        && let Some(host) = u.host_str()
        && let Some(c) = n.client_certs.iter().find(|c| c.enabled && c.matches(host, u.port_or_known_default().unwrap_or(443)))
    {
        b = b.identity(load_identity(c)?);
    }
    Ok(b)
}

fn load_identity(c: &ClientCert) -> Result<reqwest::Identity, String> {
    let read = |path: &str, what: &str| std::fs::read(path.trim()).map_err(|e| format!("Cannot read client {what} '{}' for {}: {e}", path.trim(), c.host.trim()));
    let mut pem = read(&c.cert_path, "certificate")?;
    if !c.key_path.trim().is_empty() && c.key_path.trim() != c.cert_path.trim() {
        pem.push(b'\n');
        pem.extend(read(&c.key_path, "key")?);
    }
    if pem.windows(9).any(|w| w == b"ENCRYPTED") {
        return Err(format!("The private key for {} is encrypted; decrypt it first (openssl pkey -in key.pem -out key-plain.pem)", c.host.trim()));
    }
    reqwest::Identity::from_pem(&pem).map_err(|e| format!("Invalid client certificate for {} (PEM certificate and unencrypted key expected): {e}", c.host.trim()))
}

pub async fn execute(p: Prepared, jar: Arc<reqwest::cookie::Jar>) -> Result<ResponseData, String> {
    execute_with(p, jar, &network()).await
}

pub async fn execute_with(p: Prepared, jar: Arc<reqwest::cookie::Jar>, net: &NetSettings) -> Result<ResponseData, String> {
    let redirect = if p.settings.follow_redirects {
        reqwest::redirect::Policy::limited(10)
    } else {
        reqwest::redirect::Policy::none()
    };
    let mut builder = reqwest::Client::builder()
        .redirect(redirect)
        .tls_danger_accept_invalid_certs(!p.settings.verify_ssl)
        // Send `Content-Type` instead of `content-type` like Postman; some legacy servers care.
        .http1_title_case_headers()
        .cookie_provider(jar);
    if p.settings.timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(p.settings.timeout_ms));
    }
    builder = apply_network(builder, net, &p.url)?;
    let client = builder.build().map_err(|e| format!("Cannot build HTTP client: {e}"))?;

    let mut rb = client.request(to_reqwest_method(p.method), &p.url);
    for (k, v) in &p.headers {
        rb = rb.header(k, v);
    }
    rb = match p.body {
        PreparedBody::None => rb,
        PreparedBody::Bytes(b) => rb.body(b),
        PreparedBody::File(path) => {
            let data = tokio::fs::read(&path).await.map_err(|e| format!("Cannot read file '{path}': {e}"))?;
            rb.body(data)
        }
        PreparedBody::Multipart(parts) => {
            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                if part.is_file {
                    let path = part.value.clone();
                    let p = reqwest::multipart::Part::file(&path)
                        .await
                        .map_err(|e| format!("Cannot read file '{path}': {e}"))?;
                    form = form.part(part.name, p);
                } else {
                    form = form.text(part.name, part.value);
                }
            }
            rb.multipart(form)
        }
    };

    let start = Instant::now();
    let resp = rb.send().await.map_err(describe_error)?;
    let status = resp.status();
    let http_version = format!("{:?}", resp.version());
    let final_url = resp.url().to_string();
    let remote_addr = resp.remote_addr().map(|a| a.to_string());
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let cookies = resp
        .cookies()
        .map(|c| CookieInfo {
            name: c.name().to_string(),
            value: c.value().to_string(),
            domain: c.domain().unwrap_or("").to_string(),
            path: c.path().unwrap_or("").to_string(),
            expires: c
                .expires()
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc2822())
                .or_else(|| c.max_age().map(|d| format!("max-age {}s", d.as_secs())))
                .unwrap_or_else(|| "Session".into()),
            http_only: c.http_only(),
            secure: c.secure(),
        })
        .collect();
    let body = resp.bytes().await.map_err(describe_error)?.to_vec();
    let duration = start.elapsed();

    Ok(ResponseData {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        http_version,
        url: final_url,
        headers,
        cookies,
        body,
        duration,
        remote_addr,
    })
}

fn describe_error(e: reqwest::Error) -> String {
    let kind = if e.is_timeout() {
        "Request timed out"
    } else if e.is_connect() {
        "Could not connect"
    } else if e.is_redirect() {
        "Too many redirects"
    } else if e.is_builder() {
        "Invalid request"
    } else if e.is_decode() || e.is_body() {
        "Error reading response body"
    } else {
        "Request failed"
    };
    // Include the whole source chain, it usually holds the useful part (DNS, TLS, ...).
    let mut msg = format!("{kind}: {e}");
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        msg.push_str(&format!("\n  caused by: {s}"));
        src = s.source();
    }
    msg
}

pub fn human_size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.2} KB", n as f64 / 1024.0)
    } else {
        format!("{:.2} MB", n as f64 / 1024.0 / 1024.0)
    }
}

pub fn human_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 { format!("{ms} ms") } else { format!("{:.2} s", d.as_secs_f64()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyValue, RawLang};

    #[test]
    fn prepare_applies_vars_auth_and_content_type() {
        let mut req = Request { url: "{{host}}/users?x=1".into(), method: Method::Post, ..Default::default() };
        req.body.mode = BodyMode::Raw;
        req.body.raw_lang = RawLang::Json;
        req.body.raw = r#"{"name":"{{name}}"}"#.into();
        let coll_auth = Auth { kind: AuthKind::ApiKey, api_key_name: "k".into(), api_key_value: "a b".into(), api_key_in: ApiKeyLocation::Query, ..Default::default() };
        let vars = crate::vars::build_vars(&[&[KeyValue::new("host", "api.test"), KeyValue::new("name", "Nham")]]);
        let p = prepare(&req, &vars, Some(&coll_auth)).unwrap();
        assert_eq!(p.url, "http://api.test/users?x=1&k=a%20b");
        assert!(p.headers.iter().any(|(k, v)| k == "Content-Type" && v == "application/json"));
        match p.body {
            PreparedBody::Bytes(b) => assert_eq!(String::from_utf8(b).unwrap(), r#"{"name":"Nham"}"#),
            _ => panic!("expected bytes"),
        }
    }

    /// End-to-end check against a local echo server: `RUSTMAN_TEST_URL=http://127.0.0.1:18765 cargo test`.
    #[test]
    fn live_echo_server() {
        let Ok(base) = std::env::var("RUSTMAN_TEST_URL") else { return };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let jar = Arc::new(reqwest::cookie::Jar::default());
        let coll_auth = Auth { kind: AuthKind::Bearer, bearer_token: "{{token}}".into(), ..Default::default() };
        let vars = crate::vars::build_vars(&[&[KeyValue::new("base", &base), KeyValue::new("token", "T0K")]]);

        let mut req = Request { url: "{{base}}/form".into(), method: Method::Post, ..Default::default() };
        req.body.mode = BodyMode::FormData;
        req.body.form_data = vec![
            crate::model::FormField { key: "name".into(), value: "Nhâm".into(), ..Default::default() },
            crate::model::FormField { key: "file".into(), value: file!().to_string(), kind: FormFieldKind::File, ..Default::default() },
        ];
        let p = prepare(&req, &vars, Some(&coll_auth)).unwrap();
        let r = rt.block_on(execute(p, jar.clone())).unwrap();
        assert_eq!(r.status, 201);
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["headers"]["Authorization"], "Bearer T0K");
        assert!(v["headers"]["Content-Type"].as_str().unwrap().starts_with("multipart/form-data; boundary="));
        let body = v["body"].as_str().unwrap();
        assert!(body.contains("Nhâm") && body.contains("filename=\"http.rs\""));
        assert_eq!(r.cookies.len(), 1);

        // The cookie from the first response must be sent back automatically.
        let req2 = Request { url: "{{base}}/again".into(), ..Default::default() };
        let r2 = rt.block_on(execute(prepare(&req2, &vars, None).unwrap(), jar)).unwrap();
        let v2: serde_json::Value = serde_json::from_slice(&r2.body).unwrap();
        assert_eq!(v2["headers"]["Cookie"], "session=abc123");
        assert!(v2["headers"].get("Authorization").is_none());
    }

    #[test]
    fn proxy_url_and_bypass_forms() {
        let n = |host: &str, port: &str| NetSettings { proxy_host: host.into(), proxy_port: port.into(), ..Default::default() };
        assert_eq!(proxy_url(&n("proxy.corp", "3128")).as_deref(), Some("http://proxy.corp:3128"));
        assert_eq!(proxy_url(&n("proxy.corp:8888", "3128")).as_deref(), Some("http://proxy.corp:8888"));
        assert_eq!(proxy_url(&n("https://proxy.corp/", "")).as_deref(), Some("https://proxy.corp"));
        assert_eq!(proxy_url(&n("  ", "3128")), None);
        assert_eq!(bypass_list("localhost, *.corp.io;10.0.0.0/8\n"), "localhost,.corp.io,10.0.0.0/8");
    }

    /// Fake HTTP proxy: answers every request with what it received (request line and
    /// Proxy-Authorization); refuses CONNECT so HTTPS through it fails visibly.
    async fn fake_proxy(listener: tokio::net::TcpListener, seen: Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                let auth = req.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("proxy-authorization")).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
                seen.lock().unwrap().push(line.clone());
                let resp = if line.starts_with("CONNECT") {
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_string()
                } else {
                    let body = serde_json::json!({ "line": line, "auth": auth }).to_string();
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                };
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    }

    #[test]
    fn custom_proxy_auth_bypass_and_connect() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        rt.spawn(fake_proxy(listener, seen.clone()));
        // A direct target, to check the bypass list.
        let direct = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let direct_url = format!("http://127.0.0.1:{}/direct", direct.local_addr().unwrap().port());
        rt.spawn(fake_proxy(direct, Arc::new(std::sync::Mutex::new(Vec::new()))));

        let net = NetSettings {
            custom_proxy: true,
            proxy_host: "127.0.0.1".into(),
            proxy_port: port.to_string(),
            proxy_auth: true,
            proxy_username: "u".into(),
            proxy_password: "p".into(),
            ..Default::default()
        };
        let get = |url: &str, net: &NetSettings| {
            let p = Prepared { method: Method::Get, url: url.into(), headers: vec![], body: PreparedBody::None, settings: RequestSettings { timeout_ms: 5000, ..Default::default() } };
            rt.block_on(execute_with(p, Arc::new(reqwest::cookie::Jar::default()), net))
        };

        let r = get("http://api.example.invalid/x?y=1", &net).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["line"], "GET http://api.example.invalid/x?y=1 HTTP/1.1");
        assert_eq!(v["auth"], "Basic dTpw");

        // HTTPS goes through CONNECT (refused by the fake proxy).
        assert!(get("https://secure.example.invalid/", &net).is_err());
        assert!(seen.lock().unwrap().iter().any(|l| l.starts_with("CONNECT secure.example.invalid:443")), "{:?}", seen.lock().unwrap());

        // 127.0.0.1 is in the default bypass list: reached directly.
        let before = seen.lock().unwrap().len();
        let r = get(&direct_url, &net).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["line"], "GET /direct HTTP/1.1", "origin-form means no proxy");
        assert_eq!(seen.lock().unwrap().len(), before);

        // HTTP unchecked: http:// URLs skip the proxy (and fail to resolve).
        let https_only = NetSettings { proxy_http: false, ..net.clone() };
        assert!(get("http://api.example.invalid/x", &https_only).is_err());
        assert_eq!(seen.lock().unwrap().len(), before);
    }

    #[test]
    fn certificate_file_errors() {
        let dir = std::env::temp_dir().join(format!("rustman-cert-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let garbage = dir.join("garbage.pem");
        std::fs::write(&garbage, "not a certificate").unwrap();
        let encrypted = dir.join("enc.pem");
        std::fs::write(&encrypted, "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n").unwrap();
        let b = || reqwest::Client::builder();
        let n = NetSettings { ca_enabled: true, ca_path: garbage.display().to_string(), ..Default::default() };
        assert!(apply_network(b(), &n, "https://x.io").err().unwrap().contains("No certificate found"));
        let n = NetSettings { ca_enabled: true, ca_path: dir.join("missing.pem").display().to_string(), ..Default::default() };
        assert!(apply_network(b(), &n, "https://x.io").err().unwrap().contains("Cannot read CA certificates"));
        let cert = |key: &std::path::Path| NetSettings {
            client_certs: vec![ClientCert { host: "x.io".into(), cert_path: garbage.display().to_string(), key_path: key.display().to_string(), ..Default::default() }],
            ..Default::default()
        };
        assert!(apply_network(b(), &cert(&encrypted), "https://x.io/a").err().unwrap().contains("encrypted"));
        assert!(apply_network(b(), &cert(&garbage), "https://x.io/a").err().unwrap().contains("Invalid client certificate"));
        // Only used for its host, and only over HTTPS.
        assert!(apply_network(b(), &cert(&garbage), "https://other.io/a").is_ok());
        assert!(apply_network(b(), &cert(&garbage), "http://x.io/a").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real mutual TLS against `openssl s_server` with certificates made by `openssl`
    /// (skipped when openssl is not installed).
    #[test]
    fn mutual_tls_with_openssl_server() {
        use std::process::{Command, Stdio};
        if Command::new("openssl").arg("version").output().is_err() {
            eprintln!("openssl not found, skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("rustman-mtls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = |name: &str| dir.join(name).display().to_string();
        let ssl = |args: &[&str]| {
            let out = Command::new("openssl").args(args).current_dir(&dir).output().unwrap();
            assert!(out.status.success(), "openssl {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        let ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes"];
        ssl(&[&["req", "-x509"][..], &ec, &["-keyout", "ca.key", "-out", "ca.pem", "-days", "1", "-subj", "/CN=Rustman Test CA", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=keyCertSign"]].concat());
        for (name, ext) in [("server", "subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n"), ("client", "extendedKeyUsage=clientAuth\n")] {
            std::fs::write(dir.join(format!("{name}.ext")), ext).unwrap();
            ssl(&[&["req"][..], &ec, &["-keyout", &format!("{name}.key"), "-out", &format!("{name}.csr"), "-subj", &format!("/CN={name}")]].concat());
            ssl(&["x509", "-req", "-in", &format!("{name}.csr"), "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", &format!("{name}.pem"), "-days", "1", "-extfile", &format!("{name}.ext")]);
        }
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut server = Command::new("openssl")
            .args(["s_server", "-accept", &port.to_string(), "-cert", "server.pem", "-key", "server.key", "-CAfile", "ca.pem", "-Verify", "1", "-www"])
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let url = format!("https://localhost:{port}/");
        let get = |net: &NetSettings| {
            let p = Prepared { method: Method::Get, url: url.clone(), headers: vec![], body: PreparedBody::None, settings: RequestSettings { timeout_ms: 5000, ..Default::default() } };
            rt.block_on(execute_with(p, Arc::new(reqwest::cookie::Jar::default()), net))
        };
        let ca_only = NetSettings { ca_enabled: true, ca_path: f("ca.pem"), ..Default::default() };
        let with_cert = NetSettings {
            client_certs: vec![ClientCert { host: format!("localhost:{port}"), cert_path: f("client.pem"), key_path: f("client.key"), ..Default::default() }],
            ..ca_only.clone()
        };
        let untrusted = get(&NetSettings { system_proxy: false, ..Default::default() });
        let no_cert = get(&ca_only);
        let ok = get(&with_cert);
        let _ = server.kill();
        let _ = server.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(untrusted.is_err(), "the test CA is not trusted without the CA setting");
        assert!(no_cert.is_err(), "the server requires a client certificate");
        let ok = ok.unwrap();
        assert_eq!(ok.status, 200);
        assert!(ok.text().contains("Client certificate"), "s_server reports the client certificate");
    }

    #[test]
    fn oauth2_token_in_header_or_query() {
        use crate::model::OAuth2;
        let vars: VarMap = [("t".to_string(), "abc".to_string())].into();
        let auth = |o: OAuth2| Auth { kind: AuthKind::OAuth2, oauth2: Box::new(o), ..Default::default() };
        let req = |a: Auth| Request { url: "https://x.io/a?b=1".into(), auth: a, ..Default::default() };
        let p = prepare(&req(auth(OAuth2 { access_token: "{{t}}".into(), ..Default::default() })), &vars, None).unwrap();
        assert!(p.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer abc"));
        let p = prepare(&req(auth(OAuth2 { access_token: "abc".into(), header_prefix: "".into(), ..Default::default() })), &vars, None).unwrap();
        assert!(p.headers.iter().any(|(k, v)| k == "Authorization" && v == "abc"));
        let p = prepare(&req(auth(OAuth2 { access_token: "a b".into(), add_to: TokenPlacement::Query, ..Default::default() })), &vars, None).unwrap();
        assert_eq!(p.url, "https://x.io/a?b=1&access_token=a%20b");
        assert!(!p.headers.iter().any(|(k, _)| k == "Authorization"));
        // No token yet: nothing is added.
        let p = prepare(&req(auth(OAuth2::default())), &vars, None).unwrap();
        assert!(!p.headers.iter().any(|(k, _)| k == "Authorization"));
    }

    #[test]
    fn basic_auth_header() {
        let req = Request {
            url: "https://x.io".into(),
            auth: Auth { kind: AuthKind::Basic, basic_username: "u".into(), basic_password: "p".into(), ..Default::default() },
            ..Default::default()
        };
        let p = prepare(&req, &VarMap::new(), None).unwrap();
        assert!(p.headers.iter().any(|(k, v)| k == "Authorization" && v == "Basic dTpw"));
    }
}
