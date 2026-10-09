//! A small vector icon set painted with egui shapes, so icons look the same
//! on every machine instead of depending on which glyphs a font has.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::{
    Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2,
    vec2,
};

use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Back,
    Undo,
    Redo,
    Play,
    Pause,
    PrevFrame,
    NextFrame,
    ToStart,
    ToEnd,
    Loop,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    More,
    Trash,
    Duplicate,
    Up,
    Down,
    Fit,
    Rectangle,
    Ellipse,
    Triangle,
    Hexagon,
    Star,
    Text,
    Image,
    Transform,
    Fill,
    Border,
    Timing,
    Null,
    Camera,
    Link,
    Effects,
    Group,
    Ungroup,
    Right,
    Audio,
    Light,
    Adjustment,
    Solid,
    Video,
    Pen,
    Brush,
    Pointer,
    Graph,
}

/// Paints `icon` centred in `rect`.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5; // half-size
    let w = (s * 0.16).clamp(1.2, 2.2);
    let stroke = Stroke::new(w, color);
    let p = |x: f32, y: f32| pos2(c.x + x * s, c.y + y * s);
    let line = |pts: &[Pos2]| {
        painter.add(Shape::line(pts.to_vec(), stroke));
    };
    let polygon = |n: usize, r: f32, inner: Option<f32>, fill: bool| {
        let count = if inner.is_some() { n * 2 } else { n };
        let pts: Vec<Pos2> = (0..count)
            .map(|i| {
                let a = -FRAC_PI_2 + TAU * i as f32 / count as f32;
                let r = match inner {
                    Some(ir) if i % 2 == 1 => ir,
                    _ => r,
                };
                p(a.cos() * r, a.sin() * r)
            })
            .collect();
        if fill {
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        } else {
            painter.add(Shape::closed_line(pts, stroke));
        }
    };

    match icon {
        Icon::Plus => {
            line(&[p(-0.6, 0.0), p(0.6, 0.0)]);
            line(&[p(0.0, -0.6), p(0.0, 0.6)]);
        }
        Icon::Back => line(&[p(0.25, -0.55), p(-0.3, 0.0), p(0.25, 0.55)]),
        Icon::Up => line(&[p(-0.45, 0.2), p(0.0, -0.25), p(0.45, 0.2)]),
        Icon::Down => line(&[p(-0.45, -0.2), p(0.0, 0.25), p(0.45, -0.2)]),
        Icon::Undo | Icon::Redo => {
            let dir = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let arc: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                    p(dir * -a.cos() * 0.45 + dir * 0.05, -a.sin() * 0.45 + 0.15)
                })
                .collect();
            let start = arc[0];
            painter.add(Shape::line(arc, stroke));
            let tip = vec2(dir * 0.22 * s, 0.0);
            line(&[
                start + tip + vec2(0.0, -0.22 * s),
                start,
                start + tip + vec2(0.0, 0.22 * s),
            ]);
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![p(-0.35, -0.55), p(0.55, 0.0), p(-0.35, 0.55)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [-0.25, 0.25] {
                let r = Rect::from_center_size(p(x, 0.0), vec2(0.22 * s, 1.1 * s));
                painter.rect_filled(r, CornerRadius::same(1), color);
            }
        }
        Icon::PrevFrame | Icon::NextFrame => {
            let d = if icon == Icon::NextFrame { 1.0 } else { -1.0 };
            painter.add(Shape::convex_polygon(
                vec![p(-0.3 * d, -0.45), p(0.3 * d, 0.0), p(-0.3 * d, 0.45)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::ToStart | Icon::ToEnd => {
            let d = if icon == Icon::ToEnd { 1.0 } else { -1.0 };
            painter.add(Shape::convex_polygon(
                vec![p(-0.4 * d, -0.45), p(0.2 * d, 0.0), p(-0.4 * d, 0.45)],
                color,
                Stroke::NONE,
            ));
            line(&[p(0.38 * d, -0.45), p(0.38 * d, 0.45)]);
        }
        Icon::Loop => {
            let r = Rect::from_center_size(c, vec2(1.2 * s, 0.8 * s));
            painter.rect_stroke(
                r,
                CornerRadius::same((0.3 * s) as u8),
                stroke,
                StrokeKind::Middle,
            );
            let tip = p(0.15, -0.4);
            line(&[
                tip + vec2(-0.15 * s, -0.15 * s),
                tip,
                tip + vec2(-0.15 * s, 0.15 * s),
            ]);
        }
        Icon::Eye | Icon::EyeOff => {
            let top: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    p(-0.7 + 1.4 * t, -(t * std::f32::consts::PI).sin() * 0.4)
                })
                .collect();
            let bottom: Vec<Pos2> = top.iter().map(|q| pos2(q.x, 2.0 * c.y - q.y)).collect();
            painter.add(Shape::line(top, stroke));
            painter.add(Shape::line(bottom, stroke));
            painter.circle_filled(c, 0.18 * s, color);
            if icon == Icon::EyeOff {
                line(&[p(-0.6, 0.6), p(0.6, -0.6)]);
            }
        }
        Icon::Lock | Icon::Unlock => {
            let body = Rect::from_min_max(p(-0.45, -0.05), p(0.45, 0.6));
            painter.rect_filled(body, CornerRadius::same(2), color);
            let shackle: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
                    p(a.cos() * 0.28, -0.3 + a.sin() * 0.3)
                })
                .collect();
            let (left, right) = (shackle[0], shackle[12]);
            painter.add(Shape::line(shackle, stroke));
            line(&[left, pos2(left.x, body.top())]);
            let right_end = if icon == Icon::Lock {
                body.top()
            } else {
                right.y - 0.05 * s
            };
            line(&[right, pos2(right.x, right_end)]);
        }
        Icon::More => {
            for x in [-0.45, 0.0, 0.45] {
                painter.circle_filled(p(x, 0.0), 0.11 * s, color);
            }
        }
        Icon::Trash => {
            line(&[p(-0.55, -0.4), p(0.55, -0.4)]);
            line(&[p(-0.18, -0.4), p(-0.12, -0.6), p(0.12, -0.6), p(0.18, -0.4)]);
            painter.add(Shape::closed_line(
                vec![p(-0.4, -0.3), p(0.4, -0.3), p(0.3, 0.6), p(-0.3, 0.6)],
                stroke,
            ));
        }
        Icon::Duplicate => {
            let back = Rect::from_min_max(p(-0.55, -0.55), p(0.25, 0.25));
            let front = Rect::from_min_max(p(-0.25, -0.25), p(0.55, 0.55));
            painter.rect_stroke(back, CornerRadius::same(2), stroke, StrokeKind::Middle);
            painter.rect_filled(front, CornerRadius::same(2), color);
        }
        Icon::Fit => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = p(0.55 * x, 0.55 * y);
                line(&[
                    corner + vec2(0.0, -0.3 * s * y),
                    corner,
                    corner + vec2(-0.3 * s * x, 0.0),
                ]);
            }
        }
        Icon::Rectangle => {
            let r = Rect::from_center_size(c, vec2(1.2 * s, 1.2 * s));
            painter.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Middle);
        }
        Icon::Ellipse => {
            painter.circle_stroke(c, 0.62 * s, stroke);
        }
        Icon::Triangle => polygon(3, 0.7, None, false),
        Icon::Hexagon => polygon(6, 0.65, None, false),
        Icon::Star => polygon(5, 0.72, Some(0.32), false),
        Icon::Text => {
            line(&[p(-0.5, -0.5), p(0.5, -0.5)]);
            line(&[p(0.0, -0.5), p(0.0, 0.55)]);
            line(&[p(-0.2, 0.55), p(0.2, 0.55)]);
        }
        Icon::Image => {
            let r = Rect::from_center_size(c, vec2(1.3 * s, 1.1 * s));
            painter.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Middle);
            line(&[
                p(-0.55, 0.4),
                p(-0.15, -0.05),
                p(0.1, 0.2),
                p(0.3, 0.0),
                p(0.6, 0.35),
            ]);
            painter.circle_filled(p(0.25, -0.25), 0.1 * s, color);
        }
        Icon::Video => {
            // A screen with a play triangle.
            let r = Rect::from_center_size(c, vec2(1.4 * s, 1.05 * s));
            painter.rect_stroke(r, CornerRadius::same(3), stroke, StrokeKind::Middle);
            painter.add(Shape::convex_polygon(
                vec![p(-0.15, -0.25), p(0.3, 0.0), p(-0.15, 0.25)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pen => {
            // A fountain pen nib pointing down-left.
            let nib = vec![
                p(-0.6, 0.6),
                p(-0.35, -0.05),
                p(0.15, -0.55),
                p(0.55, -0.15),
                p(0.05, 0.35),
            ];
            painter.add(Shape::closed_line(nib, stroke));
            line(&[p(-0.6, 0.6), p(-0.1, 0.1)]);
            painter.circle_filled(p(-0.05, 0.05), 0.09 * s, color);
        }
        Icon::Brush => {
            // A brush handle with a curved stroke under it.
            line(&[p(0.6, -0.65), p(-0.05, 0.05)]);
            painter.circle_filled(p(-0.15, 0.15), 0.18 * s, color);
            let swoosh: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    p(
                        -0.7 + t * 0.9,
                        0.55 + (t * std::f32::consts::PI).sin() * 0.15,
                    )
                })
                .collect();
            painter.add(Shape::line(swoosh, stroke));
        }
        Icon::Pointer => {
            // An arrow cursor.
            painter.add(Shape::convex_polygon(
                vec![p(-0.45, -0.65), p(0.45, 0.1), p(0.0, 0.15), p(-0.2, 0.6)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Graph => {
            // An easing curve with its two handles.
            let (a, b) = (p(-0.65, 0.6), p(0.65, -0.6));
            let (h1, h2) = (p(0.15, 0.6), p(-0.15, -0.6));
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [a, h1, h2, b],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            let thin = Stroke::new(w * 0.6, color);
            painter.line_segment([a, h1], thin);
            painter.line_segment([b, h2], thin);
            painter.circle_filled(h1, w * 1.3, color);
            painter.circle_filled(h2, w * 1.3, color);
        }
        Icon::Transform => {
            line(&[p(-0.6, 0.0), p(0.6, 0.0)]);
            line(&[p(0.0, -0.6), p(0.0, 0.6)]);
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let tip = p(0.6 * dx, 0.6 * dy);
                let back = vec2(-dx, -dy) * 0.2 * s;
                let side = vec2(dy, dx) * 0.16 * s;
                line(&[tip + back + side, tip, tip + back - side]);
            }
        }
        Icon::Fill => {
            // A droplet.
            let mut pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = -0.2 + (std::f32::consts::PI + 0.4) * i as f32 / 20.0;
                    p(a.cos() * 0.42, 0.15 + a.sin() * 0.42)
                })
                .collect();
            pts.push(p(0.0, -0.65));
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Border => {
            let r = Rect::from_center_size(c, vec2(1.2 * s, 1.2 * s));
            painter.rect_stroke(
                r,
                CornerRadius::same(3),
                Stroke::new(w * 1.6, color),
                StrokeKind::Middle,
            );
            painter.rect_filled(
                r.shrink(0.35 * s),
                CornerRadius::same(1),
                color.gamma_multiply(0.35),
            );
        }
        Icon::Timing => {
            painter.circle_stroke(c, 0.62 * s, stroke);
            line(&[p(0.0, -0.38), p(0.0, 0.0), p(0.28, 0.18)]);
        }
        Icon::Null => {
            let r = Rect::from_center_size(c, vec2(1.2 * s, 1.2 * s));
            for (a, b) in [
                (r.left_top(), r.right_top()),
                (r.right_top(), r.right_bottom()),
                (r.right_bottom(), r.left_bottom()),
                (r.left_bottom(), r.left_top()),
            ] {
                painter.extend(Shape::dashed_line(&[a, b], stroke, 0.3 * s, 0.2 * s));
            }
            line(&[p(-0.3, 0.0), p(0.3, 0.0)]);
            line(&[p(0.0, -0.3), p(0.0, 0.3)]);
        }
        Icon::Camera => {
            let body = Rect::from_min_max(p(-0.65, -0.35), p(0.25, 0.45));
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Middle);
            painter.add(Shape::closed_line(
                vec![p(0.25, 0.0), p(0.65, -0.3), p(0.65, 0.4), p(0.25, 0.1)],
                stroke,
            ));
            painter.circle_stroke(p(-0.35, -0.55), 0.18 * s, stroke);
            painter.circle_stroke(p(0.0, -0.55), 0.18 * s, stroke);
        }
        Icon::Effects => {
            // A big and a small sparkle.
            let sparkle = |cx: f32, cy: f32, r: f32| {
                let pts: Vec<Pos2> = (0..8)
                    .map(|i| {
                        let a = -FRAC_PI_2 + TAU * i as f32 / 8.0;
                        let r = if i % 2 == 0 { r } else { r * 0.3 };
                        p(cx + a.cos() * r, cy + a.sin() * r)
                    })
                    .collect();
                painter.add(Shape::closed_line(pts, stroke));
            };
            sparkle(-0.15, 0.1, 0.55);
            sparkle(0.45, -0.45, 0.25);
        }
        Icon::Group | Icon::Ungroup => {
            // A folder.
            let body = vec![
                p(-0.65, -0.45),
                p(-0.2, -0.45),
                p(-0.05, -0.3),
                p(0.65, -0.3),
                p(0.65, 0.5),
                p(-0.65, 0.5),
            ];
            painter.add(Shape::closed_line(body, stroke));
            if icon == Icon::Ungroup {
                line(&[p(-0.25, 0.1), p(0.25, 0.1)]);
            } else {
                line(&[p(-0.25, 0.1), p(0.25, 0.1)]);
                line(&[p(0.0, -0.15), p(0.0, 0.35)]);
            }
        }
        Icon::Right => line(&[p(-0.2, -0.45), p(0.25, 0.0), p(-0.2, 0.45)]),
        Icon::Audio => {
            // A music note.
            line(&[p(0.25, 0.35), p(0.25, -0.6), p(0.6, -0.4)]);
            painter.circle_filled(p(0.02, 0.38), 0.24 * s, color);
        }
        Icon::Light => {
            // A bulb-like sun: a disc with eight rays.
            painter.circle_filled(c, 0.3 * s, color);
            for i in 0..8 {
                let a = TAU * i as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                line(&[c + d * 0.5 * s, c + d * 0.8 * s]);
            }
        }
        Icon::Adjustment => {
            // Half-filled circle, the usual "adjust" glyph.
            painter.circle_stroke(c, 0.7 * s, stroke);
            let half: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = -FRAC_PI_2 + std::f32::consts::PI * i as f32 / 16.0;
                    c + vec2(a.cos(), a.sin()) * 0.7 * s
                })
                .collect();
            painter.add(Shape::convex_polygon(half, color, Stroke::NONE));
        }
        Icon::Solid => {
            let r = Rect::from_center_size(c, vec2(1.4 * s, 1.0 * s));
            painter.rect_filled(r, CornerRadius::same(2), color);
        }
        Icon::Link => {
            // Two chain links at an angle.
            for (x, y) in [(-0.22, 0.22), (0.22, -0.22)] {
                let pts: Vec<Pos2> = (0..24)
                    .map(|i| {
                        let a = TAU * i as f32 / 24.0;
                        let (u, v) = (a.cos() * 0.42, a.sin() * 0.2);
                        // Rotate the ellipse by -45°.
                        let k = std::f32::consts::FRAC_1_SQRT_2;
                        p(x + (u + v) * k, y + (v - u) * k)
                    })
                    .collect();
                painter.add(Shape::closed_line(pts, stroke));
            }
        }
    }
}

/// A frameless, round-highlighted icon button.
pub fn button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    icon_button(ui, icon, tooltip, false, true, 32.0)
}

pub fn toggle(ui: &mut Ui, icon: Icon, tooltip: &str, on: bool) -> Response {
    icon_button(ui, icon, tooltip, on, true, 32.0)
}

pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    tooltip: &str,
    on: bool,
    enabled: bool,
    size: f32,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let visuals = ui.visuals();
    let bg = if on {
        theme::ACCENT_SOFT
    } else if enabled && response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same((size * 0.3) as u8), bg);
    let color = if !enabled {
        visuals.weak_text_color().gamma_multiply(0.5)
    } else if on || response.hovered() {
        Color32::WHITE
    } else {
        visuals.text_color()
    };
    paint(ui.painter(), rect.shrink(size * 0.2), icon, color);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    response.on_hover_text(tooltip)
}

/// A big tile with an icon above a label, used for the add-layer menu and the
/// inspector's property categories (Alight Motion's icon grids).
pub fn tile(ui: &mut Ui, icon: Icon, label: &str, selected: bool, size: Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let bg = if selected {
        theme::ACCENT_SOFT
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), bg);
    let color = if selected || response.hovered() {
        Color32::WHITE
    } else {
        visuals.text_color()
    };
    let icon_size = (size.y * 0.36).min(26.0);
    let icon_rect = Rect::from_center_size(
        rect.center() - vec2(0.0, size.y * 0.13),
        Vec2::splat(icon_size),
    );
    paint(painter, icon_rect, icon, color);
    painter.text(
        pos2(rect.center().x, rect.bottom() - size.y * 0.2),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.5),
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
