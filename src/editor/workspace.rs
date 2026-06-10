//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::*;

use super::{BlockKind, Editor, PendingWorkspaceSearchJump};
use super::workspace_search_input::WorkspaceSearchInputElement;
use crate::components::{
    Copy, Cut, Delete, DeleteBack, End, Home, MoveLeft, MoveRight, Paste, SelectAll, SelectEnd,
    SelectHome, SelectLeft, SelectRight,
};
use crate::i18n::I18nStrings;
use crate::theme::Theme;
use unicode_segmentation::GraphemeCursor;

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const CHEVRON_RIGHT_ICON: &str = "icon/workspace/chevron-right.svg";
const CHEVRON_DOWN_ICON: &str = "icon/workspace/chevron-down.svg";
const FILES_TAB_ICON: &str = "icon/workspace/files.svg";
const OUTLINE_TAB_ICON: &str = "icon/workspace/list-tree.svg";
const SEARCH_ICON: &str = "icon/workspace/search.svg";
const WORKSPACE_PANEL_TARGET_RATIO: f32 = 0.15;
const WORKSPACE_PANEL_MIN_WIDTH: f32 = 180.0;
const WORKSPACE_PANEL_MAX_WIDTH: f32 = 360.0;
const WORKSPACE_PANEL_MAX_VIEWPORT_RATIO: f32 = 0.45;
const WORKSPACE_NODE_HEIGHT: f32 = 22.0;
const WORKSPACE_NODE_INDENT: f32 = 12.0;
const WORKSPACE_CHEVRON_SIZE: f32 = 12.0;
const WORKSPACE_ICON_SIZE: f32 = 14.0;
const WORKSPACE_TAB_ICON_SIZE: f32 = 12.0;
const WORKSPACE_RESIZE_HANDLE_WIDTH: f32 = 5.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceSearchResult {
    path: PathBuf,
    line: Option<usize>,
    preview: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceTab {
    #[default]
    Files,
    Outline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceTreeKind {
    Directory(PathBuf),
    MarkdownFile(PathBuf),
    Heading { line: usize, level: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkspaceTreeNode {
    id: String,
    label: String,
    kind: WorkspaceTreeKind,
    children: Vec<WorkspaceTreeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceSelection {
    File(PathBuf),
    Outline(String),
}

pub(super) struct WorkspaceState {
    is_open: bool,
    folder_root: Option<PathBuf>,
    panel_width: Option<f32>,
    active_tab: WorkspaceTab,
    root: Option<PathBuf>,
    file_tree: Option<WorkspaceTreeNode>,
    file_error: Option<String>,
    outline_tree: Vec<WorkspaceTreeNode>,
    outline_source: Option<String>,
    expanded: HashSet<String>,
    selected: Option<WorkspaceSelection>,
    search_open: bool,
    search_query: String,
    search_results: Vec<WorkspaceSearchResult>,
    search_marked_range: Option<Range<usize>>,
    search_selected_range: Range<usize>,
    search_selection_reversed: bool,
    search_is_selecting: bool,
    search_last_layout: Option<ShapedLine>,
    search_last_bounds: Option<Bounds<Pixels>>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            is_open: true,
            folder_root: None,
            panel_width: None,
            active_tab: WorkspaceTab::default(),
            root: None,
            file_tree: None,
            file_error: None,
            outline_tree: Vec::new(),
            outline_source: None,
            expanded: HashSet::new(),
            selected: None,
            search_open: false,
            search_query: String::new(),
            search_results: Vec::new(),
            search_marked_range: None,
            search_selected_range: 0..0,
            search_selection_reversed: false,
            search_is_selecting: false,
            search_last_layout: None,
            search_last_bounds: None,
        }
    }
}

impl Editor {
    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_open {
            self.workspace.is_open = false;
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.is_open = true;
            self.sync_workspace_models(cx);
            window.activate_window();
        }
        cx.notify();
    }

    pub(crate) fn on_toggle_workspace_action(
        &mut self,
        _: &crate::components::ToggleWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_drawer(window, cx);
    }

    pub(super) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if self.workspace.folder_root.is_none() {
            self.workspace.root = None;
            self.workspace.file_tree = None;
            self.workspace.file_error = None;
        }
        self.workspace.outline_source = None;
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
    }

    fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree();
        self.sync_workspace_outline(cx);
    }

    fn workspace_root_for_current_file(&self) -> Option<PathBuf> {
        self.file_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    fn effective_workspace_root(&self) -> Option<PathBuf> {
        self.workspace
            .folder_root
            .clone()
            .or_else(|| self.workspace_root_for_current_file())
    }

    fn sync_workspace_file_tree(&mut self) {
        let next_root = self.effective_workspace_root();
        if self.workspace.root == next_root && self.workspace.file_tree.is_some() {
            self.workspace.selected = self
                .file_path
                .as_ref()
                .map(|path| WorkspaceSelection::File(path.clone()));
            return;
        }

        self.workspace.root = next_root.clone();
        self.workspace.file_tree = None;
        self.workspace.file_error = None;

        let Some(root) = next_root else {
            self.workspace.selected = None;
            return;
        };

        // Validate the root path
        if root.as_os_str().is_empty() {
            self.workspace.file_error = Some("Invalid workspace path: empty path".to_string());
            self.workspace.selected = None;
            return;
        }

        match scan_workspace_dir(&root) {
            Ok(tree) => {
                self.workspace.expanded.insert(tree.id.clone());
                self.workspace.file_tree = Some(tree);
                self.workspace.selected = self
                    .file_path
                    .as_ref()
                    .map(|path| WorkspaceSelection::File(path.clone()));
            }
            Err(err) => {
                self.workspace.file_error = Some(err.to_string());
            }
        }
    }

    fn sync_workspace_outline(&mut self, cx: &mut Context<Self>) {
        let source = self.serialized_document_text(cx);
        if self.workspace.outline_source.as_deref() == Some(source.as_str()) {
            return;
        }

        let outline = build_outline_tree(&source);
        prune_outline_state(&mut self.workspace, &outline);
        self.workspace.outline_tree = outline;
        self.workspace.outline_source = Some(source);
    }

    pub(crate) fn open_workspace_folder(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);
        self.workspace.folder_root = Some(path);
        self.workspace.root = None;
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.is_open = true;
        self.sync_workspace_models(cx);
        window.activate_window();
        cx.notify();
    }

    fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        if self.workspace.active_tab == tab && !self.workspace.search_open {
            return;
        }
        self.workspace.active_tab = tab;
        self.workspace.search_open = false;
        self.sync_workspace_models(cx);
        cx.notify();
    }

    fn sync_workspace_search_selection(&mut self) {
        let len = self.workspace.search_query.len();
        self.workspace.search_selected_range = len..len;
        self.workspace.search_marked_range = None;
        self.workspace.search_selection_reversed = false;
        self.workspace.search_is_selecting = false;
    }

    pub(super) fn workspace_search_input_active(&self, window: &Window) -> bool {
        self.workspace.search_open && self.workspace_search_focus.is_focused(window)
    }

    pub(super) fn workspace_search_query_is_empty(&self) -> bool {
        self.workspace.search_query.is_empty()
    }

    pub(super) fn workspace_search_display_text(&self, placeholder: &SharedString) -> SharedString {
        if self.workspace.search_query.is_empty() {
            placeholder.clone()
        } else {
            self.workspace.search_query.clone().into()
        }
    }

    pub(super) fn workspace_search_marked_range(&self) -> Option<Range<usize>> {
        self.workspace.search_marked_range.clone()
    }

    pub(super) fn workspace_search_selected_range(&self) -> Range<usize> {
        self.workspace.search_selected_range.clone()
    }

    pub(super) fn workspace_search_index_for_mouse_position(
        &self,
        position: Point<Pixels>,
    ) -> usize {
        let query = &self.workspace.search_query;
        if query.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (
            self.workspace.search_last_bounds.as_ref(),
            self.workspace.search_last_layout.as_ref(),
        ) else {
            return query.len();
        };

        if position.x <= bounds.left() {
            return 0;
        }
        if position.x >= bounds.right() {
            return query.len();
        }

        line.closest_index_for_x(position.x - bounds.left())
            .min(query.len())
    }

    pub(super) fn workspace_search_cursor_offset(&self) -> usize {
        if self.workspace.search_selection_reversed {
            self.workspace.search_selected_range.start
        } else {
            self.workspace.search_selected_range.end
        }
    }

    pub(super) fn workspace_search_focus_handle(&self) -> FocusHandle {
        self.workspace_search_focus.clone()
    }

    pub(super) fn set_workspace_search_layout(
        &mut self,
        layout: ShapedLine,
        bounds: Bounds<Pixels>,
    ) {
        self.workspace.search_last_layout = Some(layout);
        self.workspace.search_last_bounds = Some(bounds);
    }

    pub(super) fn replace_workspace_search_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        mark_composing: bool,
        new_selected: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let sanitized = new_text.replace(['\r', '\n'], "");
        let query = &mut self.workspace.search_query;
        let start = range.start.min(query.len());
        let end = range.end.min(query.len());
        query.replace_range(start..end, &sanitized);

        if mark_composing && !sanitized.is_empty() {
            self.workspace.search_marked_range = Some(start..start + sanitized.len());
        } else {
            self.workspace.search_marked_range = None;
        }

        self.workspace.search_selected_range = new_selected.unwrap_or_else(|| {
            let cursor = start + sanitized.len();
            cursor..cursor
        });
        self.workspace.search_selection_reversed = false;
        self.run_workspace_search();
        cx.notify();
    }

    fn toggle_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.search_open = !self.workspace.search_open;
        if self.workspace.search_open {
            self.workspace.active_tab = WorkspaceTab::Files;
            self.sync_workspace_search_selection();
            window.focus(&self.workspace_search_focus);
            self.run_workspace_search();
        } else {
            self.workspace.search_query.clear();
            self.workspace.search_results.clear();
            self.workspace.search_marked_range = None;
            self.workspace.search_selected_range = 0..0;
            self.workspace.search_selection_reversed = false;
        }
        cx.notify();
    }

    fn close_workspace_search(&mut self, cx: &mut Context<Self>) {
        if self.workspace.search_open {
            self.workspace.search_open = false;
            self.workspace.search_query.clear();
            self.workspace.search_results.clear();
            self.workspace.search_marked_range = None;
            self.workspace.search_selected_range = 0..0;
            self.workspace.search_selection_reversed = false;
            cx.notify();
        }
    }

    fn focus_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.search_open {
            self.workspace.search_open = true;
            self.workspace.active_tab = WorkspaceTab::Files;
            self.sync_workspace_search_selection();
        }
        window.focus(&self.workspace_search_focus);
        cx.notify();
    }

    fn run_workspace_search(&mut self) {
        let Some(root) = self.effective_workspace_root() else {
            self.workspace.search_results.clear();
            return;
        };

        self.workspace.search_results = search_markdown_files(&root, &self.workspace.search_query);
    }

    pub(crate) fn on_workspace_search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.search_open || !self.workspace_search_focus.is_focused(window) {
            return;
        }

        let modifiers = &event.keystroke.modifiers;
        if workspace_search_primary_shortcut_modifiers(modifiers) {
            match event.keystroke.key.as_str() {
                "v" => {
                    self.workspace_search_paste_from_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                "c" => {
                    self.workspace_search_copy_to_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                "x" => {
                    self.workspace_search_cut_to_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                "a" => {
                    self.workspace_search_select_all_text(cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_workspace_search(cx);
                cx.stop_propagation();
            }
            "enter" => {
                self.run_workspace_search();
                cx.stop_propagation();
                cx.notify();
            }
            "backspace" => {
                self.workspace_search_delete_backward(cx);
                cx.stop_propagation();
            }
            "delete" => {
                self.workspace_search_delete_forward(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(crate) fn on_workspace_search_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.focus_workspace_search(window, cx);
        self.workspace.search_is_selecting = true;
        let offset = self.workspace_search_index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.workspace_search_select_to(offset, cx);
        } else {
            self.workspace_search_move_to(offset, cx);
        }
    }

    pub(crate) fn on_workspace_search_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.search_is_selecting {
            self.workspace.search_is_selecting = false;
            cx.notify();
        }
    }

    pub(crate) fn on_workspace_search_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.search_is_selecting || !self.workspace_search_input_active(window) {
            return;
        }
        self.workspace_search_select_to(
            self.workspace_search_index_for_mouse_position(event.position),
            cx,
        );
    }

    pub(crate) fn on_workspace_search_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_delete_backward(cx);
    }

    pub(crate) fn on_workspace_search_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_delete_forward(cx);
    }

    fn workspace_search_delete_backward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.workspace.search_marked_range.clone() {
            let cursor = marked.start;
            self.replace_workspace_search_text(
                marked,
                "",
                false,
                Some(cursor..cursor),
                cx,
            );
            return;
        }

        let selected = self.workspace.search_selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor == 0 {
                return;
            }
            let previous = workspace_search_grapheme_boundary(&self.workspace.search_query, cursor, true);
            previous..cursor
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_workspace_search_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn workspace_search_delete_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.workspace.search_marked_range.clone() {
            let cursor = marked.start;
            self.replace_workspace_search_text(
                marked,
                "",
                false,
                Some(cursor..cursor),
                cx,
            );
            return;
        }

        let query_len = self.workspace.search_query.len();
        let selected = self.workspace.search_selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor >= query_len {
                return;
            }
            let next = workspace_search_grapheme_boundary(&self.workspace.search_query, cursor, false);
            cursor..next
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_workspace_search_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn workspace_search_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.workspace.search_query.len());
        self.workspace.search_selected_range = clamped..clamped;
        self.workspace.search_selection_reversed = false;
        self.workspace.search_marked_range = None;
        self.workspace.search_is_selecting = false;
        cx.notify();
    }

    fn workspace_search_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.workspace.search_query.len());
        if self.workspace.search_selection_reversed {
            self.workspace.search_selected_range.start = clamped;
        } else {
            self.workspace.search_selected_range.end = clamped;
        }
        if self.workspace.search_selected_range.end < self.workspace.search_selected_range.start {
            self.workspace.search_selection_reversed = !self.workspace.search_selection_reversed;
            self.workspace.search_selected_range =
                self.workspace.search_selected_range.end..self.workspace.search_selected_range.start;
        }
        self.workspace.search_marked_range = None;
        cx.notify();
    }

    fn workspace_search_replace_selection(
        &mut self,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        let sanitized = new_text.replace("\r\n", " ").replace(['\r', '\n'], " ");
        self.replace_workspace_search_text(
            self.workspace.search_selected_range.clone(),
            &sanitized,
            false,
            None,
            cx,
        );
    }

    fn workspace_search_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.workspace_search_replace_selection(&text, cx);
        }
    }

    fn workspace_search_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.search_selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.workspace.search_query
                    [self.workspace.search_selected_range.clone()]
                    .to_string(),
            ));
        }
    }

    fn workspace_search_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.search_selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.workspace.search_query
                    [self.workspace.search_selected_range.clone()]
                    .to_string(),
            ));
            self.workspace_search_replace_selection("", cx);
        }
    }

    fn workspace_search_select_all_text(&mut self, cx: &mut Context<Self>) {
        self.workspace_search_move_to(0, cx);
        self.workspace_search_select_to(self.workspace.search_query.len(), cx);
    }

    pub(crate) fn on_workspace_search_paste(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_paste_from_clipboard(cx);
    }

    pub(crate) fn on_workspace_search_copy(
        &mut self,
        _: &Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_copy_to_clipboard(cx);
    }

    pub(crate) fn on_workspace_search_cut(
        &mut self,
        _: &Cut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_cut_to_clipboard(cx);
    }

    pub(crate) fn on_workspace_search_select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_select_all_text(cx);
    }

    pub(crate) fn on_workspace_search_move_left(
        &mut self,
        _: &MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        if self.workspace.search_selected_range.is_empty() {
            let previous = workspace_search_grapheme_boundary(
                &self.workspace.search_query,
                self.workspace_search_cursor_offset(),
                true,
            );
            self.workspace_search_move_to(previous, cx);
        } else {
            self.workspace_search_move_to(self.workspace.search_selected_range.start, cx);
        }
    }

    pub(crate) fn on_workspace_search_move_right(
        &mut self,
        _: &MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        if self.workspace.search_selected_range.is_empty() {
            let next = workspace_search_grapheme_boundary(
                &self.workspace.search_query,
                self.workspace_search_cursor_offset(),
                false,
            );
            self.workspace_search_move_to(next, cx);
        } else {
            self.workspace_search_move_to(self.workspace.search_selected_range.end, cx);
        }
    }

    pub(crate) fn on_workspace_search_home(
        &mut self,
        _: &Home,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_move_to(0, cx);
    }

    pub(crate) fn on_workspace_search_end(
        &mut self,
        _: &End,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_move_to(self.workspace.search_query.len(), cx);
    }

    pub(crate) fn on_workspace_search_select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_select_to(
            workspace_search_grapheme_boundary(
                &self.workspace.search_query,
                self.workspace_search_cursor_offset(),
                true,
            ),
            cx,
        );
    }

    pub(crate) fn on_workspace_search_select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_select_to(
            workspace_search_grapheme_boundary(
                &self.workspace.search_query,
                self.workspace_search_cursor_offset(),
                false,
            ),
            cx,
        );
    }

    pub(crate) fn on_workspace_search_select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_select_to(0, cx);
    }

    pub(crate) fn on_workspace_search_select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_search_select_to(self.workspace.search_query.len(), cx);
    }

    fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.expanded.remove(id) {
            self.workspace.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    fn select_outline_node(&mut self, id: String, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Outline(id));
        cx.notify();
    }

    fn open_workspace_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_workspace_search_result(path, None, window, cx);
    }

    fn open_workspace_search_result(
        &mut self,
        path: PathBuf,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));

        if let Some(line) = line {
            self.pending_workspace_search_jump = Some(PendingWorkspaceSearchJump {
                line,
                query: self.workspace.search_query.clone(),
            });
        } else {
            self.pending_workspace_search_jump = None;
        }

        if line.is_some()
            && !self.document_dirty
            && self.file_path.as_deref() == Some(path.as_path())
        {
            self.apply_pending_workspace_search_jump(cx);
            window.activate_window();
            cx.notify();
            return;
        }

        self.request_dropped_markdown_replace(path, window, cx);
    }

    pub(super) fn apply_pending_workspace_search_jump(&mut self, cx: &mut Context<Self>) {
        let Some(jump) = self.pending_workspace_search_jump.take() else {
            return;
        };

        if self.jump_to_source_line_with_query(jump.line, &jump.query, cx) {
            self.pending_scroll_active_block_into_view = true;
            self.pending_scroll_recheck_after_layout = true;
        }
    }

    pub(super) fn workspace_panel_width(&self, viewport_width: f32) -> f32 {
        workspace_panel_width_for_viewport(viewport_width, self.workspace.panel_width)
    }

    pub(crate) fn start_workspace_resize_drag(
        &mut self,
        pointer_x: f32,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) {
        let current_width = self.workspace_panel_width(viewport_width);
        self.workspace_resize_drag = Some(super::WorkspaceResizeDragSession {
            start_pointer_x: pointer_x,
            start_width: current_width,
            viewport_width,
        });
        cx.notify();
    }

    pub(crate) fn update_workspace_resize_drag(&mut self, pointer_x: f32, cx: &mut Context<Self>) {
        let Some(drag) = self.workspace_resize_drag else {
            return;
        };

        let delta = pointer_x - drag.start_pointer_x;
        let next_width =
            clamp_workspace_panel_width(drag.start_width + delta, drag.viewport_width);
        if self.workspace.panel_width != Some(next_width) {
            self.workspace.panel_width = Some(next_width);
            cx.notify();
        }
    }

    pub(crate) fn end_workspace_resize_drag(&mut self, cx: &mut Context<Self>) {
        if self.workspace_resize_drag.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open {
            return None;
        }

        self.sync_workspace_models(cx);
        let panel_width = self.workspace_panel_width(viewport_width);
        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let tab_icon_color = |active: bool| {
            if active {
                c.text_default
            } else {
                c.dialog_muted
            }
        };

        let tab = |label: String, icon: &'static str, tab: WorkspaceTab, active: bool| {
            let tab_editor = editor.clone();
            let tab_id = match tab {
                WorkspaceTab::Files => "workspace-tab-files",
                WorkspaceTab::Outline => "workspace-tab-outline",
            };
            let icon_color = tab_icon_color(active);
            div()
                .id(tab_id)
                .h(px(24.0))
                .px(px(8.0))
                .flex()
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .rounded(px(4.0))
                .bg(if active && !self.workspace.search_open {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .text_size(px(t.text_size * 0.82))
                .text_color(icon_color)
                .child(
                    svg()
                        .path(icon)
                        .size(px(WORKSPACE_TAB_ICON_SIZE))
                        .flex_shrink_0()
                        .text_color(icon_color),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .child(label),
                )
                .on_click(move |_event, _window, cx| {
                    let _ = tab_editor.update(cx, |editor, cx| {
                        editor.set_workspace_tab(tab, cx);
                    });
                })
        };

        let search_active = self.workspace.search_open;
        let search_button_editor = editor.clone();
        let search_focus = self.workspace_search_focus.clone();
        let search_button = {
            let search_focus_for_click = search_focus.clone();
            div()
                .id("workspace-tab-search")
                .w(px(24.0))
                .h(px(24.0))
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .bg(if search_active {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    svg()
                        .path(SEARCH_ICON)
                        .size(px(WORKSPACE_TAB_ICON_SIZE))
                        .text_color(if search_active {
                            c.text_default
                        } else {
                            c.dialog_muted
                        }),
                )
                .on_click(move |_event, window, cx| {
                    let _ = search_button_editor.update(cx, |editor, cx| {
                        editor.toggle_workspace_search(window, cx);
                    });
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.focus(&search_focus_for_click);
                })
        };

        let search_bar = if search_active {
            let placeholder: SharedString = strings.workspace_search_placeholder.clone().into();
            let search_bar_editor = editor.clone();
            Some(
                div()
                    .id("workspace-search-input-row")
                    .px(px(8.0))
                    .pt(px(8.0))
                    .pb(px(6.0))
                    .child(
                        div()
                            .id("workspace-search-input")
                            .h(px(26.0))
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .rounded(px(4.0))
                            .border(px(1.0))
                            .border_color(c.dialog_border.opacity(0.75))
                            .bg(c.editor_background)
                            .child(
                                svg()
                                    .path(SEARCH_ICON)
                                    .size(px(WORKSPACE_TAB_ICON_SIZE))
                                    .flex_shrink_0()
                                    .text_color(c.dialog_muted),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .h_full()
                                    .overflow_hidden()
                                    .child(WorkspaceSearchInputElement::new(
                                        cx.entity(),
                                        placeholder,
                                    ))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::on_workspace_search_mouse_down),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(Self::on_workspace_search_mouse_up),
                                    )
                                    .on_mouse_up_out(
                                        MouseButton::Left,
                                        cx.listener(Self::on_workspace_search_mouse_up),
                                    )
                                    .on_mouse_move(cx.listener(Self::on_workspace_search_mouse_move)),
                            )
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                let _ = search_bar_editor.update(cx, |editor, cx| {
                                    editor.focus_workspace_search(window, cx);
                                });
                            }),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let body = if self.workspace.search_open {
            self.render_workspace_search_results(theme, strings, &editor)
        } else {
            match self.workspace.active_tab {
                WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
                WorkspaceTab::Outline => {
                    self.render_workspace_outline_tree(theme, strings, &editor)
                }
            }
        };

        let resize_editor = editor.clone();
        let drag_start_editor = resize_editor.clone();
        let resize_viewport_width = viewport_width;

        Some(
            div()
                .id("workspace-panel-shell")
                .relative()
                .h_full()
                .w(px(panel_width))
                .flex_shrink_0()
                .child(
                    div()
                        .id("workspace-panel")
                        .size_full()
                        .flex()
                        .flex_col()
                        .track_focus(&search_focus)
                        .key_context("BlockEditor")
                        .on_key_down(cx.listener(Self::on_workspace_search_key_down))
                        .on_action(cx.listener(Self::on_workspace_search_delete_back))
                        .on_action(cx.listener(Self::on_workspace_search_delete_forward))
                        .on_action(cx.listener(Self::on_workspace_search_paste))
                        .on_action(cx.listener(Self::on_workspace_search_copy))
                        .on_action(cx.listener(Self::on_workspace_search_cut))
                        .on_action(cx.listener(Self::on_workspace_search_select_all))
                        .on_action(cx.listener(Self::on_workspace_search_move_left))
                        .on_action(cx.listener(Self::on_workspace_search_move_right))
                        .on_action(cx.listener(Self::on_workspace_search_home))
                        .on_action(cx.listener(Self::on_workspace_search_end))
                        .on_action(cx.listener(Self::on_workspace_search_select_left))
                        .on_action(cx.listener(Self::on_workspace_search_select_right))
                        .on_action(cx.listener(Self::on_workspace_search_select_home))
                        .on_action(cx.listener(Self::on_workspace_search_select_end))
                        .bg(c.dialog_surface)
                        .border_r(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .child(
                            div()
                                .px(px(8.0))
                                .pt(px(8.0))
                                .pb(px(4.0))
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .border_b(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .child(tab(
                                    strings.workspace_tab_files.clone(),
                                    FILES_TAB_ICON,
                                    WorkspaceTab::Files,
                                    self.workspace.active_tab == WorkspaceTab::Files,
                                ))
                                .child(tab(
                                    strings.workspace_tab_outline.clone(),
                                    OUTLINE_TAB_ICON,
                                    WorkspaceTab::Outline,
                                    self.workspace.active_tab == WorkspaceTab::Outline,
                                ))
                                .child(search_button),
                        )
                        .children(search_bar)
                        .child(
                            div()
                                .id("workspace-panel-scroll")
                                .flex_1()
                                .min_h(px(0.0))
                                .overflow_y_scroll()
                                .px(px(4.0))
                                .py(px(4.0))
                                .child(body),
                        ),
                )
                .child(
                    div()
                        .id("workspace-resize-handle")
                        .absolute()
                        .top_0()
                        .right(px(0.0))
                        .w(px(WORKSPACE_RESIZE_HANDLE_WIDTH))
                        .h_full()
                        .occlude()
                        .cursor_col_resize()
                        .hover(|this| this.bg(c.dialog_border.opacity(0.55)))
                        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                            cx.stop_propagation();
                            let _ = drag_start_editor.update(cx, |editor, cx| {
                                editor.start_workspace_resize_drag(
                                    f32::from(event.position.x),
                                    resize_viewport_width,
                                    cx,
                                );
                            });
                        })
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |_bounds, _, window, _| {
                                    window.on_mouse_event({
                                        let editor = resize_editor.clone();
                                        move |event: &MouseMoveEvent, phase, _window, cx| {
                                            if !phase.bubble() || !event.dragging() {
                                                return;
                                            }
                                            let _ = editor.update(cx, |editor, cx| {
                                                editor.update_workspace_resize_drag(
                                                    f32::from(event.position.x),
                                                    cx,
                                                );
                                            });
                                        }
                                    });

                                    window.on_mouse_event({
                                        let editor = resize_editor.clone();
                                        move |_event: &MouseUpEvent, phase, _window, cx| {
                                            if !phase.bubble() {
                                                return;
                                            }
                                            let _ = editor.update(cx, |editor, cx| {
                                                editor.end_workspace_resize_drag(cx);
                                            });
                                        }
                                    });
                                },
                            )
                            .size_full(),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_workspace_files_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        if let Some(error) = self.workspace.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        let Some(root) = self.workspace.file_tree.as_ref() else {
            return self.render_workspace_empty_state("", &strings.workspace_empty_files, theme);
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(std::slice::from_ref(root), 0, theme, editor))
            .into_any_element()
    }

    fn render_workspace_outline_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.outline_tree.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_outline, theme);
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(&self.workspace.outline_tree, 0, theme, editor))
            .into_any_element()
    }

    fn render_workspace_search_results(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.effective_workspace_root().is_none() {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_search_no_root,
                theme,
            );
        }

        if self.workspace.search_query.trim().is_empty() {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_search_placeholder,
                theme,
            );
        }

        if self.workspace.search_results.is_empty() {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_search_no_results,
                theme,
            );
        }

        let root = self.effective_workspace_root();
        let c = &theme.colors;
        let t = &theme.typography;
        let mut rows = Vec::new();
        for (index, result) in self.workspace.search_results.iter().enumerate() {
            let label = workspace_search_result_label(root.as_deref(), result);
            let detail = workspace_search_result_detail(result);
            let path = result.path.clone();
            let line = result.line;
            let open_editor = editor.clone();
            rows.push(
                div()
                    .id(("workspace-search-result", index))
                    .w_full()
                    .px(px(6.0))
                    .py(px(5.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .rounded(px(4.0))
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .on_click(move |_event, window, cx| {
                        let open_path = path.clone();
                        let _ = open_editor.update(cx, |editor, cx| {
                            editor.open_workspace_search_result(open_path, line, window, cx);
                        });
                    })
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(t.text_size * 0.82))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c.text_default)
                            .child(label),
                    )
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .text_size(px(t.text_size * 0.78))
                            .text_color(c.dialog_muted)
                            .child(detail),
                    )
                    .into_any_element(),
            );
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(rows)
            .into_any_element()
    }

    fn render_workspace_empty_state(
        &self,
        title: &str,
        message: &str,
        theme: &Theme,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let title = (!title.is_empty()).then(|| {
            div()
                .text_size(px(t.text_size))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.text_default)
                .child(title.to_string())
        });

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(22.0))
            .text_align(TextAlign::Center)
            .children(title)
            .child(
                div()
                    .text_size(px(t.text_size * 0.9))
                    .line_height(px(t.text_size * t.text_line_height))
                    .text_color(c.dialog_muted)
                    .child(message.to_string()),
            )
            .into_any_element()
    }

    fn render_workspace_nodes(
        &self,
        nodes: &[WorkspaceTreeNode],
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> Vec<AnyElement> {
        let mut elements = Vec::new();
        for node in nodes {
            elements.push(self.render_workspace_node(node, depth, theme, editor));
            if !node.children.is_empty() && self.workspace.expanded.contains(&node.id) {
                elements.extend(self.render_workspace_nodes(
                    &node.children,
                    depth + 1,
                    theme,
                    editor,
                ));
            }
        }
        elements
    }

    fn render_workspace_node(
        &self,
        node: &WorkspaceTreeNode,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let is_expanded = self.workspace.expanded.contains(&node.id);
        let has_children = !node.children.is_empty();
        let selected = match (&self.workspace.selected, &node.kind) {
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::MarkdownFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::Outline(selected)), _) => selected == &node.id,
            _ => false,
        };
        let node_id = node.id.clone();
        let click_editor = editor.clone();
        let click_kind = node.kind.clone();
        let arrow_node_id = node.id.clone();
        let arrow_editor = editor.clone();
        let chevron_color = c.dialog_muted;

        let mut arrow_el = div()
            .w(px(WORKSPACE_CHEVRON_SIZE))
            .h(px(WORKSPACE_CHEVRON_SIZE))
            .flex_shrink_0();
        if has_children {
            let chevron_icon = if is_expanded {
                CHEVRON_DOWN_ICON
            } else {
                CHEVRON_RIGHT_ICON
            };
            arrow_el = arrow_el
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .child(
                    svg()
                        .path(chevron_icon)
                        .size(px(WORKSPACE_CHEVRON_SIZE))
                        .text_color(chevron_color),
                )
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    let _ = arrow_editor.update(cx, |editor, cx| {
                        editor.toggle_workspace_node(&arrow_node_id, cx);
                    });
                    cx.stop_propagation();
                });
        }

        let icon = match &node.kind {
            WorkspaceTreeKind::Directory(_) => Some((FOLDER_ICON, Hsla::from(rgba(0xf59e0bff)))),
            WorkspaceTreeKind::MarkdownFile(_) => {
                Some((MARKDOWN_ICON, Hsla::from(rgba(0x2563ebff))))
            }
            WorkspaceTreeKind::Heading { .. } => None,
        };

        let label_color = if selected {
            c.text_default
        } else {
            c.dialog_muted
        };

        div()
            .id(("workspace-node", stable_node_hash(&node.id)))
            .h(px(WORKSPACE_NODE_HEIGHT))
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(4.0 + depth as f32 * WORKSPACE_NODE_INDENT))
            .pr(px(4.0))
            .rounded(px(4.0))
            .bg(if selected {
                c.selection
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .child(arrow_el)
            .children(icon.map(|(path, color)| {
                svg()
                    .path(path)
                    .size(px(WORKSPACE_ICON_SIZE))
                    .flex_shrink_0()
                    .text_color(color)
                    .into_any_element()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .text_size(px(t.text_size * 0.84))
                    .line_height(px(t.text_size * 1.15))
                    .text_color(label_color)
                    .child(node.label.clone()),
            )
            .on_click(move |_event, window, cx| {
                let node_id = node_id.clone();
                let click_kind = click_kind.clone();
                let _ = click_editor.update(cx, |editor, cx| match click_kind {
                    WorkspaceTreeKind::Directory(_) => editor.toggle_workspace_node(&node_id, cx),
                    WorkspaceTreeKind::MarkdownFile(path) => {
                        editor.open_workspace_file(path, window, cx);
                    }
                    WorkspaceTreeKind::Heading { .. } => editor.select_outline_node(node_id, cx),
                });
            })
            .into_any_element()
    }
}

fn collect_markdown_files(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files);
        } else if is_markdown_file(&path) {
            files.push(path);
        }
    }
}

fn search_markdown_files(root: &Path, query: &str) -> Vec<WorkspaceSearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut files = Vec::new();
    collect_markdown_files(root, &mut files);
    files.sort();

    let mut results = Vec::new();
    for path in files {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.to_lowercase().contains(&query_lower) {
            results.push(WorkspaceSearchResult {
                path: path.clone(),
                line: None,
                preview: String::new(),
            });
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                results.push(WorkspaceSearchResult {
                    path: path.clone(),
                    line: Some(index + 1),
                    preview: line.trim().to_string(),
                });
            }
        }
    }

    results
}

fn workspace_search_result_label(root: Option<&Path>, result: &WorkspaceSearchResult) -> String {
    let path_label = root
        .and_then(|root| result.path.strip_prefix(root).ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| result.path.to_string_lossy().into_owned());
    match result.line {
        Some(line) => format!("{path_label}:{line}"),
        None => path_label,
    }
}

fn workspace_search_result_detail(result: &WorkspaceSearchResult) -> String {
    if let Some(line) = result.line {
        if result.preview.is_empty() {
            return format!("Line {line}");
        }
        return result.preview.clone();
    }
    result
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md"))
}

fn scan_workspace_dir(path: &Path) -> Result<WorkspaceTreeNode> {
    let mut children = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            children.push(scan_workspace_dir(&entry_path)?);
        } else if file_type.is_file() && is_markdown_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::MarkdownFile(entry_path),
                children: Vec::new(),
            });
        }
    }

    children.sort_by(|left, right| {
        let left_dir = matches!(left.kind, WorkspaceTreeKind::Directory(_));
        let right_dir = matches!(right.kind, WorkspaceTreeKind::Directory(_));
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
    })
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn file_node_id(path: &Path) -> String {
    format!("file:{}", path.to_string_lossy())
}

fn stable_node_hash(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn default_workspace_panel_width(viewport_width: f32) -> f32 {
    let target = viewport_width * WORKSPACE_PANEL_TARGET_RATIO;
    target.clamp(WORKSPACE_PANEL_MIN_WIDTH, WORKSPACE_PANEL_MAX_WIDTH)
}

pub(super) fn clamp_workspace_panel_width(width: f32, viewport_width: f32) -> f32 {
    let max_width = (viewport_width * WORKSPACE_PANEL_MAX_VIEWPORT_RATIO)
        .max(WORKSPACE_PANEL_MIN_WIDTH)
        .max(WORKSPACE_PANEL_MAX_WIDTH);
    width.clamp(WORKSPACE_PANEL_MIN_WIDTH, max_width)
}

pub(super) fn workspace_panel_width_for_viewport(
    viewport_width: f32,
    user_width: Option<f32>,
) -> f32 {
    match user_width {
        Some(width) => clamp_workspace_panel_width(width, viewport_width),
        None => default_workspace_panel_width(viewport_width),
    }
}

fn prune_outline_state(workspace: &mut WorkspaceState, outline: &[WorkspaceTreeNode]) {
    let mut current_ids = HashSet::new();
    collect_node_ids(outline, &mut current_ids);
    workspace
        .expanded
        .retain(|id| !is_outline_node_id(id) || current_ids.contains(id));

    if matches!(
        &workspace.selected,
        Some(WorkspaceSelection::Outline(id)) if !current_ids.contains(id)
    ) {
        workspace.selected = None;
    }
}

fn collect_node_ids(nodes: &[WorkspaceTreeNode], ids: &mut HashSet<String>) {
    for node in nodes {
        ids.insert(node.id.clone());
        collect_node_ids(&node.children, ids);
    }
}

fn is_outline_node_id(id: &str) -> bool {
    id.starts_with("outline:")
}

fn build_outline_tree(markdown: &str) -> Vec<WorkspaceTreeNode> {
    let mut roots = Vec::new();
    let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (line_index, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, len)) = fence {
            if is_closing_fence(trimmed, marker, len) {
                fence = None;
            }
            continue;
        }

        if let Some(next_fence) = opening_fence(trimmed) {
            fence = Some(next_fence);
            continue;
        }

        let Some((level, title)) = BlockKind::parse_atx_heading_line(line) else {
            continue;
        };

        while stack
            .last()
            .is_some_and(|(parent_level, _)| *parent_level >= level)
        {
            stack.pop();
        }

        let node = WorkspaceTreeNode {
            id: format!("outline:{line_index}"),
            label: title,
            kind: WorkspaceTreeKind::Heading {
                line: line_index,
                level,
            },
            children: Vec::new(),
        };

        let siblings = if let Some((_, parent_path)) = stack.last() {
            children_at_path_mut(&mut roots, parent_path)
        } else {
            &mut roots
        };
        siblings.push(node);

        let mut node_path = stack
            .last()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        node_path.push(siblings.len() - 1);
        stack.push((level, node_path));
    }

    roots
}

fn children_at_path_mut<'a>(
    nodes: &'a mut Vec<WorkspaceTreeNode>,
    path: &[usize],
) -> &'a mut Vec<WorkspaceTreeNode> {
    let mut current = nodes;
    for &index in path {
        current = &mut current[index].children;
    }
    current
}

fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (len >= 3).then_some((marker, len))
}

fn is_closing_fence(trimmed: &str, marker: char, len: usize) -> bool {
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= len && trimmed[count..].trim().is_empty()
}

fn workspace_search_primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    (modifiers.platform || modifiers.control)
        && !modifiers.alt
        && !modifiers.function
}

fn workspace_search_grapheme_boundary(text: &str, offset: usize, backward: bool) -> usize {
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

fn workspace_search_offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;

    for ch in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }

    utf8_offset
}

fn workspace_search_offset_to_utf16(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;

    for ch in text.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }

    utf16_offset
}

fn workspace_search_range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    workspace_search_offset_to_utf16(text, range.start)
        ..workspace_search_offset_to_utf16(text, range.end)
}

fn workspace_search_range_from_utf16(text: &str, range_utf16: &Range<usize>) -> Range<usize> {
    workspace_search_offset_from_utf16(text, range_utf16.start)
        ..workspace_search_offset_from_utf16(text, range_utf16.end)
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if !self.workspace_search_input_active(window) {
            return None;
        }

        let text = self.workspace.search_query.clone();
        let range = workspace_search_range_from_utf16(&text, &range_utf16);
        actual_range.replace(workspace_search_range_to_utf16(&text, &range));
        Some(text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if !self.workspace_search_input_active(window) {
            return None;
        }

        let text = &self.workspace.search_query;
        Some(UTF16Selection {
            range: workspace_search_range_to_utf16(text, &self.workspace.search_selected_range),
            reversed: false,
        })
    }

    fn marked_text_range(&self, window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        if !self.workspace_search_input_active(window) {
            return None;
        }

        self.workspace
            .search_marked_range
            .as_ref()
            .map(|range| workspace_search_range_to_utf16(&self.workspace.search_query, range))
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.workspace_search_input_active(window) {
            self.workspace.search_marked_range = None;
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range_utf16| workspace_search_range_from_utf16(&self.workspace.search_query, range_utf16))
            .or_else(|| self.workspace.search_marked_range.clone())
            .unwrap_or_else(|| self.workspace.search_selected_range.clone());

        self.replace_workspace_search_text(range, new_text, false, None, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range_utf16| workspace_search_range_from_utf16(&self.workspace.search_query, range_utf16))
            .or_else(|| self.workspace.search_marked_range.clone())
            .unwrap_or_else(|| self.workspace.search_selected_range.clone());
        let selected = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| workspace_search_range_from_utf16(new_text, range_utf16))
            .map(|relative| relative.start + range.start..relative.end + range.start);

        self.replace_workspace_search_text(range, new_text, true, selected, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if !self.workspace_search_input_active(window) {
            return None;
        }

        let line = self.workspace.search_last_layout.as_ref()?;
        let range = workspace_search_range_from_utf16(&self.workspace.search_query, &range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if !self.workspace_search_input_active(window) {
            return None;
        }

        let bounds = self.workspace.search_last_bounds?;
        let line = self.workspace.search_last_layout.as_ref()?;
        let local = bounds.localize(&point)?;
        let utf8_index = line.index_for_x(local.x - bounds.left())?;
        Some(workspace_search_offset_to_utf16(
            &self.workspace.search_query,
            utf8_index.min(self.workspace.search_query.len()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WorkspaceSelection, WorkspaceState, WorkspaceTreeKind, build_outline_tree,
        prune_outline_state, scan_workspace_dir, search_markdown_files,
        workspace_panel_width_for_viewport,
    };
    use std::fs;

    #[test]
    fn workspace_scan_keeps_dirs_and_md_files_only() {
        let root =
            std::env::temp_dir().join(format!("velotype-workspace-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create dirs");
        fs::write(root.join("a.md"), "a").expect("write md");
        fs::write(root.join("a.txt"), "ignored").expect("write txt");
        fs::write(root.join("nested").join("b.md"), "b").expect("write nested md");

        let tree = scan_workspace_dir(&root).expect("scan tree");
        let labels = tree
            .children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["nested", "a.md"]);
        assert!(matches!(
            tree.children[0].kind,
            WorkspaceTreeKind::Directory(_)
        ));
        assert!(matches!(
            tree.children[1].kind,
            WorkspaceTreeKind::MarkdownFile(_)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn outline_tree_skips_headings_inside_fenced_code() {
        let outline = build_outline_tree(
            "# Root\n\n```md\n# ignored\n```\n\n## Child\n\n### Grandchild\n\n# Next",
        );

        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].label, "Root");
        assert_eq!(outline[0].children[0].label, "Child");
        assert_eq!(outline[0].children[0].children[0].label, "Grandchild");
        assert_eq!(outline[1].label, "Next");
    }

    #[test]
    fn outline_expansion_state_is_not_auto_populated_and_prunes_stale_ids() {
        let outline = build_outline_tree("# Root\n\n## Child\n\n# Next");
        let mut fresh = WorkspaceState::default();
        prune_outline_state(&mut fresh, &outline);
        assert!(fresh.expanded.is_empty());

        let mut existing = WorkspaceState::default();
        existing.expanded.insert("outline:0".to_string());
        existing.expanded.insert("outline:999".to_string());
        existing
            .expanded
            .insert("workspace-dir:C:/docs".to_string());
        existing.selected = Some(WorkspaceSelection::Outline("outline:999".to_string()));

        prune_outline_state(&mut existing, &outline);

        assert!(existing.expanded.contains("outline:0"));
        assert!(existing.expanded.contains("workspace-dir:C:/docs"));
        assert!(!existing.expanded.contains("outline:999"));
        assert_eq!(existing.selected, None);
    }

    #[test]
    fn workspace_search_finds_filename_and_content_matches_recursively() {
        let root =
            std::env::temp_dir().join(format!("velotype-workspace-search-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create dirs");
        fs::write(root.join("notes.md"), "# Alpha\nbeta keyword here\n").expect("write root md");
        fs::write(root.join("nested").join("deep.md"), "gamma DELTA\n").expect("write nested md");

        let results = search_markdown_files(&root, "keyword");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, Some(2));
        assert!(results[0].preview.contains("keyword"));

        let filename_results = search_markdown_files(&root, "deep");
        assert_eq!(filename_results.len(), 1);
        assert!(filename_results[0].line.is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_panel_width_uses_ratio_with_bounds() {
        assert_eq!(
            workspace_panel_width_for_viewport(1000.0, None),
            180.0
        );
        assert_eq!(
            workspace_panel_width_for_viewport(2000.0, None),
            300.0
        );
        assert_eq!(
            workspace_panel_width_for_viewport(4000.0, None),
            360.0
        );
        assert_eq!(
            workspace_panel_width_for_viewport(1000.0, Some(280.0)),
            280.0
        );
    }
}
