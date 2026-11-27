//! OAuth 2.0: getting a new access token (authorization code with or without PKCE, implicit,
//! password and client credentials grants) and refreshing it.
//!
//! Browser grants open the authorization URL in the system browser and catch the redirect on
//! the callback URL when it points at this machine (`http://localhost:<port>/...`). When it does
//! not, the user pastes the URL the browser was redirected to.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::http::{self, Prepared, PreparedBody, ResponseData};
use crate::model::{ClientAuth, GrantType, Method, OAuth2, OAuthToken, PkceMethod, RequestSettings};

/// One HTTP call made to the authorization server, for the console.
pub type Exchange = (Prepared, Result<ResponseData, String>);

#[derive(Debug)]
pub struct Outcome {
    pub token: Result<OAuthToken, String>,
    pub exchanges: Vec<Exchange>,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Everything decided before a token request starts: generated secrets, the authorization URL
/// and, for browser grants with a local callback URL, the socket listening for the redirect.
pub struct Session {
    cfg: OAuth2,
    pub grant: GrantType,
    /// Browser grants: the URL to open.
    pub auth_url: Option<String>,
    /// The redirect is caught automatically (callback URL on this machine).
    pub listens: bool,
    state: String,
    verifier: String,
    listeners: Vec<std::net::TcpListener>,
    callback_path: String,
}

impl Session {
    /// Validates `cfg` (variables already substituted) and prepares the flow.
    pub fn new(cfg: &OAuth2) -> Result<Self, String> {
        let grant = cfg.grant_type;
        let need = |v: &str, name: &str| if v.trim().is_empty() { Err(format!("{name} is required for the {} grant", grant.label())) } else { Ok(()) };
        if grant != GrantType::Implicit {
            need(&cfg.access_token_url, "Access Token URL")?;
            check_url(&cfg.access_token_url, "Access Token URL")?;
        }
        let mut s = Session {
            cfg: cfg.clone(),
            grant,
            auth_url: None,
            listens: false,
            state: String::new(),
            verifier: String::new(),
            listeners: Vec::new(),
            callback_path: String::new(),
        };
        match grant {
            GrantType::Password => {
                need(&cfg.username, "Username")?;
            }
            GrantType::ClientCredentials => {
                need(&cfg.client_id, "Client ID")?;
            }
            GrantType::AuthorizationCode | GrantType::AuthorizationCodePkce | GrantType::Implicit => {
                need(&cfg.auth_url, "Auth URL")?;
                need(&cfg.client_id, "Client ID")?;
                need(&cfg.callback_url, "Callback URL")?;
                check_url(&cfg.auth_url, "Auth URL")?;
                let callback = url::Url::parse(cfg.callback_url.trim()).map_err(|e| format!("Callback URL is not valid: {e}"))?;
                s.state = if cfg.state.trim().is_empty() { random_string() } else { cfg.state.trim().to_string() };
                if grant == GrantType::AuthorizationCodePkce {
                    s.verifier = if cfg.code_verifier.trim().is_empty() { random_string() } else { cfg.code_verifier.trim().to_string() };
                    if !(43..=128).contains(&s.verifier.len()) {
                        return Err("Code Verifier must be 43 to 128 characters long".into());
                    }
                }
                if let Some(addrs) = loopback_addrs(&callback) {
                    for a in &addrs {
                        // "localhost" may resolve to either; one of the two is enough.
                        if let Ok(l) = std::net::TcpListener::bind(a) {
                            s.listeners.push(l);
                        }
                    }
                    if s.listeners.is_empty() {
                        let port = callback.port_or_known_default().unwrap_or(80);
                        return Err(format!("Cannot listen on port {port} for the Callback URL (is it already in use?). Change the port in the Callback URL."));
                    }
                    s.listens = true;
                    s.callback_path = callback.path().to_string();
                }
                s.auth_url = Some(authorization_url(cfg, grant, &s.state, &s.verifier));
            }
        }
        Ok(s)
    }

    /// Runs the flow. `open_browser` is called with the authorization URL for browser grants;
    /// `pasted` receives a redirect URL typed in by the user.
    pub async fn run(
        self,
        settings: RequestSettings,
        jar: Arc<reqwest::cookie::Jar>,
        open_browser: impl FnOnce(&str) -> Result<(), String> + Send,
        pasted: oneshot::Receiver<String>,
    ) -> Outcome {
        let mut exchanges = Vec::new();
        let token = self.run_inner(&settings, jar, open_browser, pasted, &mut exchanges).await;
        Outcome { token, exchanges }
    }

    async fn run_inner(
        self,
        settings: &RequestSettings,
        jar: Arc<reqwest::cookie::Jar>,
        open_browser: impl FnOnce(&str) -> Result<(), String> + Send,
        pasted: oneshot::Receiver<String>,
        exchanges: &mut Vec<Exchange>,
    ) -> Result<OAuthToken, String> {
        let cfg = &self.cfg;
        let mut params: Vec<(&str, String)> = match self.grant {
            GrantType::ClientCredentials => vec![("grant_type", "client_credentials".into())],
            GrantType::Password => {
                vec![("grant_type", "password".into()), ("username", cfg.username.clone()), ("password", cfg.password.clone())]
            }
            GrantType::AuthorizationCode | GrantType::AuthorizationCodePkce | GrantType::Implicit => {
                let url = self.auth_url.clone().unwrap_or_default();
                // Listen before opening the browser so a fast redirect is not missed.
                let listeners: Vec<TcpListener> = self
                    .listeners
                    .into_iter()
                    .filter_map(|l| l.set_nonblocking(true).ok().and_then(|_| TcpListener::from_std(l).ok()))
                    .collect();
                // Not fatal: the dialog offers to copy the link instead.
                let _ = open_browser(&url);
                let cb = wait_for_callback(listeners, &self.callback_path, pasted).await?;
                if let Some(err) = cb.get("error") {
                    let desc = cb.get("error_description").map(|d| format!(": {d}")).unwrap_or_default();
                    return Err(format!("Authorization failed: {err}{desc}"));
                }
                if let Some(st) = cb.get("state")
                    && *st != self.state
                {
                    return Err("Authorization failed: the state returned by the server does not match".into());
                }
                if self.grant == GrantType::Implicit {
                    return token_from_map(&cb);
                }
                let code = cb.get("code").ok_or("The redirect URL has no authorization code")?;
                let mut p = vec![("grant_type", "authorization_code".to_string()), ("code", code.clone()), ("redirect_uri", cfg.callback_url.trim().to_string())];
                if self.grant == GrantType::AuthorizationCodePkce {
                    p.push(("code_verifier", self.verifier.clone()));
                }
                p
            }
        };
        if matches!(self.grant, GrantType::ClientCredentials | GrantType::Password) && !cfg.scope.trim().is_empty() {
            params.push(("scope", cfg.scope.trim().to_string()));
        }
        for (k, v) in [("audience", &cfg.audience), ("resource", &cfg.resource)] {
            if !v.trim().is_empty() {
                params.push((k, v.trim().to_string()));
            }
        }
        token_request(cfg, &cfg.access_token_url, params, settings, jar, exchanges).await
    }
}

/// Uses the refresh token to get a new access token.
pub async fn refresh(cfg: &OAuth2, settings: &RequestSettings, jar: Arc<reqwest::cookie::Jar>) -> Outcome {
    let mut exchanges = Vec::new();
    let url = if cfg.refresh_token_url.trim().is_empty() { &cfg.access_token_url } else { &cfg.refresh_token_url };
    let token = if cfg.refresh_token.is_empty() {
        Err("There is no refresh token".to_string())
    } else if url.trim().is_empty() {
        Err("Access Token URL is required to refresh the token".to_string())
    } else {
        let params = vec![("grant_type", "refresh_token".to_string()), ("refresh_token", cfg.refresh_token.clone())];
        token_request(cfg, url, params, settings, jar, &mut exchanges).await
    };
    Outcome { token, exchanges }
}

fn check_url(u: &str, name: &str) -> Result<(), String> {
    let u = u.trim();
    if u.starts_with("http://") || u.starts_with("https://") {
        Ok(())
    } else {
        Err(format!("{name} must start with http:// or https://"))
    }
}

/// Sockets to listen on when the callback URL points at this machine.
fn loopback_addrs(callback: &url::Url) -> Option<Vec<std::net::SocketAddr>> {
    if callback.scheme() != "http" {
        return None;
    }
    let port = callback.port_or_known_default()?;
    let v4 = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let v6 = std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port));
    match callback.host()? {
        url::Host::Domain(d) if d.eq_ignore_ascii_case("localhost") => Some(vec![v4, v6]),
        url::Host::Ipv4(ip) if ip.is_loopback() => Some(vec![v4]),
        url::Host::Ipv6(ip) if ip.is_loopback() => Some(vec![v6]),
        _ => None,
    }
}

pub fn authorization_url(cfg: &OAuth2, grant: GrantType, state: &str, verifier: &str) -> String {
    let mut q: Vec<(&str, String)> = vec![
        ("response_type", if grant == GrantType::Implicit { "token" } else { "code" }.into()),
        ("client_id", cfg.client_id.trim().into()),
        ("redirect_uri", cfg.callback_url.trim().into()),
    ];
    if !cfg.scope.trim().is_empty() {
        q.push(("scope", cfg.scope.trim().into()));
    }
    if !state.is_empty() {
        q.push(("state", state.into()));
    }
    if grant == GrantType::AuthorizationCodePkce {
        let (challenge, method) = match cfg.pkce_method {
            PkceMethod::S256 => (base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes())), "S256"),
            PkceMethod::Plain => (verifier.to_string(), "plain"),
        };
        q.push(("code_challenge", challenge));
        q.push(("code_challenge_method", method.into()));
    }
    for (k, v) in [("audience", &cfg.audience), ("resource", &cfg.resource)] {
        if !v.trim().is_empty() {
            q.push((k, v.trim().into()));
        }
    }
    let base = cfg.auth_url.trim();
    let query: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", http::encode_component(v))).collect();
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}{}", query.join("&"))
}

async fn token_request(
    cfg: &OAuth2,
    url: &str,
    mut params: Vec<(&str, String)>,
    settings: &RequestSettings,
    jar: Arc<reqwest::cookie::Jar>,
    exchanges: &mut Vec<Exchange>,
) -> Result<OAuthToken, String> {
    let mut headers = vec![
        ("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string()),
        // GitHub answers form-encoded unless asked for JSON.
        ("Accept".to_string(), "application/json".to_string()),
        ("User-Agent".to_string(), concat!("Rustman/", env!("CARGO_PKG_VERSION")).to_string()),
    ];
    // A public client (no secret) always identifies itself in the body.
    if cfg.client_auth == ClientAuth::BasicHeader && !cfg.client_secret.is_empty() {
        let raw = format!("{}:{}", cfg.client_id.trim(), cfg.client_secret);
        headers.push(("Authorization".into(), format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw))));
    } else if !cfg.client_id.trim().is_empty() {
        params.push(("client_id", cfg.client_id.trim().to_string()));
        if !cfg.client_secret.is_empty() {
            params.push(("client_secret", cfg.client_secret.clone()));
        }
    }
    let body: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", http::encode_form(v))).collect();
    let prepared = Prepared {
        method: Method::Post,
        url: url.trim().to_string(),
        headers,
        body: PreparedBody::Bytes(body.join("&").into_bytes()),
        settings: settings.clone(),
        digest: None,
    };
    let result = http::execute(prepared.clone(), jar).await;
    let token = match &result {
        Ok(resp) => parse_token_response(resp),
        Err(e) => Err(format!("Token request failed: {e}")),
    };
    exchanges.push((prepared, result));
    token
}

/// Reads a token endpoint response (JSON, or form-encoded from older servers).
pub fn parse_token_response(resp: &ResponseData) -> Result<OAuthToken, String> {
    let text = resp.text();
    let map: HashMap<String, String> = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(o)) => o
            .into_iter()
            .map(|(k, v)| {
                let v = match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                };
                (k, v)
            })
            .collect(),
        _ if text.contains('=') && !text.trim_start().starts_with('<') => url::form_urlencoded::parse(text.trim().as_bytes()).into_owned().collect(),
        _ => HashMap::new(),
    };
    if let Some(err) = map.get("error") {
        let desc = map.get("error_description").map(|d| format!(": {d}")).unwrap_or_default();
        return Err(format!("Token request failed ({} {}): {err}{desc}", resp.status, resp.status_text));
    }
    if !(200..300).contains(&resp.status) {
        let snippet: String = text.chars().take(300).collect();
        return Err(format!("Token request failed: HTTP {} {}{}", resp.status, resp.status_text, if snippet.is_empty() { String::new() } else { format!("\n{snippet}") }));
    }
    token_from_map(&map)
}

fn token_from_map(map: &HashMap<String, String>) -> Result<OAuthToken, String> {
    let get = |k: &str| map.get(k).cloned().unwrap_or_default();
    let access_token = get("access_token");
    if access_token.is_empty() {
        return Err("The server response has no access_token".into());
    }
    let expires_at = map.get("expires_in").and_then(|v| v.trim().parse::<f64>().ok()).filter(|s| *s > 0.0).map(|s| now() + s as i64);
    Ok(OAuthToken {
        access_token,
        token_type: get("token_type"),
        refresh_token: get("refresh_token"),
        id_token: get("id_token"),
        expires_at,
        scope: get("scope"),
    })
}

/// Query and fragment parameters of a redirect URL.
pub fn callback_params(redirect: &str) -> HashMap<String, String> {
    let redirect = redirect.trim();
    let mut out = HashMap::new();
    let (before_frag, frag) = redirect.split_once('#').unwrap_or((redirect, ""));
    let query = before_frag.split_once('?').map(|(_, q)| q).unwrap_or("");
    for part in [query, frag] {
        out.extend(url::form_urlencoded::parse(part.as_bytes()).into_owned());
    }
    out
}

/// Waits for the browser redirect on the local callback, or for a pasted redirect URL.
async fn wait_for_callback(listeners: Vec<TcpListener>, path: &str, pasted: oneshot::Receiver<String>) -> Result<HashMap<String, String>, String> {
    let listening = !listeners.is_empty();
    let local = async {
        if !listening {
            return std::future::pending().await;
        }
        loop {
            let accepts = listeners.iter().map(|l| Box::pin(l.accept()));
            let (res, _, _) = futures_select_all(accepts).await;
            let Ok((stream, _)) = res else { continue };
            if let Some(params) = handle_callback_connection(stream, path).await {
                return params;
            }
        }
    };
    tokio::select! {
        params = local => Ok(params),
        url = pasted => match url {
            Ok(u) => {
                let params = callback_params(&u);
                if params.is_empty() { Err("The pasted URL has no parameters from the authorization server".into()) } else { Ok(params) }
            }
            Err(_) => std::future::pending().await,
        },
    }
}

/// Minimal `select_all` over accept futures (avoids a dependency on `futures`).
async fn futures_select_all<F: std::future::Future + Unpin>(futs: impl Iterator<Item = F>) -> (F::Output, usize, ()) {
    let mut futs: Vec<F> = futs.collect();
    std::future::poll_fn(move |cx| {
        for (i, f) in futs.iter_mut().enumerate() {
            if let std::task::Poll::Ready(out) = std::pin::Pin::new(f).poll(cx) {
                return std::task::Poll::Ready((out, i, ()));
            }
        }
        std::task::Poll::Pending
    })
    .await
}

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>Rustman</title><body style=\"font-family:sans-serif;text-align:center;margin-top:15vh\"><h2>Authorization complete</h2><p>You can close this tab and return to Rustman.</p>";
const PAGE_ERROR: &str = "<!doctype html><meta charset=utf-8><title>Rustman</title><body style=\"font-family:sans-serif;text-align:center;margin-top:15vh\"><h2>Authorization failed</h2><p>Return to Rustman for details.</p>";
/// The implicit grant puts the token in the fragment, which never reaches the server:
/// this page sends it back as a query string.
const PAGE_FRAGMENT: &str = "<!doctype html><meta charset=utf-8><title>Rustman</title><body style=\"font-family:sans-serif;text-align:center;margin-top:15vh\"><p id=m>Completing authorization…</p><script>var h=location.hash.slice(1);if(h){location.replace(location.pathname+'?'+h)}else{document.getElementById('m').textContent='No authorization data in this URL.'}</script>";

/// Serves one request on the callback socket. Returns the parameters once the redirect arrived.
async fn handle_callback_connection(mut stream: TcpStream, path: &str) -> Option<HashMap<String, String>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(10), stream.read(&mut chunk)).await.ok()?.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 32 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head.lines().next()?.split_whitespace().nth(1)?.to_string();
    let (req_path, query) = target.split_once('?').unwrap_or((&target, ""));
    let (status, page, result) = if req_path != path && !(path.is_empty() && req_path == "/") {
        ("404 Not Found", "Not found", None)
    } else if query.is_empty() {
        ("200 OK", PAGE_FRAGMENT, None)
    } else {
        let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
        let page = if params.contains_key("error") { PAGE_ERROR } else { PAGE_DONE };
        ("200 OK", page, Some(params))
    };
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
    result
}

/// Opens `url` in the default browser.
pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot open the browser: {e}"))?;
    // Reap the process so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// 43 characters of URL-safe randomness (256 bits from two v4 UUIDs' random bytes).
fn random_string() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// SHA-256 (FIPS 180-4), for the PKCE code challenge.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha256_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(hex(&sha256(long)), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    }

    #[test]
    fn pkce_authorization_url() {
        // Expected challenge computed with Python's hashlib.
        let verifier = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";
        let cfg = OAuth2 { auth_url: "https://a.example/authorize?x=1".into(), client_id: "c".into(), scope: "read write".into(), ..Default::default() };
        let url = authorization_url(&cfg, GrantType::AuthorizationCodePkce, "st", verifier);
        assert!(url.starts_with("https://a.example/authorize?x=1&response_type=code&"), "{url}");
        let p = callback_params(&url);
        assert_eq!(p["code_challenge"], "RqIZl4LIgn8KxW9QO-nTnv7pf0CnNrksx9fF-CXP2FE");
        assert_eq!(p["code_challenge_method"], "S256");
        assert_eq!(p["redirect_uri"], crate::model::DEFAULT_CALLBACK_URL);
        assert_eq!((p["scope"].as_str(), p["state"].as_str(), p["client_id"].as_str()), ("read write", "st", "c"));
        let cfg = OAuth2 { pkce_method: PkceMethod::Plain, ..cfg };
        assert_eq!(callback_params(&authorization_url(&cfg, GrantType::AuthorizationCodePkce, "st", verifier))["code_challenge"], verifier);
        assert_eq!(callback_params(&authorization_url(&cfg, GrantType::Implicit, "st", ""))["response_type"], "token");
    }

    #[test]
    fn parses_token_responses() {
        let resp = |status: u16, body: &str| ResponseData { status, body: body.as_bytes().to_vec(), ..Default::default() };
        let t = parse_token_response(&resp(200, r#"{"access_token":"A","token_type":"Bearer","expires_in":3600,"refresh_token":"R","scope":"read"}"#)).unwrap();
        assert_eq!((t.access_token.as_str(), t.refresh_token.as_str(), t.scope.as_str()), ("A", "R", "read"));
        assert!(t.expires_at.unwrap() > now() + 3500);
        let t = parse_token_response(&resp(200, "access_token=gho_x&scope=repo&token_type=bearer")).unwrap();
        assert_eq!(t.access_token, "gho_x");
        assert!(t.expires_at.is_none());
        let e = parse_token_response(&resp(400, r#"{"error":"invalid_client","error_description":"bad secret"}"#)).unwrap_err();
        assert!(e.contains("invalid_client: bad secret"), "{e}");
        assert!(parse_token_response(&resp(500, "<html>oops</html>")).unwrap_err().contains("HTTP 500"));
    }

    #[test]
    fn callback_params_read_query_and_fragment() {
        let p = callback_params("http://localhost:5005/callback?code=abc&state=s1");
        assert_eq!((p["code"].as_str(), p["state"].as_str()), ("abc", "s1"));
        let p = callback_params("https://x/cb#access_token=T%2B1&expires_in=60");
        assert_eq!(p["access_token"], "T+1");
    }

    #[test]
    fn validation_errors() {
        let e = Session::new(&OAuth2 { grant_type: GrantType::ClientCredentials, ..Default::default() }).err().unwrap();
        assert!(e.contains("Access Token URL"), "{e}");
        let cfg = OAuth2 { grant_type: GrantType::AuthorizationCode, access_token_url: "https://x/token".into(), client_id: "c".into(), ..Default::default() };
        assert!(Session::new(&cfg).err().unwrap().contains("Auth URL"));
        // A non-local callback URL does not listen (the user pastes the redirect).
        let cfg = OAuth2 { auth_url: "https://x/auth".into(), callback_url: "https://example.com/cb".into(), ..cfg };
        let s = Session::new(&cfg).unwrap();
        assert!(!s.listens);
        assert!(s.auth_url.unwrap().contains("redirect_uri=https%3A%2F%2Fexample.com%2Fcb"));
    }

    // --- End to end, against a fake authorization server ------------------------------

    /// `/authorize` redirects back with a code (or a token in the fragment for the implicit
    /// grant), `/token` issues tokens, `/echo` returns the Authorization header it got.
    async fn fake_server(listener: TcpListener) {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head, mut body) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break (String::from_utf8_lossy(&buf[..i]).to_string(), buf[i + 4..].to_vec());
                    }
                };
                let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                while body.len() < len {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..n]);
                }
                let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let header = |name: &str| head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
                let q = callback_params(&target);
                let (status, extra, out) = if target.starts_with("/authorize") {
                    let redirect = &q["redirect_uri"];
                    let location = if q["response_type"] == "token" {
                        format!("{redirect}#access_token=implicit-token&token_type=bearer&expires_in=120&state={}", q["state"])
                    } else {
                        // The code carries the PKCE challenge so /token can check the verifier.
                        let code = q.get("code_challenge").map(|c| format!("code.{c}")).unwrap_or_else(|| "code".into());
                        format!("{redirect}?code={}&state={}", http::encode_component(&code), q["state"])
                    };
                    ("302 Found", format!("Location: {location}\r\n"), String::new())
                } else if target == "/token" {
                    let f: HashMap<String, String> = url::form_urlencoded::parse(&body).into_owned().collect();
                    let basic = header("authorization");
                    let client_ok = basic == format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("cid:secret"))
                        || f.get("client_id").map(String::as_str) == Some("cid");
                    let token = match f.get("grant_type").map(String::as_str) {
                        _ if !client_ok => None,
                        Some("client_credentials") => Some(format!(r#"{{"access_token":"cc:{}:{}","token_type":"Bearer","expires_in":3600}}"#, f.get("scope").cloned().unwrap_or_default(), f.get("audience").cloned().unwrap_or_default())),
                        Some("password") if f["username"] == "u" && f["password"] == "p" => Some(r#"{"access_token":"pw","expires_in":"60"}"#.into()),
                        Some("authorization_code") => {
                            let ok = match f["code"].strip_prefix("code.") {
                                Some(challenge) => f.get("code_verifier").is_some_and(|v| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256(v.as_bytes())) == challenge),
                                None => f["code"] == "code",
                            };
                            ok.then(|| r#"{"access_token":"ac","refresh_token":"r1","expires_in":3600}"#.to_string())
                        }
                        Some("refresh_token") if f["refresh_token"] == "r1" => Some(r#"{"access_token":"refreshed","expires_in":3600}"#.into()),
                        _ => None,
                    };
                    match token {
                        Some(t) => ("200 OK", "Content-Type: application/json\r\n".into(), t),
                        None => ("400 Bad Request", "Content-Type: application/json\r\n".into(), r#"{"error":"invalid_grant","error_description":"nope"}"#.into()),
                    }
                } else {
                    ("200 OK", "Content-Type: application/json\r\n".into(), serde_json::json!({ "auth": header("authorization"), "target": target }).to_string())
                };
                let resp = format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    /// Plays the browser: follows the authorization redirect to the callback. A token in the
    /// fragment is moved to the query, like the page served on the callback does.
    fn fake_browser(url: &str) -> Result<(), String> {
        let url = url.to_string();
        tokio::spawn(async move {
            let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
            let resp = client.get(&url).send().await.unwrap();
            let location = resp.headers()["location"].to_str().unwrap().replace('#', "?");
            client.get(&location).send().await.unwrap();
        });
        Ok(())
    }

    fn jar() -> Arc<reqwest::cookie::Jar> {
        Arc::new(reqwest::cookie::Jar::default())
    }

    #[test]
    fn grants_end_to_end() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(fake_server(listener));
        let settings = RequestSettings::default();
        let base_cfg = OAuth2 {
            auth_url: format!("{base}/authorize"),
            access_token_url: format!("{base}/token"),
            client_id: "cid".into(),
            client_secret: "secret".into(),
            ..Default::default()
        };
        let get = |cfg: OAuth2| {
            let session = Session::new(&cfg).unwrap();
            let (_tx, rx) = oneshot::channel();
            rt.block_on(async { tokio::time::timeout(std::time::Duration::from_secs(10), session.run(settings.clone(), jar(), fake_browser, rx)).await.expect("flow timed out") })
        };

        // Client credentials, secret in the Basic header, scope and audience in the body.
        let out = get(OAuth2 { grant_type: GrantType::ClientCredentials, scope: "read".into(), audience: "api".into(), ..base_cfg.clone() });
        assert_eq!(out.token.unwrap().access_token, "cc:read:api");
        assert_eq!(out.exchanges.len(), 1, "the token request is reported for the console");

        // Password, client credentials in the body; expires_in as a string.
        let out = get(OAuth2 { grant_type: GrantType::Password, username: "u".into(), password: "p".into(), client_auth: ClientAuth::Body, ..base_cfg.clone() });
        let t = out.token.unwrap();
        assert_eq!(t.access_token, "pw");
        assert!(t.expires_at.is_some());

        // Authorization code: the redirect is caught on a local port.
        let cb = |path: &str| format!("http://localhost:{}{path}", free_port());
        let out = get(OAuth2 { grant_type: GrantType::AuthorizationCode, callback_url: cb("/cb"), ..base_cfg.clone() });
        let t = out.token.unwrap();
        assert_eq!((t.access_token.as_str(), t.refresh_token.as_str()), ("ac", "r1"));

        // With PKCE, a public client (no secret): the server checks the verifier.
        let out = get(OAuth2 { grant_type: GrantType::AuthorizationCodePkce, callback_url: cb("/cb"), client_secret: String::new(), ..base_cfg.clone() });
        assert_eq!(out.token.unwrap().access_token, "ac");

        // Implicit: the token comes in the fragment.
        let out = get(OAuth2 { grant_type: GrantType::Implicit, callback_url: cb("/"), ..base_cfg.clone() });
        assert_eq!(out.token.unwrap().access_token, "implicit-token");
        assert!(out.exchanges.is_empty());

        // Wrong secret: the server's error is reported.
        let out = get(OAuth2 { grant_type: GrantType::ClientCredentials, client_secret: "bad".into(), ..base_cfg.clone() });
        assert!(out.token.unwrap_err().contains("invalid_grant: nope"));

        // Refresh.
        let out = rt.block_on(refresh(&OAuth2 { refresh_token: "r1".into(), ..base_cfg.clone() }, &settings, jar()));
        assert_eq!(out.token.unwrap().access_token, "refreshed");
    }

    #[test]
    fn pasted_redirect_url_and_state_check() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(fake_server(listener));
        let cfg = OAuth2 {
            grant_type: GrantType::AuthorizationCode,
            auth_url: format!("{base}/authorize"),
            access_token_url: format!("{base}/token"),
            callback_url: "https://app.example.com/cb".into(),
            client_id: "cid".into(),
            client_secret: "secret".into(),
            state: "xyz".into(),
            ..Default::default()
        };
        let run = |pasted: &str| {
            let session = Session::new(&cfg).unwrap();
            assert!(!session.listens);
            let (tx, rx) = oneshot::channel();
            tx.send(pasted.to_string()).unwrap();
            rt.block_on(session.run(RequestSettings::default(), jar(), |_: &str| Ok(()), rx)).token
        };
        assert_eq!(run("https://app.example.com/cb?code=code&state=xyz").unwrap().access_token, "ac");
        assert!(run("https://app.example.com/cb?code=code&state=evil").unwrap_err().contains("state"));
        assert!(run("https://app.example.com/cb?error=access_denied&error_description=User+said+no").unwrap_err().contains("access_denied: User said no"));
    }

    #[test]
    fn expired_token_is_refreshed_before_sending() {
        use crate::exec::{self, Job};
        use crate::model::{Auth, AuthKind, Request};
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(fake_server(listener));
        let oauth = OAuth2 {
            access_token: "old".into(),
            refresh_token: "r1".into(),
            expires_at: Some(now() - 5),
            access_token_url: "{{base}}/token".into(),
            client_id: "cid".into(),
            client_secret: "secret".into(),
            ..Default::default()
        };
        let job = |request_auth: Auth, collection_auth: Option<Auth>| Job {
            request: Request { url: "{{base}}/echo".into(), auth: request_auth, ..Default::default() },
            scripts: Vec::new(),
            auth: collection_auth,
            auth_owner: crate::model::AuthOwner::Collection,
            scopes: crate::script::Scopes::new(&[], &[crate::model::KeyValue::new("base", &base)], None, &[]),
            environment_name: None,
            info: Default::default(),
        };
        let oauth_auth = Auth { kind: AuthKind::OAuth2, oauth2: Box::new(oauth.clone()), ..Default::default() };

        // The request's own auth.
        let ex = rt.block_on(exec::run(job(oauth_auth.clone(), None), jar(), |_, _| {}));
        let body: serde_json::Value = serde_json::from_slice(&ex.result.unwrap().body).unwrap();
        assert_eq!(body["auth"], "Bearer refreshed");
        let u = ex.report.token_update.unwrap();
        assert_eq!(u.owner, None);
        assert_eq!(u.token.access_token, "refreshed");
        assert_eq!(ex.report.pre_events.len(), 1, "the refresh call shows in the console");

        // Inherited from the collection.
        let ex = rt.block_on(exec::run(job(Auth::default(), Some(oauth_auth.clone())), jar(), |_, _| {}));
        assert_eq!(ex.report.token_update.unwrap().owner, Some(crate::model::AuthOwner::Collection));

        // Auto-refresh off: the old token is sent as is.
        let off = Auth { oauth2: Box::new(OAuth2 { auto_refresh: false, ..oauth.clone() }), ..oauth_auth };
        let ex = rt.block_on(exec::run(job(off, None), jar(), |_, _| {}));
        let body: serde_json::Value = serde_json::from_slice(&ex.result.unwrap().body).unwrap();
        assert_eq!(body["auth"], "Bearer old");
        assert!(ex.report.token_update.is_none());
    }
}
