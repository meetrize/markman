//! In-document search state: query input, match list, and source highlight range.

use std::ops::Range;

use gpui::*;

use super::super::document_search::DocumentSearchState;
use super::super::Editor;

/// Owns document search UI state and the transient source-mode match highlight.
pub(in crate::editor) struct SearchController {
    pub(in crate::editor) state: DocumentSearchState,
    pub(in crate::editor) focus: FocusHandle,
    pub(in crate::editor) match_source_range: Option<Range<usize>>,
}

impl SearchController {
    pub(in crate::editor) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            state: DocumentSearchState::default(),
            focus: cx.focus_handle(),
            match_source_range: None,
        }
    }
}
