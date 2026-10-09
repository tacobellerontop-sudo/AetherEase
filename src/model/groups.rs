//! Grouping layers. A group is a layer of kind [`LayerKind::Group`]; its
//! members are the layers whose `group` field holds its id, in stack order.
//! Members' positions are in the group's space, so moving, scaling or
//! rotating the group carries them along.

use super::space::vec3;
use super::{Animated, Layer, LayerKind, Project};

impl Project {
    /// Whether `id` is `group` itself or anywhere inside it.
    pub fn is_in_group(&self, id: u64, group: u64) -> bool {
        let mut current = Some(id);
        // The depth limit guards against loops in hand-edited files.
        for _ in 0..=self.layers.len() {
            match current {
                Some(c) if c == group => return true,
                Some(c) => current = self.layer(c).and_then(|l| l.group),
                None => return false,
            }
        }
        false
    }

    /// The layers directly inside `group` (or at the top level for `None`),
    /// bottom-most first.
    pub fn members(&self, group: Option<u64>) -> impl Iterator<Item = &Layer> {
        self.layers.iter().filter(move |l| l.group == group)
    }

    /// Whether the layer is shown at `frame`, taking its groups into account.
    pub fn is_shown_at(&self, layer: &Layer, frame: i32) -> bool {
        let mut current = Some(layer);
        for _ in 0..=self.layers.len() {
            match current {
                Some(l) if !l.is_active_at(frame) => return false,
                Some(l) => current = l.group.and_then(|g| self.layer(g)),
                None => return true,
            }
        }
        true
    }

    /// Puts `id` in a new group of its own, in its place in the stack, and
    /// returns the group's id. The group's pivot is the layer's position.
    pub fn group_layer(&mut self, id: u64, frame: i32) -> Option<u64> {
        let index = self.index_of(id)?;
        let outer = self.layers[index].group;
        let (in_frame, out_frame) = (self.layers[index].in_frame, self.layers[index].out_frame);
        let pivot = self.layers[index].transform.position.sample(frame as f32);
        let group_id = self.next_id;
        self.next_id += 1;
        let name = self.unique_name("Group");
        let mut group = Layer::base(group_id, name, LayerKind::Group, pivot, (0, 0));
        // An identity transform to start with: position and anchor coincide.
        group.transform.anchor = pivot;
        group.in_frame = in_frame.min(0);
        group.out_frame = out_frame.max(self.duration);
        group.group = outer;
        self.layers.insert(index, group);
        self.move_to_group(id, Some(group_id), frame);
        Some(group_id)
    }

    /// Moves `id` into `group` (or out to the top level) without moving it on
    /// screen. Refuses to put a group inside itself.
    pub fn move_to_group(&mut self, id: u64, group: Option<u64>, frame: i32) -> bool {
        if let Some(g) = group
            && (!matches!(self.layer(g).map(|l| &l.kind), Some(LayerKind::Group))
                || self.is_in_group(g, id))
        {
            return false;
        }
        self.keeping_place(id, frame, |project| {
            let layer = project.layer_mut(id).expect("checked by keeping_place");
            layer.group = group;
            // Parents must share the group; anything else would be confusing.
            if let Some(p) = layer.parent
                && project.layer(p).is_none_or(|p| p.group != group)
            {
                project.layer_mut(id).expect("exists").parent = None;
            }
            true
        })
    }

    /// Dissolves a group, keeping its members where they are on screen.
    pub fn ungroup(&mut self, group: u64, frame: i32) {
        let outer = self.layer(group).and_then(|g| g.group);
        let members: Vec<u64> = self.members(Some(group)).map(|l| l.id).collect();
        let Some(mut index) = self.index_of(group) else {
            return;
        };
        for id in members {
            self.move_to_group(id, outer, frame);
            // Members take the group's place in the stack.
            if let Some(i) = self.index_of(id) {
                let layer = self.layers.remove(i);
                if i < index {
                    index -= 1;
                }
                self.layers.insert(index + 1, layer);
                index += 1;
            }
        }
        self.layers.retain(|l| l.id != group);
        for layer in &mut self.layers {
            if layer.parent == Some(group) {
                layer.parent = None;
            }
        }
    }

    /// Moves a layer in time; a group moves its contents too.
    pub fn shift_layer_in_time(&mut self, id: u64, delta: i32) {
        let ids: Vec<u64> = self
            .layers
            .iter()
            .filter(|l| self.is_in_group(l.id, id))
            .map(|l| l.id)
            .collect();
        for layer in self.layers.iter_mut().filter(|l| ids.contains(&l.id)) {
            layer.shift_in_time(delta);
        }
    }

    /// Applies `change` (which may move the layer between spaces) and then
    /// re-expresses the layer's position so it stays put on screen at `frame`.
    /// Animated positions shift every key by the same amount.
    pub(super) fn keeping_place(
        &mut self,
        id: u64,
        frame: i32,
        change: impl FnOnce(&mut Project) -> bool,
    ) -> bool {
        let f = frame as f32;
        let Some(layer) = self.layer(id) else {
            return false;
        };
        let t = &layer.transform;
        let p = t.position.sample(f);
        let z = if layer.is_3d() { t.z.sample(f) } else { 0.0 };
        let world = self.parent_matrix(layer, f).point(vec3(p.x, p.y, z));
        if !change(self) {
            return false;
        }
        let layer = self.layer(id).expect("still exists");
        let Some(inv) = self.parent_matrix(layer, f).inverse() else {
            return true;
        };
        let local = inv.point(world);
        let three_d = layer.is_3d();
        let layer = self.layer_mut(id).expect("still exists");
        shift(&mut layer.transform.position, local.xy() - p);
        if three_d {
            shift(&mut layer.transform.z, local.z - z);
        }
        true
    }
}

fn shift<T: super::anim::Lerp + Copy + std::ops::Add<Output = T>>(
    track: &mut Animated<T>,
    delta: T,
) {
    track.value = track.value + delta;
    for key in &mut track.keyframes {
        key.value = key.value + delta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;
    use crate::model::space::Vec3;
    use egui::Vec2;

    fn world(p: &Project, id: u64) -> Vec2 {
        let l = p.layer(id).unwrap();
        p.world_matrix(l, 0.0).point(Vec3::ZERO).xy()
    }

    #[test]
    fn grouping_keeps_layers_in_place_and_groups_move_them() {
        let mut project = Project::default();
        let a = project.add_shape(ShapeKind::Rectangle, 0);
        project.layer_mut(a).unwrap().transform.position.value = egui::vec2(300.0, 200.0);
        let b = project.add_text("B", 0);
        let g = project.group_layer(a, 0).unwrap();
        assert!(project.move_to_group(b, Some(g), 0));
        assert!((world(&project, a) - egui::vec2(300.0, 200.0)).length() < 1e-3);
        assert!((world(&project, b) - project.center()).length() < 1e-3);

        project.layer_mut(g).unwrap().transform.position.value += egui::vec2(10.0, 20.0);
        assert!((world(&project, a) - egui::vec2(310.0, 220.0)).length() < 1e-3);

        // No group inside itself.
        assert!(!project.move_to_group(g, Some(g), 0));
        let inner = project.group_layer(b, 0).unwrap();
        assert!(!project.move_to_group(g, Some(inner), 0));

        // Ungrouping keeps the layers where they are and removes the group.
        let before = world(&project, a);
        project.ungroup(g, 0);
        assert!(project.layer(g).is_none());
        assert!((world(&project, a) - before).length() < 1e-3);
        assert_eq!(project.layer(a).unwrap().group, None);
        assert_eq!(project.layer(inner).unwrap().group, None);
    }

    #[test]
    fn deleting_and_duplicating_groups_takes_their_contents() {
        let mut project = Project::default();
        let a = project.add_shape(ShapeKind::Ellipse, 0);
        let b = project.add_shape(ShapeKind::Rectangle, 0);
        let g = project.group_layer(a, 0).unwrap();
        project.move_to_group(b, Some(g), 0);
        project.set_parent(b, Some(a));

        let copy = project.duplicate_layer(g).unwrap();
        let copied: Vec<&Layer> = project.members(Some(copy)).collect();
        assert_eq!(copied.len(), 2);
        // The copied child follows the copied parent, not the original.
        let child = copied.iter().find(|l| l.parent.is_some()).unwrap();
        assert!(copied.iter().any(|l| Some(l.id) == child.parent));

        project.remove_layer(g);
        assert!(project.layer(a).is_none() && project.layer(b).is_none());
        assert_eq!(project.layers.len(), 3);
    }

    #[test]
    fn hidden_groups_hide_their_members() {
        let mut project = Project::default();
        let a = project.add_shape(ShapeKind::Ellipse, 0);
        let g = project.group_layer(a, 0).unwrap();
        assert!(project.is_shown_at(project.layer(a).unwrap(), 0));
        project.layer_mut(g).unwrap().visible = false;
        assert!(!project.is_shown_at(project.layer(a).unwrap(), 0));
    }
}
