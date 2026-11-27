//! Application state, logic and top-level layout. UI pieces live in the submodules.

mod chrome;
mod console;
mod dialogs;
mod environment;
mod mocks;
mod oauth;
mod runner;
mod terminal;
mod toolsets;
mod views;
mod request;
mod response;
mod sidebar;

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, Ui};
use uuid::Uuid;

use crate::exec;
use oauth::OAuthTarget;
use crate::http::{self, Prepared, ResponseData};
use crate::import;
use crate::model::*;
use crate::pm::WrapCache;
use crate::script::{self, ScriptEvent, Scopes, VarOp};
use crate::storage;
use crate::theme;
use crate::vars::{VarMap, build_vars};

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReqSection {
    Docs,
    Params,
    Auth,
    Headers,
    Body,
    Scripts,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespSection {
    Body,
    Cookies,
    Headers,
    Tests,
}

/// Which script of a request / collection is being edited.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum ScriptPhase {
    #[default]
    Pre,
    Post,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyView {
    Pretty,
    Raw,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarMode {
    Explorer,
    History,
}

/// Tabs of the bottom panel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BottomTab {
    Console,
    Terminal,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightPanel {
    Code,
    Info,
}

/// Rendered response, with lines pre-split for the virtualized viewer.
struct ResponseView {
    data: ResponseData,
    /// Test results and script errors.
    report: exec::Report,
    /// "JSON", "HTML", "XML", "Text"
    kind: &'static str,
    is_json: bool,
    is_binary: bool,
    pretty_lines: Vec<String>,
    raw_lines: Vec<String>,
    wrap_cache: WrapCache,
}

fn split_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text.lines().map(str::to_string).collect();
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Pretty JSON with 4-space indentation, like Postman.
fn pretty_json(v: &serde_json::Value) -> String {
    use serde::Serialize;
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    if v.serialize(&mut ser).is_err() {
        return v.to_string();
    }
    String::from_utf8(buf).unwrap_or_default()
}

impl ResponseView {
    fn new(data: ResponseData, report: exec::Report) -> Self {
        let is_binary = std::str::from_utf8(&data.body).is_err();
        let text = data.text();
        let parsed = if is_binary { None } else { serde_json::from_str::<serde_json::Value>(&text).ok() };
        let is_json = parsed.is_some();
        let ct = data.content_type();
        let kind = if is_json {
            "JSON"
        } else if ct.contains("html") {
            "HTML"
        } else if ct.contains("xml") {
            "XML"
        } else {
            "Text"
        };
        let pretty = match &parsed {
            Some(v) => pretty_json(v),
            None => text.clone(),
        };
        Self {
            pretty_lines: split_lines(&pretty),
            raw_lines: split_lines(&text),
            kind,
            is_json,
            is_binary,
            data,
            report,
            wrap_cache: WrapCache::default(),
        }
    }
}

enum TabState {
    Idle,
    Loading {
        started: Instant,
        handle: tokio::task::JoinHandle<()>,
        snapshot: Box<Request>,
        /// Console entry, once the request is actually sent (after pre-request scripts).
        log_id: Option<Uuid>,
        /// Where script variable changes go: the environment and collection active at send time.
        env: Option<Uuid>,
        collection: Option<Uuid>,
    },
    Done(Box<ResponseView>),
    Error(String),
}

struct Tab {
    request: Request,
    /// Collection the request is saved in.
    collection: Option<Uuid>,
    /// Last saved version, to detect unsaved changes.
    saved: Option<Request>,
    req_section: ReqSection,
    resp_section: RespSection,
    body_view: BodyView,
    wrap: bool,
    search_open: bool,
    search: String,
    match_idx: usize,
    scroll_to_match: bool,
    show_hidden_headers: bool,
    script_phase: ScriptPhase,
    /// Test results filter: 0 all, 1 passed, 2 skipped, 3 failed.
    test_filter: u8,
    state: TabState,
}

impl Tab {
    fn new(request: Request, collection: Option<Uuid>, saved: Option<Request>) -> Self {
        Self {
            request,
            collection,
            saved,
            req_section: ReqSection::Params,
            resp_section: RespSection::Body,
            body_view: BodyView::Pretty,
            wrap: true,
            search_open: false,
            search: String::new(),
            match_idx: 0,
            scroll_to_match: false,
            show_hidden_headers: false,
            script_phase: ScriptPhase::Pre,
            test_filter: 0,
            state: TabState::Idle,
        }
    }
    fn is_dirty(&self) -> bool {
        match &self.saved {
            Some(s) => s != &self.request,
            // An unsaved request counts as dirty once something was typed.
            None => !self.request.url.is_empty(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EnvTarget {
    Globals,
    Env(Uuid),
}

/// What the extra (non-request) tab shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SideTab {
    Env(EnvTarget),
    Entity(EntityKind, Uuid),
    /// Collection runner for a collection.
    Runner(Uuid),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollSection {
    Overview,
    Auth,
    Variables,
    Scripts,
}

/// What an inline rename is renaming.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RenameTarget {
    Collection(Uuid),
    Item { collection: Uuid, item: Uuid },
    Env(Uuid),
    /// Environment name in the header of the environment tab.
    EnvTitle(Uuid),
    /// Spec / mock / dataset / document row in the sidebar.
    Entity(EntityKind, Uuid),
    /// Spec / mock / dataset / document name in the header of its tab.
    EntityTitle(EntityKind, Uuid),
    TabName,
}

#[derive(Clone, Copy)]
enum Confirm {
    CloseTab(usize),
    /// Closing several tabs left some with unsaved changes: every tab but `keep` (a request id).
    CloseTabs { keep: Option<Uuid> },
    DeleteCollection(Uuid),
    DeleteItem { collection: Uuid, item: Uuid },
    /// The items in `tree_selection`.
    DeleteSelection,
    DeleteEnv(Uuid),
    DeleteEntity(EntityKind, Uuid),
    ClearHistory,
}

struct SaveAsState {
    tab: usize,
    name: String,
    collection: Option<Uuid>,
    folder: Option<Uuid>,
    new_collection_name: String,
}

struct ImportState {
    text: String,
    error: Option<String>,
}

/// Payload for drag & drop in the collection tree.
#[derive(Clone, Copy)]
struct DragItem {
    collection: Uuid,
    item: Uuid,
}

enum TreeAction {
    Open { collection: Uuid, request: Uuid },
    Toggle(Uuid),
    NewRequest { collection: Uuid, folder: Option<Uuid> },
    NewFolder { collection: Uuid, folder: Option<Uuid> },
    Rename(RenameTarget),
    Duplicate { collection: Uuid, item: Uuid },
    DuplicateCollection(Uuid),
    Delete(Confirm),
    EditCollection(Uuid, CollSection),
    EditFolder { collection: Uuid, folder: Uuid, section: CollSection },
    Export(Uuid),
    Run(Uuid),
    MoveInto { drag: DragItem, collection: Uuid, folder: Option<Uuid> },
    MoveAfter { drag: DragItem, collection: Uuid, after: Uuid },
    /// A click on a request or folder row, with the modifiers that change the selection.
    Select { collection: Uuid, item: Uuid, mode: SelectMode },
    ClearSelection,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectMode {
    /// Plain click: only this row.
    Only,
    /// Ctrl/Cmd+click: add or remove this row.
    Toggle,
    /// Shift+click: every row from the anchor to this one.
    Range,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Proxy,
    Certificates,
}

enum Msg {
    /// Pre-request scripts are done and the request goes out.
    Sent { tab: Uuid, prepared: Box<Prepared>, events: Vec<ScriptEvent> },
    Done { tab: Uuid, executed: Box<exec::Executed> },
    /// An OAuth 2.0 token request finished.
    OAuth { job: Uuid, outcome: Box<crate::oauth2::Outcome> },
}

pub struct RustmanApp {
    ws: Workspace,
    tabs: Vec<Tab>,
    active: usize,
    runtime: tokio::runtime::Runtime,
    jar: Arc<reqwest::cookie::Jar>,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    last_saved_json: String,
    last_save_check: Instant,

    sidebar_mode: SidebarMode,
    sidebar_visible: bool,
    env_section_open: bool,
    filter: String,
    tree_focus: Option<egui::Id>,
    /// Requests and folders selected in the collection tree, as (collection, item).
    tree_selection: Vec<(Uuid, Uuid)>,
    /// Where a Shift+click range starts.
    tree_anchor: Option<(Uuid, Uuid)>,
    /// The item rows of the tree in the order drawn last frame, for Shift+click ranges.
    tree_rows: Vec<(Uuid, Uuid)>,
    header_search: String,
    right_panel: Option<RightPanel>,

    /// Environment, spec, mock, dataset, document or runner open in the extra tab, if any.
    side_tab: Option<SideTab>,
    /// The extra tab is the one shown in the main area.
    side_focused: bool,
    /// Open sidebar sections below collections (SPECS, MOCKS, ...).
    sb_open: std::collections::HashSet<EntityKind>,
    /// Running mock servers.
    mocks_running: std::collections::HashMap<Uuid, mocks::Running>,
    /// Running toolset MCP servers.
    toolsets_running: std::collections::HashMap<Uuid, toolsets::Running>,
    runner: Option<runner::RunnerState>,
    run_tx: mpsc::Sender<runner::RunMsg>,
    run_rx: mpsc::Receiver<runner::RunMsg>,
    coll_editor: Option<(Uuid, CollSection)>,
    /// Folder settings window: (collection, folder, section). `Variables` is not used for folders.
    folder_editor: Option<(Uuid, Uuid, CollSection)>,
    coll_script_phase: ScriptPhase,
    rename: Option<(RenameTarget, String)>,
    rename_focus: bool,
    confirm: Option<Confirm>,
    save_as: Option<SaveAsState>,
    import: Option<ImportState>,
    /// OAuth 2.0 token request in progress (or failed, showing its error).
    oauth: Option<oauth::OAuthJob>,
    cookies_open: bool,
    settings_open: bool,
    settings_tab: SettingsTab,
    about_open: bool,
    toast: Option<(String, Instant, bool)>,
    console: console::Console,
    bottom_open: bool,
    bottom_tab: BottomTab,
    terminal: Option<terminal::Terminal>,
    /// The terminal had keyboard focus last frame (app shortcuts go to the shell).
    terminal_focused: bool,
    #[cfg(debug_assertions)]
    frame_no: u64,
}

impl RustmanApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ws = storage::load();
        http::set_network(&ws.network);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply_style(&cc.egui_ctx);
        cc.egui_ctx.set_theme(if ws.dark_mode { egui::Theme::Dark } else { egui::Theme::Light });
        cc.egui_ctx.set_zoom_factor(ws.zoom.clamp(0.5, 3.0));
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
        let (tx, rx) = mpsc::channel();
        let (run_tx, run_rx) = mpsc::channel();

        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            runtime,
            jar: Arc::new(reqwest::cookie::Jar::default()),
            tx,
            rx,
            last_saved_json: String::new(),
            last_save_check: Instant::now(),
            sidebar_mode: SidebarMode::Explorer,
            sidebar_visible: true,
            env_section_open: false,
            filter: String::new(),
            tree_focus: None,
            tree_selection: Vec::new(),
            tree_anchor: None,
            tree_rows: Vec::new(),
            header_search: String::new(),
            right_panel: None,
            side_tab: None,
            side_focused: false,
            sb_open: Default::default(),
            mocks_running: Default::default(),
            toolsets_running: Default::default(),
            runner: None,
            run_tx,
            run_rx,
            coll_editor: None,
            folder_editor: None,
            coll_script_phase: ScriptPhase::Pre,
            rename: None,
            rename_focus: false,
            confirm: None,
            save_as: None,
            import: None,
            oauth: None,
            cookies_open: false,
            settings_open: false,
            settings_tab: SettingsTab::General,
            about_open: false,
            toast: None,
            console: console::Console::default(),
            bottom_open: false,
            bottom_tab: BottomTab::Console,
            terminal: None,
            terminal_focused: false,
            #[cfg(debug_assertions)]
            frame_no: 0,
            ws,
        };
        for open in std::mem::take(&mut app.ws.open_tabs) {
            let saved = open
                .collection
                .and_then(|c| app.ws.collection(c))
                .and_then(|c| find_item(&c.items, open.request.id))
                .and_then(|i| if let Item::Request(r) = i { Some(r.clone()) } else { None });
            let collection = if saved.is_some() { open.collection } else { None };
            app.tabs.push(Tab::new(open.request, collection, saved));
        }
        app.active = app.ws.active_tab.min(app.tabs.len().saturating_sub(1));
        app.last_saved_json = app.snapshot_json();
        app
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    fn snapshot_json(&mut self) -> String {
        self.ws.open_tabs = self.tabs.iter().map(|t| OpenTab { request: t.request.clone(), collection: t.collection }).collect();
        self.ws.active_tab = self.active;
        serde_json::to_string(&self.ws).unwrap_or_default()
    }

    fn save_if_changed(&mut self, force: bool) {
        if !force && self.last_save_check.elapsed() < Duration::from_millis(1500) {
            return;
        }
        self.last_save_check = Instant::now();
        let json = self.snapshot_json();
        if json != self.last_saved_json {
            match storage::save(&self.ws) {
                Ok(()) => self.last_saved_json = json,
                Err(e) => self.notify_err(format!("Cannot save workspace: {e}")),
            }
        }
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now(), false));
    }
    fn notify_err(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now(), true));
    }

    // -----------------------------------------------------------------------
    // Variables
    // -----------------------------------------------------------------------

    fn vars_for(&self, collection: Option<Uuid>) -> VarMap {
        let empty: Vec<KeyValue> = Vec::new();
        let coll_vars = collection.and_then(|c| self.ws.collection(c)).map(|c| &c.variables).unwrap_or(&empty);
        let env_vars = self.ws.active_env().map(|e| &e.variables).unwrap_or(&empty);
        build_vars(&[&self.ws.globals, coll_vars, env_vars])
    }

    /// The auth a request with `Inherit` uses (from its folders or collection), and where it comes from.
    fn inherited_auth(&self, collection: Option<Uuid>, request: Uuid) -> Option<(Auth, String)> {
        let c = collection.and_then(|c| self.ws.collection(c))?;
        let (owner, auth) = c.inherited_auth(request);
        Some((auth.clone(), c.owner_label(owner)))
    }

    // -----------------------------------------------------------------------
    // Tabs
    // -----------------------------------------------------------------------

    fn new_tab(&mut self) {
        self.tabs.push(Tab::new(Request { name: "Untitled Request".into(), ..Default::default() }, None, None));
        self.active = self.tabs.len() - 1;
        self.side_focused = false;
    }

    /// Opens something in the extra tab, replacing what was shown there.
    fn open_side(&mut self, tab: SideTab) {
        self.side_tab = Some(tab);
        self.side_focused = true;
    }

    fn open_env(&mut self, target: EnvTarget) {
        self.open_side(SideTab::Env(target));
    }

    /// What the extra tab shows, when it is the visible tab.
    fn side_shown(&self) -> Option<SideTab> {
        self.side_tab.filter(|_| self.side_focused)
    }

    /// Creates a spec / mock / dataset / document, opens it and starts renaming it.
    fn new_entity(&mut self, kind: EntityKind) {
        let id = self.ws.add_entity(kind);
        self.sb_open.insert(kind);
        self.sidebar_mode = SidebarMode::Explorer;
        self.open_side(SideTab::Entity(kind, id));
        if let Some(name) = self.ws.entity_name(kind, id) {
            self.start_rename(RenameTarget::EntityTitle(kind, id), name);
        }
    }

    fn delete_entity(&mut self, kind: EntityKind, id: Uuid) {
        if kind == EntityKind::Mock {
            self.stop_mock(id);
        }
        if kind == EntityKind::Toolset {
            self.stop_toolset(id);
        }
        self.ws.delete_entity(kind, id);
        if self.side_tab == Some(SideTab::Entity(kind, id)) {
            self.close_side_tab();
        }
        if let Some(r) = &mut self.runner
            && r.dataset == Some(id)
        {
            r.dataset = None;
        }
    }

    fn close_side_tab(&mut self) {
        self.side_tab = None;
        self.side_focused = false;
    }

    /// The environment shown in the extra tab, when it is the visible tab.
    fn env_shown(&self) -> Option<EnvTarget> {
        match self.side_shown() {
            Some(SideTab::Env(t)) => Some(t),
            _ => None,
        }
    }

    fn open_request(&mut self, collection: Uuid, request: Uuid) {
        self.side_focused = false;
        if let Some(i) = self.tabs.iter().position(|t| t.request.id == request) {
            self.active = i;
            return;
        }
        let Some(Item::Request(r)) = self.ws.collection(collection).and_then(|c| find_item(&c.items, request)) else {
            return;
        };
        let r = r.clone();
        self.tabs.push(Tab::new(r.clone(), Some(collection), Some(r)));
        self.active = self.tabs.len() - 1;
    }

    fn close_tab(&mut self, i: usize, force: bool) {
        if i >= self.tabs.len() {
            return;
        }
        if !force && self.tabs[i].is_dirty() {
            self.confirm = Some(Confirm::CloseTab(i));
            return;
        }
        let tab = self.tabs.remove(i);
        self.drop_tab(tab);
        if self.active >= i && self.active > 0 {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }

    /// Closes every tab but `keep` (a request id). Without `force`, tabs with unsaved changes stay
    /// open; returns how many did.
    fn close_tabs(&mut self, keep: Option<Uuid>, force: bool) -> usize {
        let active_id = self.tabs.get(self.active).map(|t| t.request.id);
        let mut kept_dirty = 0;
        for tab in std::mem::take(&mut self.tabs) {
            if Some(tab.request.id) == keep {
                self.tabs.push(tab);
            } else if !force && tab.is_dirty() {
                kept_dirty += 1;
                self.tabs.push(tab);
            } else {
                self.drop_tab(tab);
            }
        }
        let pos = |id: Option<Uuid>| id.and_then(|id| self.tabs.iter().position(|t| t.request.id == id));
        self.active = pos(keep).or_else(|| pos(active_id)).unwrap_or(0);
        self.side_focused = false;
        kept_dirty
    }

    /// "Close Other Tabs" / "Close All Tabs": asks about the tabs with unsaved changes.
    fn close_tabs_confirm(&mut self, keep: Option<Uuid>) {
        if self.close_tabs(keep, false) > 0 {
            self.confirm = Some(Confirm::CloseTabs { keep });
        }
    }

    /// Stops what a closed tab was still doing.
    fn drop_tab(&mut self, tab: Tab) {
        if let TabState::Loading { handle, log_id, .. } = tab.state {
            handle.abort();
            if let Some(id) = log_id {
                self.console.cancel(id);
            }
        }
    }

    /// Opens a copy of tab `i` next to it, as a new unsaved request.
    fn duplicate_tab(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i) else { return };
        let request = Request { id: Uuid::new_v4(), ..tab.request.clone() };
        let mut copy = Tab::new(request, None, None);
        (copy.req_section, copy.body_view, copy.wrap) = (tab.req_section, tab.body_view, tab.wrap);
        self.tabs.insert(i + 1, copy);
        self.active = i + 1;
        self.side_focused = false;
    }

    /// After structural changes, make every tab point to the collection that holds its request.
    fn relink_tabs(&mut self) {
        for tab in &mut self.tabs {
            let id = tab.request.id;
            let location = self.ws.collections.iter().find_map(|c| match find_item(&c.items, id) {
                Some(Item::Request(r)) => Some((c.id, r.clone())),
                _ => None,
            });
            match location {
                Some((c, r)) => {
                    tab.collection = Some(c);
                    if let Some(saved) = &tab.saved
                        && saved.name != r.name && tab.request.name == saved.name {
                            tab.request.name = r.name.clone();
                        }
                    tab.saved = Some(r);
                }
                None => {
                    tab.collection = None;
                    tab.saved = None;
                }
            }
        }
    }

    fn save_tab(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i) else { return };
        let (id, coll) = (tab.request.id, tab.collection);
        let req = tab.request.clone();
        if let Some(c) = coll.and_then(|c| self.ws.collection_mut(c))
            && let Some(item) = find_item_mut(&mut c.items, id) {
                *item = Item::Request(req.clone());
                self.tabs[i].saved = Some(req);
                self.notify("Saved");
                return;
            }
        self.open_save_as(i);
    }

    fn open_save_as(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i) else { return };
        self.save_as = Some(SaveAsState {
            tab: i,
            name: tab.request.name.clone(),
            collection: tab.collection.or_else(|| self.ws.collections.first().map(|c| c.id)),
            folder: None,
            new_collection_name: "My Collection".into(),
        });
    }

    /// Everything needed to run `request` (scripts, auth, variables) off the UI thread.
    fn job_for(&self, request: &Request, collection: Option<Uuid>, data: &[KeyValue], info: script::Info) -> exec::Job {
        let coll = collection.and_then(|c| self.ws.collection(c));
        let env = self.ws.active_env();
        let (scripts, auth, auth_owner) = exec::context(coll, request);
        exec::Job {
            request: request.clone(),
            scripts,
            auth,
            auth_owner,
            scopes: Scopes::new(&self.ws.globals, coll.map(|c| &c.variables[..]).unwrap_or(&[]), env.map(|e| &e.variables[..]), data),
            environment_name: env.map(|e| e.name.clone()),
            info,
        }
    }

    fn send(&mut self, i: usize, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(i) else { return };
        if matches!(tab.state, TabState::Loading { .. }) {
            return;
        }
        let info = script::Info { request_name: tab.request.name.clone(), request_id: tab.request.id.to_string(), iteration: 0, iteration_count: 1 };
        let job = self.job_for(&tab.request, tab.collection, &[], info);
        let tab_id = tab.request.id;
        let snapshot = Box::new(tab.request.clone());
        let collection = tab.collection;
        let (tx, jar, ctx) = (self.tx.clone(), self.jar.clone(), ctx.clone());
        let handle = self.runtime.spawn(async move {
            let (sent_tx, sent_ctx) = (tx.clone(), ctx.clone());
            let executed = exec::run(job, jar, move |p, events| {
                let _ = sent_tx.send(Msg::Sent { tab: tab_id, prepared: Box::new(p.clone()), events: events.to_vec() });
                sent_ctx.request_repaint();
            })
            .await;
            let _ = tx.send(Msg::Done { tab: tab_id, executed: Box::new(executed) });
            ctx.request_repaint();
        });
        self.tabs[i].state = TabState::Loading { started: Instant::now(), handle, snapshot, log_id: None, env: self.ws.active_environment, collection };
    }

    fn cancel(&mut self, i: usize) {
        if let Some(tab) = self.tabs.get_mut(i)
            && let TabState::Loading { handle, log_id, .. } = &tab.state {
                handle.abort();
                if let Some(id) = log_id {
                    self.console.cancel(*id);
                }
                tab.state = TabState::Error("Request cancelled".into());
            }
    }

    fn receive(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Sent { tab, prepared, events } => {
                    self.log_script_events(&events);
                    let id = self.console.start(&prepared);
                    match self.tabs.iter_mut().find(|t| t.request.id == tab).map(|t| &mut t.state) {
                        Some(TabState::Loading { log_id, .. }) => *log_id = Some(id),
                        _ => self.console.cancel(id),
                    }
                }
                Msg::OAuth { job, outcome } => self.finish_oauth(job, *outcome),
                Msg::Done { tab, executed } => {
                    let Some(t) = self.tabs.iter_mut().find(|t| t.request.id == tab) else { continue };
                    let TabState::Loading { snapshot, log_id, env, collection, .. } = std::mem::replace(&mut t.state, TabState::Idle) else {
                        continue;
                    };
                    let exec::Executed { result, report, .. } = *executed;
                    if let Some(u) = &report.token_update {
                        self.apply_token_update(u, collection);
                    }
                    match log_id {
                        Some(id) => self.console.finish(id, &result),
                        None => {
                            // Never sent: a pre-request script or the request itself failed.
                            self.log_script_events(&report.pre_events);
                            if let Err(e) = &result {
                                self.console.failed_to_prepare(snapshot.method, &snapshot.url, e);
                            }
                        }
                    }
                    self.log_script_events(&report.post_events);
                    // Pre-request errors are already logged as the failed request.
                    if log_id.is_some() {
                        for e in &report.errors {
                            self.console.log(script::LogLevel::Error, e.clone());
                        }
                    }
                    self.apply_var_ops(report.persistent_ops(), collection, env);
                    let (status, duration) = match &result {
                        Ok(r) => (Some(r.status), Some(r.duration.as_millis())),
                        Err(_) => (None, None),
                    };
                    let Some(t) = self.tabs.iter_mut().find(|t| t.request.id == tab) else { continue };
                    t.state = match result {
                        Ok(r) => TabState::Done(Box::new(ResponseView::new(r, report))),
                        Err(e) => TabState::Error(e),
                    };
                    t.match_idx = 0;
                    self.ws.push_history(HistoryEntry {
                        id: Uuid::new_v4(),
                        timestamp: chrono::Local::now(),
                        request: *snapshot,
                        status,
                        duration_ms: duration,
                    });
                }
            }
        }
    }

    /// Console lines and `pm.sendRequest` calls made by scripts.
    fn log_script_events(&mut self, events: &[ScriptEvent]) {
        for e in events {
            match e {
                ScriptEvent::Log(level, text) => self.console.log(*level, text.clone()),
                ScriptEvent::Http(x) => {
                    let id = self.console.start(&x.0);
                    self.console.finish(id, &x.1);
                }
            }
        }
    }

    /// Persists variable changes made by scripts (`pm.environment.set`, ...).
    fn apply_var_ops<'a>(&mut self, ops: impl Iterator<Item = &'a VarOp>, collection: Option<Uuid>, env: Option<Uuid>) {
        for op in ops {
            let list = match op.scope() {
                script::Scope::Globals => Some(&mut self.ws.globals),
                script::Scope::Collection => collection.and_then(|c| self.ws.collection_mut(c)).map(|c| &mut c.variables),
                script::Scope::Environment => env.and_then(|id| self.ws.environments.iter_mut().find(|e| e.id == id)).map(|e| &mut e.variables),
                script::Scope::Data | script::Scope::Local => None,
            };
            if let Some(list) = list {
                op.apply_to(list);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Tree actions
    // -----------------------------------------------------------------------

    fn new_collection(&mut self) {
        let c = Collection::default();
        let id = c.id;
        self.ws.collections.push(c);
        self.ws.expanded.insert(id);
        self.sidebar_mode = SidebarMode::Explorer;
        self.start_rename(RenameTarget::Collection(id), "New Collection".into());
    }

    fn new_environment(&mut self) {
        let e = Environment::default();
        let id = e.id;
        self.ws.environments.push(e);
        self.env_section_open = true;
        self.open_env(EnvTarget::Env(id));
        self.start_rename(RenameTarget::EnvTitle(id), "New Environment".into());
    }

    fn start_rename(&mut self, target: RenameTarget, current: String) {
        self.rename = Some((target, current));
        self.rename_focus = true;
    }

    fn commit_rename(&mut self) {
        let Some((target, name)) = self.rename.take() else { return };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        match target {
            RenameTarget::Collection(c) => {
                if let Some(c) = self.ws.collection_mut(c) {
                    c.name = name;
                }
            }
            RenameTarget::Item { collection, item } => {
                if let Some(i) = self.ws.collection_mut(collection).and_then(|c| find_item_mut(&mut c.items, item)) {
                    i.set_name(name);
                }
                self.relink_tabs();
            }
            RenameTarget::Entity(kind, id) | RenameTarget::EntityTitle(kind, id) => self.ws.set_entity_name(kind, id, name),
            RenameTarget::Env(e) | RenameTarget::EnvTitle(e) => {
                if let Some(e) = self.ws.environments.iter_mut().find(|x| x.id == e) {
                    e.name = name;
                }
            }
            RenameTarget::TabName => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.request.name = name;
                }
            }
        }
    }

    fn select_tree_item(&mut self, row: (Uuid, Uuid), mode: SelectMode) {
        match mode {
            SelectMode::Only => {
                self.tree_selection = vec![row];
                self.tree_anchor = Some(row);
            }
            SelectMode::Toggle => {
                if let Some(pos) = self.tree_selection.iter().position(|r| *r == row) {
                    self.tree_selection.remove(pos);
                } else {
                    self.tree_selection.push(row);
                }
                self.tree_anchor = Some(row);
            }
            SelectMode::Range => {
                let pos = |r: (Uuid, Uuid)| self.tree_rows.iter().position(|x| *x == r);
                let (Some(to), Some(from)) = (pos(row), self.tree_anchor.and_then(pos).or(pos(row))) else {
                    return self.select_tree_item(row, SelectMode::Only);
                };
                self.tree_selection = self.tree_rows[from.min(to)..=from.max(to)].to_vec();
            }
        }
    }

    /// The selected items that still exist, with whether any of them is a folder. Items inside a
    /// selected folder are left out: they go with the folder.
    fn selected_items(&self) -> (Vec<(Uuid, Uuid)>, bool) {
        let selected: std::collections::HashSet<Uuid> = self.tree_selection.iter().map(|(_, i)| *i).collect();
        let mut folders = false;
        let mut out = Vec::new();
        for &(c, id) in &self.tree_selection {
            let Some(coll) = self.ws.collection(c) else { continue };
            let Some(path) = item_path(&coll.items, id) else { continue };
            // `path` ends with the item itself; anything before it is a parent folder.
            if path[..path.len() - 1].iter().any(|f| selected.contains(f)) {
                continue;
            }
            folders |= matches!(find_item(&coll.items, id), Some(Item::Folder(_)));
            out.push((c, id));
        }
        (out, folders)
    }

    fn apply(&mut self, action: TreeAction) {
        match action {
            TreeAction::Open { collection, request } => self.open_request(collection, request),
            TreeAction::Toggle(id) => {
                if !self.ws.expanded.remove(&id) {
                    self.ws.expanded.insert(id);
                }
            }
            TreeAction::NewRequest { collection, folder } => {
                let req = Request::default();
                let id = req.id;
                if let Some(c) = self.ws.collection_mut(collection) {
                    let target = match folder.and_then(|f| find_item_mut(&mut c.items, f)) {
                        Some(Item::Folder(f)) => &mut f.items,
                        _ => &mut c.items,
                    };
                    target.push(Item::Request(req));
                }
                self.ws.expanded.insert(collection);
                if let Some(f) = folder {
                    self.ws.expanded.insert(f);
                }
                self.open_request(collection, id);
            }
            TreeAction::NewFolder { collection, folder } => {
                let new = Folder::new("New Folder");
                let id = new.id;
                if let Some(c) = self.ws.collection_mut(collection) {
                    let target = match folder.and_then(|f| find_item_mut(&mut c.items, f)) {
                        Some(Item::Folder(f)) => &mut f.items,
                        _ => &mut c.items,
                    };
                    target.push(Item::Folder(new));
                }
                self.ws.expanded.insert(collection);
                if let Some(f) = folder {
                    self.ws.expanded.insert(f);
                }
                self.start_rename(RenameTarget::Item { collection, item: id }, "New Folder".into());
            }
            TreeAction::Rename(target) => {
                let current = match target {
                    RenameTarget::Collection(c) => self.ws.collection(c).map(|c| c.name.clone()),
                    RenameTarget::Item { collection, item } => {
                        self.ws.collection(collection).and_then(|c| find_item(&c.items, item)).map(|i| i.name().to_string())
                    }
                    RenameTarget::Entity(kind, id) | RenameTarget::EntityTitle(kind, id) => self.ws.entity_name(kind, id),
                    RenameTarget::Env(e) | RenameTarget::EnvTitle(e) => self.ws.environments.iter().find(|x| x.id == e).map(|e| e.name.clone()),
                    RenameTarget::TabName => self.tabs.get(self.active).map(|t| t.request.name.clone()),
                };
                if let Some(name) = current {
                    self.start_rename(target, name);
                }
            }
            TreeAction::Duplicate { collection, item } => {
                if let Some(c) = self.ws.collection_mut(collection)
                    && let Some(copy) = find_item(&c.items, item).map(Item::deep_duplicate) {
                        let _ = insert_after(&mut c.items, item, copy);
                    }
            }
            TreeAction::DuplicateCollection(id) => {
                if let Some(c) = self.ws.collection(id) {
                    let copy = Collection {
                        id: Uuid::new_v4(),
                        name: format!("{} Copy", c.name),
                        items: c
                            .items
                            .iter()
                            .map(|i| {
                                let mut d = i.deep_duplicate();
                                d.set_name(i.name().to_string());
                                d
                            })
                            .collect(),
                        ..c.clone()
                    };
                    self.ws.collections.push(copy);
                }
            }
            TreeAction::Delete(confirm) => self.confirm = Some(confirm),
            TreeAction::Select { collection, item, mode } => self.select_tree_item((collection, item), mode),
            TreeAction::ClearSelection => {
                self.tree_selection.clear();
                self.tree_anchor = None;
            }
            TreeAction::EditCollection(id, section) => self.coll_editor = Some((id, section)),
            TreeAction::EditFolder { collection, folder, section } => self.folder_editor = Some((collection, folder, section)),
            TreeAction::Export(id) => self.export_collection(id),
            TreeAction::Run(id) => self.open_runner(id),
            TreeAction::MoveInto { drag, collection, folder } => {
                self.move_item(
                    drag,
                    collection,
                    |items, item| match folder {
                        None => {
                            items.push(item);
                            Ok(())
                        }
                        Some(f) => match find_item_mut(items, f) {
                            Some(Item::Folder(folder)) => {
                                folder.items.push(item);
                                Ok(())
                            }
                            _ => Err(item),
                        },
                    },
                    folder,
                );
            }
            TreeAction::MoveAfter { drag, collection, after } => {
                if drag.item != after {
                    self.move_item(drag, collection, |items, item| insert_after(items, after, item), Some(after));
                }
            }
        }
    }

    /// Move an item between/within collections. `target_ref` must not be inside the dragged item.
    fn move_item(
        &mut self,
        drag: DragItem,
        collection: Uuid,
        insert: impl FnOnce(&mut Vec<Item>, Item) -> Result<(), Item>,
        target_ref: Option<Uuid>,
    ) {
        let Some(src) = self.ws.collection(drag.collection) else { return };
        let Some(dragged) = find_item(&src.items, drag.item) else { return };
        if target_ref.is_some_and(|t| contains_id(dragged, t)) {
            return;
        }
        let Some(item) = self.ws.collection_mut(drag.collection).and_then(|c| remove_item(&mut c.items, drag.item)) else {
            return;
        };
        let result = match self.ws.collection_mut(collection) {
            Some(c) => insert(&mut c.items, item),
            None => Err(item),
        };
        if let Err(item) = result
            && let Some(c) = self.ws.collection_mut(drag.collection) {
                c.items.push(item);
            }
        self.relink_tabs();
    }

    fn export_collection(&mut self, id: Uuid) {
        let Some(c) = self.ws.collection(id) else { return };
        let json = import::export_postman_collection(c);
        let file_name = format!("{}.postman_collection.json", c.name.replace(['/', '\\'], "_"));
        if let Some(path) = rfd::FileDialog::new().set_file_name(&file_name).add_filter("JSON", &["json"]).save_file() {
            match std::fs::write(&path, json) {
                Ok(()) => self.notify(format!("Exported to {}", path.display())),
                Err(e) => self.notify_err(format!("Export failed: {e}")),
            }
        }
    }

    fn export_env(&mut self, id: Uuid) {
        let Some(e) = self.ws.environments.iter().find(|e| e.id == id) else { return };
        let json = import::export_postman_environment(e);
        let file_name = format!("{}.postman_environment.json", e.name.replace(['/', '\\'], "_"));
        if let Some(path) = rfd::FileDialog::new().set_file_name(&file_name).add_filter("JSON", &["json"]).save_file() {
            match std::fs::write(&path, json) {
                Ok(()) => self.notify(format!("Exported to {}", path.display())),
                Err(e) => self.notify_err(format!("Export failed: {e}")),
            }
        }
    }

    /// Import cURL, Postman collection or Postman environment (auto-detected).
    fn import_text(&mut self, text: &str) -> Result<String, String> {
        let trimmed = text.trim_start();
        if trimmed.starts_with("curl") {
            let req = import::parse_curl(trimmed)?;
            self.tabs.push(Tab::new(req, None, None));
            self.active = self.tabs.len() - 1;
            return Ok("Imported cURL into a new tab".into());
        }
        if crate::openapi::looks_like_openapi(trimmed) {
            let c = crate::openapi::to_collection(trimmed)?;
            let spec = Spec { name: c.name.clone(), content: text.trim().to_string(), ..Default::default() };
            let msg = format!("Imported API \"{}\" as a spec and a collection", c.name);
            self.ws.expanded.insert(c.id);
            self.ws.collections.push(c);
            self.ws.specs.push(spec);
            self.sb_open.insert(EntityKind::Spec);
            self.sidebar_mode = SidebarMode::Explorer;
            return Ok(msg);
        }
        let v: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| format!("Not a cURL command, OpenAPI spec or valid JSON: {e}"))?;
        if v.get("info").is_some() || v.get("collection").is_some() {
            let c = import::import_postman_collection(trimmed)?;
            let msg = format!("Imported collection \"{}\"", c.name);
            self.ws.expanded.insert(c.id);
            self.ws.collections.push(c);
            self.sidebar_mode = SidebarMode::Explorer;
            Ok(msg)
        } else if v.get("values").is_some() {
            let e = import::import_postman_environment(trimmed)?;
            let msg = format!("Imported environment \"{}\"", e.name);
            self.ws.environments.push(e);
            self.env_section_open = true;
            Ok(msg)
        } else {
            Err("Unknown format. Supported: cURL, Postman Collection v2.1, Postman Environment, OpenAPI 3 / Swagger 2.".into())
        }
    }

    /// Status bar buttons / shortcuts: open the bottom panel on a tab, or close it if that tab is showing.
    fn toggle_bottom(&mut self, tab: BottomTab) {
        if self.bottom_open && self.bottom_tab == tab {
            self.bottom_open = false;
        } else {
            self.bottom_open = true;
            self.bottom_tab = tab;
        }
    }

    /// "Console | Terminal" switch at the left of the bottom panel toolbar.
    fn bottom_tabs(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (tab, glyph, label) in [(BottomTab::Console, crate::icons::TERMINAL_WINDOW, "Console"), (BottomTab::Terminal, crate::icons::TERMINAL, "Terminal")] {
            if crate::pm::pill_tab(ui, Some(glyph), label, None, false, self.bottom_tab == tab).clicked() {
                self.bottom_tab = tab;
            }
        }
        ui.spacing_mut().item_spacing.x = 8.0;
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.ws.dark_mode = dark;
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let sc = |m, k| KeyboardShortcut::new(m, k);
        let (console, terminal) = ctx.input_mut(|i| {
            (i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::ALT, Key::C)), i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Backtick)))
        });
        if console {
            self.toggle_bottom(BottomTab::Console);
        }
        if terminal {
            self.toggle_bottom(BottomTab::Terminal);
        }
        if self.terminal_focused && self.bottom_open && self.bottom_tab == BottomTab::Terminal {
            return; // keys belong to the shell
        }
        let (force_close, send, save, new, close, sidebar, import) = ctx.input_mut(|i| {
            (
                // Before Ctrl+W, which would match Alt+Ctrl+W too.
                i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::ALT, Key::W)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Enter)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::S)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::T)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::W)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Backslash)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::O)),
            )
        });
        let on_request = !self.side_focused;
        if send && on_request {
            self.send(self.active, ctx);
        }
        if save && on_request {
            self.save_tab(self.active);
        }
        if new {
            self.new_tab();
        }
        if force_close && on_request {
            self.close_tab(self.active, true);
        }
        if close {
            if on_request {
                self.close_tab(self.active, false);
            } else {
                self.close_side_tab();
            }
        }
        if sidebar {
            self.sidebar_visible = !self.sidebar_visible;
        }
        if import {
            self.import = Some(ImportState { text: String::new(), error: None });
        }
    }
}

#[cfg(debug_assertions)]
impl RustmanApp {
    /// Dev-only automation for UI checks without a screenshot tool:
    /// `RUSTMAN_AUTOSEND=1`, `RUSTMAN_OPEN=globals|env|collection|import|code|cookies|settings|history|new_menu|console|spec|mock|dataset|doc|toolset|runner|terminal|oauth|settings_proxy|settings_certs`,
    /// `RUSTMAN_SECTION=body|auth|headers|settings|docs|scripts|scripts_post`, `RUSTMAN_RESP=tests|headers|cookies`,
    /// `RUSTMAN_SCREENSHOT=out.png`, `RUSTMAN_SHOT_FRAME=n`.
    fn debug_hooks(&mut self, ctx: &egui::Context) {
        self.frame_no += 1;
        if self.frame_no == 5 && std::env::var_os("RUSTMAN_AUTOSEND").is_some() {
            self.send(self.active, ctx);
        }
        if self.frame_no == 2 {
            match std::env::var("RUSTMAN_OPEN").as_deref() {
                Ok("globals") => self.open_env(EnvTarget::Globals),
                Ok("env") => {
                    if let Some(id) = self.ws.environments.first().map(|e| e.id) {
                        self.env_section_open = true;
                        self.open_env(EnvTarget::Env(id));
                    }
                }
                Ok("collection") => self.coll_editor = self.ws.collections.first().map(|c| (c.id, CollSection::Variables)),
                Ok("collection_scripts") => self.coll_editor = self.ws.collections.first().map(|c| (c.id, CollSection::Scripts)),
                Ok("folder_auth") => {
                    self.folder_editor = self.ws.collections.iter().find_map(|c| {
                        c.items.iter().find_map(|i| if let Item::Folder(f) = i { Some((c.id, f.id, CollSection::Auth)) } else { None })
                    })
                }
                Ok("import") => self.import = Some(ImportState { text: String::new(), error: None }),
                Ok("code") => self.right_panel = Some(RightPanel::Code),
                Ok("cookies") => self.cookies_open = true,
                Ok("settings") => self.settings_open = true,
                Ok("settings_proxy") => {
                    self.settings_open = true;
                    self.settings_tab = SettingsTab::Proxy;
                }
                Ok("settings_certs") => {
                    self.settings_open = true;
                    self.settings_tab = SettingsTab::Certificates;
                }
                Ok("history") => self.sidebar_mode = SidebarMode::History,
                Ok("envs") => self.env_section_open = true,
                Ok("new_menu") => egui::Popup::open_id(ctx, egui::Id::unique(sidebar::NEW_MENU)),
                Ok("console") => self.toggle_bottom(BottomTab::Console),
                Ok("terminal") => self.toggle_bottom(BottomTab::Terminal),
                Ok(kind @ ("spec" | "mock" | "dataset" | "doc" | "toolset")) => {
                    let kind = match kind {
                        "spec" => EntityKind::Spec,
                        "mock" => EntityKind::Mock,
                        "dataset" => EntityKind::Dataset,
                        "toolset" => EntityKind::Toolset,
                        _ => EntityKind::Document,
                    };
                    self.sb_open.insert(kind);
                    if let Some((id, _)) = self.ws.entities(kind).first().cloned() {
                        self.open_side(SideTab::Entity(kind, id));
                        if kind == EntityKind::Mock {
                            self.start_mock(id, ctx);
                        }
                        if kind == EntityKind::Toolset {
                            if let Some(m) = self.ws.mocks.first().map(|m| m.id) {
                                self.start_mock(m, ctx);
                            }
                            self.start_toolset(id, ctx);
                        }
                    }
                }
                Ok("runner") => {
                    if let Some(c) = std::env::var("RUSTMAN_RUN_COLLECTION").ok().and_then(|n| self.ws.collections.iter().find(|c| c.name == n)).or(self.ws.collections.first()) {
                        let id = c.id;
                        if let Some(m) = self.ws.mocks.first().map(|m| m.id) {
                            self.start_mock(m, ctx);
                        }
                        self.open_runner(id);
                        if let Some(d) = self.ws.datasets.first().map(|d| (d.id, d.rows.len())) && let Some(r) = &mut self.runner {
                            r.dataset = Some(d.0);
                            r.set_iterations(d.1 as u32);
                        }
                        self.start_run(ctx);
                    }
                }
                Ok("delete") => {
                    if let Some(t) = self.tabs.get(self.active)
                        && let Some(c) = t.collection {
                            self.confirm = Some(Confirm::DeleteItem { collection: c, item: t.request.id });
                        }
                }
                Ok("close") => self.confirm = Some(Confirm::CloseTab(self.active)),
                Ok("oauth") => {
                    if let Some(t) = self.tabs.get(self.active) {
                        let target = OAuthTarget::Request { request: t.request.id, collection: t.collection };
                        self.start_oauth(target, request::OAuthOp::New, ctx);
                    }
                }
                _ => {}
            }
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.req_section = match std::env::var("RUSTMAN_SECTION").as_deref() {
                    Ok("body") => ReqSection::Body,
                    Ok("auth") => ReqSection::Auth,
                    Ok("headers") => ReqSection::Headers,
                    Ok("settings") => ReqSection::Settings,
                    Ok("docs") => ReqSection::Docs,
                    Ok("scripts") => ReqSection::Scripts,
                    Ok("scripts_post") => {
                        tab.script_phase = ScriptPhase::Post;
                        ReqSection::Scripts
                    }
                    _ => tab.req_section,
                };
                tab.resp_section = match std::env::var("RUSTMAN_RESP").as_deref() {
                    Ok("tests") => RespSection::Tests,
                    Ok("headers") => RespSection::Headers,
                    Ok("cookies") => RespSection::Cookies,
                    _ => tab.resp_section,
                };
            }
        }
        if self.frame_no == 40 && std::env::var_os("RUSTMAN_CONSOLE_EXPAND").is_some() {
            self.console.expand_all();
        }
        // `RUSTMAN_TERM_INPUT=...` types into the terminal (a literal `\\r` means Enter).
        if self.frame_no == 15
            && let Ok(input) = std::env::var("RUSTMAN_TERM_INPUT")
            && let Some(t) = &mut self.terminal
        {
            t.debug_write(input.replace("\\r", "\r").as_bytes());
        }
        // `RUSTMAN_FOCUS_HEADER=n` focuses the value cell of header row n.
        if let Some(n) = std::env::var("RUSTMAN_FOCUS_HEADER").ok().and_then(|v| v.parse::<usize>().ok())
            && self.frame_no == 10
                && let Some(tab) = self.tabs.get(self.active) {
                    let id = egui::Id::unique(("req", tab.request.id)).with("headers").with((n, "v"));
                    ctx.memory_mut(|m| m.request_focus(id));
                }
        let shot_frame: u64 = std::env::var("RUSTMAN_SHOT_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
        if let Some(path) = std::env::var_os("RUSTMAN_SCREENSHOT") {
            let busy = self.tabs.iter().any(|t| matches!(t.state, TabState::Loading { .. }));
            if self.frame_no == shot_frame && busy {
                self.frame_no -= 1;
            }
            if self.frame_no == shot_frame {
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                        let _ = buf.save(&path);
                    }
                    std::process::exit(0);
                });
            }
            ctx.request_repaint();
        }
    }
}

impl eframe::App for RustmanApp {
    /// Dev-only: `RUSTMAN_CLICKS=frame:button:x,y;…` clicks at a point (button `l`, `r` or `m`;
    /// pressed on that frame, released on the next), to check menus with `RUSTMAN_SCREENSHOT`.
    /// `frame:t:text` types text, `frame:k:Delete` presses a key, `sl` / `cl` are Shift / Ctrl clicks.
    #[cfg(debug_assertions)]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Ok(spec) = std::env::var("RUSTMAN_CLICKS") else { return };
        // Counts input frames: `frame_no` counts passes, and egui may run several per input.
        static INPUT_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let now = INPUT_FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if now == 1 {
            // A fixed size, so the points stay the same from run to run.
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1600.0, 1000.0)));
        }
        // The real mouse must not move the pointer between a scripted press and release.
        raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::PointerGone | egui::Event::MouseMoved(_)));
        for click in spec.split(';') {
            let mut parts = click.trim().splitn(3, ':');
            let (Some(frame), Some(button), Some(at)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let Ok(frame) = frame.parse::<u64>() else { continue };
            if button == "t" {
                if now == frame {
                    raw.events.push(egui::Event::Text(at.to_string()));
                }
                continue;
            }
            if button == "k" {
                if let Some(key) = egui::Key::from_name(at)
                    && (now == frame || now == frame + 1)
                {
                    raw.events.push(egui::Event::Key { key, physical_key: None, pressed: now == frame, repeat: false, modifiers: Default::default() });
                }
                continue;
            }
            // A leading `s` or `c` holds Shift or Ctrl: `sl` is Shift+click.
            let (modifiers, button) = match button.split_at(button.len().saturating_sub(1)) {
                ("s", b) => (egui::Modifiers::SHIFT, b),
                ("c", b) => (egui::Modifiers::COMMAND, b),
                (_, b) => (egui::Modifiers::NONE, b),
            };
            let Some((x, y)) = at.split_once(',').and_then(|(x, y)| Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))) else {
                continue;
            };
            let button = match button {
                "r" => egui::PointerButton::Secondary,
                "m" => egui::PointerButton::Middle,
                _ => egui::PointerButton::Primary,
            };
            let pos = egui::pos2(x, y);
            let pressed = match now {
                n if n == frame => true,
                n if n == frame + 1 => false,
                _ => continue,
            };
            if pressed {
                raw.events.push(egui::Event::PointerMoved(pos));
            }
            raw.events.push(egui::Event::ModifiersChanged(modifiers));
            raw.events.push(egui::Event::PointerButton { pos, button, pressed, modifiers });
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = theme::pal(ui);
        self.receive();
        self.receive_runs();
        self.sync_mocks();
        self.sync_toolsets();
        self.shortcuts(&ctx);

        let flat = |fill| egui::Frame::new().fill(fill);
        egui::Panel::top("menubar")
            .exact_size(34.0)
            .frame(flat(p.menubar).inner_margin(crate::pm::margin(6, 0)))
            .show_separator_line(false)
            .show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("header").exact_size(45.0).frame(flat(p.chrome).inner_margin(crate::pm::margin(10, 0))).show(ui, |ui| self.header(ui));
        egui::Panel::bottom("statusbar").exact_size(26.0).frame(flat(p.bg).inner_margin(crate::pm::margin(6, 0))).show(ui, |ui| self.status_bar(ui));
        self.terminal_focused = false;
        if self.bottom_open {
            egui::Panel::bottom("console")
                .resizable(true)
                .default_size(280.0)
                .size_range(120.0..=800.0)
                .frame(flat(p.bg).inner_margin(crate::pm::margin(10, 2)))
                .show(ui, |ui| {
                    ui.set_min_height(ui.available_height());
                    match self.bottom_tab {
                        BottomTab::Console => self.console_panel(ui),
                        BottomTab::Terminal => self.terminal_panel(ui),
                    }
                });
        }
        if self.sidebar_visible {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(self.ws.sidebar_width.clamp(240.0, 640.0))
                .size_range(240.0..=640.0)
                .frame(flat(p.chrome))
                .show(ui, |ui| {
                    ui.set_min_height(ui.available_height());
                    self.ws.sidebar_width = ui.available_width();
                    self.sidebar(ui)
                });
        }
        self.right_side(ui);
        egui::CentralPanel::default().frame(flat(p.bg)).show(ui, |ui| self.main_area(ui));

        self.windows(&ctx);
        self.ws.zoom = ctx.zoom_factor();
        #[cfg(debug_assertions)]
        self.debug_hooks(&ctx);

        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_if_changed(true);
        } else {
            self.save_if_changed(false);
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }
}

impl RustmanApp {
    /// Shown when every tab is closed, like Postman.
    fn no_tabs_view(&mut self, ui: &mut Ui) {
        let p = theme::pal(ui);
        let avail = ui.available_rect_before_wrap();
        let mut center = ui.new_child(egui::UiBuilder::new().max_rect(avail).layout(egui::Layout::top_down(egui::Align::Center)));
        center.add_space((avail.height() * 0.3).max(10.0));
        crate::pm::icon(&mut center, crate::icons::PAPER_PLANE_TILT, 44.0, p.text3);
        center.add_space(10.0);
        center.label(egui::RichText::new("No open tabs").font(theme::semibold(15.0)).color(p.text));
        center.add_space(4.0);
        let keys = ui.ctx().format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::T));
        center.label(egui::RichText::new(format!("Open a request from the sidebar, or create a new one ({keys}).")).color(p.text2));
        center.add_space(14.0);
        if crate::pm::primary_button(&mut center, "New Request").clicked() {
            self.new_tab();
        }
    }

    fn main_area(&mut self, ui: &mut Ui) {
        let p = theme::pal(ui);
        egui::Panel::top("tabbar").exact_size(40.0).frame(egui::Frame::new().fill(p.chrome)).show(ui, |ui| self.tab_bar(ui));
        let inner = egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            inner.show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                match self.side_shown() {
                    Some(SideTab::Env(target)) => return self.env_view(ui, target),
                    Some(SideTab::Entity(EntityKind::Mock, id)) => return self.mock_view(ui, id),
                    Some(SideTab::Entity(EntityKind::Spec, id)) => return self.spec_view(ui, id),
                    Some(SideTab::Entity(EntityKind::Dataset, id)) => return self.dataset_view(ui, id),
                    Some(SideTab::Entity(EntityKind::Document, id)) => return self.document_view(ui, id),
                    Some(SideTab::Entity(EntityKind::Toolset, id)) => return self.toolset_view(ui, id),
                    Some(SideTab::Runner(id)) => return self.runner_view(ui, id),
                    None => {}
                }
                if self.tabs.get(self.active).is_none() {
                    return self.no_tabs_view(ui);
                }
                self.request_header(ui);
                self.url_bar(ui);
                ui.add_space(6.0);
                let h = ui.available_height();
                egui::Panel::bottom("response")
                    .resizable(true)
                    .default_size((h * 0.5).max(260.0))
                    .size_range(120.0..=(h - 100.0).max(160.0))
                    .frame(egui::Frame::new().fill(p.bg))
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height());
                        self.response_panel(ui);
                    });
                egui::CentralPanel::no_frame().show(ui, |ui| self.request_sections(ui));
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Ids of the folders leading to the item with `id`, then `id` itself.
fn item_path(items: &[Item], id: Uuid) -> Option<Vec<Uuid>> {
    for item in items {
        if item.id() == id {
            return Some(vec![id]);
        }
        if let Item::Folder(f) = item
            && let Some(mut path) = item_path(&f.items, id)
        {
            path.insert(0, f.id);
            return Some(path);
        }
    }
    None
}

fn items_match(items: &[Item], filter: &str) -> bool {
    items.iter().any(|i| match i {
        Item::Request(r) => r.name.to_lowercase().contains(filter) || r.url.to_lowercase().contains(filter),
        Item::Folder(f) => f.name.to_lowercase().contains(filter) || items_match(&f.items, filter),
    })
}

fn collect_folders(items: &[Item], depth: usize, out: &mut Vec<(Uuid, String, usize)>) {
    for i in items {
        if let Item::Folder(f) = i {
            out.push((f.id, f.name.clone(), depth));
            collect_folders(&f.items, depth + 1, out);
        }
    }
}

/// All requests in a tree with their breadcrumb path (header search).
fn collect_requests<'a>(items: &'a [Item], path: &str, out: &mut Vec<(String, &'a Request)>) {
    for i in items {
        match i {
            Item::Request(r) => out.push((path.to_string(), r)),
            Item::Folder(f) => collect_requests(&f.items, &format!("{path} / {}", f.name), out),
        }
    }
}

fn suggest_file_name(d: &ResponseData) -> String {
    let ct = d.content_type();
    let ext = if ct.contains("json") {
        "json"
    } else if ct.contains("html") {
        "html"
    } else if ct.contains("xml") {
        "xml"
    } else if ct.contains("png") {
        "png"
    } else if ct.contains("jpeg") {
        "jpg"
    } else if ct.contains("pdf") {
        "pdf"
    } else if ct.starts_with("text/") {
        "txt"
    } else {
        "bin"
    };
    format!("response.{ext}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn pretty_json_uses_four_spaces() {
        let v: serde_json::Value = serde_json::from_str(r#"{"a":{"b":1}}"#).unwrap();
        assert_eq!(super::pretty_json(&v), "{\n    \"a\": {\n        \"b\": 1\n    }\n}");
    }
}
