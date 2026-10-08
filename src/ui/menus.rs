//! The editor's top bar and the add-layer menu.

use egui::{Align, Layout, RectAlign, RichText, Ui, vec2};

use crate::app::AetherApp;
use crate::model::ShapeKind;
use crate::ui::icons::{self, Icon};
use crate::ui::theme;

const SHORTCUTS: [(&str, &str); 11] = [
    ("Space", "Play / pause"),
    ("← / →", "Previous / next frame"),
    ("Home / End", "First / last frame"),
    ("Delete", "Delete layer or keyframe"),
    ("Ctrl+D", "Duplicate layer"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+S", "Save"),
    ("Ctrl+N", "New project"),
    ("Scroll", "Zoom the canvas"),
    ("Middle drag", "Pan the canvas"),
    ("Shift + drag", "Uniform scale, 15° rotation steps"),
];

impl AetherApp {
    /// One slim bar: back to home and the project name on the left, undo and
    /// redo in the middle, everything else behind a "more" menu on the right.
    pub fn top_bar_ui(&mut self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            if icons::button(ui, Icon::Back, "Back to your projects").clicked() {
                self.go_home();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.project.name)
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::proportional(16.0))
                    .desired_width(260.0),
            )
            .on_hover_text("Rename project");
            let saved = match (&self.path, self.is_dirty()) {
                (None, _) => "Not saved",
                (Some(_), true) => "Saving…",
                (Some(_), false) => "Saved",
            };
            ui.label(RichText::new(saved).small().weak());

            if let Some(status) = self.status.clone() {
                ui.add_space(12.0);
                ui.colored_label(theme::PLAYHEAD, status);
                if icons::icon_button(ui, Icon::Plus, "Dismiss", false, true, 20.0).clicked() {
                    self.status = None;
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let more = icons::button(ui, Icon::More, "More");
                egui::Popup::menu(&more)
                    .align(RectAlign::BOTTOM_END)
                    .show(|ui| self.more_menu(ui));
                ui.add_space(8.0);
                // Undo and redo sit together near the right edge.
                if icons::icon_button(
                    ui,
                    Icon::Redo,
                    "Redo (Ctrl+Y)",
                    false,
                    self.history.can_redo(),
                    30.0,
                )
                .clicked()
                {
                    self.redo();
                }
                if icons::icon_button(
                    ui,
                    Icon::Undo,
                    "Undo (Ctrl+Z)",
                    false,
                    self.history.can_undo(),
                    30.0,
                )
                .clicked()
                {
                    self.undo();
                }
            });
        });
    }

    fn more_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(200.0);
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
        if ui.button("Save a copy as…").clicked() {
            self.save_as();
        }
        ui.separator();
        if ui.button("Import image…").clicked() {
            self.import_image(&ui.ctx().clone());
        }
        ui.separator();
        ui.menu_button("Keyboard shortcuts", |ui| {
            egui::Grid::new("shortcuts")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    for (keys, action) in SHORTCUTS {
                        ui.label(RichText::new(keys).strong());
                        ui.label(action);
                        ui.end_row();
                    }
                });
        });
    }

    /// The round "+" button and its grid of layer types, Alight Motion style.
    pub fn add_layer_button(&mut self, ui: &mut Ui, rect: egui::Rect) {
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                let (rect, response) = ui.allocate_exact_size(rect.size(), egui::Sense::click());
                let fill = if response.hovered() {
                    theme::ACCENT
                } else {
                    theme::ACCENT_SOFT
                };
                ui.painter()
                    .circle_filled(rect.center(), rect.width() * 0.5, fill);
                icons::paint(
                    ui.painter(),
                    rect.shrink(rect.width() * 0.3),
                    Icon::Plus,
                    egui::Color32::WHITE,
                );
                response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Add a layer")
            })
            .inner;
        egui::Popup::menu(&response)
            .align(RectAlign::TOP_END)
            .gap(8.0)
            .show(|ui| self.add_layer_grid(ui));
    }

    fn add_layer_grid(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Add layer").strong());
        ui.add_space(4.0);
        let size = vec2(72.0, 64.0);
        let shape_icons = [
            Icon::Rectangle,
            Icon::Ellipse,
            Icon::Triangle,
            Icon::Hexagon,
            Icon::Star,
        ];
        ui.horizontal(|ui| {
            for ((name, shape), icon) in ShapeKind::PRESETS.into_iter().zip(shape_icons).take(4) {
                if icons::tile(ui, icon, name, false, size).clicked() {
                    self.add_shape(shape);
                }
            }
        });
        ui.horizontal(|ui| {
            let (name, star) = ShapeKind::PRESETS[4];
            if icons::tile(ui, Icon::Star, name, false, size).clicked() {
                self.add_shape(star);
            }
            if icons::tile(ui, Icon::Text, "Text", false, size).clicked() {
                self.add_text();
            }
            if icons::tile(ui, Icon::Image, "Image", false, size).clicked() {
                self.import_image(&ui.ctx().clone());
            }
        });
    }
}
