//! OpenAPI 3 / Swagger 2 (YAML or JSON): outline for the spec editor and conversion to a collection.

use serde_json::{Map, Value, json};

use crate::model::{Body, BodyMode, Collection, FormField, Folder, Item, KeyValue, Method, RawLang, Request};

const METHODS: [(&str, Method); 7] = [
    ("get", Method::Get),
    ("post", Method::Post),
    ("put", Method::Put),
    ("patch", Method::Patch),
    ("delete", Method::Delete),
    ("head", Method::Head),
    ("options", Method::Options),
];

/// Parses YAML or JSON into a JSON value (YAML is a superset of JSON).
pub fn parse(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Err("The spec is empty".into());
    }
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => serde_norway::from_str(text).map_err(|e| format!("Invalid YAML/JSON: {e}"))?,
    };
    if !v.is_object() {
        return Err("The spec must be an object".into());
    }
    Ok(v)
}

/// Is this text an OpenAPI / Swagger document? (used by Import)
pub fn looks_like_openapi(text: &str) -> bool {
    let head: String = text.chars().take(2000).collect();
    if !(head.contains("openapi") || head.contains("swagger")) {
        return false;
    }
    parse(text).is_ok_and(|v| v.get("openapi").is_some() || v.get("swagger").is_some())
}

#[derive(Debug)]
pub struct Endpoint {
    pub method: Method,
    pub path: String,
    pub summary: String,
    pub tag: Option<String>,
}

#[derive(Debug)]
pub struct Outline {
    pub title: String,
    pub version: String,
    pub spec_version: String,
    pub base_url: String,
    pub endpoints: Vec<Endpoint>,
    pub warnings: Vec<String>,
}

pub fn outline(text: &str) -> Result<Outline, String> {
    let v = parse(text)?;
    let spec_version = v
        .get("openapi")
        .or_else(|| v.get("swagger"))
        .map(value_to_string)
        .ok_or("Missing `openapi` (or `swagger`) version field")?;
    let info = v.get("info");
    let mut warnings = Vec::new();
    if info.is_none() {
        warnings.push("Missing `info` object".to_string());
    }
    let mut endpoints = Vec::new();
    match v.get("paths").and_then(Value::as_object) {
        Some(paths) => {
            for (path, item) in paths {
                for (key, method) in METHODS {
                    if let Some(op) = item.get(key) {
                        endpoints.push(Endpoint {
                            method,
                            path: path.clone(),
                            summary: op_name(op),
                            tag: op.get("tags").and_then(|t| t.get(0)).and_then(Value::as_str).map(str::to_string),
                        });
                    }
                }
            }
        }
        None => warnings.push("Missing `paths` object".to_string()),
    }
    Ok(Outline {
        title: info.and_then(|i| i.get("title")).map(value_to_string).unwrap_or_else(|| "Untitled API".into()),
        version: info.and_then(|i| i.get("version")).map(value_to_string).unwrap_or_default(),
        spec_version,
        base_url: base_url(&v),
        endpoints,
        warnings,
    })
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn op_name(op: &Value) -> String {
    op.get("summary").or_else(|| op.get("operationId")).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

fn base_url(v: &Value) -> String {
    if let Some(server) = v.get("servers").and_then(|s| s.get(0)) {
        let mut url = server.get("url").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(vars) = server.get("variables").and_then(Value::as_object) {
            for (name, def) in vars {
                let d = def.get("default").map(value_to_string).unwrap_or_default();
                url = url.replace(&format!("{{{name}}}"), &d);
            }
        }
        return url.trim_end_matches('/').to_string();
    }
    if let Some(host) = v.get("host").and_then(Value::as_str) {
        let scheme = v.get("schemes").and_then(|s| s.get(0)).and_then(Value::as_str).unwrap_or("https");
        let base = v.get("basePath").and_then(Value::as_str).unwrap_or("");
        return format!("{scheme}://{host}{}", base.trim_end_matches('/'));
    }
    String::new()
}

/// Follows a local `$ref` (`#/components/schemas/User`).
fn resolve<'a>(root: &'a Value, v: &'a Value) -> &'a Value {
    let mut cur = v;
    for _ in 0..10 {
        let Some(r) = cur.get("$ref").and_then(Value::as_str) else { break };
        let Some(ptr) = r.strip_prefix('#') else { break };
        match root.pointer(ptr) {
            Some(t) => cur = t,
            None => break,
        }
    }
    cur
}

/// Example value for a schema: explicit example/default/enum first, then generated from the type.
pub fn example(root: &Value, schema: &Value, depth: usize) -> Value {
    let s = resolve(root, schema);
    if let Some(e) = s.get("example").or_else(|| s.get("default")) {
        return e.clone();
    }
    if let Some(e) = s.get("enum").and_then(|e| e.get(0)) {
        return e.clone();
    }
    if depth > 6 {
        return Value::Null;
    }
    for key in ["allOf", "oneOf", "anyOf"] {
        if let Some(list) = s.get(key).and_then(Value::as_array) {
            if key == "allOf" {
                let mut merged = Map::new();
                for part in list {
                    if let Value::Object(o) = example(root, part, depth + 1) {
                        merged.extend(o);
                    }
                }
                return Value::Object(merged);
            }
            if let Some(first) = list.first() {
                return example(root, first, depth + 1);
            }
        }
    }
    let ty = s.get("type").and_then(|t| t.as_str().or_else(|| t.get(0).and_then(Value::as_str)));
    match ty {
        Some("object") | None if s.get("properties").is_some() => {
            let mut out = Map::new();
            if let Some(props) = s.get("properties").and_then(Value::as_object) {
                for (k, p) in props {
                    out.insert(k.clone(), example(root, p, depth + 1));
                }
            }
            Value::Object(out)
        }
        Some("object") => json!({}),
        Some("array") => match s.get("items") {
            Some(items) => json!([example(root, items, depth + 1)]),
            None => json!([]),
        },
        Some("integer") => json!(0),
        Some("number") => json!(0.0),
        Some("boolean") => json!(true),
        Some("string") => json!(match s.get("format").and_then(Value::as_str) {
            Some("email") => "user@example.com",
            Some("date-time") => "2024-01-01T00:00:00Z",
            Some("date") => "2024-01-01",
            Some("uuid") => "00000000-0000-0000-0000-000000000000",
            Some("uri") | Some("url") => "https://example.com",
            _ => "string",
        }),
        _ => Value::Null,
    }
}

fn param_example(root: &Value, p: &Value) -> String {
    if let Some(e) = p.get("example") {
        return value_to_string(e);
    }
    // Swagger 2 keeps type/default on the parameter itself.
    let schema = p.get("schema").unwrap_or(p);
    match example(root, schema, 0) {
        Value::Null => String::new(),
        v => value_to_string(&v),
    }
}

/// Builds a collection: one folder per first tag, `{{baseUrl}}` collection variable,
/// path params as `{{name}}` variables, query/header params, and an example body.
pub fn to_collection(text: &str) -> Result<Collection, String> {
    let root = parse(text)?;
    let ol = outline(text)?;
    let mut coll = Collection { name: ol.title.clone(), ..Default::default() };
    coll.description = root.get("info").and_then(|i| i.get("description")).and_then(Value::as_str).unwrap_or("").to_string();
    coll.variables.push(KeyValue::new("baseUrl", ol.base_url.clone()));

    let paths = root.get("paths").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut folders: Vec<Folder> = Vec::new();
    for (path, item) in &paths {
        let item = resolve(&root, item);
        let shared_params: Vec<Value> = item.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default();
        for (key, method) in METHODS {
            let Some(op) = item.get(key) else { continue };
            let mut req = Request { method, ..Default::default() };
            let name = op_name(op);
            req.name = if name.is_empty() { format!("{} {path}", method.as_str()) } else { name };
            req.description = op.get("description").and_then(Value::as_str).unwrap_or("").to_string();

            let mut url_path = path.clone();
            let mut params: Vec<Value> = shared_params.clone();
            params.extend(op.get("parameters").and_then(Value::as_array).cloned().unwrap_or_default());
            let mut body_schema: Option<(String, Value)> = None;
            let mut form_params: Vec<(String, String, bool)> = Vec::new();
            for p in &params {
                let p = resolve(&root, p);
                let pname = p.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let ex = param_example(&root, p);
                let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
                match p.get("in").and_then(Value::as_str) {
                    Some("path") => {
                        url_path = url_path.replace(&format!("{{{pname}}}"), &format!("{{{{{pname}}}}}"));
                        if !coll.variables.iter().any(|v| v.key == pname) {
                            coll.variables.push(KeyValue::new(pname, ex));
                        }
                    }
                    Some("query") => req.params.push(KeyValue { enabled: required || !ex.is_empty(), ..KeyValue::new(pname, ex) }),
                    Some("header") => req.headers.push(KeyValue::new(pname, ex)),
                    Some("body") => body_schema = p.get("schema").map(|s| ("application/json".to_string(), s.clone())),
                    Some("formData") => form_params.push((pname, ex, p.get("type").and_then(Value::as_str) == Some("file"))),
                    _ => {}
                }
            }
            if let Some(content) = op.get("requestBody").map(|b| resolve(&root, b)).and_then(|b| b.get("content")).and_then(Value::as_object) {
                let pick = ["application/json", "application/x-www-form-urlencoded", "multipart/form-data"]
                    .iter()
                    .find_map(|ct| content.get(*ct).map(|c| (ct.to_string(), c)))
                    .or_else(|| content.iter().next().map(|(k, c)| (k.clone(), c)));
                if let Some((ct, media)) = pick {
                    let schema = media.get("schema").cloned().unwrap_or(Value::Null);
                    match media.get("example").cloned() {
                        Some(ex) => body_schema = Some((ct, json!({ "example": ex }))),
                        None => body_schema = Some((ct, schema)),
                    }
                }
            }
            req.url = format!("{{{{baseUrl}}}}{url_path}");
            req.body = make_body(&root, body_schema, form_params);
            req.sync_url_from_params();

            let tag = op.get("tags").and_then(|t| t.get(0)).and_then(Value::as_str).map(str::to_string);
            match tag {
                Some(t) => match folders.iter_mut().find(|f| f.name == t) {
                    Some(f) => f.items.push(Item::Request(req)),
                    None => {
                        let mut f = Folder::new(t);
                        f.items.push(Item::Request(req));
                        folders.push(f);
                    }
                },
                None => coll.items.push(Item::Request(req)),
            }
        }
    }
    let loose = std::mem::take(&mut coll.items);
    coll.items = folders.into_iter().map(Item::Folder).chain(loose).collect();
    Ok(coll)
}

fn make_body(root: &Value, schema: Option<(String, Value)>, form: Vec<(String, String, bool)>) -> Body {
    let mut body = Body::default();
    if !form.is_empty() {
        body.mode = BodyMode::FormData;
        body.form_data = form
            .into_iter()
            .map(|(k, v, file)| FormField { key: k, value: if file { String::new() } else { v }, kind: if file { crate::model::FormFieldKind::File } else { Default::default() }, ..Default::default() })
            .collect();
        return body;
    }
    let Some((ct, schema)) = schema else { return body };
    let ex = example(root, &schema, 0);
    if ct.contains("x-www-form-urlencoded") || ct.contains("multipart") {
        let fields: Vec<(String, String)> = match &ex {
            Value::Object(o) => o.iter().map(|(k, v)| (k.clone(), if v.is_null() { String::new() } else { value_to_string(v) })).collect(),
            _ => Vec::new(),
        };
        if ct.contains("multipart") {
            body.mode = BodyMode::FormData;
            body.form_data = fields.into_iter().map(|(k, v)| FormField { key: k, value: v, ..Default::default() }).collect();
        } else {
            body.mode = BodyMode::UrlEncoded;
            body.urlencoded = fields.into_iter().map(|(k, v)| KeyValue::new(k, v)).collect();
        }
        return body;
    }
    body.mode = BodyMode::Raw;
    if ct.contains("json") || ex.is_object() || ex.is_array() {
        body.raw_lang = RawLang::Json;
        body.raw = pretty4(&ex);
    } else {
        body.raw_lang = if ct.contains("xml") { RawLang::Xml } else { RawLang::Text };
        body.raw = if ex.is_null() { String::new() } else { value_to_string(&ex) };
    }
    body
}

/// JSON with 4-space indentation (like request bodies typed in Postman).
fn pretty4(v: &Value) -> String {
    use serde::Serialize;
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, serde_json::ser::PrettyFormatter::with_indent(b"    "));
    match v.serialize(&mut ser) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SPEC_TEMPLATE;

    #[test]
    fn template_outline_and_collection() {
        let ol = outline(SPEC_TEMPLATE).unwrap();
        assert_eq!(ol.title, "My API");
        assert_eq!(ol.base_url, "http://localhost:8080");
        assert_eq!(ol.endpoints.len(), 3);

        let c = to_collection(SPEC_TEMPLATE).unwrap();
        assert_eq!(c.variables[0], KeyValue::new("baseUrl", "http://localhost:8080"));
        assert!(c.variables.iter().any(|v| v.key == "id" && v.value == "1"));
        let Item::Folder(f) = &c.items[0] else { panic!("expected folder") };
        assert_eq!(f.name, "users");
        let reqs: Vec<&Request> = f.items.iter().filter_map(|i| if let Item::Request(r) = i { Some(r) } else { None }).collect();
        assert_eq!(reqs[0].url, "{{baseUrl}}/users?page=1");
        assert_eq!(reqs[1].body.mode, BodyMode::Raw);
        let body: Value = serde_json::from_str(&reqs[1].body.raw).unwrap();
        assert_eq!(body, json!({"name": "Nham", "email": "user@example.com"}));
        assert_eq!(reqs[2].url, "{{baseUrl}}/users/{{id}}");
    }

    #[test]
    fn swagger2_and_refs() {
        let spec = r##"{
          "swagger": "2.0", "info": {"title": "Pet", "version": "1"},
          "host": "api.pet.io", "basePath": "/v1", "schemes": ["https"],
          "definitions": {"Pet": {"type": "object", "properties": {"name": {"type": "string", "example": "Rex"}, "tags": {"type": "array", "items": {"type": "string"}}}}},
          "paths": {"/pets": {"post": {"operationId": "addPet", "parameters": [{"in": "body", "name": "body", "schema": {"$ref": "#/definitions/Pet"}}]}}}
        }"##;
        assert!(looks_like_openapi(spec));
        let c = to_collection(spec).unwrap();
        assert_eq!(c.variables[0].value, "https://api.pet.io/v1");
        let Item::Request(r) = &c.items[0] else { panic!() };
        assert_eq!(r.name, "addPet");
        let body: Value = serde_json::from_str(&r.body.raw).unwrap();
        assert_eq!(body, json!({"name": "Rex", "tags": ["string"]}));
    }

    #[test]
    fn errors_are_reported() {
        assert!(outline("").is_err());
        assert!(outline("just: [unclosed").is_err());
        assert!(outline("foo: bar").unwrap_err().contains("openapi"));
        assert!(!looks_like_openapi(r#"{"info":{"name":"x"}}"#));
    }
}
