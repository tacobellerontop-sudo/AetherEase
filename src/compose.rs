//! Renders a project frame to pixels with tiny-skia.
//!
//! This one renderer feeds the editor preview, home-screen thumbnails and
//! export, so what you see while editing is what gets exported. Each layer
//! is drawn straight onto the frame when it can be; layers that blend
//! differently are drawn on their own first and then blended in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Vec2, vec2};
use tiny_skia::{
    FillRule, FilterQuality, GradientStop, LineJoin, LinearGradient, Paint, PathBuilder, Pattern,
    Pixmap, PixmapPaint, Point, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use crate::light::{self, Light};
use crate::model::{BlendMode, Color, EffectKind, FillStyle, Layer, LayerKind, Project};
use crate::path::{self, Seg};
use crate::render::{self, LayerGeom};
use crate::text;
use crate::video::Videos;

/// Decoded images, shared by every render.
#[derive(Default)]
pub struct Assets {
    images: HashMap<PathBuf, Option<Arc<Pixmap>>>,
    videos: Videos,
}

impl Assets {
    pub fn insert(&mut self, path: &Path, image: Pixmap) {
        self.images.insert(path.to_owned(), Some(Arc::new(image)));
    }

    /// The image at `path`, decoding it the first time. A file that fails to
    /// load is remembered so it isn't retried every frame.
    pub fn image(&mut self, path: &Path) -> Option<Arc<Pixmap>> {
        self.images
            .entry(path.to_owned())
            .or_insert_with(|| load_image(path).ok().map(Arc::new))
            .clone()
    }

    /// Frame `index` of a video clip; see [`Videos::frame`].
    pub fn video_frame(
        &mut self,
        path: &Path,
        index: i64,
        rate: f32,
        count: i64,
        size: Vec2,
    ) -> Option<Arc<Pixmap>> {
        let (w, h) = (size.x.round() as u32, size.y.round() as u32);
        self.videos.frame(path, index, rate, count, w, h)
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.videos.clear();
    }
}

/// Decodes an image file into premultiplied pixels.
pub fn load_image(path: &Path) -> Result<Pixmap, String> {
    let img = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut data = img.into_raw();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u16;
        for c in &mut px[..3] {
            *c = ((*c as u16 * a + 127) / 255) as u8;
        }
    }
    let size = tiny_skia::IntSize::from_wh(w, h).ok_or("the image is empty")?;
    Pixmap::from_vec(data, size).ok_or_else(|| "the image is too large".into())
}

/// Renders `frame` at `scale` output pixels per canvas pixel.
pub fn render(project: &Project, frame: i32, scale: f32, assets: &mut Assets) -> Pixmap {
    let w = ((project.width as f32 * scale).round() as u32).max(1);
    let h = ((project.height as f32 * scale).round() as u32).max(1);
    let mut layers = Pixmap::new(w, h).expect("non-zero size");
    let lights = light::lights_at(project, frame);
    render_stack(&mut layers, project, None, frame, scale, &lights, assets);
    // The background goes underneath last, so a top-level mask reveals it
    // rather than cutting holes in it.
    let mut out = Pixmap::new(w, h).expect("non-zero size");
    out.fill(color(project.background, 1.0));
    out.draw_pixmap(
        0,
        0,
        layers.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    out
}

/// Composites the layers directly in `group` (or the top level) onto `out`,
/// recursing into nested groups.
fn render_stack(
    out: &mut Pixmap,
    project: &Project,
    group: Option<u64>,
    frame: i32,
    scale: f32,
    lights: &[Light],
    assets: &mut Assets,
) {
    let (w, h) = (out.width(), out.height());
    let fps = project.fps.max(1) as f32;
    for layer in render::stack(project, group, frame) {
        if !layer.kind.is_visual() {
            continue;
        }
        let f = frame as f32;
        let geom = render::layer_geom(project, layer, f);
        let opacity = layer.opacity.sample(f).clamp(0.0, 1.0);
        let is_group = matches!(layer.kind, LayerKind::Group);
        if opacity <= 0.0 || (!is_group && !geom.in_front()) {
            continue;
        }
        if matches!(layer.kind, LayerKind::Adjustment) {
            adjust_below(out, layer, &geom, f, scale, opacity);
            continue;
        }
        let to_px = |p: Vec2| geom.local_to_canvas(p) * scale;
        // A group's members are lit one by one as the group is drawn.
        let lit = !lights.is_empty() && !is_group;
        let has_border = layer.kind.has_border() && layer.border.enabled;
        let has_effects = layer.effects.iter().any(|e| e.enabled);
        // A partly transparent layer with a border must be flattened first,
        // or the fill would show through the border.
        let direct = !is_group
            && !lit
            && layer.blend == BlendMode::Normal
            && !has_effects
            && (opacity >= 1.0 || !has_border);
        if direct {
            draw_layer(out, layer, &geom, f, fps, scale, opacity, assets, &to_px);
            continue;
        }
        let mut image = Pixmap::new(w, h).expect("non-zero size");
        if is_group {
            render_stack(
                &mut image,
                project,
                Some(layer.id),
                frame,
                scale,
                lights,
                assets,
            );
        } else {
            draw_layer(&mut image, layer, &geom, f, fps, scale, 1.0, assets, &to_px);
            if lit {
                light::shade(&mut image, &geom, lights, scale);
            }
        }
        // Effects work in layer pixels, so they grow and shrink with the
        // layer's scale and the zoom.
        apply_effects(&mut image, layer, f, geom.average_scale() * scale);
        out.draw_pixmap(
            0,
            0,
            image.as_ref(),
            &PixmapPaint {
                opacity,
                blend_mode: blend_mode(layer.blend),
                quality: FilterQuality::Nearest,
            },
            Transform::identity(),
            None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_layer(
    target: &mut Pixmap,
    layer: &Layer,
    geom: &LayerGeom,
    frame: f32,
    fps: f32,
    scale: f32,
    opacity: f32,
    assets: &mut Assets,
    to_px: &dyn Fn(Vec2) -> Vec2,
) {
    match &layer.kind {
        LayerKind::Shape {
            shape,
            size,
            corner_radius,
        } => {
            let outline =
                render::shape_outline(*shape, size.sample(frame), corner_radius.sample(frame));
            let segs = path::polygon(&outline);
            fill_and_stroke(target, layer, &segs, geom, frame, scale, opacity, to_px);
        }
        LayerKind::Path { path } => {
            let segs = path.sample(frame).segs();
            fill_and_stroke(target, layer, &segs, geom, frame, scale, opacity, to_px);
        }
        LayerKind::Text { text, font_size } => {
            let layout = text::layout(text, *font_size);
            let Some(path) = build_path(&layout.outline(), to_px) else {
                return;
            };
            let paint = fill_paint(layer, layout.size, frame, opacity, to_px);
            target.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        LayerKind::Image { path, size } => match assets.image(path) {
            Some(image) => draw_bitmap(target, &image, *size, opacity, to_px),
            None => draw_missing(target, geom, to_px),
        },
        LayerKind::Video {
            path,
            size,
            start,
            rate,
            seconds,
            ..
        } => {
            let index = ((frame - *start as f32) / fps * rate).floor() as i64;
            let count = (seconds * rate).floor() as i64;
            match assets.video_frame(path, index, *rate, count, *size) {
                Some(image) => draw_bitmap(target, &image, *size, opacity, to_px),
                None => draw_missing(target, geom, to_px),
            }
        }
        LayerKind::Null
        | LayerKind::Camera { .. }
        | LayerKind::Light { .. }
        | LayerKind::Adjustment
        | LayerKind::Group
        | LayerKind::Audio { .. } => {}
    }
}

/// Fills an outline with the layer's fill, then strokes it with its border.
#[allow(clippy::too_many_arguments)]
fn fill_and_stroke(
    target: &mut Pixmap,
    layer: &Layer,
    segs: &[Seg],
    geom: &LayerGeom,
    frame: f32,
    scale: f32,
    opacity: f32,
    to_px: &dyn Fn(Vec2) -> Vec2,
) {
    let Some(path) = build_path(segs, to_px) else {
        return;
    };
    let paint = fill_paint(layer, geom.size, frame, opacity, to_px);
    target.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    if !layer.border.enabled {
        return;
    }
    let width = layer.border.width.sample(frame).max(0.0) * geom.average_scale() * scale;
    if width > 0.0 {
        let mut paint = Paint::default();
        paint.set_color(color(layer.border.color.sample(frame), opacity));
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            line_join: LineJoin::Round,
            line_cap: tiny_skia::LineCap::Round,
            ..Stroke::default()
        };
        target.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// Draws `image` stretched over a `size` layer centred on its origin.
fn draw_bitmap(
    target: &mut Pixmap,
    image: &Pixmap,
    size: Vec2,
    opacity: f32,
    to_px: &dyn Fn(Vec2) -> Vec2,
) {
    // Image pixels to layer-local pixels (centred).
    let img = vec2(image.width() as f32, image.height() as f32);
    let to_local = |p: Vec2| p / img * size - size * 0.5;
    let corners = [Vec2::ZERO, vec2(img.x, 0.0), vec2(0.0, img.y)];
    let flat = affine_through(corners, corners.map(|p| to_px(to_local(p))));
    // An image facing the camera squarely (the usual case, flat on z = 0)
    // maps affinely: draw it in one go. Anything tilted goes through the
    // perspective grid.
    let facing = flat.filter(|t| {
        let mut far = [tiny_skia::Point::from_xy(img.x, img.y)];
        t.map_points(&mut far);
        let want = to_px(to_local(img));
        (far[0].x - want.x).abs() < 0.05 && (far[0].y - want.y).abs() < 0.05
    });
    match facing {
        None => draw_image_perspective(target, image, img, opacity, &|p| to_px(to_local(p))),
        Some(t) => target.draw_pixmap(
            0,
            0,
            image.as_ref(),
            &PixmapPaint {
                opacity,
                blend_mode: tiny_skia::BlendMode::SourceOver,
                quality: FilterQuality::Bilinear,
            },
            t,
            None,
        ),
    }
}

/// Runs the layer's enabled effects, in order, on its rendered image.
fn apply_effects(image: &mut Pixmap, layer: &Layer, frame: f32, px_per_unit: f32) {
    for effect in layer.effects.iter().filter(|e| e.enabled) {
        match &effect.kind {
            EffectKind::Blur { radius } => {
                blur(image, radius.sample(frame).max(0.0) * px_per_unit);
            }
            EffectKind::Shadow {
                color,
                distance,
                angle,
                blur: softness,
            } => {
                let mut shadow = silhouette(image, color.sample(frame), 1.0);
                blur(&mut shadow, softness.sample(frame).max(0.0) * px_per_unit);
                let (s, c) = angle.sample(frame).to_radians().sin_cos();
                let offset = vec2(c, s) * distance.sample(frame) * px_per_unit;
                put_behind(image, &shadow, offset);
            }
            EffectKind::Glow {
                color,
                radius,
                strength,
            } => {
                let mut glow = silhouette(image, color.sample(frame), 1.0);
                blur(&mut glow, radius.sample(frame).max(0.0) * px_per_unit);
                scale_alpha(&mut glow, strength.sample(frame).max(0.0));
                put_behind(image, &glow, Vec2::ZERO);
            }
            EffectKind::AdjustColor {
                brightness,
                contrast,
                saturation,
                hue,
            } => adjust_color(
                image,
                brightness.sample(frame),
                contrast.sample(frame).max(0.0),
                saturation.sample(frame).max(0.0),
                hue.sample(frame),
            ),
        }
    }
}

/// An adjustment layer: its effects run on everything already in `out`, and
/// the result replaces `out` inside the layer's area, at its opacity.
fn adjust_below(
    out: &mut Pixmap,
    layer: &Layer,
    geom: &LayerGeom,
    frame: f32,
    scale: f32,
    opacity: f32,
) {
    if !layer.effects.iter().any(|e| e.enabled) {
        return;
    }
    let mut adjusted = out.clone();
    apply_effects(&mut adjusted, layer, frame, geom.average_scale() * scale);
    let mut area = Pixmap::new(out.width(), out.height()).expect("same size");
    let mut pb = PathBuilder::new();
    let corners = geom
        .local_corners()
        .map(|c| geom.local_to_canvas(c) * scale);
    pb.move_to(corners[0].x, corners[0].y);
    for c in &corners[1..] {
        pb.line_to(c.x, c.y);
    }
    pb.close();
    let Some(path) = pb.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(tiny_skia::Color::WHITE);
    paint.anti_alias = true;
    area.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
    let pixels = out
        .data_mut()
        .chunks_exact_mut(4)
        .zip(adjusted.data().chunks_exact(4))
        .zip(area.data().chunks_exact(4));
    for ((dst, adj), cover) in pixels {
        let t = cover[3] as f32 / 255.0 * opacity;
        if t <= 0.0 {
            continue;
        }
        for (d, a) in dst.iter_mut().zip(adj) {
            *d = (*d as f32 + (*a as f32 - *d as f32) * t).round() as u8;
        }
    }
}

/// Brightness, contrast, saturation and hue on premultiplied pixels.
fn adjust_color(image: &mut Pixmap, brightness: f32, contrast: f32, saturation: f32, hue: f32) {
    let (s, c) = hue.to_radians().sin_cos();
    // Rotation about the grey axis, keeping luminance.
    let hue_matrix = [
        [
            0.213 + c * 0.787 - s * 0.213,
            0.715 - c * 0.715 - s * 0.715,
            0.072 - c * 0.072 + s * 0.928,
        ],
        [
            0.213 - c * 0.213 + s * 0.143,
            0.715 + c * 0.285 + s * 0.140,
            0.072 - c * 0.072 - s * 0.283,
        ],
        [
            0.213 - c * 0.213 - s * 0.787,
            0.715 - c * 0.715 + s * 0.715,
            0.072 + c * 0.928 + s * 0.072,
        ],
    ];
    for px in image.data_mut().chunks_exact_mut(4) {
        let a = px[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        let rgb = [0, 1, 2].map(|i| px[i] as f32 / 255.0 / a);
        let rotated = hue_matrix.map(|row| row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2]);
        let luma = 0.2126 * rotated[0] + 0.7152 * rotated[1] + 0.0722 * rotated[2];
        for (i, v) in rotated.into_iter().enumerate() {
            let v = luma + (v - luma) * saturation;
            let v = (v - 0.5) * contrast + 0.5 + brightness;
            px[i] = (v.clamp(0.0, 1.0) * a * 255.0).round() as u8;
        }
    }
}

/// The image's shape filled with `color`.
fn silhouette(image: &Pixmap, color: Color, opacity: f32) -> Pixmap {
    let mut out = image.clone();
    let c = color.with_alpha(color.a * opacity);
    let rgba = [c.r * c.a, c.g * c.a, c.b * c.a, c.a].map(|v| v.clamp(0.0, 1.0));
    for px in out.data_mut().chunks_exact_mut(4) {
        let a = px[3] as f32;
        for (out, v) in px.iter_mut().zip(rgba) {
            *out = (v * a).round() as u8;
        }
    }
    out
}

/// Multiplies a premultiplied image's coverage, saturating at full.
fn scale_alpha(image: &mut Pixmap, factor: f32) {
    for v in image.data_mut() {
        *v = (*v as f32 * factor).round().min(255.0) as u8;
    }
}

/// Composites `behind` (shifted by `offset`) underneath `image`.
fn put_behind(image: &mut Pixmap, behind: &Pixmap, offset: Vec2) {
    let mut out = Pixmap::new(image.width(), image.height()).expect("same size");
    out.draw_pixmap(
        0,
        0,
        behind.as_ref(),
        &PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..PixmapPaint::default()
        },
        Transform::from_translate(offset.x, offset.y),
        None,
    );
    out.draw_pixmap(
        0,
        0,
        image.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    *image = out;
}

/// Approximates a gaussian blur with standard deviation `sigma` pixels by
/// three box blurs, each pass horizontal then vertical.
pub fn blur(image: &mut Pixmap, sigma: f32) {
    if sigma < 0.3 {
        return;
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data_mut();
    let mut scratch = vec![0u8; data.len()];
    for radius in box_radii(sigma) {
        box_blur(data, &mut scratch, w, h, radius, 4, w * 4);
        box_blur(&scratch, data, h, w, radius, w * 4, 4);
    }
}

/// Radii of three box blurs whose combination approximates a gaussian
/// (after Kovesi, "Fast almost-Gaussian filtering").
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let upper = lower + 2;
    let lower_f = lower as f32;
    let m = ((12.0 * sigma * sigma - n * lower_f * lower_f - 4.0 * n * lower_f - 3.0 * n)
        / (-4.0 * lower_f - 4.0))
        .round() as i32;
    std::array::from_fn(|i| {
        let size = if (i as i32) < m { lower } else { upper };
        (size.max(1) as usize - 1) / 2
    })
}

/// One box-blur pass along lines of `len` pixels. `step` is the byte stride
/// between neighbouring pixels on a line and `line_step` between lines, so
/// the same code runs horizontally and vertically. Edges are transparent.
fn box_blur(
    src: &[u8],
    dst: &mut [u8],
    lines: usize,
    len: usize,
    radius: usize,
    line_step: usize,
    step: usize,
) {
    let window = (2 * radius + 1) as u32;
    for line in 0..lines {
        let base = line * line_step;
        for ch in 0..4 {
            let at = |i: usize| src[base + i * step + ch] as u32;
            let mut sum: u32 = (0..radius.min(len)).map(at).sum();
            for i in 0..len {
                if i + radius < len {
                    sum += at(i + radius);
                }
                dst[base + i * step + ch] = ((sum + window / 2) / window) as u8;
                if i >= radius {
                    sum -= at(i - radius);
                }
            }
        }
    }
}

/// Draws an image under perspective by splitting it into small triangles,
/// each of which is close enough to affine.
fn draw_image_perspective(
    target: &mut Pixmap,
    image: &Pixmap,
    img: Vec2,
    opacity: f32,
    to_px: &dyn Fn(Vec2) -> Vec2,
) {
    const N: usize = 10;
    let grid: Vec<Vec<(Vec2, Vec2)>> = (0..=N)
        .map(|j| {
            (0..=N)
                .map(|i| {
                    let src = vec2(i as f32, j as f32) / N as f32 * img;
                    (src, to_px(src))
                })
                .collect()
        })
        .collect();
    for j in 0..N {
        for i in 0..N {
            let (a, b, c, d) = (
                grid[j][i],
                grid[j][i + 1],
                grid[j + 1][i + 1],
                grid[j + 1][i],
            );
            for tri in [[a, b, c], [a, c, d]] {
                let Some(t) = affine_through(tri.map(|v| v.0), tri.map(|v| v.1)) else {
                    continue;
                };
                // Grow each triangle a little so neighbours overlap instead
                // of leaving hairline seams.
                let center = (tri[0].1 + tri[1].1 + tri[2].1) / 3.0;
                let grown = tri.map(|v| v.1 + (v.1 - center).normalized() * 0.6);
                let Some(path) = build_path(&path::polygon(&grown), &|p| p) else {
                    continue;
                };
                let paint = Paint {
                    shader: Pattern::new(
                        Pixmap::as_ref(image),
                        SpreadMode::Pad,
                        FilterQuality::Bilinear,
                        opacity,
                        t,
                    ),
                    anti_alias: false,
                    ..Paint::default()
                };
                target.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
    }
}

/// A crossed-out box where an image file couldn't be loaded.
fn draw_missing(target: &mut Pixmap, geom: &LayerGeom, to_px: &dyn Fn(Vec2) -> Vec2) {
    let c = geom.local_corners();
    let mut paint = Paint::default();
    paint.set_color_rgba8(200, 60, 90, 80);
    if let Some(path) = build_path(&path::polygon(&c), to_px) {
        target.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    paint.set_color_rgba8(200, 60, 90, 255);
    let mut segs = path::polygon(&c);
    segs.extend([
        Seg::Move(c[0]),
        Seg::Line(c[2]),
        Seg::Move(c[1]),
        Seg::Line(c[3]),
    ]);
    if let Some(path) = build_path(&segs, to_px) {
        let stroke = Stroke {
            width: 2.0,
            ..Stroke::default()
        };
        target.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// The paint for a shape or text fill: a solid colour or a gradient laid out
/// over a box of `size` layer pixels.
fn fill_paint(
    layer: &Layer,
    size: Vec2,
    frame: f32,
    opacity: f32,
    to_px: &dyn Fn(Vec2) -> Vec2,
) -> Paint<'static> {
    let start = color(layer.fill.sample(frame), opacity);
    let end = color(layer.fill_end.sample(frame), opacity);
    let stops = || vec![GradientStop::new(0.0, start), GradientStop::new(1.0, end)];
    let h = size * 0.5;
    // Gradients are defined in layer pixels and carried to the canvas by the
    // layer's (locally affine) transform.
    let local = affine_through(
        [Vec2::ZERO, vec2(100.0, 0.0), vec2(0.0, 100.0)],
        [Vec2::ZERO, vec2(100.0, 0.0), vec2(0.0, 100.0)].map(to_px),
    );
    let shader = match (layer.fill_style, local) {
        (FillStyle::Linear, Some(t)) => {
            let (s, c) = layer.gradient_angle.sample(frame).to_radians().sin_cos();
            // Reach the box's corners in the gradient direction.
            let extent = (c * h.x).abs() + (s * h.y).abs();
            let d = vec2(c, s) * extent.max(0.5);
            LinearGradient::new(pt(-d), pt(d), stops(), SpreadMode::Pad, t)
        }
        (FillStyle::Radial, Some(t)) => RadialGradient::new(
            Point::zero(),
            Point::zero(),
            h.x.max(h.y).max(0.5),
            stops(),
            SpreadMode::Pad,
            t,
        ),
        _ => None,
    };
    Paint {
        shader: shader.unwrap_or(Shader::SolidColor(start)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// Builds a tiny-skia path from local segments mapped by `to_px`.
fn build_path(segs: &[Seg], to_px: &dyn Fn(Vec2) -> Vec2) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for seg in segs {
        match seg.map(to_px) {
            Seg::Move(p) => pb.move_to(p.x, p.y),
            Seg::Line(p) => pb.line_to(p.x, p.y),
            Seg::Quad(a, b) => pb.quad_to(a.x, a.y, b.x, b.y),
            Seg::Cubic(a, b, c) => pb.cubic_to(a.x, a.y, b.x, b.y, c.x, c.y),
            Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

/// The affine transform taking the three `from` points to the `to` points.
fn affine_through(from: [Vec2; 3], to: [Vec2; 3]) -> Option<Transform> {
    let (s1, s2) = (from[1] - from[0], from[2] - from[0]);
    let (d1, d2) = (to[1] - to[0], to[2] - to[0]);
    let det = s1.x * s2.y - s2.x * s1.y;
    if det.abs() < 1e-9 {
        return None;
    }
    // M = D · S⁻¹
    let inv = [[s2.y / det, -s2.x / det], [-s1.y / det, s1.x / det]];
    let sx = d1.x * inv[0][0] + d2.x * inv[1][0];
    let kx = d1.x * inv[0][1] + d2.x * inv[1][1];
    let ky = d1.y * inv[0][0] + d2.y * inv[1][0];
    let sy = d1.y * inv[0][1] + d2.y * inv[1][1];
    let tx = to[0].x - (sx * from[0].x + kx * from[0].y);
    let ty = to[0].y - (ky * from[0].x + sy * from[0].y);
    let t = Transform::from_row(sx, ky, kx, sy, tx, ty);
    (t.is_finite() && t.invert().is_some()).then_some(t)
}

fn pt(v: Vec2) -> Point {
    Point::from_xy(v.x, v.y)
}

fn color(c: Color, opacity: f32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(
        c.r.clamp(0.0, 1.0),
        c.g.clamp(0.0, 1.0),
        c.b.clamp(0.0, 1.0),
        (c.a * opacity).clamp(0.0, 1.0),
    )
    .unwrap_or(tiny_skia::Color::TRANSPARENT)
}

fn blend_mode(mode: BlendMode) -> tiny_skia::BlendMode {
    use tiny_skia::BlendMode as B;
    match mode {
        BlendMode::Normal => B::SourceOver,
        BlendMode::Multiply => B::Multiply,
        BlendMode::Screen => B::Screen,
        BlendMode::Overlay => B::Overlay,
        BlendMode::Darken => B::Darken,
        BlendMode::Lighten => B::Lighten,
        BlendMode::ColorDodge => B::ColorDodge,
        BlendMode::ColorBurn => B::ColorBurn,
        BlendMode::HardLight => B::HardLight,
        BlendMode::SoftLight => B::SoftLight,
        BlendMode::Difference => B::Difference,
        BlendMode::Exclusion => B::Exclusion,
        BlendMode::Add => B::Plus,
        BlendMode::Hue => B::Hue,
        BlendMode::Saturation => B::Saturation,
        BlendMode::Color => B::Color,
        BlendMode::Luminosity => B::Luminosity,
        // Masks keep or remove what's already been drawn below them.
        BlendMode::Mask => B::DestinationIn,
        BlendMode::MaskInvert => B::DestinationOut,
    }
}

/// Converts rendered pixels for display in egui.
pub fn to_color_image(pixmap: &Pixmap) -> egui::ColorImage {
    egui::ColorImage::from_rgba_premultiplied(
        [pixmap.width() as usize, pixmap.height() as usize],
        pixmap.data(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Animated, LightKind, ShapeKind};

    fn pixel(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let c = p.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }

    #[test]
    fn renders_background_and_shapes() {
        let mut project = Project {
            background: Color::BLACK,
            ..Project::default()
        };
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        project.layer_mut(id).unwrap().fill.value = Color::new(1.0, 0.0, 0.0, 1.0);
        let out = render(&project, 0, 0.25, &mut Assets::default());
        assert_eq!((out.width(), out.height()), (480, 270));
        assert_eq!(pixel(&out, 240, 135), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 5, 5), [0, 0, 0, 255]);
    }

    #[test]
    fn flat_3d_layers_keep_stack_order_until_given_depth() {
        let mut project = Project::default();
        let below = project.add_shape(ShapeKind::Rectangle, 0);
        project.layer_mut(below).unwrap().fill.value = Color::new(1.0, 0.0, 0.0, 1.0);
        // Off-centre, so a distance sort would put it behind.
        project.layer_mut(below).unwrap().transform.position.value += vec2(40.0, 30.0);
        let above = project.add_shape(ShapeKind::Rectangle, 0);
        project.layer_mut(above).unwrap().fill.value = Color::new(0.0, 0.0, 1.0, 1.0);
        let out = render(&project, 0, 0.25, &mut Assets::default());
        assert_eq!(pixel(&out, 245, 140), [0, 0, 255, 255]);

        // Pushing the top layer back puts the other one in front.
        project.layer_mut(above).unwrap().transform.z.value = 200.0;
        let out = render(&project, 0, 0.25, &mut Assets::default());
        assert_eq!(pixel(&out, 245, 140), [255, 0, 0, 255]);
    }

    #[test]
    fn lights_shade_layers_and_leave_unlit_places_dark() {
        let mut project = Project {
            background: Color::BLACK,
            ..Project::default()
        };
        let solid = project.add_solid(0);
        project.layer_mut(solid).unwrap().fill.value = Color::WHITE;
        let unlit = render(&project, 0, 0.25, &mut Assets::default());
        assert_eq!(pixel(&unlit, 240, 135), [255, 255, 255, 255]);

        // A half-strength ambient light halves every pixel.
        let ambient = project.add_light(LightKind::Ambient, 0);
        project.layer_mut(ambient).unwrap().kind = LayerKind::Light {
            light: LightKind::Ambient,
            intensity: Animated::new(0.5),
            cone: Animated::new(90.0),
            feather: Animated::new(0.0),
        };
        let out = render(&project, 0, 0.25, &mut Assets::default());
        assert_eq!(pixel(&out, 10, 10)[0], 128);

        // A narrow spot in front of the centre lights the middle, not the edge.
        project.remove_layer(ambient);
        let spot = project.add_light(LightKind::Spot, 0);
        if let LayerKind::Light { cone, .. } = &mut project.layer_mut(spot).unwrap().kind {
            cone.value = 30.0;
        }
        let out = render(&project, 0, 0.25, &mut Assets::default());
        assert!(pixel(&out, 240, 135)[0] > 200);
        assert_eq!(pixel(&out, 10, 10)[0], 0);
    }

    #[test]
    fn adjustment_layers_change_only_what_is_below() {
        let mut project = Project {
            background: Color::BLACK,
            ..Project::default()
        };
        let below = project.add_solid(0);
        project.layer_mut(below).unwrap().fill.value = Color::new(1.0, 0.0, 0.0, 1.0);
        let adjust = project.add_adjustment(0);
        if let EffectKind::AdjustColor { saturation, .. } =
            &mut project.layer_mut(adjust).unwrap().effects[0].kind
        {
            saturation.value = 0.0;
        }
        let above = project.add_shape(ShapeKind::Rectangle, 0);
        project.layer_mut(above).unwrap().fill.value = Color::new(0.0, 0.0, 1.0, 1.0);
        let out = render(&project, 0, 0.25, &mut Assets::default());
        // Red turned grey, the blue square on top kept its colour.
        let [r, g, b, _] = pixel(&out, 10, 10);
        assert!(r == g && g == b && r > 30, "got {r} {g} {b}");
        assert_eq!(pixel(&out, 240, 135), [0, 0, 255, 255]);
    }

    #[test]
    fn blend_modes_combine_with_layers_below() {
        let mut project = Project::default();
        let below = project.add_shape(ShapeKind::Rectangle, 0);
        project.layer_mut(below).unwrap().fill.value = Color::new(1.0, 0.5, 0.0, 1.0);
        let above = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(above).unwrap();
        layer.fill.value = Color::new(0.0, 1.0, 1.0, 1.0);
        layer.blend = BlendMode::Multiply;
        let out = render(&project, 0, 0.25, &mut Assets::default());
        let [r, g, b, _] = pixel(&out, 240, 135);
        assert_eq!((r, b), (0, 0));
        assert!((120..=135).contains(&g), "multiplied green was {g}");
    }

    #[test]
    fn gradients_run_from_start_to_end() {
        let mut project = Project::default();
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(id).unwrap();
        layer.fill.value = Color::new(1.0, 0.0, 0.0, 1.0);
        layer.fill_end.value = Color::new(0.0, 0.0, 1.0, 1.0);
        layer.fill_style = FillStyle::Linear;
        let out = render(&project, 0, 0.25, &mut Assets::default());
        // The square is 270 canvas px wide, centred: x from 825 to 1095.
        let left = pixel(&out, (830.0 * 0.25) as u32, 135);
        let right = pixel(&out, (1090.0 * 0.25) as u32, 135);
        assert!(left[0] > 200 && left[2] < 50, "{left:?}");
        assert!(right[2] > 200 && right[0] < 50, "{right:?}");
    }

    #[test]
    fn blur_spreads_and_keeps_total_coverage() {
        let mut p = Pixmap::new(41, 41).unwrap();
        p.fill_rect(
            tiny_skia::Rect::from_xywh(15.0, 15.0, 11.0, 11.0).unwrap(),
            &Paint::default(),
            Transform::identity(),
            None,
        );
        let total = |p: &Pixmap| {
            p.data()
                .iter()
                .skip(3)
                .step_by(4)
                .map(|&a| a as u32)
                .sum::<u32>()
        };
        let before = total(&p);
        blur(&mut p, 3.0);
        let after = total(&p);
        assert!(pixel(&p, 12, 20)[3] > 0, "blur reaches outside the square");
        assert!(pixel(&p, 20, 20)[3] < 255, "the centre softens");
        let drift = (after as f32 - before as f32).abs() / before as f32;
        assert!(drift < 0.02, "coverage drifted by {drift}");
    }

    #[test]
    fn shadows_fall_behind_and_offset() {
        let mut project = Project {
            background: Color::WHITE,
            ..Project::default()
        };
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(id).unwrap();
        let mut shadow = crate::model::Effect::preset(1);
        if let EffectKind::Shadow {
            distance,
            blur,
            color,
            ..
        } = &mut shadow.kind
        {
            *distance = crate::model::Animated::new(200.0);
            *blur = crate::model::Animated::new(0.0);
            *color = crate::model::Animated::new(Color::BLACK);
        }
        layer.effects.push(shadow);
        let out = render(&project, 0, 0.25, &mut Assets::default());
        // Below the square (it spans y 405..675) by less than the offset.
        let below = pixel(&out, 240, (800.0 * 0.25) as u32);
        assert_eq!(below, [0, 0, 0, 255]);
        // The square itself still covers its shadow.
        let [r, g, b, _] = pixel(&out, 240, 135);
        assert!(b > 200 && r < 150 && g > 100, "{r} {g} {b}");
    }

    #[test]
    fn masks_clip_only_inside_their_group() {
        let mut project = Project {
            background: Color::BLACK,
            ..Project::default()
        };
        // A wide red bar, masked by a small square above it in a group.
        let bar = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(bar).unwrap();
        layer.fill.value = Color::new(1.0, 0.0, 0.0, 1.0);
        if let LayerKind::Shape { size, .. } = &mut layer.kind {
            size.value = vec2(1600.0, 200.0);
        }
        let mask = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(mask).unwrap();
        layer.blend = BlendMode::Mask;
        if let LayerKind::Shape { size, .. } = &mut layer.kind {
            size.value = vec2(200.0, 200.0);
        }
        let group = project.group_layer(bar, 0).unwrap();
        project.move_to_group(mask, Some(group), 0);
        // Outside the group, a green layer underneath isn't affected.
        let under = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(under).unwrap();
        layer.fill.value = Color::new(0.0, 1.0, 0.0, 1.0);
        if let LayerKind::Shape { size, .. } = &mut layer.kind {
            size.value = vec2(1000.0, 400.0);
        }
        project.reorder_layer(under, -10);

        let out = render(&project, 0, 0.25, &mut Assets::default());
        // Inside the mask: the red bar.
        assert_eq!(pixel(&out, 240, 135), [255, 0, 0, 255]);
        // Bar outside the mask is cut away, revealing green below...
        let x = (960.0 - 200.0) * 0.25;
        assert_eq!(pixel(&out, x as u32, 135), [0, 255, 0, 255]);
        // ...and further out, where there's no green, the background.
        assert_eq!(pixel(&out, (300.0 * 0.25) as u32, 135), [0, 0, 0, 255]);
    }

    #[test]
    fn affine_through_maps_points() {
        let from = [vec2(0.0, 0.0), vec2(10.0, 0.0), vec2(0.0, 5.0)];
        let to = [vec2(3.0, 4.0), vec2(3.0, 14.0), vec2(-2.0, 4.0)];
        let t = affine_through(from, to).unwrap();
        for (f, e) in from.iter().zip(to) {
            let mut p = [pt(*f)];
            t.map_points(&mut p);
            assert!((p[0].x - e.x).abs() < 1e-4 && (p[0].y - e.y).abs() < 1e-4);
        }
    }
}
