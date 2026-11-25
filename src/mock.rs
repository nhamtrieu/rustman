//! Local mock server: answers HTTP requests with the canned responses of a `MockServer`.
//!
//! Minimal HTTP/1.1 (one request per connection, `Connection: close`), enough for browsers,
//! curl and app clients during development. CORS is open so front-ends can call it directly.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::model::{Method, MockRoute};
use crate::vars::{VarMap, substitute};

const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 10 * 1024 * 1024;
const MAX_LOG: usize = 200;

/// One request received by a running mock.
#[derive(Clone, Debug)]
pub struct MockHit {
    pub time: chrono::DateTime<chrono::Local>,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub matched: bool,
    pub duration: Duration,
}

/// State shared between the UI and the server task.
#[derive(Default)]
pub struct Shared {
    pub routes: Vec<MockRoute>,
    pub log: VecDeque<MockHit>,
}

pub type SharedRef = Arc<Mutex<Shared>>;

/// Binds the port (so errors such as "address in use" are reported right away) and serves in the background.
pub async fn bind(port: u16) -> Result<TcpListener, String> {
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| format!("Cannot listen on port {port}: {e}"))
}

pub async fn serve(listener: TcpListener, shared: SharedRef, on_hit: impl Fn() + Send + Sync + Clone + 'static) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let shared = shared.clone();
        let on_hit = on_hit.clone();
        tokio::spawn(async move {
            if handle(stream, shared).await {
                on_hit();
            }
        });
    }
}

pub(crate) struct Incoming {
    pub method: String,
    pub target: String,
    pub body: Vec<u8>,
}

/// Handles one connection; returns true when a request was logged.
async fn handle(mut stream: TcpStream, shared: SharedRef) -> bool {
    let start = Instant::now();
    let Some(req) = read_request(&mut stream).await else { return false };
    let (path, query) = req.target.split_once('?').unwrap_or((&req.target, ""));
    let path = if path.is_empty() { "/" } else { path };

    let routes = shared.lock().map(|s| s.routes.clone()).unwrap_or_default();
    let found = routes.iter().filter(|r| r.enabled).find_map(|r| {
        let method_ok = r.method.is_none_or(|m| m.as_str().eq_ignore_ascii_case(&req.method));
        if !method_ok {
            return None;
        }
        match_path(&r.path, path).map(|params| (r, params))
    });

    let (status, headers, body, matched) = match found {
        Some((route, params)) => {
            if route.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(route.delay_ms.min(60_000))).await;
            }
            let mut vars: VarMap = params.into_iter().collect();
            for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
                vars.entry(k.into_owned()).or_insert_with(|| v.into_owned());
            }
            let headers: Vec<(String, String)> = route
                .headers
                .iter()
                .filter(|h| h.enabled && !h.key.trim().is_empty())
                .map(|h| (h.key.trim().to_string(), substitute(&h.value, &vars)))
                .collect();
            (route.status, headers, substitute(&route.body, &vars), true)
        }
        // CORS preflight for routes that exist with another method.
        None if req.method.eq_ignore_ascii_case("OPTIONS") => (204, Vec::new(), String::new(), false),
        None => {
            let msg = serde_json::json!({ "error": format!("No mock route matches {} {}", req.method, path) });
            (404, vec![("Content-Type".to_string(), "application/json".to_string())], msg.to_string(), false)
        }
    };

    let response = build_response(status, &headers, &body, req.method.eq_ignore_ascii_case("HEAD"));
    let _ = stream.write_all(&response).await;
    let _ = stream.shutdown().await;

    if let Ok(mut s) = shared.lock() {
        s.log.push_back(MockHit {
            time: chrono::Local::now(),
            method: req.method,
            path: req.target.clone(),
            status,
            matched,
            duration: start.elapsed(),
        });
        while s.log.len() > MAX_LOG {
            s.log.pop_front();
        }
    }
    true
}

pub(crate) async fn read_request(stream: &mut TcpStream) -> Option<Incoming> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk)).await.ok()?.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find(&buf, b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let target = first.next()?.to_string();
    let content_length = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BODY);
    // Read the whole body (also so the client does not get a reset before reading the response).
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk)).await.ok()?.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Some(Incoming { method, target, body })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Matches `/users/:id/*` style patterns. Returns the captured `:params`.
pub fn match_path(pattern: &str, path: &str) -> Option<Vec<(String, String)>> {
    let pattern = pattern.trim();
    let pattern = pattern.split('?').next().unwrap_or("");
    let segs = |s: &str| s.split('/').filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let (pat, got) = (segs(pattern), segs(path));
    let mut params = Vec::new();
    for (i, p) in pat.iter().enumerate() {
        if p == "*" {
            return Some(params);
        }
        let g = got.get(i)?;
        if let Some(name) = p.strip_prefix(':') {
            let value = url::form_urlencoded::parse(format!("v={g}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_else(|| g.clone());
            params.push((name.to_string(), value));
        } else if let Some(name) = p.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
            params.push((name.to_string(), g.clone()));
        } else if p != g {
            return None;
        }
    }
    (pat.len() == got.len()).then_some(params)
}

fn reason(status: u16) -> &'static str {
    reqwest::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("")
}

pub(crate) fn build_response(status: u16, headers: &[(String, String)], body: &str, head_only: bool) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    let has = |name: &str| headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
    for (k, v) in headers {
        if !k.eq_ignore_ascii_case("content-length") && !k.eq_ignore_ascii_case("connection") {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    if !has("access-control-allow-origin") {
        out.push_str("Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: *\r\nAccess-Control-Allow-Headers: *\r\n");
    }
    if !has("content-type") && !body.is_empty() {
        let ct = if body.trim_start().starts_with(['{', '[']) { "application/json" } else { "text/plain; charset=utf-8" };
        out.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    out.push_str("Server: Rustman-Mock\r\n");
    out.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut bytes = out.into_bytes();
    if !head_only {
        bytes.extend_from_slice(body.as_bytes());
    }
    bytes
}

/// Base URL shown in the UI.
pub fn base_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// Is a method label valid for a route ("ANY" or an HTTP method)?
pub fn method_label(m: Option<Method>) -> &'static str {
    m.map(|m| m.as_str()).unwrap_or("ANY")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::KeyValue;

    #[test]
    fn path_patterns() {
        assert_eq!(match_path("/users/:id", "/users/42"), Some(vec![("id".into(), "42".into())]));
        assert_eq!(match_path("/users/{id}/posts", "/users/7/posts"), Some(vec![("id".into(), "7".into())]));
        assert_eq!(match_path("/users/:id", "/users"), None);
        assert_eq!(match_path("/users", "/users/1"), None);
        assert_eq!(match_path("/files/*", "/files/a/b.txt"), Some(vec![]));
        assert_eq!(match_path("/", "/"), Some(vec![]));
        assert_eq!(match_path("/a/:name", "/a/Nh%C3%A2m"), Some(vec![("name".into(), "Nhâm".into())]));
    }

    #[test]
    fn serves_routes_end_to_end() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let listener = bind(0).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let shared: SharedRef = Default::default();
            shared.lock().unwrap().routes = vec![
                MockRoute {
                    method: Some(Method::Get),
                    path: "/users/:id".into(),
                    status: 200,
                    headers: vec![KeyValue::new("Content-Type", "application/json"), KeyValue::new("X-Id", "{{id}}")],
                    body: r#"{"id":"{{id}}","q":"{{q}}"}"#.into(),
                    ..Default::default()
                },
                MockRoute { method: None, path: "/any".into(), status: 201, headers: vec![], body: "ok".into(), ..Default::default() },
            ];
            tokio::spawn(serve(listener, shared.clone(), || {}));
            let client = reqwest::Client::new();
            let r = client.get(format!("http://127.0.0.1:{port}/users/42?q=x")).send().await.unwrap();
            assert_eq!(r.status(), 200);
            assert_eq!(r.headers()["x-id"], "42");
            assert_eq!(r.headers()["access-control-allow-origin"], "*");
            assert_eq!(r.text().await.unwrap(), r#"{"id":"42","q":"x"}"#);
            let r = client.post(format!("http://127.0.0.1:{port}/any")).body("payload").send().await.unwrap();
            assert_eq!(r.status(), 201);
            let r = client.delete(format!("http://127.0.0.1:{port}/users/1")).send().await.unwrap();
            assert_eq!(r.status(), 404);
            let log = &shared.lock().unwrap().log;
            assert_eq!(log.len(), 3);
            assert!(log[0].matched && !log[2].matched);
        });
    }
}
