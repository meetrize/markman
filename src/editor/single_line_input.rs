//! Shared selection, mouse interaction, and keyboard helpers for single-line inputs.

use gpui::*;

pub(super) use crate::input::single_line::{
    cursor_offset, handle_mouse_down, handle_mouse_move, handle_mouse_up,
    index_for_mouse_position, move_caret_to, primary_shortcut_modifiers, sanitize_pasted_text,
    select_caret_to, text_grapheme_boundary,
};

use super::Editor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SingleLineInputTarget {
    WorkspaceSearch,
    DocumentSearch,
    WorkspaceName,
    QuickFileOpen,
}

impl SingleLineInputTarget {
    pub(super) fn font_scale(self) -> f32 {
        match self {
            Self::WorkspaceName => 0.9,
            Self::WorkspaceSearch | Self::DocumentSearch => 0.78,
            Self::QuickFileOpen => 1.0,
        }
    }

    pub(super) fn truncation_suffix(self) -> &'static str {
        match self {
            Self::WorkspaceName => "",
            Self::WorkspaceSearch | Self::DocumentSearch => "…",
            Self::QuickFileOpen => "",
        }
    }
}

/// Prepare selection for a context menu click: keep the active range when the
/// click lands inside it, otherwise move the caret to the click position.
pub(super) fn prepare_context_menu_selection(
    selected_range: &mut std::ops::Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<std::ops::Range<usize>>,
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
            SingleLineInputTarget::WorkspaceSearch => self.workspace.search_focus.clone(),
            SingleLineInputTarget::DocumentSearch => self.search.focus.clone(),
            SingleLineInputTarget::WorkspaceName => self.workspace.name_focus.clone(),
            SingleLineInputTarget::QuickFileOpen => self.quick_file_open.focus_handle.clone(),
        }
    }
}
