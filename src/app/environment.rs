//! Environment / Globals editor shown as a tab in the main area (like Postman).

use eframe::egui::{self, Align2, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::*;
use crate::icons;
use crate::pm::{self, TableOpts, paint_text};
use crate::theme::{UI_SIZE, icon_font, pal, regular, semibold};

impl RustmanApp {
    pub(super) fn env_view(&mut self, ui: &mut Ui, target: EnvTarget) {
        let name = match target {
            EnvTarget::Globals => "Globals".to_string(),
            EnvTarget::Env(id) => match self.ws.environments.iter().find(|e| e.id == id) {
                Some(e) => e.name.clone(),
                None => {
                    self.close_side_tab();
                    return;
                }
            },
        };
        self.env_header(ui, target, name);
        ui.add_space(4.0);

        let vars = self.vars_for(None);
        let opts = TableOpts { key_label: "Variable", with_desc: false, key_hint: "Add variable", value_hint: "", ..Default::default() };
        egui::ScrollArea::vertical().id_salt("env_view_scroll").auto_shrink(false).show(ui, |ui| match target {
            EnvTarget::Globals => {
                pm::kv_table(ui, Id::unique("globals"), &mut self.ws.globals, &vars, opts);
            }
            EnvTarget::Env(id) => {
                if let Some(env) = self.ws.environments.iter_mut().find(|e| e.id == id) {
                    pm::kv_table(ui, Id::unique(("env", id)), &mut env.variables, &vars, opts);
                }
            }
        });
    }

    /// Title row: active toggle, editable name, actions.
    fn env_header(&mut self, ui: &mut Ui, target: EnvTarget, name: String) {
        let p = pal(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        let cy = rect.center().y;
        let env_id = match target {
            EnvTarget::Env(id) => Some(id),
            EnvTarget::Globals => None,
        };
        let active = env_id.is_some() && self.ws.active_environment == env_id;

        // Leading icon: globe for Globals, active-environment check otherwise.
        let lead = Rect::from_center_size(pos2(rect.left() + 10.0, cy), vec2(22.0, 22.0));
        match env_id {
            None => {
                ui.painter().text(lead.center(), Align2::CENTER_CENTER, icons::GLOBE_SIMPLE, icon_font(16.0), p.text2);
            }
            Some(id) => {
                let r = ui.interact(lead, Id::unique(("env_view_check", id)), Sense::click());
                let color = if active { p.green_dot } else if r.hovered() { p.text } else { p.text3 };
                ui.painter().text(lead.center(), Align2::CENTER_CENTER, icons::CHECK_CIRCLE, icon_font(17.0), color);
                if r.on_hover_text(if active { "Active environment (click to deactivate)" } else { "Set as active environment" }).clicked() {
                    self.ws.active_environment = if active { None } else { Some(id) };
                }
            }
        }

        // Name (click to rename).
        let x = rect.left() + 32.0;
        let max_name_w = (rect.width() - 260.0).max(120.0);
        let name_w = pm::text_width(ui, &name, semibold(15.0)).min(max_name_w);
        let name_rect = Rect::from_min_max(pos2(x - 6.0, cy - 14.0), pos2(x + name_w + 6.0, cy + 14.0));
        let renaming = env_id.is_some() && matches!(self.rename, Some((RenameTarget::EnvTitle(r), _)) if Some(r) == env_id);
        if renaming {
            let edit_rect = Rect::from_min_max(name_rect.min, pos2(name_rect.left() + max_name_w.min(360.0), name_rect.bottom()));
            ui.painter().rect(edit_rect, 4.0, p.bg, Stroke::new(1.0, p.focus), StrokeKind::Inside);
            let mut commit = false;
            let mut cancel = false;
            if let Some((_, text)) = &mut self.rename {
                let resp = ui.put(
                    edit_rect.shrink2(vec2(6.0, 0.0)),
                    egui::TextEdit::singleline(text).id(Id::unique("env_title_rename")).frame(egui::Frame::NONE).font(semibold(15.0)).align(Align2::LEFT_CENTER),
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
        } else if let Some(id) = env_id {
            let resp = ui.interact(name_rect, Id::unique("env_title"), Sense::click());
            if resp.hovered() {
                ui.painter().rect_stroke(name_rect, 4.0, Stroke::new(1.0, p.input_border), StrokeKind::Inside);
            }
            paint_text(ui, pos2(x, cy), &name, semibold(15.0), p.text, name_w);
            if resp.on_hover_text("Rename").clicked() {
                self.start_rename(RenameTarget::EnvTitle(id), name.clone());
            }
        } else {
            paint_text(ui, pos2(x, cy), &name, semibold(15.0), p.text, name_w);
        }

        // Right side actions.
        let Some(id) = env_id else {
            let hint = "Available in every request; environment and collection variables override them.";
            let w = pm::text_width(ui, hint, regular(12.5)).min(rect.width() - name_w - 60.0).max(0.0);
            paint_text(ui, pos2(rect.right() - w, cy), hint, regular(12.5), p.text3, w);
            return;
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
        child.spacing_mut().item_spacing.x = 6.0;
        let dots = pm::icon_button(&mut child, icons::DOTS_THREE, 16.0, "More actions");
        let mut action = None;
        egui::Popup::menu(&dots).show(|ui| {
            for (label, act) in [("Rename", "rename"), ("Duplicate", "duplicate"), ("Export", "export"), ("Delete", "delete")] {
                if ui.button(label).clicked() {
                    action = Some(act);
                    ui.close();
                }
            }
        });
        if active {
            let (r, _) = child.allocate_exact_size(vec2(70.0, 30.0), Sense::hover());
            paint_text(&child, pos2(r.left() + 6.0, r.center().y), "Active", regular(UI_SIZE), p.green_dot, 60.0);
        } else if pm::secondary_button(&mut child, "Set active").clicked() {
            self.ws.active_environment = Some(id);
        }
        match action {
            Some("rename") => self.start_rename(RenameTarget::EnvTitle(id), name),
            Some("duplicate") => {
                if let Some(e) = self.ws.environments.iter().find(|e| e.id == id).cloned() {
                    let copy = Environment { id: Uuid::new_v4(), name: format!("{} Copy", e.name), ..e };
                    let new_id = copy.id;
                    self.ws.environments.push(copy);
                    self.open_env(EnvTarget::Env(new_id));
                }
            }
            Some("export") => self.export_env(id),
            Some("delete") => self.confirm = Some(Confirm::DeleteEnv(id)),
            _ => {}
        }
    }
}
