//! The edits of the open photo: current state, undo history, and whether they differ from
//! what is saved in the photo's sidecar.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iris_core::{EditHistory, EditState, WhiteBalance};
use iris_persist::{read_sidecar, sidecar_path_for, write_sidecar};

/// Mergeable edits with the same label closer together than this become one undo step.
const MERGE_WINDOW: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct EditDocument {
    raw_path: Option<PathBuf>,
    defaults: EditState,
    saved: EditState,
    history: EditHistory,
    last_edit: Option<Instant>,
    in_gesture: bool,
    /// The current gesture has its undo step.
    gesture_recorded: bool,
}

impl EditDocument {
    /// Starts editing a photo: loads its sidecar if there is one, otherwise the defaults.
    /// Returns a warning if a sidecar exists but could not be read.
    pub fn load(&mut self, raw_path: &Path, as_shot: WhiteBalance) -> Option<String> {
        self.defaults = EditState::new(as_shot);
        let (initial, warning) = match read_sidecar(raw_path, &self.defaults) {
            Ok(Some(edits)) => (edits, None),
            Ok(None) => (self.defaults.clone(), None),
            Err(e) => (self.defaults.clone(), Some(e.to_string())),
        };
        self.saved = initial.clone();
        self.history.reset(initial);
        self.raw_path = Some(raw_path.to_owned());
        self.last_edit = None;
        self.in_gesture = false;
        warning
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_loaded(&self) -> bool {
        self.raw_path.is_some()
    }

    pub fn raw_path(&self) -> Option<&Path> {
        self.raw_path.as_deref()
    }

    pub fn edits(&self) -> &EditState {
        self.history.current()
    }

    pub fn defaults(&self) -> &EditState {
        &self.defaults
    }

    pub fn is_dirty(&self) -> bool {
        self.is_loaded() && *self.edits() != self.saved
    }

    pub fn sidecar_path(&self) -> Option<PathBuf> {
        self.raw_path.as_deref().map(sidecar_path_for)
    }

    /// Records a change. Mergeable changes with the same label that follow each other
    /// quickly (a slider drag) become one undo step.
    pub fn edit(&mut self, state: EditState, label: &str, mergeable: bool) {
        self.edit_at(state, label, mergeable, Instant::now());
    }

    fn edit_at(&mut self, state: EditState, label: &str, mergeable: bool, now: Instant) {
        if !self.is_loaded() || state == *self.edits() {
            return;
        }
        let same_label = self.history.latest_label() == label;
        let merge = if self.in_gesture {
            mergeable && self.gesture_recorded && same_label
        } else {
            mergeable && same_label && self.last_edit.is_some_and(|t| now.duration_since(t) < MERGE_WINDOW)
        };
        self.history.record(state, label, merge);
        if self.in_gesture {
            self.gesture_recorded = true;
        }
        self.last_edit = mergeable.then_some(now);
    }

    /// Between these, mergeable edits with the same label become one undo step however
    /// long the gesture takes (a brush stroke), and never merge with an earlier step.
    pub fn begin_gesture(&mut self) {
        self.in_gesture = true;
        self.gesture_recorded = false;
    }

    pub fn end_gesture(&mut self) {
        self.in_gesture = false;
        self.last_edit = None; // the next edit starts a new step
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_label(&self) -> &str {
        self.history.undo_label()
    }

    pub fn redo_label(&self) -> &str {
        self.history.redo_label()
    }

    /// Returns true if anything changed.
    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        self.history.undo();
        self.last_edit = None;
        true
    }

    /// Returns true if anything changed.
    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        self.history.redo();
        self.last_edit = None;
        true
    }

    /// Saves the edits to the photo's sidecar.
    pub fn save(&mut self) -> Result<(), String> {
        let (Some(raw), Some(path)) = (self.raw_path.clone(), self.sidecar_path()) else { return Ok(()) };
        write_sidecar(&path, &raw, self.edits()).map_err(|e| e.to_string())?;
        self.saved = self.edits().clone();
        Ok(())
    }

    /// Saves the edits to another file. Only saving to the photo's own sidecar marks
    /// the edits as saved.
    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let Some(raw) = self.raw_path.clone() else { return Ok(()) };
        write_sidecar(path, &raw, self.edits()).map_err(|e| e.to_string())?;
        if Some(path) == self.sidecar_path().as_deref() {
            self.saved = self.edits().clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AS_SHOT: WhiteBalance = WhiteBalance { temperature: 5500.0, tint: 10.0 };

    fn loaded() -> (tempfile::TempDir, EditDocument) {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("photo.ARW");
        std::fs::write(&raw, "raw").unwrap();
        let mut doc = EditDocument::default();
        assert!(doc.load(&raw, AS_SHOT).is_none());
        (dir, doc)
    }

    fn with_exposure(doc: &EditDocument, ev: f32) -> EditState {
        let mut s = doc.edits().clone();
        s.basic.exposure = ev;
        s
    }

    #[test]
    fn quick_slider_changes_merge_slow_ones_do_not() {
        let (_dir, mut doc) = loaded();
        let t0 = Instant::now();
        doc.edit_at(with_exposure(&doc, 0.1), "Exposure", true, t0);
        doc.edit_at(with_exposure(&doc, 0.2), "Exposure", true, t0 + Duration::from_millis(100));
        assert!(doc.undo());
        assert!(!doc.can_undo()); // one step

        doc.edit_at(with_exposure(&doc, 0.1), "Exposure", true, t0);
        doc.edit_at(with_exposure(&doc, 0.2), "Exposure", true, t0 + Duration::from_secs(5));
        assert!(doc.undo());
        assert!(doc.can_undo()); // two steps
    }

    #[test]
    fn gestures_are_one_step_and_do_not_merge_with_earlier_ones() {
        let (_dir, mut doc) = loaded();
        doc.edit(with_exposure(&doc, 0.1), "Brush Stroke", true);
        doc.begin_gesture();
        for ev in [0.2, 0.3, 0.4] {
            doc.edit(with_exposure(&doc, ev), "Brush Stroke", true);
        }
        doc.end_gesture();
        assert!(doc.undo());
        assert_eq!(doc.edits().basic.exposure, 0.1);
    }

    #[test]
    fn dirty_tracking_and_saving() {
        let (dir, mut doc) = loaded();
        assert!(!doc.is_dirty());
        doc.edit(with_exposure(&doc, 1.0), "Exposure", false);
        assert!(doc.is_dirty());
        assert_eq!(doc.undo_label(), "Exposure");

        let other = dir.path().join("copy.iris.json");
        doc.save_as(&other).unwrap();
        assert!(doc.is_dirty()); // another file
        doc.save().unwrap();
        assert!(!doc.is_dirty());
        assert!(dir.path().join("photo.iris.json").exists());

        // Reloading restores the saved edits.
        let raw = doc.raw_path().unwrap().to_owned();
        let mut again = EditDocument::default();
        again.load(&raw, AS_SHOT);
        assert_eq!(again.edits().basic.exposure, 1.0);
        assert!(!again.is_dirty());
    }

    #[test]
    fn unreadable_sidecar_gives_a_warning_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("photo.ARW");
        std::fs::write(dir.path().join("photo.iris.json"), "{ broken").unwrap();
        let mut doc = EditDocument::default();
        assert!(doc.load(&raw, AS_SHOT).is_some());
        assert_eq!(*doc.edits(), EditState::new(AS_SHOT));
    }
}
