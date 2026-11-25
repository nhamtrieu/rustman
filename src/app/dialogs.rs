//! Windows and modal dialogs.

use eframe::egui::{self, Align2, Id, RichText, Sense, Ui, pos2, vec2};
use crate::vars::VarMap;

use super::*;
use crate::icons;
use crate::pm::{self, TableOpts, paint_text};
use crate::theme::{icon_font, medium, pal, pal_ctx, regular, semibold, UI_SIZE};

fn field_label(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    ui.label(RichText::new(text).font(regular(13.0)).color(p.text2));
    ui.add_space(2.0);
}

/// Bordered dropdown-looking button for dialogs.
fn select_button(ui: &mut Ui, label: &str, width: f32) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    ui.painter().rect(rect, 4.0, p.bg, egui::Stroke::new(1.0, if resp.hovered() { p.text3 } else { p.input_border }), egui::StrokeKind::Inside);
    paint_text(ui, pos2(rect.left() + 10.0, rect.center().y), label, regular(UI_SIZE), p.text, width - 36.0);
    ui.painter().text(pos2(rect.right() - 16.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
    resp
}

impl RustmanApp {
    pub(super) fn windows(&mut self, ctx: &egui::Context) {
        self.collection_window(ctx);
        self.save_as_window(ctx);
        self.import_window(ctx);
        self.oauth_window(ctx);
        self.cookies_window(ctx);
        self.settings_window(ctx);
        self.about_window(ctx);
        self.confirm_modal(ctx);
        self.toast_area(ctx);
    }

    fn collection_window(&mut self, ctx: &egui::Context) {
        let Some((id, mut section)) = self.coll_editor else { return };
        let vars = self.vars_for(Some(id));
        let script_phase = &mut self.coll_script_phase;
        let Some(c) = self.ws.collections.iter_mut().find(|c| c.id == id) else {
            self.coll_editor = None;
            return;
        };
        let mut open = true;
        let mut oauth_op = None;
        egui::Window::new(RichText::new(c.name.clone()).font(semibold(15.0)))
            .id(Id::unique("coll_window"))
            .open(&mut open)
            .default_size([860.0, 480.0])
            .resizable(true)
            .show(ctx, |ui| {
                let p = pal(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (s, l) in [(CollSection::Overview, "Overview"), (CollSection::Auth, "Authorization"), (CollSection::Scripts, "Scripts"), (CollSection::Variables, "Variables")] {
                        if pm::pill_tab(ui, None, l, None, false, section == s).clicked() {
                            section = s;
                        }
                    }
                });
                ui.add_space(10.0);
                if section == CollSection::Scripts {
                    // The editor scrolls by itself and fills the window.
                    ui.label(RichText::new("These scripts run for every request in this collection, before the request's own scripts.").color(p.text2).font(regular(13.0)));
                    ui.add_space(8.0);
                    // Keep the editor from growing the window.
                    let size = vec2(ui.available_width(), ui.available_height().max(300.0));
                    ui.allocate_ui(size, |ui| {
                        super::request::scripts_editor(ui, Id::unique(("coll_scripts", id)), &mut c.pre_request_script, &mut c.post_response_script, script_phase);
                    });
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| match section {
                    CollSection::Scripts => {}
                    CollSection::Overview => {
                        field_label(ui, "Name");
                        pm::input(ui, Id::unique(("coll_name", id)), &mut c.name, &vars, "Collection name", 420.0);
                        ui.add_space(10.0);
                        field_label(ui, "Description");
                        ui.add(egui::TextEdit::multiline(&mut c.description).hint_text("Add a description").desired_rows(8).desired_width(f32::INFINITY));
                    }
                    CollSection::Auth => {
                        ui.label(RichText::new("This authorization method will be used for every request in this collection that inherits auth from its parent.").color(p.text2).font(regular(13.0)));
                        ui.add_space(10.0);
                        oauth_op = super::request::auth_form(ui, Id::unique(("coll_auth", id)), &mut c.auth, &vars, false, None).oauth;
                    }
                    CollSection::Variables => {
                        ui.label(RichText::new("These variables are specific to this collection and its requests. The active environment overrides them.").color(p.text2).font(regular(13.0)));
                        ui.add_space(10.0);
                        pm::kv_table(ui, Id::unique(("coll_vars", id)), &mut c.variables, &vars, TableOpts { key_label: "Variable", ..Default::default() });
                    }
                });
            });
        self.coll_editor = if open { Some((id, section)) } else { None };
        if let Some(op) = oauth_op {
            self.start_oauth(OAuthTarget::Collection(id), op, ctx);
        }
    }

    fn save_as_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.save_as.take() else { return };
        let mut done = false;
        let mut cancel = false;
        let vars = VarMap::new();
        let (_, close) = pm::modal(ctx, Id::unique("save_as_modal"), "Save request", 560.0, |ui| {
            field_label(ui, "Request name");
            pm::input(ui, Id::unique("save_as_name"), &mut st.name, &vars, "Request name", 520.0);
            ui.add_space(12.0);
            field_label(ui, "Save to");
            let coll_name = st.collection.and_then(|c| self.ws.collection(c)).map(|c| c.name.clone()).unwrap_or_else(|| "New collection".into());
            let btn = select_button(ui, &coll_name, 520.0);
            egui::Popup::menu(&btn).width(520.0).show(|ui| {
                for c in &self.ws.collections {
                    if ui.selectable_label(st.collection == Some(c.id), &c.name).clicked() {
                        st.collection = Some(c.id);
                        st.folder = None;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.selectable_label(st.collection.is_none(), "+ New collection").clicked() {
                    st.collection = None;
                    ui.close();
                }
            });
            ui.add_space(8.0);
            match st.collection.and_then(|c| self.ws.collection(c)) {
                None => {
                    field_label(ui, "Collection name");
                    pm::input(ui, Id::unique("save_as_new_coll"), &mut st.new_collection_name, &vars, "Collection name", 520.0);
                }
                Some(c) => {
                    let mut folders = Vec::new();
                    collect_folders(&c.items, 0, &mut folders);
                    if !folders.is_empty() {
                        field_label(ui, "Folder");
                        let name = st.folder.and_then(|f| folders.iter().find(|(id, _, _)| *id == f)).map(|(_, n, _)| n.clone()).unwrap_or_else(|| c.name.clone());
                        let fb = select_button(ui, &name, 520.0);
                        egui::Popup::menu(&fb).width(520.0).show(|ui| {
                            if ui.selectable_label(st.folder.is_none(), &c.name).clicked() {
                                st.folder = None;
                                ui.close();
                            }
                            for (id, n, depth) in &folders {
                                if ui.selectable_label(st.folder == Some(*id), format!("{}{}", "    ".repeat(depth + 1), n)).clicked() {
                                    st.folder = Some(*id);
                                    ui.close();
                                }
                            }
                        });
                    }
                }
            }
            pm::modal_buttons(ui, |ui| {
                if pm::modal_button(ui, "Save", pm::ButtonKind::Primary).clicked() {
                    done = true;
                }
                if pm::modal_button(ui, "Cancel", pm::ButtonKind::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
        cancel |= close;
        if done {
            let coll_id = match st.collection {
                Some(c) => c,
                None => {
                    let name = if st.new_collection_name.trim().is_empty() { "My Collection".to_string() } else { st.new_collection_name.trim().to_string() };
                    let c = Collection { name, ..Default::default() };
                    let id = c.id;
                    self.ws.collections.push(c);
                    id
                }
            };
            let Some(tab) = self.tabs.get_mut(st.tab) else { return };
            tab.request.id = Uuid::new_v4();
            tab.request.name = if st.name.trim().is_empty() { "Untitled Request".into() } else { st.name.trim().into() };
            let req = tab.request.clone();
            if let Some(c) = self.ws.collection_mut(coll_id) {
                let target = match st.folder.and_then(|f| find_item_mut(&mut c.items, f)) {
                    Some(Item::Folder(f)) => &mut f.items,
                    _ => &mut c.items,
                };
                target.push(Item::Request(req.clone()));
            }
            self.ws.expanded.insert(coll_id);
            if let Some(f) = st.folder {
                self.ws.expanded.insert(f);
            }
            tab.collection = Some(coll_id);
            tab.saved = Some(req);
            self.notify("Saved");
        } else if !cancel {
            self.save_as = Some(st);
        }
    }

    fn import_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.import.take() else { return };
        let mut open = true;
        let mut do_import = false;
        let mut picked: Option<String> = None;
        // Files dropped onto the window are imported directly.
        let dropped: Vec<egui::DroppedFileHandle> = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            match f.bytes().map(|b| String::from_utf8_lossy(&b).into_owned()) {
                Ok(t) => picked = Some(t),
                Err(e) => st.error = Some(format!("Cannot read {}: {e}", f.path().display())),
            }
        }
        egui::Window::new(RichText::new("Import").font(semibold(15.0)))
            .open(&mut open)
            .default_size([640.0, 420.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                let p = pal(ui);
                ui.add(
                    egui::TextEdit::multiline(&mut st.text)
                        .code_editor()
                        .hint_text(RichText::new("Paste cURL, Postman collection / environment JSON or an OpenAPI spec…").color(p.text3))
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(12.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::click());
                let stroke = egui::Stroke::new(1.0, if resp.hovered() { p.text3 } else { p.input_border });
                ui.painter().rect(rect, 8.0, p.bg, stroke, egui::StrokeKind::Inside);
                ui.painter().text(rect.center() - vec2(0.0, 26.0), Align2::CENTER_CENTER, icons::DOWNLOAD_SIMPLE, icon_font(30.0), p.text2);
                ui.painter().text(rect.center() + vec2(0.0, 10.0), Align2::CENTER_CENTER, "Drop a file here to import", medium(UI_SIZE), p.text);
                ui.painter().text(rect.center() + vec2(0.0, 32.0), Align2::CENTER_CENTER, "or click to select a file", regular(13.0), p.text2);
                if resp.clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("JSON / YAML", &["json", "yaml", "yml"]).add_filter("All files", &["*"]).pick_file() {
                        match std::fs::read_to_string(&path) {
                            Ok(text) => picked = Some(text),
                            Err(e) => st.error = Some(format!("Cannot read file: {e}")),
                        }
                    }
                ui.add_space(8.0);
                ui.label(RichText::new("Supported: Postman Collection v2.1, Postman Environment, cURL, OpenAPI 3 / Swagger 2 (JSON or YAML)").font(regular(12.0)).color(p.text3));
                if let Some(e) = &st.error {
                    ui.label(RichText::new(e).color(p.danger));
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pm::primary_button(ui, "Import").clicked() && !st.text.trim().is_empty() {
                        do_import = true;
                    }
                });
            });
        let text = picked.or_else(|| if do_import { Some(st.text.clone()) } else { None });
        if let Some(text) = text {
            match self.import_text(&text) {
                Ok(msg) => {
                    self.notify(msg);
                    return;
                }
                Err(e) => st.error = Some(e),
            }
        }
        if open {
            self.import = Some(st);
        }
    }

    fn cookies_window(&mut self, ctx: &egui::Context) {
        if !self.cookies_open {
            return;
        }
        let mut open = true;
        let mut clear = false;
        let last: Vec<Vec<String>> = match self.tabs.get(self.active).map(|t| &t.state) {
            Some(TabState::Done(v)) => v.data.cookies.iter().map(|c| vec![c.name.clone(), c.value.clone(), c.domain.clone(), c.path.clone()]).collect(),
            _ => Vec::new(),
        };
        egui::Window::new(RichText::new("Manage Cookies").font(semibold(15.0)))
            .open(&mut open)
            .default_size([620.0, 320.0])
            .collapsible(false)
            .show(ctx, |ui| {
                let p = pal(ui);
                ui.label(RichText::new("Cookies set by servers are kept for this session and sent automatically with matching requests.").color(p.text2).font(regular(13.0)));
                ui.add_space(10.0);
                if last.is_empty() {
                    ui.label(RichText::new("The last response did not set any cookies.").color(p.text3));
                } else {
                    ui.label(RichText::new("Set by the last response").font(medium(13.0)).color(p.text));
                    ui.add_space(4.0);
                    pm::ro_table(ui, &["Name", "Value", "Domain", "Path"], &last, &[1.0, 2.0, 1.2, 0.6], false);
                }
                ui.add_space(14.0);
                if pm::secondary_button(ui, "Clear all cookies").clicked() {
                    clear = true;
                }
            });
        if clear {
            self.jar = Arc::new(reqwest::cookie::Jar::default());
            self.notify("Cookies cleared");
        }
        self.cookies_open = open;
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut dark = self.ws.dark_mode;
        let mut tab = self.settings_tab;
        let before = self.ws.network.clone();
        let net = &mut self.ws.network;
        egui::Window::new(RichText::new("Settings").font(semibold(15.0)))
            .open(&mut open)
            .default_size([720.0, 640.0])
            .collapsible(false)
            .show(ctx, |ui| {
                let p = pal(ui);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (t, l) in [(SettingsTab::General, "General"), (SettingsTab::Proxy, "Proxy"), (SettingsTab::Certificates, "Certificates")] {
                        if pm::pill_tab(ui, None, l, None, false, tab == t).clicked() {
                            tab = t;
                        }
                    }
                });
                ui.add_space(12.0);
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| match tab {
                    SettingsTab::General => {
                        field_label(ui, "Theme");
                        ui.horizontal(|ui| {
                            if pm::radio(ui, dark, "Dark").clicked() {
                                dark = true;
                            }
                            if pm::radio(ui, !dark, "Light").clicked() {
                                dark = false;
                            }
                        });
                        ui.add_space(12.0);
                        field_label(ui, "Zoom");
                        let mut zoom = ctx.zoom_factor() * 100.0;
                        if ui.add(egui::Slider::new(&mut zoom, 50.0..=200.0).suffix("%").step_by(10.0)).changed() {
                            ctx.set_zoom_factor(zoom / 100.0);
                        }
                        ui.add_space(12.0);
                        field_label(ui, "Data folder");
                        ui.label(RichText::new(storage::data_dir().display().to_string()).color(p.text));
                    }
                    SettingsTab::Proxy => proxy_settings(ui, net),
                    SettingsTab::Certificates => certificate_settings(ui, net),
                });
            });
        if dark != self.ws.dark_mode {
            self.set_dark(ctx, dark);
        }
        if self.ws.network != before {
            http::set_network(&self.ws.network);
        }
        self.settings_tab = tab;
        self.settings_open = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut open = true;
        egui::Window::new(RichText::new("About Rustman").font(semibold(15.0))).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let p = pal(ui);
            ui.label(RichText::new(format!("Rustman {}", env!("CARGO_PKG_VERSION"))).font(semibold(UI_SIZE)));
            ui.label(RichText::new("A native API client written in Rust (egui + reqwest).").color(p.text2));
            ui.add_space(8.0);
            ui.label(RichText::new("Fonts: Inter, IBM Plex Mono (SIL OFL 1.1). Icons: Phosphor (MIT).").font(regular(12.0)).color(p.text3));
        });
        self.about_open = open;
    }

    fn confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm else { return };
        // Titles and wording follow Postman's dialogs.
        let item_name = |name: Option<&str>| name.unwrap_or("this item").to_string();
        let (title, body, ok_label, danger) = match confirm {
            Confirm::CloseTab(_) => (
                "Do you want to save?".to_string(),
                "This tab has unsaved changes which will be lost if you choose to close it. Save these changes to avoid losing your work.".to_string(),
                "Save changes",
                false,
            ),
            Confirm::CloseTabs { keep } => {
                let n = self.tabs.iter().filter(|t| Some(t.request.id) != keep && t.is_dirty()).count();
                let tabs = if n == 1 { "1 tab has".to_string() } else { format!("{n} tabs have") };
                (
                    "Do you want to save?".to_string(),
                    format!("{tabs} unsaved changes which will be lost if you choose to close them. Save these changes to avoid losing your work."),
                    "Save changes",
                    false,
                )
            }
            Confirm::DeleteCollection(id) => {
                let n = item_name(self.ws.collection(id).map(|c| c.name.as_str()));
                (format!("Delete \"{n}\"?"), format!("Are you sure you want to delete {n}? Everything in it will be deleted as well."), "Delete", true)
            }
            Confirm::DeleteItem { collection, item } => {
                let n = item_name(self.ws.collection(collection).and_then(|c| find_item(&c.items, item)).map(|i| i.name()));
                (format!("Delete \"{n}\"?"), format!("Are you sure you want to delete {n}?"), "Delete", true)
            }
            Confirm::DeleteSelection => {
                let (items, folders) = self.selected_items();
                let n = items.len();
                let extra = if folders { " Folders are deleted with everything in them." } else { "" };
                (format!("Delete {n} items?"), format!("Are you sure you want to delete these {n} items?{extra}"), "Delete", true)
            }
            Confirm::DeleteEnv(id) => {
                let n = item_name(self.ws.environments.iter().find(|e| e.id == id).map(|e| e.name.as_str()));
                (format!("Delete \"{n}\"?"), format!("Are you sure you want to delete {n}?"), "Delete", true)
            }
            Confirm::DeleteEntity(kind, id) => {
                let n = item_name(self.ws.entity_name(kind, id).as_deref());
                (format!("Delete \"{n}\"?"), format!("Are you sure you want to delete the {} {n}?", kind.singular()), "Delete", true)
            }
            Confirm::ClearHistory => ("Clear history?".to_string(), "Are you sure you want to clear all history? This can't be undone.".to_string(), "Clear", true),
        };
        let mut choice = None;
        let mut discard = false;
        let (_, close) = pm::modal(ctx, Id::unique("confirm_modal"), &title, 550.0, |ui| {
            pm::modal_text(ui, &body);
            pm::modal_buttons(ui, |ui| {
                let kind = if danger { pm::ButtonKind::Danger } else { pm::ButtonKind::Primary };
                if pm::modal_button(ui, ok_label, kind).clicked() {
                    choice = Some(true);
                }
                if pm::modal_button(ui, "Cancel", pm::ButtonKind::Secondary).clicked() {
                    choice = Some(false);
                }
                if matches!(confirm, Confirm::CloseTab(_) | Confirm::CloseTabs { .. }) {
                    // "Don't save" sits on the far left, like Postman.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        if pm::modal_button(ui, "Don't save", pm::ButtonKind::Secondary).clicked() {
                            discard = true;
                        }
                    });
                }
            });
        });
        if close && choice.is_none() && !discard {
            choice = Some(false);
        }
        if discard {
            match confirm {
                Confirm::CloseTab(i) => self.close_tab(i, true),
                Confirm::CloseTabs { keep } => {
                    self.close_tabs(keep, true);
                }
                _ => {}
            }
            self.confirm = None;
            return;
        }
        match choice {
            Some(true) => {
                self.confirm = None;
                match confirm {
                    Confirm::CloseTab(i) => {
                        self.save_tab(i);
                        if !self.tabs.get(i).is_some_and(|t| t.is_dirty()) && self.save_as.is_none() {
                            self.close_tab(i, true);
                        }
                    }
                    Confirm::CloseTabs { keep } => self.save_and_close_tabs(keep),
                    Confirm::DeleteCollection(id) => {
                        self.ws.collections.retain(|c| c.id != id);
                        self.relink_tabs();
                    }
                    Confirm::DeleteItem { collection, item } => {
                        if let Some(c) = self.ws.collection_mut(collection) {
                            remove_item(&mut c.items, item);
                        }
                        self.relink_tabs();
                    }
                    Confirm::DeleteSelection => {
                        let (items, _) = self.selected_items();
                        for (collection, item) in items {
                            if let Some(c) = self.ws.collection_mut(collection) {
                                remove_item(&mut c.items, item);
                            }
                        }
                        self.tree_selection.clear();
                        self.tree_anchor = None;
                        self.relink_tabs();
                    }
                    Confirm::DeleteEnv(id) => {
                        self.ws.environments.retain(|e| e.id != id);
                        if self.ws.active_environment == Some(id) {
                            self.ws.active_environment = None;
                        }
                        if self.side_tab == Some(SideTab::Env(EnvTarget::Env(id))) {
                            self.close_side_tab();
                        }
                    }
                    Confirm::DeleteEntity(kind, id) => self.delete_entity(kind, id),
                    Confirm::ClearHistory => self.ws.history.clear(),
                }
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    /// "Save changes" after closing several tabs: saves the tabs whose request is in a collection
    /// and closes them. New requests stay open, and Save As opens for the first one.
    fn save_and_close_tabs(&mut self, keep: Option<Uuid>) {
        for i in 0..self.tabs.len() {
            let tab = &self.tabs[i];
            if Some(tab.request.id) == keep || !tab.is_dirty() {
                continue;
            }
            let (id, req) = (tab.request.id, tab.request.clone());
            if let Some(c) = tab.collection.and_then(|c| self.ws.collection_mut(c))
                && let Some(item) = find_item_mut(&mut c.items, id)
            {
                *item = Item::Request(req.clone());
                self.tabs[i].saved = Some(req);
            }
        }
        let left = self.close_tabs(keep, false);
        if let Some(i) = self.tabs.iter().position(|t| Some(t.request.id) != keep && t.is_dirty()) {
            // New requests need a name and a collection.
            self.active = i;
            self.open_save_as(i);
            if left > 1 {
                self.notify(format!("{left} new requests are still open: save them with Save As"));
            }
        }
    }

    fn toast_area(&mut self, ctx: &egui::Context) {
        let Some((msg, at, is_err)) = &self.toast else { return };
        if at.elapsed() > Duration::from_secs(3) {
            self.toast = None;
            return;
        }
        let p = pal_ctx(ctx);
        egui::Area::new(Id::unique("toast")).anchor(Align2::LEFT_BOTTOM, [16.0, -40.0]).order(egui::Order::Tooltip).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(p.chrome).inner_margin(pm::margin(14, 10)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (glyph, color) = if *is_err { (icons::WARNING, p.danger) } else { (icons::CHECK_CIRCLE, p.green_dot) };
                    pm::icon(ui, glyph, 17.0, color);
                    ui.label(RichText::new(msg.as_str()).color(p.text));
                });
            });
        });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

// ---------------------------------------------------------------------------
// Settings: proxy and certificates
// ---------------------------------------------------------------------------

fn hint(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    ui.add(egui::Label::new(RichText::new(text).font(regular(12.0)).color(p.text2)).wrap());
}

/// Toggle with a bold title and a description under it.
fn toggle_row(ui: &mut Ui, on: &mut bool, title: &str, description: &str) {
    let p = pal(ui);
    ui.horizontal_top(|ui| {
        pm::toggle(ui, on);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(medium(UI_SIZE)).color(p.text));
            hint(ui, description);
        });
    });
    ui.add_space(12.0);
}

fn labeled_checkbox(ui: &mut Ui, id: Id, on: &mut bool, label: &str) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 24.0), Sense::hover());
    pm::checkbox(ui, rect, id, on);
    ui.label(RichText::new(label).font(regular(UI_SIZE)).color(p.text));
    ui.add_space(10.0);
}

fn proxy_settings(ui: &mut Ui, n: &mut NetSettings) {
    let vars = VarMap::new();
    let w = (ui.available_width() - 40.0).clamp(240.0, 560.0);
    toggle_row(
        ui,
        &mut n.system_proxy,
        "Use the system proxy",
        "Requests go through the proxy set in the HTTP_PROXY, HTTPS_PROXY and NO_PROXY environment variables, if any.",
    );
    toggle_row(ui, &mut n.custom_proxy, "Use custom proxy configuration", "Send requests through the proxy below. It takes precedence over the system proxy.");
    if !n.custom_proxy {
        return;
    }
    ui.indent("custom_proxy", |ui| {
        field_label(ui, "Proxy type");
        ui.horizontal(|ui| {
            labeled_checkbox(ui, Id::unique("proxy_http"), &mut n.proxy_http, "HTTP");
            labeled_checkbox(ui, Id::unique("proxy_https"), &mut n.proxy_https, "HTTPS");
        });
        hint(ui, "Which requests go through the proxy, by the scheme of the request URL.");
        ui.add_space(10.0);
        field_label(ui, "Proxy server");
        ui.horizontal(|ui| {
            pm::input(ui, Id::unique("proxy_host"), &mut n.proxy_host, &vars, "proxy.example.com", w - 110.0);
            ui.label(":");
            pm::input(ui, Id::unique("proxy_port"), &mut n.proxy_port, &vars, "8080", 90.0);
        });
        ui.add_space(10.0);
        toggle_row(ui, &mut n.proxy_auth, "Proxy auth", "Send a username and password to the proxy (Basic).");
        if n.proxy_auth {
            field_label(ui, "Username");
            pm::input(ui, Id::unique("proxy_user"), &mut n.proxy_username, &vars, "Username", w);
            ui.add_space(8.0);
            field_label(ui, "Password");
            pm::input(ui, Id::unique("proxy_pass"), &mut n.proxy_password, &vars, "Password", w);
            ui.add_space(10.0);
        }
        field_label(ui, "Proxy bypass");
        pm::input(ui, Id::unique("proxy_bypass"), &mut n.proxy_bypass, &vars, "localhost, *.internal.example.com, 10.0.0.0/8", w);
        ui.add_space(2.0);
        hint(ui, "Comma-separated hosts that are reached directly: names (a name also covers its subdomains), *.domain, IP addresses or CIDR ranges.");
    });
}

/// Path input with a "Select file" button and a warning when the file does not exist.
fn file_field(ui: &mut Ui, id: Id, path: &mut String, hint_text: &str, width: f32) {
    let vars = VarMap::new();
    ui.horizontal(|ui| {
        pm::input(ui, id, path, &vars, hint_text, width - 110.0);
        if pm::secondary_button(ui, "Select file").clicked()
            && let Some(f) = rfd::FileDialog::new().add_filter("Certificates", &["pem", "crt", "cer", "key"]).add_filter("All files", &["*"]).pick_file()
        {
            *path = f.display().to_string();
        }
    });
    if !path.trim().is_empty() && !std::path::Path::new(path.trim()).is_file() {
        ui.label(RichText::new(format!("{} File not found", icons::WARNING)).font(regular(12.0)).color(egui::Color32::from_rgb(0xe1, 0x4c, 0x3b)));
    }
}

fn certificate_settings(ui: &mut Ui, n: &mut NetSettings) {
    let p = pal(ui);
    let vars = VarMap::new();
    let w = (ui.available_width() - 40.0).clamp(280.0, 600.0);
    toggle_row(
        ui,
        &mut n.ca_enabled,
        "CA certificates",
        "Trust the certificates in this PEM file in addition to the built-in ones, e.g. a company root CA or a self-signed server certificate.",
    );
    if n.ca_enabled {
        ui.indent("ca", |ui| file_field(ui, Id::unique("ca_path"), &mut n.ca_path, "/path/to/ca-bundle.pem", w));
        ui.add_space(12.0);
    }
    pm::hline(ui);
    ui.add_space(12.0);
    ui.label(RichText::new("Client certificates").font(medium(UI_SIZE)).color(p.text));
    hint(
        ui,
        "Sent to HTTPS hosts that ask for one (mutual TLS). Host: api.example.com, *.example.com, optionally with :port. Files: a PEM certificate and an unencrypted PEM private key (or one file with both).",
    );
    ui.add_space(10.0);
    let mut remove = None;
    for (i, c) in n.client_certs.iter_mut().enumerate() {
        let id = Id::unique(("client_cert", c.id));
        egui::Frame::new().stroke(egui::Stroke::new(1.0, p.border)).corner_radius(6).inner_margin(pm::margin(12, 10)).show(ui, |ui| {
            ui.set_width(w + 20.0);
            ui.horizontal(|ui| {
                pm::toggle(ui, &mut c.enabled);
                ui.add_space(6.0);
                let title = if c.host.trim().is_empty() { "New certificate" } else { c.host.trim() };
                ui.label(RichText::new(title).font(medium(UI_SIZE)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pm::icon_button(ui, icons::TRASH, 15.0, "Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.add_space(8.0);
            field_label(ui, "Host");
            pm::input(ui, id.with("host"), &mut c.host, &vars, "api.example.com:443", w);
            ui.add_space(8.0);
            field_label(ui, "CRT file");
            file_field(ui, id.with("crt"), &mut c.cert_path, "/path/to/client.crt", w);
            ui.add_space(8.0);
            field_label(ui, "KEY file");
            file_field(ui, id.with("key"), &mut c.key_path, "/path/to/client.key", w);
        });
        ui.add_space(10.0);
    }
    if let Some(i) = remove {
        n.client_certs.remove(i);
    }
    if pm::secondary_button(ui, &format!("{} Add Certificate", icons::PLUS)).clicked() {
        n.client_certs.push(ClientCert::default());
    }
}
