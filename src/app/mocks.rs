//! Mock server tab: routes editor, start/stop, live request log.

use eframe::egui::{self, Id, RichText, Sense, Ui, pos2, vec2};

use super::*;
use crate::icons;
use crate::mock::{self, SharedRef};
use crate::model::MockRoute;
use crate::pm::{self, TableOpts, paint_text};
use crate::theme::{mono, pal, regular, semibold};

/// A mock server listening in the background.
pub(super) struct Running {
    shared: SharedRef,
    handle: tokio::task::JoinHandle<()>,
    pub port: u16,
}

impl RustmanApp {
    pub(super) fn start_mock(&mut self, id: Uuid, ctx: &egui::Context) {
        let Some(m) = self.ws.mocks.iter().find(|m| m.id == id) else { return };
        let port = m.port;
        if self.mocks_running.values().any(|r| r.port == port) || self.toolsets_running.values().any(|r| r.port == port) {
            self.notify_err(format!("Port {port} is already used by a running mock server or toolset"));
            return;
        }
        let listener = match self.runtime.block_on(mock::bind(port)) {
            Ok(l) => l,
            Err(e) => {
                self.notify_err(e);
                return;
            }
        };
        let shared: SharedRef = Default::default();
        if let Ok(mut s) = shared.lock() {
            s.routes = m.routes.clone();
        }
        let ctx = ctx.clone();
        let handle = self.runtime.spawn(mock::serve(listener, shared.clone(), move || ctx.request_repaint()));
        self.mocks_running.insert(id, Running { shared, handle, port });
        self.notify(format!("Mock server running on {}", mock::base_url(port)));
    }

    pub(super) fn stop_mock(&mut self, id: Uuid) {
        if let Some(r) = self.mocks_running.remove(&id) {
            r.handle.abort();
        }
    }

    /// Pushes route edits to running servers and stops servers whose mock was deleted.
    pub(super) fn sync_mocks(&mut self) {
        let ids: Vec<Uuid> = self.mocks_running.keys().copied().collect();
        for id in ids {
            match self.ws.mocks.iter().find(|m| m.id == id) {
                None => self.stop_mock(id),
                Some(m) => {
                    if let Some(r) = self.mocks_running.get(&id)
                        && let Ok(mut s) = r.shared.lock()
                        && s.routes != m.routes
                    {
                        s.routes = m.routes.clone();
                    }
                }
            }
        }
    }

    pub(super) fn mock_view(&mut self, ui: &mut Ui, id: Uuid) {
        let ctx = ui.ctx().clone();
        let running = self.mocks_running.get(&id).map(|r| r.port);
        let (mut start, mut stop, mut copy) = (false, false, false);
        self.entity_header(ui, EntityKind::Mock, id, |app, ui| {
            let p = pal(ui);
            match running {
                Some(_) => {
                    if pm::secondary_button(ui, "Stop").clicked() {
                        stop = true;
                    }
                }
                None => {
                    if pm::primary_button(ui, "Start").clicked() {
                        start = true;
                    }
                }
            }
            if let Some(m) = app.ws.mocks.iter_mut().find(|m| m.id == id) {
                ui.add_enabled(running.is_none(), egui::DragValue::new(&mut m.port).range(1..=65535).speed(1))
                    .on_hover_text(if running.is_some() { "Stop the server to change the port" } else { "Port" });
                ui.label(RichText::new("Port").color(p.text2));
            }
            if let Some(port) = running {
                ui.add_space(8.0);
                if pm::icon_button(ui, icons::COPY, 14.0, "Copy URL").clicked() {
                    copy = true;
                }
                ui.label(RichText::new(mock::base_url(port)).font(mono(13.0)).color(p.text));
                pm::icon(ui, icons::BROADCAST, 15.0, p.green_dot).on_hover_text("Running");
            }
        });
        if start {
            self.start_mock(id, &ctx);
        }
        if stop {
            self.stop_mock(id);
        }
        if copy && let Some(port) = running {
            ctx.copy_text(mock::base_url(port));
            self.notify("Copied to clipboard");
        }

        // Request log (bottom).
        let p = pal(ui);
        let log: Vec<mock::MockHit> = self.mocks_running.get(&id).and_then(|r| r.shared.lock().ok().map(|s| s.log.iter().cloned().collect())).unwrap_or_default();
        let mut clear_log = false;
        egui::Panel::bottom(Id::unique(("mock_log", id)))
            .resizable(true)
            .default_size(180.0)
            .size_range(80.0..=500.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Requests").font(semibold(13.5)).color(p.text));
                    ui.label(RichText::new(log.len().to_string()).font(regular(12.5)).color(p.text3));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !log.is_empty() && pm::text_button(ui, Some(icons::TRASH), "Clear", false).clicked() {
                            clear_log = true;
                        }
                    });
                });
                egui::ScrollArea::vertical().id_salt(("mock_log_scroll", id)).auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if log.is_empty() {
                        let msg = if running.is_some() { "Waiting for requests…" } else { "Start the server, then call it from any HTTP client (or from a request tab)." };
                        ui.add_space(8.0);
                        ui.label(RichText::new(msg).color(p.text3));
                    }
                    for h in &log {
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                        let cy = r.center().y;
                        paint_text(ui, pos2(r.left() + 4.0, cy), &h.time.format("%H:%M:%S").to_string(), regular(12.0), p.text3, 70.0);
                        let method = Method::parse(&h.method);
                        let color = method.map(|m| p.method(m)).unwrap_or(p.text2);
                        paint_text(ui, pos2(r.left() + 76.0, cy), &h.method, semibold(11.5), color, 60.0);
                        paint_text(ui, pos2(r.left() + 140.0, cy), &h.path, mono(12.5), p.text, r.width() - 330.0);
                        let status = format!("{}{}", h.status, if h.matched { "" } else { "  no matching route" });
                        let sc = p.status(h.status).1;
                        paint_text(ui, pos2(r.right() - 180.0, cy), &status, regular(12.5), sc, 140.0);
                        paint_text(ui, pos2(r.right() - 40.0, cy), &format!("{} ms", h.duration.as_millis()), regular(12.0), p.text3, 40.0);
                    }
                });
            });
        if clear_log && let Some(r) = self.mocks_running.get(&id) && let Ok(mut s) = r.shared.lock() {
            s.log.clear();
        }

        let vars = self.vars_for(None);
        let Some(m) = self.ws.mocks.iter_mut().find(|m| m.id == id) else { return };
        let sel_id = Id::unique(("mock_sel", id));
        let mut selected = ui.data(|d| d.get_temp::<Uuid>(sel_id)).filter(|s| m.routes.iter().any(|r| r.id == *s)).or(m.routes.first().map(|r| r.id));

        // Route list (left).
        let mut add = false;
        let mut remove = None;
        let mut dup = None;
        egui::Panel::left(Id::unique(("mock_routes", id)))
            .exact_size(300.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 0, right: 12, top: 0, bottom: 0 }))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Routes").font(semibold(13.5)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if pm::icon_button(ui, icons::PLUS, 14.0, "Add route").clicked() {
                            add = true;
                        }
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt(("mock_routes_scroll", id)).auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (i, route) in m.routes.iter_mut().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                        let is_sel = selected == Some(route.id);
                        if is_sel {
                            ui.painter().rect_filled(r, 6.0, p.active);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 6.0, p.hover);
                        }
                        let cb = egui::Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(18.0, 18.0));
                        pm::checkbox(ui, cb, Id::unique(("mock_route_on", route.id)), &mut route.enabled).on_hover_text("Enabled");
                        let dim = if route.enabled { 1.0 } else { 0.45 };
                        let label = mock::method_label(route.method);
                        let mc = route.method.map(|m| p.method(m)).unwrap_or(p.text2).gamma_multiply(dim);
                        paint_text(ui, pos2(r.left() + 32.0, r.center().y), label, semibold(11.0), mc, 52.0);
                        paint_text(ui, pos2(r.left() + 90.0, r.center().y), &route.path, mono(12.5), p.text.gamma_multiply(dim), r.width() - 136.0);
                        let st = route.status.to_string();
                        paint_text(ui, pos2(r.right() - 34.0, r.center().y), &st, regular(12.0), p.status(route.status).1.gamma_multiply(dim), 34.0);
                        if resp.clicked() {
                            selected = Some(route.id);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Duplicate").clicked() {
                                dup = Some(i);
                                ui.close();
                            }
                            if ui.button("Delete").clicked() {
                                remove = Some(i);
                                ui.close();
                            }
                        });
                    }
                    if m.routes.is_empty() {
                        ui.label(RichText::new("No routes. Click + to add one.").color(p.text3));
                    }
                });
            });

        // Route editor (center).
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let Some(route) = selected.and_then(|s| m.routes.iter_mut().find(|r| r.id == s)) else { return };
            route_editor(ui, route, &vars);
        });

        if add {
            let r = MockRoute { path: "/new-route".into(), ..Default::default() };
            selected = Some(r.id);
            m.routes.push(r);
        }
        if let Some(i) = dup {
            let mut r = m.routes[i].clone();
            r.id = Uuid::new_v4();
            selected = Some(r.id);
            m.routes.insert(i + 1, r);
        }
        if let Some(i) = remove {
            m.routes.remove(i);
        }
        if let Some(s) = selected {
            ui.data_mut(|d| d.insert_temp(sel_id, s));
        }
    }
}

fn route_editor(ui: &mut Ui, route: &mut MockRoute, vars: &VarMap) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let label = mock::method_label(route.method);
        let btn = pm::text_button(ui, None, label, true);
        egui::Popup::menu(&btn).show(|ui| {
            if ui.selectable_label(route.method.is_none(), "ANY").clicked() {
                route.method = None;
                ui.close();
            }
            for m in Method::ALL {
                if ui.selectable_label(route.method == Some(m), RichText::new(m.as_str()).color(p.method(m))).clicked() {
                    route.method = Some(m);
                    ui.close();
                }
            }
        });
        let w = (ui.available_width() - 330.0).max(160.0);
        pm::input(ui, Id::unique(("mock_path", route.id)), &mut route.path, &VarMap::new(), "/users/:id", w);
        ui.label(RichText::new("Status").color(p.text2));
        ui.add(egui::DragValue::new(&mut route.status).range(100..=599));
        ui.label(RichText::new("Delay").color(p.text2));
        ui.add(egui::DragValue::new(&mut route.delay_ms).range(0..=60_000).speed(10).suffix(" ms"));
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("Path params (:id) and query params can be used in the body and headers as {{id}}; dynamic variables like {{$guid}} work too.")
            .font(regular(12.5))
            .color(p.text3),
    );
    ui.add_space(8.0);
    let tab_id = Id::unique(("mock_tab", route.id));
    let mut headers_tab = ui.data(|d| d.get_temp::<bool>(tab_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if pm::pill_tab(ui, None, "Body", None, false, !headers_tab).clicked() {
            headers_tab = false;
        }
        let n = route.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).count();
        if pm::pill_tab(ui, None, "Headers", Some(n), false, headers_tab).clicked() {
            headers_tab = true;
        }
        if !headers_tab {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pm::text_button(ui, None, "Beautify", false).clicked() {
                    route.body = super::request::beautify(&route.body);
                }
            });
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, headers_tab));
    ui.add_space(6.0);
    if headers_tab {
        egui::ScrollArea::vertical().id_salt(("mock_headers", route.id)).auto_shrink(false).show(ui, |ui| {
            pm::kv_table(ui, Id::unique(("mock_hdr", route.id)), &mut route.headers, vars, TableOpts { with_desc: false, ..Default::default() });
        });
    } else {
        let json = route.body.trim_start().starts_with(['{', '[']);
        super::request::code_editor(ui, Id::unique(("mock_body", route.id)), &mut route.body, json, vars);
    }
}
