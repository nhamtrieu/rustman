//! Examples (saved responses) of a request: the list on the left, the selected example on the
//! right. Examples are saved from the response pane or added by hand, and feed mock servers.

use eframe::egui::{self, Align2, Id, RichText, Sense, Ui, pos2, vec2};

use super::request::{Hl, code_editor_hl};
use crate::icons;
use crate::model::{Example, KeyValue, RawLang, status_text};
use crate::pm::{self, TableOpts, paint_text};
use crate::theme::{icon_font, medium, pal, regular, semibold, UI_SIZE};
use crate::vars::VarMap;

/// Draws the Examples section. `selected` keeps the open example across frames.
pub(super) fn examples_editor(ui: &mut Ui, base: Id, examples: &mut Vec<Example>, selected: &mut Option<uuid::Uuid>, template: &Example, vars: &VarMap) {
    let p = pal(ui);
    if selected.is_none_or(|id| !examples.iter().any(|e| e.id == id)) {
        *selected = examples.first().map(|e| e.id);
    }
    let mut remove = None;
    let mut duplicate = None;

    ui.horizontal_top(|ui| {
        // List.
        let list_w = 230.0f32.min(ui.available_width() * 0.35);
        ui.vertical(|ui| {
            ui.set_width(list_w);
            if pm::text_button(ui, Some(icons::PLUS), "Add example", false).clicked() {
                let e = Example { name: format!("Example {}", examples.len() + 1), ..template.clone() };
                *selected = Some(e.id);
                examples.push(e);
            }
            ui.add_space(4.0);
            if examples.is_empty() {
                ui.add(egui::Label::new(RichText::new("No examples yet. Send the request and click Save as Example in the response, or add one by hand.").font(regular(13.0)).color(p.text2)).wrap());
            }
            for e in examples.iter() {
                let (rect, resp) = ui.allocate_exact_size(vec2(list_w, 30.0), Sense::click());
                let sel = *selected == Some(e.id);
                if sel || resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, if sel { p.active } else { p.hover });
                }
                let (_, fg) = p.status(e.status);
                let code = e.status.to_string();
                ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, &code, semibold(12.0), fg);
                paint_text(ui, pos2(rect.left() + 44.0, rect.center().y), &e.name, regular(UI_SIZE), p.text, list_w - 52.0);
                if resp.clicked() {
                    *selected = Some(e.id);
                }
            }
        });
        let (div, _) = ui.allocate_exact_size(vec2(1.0, ui.available_height().max(200.0)), Sense::hover());
        ui.painter().vline(div.center().x, div.y_range(), egui::Stroke::new(1.0, p.border));
        ui.add_space(14.0);

        // Details.
        ui.vertical(|ui| {
            let Some(e) = examples.iter_mut().find(|e| Some(e.id) == *selected) else { return };
            let id = base.with(("example", e.id));
            let w = ui.available_width();
            ui.horizontal(|ui| {
                pm::input(ui, id.with("name"), &mut e.name, vars, "Example name", (w - 190.0).max(160.0));
                if pm::text_button(ui, Some(icons::COPY), "Duplicate", false).clicked() {
                    duplicate = Some(e.id);
                }
                if pm::text_button(ui, Some(icons::TRASH), "Delete", false).on_hover_text("Delete this example").clicked() {
                    remove = Some(e.id);
                }
            });
            ui.add_space(10.0);

            ui.label(RichText::new("Request").font(medium(UI_SIZE)).color(p.text));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(84.0, 32.0), Sense::click());
                ui.painter().rect(rect, 4.0, p.bg, egui::Stroke::new(1.0, p.input_border), egui::StrokeKind::Inside);
                let (label, font) = pm::method_label_font(e.method, 13.0);
                ui.painter().text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, font, p.method(e.method));
                ui.painter().text(pos2(rect.right() - 12.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(11.0), p.text2);
                egui::Popup::menu(&resp).show(|ui| {
                    for m in crate::model::Method::ALL {
                        if ui.selectable_label(e.method == m, m.as_str()).clicked() {
                            e.method = m;
                            ui.close();
                        }
                    }
                });
                pm::input(ui, id.with("url"), &mut e.url, vars, "URL the example answers, e.g. {{baseUrl}}/users/1", (w - 92.0).max(160.0));
            });
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Response").font(medium(UI_SIZE)).color(p.text));
                ui.add_space(12.0);
                ui.label(RichText::new("Status").font(regular(13.0)).color(p.text2));
                ui.add(egui::DragValue::new(&mut e.status).range(100..=599).speed(1.0));
                ui.label(RichText::new(status_text(e.status)).font(regular(13.0)).color(p.text2));
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Body").font(regular(13.0)).color(p.text2));
                let label = e.body_lang.label();
                let resp = pm::text_button(ui, None, label, true);
                egui::Popup::menu(&resp).show(|ui| {
                    for l in RawLang::ALL {
                        if ui.selectable_label(e.body_lang == l, l.label()).clicked() {
                            e.body_lang = l;
                            ui.close();
                        }
                    }
                });
                if e.body_lang == RawLang::Json && pm::text_button(ui, None, "Beautify", false).clicked() {
                    e.body = super::request::beautify(&e.body);
                }
            });
            ui.add_space(4.0);
            let hl = match e.body_lang {
                RawLang::Json => Hl::Json,
                RawLang::JavaScript => Hl::Js,
                _ => Hl::Plain,
            };
            // Leave room for the headers table below.
            ui.allocate_ui(vec2(ui.available_width(), (ui.available_height() - 140.0).max(180.0)), |ui| {
                code_editor_hl(ui, id.with("body"), &mut e.body, hl, vars);
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Headers").font(regular(13.0)).color(p.text2));
            ui.add_space(4.0);
            pm::kv_table(ui, id.with("headers"), &mut e.headers, vars, TableOpts { with_desc: false, bulk: false, ..Default::default() });
        });
    });

    if let Some(id) = duplicate
        && let Some(pos) = examples.iter().position(|e| e.id == id)
    {
        let copy = Example { id: uuid::Uuid::new_v4(), name: format!("{} Copy", examples[pos].name), ..examples[pos].clone() };
        *selected = Some(copy.id);
        examples.insert(pos + 1, copy);
    }
    if let Some(id) = remove {
        examples.retain(|e| e.id != id);
        *selected = None;
    }
}

/// An example from a received response: status, headers and (text) body.
pub(super) fn example_from_response(request: &crate::model::Request, resp: &crate::http::ResponseData, name: String) -> Example {
    let headers: Vec<KeyValue> = resp
        .headers
        .iter()
        // Transport details that would be wrong once the body is edited or replayed by a mock.
        .filter(|(k, _)| !["content-length", "transfer-encoding", "connection", "date", "content-encoding", "keep-alive"].contains(&k.to_ascii_lowercase().as_str()))
        .map(|(k, v)| KeyValue::new(k, v))
        .collect();
    let lang = Example::lang_for_content_type(&resp.content_type());
    let text = resp.text();
    let body = if lang == RawLang::Json { super::request::beautify(&text) } else { text };
    Example { status: resp.status, headers, body, body_lang: lang, ..Example::from_request(request, name) }
}
