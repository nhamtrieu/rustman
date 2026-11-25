//! MCP server for toolsets: exposes saved requests as tools to AI agents (Claude Code, Cursor, ...).
//!
//! Transport: MCP "Streamable HTTP" without server-initiated streams — every JSON-RPC POST to
//! `/mcp` gets a plain `application/json` answer. Methods: initialize, ping, tools/list, tools/call.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use crate::http::{self, human_duration};
use crate::mock::{build_response, read_request};
use crate::model::{Auth, Request, ToolParam};
use crate::vars::VarMap;

const MAX_LOG: usize = 200;
/// Response bodies handed to the agent are cut to this many characters.
const MAX_RESULT: usize = 60_000;
const LATEST_PROTOCOL: &str = "2025-06-18";

/// Everything needed to run one tool, resolved from the workspace by the UI.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRuntime {
    pub name: String,
    pub description: String,
    pub params: Vec<ToolParam>,
    pub request: Request,
    /// Globals + collection + active environment variables.
    pub base_vars: VarMap,
    /// Collection auth for requests that inherit it.
    pub auth: Option<Auth>,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub time: chrono::DateTime<chrono::Local>,
    pub tool: String,
    pub args: String,
    pub ok: bool,
    pub summary: String,
}

#[derive(Default)]
pub struct Shared {
    pub server_name: String,
    pub tools: Vec<ToolRuntime>,
    pub log: VecDeque<ToolCall>,
}

pub type SharedRef = Arc<Mutex<Shared>>;

pub fn endpoint(port: u16) -> String {
    format!("http://localhost:{port}/mcp")
}

/// `claude mcp add` command for Claude Code.
pub fn claude_command(name: &str, port: u16) -> String {
    format!("claude mcp add --transport http {} {}", slug(name), endpoint(port))
}

/// JSON snippet for clients configured with an `mcpServers` file (Cursor, Claude Desktop via mcp-remote, ...).
pub fn json_config(name: &str, port: u16) -> String {
    let v = json!({ "mcpServers": { slug(name): { "type": "http", "url": endpoint(port) } } });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Lowercase identifier with `_` (`Get user` -> `get_user`), as tool and server names require.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    let out: String = out.chars().take(64).collect();
    if out.is_empty() { "tool".into() } else { out }
}

/// Valid MCP tool name: 1-64 chars of `[A-Za-z0-9_-]`.
pub fn valid_tool_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `{{variables}}` used anywhere in a request.
pub fn request_vars(r: &Request) -> Vec<String> {
    use crate::vars::var_names;
    let mut out = Vec::new();
    var_names(&r.url, &mut out);
    for kv in r.headers.iter().chain(&r.body.urlencoded).filter(|k| k.enabled) {
        var_names(&kv.key, &mut out);
        var_names(&kv.value, &mut out);
    }
    for f in r.body.form_data.iter().filter(|f| f.enabled) {
        var_names(&f.key, &mut out);
        var_names(&f.value, &mut out);
    }
    for s in [&r.body.raw, &r.body.graphql_query, &r.body.graphql_variables, &r.body.binary_path] {
        var_names(s, &mut out);
    }
    let a = &r.auth;
    for s in [&a.bearer_token, &a.basic_username, &a.basic_password, &a.api_key_name, &a.api_key_value, &a.oauth2.access_token, &a.oauth2.header_prefix] {
        var_names(s, &mut out);
    }
    out
}

pub fn input_schema(params: &[ToolParam]) -> Value {
    let mut props = serde_json::Map::new();
    for p in params {
        let mut prop = json!({ "type": "string" });
        if !p.description.trim().is_empty() {
            prop["description"] = json!(p.description.trim());
        }
        props.insert(p.name.clone(), prop);
    }
    let required: Vec<&str> = params.iter().filter(|p| p.required).map(|p| p.name.as_str()).collect();
    json!({ "type": "object", "properties": props, "required": required })
}

pub async fn bind(port: u16) -> Result<TcpListener, String> {
    crate::mock::bind(port).await
}

pub async fn serve(listener: TcpListener, shared: SharedRef, jar: Arc<reqwest::cookie::Jar>, on_call: impl Fn() + Send + Sync + Clone + 'static) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let (shared, jar, on_call) = (shared.clone(), jar.clone(), on_call.clone());
        tokio::spawn(async move {
            if handle(stream, shared, jar).await {
                on_call();
            }
        });
    }
}

/// Handles one HTTP connection; returns true when a tool was called.
async fn handle(mut stream: TcpStream, shared: SharedRef, jar: Arc<reqwest::cookie::Jar>) -> bool {
    let Some(req) = read_request(&mut stream).await else { return false };
    let path = req.target.split('?').next().unwrap_or("/");
    let json_ct = vec![("Content-Type".to_string(), "application/json".to_string())];
    let (status, headers, body, called) = if path != "/mcp" && path != "/" {
        (404, json_ct, json!({ "error": "MCP endpoint is /mcp" }).to_string(), false)
    } else {
        match req.method.as_str() {
            "POST" => match serde_json::from_slice::<Value>(&req.body) {
                Err(e) => (400, json_ct, rpc_error(Value::Null, -32700, &format!("Parse error: {e}")).to_string(), false),
                Ok(msg) => {
                    let (reply, called) = dispatch_all(msg, &shared, &jar).await;
                    match reply {
                        Some(v) => (200, json_ct, v.to_string(), called),
                        None => (202, Vec::new(), String::new(), called),
                    }
                }
            },
            "OPTIONS" => (204, Vec::new(), String::new(), false),
            "DELETE" => (200, Vec::new(), String::new(), false),
            // No server-initiated SSE stream.
            _ => (405, vec![("Allow".to_string(), "POST, DELETE, OPTIONS".to_string())], String::new(), false),
        }
    };
    let _ = stream.write_all(&build_response(status, &headers, &body, false)).await;
    let _ = stream.shutdown().await;
    called
}

/// Single message or batch. Notifications get no reply; `None` when nothing needs answering.
async fn dispatch_all(msg: Value, shared: &SharedRef, jar: &Arc<reqwest::cookie::Jar>) -> (Option<Value>, bool) {
    match msg {
        Value::Array(items) => {
            let mut out = Vec::new();
            let mut called = false;
            for m in items {
                let (r, c) = dispatch(m, shared, jar).await;
                called |= c;
                out.extend(r);
            }
            ((!out.is_empty()).then_some(Value::Array(out)), called)
        }
        m => dispatch(m, shared, jar).await,
    }
}

async fn dispatch(msg: Value, shared: &SharedRef, jar: &Arc<reqwest::cookie::Jar>) -> (Option<Value>, bool) {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else { return (None, false) }; // notification (e.g. notifications/initialized)
    let reply = |result: Value| json!({ "jsonrpc": "2.0", "id": id.clone(), "result": result });
    match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(LATEST_PROTOCOL);
            let name = shared.lock().map(|s| s.server_name.clone()).unwrap_or_default();
            let result = json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": slug(&name), "title": name, "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Each tool sends an HTTP request saved in Rustman and returns the response status and body.",
            });
            (Some(reply(result)), false)
        }
        "ping" => (Some(reply(json!({}))), false),
        "tools/list" => {
            let tools: Vec<Value> = shared
                .lock()
                .map(|s| {
                    s.tools
                        .iter()
                        .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": input_schema(&t.params) }))
                        .collect()
                })
                .unwrap_or_default();
            (Some(reply(json!({ "tools": tools }))), false)
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let tool = shared.lock().ok().and_then(|s| s.tools.iter().find(|t| t.name == name).cloned());
            let Some(tool) = tool else {
                return (Some(rpc_error(id, -32602, &format!("Unknown tool: {name}"))), false);
            };
            let (text, is_error, summary) = call_tool(&tool, &args, jar).await;
            if let Ok(mut s) = shared.lock() {
                s.log.push_back(ToolCall { time: chrono::Local::now(), tool: name, args: args.to_string(), ok: !is_error, summary });
                while s.log.len() > MAX_LOG {
                    s.log.pop_front();
                }
            }
            (Some(reply(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))), true)
        }
        _ => (Some(rpc_error(id, -32601, &format!("Method not found: {method}"))), false),
    }
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Runs the request with the arguments as variables. Returns (text for the agent, is_error, one-line summary).
pub async fn call_tool(tool: &ToolRuntime, args: &Value, jar: &Arc<reqwest::cookie::Jar>) -> (String, bool, String) {
    let mut vars = tool.base_vars.clone();
    if let Some(obj) = args.as_object() {
        for (k, v) in obj {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            vars.insert(k.clone(), s);
        }
    }
    let missing: Vec<&str> = tool.params.iter().filter(|p| p.required && !vars.contains_key(&p.name)).map(|p| p.name.as_str()).collect();
    if !missing.is_empty() {
        let msg = format!("Missing required argument(s): {}", missing.join(", "));
        return (msg.clone(), true, msg);
    }
    let prepared = match http::prepare(&tool.request, &vars, tool.auth.as_ref()) {
        Ok(p) => p,
        Err(e) => return (format!("Cannot build the request: {e}"), true, e),
    };
    let line = format!("{} {}", prepared.method.as_str(), prepared.url);
    match http::execute(prepared, jar.clone()).await {
        Err(e) => (format!("{line}\n\n{e}"), true, e.lines().next().unwrap_or("").to_string()),
        Ok(r) => {
            let mut body = match serde_json::from_slice::<Value>(&r.body) {
                Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
                Err(_) => String::from_utf8_lossy(&r.body).into_owned(),
            };
            if body.len() > MAX_RESULT {
                let mut cut = MAX_RESULT;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
                body.push_str(&format!("\n… (truncated, {} bytes total)", r.body.len()));
            }
            let ct = r.header("content-type").unwrap_or("").to_string();
            let status = format!("{} {}", r.status, r.status_text).trim().to_string();
            let text = format!("{line}\nHTTP {status} ({})\ncontent-type: {ct}\n\n{body}", human_duration(r.duration));
            (text, r.status >= 400, format!("{status} · {}", human_duration(r.duration)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KeyValue, Method, MockRoute};

    #[test]
    fn names_and_schema() {
        assert_eq!(slug("Get user by ID!"), "get_user_by_id");
        assert_eq!(slug("  "), "tool");
        assert!(valid_tool_name("get_user-2") && !valid_tool_name("get user"));
        let schema = input_schema(&[ToolParam { name: "id".into(), description: "User id".into(), required: true }, ToolParam { name: "q".into(), ..Default::default() }]);
        assert_eq!(schema["required"], json!(["id"]));
        assert_eq!(schema["properties"]["id"]["description"], "User id");
        let mut r = Request { url: "{{base}}/users/{{id}}?ts={{$timestamp}}".into(), ..Default::default() };
        r.headers.push(KeyValue::new("Authorization", "Bearer {{token}}"));
        r.body.raw = r#"{"name":"{{name}}","id":"{{id}}"}"#.into();
        assert_eq!(request_vars(&r), ["base", "id", "token", "name"]);
    }

    /// Full MCP round trip: initialize, tools/list, tools/call against a local mock server.
    #[test]
    fn mcp_round_trip() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Upstream API: a mock server.
            let api = crate::mock::bind(0).await.unwrap();
            let api_port = api.local_addr().unwrap().port();
            let api_shared: crate::mock::SharedRef = Default::default();
            api_shared.lock().unwrap().routes = vec![MockRoute { method: Some(Method::Get), path: "/users/:id".into(), body: r#"{"id":"{{id}}"}"#.into(), ..Default::default() }];
            tokio::spawn(crate::mock::serve(api, api_shared, || {}));

            let listener = bind(0).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let shared: SharedRef = Default::default();
            {
                let mut s = shared.lock().unwrap();
                s.server_name = "Users API".into();
                s.tools.push(ToolRuntime {
                    name: "get_user".into(),
                    description: "Get a user".into(),
                    params: vec![ToolParam { name: "id".into(), description: String::new(), required: true }],
                    request: Request { url: "{{base}}/users/{{id}}".into(), ..Default::default() },
                    base_vars: [("base".to_string(), format!("http://127.0.0.1:{api_port}"))].into_iter().collect(),
                    auth: None,
                });
            }
            tokio::spawn(serve(listener, shared.clone(), Default::default(), || {}));

            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{port}/mcp");
            let rpc = |body: Value| client.post(&url).header("Accept", "application/json, text/event-stream").json(&body).send();
            let init: Value = rpc(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}})).await.unwrap().json().await.unwrap();
            assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
            assert_eq!(init["result"]["serverInfo"]["name"], "users_api");
            let note = rpc(json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await.unwrap();
            assert_eq!(note.status(), 202);
            let list: Value = rpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await.unwrap().json().await.unwrap();
            assert_eq!(list["result"]["tools"][0]["name"], "get_user");
            assert_eq!(list["result"]["tools"][0]["inputSchema"]["required"], json!(["id"]));
            let call: Value = rpc(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_user","arguments":{"id":42}}})).await.unwrap().json().await.unwrap();
            assert_eq!(call["result"]["isError"], false);
            let text = call["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("HTTP 200 OK") && text.contains("\"id\": \"42\""), "{text}");
            let missing: Value = rpc(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_user","arguments":{}}})).await.unwrap().json().await.unwrap();
            assert_eq!(missing["result"]["isError"], true);
            let unknown: Value = rpc(json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}})).await.unwrap().json().await.unwrap();
            assert_eq!(unknown["error"]["code"], -32602);
            assert_eq!(client.get(&url).send().await.unwrap().status(), 405);
            assert_eq!(shared.lock().unwrap().log.len(), 2);
        });
    }
}
