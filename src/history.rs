//! Snapshot-based undo/redo.
//!
//! Rather than recording every edit as a command, the editor compares the
//! project with the last committed snapshot once a gesture is over (no mouse
//! button held, no text field focused). Any difference becomes one undo step,
//! so a whole drag or a typed name undoes in one go.

use crate::model::Project;

const MAX_STEPS: usize = 200;

pub struct History {
    committed: Project,
    undo: Vec<Project>,
    redo: Vec<Project>,
}

impl History {
    pub fn new(project: &Project) -> Self {
        Self {
            committed: project.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Records `project` as a new undo step if it changed since the last commit.
    pub fn commit(&mut self, project: &Project) -> bool {
        if *project == self.committed {
            return false;
        }
        let previous = std::mem::replace(&mut self.committed, project.clone());
        self.undo.push(previous);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Restores the previous snapshot into `project`. Uncommitted changes are
    /// committed first so they can be redone.
    pub fn undo(&mut self, project: &mut Project) {
        self.commit(project);
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.committed, previous));
            *project = self.committed.clone();
        }
    }

    pub fn redo(&mut self, project: &mut Project) {
        self.commit(project);
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.committed, next));
            *project = self.committed.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShapeKind;

    #[test]
    fn undo_redo_cycle() {
        let mut project = Project::default();
        let mut history = History::new(&project);
        assert!(!history.commit(&project));

        project.add_shape(ShapeKind::Rectangle, 0);
        assert!(history.commit(&project));
        project.name = "Renamed".into();
        // Not committed yet: undo commits it first, then steps back over it.
        history.undo(&mut project);
        assert_eq!(project.name, "Untitled");
        assert_eq!(project.layers.len(), 1);
        history.undo(&mut project);
        assert!(project.layers.is_empty());
        assert!(!history.can_undo());

        history.redo(&mut project);
        history.redo(&mut project);
        assert_eq!(project.name, "Renamed");
        assert!(!history.can_redo());
    }
}
