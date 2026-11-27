//! Left sidebar: the explorer (collections, environments, documents, specs, mocks, datasets, toolsets) and History.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, Id, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::*;
use crate::icons;
use crate::pm::{self, paint_text};
use crate::theme::{Palette, icon_font, pal, regular, semibold};

const ROW: f32 = 30.0;

#[derive(Clone, Copy, PartialEq)]
enum RowKind {
    Collection,
    Folder,
    Request(Method),
}

struct RowOut {
    resp: Response,
    plus: bool,
    dots: Response,
    dots_popup: Id,
}

/// Shared state while drawing the tree (keeps borrows of `self` disjoint).
struct TreeCtx<'a> {
    active: Option<Uuid>,
    filter: String,
    expanded: &'a HashSet<Uuid>,
    rename: &'a mut Option<(RenameTarget, String)>,
    rename_focus: &'a mut bool,
    commit: bool,
    cancel: bool,
    actions: Vec<TreeAction>,
    /// Row with the keyboard-focus ring (last clicked).
    focus: Option<Id>,
    clicked_row: Option<Id>,
    /// Selected items when more than one is selected (a single one is not highlighted).
    multi: &'a [(Uuid, Uuid)],
    /// Item rows in drawing order.
    rows: Vec<(Uuid, Uuid)>,
}

impl TreeCtx<'_> {
    fn in_multi(&self, row: (Uuid, Uuid)) -> bool {
        self.multi.contains(&row)
    }

    /// Clicks on a request or folder row. Returns true for a plain click, which also opens the
    /// request or toggles the folder; Shift and Ctrl/Cmd clicks only change the selection.
    fn row_clicks(&mut self, ui: &Ui, resp: &Response, row: (Uuid, Uuid)) -> bool {
        let (collection, item) = row;
        if resp.secondary_clicked() && !self.in_multi(row) {
            self.actions.push(TreeAction::Select { collection, item, mode: SelectMode::Only });
        }
        if !resp.clicked() {
            return false;
        }
        let mods = ui.input(|i| i.modifiers);
        let mode = if mods.shift {
            SelectMode::Range
        } else if mods.command {
            SelectMode::Toggle
        } else {
            SelectMode::Only
        };
        self.actions.push(TreeAction::Select { collection, item, mode });
        mode == SelectMode::Only
    }
}

/// Context menu of a row that is part of a multiple selection.
fn multi_menu(ui: &mut Ui, n: usize, actions: &mut Vec<TreeAction>, p: &Palette) {
    if ui.button(RichText::new(format!("Delete {n} items")).color(p.danger)).clicked() {
        actions.push(TreeAction::Delete(Confirm::DeleteSelection));
        ui.close();
    }
}

impl RustmanApp {
    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        // Mode icons.
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let total = 48.0 * 2.0 + 17.0;
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
            if mode_icon(ui, icons::CUBE, self.sidebar_mode == SidebarMode::Explorer, "Collections & Environments").clicked() {
                self.sidebar_mode = SidebarMode::Explorer;
            }
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(vec2(9.0, 30.0), Sense::hover());
            ui.painter().vline(r.center().x, r.top() + 8.0..=r.bottom() - 8.0, Stroke::new(1.0, p.input_border));
            ui.add_space(4.0);
            if mode_icon(ui, icons::CLOCK_COUNTER_CLOCKWISE, self.sidebar_mode == SidebarMode::History, "History").clicked() {
                self.sidebar_mode = SidebarMode::History;
            }
        });
        ui.add_space(8.0);
        // Filter + actions.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(8.0);
            let w = ui.available_width() - 3.0 * 30.0 - 12.0;
            pm::search_box_icon(ui, Id::unique("sidebar_filter"), &mut self.filter, "", w, 30.0, icons::FUNNEL_SIMPLE);
            ui.add_space(6.0);
            let plus = pm::icon_button(ui, icons::PLUS, 16.0, "New");
            let picked = egui::Popup::menu(&plus)
                .id(Id::unique(NEW_MENU))
                .width(270.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(new_menu)
                .and_then(|r| r.inner);
            if !egui::Popup::is_id_open(ui.ctx(), Id::unique(NEW_MENU)) {
                ui.data_mut(|d| d.remove::<String>(Id::unique(NEW_MENU).with("q")));
            }
            match picked {
                Some(NewKind::Http) => self.new_tab(),
                Some(NewKind::Collection) => self.new_collection(),
                Some(NewKind::Environment) => {
                    self.env_section_open = true;
                    self.sidebar_mode = SidebarMode::Explorer;
                    self.new_environment();
                }
                Some(NewKind::Entity(kind)) => self.new_entity(kind),
                Some(NewKind::Unsupported) | None => {}
            }
            if pm::icon_button(ui, icons::ARROW_SQUARE_IN, 16.0, "Import").clicked() {
                self.import = Some(ImportState::default());
            }
            let more = pm::icon_button(ui, icons::DOTS_THREE, 16.0, "More");
            egui::Popup::menu(&more).show(|ui| {
                if ui.button("Expand all").clicked() {
                    for c in &self.ws.collections {
                        self.ws.expanded.insert(c.id);
                        let mut f = Vec::new();
                        collect_folders(&c.items, 0, &mut f);
                        self.ws.expanded.extend(f.into_iter().map(|(id, _, _)| id));
                    }
                    ui.close();
                }
                if ui.button("Collapse all").clicked() {
                    self.ws.expanded.clear();
                    ui.close();
                }
                ui.separator();
                if ui.button("New Environment").clicked() {
                    self.new_environment();
                    ui.close();
                }
            });
        });
        ui.add_space(6.0);
        match self.sidebar_mode {
            SidebarMode::Explorer => self.explorer(ui),
            SidebarMode::History => self.history(ui),
        }
    }

    /// All sections stacked in one scrolling list, like Postman:
    /// COLLECTIONS, ENVIRONMENTS, DOCUMENTS, SPECS, MOCKS, DATASETS, TOOLSETS.
    fn explorer(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let sec_id = Id::unique("sec_collections");
        let coll_open = ui.data(|d| d.get_temp::<bool>(sec_id)).unwrap_or(true);
        let mut toggle_coll = false;
        let mut add_collection = false;
        let (mut actions, mut commit, mut cancel, mut clicked_row) = (Vec::new(), false, false, None);
        let mut tree_rows = Vec::new();

        egui::ScrollArea::vertical().id_salt("sidebar_scroll").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;

            // COLLECTIONS with the request tree.
            let (hr, plus) = section_header(ui, "COLLECTIONS", coll_open, true, false);
            toggle_coll = hr.clicked();
            add_collection = plus;
            if coll_open {
                let mut tc = TreeCtx {
                    active: self.tabs.get(self.active).map(|t| t.request.id),
                    filter: self.filter.trim().to_lowercase(),
                    expanded: &self.ws.expanded,
                    rename: &mut self.rename,
                    rename_focus: &mut self.rename_focus,
                    commit: false,
                    cancel: false,
                    actions: Vec::new(),
                    focus: self.tree_focus,
                    clicked_row: None,
                    multi: if self.tree_selection.len() > 1 { &self.tree_selection } else { &[] },
                    rows: Vec::new(),
                };
                if self.ws.collections.is_empty() {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Create a collection for your requests").color(p.text));
                        ui.add_space(4.0);
                        ui.label(RichText::new("A collection lets you group related requests and easily set common authorization, variables and more.").color(p.text2).font(regular(13.0)));
                        ui.add_space(10.0);
                        if pm::secondary_button(ui, "Create Collection").clicked() {
                            tc.actions.push(TreeAction::Rename(RenameTarget::Collection(Uuid::nil())));
                        }
                    });
                    ui.add_space(16.0);
                }
                for c in &self.ws.collections {
                    if !tc.filter.is_empty() && !c.name.to_lowercase().contains(&tc.filter) && !items_match(&c.items, &tc.filter) {
                        continue;
                    }
                    collection_rows(ui, &mut tc, c, p);
                }
                (actions, commit, cancel, clicked_row) = (std::mem::take(&mut tc.actions), tc.commit, tc.cancel, tc.clicked_row);
                tree_rows = std::mem::take(&mut tc.rows);
            }

            // ENVIRONMENTS
            let env_open = self.env_section_open;
            let (hr, plus) = section_header(ui, "ENVIRONMENTS", env_open, true, true);
            if hr.clicked() {
                self.env_section_open = !self.env_section_open;
            }
            if plus {
                self.new_environment();
            }
            if env_open {
                self.env_list(ui);
            }

            // DOCUMENTS, SPECS, MOCKS, DATASETS, TOOLSETS
            for kind in EntityKind::ALL {
                let open = self.sb_open.contains(&kind);
                let (hr, plus) = section_header(ui, kind.section(), open, true, true);
                if hr.clicked() && !self.sb_open.remove(&kind) {
                    self.sb_open.insert(kind);
                }
                if plus {
                    self.new_entity(kind);
                }
                if open {
                    self.entity_list(ui, kind);
                }
            }
            // Closing line under the last section.
            let r = ui.available_rect_before_wrap();
            ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, p.border));
        });

        if toggle_coll {
            ui.data_mut(|d| d.insert_temp(sec_id, !coll_open));
        }
        if add_collection {
            self.new_collection();
        }
        self.tree_rows = tree_rows;
        // Delete removes the selected items while the tree has the focus (Cmd+Backspace on macOS),
        // Escape drops a multiple selection.
        let typing = ui.ctx().egui_wants_keyboard_input();
        if self.tree_focus.is_some() && !typing && self.confirm.is_none() && self.rename.is_none() {
            let (delete, escape) = ui.input(|i| {
                (i.key_pressed(egui::Key::Delete) || (i.modifiers.command && i.key_pressed(egui::Key::Backspace)), i.key_pressed(egui::Key::Escape))
            });
            if delete {
                let (items, _) = self.selected_items();
                self.confirm = match items.as_slice() {
                    [] => None,
                    [(collection, item)] => Some(Confirm::DeleteItem { collection: *collection, item: *item }),
                    _ => Some(Confirm::DeleteSelection),
                };
            }
            if escape && self.tree_selection.len() > 1 {
                self.tree_selection.clear();
            }
        }
        // The focus ring follows the last clicked row and disappears when clicking elsewhere.
        match clicked_row {
            Some(id) => self.tree_focus = Some(id),
            None if ui.input(|i| i.pointer.any_pressed()) => self.tree_focus = None,
            None => {}
        }
        if commit {
            self.commit_rename();
        } else if cancel {
            self.rename = None;
        }
        for a in actions {
            if let TreeAction::Rename(RenameTarget::Collection(id)) = a
                && id.is_nil() {
                    self.new_collection();
                    continue;
                }
            self.apply(a);
        }
    }

    fn env_list(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut action: Option<(Option<Uuid>, &str)> = None;
        // Globals row.
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        row_bg(ui, rect, resp.hovered(), self.env_shown() == Some(EnvTarget::Globals), false, p);
        ui.painter().text(pos2(rect.left() + 37.0, rect.center().y), Align2::CENTER_CENTER, icons::GLOBE_SIMPLE, icon_font(15.0), p.text2);
        paint_text(ui, pos2(rect.left() + 52.0, rect.center().y), "Globals", regular(14.0), p.text, rect.width() - 60.0);
        if resp.clicked() {
            action = Some((None, "edit"));
        }
        let mut commit = false;
        let mut cancel = false;
        for e in &self.ws.environments {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let active = self.ws.active_environment == Some(e.id);
            let dots_id = Id::unique(("env_dots", e.id));
            let menu_open = egui::Popup::is_id_open(ui.ctx(), dots_id);
            let hovered = ui.rect_contains_pointer(rect) || menu_open;
            row_bg(ui, rect, hovered, self.env_shown() == Some(EnvTarget::Env(e.id)), false, p);
            let renaming = matches!(self.rename, Some((RenameTarget::Env(id), _)) if id == e.id);
            if renaming {
                let text_rect = Rect::from_min_max(pos2(rect.left() + 48.0, rect.top() + 3.0), pos2(rect.right() - 12.0, rect.bottom() - 3.0));
                if let Some((_, name)) = &mut self.rename {
                    let (c, x) = rename_edit(ui, text_rect, Id::unique(("env_rename", e.id)), name, &mut self.rename_focus);
                    commit |= c;
                    cancel |= x;
                }
            } else {
                paint_text(ui, pos2(rect.left() + 52.0, rect.center().y), &e.name, regular(14.0), p.text, rect.width() - 120.0);
            }
            // Active check / set-active button.
            let check_rect = Rect::from_center_size(pos2(row_highlight(rect).right() - 40.0, rect.center().y), vec2(22.0, 22.0));
            let check = ui.interact(check_rect, Id::unique(("env_check", e.id)), Sense::click());
            if active || hovered {
                let color = if active { p.green_dot } else if check.hovered() { p.text } else { p.text3 };
                ui.painter().text(check_rect.center(), Align2::CENTER_CENTER, icons::CHECK_CIRCLE, icon_font(16.0), color);
            }
            if check.on_hover_text(if active { "Active environment" } else { "Set active" }).clicked() {
                action = Some((Some(e.id), if active { "deactivate" } else { "activate" }));
            }
            let dots_rect = Rect::from_center_size(pos2(row_highlight(rect).right() - 14.0, rect.center().y), vec2(22.0, 22.0));
            let dots = ui.interact(dots_rect, Id::unique(("env_dots_btn", e.id)), Sense::click());
            if hovered {
                ui.painter().text(dots_rect.center(), Align2::CENTER_CENTER, icons::DOTS_THREE, icon_font(16.0), if dots.hovered() { p.text } else { p.text2 });
            }
            egui::Popup::menu(&dots).id(dots_id).show(|ui| {
                for (label, act) in [("Rename", "rename"), ("Duplicate", "duplicate"), ("Export", "export"), ("Delete", "delete")] {
                    if ui.button(label).clicked() {
                        action = Some((Some(e.id), act));
                        ui.close();
                    }
                }
            });
            if resp.clicked() && !renaming {
                action = Some((Some(e.id), "edit"));
            }
            if resp.double_clicked() {
                action = Some((Some(e.id), "rename"));
            }
        }
        if commit {
            self.commit_rename();
        } else if cancel {
            self.rename = None;
        }
        match action {
            Some((None, _)) => self.open_env(EnvTarget::Globals),
            Some((Some(id), act)) => match act {
                "edit" => self.open_env(EnvTarget::Env(id)),
                "activate" => self.ws.active_environment = Some(id),
                "deactivate" => self.ws.active_environment = None,
                "rename" => self.apply(TreeAction::Rename(RenameTarget::Env(id))),
                "duplicate" => {
                    if let Some(e) = self.ws.environments.iter().find(|e| e.id == id).cloned() {
                        self.ws.environments.push(Environment { id: Uuid::new_v4(), name: format!("{} Copy", e.name), ..e });
                    }
                }
                "export" => self.export_env(id),
                "delete" => self.confirm = Some(Confirm::DeleteEnv(id)),
                _ => {}
            },
            None => {}
        }
    }

    /// Rows of one entity section (specs, mocks, datasets or documents).
    fn entity_list(&mut self, ui: &mut Ui, kind: EntityKind) {
        let p = pal(ui);
        let items = self.ws.entities(kind);
        let mut action: Option<(Uuid, &str)> = None;
        let (mut commit, mut cancel) = (false, false);
        if items.is_empty() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let text = format!("Create a {}", kind.singular());
            row_bg(ui, rect, resp.hovered(), false, false, p);
            paint_text(ui, pos2(rect.left() + 52.0, rect.center().y), &text, regular(13.5), if resp.hovered() { p.text } else { p.text3 }, rect.width() - 60.0);
            if resp.clicked() {
                self.new_entity(kind);
            }
            return;
        }
        for (id, name) in &items {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let dots_id = Id::unique(("entity_dots", *id));
            let hovered = ui.rect_contains_pointer(rect) || egui::Popup::is_id_open(ui.ctx(), dots_id);
            row_bg(ui, rect, hovered, self.side_shown() == Some(SideTab::Entity(kind, *id)), false, p);
            let renaming = matches!(self.rename, Some((RenameTarget::Entity(_, r), _)) if r == *id);
            if renaming {
                let text_rect = Rect::from_min_max(pos2(rect.left() + 48.0, rect.top() + 3.0), pos2(rect.right() - 12.0, rect.bottom() - 3.0));
                if let Some((_, text)) = &mut self.rename {
                    let (c, x) = rename_edit(ui, text_rect, Id::unique(("entity_rename", *id)), text, &mut self.rename_focus);
                    commit |= c;
                    cancel |= x;
                }
            } else {
                paint_text(ui, pos2(rect.left() + 52.0, rect.center().y), name, regular(14.0), p.text, rect.width() - 100.0);
            }
            let running = match kind {
                EntityKind::Mock => self.mocks_running.contains_key(id),
                EntityKind::Toolset => self.toolsets_running.contains_key(id),
                _ => false,
            };
            if running {
                let c = pos2(rect.left() + 38.0, rect.center().y);
                ui.painter().circle_filled(c, 3.5, p.green_dot);
            }
            let dots_rect = Rect::from_center_size(pos2(row_highlight(rect).right() - 14.0, rect.center().y), vec2(22.0, 22.0));
            let dots = ui.interact(dots_rect, Id::unique(("entity_dots_btn", *id)), Sense::click());
            if hovered {
                ui.painter().text(dots_rect.center(), Align2::CENTER_CENTER, icons::DOTS_THREE, icon_font(16.0), if dots.hovered() { p.text } else { p.text2 });
            }
            egui::Popup::menu(&dots).id(dots_id).show(|ui| {
                for (label, act) in [("Rename", "rename"), ("Duplicate", "duplicate"), ("Delete", "delete")] {
                    if ui.button(label).clicked() {
                        action = Some((*id, act));
                        ui.close();
                    }
                }
            });
            if resp.clicked() && !renaming {
                action = Some((*id, "open"));
            }
            if resp.double_clicked() {
                action = Some((*id, "rename"));
            }
        }
        if commit {
            self.commit_rename();
        } else if cancel {
            self.rename = None;
        }
        match action {
            Some((id, "open")) => self.open_side(SideTab::Entity(kind, id)),
            Some((id, "rename")) => self.apply(TreeAction::Rename(RenameTarget::Entity(kind, id))),
            Some((id, "duplicate")) => {
                self.ws.duplicate_entity(kind, id);
            }
            Some((id, "delete")) => self.confirm = Some(Confirm::DeleteEntity(kind, id)),
            _ => {}
        }
    }

    fn history(&mut self, ui: &mut Ui) {
        let p = pal(ui);
        let filter = self.filter.trim().to_lowercase();
        let mut open: Option<Request> = None;
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new("HISTORY").font(semibold(12.0)).color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if !self.ws.history.is_empty() && pm::text_button(ui, None, "Clear all", false).clicked() {
                    self.confirm = Some(Confirm::ClearHistory);
                }
            });
        });
        let today = chrono::Local::now().date_naive();
        egui::ScrollArea::vertical().id_salt("history_scroll").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if self.ws.history.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("You haven't sent any requests").color(p.text));
                    ui.label(RichText::new("Any request you send will be saved here.").color(p.text2).font(regular(13.0)));
                });
            }
            let mut last_day = None;
            for h in &self.ws.history {
                if !filter.is_empty() && !h.request.url.to_lowercase().contains(&filter) && !h.request.name.to_lowercase().contains(&filter) {
                    continue;
                }
                let day = h.timestamp.date_naive();
                if last_day != Some(day) {
                    let label = if day == today {
                        "Today".to_string()
                    } else if Some(day) == today.pred_opt() {
                        "Yesterday".to_string()
                    } else {
                        h.timestamp.format("%B %-d").to_string()
                    };
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                    paint_text(ui, pos2(rect.left() + 16.0, rect.center().y), &label, semibold(12.0), p.text2, rect.width());
                    last_day = Some(day);
                }
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                row_bg(ui, rect, resp.hovered(), false, false, p);
                let (m, f) = pm::method_label_font(h.request.method, 10.0);
                paint_text(ui, pos2(rect.left() + 24.0, rect.center().y), &m, f, p.method(h.request.method), 40.0);
                let url = if h.request.url.is_empty() { &h.request.name } else { &h.request.url };
                paint_text(ui, pos2(rect.left() + 62.0, rect.center().y), url, regular(14.0), p.text, rect.width() - 72.0);
                let tip = format!(
                    "{}  ·  {}{}",
                    h.timestamp.format("%H:%M:%S"),
                    h.status.map(|s| s.to_string()).unwrap_or_else(|| "Error".into()),
                    h.duration_ms.map(|d| format!("  ·  {d} ms")).unwrap_or_default()
                );
                if resp.on_hover_text(tip).clicked() {
                    open = Some(h.request.clone());
                }
            }
        });
        if let Some(mut req) = open {
            req.id = Uuid::new_v4();
            self.tabs.push(Tab::new(req, None, None));
            self.active = self.tabs.len() - 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Tree drawing
// ---------------------------------------------------------------------------

fn collection_rows(ui: &mut Ui, tc: &mut TreeCtx, c: &Collection, p: &Palette) {
    let open = tc.expanded.contains(&c.id) || !tc.filter.is_empty();
    let target = RenameTarget::Collection(c.id);
    let out = tree_row(ui, tc, Id::unique(("coll", c.id)), 0, RowKind::Collection, &c.name, false, open, &[], target);
    let cid = c.id;
    if out.resp.clicked() {
        tc.actions.push(TreeAction::Toggle(cid));
        tc.actions.push(TreeAction::ClearSelection);
    }
    if out.plus {
        tc.actions.push(TreeAction::NewRequest { collection: cid, folder: None });
    }
    if let Some(d) = out.resp.dnd_release_payload::<DragItem>() {
        tc.actions.push(TreeAction::MoveInto { drag: *d, collection: cid, folder: None });
    }
    let menu = |ui: &mut Ui, actions: &mut Vec<TreeAction>| {
        let items: [(&str, TreeAction); 12] = [
            ("Add request", TreeAction::NewRequest { collection: cid, folder: None }),
            ("Add folder", TreeAction::NewFolder { collection: cid, folder: None }),
            ("Run collection", TreeAction::Run(cid)),
            ("Mock from examples", TreeAction::MockFromExamples(cid)),
            ("Edit", TreeAction::EditCollection(cid, CollSection::Overview)),
            ("Variables", TreeAction::EditCollection(cid, CollSection::Variables)),
            ("Authorization", TreeAction::EditCollection(cid, CollSection::Auth)),
            ("Scripts", TreeAction::EditCollection(cid, CollSection::Scripts)),
            ("Rename", TreeAction::Rename(RenameTarget::Collection(cid))),
            ("Duplicate", TreeAction::DuplicateCollection(cid)),
            ("Export", TreeAction::Export(cid)),
            ("Delete", TreeAction::Delete(Confirm::DeleteCollection(cid))),
        ];
        for (label, action) in items {
            if label == "Rename" || label == "Delete" || label == "Edit" {
                ui.separator();
            }
            let text = if label == "Delete" { RichText::new(label).color(p.danger) } else { RichText::new(label) };
            if ui.button(text).clicked() {
                actions.push(action);
                ui.close();
            }
        }
    };
    egui::Popup::menu(&out.dots).id(out.dots_popup).show(|ui| menu(ui, &mut tc.actions));
    out.resp.context_menu(|ui| menu(ui, &mut tc.actions));
    if open {
        let guides = [ui.max_rect().left() + 20.0];
        items_rows(ui, tc, c.id, &c.items, 1, &guides, p);
        if c.items.is_empty() && tc.filter.is_empty() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let text = "This collection is empty. Add a request";
            paint_text(ui, pos2(rect.left() + 47.0, rect.center().y), text, regular(13.0), if resp.hovered() { p.text } else { p.text2 }, rect.width() - 56.0);
            if resp.clicked() {
                tc.actions.push(TreeAction::NewRequest { collection: cid, folder: None });
            }
        }
    }
}

fn items_rows(ui: &mut Ui, tc: &mut TreeCtx, coll: Uuid, items: &[Item], depth: usize, guides: &[f32], p: &Palette) {
    for item in items {
        match item {
            Item::Folder(f) => {
                if !tc.filter.is_empty() && !f.name.to_lowercase().contains(&tc.filter) && !items_match(&f.items, &tc.filter) {
                    continue;
                }
                let open = tc.expanded.contains(&f.id) || !tc.filter.is_empty();
                let target = RenameTarget::Item { collection: coll, item: f.id };
                let fid = f.id;
                tc.rows.push((coll, fid));
                let multi = tc.in_multi((coll, fid));
                let out = tree_row(ui, tc, Id::unique(("folder", f.id)), depth, RowKind::Folder, &f.name, multi, open, guides, target);
                drag_and_drop(&out.resp, tc, coll, fid, true);
                if tc.row_clicks(ui, &out.resp, (coll, fid)) {
                    tc.actions.push(TreeAction::Toggle(fid));
                }
                if out.plus {
                    tc.actions.push(TreeAction::NewRequest { collection: coll, folder: Some(fid) });
                }
                let menu = |ui: &mut Ui, actions: &mut Vec<TreeAction>| {
                    let edit = |section| TreeAction::EditFolder { collection: coll, folder: fid, section };
                    let items: [(&str, TreeAction); 8] = [
                        ("Add request", TreeAction::NewRequest { collection: coll, folder: Some(fid) }),
                        ("Add folder", TreeAction::NewFolder { collection: coll, folder: Some(fid) }),
                        ("Edit", edit(CollSection::Overview)),
                        ("Authorization", edit(CollSection::Auth)),
                        ("Scripts", edit(CollSection::Scripts)),
                        ("Rename", TreeAction::Rename(RenameTarget::Item { collection: coll, item: fid })),
                        ("Duplicate", TreeAction::Duplicate { collection: coll, item: fid }),
                        ("Delete", TreeAction::Delete(Confirm::DeleteItem { collection: coll, item: fid })),
                    ];
                    for (label, action) in items {
                        if label == "Edit" || label == "Rename" || label == "Delete" {
                            ui.separator();
                        }
                        let text = if label == "Delete" { RichText::new(label).color(p.danger) } else { RichText::new(label) };
                        if ui.button(text).clicked() {
                            actions.push(action);
                            ui.close();
                        }
                    }
                };
                let n = tc.multi.len();
                egui::Popup::menu(&out.dots).id(out.dots_popup).show(|ui| if multi { multi_menu(ui, n, &mut tc.actions, p) } else { menu(ui, &mut tc.actions) });
                out.resp.context_menu(|ui| if multi { multi_menu(ui, n, &mut tc.actions, p) } else { menu(ui, &mut tc.actions) });
                if open {
                    let mut g = guides.to_vec();
                    g.push(ui.max_rect().left() + caret_x(depth) - 3.0);
                    items_rows(ui, tc, coll, &f.items, depth + 1, &g, p);
                }
            }
            Item::Request(r) => {
                if !tc.filter.is_empty() && !r.name.to_lowercase().contains(&tc.filter) && !r.url.to_lowercase().contains(&tc.filter) {
                    continue;
                }
                let target = RenameTarget::Item { collection: coll, item: r.id };
                let rid = r.id;
                tc.rows.push((coll, rid));
                let multi = tc.in_multi((coll, rid));
                let selected = tc.active == Some(r.id) || multi;
                let out = tree_row(ui, tc, Id::unique(("req", r.id)), depth, RowKind::Request(r.method), &r.name, selected, false, guides, target);
                drag_and_drop(&out.resp, tc, coll, rid, false);
                if tc.row_clicks(ui, &out.resp, (coll, rid)) {
                    tc.actions.push(TreeAction::Open { collection: coll, request: rid });
                }
                if out.resp.double_clicked() && !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                    tc.actions.push(TreeAction::Rename(target));
                }
                let menu = |ui: &mut Ui, actions: &mut Vec<TreeAction>| {
                    let items: [(&str, TreeAction); 4] = [
                        ("Open in tab", TreeAction::Open { collection: coll, request: rid }),
                        ("Rename", TreeAction::Rename(target)),
                        ("Duplicate", TreeAction::Duplicate { collection: coll, item: rid }),
                        ("Delete", TreeAction::Delete(Confirm::DeleteItem { collection: coll, item: rid })),
                    ];
                    for (label, action) in items {
                        if label == "Delete" {
                            ui.separator();
                        }
                        let text = if label == "Delete" { RichText::new(label).color(p.danger) } else { RichText::new(label) };
                        if ui.button(text).clicked() {
                            actions.push(action);
                            ui.close();
                        }
                    }
                };
                let n = tc.multi.len();
                egui::Popup::menu(&out.dots).id(out.dots_popup).show(|ui| if multi { multi_menu(ui, n, &mut tc.actions, p) } else { menu(ui, &mut tc.actions) });
                let url = r.url.clone();
                let resp = if url.is_empty() { out.resp } else { out.resp.on_hover_text_at_pointer(url) };
                resp.context_menu(|ui| if multi { multi_menu(ui, n, &mut tc.actions, p) } else { menu(ui, &mut tc.actions) });
            }
        }
    }
}

fn drag_and_drop(resp: &Response, tc: &mut TreeCtx, coll: Uuid, id: Uuid, is_folder: bool) {
    if resp.drag_started() {
        resp.dnd_set_drag_payload(DragItem { collection: coll, item: id });
    }
    if let Some(d) = resp.dnd_release_payload::<DragItem>() {
        tc.actions.push(if is_folder {
            TreeAction::MoveInto { drag: *d, collection: coll, folder: Some(id) }
        } else {
            TreeAction::MoveAfter { drag: *d, collection: coll, after: id }
        });
    }
}

/// x offset (from the sidebar's left edge) of the caret center at a given depth.
fn caret_x(depth: usize) -> f32 {
    37.0 + 10.0 * depth as f32
}

/// Highlight area of a sidebar row: full height, 9px from the left edge and 8px from the right.
fn row_highlight(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.left() + 9.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom()))
}

fn row_bg(ui: &Ui, rect: Rect, hovered: bool, selected: bool, focused: bool, p: &Palette) {
    let r = row_highlight(rect);
    if selected || hovered || focused {
        ui.painter().rect_filled(r, 4.0, p.row_hover);
    }
    if focused {
        ui.painter().rect_stroke(r, 4.0, Stroke::new(2.0, p.focus_ring), StrokeKind::Inside);
    }
}

/// Inline rename field. Returns (commit, cancel).
fn rename_edit(ui: &mut Ui, rect: Rect, id: Id, text: &mut String, focus: &mut bool) -> (bool, bool) {
    let p = pal(ui);
    ui.painter().rect(rect, 4.0, p.bg, Stroke::new(1.0, p.focus), StrokeKind::Inside);
    let resp = ui.put(
        rect.shrink2(vec2(6.0, 0.0)),
        egui::TextEdit::singleline(text).id(id).frame(egui::Frame::NONE).align(egui::Align2::LEFT_CENTER).desired_width(rect.width() - 12.0),
    );
    if *focus {
        resp.request_focus();
        *focus = false;
    }
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if esc {
        return (false, true);
    }
    (resp.lost_focus() || (resp.has_focus() && enter), false)
}

#[allow(clippy::too_many_arguments)]
fn tree_row(
    ui: &mut Ui,
    tc: &mut TreeCtx,
    id: Id,
    depth: usize,
    kind: RowKind,
    label: &str,
    selected: bool,
    open: bool,
    guides: &[f32],
    target: RenameTarget,
) -> RowOut {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
    let dots_popup = id.with("dots_popup");
    let menu_open = egui::Popup::is_id_open(ui.ctx(), dots_popup);
    #[allow(unused_mut)]
    let mut hovered = ui.rect_contains_pointer(rect) || menu_open;
    #[allow(unused_mut)]
    let mut focused = tc.focus == Some(id);
    #[cfg(debug_assertions)]
    {
        // Dev-only: force states for screenshots (RUSTMAN_HOVER_LABEL / RUSTMAN_FOCUS_LABEL).
        hovered |= std::env::var("RUSTMAN_HOVER_LABEL").is_ok_and(|v| v == label);
        focused |= std::env::var("RUSTMAN_FOCUS_LABEL").is_ok_and(|v| v == label);
    }
    let dragging_over = resp.dnd_hover_payload::<DragItem>().is_some();
    // Indent guides first: the row highlight covers them, like Postman.
    for g in guides {
        ui.painter().vline(*g, rect.y_range(), Stroke::new(1.0, p.input_border));
    }
    row_bg(ui, rect, hovered, selected, focused, p);
    if dragging_over {
        if matches!(kind, RowKind::Request(_)) {
            ui.painter().hline(rect.shrink2(vec2(8.0, 0.0)).x_range(), rect.bottom() - 1.0, Stroke::new(2.0, p.accent));
        } else {
            ui.painter().rect_stroke(rect.shrink2(vec2(8.0, 1.0)), 4.0, Stroke::new(1.5, p.accent), StrokeKind::Inside);
        }
    }
    let x0 = rect.left();
    let cy = rect.center().y;
    let base = 10.0 * depth.saturating_sub(1) as f32;
    let text_x = match kind {
        RowKind::Collection => {
            let glyph = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
            ui.painter().text(pos2(x0 + caret_x(0), cy), Align2::CENTER_CENTER, glyph, icon_font(12.0), p.text2);
            x0 + 47.0
        }
        RowKind::Folder => {
            let glyph = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
            ui.painter().text(pos2(x0 + caret_x(depth), cy), Align2::CENTER_CENTER, glyph, icon_font(12.0), p.text2);
            ui.painter().text(pos2(x0 + 64.0 + base, cy), Align2::CENTER_CENTER, icons::FOLDER_SIMPLE, icon_font(16.0), p.text2);
            x0 + 79.0 + base
        }
        RowKind::Request(m) => {
            let (s, f) = pm::method_label_font(m, 9.5);
            let w = paint_text(ui, pos2(x0 + 56.0 + base, cy + 0.5), &s, f, p.method(m), 40.0);
            (x0 + 79.0 + base).max(x0 + 56.0 + base + w + 3.0)
        }
    };
    let has_plus = !matches!(kind, RowKind::Request(_));
    let icons_w = if hovered { if has_plus { 62.0 } else { 36.0 } } else { 14.0 };
    let renaming = matches!(tc.rename, Some((t, _)) if *t == target);
    if renaming {
        let text_rect = Rect::from_min_max(pos2(text_x - 6.0, rect.top() + 3.0), pos2(rect.right() - 12.0, rect.bottom() - 3.0));
        if let Some((_, name)) = tc.rename.as_mut() {
            let (c, x) = rename_edit(ui, text_rect, id.with("rename"), name, tc.rename_focus);
            tc.commit |= c;
            tc.cancel |= x;
        }
    } else {
        paint_text(ui, pos2(text_x, cy), label, regular(14.0), p.text, rect.right() - icons_w - text_x);
    }

    // Hover actions: "+" and "...".
    let hi = row_highlight(rect);
    let dots_rect = Rect::from_center_size(pos2(hi.right() - 14.0, cy), vec2(22.0, 22.0));
    let plus_rect = Rect::from_center_size(pos2(hi.right() - 40.0, cy), vec2(22.0, 22.0));
    let dots = ui.interact(dots_rect, id.with("dots"), Sense::click());
    let mut plus = false;
    if hovered && !renaming {
        if dots.hovered() {
            ui.painter().rect_filled(dots_rect, 4.0, p.active);
        }
        ui.painter().text(dots_rect.center(), Align2::CENTER_CENTER, icons::DOTS_THREE, icon_font(16.0), if dots.hovered() { p.text } else { p.text2 });
        if has_plus {
            let pr = ui.interact(plus_rect, id.with("plus"), Sense::click());
            if pr.hovered() {
                ui.painter().rect_filled(plus_rect, 4.0, p.active);
            }
            ui.painter().text(plus_rect.center(), Align2::CENTER_CENTER, icons::PLUS, icon_font(15.0), if pr.hovered() { p.text } else { p.text2 });
            plus = pr.on_hover_text("Add request").clicked();
        }
    }
    if resp.clicked() || resp.secondary_clicked() || dots.clicked() || plus {
        tc.clicked_row = Some(id);
    }
    RowOut { resp, plus, dots, dots_popup }
}

/// Section header: `> COLLECTIONS` with an optional "+" on hover. Returns (response, plus_clicked).
/// `line_above` draws the separator from the previous section.
fn section_header(ui: &mut Ui, label: &str, open: bool, with_plus: bool, line_above: bool) -> (Response, bool) {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    if line_above {
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 0.0, p.row_hover);
    }
    let glyph = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
    ui.painter().text(pos2(rect.left() + 26.0, rect.center().y), Align2::CENTER_CENTER, glyph, icon_font(12.0), p.text);
    paint_text(ui, pos2(rect.left() + 38.0, rect.center().y), label, semibold(12.0), p.text, rect.width() - 80.0);
    let mut plus = false;
    if with_plus && ui.rect_contains_pointer(rect) {
        let r = Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(24.0, 24.0));
        let pr = ui.interact(r, Id::unique(("section_plus", label)), Sense::click());
        if pr.hovered() {
            ui.painter().rect_filled(r, 4.0, p.active);
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, icons::PLUS, icon_font(15.0), if pr.hovered() { p.text } else { p.text2 });
        plus = pr.clicked();
    }
    (resp, plus)
}

/// Big mode icon at the top of the sidebar.
fn mode_icon(ui: &mut Ui, glyph: &str, selected: bool, tip: &str) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(48.0, 30.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 6.0, p.active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, p.hover);
    }
    let color = if selected || resp.hovered() { p.text } else { p.text2 };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, icon_font(19.0), color);
    resp.on_hover_text(tip)
}

/// Popup id of the "+" (New) menu next to the sidebar filter.
pub(super) const NEW_MENU: &str = "sb_new_menu";

#[derive(Clone, Copy, PartialEq)]
enum NewKind {
    Http,
    Collection,
    Environment,
    Entity(EntityKind),
    /// Listed like Postman, but not implemented in Rustman (shown disabled).
    Unsupported,
}

struct NewEntry {
    glyph: &'static str,
    label: &'static str,
    color: Option<Color32>,
    kind: NewKind,
    shortcut: &'static str,
}

const fn entry(glyph: &'static str, label: &'static str, color: Option<Color32>, kind: NewKind) -> NewEntry {
    NewEntry { glyph, label, color, kind, shortcut: "" }
}

/// Groups of the New menu, in Postman order.
fn new_entries() -> [Vec<NewEntry>; 3] {
    use NewKind::*;
    let rgb = |r, g, b| Some(Color32::from_rgb(r, g, b));
    [
        vec![NewEntry { shortcut: "Ctrl+Alt+P", ..entry(icons::SPARKLE, "Ask AI", None, Unsupported) }],
        vec![
            entry(icons::ARROWS_LEFT_RIGHT, "HTTP", rgb(0x0f, 0x8f, 0xa8), Http),
            entry(icons::HEXAGON, "GraphQL", rgb(0xe1, 0x00, 0x98), Unsupported),
            entry(icons::SPARKLE, "AI", rgb(0x0c, 0xa3, 0x6b), Unsupported),
            entry(icons::PLUGS_CONNECTED, "MCP", None, Unsupported),
            entry(icons::BRACKETS_ANGLE, "gRPC", rgb(0x24, 0x6b, 0xd6), Unsupported),
            entry(icons::PLUGS, "WebSocket", rgb(0xf0, 0x5a, 0x24), Unsupported),
            entry(icons::LIGHTNING, "Socket.IO", rgb(0xe0, 0x3a, 0x2f), Unsupported),
            entry(icons::RSS, "MQTT", rgb(0x82, 0x4c, 0xd6), Unsupported),
            entry(icons::DATABASE, "Data", rgb(0xd6, 0x3a, 0x6b), Unsupported),
            entry(icons::GRAPH, "Kafka", None, Unsupported),
        ],
        vec![
            entry(icons::FOLDER_SIMPLE, "Collection", None, Collection),
            entry(icons::STACK, "Environment", None, Environment),
            entry(icons::MARKDOWN_LOGO, "Document", None, Entity(EntityKind::Document)),
            entry(icons::FILE_CODE, "Spec", None, Entity(EntityKind::Spec)),
            entry(icons::HARD_DRIVES, "Mock", None, Entity(EntityKind::Mock)),
            entry(icons::CLOUD, "Mock Server", None, Unsupported),
            entry(icons::PULSE, "Monitor", None, Unsupported),
            entry(icons::WEBHOOKS_LOGO, "Webhook", None, Unsupported),
            entry(icons::FLOW_ARROW, "Flow", None, Unsupported),
            entry(icons::TABLE, "Dataset", None, Entity(EntityKind::Dataset)),
            entry(icons::TOOLBOX, "AI Toolset", None, Entity(EntityKind::Toolset)),
        ],
    ]
}

/// Contents of the New menu: a search box, then the entries grouped with separators.
/// Returns what was picked (Enter picks the first available match).
fn new_menu(ui: &mut Ui) -> Option<NewKind> {
    let p = pal(ui);
    let id = Id::unique(NEW_MENU);
    let q_id = id.with("q");
    let search_id = id.with("search");
    let mut q = ui.data(|d| d.get_temp::<String>(q_id));
    let first_frame = q.is_none();
    let mut text = q.take().unwrap_or_default();
    ui.spacing_mut().item_spacing.y = 0.0;
    let r = pm::search_box_icon(ui, search_id, &mut text, "Search...", ui.available_width(), 30.0, icons::MAGNIFYING_GLASS);
    if first_frame {
        r.request_focus();
    }
    let needle = text.trim().to_lowercase();
    ui.add_space(4.0);

    let mut picked = None;
    let mut first_match = None;
    let mut any_shown = false;
    for group in new_entries() {
        let shown: Vec<&NewEntry> = group.iter().filter(|e| needle.is_empty() || e.label.to_lowercase().contains(&needle)).collect();
        if shown.is_empty() {
            continue;
        }
        if any_shown {
            ui.add_space(4.0);
            pm::hline(ui);
            ui.add_space(4.0);
        }
        any_shown = true;
        for e in shown {
            let enabled = e.kind != NewKind::Unsupported;
            if enabled && first_match.is_none() {
                first_match = Some(e.kind);
            }
            if new_menu_row(ui, e, enabled, p).clicked() && enabled {
                picked = Some(e.kind);
            }
        }
    }
    if !any_shown {
        ui.add_space(8.0);
        ui.label(RichText::new("No matches").color(p.text2).font(regular(13.0)));
        ui.add_space(8.0);
    }
    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        picked = picked.or(first_match);
    }
    ui.data_mut(|d| d.insert_temp(q_id, text));
    if picked.is_some() {
        ui.close();
    }
    picked
}

fn new_menu_row(ui: &mut Ui, e: &NewEntry, enabled: bool, p: &Palette) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.hover);
    }
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    let icon_color = dim(e.color.unwrap_or(p.text));
    ui.painter().text(pos2(rect.left() + 16.0, rect.center().y), Align2::CENTER_CENTER, e.glyph, icon_font(15.0), icon_color);
    let text_color = if enabled { p.text } else { p.text3 };
    paint_text(ui, pos2(rect.left() + 36.0, rect.center().y), e.label, regular(14.0), text_color, rect.width() - 120.0);
    if !e.shortcut.is_empty() {
        ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, e.shortcut, regular(12.0), p.text3);
    }
    if enabled { resp } else { resp.on_hover_text("Not available in Rustman yet") }
}
