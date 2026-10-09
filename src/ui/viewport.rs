//! The canvas preview with on-canvas move, scale and rotate handles.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, PointerButton, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, Ui, UiBuilder, Vec2, vec2,
};

use crate::app::AetherApp;
use crate::compose;
use crate::model::{LayerKind, Project};
use crate::render::{self, View};
use crate::ui::icons::{self, Icon};
use crate::ui::theme;

const HANDLE_RADIUS: f32 = 6.0;
const ROTATE_HANDLE_OFFSET: f32 = 28.0;

/// The rendered frame shown on the canvas, re-rendered only when the
/// project, the frame or the zoom changes.
#[derive(Default)]
pub struct Preview {
    texture: Option<egui::TextureHandle>,
    key: Option<(Project, i32, u32)>,
}

pub struct ViewportState {
    /// When true the canvas zooms to fit the panel.
    pub fit: bool,
    pub zoom: f32,
    /// Offset of the canvas centre from the panel centre, in screen points.
    pub pan: Vec2,
    drag: Option<GizmoDrag>,
    preview: Preview,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            fit: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
            drag: None,
            preview: Preview::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum GizmoDrag {
    Move {
        id: u64,
        start_position: Vec2,
        /// Where the pointer first hit the layer's plane, in parent space.
        start_hit: Vec2,
    },
    Scale {
        id: u64,
        start_scale: Vec2,
        /// The dragged corner relative to the anchor, in unscaled local pixels.
        corner: Vec2,
    },
    Rotate {
        id: u64,
        start_rotation: f32,
        start_angle: f32,
    },
}

#[derive(Clone, Copy)]
enum Handle {
    Corner(Vec2),
    Rotate,
}

impl AetherApp {
    pub fn viewport_ui(&mut self, ui: &mut Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

        let canvas_size = vec2(self.project.width as f32, self.project.height as f32);
        let fit_zoom = ((rect.width() - 48.0) / canvas_size.x)
            .min((rect.height() - 48.0) / canvas_size.y)
            .max(0.01);
        if self.viewport.fit {
            self.viewport.zoom = fit_zoom;
            self.viewport.pan = Vec2::ZERO;
        }
        self.handle_zoom_and_pan(ui, rect, &response);
        let view = View {
            origin: rect.center() + self.viewport.pan - canvas_size * self.viewport.zoom * 0.5,
            zoom: self.viewport.zoom,
        };

        self.draw_preview(ui, &painter, view);
        render::draw_guides(&painter, view, &self.project, self.frame);
        draw_outside_dim(&painter, rect, view, &self.project);

        if self.project.layers.is_empty() {
            painter.text(
                view.to_screen(canvas_size * 0.5),
                Align2::CENTER_CENTER,
                "Add a shape, text or image to start",
                FontId::proportional(18.0),
                Color32::from_gray(140),
            );
        }

        self.handle_gizmo(ui, view, &response);
        self.draw_selection(&painter, view);
        self.viewport_overlay(ui, rect);
    }

    /// Paints the composited frame onto the canvas, rendering it first if
    /// anything that affects it has changed.
    fn draw_preview(&mut self, ui: &Ui, painter: &egui::Painter, view: View) {
        // Render at screen resolution, but never above 2× the project size.
        let mut scale = (view.zoom * ui.ctx().pixels_per_point()).clamp(0.05, 2.0);
        let longest = self.project.width.max(self.project.height) as f32;
        scale = scale.min(4096.0 / longest);
        // Quantise so tiny zoom changes don't force a re-render.
        let scale = (scale * 64.0).round() / 64.0;
        let preview = &mut self.viewport.preview;
        let fresh = preview.key.as_ref().is_some_and(|(project, frame, s)| {
            *frame == self.frame && *s == scale.to_bits() && *project == self.project
        });
        if !fresh || preview.texture.is_none() {
            let pixmap = compose::render(&self.project, self.frame, scale, &mut self.assets);
            let image = compose::to_color_image(&pixmap);
            match &mut preview.texture {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => {
                    preview.texture = Some(ui.ctx().load_texture(
                        "canvas-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ))
                }
            }
            preview.key = Some((self.project.clone(), self.frame, scale.to_bits()));
        }
        if let Some(texture) = &preview.texture {
            let canvas = Rect::from_min_size(
                view.origin,
                vec2(self.project.width as f32, self.project.height as f32) * view.zoom,
            );
            painter.image(
                texture.id(),
                canvas,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    fn handle_zoom_and_pan(&mut self, ui: &Ui, rect: Rect, response: &egui::Response) {
        if response.dragged_by(PointerButton::Middle)
            || response.dragged_by(PointerButton::Secondary)
        {
            self.viewport.pan += response.drag_delta();
            self.viewport.fit = false;
        }
        let Some(pointer) = response.hover_pos() else {
            return;
        };
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = pinch * (scroll * 0.0015).exp();
        if (factor - 1.0).abs() > 1e-4 {
            let old = self.viewport.zoom;
            let new = (old * factor).clamp(0.02, 32.0);
            // Keep the canvas point under the pointer fixed while zooming.
            let from_center = pointer - rect.center() - self.viewport.pan;
            self.viewport.pan += from_center - from_center * (new / old);
            self.viewport.zoom = new;
            self.viewport.fit = false;
        }
    }

    fn handle_gizmo(&mut self, ui: &Ui, view: View, response: &egui::Response) {
        let ctx = ui.ctx().clone();
        let frame = self.frame;
        let shift = ui.input(|i| i.modifiers.shift);

        if let Some(pointer) = response.hover_pos()
            && self.viewport.drag.is_none()
        {
            match self.handle_at(view, pointer) {
                Some(Handle::Rotate) => ctx.set_cursor_icon(CursorIcon::Alias),
                Some(Handle::Corner(_)) => ctx.set_cursor_icon(CursorIcon::Crosshair),
                None => {}
            }
        }

        if response.clicked() {
            let picked = response
                .interact_pointer_pos()
                .and_then(|p| render::pick_layer(&self.project, frame, view.to_canvas(p)));
            self.select(picked);
        }

        // Start from where the button went down, not where the drag was
        // recognised a few pixels later, so the layer stays under the pointer.
        if response.drag_started_by(PointerButton::Primary)
            && let Some(pointer) = ui
                .input(|i| i.pointer.press_origin())
                .or(response.interact_pointer_pos())
        {
            self.viewport.drag = self.start_gizmo_drag(view, pointer);
        }

        if response.dragged_by(PointerButton::Primary)
            && let (Some(drag), Some(pointer)) =
                (self.viewport.drag, response.interact_pointer_pos())
        {
            let pointer_canvas = view.to_canvas(pointer);
            match drag {
                GizmoDrag::Move {
                    id,
                    start_position,
                    start_hit,
                } => {
                    if let Some(hit) = self.parent_plane_hit(id, pointer_canvas)
                        && let Some(layer) = self.project.layer_mut(id)
                    {
                        let position = start_position + hit - start_hit;
                        layer.transform.position.set(frame, position);
                    }
                }
                GizmoDrag::Scale {
                    id,
                    start_scale,
                    corner,
                } => {
                    // The pointer in the layer's own axes, scaled but not
                    // rotated, so it compares directly with the corner.
                    let local = self.project.layer(id).and_then(|layer| {
                        let geom = render::layer_geom(&self.project, layer, frame as f32);
                        let s = layer.transform.scale.sample(frame as f32);
                        Some((geom.canvas_to_local(pointer_canvas)? - geom.anchor) * s)
                    });
                    if let Some(local) = local
                        && let Some(layer) = self.project.layer_mut(id)
                    {
                        let scale = if shift {
                            let start_len = (corner * start_scale).length().max(1e-3);
                            let along = local.dot((corner * start_scale) / start_len);
                            start_scale * (along / start_len)
                        } else {
                            let axis =
                                |l: f32, c: f32, s: f32| if c.abs() < 1e-3 { s } else { l / c };
                            vec2(
                                axis(local.x, corner.x, start_scale.x),
                                axis(local.y, corner.y, start_scale.y),
                            )
                        };
                        layer.transform.scale.set(frame, scale);
                    }
                }
                GizmoDrag::Rotate {
                    id,
                    start_rotation,
                    start_angle,
                } => {
                    let center = self.project.layer(id).map(|layer| {
                        let geom = render::layer_geom(&self.project, layer, frame as f32);
                        view.to_screen(geom.anchor_canvas())
                    });
                    if let Some(center) = center
                        && let Some(layer) = self.project.layer_mut(id)
                    {
                        let d = pointer - center;
                        let mut rotation =
                            start_rotation + (d.y.atan2(d.x) - start_angle).to_degrees();
                        if shift {
                            rotation = (rotation / 15.0).round() * 15.0;
                        }
                        layer.transform.rotation.set(frame, rotation);
                    }
                }
            }
        }

        if response.drag_stopped() {
            self.viewport.drag = None;
        }
    }

    /// The selected layer's geometry, if it can be edited on the canvas.
    fn editable_selection(&self) -> Option<(u64, render::LayerGeom)> {
        let layer = self.selected_layer()?;
        if !layer.is_active_at(self.frame)
            || layer.locked
            || matches!(
                layer.kind,
                LayerKind::Camera { .. } | LayerKind::Audio { .. }
            )
        {
            return None;
        }
        let geom = render::layer_geom(&self.project, layer, self.frame as f32);
        geom.in_front().then_some((layer.id, geom))
    }

    /// Where the line through canvas point `p` crosses the plane the layer
    /// moves in, expressed in its parent's space. Moving the layer's
    /// position by the change in this point keeps it under the pointer,
    /// whatever its parents and the camera are doing.
    fn parent_plane_hit(&self, id: u64, p: Vec2) -> Option<Vec2> {
        let layer = self.project.layer(id)?;
        let frame = self.frame as f32;
        let parent = self.project.parent_matrix(layer, frame);
        let z = if layer.is_3d() {
            layer.transform.z.sample(frame)
        } else {
            0.0
        };
        self.project
            .projection_for(layer, self.frame)
            .hit_plane(&parent, p, z)
    }

    fn handle_at(&self, view: View, pointer: Pos2) -> Option<Handle> {
        let (id, geom) = self.editable_selection()?;
        if self.project.layer(id).is_some_and(is_light) {
            return None;
        }
        if pointer.distance(rotate_handle_pos(view, &geom)) <= HANDLE_RADIUS + 3.0 {
            return Some(Handle::Rotate);
        }
        geom.local_corners()
            .into_iter()
            .find(|&c| {
                pointer.distance(view.to_screen(geom.local_to_canvas(c))) <= HANDLE_RADIUS + 3.0
            })
            .map(Handle::Corner)
    }

    fn start_gizmo_drag(&mut self, view: View, pointer: Pos2) -> Option<GizmoDrag> {
        let frame = self.frame as f32;
        if let Some(handle) = self.handle_at(view, pointer) {
            let layer = self.selected_layer()?;
            let t = &layer.transform;
            return Some(match handle {
                Handle::Corner(corner) => GizmoDrag::Scale {
                    id: layer.id,
                    start_scale: t.scale.sample(frame),
                    corner: corner - t.anchor,
                },
                Handle::Rotate => {
                    let geom = render::layer_geom(&self.project, layer, frame);
                    let d = pointer - view.to_screen(geom.anchor_canvas());
                    GizmoDrag::Rotate {
                        id: layer.id,
                        start_rotation: t.rotation.sample(frame),
                        start_angle: d.y.atan2(d.x),
                    }
                }
            });
        }

        let pointer_canvas = view.to_canvas(pointer);
        // Dragging inside the current selection moves it even if another layer
        // is on top; otherwise pick whatever is under the pointer.
        let id = self
            .editable_selection()
            .filter(|(id, _)| {
                self.project
                    .layer(*id)
                    .is_some_and(|l| render::hit_test(&self.project, l, frame, pointer_canvas))
            })
            .map(|(id, _)| id)
            .or_else(|| render::pick_layer(&self.project, self.frame, pointer_canvas));
        self.select(id);
        let layer = self.project.layer(id?)?;
        Some(GizmoDrag::Move {
            id: layer.id,
            start_position: layer.transform.position.sample(frame),
            start_hit: self.parent_plane_hit(layer.id, pointer_canvas)?,
        })
    }

    fn draw_selection(&self, painter: &egui::Painter, view: View) {
        let Some(layer) = self.selected_layer() else {
            return;
        };
        if !layer.is_active_at(self.frame)
            || matches!(
                layer.kind,
                LayerKind::Camera { .. } | LayerKind::Audio { .. }
            )
        {
            return;
        }
        let geom = render::layer_geom(&self.project, layer, self.frame as f32);
        if !geom.in_front() {
            return;
        }
        if is_light(layer) {
            // Lights only move; their marker gets a ring.
            let c = view.to_screen(geom.anchor_canvas());
            painter.circle_stroke(c, 20.0, Stroke::new(1.5, theme::ACCENT));
            return;
        }
        let corners = geom
            .local_corners()
            .map(|c| view.to_screen(geom.local_to_canvas(c)));
        let outline = Stroke::new(1.5, theme::ACCENT);
        painter.add(egui::Shape::closed_line(corners.to_vec(), outline));

        // Anchor point.
        let anchor = view.to_screen(geom.anchor_canvas());
        painter.circle_stroke(anchor, 5.0, Stroke::new(1.5, Color32::WHITE));
        painter.line_segment(
            [anchor - vec2(8.0, 0.0), anchor + vec2(8.0, 0.0)],
            Stroke::new(1.0, Color32::WHITE),
        );
        painter.line_segment(
            [anchor - vec2(0.0, 8.0), anchor + vec2(0.0, 8.0)],
            Stroke::new(1.0, Color32::WHITE),
        );

        if layer.locked {
            return;
        }
        let top_mid = corners[0].lerp(corners[1], 0.5);
        let rotate_handle = rotate_handle_pos(view, &geom);
        painter.line_segment([top_mid, rotate_handle], outline);
        painter.circle(rotate_handle, HANDLE_RADIUS, Color32::WHITE, outline);
        for c in corners {
            let r = Rect::from_center_size(c, Vec2::splat(HANDLE_RADIUS * 2.0));
            painter.rect(
                r,
                CornerRadius::same(2),
                Color32::WHITE,
                outline,
                StrokeKind::Middle,
            );
        }
    }

    /// Floating controls over the canvas: zoom in the bottom-left corner and
    /// the round add-layer button in the bottom-right.
    fn viewport_overlay(&mut self, ui: &mut Ui, rect: Rect) {
        let pill = Rect::from_min_size(rect.left_bottom() + vec2(12.0, -46.0), vec2(220.0, 34.0));
        ui.scope_builder(UiBuilder::new().max_rect(pill), |ui| {
            egui::Frame::NONE
                .fill(theme::BG_PANEL.gamma_multiply(0.92))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(4, 2))
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if icons::icon_button(
                            ui,
                            Icon::Fit,
                            "Fit to window",
                            self.viewport.fit,
                            true,
                            26.0,
                        )
                        .clicked()
                        {
                            self.viewport.fit = true;
                        }
                        let actual = egui::Button::new(RichText::new("1:1").small()).frame(false);
                        if ui.add(actual).on_hover_text("Actual size").clicked() {
                            self.viewport.fit = false;
                            self.viewport.zoom = 1.0;
                            self.viewport.pan = Vec2::ZERO;
                        }
                        ui.label(
                            RichText::new(format!("{:.0}%", self.viewport.zoom * 100.0))
                                .small()
                                .weak(),
                        );
                        ui.add_space(4.0);
                    });
                });
        });

        let plus =
            Rect::from_center_size(rect.right_bottom() + vec2(-44.0, -44.0), Vec2::splat(52.0));
        self.add_layer_button(ui, plus);
    }
}

fn rotate_handle_pos(view: View, geom: &render::LayerGeom) -> Pos2 {
    let corners = geom
        .local_corners()
        .map(|c| view.to_screen(geom.local_to_canvas(c)));
    let top_mid = corners[0].lerp(corners[1], 0.5);
    let bottom_mid = corners[3].lerp(corners[2], 0.5);
    let up = (top_mid - bottom_mid).normalized();
    // A zero-height layer has no "up"; fall back to the rotated screen up.
    let up = if up.is_finite() && up != Vec2::ZERO {
        up
    } else {
        vec2(0.0, -1.0)
    };
    top_mid + up * ROTATE_HANDLE_OFFSET
}

/// Darkens everything outside the composition so off-canvas content reads as
/// out of frame, like the pasteboard in other editors.
fn draw_outside_dim(painter: &egui::Painter, rect: Rect, view: View, project: &Project) {
    let canvas = Rect::from_min_size(
        view.origin,
        vec2(project.width as f32, project.height as f32) * view.zoom,
    );
    let dim = Color32::from_black_alpha(170);
    let pieces = [
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, canvas.min.y)),
        Rect::from_min_max(Pos2::new(rect.min.x, canvas.max.y), rect.max),
        Rect::from_min_max(
            Pos2::new(rect.min.x, canvas.min.y),
            Pos2::new(canvas.min.x, canvas.max.y),
        ),
        Rect::from_min_max(
            Pos2::new(canvas.max.x, canvas.min.y),
            Pos2::new(rect.max.x, canvas.max.y),
        ),
    ];
    for piece in pieces {
        let piece = piece.intersect(rect);
        if piece.is_positive() {
            painter.rect_filled(piece, 0.0, dim);
        }
    }
    painter.rect_stroke(
        canvas,
        0.0,
        Stroke::new(1.0, Color32::from_gray(70)),
        StrokeKind::Outside,
    );
}

fn is_light(layer: &crate::model::Layer) -> bool {
    matches!(layer.kind, LayerKind::Light { .. })
}
