//! The graph editor: the easing curve between two keyframes, with handles
//! to drag into a custom curve.

use egui::{Color32, CornerRadius, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

use super::theme;
use crate::model::anim::{Bezier, Easing};

/// Progress shown from this value at the bottom to `TOP` at the top, so
/// curves that overshoot stay in view.
const BOTTOM: f32 = -0.4;
const TOP: f32 = 1.4;
const HANDLE_RADIUS: f32 = 6.0;

/// Draws the curve for `easing` and lets its handles be dragged. Dragging
/// turns a preset into a custom curve. Returns whether `easing` changed.
pub fn easing_graph(ui: &mut Ui, easing: &mut Easing) -> bool {
    let width = ui.available_width();
    let (outer, response) = ui.allocate_exact_size(vec2(width, 230.0), Sense::hover());
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, CornerRadius::same(8), theme::BG_DEEP);

    // The unit square sits in the middle with room for overshoot above and
    // below; time runs left to right.
    let plot = outer.shrink2(vec2(14.0, 8.0));
    let to_screen = |x: f32, y: f32| {
        pos2(
            plot.left() + x * plot.width(),
            plot.bottom() - (y - BOTTOM) / (TOP - BOTTOM) * plot.height(),
        )
    };
    let from_screen = |p: Pos2| {
        (
            (p.x - plot.left()) / plot.width(),
            BOTTOM + (plot.bottom() - p.y) / plot.height() * (TOP - BOTTOM),
        )
    };

    let grid = Stroke::new(1.0, Color32::from_white_alpha(14));
    for i in 0..=4 {
        let x = i as f32 / 4.0;
        painter.line_segment([to_screen(x, BOTTOM), to_screen(x, TOP)], grid);
    }
    let edge = Stroke::new(1.0, Color32::from_white_alpha(40));
    for y in [0.0, 1.0] {
        painter.line_segment([to_screen(0.0, y), to_screen(1.0, y)], edge);
    }
    painter.add(Shape::dashed_line(
        &[to_screen(0.0, 0.0), to_screen(1.0, 1.0)],
        Stroke::new(1.0, Color32::from_white_alpha(30)),
        4.0,
        4.0,
    ));

    // Handles: the first leaves (0, 0), the second arrives at (1, 1).
    let mut curve = easing.as_curve();
    let mut changed = false;
    let hold = *easing == Easing::Hold;
    let handles = [
        (to_screen(0.0, 0.0), to_screen(curve.x1, curve.y1)),
        (to_screen(1.0, 1.0), to_screen(curve.x2, curve.y2)),
    ];
    if !hold {
        for (i, &(_, at)) in handles.iter().enumerate() {
            let hit = Rect::from_center_size(at, Vec2::splat(HANDLE_RADIUS * 3.0));
            let handle = ui
                .interact(hit, Id::new(("easing handle", i)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::Grab);
            if handle.dragged()
                && let Some(pointer) = handle.interact_pointer_pos()
            {
                let (x, y) = from_screen(pointer);
                let (x, y) = (x.clamp(0.0, 1.0), y.clamp(BOTTOM, TOP));
                if i == 0 {
                    (curve.x1, curve.y1) = (x, y);
                } else {
                    (curve.x2, curve.y2) = (x, y);
                }
                changed = true;
            }
        }
    }
    if changed {
        *easing = Easing::Custom(round(curve));
        curve = easing.as_curve();
    }

    let accent = theme::ACCENT;
    let shown = *easing;
    let points: Vec<Pos2> = if hold {
        vec![
            to_screen(0.0, 0.0),
            to_screen(1.0, 0.0),
            to_screen(1.0, 1.0),
        ]
    } else {
        (0..=64)
            .map(|i| {
                let (x, y) = match shown {
                    // Draw the presets from their own formula, which the
                    // bezier only approximates.
                    Easing::Custom(_) => curve.point(i as f32 / 64.0),
                    e => {
                        let t = i as f32 / 64.0;
                        (t, e.apply(t))
                    }
                };
                to_screen(x, y)
            })
            .collect()
    };
    painter.add(Shape::line(points, Stroke::new(2.5, accent)));

    if !hold {
        let arm = Stroke::new(1.5, Color32::from_white_alpha(150));
        for (from, to) in [
            (to_screen(0.0, 0.0), to_screen(curve.x1, curve.y1)),
            (to_screen(1.0, 1.0), to_screen(curve.x2, curve.y2)),
        ] {
            painter.line_segment([from, to], arm);
            painter.circle(to, HANDLE_RADIUS, Color32::WHITE, Stroke::new(2.0, accent));
        }
    }
    for p in [to_screen(0.0, 0.0), to_screen(1.0, 1.0)] {
        painter.circle_filled(p, 3.5, theme::KEYFRAME);
    }

    // While the pointer is over the graph, a dot plays the motion: across
    // for time, and up the right edge for how far the value has got.
    if response.hovered() || changed {
        let time = ui.input(|i| i.time) as f32;
        let t = ((time % 1.6) / 1.2).min(1.0);
        let y = shown.apply(t);
        painter.circle_filled(to_screen(t, y), 4.5, theme::PLAYHEAD);
        let side = pos2(outer.right() - 6.0, to_screen(1.0, y).y);
        painter.circle_filled(side, 4.5, theme::KEYFRAME);
        ui.ctx().request_repaint();
    }
    changed
}

/// Keeps saved curves short and readable.
fn round(c: Bezier) -> Bezier {
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    Bezier::new(r(c.x1), r(c.y1), r(c.x2), r(c.y2))
}
