//! Console: a log of every request sent, with the resolved request and the response (like Postman's console).

use std::collections::VecDeque;
use std::time::Duration;

use eframe::egui::{self, Align, Align2, Id, Layout, RichText, Sense, Stroke, Ui, pos2, vec2};
use uuid::Uuid;

use super::RustmanApp;
use crate::http::{Prepared, PreparedBody, ResponseData, human_duration, human_size};
use crate::icons;
use crate::model::Method;
use crate::pm::{self, paint_text};
use crate::script::LogLevel;
use crate::theme::{icon_font, mono, pal, regular, semibold};

/// Keep at most this many entries.
const MAX_ENTRIES: usize = 500;
/// Bodies are stored truncated to this many bytes.
const MAX_BODY: usize = 64 * 1024;

pub(super) struct ConsoleEntry {
    id: Uuid,
    time: chrono::DateTime<chrono::Local>,
    method: Method,
    url: String,
    req_headers: Vec<(String, String)>,
    req_body: String,
    outcome: Outcome,
    expanded: bool,
}

enum Outcome {
    Pending,
    Done(Box<LoggedResponse>),
    Failed(String),
    /// A `console.log` line from a script (not a request).
    Log(LogLevel, String),
}

struct LoggedResponse {
    status: u16,
    status_text: String,
    http_version: String,
    remote_addr: Option<String>,
    final_url: String,
    duration: Duration,
    size: usize,
    headers: Vec<(String, String)>,
    body: String,
}

impl ConsoleEntry {
    fn is_error(&self) -> bool {
        match &self.outcome {
            Outcome::Failed(_) | Outcome::Log(LogLevel::Error, _) => true,
            Outcome::Done(r) => r.status >= 400,
            Outcome::Pending | Outcome::Log(..) => false,
        }
    }

    fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        let status = match &self.outcome {
            Outcome::Done(r) => r.status.to_string(),
            Outcome::Failed(e) | Outcome::Log(_, e) => e.to_lowercase(),
            Outcome::Pending => String::new(),
        };
        self.url.to_lowercase().contains(&q) || self.method.as_str().to_lowercase().contains(&q) || status.contains(&q)
    }
}

#[derive(Default)]
pub(super) struct Console {
    entries: VecDeque<ConsoleEntry>,
    filter: String,
    errors_only: bool,
}

impl Console {
    /// Logs a request that is about to be sent; returns the id to finish it with.
    pub fn start(&mut self, p: &Prepared) -> Uuid {
        let content_type = p.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str()).unwrap_or("");
        let req_body = match &p.body {
            PreparedBody::None => String::new(),
            PreparedBody::Bytes(b) => body_preview(b, content_type),
            PreparedBody::File(path) => format!("<binary file: {path}>"),
            PreparedBody::Multipart(parts) => parts
                .iter()
                .map(|part| if part.is_file { format!("{}: <file: {}>", part.name, part.value) } else { format!("{}: {}", part.name, part.value) })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        self.push(ConsoleEntry {
            id: Uuid::new_v4(),
            time: chrono::Local::now(),
            method: p.method,
            url: p.url.clone(),
            req_headers: p.headers.clone(),
            req_body,
            outcome: Outcome::Pending,
            expanded: false,
        })
    }

    /// Logs a request that could not even be built (bad URL, invalid GraphQL variables...).
    pub fn failed_to_prepare(&mut self, method: Method, url: &str, error: &str) {
        self.push(ConsoleEntry {
            id: Uuid::new_v4(),
            time: chrono::Local::now(),
            method,
            url: url.to_string(),
            req_headers: Vec::new(),
            req_body: String::new(),
            outcome: Outcome::Failed(error.to_string()),
            expanded: false,
        });
    }

    /// A line printed by a script.
    pub fn log(&mut self, level: LogLevel, text: String) {
        self.push(ConsoleEntry {
            id: Uuid::new_v4(),
            time: chrono::Local::now(),
            method: Method::Get,
            url: String::new(),
            req_headers: Vec::new(),
            req_body: String::new(),
            outcome: Outcome::Log(level, text),
            expanded: false,
        });
    }

    pub fn finish(&mut self, id: Uuid, result: &Result<ResponseData, String>) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        e.outcome = match result {
            Ok(r) => Outcome::Done(Box::new(LoggedResponse {
                status: r.status,
                status_text: r.status_text.clone(),
                http_version: r.http_version.clone(),
                remote_addr: r.remote_addr.clone(),
                final_url: r.url.clone(),
                duration: r.duration,
                size: r.body.len(),
                headers: r.headers.clone(),
                body: body_preview(&r.body, &r.content_type()),
            })),
            Err(err) => Outcome::Failed(err.clone()),
        };
    }

    pub fn cancel(&mut self, id: Uuid) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
            && matches!(e.outcome, Outcome::Pending)
        {
            e.outcome = Outcome::Failed("Request cancelled".into());
        }
    }

    fn push(&mut self, e: ConsoleEntry) -> Uuid {
        let id = e.id;
        self.entries.push_back(e);
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
        id
    }

    /// Dev hook for screenshots.
    #[cfg(debug_assertions)]
    pub fn expand_all(&mut self) {
        self.entries.iter_mut().for_each(|e| e.expanded = true);
    }

    pub fn error_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_error()).count()
    }
}

/// Text shown for a body: pretty JSON when possible, truncated, binary summarized.
fn body_preview(bytes: &[u8], content_type: &str) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        let kind = if content_type.is_empty() { "binary" } else { content_type };
        return format!("<{kind}, {}>", human_size(bytes.len()));
    };
    let pretty = if bytes.len() <= 1024 * 1024 {
        serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| serde_json::to_string_pretty(&v).ok())
    } else {
        None
    };
    let text = pretty.unwrap_or_else(|| text.to_string());
    if text.len() <= MAX_BODY {
        return text;
    }
    let mut cut = MAX_BODY;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… (truncated, {} total)", &text[..cut], human_size(bytes.len()))
}

#[derive(Clone, Copy)]
enum RowAction {
    Toggle(usize),
    CopyUrl(usize),
    CopyBody(usize),
    CopyText(usize),
}

impl RustmanApp {
    pub(super) fn console_panel(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let mut clear = false;
        let mut close = false;

        // Toolbar.
        ui.horizontal(|ui| {
            ui.set_height(34.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            self.bottom_tabs(ui);
            pm::search_box(ui, Id::unique("console_filter"), &mut self.console.filter, "Search messages", 240.0, 28.0);
            let label = if self.console.errors_only { "Errors" } else { "All logs" };
            let btn = pm::text_button(ui, None, label, true);
            egui::Popup::menu(&btn).show(|ui| {
                if ui.selectable_label(!self.console.errors_only, "All logs").clicked() {
                    self.console.errors_only = false;
                    ui.close();
                }
                if ui.selectable_label(self.console.errors_only, "Errors").clicked() {
                    self.console.errors_only = true;
                    ui.close();
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if pm::icon_button(ui, icons::X, 14.0, "Close console (Ctrl+Alt+C)").clicked() {
                    close = true;
                }
                if pm::text_button(ui, Some(icons::TRASH), "Clear", false).clicked() {
                    clear = true;
                }
            });
        });
        pm::hline(ui);

        let console = &mut self.console;
        let query = console.filter.trim().to_string();
        let visible: Vec<usize> = (0..console.entries.len())
            .filter(|&i| {
                let e = &console.entries[i];
                (!console.errors_only || e.is_error()) && e.matches(&query)
            })
            .collect();

        let mut action = None;
        egui::ScrollArea::vertical().id_salt("console_scroll").auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if visible.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    let msg = if console.entries.is_empty() { "No logs yet. Send a request to see it here." } else { "No logs match the filter." };
                    ui.label(RichText::new(msg).color(p.text3));
                });
                return;
            }
            for &i in &visible {
                if let Some(a) = entry_row(ui, &console.entries[i], i) {
                    action = Some(a);
                }
                if console.entries[i].expanded && !matches!(console.entries[i].outcome, Outcome::Log(..)) {
                    entry_details(ui, &console.entries[i]);
                }
            }
        });
        let pending = console.entries.iter().any(|e| matches!(e.outcome, Outcome::Pending));
        if pending {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }

        match action {
            Some(RowAction::Toggle(i)) => {
                let e = &mut self.console.entries[i];
                e.expanded = !e.expanded;
            }
            Some(RowAction::CopyUrl(i)) => {
                ui.ctx().copy_text(self.console.entries[i].url.clone());
                self.notify("Copied to clipboard");
            }
            Some(RowAction::CopyText(i)) => {
                if let Outcome::Log(_, t) = &self.console.entries[i].outcome {
                    ui.ctx().copy_text(t.clone());
                    self.notify("Copied to clipboard");
                }
            }
            Some(RowAction::CopyBody(i)) => {
                if let Outcome::Done(r) = &self.console.entries[i].outcome {
                    ui.ctx().copy_text(r.body.clone());
                    self.notify("Copied to clipboard");
                }
            }
            None => {}
        }
        if clear {
            self.console.entries.clear();
        }
        if close {
            self.bottom_open = false;
        }
    }
}

/// A script `console.log` line: first line in the row, the rest when expanded.
fn log_row(ui: &mut Ui, e: &ConsoleEntry, level: LogLevel, text: &str, i: usize) -> Option<RowAction> {
    let p = pal(ui);
    let multi = text.contains('\n');
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), if multi { Sense::click() } else { Sense::hover() });
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let cy = rect.center().y;
    if multi {
        let caret = if e.expanded { icons::CARET_DOWN } else { icons::CARET_RIGHT };
        ui.painter().text(pos2(rect.left() + 12.0, cy), Align2::CENTER_CENTER, caret, icon_font(12.0), p.text2);
    }
    let time = e.time.format("%H:%M:%S").to_string();
    let tw = pm::text_width(ui, &time, regular(12.0));
    paint_text(ui, pos2(rect.right() - 8.0 - tw, cy), &time, regular(12.0), p.text3, tw + 2.0);
    let (glyph, color) = match level {
        LogLevel::Error => (icons::X_CIRCLE, p.danger),
        LogLevel::Warn => (icons::WARNING, p.orange),
        LogLevel::Info => (icons::INFO, p.text2),
        LogLevel::Log => (icons::TERMINAL, p.text3),
    };
    ui.painter().text(pos2(rect.left() + 34.0, cy), Align2::CENTER_CENTER, glyph, icon_font(14.0), color);
    // Collapsed: the whole text on one line (pretty-printed objects read fine squeezed).
    let first = if e.expanded { String::new() } else { text.lines().map(str::trim).collect::<Vec<_>>().join(" ") };
    let fg = match level {
        LogLevel::Error => p.danger,
        LogLevel::Warn => p.orange,
        _ => p.text,
    };
    paint_text(ui, pos2(rect.left() + 48.0, cy), &first, mono(12.5), fg, (rect.width() - 72.0 - tw).max(0.0));
    let mut action = None;
    if resp.clicked() {
        action = Some(RowAction::Toggle(i));
    }
    if e.expanded {
        egui::Frame::new().inner_margin(egui::Margin { left: 48, right: 12, top: 0, bottom: 6 }).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).font(mono(12.5)).color(fg)).wrap().selectable(true));
        });
    }
    resp.context_menu(|ui| {
        if ui.button("Copy").clicked() {
            action = Some(RowAction::CopyText(i));
            ui.close();
        }
    });
    action
}

/// One-line summary: `▸ GET http://… · 200 OK · 123 ms · 1.2 KB   12:00:01`.
fn entry_row(ui: &mut Ui, e: &ConsoleEntry, i: usize) -> Option<RowAction> {
    let p = pal(ui);
    if let Outcome::Log(level, text) = &e.outcome {
        return log_row(ui, e, *level, text, i);
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let cy = rect.center().y;
    let caret = if e.expanded { icons::CARET_DOWN } else { icons::CARET_RIGHT };
    ui.painter().text(pos2(rect.left() + 12.0, cy), Align2::CENTER_CENTER, caret, icon_font(12.0), p.text2);

    // Right part: status, duration, size, time.
    let mut right = rect.right() - 8.0;
    let time = e.time.format("%H:%M:%S").to_string();
    let tw = pm::text_width(ui, &time, regular(12.0));
    paint_text(ui, pos2(right - tw, cy), &time, regular(12.0), p.text3, tw + 2.0);
    right -= tw + 16.0;
    let mut parts: Vec<(String, egui::Color32)> = Vec::new();
    match &e.outcome {
        Outcome::Pending | Outcome::Log(..) => parts.push(("pending…".into(), p.text3)),
        Outcome::Failed(err) => parts.push((err.lines().next().unwrap_or("Error").to_string(), p.danger)),
        Outcome::Done(r) => {
            parts.push((human_size(r.size), p.text2));
            parts.push((human_duration(r.duration), p.text2));
            parts.push((format!("{} {}", r.status, r.status_text).trim().to_string(), p.status(r.status).1));
        }
    }
    let max_err_w = (rect.width() * 0.4).max(120.0);
    for (text, color) in parts {
        let w = pm::text_width(ui, &text, regular(12.5)).min(max_err_w);
        paint_text(ui, pos2(right - w, cy), &text, regular(12.5), color, w);
        right -= w + 14.0;
    }

    // Left part: method and URL.
    let mut x = rect.left() + 26.0;
    let (m, f) = pm::method_label_font(e.method, 11.0);
    x += paint_text(ui, pos2(x, cy), &m, f, p.method(e.method), 60.0) + 8.0;
    paint_text(ui, pos2(x, cy), &e.url, mono(12.5), p.text, (right - x - 8.0).max(0.0));

    let mut action = None;
    if resp.clicked() {
        action = Some(RowAction::Toggle(i));
    }
    resp.context_menu(|ui| {
        if ui.button("Copy URL").clicked() {
            action = Some(RowAction::CopyUrl(i));
            ui.close();
        }
        if matches!(e.outcome, Outcome::Done(_)) && ui.button("Copy response body").clicked() {
            action = Some(RowAction::CopyBody(i));
            ui.close();
        }
    });
    action
}

/// Expanded view: network info, request and response headers and bodies.
fn entry_details(ui: &mut Ui, e: &ConsoleEntry) {
    let p = pal(ui);
    egui::Frame::new().inner_margin(egui::Margin { left: 30, right: 12, top: 4, bottom: 10 }).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut network: Vec<(String, String)> = Vec::new();
        if let Outcome::Done(r) = &e.outcome {
            network.push(("HTTP version".into(), r.http_version.clone()));
            if let Some(a) = &r.remote_addr {
                network.push(("Remote address".into(), a.clone()));
            }
            if r.final_url != e.url {
                network.push(("Final URL".into(), r.final_url.clone()));
            }
        }
        if !network.is_empty() {
            section(ui, "Network");
            kv_rows(ui, &network);
        }
        if let Outcome::Failed(err) = &e.outcome {
            section(ui, "Error");
            ui.add(egui::Label::new(RichText::new(err).font(mono(12.5)).color(p.danger)).wrap().selectable(true));
        }
        if !e.req_headers.is_empty() {
            section(ui, "Request Headers");
            kv_rows(ui, &e.req_headers);
        }
        if !e.req_body.is_empty() {
            section(ui, "Request Body");
            body_block(ui, &e.req_body);
        }
        if let Outcome::Done(r) = &e.outcome {
            section(ui, "Response Headers");
            kv_rows(ui, &r.headers);
            if !r.body.is_empty() {
                section(ui, "Response Body");
                body_block(ui, &r.body);
            }
        }
    });
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, p.border));
}

fn section(ui: &mut Ui, title: &str) {
    let p = pal(ui);
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(semibold(12.5)).color(p.text2));
    ui.add_space(2.0);
}

fn kv_rows(ui: &mut Ui, rows: &[(String, String)]) {
    let p = pal(ui);
    for (k, v) in rows {
        let mut job = egui::text::LayoutJob::default();
        job.append(&format!("{k}: "), 0.0, egui::TextFormat::simple(mono(12.5), p.text2));
        job.append(v, 0.0, egui::TextFormat::simple(mono(12.5), p.text));
        ui.add(egui::Label::new(job).wrap().selectable(true));
    }
}

fn body_block(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    egui::Frame::new().fill(p.chrome).corner_radius(4).inner_margin(pm::margin(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt(ui.next_auto_id()).max_height(260.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).font(mono(12.5)).color(p.text)).wrap().selectable(true));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_preview_pretty_prints_and_truncates() {
        assert_eq!(body_preview(br#"{"a":1}"#, "application/json"), "{\n  \"a\": 1\n}");
        assert_eq!(body_preview(&[0xff, 0xfe, 0x00], "image/png"), "<image/png, 3 B>");
        let big = "x".repeat(MAX_BODY + 10);
        let out = body_preview(big.as_bytes(), "text/plain");
        assert!(out.starts_with(&"x".repeat(MAX_BODY)) && out.contains("truncated"));
    }

    #[test]
    fn entries_are_capped_and_finished() {
        let mut c = Console::default();
        let prepared = Prepared {
            method: Method::Get,
            url: "http://x.test".into(),
            headers: vec![],
            body: PreparedBody::None,
            settings: Default::default(),
            digest: None,
        };
        let mut last = Uuid::nil();
        for _ in 0..MAX_ENTRIES + 5 {
            last = c.start(&prepared);
        }
        assert_eq!(c.entries.len(), MAX_ENTRIES);
        c.finish(last, &Err("boom".into()));
        assert_eq!(c.error_count(), 1);
        c.cancel(last); // already finished: stays an error, not "cancelled"
        assert!(matches!(&c.entries.back().unwrap().outcome, Outcome::Failed(e) if e == "boom"));
    }
}
