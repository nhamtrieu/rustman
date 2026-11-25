//! Look & feel modelled on Postman's desktop app (colors measured from screenshots at 1x scale).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle,
    epaint::text::FontTweak,
};

use crate::model::Method;

/// Semantic colors used by the custom-painted UI.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Main content area (request editor, response).
    pub bg: Color32,
    /// Sidebar, header row, tab bar.
    pub chrome: Color32,
    /// Top menu bar (File Edit View Help).
    pub menubar: Color32,
    /// 1px separators and table lines.
    pub border: Color32,
    /// Borders of input boxes (filter, search).
    pub input_border: Color32,
    /// Border of the URL box.
    pub url_border: Color32,
    /// Hovered row / button background.
    pub hover: Color32,
    /// Selected pill (request section tab, response tab).
    pub pill: Color32,
    /// Active request tab / selected sidebar icon.
    pub active: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub orange: Color32,
    pub green_dot: Color32,
    pub focus: Color32,
    /// Sidebar row hover / selected background.
    pub row_hover: Color32,
    /// Keyboard-focus ring around the clicked sidebar row.
    pub focus_ring: Color32,
    pub indent_guide: Color32,
    pub danger: Color32,
    // JSON tokens
    pub json_key: Color32,
    pub json_string: Color32,
    pub json_number: Color32,
    pub json_literal: Color32,
    pub json_punct: Color32,
    pub line_number: Color32,
    // Code snippet tokens
    pub code_keyword: Color32,
    pub code_string: Color32,
    pub code_number: Color32,
    pub code_comment: Color32,
    pub code_function: Color32,
    pub code_type: Color32,
    pub code_property: Color32,
    pub code_variable: Color32,
    pub code_punct: Color32,
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x21, 0x21, 0x21),
    chrome: Color32::from_rgb(0x26, 0x26, 0x26),
    menubar: Color32::from_rgb(0x2c, 0x2c, 0x2c),
    border: Color32::from_rgb(0x30, 0x30, 0x30),
    input_border: Color32::from_rgb(0x3b, 0x3b, 0x3b),
    url_border: Color32::from_rgb(0x4a, 0x4a, 0x4a),
    hover: Color32::from_rgb(0x2b, 0x2b, 0x2b),
    pill: Color32::from_rgb(0x2b, 0x2b, 0x2b),
    active: Color32::from_rgb(0x3b, 0x3b, 0x3b),
    text: Color32::from_rgb(0xff, 0xff, 0xff),
    text2: Color32::from_rgb(0xa6, 0xa6, 0xa6),
    text3: Color32::from_rgb(0x6b, 0x6b, 0x6b),
    accent: Color32::from_rgb(0x2f, 0x67, 0xcc),
    accent_hover: Color32::from_rgb(0x3b, 0x75, 0xdd),
    orange: Color32::from_rgb(0xff, 0x6c, 0x37),
    green_dot: Color32::from_rgb(0x3f, 0xb5, 0x6e),
    focus: Color32::from_rgb(0x8f, 0xb4, 0xf5),
    row_hover: Color32::from_rgb(0x30, 0x30, 0x30),
    focus_ring: Color32::from_rgb(0x9f, 0xc0, 0xfa),
    indent_guide: Color32::from_rgb(0x40, 0x40, 0x40),
    danger: Color32::from_rgb(0xf7, 0x9a, 0x8e),
    json_key: Color32::from_rgb(0x9c, 0xdc, 0xfe),
    json_string: Color32::from_rgb(0xce, 0x91, 0x78),
    json_number: Color32::from_rgb(0xb5, 0xce, 0xa8),
    json_literal: Color32::from_rgb(0xce, 0x91, 0x78),
    json_punct: Color32::from_rgb(0xd4, 0xd4, 0xd4),
    line_number: Color32::from_rgb(0x85, 0x85, 0x85),
    // VS Code Dark+ tones, matching the JSON viewer.
    code_keyword: Color32::from_rgb(0x56, 0x9c, 0xd6),
    code_string: Color32::from_rgb(0xce, 0x91, 0x78),
    code_number: Color32::from_rgb(0xb5, 0xce, 0xa8),
    code_comment: Color32::from_rgb(0x6a, 0x99, 0x55),
    code_function: Color32::from_rgb(0xdc, 0xdc, 0xaa),
    code_type: Color32::from_rgb(0x4e, 0xc9, 0xb0),
    code_property: Color32::from_rgb(0x9c, 0xdc, 0xfe),
    code_variable: Color32::from_rgb(0xc5, 0x86, 0xc0),
    code_punct: Color32::from_rgb(0xd4, 0xd4, 0xd4),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xff, 0xff, 0xff),
    chrome: Color32::from_rgb(0xf9, 0xf9, 0xf9),
    menubar: Color32::from_rgb(0xf2, 0xf2, 0xf2),
    border: Color32::from_rgb(0xe6, 0xe6, 0xe6),
    input_border: Color32::from_rgb(0xd9, 0xd9, 0xd9),
    url_border: Color32::from_rgb(0xc8, 0xc8, 0xc8),
    hover: Color32::from_rgb(0xf0, 0xf0, 0xf0),
    pill: Color32::from_rgb(0xed, 0xed, 0xed),
    active: Color32::from_rgb(0xe6, 0xe6, 0xe6),
    text: Color32::from_rgb(0x21, 0x21, 0x21),
    text2: Color32::from_rgb(0x6b, 0x6b, 0x6b),
    text3: Color32::from_rgb(0xa6, 0xa6, 0xa6),
    accent: Color32::from_rgb(0x09, 0x69, 0xda),
    accent_hover: Color32::from_rgb(0x07, 0x5a, 0xbd),
    orange: Color32::from_rgb(0xff, 0x6c, 0x37),
    green_dot: Color32::from_rgb(0x00, 0x7f, 0x31),
    focus: Color32::from_rgb(0x09, 0x69, 0xda),
    row_hover: Color32::from_rgb(0xed, 0xed, 0xed),
    focus_ring: Color32::from_rgb(0x09, 0x69, 0xda),
    indent_guide: Color32::from_rgb(0xe0, 0xe0, 0xe0),
    danger: Color32::from_rgb(0x8e, 0x1a, 0x10),
    json_key: Color32::from_rgb(0x04, 0x51, 0xa5),
    json_string: Color32::from_rgb(0xa3, 0x15, 0x15),
    json_number: Color32::from_rgb(0x09, 0x86, 0x58),
    json_literal: Color32::from_rgb(0x00, 0x00, 0xff),
    json_punct: Color32::from_rgb(0x3b, 0x3b, 0x3b),
    line_number: Color32::from_rgb(0x99, 0x99, 0x99),
    // GitHub light tones.
    code_keyword: Color32::from_rgb(0xcf, 0x22, 0x2e),
    code_string: Color32::from_rgb(0x0a, 0x30, 0x69),
    code_number: Color32::from_rgb(0x05, 0x50, 0xae),
    code_comment: Color32::from_rgb(0x6e, 0x77, 0x81),
    code_function: Color32::from_rgb(0x82, 0x50, 0xdf),
    code_type: Color32::from_rgb(0x95, 0x38, 0x00),
    code_property: Color32::from_rgb(0x05, 0x50, 0xae),
    code_variable: Color32::from_rgb(0x95, 0x38, 0x00),
    code_punct: Color32::from_rgb(0x57, 0x60, 0x6a),
};

pub fn pal(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode { &DARK } else { &LIGHT }
}

pub fn pal_ctx(ctx: &egui::Context) -> &'static Palette {
    if ctx.global_style().visuals.dark_mode { &DARK } else { &LIGHT }
}

impl Palette {
    pub fn method(&self, m: Method) -> Color32 {
        let (d, l) = match m {
            Method::Get | Method::Head => ((0x9d, 0xde, 0xb9), (0x00, 0x7f, 0x31)),
            Method::Post => ((0xff, 0xe0, 0x83), (0xad, 0x7a, 0x03)),
            Method::Put => ((0x74, 0xae, 0xf6), (0x00, 0x53, 0xb8)),
            Method::Patch => ((0xc0, 0xa8, 0xe1), (0x62, 0x34, 0x97)),
            Method::Delete => ((0xf7, 0x9a, 0x8e), (0x8e, 0x1a, 0x10)),
            Method::Options => ((0xf2, 0x8d, 0xc7), (0xa6, 0x14, 0x68)),
        };
        let (r, g, b) = if self.dark { d } else { l };
        Color32::from_rgb(r, g, b)
    }

    /// (background, text) of the response status pill.
    pub fn status(&self, code: u16) -> (Color32, Color32) {
        let dark = self.dark;
        match code {
            200..=299 if dark => (Color32::from_rgb(0x16, 0x46, 0x2e), Color32::from_rgb(0x9d, 0xde, 0xb9)),
            200..=299 => (Color32::from_rgb(0xe5, 0xf5, 0xec), Color32::from_rgb(0x00, 0x7f, 0x31)),
            300..=399 if dark => (Color32::from_rgb(0x1a, 0x35, 0x5c), Color32::from_rgb(0x9e, 0xc4, 0xf7)),
            300..=399 => (Color32::from_rgb(0xe6, 0xef, 0xfc), Color32::from_rgb(0x00, 0x53, 0xb8)),
            _ if dark => (Color32::from_rgb(0x52, 0x21, 0x1c), Color32::from_rgb(0xf7, 0x9a, 0x8e)),
            _ => (Color32::from_rgb(0xfd, 0xec, 0xea), Color32::from_rgb(0x8e, 0x1a, 0x10)),
        }
    }
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

pub const UI_SIZE: f32 = 14.0;
pub const MONO_SIZE: f32 = 14.0;

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, family("medium"))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, family("semibold"))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, family("icons"))
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: &'static [u8], tweak: FontTweak| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes).tweak(tweak)));
    };
    add(&mut fonts, "Inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"), FontTweak::default());
    add(&mut fonts, "Inter-Medium", include_bytes!("../assets/fonts/Inter-Medium.ttf"), FontTweak::default());
    add(&mut fonts, "Inter-SemiBold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"), FontTweak::default());
    add(&mut fonts, "PlexMono", include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"), FontTweak::default());
    // Phosphor glyphs are centered in their em box. Inline in Inter text they look a bit high,
    // so the copy used as a text fallback is nudged down; standalone icons (painted centered in
    // buttons) use the untouched copy so they sit exactly in the middle.
    const PHOSPHOR: &[u8] = include_bytes!("../assets/fonts/Phosphor.ttf");
    add(&mut fonts, "Phosphor", PHOSPHOR, FontTweak { scale: 1.1, y_offset_factor: 0.08, ..Default::default() });
    add(&mut fonts, "PhosphorIcon", PHOSPHOR, FontTweak::default());

    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with = |primary: &str| {
        let mut v = vec![primary.to_owned(), "Phosphor".to_owned()];
        v.extend(defaults.iter().cloned());
        v
    };
    fonts.families.insert(FontFamily::Proportional, with("Inter"));
    fonts.families.insert(family("medium"), with("Inter-Medium"));
    fonts.families.insert(family("semibold"), with("Inter-SemiBold"));
    fonts.families.insert(family("icons"), vec!["PhosphorIcon".to_owned()]);
    let mono_defaults = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut mono_list = vec!["PlexMono".to_owned()];
    mono_list.extend(mono_defaults);
    mono_list.push("Phosphor".to_owned());
    fonts.families.insert(FontFamily::Monospace, mono_list);
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// egui style (for the stock widgets we still use: menus, text edits, combo boxes...)
// ---------------------------------------------------------------------------

pub fn apply_style(ctx: &egui::Context) {
    for (theme, p) in [(egui::Theme::Dark, &DARK), (egui::Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.text_styles = [
                (TextStyle::Small, regular(12.0)),
                (TextStyle::Body, regular(UI_SIZE)),
                (TextStyle::Button, regular(UI_SIZE)),
                (TextStyle::Heading, semibold(18.0)),
                (TextStyle::Monospace, mono(MONO_SIZE)),
            ]
            .into();
            let s = &mut style.spacing;
            s.item_spacing = egui::vec2(8.0, 6.0);
            s.button_padding = egui::vec2(10.0, 5.0);
            s.interact_size = egui::vec2(40.0, 28.0);
            s.icon_width = 16.0;
            s.icon_width_inner = 10.0;
            s.menu_margin = Margin::same(6);
            s.window_margin = Margin::same(14);
            s.combo_width = 120.0;

            let v = &mut style.visuals;
            v.dark_mode = p.dark;
            v.override_text_color = None;
            v.panel_fill = p.bg;
            v.window_fill = p.chrome;
            v.window_stroke = Stroke::new(1.0, p.input_border);
            v.window_corner_radius = CornerRadius::same(8);
            v.menu_corner_radius = CornerRadius::same(8);
            v.window_shadow = Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }) };
            v.popup_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 100 } else { 30 }) };
            v.faint_bg_color = p.chrome;
            v.extreme_bg_color = p.bg;
            v.text_edit_bg_color = Some(p.bg);
            v.code_bg_color = p.chrome;
            v.hyperlink_color = p.accent;
            v.selection.bg_fill = p.accent.gamma_multiply(0.55);
            v.selection.stroke = Stroke::new(1.0, p.text);
            v.text_cursor.stroke = Stroke::new(1.5, p.text);
            v.striped = false;
            v.collapsing_header_frame = false;
            v.indent_has_left_vline = false;
            v.weak_text_color = Some(p.text3);

            let r = CornerRadius::same(4);
            let w = &mut v.widgets;
            w.noninteractive.bg_fill = p.chrome;
            w.noninteractive.weak_bg_fill = p.chrome;
            w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
            w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
            w.noninteractive.corner_radius = r;

            w.inactive.bg_fill = p.hover;
            w.inactive.weak_bg_fill = p.hover;
            w.inactive.bg_stroke = Stroke::new(1.0, p.input_border);
            w.inactive.fg_stroke = Stroke::new(1.0, p.text);
            w.inactive.corner_radius = r;
            w.inactive.expansion = 0.0;

            w.hovered.bg_fill = p.active;
            w.hovered.weak_bg_fill = p.active;
            w.hovered.bg_stroke = Stroke::new(1.0, p.url_border);
            w.hovered.fg_stroke = Stroke::new(1.0, p.text);
            w.hovered.corner_radius = r;
            w.hovered.expansion = 0.0;

            w.active.bg_fill = p.active;
            w.active.weak_bg_fill = p.active;
            w.active.bg_stroke = Stroke::new(1.0, p.focus);
            w.active.fg_stroke = Stroke::new(1.0, p.text);
            w.active.corner_radius = r;
            w.active.expansion = 0.0;

            w.open = w.active;
        });
    }
}
