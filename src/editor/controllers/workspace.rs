//! Workspace panel state: file tree, outline, cross-file search, and file dialogs.

use gpui::*;

use super::super::workspace::WorkspaceState;
use super::super::workspace_file_menu::{
    WorkspaceFileContextMenuState, WorkspaceNameDialogState,
};
use super::super::PendingWorkspaceSearchJump;
use super::super::Editor;

/// Active drag session for resizing the workspace panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::editor) struct WorkspaceResizeDragSession {
    pub(in crate::editor) start_pointer_x: f32,
    pub(in crate::editor) start_width: f32,
    pub(in crate::editor) viewport_width: f32,
}

/// Owns workspace panel data plus related focus handles and overlay state.
pub(in crate::editor) struct WorkspaceController {
    pub(in crate::editor) state: WorkspaceState,
    pub(in crate::editor) search_focus: FocusHandle,
    pub(in crate::editor) name_focus: FocusHandle,
    pub(in crate::editor) file_context_menu: Option<WorkspaceFileContextMenuState>,
    pub(in crate::editor) name_dialog: Option<WorkspaceNameDialogState>,
    pub(in crate::editor) resize_drag: Option<WorkspaceResizeDragSession>,
    pub(in crate::editor) pending_search_jump: Option<PendingWorkspaceSearchJump>,
}

impl WorkspaceController {
    pub(in crate::editor) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            state: WorkspaceState::default(),
            search_focus: cx.focus_handle(),
            name_focus: cx.focus_handle(),
            file_context_menu: None,
            name_dialog: None,
            resize_drag: None,
            pending_search_jump: None,
        }
    }
}
