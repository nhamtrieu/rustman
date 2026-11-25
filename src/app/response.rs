//! Response pane.

use eframe::egui::{self, Align2, Id, RichText, Sense, Ui, pos2, vec2};

use super::*;
use crate::http::{human_duration, human_size};
use crate::icons;
use crate::pm::{self, paint_text};
use crate::theme::{icon_font, mono, pal, regular, semibold, UI_SIZE};

#[derive(Default)]
struct Flags {
    cancel: bool,
    history: bool,
    clear: bool,
    toast: Option<String>,
}

impl RustmanApp {
    pub(super) fn response_panel(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let i = self.active;
        let mut flags = Flags::default();
        ui.add_space(6.0);
        let Tab { state, resp_section, body_view, wrap, search_open, search, match_idx, scroll_to_match, test_filter, .. } = &mut self.tabs[i];

        match state {
            TabState::Idle | TabState::Loading { .. } | TabState::Error(_) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Response").font(semibold(UI_SIZE)).color(p.text));
                    ui.add_space(6.0);
                    if pm::text_button(ui, Some(icons::CLOCK_COUNTER_CLOCKWISE), "History", false).clicked() {
                        flags.history = true;
                    }
                });
                let avail = ui.available_rect_before_wrap();
                let mut center = ui.new_child(egui::UiBuilder::new().max_rect(avail).layout(egui::Layout::top_down(egui::Align::Center)));
                center.add_space((avail.height() * 0.28).max(10.0));
                match state {
                    TabState::Idle => {
                        pm::icon(&mut center, icons::PAPER_PLANE_TILT, 44.0, p.text3);
                        center.add_space(10.0);
                        center.label(RichText::new("Enter the URL and click Send to get a response").color(p.text2));
                    }
                    TabState::Loading { started, .. } => {
                        center.add(egui::Spinner::new().size(30.0).color(p.text2));
                        center.add_space(10.0);
                        center.label(RichText::new(format!("Sending request…  {}", human_duration(started.elapsed()))).color(p.text2));
                        center.add_space(10.0);
                        if pm::secondary_button(&mut center, "Cancel").clicked() {
                            flags.cancel = true;
                        }
                        ui.ctx().request_repaint_after(Duration::from_millis(100));
                    }
                    TabState::Error(e) => {
                        pm::icon(&mut center, icons::WARNING, 44.0, p.text3);
                        center.add_space(10.0);
                        center.label(RichText::new("Could not send request").font(semibold(16.0)).color(p.text));
                        center.add_space(6.0);
                        center.scope(|ui| {
                            ui.set_max_width(720.0);
                            ui.add(egui::Label::new(RichText::new(e.as_str()).font(mono(13.0)).color(p.text2)).wrap().selectable(true));
                        });
                    }
                    TabState::Done(_) => {}
                }
            }
            TabState::Done(view) => {
                let d = &view.data;
                // Row 1: tabs + status.
                let report = &view.report;
                let total_tests = report.tests.iter().filter(|t| !t.skipped).count();
                let tests_label = if total_tests > 0 { format!("Test Results ({}/{})", report.passed_tests(), total_tests) } else { "Test Results".to_string() };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (sec, label, count, dot) in [
                        (RespSection::Body, "Body", None, false),
                        (RespSection::Cookies, "Cookies", Some(d.cookies.len()), false),
                        (RespSection::Headers, "Headers", Some(d.headers.len()), false),
                        (RespSection::Tests, tests_label.as_str(), None, false),
                    ] {
                        if pm::pill_tab(ui, None, label, count, dot, *resp_section == sec).clicked() {
                            *resp_section = sec;
                        }
                    }
                    pm::vsep(ui, 20.0);
                    if pm::icon_button(ui, icons::CLOCK_COUNTER_CLOCKWISE, 16.0, "History").clicked() {
                        flags.history = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let more = pm::icon_button(ui, icons::DOTS_THREE, 16.0, "More");
                        egui::Popup::menu(&more).show(|ui| {
                            if ui.button("Copy response").clicked() {
                                ui.ctx().copy_text(d.text());
                                flags.toast = Some("Copied to clipboard".into());
                                ui.close();
                            }
                            if ui.button("Clear response").clicked() {
                                flags.clear = true;
                                ui.close();
                            }
                        });
                        if pm::text_button(ui, Some(icons::FLOPPY_DISK), "Save Response", false).clicked()
                            && let Some(path) = rfd::FileDialog::new().set_file_name(suggest_file_name(d)).save_file() {
                                flags.toast = Some(match std::fs::write(&path, &d.body) {
                                    Ok(()) => format!("Saved to {}", path.display()),
                                    Err(e) => format!("Save failed: {e}"),
                                });
                            }
                        let net = pm::icon_button(ui, icons::GLOBE_SIMPLE, 16.0, "");
                        net.on_hover_ui(|ui| {
                            ui.label(RichText::new("Network").font(semibold(13.0)));
                            ui.label(format!("HTTP version: {}", d.http_version));
                            ui.label(format!("Remote address: {}", d.remote_addr.as_deref().unwrap_or("-")));
                            ui.label(format!("URL: {}", d.url));
                        });
                        let headers_size: usize = d.headers.iter().map(|(k, v)| k.len() + v.len() + 4).sum();
                        let meta = |ui: &mut Ui, text: String| ui.label(RichText::new(text).font(regular(13.0)).color(p.text2));
                        meta(ui, human_size(d.body.len())).on_hover_text(format!(
                            "Response size: {}\n  Headers: {}\n  Body: {}",
                            human_size(d.body.len() + headers_size),
                            human_size(headers_size),
                            human_size(d.body.len())
                        ));
                        meta(ui, "•".into());
                        meta(ui, human_duration(d.duration)).on_hover_text(format!("Response time: {}", human_duration(d.duration)));
                        meta(ui, "•".into());
                        status_pill(ui, d.status, &d.status_text);
                    });
                });
                ui.add_space(6.0);
                match resp_section {
                    RespSection::Body => {
                        if view.is_binary {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(format!("Binary response ({})", human_size(d.body.len()))).color(p.text2));
                                ui.label(RichText::new("Use Save Response to write it to a file.").color(p.text3));
                            });
                        } else {
                            // Row 2: format + tools.
                            let mut search_changed = false;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let label = if *body_view == BodyView::Raw { "Raw" } else { view.kind };
                                let glyph = if view.is_json && *body_view == BodyView::Pretty { icons::BRACKETS_CURLY } else { icons::TEXT_ALIGN_LEFT };
                                let fmt = format_pill(ui, glyph, label);
                                egui::Popup::menu(&fmt).show(|ui| {
                                    if ui.selectable_label(*body_view == BodyView::Pretty, view.kind).clicked() {
                                        *body_view = BodyView::Pretty;
                                        ui.close();
                                    }
                                    if ui.selectable_label(*body_view == BodyView::Raw, "Raw").clicked() {
                                        *body_view = BodyView::Raw;
                                        ui.close();
                                    }
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 2.0;
                                    if pm::icon_button(ui, icons::COPY, 16.0, "Copy").clicked() {
                                        let text = if *body_view == BodyView::Pretty { view.pretty_lines.join("\n") } else { d.text() };
                                        ui.ctx().copy_text(text);
                                        flags.toast = Some("Copied to clipboard".into());
                                    }
                                    if pm::icon_button_sel(ui, icons::MAGNIFYING_GLASS, 16.0, "Search", *search_open).clicked() {
                                        *search_open = !*search_open;
                                    }
                                    if pm::icon_button_sel(ui, icons::ARROW_ELBOW_DOWN_LEFT, 16.0, "Wrap lines", *wrap).clicked() {
                                        *wrap = !*wrap;
                                    }
                                });
                            });
                            let lines = if *body_view == BodyView::Pretty { &view.pretty_lines } else { &view.raw_lines };
                            let total = if search.is_empty() { 0 } else { lines.iter().map(|l| pm::find_all(l, search).len()).sum() };
                            if *search_open {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let id = Id::unique("resp_search");
                                    let r = pm::search_box(ui, id, search, "Find in response", 280.0, 30.0);
                                    if r.changed() {
                                        *match_idx = 0;
                                        search_changed = true;
                                    }
                                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    let shift = ui.input(|i| i.modifiers.shift);
                                    let status = if search.is_empty() { String::new() } else if total == 0 { "No results".into() } else { format!("{} of {}", *match_idx + 1, total) };
                                    ui.label(RichText::new(status).font(regular(13.0)).color(p.text2));
                                    let prev = pm::icon_button(ui, icons::ARROW_LEFT, 14.0, "Previous (Shift+Enter)").clicked() || (enter && shift);
                                    let next = pm::icon_button(ui, icons::ARROW_RIGHT, 14.0, "Next (Enter)").clicked() || (enter && !shift);
                                    if total > 0 && (prev || next) {
                                        *match_idx = if next { (*match_idx + 1) % total } else { (*match_idx + total - 1) % total };
                                        search_changed = true;
                                    }
                                    if enter {
                                        r.request_focus();
                                    }
                                    if pm::icon_button(ui, icons::X, 14.0, "Close").clicked() {
                                        *search_open = false;
                                        search.clear();
                                    }
                                });
                            }
                            ui.add_space(4.0);
                            let hl = if view.is_json && *body_view == BodyView::Pretty { pm::Highlight::Json } else { pm::Highlight::Plain };
                            let query = if *search_open { search.as_str() } else { "" };
                            pm::code_view(
                                ui,
                                Id::unique(("resp_view", i)),
                                lines,
                                hl,
                                &mut view.wrap_cache,
                                pm::CodeViewState { wrap: *wrap, hang: 0, search: query, current_match: *match_idx, scroll_to_match: search_changed || *scroll_to_match },
                            );
                            *scroll_to_match = false;
                        }
                    }
                    RespSection::Headers => {
                        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                            let rows: Vec<Vec<String>> = d.headers.iter().map(|(k, v)| vec![k.clone(), v.clone()]).collect();
                            pm::ro_table(ui, &["Key", "Value"], &rows, &[0.35, 0.65], false);
                        });
                    }
                    RespSection::Tests => test_results(ui, report, test_filter),
                    RespSection::Cookies => {
                        if d.cookies.is_empty() {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| ui.label(RichText::new("No cookies received from the server").color(p.text2)));
                        } else {
                            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                                let rows: Vec<Vec<String>> = d
                                    .cookies
                                    .iter()
                                    .map(|c| {
                                        vec![
                                            c.name.clone(),
                                            c.value.clone(),
                                            c.domain.clone(),
                                            c.path.clone(),
                                            c.expires.clone(),
                                            c.http_only.to_string(),
                                            c.secure.to_string(),
                                        ]
                                    })
                                    .collect();
                                pm::ro_table(ui, &["Name", "Value", "Domain", "Path", "Expires", "HttpOnly", "Secure"], &rows, &[1.2, 2.0, 1.2, 0.6, 1.6, 0.7, 0.6], false);
                            });
                        }
                    }
                }
            }
        }

        if flags.cancel {
            self.cancel(i);
        }
        if flags.clear {
            self.tabs[i].state = TabState::Idle;
        }
        if flags.history {
            self.sidebar_visible = true;
            self.sidebar_mode = SidebarMode::History;
        }
        if let Some(t) = flags.toast {
            self.notify(t);
        }
    }
}

/// Test results of the last run of the request's scripts, plus script errors.
fn test_results(ui: &mut Ui, report: &crate::exec::Report, filter: &mut u8) {
    let p = pal(ui);
    egui::ScrollArea::vertical().id_salt("test_results").auto_shrink(false).show(ui, |ui| {
        for e in &report.errors {
            egui::Frame::new().fill(p.status(500).0).corner_radius(4).inner_margin(pm::margin(10, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    pm::icon(ui, icons::WARNING, 15.0, p.status(500).1);
                    ui.add(egui::Label::new(RichText::new(e).font(mono(12.5)).color(p.status(500).1)).wrap().selectable(true));
                });
            });
            ui.add_space(6.0);
        }
        if report.tests.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                pm::icon(ui, icons::CHECK_CIRCLE, 40.0, p.text3);
                ui.add_space(8.0);
                ui.label(RichText::new("There are no tests for this request").color(p.text2));
                ui.label(RichText::new("Write a test in Scripts → Post-response, for example:").font(regular(12.5)).color(p.text3));
                ui.label(RichText::new("pm.test(\"Status is 200\", () => pm.response.to.have.status(200));").font(mono(12.5)).color(p.text3));
            });
            return;
        }
        let passed = report.tests.iter().filter(|t| t.passed && !t.skipped).count();
        let skipped = report.tests.iter().filter(|t| t.skipped).count();
        let failed = report.tests.len() - passed - skipped;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, label, n) in [(0u8, "All", report.tests.len()), (1, "Passed", passed), (2, "Skipped", skipped), (3, "Failed", failed)] {
                if pm::pill_tab(ui, None, label, Some(n), false, *filter == i).clicked() {
                    *filter = i;
                }
            }
        });
        ui.add_space(6.0);
        for t in &report.tests {
            let show = match *filter {
                1 => t.passed && !t.skipped,
                2 => t.skipped,
                3 => !t.passed && !t.skipped,
                _ => true,
            };
            if !show {
                continue;
            }
            ui.horizontal_top(|ui| {
                let (label, (bg, fg)) = if t.skipped {
                    ("SKIPPED", (p.pill, p.text2))
                } else if t.passed {
                    ("PASS", p.status(200))
                } else {
                    ("FAIL", p.status(500))
                };
                let (rect, _) = ui.allocate_exact_size(vec2(64.0, 22.0), Sense::hover());
                ui.painter().rect_filled(rect, 4.0, bg);
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, semibold(11.5), fg);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.add(egui::Label::new(RichText::new(&t.name).color(p.text)).wrap().selectable(true));
                    if let Some(err) = &t.error {
                        ui.add(egui::Label::new(RichText::new(err).font(mono(12.5)).color(p.text2)).wrap().selectable(true));
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
}

fn status_pill(ui: &mut Ui, status: u16, text: &str) {
    let p = pal(ui);
    let (bg, fg) = p.status(status);
    let label = format!("{status} {text}");
    let font = semibold(13.0);
    let w = pm::text_width(ui, &label, font.clone()) + 14.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, &label, font, fg);
    let meaning = match status {
        200..=299 => "Request successful. The server has responded as required.",
        300..=399 => "Redirection. Further action is needed to complete the request.",
        400..=499 => "Client error. The request contains bad syntax or cannot be fulfilled.",
        _ => "Server error. The server failed to fulfil a valid request.",
    };
    resp.on_hover_text(format!("{label}\n{meaning}"));
}

/// "{} JSON ▾" pill.
fn format_pill(ui: &mut Ui, glyph: &str, label: &str) -> egui::Response {
    let p = pal(ui);
    let font = regular(UI_SIZE);
    let w = 26.0 + pm::text_width(ui, label, font.clone()) + 26.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    ui.painter().rect_filled(rect, 6.0, if resp.hovered() { p.active } else { p.pill });
    ui.painter().text(pos2(rect.left() + 14.0, rect.center().y), Align2::CENTER_CENTER, glyph, icon_font(15.0), p.text);
    paint_text(ui, pos2(rect.left() + 26.0, rect.center().y), label, font, p.text, 120.0);
    ui.painter().text(pos2(rect.right() - 13.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
    resp
}
