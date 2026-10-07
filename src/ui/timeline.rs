//! The timeline: transport controls, a time ruler, one row per layer with a
//! draggable/trimmable bar, and expandable property rows showing keyframes.

use std::collections::HashSet;

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, vec2,
};

use crate::app::{AetherApp, KeySelection};
use crate::model::{Easing, PropId};
use crate::ui::theme;

const LABEL_WIDTH: f32 = 230.0;
const RULER_HEIGHT: f32 = 24.0;
const LAYER_ROW: f32 = 28.0;
const PROP_ROW: f32 = 22.0;
const EDGE_GRAB: f32 = 6.0;

#[derive(Default)]
pub struct TimelineState {
    pub expanded: HashSet<u64>,
    /// Points per frame; `None` fits the whole composition in view.
    pub zoom: Option<f32>,
    /// First visible frame (fractional) when zoomed in.
    pub scroll_frame: f32,
    drag: Option<TimelineDrag>,
}

/// An in-progress drag. Tracked here instead of on a widget response so a
/// keyframe or bar keeps following the pointer when its widget id changes.
#[derive(Clone, Copy, Debug)]
enum TimelineDrag {
    Scrub,
    MoveLayer {
        id: u64,
        origin: i32,
        applied: i32,
    },
    TrimIn {
        id: u64,
    },
    TrimOut {
        id: u64,
    },
    Key {
        layer: u64,
        prop: PropId,
        frame: i32,
    },
}

#[derive(Clone, Copy)]
enum Row {
    Layer(u64),
    Prop(u64, PropId),
}

/// Frame <-> x conversion for the track area.
#[derive(Clone, Copy)]
struct TimeScale {
    x0: f32,
    points_per_frame: f32,
    scroll: f32,
}

impl TimeScale {
    fn x(&self, frame: f32) -> f32 {
        self.x0 + (frame - self.scroll) * self.points_per_frame
    }

    fn frame_at(&self, x: f32) -> f32 {
        (x - self.x0) / self.points_per_frame + self.scroll
    }
}

pub fn timecode(frame: i32, fps: u32) -> String {
    let fps = fps.max(1) as i32;
    let seconds = frame.max(0) / fps;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 60,
        seconds % 60,
        frame.max(0) % fps
    )
}

impl AetherApp {
    pub fn timeline_ui(&mut self, ui: &mut Ui) {
        self.transport_ui(ui);
        ui.add_space(2.0);

        let full = ui.available_rect_before_wrap();
        let x0 = full.left() + LABEL_WIDTH;
        let track_width = (full.right() - x0 - 12.0).max(40.0);
        let duration = self.project.duration.max(1) as f32;
        let fit = track_width / duration;
        if self.timeline.zoom.is_none() {
            self.timeline.scroll_frame = 0.0;
        }
        let mut scale = TimeScale {
            x0,
            points_per_frame: self.timeline.zoom.unwrap_or(fit),
            scroll: self.timeline.scroll_frame,
        };

        // Ctrl+scroll zooms around the pointer; horizontal scroll pans.
        if ui.rect_contains_pointer(full) {
            let (zoom, scroll_x, pointer) = ui.input(|i| {
                (
                    i.zoom_delta(),
                    i.smooth_scroll_delta.x,
                    i.pointer.hover_pos(),
                )
            });
            if (zoom - 1.0).abs() > 1e-4
                && let Some(p) = pointer
            {
                let anchor = scale.frame_at(p.x);
                let ppf = (scale.points_per_frame * zoom).clamp(fit.min(0.5), 60.0);
                self.timeline.zoom = Some(ppf);
                self.timeline.scroll_frame = anchor - (p.x - x0) / ppf;
            }
            if scroll_x != 0.0 && self.timeline.zoom.is_some() {
                self.timeline.scroll_frame -= scroll_x / scale.points_per_frame;
            }
            let ppf = self.timeline.zoom.unwrap_or(fit);
            let visible = track_width / ppf;
            self.timeline.scroll_frame = self
                .timeline
                .scroll_frame
                .clamp(0.0, (duration - visible).max(0.0));
            scale.points_per_frame = ppf;
            scale.scroll = self.timeline.scroll_frame;
        }

        self.update_drag(ui, scale);
        self.ruler_ui(ui, scale);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| self.rows_ui(ui, scale));
    }

    fn transport_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("First frame (Home)").clicked() {
                self.set_frame(0);
            }
            if ui.button("◀").on_hover_text("Previous frame (←)").clicked() {
                self.set_frame(self.frame - 1);
            }
            let play = if self.playing { "⏸" } else { "▶" };
            let play_button =
                egui::Button::new(egui::RichText::new(play).size(16.0)).fill(theme::ACCENT_SOFT);
            if ui
                .add(play_button)
                .on_hover_text("Play / pause (Space)")
                .clicked()
            {
                self.toggle_playback();
            }
            if ui.button("▶|").on_hover_text("Next frame (→)").clicked() {
                self.set_frame(self.frame + 1);
            }
            if ui.button("⏭").on_hover_text("Last frame (End)").clicked() {
                self.set_frame(self.project.duration - 1);
            }
            ui.toggle_value(&mut self.looping, "Loop");
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "{} / {}",
                    timecode(self.frame, self.project.fps),
                    timecode(self.project.duration, self.project.fps)
                ))
                .monospace(),
            );
            let mut frame = self.frame;
            if ui
                .add(
                    egui::DragValue::new(&mut frame)
                        .range(0..=self.project.duration - 1)
                        .prefix("frame "),
                )
                .changed()
            {
                self.set_frame(frame);
            }
            ui.separator();
            ui.menu_button("+ Add layer", |ui| self.add_layer_menu(ui));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Fit")
                    .on_hover_text("Show the whole composition")
                    .clicked()
                {
                    self.timeline.zoom = None;
                }
                ui.label(egui::RichText::new("Ctrl+scroll to zoom").weak().small());
            });
        });
    }

    fn update_drag(&mut self, ui: &Ui, scale: TimeScale) {
        let Some(drag) = self.timeline.drag else {
            return;
        };
        let (down, pointer) = ui.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos()));
        if !down {
            self.timeline.drag = None;
            return;
        }
        let Some(pointer) = pointer else {
            return;
        };
        let frame = scale.frame_at(pointer.x).round() as i32;
        match drag {
            TimelineDrag::Scrub => self.set_frame(frame),
            TimelineDrag::MoveLayer {
                id,
                origin,
                applied,
            } => {
                let delta = frame - origin;
                if delta != applied
                    && let Some(layer) = self.project.layer_mut(id)
                {
                    layer.shift_in_time(delta - applied);
                    self.timeline.drag = Some(TimelineDrag::MoveLayer {
                        id,
                        origin,
                        applied: delta,
                    });
                }
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            TimelineDrag::TrimIn { id } => {
                if let Some(layer) = self.project.layer_mut(id) {
                    layer.in_frame = frame.min(layer.out_frame - 1);
                }
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            TimelineDrag::TrimOut { id } => {
                if let Some(layer) = self.project.layer_mut(id) {
                    layer.out_frame = frame.max(layer.in_frame + 1);
                }
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            TimelineDrag::Key {
                layer,
                prop,
                frame: from,
            } => {
                let frame = frame.max(0);
                if let Some(track) = self
                    .project
                    .layer_mut(layer)
                    .and_then(|l| l.track_mut(prop))
                    && frame != from
                    && !track.has_key(frame)
                {
                    // Keys never land on an occupied frame, so dragging past
                    // another key can't swallow it.
                    track.move_key(from, frame);
                    self.timeline.drag = Some(TimelineDrag::Key { layer, prop, frame });
                    self.selected_key = Some(KeySelection { layer, prop, frame });
                }
            }
        }
    }

    fn ruler_ui(&mut self, ui: &mut Ui, scale: TimeScale) {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), RULER_HEIGHT),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

        let track = Rect::from_x_y_ranges(scale.x0..=rect.right(), rect.y_range());
        let fps = self.project.fps.max(1) as i32;
        // Pick a tick step that keeps labels at least ~70pt apart.
        let steps = [
            1,
            2,
            5,
            10,
            fps / 2,
            fps,
            fps * 2,
            fps * 5,
            fps * 10,
            fps * 30,
            fps * 60,
        ];
        let step = steps
            .into_iter()
            .filter(|s| *s > 0)
            .find(|s| *s as f32 * scale.points_per_frame >= 70.0)
            .unwrap_or(fps * 60);
        let first = (scale.frame_at(track.left()).floor() as i32 / step) * step;
        let last = scale.frame_at(track.right()).ceil() as i32;
        let text_color = ui.visuals().weak_text_color();
        for frame in (first.max(0)..=last.min(self.project.duration)).step_by(step as usize) {
            let x = scale.x(frame as f32);
            painter.line_segment(
                [
                    Pos2::new(x, rect.bottom() - 8.0),
                    Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(1.0, text_color),
            );
            let label = if frame % fps == 0 {
                format!("{}s", frame / fps)
            } else {
                format!("{}f", frame % fps)
            };
            painter.text(
                Pos2::new(x + 3.0, rect.top() + 3.0),
                Align2::LEFT_TOP,
                label,
                FontId::monospace(10.0),
                text_color,
            );
        }
        // Minor ticks.
        let minor = (step / 5).max(1);
        if minor as f32 * scale.points_per_frame >= 6.0 {
            for frame in (first.max(0)..=last.min(self.project.duration)).step_by(minor as usize) {
                let x = scale.x(frame as f32);
                painter.line_segment(
                    [
                        Pos2::new(x, rect.bottom() - 4.0),
                        Pos2::new(x, rect.bottom()),
                    ],
                    Stroke::new(1.0, text_color.gamma_multiply(0.5)),
                );
            }
        }

        // Current time in the label column.
        let label_rect = Rect::from_x_y_ranges(rect.left()..=scale.x0, rect.y_range());
        painter.rect_filled(label_rect, 0.0, ui.visuals().panel_fill);
        painter.text(
            label_rect.left_center() + vec2(6.0, 0.0),
            Align2::LEFT_CENTER,
            timecode(self.frame, self.project.fps),
            FontId::monospace(13.0),
            theme::PLAYHEAD,
        );

        // Playhead handle.
        let x = scale.x(self.frame as f32);
        if track.x_range().contains(x) {
            let head = [
                Pos2::new(x - 6.0, rect.top()),
                Pos2::new(x + 6.0, rect.top()),
                Pos2::new(x, rect.top() + 10.0),
            ];
            painter.add(Shape::convex_polygon(
                head.to_vec(),
                theme::PLAYHEAD,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.5, theme::PLAYHEAD),
            );
        }

        if (response.is_pointer_button_down_on() || response.clicked())
            && self.timeline.drag.is_none()
            && let Some(p) = response.interact_pointer_pos()
            && p.x >= scale.x0
        {
            self.set_frame(scale.frame_at(p.x).round() as i32);
            self.timeline.drag = Some(TimelineDrag::Scrub);
        }
    }

    fn rows_ui(&mut self, ui: &mut Ui, scale: TimeScale) {
        let rows: Vec<Row> = self
            .project
            .layers
            .iter()
            .rev()
            .flat_map(|layer| {
                let mut rows = vec![Row::Layer(layer.id)];
                if self.timeline.expanded.contains(&layer.id) {
                    rows.extend(layer.props().into_iter().map(|p| Row::Prop(layer.id, p)));
                }
                rows
            })
            .collect();
        let content_height: f32 = rows
            .iter()
            .map(|r| match r {
                Row::Layer(_) => LAYER_ROW,
                Row::Prop(..) => PROP_ROW,
            })
            .sum();

        let size = vec2(
            ui.available_width(),
            content_height.max(ui.available_height()),
        );
        let (rect, background) = ui.allocate_exact_size(size, Sense::click());
        if background.clicked() {
            self.select(None);
        }
        let painter = ui.painter_at(rect);
        let track_clip = Rect::from_x_y_ranges(scale.x0..=rect.right(), rect.y_range())
            .intersect(painter.clip_rect());
        let track_painter = painter.with_clip_rect(track_clip);

        // Shade time past the end of the composition.
        let end_x = scale.x(self.project.duration as f32);
        if end_x < rect.right() {
            let past = Rect::from_x_y_ranges(end_x.max(scale.x0)..=rect.right(), rect.y_range());
            track_painter.rect_filled(past, 0.0, Color32::from_black_alpha(90));
        }
        // Label column divider.
        painter.line_segment(
            [
                Pos2::new(scale.x0, rect.top()),
                Pos2::new(scale.x0, rect.bottom()),
            ],
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        if rows.is_empty() {
            painter.text(
                Pos2::new(rect.left() + 12.0, rect.top() + 16.0),
                Align2::LEFT_CENTER,
                "No layers yet. Use \"+ Add layer\" or the toolbar above the canvas.",
                FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        }

        let mut y = rect.top();
        for (i, row) in rows.iter().enumerate() {
            match *row {
                Row::Layer(id) => {
                    let row_rect = Rect::from_x_y_ranges(rect.x_range(), y..=y + LAYER_ROW);
                    if i % 2 == 1 {
                        painter.rect_filled(row_rect, 0.0, ui.visuals().faint_bg_color);
                    }
                    self.layer_row(
                        ui,
                        &painter,
                        &track_painter,
                        track_clip,
                        row_rect,
                        scale,
                        id,
                    );
                    y += LAYER_ROW;
                }
                Row::Prop(id, prop) => {
                    let row_rect = Rect::from_x_y_ranges(rect.x_range(), y..=y + PROP_ROW);
                    self.prop_row(
                        ui,
                        &painter,
                        &track_painter,
                        track_clip,
                        row_rect,
                        scale,
                        id,
                        prop,
                    );
                    y += PROP_ROW;
                }
            }
        }

        // Playhead across all rows.
        let x = scale.x(self.frame as f32);
        if track_clip.x_range().contains(x) {
            track_painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.5, theme::PLAYHEAD),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn layer_row(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        track_painter: &egui::Painter,
        track_clip: Rect,
        row: Rect,
        scale: TimeScale,
        id: u64,
    ) {
        let Some(layer) = self.project.layer(id) else {
            return;
        };
        let selected = self.selected == Some(id);
        let (visible, locked, expanded) = (
            layer.visible,
            layer.locked,
            self.timeline.expanded.contains(&id),
        );
        let color = theme::layer_color(&layer.kind);
        let name = layer.name.clone();
        let icon = theme::layer_icon(&layer.kind);
        let (in_frame, out_frame) = (layer.in_frame, layer.out_frame);
        let key_frames = layer.all_key_frames();

        if selected {
            painter.rect_filled(row, 0.0, theme::ACCENT_SOFT.gamma_multiply(0.35));
        }

        // --- Label column: expand, visibility, lock, name.
        let text_color = if visible {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let mut x = row.left() + 4.0;
        let mut small_button = |ui: &mut Ui, glyph: &str, salt: &str, active: bool| {
            let r = Rect::from_min_size(Pos2::new(x, row.top()), vec2(22.0, row.height()));
            x += 22.0;
            let response = ui.interact(r, Id::new((salt, id)), Sense::click());
            let c = if response.hovered() {
                Color32::WHITE
            } else if active {
                text_color
            } else {
                ui.visuals().weak_text_color().gamma_multiply(0.6)
            };
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(13.0),
                c,
            );
            response
        };
        if small_button(ui, if expanded { "⏷" } else { "⏵" }, "expand", true)
            .on_hover_text("Show keyframed properties")
            .clicked()
            && !self.timeline.expanded.remove(&id)
        {
            self.timeline.expanded.insert(id);
        }
        let toggle_visible = small_button(ui, "👁", "visible", visible)
            .on_hover_text("Show / hide")
            .clicked();
        let toggle_lock = small_button(ui, "🔒", "lock", locked)
            .on_hover_text("Lock / unlock")
            .clicked();

        let name_rect = Rect::from_x_y_ranges(x..=scale.x0 - 4.0, row.y_range());
        let name_response = ui.interact(name_rect, Id::new(("name", id)), Sense::click());
        painter.text(
            name_rect.left_center() + vec2(2.0, 0.0),
            Align2::LEFT_CENTER,
            icon,
            FontId::proportional(13.0),
            color,
        );
        painter.with_clip_rect(name_rect).text(
            name_rect.left_center() + vec2(20.0, 0.0),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(13.0),
            text_color,
        );
        if name_response.clicked() {
            self.select(Some(id));
        }

        // --- Track: the layer's bar from in to out.
        let bar = Rect::from_x_y_ranges(
            scale.x(in_frame as f32)..=scale.x(out_frame as f32),
            row.top() + 4.0..=row.bottom() - 4.0,
        );
        let fill = if visible {
            color.gamma_multiply(0.75)
        } else {
            color.gamma_multiply(0.3)
        };
        track_painter.rect_filled(bar, CornerRadius::same(4), fill);
        if selected {
            track_painter.rect_stroke(
                bar,
                CornerRadius::same(4),
                Stroke::new(1.5, Color32::WHITE),
                StrokeKind::Inside,
            );
        }
        track_painter
            .with_clip_rect(bar.intersect(track_clip))
            .text(
                bar.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                &name,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
        for frame in &key_frames {
            diamond(
                track_painter,
                Pos2::new(scale.x(*frame as f32), bar.center().y),
                3.5,
                Color32::WHITE,
                Stroke::NONE,
            );
        }

        let clipped_bar = bar.intersect(track_clip);
        if clipped_bar.is_positive() && !locked {
            let body = ui.interact(clipped_bar, Id::new(("bar", id)), Sense::click_and_drag());
            if body.clicked() {
                self.select(Some(id));
            }
            if body.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            if body.drag_started()
                && let Some(p) = body.interact_pointer_pos()
            {
                self.select(Some(id));
                let origin = scale.frame_at(p.x).round() as i32;
                self.timeline.drag = Some(TimelineDrag::MoveLayer {
                    id,
                    origin,
                    applied: 0,
                });
            }
            let edges = [
                (
                    Rect::from_x_y_ranges(bar.left()..=bar.left() + EDGE_GRAB, bar.y_range()),
                    "trim_in",
                ),
                (
                    Rect::from_x_y_ranges(bar.right() - EDGE_GRAB..=bar.right(), bar.y_range()),
                    "trim_out",
                ),
            ];
            for (edge, salt) in edges {
                let edge = edge.intersect(track_clip);
                if !edge.is_positive() {
                    continue;
                }
                let response = ui.interact(edge, Id::new((salt, id)), Sense::drag());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                }
                if response.drag_started() {
                    self.select(Some(id));
                    self.timeline.drag = Some(if salt == "trim_in" {
                        TimelineDrag::TrimIn { id }
                    } else {
                        TimelineDrag::TrimOut { id }
                    });
                }
            }
        }

        if let Some(layer) = self.project.layer_mut(id) {
            if toggle_visible {
                layer.visible = !layer.visible;
            }
            if toggle_lock {
                layer.locked = !layer.locked;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn prop_row(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        track_painter: &egui::Painter,
        track_clip: Rect,
        row: Rect,
        scale: TimeScale,
        id: u64,
        prop: PropId,
    ) {
        let frame = self.frame;
        let Some(track) = self.project.layer(id).and_then(|l| l.track(prop)) else {
            return;
        };
        let keys = track.key_frames();
        let has_key_here = track.has_key(frame);
        let easings: Vec<Easing> = keys
            .iter()
            .map(|k| track.easing(*k).unwrap_or_default())
            .collect();

        painter.rect_filled(row, 0.0, Color32::from_black_alpha(40));

        // Label column: keyframe toggle and name.
        let toggle_rect = Rect::from_center_size(
            Pos2::new(row.left() + 40.0, row.center().y),
            Vec2::splat(16.0),
        );
        let toggle = ui.interact(toggle_rect, Id::new(("prop_key", id, prop)), Sense::click());
        let (fill, stroke) = if has_key_here {
            (theme::KEYFRAME, Stroke::NONE)
        } else {
            (
                Color32::TRANSPARENT,
                Stroke::new(
                    1.0,
                    if keys.is_empty() {
                        ui.visuals().weak_text_color()
                    } else {
                        theme::KEYFRAME
                    },
                ),
            )
        };
        diamond(painter, toggle_rect.center(), 5.0, fill, stroke);
        let toggle_clicked = toggle
            .on_hover_text("Add / remove a keyframe at the playhead")
            .clicked();
        let label_color = if keys.is_empty() {
            ui.visuals().weak_text_color()
        } else {
            ui.visuals().text_color()
        };
        painter.text(
            Pos2::new(row.left() + 54.0, row.center().y),
            Align2::LEFT_CENTER,
            prop.label(),
            FontId::proportional(12.0),
            label_color,
        );

        // Segments between keys, then the keys themselves.
        for (pair, easing) in keys.windows(2).zip(&easings) {
            let a = Pos2::new(scale.x(pair[0] as f32), row.center().y);
            let b = Pos2::new(scale.x(pair[1] as f32), row.center().y);
            let stroke = if *easing == Easing::Hold {
                Stroke::new(1.0, theme::KEYFRAME.gamma_multiply(0.25))
            } else {
                Stroke::new(2.0, theme::KEYFRAME.gamma_multiply(0.45))
            };
            track_painter.line_segment([a, b], stroke);
        }

        let mut clicked_key = None;
        let mut drag_key = None;
        let mut jump_to = None;
        let mut menu_action = None;
        for &key in &keys {
            let center = Pos2::new(scale.x(key as f32), row.center().y);
            let is_selected = self.selected_key
                == Some(KeySelection {
                    layer: id,
                    prop,
                    frame: key,
                });
            let stroke = if is_selected {
                Stroke::new(2.0, Color32::WHITE)
            } else {
                Stroke::new(1.0, Color32::from_black_alpha(160))
            };
            diamond(
                track_painter,
                center,
                if is_selected { 6.5 } else { 5.5 },
                theme::KEYFRAME,
                stroke,
            );

            let hit = Rect::from_center_size(center, Vec2::splat(14.0)).intersect(track_clip);
            if !hit.is_positive() {
                continue;
            }
            let response = ui.interact(
                hit,
                Id::new(("key", id, prop, key)),
                Sense::click_and_drag(),
            );
            if response.clicked() {
                clicked_key = Some(key);
            }
            if response.double_clicked() {
                jump_to = Some(key);
            }
            if response.drag_started() {
                drag_key = Some(key);
            }
            response.context_menu(|ui| {
                ui.label(format!("Keyframe at frame {key}"));
                ui.separator();
                for easing in Easing::ALL {
                    if ui.button(easing.label()).clicked() {
                        menu_action = Some((key, Some(easing)));
                    }
                }
                ui.separator();
                if ui.button("Delete keyframe").clicked() {
                    menu_action = Some((key, None));
                }
            });
        }

        if let Some(key) = clicked_key.or(drag_key) {
            self.selected = Some(id);
            self.selected_key = Some(KeySelection {
                layer: id,
                prop,
                frame: key,
            });
        }
        if let Some(key) = drag_key {
            self.timeline.drag = Some(TimelineDrag::Key {
                layer: id,
                prop,
                frame: key,
            });
        }
        if let Some(key) = jump_to {
            self.set_frame(key);
        }
        let Some(track) = self.project.layer_mut(id).and_then(|l| l.track_mut(prop)) else {
            return;
        };
        if toggle_clicked {
            track.toggle_key(frame);
        }
        match menu_action {
            Some((key, Some(easing))) => track.set_easing(key, easing),
            Some((key, None)) => {
                track.remove_key(key);
                if self
                    .selected_key
                    .is_some_and(|k| k.layer == id && k.prop == prop && k.frame == key)
                {
                    self.selected_key = None;
                }
            }
            None => {}
        }
    }
}

pub fn diamond(painter: &egui::Painter, center: Pos2, radius: f32, fill: Color32, stroke: Stroke) {
    let points = vec![
        center + vec2(0.0, -radius),
        center + vec2(radius, 0.0),
        center + vec2(0.0, radius),
        center + vec2(-radius, 0.0),
    ];
    painter.add(Shape::convex_polygon(points, fill, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_formats_minutes_seconds_frames() {
        assert_eq!(timecode(0, 30), "00:00:00");
        assert_eq!(timecode(95, 30), "00:03:05");
        assert_eq!(timecode(30 * 61 + 2, 30), "01:01:02");
    }

    #[test]
    fn time_scale_round_trip() {
        let scale = TimeScale {
            x0: 100.0,
            points_per_frame: 4.0,
            scroll: 10.0,
        };
        assert_eq!(scale.x(10.0), 100.0);
        assert_eq!(scale.frame_at(scale.x(37.0)), 37.0);
    }
}
