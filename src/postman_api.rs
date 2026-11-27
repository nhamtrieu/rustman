//! Postman API client (https://api.getpostman.com): lists the workspaces, collections and
//! environments of an account and downloads them, so they can be imported without exporting
//! files from the Postman app. Needs a Postman API key (`PMAK-...`).
//!
//! Requests go through `http::execute`, so the proxy and certificate settings apply.

use std::sync::Arc;

use serde_json::Value;

use crate::http::{self, Prepared, PreparedBody};
use crate::model::{Method, RequestSettings};

pub const API_BASE: &str = "https://api.getpostman.com";
/// Where users create a key.
pub const KEYS_URL: &str = "https://go.postman.co/settings/me/api-keys";

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// `personal`, `team`, `public`, ...
    pub kind: String,
}

/// A collection or environment in a workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub uid: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub collections: Vec<Entry>,
    pub environments: Vec<Entry>,
}

pub struct Client {
    base: String,
    key: String,
    jar: Arc<reqwest::cookie::Jar>,
}

impl Client {
    pub fn new(key: &str) -> Self {
        Self::with_base(API_BASE, key)
    }

    pub fn with_base(base: &str, key: &str) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), key: key.trim().to_string(), jar: Arc::new(reqwest::cookie::Jar::default()) }
    }

    async fn get(&self, path: &str) -> Result<String, String> {
        if self.key.is_empty() {
            return Err("Enter your Postman API key".into());
        }
        let p = Prepared {
            method: Method::Get,
            url: format!("{}{path}", self.base),
            headers: vec![("X-Api-Key".into(), self.key.clone()), ("Accept".into(), "application/json".into())],
            body: PreparedBody::None,
            settings: RequestSettings { timeout_ms: 60_000, ..Default::default() },
            digest: None,
        };
        let r = http::execute(p, self.jar.clone()).await.map_err(|e| format!("Cannot reach the Postman API: {e}"))?;
        let text = r.text();
        match r.status {
            200..=299 => Ok(text),
            401 | 403 => Err("Postman rejected the API key. Check that it is complete and not expired, or create a new one in Postman → Settings → API keys.".into()),
            429 => Err("Too many requests to the Postman API. Wait a minute and try again.".into()),
            code => {
                // {"error": {"name": "...", "message": "..."}}
                let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string));
                Err(format!("Postman API error {code}: {}", msg.unwrap_or(text)))
            }
        }
    }

    async fn get_json(&self, path: &str) -> Result<Value, String> {
        let text = self.get(path).await?;
        serde_json::from_str(&text).map_err(|e| format!("Unexpected answer from the Postman API: {e}"))
    }

    pub async fn workspaces(&self) -> Result<Vec<Workspace>, String> {
        let v = self.get_json("/workspaces").await?;
        let mut out: Vec<Workspace> = v["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| Some(Workspace { id: w["id"].as_str()?.to_string(), name: w["name"].as_str().unwrap_or("Untitled").to_string(), kind: w["type"].as_str().unwrap_or("").to_string() }))
            .collect();
        out.sort_by_key(|w| (w.kind != "personal", w.name.to_lowercase()));
        Ok(out)
    }

    /// Collections and environments of one workspace.
    pub async fn listing(&self, workspace: &str) -> Result<Listing, String> {
        let v = self.get_json(&format!("/workspaces/{workspace}")).await?;
        let entries = |key: &str| -> Vec<Entry> {
            let mut list: Vec<Entry> = v["workspace"][key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| Some(Entry { uid: e["uid"].as_str().or_else(|| e["id"].as_str())?.to_string(), name: e["name"].as_str().unwrap_or("Untitled").to_string() }))
                .collect();
            list.sort_by_key(|e| e.name.to_lowercase());
            list
        };
        Ok(Listing { collections: entries("collections"), environments: entries("environments") })
    }

    /// Collection JSON (v2.1, wrapped in `{"collection": ...}`), for `import_postman_collection`.
    pub async fn collection(&self, uid: &str) -> Result<String, String> {
        self.get(&format!("/collections/{uid}")).await
    }

    /// Environment JSON (wrapped in `{"environment": ...}`), for `import_postman_environment`.
    pub async fn environment(&self, uid: &str) -> Result<String, String> {
        self.get(&format!("/environments/{uid}")).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves canned Postman API answers and checks the API key.
    async fn serve(listener: tokio::net::TcpListener) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let key_ok = req.lines().any(|l| l.eq_ignore_ascii_case("x-api-key: PMAK-good"));
                let (status, body) = if !key_ok {
                    ("401 Unauthorized", r#"{"error":{"name":"AuthenticationError","message":"Invalid API Key."}}"#.to_string())
                } else {
                    match path.as_str() {
                        "/workspaces" => ("200 OK", r#"{"workspaces":[{"id":"t1","name":"Team","type":"team"},{"id":"w1","name":"My Workspace","type":"personal"}]}"#.into()),
                        "/workspaces/w1" => ("200 OK", r#"{"workspace":{"id":"w1","collections":[{"id":"c","uid":"1-c","name":"Shop"}],"environments":[{"id":"e","uid":"1-e","name":"Dev"}]}}"#.into()),
                        "/collections/1-c" => ("200 OK", r#"{"collection":{"info":{"name":"Shop","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[{"name":"List","request":{"method":"GET","url":"{{base}}/items"}}]}}"#.into()),
                        "/environments/1-e" => ("200 OK", r#"{"environment":{"id":"e","name":"Dev","values":[{"key":"base","value":"http://dev","enabled":true}]}}"#.into()),
                        _ => ("404 Not Found", r#"{"error":{"name":"instanceNotFoundError","message":"We could not find the collection you are looking for"}}"#.into()),
                    }
                };
                let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    }

    #[test]
    fn lists_and_downloads_then_imports() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        rt.spawn(serve(listener));

        let bad = Client::with_base(&base, "PMAK-bad");
        assert!(rt.block_on(bad.workspaces()).unwrap_err().contains("rejected the API key"));
        assert_eq!(rt.block_on(Client::with_base(&base, " ").workspaces()).unwrap_err(), "Enter your Postman API key");

        let c = Client::with_base(&base, " PMAK-good ");
        let ws = rt.block_on(c.workspaces()).unwrap();
        assert_eq!(ws.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(), vec!["My Workspace", "Team"], "personal workspace first");
        let l = rt.block_on(c.listing("w1")).unwrap();
        assert_eq!((l.collections[0].uid.as_str(), l.environments[0].name.as_str()), ("1-c", "Dev"));

        let coll = crate::import::import_postman_collection(&rt.block_on(c.collection("1-c")).unwrap()).unwrap();
        assert_eq!((coll.name.as_str(), coll.items.len()), ("Shop", 1));
        let env = crate::import::import_postman_environment(&rt.block_on(c.environment("1-e")).unwrap()).unwrap();
        assert_eq!((env.name.as_str(), env.variables[0].value.as_str()), ("Dev", "http://dev"));
        assert!(rt.block_on(c.collection("nope")).unwrap_err().contains("could not find the collection"));
    }
}
