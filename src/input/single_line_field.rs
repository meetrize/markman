//! Shared state and helpers for single-line text fields in the editor UI.

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui::*;

use super::single_line::{cursor_offset, sanitize_pasted_text};

const UNDO_COALESCE_WINDOW: Duration = Duration::from_millis(400);
const UNDO_LIMIT: usize = 100;

/// Snapshot of editable single-line field state for undo/redo.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SingleLineFieldSnapshot {
    query: String,
    marked_range: Option<Range<usize>>,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

/// Text, selection, IME composition, and layout state for a single-line input.
#[derive(Clone, Debug)]
pub(crate) struct SingleLineFieldState {
    pub query: String,
    pub marked_range: Option<Range<usize>>,
    pub selected_range: Range<usize>,
    pub selection_reversed: bool,
    pub is_selecting: bool,
    pub last_layout: Option<ShapedLine>,
    pub last_bounds: Option<Bounds<Pixels>>,
    undo_stack: Vec<SingleLineFieldSnapshot>,
    redo_stack: Vec<SingleLineFieldSnapshot>,
    last_undo_push: Option<Instant>,
}

impl Default for SingleLineFieldState {
    fn default() -> Self {
        Self::new()
    }
}

impl SingleLineFieldState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            marked_range: None,
            selected_range: 0..0,
            selection_reversed: false,
            is_selecting: false,
            last_layout: None,
            last_bounds: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_undo_push: None,
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.clear_selection_and_layout();
        self.clear_undo_history();
    }

    pub fn clear_undo_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_undo_push = None;
    }

    fn snapshot(&self) -> SingleLineFieldSnapshot {
        SingleLineFieldSnapshot {
            query: self.query.clone(),
            marked_range: self.marked_range.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    fn restore(&mut self, snapshot: SingleLineFieldSnapshot) {
        self.query = snapshot.query;
        self.marked_range = snapshot.marked_range;
        self.selected_range = snapshot.selected_range;
        self.selection_reversed = snapshot.selection_reversed;
        self.is_selecting = false;
        self.last_layout = None;
        self.last_bounds = None;
    }

    /// Push the current state onto the undo stack before applying an edit.
    ///
    /// When `coalesce` is true (IME composition), rapid successive edits within
    /// [`UNDO_COALESCE_WINDOW`] share one undo entry.
    pub fn prepare_undo(&mut self, coalesce: bool) {
        if coalesce
            && self.last_undo_push.is_some_and(|last| {
                last.elapsed() <= UNDO_COALESCE_WINDOW
            })
        {
            return;
        }

        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
        if self.undo_stack.len() > UNDO_LIMIT {
            let overflow = self.undo_stack.len() - UNDO_LIMIT;
            self.undo_stack.drain(0..overflow);
        }
        self.last_undo_push = Some(Instant::now());
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.restore(snapshot);
        self.last_undo_push = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.snapshot());
        self.restore(snapshot);
        self.last_undo_push = None;
        true
    }

    pub fn clear_selection_and_layout(&mut self) {
        self.marked_range = None;
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.is_selecting = false;
        self.last_layout = None;
        self.last_bounds = None;
    }

    pub fn sync_caret_to_end(&mut self) {
        let len = self.text_len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
        self.is_selecting = false;
    }

    pub fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub fn text_len(&self) -> usize {
        self.query.len()
    }

    pub fn cursor_offset(&self) -> usize {
        cursor_offset(&self.selected_range, self.selection_reversed)
    }

    pub fn replace_text(&mut self, range: Range<usize>, new_text: &str) {
        let end = range.end.min(self.query.len());
        let start = range.start.min(end);
        self.query.replace_range(start..end, new_text);
    }

    pub fn delete_backward(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.pop();
        true
    }

    pub fn sanitize_paste(&self, text: &str) -> String {
        sanitize_pasted_text(text)
    }

    pub fn set_layout(&mut self, line: ShapedLine, bounds: Bounds<Pixels>) {
        self.last_layout = Some(line);
        self.last_bounds = Some(bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::SingleLineFieldState;

    #[test]
    fn clear_resets_query_and_selection_state() {
        let mut field = SingleLineFieldState::new();
        field.query.push_str("hello");
        field.selected_range = 1..4;
        field.selection_reversed = true;
        field.marked_range = Some(0..2);
        field.is_selecting = true;

        field.clear();

        assert!(field.query.is_empty());
        assert_eq!(field.selected_range, 0..0);
        assert!(!field.selection_reversed);
        assert!(field.marked_range.is_none());
        assert!(!field.is_selecting);
    }

    #[test]
    fn replace_text_clamps_out_of_bounds_range() {
        let mut field = SingleLineFieldState::new();
        field.query = "hello".into();
        field.replace_text(3..99, "p");
        assert_eq!(field.query, "help");
    }

    #[test]
    fn sanitize_paste_flattens_newlines() {
        let field = SingleLineFieldState::new();
        assert_eq!(field.sanitize_paste("a\r\nb\nc"), "a b c");
    }

    #[test]
    fn undo_redo_restores_text_and_selection() {
        let mut field = SingleLineFieldState::new();
        field.query = "hello".into();
        field.selected_range = 5..5;

        field.prepare_undo(false);
        field.query = "hello world".into();
        field.selected_range = 11..11;

        assert!(field.undo());
        assert_eq!(field.query, "hello");
        assert_eq!(field.selected_range, 5..5);

        assert!(field.redo());
        assert_eq!(field.query, "hello world");
        assert_eq!(field.selected_range, 11..11);
    }

    #[test]
    fn clear_resets_undo_history() {
        let mut field = SingleLineFieldState::new();
        field.query = "a".into();
        field.prepare_undo(false);
        field.query = "b".into();
        field.clear();
        assert!(!field.undo());
    }
}
