//! The right-hand panel: properties of the selected layer, or of the project
//! when nothing is selected.

use egui::{Color32, CornerRadius, DragValue, Grid, RichText, Sense, Stroke, Ui, Vec2, vec2};

use crate::app::AetherApp;
use crate::model::anim::Lerp;
use crate::model::{
    Animated, BlendMode, Color, Easing, Effect, EffectKind, FillStyle, KeyTrack, Layer, LayerKind,
    LightKind, ShapeKind,
};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;
use crate::ui::timeline::{diamond, timecode};

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
        ui.label(RichText::new("Project settings").size(16.0).strong());
        ui.add_space(6.0);
        let fps_before = self.project.fps;
        let project = &mut self.project;
        card(ui, |ui| {
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
                                    .selectable_label(
                                        project.width == w && project.height == h,
                                        name,
                                    )
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

        let Some(layer) = self.project.layer_mut(id) else {
            return;
        };

        // Header: type chip and name, then the layer's quick actions.
        ui.horizontal(|ui| {
            let (chip, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            ui.painter()
                .rect_filled(chip, CornerRadius::same(8), theme::layer_color(&layer.kind));
            icons::paint(
                ui.painter(),
                chip.shrink(7.0),
                theme::layer_icon(&layer.kind),
                Color32::WHITE,
            );
            ui.add(
                egui::TextEdit::singleline(&mut layer.name)
                    .font(egui::FontId::proportional(15.0))
                    .desired_width(f32::INFINITY),
            );
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let eye = if layer.visible {
                Icon::Eye
            } else {
                Icon::EyeOff
            };
            if icons::toggle(ui, eye, "Show / hide", !layer.visible).clicked() {
                layer.visible = !layer.visible;
            }
            let lock = if layer.locked {
                Icon::Lock
            } else {
                Icon::Unlock
            };
            if icons::toggle(ui, lock, "Lock / unlock", layer.locked).clicked() {
                layer.locked = !layer.locked;
            }
            if matches!(layer.kind, LayerKind::Group) {
                if icons::button(ui, Icon::Ungroup, "Ungroup (Ctrl+Shift+G)").clicked() {
                    action = Some(LayerAction::Ungroup);
                }
            } else if icons::button(ui, Icon::Group, "Put in a new group (Ctrl+G)").clicked() {
                action = Some(LayerAction::Group);
            }
            if icons::button(ui, Icon::Up, "Bring forward").clicked() {
                action = Some(LayerAction::Reorder(1));
            }
            if icons::button(ui, Icon::Down, "Send backward").clicked() {
                action = Some(LayerAction::Reorder(-1));
            }
            if icons::button(ui, Icon::Duplicate, "Duplicate (Ctrl+D)").clicked() {
                action = Some(LayerAction::Duplicate);
            }
            if icons::button(ui, Icon::Trash, "Delete layer").clicked() {
                action = Some(LayerAction::Delete);
            }
        });
        ui.add_space(6.0);

        // Property categories as icon tiles, one page at a time, the way
        // Alight Motion groups a layer's settings.
        let has_border = layer.kind.has_border();
        let is_path = matches!(layer.kind, LayerKind::Path { .. });
        let has_content = layer.kind.is_visual();
        let is_group = matches!(layer.kind, LayerKind::Group);
        let is_audio = matches!(layer.kind, LayerKind::Audio { .. });
        let is_light = matches!(layer.kind, LayerKind::Light { .. });
        // Adjustment layers have no content of their own, only effects.
        let has_own_content =
            (has_content && !is_group && !matches!(layer.kind, LayerKind::Adjustment)) || is_light;
        let page_id = egui::Id::new("inspector_page");
        let mut page = ui
            .data(|d| d.get_temp::<Page>(page_id))
            .unwrap_or(Page::Transform);
        if (page == Page::Border && !has_border)
            || (matches!(page, Page::Fill | Page::Effects) && !has_content)
            || (page == Page::Content && !has_own_content)
        {
            page = Page::Transform;
        }
        // Sound has no place on the canvas: just its settings and timing.
        if is_audio && !matches!(page, Page::Content | Page::Timing) {
            page = Page::Content;
        }
        let mut pages = if is_audio {
            vec![(Page::Content, Icon::Audio, "Audio")]
        } else {
            vec![(Page::Transform, Icon::Transform, "Move")]
        };
        if has_own_content {
            pages.push((
                Page::Content,
                theme::layer_icon(&layer.kind),
                layer.kind.label(),
            ));
        }
        if has_content {
            pages.push((Page::Fill, Icon::Fill, "Color"));
        }
        if has_border {
            let label = if is_path { "Stroke" } else { "Border" };
            pages.push((Page::Border, Icon::Border, label));
        }
        if has_content {
            pages.push((Page::Effects, Icon::Effects, "Effects"));
        }
        pages.push((Page::Timing, Icon::Timing, "Timing"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let n = pages.len() as f32;
            let width = (ui.available_width() - 6.0 * (n - 1.0)) / n;
            for (p, icon, label) in &pages {
                if icons::tile(ui, *icon, label, page == *p, vec2(width, 58.0)).clicked() {
                    page = *p;
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(page_id, page));
        ui.add_space(8.0);

        // Layers this one could be parented to: anything that isn't itself
        // or already one of its children.
        // Parents must share the layer's group.
        let own_group = self.project.layer(id).and_then(|l| l.group);
        let mut parent_choices: Vec<(u64, String)> = self
            .project
            .members(own_group)
            .filter(|l| !self.project.is_ancestor(id, l.id))
            .map(|l| (l.id, l.name.clone()))
            .collect();
        parent_choices.reverse();
        // Groups this layer could move into: any group not inside it.
        let group_choices: Vec<(u64, String)> = self
            .project
            .layers
            .iter()
            .rev()
            .filter(|l| matches!(l.kind, LayerKind::Group) && !self.project.is_in_group(l.id, id))
            .map(|l| (l.id, l.name.clone()))
            .collect();
        let Some(layer) = self.project.layer_mut(id) else {
            return;
        };

        card(ui, |ui| match page {
            Page::Transform => {
                transform_section(ui, layer, frame);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                    icons::paint(
                        ui.painter(),
                        icon_rect,
                        Icon::Link,
                        ui.visuals().weak_text_color(),
                    );
                    ui.label("Parent");
                    let current = layer.parent.and_then(|p| {
                        parent_choices
                            .iter()
                            .find(|(id, _)| *id == p)
                            .map(|(_, n)| n.as_str())
                    });
                    egui::ComboBox::from_id_salt("parent")
                        .selected_text(current.unwrap_or("None"))
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(layer.parent.is_none(), "None")
                                .clicked()
                            {
                                action = Some(LayerAction::Parent(None));
                            }
                            for (pid, name) in &parent_choices {
                                if ui
                                    .selectable_label(layer.parent == Some(*pid), name)
                                    .clicked()
                                {
                                    action = Some(LayerAction::Parent(Some(*pid)));
                                }
                            }
                        })
                        .response
                        .on_hover_text("Follow another layer's move, scale and rotation");
                });
                ui.horizontal(|ui| {
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                    icons::paint(
                        ui.painter(),
                        icon_rect,
                        Icon::Group,
                        ui.visuals().weak_text_color(),
                    );
                    ui.label("Group");
                    let current = layer.group.and_then(|g| {
                        group_choices
                            .iter()
                            .find(|(id, _)| *id == g)
                            .map(|(_, n)| n.as_str())
                    });
                    egui::ComboBox::from_id_salt("group")
                        .selected_text(current.unwrap_or("None"))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(layer.group.is_none(), "None").clicked() {
                                action = Some(LayerAction::MoveToGroup(None));
                            }
                            for (gid, name) in &group_choices {
                                if ui
                                    .selectable_label(layer.group == Some(*gid), name)
                                    .clicked()
                                {
                                    action = Some(LayerAction::MoveToGroup(Some(*gid)));
                                }
                            }
                        });
                });
            }
            Page::Content => match &mut layer.kind {
                LayerKind::Shape {
                    shape,
                    size,
                    corner_radius,
                } => shape_section(ui, shape, size, corner_radius, frame),
                LayerKind::Path { path } => {
                    Grid::new("path_grid").num_columns(3).show(ui, |ui| {
                        key_toggle(ui, path, frame);
                        ui.label("Path");
                        let mut shape = path.sample(frame as f32);
                        ui.label(format!("{} points", shape.nodes.len()));
                        ui.end_row();
                        ui.label("");
                        ui.label("");
                        if ui.checkbox(&mut shape.closed, "Closed").changed() {
                            path.set(frame, shape);
                        }
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    for tip in [
                        "Drag points and their handles on the canvas.",
                        "Double-click a point to make it smooth or sharp.",
                        "Alt-click a point to delete it.",
                        "Key the path to morph it between shapes with the same number of points.",
                    ] {
                        ui.label(RichText::new(tip).small().weak());
                    }
                }
                LayerKind::Text { text, font_size } => {
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
                }
                LayerKind::Image { path, size } => {
                    ui.label(format!("{} × {} px", size.x, size.y));
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                }
                LayerKind::Video {
                    path,
                    size,
                    seconds,
                    rate,
                    has_audio,
                    volume,
                    ..
                } => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.label(RichText::new(name).strong());
                    ui.label(format!(
                        "{} × {} px · {:.1} s · {} fps",
                        size.x,
                        size.y,
                        seconds,
                        (*rate * 100.0).round() / 100.0
                    ));
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                    if *has_audio {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Volume");
                            let mut percent = *volume * 100.0;
                            if ui
                                .add(egui::Slider::new(&mut percent, 0.0..=200.0).suffix("%"))
                                .changed()
                            {
                                *volume = percent / 100.0;
                            }
                        });
                    }
                    ui.label(
                        RichText::new("Drag the bar's edges to trim the clip.")
                            .small()
                            .weak(),
                    );
                }
                LayerKind::Audio { path, volume, .. } => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(path.display().to_string()).small().weak());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Volume");
                        let mut percent = *volume * 100.0;
                        if ui
                            .add(egui::Slider::new(&mut percent, 0.0..=200.0).suffix("%"))
                            .changed()
                        {
                            *volume = percent / 100.0;
                        }
                    });
                    ui.label(
                        RichText::new("Drag the bar's left edge to trim the start.")
                            .small()
                            .weak(),
                    );
                }
                LayerKind::Light {
                    light,
                    intensity,
                    cone,
                    feather,
                } => light_section(ui, light, intensity, cone, feather, &mut layer.fill, frame),
                LayerKind::Null
                | LayerKind::Camera { .. }
                | LayerKind::Group
                | LayerKind::Adjustment => {}
            },
            Page::Fill => {
                let is_image = !layer.kind.has_fill();
                if !is_image {
                    ui.horizontal_wrapped(|ui| {
                        for style in FillStyle::ALL {
                            let before = layer.fill_style;
                            ui.selectable_value(&mut layer.fill_style, style, style.label());
                            // Give a fresh gradient a visible second colour.
                            if before == FillStyle::Solid
                                && layer.fill_style != FillStyle::Solid
                                && layer.fill_end.sample(frame as f32)
                                    == layer.fill.sample(frame as f32)
                            {
                                layer.fill_end = Animated::new(gradient_partner(layer.fill.value));
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                Grid::new("fill_grid").num_columns(3).show(ui, |ui| {
                    match (is_image, layer.fill_style) {
                        (true, _) => {}
                        (false, FillStyle::Solid) => {
                            anim_row(ui, "Color", &mut layer.fill, frame, color_edit);
                        }
                        (false, style) => {
                            let (a, b) = if style == FillStyle::Radial {
                                ("Center", "Edge")
                            } else {
                                ("Start", "End")
                            };
                            anim_row(ui, a, &mut layer.fill, frame, color_edit);
                            anim_row(ui, b, &mut layer.fill_end, frame, color_edit);
                            if style == FillStyle::Linear {
                                anim_row(ui, "Angle", &mut layer.gradient_angle, frame, |ui, v| {
                                    ui.add(DragValue::new(v).speed(1.0).suffix("°")).changed()
                                });
                            }
                        }
                    }
                    anim_row(ui, "Opacity", &mut layer.opacity, frame, opacity_edit);
                    ui.label("");
                    ui.label("Blending");
                    egui::ComboBox::from_id_salt("blend")
                        .selected_text(layer.blend.label())
                        .height(400.0)
                        .show_ui(ui, |ui| {
                            for mode in BlendMode::ALL {
                                ui.selectable_value(&mut layer.blend, mode, mode.label());
                            }
                        });
                    ui.end_row();
                });
            }
            Page::Border => {
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
            }
            Page::Effects => effects_section(ui, &mut layer.effects, frame),
            Page::Timing => {
                Grid::new("timing_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Starts at");
                    ui.add(
                        DragValue::new(&mut layer.in_frame)
                            .range(-duration..=layer.out_frame - 1)
                            .suffix(" f"),
                    );
                    ui.end_row();
                    ui.label("Ends at");
                    ui.add(
                        DragValue::new(&mut layer.out_frame)
                            .range(layer.in_frame + 1..=duration * 4)
                            .suffix(" f"),
                    );
                    ui.end_row();
                });
            }
        });

        // The selected keyframe (picked on the layer's timeline bar).
        let fps = self.project.fps;
        if let Some(key) = self.selected_key.filter(|k| k.layer == id)
            && let Some(layer) = self.project.layer_mut(id)
            && let Some(mut easing) = layer.easing_at(key.frame)
        {
            ui.add_space(8.0);
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    diamond(
                        ui.painter(),
                        rect.center(),
                        6.0,
                        theme::KEYFRAME,
                        Stroke::NONE,
                    );
                    ui.label(RichText::new("Keyframe").strong());
                    ui.label(RichText::new(timecode(key.frame, fps)).monospace().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icons::button(ui, Icon::Trash, "Delete keyframe").clicked() {
                            action = Some(LayerAction::DeleteKey);
                        }
                    });
                });
                ui.label(RichText::new("Easing to the next keyframe").weak());
                ui.horizontal_wrapped(|ui| {
                    for e in Easing::ALL {
                        ui.selectable_value(&mut easing, e, e.label());
                    }
                });
            });
            layer.set_easing_at(key.frame, easing);
        }

        match action {
            Some(LayerAction::DeleteKey) => self.delete_selection(),
            Some(LayerAction::Duplicate) => self.duplicate_selected(),
            Some(LayerAction::Delete) => {
                self.selected_key = None;
                self.delete_selection();
            }
            Some(LayerAction::Reorder(delta)) => self.project.reorder_layer(id, delta),
            Some(LayerAction::Parent(parent)) => {
                self.project.reparent_in_place(id, parent, frame);
            }
            Some(LayerAction::MoveToGroup(group)) => {
                self.project.move_to_group(id, group, frame);
            }
            Some(LayerAction::Group) => self.group_selected(),
            Some(LayerAction::Ungroup) => self.ungroup_selected(),
            None => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Transform,
    Content,
    Fill,
    Border,
    Effects,
    Timing,
}

enum LayerAction {
    DeleteKey,
    Duplicate,
    Delete,
    Reorder(isize),
    Parent(Option<u64>),
    MoveToGroup(Option<u64>),
    Group,
    Ungroup,
}

/// A rounded surface that groups a page of settings.
fn card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

fn transform_section(ui: &mut Ui, layer: &mut Layer, frame: i32) {
    // Cameras and lights have a place and a direction but no size.
    let camera = matches!(
        layer.kind,
        LayerKind::Camera { .. } | LayerKind::Light { .. }
    );
    let three_d = layer.is_3d();
    Grid::new("transform_grid").num_columns(3).show(ui, |ui| {
        let t = &mut layer.transform;
        anim_row(ui, "Position", &mut t.position, frame, |ui, v| {
            vec2_edit(ui, v, 1.0, " px")
        });
        if three_d {
            anim_row(ui, "Depth", &mut t.z, frame, |ui, v| {
                ui.add(DragValue::new(v).speed(1.0).prefix("Z ").suffix(" px"))
                    .on_hover_text("Positive values move away from the camera")
                    .changed()
            });
        }
        if !camera {
            anim_row(ui, "Scale", &mut t.scale, frame, |ui, v| {
                let mut percent = *v * 100.0;
                let changed = vec2_edit(ui, &mut percent, 0.5, "%");
                *v = percent / 100.0;
                changed
            });
        }
        let degrees =
            |ui: &mut Ui, v: &mut f32| ui.add(DragValue::new(v).speed(0.5).suffix("°")).changed();
        if three_d {
            anim_row(ui, "Tilt X", &mut t.rotation_x, frame, degrees);
            anim_row(ui, "Turn Y", &mut t.rotation_y, frame, degrees);
            anim_row(ui, "Rotate Z", &mut t.rotation, frame, degrees);
        } else {
            anim_row(ui, "Rotation", &mut t.rotation, frame, degrees);
        }
        if let LayerKind::Camera { zoom } = &mut layer.kind {
            anim_row(ui, "Zoom", zoom, frame, |ui, v| {
                ui.add(
                    DragValue::new(v)
                        .range(10.0..=100_000.0)
                        .speed(2.0)
                        .suffix(" px"),
                )
                .on_hover_text("Distance at which a layer appears at 100%")
                .changed()
            });
            return;
        }
        if camera {
            return;
        }

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

fn light_section(
    ui: &mut Ui,
    light: &mut LightKind,
    intensity: &mut Animated<f32>,
    cone: &mut Animated<f32>,
    feather: &mut Animated<f32>,
    color: &mut Animated<Color>,
    frame: i32,
) {
    ui.horizontal_wrapped(|ui| {
        for kind in LightKind::ALL {
            ui.selectable_value(light, kind, kind.label());
        }
    });
    ui.add_space(4.0);
    Grid::new("light_grid").num_columns(3).show(ui, |ui| {
        anim_row(ui, "Color", color, frame, color_edit);
        anim_row(ui, "Intensity", intensity, frame, percent(0.0, 1000.0));
        if *light == LightKind::Spot {
            anim_row(ui, "Cone", cone, frame, |ui, v| {
                ui.add(DragValue::new(v).range(1.0..=179.0).speed(0.5).suffix("°"))
                    .changed()
            });
            anim_row(ui, "Feather", feather, frame, percent(0.0, 100.0));
        }
    });
    ui.add_space(4.0);
    let tip = match light {
        LightKind::Ambient => "Lights every layer evenly.",
        LightKind::Point => "Shines in all directions from where it is. Move it with Depth too.",
        LightKind::Spot | LightKind::Parallel => {
            "Points along its dashed line. Aim it with Tilt X and Turn Y on the Move page."
        }
    };
    ui.label(RichText::new(tip).small().weak());
    ui.label(
        RichText::new("With any light in the scene, places no light reaches go dark.")
            .small()
            .weak(),
    );
}

/// An editor for a 0-based factor shown as a percentage.
fn percent(min: f32, max: f32) -> impl Fn(&mut Ui, &mut f32) -> bool {
    move |ui, v| {
        let mut p = *v * 100.0;
        let changed = ui
            .add(
                DragValue::new(&mut p)
                    .range(min..=max)
                    .speed(1.0)
                    .suffix("%"),
            )
            .changed();
        *v = p / 100.0;
        changed
    }
}

fn effects_section(ui: &mut Ui, effects: &mut Vec<Effect>, frame: i32) {
    let mut remove = None;
    let mut swap = None;
    let count = effects.len();
    for (i, effect) in effects.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut effect.enabled, "");
                ui.label(RichText::new(effect.name()).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, Icon::Trash, "Remove effect").clicked() {
                        remove = Some(i);
                    }
                    if i + 1 < count && icons::button(ui, Icon::Down, "Apply later").clicked() {
                        swap = Some(i);
                    }
                    if i > 0 && icons::button(ui, Icon::Up, "Apply earlier").clicked() {
                        swap = Some(i - 1);
                    }
                });
            });
            ui.add_enabled_ui(effect.enabled, |ui| {
                Grid::new("effect_grid").num_columns(3).show(ui, |ui| {
                    let px = |max: f32| {
                        move |ui: &mut Ui, v: &mut f32| {
                            ui.add(DragValue::new(v).range(0.0..=max).speed(0.3).suffix(" px"))
                                .changed()
                        }
                    };
                    match &mut effect.kind {
                        EffectKind::Blur { radius } => {
                            anim_row(ui, "Radius", radius, frame, px(500.0));
                        }
                        EffectKind::Shadow {
                            color,
                            distance,
                            angle,
                            blur,
                        } => {
                            anim_row(ui, "Color", color, frame, color_edit);
                            anim_row(ui, "Distance", distance, frame, px(2000.0));
                            anim_row(ui, "Angle", angle, frame, |ui, v| {
                                ui.add(DragValue::new(v).speed(1.0).suffix("°")).changed()
                            });
                            anim_row(ui, "Softness", blur, frame, px(500.0));
                        }
                        EffectKind::Glow {
                            color,
                            radius,
                            strength,
                        } => {
                            anim_row(ui, "Color", color, frame, color_edit);
                            anim_row(ui, "Radius", radius, frame, px(500.0));
                            anim_row(ui, "Strength", strength, frame, |ui, v| {
                                let mut percent = *v * 100.0;
                                let changed = ui
                                    .add(
                                        DragValue::new(&mut percent)
                                            .range(0.0..=1000.0)
                                            .speed(1.0)
                                            .suffix("%"),
                                    )
                                    .changed();
                                *v = percent / 100.0;
                                changed
                            });
                        }
                        EffectKind::AdjustColor {
                            brightness,
                            contrast,
                            saturation,
                            hue,
                        } => {
                            anim_row(ui, "Brightness", brightness, frame, percent(-100.0, 100.0));
                            anim_row(ui, "Contrast", contrast, frame, percent(0.0, 300.0));
                            anim_row(ui, "Saturation", saturation, frame, percent(0.0, 300.0));
                            anim_row(ui, "Hue", hue, frame, |ui, v| {
                                ui.add(DragValue::new(v).speed(1.0).suffix("°")).changed()
                            });
                        }
                    }
                });
            });
            ui.separator();
        });
    }
    if let Some(i) = remove {
        effects.remove(i);
    }
    if let Some(i) = swap {
        effects.swap(i, i + 1);
    }
    if effects.is_empty() {
        ui.label(RichText::new("Effects run in order on this layer's image.").weak());
    }
    ui.menu_button("+ Add effect", |ui| {
        for (i, name) in Effect::PRESETS.iter().enumerate() {
            if ui.button(*name).clicked() {
                effects.push(Effect::preset(i));
                ui.close();
            }
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

fn opacity_edit(ui: &mut Ui, v: &mut f32) -> bool {
    let mut percent = *v * 100.0;
    let changed = ui
        .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"))
        .changed();
    *v = percent / 100.0;
    changed
}

/// A second gradient colour that reads clearly against `c`.
fn gradient_partner(c: Color) -> Color {
    let luma = 0.3 * c.r + 0.59 * c.g + 0.11 * c.b;
    if luma > 0.5 {
        Color::new(c.r * 0.25, c.g * 0.25, c.b * 0.35, c.a)
    } else {
        Color::new(0.55 + c.r * 0.45, 0.4 + c.g * 0.6, 0.95, c.a)
    }
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
