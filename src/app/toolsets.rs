//! Toolset tab: pick requests to expose as MCP tools, describe their parameters, run the MCP server.

use std::sync::Arc;

use eframe::egui::{self, Id, RichText, Sense, Ui, pos2, vec2};

use super::*;
use crate::icons;
use crate::mcp::{self, SharedRef, ToolRuntime};
use crate::model::{ToolDef, ToolParam, Toolset};
use crate::pm::{self, paint_text};
use crate::theme::{mono, pal, regular, semibold};

/// A toolset MCP server listening in the background.
pub(super) struct Running {
    shared: SharedRef,
    handle: tokio::task::JoinHandle<()>,
    pub port: u16,
}

/// (collection id, collection name, [(path, request id, method, request name)])
type RequestIndex = Vec<(Uuid, String, Vec<(String, Uuid, Method, String)>)>;

enum Action {
    Add { collection: Uuid, request: Uuid },
    AddAll(Uuid),
    Remove(usize),
    Open { collection: Uuid, request: Uuid },
}

impl RustmanApp {
    /// Resolves the enabled tools of a toolset against the current workspace.
    fn tool_runtimes(&self, ts: &Toolset) -> Vec<ToolRuntime> {
        let empty = Vec::new();
        let env = self.ws.active_env().map(|e| &e.variables).unwrap_or(&empty);
        ts.tools
            .iter()
            .filter(|t| t.enabled && mcp::valid_tool_name(&t.name))
            .filter_map(|t| {
                let c = self.ws.collection(t.collection)?;
                let Some(Item::Request(r)) = find_item(&c.items, t.request) else { return None };
                let base_vars = crate::vars::build_vars(&[&self.ws.globals, &c.variables, env]);
                let description = if t.description.trim().is_empty() {
                    format!("{} {}", r.method.as_str(), crate::vars::substitute(&r.url, &base_vars))
                } else {
                    t.description.trim().to_string()
                };
                Some(ToolRuntime {
                    name: t.name.clone(),
                    description,
                    params: t.params.clone(),
                    request: r.clone(),
                    base_vars,
                    auth: Some(c.inherited_auth(r.id).1.clone()),
                })
            })
            .collect()
    }

    pub(super) fn start_toolset(&mut self, id: Uuid, ctx: &egui::Context) {
        let Some(ts) = self.ws.toolsets.iter().find(|t| t.id == id) else { return };
        let port = ts.port;
        let busy = self.mocks_running.values().any(|r| r.port == port) || self.toolsets_running.values().any(|r| r.port == port);
        if busy {
            self.notify_err(format!("Port {port} is already used by a running mock server or toolset"));
            return;
        }
        let listener = match self.runtime.block_on(mcp::bind(port)) {
            Ok(l) => l,
            Err(e) => {
                self.notify_err(e);
                return;
            }
        };
        let shared: SharedRef = Default::default();
        if let Ok(mut s) = shared.lock() {
            s.server_name = ts.name.clone();
            s.tools = self.tool_runtimes(ts);
        }
        let ctx = ctx.clone();
        let jar = Arc::new(reqwest::cookie::Jar::default());
        let handle = self.runtime.spawn(mcp::serve(listener, shared.clone(), jar, move || ctx.request_repaint()));
        self.toolsets_running.insert(id, Running { shared, handle, port });
        self.notify(format!("MCP server running on {}", mcp::endpoint(port)));
    }

    pub(super) fn stop_toolset(&mut self, id: Uuid) {
        if let Some(r) = self.toolsets_running.remove(&id) {
            r.handle.abort();
        }
    }

    /// Keeps running servers in sync with edits (tools, requests, variables) and stops deleted ones.
    pub(super) fn sync_toolsets(&mut self) {
        let ids: Vec<Uuid> = self.toolsets_running.keys().copied().collect();
        for id in ids {
            let Some(ts) = self.ws.toolsets.iter().find(|t| t.id == id) else {
                self.stop_toolset(id);
                continue;
            };
            let tools = self.tool_runtimes(ts);
            if let Some(r) = self.toolsets_running.get(&id)
                && let Ok(mut s) = r.shared.lock()
            {
                if s.tools != tools {
                    s.tools = tools;
                }
                if s.server_name != ts.name {
                    s.server_name = ts.name.clone();
                }
            }
        }
    }

    fn new_tool(&self, ts: &Toolset, collection: Uuid, request: Uuid) -> Option<ToolDef> {
        let c = self.ws.collection(collection)?;
        let Some(Item::Request(r)) = find_item(&c.items, request) else { return None };
        let mut name = mcp::slug(&r.name);
        let base = name.clone();
        let mut n = 2;
        while ts.tools.iter().any(|t| t.name == name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        // Variables not defined anywhere are what the agent must provide.
        let defined = self.vars_for(Some(collection));
        let params = mcp::request_vars(r)
            .into_iter()
            .filter(|v| !defined.contains_key(v))
            .map(|v| ToolParam { name: v, description: String::new(), required: true })
            .collect();
        Some(ToolDef { name, description: r.description.trim().to_string(), collection, request, params, ..Default::default() })
    }

    pub(super) fn toolset_view(&mut self, ui: &mut Ui, id: Uuid) {
        let ctx = ui.ctx().clone();
        let running = self.toolsets_running.get(&id).map(|r| r.port);
        let (mut start, mut stop, mut copy) = (false, false, None::<String>);
        self.entity_header(ui, EntityKind::Toolset, id, |app, ui| {
            let p = pal(ui);
            match running {
                Some(_) => {
                    if pm::secondary_button(ui, "Stop").clicked() {
                        stop = true;
                    }
                }
                None => {
                    if pm::primary_button(ui, "Start MCP Server").clicked() {
                        start = true;
                    }
                }
            }
            if let Some(ts) = app.ws.toolsets.iter_mut().find(|t| t.id == id) {
                ui.add_enabled(running.is_none(), egui::DragValue::new(&mut ts.port).range(1..=65535).speed(1))
                    .on_hover_text(if running.is_some() { "Stop the server to change the port" } else { "Port" });
                ui.label(RichText::new("Port").color(p.text2));
                let connect = pm::text_button(ui, Some(icons::LINK), "Connect", true);
                egui::Popup::menu(&connect).width(520.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    let cmd = mcp::claude_command(&ts.name, ts.port);
                    let cfg = mcp::json_config(&ts.name, ts.port);
                    ui.label(RichText::new("Connect an AI agent").font(semibold(14.0)).color(p.text));
                    ui.label(RichText::new("Start the server, then add it to your MCP client. Tools run the saved requests with the active environment.").font(regular(12.5)).color(p.text2));
                    ui.add_space(8.0);
                    for (title, text) in [("Claude Code", cmd), ("mcpServers config (Cursor, VS Code, …)", cfg)] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(title).font(semibold(12.5)).color(p.text2));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if pm::icon_button(ui, icons::COPY, 13.0, "Copy").clicked() {
                                    copy = Some(text.clone());
                                }
                            });
                        });
                        egui::Frame::new().fill(p.chrome).corner_radius(4).inner_margin(pm::margin(8, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(RichText::new(&text).font(mono(12.5)).color(p.text)).wrap().selectable(true));
                        });
                        ui.add_space(8.0);
                    }
                });
            }
            if let Some(port) = running {
                ui.add_space(6.0);
                if pm::icon_button(ui, icons::COPY, 14.0, "Copy endpoint").clicked() {
                    copy = Some(mcp::endpoint(port));
                }
                ui.label(RichText::new(mcp::endpoint(port)).font(mono(13.0)).color(p.text));
                pm::icon(ui, icons::BROADCAST, 15.0, p.green_dot).on_hover_text("Running");
            }
        });
        if start {
            self.start_toolset(id, &ctx);
        }
        if stop {
            self.stop_toolset(id);
        }
        if let Some(text) = copy {
            ctx.copy_text(text);
            self.notify("Copied to clipboard");
        }

        let p = pal(ui);
        // Calls log (bottom).
        let log: Vec<mcp::ToolCall> = self.toolsets_running.get(&id).and_then(|r| r.shared.lock().ok().map(|s| s.log.iter().cloned().collect())).unwrap_or_default();
        egui::Panel::bottom(Id::unique(("toolset_log", id)))
            .resizable(true)
            .default_size(170.0)
            .size_range(80.0..=500.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tool calls").font(semibold(13.5)).color(p.text));
                    ui.label(RichText::new(log.len().to_string()).font(regular(12.5)).color(p.text3));
                });
                egui::ScrollArea::vertical().id_salt(("toolset_log_scroll", id)).auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if log.is_empty() {
                        ui.add_space(8.0);
                        let msg = if running.is_some() { "Waiting for an agent to call a tool…" } else { "Start the MCP server and connect an agent (see Connect)." };
                        ui.label(RichText::new(msg).color(p.text3));
                    }
                    for c in &log {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                        let cy = r.center().y;
                        paint_text(ui, pos2(r.left() + 4.0, cy), &c.time.format("%H:%M:%S").to_string(), regular(12.0), p.text3, 70.0);
                        let w = paint_text(ui, pos2(r.left() + 76.0, cy), &c.tool, mono(12.5), p.text, 220.0);
                        paint_text(ui, pos2(r.left() + 90.0 + w, cy), &c.args, mono(12.0), p.text3, r.width() - w - 360.0);
                        let color = if c.ok { p.green_dot } else { p.danger };
                        paint_text(ui, pos2(r.right() - 250.0, cy), &c.summary, regular(12.5), color, 246.0);
                        resp.on_hover_text(&c.args);
                    }
                });
            });

        // Tool list (left).
        let vars_by_tool: Vec<VarMap> = self.ws.toolsets.iter().find(|t| t.id == id).map(|ts| ts.tools.iter().map(|t| self.vars_for(Some(t.collection))).collect()).unwrap_or_default();
        let requests: RequestIndex = self
            .ws
            .collections
            .iter()
            .map(|c| {
                let mut all = Vec::new();
                collect_requests(&c.items, &c.name, &mut all);
                (c.id, c.name.clone(), all.into_iter().map(|(path, r)| (path, r.id, r.method, r.name.clone())).collect())
            })
            .collect();
        let lookup = |cid: Uuid, rid: Uuid| -> Option<Request> {
            let c = self.ws.collection(cid)?;
            match find_item(&c.items, rid) {
                Some(Item::Request(r)) => Some(r.clone()),
                _ => None,
            }
        };
        let linked: Vec<Option<Request>> = self.ws.toolsets.iter().find(|t| t.id == id).map(|ts| ts.tools.iter().map(|t| lookup(t.collection, t.request)).collect()).unwrap_or_default();
        let coll_names: std::collections::HashMap<Uuid, String> = self.ws.collections.iter().map(|c| (c.id, c.name.clone())).collect();

        let sel_id = Id::unique(("toolset_sel", id));
        let mut action = None;
        let Some(ts) = self.ws.toolsets.iter_mut().find(|t| t.id == id) else { return };
        let mut selected = ui.data(|d| d.get_temp::<Uuid>(sel_id)).filter(|s| ts.tools.iter().any(|t| t.id == *s)).or(ts.tools.first().map(|t| t.id));
        egui::Panel::left(Id::unique(("toolset_tools", id)))
            .exact_size(300.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 12, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tools").font(semibold(13.5)).color(p.text));
                    ui.label(RichText::new(ts.tools.iter().filter(|t| t.enabled).count().to_string()).font(regular(12.5)).color(p.text3));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let add = pm::text_button(ui, Some(icons::PLUS), "Add tool", false);
                        egui::Popup::menu(&add).width(340.0).show(|ui| {
                            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                                if requests.iter().all(|c| c.2.is_empty()) {
                                    ui.label(RichText::new("Save requests in a collection first.").color(p.text3));
                                }
                                for (cid, cname, reqs) in &requests {
                                    if reqs.is_empty() {
                                        continue;
                                    }
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(cname).font(semibold(13.0)).color(p.text2));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.small_button("Add all").clicked() {
                                                action = Some(Action::AddAll(*cid));
                                                ui.close();
                                            }
                                        });
                                    });
                                    for (path, rid, method, rname) in reqs {
                                        let used = ts.tools.iter().any(|t| t.request == *rid);
                                        let text = RichText::new(format!("{:<7} {rname}", method.as_str())).font(mono(12.5)).color(if used { p.text3 } else { p.text });
                                        if ui.selectable_label(false, text).on_hover_text(path).clicked() {
                                            action = Some(Action::Add { collection: *cid, request: *rid });
                                            ui.close();
                                        }
                                    }
                                    ui.add_space(4.0);
                                }
                            });
                        });
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt(("toolset_tools_scroll", id)).auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if ts.tools.is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new("No tools yet. Click \"Add tool\" and pick saved requests. Each one becomes a tool an AI agent can call.").color(p.text3));
                    }
                    for (i, tool) in ts.tools.iter_mut().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                        if selected == Some(tool.id) {
                            ui.painter().rect_filled(r, 6.0, p.active);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, p.hover);
                        }
                        let cb = egui::Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(18.0, 18.0));
                        pm::checkbox(ui, cb, Id::unique(("tool_on", tool.id)), &mut tool.enabled).on_hover_text("Enabled");
                        let dim = if tool.enabled { 1.0 } else { 0.45 };
                        let bad = !mcp::valid_tool_name(&tool.name) || linked.get(i).is_none_or(Option::is_none);
                        let color = if bad { p.danger } else { p.text };
                        paint_text(ui, pos2(r.left() + 32.0, r.top() + 13.0), &tool.name, mono(12.5), color.gamma_multiply(dim), r.width() - 40.0);
                        if let Some(Some(req)) = linked.get(i) {
                            let (m, f) = pm::method_label_font(req.method, 9.5);
                            let w = paint_text(ui, pos2(r.left() + 32.0, r.top() + 29.0), &m, f, p.method(req.method).gamma_multiply(dim), 40.0);
                            paint_text(ui, pos2(r.left() + 38.0 + w, r.top() + 29.0), &req.name, regular(12.0), p.text3, r.width() - 50.0 - w);
                        } else {
                            paint_text(ui, pos2(r.left() + 32.0, r.top() + 29.0), "request was deleted", regular(12.0), p.danger, r.width() - 40.0);
                        }
                        if resp.clicked() {
                            selected = Some(tool.id);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Remove").clicked() {
                                action = Some(Action::Remove(i));
                                ui.close();
                            }
                        });
                    }
                });
            });

        // Tool editor (center).
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let Some(i) = selected.and_then(|s| ts.tools.iter().position(|t| t.id == s)) else { return };
            let req = linked.get(i).cloned().flatten();
            let vars = vars_by_tool.get(i).cloned().unwrap_or_default();
            let tool = &mut ts.tools[i];
            egui::ScrollArea::vertical().id_salt(("tool_editor", tool.id)).auto_shrink(false).show(ui, |ui| {
                ui.label(RichText::new("Tool name").font(regular(13.0)).color(p.text2));
                pm::input(ui, Id::unique(("tool_name", tool.id)), &mut tool.name, &VarMap::new(), "get_user", 360.0);
                if !mcp::valid_tool_name(&tool.name) {
                    ui.label(RichText::new("Use 1-64 letters, digits, _ or - (the tool is hidden until fixed).").font(regular(12.5)).color(p.danger));
                }
                ui.add_space(10.0);
                ui.label(RichText::new("Description").font(regular(13.0)).color(p.text2));
                ui.label(RichText::new("Tell the agent what the tool does and when to use it.").font(regular(12.0)).color(p.text3));
                ui.add(
                    egui::TextEdit::multiline(&mut tool.description)
                        .id(Id::unique(("tool_desc", tool.id)))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text(req.as_ref().map(|r| format!("{} {}", r.method.as_str(), r.url)).unwrap_or_default()),
                );
                ui.add_space(10.0);
                ui.label(RichText::new("Request").font(regular(13.0)).color(p.text2));
                ui.horizontal(|ui| match &req {
                    Some(r) => {
                        ui.label(RichText::new(r.method.as_str()).font(semibold(12.0)).color(p.method(r.method)));
                        ui.label(RichText::new(&r.name).color(p.text));
                        ui.label(RichText::new(format!("in {}", coll_names.get(&tool.collection).cloned().unwrap_or_default())).color(p.text3));
                        if pm::text_button(ui, None, "Open", false).clicked() {
                            action = Some(Action::Open { collection: tool.collection, request: tool.request });
                        }
                    }
                    None => {
                        ui.label(RichText::new("The linked request no longer exists. Remove this tool or add the request again.").color(p.danger));
                    }
                });
                ui.add(egui::Label::new(RichText::new(req.as_ref().map(|r| r.url.clone()).unwrap_or_default()).font(mono(12.5)).color(p.text3)).wrap());
                ui.add_space(12.0);
                ui.label(RichText::new("Parameters").font(semibold(13.5)).color(p.text));
                ui.label(
                    RichText::new("Variables used by the request. Tick the ones the agent provides; the others keep their environment / collection value.")
                        .font(regular(12.5))
                        .color(p.text3),
                );
                ui.add_space(6.0);
                let mut names = req.as_ref().map(mcp::request_vars).unwrap_or_default();
                for prm in &tool.params {
                    if !names.contains(&prm.name) {
                        names.push(prm.name.clone());
                    }
                }
                if names.is_empty() {
                    ui.label(RichText::new("This request uses no {{variables}}, so the tool takes no input. Add {{name}} placeholders to the URL, headers or body to make it configurable.").color(p.text3));
                }
                egui::Grid::new(("tool_params", tool.id)).num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
                    for h in ["Agent input", "Variable", "Description", "Required"] {
                        ui.label(RichText::new(h).font(semibold(12.5)).color(p.text2));
                    }
                    ui.end_row();
                    for name in &names {
                        let pos = tool.params.iter().position(|x| &x.name == name);
                        let mut on = pos.is_some();
                        if ui.checkbox(&mut on, "").changed() {
                            if on {
                                tool.params.push(ToolParam { name: name.clone(), description: String::new(), required: true });
                            } else if let Some(k) = pos {
                                tool.params.remove(k);
                            }
                        }
                        let used = req.as_ref().is_some_and(|r| mcp::request_vars(r).contains(name));
                        ui.vertical(|ui| {
                            ui.set_width(240.0);
                            ui.add(egui::Label::new(RichText::new(format!("{{{{{name}}}}}")).font(mono(12.5)).color(if used { p.text } else { p.danger })).truncate());
                            let hint = match vars.get(name) {
                                _ if !used => "not used by the request".to_string(),
                                Some(v) => format!("default: {v}"),
                                None => "no default value".to_string(),
                            };
                            ui.add(egui::Label::new(RichText::new(hint).font(regular(11.5)).color(p.text3)).truncate());
                        });
                        match tool.params.iter_mut().find(|x| &x.name == name) {
                            Some(prm) => {
                                ui.add_sized(vec2(340.0, 26.0), egui::TextEdit::singleline(&mut prm.description).id(Id::unique(("tp_desc", tool.id, name.as_str()))).hint_text("What value to pass"));
                                ui.checkbox(&mut prm.required, "");
                            }
                            None => {
                                ui.label("");
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
                if !tool.params.is_empty() {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Input schema sent to the agent").font(regular(12.5)).color(p.text3));
                    let schema = serde_json::to_string_pretty(&mcp::input_schema(&tool.params)).unwrap_or_default();
                    egui::Frame::new().fill(p.chrome).corner_radius(4).inner_margin(pm::margin(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(schema).font(mono(12.0)).color(p.text2)).selectable(true));
                    });
                }
            });
        });
        if let Some(s) = selected {
            ui.data_mut(|d| d.insert_temp(sel_id, s));
        }

        match action {
            Some(Action::Add { collection, request }) => {
                let ts = self.ws.toolsets.iter().find(|t| t.id == id).cloned();
                if let Some(tool) = ts.and_then(|ts| self.new_tool(&ts, collection, request))
                    && let Some(ts) = self.ws.toolsets.iter_mut().find(|t| t.id == id)
                {
                    ui.data_mut(|d| d.insert_temp(sel_id, tool.id));
                    ts.tools.push(tool);
                }
            }
            Some(Action::AddAll(cid)) => {
                let ids: Vec<Uuid> = requests.iter().find(|c| c.0 == cid).map(|c| c.2.iter().map(|r| r.1).collect()).unwrap_or_default();
                for rid in ids {
                    let Some(ts) = self.ws.toolsets.iter().find(|t| t.id == id).cloned() else { break };
                    if ts.tools.iter().any(|t| t.request == rid) {
                        continue;
                    }
                    if let Some(tool) = self.new_tool(&ts, cid, rid)
                        && let Some(ts) = self.ws.toolsets.iter_mut().find(|t| t.id == id)
                    {
                        ts.tools.push(tool);
                    }
                }
            }
            Some(Action::Remove(i)) => {
                if let Some(ts) = self.ws.toolsets.iter_mut().find(|t| t.id == id)
                    && i < ts.tools.len()
                {
                    ts.tools.remove(i);
                }
            }
            Some(Action::Open { collection, request }) => self.open_request(collection, request),
            None => {}
        }
    }
}
