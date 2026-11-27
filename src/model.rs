//! Data model: requests, collections, environments, history.
//! Everything here is serializable so the whole workspace can be persisted as JSON.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    pub fn parse(s: &str) -> Option<Method> {
        let up = s.trim().to_ascii_uppercase();
        Method::ALL.into_iter().find(|m| m.as_str() == up)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyValue {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub description: String,
}

impl Default for KeyValue {
    fn default() -> Self {
        Self { enabled: true, key: String::new(), value: String::new(), description: String::new() }
    }
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), ..Default::default() }
    }
    pub fn is_blank(&self) -> bool {
        self.key.is_empty() && self.value.is_empty() && self.description.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RawLang {
    #[default]
    Json,
    Text,
    Xml,
    Html,
    JavaScript,
}

impl RawLang {
    pub const ALL: [RawLang; 5] = [RawLang::Json, RawLang::Text, RawLang::Xml, RawLang::Html, RawLang::JavaScript];
    pub fn label(self) -> &'static str {
        match self {
            RawLang::Json => "JSON",
            RawLang::Text => "Text",
            RawLang::Xml => "XML",
            RawLang::Html => "HTML",
            RawLang::JavaScript => "JavaScript",
        }
    }
    pub fn content_type(self) -> &'static str {
        match self {
            RawLang::Json => "application/json",
            RawLang::Text => "text/plain",
            RawLang::Xml => "application/xml",
            RawLang::Html => "text/html",
            RawLang::JavaScript => "application/javascript",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FormFieldKind {
    #[default]
    Text,
    File,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormField {
    pub enabled: bool,
    pub key: String,
    /// Text value, or the file path when `kind == File`.
    pub value: String,
    pub kind: FormFieldKind,
}

impl Default for FormField {
    fn default() -> Self {
        Self { enabled: true, key: String::new(), value: String::new(), kind: FormFieldKind::Text }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BodyMode {
    #[default]
    None,
    Raw,
    UrlEncoded,
    FormData,
    Binary,
    GraphQl,
}

impl BodyMode {
    pub const ALL: [BodyMode; 6] =
        [BodyMode::None, BodyMode::FormData, BodyMode::UrlEncoded, BodyMode::Raw, BodyMode::Binary, BodyMode::GraphQl];
    pub fn label(self) -> &'static str {
        match self {
            BodyMode::None => "none",
            BodyMode::Raw => "raw",
            BodyMode::UrlEncoded => "x-www-form-urlencoded",
            BodyMode::FormData => "form-data",
            BodyMode::Binary => "binary",
            BodyMode::GraphQl => "GraphQL",
        }
    }
}

/// Body keeps the data of every mode so switching modes does not lose input (like Postman).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Body {
    pub mode: BodyMode,
    pub raw: String,
    pub raw_lang: RawLang,
    pub urlencoded: Vec<KeyValue>,
    pub form_data: Vec<FormField>,
    pub binary_path: String,
    pub graphql_query: String,
    pub graphql_variables: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthKind {
    /// Use the auth of the parent collection.
    #[default]
    Inherit,
    None,
    Bearer,
    Basic,
    ApiKey,
    OAuth2,
    /// AWS Signature Version 4.
    AwsSigV4,
    /// HTTP Digest (RFC 7616): answers the server's 401 challenge.
    Digest,
}

impl AuthKind {
    pub fn label(self) -> &'static str {
        match self {
            AuthKind::Inherit => "Inherit auth from parent",
            AuthKind::None => "No Auth",
            AuthKind::Bearer => "Bearer Token",
            AuthKind::Basic => "Basic Auth",
            AuthKind::ApiKey => "API Key",
            AuthKind::OAuth2 => "OAuth 2.0",
            AuthKind::AwsSigV4 => "AWS Signature",
            AuthKind::Digest => "Digest Auth",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Auth {
    pub kind: AuthKind,
    pub bearer_token: String,
    pub basic_username: String,
    pub basic_password: String,
    pub api_key_name: String,
    pub api_key_value: String,
    pub api_key_in: ApiKeyLocation,
    #[serde(skip_serializing_if = "OAuth2::is_default")]
    pub oauth2: Box<OAuth2>,
    pub aws_access_key: String,
    pub aws_secret_key: String,
    /// Empty: `us-east-1`.
    pub aws_region: String,
    /// `execute-api`, `s3`, `sqs`, ...
    pub aws_service: String,
    pub aws_session_token: String,
    pub digest_username: String,
    pub digest_password: String,
}

impl Auth {
    pub fn none() -> Self {
        Self { kind: AuthKind::None, ..Default::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GrantType {
    #[default]
    AuthorizationCode,
    AuthorizationCodePkce,
    Implicit,
    Password,
    ClientCredentials,
}

impl GrantType {
    pub const ALL: [GrantType; 5] =
        [GrantType::AuthorizationCode, GrantType::AuthorizationCodePkce, GrantType::Implicit, GrantType::Password, GrantType::ClientCredentials];
    pub fn label(self) -> &'static str {
        match self {
            GrantType::AuthorizationCode => "Authorization Code",
            GrantType::AuthorizationCodePkce => "Authorization Code (With PKCE)",
            GrantType::Implicit => "Implicit",
            GrantType::Password => "Password Credentials",
            GrantType::ClientCredentials => "Client Credentials",
        }
    }
    /// The user signs in through a browser.
    pub fn uses_browser(self) -> bool {
        matches!(self, GrantType::AuthorizationCode | GrantType::AuthorizationCodePkce | GrantType::Implicit)
    }
}

/// Where the access token goes in the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TokenPlacement {
    #[default]
    Header,
    Query,
}

/// How the client id/secret are sent to the token endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ClientAuth {
    #[default]
    BasicHeader,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PkceMethod {
    #[default]
    S256,
    Plain,
}

/// OAuth 2.0: the current token plus the configuration used to get a new one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OAuth2 {
    // Current token
    pub access_token: String,
    pub header_prefix: String,
    pub add_to: TokenPlacement,
    pub refresh_token: String,
    pub id_token: String,
    /// Unix seconds; `None` when the server did not say.
    pub expires_at: Option<i64>,
    /// Scope granted by the server.
    pub granted_scope: String,

    // Configure new token
    pub token_name: String,
    pub grant_type: GrantType,
    pub callback_url: String,
    pub auth_url: String,
    pub access_token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    pub state: String,
    pub username: String,
    pub password: String,
    pub pkce_method: PkceMethod,
    /// Empty: generated for each authorization.
    pub code_verifier: String,
    pub client_auth: ClientAuth,
    /// Empty: the access token URL.
    pub refresh_token_url: String,
    pub audience: String,
    pub resource: String,
    /// Refresh an expired token before sending, when a refresh token is available.
    pub auto_refresh: bool,
}

pub const DEFAULT_CALLBACK_URL: &str = "http://localhost:5005/callback";

impl Default for OAuth2 {
    fn default() -> Self {
        Self {
            access_token: String::new(),
            header_prefix: "Bearer".into(),
            add_to: TokenPlacement::Header,
            refresh_token: String::new(),
            id_token: String::new(),
            expires_at: None,
            granted_scope: String::new(),
            token_name: String::new(),
            grant_type: GrantType::AuthorizationCode,
            callback_url: DEFAULT_CALLBACK_URL.into(),
            auth_url: String::new(),
            access_token_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            scope: String::new(),
            state: String::new(),
            username: String::new(),
            password: String::new(),
            pkce_method: PkceMethod::S256,
            code_verifier: String::new(),
            client_auth: ClientAuth::BasicHeader,
            refresh_token_url: String::new(),
            audience: String::new(),
            resource: String::new(),
            auto_refresh: true,
        }
    }
}

/// A token returned by the authorization server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OAuthToken {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_at: Option<i64>,
    pub scope: String,
}

impl OAuth2 {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    /// Stores a new token. A refresh response without a refresh token keeps the old one.
    pub fn set_token(&mut self, t: &OAuthToken) {
        self.access_token = t.access_token.clone();
        if !t.refresh_token.is_empty() {
            self.refresh_token = t.refresh_token.clone();
        }
        if !t.id_token.is_empty() {
            self.id_token = t.id_token.clone();
        }
        self.expires_at = t.expires_at;
        self.granted_scope = t.scope.clone();
    }
    pub fn clear_token(&mut self) {
        self.access_token.clear();
        self.refresh_token.clear();
        self.id_token.clear();
        self.expires_at = None;
        self.granted_scope.clear();
    }
    /// Expired (with a few seconds of margin so it does not expire in flight).
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|t| t <= now + 10)
    }
    pub fn should_auto_refresh(&self, now: i64) -> bool {
        self.auto_refresh && !self.refresh_token.is_empty() && self.is_expired(now)
    }
    /// Every field with `{{variables}}` substituted.
    pub fn resolved(&self, vars: &crate::vars::VarMap) -> Self {
        let s = |x: &String| crate::vars::substitute(x, vars);
        Self {
            access_token: s(&self.access_token),
            header_prefix: s(&self.header_prefix),
            refresh_token: s(&self.refresh_token),
            callback_url: s(&self.callback_url),
            auth_url: s(&self.auth_url),
            access_token_url: s(&self.access_token_url),
            client_id: s(&self.client_id),
            client_secret: s(&self.client_secret),
            scope: s(&self.scope),
            state: s(&self.state),
            username: s(&self.username),
            password: s(&self.password),
            code_verifier: s(&self.code_verifier),
            refresh_token_url: s(&self.refresh_token_url),
            audience: s(&self.audience),
            resource: s(&self.resource),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestSettings {
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    pub verify_ssl: bool,
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self { timeout_ms: 30_000, follow_redirects: true, verify_ssl: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub id: Uuid,
    pub name: String,
    pub method: Method,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
    pub settings: RequestSettings,
    pub description: String,
    /// JavaScript run before the request is sent.
    pub pre_request_script: String,
    /// JavaScript run after the response arrives (tests).
    pub post_response_script: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "New Request".into(),
            method: Method::Get,
            url: String::new(),
            params: Vec::new(),
            headers: Vec::new(),
            body: Body::default(),
            auth: Auth::default(),
            settings: RequestSettings::default(),
            description: String::new(),
            pre_request_script: String::new(),
            post_response_script: String::new(),
        }
    }
}

impl Request {
    /// Deep copy with fresh ids.
    pub fn duplicate(&self) -> Self {
        Self { id: Uuid::new_v4(), name: format!("{} Copy", self.name), ..self.clone() }
    }

    /// Re-parse the query string of `url` into `params`, keeping disabled params.
    pub fn sync_params_from_url(&mut self) {
        let query = match self.url.split_once('?') {
            Some((_, q)) => q.split('#').next().unwrap_or(""),
            None => "",
        };
        let mut parsed: Vec<KeyValue> = query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) => KeyValue::new(k, v),
                None => KeyValue::new(pair, ""),
            })
            .collect();
        // Keep the description of params that still exist.
        for p in &mut parsed {
            if let Some(old) = self.params.iter().find(|o| o.enabled && o.key == p.key) {
                p.description = old.description.clone();
            }
        }
        let disabled: Vec<KeyValue> = self.params.iter().filter(|p| !p.enabled).cloned().collect();
        parsed.extend(disabled);
        self.params = parsed;
    }

    /// Rebuild the query string of `url` from the enabled `params`.
    pub fn sync_url_from_params(&mut self) {
        let (base, fragment) = match self.url.split_once('#') {
            Some((b, f)) => (b.to_string(), Some(f.to_string())),
            None => (self.url.clone(), None),
        };
        let base = base.split('?').next().unwrap_or("").to_string();
        let query: Vec<String> = self
            .params
            .iter()
            .filter(|p| p.enabled && !(p.key.is_empty() && p.value.is_empty()))
            .map(|p| if p.value.is_empty() && !p.key.is_empty() { p.key.clone() } else { format!("{}={}", p.key, p.value) })
            .collect();
        let mut url = base;
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }
        if let Some(f) = fragment {
            url.push('#');
            url.push_str(&f);
        }
        self.url = url;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub description: String,
    /// Used by the requests below that inherit auth; `Inherit` passes the parent's auth through.
    #[serde(default)]
    pub auth: Auth,
    /// Run for every request below, after the collection's scripts and before the request's own.
    #[serde(default)]
    pub pre_request_script: String,
    #[serde(default)]
    pub post_response_script: String,
}

impl Folder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            items: Vec::new(),
            description: String::new(),
            auth: Auth::default(),
            pre_request_script: String::new(),
            post_response_script: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Folder(Folder),
    Request(Request),
}

impl Item {
    pub fn id(&self) -> Uuid {
        match self {
            Item::Folder(f) => f.id,
            Item::Request(r) => r.id,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Item::Folder(f) => &f.name,
            Item::Request(r) => &r.name,
        }
    }
    pub fn set_name(&mut self, name: String) {
        match self {
            Item::Folder(f) => f.name = name,
            Item::Request(r) => r.name = name,
        }
    }
    /// Deep copy with fresh ids for every node.
    pub fn deep_duplicate(&self) -> Item {
        match self {
            Item::Request(r) => Item::Request(r.duplicate()),
            Item::Folder(f) => Item::Folder(Folder {
                id: Uuid::new_v4(),
                name: format!("{} Copy", f.name),
                items: f.items.iter().map(Item::fresh_ids).collect(),
                ..f.clone()
            }),
        }
    }
    fn fresh_ids(&self) -> Item {
        match self {
            Item::Request(r) => Item::Request(Request { id: Uuid::new_v4(), ..r.clone() }),
            Item::Folder(f) => Item::Folder(Folder {
                id: Uuid::new_v4(),
                items: f.items.iter().map(Item::fresh_ids).collect(),
                ..f.clone()
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Collection {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub items: Vec<Item>,
    pub variables: Vec<KeyValue>,
    pub auth: Auth,
    /// Run before / after every request of the collection, before the request's own scripts.
    pub pre_request_script: String,
    pub post_response_script: String,
}

impl Default for Collection {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "New Collection".into(),
            description: String::new(),
            items: Vec::new(),
            variables: Vec::new(),
            auth: Auth::none(),
            pre_request_script: String::new(),
            post_response_script: String::new(),
        }
    }
}

/// Whose auth a request with `Inherit` ends up using.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum AuthOwner {
    #[default]
    Collection,
    Folder(Uuid),
}

impl Collection {
    /// Folders that contain the item `id`, outermost first. Empty when the item is at the top
    /// level or not in this collection (e.g. a new request not saved yet).
    pub fn ancestors(&self, id: Uuid) -> Vec<&Folder> {
        fn walk<'a>(items: &'a [Item], id: Uuid, path: &mut Vec<&'a Folder>) -> bool {
            for item in items {
                if item.id() == id {
                    return true;
                }
                if let Item::Folder(f) = item {
                    path.push(f);
                    if walk(&f.items, id, path) {
                        return true;
                    }
                    path.pop();
                }
            }
            false
        }
        let mut path = Vec::new();
        if walk(&self.items, id, &mut path) { path } else { Vec::new() }
    }

    /// The auth a request (or folder) `id` inherits: the nearest folder above it that sets
    /// one, otherwise the collection's.
    pub fn inherited_auth(&self, id: Uuid) -> (AuthOwner, &Auth) {
        self.ancestors(id)
            .into_iter()
            .rev()
            .find(|f| f.auth.kind != AuthKind::Inherit)
            .map(|f| (AuthOwner::Folder(f.id), &f.auth))
            .unwrap_or((AuthOwner::Collection, &self.auth))
    }

    pub fn auth_mut(&mut self, owner: AuthOwner) -> Option<&mut Auth> {
        match owner {
            AuthOwner::Collection => Some(&mut self.auth),
            AuthOwner::Folder(id) => match find_item_mut(&mut self.items, id) {
                Some(Item::Folder(f)) => Some(&mut f.auth),
                _ => None,
            },
        }
    }

    /// "collection 'Shop'" / "folder 'Users'", for messages about inherited settings.
    pub fn owner_label(&self, owner: AuthOwner) -> String {
        match owner {
            AuthOwner::Collection => format!("collection '{}'", self.name),
            AuthOwner::Folder(id) => match find_item(&self.items, id) {
                Some(Item::Folder(f)) => format!("folder '{}'", f.name),
                _ => "folder".into(),
            },
        }
    }
}

/// Recursive helpers over item trees.
pub fn find_item_mut(items: &mut [Item], id: Uuid) -> Option<&mut Item> {
    for item in items.iter_mut() {
        if item.id() == id {
            return Some(item);
        }
        if let Item::Folder(f) = item
            && let Some(found) = find_item_mut(&mut f.items, id) {
                return Some(found);
            }
    }
    None
}

pub fn find_item(items: &[Item], id: Uuid) -> Option<&Item> {
    for item in items {
        if item.id() == id {
            return Some(item);
        }
        if let Item::Folder(f) = item
            && let Some(found) = find_item(&f.items, id) {
                return Some(found);
            }
    }
    None
}

/// Removes the item with `id` from the tree and returns it.
pub fn remove_item(items: &mut Vec<Item>, id: Uuid) -> Option<Item> {
    if let Some(pos) = items.iter().position(|i| i.id() == id) {
        return Some(items.remove(pos));
    }
    for item in items.iter_mut() {
        if let Item::Folder(f) = item
            && let Some(removed) = remove_item(&mut f.items, id) {
                return Some(removed);
            }
    }
    None
}

/// Inserts `new` right after the item with `after`. Returns false if not found.
pub fn insert_after(items: &mut Vec<Item>, after: Uuid, new: Item) -> Result<(), Item> {
    if let Some(pos) = items.iter().position(|i| i.id() == after) {
        items.insert(pos + 1, new);
        return Ok(());
    }
    let mut new = new;
    for item in items.iter_mut() {
        if let Item::Folder(f) = item {
            match insert_after(&mut f.items, after, new) {
                Ok(()) => return Ok(()),
                Err(back) => new = back,
            }
        }
    }
    Err(new)
}

/// True if `ancestor` is `id` or contains it somewhere below.
pub fn contains_id(item: &Item, id: Uuid) -> bool {
    if item.id() == id {
        return true;
    }
    match item {
        Item::Folder(f) => f.items.iter().any(|c| contains_id(c, id)),
        Item::Request(_) => false,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub id: Uuid,
    pub name: String,
    pub variables: Vec<KeyValue>,
}

impl Default for Environment {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "New Environment".into(), variables: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub request: Request,
    pub status: Option<u16>,
    pub duration_ms: Option<u128>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub collections: Vec<Collection>,
    pub environments: Vec<Environment>,
    pub globals: Vec<KeyValue>,
    pub active_environment: Option<Uuid>,
    pub history: Vec<HistoryEntry>,
    pub dark_mode: bool,
    /// UI zoom factor (Ctrl + / Ctrl - / Ctrl 0).
    pub zoom: f32,
    /// Collections / folders expanded in the sidebar.
    pub expanded: std::collections::HashSet<Uuid>,
    pub sidebar_width: f32,
    /// Code snippet panel: language and generator options.
    pub snippet: crate::codegen::Options,
    /// Tabs open when the app was closed.
    pub open_tabs: Vec<OpenTab>,
    pub active_tab: usize,
    pub specs: Vec<Spec>,
    pub mocks: Vec<MockServer>,
    pub datasets: Vec<Dataset>,
    pub documents: Vec<Document>,
    pub toolsets: Vec<Toolset>,
    /// Proxy and certificates (Settings).
    pub network: NetSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenTab {
    pub request: Request,
    /// Collection the request is saved in, if any.
    pub collection: Option<Uuid>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            collections: Vec::new(),
            environments: Vec::new(),
            globals: Vec::new(),
            active_environment: None,
            history: Vec::new(),
            dark_mode: true,
            zoom: 1.0,
            expanded: Default::default(),
            sidebar_width: 340.0,
            snippet: Default::default(),
            open_tabs: Vec::new(),
            active_tab: 0,
            specs: Vec::new(),
            mocks: Vec::new(),
            datasets: Vec::new(),
            documents: Vec::new(),
            toolsets: Vec::new(),
            network: NetSettings::default(),
        }
    }
}

/// Proxy and TLS certificate settings, used by every request the app sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NetSettings {
    /// Use HTTP_PROXY / HTTPS_PROXY / NO_PROXY from the environment.
    pub system_proxy: bool,
    /// Use the proxy below instead of the system one.
    pub custom_proxy: bool,
    /// Requests to http:// URLs go through the custom proxy.
    pub proxy_http: bool,
    /// Requests to https:// URLs go through the custom proxy.
    pub proxy_https: bool,
    pub proxy_host: String,
    pub proxy_port: String,
    pub proxy_auth: bool,
    pub proxy_username: String,
    pub proxy_password: String,
    /// Comma-separated hosts that skip the custom proxy.
    pub proxy_bypass: String,
    /// Trust the CA certificates in `ca_path` (PEM) besides the built-in ones.
    pub ca_enabled: bool,
    pub ca_path: String,
    pub client_certs: Vec<ClientCert>,
}

impl Default for NetSettings {
    fn default() -> Self {
        Self {
            system_proxy: true,
            custom_proxy: false,
            proxy_http: true,
            proxy_https: true,
            proxy_host: String::new(),
            proxy_port: "8080".into(),
            proxy_auth: false,
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_bypass: "localhost, 127.0.0.1, ::1".into(),
            ca_enabled: false,
            ca_path: String::new(),
            client_certs: Vec::new(),
        }
    }
}

/// A client certificate (mutual TLS) sent to one host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientCert {
    pub id: Uuid,
    pub enabled: bool,
    /// `api.example.com`, `*.example.com`, optionally with `:port`.
    pub host: String,
    /// PEM certificate (chain).
    pub cert_path: String,
    /// PEM private key (unencrypted PKCS#8, PKCS#1 or SEC1).
    pub key_path: String,
}

impl Default for ClientCert {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), enabled: true, host: String::new(), cert_path: String::new(), key_path: String::new() }
    }
}

impl ClientCert {
    /// Whether this certificate is for `host:port`. A pattern without a port matches any port.
    pub fn matches(&self, host: &str, port: u16) -> bool {
        let pattern = self.host.trim().to_ascii_lowercase();
        let pattern = pattern.strip_prefix("https://").unwrap_or(&pattern).trim_end_matches('/');
        if pattern.is_empty() {
            return false;
        }
        // Every host (`rustman run --ssl-client-cert`).
        if pattern == "*" {
            return true;
        }
        // `[::1]:8443`, `host:8443`, `host`.
        let (pat_host, pat_port) = match pattern.rsplit_once(':') {
            Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) && (!h.contains(':') || h.ends_with(']')) => (h, p.parse::<u16>().ok()),
            _ => (pattern, None),
        };
        let pat_host = pat_host.trim_start_matches('[').trim_end_matches(']');
        if pat_port.is_some_and(|p| p != port) {
            return false;
        }
        let host = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        match pat_host.strip_prefix("*.") {
            Some(suffix) => host.ends_with(&format!(".{suffix}")),
            None => host == pat_host,
        }
    }
}

pub const MAX_HISTORY: usize = 300;

// ---------------------------------------------------------------------------
// Specs, mock servers, datasets, documents
// ---------------------------------------------------------------------------

/// An API definition (OpenAPI 3 / Swagger 2, YAML or JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spec {
    pub id: Uuid,
    pub name: String,
    pub content: String,
}

impl Default for Spec {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "New API Spec".into(), content: SPEC_TEMPLATE.into() }
    }
}

pub const SPEC_TEMPLATE: &str = r#"openapi: 3.0.3
info:
  title: My API
  version: 1.0.0
servers:
  - url: http://localhost:8080
paths:
  /users:
    get:
      summary: List users
      tags: [users]
      parameters:
        - name: page
          in: query
          schema: { type: integer, default: 1 }
      responses:
        '200': { description: OK }
    post:
      summary: Create user
      tags: [users]
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                name: { type: string, example: Nham }
                email: { type: string, format: email }
      responses:
        '201': { description: Created }
  /users/{id}:
    get:
      summary: Get user
      tags: [users]
      parameters:
        - name: id
          in: path
          required: true
          schema: { type: integer, example: 1 }
      responses:
        '200': { description: OK }
"#;

/// A local mock server: a port and a list of canned responses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MockServer {
    pub id: Uuid,
    pub name: String,
    pub port: u16,
    pub routes: Vec<MockRoute>,
}

impl Default for MockServer {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "New Mock Server".into(), port: 3000, routes: vec![MockRoute::default()] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MockRoute {
    pub id: Uuid,
    pub enabled: bool,
    /// `None` matches any method.
    pub method: Option<Method>,
    /// `/users/:id`, `/files/*`
    pub path: String,
    pub status: u16,
    pub headers: Vec<KeyValue>,
    pub body: String,
    pub delay_ms: u64,
}

impl Default for MockRoute {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            enabled: true,
            method: Some(Method::Get),
            path: "/hello".into(),
            status: 200,
            headers: vec![KeyValue::new("Content-Type", "application/json")],
            body: "{\n    \"message\": \"Hello from Rustman mock\",\n    \"id\": \"{{$guid}}\"\n}".into(),
            delay_ms: 0,
        }
    }
}

/// Tabular data (CSV/JSON) used to drive collection runs: one row per iteration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dataset {
    pub id: Uuid,
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Default for Dataset {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "New Dataset".into(), columns: vec!["name".into(), "email".into()], rows: vec![vec![String::new(), String::new()]] }
    }
}

impl Dataset {
    /// Variables of row `i` (column name -> cell).
    pub fn row_vars(&self, i: usize) -> Vec<KeyValue> {
        let Some(row) = self.rows.get(i) else { return Vec::new() };
        self.columns
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.trim().is_empty())
            .map(|(j, c)| KeyValue::new(c.trim(), row.get(j).cloned().unwrap_or_default()))
            .collect()
    }
}

/// A markdown note kept in the workspace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Document {
    pub id: Uuid,
    pub name: String,
    pub content: String,
}

impl Default for Document {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "New Document".into(), content: String::new() }
    }
}

/// Requests exposed as tools to AI agents through a local MCP server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Toolset {
    pub id: Uuid,
    pub name: String,
    pub port: u16,
    pub tools: Vec<ToolDef>,
}

impl Default for Toolset {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), name: "New Toolset".into(), port: 7300, tools: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolDef {
    pub id: Uuid,
    pub enabled: bool,
    /// Tool name the agent sees (`get_user`).
    pub name: String,
    pub description: String,
    pub collection: Uuid,
    pub request: Uuid,
    /// `{{variables}}` of the request that the agent fills in.
    pub params: Vec<ToolParam>,
}

impl Default for ToolDef {
    fn default() -> Self {
        Self { id: Uuid::new_v4(), enabled: true, name: String::new(), description: String::new(), collection: Uuid::nil(), request: Uuid::nil(), params: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ToolParam {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// Workspace entities listed in the sidebar sections below collections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Spec,
    Mock,
    Dataset,
    Document,
    Toolset,
}

impl EntityKind {
    /// Sidebar order, same as Postman (after COLLECTIONS and ENVIRONMENTS).
    pub const ALL: [EntityKind; 5] = [EntityKind::Document, EntityKind::Spec, EntityKind::Mock, EntityKind::Dataset, EntityKind::Toolset];
    pub fn section(self) -> &'static str {
        match self {
            EntityKind::Spec => "SPECS",
            EntityKind::Mock => "MOCKS",
            EntityKind::Dataset => "DATASETS",
            EntityKind::Document => "DOCUMENTS",
            EntityKind::Toolset => "TOOLSETS",
        }
    }
    pub fn singular(self) -> &'static str {
        match self {
            EntityKind::Spec => "API spec",
            EntityKind::Mock => "mock server",
            EntityKind::Dataset => "dataset",
            EntityKind::Document => "document",
            EntityKind::Toolset => "toolset",
        }
    }
}

impl Workspace {
    pub fn collection(&self, id: Uuid) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }
    pub fn collection_mut(&mut self, id: Uuid) -> Option<&mut Collection> {
        self.collections.iter_mut().find(|c| c.id == id)
    }
    pub fn active_env(&self) -> Option<&Environment> {
        let id = self.active_environment?;
        self.environments.iter().find(|e| e.id == id)
    }
    pub fn push_history(&mut self, entry: HistoryEntry) {
        self.history.insert(0, entry);
        self.history.truncate(MAX_HISTORY);
    }

    /// (id, name) of every entity of a kind, in order.
    pub fn entities(&self, kind: EntityKind) -> Vec<(Uuid, String)> {
        match kind {
            EntityKind::Spec => self.specs.iter().map(|x| (x.id, x.name.clone())).collect(),
            EntityKind::Mock => self.mocks.iter().map(|x| (x.id, x.name.clone())).collect(),
            EntityKind::Dataset => self.datasets.iter().map(|x| (x.id, x.name.clone())).collect(),
            EntityKind::Document => self.documents.iter().map(|x| (x.id, x.name.clone())).collect(),
            EntityKind::Toolset => self.toolsets.iter().map(|x| (x.id, x.name.clone())).collect(),
        }
    }

    pub fn entity_name(&self, kind: EntityKind, id: Uuid) -> Option<String> {
        self.entities(kind).into_iter().find(|(i, _)| *i == id).map(|(_, n)| n)
    }

    pub fn set_entity_name(&mut self, kind: EntityKind, id: Uuid, name: String) {
        match kind {
            EntityKind::Spec => self.specs.iter_mut().filter(|x| x.id == id).for_each(|x| x.name = name.clone()),
            EntityKind::Mock => self.mocks.iter_mut().filter(|x| x.id == id).for_each(|x| x.name = name.clone()),
            EntityKind::Dataset => self.datasets.iter_mut().filter(|x| x.id == id).for_each(|x| x.name = name.clone()),
            EntityKind::Document => self.documents.iter_mut().filter(|x| x.id == id).for_each(|x| x.name = name.clone()),
            EntityKind::Toolset => self.toolsets.iter_mut().filter(|x| x.id == id).for_each(|x| x.name = name.clone()),
        }
    }

    /// Creates a new entity with default content; returns its id.
    pub fn add_entity(&mut self, kind: EntityKind) -> Uuid {
        match kind {
            EntityKind::Spec => push_new(&mut self.specs, |x: &Spec| x.id),
            EntityKind::Mock => {
                // Pick a port no other mock uses.
                let mut m = MockServer::default();
                while self.used_ports().contains(&m.port) {
                    m.port += 1;
                }
                let id = m.id;
                self.mocks.push(m);
                id
            }
            EntityKind::Dataset => push_new(&mut self.datasets, |x: &Dataset| x.id),
            EntityKind::Document => push_new(&mut self.documents, |x: &Document| x.id),
            EntityKind::Toolset => {
                let mut t = Toolset::default();
                while self.used_ports().contains(&t.port) {
                    t.port += 1;
                }
                let id = t.id;
                self.toolsets.push(t);
                id
            }
        }
    }

    /// Copies an entity right after the original; returns the copy's id.
    pub fn duplicate_entity(&mut self, kind: EntityKind, id: Uuid) -> Option<Uuid> {
        fn dup<T: Clone>(v: &mut Vec<T>, id: Uuid, get: impl Fn(&mut T) -> (&mut Uuid, &mut String)) -> Option<Uuid> {
            let i = v.iter_mut().position(|x| *get(x).0 == id)?;
            let mut copy = v[i].clone();
            let (cid, name) = get(&mut copy);
            *cid = Uuid::new_v4();
            name.push_str(" Copy");
            let new_id = *cid;
            v.insert(i + 1, copy);
            Some(new_id)
        }
        match kind {
            EntityKind::Spec => dup(&mut self.specs, id, |x| (&mut x.id, &mut x.name)),
            EntityKind::Mock => {
                let new = dup(&mut self.mocks, id, |x| (&mut x.id, &mut x.name))?;
                let max = self.mocks.iter().map(|m| m.port).max().unwrap_or(3000);
                if let Some(m) = self.mocks.iter_mut().find(|m| m.id == new) {
                    m.port = max.saturating_add(1);
                }
                Some(new)
            }
            EntityKind::Dataset => dup(&mut self.datasets, id, |x| (&mut x.id, &mut x.name)),
            EntityKind::Document => dup(&mut self.documents, id, |x| (&mut x.id, &mut x.name)),
            EntityKind::Toolset => {
                let new = dup(&mut self.toolsets, id, |x| (&mut x.id, &mut x.name))?;
                let max = self.used_ports().into_iter().max().unwrap_or(7300);
                if let Some(t) = self.toolsets.iter_mut().find(|t| t.id == new) {
                    t.port = max.saturating_add(1);
                }
                Some(new)
            }
        }
    }

    /// Ports configured for mock servers and toolsets.
    pub fn used_ports(&self) -> Vec<u16> {
        self.mocks.iter().map(|m| m.port).chain(self.toolsets.iter().map(|t| t.port)).collect()
    }

    pub fn delete_entity(&mut self, kind: EntityKind, id: Uuid) {
        match kind {
            EntityKind::Spec => self.specs.retain(|x| x.id != id),
            EntityKind::Mock => self.mocks.retain(|x| x.id != id),
            EntityKind::Dataset => self.datasets.retain(|x| x.id != id),
            EntityKind::Document => self.documents.retain(|x| x.id != id),
            EntityKind::Toolset => self.toolsets.retain(|x| x.id != id),
        }
    }
}

fn push_new<T: Default>(v: &mut Vec<T>, id: impl Fn(&T) -> Uuid) -> Uuid {
    let x = T::default();
    let i = id(&x);
    v.push(x);
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_cert_host_patterns() {
        let c = |host: &str| ClientCert { host: host.into(), ..Default::default() };
        assert!(c("api.x.io").matches("api.x.io", 443));
        assert!(c("API.x.io").matches("api.x.io", 8443), "no port in the pattern: any port");
        assert!(c("api.x.io:8443").matches("api.x.io", 8443));
        assert!(!c("api.x.io:8443").matches("api.x.io", 443));
        assert!(c("*.x.io").matches("a.b.x.io", 443));
        assert!(!c("*.x.io").matches("x.io", 443));
        assert!(!c("x.io").matches("api.x.io", 443));
        assert!(c("https://api.x.io/").matches("api.x.io", 443));
        assert!(c("[::1]:8443").matches("[::1]", 8443));
        assert!(c("::1").matches("[::1]", 443));
        assert!(!c("").matches("x.io", 443));
    }

    #[test]
    fn params_roundtrip() {
        let mut r = Request { url: "https://x.io/a?b=1&c={{v}}#frag".into(), ..Default::default() };
        r.sync_params_from_url();
        assert_eq!(r.params.len(), 2);
        assert_eq!(r.params[1].value, "{{v}}");
        r.params[0].enabled = false;
        r.sync_url_from_params();
        assert_eq!(r.url, "https://x.io/a?c={{v}}#frag");
        r.sync_params_from_url();
        assert_eq!(r.params.len(), 2, "disabled param must be kept");
    }

    #[test]
    fn entities_crud_and_old_workspace_compat() {
        let mut ws: Workspace = serde_json::from_str(r#"{"collections":[]}"#).unwrap();
        assert!(ws.mocks.is_empty());
        let a = ws.add_entity(EntityKind::Mock);
        let b = ws.add_entity(EntityKind::Mock);
        assert_ne!(ws.mocks[0].port, ws.mocks[1].port);
        let c = ws.duplicate_entity(EntityKind::Mock, a).unwrap();
        assert_eq!(ws.entities(EntityKind::Mock).iter().map(|e| e.0).collect::<Vec<_>>(), vec![a, c, b]);
        assert_eq!(ws.entity_name(EntityKind::Mock, c).unwrap(), "New Mock Server Copy");
        ws.delete_entity(EntityKind::Mock, a);
        assert_eq!(ws.mocks.len(), 2);
        let mut d = Dataset::default();
        d.rows = vec![vec!["Nham".into(), "n@x.io".into()]];
        assert_eq!(d.row_vars(0)[1], KeyValue::new("email", "n@x.io"));
    }

    #[test]
    fn folder_auth_inheritance() {
        let req = Request::default();
        let rid = req.id;
        let mut inner = Folder::new("inner");
        inner.items.push(Item::Request(req));
        let inner_id = inner.id;
        let mut outer = Folder::new("outer");
        outer.auth = Auth { kind: AuthKind::Bearer, bearer_token: "outer".into(), ..Default::default() };
        outer.items.push(Item::Folder(inner));
        let outer_id = outer.id;
        let mut c = Collection { auth: Auth { kind: AuthKind::Basic, ..Default::default() }, ..Default::default() };
        c.items.push(Item::Folder(outer));

        assert_eq!(c.ancestors(rid).iter().map(|f| f.id).collect::<Vec<_>>(), vec![outer_id, inner_id]);
        assert!(c.ancestors(Uuid::new_v4()).is_empty(), "unknown items have no folders");
        // `inner` inherits, so the request gets `outer`'s bearer token.
        let (owner, auth) = c.inherited_auth(rid);
        assert_eq!((owner, auth.bearer_token.as_str()), (AuthOwner::Folder(outer_id), "outer"));
        assert_eq!(c.owner_label(owner), "folder 'outer'");

        c.auth_mut(AuthOwner::Folder(inner_id)).unwrap().kind = AuthKind::None;
        assert_eq!(c.inherited_auth(rid).0, AuthOwner::Folder(inner_id), "No Auth on a folder stops inheritance");
        c.auth_mut(AuthOwner::Folder(inner_id)).unwrap().kind = AuthKind::Inherit;
        c.auth_mut(AuthOwner::Folder(outer_id)).unwrap().kind = AuthKind::Inherit;
        assert_eq!(c.inherited_auth(rid), (AuthOwner::Collection, &c.auth));

        // Old workspaces have folders without the new fields.
        let f: Folder = serde_json::from_str(r#"{"id":"6f1c1a8e-3c2b-4f0e-9a51-0b8f7f1e2d3c","name":"old"}"#).unwrap();
        assert_eq!(f.auth.kind, AuthKind::Inherit);
    }

    #[test]
    fn tree_ops() {
        let mut items = vec![Item::Folder(Folder::new("f")), Item::Request(Request::default())];
        let fid = items[0].id();
        let req = Request::default();
        let rid = req.id;
        if let Item::Folder(f) = &mut items[0] {
            f.items.push(Item::Request(req));
        }
        assert!(find_item(&items, rid).is_some());
        assert!(contains_id(&items[0], rid));
        let removed = remove_item(&mut items, rid).unwrap();
        assert!(insert_after(&mut items, fid, removed).is_ok());
        assert_eq!(items.len(), 3);
    }
}
