//! Custom-painted widgets in the style of Postman's desktop app.

use eframe::egui::{
    self, Align2, Color32, FontId, Frame, Id, Margin, Pos2, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, TextBuffer, Ui, pos2, text::LayoutJob, vec2,
};

use crate::icons;
use crate::model::{FormField, FormFieldKind, KeyValue, Method};
use crate::theme::*;
use crate::vars::{VarMap, substitute, var_spans};

// ---------------------------------------------------------------------------
// Basics
// ---------------------------------------------------------------------------

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER).size().x
}

/// Paints `text` vertically centered at `pos` (left aligned), truncating to `max_w`.
pub fn paint_text(ui: &Ui, pos: Pos2, text: &str, font: FontId, color: Color32, max_w: f32) -> f32 {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let w = galley.size().x;
    ui.painter().galley(pos2(pos.x, pos.y - galley.size().y / 2.0), galley, color);
    w
}

pub fn icon(ui: &mut Ui, glyph: &str, size: f32, color: Color32) -> Response {
    ui.label(RichText::new(glyph).font(icon_font(size)).color(color))
}

/// Square, frameless icon button with a hover background.
pub fn icon_button(ui: &mut Ui, glyph: &str, size: f32, tooltip: &str) -> Response {
    icon_button_sel(ui, glyph, size, tooltip, false)
}

pub fn icon_button_sel(ui: &mut Ui, glyph: &str, size: f32, tooltip: &str, selected: bool) -> Response {
    let p = pal(ui);
    let side = size + 12.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 6.0, p.active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let color = if resp.hovered() || selected { p.text } else { p.text2 };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(size), color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// Frameless text button (grey, white on hover), optional leading icon and trailing caret.
pub fn text_button(ui: &mut Ui, lead_icon: Option<&str>, label: &str, caret: bool) -> Response {
    let p = pal(ui);
    let font = regular(UI_SIZE);
    let icon_w = if lead_icon.is_some() { 20.0 } else { 0.0 };
    let caret_w = if caret { 16.0 } else { 0.0 };
    let w = 12.0 + icon_w + text_width(ui, label, font.clone()) + caret_w;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let color = if resp.hovered() { p.text } else { p.text2 };
    let mut x = rect.left() + 6.0;
    if let Some(g) = lead_icon {
        ui.painter().text(pos2(x + 8.0, rect.center().y), Align2::CENTER_CENTER, g, icon_font(15.0), color);
        x += icon_w;
    }
    x += paint_text(ui, pos2(x, rect.center().y), label, font, color, 1000.0);
    if caret {
        ui.painter().text(pos2(x + 9.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
    }
    resp
}

/// Bordered secondary button ("Cancel", "Select file…").
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    let p = pal(ui);
    let font = regular(UI_SIZE);
    let w = 24.0 + text_width(ui, label, font.clone());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let fill = if resp.hovered() { p.active } else { p.hover };
    ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0, p.input_border), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, p.text);
    resp
}

/// Solid blue primary button.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let p = pal(ui);
    let font = semibold(UI_SIZE);
    let w = 28.0 + text_width(ui, label, font.clone());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    ui.painter().rect_filled(rect, 4.0, if resp.hovered() { p.accent_hover } else { p.accent });
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, Color32::WHITE);
    resp
}

/// Tab used for request sections and response tabs: `Headers 24 •`.
pub fn pill_tab(ui: &mut Ui, lead_icon: Option<&str>, label: &str, count: Option<usize>, dot: bool, selected: bool) -> Response {
    let p = pal(ui);
    let font = regular(UI_SIZE);
    let label_w = text_width(ui, label, font.clone());
    let count_s = count.filter(|c| *c > 0).map(|c| c.to_string());
    let count_w = count_s.as_ref().map(|c| 5.0 + text_width(ui, c, regular(13.0))).unwrap_or(0.0);
    let icon_w = if lead_icon.is_some() { 20.0 } else { 0.0 };
    let dot_w = if dot { 11.0 } else { 0.0 };
    let w = 16.0 + icon_w + label_w + count_w + dot_w;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4.0, p.pill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let color = if selected || resp.hovered() { p.text } else { p.text2 };
    let cy = rect.center().y;
    let mut x = rect.left() + 8.0;
    if let Some(g) = lead_icon {
        ui.painter().text(pos2(x + 7.0, cy), Align2::CENTER_CENTER, g, icon_font(15.0), color);
        x += icon_w;
    }
    x += paint_text(ui, pos2(x, cy), label, font, color, 1000.0);
    if let Some(c) = count_s {
        x += 5.0;
        x += paint_text(ui, pos2(x, cy), &c, regular(13.0), p.text3, 100.0);
    }
    if dot {
        ui.painter().circle_filled(pos2(x + 7.0, cy + 0.5), 3.0, p.green_dot);
    }
    resp
}

/// Small method label (sidebar, tabs, history).
pub fn method_label_font(m: Method, size: f32) -> (String, FontId) {
    let s = match m {
        Method::Delete => "DEL",
        Method::Options => "OPT",
        other => other.as_str(),
    };
    (s.to_owned(), semibold(size))
}

/// Postman-style checkbox.
pub fn checkbox(ui: &mut Ui, rect: Rect, id: Id, checked: &mut bool) -> Response {
    let p = pal(ui);
    let mut resp = ui.interact(rect, id, Sense::click());
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let b = Rect::from_center_size(rect.center(), vec2(15.0, 15.0));
    if *checked {
        let fill = if p.dark { Color32::from_rgb(0x3b, 0x3b, 0x3b) } else { Color32::from_rgb(0x21, 0x21, 0x21) };
        ui.painter().rect(b, 3.0, fill, Stroke::new(1.0, if p.dark { p.text3 } else { fill }), StrokeKind::Inside);
        let c = b.center();
        let pts = vec![pos2(c.x - 3.5, c.y + 0.2), pos2(c.x - 1.0, c.y + 2.8), pos2(c.x + 3.8, c.y - 2.6)];
        ui.painter().add(egui::Shape::line(pts, Stroke::new(1.6, Color32::WHITE)));
    } else {
        let stroke = if resp.hovered() { p.text2 } else { p.text3 };
        ui.painter().rect_stroke(b, 3.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    }
    resp
}

/// Postman-style radio button with label.
pub fn radio(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let p = pal(ui);
    let font = regular(UI_SIZE);
    let w = 24.0 + text_width(ui, label, font.clone()) + 6.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let c = pos2(rect.left() + 9.0, rect.center().y);
    if selected {
        ui.painter().circle_filled(c, 8.0, Color32::from_rgb(0x4c, 0x8b, 0xf5));
        ui.painter().circle_filled(c, 3.0, Color32::WHITE);
    } else {
        ui.painter().circle_stroke(c, 7.5, Stroke::new(1.0, if resp.hovered() { p.text2 } else { p.text3 }));
    }
    paint_text(ui, pos2(rect.left() + 24.0, rect.center().y), label, font, p.text, 1000.0);
    resp
}

/// iOS-like toggle used on the Settings tab.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = pal(ui);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let off = if p.dark { Color32::from_rgb(0x4a, 0x4a, 0x4a) } else { Color32::from_rgb(0xd0, 0xd0, 0xd0) };
    let fill = lerp_color(off, p.accent, t);
    ui.painter().rect_filled(rect, 10.0, fill);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 7.0, Color32::WHITE);
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Horizontal 1px separator spanning the available width.
pub fn hline(ui: &mut Ui) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.border));
}

pub fn vsep(ui: &mut Ui, height: f32) {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, height), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, p.input_border));
}

/// Bordered box with an icon and a frameless text field inside (sidebar filter, header search).
pub fn search_box(ui: &mut Ui, id: Id, text: &mut String, hint: &str, width: f32, height: f32) -> Response {
    search_box_icon(ui, id, text, hint, width, height, icons::MAGNIFYING_GLASS)
}

pub fn search_box_icon(ui: &mut Ui, id: Id, text: &mut String, hint: &str, width: f32, height: f32, glyph: &str) -> Response {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(rect, 6.0, p.chrome, Stroke::new(1.0, if focused { p.text3 } else { p.input_border }), StrokeKind::Inside);
    ui.painter().text(pos2(rect.left() + 16.0, rect.center().y), Align2::CENTER_CENTER, glyph, icon_font(15.0), p.text2);
    let inner = Rect::from_min_max(pos2(rect.left() + 30.0, rect.top() + 1.0), pos2(rect.right() - 6.0, rect.bottom() - 1.0));
    ui.put(
        inner,
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(Frame::NONE)
            .hint_text(RichText::new(hint).color(p.text2))
            .align(egui::Align2::LEFT_CENTER)
            .desired_width(inner.width()),
    )
}

/// Variable-aware single-line text edit without frame (URL box, form inputs).
/// Long values are elided with "…" while not focused and scroll horizontally while editing.
pub fn var_edit(ui: &mut Ui, rect: Rect, id: Id, text: &mut String, vars: &VarMap, hint: &str, font: FontId) -> Response {
    var_edit_impl(ui, rect, id, text, vars, hint, font, false)
}

/// Table cell: like [`var_edit`], but when a long value is focused the cell expands into a
/// floating multi-line editor over the rows below (Postman behaviour).
pub fn var_cell(ui: &mut Ui, rect: Rect, id: Id, text: &mut String, vars: &VarMap, hint: &str, font: FontId) -> Response {
    var_edit_impl(ui, rect, id, text, vars, hint, font, true)
}

#[allow(clippy::too_many_arguments)]
fn var_edit_impl(ui: &mut Ui, rect: Rect, id: Id, text: &mut String, vars: &VarMap, hint: &str, font: FontId, expandable: bool) -> Response {
    let p = pal(ui);
    let normal = ui.visuals().override_text_color.unwrap_or(p.text);
    let focused = ui.memory(|m| m.has_focus(id));
    let overflows = text.contains('\n') || text_width(ui, text, font.clone()) > rect.width() - 2.0;
    if expandable && focused && overflows {
        return expanded_cell(ui, rect, id, text, vars, font, normal);
    }
    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, _wrap_width: f32| {
        let mut job = var_layout(buf.as_str(), vars, font.clone(), normal);
        if focused {
            job.wrap.max_width = f32::INFINITY;
        } else {
            job.wrap.max_width = rect.width();
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            job.wrap.overflow_character = Some('…');
        }
        ui.painter().layout_job(job)
    };
    let resp = ui.put(
        rect,
        egui::TextEdit::singleline(text)
            .id(id)
            .frame(Frame::NONE)
            .hint_text(RichText::new(hint).color(p.text3))
            .align(egui::Align2::LEFT_CENTER)
            .desired_width(rect.width())
            .clip_text(true)
            .layouter(&mut layouter),
    );
    if resp.hovered() && !resp.has_focus() && text.contains("{{") {
        let resolved = substitute(text, vars);
        if &resolved != text {
            return resp.on_hover_ui(|ui| {
                ui.set_max_width(600.0);
                ui.add(egui::Label::new(resolved).wrap());
            });
        }
    }
    resp
}

/// Floating, wrapping editor drawn on top of the table while a long cell is being edited.
fn expanded_cell(ui: &mut Ui, rect: Rect, id: Id, text: &mut String, vars: &VarMap, font: FontId, normal: Color32) -> Response {
    let p = pal(ui);
    // Table cells start 12px after the column line and end 8px before the next one.
    let (pad_l, pad_r): (f32, f32) = (12.0, 8.0);
    let line_h = ui.painter().layout_no_wrap("Ag".into(), font.clone(), Color32::PLACEHOLDER).size().y;
    let pad_top = ((rect.height() - line_h) / 2.0).max(2.0);
    let width = rect.width();
    let ctx = ui.ctx().clone();
    let area_id = id.with("expanded");
    let pos = pos2(rect.left() - pad_l, rect.top() - 1.0);
    // A brand-new Area spends its first frame in an invisible, non-interactive "sizing pass",
    // which would make the text edit lose focus immediately. Seed its state so it is live at once.
    if egui::AreaState::load(&ctx, area_id).is_none() {
        let est_h = rect.height() + 2.0;
        let state = egui::AreaState {
            pivot_pos: Some(pos),
            pivot: Align2::LEFT_TOP,
            size: Some(vec2(width + pad_l + pad_r, est_h)),
            interactable: true,
            last_became_visible_at: None,
        };
        ctx.memory_mut(|m| m.areas_mut().set_state(egui::LayerId::new(egui::Order::Foreground, area_id), state));
    }
    let area = egui::Area::new(area_id)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(&ctx, |ui| {
            Frame::new()
                .fill(p.bg)
                .stroke(Stroke::new(1.0, p.text2))
                .inner_margin(Margin { left: pad_l as i8, right: pad_r as i8, top: pad_top as i8, bottom: pad_top as i8 })
                .show(ui, |ui| {
                    ui.set_width(width);
                    let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, _wrap_width: f32| {
                        let mut job = var_layout(buf.as_str(), vars, font.clone(), normal);
                        job.wrap.max_width = width;
                        job.wrap.break_anywhere = true;
                        ui.painter().layout_job(job)
                    };
                    let resp = ui.add(
                        egui::TextEdit::multiline(text)
                            .id(id)
                            .frame(Frame::NONE)
                            .desired_width(width)
                            .desired_rows(1)
                            .return_key(Option::<egui::KeyboardShortcut>::None)
                            .layouter(&mut layouter),
                    );
                    // Enter finishes editing instead of inserting a newline.
                    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        resp.surrender_focus();
                    }
                    resp
                })
                .inner
        });
    // Keep the cell's own slot allocated so the table layout does not change.
    ui.allocate_rect(rect, Sense::hover());
    area.inner
}

pub fn var_layout(text: &str, vars: &VarMap, font: FontId, normal: Color32) -> LayoutJob {
    let ok = Color32::from_rgb(0x56, 0xc2, 0x7f);
    let bad = Color32::from_rgb(0xf2, 0x6b, 0x5b);
    let mut job = LayoutJob::default();
    let mut last = 0;
    let fmt = |c| egui::TextFormat::simple(font.clone(), c);
    for (s, e, resolved) in var_spans(text, vars) {
        job.append(&text[last..s], 0.0, fmt(normal));
        job.append(&text[s..e], 0.0, fmt(if resolved { ok } else { bad }));
        last = e;
    }
    job.append(&text[last..], 0.0, fmt(normal));
    job
}

/// Bordered single-line input (forms in dialogs, auth fields).
pub fn input(ui: &mut Ui, id: Id, text: &mut String, vars: &VarMap, hint: &str, width: f32) -> Response {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(rect, 4.0, p.bg, Stroke::new(1.0, if focused { p.text3 } else { p.input_border }), StrokeKind::Inside);
    var_edit(ui, rect.shrink2(vec2(10.0, 1.0)), id, text, vars, hint, regular(UI_SIZE))
}

// ---------------------------------------------------------------------------
// Modal dialogs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Secondary,
    Primary,
    Danger,
}

/// Postman modal button: 30px high, 6px radius.
pub fn modal_button(ui: &mut Ui, label: &str, kind: ButtonKind) -> Response {
    let p = pal(ui);
    let font = if kind == ButtonKind::Secondary { regular(UI_SIZE) } else { medium(UI_SIZE) };
    let w = 20.0 + text_width(ui, label, font.clone());
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let h = resp.hovered();
    let (fill, text) = match kind {
        ButtonKind::Secondary if p.dark => (if h { Color32::from_rgb(0x4a, 0x4a, 0x4a) } else { Color32::from_rgb(0x3b, 0x3b, 0x3b) }, p.text),
        ButtonKind::Secondary => (if h { Color32::from_rgb(0xe6, 0xe6, 0xe6) } else { Color32::from_rgb(0xf2, 0xf2, 0xf2) }, p.text),
        ButtonKind::Primary => (if h { p.accent_hover } else { p.accent }, Color32::WHITE),
        ButtonKind::Danger => (if h { Color32::from_rgb(0xc9, 0x40, 0x31) } else { Color32::from_rgb(0xe1, 0x4c, 0x3b) }, Color32::WHITE),
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, font, text);
    resp
}

/// Body text of a modal (14px, 1.75 line height like Postman).
pub fn modal_text(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    ui.add(egui::Label::new(RichText::new(text).font(regular(UI_SIZE)).color(p.text).line_height(Some(25.0))).wrap());
}

/// Postman-style modal: title with a close button, 20px padding, 12px radius, dimmed backdrop.
/// Returns the content's result and whether the dialog asked to close (✕, Esc or backdrop click).
pub fn modal<R>(ctx: &egui::Context, id: Id, title: &str, width: f32, content: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let p = pal_ctx(ctx);
    let fill = if p.dark { Color32::from_rgb(0x2b, 0x2b, 0x2b) } else { Color32::WHITE };
    let stroke = if p.dark { Stroke::new(1.0, p.input_border) } else { Stroke::NONE };
    let frame = Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(12)
        .inner_margin(Margin::same(20))
        .shadow(egui::Shadow { offset: [0, 8], blur: 32, spread: 0, color: Color32::from_black_alpha(60) });
    let resp = egui::Modal::new(id).frame(frame).backdrop_color(Color32::from_black_alpha(140)).show(ctx, |ui| {
        let inner_w = width - 40.0;
        ui.set_width(inner_w);
        let mut close = false;
        ui.horizontal(|ui| {
            ui.scope(|ui| {
                ui.set_max_width(inner_w - 36.0);
                ui.add(egui::Label::new(RichText::new(title).font(semibold(16.0)).color(p.text)).wrap());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, r) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                if r.hovered() {
                    ui.painter().rect_filled(rect, 4.0, p.hover);
                }
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, icons::X, icon_font(17.0), if r.hovered() { p.text } else { p.text2 });
                close = r.clicked();
            });
        });
        ui.add_space(14.0);
        let out = content(ui);
        (out, close)
    });
    let should_close = resp.should_close();
    let (out, close) = resp.inner;
    (out, close || should_close)
}

/// Right-aligned button row at the bottom of a modal.
pub fn modal_buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(22.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        add(ui);
    });
}

// ---------------------------------------------------------------------------
// Key/value table (Params, Headers, form bodies, variables)
// ---------------------------------------------------------------------------

pub const ROW_H: f32 = 31.0;

pub struct TableOpts<'a> {
    pub key_label: &'a str,
    pub value_label: &'a str,
    pub with_desc: bool,
    pub bulk: bool,
    /// Placeholders of the empty last row.
    pub key_hint: &'a str,
    pub value_hint: &'a str,
}

impl Default for TableOpts<'_> {
    fn default() -> Self {
        Self { key_label: "Key", value_label: "Value", with_desc: true, bulk: true, key_hint: "Key", value_hint: "Value" }
    }
}

struct Cols {
    key: f32,
    value: f32,
    desc: f32,
    right: f32,
}

fn columns(rect: Rect, with_desc: bool) -> Cols {
    let first = 56.0;
    let rest = rect.width() - first;
    let (kw, vw) = if with_desc { (rest * 0.34, rest * 0.34) } else { (rest * 0.5, rest * 0.5) };
    let key = rect.left() + first;
    let value = key + kw;
    let desc = value + vw;
    Cols { key, value, desc, right: rect.right() }
}

fn drag_handle(ui: &Ui, center: Pos2, color: Color32) {
    ui.painter().text(center, Align2::CENTER_CENTER, icons::DOTS_SIX_VERTICAL, icon_font(14.0), color);
}

/// Shared row chrome: checkbox, drag handle, delete button. Returns (row_rect, deleted, drag_resp).
fn row_frame(ui: &mut Ui, id: Id, i: usize, top: f32, rect: Rect, enabled: Option<&mut bool>, changed: &mut bool) -> (Rect, bool, Option<Response>) {
    let p = pal(ui);
    let row = Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), top + ROW_H));
    let hovered = ui.rect_contains_pointer(row);
    let mut deleted = false;
    let mut drag = None;
    if let Some(enabled) = enabled {
        let cb = Rect::from_center_size(pos2(rect.left() + 38.0, row.center().y), vec2(20.0, 20.0));
        if checkbox(ui, cb, id.with((i, "cb")), enabled).changed() {
            *changed = true;
        }
        let handle = Rect::from_center_size(pos2(rect.left() + 16.0, row.center().y), vec2(16.0, 22.0));
        let r = ui.interact(handle, id.with((i, "drag")), Sense::drag());
        if hovered || r.dragged() {
            drag_handle(ui, handle.center(), if r.hovered() { p.text } else { p.text3 });
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        drag = Some(r);
        if hovered {
            let del = Rect::from_center_size(pos2(rect.right() - 18.0, row.center().y), vec2(24.0, 24.0));
            let d = ui.interact(del, id.with((i, "del")), Sense::click());
            if d.hovered() {
                ui.painter().rect_filled(del, 4.0, p.hover);
            }
            ui.painter().text(del.center(), Align2::CENTER_CENTER, icons::TRASH, icon_font(15.0), if d.hovered() { p.text } else { p.text2 });
            if d.on_hover_text("Delete").clicked() {
                deleted = true;
            }
        }
    }
    (row, deleted, drag)
}

fn paint_grid(ui: &Ui, rect: Rect, cols: &Cols, rows: usize, with_desc: bool, bulk_w: f32) {
    let p = pal(ui);
    let s = Stroke::new(1.0, p.border);
    let painter = ui.painter();
    painter.rect_stroke(rect, 0.0, s, StrokeKind::Inside);
    for r in 1..=rows {
        let y = rect.top() + ROW_H * r as f32;
        painter.hline(rect.x_range(), y, s);
    }
    let body = rect.top()..=rect.bottom();
    painter.vline(cols.value, body.clone(), s);
    if with_desc {
        painter.vline(cols.desc, body, s);
    }
    painter.vline(cols.key, rect.top()..=rect.bottom(), Stroke::new(1.0, Color32::TRANSPARENT));
    if bulk_w > 0.0 {
        painter.vline(cols.right - bulk_w, rect.top()..=rect.top() + ROW_H, s);
    }
}

fn header_row(ui: &mut Ui, id: Id, rect: Rect, cols: &Cols, opts: &TableOpts, bulk_mode: &mut bool) -> f32 {
    let p = pal(ui);
    let y = rect.top() + ROW_H / 2.0;
    let f = medium(UI_SIZE);
    paint_text(ui, pos2(cols.key + 12.0, y), opts.key_label, f.clone(), p.text2, cols.value - cols.key - 16.0);
    paint_text(ui, pos2(cols.value + 12.0, y), opts.value_label, f.clone(), p.text2, (if opts.with_desc { cols.desc } else { cols.right }) - cols.value - 16.0);
    if opts.with_desc {
        paint_text(ui, pos2(cols.desc + 12.0, y), "Description", f, p.text2, 200.0);
    }
    if !opts.bulk {
        return 0.0;
    }
    let bulk_w: f32 = 118.0;
    let label = if *bulk_mode { "Key-Value Edit" } else { "Bulk Edit" };
    let lw = text_width(ui, label, regular(UI_SIZE));
    let bulk_w = bulk_w.max(lw + 50.0);
    let r = Rect::from_min_max(pos2(cols.right - bulk_w + 8.0, rect.top() + 3.0), pos2(cols.right - 34.0, rect.top() + ROW_H - 3.0));
    let resp = ui.interact(r, id.with("bulk_btn"), Sense::click());
    paint_text(ui, pos2(r.left() + 4.0, y), label, regular(UI_SIZE), if resp.hovered() { p.text } else { p.text2 }, 200.0);
    if resp.clicked() {
        *bulk_mode = !*bulk_mode;
    }
    ui.painter().text(pos2(cols.right - 18.0, y), Align2::CENTER_CENTER, icons::DOTS_THREE, icon_font(16.0), p.text2);
    bulk_w
}

fn bulk_to_text(rows: &[KeyValue]) -> String {
    rows.iter()
        .map(|kv| format!("{}{}:{}", if kv.enabled { "" } else { "//" }, kv.key, kv.value))
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_to_bulk(text: &str, old: &[KeyValue]) -> Vec<KeyValue> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (enabled, l) = match l.trim_start().strip_prefix("//") {
                Some(rest) => (false, rest),
                None => (true, l),
            };
            let (k, v) = l.split_once(':').unwrap_or((l, ""));
            let description = old.iter().find(|o| o.key == k.trim()).map(|o| o.description.clone()).unwrap_or_default();
            KeyValue { enabled, key: k.trim().to_string(), value: v.trim_start().to_string(), description }
        })
        .collect()
}

/// Reorders rows when a drag handle is released over another row.
fn handle_reorder<T>(ui: &Ui, rows: &mut Vec<T>, drags: &[(usize, Response)], table_top: f32, rect: Rect) -> bool {
    let p = pal(ui);
    for (i, r) in drags {
        if !(r.dragged() || r.drag_stopped()) {
            continue;
        }
        let Some(pos) = ui.ctx().pointer_latest_pos() else { continue };
        let target = (((pos.y - table_top) / ROW_H).round() as isize).clamp(0, rows.len() as isize) as usize;
        if r.dragged() {
            let y = table_top + target as f32 * ROW_H;
            ui.painter().hline(rect.x_range(), y, Stroke::new(2.0, p.accent));
        }
        if r.drag_stopped() && target != *i && target != i + 1 {
            let item = rows.remove(*i);
            let dest = if target > *i { target - 1 } else { target };
            rows.insert(dest.min(rows.len()), item);
            return true;
        }
    }
    false
}

/// Postman-style editable key/value table. Returns true if anything changed.
pub fn kv_table(ui: &mut Ui, id: Id, rows: &mut Vec<KeyValue>, vars: &VarMap, opts: TableOpts) -> bool {
    let p = pal(ui);
    let mut changed = false;
    let mut bulk_mode = ui.data(|d| d.get_temp::<bool>(id.with("bulk_mode"))).unwrap_or(false);
    let width = ui.available_width();
    let header = Rect::from_min_size(ui.cursor().min, vec2(width, ROW_H));
    let cols = columns(header, opts.with_desc);
    let was_bulk = bulk_mode;

    if bulk_mode {
        let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, p.border), StrokeKind::Inside);
        header_row(ui, id, rect, &cols, &opts, &mut bulk_mode);
        let key = id.with("bulk_text");
        let mut text = ui.data(|d| d.get_temp::<String>(key)).unwrap_or_else(|| bulk_to_text(rows));
        let resp = ui.add(
            egui::TextEdit::multiline(&mut text)
                .code_editor()
                .desired_rows(8)
                .desired_width(width)
                .hint_text("Rows are separated by new lines\nKeys and values are separated by :\nPrepend // to any row you want to add but keep disabled"),
        );
        if resp.changed() {
            *rows = text_to_bulk(&text, rows);
            changed = true;
        }
        ui.data_mut(|d| d.insert_temp(key, text));
    } else {
        let n = rows.len() + 1;
        let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H * (n as f32 + 1.0)), Sense::hover());
        let bulk_w = header_row(ui, id, rect, &cols, &opts, &mut bulk_mode);
        paint_grid(ui, rect, &cols, n, opts.with_desc, bulk_w);
        let font = regular(UI_SIZE);
        let mut remove = None;
        let mut drags = Vec::new();
        for i in 0..n {
            let ghost = i == rows.len();
            let top = rect.top() + ROW_H * (i as f32 + 1.0);
            let mut tmp = KeyValue::default();
            let row = if ghost { &mut tmp } else { &mut rows[i] };
            let (r, deleted, drag) = row_frame(ui, id, i, top, rect, if ghost { None } else { Some(&mut row.enabled) }, &mut changed);
            if deleted {
                remove = Some(i);
            }
            if let Some(d) = drag {
                drags.push((i, d));
            }
            let text_color_dim = !ghost && !row.enabled;
            let cell = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0 + 12.0, r.top() + 1.0), pos2(x1 - 8.0, r.bottom() - 1.0));
            let value_end = if opts.with_desc { cols.desc } else { cols.right - 30.0 };
            let f = font.clone();
            ui.scope(|ui| {
                if text_color_dim {
                    ui.style_mut().visuals.override_text_color = Some(p.text3);
                }
                // Like Postman, placeholders only show on the empty last row.
                let (hk, hv, hd) = if ghost { (opts.key_hint, opts.value_hint, "Description") } else { ("", "", "") };
                changed |= var_cell(ui, cell(cols.key, cols.value), id.with((i, "k")), &mut row.key, vars, hk, f.clone()).changed();
                changed |= var_cell(ui, cell(cols.value, value_end), id.with((i, "v")), &mut row.value, vars, hv, f.clone()).changed();
                if opts.with_desc {
                    let desc_rect = cell(cols.desc, cols.right - 30.0);
                    changed |= var_cell(ui, desc_rect, id.with((i, "d")), &mut row.description, vars, hd, f.clone()).changed();
                }
            });
            if ghost && !tmp.is_blank() {
                rows.push(tmp);
                changed = true;
            }
        }
        if let Some(i) = remove {
            rows.remove(i);
            changed = true;
        }
        changed |= handle_reorder(ui, rows, &drags, rect.top() + ROW_H, rect);
    }
    if was_bulk != bulk_mode {
        // Entering bulk mode starts from the current rows.
        ui.data_mut(|d| d.remove::<String>(id.with("bulk_text")));
    }
    ui.data_mut(|d| d.insert_temp(id.with("bulk_mode"), bulk_mode));
    changed
}

/// multipart/form-data table: key cell has a Text/File selector, value is text or a file picker.
pub fn form_table(ui: &mut Ui, id: Id, rows: &mut Vec<FormField>, vars: &VarMap) -> bool {
    let p = pal(ui);
    let mut changed = false;
    let width = ui.available_width();
    let n = rows.len() + 1;
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H * (n as f32 + 1.0)), Sense::hover());
    let cols = columns(rect, false);
    let mut dummy = false;
    let opts = TableOpts { with_desc: false, bulk: false, ..Default::default() };
    header_row(ui, id, rect, &cols, &opts, &mut dummy);
    paint_grid(ui, rect, &cols, n, false, 0.0);
    let font = regular(UI_SIZE);
    let mut remove = None;
    let mut drags = Vec::new();
    for i in 0..n {
        let ghost = i == rows.len();
        let top = rect.top() + ROW_H * (i as f32 + 1.0);
        let mut tmp = FormField::default();
        let row = if ghost { &mut tmp } else { &mut rows[i] };
        let (r, deleted, drag) = row_frame(ui, id, i, top, rect, if ghost { None } else { Some(&mut row.enabled) }, &mut changed);
        if deleted {
            remove = Some(i);
        }
        if let Some(d) = drag {
            drags.push((i, d));
        }
        // Key + type selector.
        let type_w = 62.0;
        let key_rect = Rect::from_min_max(pos2(cols.key + 12.0, r.top() + 1.0), pos2(cols.value - type_w - 4.0, r.bottom() - 1.0));
        changed |= var_cell(ui, key_rect, id.with((i, "k")), &mut row.key, vars, if ghost { "Key" } else { "" }, font.clone()).changed();
        let type_rect = Rect::from_min_max(pos2(cols.value - type_w, r.top() + 4.0), pos2(cols.value - 4.0, r.bottom() - 4.0));
        let tr = ui.interact(type_rect, id.with((i, "type")), Sense::click());
        if tr.hovered() {
            ui.painter().rect_filled(type_rect, 4.0, p.hover);
        }
        let tlabel = if row.kind == FormFieldKind::File { "File" } else { "Text" };
        paint_text(ui, pos2(type_rect.left() + 6.0, type_rect.center().y), tlabel, regular(13.0), p.text2, 40.0);
        ui.painter().text(pos2(type_rect.right() - 9.0, type_rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(11.0), p.text2);
        egui::Popup::menu(&tr).show(|ui| {
            for (k, l) in [(FormFieldKind::Text, "Text"), (FormFieldKind::File, "File")] {
                if ui.selectable_label(row.kind == k, l).clicked() && row.kind != k {
                    row.kind = k;
                    row.value.clear();
                    changed = true;
                }
            }
        });
        // Value.
        let val_rect = Rect::from_min_max(pos2(cols.value + 12.0, r.top() + 1.0), pos2(cols.right - 34.0, r.bottom() - 1.0));
        match row.kind {
            FormFieldKind::Text => {
                changed |= var_cell(ui, val_rect, id.with((i, "v")), &mut row.value, vars, if ghost { "Value" } else { "" }, font.clone()).changed();
            }
            FormFieldKind::File => {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(val_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                if secondary_button(&mut child, if row.value.is_empty() { "Select files" } else { "Change" }).clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file() {
                        row.value = path.display().to_string();
                        changed = true;
                    }
                if !row.value.is_empty() {
                    child.add(egui::Label::new(RichText::new(&row.value).color(p.text2)).truncate());
                }
            }
        }
        if ghost && !(tmp.key.is_empty() && tmp.value.is_empty() && tmp.kind == FormFieldKind::Text) {
            rows.push(tmp);
            changed = true;
        }
    }
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    changed |= handle_reorder(ui, rows, &drags, rect.top() + ROW_H, rect);
    changed
}

/// Read-only table (response headers, cookies, auto-generated headers).
pub fn ro_table(ui: &mut Ui, headers: &[&str], rows: &[Vec<String>], weights: &[f32], dim: bool) {
    let p = pal(ui);
    let width = ui.available_width();
    let n = rows.len();
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H * (n as f32 + 1.0)), Sense::hover());
    let s = Stroke::new(1.0, p.border);
    ui.painter().rect_stroke(rect, 0.0, s, StrokeKind::Inside);
    let total: f32 = weights.iter().sum();
    let mut xs = vec![rect.left()];
    for w in weights {
        let last = *xs.last().unwrap();
        xs.push(last + rect.width() * w / total);
    }
    for x in &xs[1..xs.len() - 1] {
        ui.painter().vline(*x, rect.y_range(), s);
    }
    for r in 1..=n {
        ui.painter().hline(rect.x_range(), rect.top() + ROW_H * r as f32, s);
    }
    for (c, h) in headers.iter().enumerate() {
        paint_text(ui, pos2(xs[c] + 12.0, rect.top() + ROW_H / 2.0), h, medium(UI_SIZE), p.text2, xs[c + 1] - xs[c] - 20.0);
    }
    for (r, row) in rows.iter().enumerate() {
        let y = rect.top() + ROW_H * (r as f32 + 1.5);
        for (c, cell) in row.iter().enumerate().take(headers.len()) {
            let color = if dim { p.text2 } else if c == 0 { p.text } else { p.text2 };
            let cell_rect = Rect::from_min_max(pos2(xs[c] + 12.0, y - ROW_H / 2.0 + 1.0), pos2(xs[c + 1] - 8.0, y + ROW_H / 2.0 - 1.0));
            let resp = ui.put(cell_rect, egui::Label::new(RichText::new(cell).color(color)).truncate().selectable(true));
            if resp.hovered() && cell.len() > 30 {
                resp.on_hover_text(cell);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Code viewer with line numbers, indent guides, wrapping and search
// ---------------------------------------------------------------------------

/// One visual row of the viewer: a slice of a logical line.
#[derive(Clone, Copy)]
pub struct VisualRow {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct WrapCache {
    key: (usize, usize, usize),
    rows: Vec<VisualRow>,
}

/// Splits lines into visual rows of at most `cols` characters. Continuation rows are indented
/// to the line's own indentation plus `hang` extra columns.
fn wrap_rows(lines: &[String], cols: usize, wrap: bool, hang: usize) -> Vec<VisualRow> {
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        if !wrap || l.chars().count() <= cols {
            out.push(VisualRow { line: i, start: 0, end: l.len() });
            continue;
        }
        let indent = l.len() - l.trim_start().len();
        let mut start = 0;
        let mut count = 0;
        let mut first = true;
        for (b, _) in l.char_indices() {
            let limit = if first { cols } else { cols.saturating_sub(indent + hang).max(10) };
            if count >= limit {
                out.push(VisualRow { line: i, start, end: b });
                start = b;
                count = 0;
                first = false;
            }
            count += 1;
        }
        out.push(VisualRow { line: i, start, end: l.len() });
    }
    out
}

#[derive(Clone, Copy)]
pub enum Highlight {
    Json,
    Plain,
    Code(crate::codegen::Syntax),
}

/// Find the byte ranges of `needle` (case-insensitive ASCII) in `hay`.
pub fn find_all(hay: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let h = hay.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(pos) = h[i..].find(&n) {
        let s = i + pos;
        out.push((s, s + n.len()));
        i = s + n.len().max(1);
        if i >= h.len() {
            break;
        }
    }
    out
}

/// Byte ranges of JSON tokens with their colors (tolerant of invalid / partial JSON).
fn json_ranges(text: &str, p: &Palette) -> Vec<(usize, usize, Color32)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(bytes.len());
            while i < bytes.len() && !text.is_char_boundary(i) {
                i += 1;
            }
            let mut j = i;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            let color = if j < bytes.len() && bytes[j] == b':' { p.json_key } else { p.json_string };
            out.push((start, i, color));
        } else if c == b'-' || c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-')) {
                i += 1;
            }
            out.push((start, i, p.json_number));
        } else if c.is_ascii_alphabetic() {
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            out.push((start, i, p.json_literal));
        } else {
            while i < bytes.len() {
                let b = bytes[i];
                if b == b'"' || b == b'-' || b.is_ascii_alphanumeric() {
                    break;
                }
                i += 1;
            }
            while i < bytes.len() && !text.is_char_boundary(i) {
                i += 1;
            }
            out.push((start, i, p.json_punct));
        }
    }
    out
}

fn tok_color(t: crate::codegen::Tok, p: &Palette) -> Color32 {
    use crate::codegen::Tok;
    match t {
        Tok::Plain => p.text,
        Tok::Keyword => p.code_keyword,
        Tok::String => p.code_string,
        Tok::Number => p.code_number,
        Tok::Comment => p.code_comment,
        Tok::Function => p.code_function,
        Tok::Type => p.code_type,
        Tok::Property => p.code_property,
        Tok::Variable => p.code_variable,
        Tok::Punct => p.code_punct,
    }
}

/// Colored byte ranges for a whole logical line.
fn line_colors(line: &str, hl: Highlight, p: &Palette) -> Vec<(usize, usize, Color32)> {
    match hl {
        Highlight::Json => json_ranges(line, p),
        Highlight::Plain => vec![(0, line.len(), p.text)],
        Highlight::Code(syn) => crate::codegen::tokenize(line, syn).into_iter().map(|(s, e, t)| (s, e, tok_color(t, p))).collect(),
    }
}

/// Builds a layout job for `line[start..end]` from ranges computed on the whole line,
/// so tokens split by wrapping keep their color.
fn sliced_job(line: &str, start: usize, end: usize, ranges: &[(usize, usize, Color32)], font: &FontId, fallback: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut pos = start;
    for (s, e, c) in ranges {
        let (s, e) = ((*s).max(start), (*e).min(end));
        if s >= e {
            continue;
        }
        if s > pos {
            job.append(&line[pos..s], 0.0, egui::TextFormat::simple(font.clone(), fallback));
        }
        job.append(&line[s..e], 0.0, egui::TextFormat::simple(font.clone(), *c));
        pos = e;
    }
    if pos < end {
        job.append(&line[pos..end], 0.0, egui::TextFormat::simple(font.clone(), fallback));
    }
    job
}

/// Layout job for multi-line source code (used by the script editor).
pub fn code_job(text: &str, syn: crate::codegen::Syntax, font: FontId, p: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            job.append("\n", 0.0, egui::TextFormat::simple(font.clone(), p.text));
        }
        let mut pos = 0;
        for (s, e, c) in line_colors(line, Highlight::Code(syn), p) {
            let (s, e) = (s.max(pos), e.min(line.len()));
            if s >= e {
                continue;
            }
            if s > pos {
                job.append(&line[pos..s], 0.0, egui::TextFormat::simple(font.clone(), p.text));
            }
            job.append(&line[s..e], 0.0, egui::TextFormat::simple(font.clone(), c));
            pos = e;
        }
        if pos < line.len() {
            job.append(&line[pos..], 0.0, egui::TextFormat::simple(font.clone(), p.text));
        }
    }
    job
}

pub fn json_job_colored(text: &str, font: FontId, p: &Palette) -> LayoutJob {
    let ranges = json_ranges(text, p);
    sliced_job(text, 0, text.len(), &ranges, &font, p.text)
}

/// Marks search matches with a background color.
fn apply_search(job: &mut LayoutJob, matches: &[(usize, usize)], current: Option<(usize, usize)>) {
    if matches.is_empty() {
        return;
    }
    use egui::epaint::text::ByteIndex;
    let mut sections = Vec::new();
    for sec in job.sections.drain(..) {
        let (start, end) = (sec.byte_range.start.0, sec.byte_range.end.0);
        let mut cuts = vec![start, end];
        for (s, e) in matches {
            for b in [*s, *e] {
                if b > start && b < end {
                    cuts.push(b);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let mut part = sec.clone();
            part.byte_range = ByteIndex(w[0])..ByteIndex(w[1]);
            if w[0] != start {
                part.leading_space = 0.0;
            }
            if let Some(m) = matches.iter().find(|(s, e)| w[0] >= *s && w[1] <= *e) {
                part.format.background = if Some(*m) == current {
                    Color32::from_rgba_unmultiplied(0xff, 0x9f, 0x0a, 160)
                } else {
                    Color32::from_rgba_unmultiplied(0xff, 0xd3, 0x3d, 70)
                };
            }
            sections.push(part);
        }
    }
    job.sections = sections;
}

pub struct CodeViewState<'a> {
    pub wrap: bool,
    /// Extra indentation (in columns) of wrapped continuation rows.
    pub hang: usize,
    pub search: &'a str,
    /// Index into all matches of the current match; scrolls to it when `scroll_to_match` is set.
    pub current_match: usize,
    pub scroll_to_match: bool,
}

/// Virtualized code viewer. Returns the total number of search matches.
pub fn code_view(ui: &mut Ui, id: Id, lines: &[String], hl: Highlight, cache: &mut WrapCache, st: CodeViewState) -> usize {
    let p = pal(ui);
    let font = mono(MONO_SIZE);
    let row_h = 22.0;
    let char_w = text_width(ui, "0000000000", font.clone()) / 10.0;
    let digits = lines.len().to_string().len().max(2);
    let gutter = char_w * digits as f32 + 28.0;
    let avail_w = ui.available_width() - gutter - 16.0;
    let cols = ((avail_w / char_w).floor() as usize).max(20);
    let key = (lines.as_ptr() as usize, lines.len(), if st.wrap { cols * 16 + st.hang } else { 0 });
    if cache.key != key {
        cache.rows = wrap_rows(lines, cols, st.wrap, st.hang);
        cache.key = key;
    }
    let rows = &cache.rows;

    // Search: matches per logical line.
    let mut all_matches: Vec<(usize, usize, usize)> = Vec::new(); // (line, start, end)
    if !st.search.is_empty() {
        for (i, l) in lines.iter().enumerate() {
            for (s, e) in find_all(l, st.search) {
                all_matches.push((i, s, e));
            }
        }
    }
    let current = all_matches.get(st.current_match.min(all_matches.len().saturating_sub(1))).copied();
    let mut area = egui::ScrollArea::both().id_salt(id).auto_shrink(false);
    if st.scroll_to_match
        && let Some((line, s, _)) = current
            && let Some(row) = rows.iter().position(|r| r.line == line && s >= r.start && s <= r.end) {
                area = area.vertical_scroll_offset((row as f32 * row_h - 3.0 * row_h).max(0.0));
            }
    // `show_rows` computes the visible range from the item spacing *at call time*.
    ui.spacing_mut().item_spacing.y = 0.0;
    area.show_rows(ui, row_h, rows.len(), |ui, range| {
        let mut cached: Option<(usize, Vec<(usize, usize, Color32)>)> = None;
        for vi in range {
            let vr = rows[vi];
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(gutter + 10.0), row_h), Sense::hover());
            let line = &lines[vr.line];
            // Line number (only on the first visual row of a line).
            if vr.start == 0 {
                ui.painter().text(
                    pos2(rect.left() + gutter - 20.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    (vr.line + 1).to_string(),
                    font.clone(),
                    p.line_number,
                );
            }
            let text_x = rect.left() + gutter;
            let indent = line.len() - line.trim_start_matches(' ').len();
            // Indent guides every 4 spaces (JSON).
            if matches!(hl, Highlight::Json) {
                for level in 0..indent / 4 {
                    let x = text_x + (level * 4) as f32 * char_w + 1.0;
                    ui.painter().vline(x, rect.y_range(), Stroke::new(1.0, p.indent_guide));
                }
            }
            // Tokenize the whole logical line once, then cut out this visual row.
            if cached.as_ref().is_none_or(|(l, _)| *l != vr.line) {
                cached = Some((vr.line, line_colors(line, hl, p)));
            }
            let ranges = &cached.as_ref().expect("just set").1;
            let mut job = sliced_job(line, vr.start, vr.end, ranges, &font, p.text);
            if !all_matches.is_empty() {
                let ms: Vec<(usize, usize)> = all_matches
                    .iter()
                    .filter(|(l, s, e)| *l == vr.line && *e > vr.start && *s < vr.end)
                    .map(|(_, s, e)| ((*s).max(vr.start) - vr.start, (*e).min(vr.end) - vr.start))
                    .collect();
                let cur = current.filter(|(l, _, _)| *l == vr.line).map(|(_, s, e)| (s.max(vr.start) - vr.start, e.min(vr.end) - vr.start));
                apply_search(&mut job, &ms, cur);
            }
            let x0 = if vr.start > 0 { text_x + ((indent + st.hang) as f32) * char_w } else { text_x };
            if vr.start == vr.end {
                continue; // empty line: nothing to place
            }
            let galley = ui.painter().layout_job(job);
            // The label spans the full row height: `put` must never move the layout cursor
            // above the row's bottom, or the following rows would overlap.
            let label_rect = Rect::from_min_max(pos2(x0, rect.top()), pos2(x0 + galley.size().x, rect.bottom()));
            // Placed as a widget so horizontal scrolling knows the content width, and allows selection.
            ui.put(label_rect, egui::Label::new(galley).selectable(true).extend());
        }
    });
    all_matches.len()
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

pub fn margin(x: i8, y: i8) -> Margin {
    Margin::symmetric(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_roundtrip() {
        let rows = vec![KeyValue::new("a", "1"), KeyValue { enabled: false, ..KeyValue::new("b", "x:y") }];
        let text = bulk_to_text(&rows);
        assert_eq!(text, "a:1\n//b:x:y");
        let back = text_to_bulk(&text, &rows);
        assert_eq!(back, rows);
    }

    #[test]
    fn wrapping_and_search() {
        let lines = vec!["    \"key\": \"abcdefghijklmnopqrstuvwxyz\"".to_string()];
        let rows = wrap_rows(&lines, 20, true, 0);
        assert!(rows.len() > 1);
        assert_eq!(rows.last().unwrap().end, lines[0].len());
        assert_eq!(find_all("Hello hello", "HELLO"), vec![(0, 5), (6, 11)]);
    }
}
