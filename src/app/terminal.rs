//! Integrated terminal: the user's shell in a PTY, decoded by `vt100` and painted as a character grid.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Event, Id, Key, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::*;
use crate::icons;
use crate::pm;
use crate::theme::{Palette, mono, pal, regular};

const FONT_SIZE: f32 = 13.0;
const SCROLLBACK: usize = 5000;

pub(super) struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    size: (u16, u16),
    pub shell: String,
    /// Fractional mouse-wheel lines not applied yet.
    scroll_acc: f32,
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n.max(1)).unwrap_or(NonZeroU16::MIN)
}

impl Terminal {
    pub fn spawn(ctx: &egui::Context, rows: u16, cols: u16) -> Result<Self, String> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| format!("Cannot open a PTY: {e}"))?;
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Rustman");
        if let Some(home) = dirs::home_dir() {
            cmd.cwd(home);
        }
        let shell = std::env::var("SHELL").ok().and_then(|s| s.rsplit('/').next().map(str::to_string)).unwrap_or_else(|| if cfg!(windows) { "powershell".into() } else { "sh".into() });
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("Cannot start the shell: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK)));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let (parser, exited, ctx) = (parser.clone(), exited.clone(), ctx.clone());
            std::thread::Builder::new()
                .name("terminal-reader".into())
                .spawn(move || {
                    let mut buf = [0u8; 16 * 1024];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if let Ok(mut p) = parser.lock() {
                                    p.process(&buf[..n]);
                                }
                                ctx.request_repaint();
                            }
                        }
                    }
                    exited.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(Self { parser, writer, master: pair.master, child, exited, size: (rows, cols), shell, scroll_acc: 0.0 })
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_scrollback(0);
        }
    }

    #[cfg(debug_assertions)]
    pub fn debug_write(&mut self, bytes: &[u8]) {
        self.write(bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_size(nz(rows), nz(cols));
        }
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }

    pub fn contents(&self) -> String {
        self.parser.lock().map(|p| p.screen().contents()).unwrap_or_default()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl RustmanApp {
    pub(super) fn terminal_panel(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let ctx = ui.ctx().clone();
        let (mut restart, mut copy, mut close) = (false, false, false);
        ui.horizontal(|ui| {
            ui.set_height(34.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            self.bottom_tabs(ui);
            if let Some(t) = &self.terminal {
                let status = if t.exited() { format!("{} (exited)", t.shell) } else { t.shell.clone() };
                ui.label(egui::RichText::new(status).font(regular(12.5)).color(p.text3));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if pm::icon_button(ui, icons::X, 14.0, "Close panel").clicked() {
                    close = true;
                }
                if pm::icon_button(ui, icons::ARROW_COUNTER_CLOCKWISE, 14.0, "Restart shell").clicked() {
                    restart = true;
                }
                if pm::icon_button(ui, icons::COPY, 14.0, "Copy screen (Ctrl+Shift+C)").clicked() {
                    copy = true;
                }
            });
        });
        pm::hline(ui);

        let font = mono(FONT_SIZE);
        let cell = {
            let g = ui.painter().layout_no_wrap("M".repeat(20), font.clone(), Color32::PLACEHOLDER);
            vec2(g.size().x / 20.0, g.size().y)
        };
        let avail = ui.available_rect_before_wrap().shrink2(vec2(6.0, 4.0));
        let rows = ((avail.height() / cell.y).floor() as u16).max(2);
        let cols = ((avail.width() / cell.x).floor() as u16).max(10);

        if restart || self.terminal.is_none() {
            self.terminal = None;
            match Terminal::spawn(&ctx, rows, cols) {
                Ok(t) => self.terminal = Some(t),
                Err(e) => {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(e).color(p.danger));
                    return;
                }
            }
        }
        let term_id = Id::unique("terminal_area");
        let Some(term) = &mut self.terminal else { return };
        term.resize(rows, cols);

        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        if resp.clicked() || restart {
            ui.memory_mut(|m| m.request_focus(term_id));
        }
        let focused = ui.memory(|m| m.has_focus(term_id));
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(term_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        }
        self.terminal_focused = focused;
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }

        // Input.
        let (app_cursor, bracketed) = term.parser.lock().map(|p| (p.screen().application_cursor(), p.screen().bracketed_paste())).unwrap_or((false, false));
        if focused {
            let (events, shift) = ui.input(|i| (i.events.clone(), i.modifiers.shift));
            let mut out: Vec<u8> = Vec::new();
            for ev in events {
                match ev {
                    Event::Text(t) => out.extend_from_slice(t.as_bytes()),
                    Event::Paste(t) => {
                        let t = t.replace("\r\n", "\r").replace('\n', "\r");
                        if bracketed {
                            out.extend_from_slice(b"\x1b[200~");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"\x1b[201~");
                        } else {
                            out.extend_from_slice(t.as_bytes());
                        }
                    }
                    Event::Copy if shift => copy = true,
                    Event::Copy => out.push(0x03),
                    Event::Cut => out.push(0x18),
                    Event::Key { key, pressed: true, modifiers, .. } => {
                        if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                            out.extend_from_slice(&bytes);
                        }
                    }
                    _ => {}
                }
            }
            if !out.is_empty() {
                if term.exited() && out.contains(&b'\r') {
                    self.terminal = None;
                    ui.ctx().request_repaint();
                    return;
                }
                term.write(&out);
            }
        }

        // Mouse wheel scrolls the scrollback.
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                term.scroll_acc += dy / cell.y;
                let lines = term.scroll_acc.trunc() as i64;
                if lines != 0 {
                    term.scroll_acc -= lines as f32;
                    if let Ok(mut pr) = term.parser.lock() {
                        let cur = pr.screen().scrollback() as i64;
                        pr.screen_mut().set_scrollback((cur + lines).max(0) as usize);
                    }
                }
            }
        }

        // Paint.
        let painter = ui.painter_at(rect);
        let origin = rect.min + vec2(6.0, 4.0);
        if let Ok(pr) = term.parser.lock() {
            let screen = pr.screen();
            for row in 0..rows {
                let mut job = egui::text::LayoutJob::default();
                let mut run = String::new();
                let mut fmt: Option<egui::TextFormat> = None;
                for col in 0..cols {
                    let Some(c) = screen.cell(row, col) else { break };
                    if c.is_wide_continuation() {
                        continue;
                    }
                    let text = if c.has_contents() { c.contents() } else { " " };
                    let (mut fg, mut bg) = (term_color(c.fgcolor(), p, true, c.bold()), term_color(c.bgcolor(), p, false, false));
                    if c.inverse() {
                        std::mem::swap(&mut fg, &mut bg);
                        if fg == Color32::TRANSPARENT {
                            fg = p.bg;
                        }
                        if bg == Color32::TRANSPARENT {
                            bg = p.text;
                        }
                    }
                    if c.dim() {
                        fg = fg.gamma_multiply(0.6);
                    }
                    let f = egui::TextFormat {
                        font_id: font.clone(),
                        color: fg,
                        background: bg,
                        italics: c.italic(),
                        underline: if c.underline() { Stroke::new(1.0, fg) } else { Stroke::NONE },
                        ..Default::default()
                    };
                    if fmt.as_ref() != Some(&f) {
                        if let Some(prev) = fmt.take() {
                            job.append(&run, 0.0, prev);
                            run.clear();
                        }
                        fmt = Some(f);
                    }
                    run.push_str(text);
                }
                if let Some(prev) = fmt {
                    job.append(&run, 0.0, prev);
                }
                let galley = painter.layout_job(job);
                painter.galley(origin + vec2(0.0, row as f32 * cell.y), galley, p.text);
            }
            if screen.scrollback() == 0 && !screen.hide_cursor() && !term.exited() {
                let (cr, cc) = screen.cursor_position();
                let r = Rect::from_min_size(origin + vec2(cc as f32 * cell.x, cr as f32 * cell.y), cell);
                if focused {
                    painter.rect_filled(r, 1.0, p.text.gamma_multiply(0.55));
                } else {
                    painter.rect_stroke(r, 1.0, Stroke::new(1.0, p.text3), StrokeKind::Inside);
                }
            }
            if screen.scrollback() > 0 {
                let label = format!("↑ {} lines", screen.scrollback());
                pm::paint_text(ui, pos2(rect.right() - 110.0, rect.top() + 12.0), &label, regular(12.0), p.text3, 100.0);
            }
        }
        if term.exited() {
            pm::paint_text(ui, pos2(rect.left() + 8.0, rect.bottom() - 12.0), "Process exited. Press Enter or click ↺ to start a new shell.", regular(12.5), p.orange, rect.width());
        }
        if !focused && !term.exited() {
            pm::paint_text(ui, pos2(rect.right() - 170.0, rect.bottom() - 12.0), "Click to type", regular(12.0), p.text3, 160.0);
        }

        if copy {
            ui.ctx().copy_text(term.contents());
            self.notify("Copied terminal screen");
        }
        if close {
            self.bottom_open = false;
        }
    }
}

/// Bytes a key press sends to the shell (xterm conventions).
fn key_bytes(key: Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let csi = |s: &str| Some(format!("\x1b[{s}").into_bytes());
    let arrow = |c: char| {
        if m.ctrl {
            Some(format!("\x1b[1;5{c}").into_bytes())
        } else if app_cursor {
            Some(format!("\x1bO{c}").into_bytes())
        } else {
            Some(format!("\x1b[{c}").into_bytes())
        }
    };
    // Ctrl+letter -> control character (Ctrl+C/X/V arrive as Copy/Cut/Paste events instead).
    if m.ctrl && !m.alt {
        let name = key.name();
        if name.len() == 1 {
            let ch = name.as_bytes()[0];
            if ch.is_ascii_uppercase() && !matches!(ch, b'C' | b'X' | b'V') {
                return Some(vec![ch - b'A' + 1]);
            }
        }
        match key {
            Key::OpenBracket => return Some(vec![0x1b]),
            Key::Backslash => return Some(vec![0x1c]),
            Key::CloseBracket => return Some(vec![0x1d]),
            Key::Space => return Some(vec![0x00]),
            _ => {}
        }
    }
    match key {
        Key::Enter => Some(b"\r".to_vec()),
        Key::Backspace => Some(if m.ctrl { vec![0x17] } else { vec![0x7f] }),
        Key::Tab => if m.shift { csi("Z") } else { Some(b"\t".to_vec()) },
        Key::Escape => Some(vec![0x1b]),
        Key::ArrowUp => arrow('A'),
        Key::ArrowDown => arrow('B'),
        Key::ArrowRight => arrow('C'),
        Key::ArrowLeft => arrow('D'),
        Key::Home => csi("H"),
        Key::End => csi("F"),
        Key::PageUp => csi("5~"),
        Key::PageDown => csi("6~"),
        Key::Insert => csi("2~"),
        Key::Delete => csi("3~"),
        Key::F1 => Some(b"\x1bOP".to_vec()),
        Key::F2 => Some(b"\x1bOQ".to_vec()),
        Key::F3 => Some(b"\x1bOR".to_vec()),
        Key::F4 => Some(b"\x1bOS".to_vec()),
        Key::F5 => csi("15~"),
        Key::F6 => csi("17~"),
        Key::F7 => csi("18~"),
        Key::F8 => csi("19~"),
        Key::F9 => csi("20~"),
        Key::F10 => csi("21~"),
        Key::F11 => csi("23~"),
        Key::F12 => csi("24~"),
        _ => None,
    }
}

/// vt100 color -> egui color. Default background is transparent (the panel shows through).
fn term_color(c: vt100::Color, p: &Palette, fg: bool, bold: bool) -> Color32 {
    match c {
        vt100::Color::Default => if fg { p.text } else { Color32::TRANSPARENT },
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(i) => {
            let i = if fg && bold && i < 8 { i + 8 } else { i };
            ansi_color(i, p.dark)
        }
    }
}

fn ansi_color(i: u8, dark: bool) -> Color32 {
    const DARK: [(u8, u8, u8); 16] = [
        (0x1e, 0x1e, 0x1e), (0xf2, 0x6d, 0x6d), (0x7e, 0xc6, 0x99), (0xe5, 0xc0, 0x7b),
        (0x6c, 0xa8, 0xf7), (0xc6, 0x92, 0xe8), (0x5f, 0xc8, 0xd6), (0xd4, 0xd4, 0xd4),
        (0x80, 0x80, 0x80), (0xff, 0x8a, 0x8a), (0x9d, 0xde, 0xb9), (0xff, 0xe0, 0x83),
        (0x8c, 0xbd, 0xff), (0xdb, 0xb0, 0xff), (0x86, 0xe1, 0xec), (0xff, 0xff, 0xff),
    ];
    const LIGHT: [(u8, u8, u8); 16] = [
        (0x21, 0x21, 0x21), (0xc4, 0x1a, 0x16), (0x00, 0x7f, 0x31), (0x9a, 0x6a, 0x00),
        (0x00, 0x53, 0xb8), (0x8a, 0x3f, 0xb8), (0x00, 0x7f, 0x8c), (0x6b, 0x6b, 0x6b),
        (0x8a, 0x8a, 0x8a), (0xe0, 0x3e, 0x3a), (0x1a, 0x9e, 0x50), (0xb8, 0x86, 0x00),
        (0x25, 0x75, 0xe0), (0xa8, 0x5c, 0xd6), (0x0d, 0x9b, 0xaa), (0x3a, 0x3a, 0x3a),
    ];
    match i {
        0..=15 => {
            let (r, g, b) = if dark { DARK[i as usize] } else { LIGHT[i as usize] };
            Color32::from_rgb(r, g, b)
        }
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_colors() {
        let none = egui::Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(Key::D, egui::Modifiers::CTRL, false), Some(vec![4]));
        assert_eq!(key_bytes(Key::C, egui::Modifiers::CTRL, false), None);
        assert_eq!(key_bytes(Key::A, none, false), None, "plain letters arrive as Text events");
        assert_eq!(ansi_color(196, true), Color32::from_rgb(255, 0, 0));
        assert_eq!(ansi_color(232, true), Color32::from_rgb(8, 8, 8));
    }

    /// Spawns a real shell in a PTY, runs a command and reads the decoded screen.
    #[cfg(unix)]
    #[test]
    fn runs_a_shell_command() {
        let ctx = egui::Context::default();
        let mut t = Terminal::spawn(&ctx, 24, 80).expect("spawn shell");
        t.write(b"echo rustman-$((40+2))\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !t.contents().contains("rustman-42") {
            assert!(std::time::Instant::now() < deadline, "screen: {}", t.contents());
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        t.write(b"exit\r");
        while !t.exited() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
