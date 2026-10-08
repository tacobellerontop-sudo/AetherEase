//! Menu bar and the "add layer" toolbar.

use egui::{RichText, Ui};

use crate::app::AetherApp;
use crate::model::ShapeKind;
use crate::ui::theme;

impl AetherApp {
    pub fn menu_bar_ui(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Home").clicked() {
                    self.go_home();
                }
                ui.separator();
                if ui.button("New project…").clicked() {
                    self.new_project();
                }
                if ui.button("Open…").clicked() {
                    self.open_dialog();
                }
                ui.separator();
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("Save as…").clicked() {
                    self.save_as();
                }
                ui.separator();
                if ui.button("Import image…").clicked() {
                    self.import_image(&ui.ctx().clone());
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new("Undo"))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new("Redo"))
                    .clicked()
                {
                    self.redo();
                }
                ui.separator();
                let has_layer = self.selected.is_some();
                if ui
                    .add_enabled(has_layer, egui::Button::new("Duplicate layer"))
                    .clicked()
                {
                    self.duplicate_selected();
                }
                if ui
                    .add_enabled(
                        has_layer || self.selected_key.is_some(),
                        egui::Button::new("Delete"),
                    )
                    .clicked()
                {
                    self.delete_selection();
                }
            });
            ui.menu_button("Layer", |ui| self.add_layer_menu(ui));
            ui.menu_button("Help", |ui| {
                ui.label("Shortcuts");
                ui.separator();
                for (keys, action) in [
                    ("Space", "Play / pause"),
                    ("← / →", "Previous / next frame"),
                    ("Home / End", "First / last frame"),
                    ("Delete", "Delete layer or keyframe"),
                    ("Ctrl+D", "Duplicate layer"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                    ("Ctrl+S", "Save"),
                    ("Scroll", "Zoom the canvas"),
                    ("Middle drag", "Pan the canvas"),
                    ("Shift (dragging)", "Uniform scale, 15° rotation steps"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(keys).strong());
                        ui.label(action);
                    });
                }
            });
        });
    }

    /// Entries for adding layers; shared by the menu bar and the timeline's
    /// "+" button.
    pub fn add_layer_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Shape", |ui| {
            for (name, shape) in ShapeKind::PRESETS {
                if ui.button(name).clicked() {
                    self.add_shape(shape);
                }
            }
        });
        if ui.button("Text").clicked() {
            self.add_text();
        }
        if ui.button("Image…").clicked() {
            self.import_image(&ui.ctx().clone());
        }
    }

    pub fn toolbar_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("⏴ Home").strong().color(theme::ACCENT))
                .on_hover_text("Back to your projects")
                .clicked()
            {
                self.go_home();
            }
            ui.separator();
            ui.label("Add");
            for (name, shape) in ShapeKind::PRESETS {
                if ui
                    .button(name)
                    .on_hover_text(format!("Add a {} layer", name.to_lowercase()))
                    .clicked()
                {
                    self.add_shape(shape);
                }
            }
            if ui
                .button("Text")
                .on_hover_text("Add a text layer")
                .clicked()
            {
                self.add_text();
            }
            if ui
                .button("Image…")
                .on_hover_text("Import an image as a layer")
                .clicked()
            {
                self.import_image(&ui.ctx().clone());
            }
            ui.separator();
            if ui
                .add_enabled(self.history.can_undo(), egui::Button::new("⟲"))
                .on_hover_text("Undo (Ctrl+Z)")
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new("⟳"))
                .on_hover_text("Redo (Ctrl+Y)")
                .clicked()
            {
                self.redo();
            }
            if let Some(status) = self.status.clone() {
                ui.separator();
                ui.colored_label(theme::PLAYHEAD, status);
                if ui.small_button("✕").clicked() {
                    self.status = None;
                }
            }
        });
    }
}
