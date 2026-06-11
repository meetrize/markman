//! Shared selection, mouse interaction, and keyboard helpers for single-line inputs.

use std::ops::Range;

use gpui::*;
use unicode_segmentation::GraphemeCursor;

use super::Editor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SingleLineInputTarget {
    WorkspaceSearch,
    DocumentSearch,
    WorkspaceName,
}

impl SingleLineInputTarget {
    pub(super) fn font_scale(self) -> f32 {
        match self {
            Self::WorkspaceName => 0.9,
            Self::WorkspaceSearch | Self::DocumentSearch => 0.78,
        }
    }

    pub(super) fn truncation_suffix(self) -> &'static str {
        match self {
            Self::WorkspaceName => "",
            Self::WorkspaceSearch | Self::DocumentSearch => "…",
        }
    }
}

pub(super) fn primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    (modifiers.platform || modifiers.control) && !modifiers.alt && !modifiers.function
}

pub(super) fn extend_single_line_selection(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    offset: usize,
    text_len: usize,
) {
    let offset = offset.min(text_len);
    if *selection_reversed {
        selected_range.start = offset;
    } else {
        selected_range.end = offset;
    }
    if selected_range.end < selected_range.start {
        *selection_reversed = !*selection_reversed;
        *selected_range = selected_range.end..selected_range.start;
    }
}

pub(super) fn cursor_offset(selected_range: &Range<usize>, selection_reversed: bool) -> usize {
    if selection_reversed {
        selected_range.start
    } else {
        selected_range.end
    }
}

pub(super) fn move_caret_to(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting: &mut bool,
    offset: usize,
    text_len: usize,
) {
    let offset = offset.min(text_len);
    *selected_range = offset..offset;
    *selection_reversed = false;
    *marked_range = None;
    *is_selecting = false;
}

pub(super) fn select_caret_to(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    offset: usize,
    text_len: usize,
) {
    extend_single_line_selection(selected_range, selection_reversed, offset, text_len);
    *marked_range = None;
}

pub(super) fn index_for_mouse_position(
    text_len: usize,
    bounds: Option<&Bounds<Pixels>>,
    line: Option<&ShapedLine>,
    position: Point<Pixels>,
) -> usize {
    if text_len == 0 {
        return 0;
    }

    let (Some(bounds), Some(line)) = (bounds, line) else {
        return text_len;
    };

    if position.x <= bounds.left() {
        return 0;
    }
    if position.x >= bounds.right() {
        return text_len;
    }

    line.closest_index_for_x(position.x - bounds.left())
        .min(text_len)
}

pub(super) fn handle_mouse_down(
    shift: bool,
    offset: usize,
    text_len: usize,
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting: &mut bool,
) {
    if shift {
        select_caret_to(
            selected_range,
            selection_reversed,
            marked_range,
            offset,
            text_len,
        );
    } else {
        move_caret_to(
            selected_range,
            selection_reversed,
            marked_range,
            is_selecting,
            offset,
            text_len,
        );
    }
    *is_selecting = true;
}

/// Prepare selection for a context menu click: keep the active range when the
/// click lands inside it, otherwise move the caret to the click position.
pub(super) fn prepare_context_menu_selection(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting: &mut bool,
    offset: usize,
    text_len: usize,
) {
    let offset = offset.min(text_len);
    if selected_range.start < selected_range.end
        && (selected_range.start..=selected_range.end).contains(&offset)
    {
        *is_selecting = false;
        *marked_range = None;
        return;
    }
    move_caret_to(
        selected_range,
        selection_reversed,
        marked_range,
        is_selecting,
        offset,
        text_len,
    );
}

/// Returns `true` when the caller should notify the UI.
pub(super) fn handle_mouse_move(
    dragging: bool,
    offset: usize,
    text_len: usize,
    is_selecting: bool,
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting_flag: &mut bool,
) -> bool {
    if !is_selecting {
        return false;
    }
    if !dragging {
        *is_selecting_flag = false;
        return true;
    }
    select_caret_to(
        selected_range,
        selection_reversed,
        marked_range,
        offset,
        text_len,
    );
    true
}

/// Returns `true` when the caller should notify the UI.
pub(super) fn handle_mouse_up(is_selecting_flag: &mut bool) -> bool {
    if *is_selecting_flag {
        *is_selecting_flag = false;
        return true;
    }
    false
}

pub(super) fn sanitize_pasted_text(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

pub(super) fn text_grapheme_boundary(text: &str, offset: usize, backward: bool) -> usize {
    let offset = offset.min(text.len());
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    if backward {
        cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
    } else {
        cursor
            .next_boundary(text, 0)
            .ok()
            .flatten()
            .unwrap_or(text.len())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SingleLineArrowKey {
    MoveLeft,
    MoveRight,
    Home,
    End,
    SelectLeft,
    SelectRight,
    SelectHome,
    SelectEnd,
}

pub(super) fn arrow_key_from_event(event: &KeyDownEvent) -> Option<SingleLineArrowKey> {
    if event.keystroke.modifiers.platform
        || event.keystroke.modifiers.control
        || event.keystroke.modifiers.alt
    {
        return None;
    }
    let shift = event.keystroke.modifiers.shift;
    match event.keystroke.key.as_str() {
        "left" if shift => Some(SingleLineArrowKey::SelectLeft),
        "left" => Some(SingleLineArrowKey::MoveLeft),
        "right" if shift => Some(SingleLineArrowKey::SelectRight),
        "right" => Some(SingleLineArrowKey::MoveRight),
        "home" if shift => Some(SingleLineArrowKey::SelectHome),
        "home" => Some(SingleLineArrowKey::Home),
        "end" if shift => Some(SingleLineArrowKey::SelectEnd),
        "end" => Some(SingleLineArrowKey::End),
        _ => None,
    }
}

impl Editor {
    pub(super) fn single_line_input_focus_handle(
        &self,
        target: SingleLineInputTarget,
    ) -> FocusHandle {
        match target {
            SingleLineInputTarget::WorkspaceSearch => self.workspace_search_focus.clone(),
            SingleLineInputTarget::DocumentSearch => self.document_search_focus.clone(),
            SingleLineInputTarget::WorkspaceName => self.workspace_name_focus.clone(),
        }
    }
}
