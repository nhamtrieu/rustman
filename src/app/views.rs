//! Tabs for workspace entities: API specs, datasets and documents (mock servers live in `mocks.rs`).

use eframe::egui::{self, Align, Align2, Id, Layout, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use super::*;
use crate::icons;
use crate::pm::{self, paint_text};
use crate::theme::{icon_font, mono, pal, regular, semibold};

impl RustmanApp {
    /// Title row shared by entity tabs: icon, click-to-rename name, `⋯` menu, then `right` controls (right-to-left).
    pub(super) fn entity_header(&mut self, ui: &mut Ui, kind: EntityKind, id: Uuid, right: impl FnOnce(&mut Self, &mut Ui)) {
        let p = pal(ui);
        let Some(name) = self.ws.entity_name(kind, id) else { return };
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        let cy = rect.center().y;
        ui.painter().text(pos2(rect.left() + 10.0, cy), Align2::CENTER_CENTER, super::chrome::entity_icon(kind), icon_font(16.0), p.text2);

        let x = rect.left() + 32.0;
        let max_name_w = (rect.width() * 0.45).max(120.0);
        let name_w = pm::text_width(ui, &name, semibold(15.0)).min(max_name_w);
        let name_rect = Rect::from_min_max(pos2(x - 6.0, cy - 14.0), pos2(x + name_w + 6.0, cy + 14.0));
        let target = RenameTarget::EntityTitle(kind, id);
        if matches!(self.rename, Some((t, _)) if t == target) {
            let edit_rect = Rect::from_min_max(name_rect.min, pos2(name_rect.left() + max_name_w.min(360.0), name_rect.bottom()));
            ui.painter().rect(edit_rect, 4.0, p.bg, Stroke::new(1.0, p.focus), StrokeKind::Inside);
            let (mut commit, mut cancel) = (false, false);
            if let Some((_, text)) = &mut self.rename {
                let resp = ui.put(
                    edit_rect.shrink2(vec2(6.0, 0.0)),
                    egui::TextEdit::singleline(text).id(Id::unique("entity_title_rename")).frame(egui::Frame::NONE).font(semibold(15.0)).align(Align2::LEFT_CENTER),
                );
                if self.rename_focus {
                    resp.request_focus();
                    self.rename_focus = false;
                }
                cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                commit = !cancel && resp.lost_focus();
            }
            if commit {
                self.commit_rename();
            } else if cancel {
                self.rename = None;
            }
        } else {
            let resp = ui.interact(name_rect, Id::unique(("entity_title", id)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_stroke(name_rect, 4.0, Stroke::new(1.0, p.input_border), StrokeKind::Inside);
            }
            paint_text(ui, pos2(x, cy), &name, semibold(15.0), p.text, name_w);
            if resp.on_hover_text("Rename").clicked() {
                self.start_rename(target, name.clone());
            }
        }

        let controls = Rect::from_min_max(pos2(name_rect.right() + 16.0, rect.top()), rect.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(controls).layout(Layout::right_to_left(Align::Center)));
        child.spacing_mut().item_spacing.x = 6.0;
        let dots = pm::icon_button(&mut child, icons::DOTS_THREE, 16.0, "More actions");
        let mut action = None;
        egui::Popup::menu(&dots).show(|ui| {
            for (label, act) in [("Rename", 0), ("Duplicate", 1), ("Delete", 2)] {
                if ui.button(label).clicked() {
                    action = Some(act);
                    ui.close();
                }
            }
        });
        right(self, &mut child);
        match action {
            Some(0) => self.start_rename(target, name),
            Some(1) => {
                if let Some(new) = self.ws.duplicate_entity(kind, id) {
                    self.open_side(SideTab::Entity(kind, new));
                }
            }
            Some(2) => self.confirm = Some(Confirm::DeleteEntity(kind, id)),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // API spec: YAML/JSON editor + outline + "Generate collection"
    // -----------------------------------------------------------------------

    pub(super) fn spec_view(&mut self, ui: &mut Ui, id: Uuid) {
        let mut generate = false;
        self.entity_header(ui, EntityKind::Spec, id, |_, ui| {
            if pm::primary_button(ui, "Generate Collection").on_hover_text("Create a collection with one request per operation").clicked() {
                generate = true;
            }
        });
        let Some(spec) = self.ws.specs.iter_mut().find(|s| s.id == id) else { return };
        let p = pal(ui);

        // Re-parse only when the text changes.
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            spec.content.hash(&mut h);
            h.finish()
        };
        let cache_id = Id::unique(("spec_outline", id));
        let cached = ui.data(|d| d.get_temp::<(u64, std::sync::Arc<Result<crate::openapi::Outline, String>>)>(cache_id));
        let outline = match cached {
            Some((h, o)) if h == hash => o,
            _ => {
                let o = std::sync::Arc::new(crate::openapi::outline(&spec.content));
                ui.data_mut(|d| d.insert_temp(cache_id, (hash, o.clone())));
                o
            }
        };

        let outline_w = (ui.available_width() * 0.36).clamp(260.0, 480.0);
        egui::Panel::right(Id::unique(("spec_outline_panel", id)))
            .exact_size(outline_w)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 0, top: 0, bottom: 0 }))
            .show_separator_line(true)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("spec_outline_scroll").auto_shrink(false).show(ui, |ui| match outline.as_ref() {
                    Err(e) => {
                        ui.horizontal(|ui| {
                            pm::icon(ui, icons::WARNING, 15.0, p.danger);
                            ui.label(RichText::new("Invalid spec").font(semibold(14.0)).color(p.danger));
                        });
                        ui.add(egui::Label::new(RichText::new(e).font(mono(12.5)).color(p.text2)).wrap());
                    }
                    Ok(o) => {
                        ui.label(RichText::new(format!("{} {}", o.title, o.version)).font(semibold(15.0)).color(p.text));
                        ui.label(RichText::new(format!("OpenAPI {}", o.spec_version)).font(regular(12.5)).color(p.text3));
                        if !o.base_url.is_empty() {
                            ui.add_space(4.0);
                            ui.label(RichText::new(&o.base_url).font(mono(12.5)).color(p.text2));
                        }
                        for w in &o.warnings {
                            ui.label(RichText::new(format!("⚠ {w}")).font(regular(12.5)).color(p.orange));
                        }
                        ui.add_space(10.0);
                        ui.label(RichText::new(format!("{} operations", o.endpoints.len())).font(semibold(12.5)).color(p.text2));
                        ui.add_space(4.0);
                        let mut last_tag: Option<&str> = None;
                        let mut sorted: Vec<&crate::openapi::Endpoint> = o.endpoints.iter().collect();
                        sorted.sort_by_key(|e| e.tag.as_deref().unwrap_or("\u{10ffff}"));
                        for e in sorted {
                            if e.tag.as_deref() != last_tag {
                                last_tag = e.tag.as_deref();
                                ui.add_space(6.0);
                                ui.label(RichText::new(last_tag.unwrap_or("(no tag)")).font(semibold(13.0)).color(p.text));
                            }
                            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                            let (m, f) = pm::method_label_font(e.method, 10.5);
                            paint_text(ui, pos2(r.left() + 4.0, r.center().y), &m, f, p.method(e.method), 50.0);
                            let w = paint_text(ui, pos2(r.left() + 56.0, r.center().y), &e.path, mono(12.5), p.text, r.width() - 60.0);
                            if !e.summary.is_empty() {
                                paint_text(ui, pos2(r.left() + 66.0 + w, r.center().y), &e.summary, regular(12.5), p.text3, (r.width() - w - 70.0).max(0.0));
                            }
                        }
                    }
                });
            });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let is_json = spec.content.trim_start().starts_with('{');
            super::request::code_editor(ui, Id::unique(("spec_editor", id)), &mut spec.content, is_json, &VarMap::new());
        });

        if generate {
            match crate::openapi::to_collection(&spec.content) {
                Ok(c) => {
                    let (cid, name, n) = (c.id, c.name.clone(), count_requests(&c.items));
                    self.ws.collections.push(c);
                    self.ws.expanded.insert(cid);
                    self.notify(format!("Created collection \"{name}\" with {n} requests"));
                }
                Err(e) => self.notify_err(e),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Document: markdown with Edit / Preview
    // -----------------------------------------------------------------------

    pub(super) fn document_view(&mut self, ui: &mut Ui, id: Uuid) {
        let mode_id = Id::unique(("doc_mode", id));
        let empty = self.ws.documents.iter().find(|d| d.id == id).is_none_or(|d| d.content.trim().is_empty());
        let mut editing = ui.data(|d| d.get_temp::<bool>(mode_id)).unwrap_or(empty);
        self.entity_header(ui, EntityKind::Document, id, |_, ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (label, edit) in [("Preview", false), ("Edit", true)] {
                if pm::pill_tab(ui, None, label, None, false, editing == edit).clicked() {
                    editing = edit;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(mode_id, editing));
        let Some(doc) = self.ws.documents.iter_mut().find(|d| d.id == id) else { return };
        let p = pal(ui);
        if editing {
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt(("doc_edit", id)).auto_shrink(false).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut doc.content)
                        .id(Id::unique(("doc_editor", id)))
                        .font(mono(13.5))
                        .hint_text("Write in Markdown: # Heading, - list, **bold**, `code`, ``` code blocks ```")
                        .desired_width(f32::INFINITY)
                        .min_size(vec2(0.0, ui.available_height() - 8.0)),
                );
            });
        } else {
            egui::ScrollArea::vertical().id_salt(("doc_preview", id)).auto_shrink(false).show(ui, |ui| {
                ui.set_max_width(900.0f32.min(ui.available_width()));
                if doc.content.trim().is_empty() {
                    ui.label(RichText::new("This document is empty. Switch to Edit to write something.").color(p.text3));
                }
                markdown(ui, &doc.content);
            });
        }
    }

    // -----------------------------------------------------------------------
    // Dataset: editable grid, CSV/JSON import & export
    // -----------------------------------------------------------------------

    pub(super) fn dataset_view(&mut self, ui: &mut Ui, id: Uuid) {
        let (mut import, mut export) = (false, false);
        self.entity_header(ui, EntityKind::Dataset, id, |_, ui| {
            if pm::text_button(ui, Some(icons::EXPORT), "Export CSV", false).clicked() {
                export = true;
            }
            if pm::text_button(ui, Some(icons::ARROW_SQUARE_IN), "Import CSV / JSON", false).clicked() {
                import = true;
            }
        });
        let p = pal(ui);
        let Some(ds) = self.ws.datasets.iter_mut().find(|d| d.id == id) else { return };
        ui.label(
            RichText::new("Each row is one iteration of a collection run. Use the column names as variables, e.g. {{email}}.")
                .font(regular(13.0))
                .color(p.text2),
        );
        ui.add_space(8.0);
        let mut add_row = false;
        let mut add_col = false;
        let mut del_row = None;
        let mut del_col = None;
        let col_w = 190.0;
        egui::ScrollArea::both().id_salt(("ds_grid", id)).auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let row_h = pm::ROW_H;
            let ncols = ds.columns.len();
            let width = 44.0 + col_w * ncols as f32 + 44.0;
            let (rect, _) = ui.allocate_exact_size(vec2(width, row_h * (ds.rows.len() as f32 + 1.0)), Sense::hover());
            let stroke = Stroke::new(1.0, p.border);
            ui.painter().rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
            for r in 1..=ds.rows.len() {
                ui.painter().hline(rect.x_range(), rect.top() + row_h * r as f32, stroke);
            }
            for c in 0..=ncols {
                ui.painter().vline(rect.left() + 44.0 + col_w * c as f32, rect.y_range(), stroke);
            }
            ui.painter().rect_filled(Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(width - 2.0, row_h - 1.0)), 0.0, p.chrome);
            let cell = |r: usize, c: usize| Rect::from_min_size(pos2(rect.left() + 44.0 + col_w * c as f32 + 10.0, rect.top() + row_h * r as f32 + 1.0), vec2(col_w - 34.0, row_h - 2.0));
            // Header: column names + delete column.
            for (c, name) in ds.columns.iter_mut().enumerate() {
                let r = cell(0, c);
                ui.put(r, egui::TextEdit::singleline(name).id(Id::unique(("ds_col", id, c))).frame(egui::Frame::NONE).font(semibold(13.5)).hint_text("column").align(Align2::LEFT_CENTER));
                let del = Rect::from_center_size(pos2(r.right() + 12.0, r.center().y), vec2(20.0, 20.0));
                let resp = ui.interact(del, Id::unique(("ds_col_del", id, c)), Sense::click());
                if resp.hovered() || ui.rect_contains_pointer(r.expand2(vec2(10.0, 0.0))) {
                    ui.painter().text(del.center(), Align2::CENTER_CENTER, icons::X, icon_font(12.0), if resp.hovered() { p.danger } else { p.text3 });
                }
                if resp.on_hover_text("Delete column").clicked() {
                    del_col = Some(c);
                }
            }
            let plus_col = Rect::from_center_size(pos2(rect.right() - 22.0, rect.top() + row_h / 2.0), vec2(24.0, 24.0));
            let pr = ui.interact(plus_col, Id::unique(("ds_add_col", id)), Sense::click());
            ui.painter().text(plus_col.center(), Align2::CENTER_CENTER, icons::PLUS, icon_font(14.0), if pr.hovered() { p.text } else { p.text2 });
            if pr.on_hover_text("Add column").clicked() {
                add_col = true;
            }
            // Rows.
            for (ri, row) in ds.rows.iter_mut().enumerate() {
                row.resize(ncols, String::new());
                let y = rect.top() + row_h * (ri as f32 + 1.5);
                paint_text(ui, pos2(rect.left() + 12.0, y), &(ri + 1).to_string(), regular(12.0), p.text3, 30.0);
                for (c, value) in row.iter_mut().enumerate() {
                    ui.put(cell(ri + 1, c), egui::TextEdit::singleline(value).id(Id::unique(("ds_cell", id, ri, c))).frame(egui::Frame::NONE).font(regular(13.5)).align(Align2::LEFT_CENTER));
                }
                let del = Rect::from_center_size(pos2(rect.right() - 22.0, y), vec2(22.0, 22.0));
                let hovered_row = ui.rect_contains_pointer(Rect::from_min_max(pos2(rect.left(), y - row_h / 2.0), pos2(rect.right(), y + row_h / 2.0)));
                let resp = ui.interact(del, Id::unique(("ds_row_del", id, ri)), Sense::click());
                if hovered_row {
                    ui.painter().text(del.center(), Align2::CENTER_CENTER, icons::TRASH, icon_font(14.0), if resp.hovered() { p.text } else { p.text2 });
                }
                if resp.on_hover_text("Delete row").clicked() {
                    del_row = Some(ri);
                }
            }
            ui.add_space(10.0);
            if pm::text_button(ui, Some(icons::PLUS), "Add row", false).clicked() {
                add_row = true;
            }
        });
        if add_row {
            ds.rows.push(vec![String::new(); ds.columns.len()]);
        }
        if add_col {
            ds.columns.push(format!("column{}", ds.columns.len() + 1));
            ds.rows.iter_mut().for_each(|r| r.push(String::new()));
        }
        if let Some(r) = del_row {
            ds.rows.remove(r);
        }
        if let Some(c) = del_col {
            ds.columns.remove(c);
            ds.rows.iter_mut().for_each(|r| {
                if c < r.len() {
                    r.remove(c);
                }
            });
        }
        if import && let Some(path) = rfd::FileDialog::new().add_filter("Data", &["csv", "json", "txt"]).pick_file() {
            match crate::dataset::import_file(&path) {
                Ok((cols, rows)) => {
                    let n = rows.len();
                    ds.columns = cols;
                    ds.rows = rows;
                    self.notify(format!("Imported {n} rows"));
                }
                Err(e) => self.notify_err(e),
            }
        }
        if export {
            let Some(ds) = self.ws.datasets.iter().find(|d| d.id == id) else { return };
            let csv = crate::dataset::to_csv(ds);
            let file = format!("{}.csv", ds.name.replace(['/', '\\'], "_"));
            if let Some(path) = rfd::FileDialog::new().set_file_name(&file).add_filter("CSV", &["csv"]).save_file() {
                match std::fs::write(&path, csv) {
                    Ok(()) => self.notify(format!("Exported to {}", path.display())),
                    Err(e) => self.notify_err(format!("Export failed: {e}")),
                }
            }
        }
    }
}

pub(super) fn count_requests(items: &[Item]) -> usize {
    items
        .iter()
        .map(|i| match i {
            Item::Request(_) => 1,
            Item::Folder(f) => count_requests(&f.items),
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Minimal markdown renderer: headings, lists, quotes, rules, fenced code, **bold**, *italic*, `code`.
// ---------------------------------------------------------------------------

fn markdown(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    let mut in_code = false;
    let mut code = String::new();
    let mut para: Vec<&str> = Vec::new();
    let flush = |ui: &mut Ui, para: &mut Vec<&str>| {
        if !para.is_empty() {
            inline(ui, &para.join(" "), regular(14.5), 0.0);
            ui.add_space(8.0);
            para.clear();
        }
    };
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim_start().starts_with("```") {
            if in_code {
                egui::Frame::new().fill(p.chrome).corner_radius(6).inner_margin(pm::margin(10, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(code.trim_end_matches('\n')).font(mono(13.0)).color(p.text)).selectable(true));
                });
                ui.add_space(8.0);
                code.clear();
            } else {
                flush(ui, &mut para);
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        let s = t.trim_start();
        if s.is_empty() {
            flush(ui, &mut para);
            continue;
        }
        let heading = s.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&heading) && s[heading..].starts_with(' ') {
            flush(ui, &mut para);
            let size = [24.0, 20.0, 17.0, 15.5, 14.5, 14.0][heading - 1];
            ui.add_space(if heading <= 2 { 10.0 } else { 6.0 });
            inline(ui, s[heading..].trim(), semibold(size), 0.0);
            if heading <= 2 {
                ui.add_space(4.0);
                pm::hline(ui);
            }
            ui.add_space(6.0);
        } else if s == "---" || s == "***" {
            flush(ui, &mut para);
            pm::hline(ui);
            ui.add_space(8.0);
        } else if let Some(rest) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")).or_else(|| s.strip_prefix("+ ")) {
            flush(ui, &mut para);
            let indent = (t.len() - s.len()) as f32 * 6.0;
            let rest = match rest.strip_prefix("[ ] ") {
                Some(r) => format!("☐ {r}"),
                None => rest.strip_prefix("[x] ").map(|r| format!("☑ {r}")).unwrap_or_else(|| format!("•  {rest}")),
            };
            inline(ui, &rest, regular(14.5), 12.0 + indent);
            ui.add_space(3.0);
        } else if let Some((num, rest)) = s.split_once(". ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            flush(ui, &mut para);
            inline(ui, &format!("{num}.  {rest}"), regular(14.5), 12.0);
            ui.add_space(3.0);
        } else if let Some(rest) = s.strip_prefix("> ").or_else(|| s.strip_prefix('>')) {
            flush(ui, &mut para);
            let r = ui.horizontal(|ui| {
                ui.add_space(14.0);
                inline(ui, rest, regular(14.5), 0.0);
            });
            let rr = r.response.rect;
            ui.painter().vline(rr.left() + 3.0, rr.y_range(), Stroke::new(3.0, p.border));
            ui.add_space(4.0);
        } else {
            para.push(s);
        }
    }
    if in_code && !code.is_empty() {
        ui.add(egui::Label::new(RichText::new(code).font(mono(13.0))));
    }
    flush(ui, &mut para);
}

/// One wrapped paragraph with `**bold**`, `*italic*` and `` `code` `` spans.
fn inline(ui: &mut Ui, text: &str, font: egui::FontId, indent: f32) {
    let p = pal(ui);
    let mut job = egui::text::LayoutJob::default();
    let bold_font = semibold(font.size);
    let (mut bold, mut italic) = (false, false);
    let mut buf = String::new();
    let push = |job: &mut egui::text::LayoutJob, buf: &mut String, bold: bool, italic: bool| {
        if !buf.is_empty() {
            let f = if bold { bold_font.clone() } else { font.clone() };
            job.append(buf, 0.0, egui::TextFormat { font_id: f, color: p.text, italics: italic, ..Default::default() });
            buf.clear();
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' if chars.peek() == Some(&c) => {
                chars.next();
                push(&mut job, &mut buf, bold, italic);
                bold = !bold;
            }
            '*' => {
                push(&mut job, &mut buf, bold, italic);
                italic = !italic;
            }
            '`' => {
                push(&mut job, &mut buf, bold, italic);
                let code: String = chars.by_ref().take_while(|c| *c != '`').collect();
                job.append(&code, 0.0, egui::TextFormat { font_id: mono(font.size - 1.0), color: p.json_string, background: p.chrome, ..Default::default() });
            }
            _ => buf.push(c),
        }
    }
    push(&mut job, &mut buf, bold, italic);
    job.wrap.max_width = ui.available_width() - indent;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.add(egui::Label::new(job).wrap().selectable(true));
    });
}
