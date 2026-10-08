//! The timeline: transport controls, a time ruler, one row per layer with a
//! draggable/trimmable bar, and expandable property rows showing keyframes.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, vec2,
};

use crate::app::{AetherApp, KeySelection};
use crate::model::Easing;
use crate::ui::icons::{self, Icon};
use crate::ui::theme;

const LABEL_WIDTH: f32 = 230.0;
const RULER_HEIGHT: f32 = 24.0;
const LAYER_ROW: f32 = 36.0;
const EDGE_GRAB: f32 = 6.0;
const TRACK_PAD: f32 = 10.0;

#[derive(Default)]
pub struct TimelineState {
    /// Points per frame; `None` fits the whole composition in view.
    pub zoom: Option<f32>,
    /// First visible frame (fractional) when zoomed in.
    pub scroll_frame: f32,
    drag: Option<TimelineDrag>,
    /// Groups whose contents are folded away.
    pub collapsed: std::collections::HashSet<u64>,
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
    /// All of a layer's keys at one frame, moved together.
    Key {
        layer: u64,
        frame: i32,
    },
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
        // Frame 0 sits a little right of the label column so its keyframes
        // aren't cut in half.
        let x0 = full.left() + LABEL_WIDTH + TRACK_PAD;
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

    /// Timecode on the left, playback controls centred, timeline zoom on the right.
    fn transport_ui(&mut self, ui: &mut Ui) {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
        let row_layout = |layout| egui::UiBuilder::new().max_rect(row).layout(layout);

        ui.scope_builder(
            row_layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(timecode(self.frame, self.project.fps))
                        .monospace()
                        .size(15.0)
                        .color(ui.visuals().strong_text_color()),
                );
                ui.label(
                    egui::RichText::new(format!(
                        "/ {}",
                        timecode(self.project.duration, self.project.fps)
                    ))
                    .monospace()
                    .weak(),
                );
                let mut frame = self.frame;
                if ui
                    .add(
                        egui::DragValue::new(&mut frame)
                            .range(0..=self.project.duration - 1)
                            .prefix("f "),
                    )
                    .on_hover_text("Current frame")
                    .changed()
                {
                    self.set_frame(frame);
                }
            },
        );

        let center = Rect::from_center_size(row.center(), vec2(5.0 * 34.0 + 48.0, row.height()));
        let center_layout = egui::UiBuilder::new()
            .max_rect(center)
            .layout(egui::Layout::left_to_right(egui::Align::Center));
        ui.scope_builder(center_layout, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if icons::button(ui, Icon::ToStart, "First frame (Home)").clicked() {
                self.set_frame(0);
            }
            if icons::button(ui, Icon::PrevFrame, "Previous frame (←)").clicked() {
                self.set_frame(self.frame - 1);
            }
            let (rect, play) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
            let fill = if play.hovered() {
                theme::ACCENT
            } else {
                theme::ACCENT_SOFT
            };
            ui.painter().circle_filled(rect.center(), 19.0, fill);
            let glyph = if self.playing {
                Icon::Pause
            } else {
                Icon::Play
            };
            icons::paint(ui.painter(), rect.shrink(12.0), glyph, Color32::WHITE);
            if play
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Play / pause (Space)")
                .clicked()
            {
                self.toggle_playback();
            }
            if icons::button(ui, Icon::NextFrame, "Next frame (→)").clicked() {
                self.set_frame(self.frame + 1);
            }
            if icons::button(ui, Icon::ToEnd, "Last frame (End)").clicked() {
                self.set_frame(self.project.duration - 1);
            }
            if icons::toggle(ui, Icon::Loop, "Loop playback", self.looping).clicked() {
                self.looping = !self.looping;
            }
        });

        ui.scope_builder(
            row_layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.add_space(6.0);
                if icons::toggle(
                    ui,
                    Icon::Fit,
                    "Fit the whole timeline (Ctrl+scroll zooms)",
                    self.timeline.zoom.is_none(),
                )
                .clicked()
                {
                    self.timeline.zoom = None;
                }
            },
        );
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
                if delta != applied && self.project.layer(id).is_some() {
                    self.project.shift_layer_in_time(id, delta - applied);
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
            TimelineDrag::Key { layer, frame: from } => {
                let frame = frame.max(0);
                if frame != from
                    && let Some(l) = self.project.layer_mut(layer)
                    && l.move_keys_at(from, frame)
                {
                    self.timeline.drag = Some(TimelineDrag::Key { layer, frame });
                    self.selected_key = Some(KeySelection { layer, frame });
                    // Keep the preview on the key being moved.
                    self.set_frame(frame);
                }
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
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

        let track = Rect::from_x_y_ranges(scale.x0 - TRACK_PAD..=rect.right(), rect.y_range());
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

        // Keep the label column plain; the transport bar shows the time.
        let label_rect = Rect::from_x_y_ranges(rect.left()..=scale.x0 - TRACK_PAD, rect.y_range());
        painter.rect_filled(label_rect, 0.0, ui.visuals().panel_fill);

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
            && p.x >= scale.x0 - TRACK_PAD
        {
            self.set_frame(scale.frame_at(p.x).round() as i32);
            self.timeline.drag = Some(TimelineDrag::Scrub);
        }
    }

    fn rows_ui(&mut self, ui: &mut Ui, scale: TimeScale) {
        let mut rows = Vec::new();
        self.collect_rows(None, 0, &mut rows);
        let content_height = rows.len() as f32 * LAYER_ROW;

        let size = vec2(
            ui.available_width(),
            content_height.max(ui.available_height()),
        );
        let (rect, background) = ui.allocate_exact_size(size, Sense::click());
        if background.clicked() {
            self.select(None);
        }
        let painter = ui.painter_at(rect);
        let track_clip = Rect::from_x_y_ranges(scale.x0 - TRACK_PAD..=rect.right(), rect.y_range())
            .intersect(painter.clip_rect());
        let track_painter = painter.with_clip_rect(track_clip);

        // Shade time past the end of the composition.
        let end_x = scale.x(self.project.duration as f32);
        if end_x < rect.right() {
            let past = Rect::from_x_y_ranges(
                end_x.max(scale.x0 - TRACK_PAD)..=rect.right(),
                rect.y_range(),
            );
            track_painter.rect_filled(past, 0.0, Color32::from_black_alpha(90));
        }
        // Label column divider.
        painter.line_segment(
            [
                Pos2::new(scale.x0 - TRACK_PAD, rect.top()),
                Pos2::new(scale.x0 - TRACK_PAD, rect.bottom()),
            ],
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        if rows.is_empty() {
            painter.text(
                Pos2::new(rect.left() + 12.0, rect.top() + 16.0),
                Align2::LEFT_CENTER,
                "No layers yet. Click the + button on the canvas to add one.",
                FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        }

        let mut y = rect.top();
        for &(id, depth) in &rows {
            let row_rect = Rect::from_x_y_ranges(rect.x_range(), y..=y + LAYER_ROW);
            self.layer_row(
                ui,
                &painter,
                &track_painter,
                track_clip,
                row_rect,
                scale,
                id,
                depth,
            );
            y += LAYER_ROW;
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

    /// Timeline rows, front-most first, with each group's contents (unless
    /// folded) right under it, one level deeper.
    fn collect_rows(&self, group: Option<u64>, depth: usize, out: &mut Vec<(u64, usize)>) {
        let members: Vec<&crate::model::Layer> = self.project.members(group).collect();
        for layer in members.into_iter().rev() {
            out.push((layer.id, depth));
            if matches!(layer.kind, crate::model::LayerKind::Group)
                && !self.timeline.collapsed.contains(&layer.id)
                && depth < 16
            {
                self.collect_rows(Some(layer.id), depth + 1, out);
            }
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
        depth: usize,
    ) {
        let Some(layer) = self.project.layer(id) else {
            return;
        };
        let is_group = matches!(layer.kind, crate::model::LayerKind::Group);
        let selected = self.selected == Some(id);
        let (visible, locked) = (layer.visible, layer.locked);
        let color = theme::layer_color(&layer.kind);
        let name = layer.name.clone();
        let icon = theme::layer_icon(&layer.kind);
        let row_hovered = ui.rect_contains_pointer(row);
        let (in_frame, out_frame) = (layer.in_frame, layer.out_frame);
        let key_frames = layer.all_key_frames();

        let label_area = Rect::from_x_y_ranges(
            row.left() + 2.0..=scale.x0 - TRACK_PAD - 4.0,
            row.top() + 2.0..=row.bottom() - 2.0,
        );
        if selected {
            painter.rect_filled(
                label_area,
                CornerRadius::same(8),
                theme::ACCENT_SOFT.gamma_multiply(0.55),
            );
        } else if row_hovered {
            painter.rect_filled(
                label_area,
                CornerRadius::same(8),
                ui.visuals()
                    .widgets
                    .hovered
                    .weak_bg_fill
                    .gamma_multiply(0.5),
            );
        }

        // --- Label column: expand, visibility, lock, name.
        let text_color = if visible {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let mut x = row.left() + 6.0 + depth as f32 * 14.0;
        if is_group {
            // Fold / unfold the group's rows.
            let folded = self.timeline.collapsed.contains(&id);
            let r = Rect::from_center_size(Pos2::new(x + 8.0, row.center().y), Vec2::splat(18.0));
            let response = ui
                .interact(r, Id::new(("fold", id)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            let c = if response.hovered() {
                Color32::WHITE
            } else {
                ui.visuals().text_color()
            };
            let glyph = if folded { Icon::Right } else { Icon::Down };
            icons::paint(painter, r.shrink(4.0), glyph, c);
            if response.clicked() && !self.timeline.collapsed.remove(&id) {
                self.timeline.collapsed.insert(id);
            }
        }
        x += 16.0;
        let mut small_button =
            |ui: &mut Ui, glyph: Icon, salt: &str, active: bool, always: bool| {
                let r =
                    Rect::from_center_size(Pos2::new(x + 11.0, row.center().y), Vec2::splat(22.0));
                x += 24.0;
                let response = ui.interact(r, Id::new((salt, id)), Sense::click());
                // Visibility and lock icons only show on hover unless they're
                // switched away from the default, which keeps rows quiet.
                if always || row_hovered || !active {
                    let c = if response.hovered() {
                        Color32::WHITE
                    } else if active {
                        text_color
                    } else {
                        theme::PLAYHEAD.gamma_multiply(0.9)
                    };
                    icons::paint(painter, r.shrink(5.0), glyph, c);
                }
                response.on_hover_cursor(CursorIcon::PointingHand)
            };
        let eye = if visible { Icon::Eye } else { Icon::EyeOff };
        let toggle_visible = small_button(ui, eye, "visible", visible, false)
            .on_hover_text("Show / hide")
            .clicked();
        let lock = if locked { Icon::Lock } else { Icon::Unlock };
        let toggle_lock = small_button(ui, lock, "lock", !locked, false)
            .on_hover_text("Lock / unlock")
            .clicked();

        // A coloured "thumbnail" chip with the layer type's icon, then the name.
        let chip = Rect::from_center_size(Pos2::new(x + 13.0, row.center().y), Vec2::splat(24.0));
        painter.rect_filled(
            chip,
            CornerRadius::same(6),
            color.gamma_multiply(if visible { 0.9 } else { 0.35 }),
        );
        icons::paint(painter, chip.shrink(5.0), icon, Color32::WHITE);
        let name_rect = Rect::from_x_y_ranges(
            chip.right() + 8.0..=scale.x0 - TRACK_PAD - 8.0,
            row.y_range(),
        );
        let name_response = ui.interact(
            Rect::from_x_y_ranges(chip.left()..=scale.x0 - TRACK_PAD - 4.0, row.y_range()),
            Id::new(("name", id)),
            Sense::click(),
        );
        painter.with_clip_rect(name_rect).text(
            name_rect.left_center(),
            Align2::LEFT_CENTER,
            &name,
            FontId::proportional(13.5),
            text_color,
        );
        if name_response.clicked() {
            self.select(Some(id));
        }

        // --- Track: the layer's bar from in to out.
        let bar = Rect::from_x_y_ranges(
            scale.x(in_frame as f32)..=scale.x(out_frame as f32),
            row.top() + 5.0..=row.bottom() - 5.0,
        );
        let fill = if visible {
            color.gamma_multiply(0.75)
        } else {
            color.gamma_multiply(0.3)
        };
        track_painter.rect_filled(bar, CornerRadius::same(7), fill);
        if selected {
            track_painter.rect_stroke(
                bar,
                CornerRadius::same(7),
                Stroke::new(1.5, Color32::WHITE),
                StrokeKind::Inside,
            );
        }
        track_painter
            .with_clip_rect(bar.intersect(track_clip))
            .text(
                bar.left_center() + vec2(10.0, 0.0),
                Align2::LEFT_CENTER,
                &name,
                FontId::proportional(12.0),
                Color32::WHITE,
            );

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

        // Keyframes sit on the bar itself, as in Alight Motion: click one to
        // select it and jump to it, drag to retime it, right-click for easing.
        let mut clicked_key = None;
        let mut drag_key = None;
        let mut menu_action = None;
        for &key in &key_frames {
            let center = Pos2::new(scale.x(key as f32), bar.center().y);
            let is_selected = self.selected_key
                == Some(KeySelection {
                    layer: id,
                    frame: key,
                });
            let (radius, fill, stroke) = if is_selected {
                (7.0, theme::KEYFRAME, Stroke::new(2.0, Color32::WHITE))
            } else if selected {
                (
                    6.0,
                    theme::KEYFRAME,
                    Stroke::new(1.0, Color32::from_black_alpha(140)),
                )
            } else {
                (
                    5.0,
                    Color32::WHITE,
                    Stroke::new(1.0, Color32::from_black_alpha(120)),
                )
            };
            diamond(track_painter, center, radius, fill, stroke);

            let hit = Rect::from_center_size(center, Vec2::splat(16.0)).intersect(track_clip);
            if !hit.is_positive() || locked {
                continue;
            }
            let response = ui
                .interact(hit, Id::new(("key", id, key)), Sense::click_and_drag())
                .on_hover_cursor(CursorIcon::PointingHand);
            if response.clicked() {
                clicked_key = Some(key);
            }
            if response.drag_started() {
                drag_key = Some(key);
            }
            response.context_menu(|ui| {
                ui.label(
                    egui::RichText::new(format!("Keyframe at {}", timecode(key, self.project.fps)))
                        .strong(),
                );
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
            self.select(Some(id));
            self.selected_key = Some(KeySelection {
                layer: id,
                frame: key,
            });
            self.set_frame(key);
        }
        if let Some(key) = drag_key {
            self.timeline.drag = Some(TimelineDrag::Key {
                layer: id,
                frame: key,
            });
        }

        if let Some(layer) = self.project.layer_mut(id) {
            match menu_action {
                Some((key, Some(easing))) => layer.set_easing_at(key, easing),
                Some((key, None)) => {
                    layer.remove_keys_at(key);
                    if self.selected_key
                        == Some(KeySelection {
                            layer: id,
                            frame: key,
                        })
                    {
                        self.selected_key = None;
                    }
                }
                None => {}
            }
            if toggle_visible {
                layer.visible = !layer.visible;
            }
            if toggle_lock {
                layer.locked = !layer.locked;
            }
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
