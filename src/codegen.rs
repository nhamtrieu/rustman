//! Code snippet generation for many languages / HTTP clients (Postman's "Code snippet" panel).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::http::{self, PreparedBody, encode_form};
use crate::model::{Auth, BodyMode, Request};
use crate::vars::{VarMap, substitute};

// ---------------------------------------------------------------------------
// Request model used by the generators (variables resolved, auth applied)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub value: String,
    pub is_file: bool,
}

#[derive(Clone, Debug)]
pub enum SBody {
    None,
    Raw(String),
    UrlEncoded(Vec<(String, String)>),
    FormData(Vec<Part>),
    File(String),
}

#[derive(Clone, Debug)]
pub struct Snippet {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: SBody,
}

impl Snippet {
    pub fn content_type(&self) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str())
    }
    /// Headers except Content-Type (for clients that set it on the body object).
    pub fn headers_no_ct(&self) -> Vec<&(String, String)> {
        self.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("content-type")).collect()
    }
    fn has_body(&self) -> bool {
        !matches!(self.body, SBody::None)
    }
    fn urlencoded_string(&self) -> String {
        match &self.body {
            SBody::UrlEncoded(pairs) => pairs.iter().map(|(k, v)| format!("{}={}", encode_form(k), encode_form(v))).collect::<Vec<_>>().join("&"),
            _ => String::new(),
        }
    }
    /// Body as a single string when it is textual (raw or urlencoded).
    fn text_body(&self) -> Option<String> {
        match &self.body {
            SBody::Raw(s) => Some(s.clone()),
            SBody::UrlEncoded(_) => Some(self.urlencoded_string()),
            _ => None,
        }
    }
}

/// Build the snippet model from a request: variables substituted, auth headers/params applied.
pub fn build(req: &Request, vars: &VarMap, inherited_auth: Option<&Auth>) -> Result<Snippet, String> {
    let p = http::prepare(req, vars, inherited_auth)?;
    let headers = p
        .headers
        .into_iter()
        .filter(|(k, v)| !(k.eq_ignore_ascii_case("user-agent") && v.starts_with("Rustman/") || k.eq_ignore_ascii_case("accept") && v == "*/*"))
        .collect();
    let sub = |s: &str| substitute(s, vars);
    let body = match p.body {
        PreparedBody::None => SBody::None,
        PreparedBody::Bytes(b) if req.body.mode == BodyMode::UrlEncoded => {
            let _ = b;
            SBody::UrlEncoded(req.body.urlencoded.iter().filter(|kv| kv.enabled && !kv.key.is_empty()).map(|kv| (sub(&kv.key), sub(&kv.value))).collect())
        }
        PreparedBody::Bytes(b) => SBody::Raw(String::from_utf8_lossy(&b).into_owned()),
        PreparedBody::File(path) => SBody::File(path),
        PreparedBody::Multipart(parts) => SBody::FormData(parts.into_iter().map(|p| Part { name: p.name, value: p.value, is_file: p.is_file }).collect()),
    };
    Ok(Snippet { method: req.method.as_str(), url: p.url, headers, body })
}

// ---------------------------------------------------------------------------
// Languages
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Lang {
    CLibcurl,
    CsharpHttpClient,
    CsharpRestSharp,
    #[default]
    Curl,
    DartDio,
    DartHttp,
    GoNative,
    Http,
    JavaOkHttp,
    JavaUnirest,
    JsFetch,
    JsJquery,
    JsXhr,
    KotlinOkhttp,
    NodeAxios,
    NodeNative,
    NodeRequest,
    NodeUnirest,
    ObjcNsUrlSession,
    OcamlCohttp,
    PhpCurl,
    PhpGuzzle,
    PhpHttpRequest2,
    PhpPeclHttp,
    PowershellRestMethod,
    PythonHttpClient,
    PythonRequests,
    RHttr,
    RRcurl,
    RubyNetHttp,
    RustReqwest,
    ShellHttpie,
    ShellWget,
    SwiftUrlSession,
}

/// Token rules used by the highlighter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Shell,
    Http,
    C,
    CSharp,
    Dart,
    Go,
    Java,
    Js,
    Kotlin,
    ObjC,
    OCaml,
    Php,
    PowerShell,
    Python,
    R,
    Ruby,
    Rust,
    Swift,
}

impl Lang {
    pub const ALL: [Lang; 34] = [
        Lang::CLibcurl,
        Lang::CsharpHttpClient,
        Lang::CsharpRestSharp,
        Lang::Curl,
        Lang::DartDio,
        Lang::DartHttp,
        Lang::GoNative,
        Lang::Http,
        Lang::JavaOkHttp,
        Lang::JavaUnirest,
        Lang::JsFetch,
        Lang::JsJquery,
        Lang::JsXhr,
        Lang::KotlinOkhttp,
        Lang::NodeAxios,
        Lang::NodeNative,
        Lang::NodeRequest,
        Lang::NodeUnirest,
        Lang::ObjcNsUrlSession,
        Lang::OcamlCohttp,
        Lang::PhpCurl,
        Lang::PhpGuzzle,
        Lang::PhpHttpRequest2,
        Lang::PhpPeclHttp,
        Lang::PowershellRestMethod,
        Lang::PythonHttpClient,
        Lang::PythonRequests,
        Lang::RHttr,
        Lang::RRcurl,
        Lang::RubyNetHttp,
        Lang::RustReqwest,
        Lang::ShellHttpie,
        Lang::ShellWget,
        Lang::SwiftUrlSession,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Lang::CLibcurl => "C - libcurl",
            Lang::CsharpHttpClient => "C# - HttpClient",
            Lang::CsharpRestSharp => "C# - RestSharp",
            Lang::Curl => "cURL",
            Lang::DartDio => "Dart - dio",
            Lang::DartHttp => "Dart - http",
            Lang::GoNative => "Go - Native",
            Lang::Http => "HTTP",
            Lang::JavaOkHttp => "Java - OkHttp",
            Lang::JavaUnirest => "Java - Unirest",
            Lang::JsFetch => "JavaScript - Fetch",
            Lang::JsJquery => "JavaScript - jQuery",
            Lang::JsXhr => "JavaScript - XHR",
            Lang::KotlinOkhttp => "Kotlin - Okhttp",
            Lang::NodeAxios => "NodeJs - Axios",
            Lang::NodeNative => "NodeJs - Native",
            Lang::NodeRequest => "NodeJs - Request",
            Lang::NodeUnirest => "NodeJs - Unirest",
            Lang::ObjcNsUrlSession => "Objective-C - NSURLSession",
            Lang::OcamlCohttp => "OCaml - Cohttp",
            Lang::PhpCurl => "PHP - cURL",
            Lang::PhpGuzzle => "PHP - Guzzle",
            Lang::PhpHttpRequest2 => "PHP - HTTP_Request2",
            Lang::PhpPeclHttp => "PHP - pecl_http",
            Lang::PowershellRestMethod => "PowerShell - RestMethod",
            Lang::PythonHttpClient => "Python - http.client",
            Lang::PythonRequests => "Python - Requests",
            Lang::RHttr => "R - httr",
            Lang::RRcurl => "R - RCurl",
            Lang::RubyNetHttp => "Ruby - Net::HTTP",
            Lang::RustReqwest => "Rust - reqwest",
            Lang::ShellHttpie => "Shell - Httpie",
            Lang::ShellWget => "Shell - wget",
            Lang::SwiftUrlSession => "Swift - URLSession",
        }
    }

    pub fn syntax(self) -> Syntax {
        match self {
            Lang::Curl | Lang::ShellHttpie | Lang::ShellWget => Syntax::Shell,
            Lang::Http => Syntax::Http,
            Lang::CLibcurl => Syntax::C,
            Lang::CsharpHttpClient | Lang::CsharpRestSharp => Syntax::CSharp,
            Lang::DartDio | Lang::DartHttp => Syntax::Dart,
            Lang::GoNative => Syntax::Go,
            Lang::JavaOkHttp | Lang::JavaUnirest => Syntax::Java,
            Lang::JsFetch | Lang::JsJquery | Lang::JsXhr | Lang::NodeAxios | Lang::NodeNative | Lang::NodeRequest | Lang::NodeUnirest => Syntax::Js,
            Lang::KotlinOkhttp => Syntax::Kotlin,
            Lang::ObjcNsUrlSession => Syntax::ObjC,
            Lang::OcamlCohttp => Syntax::OCaml,
            Lang::PhpCurl | Lang::PhpGuzzle | Lang::PhpHttpRequest2 | Lang::PhpPeclHttp => Syntax::Php,
            Lang::PowershellRestMethod => Syntax::PowerShell,
            Lang::PythonHttpClient | Lang::PythonRequests => Syntax::Python,
            Lang::RHttr | Lang::RRcurl => Syntax::R,
            Lang::RubyNetHttp => Syntax::Ruby,
            Lang::RustReqwest => Syntax::Rust,
            Lang::SwiftUrlSession => Syntax::Swift,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub lang: Lang,
    pub indent: usize,
    pub tabs: bool,
    pub follow_redirects: bool,
    /// cURL: `--header` instead of `-H`.
    pub curl_long: bool,
    /// cURL line continuation: '\\' (bash), '^' (cmd) or '`' (PowerShell).
    pub curl_continuation: char,
}

impl Default for Options {
    fn default() -> Self {
        Self { lang: Lang::Curl, indent: 2, tabs: false, follow_redirects: true, curl_long: true, curl_continuation: '\\' }
    }
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

/// JSON-style double-quoted literal (valid in JS, Java, C#, Go, Python, R, ObjC...).
fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// Shell single quotes.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

/// PHP / Ruby-style single-quoted literal.
fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// PowerShell single-quoted literal.
fn pwsh(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Dart single-quoted literal (also escapes `$` interpolation).
fn dart(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Kotlin literal: JSON escapes minus `\f`, plus `$`.
fn kt(s: &str) -> String {
    q(s).replace("\\f", "\\u000C").replace('$', "\\$")
}

/// Ruby double-quoted literal: JSON escapes, `#` escaped to avoid interpolation.
fn rb(s: &str) -> String {
    q(s).replace('#', "\\#")
}

/// C / Objective-C literal (control characters as octal escapes).
fn cq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{:03o}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Swift literal.
fn swq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// OCaml literal (control characters as decimal escapes).
fn ocq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Rust literal.
fn rsq(s: &str) -> String {
    format!("{s:?}")
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

struct UrlParts {
    https: bool,
    host: String,
    port: Option<u16>,
    path: String,
}

fn url_parts(url: &str) -> UrlParts {
    match url::Url::parse(url) {
        Ok(u) => {
            let mut path = u.path().to_string();
            if let Some(qs) = u.query() {
                path.push('?');
                path.push_str(qs);
            }
            UrlParts { https: u.scheme() == "https", host: u.host_str().unwrap_or("").to_string(), port: u.port(), path }
        }
        Err(_) => UrlParts { https: url.starts_with("https"), host: String::new(), port: None, path: url.to_string() },
    }
}

const BOUNDARY: &str = "----WebKitFormBoundary7MA4YWxkTrZu0gW";

/// Line writer with configurable indentation (body strings may contain newlines and are not re-indented).
struct W {
    lines: Vec<String>,
    unit: String,
}

impl W {
    fn new(o: &Options) -> Self {
        let unit = if o.tabs { "\t".to_string() } else { " ".repeat(o.indent.max(1)) };
        Self { lines: Vec::new(), unit }
    }
    fn l(&mut self, level: usize, s: impl AsRef<str>) {
        self.lines.push(format!("{}{}", self.unit.repeat(level), s.as_ref()));
    }
    fn blank(&mut self) {
        self.lines.push(String::new());
    }
    /// Comma-separated items, one per line, no trailing comma.
    fn list(&mut self, level: usize, items: &[String]) {
        for (i, it) in items.iter().enumerate() {
            let comma = if i + 1 < items.len() { "," } else { "" };
            self.l(level, format!("{it}{comma}"));
        }
    }
    fn finish(self) -> String {
        self.lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

pub fn generate(s: &Snippet, o: &Options) -> String {
    match o.lang {
        Lang::CLibcurl => c_libcurl(s, o),
        Lang::CsharpHttpClient => cs_httpclient(s, o),
        Lang::CsharpRestSharp => cs_restsharp(s, o),
        Lang::Curl => curl(s, o),
        Lang::DartDio => dart_dio(s, o),
        Lang::DartHttp => dart_http(s, o),
        Lang::GoNative => go_native(s, o),
        Lang::Http => raw_http(s),
        Lang::JavaOkHttp => java_okhttp(s, o),
        Lang::JavaUnirest => java_unirest(s, o),
        Lang::JsFetch => js_fetch(s, o),
        Lang::JsJquery => js_jquery(s, o),
        Lang::JsXhr => js_xhr(s, o),
        Lang::KotlinOkhttp => kotlin_okhttp(s, o),
        Lang::NodeAxios => node_axios(s, o),
        Lang::NodeNative => node_native(s, o),
        Lang::NodeRequest => node_request(s, o),
        Lang::NodeUnirest => node_unirest(s, o),
        Lang::ObjcNsUrlSession => objc(s, o),
        Lang::OcamlCohttp => ocaml(s, o),
        Lang::PhpCurl => php_curl(s, o),
        Lang::PhpGuzzle => php_guzzle(s, o),
        Lang::PhpHttpRequest2 => php_request2(s, o),
        Lang::PhpPeclHttp => php_pecl(s, o),
        Lang::PowershellRestMethod => powershell(s, o),
        Lang::PythonHttpClient => python_http_client(s, o),
        Lang::PythonRequests => python_requests(s, o),
        Lang::RHttr => r_httr(s, o),
        Lang::RRcurl => r_rcurl(s, o),
        Lang::RubyNetHttp => ruby(s, o),
        Lang::RustReqwest => rust_reqwest(s, o),
        Lang::ShellHttpie => httpie(s, o),
        Lang::ShellWget => wget(s, o),
        Lang::SwiftUrlSession => swift(s, o),
    }
}

fn curl(s: &Snippet, o: &Options) -> String {
    let (loc, req, hdr, data, form) = if o.curl_long {
        ("--location", "--request", "--header", "--data", "--form")
    } else {
        ("-L", "-X", "-H", "-d", "-F")
    };
    let cont = format!(" {}", o.curl_continuation);
    let mut first = String::from("curl");
    if o.follow_redirects {
        first.push(' ');
        first.push_str(loc);
    }
    let default_method = match (&s.body, s.method) {
        (SBody::None, "GET") => true,
        (b, "POST") => !matches!(b, SBody::None),
        _ => false,
    };
    if !default_method {
        first.push_str(&format!(" {req} {}", s.method));
    }
    first.push_str(&format!(" {}", sq(&s.url)));
    let mut parts = vec![first];
    for (k, v) in &s.headers {
        parts.push(format!("{hdr} {}", sq(&format!("{k}: {v}"))));
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => parts.push(format!("{data} {}", sq(t))),
        SBody::UrlEncoded(pairs) => {
            for (k, v) in pairs {
                parts.push(format!("--data-urlencode {}", sq(&format!("{k}={v}"))));
            }
        }
        SBody::FormData(ps) => {
            for p in ps {
                let v = if p.is_file { format!("{}=@\"{}\"", p.name, p.value) } else { format!("{}=\"{}\"", p.name, p.value) };
                parts.push(format!("{form} {}", sq(&v)));
            }
        }
        SBody::File(path) => parts.push(format!("--data-binary {}", sq(&format!("@{path}")))),
    }
    parts.join(&format!("{cont}\n"))
}

fn httpie(s: &Snippet, o: &Options) -> String {
    let mut flags = String::from("http --ignore-stdin");
    match &s.body {
        SBody::UrlEncoded(_) => flags.push_str(" --form"),
        SBody::FormData(_) => flags.push_str(" --multipart"),
        _ => {}
    }
    if o.follow_redirects {
        flags.push_str(" --follow");
    }
    flags.push_str(&format!(" --timeout 3600 {} {}", s.method, sq(&s.url)));
    let mut parts = vec![flags];
    for (k, v) in &s.headers {
        if matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type") {
            continue;
        }
        parts.push(format!(" {}:{}", k, sq(v)));
    }
    match &s.body {
        SBody::UrlEncoded(pairs) => parts.extend(pairs.iter().map(|(k, v)| format!(" {}={}", k, sq(v)))),
        SBody::FormData(ps) => parts.extend(ps.iter().map(|p| if p.is_file { format!(" {}@{}", p.name, sq(&p.value)) } else { format!(" {}={}", p.name, sq(&p.value)) })),
        _ => {}
    }
    let cmd = parts.join(" \\\n");
    match &s.body {
        SBody::Raw(t) => format!("printf {}| {cmd}", sq(t)),
        SBody::File(p) => format!("{cmd} < {}", sq(p)),
        _ => cmd,
    }
}

fn wget(s: &Snippet, _o: &Options) -> String {
    let mut parts = vec!["wget --no-check-certificate --quiet".to_string(), format!("  --method {}", s.method), "  --timeout=0".to_string()];
    for (k, v) in &s.headers {
        parts.push(format!("  --header {}", sq(&format!("{k}: {v}"))));
    }
    let mut note = None;
    match &s.body {
        SBody::Raw(_) | SBody::UrlEncoded(_) => parts.push(format!("  --body-data {}", sq(&s.text_body().unwrap_or_default()))),
        SBody::File(p) => parts.push(format!("  --body-file={}", sq(p))),
        SBody::FormData(_) => note = Some("# wget does not support multipart/form-data bodies"),
        SBody::None => {}
    }
    parts.push(format!("   {}", sq(&s.url)));
    let cmd = parts.join(" \\\n");
    match note {
        Some(n) => format!("{n}\n{cmd}"),
        None => cmd,
    }
}

fn raw_http(s: &Snippet) -> String {
    let u = url_parts(&s.url);
    let mut out = vec![format!("{} {} HTTP/1.1", s.method, u.path)];
    let host = match u.port {
        Some(p) => format!("{}:{p}", u.host),
        None => u.host.clone(),
    };
    out.push(format!("Host: {host}"));
    let body = match &s.body {
        SBody::FormData(ps) => {
            let mut b = String::new();
            for p in ps {
                b.push_str(&format!("--{BOUNDARY}\r\n"));
                if p.is_file {
                    b.push_str(&format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: <Content-Type header here>\r\n\r\n(data)\r\n", p.name, file_name(&p.value)));
                } else {
                    b.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n", p.name, p.value));
                }
            }
            b.push_str(&format!("--{BOUNDARY}--"));
            Some(b.replace("\r\n", "\n"))
        }
        SBody::File(p) => Some(format!("\"<file contents here: {p}>\"")),
        _ => s.text_body(),
    };
    for (k, v) in &s.headers {
        out.push(format!("{k}: {v}"));
    }
    if matches!(s.body, SBody::FormData(_)) {
        out.push(format!("Content-Type: multipart/form-data; boundary={BOUNDARY}"));
    }
    if let Some(b) = &body {
        out.push(format!("Content-Length: {}", b.len()));
        out.push(String::new());
        out.push(b.clone());
    }
    out.join("\n")
}

fn js_fetch(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    if !s.headers.is_empty() {
        w.l(0, "const myHeaders = new Headers();");
        for (k, v) in &s.headers {
            if matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type") {
                continue;
            }
            w.l(0, format!("myHeaders.append({}, {});", q(k), q(v)));
        }
        w.blank();
    }
    let body_var = match &s.body {
        SBody::None => None,
        SBody::Raw(t) => {
            w.l(0, format!("const raw = {};", q(t)));
            Some("raw")
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "const urlencoded = new URLSearchParams();");
            for (k, v) in pairs {
                w.l(0, format!("urlencoded.append({}, {});", q(k), q(v)));
            }
            Some("urlencoded")
        }
        SBody::FormData(ps) => {
            w.l(0, "const formdata = new FormData();");
            for p in ps {
                if p.is_file {
                    w.l(0, format!("formdata.append({}, fileInput.files[0], {});", q(&p.name), q(&p.value)));
                } else {
                    w.l(0, format!("formdata.append({}, {});", q(&p.name), q(&p.value)));
                }
            }
            Some("formdata")
        }
        SBody::File(_) => {
            w.l(0, "const file = \"<file contents here>\";");
            Some("file")
        }
    };
    if body_var.is_some() {
        w.blank();
    }
    w.l(0, "const requestOptions = {");
    let mut items = vec![format!("method: {}", q(s.method))];
    if !s.headers.is_empty() {
        items.push("headers: myHeaders".into());
    }
    if let Some(b) = body_var {
        items.push(format!("body: {b}"));
    }
    items.push(format!("redirect: {}", q(if o.follow_redirects { "follow" } else { "manual" })));
    w.list(1, &items);
    w.l(0, "};");
    w.blank();
    w.l(0, format!("fetch({}, requestOptions)", q(&s.url)));
    w.l(1, ".then((response) => response.text())");
    w.l(1, ".then((result) => console.log(result))");
    w.l(1, ".catch((error) => console.error(error));");
    w.finish()
}

fn js_jquery(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    if let SBody::FormData(ps) = &s.body {
        w.l(0, "var form = new FormData();");
        for p in ps {
            if p.is_file {
                w.l(0, format!("form.append({}, fileInput.files[0], {});", q(&p.name), q(&p.value)));
            } else {
                w.l(0, format!("form.append({}, {});", q(&p.name), q(&p.value)));
            }
        }
        w.blank();
    }
    w.l(0, "var settings = {");
    let mut items = vec![format!("\"url\": {}", q(&s.url)), format!("\"method\": {}", q(s.method)), "\"timeout\": 0".to_string()];
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", q(k), q(v)))
        .collect();
    let mut header_block = None;
    if !headers.is_empty() {
        header_block = Some(headers);
        items.push("\"headers\": {".into());
    }
    match &s.body {
        SBody::Raw(t) => items.push(format!("\"data\": {}", q(t))),
        SBody::UrlEncoded(pairs) => {
            let fields: Vec<String> = pairs.iter().map(|(k, v)| format!("{}: {}", q(k), q(v))).collect();
            items.push(format!("\"data\": {{ {} }}", fields.join(", ")));
        }
        SBody::FormData(_) => {
            items.push("\"processData\": false".into());
            items.push("\"mimeType\": \"multipart/form-data\"".into());
            items.push("\"contentType\": false".into());
            items.push("\"data\": form".into());
        }
        SBody::File(_) => items.push("\"data\": \"<file contents here>\"".into()),
        SBody::None => {}
    }
    for (i, it) in items.iter().enumerate() {
        let last = i + 1 == items.len();
        if it == "\"headers\": {" {
            w.l(1, it);
            w.list(2, header_block.as_deref().unwrap_or(&[]));
            w.l(1, if last { "}" } else { "}," });
        } else {
            w.l(1, format!("{it}{}", if last { "" } else { "," }));
        }
    }
    w.l(0, "};");
    w.blank();
    w.l(0, "$.ajax(settings).done(function (response) {");
    w.l(1, "console.log(response);");
    w.l(0, "});");
    w.finish()
}

fn js_xhr(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    let has_data = match &s.body {
        SBody::FormData(ps) => {
            w.l(0, "// WARNING: For POST requests, body is set to null by browsers.");
            w.l(0, "var data = new FormData();");
            for p in ps {
                if p.is_file {
                    w.l(0, format!("data.append({}, fileInput.files[0], {});", q(&p.name), q(&p.value)));
                } else {
                    w.l(0, format!("data.append({}, {});", q(&p.name), q(&p.value)));
                }
            }
            true
        }
        SBody::File(_) => {
            w.l(0, "var data = \"<file contents here>\";");
            true
        }
        SBody::None => false,
        _ => {
            w.l(0, format!("var data = {};", q(&s.text_body().unwrap_or_default())));
            true
        }
    };
    if has_data {
        w.blank();
    }
    w.l(0, "var xhr = new XMLHttpRequest();");
    w.l(0, "xhr.withCredentials = true;");
    w.blank();
    w.l(0, "xhr.addEventListener(\"readystatechange\", function() {");
    w.l(1, "if(this.readyState === 4) {");
    w.l(2, "console.log(this.responseText);");
    w.l(1, "}");
    w.l(0, "});");
    w.blank();
    w.l(0, format!("xhr.open({}, {});", q(s.method), q(&s.url)));
    for (k, v) in &s.headers {
        if matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type") {
            continue;
        }
        w.l(0, format!("xhr.setRequestHeader({}, {});", q(k), q(v)));
    }
    w.blank();
    w.l(0, if has_data { "xhr.send(data);" } else { "xhr.send();" });
    w.finish()
}

fn node_axios(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "const axios = require('axios');");
    let data = match &s.body {
        SBody::FormData(ps) => {
            w.l(0, "const FormData = require('form-data');");
            if ps.iter().any(|p| p.is_file) {
                w.l(0, "const fs = require('fs');");
            }
            w.l(0, "let data = new FormData();");
            for p in ps {
                if p.is_file {
                    w.l(0, format!("data.append({}, fs.createReadStream({}));", q(&p.name), q(&p.value)));
                } else {
                    w.l(0, format!("data.append({}, {});", q(&p.name), q(&p.value)));
                }
            }
            true
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "let data = new URLSearchParams();");
            for (k, v) in pairs {
                w.l(0, format!("data.append({}, {});", q(k), q(v)));
            }
            true
        }
        SBody::Raw(t) => {
            w.l(0, format!("let data = {};", q(t)));
            true
        }
        SBody::File(p) => {
            w.l(0, "const fs = require('fs');");
            w.l(0, format!("let data = fs.readFileSync({});", q(p)));
            true
        }
        SBody::None => false,
    };
    w.blank();
    w.l(0, "let config = {");
    w.l(1, format!("method: {},", q(&s.method.to_ascii_lowercase())));
    w.l(1, "maxBodyLength: Infinity,");
    w.l(1, format!("url: {},", q(&s.url)));
    let mut headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", q(k), q(v)))
        .collect();
    if matches!(s.body, SBody::FormData(_)) {
        headers.push("...data.getHeaders()".into());
    }
    w.l(1, format!("headers: {{{}", if headers.is_empty() { " }" } else { "" }));
    if !headers.is_empty() {
        w.list(2, &headers);
        w.l(1, format!("}}{}", if data { "," } else { "" }));
    }
    if data {
        w.l(1, "data: data");
    }
    w.l(0, "};");
    w.blank();
    w.l(0, "axios.request(config)");
    w.l(0, ".then((response) => {");
    w.l(1, "console.log(JSON.stringify(response.data));");
    w.l(0, "})");
    w.l(0, ".catch((error) => {");
    w.l(1, "console.log(error);");
    w.l(0, "});");
    w.finish()
}

fn node_native(s: &Snippet, o: &Options) -> String {
    let u = url_parts(&s.url);
    let module = if u.https { "https" } else { "http" };
    let mut w = W::new(o);
    w.l(0, format!("const {module} = require('{module}');"));
    let files = matches!(&s.body, SBody::FormData(ps) if ps.iter().any(|p| p.is_file)) || matches!(s.body, SBody::File(_));
    if files {
        w.l(0, "const fs = require('fs');");
    }
    w.blank();
    w.l(0, "let options = {");
    let mut items = vec![format!("'method': {}", q(s.method)), format!("'hostname': {}", q(&u.host))];
    if let Some(p) = u.port {
        items.push(format!("'port': {p}"));
    }
    items.push(format!("'path': {}", q(&u.path)));
    let headers: Vec<String> = s.headers.iter().map(|(k, v)| format!("{}: {}", q(k), q(v))).collect();
    for it in &items {
        w.l(1, format!("{it},"));
    }
    w.l(1, "'headers': {");
    w.list(2, &headers);
    w.l(1, "},");
    w.l(1, "'maxRedirects': 20");
    w.l(0, "};");
    w.blank();
    w.l(0, format!("const req = {module}.request(options, (res) => {{"));
    w.l(1, "let chunks = [];");
    w.blank();
    w.l(1, "res.on(\"data\", (chunk) => {");
    w.l(2, "chunks.push(chunk);");
    w.l(1, "});");
    w.blank();
    w.l(1, "res.on(\"end\", () => {");
    w.l(2, "let body = Buffer.concat(chunks);");
    w.l(2, "console.log(body.toString());");
    w.l(1, "});");
    w.blank();
    w.l(1, "res.on(\"error\", (error) => {");
    w.l(2, "console.error(error);");
    w.l(1, "});");
    w.l(0, "});");
    w.blank();
    match &s.body {
        SBody::None => {}
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            w.l(0, format!("let postData = {};", q(&s.text_body().unwrap_or_default())));
            w.blank();
            w.l(0, "req.setHeader('Content-Length', Buffer.byteLength(postData));");
            w.l(0, "req.write(postData);");
            w.blank();
        }
        SBody::File(p) => {
            w.l(0, format!("let postData = fs.readFileSync({});", q(p)));
            w.blank();
            w.l(0, "req.setHeader('Content-Length', postData.length);");
            w.l(0, "req.write(postData);");
            w.blank();
        }
        SBody::FormData(ps) => {
            let mut expr = Vec::new();
            for p in ps {
                if p.is_file {
                    expr.push(q(&format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", p.name, file_name(&p.value))));
                    expr.push(format!("fs.readFileSync({})", q(&p.value)));
                    expr.push(q("\r\n"));
                } else {
                    expr.push(q(&format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n", p.name, p.value)));
                }
            }
            expr.push(q(&format!("--{BOUNDARY}--")));
            w.l(0, format!("let postData = {};", expr.join(" + ")));
            w.blank();
            w.l(0, format!("req.setHeader('content-type', 'multipart/form-data; boundary={BOUNDARY}');"));
            w.l(0, "req.setHeader('Content-Length', Buffer.byteLength(postData));");
            w.blank();
            w.l(0, "req.write(postData);");
            w.blank();
        }
    }
    w.l(0, "req.end();");
    w.finish()
}

fn node_request(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "var request = require('request');");
    let files = matches!(&s.body, SBody::FormData(ps) if ps.iter().any(|p| p.is_file)) || matches!(s.body, SBody::File(_));
    if files {
        w.l(0, "var fs = require('fs');");
    }
    w.l(0, "var options = {");
    w.l(1, format!("'method': {},", q(s.method)));
    w.l(1, format!("'url': {},", q(&s.url)));
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", q(k), q(v)))
        .collect();
    let has_body = s.has_body();
    w.l(1, "'headers': {");
    w.list(2, &headers);
    w.l(1, if has_body { "}," } else { "}" });
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => w.l(1, format!("body: {}", q(t))),
        SBody::File(p) => w.l(1, format!("body: fs.createReadStream({})", q(p))),
        SBody::UrlEncoded(pairs) => {
            w.l(1, "form: {");
            w.list(2, &pairs.iter().map(|(k, v)| format!("{}: {}", q(k), q(v))).collect::<Vec<_>>());
            w.l(1, "}");
        }
        SBody::FormData(ps) => {
            w.l(1, "formData: {");
            let mut items = Vec::new();
            for p in ps {
                if p.is_file {
                    items.push(format!(
                        "{}: {{\n{u}{u}{u}'value': fs.createReadStream({}),\n{u}{u}{u}'options': {{\n{u}{u}{u}{u}'filename': {},\n{u}{u}{u}{u}'contentType': null\n{u}{u}{u}}}\n{u}{u}}}",
                        q(&p.name),
                        q(&p.value),
                        q(&file_name(&p.value)),
                        u = w.unit
                    ));
                } else {
                    items.push(format!("{}: {}", q(&p.name), q(&p.value)));
                }
            }
            w.list(2, &items);
            w.l(1, "}");
        }
    }
    w.l(0, "};");
    w.l(0, "request(options, function (error, response) {");
    w.l(1, "if (error) throw new Error(error);");
    w.l(1, "console.log(response.body);");
    w.l(0, "});");
    w.finish()
}

fn node_unirest(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "var unirest = require('unirest');");
    w.l(0, format!("var req = unirest({}, {})", q(s.method), q(&s.url)));
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", q(k), q(v)))
        .collect();
    if !headers.is_empty() {
        w.l(1, ".headers({");
        w.list(2, &headers);
        w.l(1, "})");
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => w.l(1, format!(".send({})", q(t))),
        SBody::UrlEncoded(pairs) => {
            for (k, v) in pairs {
                w.l(1, format!(".send({})", q(&format!("{}={}", encode_form(k), encode_form(v)))));
            }
        }
        SBody::FormData(ps) => {
            for p in ps {
                if p.is_file {
                    w.l(1, format!(".attach('file', {})", q(&p.value)));
                } else {
                    w.l(1, format!(".field({}, {})", q(&p.name), q(&p.value)));
                }
            }
        }
        SBody::File(p) => w.l(1, format!(".send(require('fs').readFileSync({}))", q(p))),
    }
    w.l(1, ".end(function (res) {");
    w.l(2, "if (res.error) throw new Error(res.error);");
    w.l(2, "console.log(res.raw_body);");
    w.l(1, "});");
    w.finish()
}

fn python_requests(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "import requests");
    w.blank();
    w.l(0, format!("url = {}", q(&s.url)));
    w.blank();
    let mut files = false;
    match &s.body {
        SBody::None => w.l(0, "payload = {}"),
        SBody::Raw(_) | SBody::UrlEncoded(_) => w.l(0, format!("payload = {}", q(&s.text_body().unwrap_or_default()))),
        SBody::File(p) => w.l(0, format!("payload = open({}, \"rb\")", q(p))),
        SBody::FormData(ps) => {
            let fields: Vec<String> = ps.iter().filter(|p| !p.is_file).map(|p| format!("{}: {}", q(&p.name), q(&p.value))).collect();
            w.l(0, format!("payload = {{{}}}", fields.join(", ")));
            let fs: Vec<String> = ps
                .iter()
                .filter(|p| p.is_file)
                .map(|p| format!("({}, ({}, open({}, \"rb\"), \"application/octet-stream\"))", q(&p.name), q(&file_name(&p.value)), q(&p.value)))
                .collect();
            if !fs.is_empty() {
                files = true;
                w.l(0, "files = [");
                w.list(1, &fs);
                w.l(0, "]");
            }
        }
    }
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", q(k), q(v)))
        .collect();
    if headers.is_empty() {
        w.l(0, "headers = {}");
    } else {
        w.l(0, "headers = {");
        w.list(1, &headers);
        w.l(0, "}");
    }
    w.blank();
    let files_arg = if files { ", files=files" } else { "" };
    let redir = if o.follow_redirects { "" } else { ", allow_redirects=False" };
    w.l(0, format!("response = requests.request({}, url, headers=headers, data=payload{files_arg}{redir})", q(s.method)));
    w.blank();
    w.l(0, "print(response.text)");
    w.finish()
}

fn python_http_client(s: &Snippet, o: &Options) -> String {
    let u = url_parts(&s.url);
    let mut w = W::new(o);
    w.l(0, "import http.client");
    let multipart = matches!(s.body, SBody::FormData(_));
    if multipart {
        w.l(0, "import mimetypes");
        w.l(0, "from codecs import encode");
    }
    w.blank();
    let conn_cls = if u.https { "HTTPSConnection" } else { "HTTPConnection" };
    match u.port {
        Some(p) => w.l(0, format!("conn = http.client.{conn_cls}({}, {p})", q(&u.host))),
        None => w.l(0, format!("conn = http.client.{conn_cls}({})", q(&u.host))),
    }
    match &s.body {
        SBody::None => w.l(0, "payload = ''"),
        // http.client would encode a str body as Latin-1; send UTF-8 bytes instead.
        SBody::Raw(_) | SBody::UrlEncoded(_) => w.l(0, format!("payload = {}.encode(\"utf-8\")", q(&s.text_body().unwrap_or_default()))),
        SBody::File(p) => w.l(0, format!("payload = open({}, 'rb').read()", q(p))),
        SBody::FormData(ps) => {
            w.l(0, "dataList = []");
            w.l(0, "boundary = 'wL36Yn8afVp8Ag7AmP8qZ0SA4n1v9T'");
            for p in ps {
                w.l(0, "dataList.append(encode('--' + boundary))");
                if p.is_file {
                    let cd = format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"", p.name, file_name(&p.value));
                    w.l(0, format!("dataList.append(encode({}))", q(&cd)));
                    w.blank();
                    w.l(0, format!("fileType = mimetypes.guess_type({})[0] or 'application/octet-stream'", q(&p.value)));
                    w.l(0, "dataList.append(encode('Content-Type: {}'.format(fileType)))");
                    w.l(0, "dataList.append(encode(''))");
                    w.blank();
                    w.l(0, format!("with open({}, 'rb') as f:", q(&p.value)));
                    w.l(1, "dataList.append(f.read())");
                } else {
                    w.l(0, format!("dataList.append(encode({}))", q(&format!("Content-Disposition: form-data; name=\"{}\"", p.name))));
                    w.blank();
                    w.l(0, "dataList.append(encode('Content-Type: {}'.format('text/plain')))");
                    w.l(0, "dataList.append(encode(''))");
                    w.blank();
                    w.l(0, format!("dataList.append(encode({}))", q(&p.value)));
                }
            }
            w.l(0, "dataList.append(encode('--'+boundary+'--'))");
            w.l(0, "dataList.append(encode(''))");
            w.l(0, "body = b'\\r\\n'.join(dataList)");
            w.l(0, "payload = body");
        }
    }
    let mut headers: Vec<String> = s.headers.iter().map(|(k, v)| format!("{}: {}", q(k), q(v))).collect();
    if multipart {
        headers.push("'Content-type': 'multipart/form-data; boundary={}'.format(boundary)".into());
    }
    if headers.is_empty() {
        w.l(0, "headers = {}");
    } else {
        w.l(0, "headers = {");
        w.list(1, &headers);
        w.l(0, "}");
    }
    w.l(0, format!("conn.request({}, {}, payload, headers)", q(s.method), q(&u.path)));
    w.l(0, "res = conn.getresponse()");
    w.l(0, "data = res.read()");
    w.l(0, "print(data.decode(\"utf-8\"))");
    w.finish()
}

fn go_native(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    let mut imports = vec!["fmt", "net/http", "io"];
    match &s.body {
        SBody::Raw(_) | SBody::UrlEncoded(_) => imports.insert(1, "strings"),
        SBody::FormData(ps) => {
            imports.extend(["bytes", "mime/multipart"]);
            if ps.iter().any(|p| p.is_file) {
                imports.extend(["os", "path/filepath"]);
            }
        }
        SBody::File(_) => imports.push("os"),
        SBody::None => {}
    }
    w.l(0, "package main");
    w.blank();
    w.l(0, "import (");
    for i in &imports {
        w.l(1, format!("\"{i}\""));
    }
    w.l(0, ")");
    w.blank();
    w.l(0, "func main() {");
    w.blank();
    w.l(1, format!("url := {}", q(&s.url)));
    w.l(1, format!("method := {}", q(s.method)));
    w.blank();
    let payload = match &s.body {
        SBody::None => "nil",
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            let t = s.text_body().unwrap_or_default();
            let lit = if t.contains('`') { q(&t) } else { format!("`{t}`") };
            w.l(1, format!("payload := strings.NewReader({lit})"));
            w.blank();
            "payload"
        }
        SBody::File(p) => {
            w.l(1, format!("payload, err := os.Open({})", q(p)));
            w.l(1, "if err != nil {");
            w.l(2, "fmt.Println(err)");
            w.l(2, "return");
            w.l(1, "}");
            w.l(1, "defer payload.Close()");
            w.blank();
            "payload"
        }
        SBody::FormData(ps) => {
            w.l(1, "payload := &bytes.Buffer{}");
            w.l(1, "writer := multipart.NewWriter(payload)");
            for (i, p) in ps.iter().enumerate() {
                if p.is_file {
                    let n = i + 1;
                    w.l(1, format!("file{n}, errFile{n} := os.Open({})", q(&p.value)));
                    w.l(1, format!("if errFile{n} != nil {{"));
                    w.l(2, format!("fmt.Println(errFile{n})"));
                    w.l(2, "return");
                    w.l(1, "}");
                    w.l(1, format!("defer file{n}.Close()"));
                    w.l(1, format!("part{n}, errFile{n} := writer.CreateFormFile({}, filepath.Base({}))", q(&p.name), q(&p.value)));
                    w.l(1, format!("_, errFile{n} = io.Copy(part{n}, file{n})"));
                    w.l(1, format!("if errFile{n} != nil {{"));
                    w.l(2, format!("fmt.Println(errFile{n})"));
                    w.l(2, "return");
                    w.l(1, "}");
                } else {
                    w.l(1, format!("_ = writer.WriteField({}, {})", q(&p.name), q(&p.value)));
                }
            }
            w.l(1, "err := writer.Close()");
            w.l(1, "if err != nil {");
            w.l(2, "fmt.Println(err)");
            w.l(2, "return");
            w.l(1, "}");
            w.blank();
            "payload"
        }
    };
    w.blank();
    w.l(1, "client := &http.Client {");
    w.l(1, "}");
    // `:=` is valid even when `err` already exists, because `req` is new.
    w.l(1, format!("req, err := http.NewRequest(method, url, {payload})"));
    w.blank();
    w.l(1, "if err != nil {");
    w.l(2, "fmt.Println(err)");
    w.l(2, "return");
    w.l(1, "}");
    for (k, v) in &s.headers {
        w.l(1, format!("req.Header.Add({}, {})", q(k), q(v)));
    }
    if matches!(s.body, SBody::FormData(_)) {
        w.blank();
        w.l(1, "req.Header.Set(\"Content-Type\", writer.FormDataContentType())");
    }
    w.l(1, "res, err := client.Do(req)");
    w.l(1, "if err != nil {");
    w.l(2, "fmt.Println(err)");
    w.l(2, "return");
    w.l(1, "}");
    w.l(1, "defer res.Body.Close()");
    w.blank();
    w.l(1, "body, err := io.ReadAll(res.Body)");
    w.l(1, "if err != nil {");
    w.l(2, "fmt.Println(err)");
    w.l(2, "return");
    w.l(1, "}");
    w.l(1, "fmt.Println(string(body))");
    w.l(0, "}");
    w.finish()
}

fn c_libcurl(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "CURL *curl;");
    w.l(0, "CURLcode res;");
    w.l(0, "curl = curl_easy_init();");
    w.l(0, "if(curl) {");
    w.l(1, format!("curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, {});", cq(s.method)));
    w.l(1, format!("curl_easy_setopt(curl, CURLOPT_URL, {});", cq(&s.url)));
    if o.follow_redirects {
        w.l(1, "curl_easy_setopt(curl, CURLOPT_FOLLOWLOCATION, 1L);");
    }
    w.l(1, "curl_easy_setopt(curl, CURLOPT_DEFAULT_PROTOCOL, \"https\");");
    let headers: Vec<&(String, String)> = s.headers.iter().filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type"))).collect();
    if !headers.is_empty() {
        w.l(1, "struct curl_slist *headers = NULL;");
        for (k, v) in &headers {
            w.l(1, format!("headers = curl_slist_append(headers, {});", cq(&format!("{k}: {v}"))));
        }
        w.l(1, "curl_easy_setopt(curl, CURLOPT_HTTPHEADER, headers);");
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            w.l(1, format!("const char *data = {};", cq(&s.text_body().unwrap_or_default())));
            w.l(1, "curl_easy_setopt(curl, CURLOPT_POSTFIELDS, data);");
        }
        SBody::File(p) => {
            w.l(1, format!("FILE *fd = fopen({}, \"rb\");", cq(p)));
            w.l(1, "curl_easy_setopt(curl, CURLOPT_UPLOAD, 1L);");
            w.l(1, "curl_easy_setopt(curl, CURLOPT_READDATA, fd);");
        }
        SBody::FormData(ps) => {
            w.l(1, "curl_mime *mime;");
            w.l(1, "curl_mimepart *part;");
            w.l(1, "mime = curl_mime_init(curl);");
            for p in ps {
                w.l(1, "part = curl_mime_addpart(mime);");
                w.l(1, format!("curl_mime_name(part, {});", cq(&p.name)));
                if p.is_file {
                    w.l(1, format!("curl_mime_filedata(part, {});", cq(&p.value)));
                } else {
                    w.l(1, format!("curl_mime_data(part, {}, CURL_ZERO_TERMINATED);", cq(&p.value)));
                }
            }
            w.l(1, "curl_easy_setopt(curl, CURLOPT_MIMEPOST, mime);");
        }
    }
    w.l(1, "res = curl_easy_perform(curl);");
    if matches!(s.body, SBody::FormData(_)) {
        w.l(1, "curl_mime_free(mime);");
    }
    if !headers.is_empty() {
        w.l(1, "curl_slist_free_all(headers);");
    }
    w.l(0, "}");
    w.l(0, "curl_easy_cleanup(curl);");
    w.finish()
}

fn cs_method(m: &str) -> String {
    let mut c = m.to_ascii_lowercase();
    if let Some(f) = c.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    c
}

fn cs_httpclient(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "var client = new HttpClient();");
    w.l(0, format!("var request = new HttpRequestMessage(HttpMethod.{}, {});", cs_method(s.method), q(&s.url)));
    for (k, v) in s.headers_no_ct() {
        w.l(0, format!("request.Headers.Add({}, {});", q(k), q(v)));
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => {
            let ct = s.content_type().map(q).unwrap_or_else(|| "null".into());
            w.l(0, format!("var content = new StringContent({}, null, {ct});", q(t)));
            w.l(0, "request.Content = content;");
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "var collection = new List<KeyValuePair<string, string>>();");
            for (k, v) in pairs {
                w.l(0, format!("collection.Add(new({}, {}));", q(k), q(v)));
            }
            w.l(0, "var content = new FormUrlEncodedContent(collection);");
            w.l(0, "request.Content = content;");
        }
        SBody::FormData(ps) => {
            w.l(0, "var content = new MultipartFormDataContent();");
            for p in ps {
                if p.is_file {
                    w.l(0, format!("content.Add(new StreamContent(File.OpenRead({})), {}, {});", q(&p.value), q(&p.name), q(&file_name(&p.value))));
                } else {
                    w.l(0, format!("content.Add(new StringContent({}), {});", q(&p.value), q(&p.name)));
                }
            }
            w.l(0, "request.Content = content;");
        }
        SBody::File(p) => {
            w.l(0, format!("request.Content = new StreamContent(File.OpenRead({}));", q(p)));
        }
    }
    w.l(0, "var response = await client.SendAsync(request);");
    w.l(0, "response.EnsureSuccessStatusCode();");
    w.l(0, "Console.WriteLine(await response.Content.ReadAsStringAsync());");
    w.finish()
}

fn cs_restsharp(s: &Snippet, o: &Options) -> String {
    let verbatim = |t: &str| format!("@\"{}\"", t.replace('"', "\"\""));
    let mut w = W::new(o);
    w.l(0, "var options = new RestClientOptions(\"\")");
    w.l(0, "{");
    w.l(1, "MaxTimeout = -1,");
    w.l(0, "};");
    w.l(0, "var client = new RestClient(options);");
    w.l(0, format!("var request = new RestRequest({}, Method.{});", q(&s.url), cs_method(s.method)));
    for (k, v) in s.headers.iter().filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type"))) {
        w.l(0, format!("request.AddHeader({}, {});", q(k), q(v)));
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => {
            w.l(0, format!("var body = {};", verbatim(t)));
            let ct = s.content_type().unwrap_or("text/plain");
            w.l(0, format!("request.AddStringBody(body, {});", q(ct)));
        }
        SBody::UrlEncoded(pairs) => {
            for (k, v) in pairs {
                w.l(0, format!("request.AddParameter({}, {});", q(k), q(v)));
            }
        }
        SBody::FormData(ps) => {
            w.l(0, "request.AlwaysMultipartFormData = true;");
            for p in ps {
                if p.is_file {
                    w.l(0, format!("request.AddFile({}, {});", q(&p.name), q(&p.value)));
                } else {
                    w.l(0, format!("request.AddParameter({}, {});", q(&p.name), q(&p.value)));
                }
            }
        }
        SBody::File(p) => {
            w.l(0, format!("request.AddParameter(\"application/octet-stream\", File.ReadAllBytes({}), ParameterType.RequestBody);", q(p)));
        }
    }
    w.l(0, "RestResponse response = await client.ExecuteAsync(request);");
    w.l(0, "Console.WriteLine(response.Content);");
    w.finish()
}

fn dart_dio(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", dart(k), dart(v)))
        .collect();
    if !headers.is_empty() {
        w.l(0, "var headers = {");
        w.list(1, &headers);
        w.l(0, "};");
    }
    let data = match &s.body {
        SBody::None => false,
        SBody::Raw(t) => {
            w.l(0, format!("var data = {};", dart(t)));
            true
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "var data = {");
            w.list(1, &pairs.iter().map(|(k, v)| format!("{}: {}", dart(k), dart(v))).collect::<Vec<_>>());
            w.l(0, "};");
            true
        }
        SBody::FormData(ps) => {
            w.l(0, "var data = FormData.fromMap({");
            let items: Vec<String> = ps
                .iter()
                .map(|p| {
                    if p.is_file {
                        format!("{}: [await MultipartFile.fromFile({}, filename: {})]", dart(&p.name), dart(&p.value), dart(&file_name(&p.value)))
                    } else {
                        format!("{}: {}", dart(&p.name), dart(&p.value))
                    }
                })
                .collect();
            w.list(1, &items);
            w.l(0, "});");
            true
        }
        SBody::File(p) => {
            w.l(0, format!("var data = File({}).openRead();", dart(p)));
            true
        }
    };
    w.blank();
    w.l(0, "var dio = Dio();");
    w.l(0, "var response = await dio.request(");
    w.l(1, format!("{},", dart(&s.url)));
    w.l(1, "options: Options(");
    w.l(2, format!("method: {},", dart(s.method)));
    if !headers.is_empty() {
        w.l(2, "headers: headers,");
    }
    if matches!(s.body, SBody::UrlEncoded(_)) {
        w.l(2, "contentType: Headers.formUrlEncodedContentType,");
    }
    w.l(1, "),");
    if data {
        w.l(1, "data: data,");
    }
    w.l(0, ");");
    w.blank();
    w.l(0, "if (response.statusCode == 200) {");
    w.l(1, "print(json.encode(response.data));");
    w.l(0, "}");
    w.l(0, "else {");
    w.l(1, "print(response.statusMessage);");
    w.l(0, "}");
    w.finish()
}

fn dart_http(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    let multipart = matches!(s.body, SBody::FormData(_));
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", dart(k), dart(v)))
        .collect();
    if !headers.is_empty() {
        w.l(0, "var headers = {");
        w.list(1, &headers);
        w.l(0, "};");
    }
    let cls = if multipart { "http.MultipartRequest" } else { "http.Request" };
    w.l(0, format!("var request = {cls}({}, Uri.parse({}));", dart(s.method), dart(&s.url)));
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => w.l(0, format!("request.body = {};", dart(t))),
        SBody::UrlEncoded(pairs) => {
            w.l(0, "request.bodyFields = {");
            w.list(1, &pairs.iter().map(|(k, v)| format!("{}: {}", dart(k), dart(v))).collect::<Vec<_>>());
            w.l(0, "};");
        }
        SBody::FormData(ps) => {
            let fields: Vec<String> = ps.iter().filter(|p| !p.is_file).map(|p| format!("{}: {}", dart(&p.name), dart(&p.value))).collect();
            if !fields.is_empty() {
                w.l(0, "request.fields.addAll({");
                w.list(1, &fields);
                w.l(0, "});");
            }
            for p in ps.iter().filter(|p| p.is_file) {
                w.l(0, format!("request.files.add(await http.MultipartFile.fromPath({}, {}));", dart(&p.name), dart(&p.value)));
            }
        }
        SBody::File(p) => w.l(0, format!("request.bodyBytes = await File({}).readAsBytes();", dart(p))),
    }
    if !headers.is_empty() {
        w.blank();
        w.l(0, "request.headers.addAll(headers);");
    }
    w.blank();
    w.l(0, "http.StreamedResponse response = await request.send();");
    w.blank();
    w.l(0, "if (response.statusCode == 200) {");
    w.l(1, "print(await response.stream.bytesToString());");
    w.l(0, "}");
    w.l(0, "else {");
    w.l(1, "print(response.reasonPhrase);");
    w.l(0, "}");
    w.finish()
}

fn java_okhttp(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "OkHttpClient client = new OkHttpClient().newBuilder()");
    if !o.follow_redirects {
        w.l(1, ".followRedirects(false)");
    }
    w.l(1, ".build();");
    let needs_body = matches!(s.method, "POST" | "PUT" | "PATCH");
    let body = match &s.body {
        SBody::None if needs_body => {
            w.l(0, "MediaType mediaType = MediaType.parse(\"text/plain\");");
            w.l(0, "RequestBody body = RequestBody.create(mediaType, \"\");");
            "body"
        }
        SBody::None => "null",
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            let ct = s.content_type().unwrap_or("text/plain");
            w.l(0, format!("MediaType mediaType = MediaType.parse({});", q(ct)));
            w.l(0, format!("RequestBody body = RequestBody.create(mediaType, {});", q(&s.text_body().unwrap_or_default())));
            "body"
        }
        SBody::File(p) => {
            w.l(0, "MediaType mediaType = MediaType.parse(\"application/octet-stream\");");
            w.l(0, format!("RequestBody body = RequestBody.create(mediaType, new File({}));", q(p)));
            "body"
        }
        SBody::FormData(ps) => {
            w.l(0, "RequestBody body = new MultipartBody.Builder().setType(MultipartBody.FORM)");
            for p in ps {
                if p.is_file {
                    w.l(1, format!(
                        ".addFormDataPart({}, {}, RequestBody.create(MediaType.parse(\"application/octet-stream\"), new File({})))",
                        q(&p.name),
                        q(&file_name(&p.value)),
                        q(&p.value)
                    ));
                } else {
                    w.l(1, format!(".addFormDataPart({}, {})", q(&p.name), q(&p.value)));
                }
            }
            w.l(1, ".build();");
            "body"
        }
    };
    w.l(0, "Request request = new Request.Builder()");
    w.l(1, format!(".url({})", q(&s.url)));
    w.l(1, format!(".method({}, {body})", q(s.method)));
    for (k, v) in s.headers.iter().filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type"))) {
        w.l(1, format!(".addHeader({}, {})", q(k), q(v)));
    }
    w.l(1, ".build();");
    w.l(0, "Response response = client.newCall(request).execute();");
    w.finish()
}

fn java_unirest(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "Unirest.setTimeouts(0, 0);");
    w.l(0, format!("HttpResponse<String> response = Unirest.{}({})", s.method.to_ascii_lowercase(), q(&s.url)));
    for (k, v) in s.headers.iter().filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type"))) {
        w.l(1, format!(".header({}, {})", q(k), q(v)));
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => w.l(1, format!(".body({})", q(t))),
        SBody::UrlEncoded(pairs) => {
            for (k, v) in pairs {
                w.l(1, format!(".field({}, {})", q(k), q(v)));
            }
        }
        SBody::FormData(ps) => {
            w.l(1, ".multiPartContent()");
            for p in ps {
                if p.is_file {
                    w.l(1, format!(".field({}, new File({}))", q(&p.name), q(&p.value)));
                } else {
                    w.l(1, format!(".field({}, {})", q(&p.name), q(&p.value)));
                }
            }
        }
        SBody::File(p) => w.l(1, format!(".body(Files.readAllBytes(Paths.get({})))", q(p))),
    }
    w.l(1, ".asString();");
    w.finish()
}

fn kotlin_okhttp(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "val client = OkHttpClient()");
    let needs_body = matches!(s.method, "POST" | "PUT" | "PATCH");
    let body = match &s.body {
        SBody::None if needs_body => {
            w.l(0, "val body = \"\".toRequestBody(\"text/plain\".toMediaType())");
            "body"
        }
        SBody::None => "null",
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            let ct = s.content_type().unwrap_or("text/plain");
            w.l(0, format!("val mediaType = {}.toMediaType()", kt(ct)));
            w.l(0, format!("val body = {}.toRequestBody(mediaType)", kt(&s.text_body().unwrap_or_default())));
            "body"
        }
        SBody::File(p) => {
            w.l(0, format!("val body = File({}).asRequestBody(\"application/octet-stream\".toMediaType())", kt(p)));
            "body"
        }
        SBody::FormData(ps) => {
            w.l(0, "val body = MultipartBody.Builder().setType(MultipartBody.FORM)");
            for p in ps {
                if p.is_file {
                    w.l(1, format!(".addFormDataPart({}, {}, File({}).asRequestBody(\"application/octet-stream\".toMediaType()))", kt(&p.name), kt(&file_name(&p.value)), kt(&p.value)));
                } else {
                    w.l(1, format!(".addFormDataPart({}, {})", kt(&p.name), kt(&p.value)));
                }
            }
            w.l(1, ".build()");
            "body"
        }
    };
    w.l(0, "val request = Request.Builder()");
    w.l(1, format!(".url({})", kt(&s.url)));
    w.l(1, format!(".method({}, {body})", kt(s.method)));
    for (k, v) in s.headers.iter().filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type"))) {
        w.l(1, format!(".addHeader({}, {})", kt(k), kt(v)));
    }
    w.l(1, ".build()");
    w.l(0, "val response = client.newCall(request).execute()");
    w.finish()
}

fn objc(s: &Snippet, o: &Options) -> String {
    let oq = |t: &str| format!("@{}", cq(t));
    let mut w = W::new(o);
    w.l(0, "#import <Foundation/Foundation.h>");
    w.blank();
    w.l(0, "dispatch_semaphore_t sema = dispatch_semaphore_create(0);");
    w.blank();
    w.l(0, format!("NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:[NSURL URLWithString:{}]", oq(&s.url)));
    w.l(1, "cachePolicy:NSURLRequestUseProtocolCachePolicy");
    w.l(1, "timeoutInterval:10.0];");
    let multipart = matches!(s.body, SBody::FormData(_));
    let mut headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{}: {}", oq(k), oq(v)))
        .collect();
    if multipart {
        headers.push(format!("@\"Content-Type\": @\"multipart/form-data; boundary={BOUNDARY}\""));
    }
    if !headers.is_empty() {
        w.l(0, "NSDictionary *headers = @{");
        w.list(1, &headers);
        w.l(0, "};");
        w.blank();
        w.l(0, "[request setAllHTTPHeaderFields:headers];");
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            w.l(0, format!("NSData *postData = [[NSData alloc] initWithData:[{} dataUsingEncoding:NSUTF8StringEncoding]];", oq(&s.text_body().unwrap_or_default())));
            w.l(0, "[request setHTTPBody:postData];");
        }
        SBody::File(p) => {
            w.l(0, format!("NSData *postData = [NSData dataWithContentsOfFile:{}];", oq(p)));
            w.l(0, "[request setHTTPBody:postData];");
        }
        SBody::FormData(ps) => {
            w.l(0, "NSArray *parameters = @[");
            let items: Vec<String> = ps
                .iter()
                .map(|p| {
                    if p.is_file {
                        format!("@{{ @\"name\": {}, @\"fileName\": {} }}", oq(&p.name), oq(&p.value))
                    } else {
                        format!("@{{ @\"name\": {}, @\"value\": {} }}", oq(&p.name), oq(&p.value))
                    }
                })
                .collect();
            w.list(1, &items);
            w.l(0, "];");
            w.blank();
            w.l(0, format!("NSString *boundary = @\"{BOUNDARY}\";"));
            w.l(0, "NSError *error;");
            w.l(0, "NSMutableString *body = [NSMutableString string];");
            w.blank();
            w.l(0, "for (NSDictionary *param in parameters) {");
            w.l(1, "[body appendFormat:@\"--%@\\r\\n\", boundary];");
            w.l(1, "if (param[@\"fileName\"]) {");
            w.l(2, "[body appendFormat:@\"Content-Disposition:form-data; name=\\\"%@\\\"; filename=\\\"%@\\\"\\r\\n\", param[@\"name\"], param[@\"fileName\"]];");
            w.l(2, "[body appendString:@\"Content-Type: application/octet-stream\\r\\n\\r\\n\"];");
            w.l(2, "[body appendFormat:@\"%@\", [NSString stringWithContentsOfFile:param[@\"fileName\"] encoding:NSUTF8StringEncoding error:&error]];");
            w.l(2, "if (error) {");
            w.l(3, "NSLog(@\"%@\", error);");
            w.l(2, "}");
            w.l(1, "} else {");
            w.l(2, "[body appendFormat:@\"Content-Disposition:form-data; name=\\\"%@\\\"\\r\\n\\r\\n\", param[@\"name\"]];");
            w.l(2, "[body appendFormat:@\"%@\", param[@\"value\"]];");
            w.l(1, "}");
            w.l(1, "[body appendString:@\"\\r\\n\"];");
            w.l(0, "}");
            w.l(0, "[body appendFormat:@\"--%@--\\r\\n\", boundary];");
            w.l(0, "NSData *postData = [body dataUsingEncoding:NSUTF8StringEncoding];");
            w.l(0, "[request setHTTPBody:postData];");
        }
    }
    w.blank();
    w.l(0, format!("[request setHTTPMethod:{}];", oq(s.method)));
    w.blank();
    w.l(0, "NSURLSession *session = [NSURLSession sharedSession];");
    w.l(0, "NSURLSessionDataTask *dataTask = [session dataTaskWithRequest:request");
    w.l(0, "completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {");
    w.l(1, "if (error) {");
    w.l(2, "NSLog(@\"%@\", error);");
    w.l(1, "} else {");
    w.l(2, "NSLog(@\"%@\", [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding]);");
    w.l(1, "}");
    w.l(1, "dispatch_semaphore_signal(sema);");
    w.l(0, "}];");
    w.l(0, "[dataTask resume];");
    w.l(0, "dispatch_semaphore_wait(sema, DISPATCH_TIME_FOREVER);");
    w.finish()
}

fn ocaml(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "open Lwt");
    w.l(0, "open Cohttp");
    w.l(0, "open Cohttp_lwt_unix");
    w.blank();
    let mut extra_header = None;
    let body = match &s.body {
        SBody::None => None,
        SBody::Raw(_) | SBody::UrlEncoded(_) => Some(ocq(&s.text_body().unwrap_or_default())),
        SBody::File(p) => Some(format!("(* contents of {} *) \"\"", p.replace("*)", "* )"))),
        SBody::FormData(ps) => {
            let mut b = String::new();
            for p in ps {
                b.push_str(&format!("--{BOUNDARY}\r\n"));
                if p.is_file {
                    b.push_str(&format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\r\n<file contents here>\r\n", p.name, file_name(&p.value)));
                } else {
                    b.push_str(&format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n", p.name, p.value));
                }
            }
            b.push_str(&format!("--{BOUNDARY}--"));
            extra_header = Some(format!("multipart/form-data; boundary={BOUNDARY}"));
            Some(ocq(&b))
        }
    };
    if let Some(b) = &body {
        w.l(0, format!("let postData = ref {b};;"));
        w.blank();
    }
    w.l(0, "let reqBody =");
    w.l(1, format!("let uri = Uri.of_string {} in", ocq(&s.url)));
    w.l(1, "let headers = Header.init ()");
    for (k, v) in &s.headers {
        w.l(2, format!("|> fun h -> Header.add h {} {}", ocq(k), ocq(v)));
    }
    if let Some(ct) = extra_header {
        w.l(2, format!("|> fun h -> Header.add h \"Content-Type\" {}", ocq(&ct)));
    }
    w.l(1, "in");
    let method = format!("`{}", s.method);
    if body.is_some() {
        w.l(1, "let body = Cohttp_lwt.Body.of_string !postData in");
        w.blank();
        w.l(1, format!("Client.call ~headers ~body {method} uri >>= fun (_resp, body) ->"));
    } else {
        w.blank();
        w.l(1, format!("Client.call ~headers {method} uri >>= fun (_resp, body) ->"));
    }
    w.l(1, "body |> Cohttp_lwt.Body.to_string >|= fun body -> body");
    w.blank();
    w.l(0, "let () =");
    w.l(1, "let respBody = Lwt_main.run reqBody in");
    w.l(1, "print_endline (respBody)");
    w.finish()
}

fn php_curl(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "<?php");
    w.blank();
    w.l(0, "$curl = curl_init();");
    w.blank();
    w.l(0, "curl_setopt_array($curl, array(");
    let mut items = vec![
        format!("CURLOPT_URL => {}", psq(&s.url)),
        "CURLOPT_RETURNTRANSFER => true".into(),
        "CURLOPT_ENCODING => ''".into(),
        "CURLOPT_MAXREDIRS => 10".into(),
        "CURLOPT_TIMEOUT => 0".into(),
        format!("CURLOPT_FOLLOWLOCATION => {}", o.follow_redirects),
        "CURLOPT_HTTP_VERSION => CURL_HTTP_VERSION_1_1".into(),
        format!("CURLOPT_CUSTOMREQUEST => {}", psq(s.method)),
    ];
    match &s.body {
        SBody::None => {}
        SBody::Raw(_) | SBody::UrlEncoded(_) => items.push(format!("CURLOPT_POSTFIELDS => {}", psq(&s.text_body().unwrap_or_default()))),
        SBody::File(p) => items.push(format!("CURLOPT_POSTFIELDS => file_get_contents({})", psq(p))),
        SBody::FormData(ps) => {
            let fields: Vec<String> = ps
                .iter()
                .map(|p| if p.is_file { format!("{} => new CURLFILE({})", psq(&p.name), psq(&p.value)) } else { format!("{} => {}", psq(&p.name), psq(&p.value)) })
                .collect();
            items.push(format!("CURLOPT_POSTFIELDS => array({})", fields.join(",")));
        }
    }
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| psq(&format!("{k}: {v}")))
        .collect();
    for it in &items {
        w.l(1, format!("{it},"));
    }
    if !headers.is_empty() {
        w.l(1, "CURLOPT_HTTPHEADER => array(");
        w.list(2, &headers);
        w.l(1, "),");
    }
    w.l(0, "));");
    w.blank();
    w.l(0, "$response = curl_exec($curl);");
    w.blank();
    w.l(0, "curl_close($curl);");
    w.l(0, "echo $response;");
    w.finish()
}

fn php_guzzle(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "<?php");
    w.l(0, "use GuzzleHttp\\Client;");
    w.l(0, "use GuzzleHttp\\Psr7\\Request;");
    if matches!(&s.body, SBody::FormData(ps) if ps.iter().any(|p| p.is_file)) {
        w.l(0, "use GuzzleHttp\\Psr7\\Utils;");
    }
    w.blank();
    w.l(0, "$client = new Client();");
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_) | SBody::UrlEncoded(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{} => {}", psq(k), psq(v)))
        .collect();
    w.l(0, "$headers = [");
    w.list(1, &headers);
    w.l(0, "];");
    let mut options = false;
    let mut body = false;
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => {
            w.l(0, format!("$body = {};", psq(t)));
            body = true;
        }
        SBody::File(p) => {
            w.l(0, format!("$body = Utils::tryFopen({}, 'r');", psq(p)));
            body = true;
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "$options = [");
            w.l(1, "'form_params' => [");
            w.list(2, &pairs.iter().map(|(k, v)| format!("{} => {}", psq(k), psq(v))).collect::<Vec<_>>());
            w.l(1, "]];");
            options = true;
        }
        SBody::FormData(ps) => {
            w.l(0, "$options = [");
            w.l(1, "'multipart' => [");
            let items: Vec<String> = ps
                .iter()
                .map(|p| {
                    if p.is_file {
                        format!("[\n{u}{u}{u}'name' => {},\n{u}{u}{u}'contents' => Utils::tryFopen({}, 'r'),\n{u}{u}{u}'filename' => {}\n{u}{u}]", psq(&p.name), psq(&p.value), psq(&file_name(&p.value)), u = w.unit)
                    } else {
                        format!("[\n{u}{u}{u}'name' => {},\n{u}{u}{u}'contents' => {}\n{u}{u}]", psq(&p.name), psq(&p.value), u = w.unit)
                    }
                })
                .collect();
            w.list(2, &items);
            w.l(1, "]];");
            options = true;
        }
    }
    let b = if body { ", $body" } else { "" };
    w.l(0, format!("$request = new Request({}, {}, $headers{b});", psq(s.method), psq(&s.url)));
    if options {
        w.l(0, "$res = $client->sendAsync($request, $options)->wait();");
    } else {
        w.l(0, "$res = $client->sendAsync($request)->wait();");
    }
    w.l(0, "echo $res->getBody();");
    w.finish()
}

fn php_request2(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "<?php");
    w.l(0, "require_once 'HTTP/Request2.php';");
    w.l(0, "$request = new HTTP_Request2();");
    w.l(0, format!("$request->setUrl({});", psq(&s.url)));
    w.l(0, format!("$request->setMethod({});", psq(s.method)));
    w.l(0, "$request->setConfig(array(");
    w.l(1, format!("'follow_redirects' => {}", if o.follow_redirects { "TRUE" } else { "FALSE" }));
    w.l(0, "));");
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{} => {}", psq(k), psq(v)))
        .collect();
    if !headers.is_empty() {
        w.l(0, "$request->setHeader(array(");
        w.list(1, &headers);
        w.l(0, "));");
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => w.l(0, format!("$request->setBody({});", psq(t))),
        SBody::File(p) => w.l(0, format!("$request->setBody(file_get_contents({}));", psq(p))),
        SBody::UrlEncoded(pairs) => {
            w.l(0, "$request->addPostParameter(array(");
            w.list(1, &pairs.iter().map(|(k, v)| format!("{} => {}", psq(k), psq(v))).collect::<Vec<_>>());
            w.l(0, "));");
        }
        SBody::FormData(ps) => {
            let fields: Vec<String> = ps.iter().filter(|p| !p.is_file).map(|p| format!("{} => {}", psq(&p.name), psq(&p.value))).collect();
            if !fields.is_empty() {
                w.l(0, "$request->addPostParameter(array(");
                w.list(1, &fields);
                w.l(0, "));");
            }
            for p in ps.iter().filter(|p| p.is_file) {
                w.l(0, format!("$request->addUpload({}, {}, {}, 'application/octet-stream');", psq(&p.name), psq(&p.value), psq(&file_name(&p.value))));
            }
        }
    }
    w.l(0, "try {");
    w.l(1, "$response = $request->send();");
    w.l(1, "if ($response->getStatus() == 200) {");
    w.l(2, "echo $response->getBody();");
    w.l(1, "}");
    w.l(1, "else {");
    w.l(2, "echo 'Unexpected HTTP status: ' . $response->getStatus() . ' ' .");
    w.l(2, "$response->getReasonPhrase();");
    w.l(1, "}");
    w.l(0, "}");
    w.l(0, "catch(HTTP_Request2_Exception $e) {");
    w.l(1, "echo 'Error: ' . $e->getMessage();");
    w.l(0, "}");
    w.finish()
}

fn php_pecl(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "<?php");
    w.l(0, "$client = new http\\Client;");
    w.l(0, "$request = new http\\Client\\Request;");
    w.l(0, format!("$request->setRequestUrl({});", psq(&s.url)));
    w.l(0, format!("$request->setRequestMethod({});", psq(s.method)));
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => {
            w.l(0, "$body = new http\\Message\\Body;");
            w.l(0, format!("$body->append({});", psq(t)));
            w.l(0, "$request->setBody($body);");
        }
        SBody::File(p) => {
            w.l(0, "$body = new http\\Message\\Body;");
            w.l(0, format!("$body->append(file_get_contents({}));", psq(p)));
            w.l(0, "$request->setBody($body);");
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "$body = new http\\Message\\Body;");
            w.l(0, "$body->append(new http\\QueryString(array(");
            w.list(1, &pairs.iter().map(|(k, v)| format!("{} => {}", psq(k), psq(v))).collect::<Vec<_>>());
            w.l(0, ")));");
            w.l(0, "$request->setBody($body);");
        }
        SBody::FormData(ps) => {
            w.l(0, "$body = new http\\Message\\Body;");
            let fields: Vec<String> = ps.iter().filter(|p| !p.is_file).map(|p| format!("{} => {}", psq(&p.name), psq(&p.value))).collect();
            let files: Vec<String> = ps
                .iter()
                .filter(|p| p.is_file)
                .map(|p| format!("array('name' => {}, 'type' => 'application/octet-stream', 'file' => {}, 'data' => null)", psq(&p.name), psq(&p.value)))
                .collect();
            w.l(0, format!("$body->addForm(array({}), array({}));", fields.join(", "), files.join(", ")));
            w.l(0, "$request->setBody($body);");
        }
    }
    w.l(0, "$request->setOptions(array());");
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{} => {}", psq(k), psq(v)))
        .collect();
    w.l(0, "$request->setHeaders(array(");
    w.list(1, &headers);
    w.l(0, "));");
    w.l(0, "$client->enqueue($request)->send();");
    w.l(0, "$response = $client->getResponse();");
    w.l(0, "echo $response->getBody();");
    w.finish()
}

fn powershell(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    let multipart = matches!(s.body, SBody::FormData(_));
    let headers: Vec<&(String, String)> = s.headers.iter().filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type"))).collect();
    if !headers.is_empty() {
        w.l(0, "$headers = New-Object \"System.Collections.Generic.Dictionary[[String],[String]]\"");
        for (k, v) in &headers {
            w.l(0, format!("$headers.Add({}, {})", pwsh(k), pwsh(v)));
        }
        w.blank();
    }
    let body = match &s.body {
        SBody::None => false,
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            w.l(0, format!("$body = {}", pwsh(&s.text_body().unwrap_or_default())));
            true
        }
        SBody::File(p) => {
            w.l(0, format!("$body = [System.IO.File]::ReadAllBytes({})", pwsh(p)));
            true
        }
        SBody::FormData(ps) => {
            w.l(0, "$multipartContent = [System.Net.Http.MultipartFormDataContent]::new()");
            for p in ps {
                if p.is_file {
                    w.l(0, format!("$multipartFile = {}", pwsh(&p.value)));
                    w.l(0, "$FileStream = [System.IO.FileStream]::new($multipartFile, [System.IO.FileMode]::Open)");
                    w.l(0, "$fileHeader = [System.Net.Http.Headers.ContentDispositionHeaderValue]::new(\"form-data\")");
                    w.l(0, format!("$fileHeader.Name = {}", pwsh(&p.name)));
                    w.l(0, format!("$fileHeader.FileName = {}", pwsh(&file_name(&p.value))));
                    w.l(0, "$fileContent = [System.Net.Http.StreamContent]::new($FileStream)");
                    w.l(0, "$fileContent.Headers.ContentDisposition = $fileHeader");
                    w.l(0, "$multipartContent.Add($fileContent)");
                } else {
                    w.l(0, "$stringHeader = [System.Net.Http.Headers.ContentDispositionHeaderValue]::new(\"form-data\")");
                    w.l(0, format!("$stringHeader.Name = {}", pwsh(&p.name)));
                    w.l(0, format!("$stringContent = [System.Net.Http.StringContent]::new({})", pwsh(&p.value)));
                    w.l(0, "$stringContent.Headers.ContentDisposition = $stringHeader");
                    w.l(0, "$multipartContent.Add($stringContent)");
                }
                w.blank();
            }
            w.l(0, "$body = $multipartContent");
            true
        }
    };
    if body {
        w.blank();
    }
    let mut cmd = format!("$response = Invoke-RestMethod {} -Method {}", pwsh(&s.url), pwsh(s.method));
    if !headers.is_empty() {
        cmd.push_str(" -Headers $headers");
    }
    if body {
        cmd.push_str(" -Body $body");
    }
    w.l(0, cmd);
    w.l(0, "$response | ConvertTo-Json");
    w.finish()
}

fn r_httr(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "library(httr)");
    w.blank();
    let multipart = matches!(s.body, SBody::FormData(_));
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{} = {}", q(k), q(v)))
        .collect();
    if !headers.is_empty() {
        w.l(0, "headers = c(");
        w.list(1, &headers);
        w.l(0, ")");
        w.blank();
    }
    let mut encode = None;
    let body = match &s.body {
        SBody::None => false,
        SBody::Raw(t) => {
            w.l(0, format!("body = {};", q(t)));
            true
        }
        SBody::File(p) => {
            w.l(0, format!("body = upload_file({});", q(p)));
            true
        }
        SBody::UrlEncoded(pairs) => {
            w.l(0, "body = list(");
            w.list(1, &pairs.iter().map(|(k, v)| format!("{} = {}", q(k), q(v))).collect::<Vec<_>>());
            w.l(0, ")");
            encode = Some("form");
            true
        }
        SBody::FormData(ps) => {
            w.l(0, "body = list(");
            let items: Vec<String> = ps
                .iter()
                .map(|p| if p.is_file { format!("{} = upload_file({})", q(&p.name), q(&p.value)) } else { format!("{} = {}", q(&p.name), q(&p.value)) })
                .collect();
            w.list(1, &items);
            w.l(0, ")");
            encode = Some("multipart");
            true
        }
    };
    if body {
        w.blank();
    }
    let mut call = format!("res <- VERB({}, url = {}", q(s.method), q(&s.url));
    if body {
        call.push_str(", body = body");
    }
    if !headers.is_empty() {
        call.push_str(", add_headers(headers)");
    }
    if let Some(e) = encode {
        call.push_str(&format!(", encode = {}", q(e)));
    }
    call.push(')');
    w.l(0, call);
    w.blank();
    w.l(0, "cat(content(res, 'text'))");
    w.finish()
}

fn r_rcurl(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "library(RCurl)");
    let multipart = matches!(s.body, SBody::FormData(_));
    let headers: Vec<String> = s
        .headers
        .iter()
        .filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type")))
        .map(|(k, v)| format!("{} = {}", q(k), q(v)))
        .collect();
    if !headers.is_empty() {
        w.l(0, "headers = c(");
        w.list(1, &headers);
        w.l(0, ")");
    }
    let follow = if o.follow_redirects { "TRUE" } else { "FALSE" };
    let hdr_opt = if headers.is_empty() { String::new() } else { "httpheader = headers, ".into() };
    match &s.body {
        SBody::FormData(ps) => {
            let fields: Vec<String> = ps
                .iter()
                .map(|p| if p.is_file { format!("{} = fileUpload({})", q(&p.name), q(&p.value)) } else { format!("{} = {}", q(&p.name), q(&p.value)) })
                .collect();
            w.l(0, format!("res <- postForm({}, {}, .opts=list({hdr_opt}followlocation = {follow}), style = \"httppost\")", q(&s.url), fields.join(", ")));
        }
        SBody::None => {
            let custom = if s.method == "GET" { String::new() } else { format!("customrequest = {}, ", q(s.method)) };
            w.l(0, format!("res <- getURL({}, .opts=list({custom}{hdr_opt}followlocation = {follow}))", q(&s.url)));
        }
        body => {
            let params = match body {
                SBody::File(p) => format!("readBin({p}, \"raw\", file.info({p})$size)", p = q(p)),
                _ => q(&s.text_body().unwrap_or_default()),
            };
            w.l(0, format!("params = {params}"));
            w.l(0, format!("res <- getURL({}, .opts=list(customrequest = {}, postfields = params, {hdr_opt}followlocation = {follow}))", q(&s.url), q(s.method)));
        }
    }
    w.l(0, "cat(res)");
    w.finish()
}

fn ruby(s: &Snippet, o: &Options) -> String {
    let u = url_parts(&s.url);
    let mut w = W::new(o);
    w.l(0, "require \"uri\"");
    w.l(0, "require \"net/http\"");
    w.blank();
    w.l(0, format!("url = URI({})", rb(&s.url)));
    w.blank();
    let var = if u.https { "https" } else { "http" };
    w.l(0, format!("{var} = Net::HTTP.new(url.host, url.port)"));
    if u.https {
        w.l(0, "https.use_ssl = true");
    }
    w.blank();
    let cls = match s.method {
        "GET" => "Get",
        "POST" => "Post",
        "PUT" => "Put",
        "PATCH" => "Patch",
        "DELETE" => "Delete",
        "HEAD" => "Head",
        _ => "Options",
    };
    w.l(0, format!("request = Net::HTTP::{cls}.new(url)"));
    for (k, v) in s.headers.iter().filter(|(k, _)| !(matches!(s.body, SBody::FormData(_)) && k.eq_ignore_ascii_case("content-type"))) {
        w.l(0, format!("request[{}] = {}", rb(k), rb(v)));
    }
    match &s.body {
        SBody::None => {}
        SBody::Raw(t) => w.l(0, format!("request.body = {}", rb(t))),
        SBody::File(p) => w.l(0, format!("request.body = File.binread({})", rb(p))),
        SBody::UrlEncoded(pairs) => {
            let items: Vec<String> = pairs.iter().map(|(k, v)| format!("[{}, {}]", rb(k), rb(v))).collect();
            w.l(0, format!("request.body = URI.encode_www_form([{}])", items.join(", ")));
        }
        SBody::FormData(ps) => {
            let items: Vec<String> = ps.iter().map(|p| if p.is_file { format!("[{}, File.open({})]", rb(&p.name), rb(&p.value)) } else { format!("[{}, {}]", rb(&p.name), rb(&p.value)) }).collect();
            w.l(0, format!("form_data = [{}]", items.join(",")));
            w.l(0, "request.set_form form_data, 'multipart/form-data'");
        }
    }
    w.blank();
    w.l(0, format!("response = {var}.request(request)"));
    w.l(0, "puts response.read_body");
    w.finish()
}

fn rust_reqwest(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    w.l(0, "#[tokio::main]");
    w.l(0, "async fn main() -> Result<(), Box<dyn std::error::Error>> {");
    w.l(1, "let client = reqwest::Client::builder()");
    if !o.follow_redirects {
        w.l(2, ".redirect(reqwest::redirect::Policy::none())");
    }
    w.l(2, ".build()?;");
    w.blank();
    let multipart = matches!(s.body, SBody::FormData(_));
    let headers: Vec<&(String, String)> = s.headers.iter().filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type"))).collect();
    if !headers.is_empty() {
        w.l(1, "let mut headers = reqwest::header::HeaderMap::new();");
        for (k, v) in &headers {
            w.l(1, format!("headers.insert({}, {}.parse()?);", rsq(&k.to_ascii_lowercase()), rsq(v)));
        }
        w.blank();
    }
    let body_call = match &s.body {
        SBody::None => None,
        SBody::Raw(t) => {
            w.l(1, format!("let data = {};", rsq(t)));
            w.blank();
            Some(".body(data)")
        }
        SBody::File(p) => {
            w.l(1, format!("let data = std::fs::read({})?;", rsq(p)));
            w.blank();
            Some(".body(data)")
        }
        SBody::UrlEncoded(pairs) => {
            w.l(1, "let params = [");
            w.list(2, &pairs.iter().map(|(k, v)| format!("({}, {})", rsq(k), rsq(v))).collect::<Vec<_>>());
            w.l(1, "];");
            w.blank();
            Some(".form(&params)")
        }
        SBody::FormData(ps) => {
            w.l(1, "let form = reqwest::multipart::Form::new()");
            let last = ps.len().saturating_sub(1);
            for (i, p) in ps.iter().enumerate() {
                let end = if i == last { ";" } else { "" };
                if p.is_file {
                    w.l(2, format!(".file({}, {}).await?{end}", rsq(&p.name), rsq(&p.value)));
                } else {
                    w.l(2, format!(".text({}, {}){end}", rsq(&p.name), rsq(&p.value)));
                }
            }
            if ps.is_empty() {
                w.l(2, ";");
            }
            w.blank();
            Some(".multipart(form)")
        }
    };
    w.l(1, format!("let request = client.request(reqwest::Method::from_bytes(b{})?, {})", rsq(s.method), rsq(&s.url)));
    let mut chain: Vec<&str> = Vec::new();
    if !headers.is_empty() {
        chain.push(".headers(headers)");
    }
    if let Some(b) = body_call {
        chain.push(b);
    }
    if chain.is_empty() {
        let last = w.lines.pop().unwrap_or_default();
        w.lines.push(format!("{last};"));
    } else {
        let n = chain.len();
        for (i, c) in chain.into_iter().enumerate() {
            w.l(2, format!("{c}{}", if i + 1 == n { ";" } else { "" }));
        }
    }
    w.blank();
    w.l(1, "let response = request.send().await?;");
    w.l(1, "let body = response.text().await?;");
    w.blank();
    w.l(1, "println!(\"{}\", body);");
    w.blank();
    w.l(1, "Ok(())");
    w.l(0, "}");
    w.finish()
}

fn swift(s: &Snippet, o: &Options) -> String {
    let mut w = W::new(o);
    let multipart = matches!(s.body, SBody::FormData(_));
    match &s.body {
        SBody::None => {}
        SBody::Raw(_) | SBody::UrlEncoded(_) => {
            w.l(0, format!("let parameters = {}", swq(&s.text_body().unwrap_or_default())));
            w.l(0, "let postData = parameters.data(using: .utf8)");
            w.blank();
        }
        SBody::File(p) => {
            w.l(0, format!("let postData = try? Data(contentsOf: URL(fileURLWithPath: {}))", swq(p)));
            w.blank();
        }
        SBody::FormData(ps) => {
            w.l(0, "let parameters = [");
            let items: Vec<String> = ps
                .iter()
                .map(|p| {
                    if p.is_file {
                        format!("[\n{u}{u}\"key\": {},\n{u}{u}\"src\": {},\n{u}{u}\"type\": \"file\"\n{u}]", swq(&p.name), swq(&p.value), u = w.unit)
                    } else {
                        format!("[\n{u}{u}\"key\": {},\n{u}{u}\"value\": {},\n{u}{u}\"type\": \"text\"\n{u}]", swq(&p.name), swq(&p.value), u = w.unit)
                    }
                })
                .collect();
            w.list(1, &items);
            w.l(0, "] as [[String: Any]]");
            w.blank();
            w.l(0, "let boundary = \"Boundary-\\(UUID().uuidString)\"");
            w.l(0, "var body = Data()");
            w.l(0, "for param in parameters {");
            w.l(1, "let paramName = param[\"key\"]!");
            w.l(1, "body += Data(\"--\\(boundary)\\r\\n\".utf8)");
            w.l(1, "body += Data(\"Content-Disposition:form-data; name=\\\"\\(paramName)\\\"\".utf8)");
            w.l(1, "if param[\"type\"] as! String == \"text\" {");
            w.l(2, "let paramValue = param[\"value\"] as! String");
            w.l(2, "body += Data(\"\\r\\n\\r\\n\\(paramValue)\\r\\n\".utf8)");
            w.l(1, "} else {");
            w.l(2, "let paramSrc = param[\"src\"] as! String");
            w.l(2, "let fileURL = URL(fileURLWithPath: paramSrc)");
            w.l(2, "if let fileContent = try? Data(contentsOf: fileURL) {");
            w.l(3, "body += Data(\"; filename=\\\"\\(fileURL.lastPathComponent)\\\"\\r\\n\".utf8)");
            w.l(3, "body += Data(\"Content-Type: application/octet-stream\\r\\n\\r\\n\".utf8)");
            w.l(3, "body += fileContent");
            w.l(3, "body += Data(\"\\r\\n\".utf8)");
            w.l(2, "}");
            w.l(1, "}");
            w.l(0, "}");
            w.l(0, "body += Data(\"--\\(boundary)--\\r\\n\".utf8);");
            w.l(0, "let postData = body");
            w.blank();
        }
    }
    w.l(0, format!("var request = URLRequest(url: URL(string: {})!,timeoutInterval: Double.infinity)", swq(&s.url)));
    for (k, v) in s.headers.iter().filter(|(k, _)| !(multipart && k.eq_ignore_ascii_case("content-type"))) {
        w.l(0, format!("request.addValue({}, forHTTPHeaderField: {})", swq(v), swq(k)));
    }
    if multipart {
        w.l(0, "request.addValue(\"multipart/form-data; boundary=\\(boundary)\", forHTTPHeaderField: \"Content-Type\")");
    }
    w.blank();
    w.l(0, format!("request.httpMethod = {}", swq(s.method)));
    if s.has_body() {
        w.l(0, "request.httpBody = postData");
    }
    w.blank();
    w.l(0, "let task = URLSession.shared.dataTask(with: request) { data, response, error in");
    w.l(1, "guard let data = data else {");
    w.l(2, "print(String(describing: error))");
    w.l(2, "return");
    w.l(1, "}");
    w.l(1, "print(String(data: data, encoding: .utf8)!)");
    w.l(0, "}");
    w.blank();
    w.l(0, "task.resume()");
    w.finish()
}

// ---------------------------------------------------------------------------
// Syntax highlighting (token classes; colors are picked by the UI)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Type,
    Property,
    Variable,
    Punct,
}

fn keywords(syn: Syntax) -> &'static [&'static str] {
    match syn {
        Syntax::Shell => &["curl", "http", "wget", "printf"],
        Syntax::Http => &[],
        Syntax::C | Syntax::ObjC => &["if", "else", "for", "in", "return", "struct", "const", "char", "void", "int", "NULL", "nil", "long"],
        Syntax::CSharp => &["var", "new", "await", "null", "true", "false", "using", "string", "return"],
        Syntax::Dart => &["var", "await", "if", "else", "null", "true", "false", "final"],
        Syntax::Go => &["package", "import", "func", "return", "if", "nil", "defer", "var", "err"],
        Syntax::Java => &["new", "null", "true", "false", "final"],
        Syntax::Js => &[
            "const", "let", "var", "new", "function", "require", "if", "else", "throw", "true", "false", "null", "undefined", "Infinity", "this",
            "return", "for", "of", "in", "while", "do", "try", "catch", "finally", "async", "await", "typeof", "instanceof", "break", "continue",
            "switch", "case", "default", "class", "delete",
        ],
        Syntax::Kotlin => &["val", "var", "null", "true", "false"],
        Syntax::OCaml => &["open", "let", "in", "fun", "ref"],
        Syntax::Php => &["new", "echo", "use", "array", "true", "false", "null", "try", "catch", "if", "else", "require_once", "TRUE", "FALSE"],
        Syntax::PowerShell => &["New-Object", "Invoke-RestMethod", "ConvertTo-Json"],
        Syntax::Python => &["import", "from", "as", "with", "print", "open", "True", "False", "None"],
        Syntax::R => &["library", "TRUE", "FALSE", "c", "list"],
        Syntax::Ruby => &["require", "puts", "true", "false", "nil"],
        Syntax::Rust => &["async", "fn", "let", "mut", "await", "Ok", "Box", "dyn", "b"],
        Syntax::Swift => &["let", "var", "for", "in", "if", "else", "guard", "return", "try", "as"],
    }
}

/// Splits a line into highlighted byte ranges.
pub fn tokenize(line: &str, syn: Syntax) -> Vec<(usize, usize, Tok)> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let kw = keywords(syn);
    let hash_comment = matches!(syn, Syntax::Shell | Syntax::Python | Syntax::R | Syntax::Ruby | Syntax::PowerShell);
    let slash_comment = !matches!(syn, Syntax::Shell | Syntax::Python | Syntax::R | Syntax::Ruby | Syntax::PowerShell | Syntax::Http | Syntax::OCaml);

    if syn == Syntax::Http {
        // `Header-Name: value` lines and the request line.
        if let Some(colon) = line.find(':')
            && (!line.starts_with(' ') && !line.contains(' ') || line[..colon].bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')) {
                out.push((0, colon, Tok::Property));
                out.push((colon, line.len(), Tok::String));
                return out;
            }
        if let Some(sp) = line.find(' ')
            && line.ends_with("HTTP/1.1") {
                out.push((0, sp, Tok::Keyword));
                out.push((sp, line.len(), Tok::Plain));
                return out;
            }
        out.push((0, line.len(), Tok::Plain));
        return out;
    }

    while i < b.len() {
        let c = b[i];
        let start = i;
        // Comments.
        if (hash_comment && c == b'#' && (i == 0 || b[i - 1] == b' ')) || (slash_comment && c == b'/' && b.get(i + 1) == Some(&b'/')) || (syn == Syntax::OCaml && c == b'(' && b.get(i + 1) == Some(&b'*')) {
            if syn == Syntax::Rust && b.get(i + 1) == Some(&b'[') {
                // #[attribute]
            } else {
                out.push((i, b.len(), Tok::Comment));
                break;
            }
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            // Strings (with backslash escapes, except shell single quotes).
            let quote = c;
            i += 1;
            while i < b.len() && b[i] != quote {
                if b[i] == b'\\' && !(syn == Syntax::Shell && quote == b'\'') {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            while i < b.len() && !line.is_char_boundary(i) {
                i += 1;
            }
            // `"key": value` → property.
            let mut j = i;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            let is_key = matches!(syn, Syntax::Js | Syntax::Python | Syntax::R | Syntax::Dart | Syntax::Swift | Syntax::Php | Syntax::ObjC) && j < b.len() && (b[j] == b':' || (b[j] == b'=' && b.get(j + 1) == Some(&b'>')));
            out.push((start, i, if is_key { Tok::Property } else { Tok::String }));
            continue;
        }
        if c == b'@' && syn == Syntax::ObjC && b.get(i + 1) == Some(&b'"') {
            out.push((i, i + 1, Tok::String));
            i += 1;
            continue;
        }
        if c == b'$' && matches!(syn, Syntax::Php | Syntax::PowerShell) {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((start, i, Tok::Variable));
            continue;
        }
        if c == b'-' && syn == Syntax::Shell && (i == 0 || b[i - 1] == b' ') {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'=') {
                i += 1;
            }
            out.push((start, i, Tok::Property));
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            out.push((start, i, Tok::Number));
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || (syn == Syntax::PowerShell && b[i] == b'-')) {
                i += 1;
            }
            let word = &line[start..i];
            let next = b.get(i).copied();
            let tok = if kw.contains(&word) {
                Tok::Keyword
            } else if next == Some(b'(') || (syn == Syntax::ObjC && next == Some(b':')) {
                Tok::Function
            } else if word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                Tok::Type
            } else {
                Tok::Plain
            };
            out.push((start, i, tok));
            continue;
        }
        // Punctuation / whitespace / non-ASCII run.
        i += 1;
        while i < b.len() && !line.is_char_boundary(i) {
            i += 1;
        }
        let tok = if c.is_ascii_punctuation() { Tok::Punct } else { Tok::Plain };
        out.push((start, i, tok));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FormField, FormFieldKind, KeyValue, Method, RawLang};

    fn sample(mode: BodyMode) -> Snippet {
        let url = std::env::var("RUSTMAN_SNIPPET_URL").unwrap_or_else(|_| "https://api.example.com:8443/v1/users?page=1".into());
        let mut req = Request { url, method: Method::Post, ..Default::default() };
        req.headers = vec![KeyValue::new("X-Note", "it's \"quoted\" $HOME #{x}")];
        req.body.mode = mode;
        req.body.raw_lang = RawLang::Json;
        req.body.raw = "{\n  \"name\": \"Nhâm\"\n}".into();
        req.body.urlencoded = vec![KeyValue::new("a", "1 2"), KeyValue::new("b", "x&y")];
        req.body.form_data = vec![
            FormField { key: "name".into(), value: "v".into(), ..Default::default() },
            FormField { key: "file".into(), value: std::env::var("RUSTMAN_SNIPPET_FILE").unwrap_or_else(|_| "/tmp/a b.png".into()), kind: FormFieldKind::File, ..Default::default() },
        ];
        build(&req, &VarMap::new(), None).unwrap()
    }

    #[test]
    fn every_language_generates_for_every_body() {
        for mode in [BodyMode::None, BodyMode::Raw, BodyMode::UrlEncoded, BodyMode::FormData, BodyMode::Binary] {
            let s = sample(mode);
            for lang in Lang::ALL {
                let code = generate(&s, &Options { lang, ..Default::default() });
                assert!(!code.trim().is_empty(), "{lang:?} {mode:?}");
                assert!(code.contains("api.example.com"), "{lang:?} {mode:?} lost the host:\n{code}");
            }
        }
    }

    /// `RUSTMAN_SNIPPET_DIR=dir cargo test dump_snippets` writes every snippet to disk for syntax checks.
    #[test]
    fn dump_snippets() {
        let Ok(dir) = std::env::var("RUSTMAN_SNIPPET_DIR") else { return };
        for (mode, tag) in [(BodyMode::None, "none"), (BodyMode::Raw, "raw"), (BodyMode::UrlEncoded, "urlenc"), (BodyMode::FormData, "form"), (BodyMode::Binary, "file")] {
            let s = sample(mode);
            for lang in Lang::ALL {
                let code = generate(&s, &Options { lang, ..Default::default() });
                let name = format!("{dir}/{lang:?}_{tag}.txt");
                std::fs::write(name, code).unwrap();
            }
        }
    }

    #[test]
    fn curl_matches_postman_style() {
        let s = sample(BodyMode::UrlEncoded);
        let code = generate(&s, &Options::default());
        assert!(code.starts_with("curl --location 'https://api.example.com:8443/v1/users?page=1' \\\n--header"), "{code}");
        assert!(code.contains("--data-urlencode 'a=1 2'"));
        let short = generate(&s, &Options { curl_long: false, curl_continuation: '^', ..Default::default() });
        assert!(short.contains(" ^\n-H "), "{short}");
    }

    #[test]
    fn indentation_options() {
        let s = sample(BodyMode::Raw);
        let tabs = generate(&s, &Options { lang: Lang::JsFetch, tabs: true, ..Default::default() });
        assert!(tabs.contains("\n\tmethod: \"POST\""), "{tabs}");
        let four = generate(&s, &Options { lang: Lang::JsFetch, indent: 4, ..Default::default() });
        assert!(four.contains("\n    method: \"POST\""), "{four}");
    }

    #[test]
    fn tokenizer_classifies() {
        let t = tokenize("curl --location 'https://x' \\", Syntax::Shell);
        assert_eq!(t[0].2, Tok::Keyword);
        assert!(t.iter().any(|(_, _, k)| *k == Tok::Property));
        assert!(t.iter().any(|(_, _, k)| *k == Tok::String));
        let j = tokenize("myHeaders.append(\"a\", \"b\"); // note", Syntax::Js);
        assert!(j.iter().any(|(_, _, k)| *k == Tok::Function));
        assert_eq!(j.last().unwrap().2, Tok::Comment);
        // Tokens cover the whole line.
        let line = "  let x = \"Nhâm\";";
        let toks = tokenize(line, Syntax::Rust);
        assert_eq!(toks.first().unwrap().0, 0);
        assert_eq!(toks.last().unwrap().1, line.len());
    }
}
