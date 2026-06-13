//! Shared state and helpers for single-line text fields in the editor UI.

use std::ops::Range;

use gpui::*;

use super::single_line::{cursor_offset, sanitize_pasted_text};

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
        }
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.clear_selection_and_layout();
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
}
