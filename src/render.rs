//! Where layers are on the canvas: geometry, paint order and hit testing,
//! plus the editor-only guides drawn over the rendered frame.

use egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2, vec2};

use crate::model::space::{Affine3, NEAR, Projection, vec3};
use crate::model::{LIGHT_SIZE, Layer, LayerKind, LightKind, NULL_SIZE, Project, ShapeKind};
use crate::text;

/// Maps canvas pixels to screen points.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Screen position of the canvas's top-left corner.
    pub origin: Pos2,
    /// Screen points per canvas pixel.
    pub zoom: f32,
}

impl View {
    pub fn to_screen(self, p: Vec2) -> Pos2 {
        self.origin + p * self.zoom
    }

    pub fn to_canvas(self, p: Pos2) -> Vec2 {
        (p - self.origin) / self.zoom
    }
}

/// Where a layer sits at one frame: how its local pixels reach the canvas.
#[derive(Clone, Copy, Debug)]
pub struct LayerGeom {
    /// Layer-local pixels to world space, parents included.
    pub world: Affine3,
    pub projection: Projection,
    /// Untransformed size of the layer's content.
    pub size: Vec2,
    /// Centre of the content box in layer pixels; only groups are off-centre.
    pub center: Vec2,
    pub anchor: Vec2,
}

impl LayerGeom {
    /// Layer-local pixels (origin at the layer's centre) to canvas pixels.
    /// Points behind the camera are pushed onto its near plane.
    pub fn local_to_canvas(&self, p: Vec2) -> Vec2 {
        let world = self.world.point(vec3(p.x, p.y, 0.0));
        self.projection.project(world).unwrap_or_else(|| {
            // Rare: only reached for handles of a layer partly behind the camera.
            let Projection::Perspective {
                view, zoom, center, ..
            } = self.projection
            else {
                return world.xy();
            };
            let c = view.point(world);
            center + c.xy() * (zoom / NEAR)
        })
    }

    pub fn canvas_to_local(&self, p: Vec2) -> Option<Vec2> {
        self.projection.hit_plane(&self.world, p, 0.0)
    }

    /// Whether every corner is in front of the camera.
    pub fn in_front(&self) -> bool {
        self.local_corners().iter().all(|c| {
            self.projection
                .project(self.world.point(vec3(c.x, c.y, 0.0)))
                .is_some()
        })
    }

    /// The anchor point (the layer's pivot) on the canvas.
    pub fn anchor_canvas(&self) -> Vec2 {
        self.local_to_canvas(self.anchor)
    }

    /// Bounding box corners in layer-local pixels, clockwise from top-left.
    pub fn local_corners(&self) -> [Vec2; 4] {
        let (h, c) = (self.size * 0.5, self.center);
        [
            c + vec2(-h.x, -h.y),
            c + vec2(h.x, -h.y),
            c + vec2(h.x, h.y),
            c + vec2(-h.x, h.y),
        ]
    }

    /// Roughly how many canvas pixels one local pixel covers at the anchor.
    pub fn average_scale(&self) -> f32 {
        let a = self.anchor_canvas();
        let dx = (self.local_to_canvas(self.anchor + vec2(1.0, 0.0)) - a).length();
        let dy = (self.local_to_canvas(self.anchor + vec2(0.0, 1.0)) - a).length();
        (dx + dy) * 0.5
    }

    /// Camera distance of the anchor, for sorting 3D layers.
    pub fn depth(&self) -> f32 {
        self.projection
            .depth(self.world.point(vec3(self.anchor.x, self.anchor.y, 0.0)))
    }
}

pub fn layer_geom(project: &Project, layer: &Layer, frame: f32) -> LayerGeom {
    let world = project.world_matrix(layer, frame);
    let (size, center) = match &layer.kind {
        LayerKind::Shape { size, .. } => (size.sample(frame), Vec2::ZERO),
        LayerKind::Text { text, font_size } => (text::layout(text, *font_size).size, Vec2::ZERO),
        LayerKind::Image { size, .. } => (*size, Vec2::ZERO),
        LayerKind::Null => (Vec2::splat(NULL_SIZE), Vec2::ZERO),
        LayerKind::Light { .. } => (Vec2::splat(LIGHT_SIZE), Vec2::ZERO),
        LayerKind::Adjustment => (
            vec2(project.width as f32, project.height as f32),
            Vec2::ZERO,
        ),
        LayerKind::Camera { .. } | LayerKind::Audio { .. } => (Vec2::ZERO, Vec2::ZERO),
        LayerKind::Group => group_bounds(project, layer, &world, frame, 0)
            .map_or((Vec2::ZERO, layer.transform.anchor), |r| {
                (r.size(), r.center().to_vec2())
            }),
    };
    LayerGeom {
        world,
        projection: project.projection_for(layer, frame.round() as i32),
        size,
        center,
        anchor: layer.transform.anchor,
    }
}

/// The box around a group's contents, in the group's own pixels.
fn group_bounds(
    project: &Project,
    group: &Layer,
    group_world: &Affine3,
    frame: f32,
    depth: usize,
) -> Option<egui::Rect> {
    let inv = group_world.inverse()?;
    if depth > project.layers.len() {
        return None;
    }
    let mut rect: Option<egui::Rect> = None;
    for member in project
        .members(Some(group.id))
        .filter(|l| l.kind.is_visual() && project.is_shown_at(l, frame as i32))
    {
        let world = project.world_matrix(member, frame);
        let corners: Vec<Vec2> = match member.kind {
            LayerKind::Group => {
                let Some(r) = group_bounds(project, member, &world, frame, depth + 1) else {
                    continue;
                };
                [
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                ]
                .map(|p| p.to_vec2())
                .to_vec()
            }
            _ => layer_geom(project, member, frame).local_corners().to_vec(),
        };
        for c in corners {
            let p = inv.point(world.point(vec3(c.x, c.y, 0.0))).xy().to_pos2();
            rect = Some(rect.map_or(egui::Rect::from_min_max(p, p), |r| {
                r.union(egui::Rect::from_min_max(p, p))
            }));
        }
    }
    rect
}

/// Whether canvas point `p` lands on the layer at `frame`.
pub fn hit_test(project: &Project, layer: &Layer, frame: f32, p: Vec2) -> bool {
    if matches!(
        layer.kind,
        LayerKind::Camera { .. } | LayerKind::Audio { .. }
    ) {
        return false;
    }
    if matches!(layer.kind, LayerKind::Group) {
        // A group is hit where any of its contents is.
        return stack(project, Some(layer.id), frame as i32)
            .into_iter()
            .any(|m| hit_test(project, m, frame, p));
    }
    let geom = layer_geom(project, layer, frame);
    if matches!(layer.kind, LayerKind::Light { .. }) {
        // Lights are grabbed by their marker, whichever way they face.
        return geom.in_front() && (geom.anchor_canvas() - p).length() <= LIGHT_SIZE * 0.5;
    }
    let Some(local) = geom.canvas_to_local(p) else {
        return false;
    };
    let local = local - geom.center;
    let h = geom.size * 0.5;
    match &layer.kind {
        LayerKind::Shape {
            shape: ShapeKind::Ellipse,
            ..
        } => {
            if h.x <= 0.0 || h.y <= 0.0 {
                return false;
            }
            (local.x / h.x).powi(2) + (local.y / h.y).powi(2) <= 1.0
        }
        _ => local.x.abs() <= h.x && local.y.abs() <= h.y,
    }
}

/// The active layers directly in `group` (or at the top level), in paint
/// order, back to front. Layers keep their stack order, except that each run
/// of adjacent 3D layers is sorted by distance from the camera so nearer ones
/// cover farther ones.
pub fn stack<'a>(project: &'a Project, group: Option<u64>, frame: i32) -> Vec<&'a Layer> {
    let mut out: Vec<&Layer> = Vec::new();
    let mut run: Vec<(f32, &Layer)> = Vec::new();
    let flush = |run: &mut Vec<(f32, &'a Layer)>, out: &mut Vec<&'a Layer>| {
        run.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.extend(run.drain(..).map(|(_, l)| l));
    };
    for layer in project
        .members(group)
        .filter(|l| l.is_active_at(frame) && !matches!(l.kind, LayerKind::Camera { .. }))
    {
        if layer.is_3d() {
            // Rounded so layers sharing a plane keep their stack order rather
            // than being shuffled by rounding noise.
            let depth = (layer_geom(project, layer, frame as f32).depth() * 1000.0).round();
            run.push((depth, layer));
        } else {
            flush(&mut run, &mut out);
            out.push(layer);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The front-most layer under canvas point `p`, ignoring locked layers.
/// Clicking on a group's contents picks the group.
pub fn pick_layer(project: &Project, frame: i32, p: Vec2) -> Option<u64> {
    stack(project, None, frame)
        .into_iter()
        .rev()
        .filter(|l| !l.locked)
        .find(|l| hit_test(project, l, frame as f32, p))
        .map(|l| l.id)
}

/// Outline of a shape in layer-local pixels, centred on the origin.
pub fn shape_outline(shape: ShapeKind, size: Vec2, corner_radius: f32) -> Vec<Vec2> {
    use std::f32::consts::{FRAC_PI_2, TAU};
    let h = size * 0.5;
    let ring = |n: u32, radius: &dyn Fn(u32) -> Vec2| -> Vec<Vec2> {
        (0..n)
            .map(|i| {
                let a = -FRAC_PI_2 + TAU * i as f32 / n as f32;
                let r = radius(i);
                vec2(a.cos() * r.x, a.sin() * r.y)
            })
            .collect()
    };
    match shape {
        ShapeKind::Rectangle => {
            let r = corner_radius.clamp(0.0, h.x.abs().min(h.y.abs()));
            if r < 0.5 {
                return vec![
                    vec2(-h.x, -h.y),
                    vec2(h.x, -h.y),
                    vec2(h.x, h.y),
                    vec2(-h.x, h.y),
                ];
            }
            const STEPS: u32 = 8;
            let centers = [
                (vec2(h.x - r, -h.y + r), -FRAC_PI_2),
                (vec2(h.x - r, h.y - r), 0.0),
                (vec2(-h.x + r, h.y - r), FRAC_PI_2),
                (vec2(-h.x + r, -h.y + r), std::f32::consts::PI),
            ];
            centers
                .iter()
                .flat_map(|&(c, start)| {
                    (0..=STEPS).map(move |i| {
                        let a = start + FRAC_PI_2 * i as f32 / STEPS as f32;
                        c + vec2(a.cos(), a.sin()) * r
                    })
                })
                .collect()
        }
        ShapeKind::Ellipse => ring(72, &|_| h),
        ShapeKind::Polygon { sides } => ring(sides.max(3), &|_| h),
        ShapeKind::Star {
            points,
            inner_ratio,
        } => ring(points.max(2) * 2, &|i| {
            if i % 2 == 0 { h } else { h * inner_ratio }
        }),
    }
}

/// Editor-only overlays: each active null's box and each light.
pub fn draw_guides(painter: &Painter, view: View, project: &Project, frame: i32) {
    for layer in project
        .layers
        .iter()
        .filter(|l| project.is_shown_at(l, frame))
    {
        match &layer.kind {
            LayerKind::Null => {
                draw_null(painter, view, &layer_geom(project, layer, frame as f32));
            }
            LayerKind::Light { light, .. } => {
                draw_light(
                    painter,
                    view,
                    &layer_geom(project, layer, frame as f32),
                    *light,
                );
            }
            _ => {}
        }
    }
}

/// A light: a small sun at its position and, for lights with a direction,
/// a line showing where it points.
fn draw_light(painter: &Painter, view: View, geom: &LayerGeom, light: LightKind) {
    if !geom.in_front() {
        return;
    }
    let color = Color32::from_rgb(255, 205, 80);
    let c = view.to_screen(geom.anchor_canvas());
    if matches!(light, LightKind::Spot | LightKind::Parallel) {
        let tip = geom
            .projection
            .project(geom.world.point(vec3(0.0, 0.0, LIGHT_SIZE * 2.5)))
            .map(|p| view.to_screen(p));
        if let Some(tip) = tip {
            painter.extend(Shape::dashed_line(
                &[c, tip],
                Stroke::new(1.5, color),
                6.0,
                4.0,
            ));
            painter.circle_filled(tip, 3.0, color);
        }
    }
    painter.circle_filled(c, 7.0, color);
    let stroke = Stroke::new(1.5, color);
    for i in 0..8 {
        let (s, k) = (i as f32 * std::f32::consts::FRAC_PI_4).sin_cos();
        let d = vec2(k, s);
        painter.line_segment([c + d * 10.0, c + d * 15.0], stroke);
    }
}

/// A null's box: a dashed square with a cross through its pivot.
fn draw_null(painter: &Painter, view: View, geom: &LayerGeom) {
    if !geom.in_front() {
        return;
    }
    let to_screen = |p: Vec2| view.to_screen(geom.local_to_canvas(p));
    let color = Color32::from_rgba_unmultiplied(230, 80, 90, 200);
    let corners = geom.local_corners().map(to_screen);
    for i in 0..4 {
        painter.extend(Shape::dashed_line(
            &[corners[i], corners[(i + 1) % 4]],
            Stroke::new(1.5, color),
            6.0,
            4.0,
        ));
    }
    let h = geom.size * 0.5;
    let stroke = Stroke::new(1.0, color);
    painter.line_segment(
        [to_screen(vec2(-h.x, 0.0)), to_screen(vec2(h.x, 0.0))],
        stroke,
    );
    painter.line_segment(
        [to_screen(vec2(0.0, -h.y)), to_screen(vec2(0.0, h.y))],
        stroke,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines_have_expected_vertex_counts() {
        let size = vec2(100.0, 50.0);
        assert_eq!(shape_outline(ShapeKind::Rectangle, size, 0.0).len(), 4);
        assert_eq!(shape_outline(ShapeKind::Rectangle, size, 10.0).len(), 36);
        assert_eq!(
            shape_outline(ShapeKind::Polygon { sides: 6 }, size, 0.0).len(),
            6
        );
        let star = ShapeKind::Star {
            points: 5,
            inner_ratio: 0.5,
        };
        assert_eq!(shape_outline(star, size, 0.0).len(), 10);
    }
}
