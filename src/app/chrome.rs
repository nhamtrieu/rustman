//! Window chrome: menu bar, header, tab bar, status bar, right side panel.

use eframe::egui::{self, Align, Align2, Id, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use super::*;
use crate::icons;
use crate::pm::{self, paint_text};
use crate::theme::{icon_font, pal, regular, semibold};

impl RustmanApp {
    // -----------------------------------------------------------------------
    // Menu bar: File Edit View Help
    // -----------------------------------------------------------------------

    pub(super) fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.style_mut().visuals.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
            ui.menu_button("File", |ui| {
                if ui.button("New Request            Ctrl+T").clicked() {
                    self.new_tab();
                    ui.close();
                }
                if ui.button("New Collection").clicked() {
                    self.new_collection();
                    ui.close();
                }
                if ui.button("New Environment").clicked() {
                    self.new_environment();
                    ui.close();
                }
                ui.separator();
                if ui.button("Import…                      Ctrl+O").clicked() {
                    self.import = Some(ImportState::default());
                    ui.close();
                }
                if ui.button("Save                           Ctrl+S").clicked() {
                    self.save_tab(self.active);
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    self.open_save_as(self.active);
                    ui.close();
                }
                ui.separator();
                if ui.button("Close Tab                   Ctrl+W").clicked() {
                    self.close_tab(self.active, false);
                    ui.close();
                }
                if ui.button("Settings").clicked() {
                    self.settings_open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Clear History").clicked() {
                    self.confirm = Some(Confirm::ClearHistory);
                    ui.close();
                }
                if ui.button("Clear Cookies").clicked() {
                    self.jar = Arc::new(reqwest::cookie::Jar::default());
                    self.notify("Cookies cleared");
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                let zoom = ctx.zoom_factor();
                if ui.button("Toggle Sidebar          Ctrl+\\").clicked() {
                    self.sidebar_visible = !self.sidebar_visible;
                    ui.close();
                }
                if ui.button(if self.ws.dark_mode { "Light Theme" } else { "Dark Theme" }).clicked() {
                    let dark = !self.ws.dark_mode;
                    self.set_dark(&ctx, dark);
                    ui.close();
                }
                ui.separator();
                if ui.button("Zoom In                    Ctrl +").clicked() {
                    ctx.set_zoom_factor((zoom + 0.1).min(3.0));
                }
                if ui.button("Zoom Out                 Ctrl -").clicked() {
                    ctx.set_zoom_factor((zoom - 0.1).max(0.5));
                }
                if ui.button("Actual Size              Ctrl 0").clicked() {
                    ctx.set_zoom_factor(1.0);
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Open Data Folder").clicked() {
                    let dir = storage::data_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    let opener = if cfg!(target_os = "macos") { "open" } else if cfg!(windows) { "explorer" } else { "xdg-open" };
                    let _ = std::process::Command::new(opener).arg(&dir).spawn();
                    ui.close();
                }
                if ui.button("About Rustman").clicked() {
                    self.about_open = true;
                    ui.close();
                }
            });
        });
    }

    // -----------------------------------------------------------------------
    // Header: navigation, workspace, search, settings
    // -----------------------------------------------------------------------

    pub(super) fn header(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if pm::icon_button(ui, icons::ARROW_LEFT, 16.0, "Previous tab").clicked() {
                if self.side_focused {
                    self.side_focused = false;
                } else if self.active > 0 {
                    self.active -= 1;
                }
            }
            if pm::icon_button(ui, icons::ARROW_RIGHT, 16.0, "Next tab").clicked() {
                if self.active + 1 < self.tabs.len() && !self.side_focused {
                    self.active += 1;
                } else if self.side_tab.is_some() {
                    self.side_focused = true;
                }
            }
            ui.add_space(6.0);
            let ws_btn = pm::text_button(ui, Some(icons::LOCK_SIMPLE), "My Workspace", true);
            egui::Popup::menu(&ws_btn).show(|ui| {
                if ui.button("Import…").clicked() {
                    self.import = Some(ImportState::default());
                    ui.close();
                }
                if ui.button("New Collection").clicked() {
                    self.new_collection();
                    ui.close();
                }
                if ui.button("New Environment").clicked() {
                    self.new_environment();
                    ui.close();
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if pm::icon_button(ui, icons::GEAR_SIX, 17.0, "Settings").clicked() {
                    self.settings_open = true;
                }
                let dark = self.ws.dark_mode;
                if pm::icon_button(ui, if dark { icons::SUN } else { icons::MOON }, 17.0, "Toggle theme").clicked() {
                    self.set_dark(&ctx, !dark);
                }
            });
        });

        // Centered search box.
        let rect = Rect::from_center_size(full.center(), vec2(320.0f32.min(full.width() * 0.3), 32.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        let id = Id::unique("header_search");
        let resp = pm::search_box(&mut child, id, &mut self.header_search, "Search", rect.width(), rect.height());
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.header_search.clear();
        }
        let query = self.header_search.trim().to_lowercase();
        if !query.is_empty() {
            let mut hits: Vec<(Uuid, String, Request)> = Vec::new();
            for c in &self.ws.collections {
                let mut all = Vec::new();
                collect_requests(&c.items, &c.name, &mut all);
                for (path, r) in all {
                    if r.name.to_lowercase().contains(&query) || r.url.to_lowercase().contains(&query) {
                        hits.push((c.id, path, r.clone()));
                    }
                }
            }
            let mut open: Option<(Uuid, Uuid)> = None;
            egui::Area::new(Id::unique("header_search_results"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(rect.left(), rect.bottom() + 4.0))
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_width(rect.width().max(380.0));
                        if hits.is_empty() {
                            ui.label(RichText::new("No results found").color(p.text2));
                        }
                        for (cid, path, r) in hits.iter().take(12) {
                            let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(row, 4.0, p.hover);
                            }
                            let (m, f) = pm::method_label_font(r.method, 10.0);
                            paint_text(ui, pos2(row.left() + 8.0, row.top() + 14.0), &m, f, p.method(r.method), 40.0);
                            paint_text(ui, pos2(row.left() + 48.0, row.top() + 14.0), &r.name, regular(14.0), p.text, row.width() - 56.0);
                            paint_text(ui, pos2(row.left() + 48.0, row.top() + 32.0), path, regular(12.0), p.text3, row.width() - 56.0);
                            if resp.clicked() {
                                open = Some((*cid, r.id));
                            }
                        }
                    });
                });
            if let Some((c, r)) = open {
                self.open_request(c, r);
                self.header_search.clear();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Request tabs
    // -----------------------------------------------------------------------

    pub(super) fn tab_bar(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let full = ui.max_rect();
        let right_w = 290.0;
        let tabs_rect = Rect::from_min_max(full.min, pos2(full.right() - right_w, full.bottom()));
        let right_rect = Rect::from_min_max(pos2(full.right() - right_w, full.top()), full.max);

        let mut activate = None;
        let mut close = None;
        let mut menu_action = None;
        let mut new_tab = false;
        let mut env_tab_click = false;
        let mut env_tab_close = false;
        let env_tab = self.side_tab_label();
        let side_focused = self.side_focused && env_tab.is_some();
        let mut child = ui.new_child(UiBuilder::new().max_rect(tabs_rect).layout(Layout::left_to_right(Align::Center)));
        egui::ScrollArea::horizontal()
            .id_salt("tabs_scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let n = (self.tabs.len() + usize::from(env_tab.is_some())).max(1) as f32;
                let w = ((tabs_rect.width() - 44.0) / n).clamp(130.0, 220.0);
                for (i, tab) in self.tabs.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
                    let selected = i == self.active && !side_focused;
                    let inner = rect.shrink2(vec2(3.0, 4.0));
                    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
                    if selected {
                        ui.painter().rect_filled(inner, 6.0, p.active);
                    } else if hovered {
                        ui.painter().rect_filled(inner, 6.0, p.hover);
                    }
                    // Separator to the right of inactive tabs.
                    let next_selected = if i + 1 == self.tabs.len() { side_focused } else { i + 1 == self.active && !side_focused };
                    let has_next = i + 1 < self.tabs.len() || env_tab.is_some();
                    if !selected && !next_selected && has_next {
                        ui.painter().vline(rect.right(), rect.top() + 12.0..=rect.bottom() - 12.0, Stroke::new(1.0, p.border));
                    }
                    let cy = rect.center().y;
                    let mut x = inner.left() + 12.0;
                    let (m, f) = pm::method_label_font(tab.request.method, 11.0);
                    let mcolor = if selected { p.method(tab.request.method) } else { p.method(tab.request.method).gamma_multiply(0.75) };
                    x += paint_text(ui, pos2(x, cy), &m, f, mcolor, 50.0) + 6.0;
                    let name_color = if selected { p.text } else { p.text2 };
                    paint_text(ui, pos2(x, cy), &tab.request.name, regular(13.0), name_color, inner.right() - 30.0 - x);
                    let close_rect = Rect::from_center_size(pos2(inner.right() - 14.0, cy), vec2(20.0, 20.0));
                    let close_resp = ui.interact(close_rect, Id::unique(("tab_close", tab.request.id)), Sense::click());
                    if hovered || close_resp.hovered() {
                        if close_resp.hovered() {
                            ui.painter().rect_filled(close_rect, 4.0, p.active.gamma_multiply(1.4));
                        }
                        ui.painter().text(close_rect.center(), Align2::CENTER_CENTER, icons::X, icon_font(13.0), p.text2);
                    } else if tab.is_dirty() {
                        ui.painter().circle_filled(close_rect.center(), 3.5, p.orange);
                    }
                    if close_resp.clicked() || resp.middle_clicked() {
                        close = Some(i);
                    } else if resp.clicked() {
                        activate = Some(i);
                    }
                    let resp = resp.on_hover_text(if tab.request.url.is_empty() { tab.request.name.clone() } else { tab.request.url.clone() });
                    resp.context_menu(|ui| {
                        if let Some(a) = tab_menu(ui, i) {
                            menu_action = Some(a);
                            ui.close();
                        }
                    });
                }
                if let Some((glyph, name)) = &env_tab {
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
                    let inner = rect.shrink2(vec2(3.0, 4.0));
                    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
                    if side_focused {
                        ui.painter().rect_filled(inner, 6.0, p.active);
                    } else if hovered {
                        ui.painter().rect_filled(inner, 6.0, p.hover);
                    }
                    let cy = rect.center().y;
                    let color = if side_focused { p.text } else { p.text2 };
                    ui.painter().text(pos2(inner.left() + 20.0, cy), Align2::CENTER_CENTER, *glyph, icon_font(15.0), color);
                    // Italic like Postman's preview tab.
                    let x = inner.left() + 34.0;
                    let job = egui::text::LayoutJob::single_section(
                        name.clone(),
                        egui::TextFormat { font_id: regular(13.0), color, italics: true, ..Default::default() },
                    );
                    let mut job = job;
                    job.wrap = egui::text::TextWrapping::truncate_at_width(inner.right() - 30.0 - x);
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(pos2(x, cy - galley.size().y / 2.0), galley, color);
                    let close_rect = Rect::from_center_size(pos2(inner.right() - 14.0, cy), vec2(20.0, 20.0));
                    let close_resp = ui.interact(close_rect, Id::unique("env_tab_close"), Sense::click());
                    if hovered || close_resp.hovered() {
                        if close_resp.hovered() {
                            ui.painter().rect_filled(close_rect, 4.0, p.active.gamma_multiply(1.4));
                        }
                        ui.painter().text(close_rect.center(), Align2::CENTER_CENTER, icons::X, icon_font(13.0), p.text2);
                    }
                    if close_resp.clicked() || resp.middle_clicked() {
                        env_tab_close = true;
                    } else if resp.clicked() {
                        env_tab_click = true;
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Close Tab").clicked() {
                            env_tab_close = true;
                            ui.close();
                        }
                    });
                }
                ui.add_space(6.0);
                if pm::icon_button(ui, icons::PLUS, 16.0, "New tab (Ctrl+T)").clicked() {
                    new_tab = true;
                }
            });
        if let Some(i) = activate {
            self.active = i;
            self.side_focused = false;
        }
        if env_tab_click {
            self.side_focused = true;
        }
        if env_tab_close {
            self.close_side_tab();
        }
        if new_tab {
            self.new_tab();
        }
        if let Some(i) = close {
            self.close_tab(i, false);
        }
        if let Some(a) = menu_action {
            let id = |i: usize| self.tabs.get(i).map(|t| t.request.id);
            match a {
                TabMenu::New => self.new_tab(),
                TabMenu::Duplicate(i) => self.duplicate_tab(i),
                TabMenu::Close(i) => self.close_tab(i, false),
                TabMenu::ForceClose(i) => self.close_tab(i, true),
                TabMenu::CloseOthers(i) => self.close_tabs_confirm(id(i)),
                TabMenu::CloseAll => self.close_tabs_confirm(None),
                TabMenu::ForceCloseAll => {
                    self.close_tabs(None, true);
                }
            }
        }

        // Right side: tab list, environment selector, environment quick look.
        let mut right = ui.new_child(UiBuilder::new().max_rect(right_rect).layout(Layout::left_to_right(Align::Center)));
        right.spacing_mut().item_spacing.x = 2.0;
        let list = pm::icon_button(&mut right, icons::CARET_DOWN, 14.0, "Open tabs");
        egui::Popup::menu(&list).show(|ui| {
            let mut go = None;
            for (i, t) in self.tabs.iter().enumerate() {
                if ui.selectable_label(i == self.active, format!("{}  {}", t.request.method.as_str(), t.request.name)).clicked() {
                    go = Some(i);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Close all saved tabs").clicked() {
                self.tabs.retain(|t| t.is_dirty());
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
                ui.close();
            }
            if let Some(i) = go {
                self.active = i;
                self.side_focused = false;
            }
        });
        let (div, _) = right.allocate_exact_size(vec2(9.0, 40.0), Sense::hover());
        right.painter().vline(div.center().x, div.y_range(), Stroke::new(1.0, p.border));
        let env_name = self.ws.active_env().map(|e| e.name.clone()).unwrap_or_else(|| "No Environment".into());
        let env_btn = env_selector(&mut right, &env_name, 200.0);
        egui::Popup::menu(&env_btn).width(240.0).show(|ui| {
            if ui.selectable_label(self.ws.active_environment.is_none(), "No Environment").clicked() {
                self.ws.active_environment = None;
                ui.close();
            }
            for e in &self.ws.environments {
                if ui.selectable_label(self.ws.active_environment == Some(e.id), &e.name).clicked() {
                    self.ws.active_environment = Some(e.id);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Globals").clicked() {
                self.open_env(EnvTarget::Globals);
                ui.close();
            }
            if ui.button("New Environment").clicked() {
                self.new_environment();
                ui.close();
            }
        });
        if pm::icon_button(&mut right, icons::EYE, 16.0, "Environment quick look").clicked() {
            self.open_env(self.ws.active_environment.map(EnvTarget::Env).unwrap_or(EnvTarget::Globals));
        }
    }

    // -----------------------------------------------------------------------
    // Status bar
    // -----------------------------------------------------------------------

    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if status_button(ui, Some(icons::SIDEBAR_SIMPLE), "").on_hover_text("Toggle sidebar (Ctrl+\\)").clicked() {
                self.sidebar_visible = !self.sidebar_visible;
            }
            if status_button(ui, Some(icons::ARROW_SQUARE_IN), "Import").clicked() {
                self.import = Some(ImportState::default());
            }
            if status_button(ui, Some(icons::COOKIE), "Cookies").clicked() {
                self.cookies_open = true;
            }
            if status_button(ui, Some(icons::TERMINAL), "Terminal").on_hover_text("Toggle terminal (Ctrl+`)").clicked() {
                self.toggle_bottom(BottomTab::Terminal);
            }
            let errors = self.console.error_count();
            let label = if errors > 0 { format!("Console ({errors})") } else { "Console".to_string() };
            if status_button(ui, Some(icons::TERMINAL_WINDOW), &label).on_hover_text("Toggle console (Ctrl+Alt+C)").clicked() {
                self.toggle_bottom(BottomTab::Console);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let dark = self.ws.dark_mode;
                if status_button(ui, Some(if dark { icons::SUN } else { icons::MOON }), "").on_hover_text("Toggle theme").clicked() {
                    self.set_dark(&ctx, !dark);
                }
                if status_button(ui, None, &format!("{:.0}%", ctx.zoom_factor() * 100.0)).on_hover_text("Zoom (Ctrl + / Ctrl - / Ctrl 0)").clicked() {
                    ctx.set_zoom_factor(1.0);
                }
                if status_button(ui, None, "Globals").clicked() {
                    self.open_env(EnvTarget::Globals);
                }
            });
        });
    }

    // -----------------------------------------------------------------------
    // Right side: icon rail or panel (code snippet / request info)
    // -----------------------------------------------------------------------

    pub(super) fn right_side(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let frame = egui::Frame::new().fill(p.chrome).inner_margin(pm::margin(6, 8));
        let Some(panel) = self.right_panel else {
            egui::Panel::right("rail").exact_size(46.0).frame(frame).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if pm::icon_button(ui, icons::CODE, 17.0, "Code snippet").clicked() {
                        self.right_panel = Some(RightPanel::Code);
                    }
                    if pm::icon_button(ui, icons::INFO, 17.0, "Request info").clicked() {
                        self.right_panel = Some(RightPanel::Info);
                    }
                });
            });
            return;
        };
        egui::Panel::right("right_panel").resizable(true).default_size(420.0).size_range(300.0..=800.0).frame(frame.inner_margin(pm::margin(12, 8))).show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (kind, glyph, tip) in [(RightPanel::Code, icons::CODE, "Code snippet"), (RightPanel::Info, icons::INFO, "Request info")] {
                    if pm::icon_button_sel(ui, glyph, 17.0, tip, panel == kind).clicked() {
                        self.right_panel = if panel == kind { None } else { Some(kind) };
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if pm::icon_button(ui, icons::X, 15.0, "Close").clicked() {
                        self.right_panel = None;
                    }
                });
            });
            ui.add_space(8.0);
            match panel {
                RightPanel::Code => self.code_panel(ui),
                RightPanel::Info => self.info_panel(ui),
            }
        });
    }

    fn code_panel(&mut self, ui: &mut Ui) {
        use crate::codegen::{self, Lang};
        let p = pal(ui);
        let Some(tab) = self.tabs.get(self.active) else { return };
        let vars = self.vars_for(tab.collection);
        let inherited = self.inherited_auth(tab.collection, tab.request.id).map(|(a, _)| a);
        let opts = self.ws.snippet;
        let code = match codegen::build(&tab.request, &vars, inherited.as_ref()) {
            Ok(snippet) => codegen::generate(&snippet, &opts),
            Err(e) => format!("// {e}"),
        };
        ui.label(RichText::new("Code snippet").font(semibold(15.0)).color(p.text));
        ui.add_space(6.0);
        let mut copied = false;
        ui.horizontal(|ui| {
            let btn = pm::text_button(ui, None, opts.lang.label(), true);
            // Scroll the selected language into view once, when the menu opens.
            let scrolled_id = Id::unique("lang_menu_scrolled");
            let shown = egui::Popup::menu(&btn).width(260.0).show(|ui| {
                let scrolled = ui.data(|d| d.get_temp::<bool>(scrolled_id)).unwrap_or(false);
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    for l in Lang::ALL {
                        let selected = self.ws.snippet.lang == l;
                        let r = lang_item(ui, l.label(), selected);
                        if selected && !scrolled {
                            r.scroll_to_me(Some(Align::Center));
                        }
                        if r.clicked() {
                            self.ws.snippet.lang = l;
                            ui.close();
                        }
                    }
                });
                ui.data_mut(|d| d.insert_temp(scrolled_id, true));
            });
            if shown.is_none() {
                ui.data_mut(|d| d.remove::<bool>(scrolled_id));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if pm::icon_button(ui, icons::COPY, 16.0, "Copy snippet").clicked() {
                    ui.ctx().copy_text(code.clone());
                    copied = true;
                }
                let gear = pm::icon_button(ui, icons::GEAR_SIX, 16.0, "Snippet settings");
                egui::Popup::menu(&gear)
                    .width(280.0)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| snippet_settings(ui, &mut self.ws.snippet));
            });
        });
        if copied {
            self.notify("Copied to clipboard");
        }
        ui.add_space(6.0);
        let lines: Vec<String> = code.lines().map(str::to_string).collect();
        let mut cache = pm::WrapCache::default();
        egui::Frame::new().fill(p.bg).corner_radius(6).inner_margin(pm::margin(4, 6)).show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            pm::code_view(
                ui,
                Id::unique("code_snippet_view"),
                &lines,
                pm::Highlight::Code(opts.lang.syntax()),
                &mut cache,
                pm::CodeViewState { wrap: true, hang: 4, search: "", current_match: 0, scroll_to_match: false },
            );
        });
    }

    fn info_panel(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let Some(tab) = self.tabs.get(self.active) else { return };
        let vars = self.vars_for(tab.collection);
        ui.label(RichText::new("Request details").font(semibold(15.0)).color(p.text));
        ui.add_space(10.0);
        let coll = tab.collection.and_then(|c| self.ws.collection(c)).map(|c| c.name.clone()).unwrap_or_else(|| "Not saved".into());
        let resolved = crate::vars::substitute(&tab.request.url, &vars);
        let rows: Vec<(&str, String)> = vec![
            ("Name", tab.request.name.clone()),
            ("Method", tab.request.method.as_str().to_string()),
            ("URL", resolved),
            ("Collection", coll),
            ("ID", tab.request.id.to_string()),
        ];
        for (k, v) in rows {
            ui.label(RichText::new(k).color(p.text2).font(regular(12.0)));
            ui.add(egui::Label::new(RichText::new(v).color(p.text)).wrap().selectable(true));
            ui.add_space(8.0);
        }
        if let TabState::Done(view) = &tab.state {
            let d = &view.data;
            ui.label(RichText::new("Last response").color(p.text2).font(regular(12.0)));
            ui.label(format!("{} {} · {} · {}", d.status, d.status_text, http::human_duration(d.duration), http::human_size(d.body.len())));
        }
    }
}

impl RustmanApp {
    /// Icon and title of the extra tab; `None` when nothing is open (or it was deleted).
    fn side_tab_label(&self) -> Option<(&'static str, String)> {
        match self.side_tab? {
            SideTab::Env(EnvTarget::Globals) => Some((icons::GLOBE_SIMPLE, "Globals".to_string())),
            SideTab::Env(EnvTarget::Env(id)) => self.ws.environments.iter().find(|e| e.id == id).map(|e| (icons::STACK, e.name.clone())),
            SideTab::Entity(kind, id) => self.ws.entity_name(kind, id).map(|n| (entity_icon(kind), n)),
            SideTab::Runner(id) => self.ws.collection(id).map(|c| (icons::PLAY, format!("Runner · {}", c.name))),
        }
    }
}

pub(super) fn entity_icon(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Spec => icons::FILE_CODE,
        EntityKind::Mock => icons::HARD_DRIVES,
        EntityKind::Dataset => icons::TABLE,
        EntityKind::Document => icons::FILE_TEXT,
        EntityKind::Toolset => icons::TOOLBOX,
    }
}

/// "No Environment ▾" selector in the tab bar.
fn env_selector(ui: &mut Ui, label: &str, width: f32) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let color = if resp.hovered() { p.text } else { p.text2 };
    paint_text(ui, pos2(rect.left() + 10.0, rect.center().y), label, regular(13.0), color, width - 34.0);
    ui.painter().text(pos2(rect.right() - 14.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
    resp
}

/// Compact text/icon button for the status bar.
fn status_button(ui: &mut Ui, glyph: Option<&str>, label: &str) -> egui::Response {
    let p = pal(ui);
    let font = regular(12.5);
    let icon_w = if glyph.is_some() { 18.0 } else { 0.0 };
    let text_w = if label.is_empty() { 0.0 } else { pm::text_width(ui, label, font.clone()) + 4.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(10.0 + icon_w + text_w, 22.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let color = if resp.hovered() { p.text } else { p.text2 };
    let mut x = rect.left() + 5.0;
    if let Some(g) = glyph {
        ui.painter().text(pos2(x + 8.0, rect.center().y), Align2::CENTER_CENTER, g, icon_font(14.0), color);
        x += icon_w;
    }
    if !label.is_empty() {
        paint_text(ui, pos2(x, rect.center().y), label, font, color, 200.0);
    }
    resp
}

/// Language list entry with a check mark for the selected one (like Postman's menu).
fn lang_item(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    if selected {
        ui.painter().text(pos2(rect.left() + 14.0, rect.center().y), Align2::CENTER_CENTER, icons::CHECK, icon_font(14.0), p.text);
    }
    paint_text(ui, pos2(rect.left() + 30.0, rect.center().y), label, regular(14.0), p.text, rect.width() - 36.0);
    resp
}

/// Generator options (gear menu in the code snippet panel).
fn snippet_settings(ui: &mut Ui, o: &mut crate::codegen::Options) {
    let p = pal(ui);
    let label = |ui: &mut Ui, t: &str| {
        ui.label(RichText::new(t).font(regular(13.0)).color(p.text2));
    };
    ui.label(RichText::new("Snippet settings").font(semibold(14.0)).color(p.text));
    ui.add_space(6.0);
    label(ui, "Indentation");
    ui.horizontal(|ui| {
        for (text, tabs, n) in [("2 spaces", false, 2), ("4 spaces", false, 4), ("Tab", true, 1)] {
            let selected = o.tabs == tabs && (tabs || o.indent == n);
            if pm::radio(ui, selected, text).clicked() {
                o.tabs = tabs;
                if !tabs {
                    o.indent = n;
                }
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        pm::toggle(ui, &mut o.follow_redirects);
        ui.label("Follow redirects");
    });
    ui.add_space(8.0);
    ui.separator();
    label(ui, "cURL");
    ui.horizontal(|ui| {
        pm::toggle(ui, &mut o.curl_long);
        ui.label("Use long-form options (--header)");
    });
    ui.add_space(4.0);
    label(ui, "Line continuation character");
    ui.horizontal(|ui| {
        for (text, c) in [("\\ (bash)", '\\'), ("^ (cmd)", '^'), ("` (PowerShell)", '`')] {
            if pm::radio(ui, o.curl_continuation == c, text).clicked() {
                o.curl_continuation = c;
            }
        }
    });
}

/// Right-click menu of a request tab.
#[derive(Clone, Copy)]
enum TabMenu {
    New,
    Duplicate(usize),
    Close(usize),
    ForceClose(usize),
    CloseOthers(usize),
    CloseAll,
    ForceCloseAll,
}

/// The items of Postman's tab menu, with their shortcuts.
fn tab_menu(ui: &mut Ui, i: usize) -> Option<TabMenu> {
    use egui::{Key, KeyboardShortcut, Modifiers};
    let ctx = ui.ctx().clone();
    let keys = |m: Modifiers, k: Key| Some(ctx.format_shortcut(&KeyboardShortcut::new(m, k)));
    let items = [
        ("New Request", keys(Modifiers::COMMAND, Key::T), TabMenu::New),
        ("Duplicate Tab", None, TabMenu::Duplicate(i)),
        ("Close Tab", keys(Modifiers::COMMAND, Key::W), TabMenu::Close(i)),
        ("Force Close Tab", keys(Modifiers::COMMAND | Modifiers::ALT, Key::W), TabMenu::ForceClose(i)),
        ("Close Other Tabs", None, TabMenu::CloseOthers(i)),
        ("Close All Tabs", None, TabMenu::CloseAll),
        ("Force Close All Tabs", None, TabMenu::ForceCloseAll),
    ];
    let mut out = None;
    for (label, shortcut, action) in items {
        if matches!(action, TabMenu::Close(_)) {
            ui.separator();
        }
        let mut b = egui::Button::new(label).min_size(vec2(220.0, 0.0));
        if let Some(s) = shortcut {
            b = b.shortcut_text(s);
        }
        if ui.add(b).clicked() {
            out = Some(action);
        }
    }
    out
}
