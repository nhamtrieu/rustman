//! Import → "From Postman": sign in with a Postman API key, pick a workspace, tick the
//! collections and environments to bring over. The calls run on the runtime and answer
//! through `Msg::Postman`.

use eframe::egui::{self, RichText, Ui};

use super::*;
use crate::pm;
use crate::postman_api::{Client, KEYS_URL};
use crate::theme::{medium, pal, regular, semibold, UI_SIZE};

/// What the "From Postman" tab asks for this frame.
#[derive(Default)]
pub(super) struct PostmanActions {
    connect: bool,
    workspace: Option<String>,
    import: bool,
}

impl RustmanApp {
    fn postman_spawn<F>(&self, ctx: &egui::Context, f: impl FnOnce(Client) -> F + Send + 'static)
    where
        F: std::future::Future<Output = PostmanReply> + Send + 'static,
    {
        let client = Client::new(&self.postman_key);
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.runtime.spawn(async move {
            let reply = f(client).await;
            let _ = tx.send(Msg::Postman(reply));
            ctx.request_repaint();
        });
    }

    pub(super) fn postman_actions(&mut self, st: &mut PostmanImport, a: PostmanActions, ctx: &egui::Context) {
        if a.connect {
            st.error = None;
            st.busy = Some("Loading workspaces…");
            self.postman_spawn(ctx, |c| async move { PostmanReply::Workspaces(c.workspaces().await) });
        }
        if let Some(ws) = a.workspace {
            st.workspace = Some(ws.clone());
            st.listing = None;
            st.selected.clear();
            st.error = None;
            st.busy = Some("Loading collections and environments…");
            self.postman_spawn(ctx, move |c| async move {
                let r = c.listing(&ws).await;
                PostmanReply::Listing(ws, r)
            });
        }
        if a.import
            && let Some(l) = &st.listing
        {
            let picks: Vec<(bool, String, String)> = l
                .collections
                .iter()
                .map(|e| (false, e))
                .chain(l.environments.iter().map(|e| (true, e)))
                .filter(|(_, e)| st.selected.contains(&e.uid))
                .map(|(env, e)| (env, e.uid.clone(), e.name.clone()))
                .collect();
            if picks.is_empty() {
                return;
            }
            st.error = None;
            st.busy = Some("Downloading from Postman…");
            self.postman_spawn(ctx, move |c| async move {
                let (mut ok, mut failed) = (Vec::new(), Vec::new());
                for (env, uid, name) in picks {
                    let r = if env { c.environment(&uid).await } else { c.collection(&uid).await };
                    match r {
                        Ok(json) => ok.push((env, name, json)),
                        Err(e) => failed.push(format!("{name}: {e}")),
                    }
                }
                PostmanReply::Downloaded(ok, failed)
            });
        }
    }

    pub(super) fn postman_reply(&mut self, reply: PostmanReply) {
        // The dialog may have been closed meanwhile; downloads are still imported.
        let st = self.import.as_mut().map(|i| &mut i.postman);
        match reply {
            PostmanReply::Workspaces(r) => {
                let Some(st) = st else { return };
                st.busy = None;
                match r {
                    Ok(ws) if ws.is_empty() => st.error = Some("This Postman account has no workspaces.".into()),
                    Ok(ws) => {
                        st.workspaces = ws;
                        st.workspace = None;
                        st.listing = None;
                    }
                    Err(e) => st.error = Some(e),
                }
            }
            PostmanReply::Listing(ws, r) => {
                let Some(st) = st.filter(|s| s.workspace.as_deref() == Some(ws.as_str())) else { return };
                st.busy = None;
                match r {
                    Ok(l) => st.listing = Some(l),
                    Err(e) => st.error = Some(e),
                }
            }
            PostmanReply::Downloaded(ok, failed) => {
                let mut imported = Vec::new();
                let mut errors = failed;
                for (env, name, json) in ok {
                    if env {
                        match import::import_postman_environment(&json) {
                            Ok(e) => {
                                imported.push(format!("environment \"{}\"", e.name));
                                self.ws.environments.push(e);
                                self.env_section_open = true;
                            }
                            Err(e) => errors.push(format!("{name}: {e}")),
                        }
                    } else {
                        match import::import_postman_collection(&json) {
                            Ok(c) => {
                                imported.push(format!("collection \"{}\"", c.name));
                                self.ws.expanded.insert(c.id);
                                self.ws.collections.push(c);
                                self.sidebar_mode = SidebarMode::Explorer;
                            }
                            Err(e) => errors.push(format!("{name}: {e}")),
                        }
                    }
                }
                if let Some(st) = self.import.as_mut().map(|i| &mut i.postman) {
                    st.busy = None;
                    st.selected.clear();
                    st.error = (!errors.is_empty()).then(|| format!("Could not import:\n{}", errors.join("\n")));
                }
                if !imported.is_empty() {
                    let msg = if imported.len() == 1 { format!("Imported {}", imported[0]) } else { format!("Imported {} items from Postman", imported.len()) };
                    self.notify(msg);
                    if errors.is_empty() {
                        self.import = None;
                    }
                } else if !errors.is_empty() && self.import.is_none() {
                    self.notify_err(errors.join("\n"));
                }
            }
        }
    }
}

/// The "From Postman" tab of the import dialog.
pub(super) fn postman_tab(ui: &mut Ui, st: &mut PostmanImport, key: &mut String) -> PostmanActions {
    let p = pal(ui);
    let mut a = PostmanActions::default();
    ui.add(
        egui::Label::new(
            RichText::new("Import collections and environments straight from your Postman account, without exporting files. You need a Postman API key; it is kept only until Rustman closes.")
                .font(regular(13.0))
                .color(p.text2),
        )
        .wrap(),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("API key").font(regular(UI_SIZE)).color(p.text));
        let w = (ui.available_width() - 120.0).max(160.0);
        let resp = ui.add(egui::TextEdit::singleline(key).password(true).hint_text("PMAK-…").desired_width(w));
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (pm::secondary_button(ui, "Connect").clicked() || enter) && st.busy.is_none() {
            a.connect = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("No key yet?").font(regular(12.0)).color(p.text3));
        ui.hyperlink_to(RichText::new("Create one in Postman → Settings → API keys").font(regular(12.0)), KEYS_URL);
    });
    ui.add_space(10.0);

    if !st.workspaces.is_empty() {
        // Open the personal workspace (listed first) once connected.
        if st.workspace.is_none() && st.busy.is_none() && st.error.is_none() {
            a.workspace = Some(st.workspaces[0].id.clone());
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Workspace").font(regular(UI_SIZE)).color(p.text));
            let current = st.workspace.as_ref().and_then(|id| st.workspaces.iter().find(|w| &w.id == id)).map(|w| w.name.clone()).unwrap_or_else(|| "Choose a workspace".into());
            let resp = pm::text_button(ui, None, &current, true);
            egui::Popup::menu(&resp).show(|ui| {
                for w in &st.workspaces {
                    let label = if w.kind.is_empty() { w.name.clone() } else { format!("{}  ·  {}", w.name, w.kind) };
                    if ui.selectable_label(st.workspace.as_ref() == Some(&w.id), label).clicked() {
                        a.workspace = Some(w.id.clone());
                        ui.close();
                    }
                }
            });
        });
        ui.add_space(6.0);
    }

    if let Some(l) = &st.listing {
        let all: Vec<&String> = l.collections.iter().chain(&l.environments).map(|e| &e.uid).collect();
        egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            for (title, list) in [("Collections", &l.collections), ("Environments", &l.environments)] {
                ui.label(RichText::new(format!("{title} ({})", list.len())).font(semibold(13.0)).color(p.text2));
                if list.is_empty() {
                    ui.label(RichText::new("None in this workspace").font(regular(12.0)).color(p.text3));
                }
                for e in list {
                    let mut on = st.selected.contains(&e.uid);
                    if ui.checkbox(&mut on, RichText::new(&e.name).font(regular(UI_SIZE))).changed() {
                        if on {
                            st.selected.insert(e.uid.clone());
                        } else {
                            st.selected.remove(&e.uid);
                        }
                    }
                }
                ui.add_space(6.0);
            }
        });
        ui.horizontal(|ui| {
            if !all.is_empty() && pm::text_button(ui, None, if st.selected.len() == all.len() { "Select none" } else { "Select all" }, false).clicked() {
                if st.selected.len() == all.len() {
                    st.selected.clear();
                } else {
                    st.selected = all.iter().map(|s| s.to_string()).collect();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let n = st.selected.len();
                let label = if n == 0 { "Import".to_string() } else { format!("Import {n} selected") };
                if pm::primary_button(ui, &label).clicked() && n > 0 && st.busy.is_none() {
                    a.import = true;
                }
            });
        });
    }

    if let Some(b) = st.busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(b).font(medium(13.0)).color(p.text2));
        });
    }
    if let Some(e) = &st.error {
        ui.add(egui::Label::new(RichText::new(e).color(p.danger)).wrap());
    }
    a
}
