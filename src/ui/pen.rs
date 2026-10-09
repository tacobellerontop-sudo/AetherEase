//! The canvas drawing tools: the pen for placing path points, the brush for
//! freehand strokes, and point editing for the selected path layer.

use egui::{Color32, CornerRadius, CursorIcon, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2};

use crate::app::AetherApp;
use crate::model::space::Affine3;
use crate::model::{LayerKind, PathShape, vector::PathNode};
use crate::render::{self, View};
use crate::ui::theme;

/// How close (in screen points) the pointer must be to grab a point.
const GRAB: f32 = 8.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Pen,
    Brush,
}

/// Which part of a path point is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointPart {
    Node,
    In,
    Out,
}

#[derive(Clone, Copy, Debug)]
pub struct PointDrag {
    id: u64,
    node: usize,
    part: PointPart,
}

impl AetherApp {
    pub fn set_tool(&mut self, tool: Tool) {
        if self.viewport.tool != tool {
            self.finish_pen();
            self.viewport.tool = tool;
        }
    }

    /// Ends the path being placed with the pen. A path of a single point
    /// isn't worth keeping.
    pub fn finish_pen(&mut self) -> bool {
        let Some(id) = self.viewport.pen.take() else {
            return false;
        };
        if let Some(LayerKind::Path { path }) = self.project.layer(id).map(|l| &l.kind)
            && path.sample(self.frame as f32).nodes.len() < 2
        {
            self.project.remove_layer(id);
            self.select(None);
        }
        true
    }

    /// The canvas point on the z = 0 plane under canvas point `q`, which is
    /// where new paths are drawn whatever the camera does.
    fn on_ground(&self, q: Vec2) -> Option<Vec2> {
        self.project
            .camera_projection(self.frame)
            .hit_plane(&Affine3::IDENTITY, q, 0.0)
    }

    pub fn pen_tool(&mut self, ui: &Ui, view: View, response: &egui::Response) {
        let frame = self.frame;
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        // Keep going only while the path being drawn still exists.
        if self
            .viewport
            .pen
            .is_some_and(|id| self.project.layer(id).is_none())
        {
            self.viewport.pen = None;
        }

        let press = ui.input(|i| i.pointer.press_origin());
        let place = if response.drag_started_by(egui::PointerButton::Primary) {
            press
        } else if response.clicked() {
            response.interact_pointer_pos()
        } else {
            None
        };
        if let Some(pointer) = place {
            let q = view.to_canvas(pointer);
            match self.viewport.pen {
                None => {
                    if let Some(p) = self.on_ground(q) {
                        let shape = PathShape {
                            nodes: vec![PathNode::corner(p)],
                            closed: false,
                        };
                        let id = self.project.add_path(shape, frame, false);
                        self.viewport.pen = Some(id);
                        self.select(Some(id));
                    }
                }
                Some(id) => {
                    let Some((geom, mut shape)) = self.path_at(id) else {
                        return;
                    };
                    let first = shape
                        .nodes
                        .first()
                        .map(|n| view.to_screen(geom.local_to_canvas(n.pos)));
                    if shape.nodes.len() > 2 && first.is_some_and(|f| f.distance(pointer) < GRAB) {
                        shape.closed = true;
                        self.set_path(id, shape);
                        self.viewport.pen = None;
                        return;
                    }
                    if let Some(local) = geom.canvas_to_local(q) {
                        shape.nodes.push(PathNode::corner(local));
                        self.set_path(id, shape);
                    }
                }
            }
        }

        // Dragging after placing a point pulls out its handles.
        if response.dragged_by(egui::PointerButton::Primary)
            && let (Some(id), Some(pointer)) = (self.viewport.pen, response.interact_pointer_pos())
            && let Some((geom, mut shape)) = self.path_at(id)
            && let Some(local) = geom.canvas_to_local(view.to_canvas(pointer))
            && let Some(last) = shape.nodes.last_mut()
        {
            last.out_handle = local - last.pos;
            last.in_handle = -last.out_handle;
            self.set_path(id, shape);
        }
    }

    /// Draws the rubber band from the last pen point to the pointer.
    pub fn draw_pen_preview(&self, painter: &egui::Painter, view: View, hover: Option<Pos2>) {
        let (Some(id), Some(hover)) = (self.viewport.pen, hover) else {
            return;
        };
        let Some((geom, shape)) = self.path_at(id) else {
            return;
        };
        if let Some(last) = shape.nodes.last() {
            let from = view.to_screen(geom.local_to_canvas(last.pos));
            painter.extend(Shape::dashed_line(
                &[from, hover],
                Stroke::new(1.5, theme::ACCENT),
                6.0,
                4.0,
            ));
        }
    }

    pub fn brush_tool(&mut self, ui: &Ui, view: View, response: &egui::Response) {
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            self.viewport.stroke = vec![view.to_canvas(p)];
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(p) = response.interact_pointer_pos()
        {
            let q = view.to_canvas(p);
            let far_enough = self
                .viewport
                .stroke
                .last()
                .is_none_or(|last| (*last - q).length() * view.zoom > 1.5);
            if far_enough {
                self.viewport.stroke.push(q);
            }
        }
        if response.drag_stopped() {
            let stroke = std::mem::take(&mut self.viewport.stroke);
            let points: Vec<Vec2> = stroke.iter().filter_map(|&q| self.on_ground(q)).collect();
            if points.len() >= 2 {
                // About a screen pixel of wobble is smoothed away.
                let shape = PathShape::from_stroke(&points, 1.2 / view.zoom);
                let id = self.project.add_path(shape, self.frame, true);
                self.select(Some(id));
            }
        }
    }

    /// The stroke being drawn with the brush.
    pub fn draw_brush_preview(&self, painter: &egui::Painter, view: View) {
        if self.viewport.stroke.len() < 2 {
            return;
        }
        let points: Vec<Pos2> = self
            .viewport
            .stroke
            .iter()
            .map(|&q| view.to_screen(q))
            .collect();
        painter.add(Shape::line(
            points,
            Stroke::new((10.0 * view.zoom).max(2.0), Color32::from_rgb(84, 140, 255)),
        ));
    }

    /// Point editing for the selected path layer. Returns true when the
    /// pointer was used here, so the move/scale/rotate handles leave it be.
    pub fn edit_path_points(&mut self, ui: &Ui, view: View, response: &egui::Response) -> bool {
        let Some(id) = self.editable_path() else {
            self.viewport.point_drag = None;
            return false;
        };
        let (geom, shape) = self.path_at(id).expect("checked by editable_path");
        let pointer = response.hover_pos();
        let grabbed = pointer.and_then(|p| point_under(view, &geom, &shape, p));
        if grabbed.is_some() && self.viewport.point_drag.is_none() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }

        if response.drag_started_by(egui::PointerButton::Primary) {
            let press = ui.input(|i| i.pointer.press_origin());
            self.viewport.point_drag = press
                .and_then(|p| point_under(view, &geom, &shape, p))
                .map(|(node, part)| PointDrag { id, node, part });
        }
        if let Some(drag) = self.viewport.point_drag {
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = response.interact_pointer_pos()
                && let Some(local) = geom.canvas_to_local(view.to_canvas(p))
            {
                let alt = ui.input(|i| i.modifiers.alt);
                let mut shape = shape.clone();
                if let Some(node) = shape.nodes.get_mut(drag.node) {
                    match drag.part {
                        PointPart::Node => node.pos = local,
                        PointPart::In => {
                            node.in_handle = local - node.pos;
                            if !alt {
                                node.out_handle = mirrored(node.in_handle, node.out_handle);
                            }
                        }
                        PointPart::Out => {
                            node.out_handle = local - node.pos;
                            if !alt {
                                node.in_handle = mirrored(node.out_handle, node.in_handle);
                            }
                        }
                    }
                }
                self.set_path(drag.id, shape);
            }
            if response.drag_stopped() {
                self.viewport.point_drag = None;
            }
            return true;
        }

        let Some((node, part)) = grabbed else {
            return false;
        };
        if part != PointPart::Node {
            // Clicks on a handle do nothing, but shouldn't select through it.
            return response.clicked() || response.drag_started();
        }
        if response.double_clicked() {
            let mut shape = shape.clone();
            shape.toggle_smooth(node);
            self.set_path(id, shape);
            return true;
        }
        if response.clicked() {
            if ui.input(|i| i.modifiers.alt) && shape.nodes.len() > 2 {
                let mut shape = shape.clone();
                shape.nodes.remove(node);
                self.set_path(id, shape);
            }
            return true;
        }
        false
    }

    /// Draws the selected path's points and handles.
    pub fn draw_path_points(&self, painter: &egui::Painter, view: View) {
        let Some(id) = self.editable_path().or(self.viewport.pen) else {
            return;
        };
        let Some((geom, shape)) = self.path_at(id) else {
            return;
        };
        let at = |p: Vec2| view.to_screen(geom.local_to_canvas(p));
        let line = Stroke::new(1.0, theme::ACCENT);
        for node in &shape.nodes {
            let c = at(node.pos);
            for handle in [node.in_handle, node.out_handle] {
                if handle != Vec2::ZERO {
                    let h = at(node.pos + handle);
                    painter.line_segment([c, h], line);
                    painter.circle(h, 3.5, Color32::WHITE, line);
                }
            }
            painter.rect(
                Rect::from_center_size(c, Vec2::splat(8.0)),
                CornerRadius::same(1),
                theme::ACCENT,
                Stroke::new(1.0, Color32::WHITE),
                StrokeKind::Middle,
            );
        }
    }

    /// The selected layer, if it's a path whose points can be edited now.
    fn editable_path(&self) -> Option<u64> {
        let layer = self.selected_layer()?;
        (matches!(layer.kind, LayerKind::Path { .. })
            && !layer.locked
            && self.project.is_shown_at(layer, self.frame))
        .then_some(layer.id)
    }

    fn path_at(&self, id: u64) -> Option<(render::LayerGeom, PathShape)> {
        let layer = self.project.layer(id)?;
        let LayerKind::Path { path } = &layer.kind else {
            return None;
        };
        let f = self.frame as f32;
        Some((render::layer_geom(&self.project, layer, f), path.sample(f)))
    }

    fn set_path(&mut self, id: u64, shape: PathShape) {
        let frame = self.frame;
        if let Some(layer) = self.project.layer_mut(id)
            && let LayerKind::Path { path } = &mut layer.kind
        {
            path.set(frame, shape);
        }
    }
}

/// The point or handle under screen point `p`, handles first since they
/// sit on top.
fn point_under(
    view: View,
    geom: &render::LayerGeom,
    shape: &PathShape,
    p: Pos2,
) -> Option<(usize, PointPart)> {
    let at = |v: Vec2| view.to_screen(geom.local_to_canvas(v));
    let near = |v: Vec2| at(v).distance(p) < GRAB;
    let handle = shape.nodes.iter().enumerate().find_map(|(i, n)| {
        if n.in_handle != Vec2::ZERO && near(n.pos + n.in_handle) {
            Some((i, PointPart::In))
        } else if n.out_handle != Vec2::ZERO && near(n.pos + n.out_handle) {
            Some((i, PointPart::Out))
        } else {
            None
        }
    });
    handle.or_else(|| {
        shape
            .nodes
            .iter()
            .position(|n| near(n.pos))
            .map(|i| (i, PointPart::Node))
    })
}

/// The opposite handle kept in line with `moved`, at its own length (or
/// `moved`'s if it had none), so the curve stays smooth through the point.
fn mirrored(moved: Vec2, other: Vec2) -> Vec2 {
    if moved == Vec2::ZERO {
        return Vec2::ZERO;
    }
    let len = if other == Vec2::ZERO {
        moved.length()
    } else {
        other.length()
    };
    -moved.normalized() * len
}
