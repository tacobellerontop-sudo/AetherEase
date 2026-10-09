//! Light layers shading the layers they shine on.
//!
//! Lighting works the way After Effects' does: once a composition has any
//! light, each layer's colour is multiplied by the light reaching each of its
//! pixels, so places no light reaches go dark. Layers are lit on the side
//! facing the camera.

use egui::vec2;
use tiny_skia::Pixmap;

use crate::model::space::{Vec3, vec3};
use crate::model::{LayerKind, LightKind, Project};
use crate::render::LayerGeom;

/// One light at one frame, in world space.
pub struct Light {
    kind: LightKind,
    /// Colour times intensity.
    rgb: [f32; 3],
    position: Vec3,
    /// Unit vector the light faces (spot and parallel lights).
    facing: Vec3,
    /// Spot lights: cosines of the half-angles where the beam starts to fade
    /// and where it ends.
    inner: f32,
    outer: f32,
}

/// The lights shining at `frame`, wherever they sit in the stack or groups.
pub fn lights_at(project: &Project, frame: i32) -> Vec<Light> {
    let f = frame as f32;
    project
        .layers
        .iter()
        .filter(|l| project.is_shown_at(l, frame))
        .filter_map(|l| {
            let LayerKind::Light {
                light,
                intensity,
                cone,
                feather,
            } = &l.kind
            else {
                return None;
            };
            let world = project.world_matrix(l, f);
            let c = l.fill.sample(f);
            let k = intensity.sample(f).max(0.0) * c.a;
            let half = (cone.sample(f).clamp(1.0, 179.0) * 0.5).to_radians();
            let soft = feather.sample(f).clamp(0.0, 1.0);
            Some(Light {
                kind: *light,
                rgb: [c.r * k, c.g * k, c.b * k],
                position: world.point(Vec3::ZERO),
                facing: world.vector(vec3(0.0, 0.0, 1.0)).normalized(),
                inner: (half * (1.0 - soft)).cos(),
                outer: half.cos(),
            })
        })
        .collect()
}

impl Light {
    /// The light reaching point `p` on a surface facing `normal`.
    fn at(&self, p: Vec3, normal: Vec3) -> f32 {
        match self.kind {
            LightKind::Ambient => 1.0,
            LightKind::Parallel => normal.dot(-self.facing).max(0.0),
            LightKind::Point | LightKind::Spot => {
                let to_light = (self.position - p).normalized();
                let k = normal.dot(to_light).max(0.0);
                if self.kind == LightKind::Point || k == 0.0 {
                    return k;
                }
                let along = (-to_light).dot(self.facing);
                k * smoothstep(self.outer, self.inner, along)
            }
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 - edge0 < 1e-6 {
        return if x >= edge0 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Multiplies a layer's rendered pixels (`image`, at `scale` output pixels
/// per canvas pixel) by the light reaching each one.
pub fn shade(image: &mut Pixmap, geom: &LayerGeom, lights: &[Light], scale: f32) {
    let origin = geom.world.point(Vec3::ZERO);
    let x = geom.world.vector(vec3(1.0, 0.0, 0.0));
    let y = geom.world.vector(vec3(0.0, 1.0, 0.0));
    let mut normal = x.cross(y).normalized();
    // Light the side the camera sees.
    let (_, eye) = geom.projection.ray(geom.anchor_canvas());
    if normal.dot(eye) > 0.0 {
        normal = -normal;
    }
    let width = image.width() as usize;
    for (i, px) in image.data_mut().chunks_exact_mut(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let q = vec2((i % width) as f32 + 0.5, (i / width) as f32 + 0.5) / scale;
        let (o, d) = geom.projection.ray(q);
        let denom = normal.dot(d);
        let mut rgb = [0.0f32; 3];
        if denom.abs() > 1e-9 {
            let p = o + d * (normal.dot(origin - o) / denom);
            for light in lights {
                let k = light.at(p, normal);
                for (sum, c) in rgb.iter_mut().zip(light.rgb) {
                    *sum += c * k;
                }
            }
        }
        let a = px[3] as f32;
        for (v, k) in px.iter_mut().zip(rgb) {
            *v = (*v as f32 * k).round().min(a) as u8;
        }
    }
}
