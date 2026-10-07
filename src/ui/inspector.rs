//! The right-hand panel: properties of the selected layer, or of the project
//! when nothing is selected.

use egui::{CollapsingHeader, Color32, DragValue, Grid, RichText, Sense, Stroke, Ui, Vec2};

use crate::app::AetherApp;
use crate::model::anim::Lerp;
use crate::model::{Animated, Color, Easing, KeyTrack, Layer, LayerKind, ShapeKind};
use crate::ui::theme;
use crate::ui::timeline::diamond;

const RESOLUTIONS: [(&str, u32, u32); 6] = [
    ("1080p landscape (16:9)", 1920, 1080),
    ("1080p portrait (9:16)", 1080, 1920),
    ("Square (1:1)", 1080, 1080),
    ("4K UHD", 3840, 2160),
    ("720p", 1280, 720),
    ("Portrait 4:5", 1080, 1350),
];
const FRAME_RATES: [u32; 6] = [12, 24, 25, 30, 50, 60];

impl AetherApp {
    pub fn inspector_ui(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| match self.selected {
                Some(id) if self.project.layer(id).is_some() => self.layer_inspector(ui, id),
                _ => self.project_inspector(ui),
            });
    }

    fn project_inspector(&mut self, ui: &mut Ui) {
        ui.heading("Project");
        ui.add_space(4.0);
        let fps_before = self.project.fps;
        let project = &mut self.project;
        Grid::new("project_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut project.name);
                ui.end_row();

                ui.label("Preset");
                let current = RESOLUTIONS
                    .iter()
                    .find(|(_, w, h)| *w == project.width && *h == project.height)
                    .map_or("Custom", |(name, ..)| name);
                egui::ComboBox::from_id_salt("resolution")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (name, w, h) in RESOLUTIONS {
                            if ui
                                .selectable_label(project.width == w && project.height == h, name)
                                .clicked()
                            {
                                project.width = w;
                                project.height = h;
                            }
                        }
                    });
                ui.end_row();

                ui.label("Size");
                ui.horizontal(|ui| {
                    ui.add(
                        DragValue::new(&mut project.width)
                            .range(16..=8192)
                            .suffix(" px"),
                    );
                    ui.label("×");
                    ui.add(
                        DragValue::new(&mut project.height)
                            .range(16..=8192)
                            .suffix(" px"),
                    );
                });
                ui.end_row();

                ui.label("Frame rate");
                egui::ComboBox::from_id_salt("fps")
                    .selected_text(format!("{} fps", project.fps))
                    .show_ui(ui, |ui| {
                        for fps in FRAME_RATES {
                            ui.selectable_value(&mut project.fps, fps, format!("{fps} fps"));
                        }
                    });
                ui.end_row();

                ui.label("Duration");
                let mut seconds = project.duration as f32 / fps_before as f32;
                if ui
                    .add(
                        DragValue::new(&mut seconds)
                            .range(0.1..=3600.0)
                            .speed(0.1)
                            .suffix(" s"),
                    )
                    .changed()
                {
                    project.duration = (seconds * fps_before as f32).round().max(1.0) as i32;
                }
                ui.end_row();

                ui.label("Background");
                color_edit(ui, &mut project.background);
                ui.end_row();
            });

        // Keep the same length in seconds when the frame rate changes.
        if self.project.fps != fps_before {
            let ratio = self.project.fps as f32 / fps_before as f32;
            self.project.duration = (self.project.duration as f32 * ratio).round().max(1.0) as i32;
            self.frame = (self.frame as f32 * ratio).round() as i32;
        }
        self.set_frame(self.frame);

        ui.add_space(12.0);
        ui.label(
            RichText::new("Select a layer on the canvas or in the timeline to edit it.").weak(),
        );
    }

    fn layer_inspector(&mut self, ui: &mut Ui, id: u64) {
        let frame = self.frame;
        let duration = self.project.duration;
        let mut action = None;

        if let Some(key) = self.selected_key
            && let Some(track) = self
                .project
                .layer_mut(key.layer)
                .and_then(|l| l.track_mut(key.prop))
            && let Some(mut easing) = track.easing(key.frame)
        {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                diamond(
                    ui.painter(),
                    rect.center(),
                    5.0,
                    theme::KEYFRAME,
                    Stroke::NONE,
                );
                ui.label(format!("{} key at frame {}", key.prop.label(), key.frame));
            });
            ui.horizontal(|ui| {
                ui.label("Easing");
                egui::ComboBox::from_id_salt("key_easing")
                    .selected_text(easing.label())
                    .show_ui(ui, |ui| {
                        for e in Easing::ALL {
                            ui.selectable_value(&mut easing, e, e.label());
                        }
                    });
            });
            track.set_easing(key.frame, easing);
            ui.separator();
        }

        let Some(layer) = self.project.layer_mut(id) else {
            return;
        };

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(theme::layer_icon(&layer.kind))
                    .color(theme::layer_color(&layer.kind)),
            );
            ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(f32::INFINITY));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut layer.visible, "Visible");
            ui.checkbox(&mut layer.locked, "Locked");
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Duplicate").clicked() {
                action = Some(LayerAction::Duplicate);
            }
            if ui.button("Delete").clicked() {
                action = Some(LayerAction::Delete);
            }
            if ui
                .button("Forward")
                .on_hover_text("Move up the layer stack")
                .clicked()
            {
                action = Some(LayerAction::Reorder(1));
            }
            if ui
                .button("Backward")
                .on_hover_text("Move down the layer stack")
                .clicked()
            {
                action = Some(LayerAction::Reorder(-1));
            }
        });
        ui.add_space(4.0);

        section(ui, "Move & Transform", |ui| {
            transform_section(ui, layer, frame)
        });
        match &mut layer.kind {
            LayerKind::Shape {
                shape,
                size,
                corner_radius,
            } => section(ui, "Shape", |ui| {
                shape_section(ui, shape, size, corner_radius, frame)
            }),
            LayerKind::Text { text, font_size } => section(ui, "Text", |ui| {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    ui.label("Font size");
                    ui.add(
                        DragValue::new(font_size)
                            .range(1.0..=1000.0)
                            .speed(0.5)
                            .suffix(" px"),
                    );
                });
            }),
            LayerKind::Image { path, size } => section(ui, "Image", |ui| {
                ui.label(RichText::new(path.display().to_string()).small().weak());
                ui.label(format!("{} × {} px", size.x, size.y));
            }),
        }
        section(ui, "Color & Fill", |ui| {
            Grid::new("fill_grid").num_columns(3).show(ui, |ui| {
                let label = if matches!(layer.kind, LayerKind::Image { .. }) {
                    "Tint"
                } else {
                    "Color"
                };
                anim_row(ui, label, &mut layer.fill, frame, color_edit);
            });
        });
        if matches!(layer.kind, LayerKind::Shape { .. }) {
            section(ui, "Border", |ui| {
                ui.checkbox(&mut layer.border.enabled, "Draw border");
                ui.add_enabled_ui(layer.border.enabled, |ui| {
                    Grid::new("border_grid").num_columns(3).show(ui, |ui| {
                        anim_row(ui, "Width", &mut layer.border.width, frame, |ui, v| {
                            ui.add(
                                DragValue::new(v)
                                    .range(0.0..=500.0)
                                    .speed(0.2)
                                    .suffix(" px"),
                            )
                            .changed()
                        });
                        anim_row(ui, "Color", &mut layer.border.color, frame, color_edit);
                    });
                });
            });
        }
        section(ui, "Timing", |ui| {
            Grid::new("timing_grid").num_columns(2).show(ui, |ui| {
                ui.label("In");
                ui.add(
                    DragValue::new(&mut layer.in_frame)
                        .range(-duration..=layer.out_frame - 1)
                        .suffix(" f"),
                );
                ui.end_row();
                ui.label("Out");
                ui.add(
                    DragValue::new(&mut layer.out_frame)
                        .range(layer.in_frame + 1..=duration * 4)
                        .suffix(" f"),
                );
                ui.end_row();
            });
        });

        match action {
            Some(LayerAction::Duplicate) => self.duplicate_selected(),
            Some(LayerAction::Delete) => {
                self.selected_key = None;
                self.delete_selection();
            }
            Some(LayerAction::Reorder(delta)) => self.project.reorder_layer(id, delta),
            None => {}
        }
    }
}

enum LayerAction {
    Duplicate,
    Delete,
    Reorder(isize),
}

fn section(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    CollapsingHeader::new(RichText::new(title).strong())
        .default_open(true)
        .show(ui, add_contents);
}

fn transform_section(ui: &mut Ui, layer: &mut Layer, frame: i32) {
    Grid::new("transform_grid").num_columns(3).show(ui, |ui| {
        let t = &mut layer.transform;
        anim_row(ui, "Position", &mut t.position, frame, |ui, v| {
            vec2_edit(ui, v, 1.0, " px")
        });
        anim_row(ui, "Scale", &mut t.scale, frame, |ui, v| {
            let mut percent = *v * 100.0;
            let changed = vec2_edit(ui, &mut percent, 0.5, "%");
            *v = percent / 100.0;
            changed
        });
        anim_row(ui, "Rotation", &mut t.rotation, frame, |ui, v| {
            ui.add(DragValue::new(v).speed(0.5).suffix("°")).changed()
        });
        anim_row(ui, "Opacity", &mut layer.opacity, frame, |ui, v| {
            let mut percent = *v * 100.0;
            let changed = ui
                .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"))
                .changed();
            *v = percent / 100.0;
            changed
        });

        ui.label("");
        ui.label("Anchor");
        ui.horizontal(|ui| {
            vec2_edit(ui, &mut layer.transform.anchor, 1.0, " px");
            if ui.small_button("Center").clicked() {
                layer.transform.anchor = Vec2::ZERO;
            }
        });
        ui.end_row();
    });
}

fn shape_section(
    ui: &mut Ui,
    shape: &mut ShapeKind,
    size: &mut Animated<Vec2>,
    corner_radius: &mut Animated<f32>,
    frame: i32,
) {
    ui.horizontal(|ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("shape_kind")
            .selected_text(shape.name())
            .show_ui(ui, |ui| {
                for (name, preset) in ShapeKind::PRESETS {
                    ui.selectable_value(shape, preset, name);
                }
            });
    });
    match shape {
        ShapeKind::Polygon { sides } => {
            ui.horizontal(|ui| {
                ui.label("Sides");
                ui.add(DragValue::new(sides).range(3..=64));
            });
        }
        ShapeKind::Star {
            points,
            inner_ratio,
        } => {
            ui.horizontal(|ui| {
                ui.label("Points");
                ui.add(DragValue::new(points).range(2..=64));
                ui.label("Inner");
                ui.add(DragValue::new(inner_ratio).range(0.05..=1.0).speed(0.01));
            });
        }
        _ => {}
    }
    Grid::new("shape_grid").num_columns(3).show(ui, |ui| {
        anim_row(ui, "Size", size, frame, |ui, v| {
            vec2_edit(ui, v, 1.0, " px")
        });
        if *shape == ShapeKind::Rectangle {
            anim_row(ui, "Corners", corner_radius, frame, |ui, v| {
                ui.add(
                    DragValue::new(v)
                        .range(0.0..=10_000.0)
                        .speed(0.5)
                        .suffix(" px"),
                )
                .changed()
            });
        }
    });
}

/// One grid row for an animatable property: keyframe toggle, label, editor.
/// Edits go through [`Animated::set`], so animated properties get keyed.
fn anim_row<T: Lerp>(
    ui: &mut Ui,
    label: &str,
    anim: &mut Animated<T>,
    frame: i32,
    edit: impl FnOnce(&mut Ui, &mut T) -> bool,
) {
    key_toggle(ui, anim, frame);
    ui.label(label);
    let mut value = anim.sample(frame as f32);
    ui.horizontal(|ui| {
        if edit(ui, &mut value) {
            anim.set(frame, value);
        }
    });
    ui.end_row();
}

/// The diamond button that adds or removes a keyframe at the playhead.
fn key_toggle(ui: &mut Ui, track: &mut dyn KeyTrack, frame: i32) {
    let has_key = track.has_key(frame);
    let animated = !track.key_frames().is_empty();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    let color = if animated || response.hovered() {
        theme::KEYFRAME
    } else {
        ui.visuals().weak_text_color()
    };
    let (fill, stroke) = if has_key {
        (color, Stroke::NONE)
    } else {
        (Color32::TRANSPARENT, Stroke::new(1.2, color))
    };
    diamond(ui.painter(), rect.center(), 5.5, fill, stroke);
    let tip = if has_key {
        "Remove keyframe"
    } else {
        "Add keyframe at the playhead"
    };
    if response.on_hover_text(tip).clicked() {
        track.toggle_key(frame);
    }
}

fn vec2_edit(ui: &mut Ui, v: &mut Vec2, speed: f32, suffix: &str) -> bool {
    let x = ui.add(
        DragValue::new(&mut v.x)
            .speed(speed)
            .prefix("X ")
            .suffix(suffix)
            .max_decimals(1),
    );
    let y = ui.add(
        DragValue::new(&mut v.y)
            .speed(speed)
            .prefix("Y ")
            .suffix(suffix)
            .max_decimals(1),
    );
    x.changed() || y.changed()
}

fn color_edit(ui: &mut Ui, color: &mut Color) -> bool {
    let mut rgba = color.to_color32().to_srgba_unmultiplied();
    let changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
    if changed {
        *color = Color::from_color32(egui::Color32::from_rgba_unmultiplied(
            rgba[0], rgba[1], rgba[2], rgba[3],
        ));
    }
    changed
}
