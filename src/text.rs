//! Text layout and glyph outlines, so text can be drawn as vector paths
//! that follow any transform (rotation, parenting, 3D perspective).

use std::sync::LazyLock;

use ab_glyph::{Font, FontRef, GlyphId, OutlineCurve, PxScale, ScaleFont};
use egui::{Vec2, vec2};

use crate::path::Seg;

static FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font is valid")
});

/// Laid-out text: each glyph with its pen position, in pixels from the
/// top-left of the text block.
pub struct TextLayout {
    pub size: Vec2,
    glyphs: Vec<(GlyphId, Vec2)>,
    font_size: f32,
}

pub fn layout(text: &str, font_size: f32) -> TextLayout {
    let font_size = font_size.max(0.1);
    let font = FONT.as_scaled(PxScale::from(font_size));
    let line_height = font.height() + font.line_gap();
    let mut glyphs = Vec::new();
    let mut width = 0.0_f32;
    let mut lines = 0;
    for (i, line) in text.split('\n').enumerate() {
        lines += 1;
        let baseline = i as f32 * line_height + font.ascent();
        let mut x = 0.0;
        let mut prev = None;
        for c in line.chars() {
            let id = font.glyph_id(c);
            if let Some(p) = prev {
                x += font.kern(p, id);
            }
            glyphs.push((id, vec2(x, baseline)));
            x += font.h_advance(id);
            prev = Some(id);
        }
        width = width.max(x);
    }
    let height = lines as f32 * font.height() + (lines - 1) as f32 * font.line_gap();
    TextLayout {
        size: vec2(width, height),
        glyphs,
        font_size,
    }
}

impl TextLayout {
    /// Glyph outlines in layer-local pixels, centred on the text block.
    pub fn outline(&self) -> Vec<Seg> {
        let font = FONT.as_scaled(PxScale::from(self.font_size));
        let (sx, sy) = (font.h_scale_factor(), font.v_scale_factor());
        let half = self.size * 0.5;
        let mut segs = Vec::new();
        for &(id, pen) in &self.glyphs {
            let Some(outline) = FONT.outline(id) else {
                continue;
            };
            // Font units have y pointing up.
            let p = |q: ab_glyph::Point| pen - half + vec2(q.x * sx, -q.y * sy);
            let mut last: Option<Vec2> = None;
            for curve in &outline.curves {
                let (start, seg) = match *curve {
                    OutlineCurve::Line(a, b) => (p(a), Seg::Line(p(b))),
                    OutlineCurve::Quad(a, b, c) => (p(a), Seg::Quad(p(b), p(c))),
                    OutlineCurve::Cubic(a, b, c, d) => (p(a), Seg::Cubic(p(b), p(c), p(d))),
                };
                // A curve that doesn't continue the previous one starts a
                // new contour.
                if last.is_none_or(|l| (l - start).length_sq() > 1e-6) {
                    if last.is_some() {
                        segs.push(Seg::Close);
                    }
                    segs.push(Seg::Move(start));
                }
                last = Some(seg.end());
                segs.push(seg);
            }
            if last.is_some() {
                segs.push(Seg::Close);
            }
        }
        segs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_grows_with_text() {
        let one = layout("A", 100.0);
        let two = layout("AA", 100.0);
        let lines = layout("A\nA", 100.0);
        assert!(one.size.x > 20.0 && two.size.x > one.size.x * 1.5);
        assert!(lines.size.y > one.size.y * 1.8);
        assert!(!one.outline().is_empty());
        assert_eq!(layout("", 50.0).size.x, 0.0);
    }
}
