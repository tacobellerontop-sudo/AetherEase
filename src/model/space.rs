//! 3D placement of layers: affine matrices, parenting, and the camera.
//!
//! Layer-local pixels (origin at the layer's centre, y down) go through the
//! layer's own transform and then each parent's in turn to reach world space,
//! where x and y are canvas pixels and z points away from the viewer. 2D
//! layers are then drawn flat (z ignored); 3D layers are seen through the
//! active camera in perspective.

use std::ops::{Add, Mul, Neg, Sub};

use egui::{Vec2, vec2};

use super::{Layer, LayerKind, Project, default_camera_zoom};

/// Points closer to the camera than this are treated as behind it.
pub const NEAR: f32 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = vec3(0.0, 0.0, 0.0);

    pub fn xy(self) -> Vec2 {
        vec2(self.x, self.y)
    }

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        vec3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// The unit vector in the same direction (zero stays zero).
    pub fn normalized(self) -> Vec3 {
        let len = self.dot(self).sqrt();
        if len > 0.0 { self * (1.0 / len) } else { self }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        vec3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        vec3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        vec3(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        vec3(-self.x, -self.y, -self.z)
    }
}

/// A 3×4 affine transform: a 3×3 linear part plus a translation column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3 {
    /// Row-major; column 3 is the translation.
    pub m: [[f32; 4]; 3],
}

impl Affine3 {
    pub const IDENTITY: Affine3 = Affine3 {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };

    pub fn translate(t: Vec3) -> Self {
        let mut a = Self::IDENTITY;
        a.m[0][3] = t.x;
        a.m[1][3] = t.y;
        a.m[2][3] = t.z;
        a
    }

    pub fn scale(s: Vec3) -> Self {
        let mut a = Self::IDENTITY;
        a.m[0][0] = s.x;
        a.m[1][1] = s.y;
        a.m[2][2] = s.z;
        a
    }

    /// Rotation by `degrees` around X, Y or Z (0, 1, 2). With y pointing
    /// down, a positive Z rotation turns clockwise on screen.
    pub fn rotate(axis: usize, degrees: f32) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        let (i, j) = match axis {
            0 => (1, 2),
            1 => (2, 0),
            _ => (0, 1),
        };
        let mut a = Self::IDENTITY;
        a.m[i][i] = c;
        a.m[i][j] = -s;
        a.m[j][i] = s;
        a.m[j][j] = c;
        a
    }

    /// The transform that applies `other` first, then `self`.
    pub fn then_after(&self, other: &Affine3) -> Affine3 {
        let (a, b) = (&self.m, &other.m);
        let mut m = [[0.0; 4]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, out) in row.iter_mut().enumerate() {
                *out = (0..3).map(|k| a[r][k] * b[k][c]).sum::<f32>()
                    + if c == 3 { a[r][3] } else { 0.0 };
            }
        }
        Affine3 { m }
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        self.vector(p) + vec3(self.m[0][3], self.m[1][3], self.m[2][3])
    }

    /// Applies only the linear part (no translation).
    pub fn vector(&self, v: Vec3) -> Vec3 {
        let m = &self.m;
        vec3(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }

    pub fn inverse(&self) -> Option<Affine3> {
        let m = &self.m;
        let cof = |r0: usize, r1: usize, c0: usize, c1: usize| {
            m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
        };
        // Inverse of the 3×3 part via the adjugate.
        let inv = [
            [cof(1, 2, 1, 2), -cof(0, 2, 1, 2), cof(0, 1, 1, 2)],
            [-cof(1, 2, 0, 2), cof(0, 2, 0, 2), -cof(0, 1, 0, 2)],
            [cof(1, 2, 0, 1), -cof(0, 2, 0, 1), cof(0, 1, 0, 1)],
        ];
        let det = m[0][0] * inv[0][0] + m[0][1] * inv[1][0] + m[0][2] * inv[2][0];
        if det.abs() < 1e-9 {
            return None;
        }
        let mut out = Affine3::IDENTITY;
        for (out_row, inv_row) in out.m.iter_mut().zip(inv) {
            for (o, v) in out_row.iter_mut().zip(inv_row) {
                *o = v / det;
            }
        }
        let t = out.vector(vec3(m[0][3], m[1][3], m[2][3]));
        out.m[0][3] = -t.x;
        out.m[1][3] = -t.y;
        out.m[2][3] = -t.z;
        Some(out)
    }
}

/// How world space reaches the canvas for one layer.
#[derive(Clone, Copy, Debug)]
pub enum Projection {
    /// 2D layers: x and y are canvas pixels, z is ignored.
    Flat,
    /// 3D layers, seen through a camera.
    Perspective {
        /// World to camera space (camera at the origin looking down +z).
        view: Affine3,
        /// Camera space to world space.
        camera: Affine3,
        /// Canvas pixels per unit of x/z.
        zoom: f32,
        /// The canvas point the camera looks at.
        center: Vec2,
    },
}

impl Projection {
    /// Canvas position of world point `p`, or `None` behind the camera.
    pub fn project(&self, p: Vec3) -> Option<Vec2> {
        match self {
            Projection::Flat => Some(p.xy()),
            Projection::Perspective {
                view, zoom, center, ..
            } => {
                let c = view.point(p);
                (c.z >= NEAR).then(|| *center + c.xy() * (*zoom / c.z))
            }
        }
    }

    /// Distance from the camera, used to sort 3D layers back to front.
    pub fn depth(&self, p: Vec3) -> f32 {
        match self {
            Projection::Flat => p.z,
            Projection::Perspective { view, .. } => view.point(p).z,
        }
    }

    /// The line through canvas point `q`, as a world-space origin and direction.
    pub fn ray(&self, q: Vec2) -> (Vec3, Vec3) {
        match self {
            Projection::Flat => (vec3(q.x, q.y, 0.0), vec3(0.0, 0.0, 1.0)),
            Projection::Perspective {
                camera,
                zoom,
                center,
                ..
            } => {
                let d = (q - *center) / *zoom;
                (camera.point(Vec3::ZERO), camera.vector(vec3(d.x, d.y, 1.0)))
            }
        }
    }

    /// Where the line through canvas point `q` crosses the z = `z` plane of
    /// the space `to_world` maps from.
    pub fn hit_plane(&self, to_world: &Affine3, q: Vec2, z: f32) -> Option<Vec2> {
        let inv = to_world.inverse()?;
        let (origin, dir) = self.ray(q);
        let (o, d) = (inv.point(origin), inv.vector(dir));
        if d.z.abs() < 1e-6 {
            return None;
        }
        let t = (z - o.z) / d.z;
        if matches!(self, Projection::Perspective { .. }) && t <= 0.0 {
            return None;
        }
        Some((o + d * t).xy())
    }
}

/// The transform from a layer's local pixels to its parent's space.
pub fn local_matrix(layer: &Layer, frame: f32) -> Affine3 {
    let t = &layer.transform;
    let p = t.position.sample(frame);
    let s = match layer.kind {
        LayerKind::Camera { .. } | LayerKind::Light { .. } => vec2(1.0, 1.0),
        _ => t.scale.sample(frame),
    };
    let three_d = layer.is_3d();
    let z = if three_d { t.z.sample(frame) } else { 0.0 };
    let mut m = Affine3::translate(vec3(p.x, p.y, z))
        .then_after(&Affine3::rotate(2, t.rotation.sample(frame)));
    if three_d {
        m = m
            .then_after(&Affine3::rotate(1, t.rotation_y.sample(frame)))
            .then_after(&Affine3::rotate(0, t.rotation_x.sample(frame)));
    }
    m.then_after(&Affine3::scale(vec3(s.x, s.y, 1.0)))
        .then_after(&Affine3::translate(vec3(-t.anchor.x, -t.anchor.y, 0.0)))
}

impl Project {
    /// The transform from the layer's parent space to world space: its parent
    /// chain, then the group it belongs to (identity for top-level layers).
    pub fn parent_matrix(&self, layer: &Layer, frame: f32) -> Affine3 {
        self.parent_matrix_depth(layer, frame, 0)
    }

    fn parent_matrix_depth(&self, layer: &Layer, frame: f32, depth: usize) -> Affine3 {
        let mut m = Affine3::IDENTITY;
        // Bounded so a loop in a hand-edited file can't hang the app.
        if depth > self.layers.len() {
            return m;
        }
        let mut next = layer.parent;
        for _ in 0..self.layers.len() {
            let Some(parent) = next.and_then(|id| self.layer(id)) else {
                break;
            };
            m = local_matrix(parent, frame).then_after(&m);
            next = parent.parent;
        }
        if let Some(group) = layer.group.and_then(|g| self.layer(g)) {
            let group_world = self
                .parent_matrix_depth(group, frame, depth + 1)
                .then_after(&local_matrix(group, frame));
            m = group_world.then_after(&m);
        }
        m
    }

    /// Layer-local pixels to world space.
    pub fn world_matrix(&self, layer: &Layer, frame: f32) -> Affine3 {
        self.parent_matrix(layer, frame)
            .then_after(&local_matrix(layer, frame))
    }

    /// The top-most visible camera at `frame`.
    pub fn active_camera(&self, frame: i32) -> Option<&Layer> {
        self.layers
            .iter()
            .rev()
            .find(|l| matches!(l.kind, LayerKind::Camera { .. }) && l.is_active_at(frame))
    }

    /// The camera that 3D layers are seen through. Without a camera layer the
    /// default one shows the z = 0 plane exactly as it looks in 2D.
    pub fn camera_projection(&self, frame: i32) -> Projection {
        let f = frame as f32;
        let (camera, zoom) = match self.active_camera(frame) {
            Some(cam) => {
                let zoom = match &cam.kind {
                    LayerKind::Camera { zoom } => zoom.sample(f),
                    _ => unreachable!(),
                };
                // The camera's anchor is irrelevant; only where it is and
                // which way it faces matter.
                let world = self
                    .world_matrix(cam, f)
                    .then_after(&Affine3::translate(vec3(
                        cam.transform.anchor.x,
                        cam.transform.anchor.y,
                        0.0,
                    )));
                (world, zoom)
            }
            None => {
                let zoom = default_camera_zoom(self.width);
                let c = self.center();
                (Affine3::translate(vec3(c.x, c.y, -zoom)), zoom)
            }
        };
        Projection::Perspective {
            view: camera.inverse().unwrap_or(Affine3::IDENTITY),
            camera,
            zoom: zoom.max(1.0),
            center: self.center(),
        }
    }

    pub fn projection_for(&self, layer: &Layer, frame: i32) -> Projection {
        if layer.is_3d() {
            self.camera_projection(frame)
        } else {
            Projection::Flat
        }
    }

    /// Parents `id` to `parent` (or unparents it) without moving it on screen
    /// at `frame`.
    pub fn reparent_in_place(&mut self, id: u64, parent: Option<u64>, frame: i32) -> bool {
        self.keeping_place(id, frame, |project| project.set_parent(id, parent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-2
    }

    #[test]
    fn inverse_round_trips() {
        let m = Affine3::translate(vec3(10.0, -4.0, 30.0))
            .then_after(&Affine3::rotate(2, 33.0))
            .then_after(&Affine3::rotate(1, -50.0))
            .then_after(&Affine3::rotate(0, 20.0))
            .then_after(&Affine3::scale(vec3(2.0, 0.5, 1.0)));
        let p = vec3(3.0, 7.0, -2.0);
        let back = m.inverse().unwrap().point(m.point(p));
        let d = back - p;
        assert!(d.x.abs() + d.y.abs() + d.z.abs() < 1e-4);
    }

    #[test]
    fn default_camera_matches_2d_at_depth_zero() {
        let project = Project::default();
        let proj = project.camera_projection(0);
        for p in [vec2(0.0, 0.0), vec2(1920.0, 1080.0), vec2(300.0, 900.0)] {
            assert!(close(proj.project(vec3(p.x, p.y, 0.0)).unwrap(), p));
        }
        // Further away means smaller, towards the centre.
        let far = proj.project(vec3(0.0, 0.0, 1000.0)).unwrap();
        assert!(far.x > 0.0 && far.x < 960.0);
        // Behind the camera.
        assert!(proj.project(vec3(0.0, 0.0, -5000.0)).is_none());
    }

    #[test]
    fn a_new_camera_layer_changes_nothing() {
        let mut project = Project::default();
        let before = project.camera_projection(0);
        project.add_camera(0);
        let after = project.camera_projection(0);
        let p = vec3(123.0, 456.0, 300.0);
        assert!(close(before.project(p).unwrap(), after.project(p).unwrap()));
    }

    #[test]
    fn children_follow_their_parent() {
        let mut project = Project::default();
        let null = project.add_null(0);
        let child = project.add_shape(ShapeKind::Rectangle, 0);
        // Reparenting keeps the child where it was.
        assert!(project.reparent_in_place(child, Some(null), 0));
        let world = |p: &Project| {
            let l = p.layer(child).unwrap();
            p.world_matrix(l, 0.0).point(Vec3::ZERO).xy()
        };
        assert!(close(world(&project), project.center()));

        // Moving and rotating the null carries the child along.
        let n = project.layer_mut(null).unwrap();
        n.transform.position.value += vec2(100.0, 0.0);
        n.transform.rotation.value = 90.0;
        project.layer_mut(child).unwrap().transform.position.value = vec2(50.0, 0.0);
        assert!(close(world(&project), project.center() + vec2(100.0, 50.0)));

        // Unparenting keeps it in place too.
        let placed = world(&project);
        assert!(project.reparent_in_place(child, None, 0));
        assert!(close(world(&project), placed));
    }

    #[test]
    fn rays_hit_what_they_project_to() {
        let mut project = Project::default();
        let id = project.add_shape(ShapeKind::Rectangle, 0);
        let layer = project.layer_mut(id).unwrap();
        layer.transform.rotation_y.value = 40.0;
        layer.transform.z.value = 250.0;
        let layer = project.layer(id).unwrap();
        let world = project.world_matrix(layer, 0.0);
        let proj = project.projection_for(layer, 0);
        let local = vec2(80.0, -30.0);
        let canvas = proj
            .project(world.point(vec3(local.x, local.y, 0.0)))
            .unwrap();
        let back = proj.hit_plane(&world, canvas, 0.0).unwrap();
        assert!(close(back, local));
    }
}
