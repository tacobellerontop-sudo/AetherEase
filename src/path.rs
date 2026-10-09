//! Vector outlines in layer-local pixels, before they are placed on the canvas.

use egui::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Vec2),
    Line(Vec2),
    Quad(Vec2, Vec2),
    Cubic(Vec2, Vec2, Vec2),
    Close,
}

impl Seg {
    /// The point the segment ends on (the origin for `Close`).
    pub fn end(&self) -> Vec2 {
        match *self {
            Seg::Move(p) | Seg::Line(p) | Seg::Quad(_, p) | Seg::Cubic(_, _, p) => p,
            Seg::Close => Vec2::ZERO,
        }
    }

    /// The same segment with every point passed through `f`.
    pub fn map(&self, mut f: impl FnMut(Vec2) -> Vec2) -> Seg {
        match *self {
            Seg::Move(p) => Seg::Move(f(p)),
            Seg::Line(p) => Seg::Line(f(p)),
            Seg::Quad(a, b) => Seg::Quad(f(a), f(b)),
            Seg::Cubic(a, b, c) => Seg::Cubic(f(a), f(b), f(c)),
            Seg::Close => Seg::Close,
        }
    }
}

/// A closed polygon as path segments.
pub fn polygon(points: &[Vec2]) -> Vec<Seg> {
    let mut segs = Vec::with_capacity(points.len() + 1);
    for (i, &p) in points.iter().enumerate() {
        segs.push(if i == 0 { Seg::Move(p) } else { Seg::Line(p) });
    }
    if !points.is_empty() {
        segs.push(Seg::Close);
    }
    segs
}
