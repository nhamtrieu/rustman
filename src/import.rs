//! Import / export: Postman Collection v2.1, Postman environments, cURL, and code snippets.

use serde_json::{Value, json};

use crate::model::*;

// ---------------------------------------------------------------------------
// Postman Collection v2.1 -> Rustman
// ---------------------------------------------------------------------------

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn kv_list(v: Option<&Value>) -> Vec<KeyValue> {
    v.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|e| KeyValue {
                    enabled: !e.get("disabled").and_then(Value::as_bool).unwrap_or(false)
                        && e.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                    key: e.get("key").map(s).unwrap_or_default(),
                    value: e.get("value").map(s).unwrap_or_default(),
                    description: match e.get("description") {
                        Some(Value::Object(o)) => o.get("content").map(s).unwrap_or_default(),
                        Some(d) => s(d),
                        None => String::new(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Postman stores auth params as `[{key, value}]`.
fn auth_param(auth: &Value, kind: &str, key: &str) -> String {
    auth.get(kind)
        .and_then(Value::as_array)
        .and_then(|arr| arr.iter().find(|e| e.get("key").and_then(Value::as_str) == Some(key)))
        .and_then(|e| e.get("value"))
        .map(s)
        .unwrap_or_default()
}

fn parse_auth(v: Option<&Value>, default: AuthKind) -> Auth {
    let Some(auth) = v.filter(|v| v.is_object()) else {
        return Auth { kind: default, ..Default::default() };
    };
    match auth.get("type").and_then(Value::as_str).unwrap_or("") {
        "bearer" => Auth { kind: AuthKind::Bearer, bearer_token: auth_param(auth, "bearer", "token"), ..Default::default() },
        "basic" => Auth {
            kind: AuthKind::Basic,
            basic_username: auth_param(auth, "basic", "username"),
            basic_password: auth_param(auth, "basic", "password"),
            ..Default::default()
        },
        "apikey" => Auth {
            kind: AuthKind::ApiKey,
            api_key_name: auth_param(auth, "apikey", "key"),
            api_key_value: auth_param(auth, "apikey", "value"),
            api_key_in: if auth_param(auth, "apikey", "in") == "query" { ApiKeyLocation::Query } else { ApiKeyLocation::Header },
            ..Default::default()
        },
        "oauth2" => Auth { kind: AuthKind::OAuth2, oauth2: Box::new(parse_oauth2(auth)), ..Default::default() },
        "noauth" => Auth::none(),
        _ => Auth { kind: default, ..Default::default() },
    }
}

fn parse_oauth2(auth: &Value) -> OAuth2 {
    let p = |k: &str| auth_param(auth, "oauth2", k);
    let mut o = OAuth2 {
        access_token: p("accessToken"),
        add_to: if p("addTokenTo") == "queryParams" { TokenPlacement::Query } else { TokenPlacement::Header },
        grant_type: match p("grant_type").as_str() {
            "authorization_code_with_pkce" => GrantType::AuthorizationCodePkce,
            "implicit" => GrantType::Implicit,
            "password_credentials" => GrantType::Password,
            "client_credentials" => GrantType::ClientCredentials,
            _ => GrantType::AuthorizationCode,
        },
        token_name: p("tokenName"),
        auth_url: p("authUrl"),
        access_token_url: p("accessTokenUrl"),
        client_id: p("clientId"),
        client_secret: p("clientSecret"),
        scope: p("scope"),
        state: p("state"),
        username: p("username"),
        password: p("password"),
        pkce_method: if p("challengeAlgorithm") == "plain" { PkceMethod::Plain } else { PkceMethod::S256 },
        code_verifier: p("code_verifier"),
        client_auth: if p("client_authentication") == "body" { ClientAuth::Body } else { ClientAuth::BasicHeader },
        refresh_token_url: p("refreshTokenUrl"),
        ..Default::default()
    };
    let prefix = p("headerPrefix");
    if !prefix.is_empty() {
        o.header_prefix = prefix;
    }
    let callback = p("redirect_uri");
    if !callback.is_empty() {
        o.callback_url = callback;
    }
    // Advanced options are lists of {key, value, enabled, send_as}.
    for list in ["tokenRequestParams", "authRequestParams"] {
        let entries = auth
            .get("oauth2")
            .and_then(Value::as_array)
            .and_then(|arr| arr.iter().find(|e| e.get("key").and_then(Value::as_str) == Some(list)))
            .and_then(|e| e.get("value"))
            .and_then(Value::as_array);
        for e in entries.into_iter().flatten() {
            if e.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let value = e.get("value").map(s).unwrap_or_default();
            match e.get("key").and_then(Value::as_str) {
                Some("audience") if o.audience.is_empty() => o.audience = value,
                Some("resource") if o.resource.is_empty() => o.resource = value,
                _ => {}
            }
        }
    }
    o
}

fn parse_url(v: Option<&Value>) -> (String, Vec<KeyValue>) {
    match v {
        Some(Value::String(u)) => {
            let mut r = Request { url: u.clone(), ..Default::default() };
            r.sync_params_from_url();
            (r.url, r.params)
        }
        Some(obj @ Value::Object(_)) => {
            let raw = obj.get("raw").map(s).unwrap_or_default();
            let query = kv_list(obj.get("query"));
            if query.is_empty() {
                let mut r = Request { url: raw, ..Default::default() };
                r.sync_params_from_url();
                (r.url, r.params)
            } else {
                // Postman's `query` also contains disabled params which are not in `raw`.
                let mut r = Request { url: raw, params: query, ..Default::default() };
                r.sync_url_from_params();
                (r.url, r.params)
            }
        }
        _ => (String::new(), Vec::new()),
    }
}

fn parse_body(v: Option<&Value>) -> Body {
    let mut body = Body::default();
    let Some(b) = v.filter(|v| v.is_object()) else { return body };
    match b.get("mode").and_then(Value::as_str).unwrap_or("") {
        "raw" => {
            body.mode = BodyMode::Raw;
            body.raw = b.get("raw").map(s).unwrap_or_default();
            let lang = b.pointer("/options/raw/language").and_then(Value::as_str).unwrap_or("text");
            body.raw_lang = match lang {
                "json" => RawLang::Json,
                "xml" => RawLang::Xml,
                "html" => RawLang::Html,
                "javascript" => RawLang::JavaScript,
                _ if body.raw.trim_start().starts_with(['{', '[']) => RawLang::Json,
                _ => RawLang::Text,
            };
        }
        "urlencoded" => {
            body.mode = BodyMode::UrlEncoded;
            body.urlencoded = kv_list(b.get("urlencoded"));
        }
        "formdata" => {
            body.mode = BodyMode::FormData;
            body.form_data = b
                .get("formdata")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .map(|e| {
                            let is_file = e.get("type").and_then(Value::as_str) == Some("file");
                            FormField {
                                enabled: !e.get("disabled").and_then(Value::as_bool).unwrap_or(false),
                                key: e.get("key").map(s).unwrap_or_default(),
                                value: if is_file {
                                    match e.get("src") {
                                        Some(Value::Array(a)) => a.first().map(s).unwrap_or_default(),
                                        Some(v) => s(v),
                                        None => String::new(),
                                    }
                                } else {
                                    e.get("value").map(s).unwrap_or_default()
                                },
                                kind: if is_file { FormFieldKind::File } else { FormFieldKind::Text },
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        "file" => {
            body.mode = BodyMode::Binary;
            body.binary_path = b.pointer("/file/src").map(s).unwrap_or_default();
        }
        "graphql" => {
            body.mode = BodyMode::GraphQl;
            body.graphql_query = b.pointer("/graphql/query").map(s).unwrap_or_default();
            body.graphql_variables = b.pointer("/graphql/variables").map(s).unwrap_or_default();
        }
        _ => {}
    }
    body
}

fn description_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::Object(o)) => o.get("content").map(s).unwrap_or_default(),
        Some(d) => s(d),
        None => String::new(),
    }
}

fn parse_request(name: String, r: &Value) -> Request {
    // A request can be a bare URL string.
    if let Value::String(u) = r {
        let mut req = Request { name, url: u.clone(), ..Default::default() };
        req.sync_params_from_url();
        return req;
    }
    let (url, params) = parse_url(r.get("url"));
    Request {
        name,
        method: r.get("method").and_then(Value::as_str).and_then(Method::parse).unwrap_or_default(),
        url,
        params,
        headers: kv_list(r.get("header")),
        body: parse_body(r.get("body")),
        auth: parse_auth(r.get("auth"), AuthKind::Inherit),
        description: description_of(r.get("description")),
        ..Default::default()
    }
}

/// `(pre-request, test)` scripts from a Postman `event` array.
fn parse_events(v: Option<&Value>) -> (String, String) {
    let (mut pre, mut test) = (String::new(), String::new());
    for e in v.and_then(Value::as_array).into_iter().flatten() {
        let Some(script) = e.get("script") else { continue };
        let code = match script.get("exec") {
            Some(Value::Array(lines)) => lines.iter().map(s).collect::<Vec<_>>().join("\n"),
            Some(other) => s(other),
            None => continue,
        };
        if script.get("disabled").and_then(Value::as_bool).unwrap_or(false) || e.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let target = match e.get("listen").and_then(Value::as_str) {
            Some("prerequest") => &mut pre,
            Some("test") => &mut test,
            _ => continue,
        };
        if !target.is_empty() {
            target.push_str("\n\n");
        }
        target.push_str(&code);
    }
    (pre, test)
}

/// Prepends folder scripts (Rustman has no folder-level scripts) to every request below.
fn push_folder_scripts(items: &mut [Item], pre: &str, test: &str) {
    for item in items {
        match item {
            Item::Folder(f) => push_folder_scripts(&mut f.items, pre, test),
            Item::Request(r) => {
                for (own, folder) in [(&mut r.pre_request_script, pre), (&mut r.post_response_script, test)] {
                    if !folder.trim().is_empty() {
                        *own = if own.trim().is_empty() { folder.to_string() } else { format!("{folder}\n\n{own}") };
                    }
                }
            }
        }
    }
}

fn parse_items(v: Option<&Value>) -> Vec<Item> {
    let Some(arr) = v.and_then(Value::as_array) else { return Vec::new() };
    arr.iter()
        .map(|e| {
            let name = e.get("name").map(s).unwrap_or_else(|| "Untitled".into());
            let (pre, test) = parse_events(e.get("event"));
            if e.get("item").is_some() {
                let mut items = parse_items(e.get("item"));
                push_folder_scripts(&mut items, &pre, &test);
                Item::Folder(Folder { items, ..Folder::new(name) })
            } else {
                let mut r = parse_request(name, e.get("request").unwrap_or(&Value::Null));
                r.pre_request_script = pre;
                r.post_response_script = test;
                Item::Request(r)
            }
        })
        .collect()
}

pub fn import_postman_collection(json_text: &str) -> Result<Collection, String> {
    let v: Value = serde_json::from_str(json_text).map_err(|e| format!("Invalid JSON: {e}"))?;
    // Some exports wrap the collection: {"collection": {...}}
    let v = v.get("collection").cloned().unwrap_or(v);
    let info = v.get("info").ok_or("Not a Postman collection (missing `info`)")?;
    if v.get("item").is_none() {
        return Err("Not a Postman collection (missing `item`)".into());
    }
    let (pre, test) = parse_events(v.get("event"));
    Ok(Collection {
        pre_request_script: pre,
        post_response_script: test,
        name: info.get("name").map(s).unwrap_or_else(|| "Imported".into()),
        description: description_of(info.get("description")),
        items: parse_items(v.get("item")),
        variables: kv_list(v.get("variable")),
        auth: parse_auth(v.get("auth"), AuthKind::None),
        ..Default::default()
    })
}

pub fn import_postman_environment(json_text: &str) -> Result<Environment, String> {
    let v: Value = serde_json::from_str(json_text).map_err(|e| format!("Invalid JSON: {e}"))?;
    let values = v.get("values").ok_or("Not a Postman environment (missing `values`)")?;
    Ok(Environment {
        name: v.get("name").map(s).unwrap_or_else(|| "Imported".into()),
        variables: kv_list(Some(values)),
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Rustman -> Postman Collection v2.1
// ---------------------------------------------------------------------------

fn kv_to_json(list: &[KeyValue]) -> Value {
    Value::Array(
        list.iter()
            .map(|kv| {
                let mut o = json!({ "key": kv.key, "value": kv.value });
                if !kv.enabled {
                    o["disabled"] = json!(true);
                }
                if !kv.description.is_empty() {
                    o["description"] = json!(kv.description);
                }
                o
            })
            .collect(),
    )
}

fn auth_to_json(a: &Auth) -> Option<Value> {
    let p = |k: &str, v: &str| json!({ "key": k, "value": v, "type": "string" });
    match a.kind {
        AuthKind::Inherit => None,
        AuthKind::None => Some(json!({ "type": "noauth" })),
        AuthKind::Bearer => Some(json!({ "type": "bearer", "bearer": [p("token", &a.bearer_token)] })),
        AuthKind::Basic => Some(json!({
            "type": "basic",
            "basic": [p("username", &a.basic_username), p("password", &a.basic_password)]
        })),
        AuthKind::ApiKey => Some(json!({
            "type": "apikey",
            "apikey": [
                p("key", &a.api_key_name),
                p("value", &a.api_key_value),
                p("in", if a.api_key_in == ApiKeyLocation::Query { "query" } else { "header" })
            ]
        })),
        AuthKind::OAuth2 => Some(oauth2_to_json(&a.oauth2)),
    }
}

fn oauth2_to_json(o: &OAuth2) -> Value {
    let p = |k: &str, v: &str| json!({ "key": k, "value": v, "type": "string" });
    let grant = match o.grant_type {
        GrantType::AuthorizationCode => "authorization_code",
        GrantType::AuthorizationCodePkce => "authorization_code_with_pkce",
        GrantType::Implicit => "implicit",
        GrantType::Password => "password_credentials",
        GrantType::ClientCredentials => "client_credentials",
    };
    let mut params = vec![
        p("grant_type", grant),
        p("addTokenTo", if o.add_to == TokenPlacement::Query { "queryParams" } else { "header" }),
        p("headerPrefix", &o.header_prefix),
        p("client_authentication", if o.client_auth == ClientAuth::Body { "body" } else { "header" }),
    ];
    for (k, v) in [
        ("accessToken", &o.access_token),
        ("tokenName", &o.token_name),
        ("redirect_uri", &o.callback_url),
        ("authUrl", &o.auth_url),
        ("accessTokenUrl", &o.access_token_url),
        ("clientId", &o.client_id),
        ("clientSecret", &o.client_secret),
        ("scope", &o.scope),
        ("state", &o.state),
        ("username", &o.username),
        ("password", &o.password),
        ("code_verifier", &o.code_verifier),
        ("refreshTokenUrl", &o.refresh_token_url),
    ] {
        if !v.is_empty() {
            params.push(p(k, v));
        }
    }
    if o.grant_type == GrantType::AuthorizationCodePkce {
        params.push(p("challengeAlgorithm", if o.pkce_method == PkceMethod::Plain { "plain" } else { "S256" }));
    }
    let extra = |send_as: &str| -> Vec<Value> {
        [("audience", &o.audience), ("resource", &o.resource)]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| json!({ "key": k, "value": v, "enabled": true, "send_as": send_as }))
            .collect()
    };
    let (auth_params, token_params) = (extra("request_url"), extra("request_body"));
    if !token_params.is_empty() {
        params.push(json!({ "key": "authRequestParams", "value": auth_params, "type": "any" }));
        params.push(json!({ "key": "tokenRequestParams", "value": token_params, "type": "any" }));
    }
    json!({ "type": "oauth2", "oauth2": params })
}

fn body_to_json(b: &Body) -> Option<Value> {
    match b.mode {
        BodyMode::None => None,
        BodyMode::Raw => Some(json!({
            "mode": "raw",
            "raw": b.raw,
            "options": { "raw": { "language": b.raw_lang.label().to_ascii_lowercase() } }
        })),
        BodyMode::UrlEncoded => Some(json!({ "mode": "urlencoded", "urlencoded": kv_to_json(&b.urlencoded) })),
        BodyMode::FormData => Some(json!({
            "mode": "formdata",
            "formdata": b.form_data.iter().map(|f| {
                let mut o = if f.kind == FormFieldKind::File {
                    json!({ "key": f.key, "type": "file", "src": f.value })
                } else {
                    json!({ "key": f.key, "type": "text", "value": f.value })
                };
                if !f.enabled { o["disabled"] = json!(true); }
                o
            }).collect::<Vec<_>>()
        })),
        BodyMode::Binary => Some(json!({ "mode": "file", "file": { "src": b.binary_path } })),
        BodyMode::GraphQl => Some(json!({
            "mode": "graphql",
            "graphql": { "query": b.graphql_query, "variables": b.graphql_variables }
        })),
    }
}

fn request_to_json(r: &Request) -> Value {
    let mut req = json!({
        "method": r.method.as_str(),
        "header": kv_to_json(&r.headers),
        "url": { "raw": r.url, "query": kv_to_json(&r.params) },
    });
    if let Some(a) = auth_to_json(&r.auth) {
        req["auth"] = a;
    }
    if let Some(b) = body_to_json(&r.body) {
        req["body"] = b;
    }
    if !r.description.is_empty() {
        req["description"] = json!(r.description);
    }
    req
}

/// Postman `event` array for a pre-request and a test script (`None` when both are empty).
fn events_to_json(pre: &str, test: &str) -> Option<Value> {
    let events: Vec<Value> = [("prerequest", pre), ("test", test)]
        .into_iter()
        .filter(|(_, code)| !code.trim().is_empty())
        .map(|(listen, code)| json!({ "listen": listen, "script": { "type": "text/javascript", "exec": code.split('\n').collect::<Vec<_>>() } }))
        .collect();
    (!events.is_empty()).then(|| Value::Array(events))
}

fn items_to_json(items: &[Item]) -> Value {
    Value::Array(
        items
            .iter()
            .map(|i| match i {
                Item::Folder(f) => json!({ "name": f.name, "item": items_to_json(&f.items) }),
                Item::Request(r) => {
                    let mut v = json!({ "name": r.name, "request": request_to_json(r), "response": [] });
                    if let Some(ev) = events_to_json(&r.pre_request_script, &r.post_response_script) {
                        v["event"] = ev;
                    }
                    v
                }
            })
            .collect(),
    )
}

pub fn export_postman_collection(c: &Collection) -> String {
    let mut v = json!({
        "info": {
            "_postman_id": c.id.to_string(),
            "name": c.name,
            "description": c.description,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
        },
        "item": items_to_json(&c.items),
        "variable": kv_to_json(&c.variables),
    });
    if let Some(a) = auth_to_json(&c.auth) {
        v["auth"] = a;
    }
    if let Some(ev) = events_to_json(&c.pre_request_script, &c.post_response_script) {
        v["event"] = ev;
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

pub fn export_postman_environment(e: &Environment) -> String {
    let v = json!({
        "id": e.id.to_string(),
        "name": e.name,
        "values": e.variables.iter().map(|kv| json!({
            "key": kv.key, "value": kv.value, "type": "default", "enabled": kv.enabled
        })).collect::<Vec<_>>(),
        "_postman_variable_scope": "environment"
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// cURL -> Request
// ---------------------------------------------------------------------------

pub fn parse_curl(cmd: &str) -> Result<Request, String> {
    // Join line continuations (bash `\`, Windows cmd `^`, PowerShell backtick).
    let joined = cmd
        .replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .replace("^\r\n", " ")
        .replace("^\n", " ")
        .replace("`\r\n", " ")
        .replace("`\n", " ");
    let tokens = shell_words::split(joined.trim()).map_err(|e| format!("Cannot parse command: {e}"))?;
    let mut it = tokens.into_iter().peekable();
    match it.peek() {
        Some(t) if t == "curl" || t.ends_with("/curl") || t.ends_with("curl.exe") => {
            it.next();
        }
        _ => return Err("Command must start with `curl`".into()),
    }

    let mut req = Request { name: "Imported from cURL".into(), ..Default::default() };
    let mut explicit_method: Option<Method> = None;
    let mut data_parts: Vec<String> = Vec::new();
    let mut urlencoded_parts: Vec<String> = Vec::new();
    let mut form_parts: Vec<FormField> = Vec::new();
    let mut force_get = false;
    let mut url: Option<String> = None;

    while let Some(tok) = it.next() {
        // Support `--flag=value`.
        let (flag, inline_val) = match tok.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (tok.clone(), None),
        };
        // Support glued short options like `-XPOST` or `-HAccept:x`.
        let (flag, inline_val) = if inline_val.is_none()
            && flag.len() > 2
            && flag.starts_with('-')
            && !flag.starts_with("--")
            && matches!(&flag[..2], "-X" | "-H" | "-d" | "-u" | "-F" | "-A" | "-b" | "-e")
        {
            (flag[..2].to_string(), Some(flag[2..].to_string()))
        } else {
            (flag, inline_val)
        };
        let mut val = || inline_val.clone().or_else(|| it.next()).unwrap_or_default();

        match flag.as_str() {
            "-X" | "--request" => explicit_method = Method::parse(&val()),
            "-H" | "--header" => {
                let h = val();
                if let Some((k, v)) = h.split_once(':') {
                    req.headers.push(KeyValue::new(k.trim(), v.trim()));
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" | "--json" => {
                let d = val();
                if flag == "--json" {
                    req.headers.push(KeyValue::new("Content-Type", "application/json"));
                    req.headers.push(KeyValue::new("Accept", "application/json"));
                }
                if let Some(path) = d.strip_prefix('@') {
                    req.body.mode = BodyMode::Binary;
                    req.body.binary_path = path.to_string();
                } else {
                    data_parts.push(d);
                }
            }
            "--data-urlencode" => urlencoded_parts.push(val()),
            "-F" | "--form" | "--form-string" => {
                let f = val();
                let (k, v) = f.split_once('=').unwrap_or((&f, ""));
                let is_file = flag != "--form-string" && v.starts_with('@');
                let v = if is_file { v[1..].split(';').next().unwrap_or("").to_string() } else { v.to_string() };
                form_parts.push(FormField {
                    key: k.to_string(),
                    value: v,
                    kind: if is_file { FormFieldKind::File } else { FormFieldKind::Text },
                    ..Default::default()
                });
            }
            "-u" | "--user" => {
                let u = val();
                let (user, pass) = u.split_once(':').unwrap_or((&u, ""));
                req.auth = Auth {
                    kind: AuthKind::Basic,
                    basic_username: user.into(),
                    basic_password: pass.into(),
                    ..Default::default()
                };
            }
            "-A" | "--user-agent" => req.headers.push(KeyValue::new("User-Agent", val())),
            "-b" | "--cookie" => req.headers.push(KeyValue::new("Cookie", val())),
            "-e" | "--referer" => req.headers.push(KeyValue::new("Referer", val())),
            "-G" | "--get" => force_get = true,
            "-I" | "--head" => explicit_method = Some(Method::Head),
            "-k" | "--insecure" => req.settings.verify_ssl = false,
            "-L" | "--location" => req.settings.follow_redirects = true,
            "-m" | "--max-time" => {
                if let Ok(secs) = val().parse::<f64>() {
                    req.settings.timeout_ms = (secs * 1000.0) as u64;
                }
            }
            "--url" => url = Some(val()),
            // Options that take an argument we don't use.
            "-o" | "--output" | "-w" | "--write-out" | "--connect-timeout" | "-x" | "--proxy" | "--cacert"
            | "--cert" | "--key" | "-c" | "--cookie-jar" | "--retry" => {
                val();
            }
            f if f.starts_with('-') => {} // flags without args: -s, -v, --compressed, ...
            _ => {
                if url.is_none() {
                    url = Some(tok);
                }
            }
        }
    }

    req.url = url.ok_or("No URL found in the cURL command")?;
    let has_data = !data_parts.is_empty() || !urlencoded_parts.is_empty();

    if force_get && has_data {
        // -G moves data into the query string.
        let mut all = data_parts.clone();
        all.extend(urlencoded_parts.iter().cloned());
        let sep = if req.url.contains('?') { '&' } else { '?' };
        req.url = format!("{}{sep}{}", req.url, all.join("&"));
    } else if !form_parts.is_empty() {
        req.body.mode = BodyMode::FormData;
        req.body.form_data = form_parts;
    } else if has_data {
        let ct = req
            .headers
            .iter()
            .find(|h| h.key.eq_ignore_ascii_case("content-type"))
            .map(|h| h.value.to_ascii_lowercase())
            .unwrap_or_default();
        let joined = data_parts.join("&");
        let looks_json = joined.trim_start().starts_with(['{', '[']);
        if urlencoded_parts.is_empty() && (ct.contains("json") || looks_json) {
            req.body.mode = BodyMode::Raw;
            req.body.raw_lang = RawLang::Json;
            req.body.raw = pretty_json_if_possible(&joined);
        } else if urlencoded_parts.is_empty() && !ct.is_empty() && !ct.contains("x-www-form-urlencoded") {
            req.body.mode = BodyMode::Raw;
            req.body.raw_lang = if ct.contains("xml") { RawLang::Xml } else { RawLang::Text };
            req.body.raw = joined;
        } else {
            req.body.mode = BodyMode::UrlEncoded;
            let mut fields: Vec<KeyValue> = Vec::new();
            for part in data_parts.iter().flat_map(|d| d.split('&')) {
                let (k, v) = part.split_once('=').unwrap_or((part, ""));
                fields.push(KeyValue::new(decode_form(k), decode_form(v)));
            }
            for part in &urlencoded_parts {
                let (k, v) = part.split_once('=').unwrap_or((part, ""));
                fields.push(KeyValue::new(k, v));
            }
            req.body.urlencoded = fields;
        }
    }

    req.method = explicit_method.unwrap_or(if force_get {
        Method::Get
    } else if has_data || req.body.mode != BodyMode::None {
        Method::Post
    } else {
        Method::Get
    });
    // Drop Content-Type that conflicts with an auto-managed multipart boundary.
    if req.body.mode == BodyMode::FormData {
        req.headers.retain(|h| !h.key.eq_ignore_ascii_case("content-type"));
    }
    req.sync_params_from_url();
    Ok(req)
}

fn decode_form(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={s}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_string())
}

pub fn pretty_json_if_possible(s: &str) -> String {
    serde_json::from_str::<Value>(s)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postman_oauth2_round_trip() {
        let src = json!({
            "type": "oauth2",
            "oauth2": [
                { "key": "grant_type", "value": "authorization_code_with_pkce" },
                { "key": "addTokenTo", "value": "queryParams" },
                { "key": "accessToken", "value": "tok" },
                { "key": "authUrl", "value": "https://id.x/authorize" },
                { "key": "accessTokenUrl", "value": "https://id.x/token" },
                { "key": "clientId", "value": "{{cid}}" },
                { "key": "scope", "value": "openid" },
                { "key": "redirect_uri", "value": "http://localhost:8080/cb" },
                { "key": "challengeAlgorithm", "value": "plain" },
                { "key": "client_authentication", "value": "body" },
                { "key": "tokenRequestParams", "value": [{ "key": "audience", "value": "api", "enabled": true, "send_as": "request_body" }] }
            ]
        });
        let a = parse_auth(Some(&src), AuthKind::Inherit);
        assert_eq!(a.kind, AuthKind::OAuth2);
        let o = &a.oauth2;
        assert_eq!(o.grant_type, GrantType::AuthorizationCodePkce);
        assert_eq!(o.add_to, TokenPlacement::Query);
        assert_eq!((o.access_token.as_str(), o.client_id.as_str(), o.callback_url.as_str()), ("tok", "{{cid}}", "http://localhost:8080/cb"));
        assert_eq!((o.pkce_method, o.client_auth, o.audience.as_str()), (PkceMethod::Plain, ClientAuth::Body, "api"));
        assert_eq!(o.header_prefix, "Bearer");
        let back = parse_auth(auth_to_json(&a).as_ref(), AuthKind::Inherit);
        assert_eq!(back.oauth2, a.oauth2);
    }

    #[test]
    fn curl_json_post() {
        let r = parse_curl(
            r#"curl 'https://api.x.io/users?page=2' \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer abc" \
  --data-raw '{"name":"a"}' --compressed"#,
        )
        .unwrap();
        assert_eq!(r.method, Method::Post);
        assert_eq!(r.url, "https://api.x.io/users?page=2");
        assert_eq!(r.params.len(), 1);
        assert_eq!(r.headers.len(), 2);
        assert_eq!(r.body.mode, BodyMode::Raw);
        assert!(r.body.raw.contains("\"name\": \"a\""));
    }

    #[test]
    fn curl_form_and_user() {
        let r = parse_curl("curl -XPUT -u admin:secret -F name=x -F file=@/tmp/a.png http://h/up").unwrap();
        assert_eq!(r.method, Method::Put);
        assert_eq!(r.auth.kind, AuthKind::Basic);
        assert_eq!(r.body.form_data.len(), 2);
        assert_eq!(r.body.form_data[1].kind, FormFieldKind::File);
        assert_eq!(r.body.form_data[1].value, "/tmp/a.png");
    }

    #[test]
    fn curl_urlencoded_and_get() {
        let r = parse_curl("curl -d 'a=1&b=hello+world' http://h").unwrap();
        assert_eq!(r.body.mode, BodyMode::UrlEncoded);
        assert_eq!(r.body.urlencoded[1].value, "hello world");
        let g = parse_curl("curl -G -d q=rust http://h/search").unwrap();
        assert_eq!(g.method, Method::Get);
        assert_eq!(g.url, "http://h/search?q=rust");
    }

    #[test]
    fn postman_roundtrip() {
        let src = r#"{
          "info": {"name": "Demo", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "auth": {"type": "bearer", "bearer": [{"key": "token", "value": "{{token}}"}]},
          "variable": [{"key": "base", "value": "https://api.io"}],
          "item": [
            {"name": "Users", "item": [
              {"name": "List", "request": {"method": "GET", "header": [],
                "url": {"raw": "{{base}}/users?page=1", "query": [{"key":"page","value":"1"},{"key":"limit","value":"5","disabled":true}]}}},
              {"name": "Create", "request": {"method": "POST",
                "header": [{"key": "X-A", "value": "1", "disabled": true}],
                "body": {"mode": "raw", "raw": "{\"a\":1}", "options": {"raw": {"language": "json"}}},
                "url": "{{base}}/users"}}
            ]}
          ]
        }"#;
        let c = import_postman_collection(src).unwrap();
        assert_eq!(c.name, "Demo");
        assert_eq!(c.auth.kind, AuthKind::Bearer);
        let Item::Folder(f) = &c.items[0] else { panic!() };
        let Item::Request(list) = &f.items[0] else { panic!() };
        assert_eq!(list.params.len(), 2);
        assert!(!list.params[1].enabled);
        assert_eq!(list.url, "{{base}}/users?page=1");
        assert_eq!(list.auth.kind, AuthKind::Inherit);
        let Item::Request(create) = &f.items[1] else { panic!() };
        assert_eq!(create.method, Method::Post);
        assert!(!create.headers[0].enabled);
        assert_eq!(create.body.raw_lang, RawLang::Json);

        let exported = export_postman_collection(&c);
        let again = import_postman_collection(&exported).unwrap();
        assert_eq!(again.items.len(), 1);
        assert_eq!(again.variables, c.variables);
    }

    #[test]
    fn postman_scripts_roundtrip() {
        let src = r#"{
          "info": {"name": "S"},
          "event": [{"listen": "prerequest", "script": {"type": "text/javascript", "exec": ["console.log('coll');"]}}],
          "item": [
            {"name": "F", "event": [{"listen": "test", "script": {"exec": ["pm.test('folder', () => {});"]}}], "item": [
              {"name": "R", "request": {"method": "GET", "url": "http://x"},
               "event": [
                 {"listen": "test", "script": {"exec": ["pm.test('a', () => {});", "pm.test('b', () => {});"]}},
                 {"listen": "prerequest", "script": {"exec": "pm.variables.set('x', 1);", "disabled": true}}
               ]}
            ]}
          ]
        }"#;
        let c = import_postman_collection(src).unwrap();
        assert_eq!(c.pre_request_script, "console.log('coll');");
        let Item::Folder(f) = &c.items[0] else { panic!() };
        let Item::Request(r) = &f.items[0] else { panic!() };
        assert_eq!(r.pre_request_script, "", "disabled scripts are dropped");
        assert_eq!(r.post_response_script, "pm.test('folder', () => {});\n\npm.test('a', () => {});\npm.test('b', () => {});");

        let again = import_postman_collection(&export_postman_collection(&c)).unwrap();
        assert_eq!(again.pre_request_script, c.pre_request_script);
        let Item::Folder(f2) = &again.items[0] else { panic!() };
        let Item::Request(r2) = &f2.items[0] else { panic!() };
        assert_eq!((&r2.pre_request_script, &r2.post_response_script), (&r.pre_request_script, &r.post_response_script));
    }
}
