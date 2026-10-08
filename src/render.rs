//! Draws a project's layers with egui's painter and answers geometry
//! questions (bounds, hit testing) about them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::epaint::{Mesh, Tessellator, TextShape, Vertex};
use egui::{
    Color32, ColorImage, Context, FontId, Galley, Painter, Pos2, Shape, Stroke, TextureHandle,
    Vec2, vec2,
};

use crate::model::space::{Affine3, NEAR, Projection, vec3};
use crate::model::{Layer, LayerKind, NULL_SIZE, Project, ShapeKind};

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
        let h = self.size * 0.5;
        [
            vec2(-h.x, -h.y),
            vec2(h.x, -h.y),
            vec2(h.x, h.y),
            vec2(-h.x, h.y),
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

pub fn layer_geom(ctx: &Context, project: &Project, layer: &Layer, frame: f32) -> LayerGeom {
    let size = match &layer.kind {
        LayerKind::Shape { size, .. } => size.sample(frame),
        LayerKind::Text { text, font_size } => {
            text_galley(ctx, text, *font_size, Color32::WHITE).size()
        }
        LayerKind::Image { size, .. } => *size,
        LayerKind::Null => Vec2::splat(NULL_SIZE),
        LayerKind::Camera { .. } => Vec2::ZERO,
    };
    LayerGeom {
        world: project.world_matrix(layer, frame),
        projection: project.projection_for(layer, frame.round() as i32),
        size,
        anchor: layer.transform.anchor,
    }
}

fn text_galley(ctx: &Context, text: &str, size: f32, color: Color32) -> Arc<Galley> {
    // Keep glyph rasterisation sane when zoomed far in or out.
    let size = size.clamp(1.0, 1024.0);
    ctx.fonts_mut(|f| {
        f.layout(
            text.to_owned(),
            FontId::proportional(size),
            color,
            f32::INFINITY,
        )
    })
}

/// Whether canvas point `p` lands on the layer at `frame`.
pub fn hit_test(ctx: &Context, project: &Project, layer: &Layer, frame: f32, p: Vec2) -> bool {
    if matches!(layer.kind, LayerKind::Camera { .. }) {
        return false;
    }
    let geom = layer_geom(ctx, project, layer, frame);
    let Some(local) = geom.canvas_to_local(p) else {
        return false;
    };
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

/// Active layers in paint order, back to front. Layers keep their stack
/// order, except that each run of adjacent 3D layers is sorted by distance
/// from the camera so nearer ones cover farther ones.
pub fn paint_order<'a>(ctx: &Context, project: &'a Project, frame: i32) -> Vec<&'a Layer> {
    let mut out: Vec<&Layer> = Vec::new();
    let mut run: Vec<(f32, &Layer)> = Vec::new();
    let flush = |run: &mut Vec<(f32, &'a Layer)>, out: &mut Vec<&'a Layer>| {
        run.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.extend(run.drain(..).map(|(_, l)| l));
    };
    for layer in project
        .layers
        .iter()
        .filter(|l| l.is_active_at(frame) && !matches!(l.kind, LayerKind::Camera { .. }))
    {
        if layer.three_d {
            let depth = layer_geom(ctx, project, layer, frame as f32).depth();
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
pub fn pick_layer(ctx: &Context, project: &Project, frame: i32, p: Vec2) -> Option<u64> {
    paint_order(ctx, project, frame)
        .into_iter()
        .rev()
        .filter(|l| !l.locked)
        .find(|l| hit_test(ctx, project, l, frame as f32, p))
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

/// Decoded images, uploaded to the GPU on first use.
#[derive(Default)]
pub struct TextureCache {
    textures: HashMap<PathBuf, Option<TextureHandle>>,
}

impl TextureCache {
    pub fn insert(&mut self, ctx: &Context, path: &Path, image: ColorImage) {
        let handle = ctx.load_texture(path.to_string_lossy(), image, Default::default());
        self.textures.insert(path.to_owned(), Some(handle));
    }

    /// The texture for `path`, decoding it the first time. A file that fails
    /// to load is remembered so it isn't retried every frame.
    pub fn get(&mut self, ctx: &Context, path: &Path) -> Option<&TextureHandle> {
        if !self.textures.contains_key(path) {
            let handle = load_image(path)
                .ok()
                .map(|img| ctx.load_texture(path.to_string_lossy(), img, Default::default()));
            self.textures.insert(path.to_owned(), handle);
        }
        self.textures.get(path).and_then(Option::as_ref)
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

pub fn load_image(path: &Path) -> Result<ColorImage, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

/// Paints the whole composition at `frame`: background, then layers bottom-up.
/// `guides` also draws editor-only layers (nulls) as outlines.
pub fn draw_project(
    painter: &Painter,
    view: View,
    project: &Project,
    frame: i32,
    textures: &mut TextureCache,
    guides: bool,
) {
    let canvas = egui::Rect::from_min_size(
        view.origin,
        vec2(project.width as f32, project.height as f32) * view.zoom,
    );
    painter.rect_filled(canvas, 0.0, project.background.to_color32());
    let ctx = painter.ctx();
    for layer in paint_order(ctx, project, frame) {
        let geom = layer_geom(ctx, project, layer, frame as f32);
        if layer.kind.is_visual() {
            draw_layer(painter, view, layer, &geom, frame as f32, textures);
        } else if guides {
            draw_null(painter, view, &geom);
        }
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

pub fn draw_layer(
    painter: &Painter,
    view: View,
    layer: &Layer,
    geom: &LayerGeom,
    frame: f32,
    textures: &mut TextureCache,
) {
    let ctx = painter.ctx();
    let opacity = layer.opacity.sample(frame).clamp(0.0, 1.0);
    if opacity <= 0.0 || !geom.in_front() {
        return;
    }
    let to_screen = |p: Vec2| view.to_screen(geom.local_to_canvas(p));
    let fill = layer
        .fill
        .sample(frame)
        .with_alpha_factor(opacity)
        .to_color32();

    match &layer.kind {
        LayerKind::Shape {
            shape,
            size,
            corner_radius,
        } => {
            let outline = shape_outline(*shape, size.sample(frame), corner_radius.sample(frame));
            let points: Vec<Pos2> = outline.iter().map(|&p| to_screen(p)).collect();
            let stroke = if layer.border.enabled {
                let width =
                    layer.border.width.sample(frame).max(0.0) * geom.average_scale() * view.zoom;
                let color = layer.border.color.sample(frame).with_alpha_factor(opacity);
                Stroke::new(width, color.to_color32())
            } else {
                Stroke::NONE
            };
            if matches!(shape, ShapeKind::Star { .. }) || layer.three_d {
                // Stars are concave, and a tilted 3D outline can flip its
                // winding, neither of which egui's polygon fill supports; a
                // triangle fan from the centre fills both correctly.
                let mut mesh = Mesh::default();
                mesh.colored_vertex(to_screen(Vec2::ZERO), fill);
                for &p in &points {
                    mesh.colored_vertex(p, fill);
                }
                let n = points.len() as u32;
                for i in 0..n {
                    mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
                }
                painter.add(mesh);
                // Meshes aren't anti-aliased; a hairline in the fill colour
                // smooths the edge when there's no border to cover it.
                let edge = if stroke.width > 0.0 {
                    stroke
                } else {
                    Stroke::new(1.0, fill)
                };
                painter.add(Shape::closed_line(points, edge));
            } else {
                painter.add(Shape::convex_polygon(points, fill, stroke));
            }
        }
        LayerKind::Text { text, font_size } => {
            // Lay the text out at roughly its on-screen size so glyphs stay
            // sharp, then map each glyph vertex through the layer's
            // transform; this handles rotation, parenting and perspective.
            let raster = (font_size * geom.average_scale() * view.zoom).clamp(1.0, 1024.0);
            let galley = text_galley(ctx, text, raster, fill);
            let to_local = *font_size / raster;
            let half = galley.size() * 0.5;
            let shape = TextShape::new(Pos2::ZERO, galley, fill);
            let mut mesh = Mesh::default();
            let mut tessellator = Tessellator::new(
                ctx.pixels_per_point(),
                Default::default(),
                ctx.fonts(|f| f.font_image_size()),
                Vec::new(),
            );
            tessellator.tessellate_text(&shape, &mut mesh);
            for v in &mut mesh.vertices {
                v.pos = to_screen((v.pos.to_vec2() - half) * to_local);
            }
            painter.add(mesh);
        }
        LayerKind::Image { path, .. } => {
            let corners = geom.local_corners();
            match textures.get(ctx, path) {
                Some(texture) => {
                    // Flat images need one quad; tilted ones are split into a
                    // grid so the texture follows the perspective.
                    let n: u32 = if layer.three_d { 12 } else { 1 };
                    let mut mesh = Mesh::with_texture(texture.id());
                    for j in 0..=n {
                        for i in 0..=n {
                            let uv = vec2(i as f32, j as f32) / n as f32;
                            let local = corners[0] + uv * geom.size;
                            mesh.vertices.push(Vertex {
                                pos: to_screen(local),
                                uv: uv.to_pos2(),
                                color: fill,
                            });
                        }
                    }
                    for j in 0..n {
                        for i in 0..n {
                            let a = j * (n + 1) + i;
                            let b = a + n + 1;
                            mesh.add_triangle(a, a + 1, b + 1);
                            mesh.add_triangle(a, b + 1, b);
                        }
                    }
                    painter.add(mesh);
                }
                None => {
                    // Missing file: draw a placeholder so the layer stays visible.
                    let corners = corners.map(to_screen);
                    let warn = Color32::from_rgb(200, 60, 90);
                    painter.add(Shape::convex_polygon(
                        corners.to_vec(),
                        warn.gamma_multiply(0.3),
                        Stroke::new(2.0, warn),
                    ));
                    painter.line_segment([corners[0], corners[2]], Stroke::new(1.0, warn));
                    painter.line_segment([corners[1], corners[3]], Stroke::new(1.0, warn));
                }
            }
        }
        LayerKind::Null | LayerKind::Camera { .. } => {}
    }
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
