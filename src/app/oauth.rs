//! OAuth 2.0 token requests started from the authorization forms, and the dialog shown while
//! one runs (waiting for the browser sign-in, or for a pasted redirect URL).

use eframe::egui::{self, Color32, Id, RichText};
use tokio::sync::oneshot;

use super::request::OAuthOp;
use super::*;
use crate::oauth2;
use crate::pm::{self, ButtonKind};
use crate::theme::{pal, regular, UI_SIZE};

/// Whose auth gets the token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OAuthTarget {
    /// A request open in a tab (and its saved copy in `collection`, if any).
    Request { request: Uuid, collection: Option<Uuid> },
    Collection(Uuid),
    Folder { collection: Uuid, folder: Uuid },
}

pub(super) struct OAuthJob {
    id: Uuid,
    target: OAuthTarget,
    op: OAuthOp,
    /// Browser grants: the authorization URL.
    auth_url: Option<String>,
    callback_url: String,
    token_url: String,
    /// The redirect is caught on a local port.
    listens: bool,
    handle: tokio::task::JoinHandle<()>,
    paste: String,
    paste_tx: Option<oneshot::Sender<String>>,
    started: Instant,
    error: Option<String>,
}

impl RustmanApp {
    /// The auth to use for `target`, the collection whose variables apply, and the request settings.
    fn oauth_source(&self, target: OAuthTarget) -> Option<(Auth, Option<Uuid>, RequestSettings)> {
        match target {
            OAuthTarget::Request { request, .. } => {
                self.tabs.iter().find(|t| t.request.id == request).map(|t| (t.request.auth.clone(), t.collection, t.request.settings.clone()))
            }
            OAuthTarget::Collection(c) => self.ws.collection(c).map(|c| (c.auth.clone(), Some(c.id), RequestSettings::default())),
            OAuthTarget::Folder { collection, folder } => match self.ws.collection(collection).and_then(|c| find_item(&c.items, folder)) {
                Some(Item::Folder(f)) => Some((f.auth.clone(), Some(collection), RequestSettings::default())),
                _ => None,
            },
        }
    }

    pub(super) fn start_oauth(&mut self, target: OAuthTarget, op: OAuthOp, ctx: &egui::Context) {
        if let Some(job) = self.oauth.take() {
            job.handle.abort();
        }
        let Some((auth, collection, settings)) = self.oauth_source(target) else { return };
        let cfg = auth.oauth2.resolved(&self.vars_for(collection));
        let id = Uuid::new_v4();
        let callback_url = cfg.callback_url.clone();
        let (tx, jar, ctx) = (self.tx.clone(), self.jar.clone(), ctx.clone());
        let done = move |outcome: oauth2::Outcome| {
            let _ = tx.send(Msg::OAuth { job: id, outcome: Box::new(outcome) });
            ctx.request_repaint();
        };
        let token_url = if op == OAuthOp::Refresh && !cfg.refresh_token_url.trim().is_empty() { cfg.refresh_token_url.clone() } else { cfg.access_token_url.clone() };
        let (auth_url, listens, paste_tx, handle) = match op {
            OAuthOp::Refresh => {
                if cfg.refresh_token.is_empty() {
                    self.notify_err("There is no refresh token");
                    return;
                }
                let handle = self.runtime.spawn(async move { done(oauth2::refresh(&cfg, &settings, jar).await) });
                (None, false, None, handle)
            }
            OAuthOp::New => {
                let session = match oauth2::Session::new(&cfg) {
                    Ok(s) => s,
                    Err(e) => {
                        self.notify_err(e);
                        return;
                    }
                };
                let (auth_url, listens) = (session.auth_url.clone(), session.listens);
                let (paste_tx, paste_rx) = oneshot::channel();
                let handle = self.runtime.spawn(async move { done(session.run(settings, jar, oauth2::open_browser, paste_rx).await) });
                (auth_url.clone(), listens, auth_url.map(|_| paste_tx), handle)
            }
        };
        self.oauth = Some(OAuthJob {
            id,
            target,
            op,
            auth_url,
            callback_url,
            token_url,
            listens,
            handle,
            paste: String::new(),
            paste_tx,
            started: Instant::now(),
            error: None,
        });
    }

    pub(super) fn finish_oauth(&mut self, job: Uuid, outcome: oauth2::Outcome) {
        for (prepared, result) in &outcome.exchanges {
            let log = self.console.start(prepared);
            self.console.finish(log, result);
        }
        let Some(j) = self.oauth.as_mut().filter(|j| j.id == job) else { return };
        match outcome.token {
            Ok(token) => {
                let (target, op) = (j.target, j.op);
                self.oauth = None;
                self.apply_token(target, &token);
                self.notify(if op == OAuthOp::Refresh { "Access token refreshed" } else { "New access token received" });
            }
            Err(e) => {
                self.console.log(script::LogLevel::Error, format!("OAuth 2.0: {e}"));
                j.error = Some(e);
                j.paste_tx = None;
            }
        }
    }

    /// Stores a token in the auth of `target`. For a request, the open tab, its saved copy and
    /// the collection item all get it, so a new token does not count as an unsaved change.
    pub(super) fn apply_token(&mut self, target: OAuthTarget, token: &OAuthToken) {
        match target {
            OAuthTarget::Collection(c) => {
                if let Some(c) = self.ws.collection_mut(c) {
                    c.auth.oauth2.set_token(token);
                }
            }
            OAuthTarget::Folder { collection, folder } => {
                if let Some(a) = self.ws.collection_mut(collection).and_then(|c| c.auth_mut(AuthOwner::Folder(folder))) {
                    a.oauth2.set_token(token);
                }
            }
            OAuthTarget::Request { request, collection } => {
                for tab in self.tabs.iter_mut().filter(|t| t.request.id == request) {
                    tab.request.auth.oauth2.set_token(token);
                    if let Some(saved) = &mut tab.saved {
                        saved.auth.oauth2.set_token(token);
                    }
                }
                if let Some(c) = collection.and_then(|c| self.ws.collection_mut(c))
                    && let Some(Item::Request(r)) = find_item_mut(&mut c.items, request)
                {
                    r.auth.oauth2.set_token(token);
                }
            }
        }
    }

    /// Stores a token refreshed automatically while sending a request.
    pub(super) fn apply_token_update(&mut self, update: &exec::TokenUpdate, collection: Option<Uuid>) {
        let target = match (update.owner, collection) {
            (None, _) => OAuthTarget::Request { request: update.request, collection },
            (Some(AuthOwner::Collection), Some(c)) => OAuthTarget::Collection(c),
            (Some(AuthOwner::Folder(folder)), Some(c)) => OAuthTarget::Folder { collection: c, folder },
            (Some(_), None) => return,
        };
        self.apply_token(target, &update.token);
    }

    pub(super) fn oauth_window(&mut self, ctx: &egui::Context) {
        let Some(mut job) = self.oauth.take() else { return };
        let mut cancel = false;
        let title = match (job.op, &job.error) {
            (_, Some(_)) => "Could not get an access token",
            (OAuthOp::Refresh, None) => "Refreshing access token",
            (OAuthOp::New, None) => "Get new access token",
        };
        let (_, close) = pm::modal(ctx, Id::unique("oauth_modal"), title, 560.0, |ui| {
            let p = pal(ui);
            if let Some(e) = &job.error {
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(e).font(regular(UI_SIZE)).color(Color32::from_rgb(0xe1, 0x4c, 0x3b))).wrap());
                });
                ui.add_space(6.0);
                ui.label(RichText::new("The token requests are in the Console.").font(regular(12.0)).color(p.text2));
                pm::modal_buttons(ui, |ui| {
                    if pm::modal_button(ui, "Close", ButtonKind::Primary).clicked() {
                        cancel = true;
                    }
                });
                return;
            }
            let waiting_browser = job.auth_url.is_some() && job.paste_tx.is_some();
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0));
                let secs = job.started.elapsed().as_secs();
                let text = if waiting_browser {
                    if job.listens {
                        format!("Waiting for the sign-in to finish in your browser… ({secs} s)")
                    } else {
                        format!("Waiting for the redirect URL… ({secs} s)")
                    }
                } else {
                    format!("Requesting a token from {}…", job.token_url)
                };
                ui.add(egui::Label::new(RichText::new(text).font(regular(UI_SIZE)).color(p.text)).wrap());
            });
            ui.ctx().request_repaint_after(Duration::from_millis(500));
            if let Some(url) = job.auth_url.clone().filter(|_| waiting_browser) {
                ui.add_space(12.0);
                let explain = if job.listens {
                    format!("A browser window was opened. Rustman catches the redirect to {} by itself.", job.callback_url)
                } else {
                    format!("A browser window was opened. After signing in, the browser goes to {}: copy the whole URL from the address bar and paste it below.", job.callback_url)
                };
                ui.add(egui::Label::new(RichText::new(explain).font(regular(13.0)).color(p.text2)).wrap());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if pm::secondary_button(ui, "Open browser again").clicked()
                        && let Err(e) = oauth2::open_browser(&url)
                    {
                        job.error = Some(e);
                    }
                    if pm::secondary_button(ui, "Copy sign-in link").clicked() {
                        ui.ctx().copy_text(url.clone());
                    }
                });
                ui.add_space(12.0);
                let label = if job.listens { "Or paste the URL the browser was redirected to" } else { "Redirect URL" };
                ui.label(RichText::new(label).font(regular(13.0)).color(p.text2));
                ui.add_space(2.0);
                let vars = VarMap::new();
                ui.horizontal(|ui| {
                    let placeholder = format!("{}?code=…", job.callback_url);
                    let resp = pm::input(ui, Id::unique("oauth_paste"), &mut job.paste, &vars, &placeholder, 400.0);
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (pm::secondary_button(ui, "Submit").clicked() || enter)
                        && !job.paste.trim().is_empty()
                        && let Some(tx) = job.paste_tx.take()
                    {
                        let _ = tx.send(job.paste.trim().to_string());
                    }
                });
            }
            pm::modal_buttons(ui, |ui| {
                if pm::modal_button(ui, "Cancel", ButtonKind::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
        if cancel || close {
            job.handle.abort();
        } else {
            self.oauth = Some(job);
        }
    }
}
