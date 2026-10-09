//! Editable vector paths: the outlines drawn with the pen and freehand tools.

use egui::{Rect, Vec2};
use serde::{Deserialize, Serialize};

use super::anim::Lerp;
use crate::path::Seg;

/// One point of a path. Handles are offsets from the point; zero handles
/// make a sharp corner.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathNode {
    pub pos: Vec2,
    /// Controls the curve arriving at this point.
    pub in_handle: Vec2,
    /// Controls the curve leaving this point.
    pub out_handle: Vec2,
}

impl PathNode {
    pub fn corner(pos: Vec2) -> Self {
        Self {
            pos,
            in_handle: Vec2::ZERO,
            out_handle: Vec2::ZERO,
        }
    }

    pub fn is_corner(&self) -> bool {
        self.in_handle == Vec2::ZERO && self.out_handle == Vec2::ZERO
    }
}

/// A path's points, in layer pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathShape {
    pub nodes: Vec<PathNode>,
    pub closed: bool,
}

/// Paths with the same number of points morph point by point; otherwise
/// the shape holds until the next keyframe.
impl Lerp for PathShape {
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        if a.nodes.len() != b.nodes.len() {
            return if t < 1.0 { a.clone() } else { b.clone() };
        }
        let mix = |p: Vec2, q: Vec2| p + (q - p) * t;
        PathShape {
            nodes: a
                .nodes
                .iter()
                .zip(&b.nodes)
                .map(|(p, q)| PathNode {
                    pos: mix(p.pos, q.pos),
                    in_handle: mix(p.in_handle, q.in_handle),
                    out_handle: mix(p.out_handle, q.out_handle),
                })
                .collect(),
            closed: if t < 1.0 { a.closed } else { b.closed },
        }
    }
}

impl PathShape {
    /// The outline as path segments.
    pub fn segs(&self) -> Vec<Seg> {
        let Some(first) = self.nodes.first() else {
            return Vec::new();
        };
        let mut segs = vec![Seg::Move(first.pos)];
        let curve = |a: &PathNode, b: &PathNode| {
            if a.out_handle == Vec2::ZERO && b.in_handle == Vec2::ZERO {
                Seg::Line(b.pos)
            } else {
                Seg::Cubic(a.pos + a.out_handle, b.pos + b.in_handle, b.pos)
            }
        };
        for pair in self.nodes.windows(2) {
            segs.push(curve(&pair[0], &pair[1]));
        }
        if self.closed && self.nodes.len() > 2 {
            segs.push(curve(&self.nodes[self.nodes.len() - 1], first));
            segs.push(Seg::Close);
        }
        segs
    }

    /// A box around the drawn outline.
    pub fn bounds(&self) -> Option<Rect> {
        let mut points = Vec::new();
        let mut from = Vec2::ZERO;
        for seg in self.segs() {
            match seg {
                Seg::Move(p) | Seg::Line(p) => points.push(p),
                Seg::Quad(a, b) => points.extend((1..=16).map(|i| {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    from * (u * u) + a * (2.0 * u * t) + b * (t * t)
                })),
                Seg::Cubic(a, b, c) => points.extend((1..=16).map(|i| {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    from * (u * u * u)
                        + a * (3.0 * u * u * t)
                        + b * (3.0 * u * t * t)
                        + c * (t * t * t)
                })),
                Seg::Close => {}
            }
            if let Some(&p) = points.last() {
                from = p;
            }
        }
        let first = *points.first()?;
        Some(points.iter().fold(
            Rect::from_min_max(first.to_pos2(), first.to_pos2()),
            |r, p| r.union(Rect::from_min_max(p.to_pos2(), p.to_pos2())),
        ))
    }

    /// Smooth handles for node `i` from its neighbours, as a curve through
    /// the points would have (Catmull-Rom).
    pub fn auto_handles(&self, i: usize) -> (Vec2, Vec2) {
        let n = self.nodes.len();
        if n < 2 {
            return (Vec2::ZERO, Vec2::ZERO);
        }
        let at = |j: usize| self.nodes[j].pos;
        let prev = if i > 0 {
            Some(at(i - 1))
        } else if self.closed {
            Some(at(n - 1))
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(at(i + 1))
        } else if self.closed {
            Some(at(0))
        } else {
            None
        };
        let p = at(i);
        let (a, b) = (prev.unwrap_or(p), next.unwrap_or(p));
        let tangent = (b - a) / 6.0;
        (-tangent, tangent)
    }

    /// Makes node `i` a sharp corner, or smooth if it already is one.
    pub fn toggle_smooth(&mut self, i: usize) {
        if i >= self.nodes.len() {
            return;
        }
        let (in_handle, out_handle) = if self.nodes[i].is_corner() {
            self.auto_handles(i)
        } else {
            (Vec2::ZERO, Vec2::ZERO)
        };
        self.nodes[i].in_handle = in_handle;
        self.nodes[i].out_handle = out_handle;
    }

    /// A smooth path through a freehand stroke, with as few points as keep
    /// it within `tolerance` pixels of the stroke.
    pub fn from_stroke(points: &[Vec2], tolerance: f32) -> PathShape {
        let kept = simplify(points, tolerance);
        let mut shape = PathShape {
            nodes: kept.into_iter().map(PathNode::corner).collect(),
            closed: false,
        };
        for i in 0..shape.nodes.len() {
            let (a, b) = shape.auto_handles(i);
            shape.nodes[i].in_handle = a;
            shape.nodes[i].out_handle = b;
        }
        shape
    }

    /// Moves every point by `delta`.
    pub fn translate(&mut self, delta: Vec2) {
        for node in &mut self.nodes {
            node.pos += delta;
        }
    }
}

/// Ramer–Douglas–Peucker: the points needed to stay within `tolerance`.
fn simplify(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (p, q) = (points[a], points[b]);
        let distance = |x: Vec2| {
            let d = q - p;
            let len = d.length();
            if len < 1e-6 {
                (x - p).length()
            } else {
                (d.x * (p.y - x.y) - d.y * (p.x - x.x)).abs() / len
            }
        };
        let far = (a + 1..b).max_by(|&i, &j| distance(points[i]).total_cmp(&distance(points[j])));
        if let Some(i) = far
            && distance(points[i]) > tolerance
        {
            keep[i] = true;
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(&p, k)| k.then_some(p))
        .collect()
}

/// A rectangle of `size` as a closed path.
#[cfg(test)]
pub fn rectangle(size: Vec2) -> PathShape {
    use egui::vec2;
    let h = size * 0.5;
    PathShape {
        nodes: [
            vec2(-h.x, -h.y),
            vec2(h.x, -h.y),
            vec2(h.x, h.y),
            vec2(-h.x, h.y),
        ]
        .into_iter()
        .map(PathNode::corner)
        .collect(),
        closed: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn freehand_strokes_keep_only_needed_points() {
        // A straight line with jitter collapses to its ends; a corner stays.
        let line: Vec<Vec2> = (0..50)
            .map(|i| vec2(i as f32 * 4.0, if i % 2 == 0 { 0.3 } else { -0.3 }))
            .collect();
        assert_eq!(PathShape::from_stroke(&line, 2.0).nodes.len(), 2);
        let mut corner: Vec<Vec2> = (0..20).map(|i| vec2(i as f32 * 5.0, 0.0)).collect();
        corner.extend((1..20).map(|i| vec2(95.0, i as f32 * 5.0)));
        let shape = PathShape::from_stroke(&corner, 2.0);
        assert_eq!(shape.nodes.len(), 3);
        assert!(!shape.nodes[1].is_corner(), "freehand paths are smooth");
    }

    #[test]
    fn paths_morph_when_point_counts_match() {
        let a = rectangle(vec2(100.0, 100.0));
        let b = rectangle(vec2(200.0, 100.0));
        let mid = PathShape::lerp(&a, &b, 0.5);
        assert_eq!(mid.nodes[1].pos, vec2(75.0, -50.0));
        let mut c = b.clone();
        c.nodes.pop();
        assert_eq!(PathShape::lerp(&a, &c, 0.5), a);
    }

    #[test]
    fn closed_paths_end_where_they_start() {
        let mut shape = rectangle(vec2(10.0, 10.0));
        assert!(matches!(shape.segs().last(), Some(Seg::Close)));
        shape.toggle_smooth(0);
        assert!(!shape.nodes[0].is_corner());
        assert!(matches!(shape.segs()[4], Seg::Cubic(..)));
        shape.toggle_smooth(0);
        assert!(shape.nodes[0].is_corner());
    }
}
