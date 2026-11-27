//! Request editor: breadcrumb, URL bar and the Params / Auth / Headers / Body / Scripts / Settings sections.

use eframe::egui::{self, Align2, Color32, Id, Rect, RichText, Sense, Stroke, StrokeKind, TextBuffer, Ui, pos2, vec2};

use super::*;
use crate::icons;
use crate::pm::{self, TableOpts, paint_text};
use crate::theme::{Palette, icon_font, medium, mono, pal, regular, semibold, MONO_SIZE, UI_SIZE};

impl RustmanApp {
    pub(super) fn request_header(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let i = self.active;
        let coll_name = self.tabs[i].collection.and_then(|c| self.ws.collection(c)).map(|c| c.name.clone());
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        let cy = rect.center().y;

        // "HTTP" badge.
        let teal = Color32::from_rgb(0x4f, 0xc1, 0xcf);
        paint_text(ui, pos2(rect.left(), cy - 4.0), "HTTP", semibold(8.0), teal, 30.0);
        let ay = cy + 5.0;
        let s = Stroke::new(1.2, teal);
        ui.painter().hline(rect.left() + 1.0..=rect.left() + 19.0, ay, s);
        ui.painter().line_segment([pos2(rect.left() + 15.0, ay - 3.0), pos2(rect.left() + 19.0, ay)], s);

        let mut x = rect.left() + 32.0;
        if let Some(c) = &coll_name {
            x += paint_text(ui, pos2(x, cy), c, regular(UI_SIZE), p.text2, 260.0) + 8.0;
            ui.painter().text(pos2(x + 2.0, cy), Align2::CENTER_CENTER, icons::CARET_RIGHT, icon_font(12.0), p.text3);
            x += 14.0;
        }
        let renaming = matches!(self.rename, Some((RenameTarget::TabName, _)));
        let name = self.tabs[i].request.name.clone();
        let name_w = pm::text_width(ui, &name, semibold(UI_SIZE));
        let name_rect = Rect::from_min_max(pos2(x - 6.0, cy - 14.0), pos2((x + name_w + 6.0).min(rect.right() - 200.0), cy + 14.0));
        if renaming {
            let edit_rect = Rect::from_min_max(name_rect.min, pos2((name_rect.left() + 360.0).min(rect.right() - 200.0), name_rect.bottom()));
            ui.painter().rect(edit_rect, 4.0, p.bg, Stroke::new(1.0, p.focus), StrokeKind::Inside);
            let mut commit = false;
            let mut cancel = false;
            if let Some((_, text)) = &mut self.rename {
                let resp = ui.put(
                    edit_rect.shrink2(vec2(6.0, 0.0)),
                    egui::TextEdit::singleline(text).id(Id::unique("tab_rename")).frame(egui::Frame::NONE).font(semibold(UI_SIZE)).align(egui::Align2::LEFT_CENTER),
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
            let resp = ui.interact(name_rect, Id::unique("tab_name"), Sense::click());
            if resp.hovered() {
                ui.painter().rect_stroke(name_rect, 4.0, Stroke::new(1.0, p.input_border), StrokeKind::Inside);
            }
            paint_text(ui, pos2(x, cy), &name, semibold(UI_SIZE), p.text, name_rect.width());
            if resp.on_hover_text("Rename").clicked() {
                self.start_rename(RenameTarget::TabName, name);
            }
        }

        // Save split button.
        let group = Rect::from_min_max(pos2(rect.right() - 112.0, cy - 15.0), pos2(rect.right(), cy + 15.0));
        let main = Rect::from_min_max(group.min, pos2(group.right() - 30.0, group.bottom()));
        let caret = Rect::from_min_max(pos2(group.right() - 30.0, group.top()), group.max);
        let main_r = ui.interact(main, Id::unique("save_btn"), Sense::click());
        let caret_r = ui.interact(caret, Id::unique("save_caret"), Sense::click());
        for (r, resp) in [(main, &main_r), (caret, &caret_r)] {
            if resp.hovered() {
                ui.painter().rect_filled(r, 4.0, p.hover);
            }
        }
        let color = if main_r.hovered() { p.text } else { p.text2 };
        ui.painter().text(pos2(main.left() + 16.0, cy), Align2::CENTER_CENTER, icons::FLOPPY_DISK, icon_font(16.0), color);
        paint_text(ui, pos2(main.left() + 30.0, cy), "Save", regular(UI_SIZE), color, 60.0);
        ui.painter().text(caret.center(), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
        if main_r.on_hover_text("Save (Ctrl+S)").clicked() {
            self.save_tab(i);
        }
        egui::Popup::menu(&caret_r).show(|ui| {
            if ui.button("Save As…").clicked() {
                self.open_save_as(i);
                ui.close();
            }
        });
    }

    pub(super) fn url_bar(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let i = self.active;
        let vars = self.vars_for(self.tabs[i].collection);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let send_w = 128.0;
        let url_box = Rect::from_min_max(rect.min, pos2(rect.right() - send_w - 8.0, rect.bottom()));
        let send_rect = Rect::from_min_max(pos2(rect.right() - send_w, rect.top()), rect.max);
        let url_id = Id::unique(("url", self.tabs[i].request.id));
        let focused = ui.memory(|m| m.has_focus(url_id));
        let hovered = ui.rect_contains_pointer(url_box);
        let border = if focused || hovered { Color32::from_rgb(0x6b, 0x6b, 0x6b) } else { p.url_border };
        let border = if p.dark { border } else if focused || hovered { p.text3 } else { p.url_border };
        ui.painter().rect(url_box, 6.0, p.bg, Stroke::new(1.0, border), StrokeKind::Inside);

        // Method selector.
        let method_rect = Rect::from_min_max(url_box.min, pos2(url_box.left() + 130.0, url_box.bottom()));
        let mr = ui.interact(method_rect.shrink(1.0), Id::unique("method_btn"), Sense::click());
        if mr.hovered() {
            ui.painter().rect_filled(method_rect.shrink(1.0), 5.0, p.hover);
        }
        let tab = &mut self.tabs[i];
        let m = tab.request.method;
        paint_text(ui, pos2(method_rect.left() + 16.0, method_rect.center().y), m.as_str(), semibold(15.0), p.method(m), 90.0);
        ui.painter().text(pos2(method_rect.right() - 18.0, method_rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
        ui.painter().vline(method_rect.right(), url_box.top() + 1.0..=url_box.bottom() - 1.0, Stroke::new(1.0, p.input_border));
        egui::Popup::menu(&mr).width(140.0).show(|ui| {
            for mm in Method::ALL {
                let text = RichText::new(mm.as_str()).font(semibold(13.0)).color(p.method(mm));
                if ui.selectable_label(tab.request.method == mm, text).clicked() {
                    tab.request.method = mm;
                    ui.close();
                }
            }
        });

        // URL text.
        let text_rect = Rect::from_min_max(pos2(method_rect.right() + 12.0, url_box.top() + 1.0), pos2(url_box.right() - 10.0, url_box.bottom() - 1.0));
        let r = pm::var_edit(ui, text_rect, url_id, &mut tab.request.url, &vars, "Enter URL or paste text", regular(15.0));
        if r.changed() {
            if tab.request.url.trim_start().starts_with("curl ") {
                if let Ok(mut parsed) = import::parse_curl(&tab.request.url) {
                    parsed.id = tab.request.id;
                    parsed.name = tab.request.name.clone();
                    tab.request = parsed;
                }
            } else {
                tab.request.sync_params_from_url();
            }
        }
        let mut send = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

        // Send split button.
        let loading = matches!(tab.state, TabState::Loading { .. });
        let main = Rect::from_min_max(send_rect.min, pos2(send_rect.right() - 36.0, send_rect.bottom()));
        let caret = Rect::from_min_max(pos2(send_rect.right() - 36.0, send_rect.top()), send_rect.max);
        let main_r = ui.interact(main, Id::unique("send_btn"), Sense::click());
        let caret_r = ui.interact(caret, Id::unique("send_caret"), Sense::click());
        let base = if loading { p.active } else { p.accent };
        let hover = if loading { p.active.gamma_multiply(1.2) } else { p.accent_hover };
        ui.painter().rect_filled(send_rect, 6.0, base);
        if main_r.hovered() {
            ui.painter().rect_filled(main, egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, hover);
        }
        if caret_r.hovered() {
            ui.painter().rect_filled(caret, egui::CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 }, hover);
        }
        let divider = if p.dark { Color32::from_rgb(0x24, 0x4f, 0x9e) } else { Color32::from_rgb(0x05, 0x4c, 0xa0) };
        ui.painter().vline(caret.left(), send_rect.top()..=send_rect.bottom(), Stroke::new(1.0, if loading { p.border } else { divider }));
        ui.painter().text(main.center(), Align2::CENTER_CENTER, if loading { "Cancel" } else { "Send" }, semibold(15.0), Color32::WHITE);
        ui.painter().text(caret.center(), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(13.0), Color32::WHITE);
        let mut cancel = false;
        if main_r.on_hover_text("Ctrl+Enter").clicked() {
            if loading {
                cancel = true;
            } else {
                send = true;
            }
        }
        let mut send_download = false;
        egui::Popup::menu(&caret_r).show(|ui| {
            if ui.button("Send and Download").clicked() {
                send_download = true;
                ui.close();
            }
        });
        if send_download {
            self.notify("The response can be saved with \"Save Response\" once it arrives");
            send = true;
        }
        if send {
            let ctx = ui.ctx().clone();
            self.send(i, &ctx);
        }
        if cancel {
            self.cancel(i);
        }
    }

    pub(super) fn request_sections(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let i = self.active;
        let vars = self.vars_for(self.tabs[i].collection);
        let inherited = self.inherited_auth(self.tabs[i].collection, self.tabs[i].request.id);
        let auto_headers: Vec<(String, String)> = {
            let tab = &self.tabs[i];
            http::prepare(&tab.request, &vars, inherited.as_ref().map(|(a, _)| a))
                .map(|prep| {
                    prep.headers
                        .into_iter()
                        .filter(|(k, _)| !tab.request.headers.iter().any(|h| h.enabled && h.key.eq_ignore_ascii_case(k)))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut open_cookies = false;
        let mut oauth_op = None;
        let tab = &mut self.tabs[i];
        let base = Id::unique(("req", tab.request.id));

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let r = &tab.request;
            let params_on = r.params.iter().any(|p| p.enabled && !p.key.is_empty());
            let auth_on = !matches!(r.auth.kind, AuthKind::Inherit | AuthKind::None);
            let nh = r.headers.iter().filter(|h| h.enabled && !h.key.is_empty()).count() + auto_headers.len();
            let body_on = r.body.mode != BodyMode::None;
            let scripts_on = !r.pre_request_script.trim().is_empty() || !r.post_response_script.trim().is_empty();
            let sections = [
                (ReqSection::Docs, Some(icons::TEXT_ALIGN_LEFT), "Docs", None, false),
                (ReqSection::Params, None, "Params", None, params_on),
                (ReqSection::Auth, None, "Authorization", None, auth_on),
                (ReqSection::Headers, None, "Headers", Some(nh), nh > 0),
                (ReqSection::Body, None, "Body", None, body_on),
                (ReqSection::Scripts, None, "Scripts", None, scripts_on),
                (ReqSection::Settings, None, "Settings", None, false),
            ];
            for (sec, icon, label, count, dot) in sections {
                if pm::pill_tab(ui, icon, label, count, dot, tab.req_section == sec).clicked() {
                    tab.req_section = sec;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pm::text_button(ui, None, "Cookies", false).clicked() {
                    open_cookies = true;
                }
            });
        });
        ui.add_space(8.0);

        egui::ScrollArea::vertical().id_salt(base.with("scroll")).auto_shrink(false).show(ui, |ui| match tab.req_section {
            ReqSection::Docs => {
                ui.add(
                    egui::TextEdit::multiline(&mut tab.request.description)
                        .hint_text(RichText::new("Make things easier for your teammates with a complete request description.").color(p.text3))
                        .frame(egui::Frame::NONE)
                        .desired_rows(12)
                        .desired_width(f32::INFINITY),
                );
            }
            ReqSection::Params => {
                ui.label(RichText::new("Query Params").font(medium(UI_SIZE)).color(p.text));
                ui.add_space(6.0);
                if pm::kv_table(ui, base.with("params"), &mut tab.request.params, &vars, TableOpts::default()) {
                    tab.request.sync_url_from_params();
                }
            }
            ReqSection::Auth => {
                oauth_op = auth_form(ui, base.with("auth"), &mut tab.request.auth, &vars, true, inherited.as_ref().map(|(a, from)| (a, from.as_str()))).oauth;
            }
            ReqSection::Headers => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Headers").font(medium(UI_SIZE)).color(p.text));
                    if !auto_headers.is_empty() {
                        let label = if tab.show_hidden_headers { "Hide auto-generated headers".to_string() } else { format!("{} hidden", auto_headers.len()) };
                        let glyph = icons::EYE;
                        if pm::text_button(ui, Some(glyph), &label, false).clicked() {
                            tab.show_hidden_headers = !tab.show_hidden_headers;
                        }
                    }
                });
                ui.add_space(6.0);
                if tab.show_hidden_headers && !auto_headers.is_empty() {
                    let rows: Vec<Vec<String>> = auto_headers.iter().map(|(k, v)| vec![k.clone(), v.clone(), "Auto-generated".into()]).collect();
                    pm::ro_table(ui, &["Key", "Value", "Description"], &rows, &[0.34, 0.34, 0.32], true);
                    ui.add_space(10.0);
                }
                pm::kv_table(ui, base.with("headers"), &mut tab.request.headers, &vars, TableOpts::default());
            }
            ReqSection::Body => body_editor(ui, base, &mut tab.request.body, &vars, p),
            ReqSection::Scripts => scripts_editor(ui, base, &mut tab.request.pre_request_script, &mut tab.request.post_response_script, &mut tab.script_phase),
            ReqSection::Settings => settings_form(ui, &mut tab.request.settings, p),
        });
        if open_cookies {
            self.cookies_open = true;
        }
        if let Some(op) = oauth_op {
            let target = OAuthTarget::Request { request: self.tabs[i].request.id, collection: self.tabs[i].collection };
            self.start_oauth(target, op, ui.ctx());
        }
    }
}

// ---------------------------------------------------------------------------
// Authorization
// ---------------------------------------------------------------------------

fn auth_description(kind: AuthKind, inherited: Option<(&Auth, &str)>) -> String {
    match kind {
        AuthKind::Inherit => match inherited {
            Some((a, from)) if !matches!(a.kind, AuthKind::None | AuthKind::Inherit) => format!("This request is using {} from {from}.", a.kind.label()),
            Some((_, from)) => format!("This request is using No Auth from {from}."),
            None => "This request is not inheriting any authorization helper at the moment. Save it in a collection to use the authorization of its collection or folder.".into(),
        },
        AuthKind::None => "This request does not use any authorization.".into(),
        AuthKind::Bearer => "The authorization header will be automatically generated when you send the request.".into(),
        AuthKind::Basic => "The authorization header will be automatically generated when you send the request.".into(),
        AuthKind::ApiKey => "The key-value pair will be added as a header or query parameter when you send the request.".into(),
        AuthKind::OAuth2 => "The current access token is added to the request when you send it. Configure the authorization server and click Get New Access Token to sign in and fetch one.".into(),
        AuthKind::AwsSigV4 => "The request is signed with AWS Signature Version 4 when you send it: Authorization, X-Amz-Date and X-Amz-Content-Sha256 headers are added.".into(),
        AuthKind::Digest => "The request is sent once to get the server's Digest challenge, then sent again with the computed Authorization header.".into(),
    }
}

/// What the authorization form asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OAuthOp {
    /// Run the configured grant to get a new token.
    New,
    /// Use the refresh token.
    Refresh,
}

#[derive(Default)]
pub(super) struct AuthFormOut {
    pub changed: bool,
    pub oauth: Option<OAuthOp>,
}

/// Bordered dropdown showing the label of `current`.
fn dropdown<T: Copy + PartialEq>(ui: &mut Ui, current: &mut T, options: &[(T, &str)], w: f32) -> bool {
    let p = pal(ui);
    let mut changed = false;
    let label = options.iter().find(|(v, _)| v == current).map(|(_, l)| *l).unwrap_or("");
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    ui.painter().rect(rect, 4.0, p.bg, Stroke::new(1.0, if resp.hovered() { p.text3 } else { p.input_border }), StrokeKind::Inside);
    paint_text(ui, pos2(rect.left() + 10.0, rect.center().y), label, regular(UI_SIZE), p.text, w - 30.0);
    ui.painter().text(pos2(rect.right() - 16.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
    egui::Popup::menu(&resp).width(w).show(|ui| {
        for (v, l) in options {
            if ui.selectable_label(*current == *v, *l).clicked() {
                *current = *v;
                changed = true;
                ui.close();
            }
        }
    });
    changed
}

/// "45 s", "12 min", "3 h 5 min", "2 d 4 h".
fn human_duration(secs: i64) -> String {
    let s = secs.max(0);
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        3600..86400 => format!("{} h {} min", s / 3600, s % 3600 / 60),
        _ => format!("{} d {} h", s / 86400, s % 86400 / 3600),
    }
}

/// The OAuth 2.0 part of the authorization form: current token, then the new token configuration.
fn oauth2_form(ui: &mut Ui, id: Id, o: &mut OAuth2, vars: &VarMap, out: &mut AuthFormOut) {
    let p = pal(ui);
    const LABEL_W: f32 = 170.0;
    let w = (ui.available_width() - LABEL_W - 20.0).clamp(160.0, 520.0);
    let label = |ui: &mut Ui, text: &str| {
        let (r, _) = ui.allocate_exact_size(vec2(LABEL_W, 32.0), Sense::hover());
        paint_text(ui, pos2(r.left(), r.center().y), text, regular(UI_SIZE), p.text, LABEL_W - 8.0);
    };
    let hint = |ui: &mut Ui, text: &str| {
        ui.horizontal_top(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            ui.vertical(|ui| {
                ui.set_width(w);
                ui.add(egui::Label::new(RichText::new(text).font(regular(12.0)).color(p.text2)).wrap());
            });
        });
        ui.add_space(6.0);
    };
    let mut changed = false;
    let field = |ui: &mut Ui, text: &str, value: &mut String, key: &str, placeholder: &str| -> bool {
        let mut ch = false;
        ui.horizontal(|ui| {
            label(ui, text);
            ch = pm::input(ui, id.with(key), value, vars, placeholder, w).changed();
        });
        ui.add_space(6.0);
        ch
    };
    let section = |ui: &mut Ui, text: &str| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).font(semibold(UI_SIZE)).color(p.text));
        ui.add_space(8.0);
    };

    // --- Current token -------------------------------------------------------
    section(ui, "Current Token");
    changed |= field(ui, "Token", &mut o.access_token, "o_token", "Token");
    let now = crate::oauth2::now();
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
        let (text, color) = if o.access_token.is_empty() {
            ("No token yet. Configure the server below and get a new one.".to_string(), p.text2)
        } else {
            match o.expires_at {
                Some(t) if t <= now => (format!("Expired {} ago", human_duration(now - t)), Color32::from_rgb(0xe1, 0x4c, 0x3b)),
                Some(t) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(30));
                    (format!("Expires in {}", human_duration(t - now)), p.text2)
                }
                None => ("The server did not say when this token expires".to_string(), p.text2),
            }
        };
        let extra = if o.refresh_token.is_empty() { "" } else { " · refresh token available" };
        ui.label(RichText::new(format!("{text}{extra}")).font(regular(12.0)).color(color));
        if !o.refresh_token.is_empty() && pm::text_button(ui, Some(icons::ARROWS_CLOCKWISE), "Refresh", false).clicked() {
            out.oauth = Some(OAuthOp::Refresh);
        }
        if !o.access_token.is_empty() && pm::text_button(ui, None, "Clear", false).clicked() {
            o.clear_token();
            changed = true;
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        label(ui, "Add token to");
        changed |= dropdown(ui, &mut o.add_to, &[(TokenPlacement::Header, "Request Headers"), (TokenPlacement::Query, "Request URL")], w);
    });
    ui.add_space(6.0);
    if o.add_to == TokenPlacement::Header {
        changed |= field(ui, "Header Prefix", &mut o.header_prefix, "o_prefix", "Bearer");
    }
    ui.horizontal(|ui| {
        label(ui, "Auto-refresh token");
        changed |= pm::toggle(ui, &mut o.auto_refresh).changed();
    });
    hint(ui, "When the token has expired and a refresh token is available, a new token is fetched before the request is sent.");

    ui.add_space(8.0);
    pm::hline(ui);
    ui.add_space(8.0);

    // --- Configure new token -------------------------------------------------
    section(ui, "Configure New Token");
    let grants: Vec<(GrantType, &str)> = GrantType::ALL.iter().map(|g| (*g, g.label())).collect();
    ui.horizontal(|ui| {
        label(ui, "Grant type");
        changed |= dropdown(ui, &mut o.grant_type, &grants, w);
    });
    ui.add_space(6.0);
    let g = o.grant_type;
    if g.uses_browser() {
        changed |= field(ui, "Callback URL", &mut o.callback_url, "o_cb", crate::model::DEFAULT_CALLBACK_URL);
        hint(ui, "Register this URL with the provider. With http://localhost or 127.0.0.1, Rustman catches the redirect itself; otherwise you paste the URL the browser lands on.");
        changed |= field(ui, "Auth URL", &mut o.auth_url, "o_auth", "https://example.com/login/oauth/authorize");
    }
    if g != GrantType::Implicit {
        changed |= field(ui, "Access Token URL", &mut o.access_token_url, "o_tok_url", "https://example.com/login/oauth/access_token");
    }
    changed |= field(ui, "Client ID", &mut o.client_id, "o_cid", "Client ID");
    if g != GrantType::Implicit {
        let ph = if g == GrantType::AuthorizationCodePkce { "Client Secret (optional for public clients)" } else { "Client Secret" };
        changed |= field(ui, "Client Secret", &mut o.client_secret, "o_secret", ph);
    }
    if g == GrantType::Password {
        changed |= field(ui, "Username", &mut o.username, "o_user", "Username");
        changed |= field(ui, "Password", &mut o.password, "o_pass", "Password");
    }
    if g == GrantType::AuthorizationCodePkce {
        ui.horizontal(|ui| {
            label(ui, "Code Challenge Method");
            changed |= dropdown(ui, &mut o.pkce_method, &[(PkceMethod::S256, "SHA-256"), (PkceMethod::Plain, "Plain")], w);
        });
        ui.add_space(6.0);
        changed |= field(ui, "Code Verifier", &mut o.code_verifier, "o_verifier", "Generated automatically if empty");
    }
    changed |= field(ui, "Scope", &mut o.scope, "o_scope", "e.g. read:org");
    if g.uses_browser() {
        changed |= field(ui, "State", &mut o.state, "o_state", "Generated automatically if empty");
    }
    if g != GrantType::Implicit {
        ui.horizontal(|ui| {
            label(ui, "Client Authentication");
            changed |= dropdown(ui, &mut o.client_auth, &[(ClientAuth::BasicHeader, "Send as Basic Auth header"), (ClientAuth::Body, "Send client credentials in body")], w);
        });
        ui.add_space(6.0);
        changed |= field(ui, "Refresh Token URL", &mut o.refresh_token_url, "o_refresh_url", "Same as Access Token URL");
    }
    changed |= field(ui, "Audience", &mut o.audience, "o_aud", "Optional");
    changed |= field(ui, "Resource", &mut o.resource, "o_res", "Optional");
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
        if pm::primary_button(ui, "Get New Access Token").clicked() {
            out.oauth = Some(OAuthOp::New);
        }
    });
    ui.add_space(12.0);
    out.changed |= changed;
}

/// Postman's two-column authorization form. Also used by the collection editor.
/// `inherited`: the auth a request or folder set to `Inherit` would use, and where it comes from.
pub(super) fn auth_form(ui: &mut Ui, id: Id, auth: &mut Auth, vars: &VarMap, allow_inherit: bool, inherited: Option<(&Auth, &str)>) -> AuthFormOut {
    let p = pal(ui);
    let mut changed = false;
    let mut out = AuthFormOut::default();
    let kinds: &[AuthKind] = if allow_inherit {
        &[AuthKind::Inherit, AuthKind::None, AuthKind::Bearer, AuthKind::Basic, AuthKind::ApiKey, AuthKind::Digest, AuthKind::OAuth2, AuthKind::AwsSigV4]
    } else {
        &[AuthKind::None, AuthKind::Bearer, AuthKind::Basic, AuthKind::ApiKey, AuthKind::Digest, AuthKind::OAuth2, AuthKind::AwsSigV4]
    };
    let left_w = 300.0f32.min(ui.available_width() * 0.4);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(left_w);
            ui.label(RichText::new("Auth Type").font(regular(13.0)).color(p.text2));
            ui.add_space(4.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(left_w - 24.0, 32.0), Sense::click());
            ui.painter().rect(rect, 4.0, p.bg, Stroke::new(1.0, if resp.hovered() { p.text3 } else { p.input_border }), StrokeKind::Inside);
            paint_text(ui, pos2(rect.left() + 10.0, rect.center().y), auth.kind.label(), regular(UI_SIZE), p.text, rect.width() - 36.0);
            ui.painter().text(pos2(rect.right() - 16.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
            egui::Popup::menu(&resp).width(rect.width()).show(|ui| {
                for k in kinds {
                    if ui.selectable_label(auth.kind == *k, k.label()).clicked() {
                        auth.kind = *k;
                        changed = true;
                        ui.close();
                    }
                }
            });
            ui.add_space(10.0);
            ui.add(egui::Label::new(RichText::new(auth_description(auth.kind, inherited)).font(regular(13.0)).color(p.text2)).wrap());
        });
        let (div, _) = ui.allocate_exact_size(vec2(1.0, 160.0), Sense::hover());
        ui.painter().vline(div.center().x, div.y_range(), Stroke::new(1.0, p.border));
        ui.add_space(16.0);
        ui.vertical(|ui| {
            let w = (ui.available_width() - 120.0).clamp(160.0, 520.0);
            let field = |ui: &mut Ui, label: &str, value: &mut String, key: &str, hint: &str| -> bool {
                let mut ch = false;
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(100.0, 32.0), Sense::hover());
                    paint_text(ui, pos2(r.left(), r.center().y), label, regular(UI_SIZE), p.text, 100.0);
                    ch = pm::input(ui, id.with(key), value, vars, hint, w).changed();
                });
                ui.add_space(6.0);
                ch
            };
            match auth.kind {
                AuthKind::Inherit | AuthKind::None => {
                    ui.add_space(30.0);
                    ui.label(RichText::new(if auth.kind == AuthKind::None { "No Auth" } else { "Inherit auth from parent" }).color(p.text2));
                }
                AuthKind::Bearer => {
                    changed |= field(ui, "Token", &mut auth.bearer_token, "token", "Token");
                }
                AuthKind::Basic => {
                    changed |= field(ui, "Username", &mut auth.basic_username, "user", "Username");
                    changed |= field(ui, "Password", &mut auth.basic_password, "pass", "Password");
                }
                AuthKind::OAuth2 => oauth2_form(ui, id.with("oauth2"), &mut auth.oauth2, vars, &mut out),
                AuthKind::Digest => {
                    changed |= field(ui, "Username", &mut auth.digest_username, "dig_user", "Username");
                    changed |= field(ui, "Password", &mut auth.digest_password, "dig_pass", "Password");
                }
                AuthKind::AwsSigV4 => {
                    changed |= field(ui, "AccessKey", &mut auth.aws_access_key, "aws_ak", "Access key ID");
                    changed |= field(ui, "SecretKey", &mut auth.aws_secret_key, "aws_sk", "Secret access key");
                    changed |= field(ui, "AWS Region", &mut auth.aws_region, "aws_region", "us-east-1");
                    changed |= field(ui, "Service Name", &mut auth.aws_service, "aws_service", "execute-api, s3, ...");
                    changed |= field(ui, "Session Token", &mut auth.aws_session_token, "aws_token", "Optional, for temporary credentials");
                }
                AuthKind::ApiKey => {
                    changed |= field(ui, "Key", &mut auth.api_key_name, "k", "Key");
                    changed |= field(ui, "Value", &mut auth.api_key_value, "v", "Value");
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(100.0, 32.0), Sense::hover());
                        paint_text(ui, pos2(r.left(), r.center().y), "Add to", regular(UI_SIZE), p.text, 100.0);
                        let label = if auth.api_key_in == ApiKeyLocation::Header { "Header" } else { "Query Params" };
                        let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
                        ui.painter().rect(rect, 4.0, p.bg, Stroke::new(1.0, p.input_border), StrokeKind::Inside);
                        paint_text(ui, pos2(rect.left() + 10.0, rect.center().y), label, regular(UI_SIZE), p.text, w - 30.0);
                        ui.painter().text(pos2(rect.right() - 16.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(12.0), p.text2);
                        egui::Popup::menu(&resp).width(w).show(|ui| {
                            for (loc, l) in [(ApiKeyLocation::Header, "Header"), (ApiKeyLocation::Query, "Query Params")] {
                                if ui.selectable_label(auth.api_key_in == loc, l).clicked() {
                                    auth.api_key_in = loc;
                                    changed = true;
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            }
        });
    });
    out.changed |= changed;
    out
}

// ---------------------------------------------------------------------------
// Body
// ---------------------------------------------------------------------------

fn body_editor(ui: &mut Ui, base: Id, body: &mut Body, vars: &VarMap, p: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for m in BodyMode::ALL {
            if pm::radio(ui, body.mode == m, m.label()).clicked() {
                body.mode = m;
            }
        }
        if body.mode == BodyMode::Raw {
            ui.add_space(6.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(pm::text_width(ui, body.raw_lang.label(), medium(13.0)) + 30.0, 26.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, p.hover);
            }
            let blue = if p.dark { Color32::from_rgb(0x6d, 0xa2, 0xf7) } else { p.accent };
            paint_text(ui, pos2(rect.left() + 6.0, rect.center().y), body.raw_lang.label(), medium(13.0), blue, 100.0);
            ui.painter().text(pos2(rect.right() - 11.0, rect.center().y), Align2::CENTER_CENTER, icons::CARET_DOWN, icon_font(11.0), blue);
            egui::Popup::menu(&resp).show(|ui| {
                for l in RawLang::ALL {
                    if ui.selectable_label(body.raw_lang == l, l.label()).clicked() {
                        body.raw_lang = l;
                        ui.close();
                    }
                }
            });
            if body.raw_lang == RawLang::Json {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pm::text_button(ui, None, "Beautify", false).clicked() {
                        body.raw = beautify(&body.raw);
                    }
                });
            }
        }
    });
    ui.add_space(8.0);
    match body.mode {
        BodyMode::None => {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("This request does not have a body").color(p.text2)));
        }
        BodyMode::Raw => {
            let json = body.raw_lang == RawLang::Json;
            code_editor(ui, base.with("raw"), &mut body.raw, json, vars);
            if json && !body.raw.trim().is_empty() && !body.raw.contains("{{")
                && let Err(e) = serde_json::from_str::<serde_json::Value>(&body.raw) {
                    ui.label(RichText::new(format!("{}  {e}", icons::WARNING)).color(Color32::from_rgb(0xe6, 0x96, 0x3c)).font(regular(12.0)));
                }
        }
        BodyMode::UrlEncoded => {
            pm::kv_table(ui, base.with("urlenc"), &mut body.urlencoded, vars, TableOpts::default());
        }
        BodyMode::FormData => {
            pm::form_table(ui, base.with("form"), &mut body.form_data, vars);
        }
        BodyMode::Binary => {
            ui.horizontal(|ui| {
                if pm::secondary_button(ui, if body.binary_path.is_empty() { "Select file" } else { "Change file" }).clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_file() {
                        body.binary_path = path.display().to_string();
                    }
                if !body.binary_path.is_empty() {
                    ui.label(RichText::new(&body.binary_path).color(p.text2));
                }
            });
        }
        BodyMode::GraphQl => {
            ui.columns(2, |cols| {
                cols[0].label(RichText::new("QUERY").font(semibold(12.0)).color(p.text2));
                code_editor(&mut cols[0], base.with("gql_q"), &mut body.graphql_query, false, vars);
                cols[1].label(RichText::new("GRAPHQL VARIABLES").font(semibold(12.0)).color(p.text2));
                code_editor(&mut cols[1], base.with("gql_v"), &mut body.graphql_variables, true, vars);
            });
        }
    }
}

/// Pretty-print JSON with Postman's 4-space indentation, leaving invalid JSON untouched.
pub(super) fn beautify(s: &str) -> String {
    serde_json::from_str::<serde_json::Value>(s).map(|v| super::pretty_json(&v)).unwrap_or_else(|_| s.to_string())
}

// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

const PRE_SNIPPETS: &[(&str, &str)] = &[
    ("Get an environment variable", "pm.environment.get(\"variable_key\");"),
    ("Get a collection variable", "pm.collectionVariables.get(\"variable_key\");"),
    ("Get a variable", "pm.variables.get(\"variable_key\");"),
    ("Set an environment variable", "pm.environment.set(\"variable_key\", \"variable_value\");"),
    ("Set a collection variable", "pm.collectionVariables.set(\"variable_key\", \"variable_value\");"),
    ("Set a global variable", "pm.globals.set(\"variable_key\", \"variable_value\");"),
    ("Add a header", "pm.request.headers.upsert({ key: \"X-Request-Id\", value: pm.variables.replaceIn(\"{{$guid}}\") });"),
    ("Add a query param", "pm.request.url.query.upsert({ key: \"ts\", value: Date.now() });"),
    ("Fetch a token first", "const res = await pm.sendRequest({\n    url: pm.variables.replaceIn(\"{{baseUrl}}/auth/login\"),\n    method: \"POST\",\n    header: { \"Content-Type\": \"application/json\" },\n    body: { mode: \"raw\", raw: JSON.stringify({ username: \"user\", password: \"pass\" }) }\n});\npm.environment.set(\"token\", res.json().token);"),
    ("Skip this request", "pm.execution.skipRequest();"),
    ("Log to the console", "console.log(pm.request.url.toString());"),
];

const POST_SNIPPETS: &[(&str, &str)] = &[
    ("Status code is 200", "pm.test(\"Status code is 200\", function () {\n    pm.response.to.have.status(200);\n});"),
    ("Response time is below 200ms", "pm.test(\"Response time is less than 200ms\", function () {\n    pm.expect(pm.response.responseTime).to.be.below(200);\n});"),
    ("Body contains string", "pm.test(\"Body matches string\", function () {\n    pm.expect(pm.response.text()).to.include(\"string_you_want_to_search\");\n});"),
    ("JSON value check", "pm.test(\"Your test name\", function () {\n    const jsonData = pm.response.json();\n    pm.expect(jsonData.value).to.eql(100);\n});"),
    ("Content-Type header is present", "pm.test(\"Content-Type is present\", function () {\n    pm.response.to.have.header(\"Content-Type\");\n});"),
    ("JSON schema validation", "const schema = {\n    type: \"object\",\n    required: [\"id\"],\n    properties: { id: { type: \"integer\" } }\n};\npm.test(\"Schema is valid\", function () {\n    pm.response.to.have.jsonSchema(schema);\n});"),
    ("Save a value from the response", "pm.environment.set(\"token\", pm.response.json().token);"),
    ("Successful POST request", "pm.test(\"Successful POST request\", function () {\n    pm.expect(pm.response.code).to.be.oneOf([201, 202]);\n});"),
    ("Run another request next", "pm.execution.setNextRequest(\"Request name\");"),
    ("Log the response", "console.log(pm.response.json());"),
];

/// Pre-request / post-response script editors with a snippet list (requests and collections).
pub(super) fn scripts_editor(ui: &mut Ui, base: Id, pre: &mut String, post: &mut String, phase: &mut ScriptPhase) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (ph, label, has) in [(ScriptPhase::Pre, "Pre-request", !pre.trim().is_empty()), (ScriptPhase::Post, "Post-response", !post.trim().is_empty())] {
            if pm::pill_tab(ui, None, label, None, has, *phase == ph).clicked() {
                *phase = ph;
            }
        }
        ui.add_space(10.0);
        let hint = match phase {
            ScriptPhase::Pre => "Runs before the request is sent: prepare variables, headers or the URL.",
            ScriptPhase::Post => "Runs after the response arrives: write tests with pm.test and pm.expect.",
        };
        ui.label(RichText::new(hint).font(regular(12.5)).color(p.text3));
    });
    ui.add_space(6.0);
    let (text, snippets, key) = match phase {
        ScriptPhase::Pre => (pre, PRE_SNIPPETS, "pre"),
        ScriptPhase::Post => (post, POST_SNIPPETS, "post"),
    };
    let avail = ui.available_size();
    let side_w = 240.0_f32.min(avail.x * 0.35);
    let mut insert = None;
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2((avail.x - side_w - 16.0).max(200.0), avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
            code_editor_hl(ui, base.with(("script", key)), text, Hl::Js, &VarMap::new());
        });
        ui.add_space(16.0);
        ui.allocate_ui_with_layout(vec2(side_w, avail.y), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.label(RichText::new("Snippets").font(semibold(13.0)).color(p.text2));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt(base.with(("snippets", key))).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for (label, code) in snippets {
                    let r = ui.add(egui::Label::new(RichText::new(*label).font(regular(13.0)).color(p.accent)).sense(Sense::click()));
                    if r.clicked() {
                        insert = Some(*code);
                    }
                    r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(RichText::new(*code).font(mono(12.0)));
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Scripts use a Postman-compatible pm API (pm.test, pm.expect, pm.environment, pm.sendRequest…). Collection scripts run before request scripts.").font(regular(12.0)).color(p.text3));
            });
        });
    });
    if let Some(code) = insert {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(code);
        text.push('\n');
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hl {
    Plain,
    Json,
    Js,
}

/// Multi-line code editor with a line-number gutter.
pub(super) fn code_editor(ui: &mut Ui, id: Id, text: &mut String, json: bool, vars: &VarMap) {
    code_editor_hl(ui, id, text, if json { Hl::Json } else { Hl::Plain }, vars);
}

pub(super) fn code_editor_hl(ui: &mut Ui, id: Id, text: &mut String, hl: Hl, vars: &VarMap) {
    let p = pal(ui);
    let font = mono(MONO_SIZE);
    let lines = text.split('\n').count().max(1);
    let min_h = (ui.available_height() - 8.0).max(220.0);
    egui::ScrollArea::both().id_salt(id.with("scroll")).max_height(min_h).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let numbers: String = (1..=lines).map(|n| format!("{n}\n")).collect();
            let digits = lines.to_string().len().max(2) as f32;
            let gutter_w = digits * 9.0 + 22.0;
            ui.allocate_ui(vec2(gutter_w, 0.0), |ui| {
                ui.set_width(gutter_w);
                ui.add(egui::Label::new(RichText::new(numbers.trim_end()).font(font.clone()).color(p.line_number)).halign(egui::Align::RIGHT).selectable(false));
            });
            ui.add_space(14.0);
            let mut layouter = |ui: &Ui, buf: &dyn TextBuffer, _wrap: f32| {
                let mut job = match hl {
                    Hl::Json => pm::json_job_colored(buf.as_str(), font.clone(), p),
                    Hl::Js => pm::code_job(buf.as_str(), crate::codegen::Syntax::Js, font.clone(), p),
                    Hl::Plain => pm::var_layout(buf.as_str(), vars, font.clone(), p.text),
                };
                job.wrap.max_width = f32::INFINITY;
                ui.painter().layout_job(job)
            };
            ui.add(
                egui::TextEdit::multiline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .code_editor()
                    .lock_focus(true)
                    .desired_rows(12)
                    .desired_width(f32::INFINITY)
                    .min_size(vec2(0.0, min_h - 10.0))
                    .layouter(&mut layouter),
            );
        });
    });
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn settings_form(ui: &mut Ui, s: &mut RequestSettings, p: &Palette) {
    let row = |ui: &mut Ui, title: &str, desc: &str, control: &mut dyn FnMut(&mut Ui)| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 160.0).max(200.0));
                ui.label(RichText::new(title).color(p.text));
                ui.add(egui::Label::new(RichText::new(desc).font(regular(13.0)).color(p.text2)).wrap());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| control(ui));
        });
        ui.add_space(10.0);
        pm::hline(ui);
        ui.add_space(10.0);
    };
    row(ui, "Enable SSL certificate verification", "Verify SSL certificates when sending a request. Verification failures will result in the request being aborted.", &mut |ui| {
        pm::toggle(ui, &mut s.verify_ssl);
    });
    row(ui, "Automatically follow redirects", "Follow HTTP 3xx responses as redirects.", &mut |ui| {
        pm::toggle(ui, &mut s.follow_redirects);
    });
    row(ui, "Request timeout", "Set how long a request should wait for a response before timing out. To never time out, set to 0.", &mut |ui| {
        ui.label(RichText::new("ms").color(p.text2));
        ui.add(egui::DragValue::new(&mut s.timeout_ms).range(0..=600_000).speed(100));
    });
}
