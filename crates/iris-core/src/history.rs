use crate::EditState;

/// Undo/redo as a list of immutable [`EditState`] snapshots.
#[derive(Clone, Debug)]
pub struct EditHistory {
    steps: Vec<Step>,
    index: usize,
}

#[derive(Clone, Debug)]
struct Step {
    state: EditState,
    label: String,
}

impl EditHistory {
    pub const MAX_STEPS: usize = 500;

    pub fn new(initial: EditState) -> Self {
        Self { steps: vec![Step { state: initial, label: String::new() }], index: 0 }
    }

    pub fn reset(&mut self, initial: EditState) {
        *self = Self::new(initial);
    }

    pub fn current(&self) -> &EditState {
        &self.steps[self.index].state
    }

    /// Records a new state as one undoable step, discarding any redo steps. With
    /// `merge_with_latest` the latest step is updated instead, so a whole slider drag is one
    /// step. A step that ends up equal to the state before it is dropped.
    pub fn record(&mut self, state: EditState, label: &str, merge_with_latest: bool) {
        if state == *self.current() {
            return;
        }
        self.steps.truncate(self.index + 1); // drop redo steps

        if merge_with_latest && self.index > 0 && self.steps[self.index].label == label {
            let drag_ended_at_start = self.steps[self.index - 1].state == state;
            self.steps[self.index].state = state;
            if drag_ended_at_start {
                self.steps.pop();
                self.index -= 1;
            }
            return;
        }

        self.steps.push(Step { state, label: label.to_owned() });
        self.index += 1;
        if self.steps.len() > Self::MAX_STEPS {
            self.steps.remove(0);
            self.index -= 1;
        }
    }

    pub fn can_undo(&self) -> bool {
        self.index > 0
    }

    pub fn can_redo(&self) -> bool {
        self.index + 1 < self.steps.len()
    }

    /// Label of the step that undo() would revert ("" if none).
    pub fn undo_label(&self) -> &str {
        if self.can_undo() { &self.steps[self.index].label } else { "" }
    }

    /// Label of the step that redo() would re-apply ("" if none).
    pub fn redo_label(&self) -> &str {
        if self.can_redo() { &self.steps[self.index + 1].label } else { "" }
    }

    /// Label of the latest step (for merging decisions).
    pub fn latest_label(&self) -> &str {
        &self.steps[self.index].label
    }

    pub fn undo(&mut self) -> &EditState {
        if self.can_undo() {
            self.index -= 1;
        }
        self.current()
    }

    pub fn redo(&mut self) -> &EditState {
        if self.can_redo() {
            self.index += 1;
        }
        self.current()
    }
}

impl Default for EditHistory {
    fn default() -> Self {
        Self::new(EditState::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WhiteBalance;

    fn with_exposure(ev: f32) -> EditState {
        let mut s = EditState::new(WhiteBalance { temperature: 5500.0, tint: 10.0 });
        s.basic.exposure = ev;
        s
    }

    #[test]
    fn undo_redo() {
        let mut h = EditHistory::new(with_exposure(0.0));
        assert!(!h.can_undo());
        h.record(with_exposure(1.0), "Exposure", false);
        h.record(with_exposure(2.0), "Preset", false);
        assert_eq!(h.undo_label(), "Preset");
        assert_eq!(h.undo().basic.exposure, 1.0);
        assert_eq!(h.undo().basic.exposure, 0.0);
        assert!(!h.can_undo());
        assert_eq!(h.redo_label(), "Exposure");
        assert_eq!(h.redo().basic.exposure, 1.0);

        // A new edit after undo discards the redo steps.
        h.record(with_exposure(5.0), "Exposure", false);
        assert!(!h.can_redo());
        assert_eq!(h.undo().basic.exposure, 1.0);
    }

    #[test]
    fn slider_drag_is_one_step() {
        let mut h = EditHistory::new(with_exposure(0.0));
        for ev in [0.1, 0.2, 0.3, 0.4] {
            h.record(with_exposure(ev), "Exposure", true);
        }
        assert_eq!(h.current().basic.exposure, 0.4);
        assert_eq!(h.undo().basic.exposure, 0.0);
        assert!(!h.can_undo());

        // Dragging back to the start leaves no step behind.
        let mut back = EditHistory::new(with_exposure(0.0));
        back.record(with_exposure(0.5), "Exposure", true);
        back.record(with_exposure(0.0), "Exposure", true);
        assert!(!back.can_undo());

        // Different controls are separate steps.
        let mut two = EditHistory::new(with_exposure(0.0));
        two.record(with_exposure(1.0), "Exposure", true);
        let mut contrast = two.current().clone();
        contrast.basic.contrast = 20.0;
        two.record(contrast, "Contrast", true);
        two.undo();
        assert_eq!(two.current().basic.exposure, 1.0);
        assert_eq!(two.current().basic.contrast, 0.0);
    }

    #[test]
    fn history_is_bounded() {
        let mut h = EditHistory::new(with_exposure(0.0));
        for i in 1..=EditHistory::MAX_STEPS + 10 {
            h.record(with_exposure(i as f32 * 0.001), "Exposure", false);
        }
        let mut undos = 0;
        while h.can_undo() {
            h.undo();
            undos += 1;
        }
        assert_eq!(undos, EditHistory::MAX_STEPS - 1);
    }
}
