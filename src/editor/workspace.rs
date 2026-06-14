//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::{BlockKind, Editor, PendingWorkspaceSearchJump};
use super::document_search::{
    document_search_offset_to_utf16, document_search_range_from_utf16,
    document_search_range_to_utf16,
};
use super::markdown_files::{collect_markdown_files, is_markdown_file};
use super::single_line_input::{
    SingleLineInputTarget, arrow_key_from_event, handle_mouse_down, handle_mouse_move,
    handle_mouse_up, index_for_mouse_position, move_caret_to, prepare_context_menu_selection,
    primary_shortcut_modifiers, select_caret_to, text_grapheme_boundary, SingleLineArrowKey,
};
use super::tag_index::{
    build_workspace_tag_index, refresh_tag_index_for_file, TagOccurrence, WorkspaceTagIndex,
};
use crate::components::markdown::inline::normalize_tag_name;
use super::single_line_input_element::SingleLineInputElement;
use crate::components::{
    Copy, Cut, Delete, DeleteBack, End, FenceInfo, Home, MoveLeft, MoveRight, Paste, SelectAll,
    SelectEnd, SelectHome, SelectLeft, SelectRight, is_closing_fence, parse_opening_fence,
};
use crate::i18n::I18nStrings;
use crate::input::single_line_field::SingleLineFieldState;
use crate::input::text_norm::flatten_paste_to_single_line;
use crate::theme::Theme;

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const CHEVRON_RIGHT_ICON: &str = "icon/workspace/chevron-right.svg";
const CHEVRON_DOWN_ICON: &str = "icon/workspace/chevron-down.svg";
const FILES_TAB_ICON: &str = "icon/workspace/files.svg";
const OUTLINE_TAB_ICON: &str = "icon/workspace/list-tree.svg";
const TAGS_TAB_ICON: &str = "icon/workspace/tags.svg";
const GRAPH_TAB_ICON: &str = "icon/workspace/graph.svg";
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
    match_start_byte: Option<usize>,
    raw_file_len: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceTab {
    #[default]
    Files,
    Outline,
    Tags,
    Graph,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WorkspaceTagSort {
    #[default]
    ByCountDesc,
    ByNameAsc,
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
    search_input: SingleLineFieldState,
    search_results: Vec<WorkspaceSearchResult>,
    pub(super) tag_index: Option<WorkspaceTagIndex>,
    pub(super) tag_index_busy: bool,
    pub(super) tag_index_root: Option<PathBuf>,
    /// Cached `(path, serialized markdown)` last merged into `tag_index`.
    pub(super) tag_index_live_source: Option<(PathBuf, String)>,
    pub(super) link_index: Option<super::link_index::WorkspaceLinkIndex>,
    pub(super) link_index_busy: bool,
    pub(super) link_index_root: Option<PathBuf>,
    pub(super) graph_revision: Option<u64>,
    pub(super) graph_busy: bool,
    selected_tag: Option<String>,
    tag_sort: WorkspaceTagSort,
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
            search_input: SingleLineFieldState::new(),
            search_results: Vec::new(),
            tag_index: None,
            tag_index_busy: false,
            tag_index_root: None,
            tag_index_live_source: None,
            link_index: None,
            link_index_busy: false,
            link_index_root: None,
            graph_revision: None,
            graph_busy: false,
            selected_tag: None,
            tag_sort: WorkspaceTagSort::default(),
        }
    }
}

impl Editor {
    pub(super) fn workspace_root_for_ai(&self) -> Option<PathBuf> {
        self.workspace.state.root.clone()
    }

    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.state.is_open {
            self.workspace.state.is_open = false;
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.state.is_open = true;
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

    pub(crate) fn on_open_workspace_search_action(
        &mut self,
        _: &crate::components::OpenWorkspaceSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_workspace_search(window, cx);
    }

    pub(super) fn open_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);

        let was_open = self.workspace.state.is_open;
        self.workspace.state.is_open = true;
        if !was_open {
            self.sync_workspace_models(cx);
        }

        self.workspace.state.active_tab = WorkspaceTab::Files;
        self.workspace.state.search_open = true;
        self.sync_workspace_search_selection();
        self.run_workspace_search();
        window.focus(&self.workspace.search_focus);
        window.activate_window();
        cx.notify();
    }

    pub(super) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if self.workspace.state.folder_root.is_none() {
            self.workspace.state.root = None;
            self.workspace.state.file_tree = None;
            self.workspace.state.file_error = None;
        }
        self.workspace.state.outline_source = None;
        self.workspace.state.tag_index = None;
        self.workspace.state.tag_index_root = None;
        self.workspace.state.tag_index_busy = false;
        self.workspace.state.tag_index_live_source = None;
        self.clear_workspace_graph_state();
        if self.workspace.state.is_open {
            self.sync_workspace_models(cx);
        }
    }

    fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree();
        self.sync_workspace_outline(cx);
        self.sync_workspace_tag_index(cx);
        self.sync_workspace_tag_index_for_active_file(cx);
        self.sync_workspace_link_index(cx);
        self.sync_knowledge_graph(cx);
    }

    fn workspace_root_for_current_file(&self) -> Option<PathBuf> {
        self.file_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    pub(super) fn effective_workspace_root(&self) -> Option<PathBuf> {
        self.workspace
            .state
            .folder_root
            .clone()
            .or_else(|| self.workspace_root_for_current_file())
    }

    fn sync_workspace_file_tree(&mut self) {
        let next_root = self.effective_workspace_root();
        if self.workspace.state.root == next_root && self.workspace.state.file_tree.is_some() {
            self.workspace.state.selected = self
                .file_path
                .as_ref()
                .map(|path| WorkspaceSelection::File(path.clone()));
            return;
        }

        self.workspace.state.root = next_root.clone();
        self.workspace.state.file_tree = None;
        self.workspace.state.file_error = None;

        let Some(root) = next_root else {
            self.workspace.state.selected = None;
            self.workspace.state.selected_tag = None;
            self.workspace.state.tag_index = None;
            self.workspace.state.tag_index_root = None;
            self.workspace.state.tag_index_busy = false;
            self.workspace.state.tag_index_live_source = None;
            self.clear_workspace_graph_state();
            return;
        };

        // Validate the root path
        if root.as_os_str().is_empty() {
            self.workspace.state.file_error = Some("Invalid workspace path: empty path".to_string());
            self.workspace.state.selected = None;
            return;
        }

        match scan_workspace_dir(&root) {
            Ok(tree) => {
                self.workspace.state.expanded.insert(tree.id.clone());
                self.workspace.state.file_tree = Some(tree);
                self.workspace.state.selected = self
                    .file_path
                    .as_ref()
                    .map(|path| WorkspaceSelection::File(path.clone()));
            }
            Err(err) => {
                self.workspace.state.file_error = Some(err.to_string());
            }
        }
    }

    fn sync_workspace_outline(&mut self, cx: &mut Context<Self>) {
        let source = self.serialized_document_text(cx);
        if self.workspace.state.outline_source.as_deref() == Some(source.as_str()) {
            return;
        }

        let outline = build_outline_tree(&source);
        prune_outline_state(&mut self.workspace.state, &outline);
        self.workspace.state.outline_tree = outline;
        self.workspace.state.outline_source = Some(source);
    }

    fn sync_workspace_tag_index(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.effective_workspace_root() else {
            self.workspace.state.tag_index = None;
            self.workspace.state.tag_index_root = None;
            self.workspace.state.tag_index_busy = false;
            self.workspace.state.tag_index_live_source = None;
            self.clear_workspace_graph_state();
            return;
        };

        if self.workspace.state.tag_index_root.as_deref() == Some(root.as_path())
            && self.workspace.state.tag_index.is_some()
        {
            return;
        }

        self.workspace.state.tag_index = None;
        self.workspace.state.tag_index_root = Some(root.clone());
        self.workspace.state.tag_index_busy = true;
        self.workspace.state.tag_index_live_source = None;

        let editor = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let index = build_workspace_tag_index(&root);
            let _ = editor.update(cx, |editor, cx| {
                if editor.effective_workspace_root().as_deref() != Some(root.as_path()) {
                    return;
                }
                editor.workspace.state.tag_index = Some(index);
                editor.workspace.state.tag_index_busy = false;
                editor.workspace.state.tag_index_live_source = None;
                editor.sync_knowledge_graph(cx);
            });
        })
        .detach();
    }

    /// Incrementally refresh the open file's tag entries in the workspace index.
    fn sync_workspace_tag_index_for_active_file(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.effective_workspace_root() else {
            self.workspace.state.tag_index_live_source = None;
            return;
        };
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if !path.starts_with(&root) {
            return;
        }
        if self.workspace.state.tag_index.is_none() || self.workspace.state.tag_index_busy {
            return;
        }
        if self.workspace.state.tag_index_root.as_deref() != Some(root.as_path()) {
            return;
        }

        let source = self.serialized_document_text(cx);
        if self.workspace.state.tag_index_live_source.as_ref()
            == Some(&(path.clone(), source.clone()))
        {
            return;
        }

        if let Some(index) = self.workspace.state.tag_index.as_mut() {
            refresh_tag_index_for_file(index, &path, &source);
            self.workspace.state.tag_index_live_source = Some((path, source));
            self.sync_knowledge_graph(cx);
        }
    }

    pub(super) fn refresh_workspace_tag_index_for_saved_file(
        &mut self,
        path: &Path,
        content: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.effective_workspace_root() else {
            return;
        };
        if !path.starts_with(&root) {
            return;
        }

        if self.workspace.state.tag_index.is_none() {
            self.sync_workspace_tag_index(cx);
            return;
        }

        if self.workspace.state.tag_index_root.as_deref() != Some(root.as_path()) {
            self.sync_workspace_tag_index(cx);
            return;
        }

        if let Some(index) = self.workspace.state.tag_index.as_mut() {
            refresh_tag_index_for_file(index, path, content);
            self.workspace.state.tag_index_live_source =
                Some((path.to_path_buf(), content.to_string()));
            self.refresh_workspace_link_index_for_saved_file(path, content, cx);
            self.sync_knowledge_graph(cx);
        }
    }

    pub(super) fn workspace_files_panel_active(&self) -> bool {
        matches!(self.workspace.state.active_tab, WorkspaceTab::Files) && !self.workspace.state.search_open
    }

    pub(super) fn workspace_tree_root_path(&self) -> Option<PathBuf> {
        self.workspace.state.file_tree.as_ref().and_then(|root| {
            if let WorkspaceTreeKind::Directory(path) = &root.kind {
                Some(path.clone())
            } else {
                None
            }
        })
    }

    pub(super) fn workspace_is_tree_root(&self, path: &Path) -> bool {
        self.workspace_tree_root_path().as_deref() == Some(path)
    }

    pub(super) fn workspace_expand_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let mut current = Some(path);
        while let Some(path) = current {
            self.workspace.state.expanded.insert(workspace_file_node_id(path));
            current = path.parent();
        }
        cx.notify();
    }

    pub(super) fn workspace_select_file_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.workspace.state.selected = Some(WorkspaceSelection::File(path));
        cx.notify();
    }

    pub(super) fn workspace_refresh_file_tree(&mut self, cx: &mut Context<Self>) {
        self.workspace.state.root = None;
        self.sync_workspace_file_tree();
        cx.notify();
    }

    pub(super) fn workspace_clear_file_selection_if(&mut self, path: &Path, cx: &mut Context<Self>) {
        if matches!(
            &self.workspace.state.selected,
            Some(WorkspaceSelection::File(selected)) if selected == path
        ) {
            self.workspace.state.selected = None;
            cx.notify();
        }
    }

    pub(crate) fn open_workspace_folder(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !path.is_dir() {
            return;
        }

        if let Err(err) = crate::config::record_last_workspace_folder(&path) {
            eprintln!("failed to record last workspace folder: {err}");
        }

        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);
        self.workspace.state.folder_root = Some(path);
        self.workspace.state.root = None;
        self.workspace.state.file_tree = None;
        self.workspace.state.file_error = None;
        self.workspace.state.is_open = true;
        self.sync_workspace_models(cx);
        window.activate_window();
        cx.notify();
    }

    fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        if self.workspace.state.active_tab == tab && !self.workspace.state.search_open {
            return;
        }
        self.workspace.state.active_tab = tab;
        self.workspace.state.search_open = false;
        self.sync_workspace_models(cx);
        if matches!(tab, WorkspaceTab::Graph) {
            self.start_knowledge_graph_animation(cx);
        }
        cx.notify();
    }

    fn sync_workspace_search_selection(&mut self) {
        self.workspace.state.search_input.sync_caret_to_end();
    }

    pub(super) fn workspace_search_input_active(&self, window: &Window) -> bool {
        self.workspace.state.search_open && self.workspace.search_focus.is_focused(window)
    }

    pub(super) fn workspace_search_is_open(&self) -> bool {
        self.workspace.state.search_open
    }

    pub(super) fn workspace_search_has_selection(&self) -> bool {
        !self.workspace.state.search_input.selected_range.is_empty()
    }

    pub(super) fn workspace_search_query_is_empty(&self) -> bool {
        self.workspace.state.search_input.query.is_empty()
    }

    pub(super) fn workspace_search_display_text(&self, placeholder: &SharedString) -> SharedString {
        if self.workspace.state.search_input.query.is_empty() {
            placeholder.clone()
        } else {
            self.workspace.state.search_input.query.clone().into()
        }
    }

    pub(super) fn workspace_search_marked_range(&self) -> Option<Range<usize>> {
        self.workspace.state.search_input.marked_range.clone()
    }

    pub(super) fn workspace_search_selected_range(&self) -> Range<usize> {
        self.workspace.state.search_input.selected_range.clone()
    }

    pub(super) fn workspace_search_index_for_mouse_position(
        &self,
        position: Point<Pixels>,
    ) -> usize {
        index_for_mouse_position(
            self.workspace.state.search_input.query.len(),
            self.workspace.state.search_input.last_bounds.as_ref(),
            self.workspace.state.search_input.last_layout.as_ref(),
            position,
        )
    }

    pub(super) fn workspace_search_cursor_offset(&self) -> usize {
        self.workspace.state.search_input.cursor_offset()
    }

    pub(super) fn set_workspace_search_layout(
        &mut self,
        layout: ShapedLine,
        bounds: Bounds<Pixels>,
    ) {
        self.workspace.state.search_input.set_layout(layout, bounds);
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
        let query = &mut self.workspace.state.search_input.query;
        let start = range.start.min(query.len());
        let end = range.end.min(query.len());
        query.replace_range(start..end, &sanitized);

        if mark_composing && !sanitized.is_empty() {
            self.workspace.state.search_input.marked_range = Some(start..start + sanitized.len());
        } else {
            self.workspace.state.search_input.marked_range = None;
        }

        self.workspace.state.search_input.selected_range = new_selected.unwrap_or_else(|| {
            let cursor = start + sanitized.len();
            cursor..cursor
        });
        self.workspace.state.search_input.selection_reversed = false;
        self.run_workspace_search();
        cx.notify();
    }

    fn toggle_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.state.search_open = !self.workspace.state.search_open;
        if self.workspace.state.search_open {
            self.workspace.state.active_tab = WorkspaceTab::Files;
            self.sync_workspace_search_selection();
            window.focus(&self.workspace.search_focus);
            self.run_workspace_search();
        } else {
            self.workspace.state.search_input.query.clear();
            self.workspace.state.search_results.clear();
            self.workspace.state.search_input.marked_range = None;
            self.workspace.state.search_input.selected_range = 0..0;
            self.workspace.state.search_input.selection_reversed = false;
        }
        cx.notify();
    }

    fn close_workspace_search(&mut self, cx: &mut Context<Self>) {
        if self.workspace.state.search_open {
            self.workspace.state.search_open = false;
            self.workspace.state.search_input.query.clear();
            self.workspace.state.search_results.clear();
            self.workspace.state.search_input.marked_range = None;
            self.workspace.state.search_input.selected_range = 0..0;
            self.workspace.state.search_input.selection_reversed = false;
            self.clear_search_match_highlight(cx);
            self.close_single_line_input_context_menu(cx);
            cx.notify();
        }
    }

    fn focus_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.workspace.state.search_open {
            self.workspace.state.search_open = true;
            self.workspace.state.active_tab = WorkspaceTab::Files;
            self.sync_workspace_search_selection();
        }
        window.focus(&self.workspace.search_focus);
        cx.notify();
    }

    fn run_workspace_search(&mut self) {
        let Some(root) = self.effective_workspace_root() else {
            self.workspace.state.search_results.clear();
            return;
        };

        self.workspace.state.search_results = search_markdown_files(&root, &self.workspace.state.search_input.query);
    }

    pub(crate) fn on_workspace_search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.state.search_open || !self.workspace.search_focus.is_focused(window) {
            return;
        }

        let modifiers = &event.keystroke.modifiers;
        if primary_shortcut_modifiers(modifiers) {
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
            _ => {
                if let Some(arrow) = arrow_key_from_event(event) {
                    self.workspace_search_apply_arrow_key(arrow, cx);
                    cx.stop_propagation();
                }
            }
        }
    }

    fn workspace_search_apply_arrow_key(
        &mut self,
        arrow: SingleLineArrowKey,
        cx: &mut Context<Self>,
    ) {
        match arrow {
            SingleLineArrowKey::MoveLeft => {
                if self.workspace.state.search_input.selected_range.is_empty() {
                    let previous = text_grapheme_boundary(
                        &self.workspace.state.search_input.query,
                        self.workspace_search_cursor_offset(),
                        true,
                    );
                    self.workspace_search_move_to(previous, cx);
                } else {
                    self.workspace_search_move_to(self.workspace.state.search_input.selected_range.start, cx);
                }
            }
            SingleLineArrowKey::MoveRight => {
                if self.workspace.state.search_input.selected_range.is_empty() {
                    let next = text_grapheme_boundary(
                        &self.workspace.state.search_input.query,
                        self.workspace_search_cursor_offset(),
                        false,
                    );
                    self.workspace_search_move_to(next, cx);
                } else {
                    self.workspace_search_move_to(self.workspace.state.search_input.selected_range.end, cx);
                }
            }
            SingleLineArrowKey::Home => self.workspace_search_move_to(0, cx),
            SingleLineArrowKey::End => {
                self.workspace_search_move_to(self.workspace.state.search_input.query.len(), cx);
            }
            SingleLineArrowKey::SelectLeft => self.workspace_search_select_to(
                text_grapheme_boundary(
                    &self.workspace.state.search_input.query,
                    self.workspace_search_cursor_offset(),
                    true,
                ),
                cx,
            ),
            SingleLineArrowKey::SelectRight => self.workspace_search_select_to(
                text_grapheme_boundary(
                    &self.workspace.state.search_input.query,
                    self.workspace_search_cursor_offset(),
                    false,
                ),
                cx,
            ),
            SingleLineArrowKey::SelectHome => self.workspace_search_select_to(0, cx),
            SingleLineArrowKey::SelectEnd => {
                self.workspace_search_select_to(self.workspace.state.search_input.query.len(), cx);
            }
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
        let text_len = self.workspace.state.search_input.query.len();
        let offset = self.workspace_search_index_for_mouse_position(event.position);
        handle_mouse_down(
            event.modifiers.shift,
            offset,
            text_len,
            &mut self.workspace.state.search_input.selected_range,
            &mut self.workspace.state.search_input.selection_reversed,
            &mut self.workspace.state.search_input.marked_range,
            &mut self.workspace.state.search_input.is_selecting,
        );
        cx.notify();
    }

    pub(super) fn workspace_search_prepare_context_menu(
        &mut self,
        position: Point<Pixels>,
    ) {
        let offset = self.workspace_search_index_for_mouse_position(position);
        prepare_context_menu_selection(
            &mut self.workspace.state.search_input.selected_range,
            &mut self.workspace.state.search_input.selection_reversed,
            &mut self.workspace.state.search_input.marked_range,
            &mut self.workspace.state.search_input.is_selecting,
            offset,
            self.workspace.state.search_input.query.len(),
        );
    }

    pub(crate) fn on_workspace_search_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handle_mouse_up(&mut self.workspace.state.search_input.is_selecting) {
            cx.notify();
        }
    }

    pub(crate) fn on_workspace_search_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_search_input_active(window) {
            return;
        }
        let text_len = self.workspace.state.search_input.query.len();
        let offset = self.workspace_search_index_for_mouse_position(event.position);
        if handle_mouse_move(
            event.dragging(),
            offset,
            text_len,
            self.workspace.state.search_input.is_selecting,
            &mut self.workspace.state.search_input.selected_range,
            &mut self.workspace.state.search_input.selection_reversed,
            &mut self.workspace.state.search_input.marked_range,
            &mut self.workspace.state.search_input.is_selecting,
        ) {
            cx.notify();
        }
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
        if let Some(marked) = self.workspace.state.search_input.marked_range.clone() {
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

        let selected = self.workspace.state.search_input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor == 0 {
                return;
            }
            let previous = text_grapheme_boundary(&self.workspace.state.search_input.query, cursor, true);
            previous..cursor
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_workspace_search_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn workspace_search_delete_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.workspace.state.search_input.marked_range.clone() {
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

        let query_len = self.workspace.state.search_input.query.len();
        let selected = self.workspace.state.search_input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor >= query_len {
                return;
            }
            let next = text_grapheme_boundary(&self.workspace.state.search_input.query, cursor, false);
            cursor..next
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_workspace_search_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn workspace_search_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        move_caret_to(
            &mut self.workspace.state.search_input.selected_range,
            &mut self.workspace.state.search_input.selection_reversed,
            &mut self.workspace.state.search_input.marked_range,
            &mut self.workspace.state.search_input.is_selecting,
            offset,
            self.workspace.state.search_input.query.len(),
        );
        cx.notify();
    }

    fn workspace_search_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        select_caret_to(
            &mut self.workspace.state.search_input.selected_range,
            &mut self.workspace.state.search_input.selection_reversed,
            &mut self.workspace.state.search_input.marked_range,
            offset,
            self.workspace.state.search_input.query.len(),
        );
        cx.notify();
    }

    fn workspace_search_replace_selection(
        &mut self,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        let sanitized = flatten_paste_to_single_line(new_text);
        self.replace_workspace_search_text(
            self.workspace.state.search_input.selected_range.clone(),
            &sanitized,
            false,
            None,
            cx,
        );
    }

    pub(super) fn workspace_search_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.workspace_search_replace_selection(&text, cx);
        }
    }

    pub(super) fn workspace_search_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.state.search_input.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.workspace.state.search_input.query
                    [self.workspace.state.search_input.selected_range.clone()]
                    .to_string(),
            ));
        }
    }

    pub(super) fn workspace_search_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.state.search_input.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.workspace.state.search_input.query
                    [self.workspace.state.search_input.selected_range.clone()]
                    .to_string(),
            ));
            self.workspace_search_replace_selection("", cx);
        }
    }

    fn workspace_search_select_all_text(&mut self, cx: &mut Context<Self>) {
        self.workspace_search_move_to(0, cx);
        self.workspace_search_select_to(self.workspace.state.search_input.query.len(), cx);
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
        if self.workspace.state.search_input.selected_range.is_empty() {
            let previous = text_grapheme_boundary(
                &self.workspace.state.search_input.query,
                self.workspace_search_cursor_offset(),
                true,
            );
            self.workspace_search_move_to(previous, cx);
        } else {
            self.workspace_search_move_to(self.workspace.state.search_input.selected_range.start, cx);
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
        if self.workspace.state.search_input.selected_range.is_empty() {
            let next = text_grapheme_boundary(
                &self.workspace.state.search_input.query,
                self.workspace_search_cursor_offset(),
                false,
            );
            self.workspace_search_move_to(next, cx);
        } else {
            self.workspace_search_move_to(self.workspace.state.search_input.selected_range.end, cx);
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
        self.workspace_search_move_to(self.workspace.state.search_input.query.len(), cx);
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
            text_grapheme_boundary(
                &self.workspace.state.search_input.query,
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
            text_grapheme_boundary(
                &self.workspace.state.search_input.query,
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
        self.workspace_search_select_to(self.workspace.state.search_input.query.len(), cx);
    }

    fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.state.expanded.remove(id) {
            self.workspace.state.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    fn select_outline_node(&mut self, id: String, line_index: usize, cx: &mut Context<Self>) {
        self.workspace.state.selected = Some(WorkspaceSelection::Outline(id));
        if self.jump_to_source_line_index(line_index, cx) {
            self.pending_scroll_active_block_into_view = true;
            self.pending_scroll_recheck_after_layout = true;
        }
        cx.notify();
    }

    pub(super) fn workspace_clear_folder_root_if(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.workspace.state.folder_root.as_deref() == Some(path) {
            self.workspace.state.folder_root = None;
            cx.notify();
        }
    }

    pub(super) fn workspace_set_folder_root(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.workspace.state.folder_root = Some(path);
        cx.notify();
    }

    pub(super) fn workspace_folder_root_is(&self, path: &Path) -> bool {
        self.workspace.state.folder_root.as_deref() == Some(path)
    }

    pub(super) fn open_workspace_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_workspace_search_result(path, None, String::new(), None, None, window, cx);
    }

    pub(super) fn filter_workspace_by_tag(
        &mut self,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let canonical = normalize_tag_name(&name);
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);

        let was_open = self.workspace.state.is_open;
        self.workspace.state.is_open = true;
        if !was_open {
            self.sync_workspace_models(cx);
        }

        self.workspace.state.active_tab = WorkspaceTab::Tags;
        self.workspace.state.search_open = false;
        self.workspace.state.selected_tag = Some(canonical);
        cx.notify();
    }

    fn open_workspace_tag_occurrence(
        &mut self,
        path: PathBuf,
        occurrence: TagOccurrence,
        tag_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = format!("#{tag_name}");
        self.workspace.state.selected = Some(WorkspaceSelection::File(path.clone()));
        self.workspace.pending_search_jump = Some(PendingWorkspaceSearchJump {
            line: occurrence.line,
            query,
            preview: occurrence.preview,
            match_start_byte: Some(occurrence.match_start_byte),
            raw_file_len: Some(occurrence.raw_file_len),
        });

        if !self.document_dirty && self.file_path.as_deref() == Some(path.as_path()) {
            self.apply_pending_workspace_search_jump(cx);
            window.activate_window();
            cx.notify();
            return;
        }

        self.request_dropped_markdown_replace(path, window, cx);
    }

    fn toggle_workspace_tag_sort(&mut self, cx: &mut Context<Self>) {
        self.workspace.state.tag_sort = match self.workspace.state.tag_sort {
            WorkspaceTagSort::ByCountDesc => WorkspaceTagSort::ByNameAsc,
            WorkspaceTagSort::ByNameAsc => WorkspaceTagSort::ByCountDesc,
        };
        cx.notify();
    }

    fn select_workspace_tag(&mut self, tag: String, cx: &mut Context<Self>) {
        self.workspace.state.selected_tag = Some(tag);
        cx.notify();
    }

    fn open_workspace_search_result(
        &mut self,
        path: PathBuf,
        line: Option<usize>,
        preview: String,
        match_start_byte: Option<usize>,
        raw_file_len: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.state.selected = Some(WorkspaceSelection::File(path.clone()));

        if let Some(line) = line {
            self.workspace.pending_search_jump = Some(PendingWorkspaceSearchJump {
                line,
                query: self.workspace.state.search_input.query.clone(),
                preview,
                match_start_byte,
                raw_file_len,
            });
        } else {
            self.workspace.pending_search_jump = None;
            self.clear_search_match_highlight(cx);
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
        let Some(jump) = self.workspace.pending_search_jump.clone() else {
            return;
        };

        if self.jump_to_source_line_with_query(
            jump.line,
            &jump.query,
            &jump.preview,
            jump.match_start_byte,
            jump.raw_file_len,
            cx,
        ) {
            self.workspace.pending_search_jump = None;
            self.pending_scroll_active_block_into_view = true;
            self.pending_scroll_recheck_after_layout = true;
        }
    }

    pub(super) fn workspace_panel_width(&self, viewport_width: f32) -> f32 {
        workspace_panel_width_for_viewport(viewport_width, self.workspace.state.panel_width)
    }

    pub(crate) fn start_workspace_resize_drag(
        &mut self,
        pointer_x: f32,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) {
        let current_width = self.workspace_panel_width(viewport_width);
        self.workspace.resize_drag = Some(super::controllers::workspace::WorkspaceResizeDragSession {
            start_pointer_x: pointer_x,
            start_width: current_width,
            viewport_width,
        });
        cx.notify();
    }

    pub(crate) fn update_workspace_resize_drag(&mut self, pointer_x: f32, cx: &mut Context<Self>) {
        let Some(drag) = self.workspace.resize_drag else {
            return;
        };

        let delta = pointer_x - drag.start_pointer_x;
        let next_width =
            clamp_workspace_panel_width(drag.start_width + delta, drag.viewport_width);
        if self.workspace.state.panel_width != Some(next_width) {
            self.workspace.state.panel_width = Some(next_width);
            cx.notify();
        }
    }

    pub(crate) fn end_workspace_resize_drag(&mut self, cx: &mut Context<Self>) {
        if self.workspace.resize_drag.take().is_some() {
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
        if !self.workspace.state.is_open {
            return None;
        }

        self.sync_workspace_models(cx);
        let panel_width = self.workspace_panel_width(viewport_width);
        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let d = &theme.dimensions;

        let tab_icon_color = |active: bool| {
            if active {
                c.text_default
            } else {
                c.dialog_muted
            }
        };

        let tab = |icon: &'static str, tab: WorkspaceTab, active: bool| {
            let tab_editor = editor.clone();
            let tab_id = match tab {
                WorkspaceTab::Files => "workspace-tab-files",
                WorkspaceTab::Outline => "workspace-tab-outline",
                WorkspaceTab::Tags => "workspace-tab-tags",
                WorkspaceTab::Graph => "workspace-tab-graph",
            };
            let icon_color = tab_icon_color(active);
            div()
                .id(tab_id)
                .w(px(24.0))
                .h(px(24.0))
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .bg(if active && !self.workspace.state.search_open {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    svg()
                        .path(icon)
                        .size(px(WORKSPACE_TAB_ICON_SIZE))
                        .flex_shrink_0()
                        .text_color(icon_color),
                )
                .on_click(move |_event, _window, cx| {
                    let _ = tab_editor.update(cx, |editor, cx| {
                        editor.set_workspace_tab(tab, cx);
                    });
                })
        };

        let search_active = self.workspace.state.search_open;
        let search_button_editor = editor.clone();
        let search_focus = self.workspace.search_focus.clone();
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
            Some(
                div()
                    .id("workspace-search-input-row")
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
                                    .child(SingleLineInputElement::new(
                                        cx.entity(),
                                        SingleLineInputTarget::WorkspaceSearch,
                                        placeholder,
                                    )),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_workspace_search_mouse_down),
                            ),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let graph_tab_active = matches!(self.workspace.state.active_tab, WorkspaceTab::Graph);

        let body = if self.workspace.state.search_open {
            self.render_workspace_search_results(theme, strings, &editor)
        } else {
            match self.workspace.state.active_tab {
                WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
                WorkspaceTab::Outline => {
                    self.render_workspace_outline_tree(theme, strings, &editor)
                }
                WorkspaceTab::Tags => self.render_workspace_tags_panel(theme, strings, &editor),
                WorkspaceTab::Graph => self.render_workspace_graph_panel(theme, strings, &editor),
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
                                    FILES_TAB_ICON,
                                    WorkspaceTab::Files,
                                    self.workspace.state.active_tab == WorkspaceTab::Files,
                                ))
                                .child(tab(
                                    OUTLINE_TAB_ICON,
                                    WorkspaceTab::Outline,
                                    self.workspace.state.active_tab == WorkspaceTab::Outline,
                                ))
                                .child(tab(
                                    TAGS_TAB_ICON,
                                    WorkspaceTab::Tags,
                                    self.workspace.state.active_tab == WorkspaceTab::Tags,
                                ))
                                .child(tab(
                                    GRAPH_TAB_ICON,
                                    WorkspaceTab::Graph,
                                    self.workspace.state.active_tab == WorkspaceTab::Graph,
                                ))
                                .child(search_button),
                        )
                        .children(search_bar)
                        .child(if graph_tab_active {
                            div()
                                .id("workspace-panel-scroll")
                                .flex_1()
                                .min_h(px(0.0))
                                .overflow_hidden()
                                .px(px(4.0))
                                .py(px(4.0))
                                .child(body)
                                .into_any_element()
                        } else {
                            div()
                                .id("workspace-panel-scroll")
                                .flex_1()
                                .min_h(px(0.0))
                                .overflow_y_scroll()
                                .px(px(4.0))
                                .py(px(4.0))
                                .child(body)
                                .into_any_element()
                        }),
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
        if self.workspace.state.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        if let Some(error) = self.workspace.state.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        let Some(root) = self.workspace.state.file_tree.as_ref() else {
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
        if self.workspace.state.outline_tree.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_outline, theme);
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(&self.workspace.state.outline_tree, 0, theme, editor))
            .into_any_element()
    }

    fn render_workspace_tags_panel(
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

        let Some(index) = self.workspace.state.tag_index.as_ref() else {
            if self.workspace.state.tag_index_busy {
                return self.render_workspace_empty_state("", &strings.workspace_empty_tags, theme);
            }
            return self.render_workspace_empty_state("", &strings.workspace_empty_tags, theme);
        };

        if index.counts.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_tags, theme);
        }

        let c = &theme.colors;
        let t = &theme.typography;
        let root = self.effective_workspace_root();
        let sort_editor = editor.clone();
        let sort_label = match self.workspace.state.tag_sort {
            WorkspaceTagSort::ByCountDesc => strings.workspace_tag_sort_by_name.clone(),
            WorkspaceTagSort::ByNameAsc => strings.workspace_tag_sort_by_count.clone(),
        };

        let mut tags: Vec<(String, usize)> = index
            .counts
            .iter()
            .map(|(name, count)| (name.clone(), *count))
            .collect();
        match self.workspace.state.tag_sort {
            WorkspaceTagSort::ByCountDesc => {
                tags.sort_by(|left, right| {
                    right
                        .1
                        .cmp(&left.1)
                        .then_with(|| left.0.cmp(&right.0))
                });
            }
            WorkspaceTagSort::ByNameAsc => {
                tags.sort_by(|left, right| left.0.cmp(&right.0));
            }
        }

        let selected_tag = self.workspace.state.selected_tag.clone();
        let mut tag_rows = Vec::new();
        for (index, (tag, count)) in tags.iter().enumerate() {
            let active = selected_tag.as_deref() == Some(tag.as_str());
            let select_tag = tag.clone();
            let select_editor = editor.clone();
            tag_rows.push(
                div()
                    .id(("workspace-tag", index))
                    .w_full()
                    .px(px(6.0))
                    .py(px(4.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .rounded(px(4.0))
                    .bg(if active {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .on_click(move |_event, _window, cx| {
                        let tag = select_tag.clone();
                        let _ = select_editor.update(cx, |editor, cx| {
                            editor.select_workspace_tag(tag, cx);
                        });
                    })
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_1()
                            .truncate()
                            .text_size(px(t.text_size * 0.82))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c.text_tag)
                            .child(format!("#{tag}")),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(t.text_size * 0.78))
                            .text_color(c.dialog_muted)
                            .child(count.to_string()),
                    )
                    .into_any_element(),
            );
        }

        let mut children: Vec<AnyElement> = vec![
            div()
                .w_full()
                .px(px(4.0))
                .pb(px(4.0))
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .id("workspace-tag-sort-toggle")
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .text_size(px(t.text_size * 0.75))
                        .text_color(c.dialog_muted)
                        .hover(|this| {
                            this.bg(c.dialog_secondary_button_hover)
                                .text_color(c.text_default)
                        })
                        .cursor_pointer()
                        .child(sort_label)
                        .on_click(move |_event, _window, cx| {
                            let _ = sort_editor.update(cx, |editor, cx| {
                                editor.toggle_workspace_tag_sort(cx);
                            });
                        }),
                )
                .into_any_element(),
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .children(tag_rows)
                .into_any_element(),
        ];

        if let Some(selected_tag) = selected_tag {
            if let Some(occurrences) = index.by_tag.get(&selected_tag) {
                let title = strings
                    .workspace_tag_occurrences_title
                    .replace("{tag}", &format!("#{selected_tag}"));
                children.push(
                    div()
                        .w_full()
                        .mt(px(8.0))
                        .pt(px(8.0))
                        .border_t(px(1.0))
                        .border_color(c.dialog_border.opacity(0.75))
                        .child(
                            div()
                                .px(px(6.0))
                                .pb(px(4.0))
                                .text_size(px(t.text_size * 0.78))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.dialog_muted)
                                .child(title),
                        )
                        .into_any_element(),
                );

                let mut occurrence_rows = Vec::new();
                let total = occurrences.len();
                for (index, occurrence) in occurrences.iter().enumerate() {
                    let result = WorkspaceSearchResult {
                        path: occurrence.path.clone(),
                        line: Some(occurrence.line),
                        preview: occurrence.preview.clone(),
                        match_start_byte: Some(occurrence.match_start_byte),
                        raw_file_len: Some(occurrence.raw_file_len),
                    };
                    let label = workspace_search_result_label(root.as_deref(), &result);
                    let detail = workspace_search_result_detail(&result);
                    let path = occurrence.path.clone();
                    let open_occurrence = occurrence.clone();
                    let tag_name = selected_tag.clone();
                    let open_editor = editor.clone();
                    occurrence_rows.push(
                        div()
                            .id(("workspace-tag-occurrence", index))
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
                                let open_occurrence = open_occurrence.clone();
                                let tag_name = tag_name.clone();
                                let _ = open_editor.update(cx, |editor, cx| {
                                    editor.open_workspace_tag_occurrence(
                                        open_path,
                                        open_occurrence,
                                        &tag_name,
                                        window,
                                        cx,
                                    );
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
                    if index + 1 < total {
                        occurrence_rows.push(
                            div()
                                .w_full()
                                .px(px(6.0))
                                .border_t(px(1.0))
                                .border_color(c.dialog_border.opacity(0.35))
                                .into_any_element(),
                        );
                    }
                }

                children.push(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .children(occurrence_rows)
                        .into_any_element(),
                );
            }
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(children)
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

        if self.workspace.state.search_input.query.trim().is_empty() {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_search_placeholder,
                theme,
            );
        }

        if self.workspace.state.search_results.is_empty() {
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
        for (index, result) in self.workspace.state.search_results.iter().enumerate() {
            let label = workspace_search_result_label(root.as_deref(), result);
            let detail = workspace_search_result_detail(result);
            let path = result.path.clone();
            let line = result.line;
            let preview = result.preview.clone();
            let match_start_byte = result.match_start_byte;
            let raw_file_len = result.raw_file_len;
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
                        let open_preview = preview.clone();
                        let _ = open_editor.update(cx, |editor, cx| {
                            editor.open_workspace_search_result(
                                open_path,
                                line,
                                open_preview,
                                match_start_byte,
                                raw_file_len,
                                window,
                                cx,
                            );
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

    pub(super) fn render_workspace_empty_state(
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
            if !node.children.is_empty() && self.workspace.state.expanded.contains(&node.id) {
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
        let is_expanded = self.workspace.state.expanded.contains(&node.id);
        let has_children = !node.children.is_empty();
        let selected = match (&self.workspace.state.selected, &node.kind) {
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::MarkdownFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::Outline(selected)), _) => selected == &node.id,
            _ => false,
        };
        let supports_file_menu = matches!(
            &node.kind,
            WorkspaceTreeKind::Directory(_) | WorkspaceTreeKind::MarkdownFile(_)
        );
        let node_id = node.id.clone();
        let click_editor = editor.clone();
        let click_kind = node.kind.clone();
        let context_menu_editor = editor.clone();
        let context_menu_kind = node.kind.clone();
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
                    WorkspaceTreeKind::Heading { line, .. } => {
                        editor.select_outline_node(node_id, line, cx);
                    }
                });
            })
            .when(supports_file_menu, |this| {
                    this.on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                        cx.stop_propagation();
                        let target = match context_menu_kind.clone() {
                            WorkspaceTreeKind::Directory(path) => {
                                Some(super::workspace_file_menu::WorkspaceFileMenuTarget::Directory(
                                    path,
                                ))
                            }
                            WorkspaceTreeKind::MarkdownFile(path) => Some(
                                super::workspace_file_menu::WorkspaceFileMenuTarget::MarkdownFile(
                                    path,
                                ),
                            ),
                            WorkspaceTreeKind::Heading { .. } => None,
                        };
                        let Some(target) = target else {
                            return;
                        };
                        let _ = context_menu_editor.update(cx, |editor, cx| {
                            editor.open_workspace_file_context_menu(event.position, target, cx);
                        });
                    })
            })
            .into_any_element()
    }
}

fn keyword_byte_offset_in_line(line: &str, query: &str) -> usize {
    super::search_match::find_case_insensitive_start(line, query)
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
                match_start_byte: None,
                raw_file_len: None,
            });
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let raw_file_len = content.len();
        for (line_number, line_start, line) in markdown_line_ranges(&content) {
            if !line.to_lowercase().contains(&query_lower) {
                continue;
            }
            let keyword_offset = keyword_byte_offset_in_line(line, query);
            results.push(WorkspaceSearchResult {
                path: path.clone(),
                line: Some(line_number),
                preview: line.trim().to_string(),
                match_start_byte: Some(line_start + keyword_offset),
                raw_file_len: Some(raw_file_len),
            });
        }
    }

    results
}

fn markdown_line_ranges(content: &str) -> impl Iterator<Item = (usize, usize, &str)> + '_ {
    let mut line_number = 1usize;
    let mut line_start = 0usize;
    content
        .split_inclusive('\n')
        .map(move |segment| {
            let line_end = line_start + segment.len();
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let current = (line_number, line_start, line);
            line_start = line_end;
            line_number += 1;
            current
        })
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

pub(super) fn workspace_file_node_id(path: &Path) -> String {
    file_node_id(path)
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
    let mut fence: Option<FenceInfo> = None;

    for (line_index, line) in markdown.lines().enumerate() {
        if let Some(ref opener) = fence {
            if is_closing_fence(line, opener) {
                fence = None;
            }
            continue;
        }

        if let Some(next_fence) = parse_opening_fence(line) {
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
        if self.ai_prompt_input_active(window) {
            let text = self.ai_prompt_text().to_string();
            let range = document_search_range_from_utf16(&text, &range_utf16);
            actual_range.replace(document_search_range_to_utf16(&text, &range));
            return Some(text[range].to_string());
        }

        if self.quick_file_open_input_active(window) {
            let text = self.quick_file_open.input.query.clone();
            let range = range_utf16.start.min(text.len())..range_utf16.end.min(text.len());
            actual_range.replace(range.clone());
            return Some(text[range].to_string());
        }

        if self.wiki_link_picker_input_active(window) {
            let text = self.wiki_link_picker.input.query.clone();
            let range = document_search_range_from_utf16(&text, &range_utf16);
            actual_range.replace(document_search_range_to_utf16(&text, &range));
            return Some(text[range].to_string());
        }

        if self.workspace_name_input_active(window) {
            let text = self.workspace.name_dialog.as_ref()?.input.query.clone();
            let range = workspace_search_range_from_utf16(&text, &range_utf16);
            actual_range.replace(workspace_search_range_to_utf16(&text, &range));
            return Some(text[range].to_string());
        }

        if self.document_search_input_active(window) {
            let text = self.search.state.input.query.clone();
            let range = document_search_range_from_utf16(&text, &range_utf16);
            actual_range.replace(document_search_range_to_utf16(&text, &range));
            return Some(text[range].to_string());
        }

        if !self.workspace_search_input_active(window) {
            return None;
        }

        let text = self.workspace.state.search_input.query.clone();
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
        if self.ai_prompt_input_active(window) {
            let text = self.ai_prompt_text();
            return Some(UTF16Selection {
                range: document_search_range_to_utf16(text, &self.ai_prompt_selected_range()),
                reversed: self.ai_prompt_selection_reversed(),
            });
        }

        if self.quick_file_open_input_active(window) {
            let len = self.quick_file_open.input.text_len();
            return Some(UTF16Selection {
                range: len..len,
                reversed: false,
            });
        }

        if self.wiki_link_picker_input_active(window) {
            let text = &self.wiki_link_picker.input.query;
            return Some(UTF16Selection {
                range: document_search_range_to_utf16(text, &self.wiki_link_picker.input.selected_range),
                reversed: self.wiki_link_picker.input.selection_reversed,
            });
        }

        if self.workspace_name_input_active(window) {
            let dialog = self.workspace.name_dialog.as_ref()?;
            return Some(UTF16Selection {
                range: workspace_search_range_to_utf16(&dialog.input.query, &dialog.input.selected_range),
                reversed: false,
            });
        }

        if self.document_search_input_active(window) {
            let text = &self.search.state.input.query;
            return Some(UTF16Selection {
                range: document_search_range_to_utf16(text, &self.search.state.input.selected_range),
                reversed: self.search.state.input.selection_reversed,
            });
        }

        if !self.workspace_search_input_active(window) {
            return None;
        }

        let text = &self.workspace.state.search_input.query;
        Some(UTF16Selection {
            range: workspace_search_range_to_utf16(text, &self.workspace.state.search_input.selected_range),
            reversed: false,
        })
    }

    fn marked_text_range(&self, window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        if self.ai_prompt_input_active(window) {
            return self
                .ai_prompt_marked_range()
                .as_ref()
                .map(|range| document_search_range_to_utf16(self.ai_prompt_text(), range));
        }

        if self.quick_file_open_input_active(window) {
            return None;
        }

        if self.wiki_link_picker_input_active(window) {
            return self
                .wiki_link_picker
                .input
                .marked_range
                .as_ref()
                .map(|range| document_search_range_to_utf16(&self.wiki_link_picker.input.query, range));
        }

        if self.workspace_name_input_active(window) {
            let dialog = self.workspace.name_dialog.as_ref()?;
            return dialog
                .input
                .marked_range
                .as_ref()
                .map(|range| workspace_search_range_to_utf16(&dialog.input.query, range));
        }

        if self.document_search_input_active(window) {
            return self
                .search
                .state
                .input
                .marked_range
                .as_ref()
                .map(|range| document_search_range_to_utf16(&self.search.state.input.query, range));
        }

        if !self.workspace_search_input_active(window) {
            return None;
        }

        self.workspace
            .state
            .search_input
            .marked_range
            .as_ref()
            .map(|range| workspace_search_range_to_utf16(&self.workspace.state.search_input.query, range))
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.ai_prompt_input_active(window) {
            self.unmark_ai_prompt_text();
            return;
        }

        if self.quick_file_open_input_active(window) {
            return;
        }

        if self.wiki_link_picker_input_active(window) {
            self.wiki_link_picker.input.marked_range = None;
            return;
        }

        if self.workspace_name_input_active(window) {
            if let Some(dialog) = self.workspace.name_dialog.as_mut() {
                dialog.input.marked_range = None;
            }
            return;
        }

        if self.document_search_input_active(window) {
            self.search.state.input.marked_range = None;
            return;
        }

        if self.workspace_search_input_active(window) {
            self.workspace.state.search_input.marked_range = None;
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai_prompt_input_active(window) {
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| document_search_range_from_utf16(self.ai_prompt_text(), range_utf16))
                .or_else(|| self.ai_prompt_marked_range())
                .unwrap_or_else(|| self.ai_prompt_selected_range());
            self.replace_ai_prompt_text(range, new_text, false, None, cx);
            return;
        }

        if self.replace_quick_file_open_from_utf16(range_utf16.as_ref(), new_text, window, cx) {
            return;
        }

        if self.replace_wiki_link_picker_from_utf16(range_utf16.as_ref(), new_text, window, cx) {
            return;
        }

        if self.workspace_name_input_active(window) {
            let Some(dialog) = self.workspace.name_dialog.as_ref() else {
                return;
            };
            let text = dialog.input.query.clone();
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| workspace_search_range_from_utf16(&text, range_utf16))
                .or_else(|| dialog.input.marked_range.clone())
                .unwrap_or_else(|| dialog.input.selected_range.clone());
            self.replace_workspace_name_dialog_text(range, new_text, false, None, cx);
            return;
        }

        if self.document_search_input_active(window) {
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| {
                    document_search_range_from_utf16(&self.search.state.input.query, range_utf16)
                })
                .or_else(|| self.search.state.input.marked_range.clone())
                .unwrap_or_else(|| self.search.state.input.selected_range.clone());
            self.replace_document_search_text(range, new_text, false, None, cx);
            return;
        }

        if !self.workspace_search_input_active(window) {
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range_utf16| workspace_search_range_from_utf16(&self.workspace.state.search_input.query, range_utf16))
            .or_else(|| self.workspace.state.search_input.marked_range.clone())
            .unwrap_or_else(|| self.workspace.state.search_input.selected_range.clone());

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
        if self.ai_prompt_input_active(window) {
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| document_search_range_from_utf16(self.ai_prompt_text(), range_utf16))
                .or_else(|| self.ai_prompt_marked_range())
                .unwrap_or_else(|| self.ai_prompt_selected_range());
            let selected = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| document_search_range_from_utf16(new_text, range_utf16))
                .map(|relative| relative.start + range.start..relative.end + range.start);
            self.replace_ai_prompt_text(range, new_text, true, selected, cx);
            return;
        }

        if self.replace_quick_file_open_from_utf16(range_utf16.as_ref(), new_text, window, cx) {
            return;
        }

        if self.wiki_link_picker_input_active(window) {
            let text = self.wiki_link_picker.input.query.clone();
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| document_search_range_from_utf16(&text, range_utf16))
                .or_else(|| self.wiki_link_picker.input.marked_range.clone())
                .unwrap_or_else(|| self.wiki_link_picker.input.selected_range.clone());
            let selected = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| document_search_range_from_utf16(new_text, range_utf16))
                .map(|relative| relative.start + range.start..relative.end + range.start);
            self.replace_wiki_link_picker_text(range, new_text, true, selected, cx);
            return;
        }

        if self.workspace_name_input_active(window) {
            let Some(dialog) = self.workspace.name_dialog.as_ref() else {
                return;
            };
            let text = dialog.input.query.clone();
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| workspace_search_range_from_utf16(&text, range_utf16))
                .or_else(|| dialog.input.marked_range.clone())
                .unwrap_or_else(|| dialog.input.selected_range.clone());
            let selected = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| workspace_search_range_from_utf16(new_text, range_utf16))
                .map(|relative| relative.start + range.start..relative.end + range.start);
            self.replace_workspace_name_dialog_text(range, new_text, true, selected, cx);
            return;
        }

        if self.document_search_input_active(window) {
            let range = range_utf16
                .as_ref()
                .map(|range_utf16| {
                    document_search_range_from_utf16(&self.search.state.input.query, range_utf16)
                })
                .or_else(|| self.search.state.input.marked_range.clone())
                .unwrap_or_else(|| self.search.state.input.selected_range.clone());
            let selected = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| document_search_range_from_utf16(new_text, range_utf16))
                .map(|relative| relative.start + range.start..relative.end + range.start);
            self.replace_document_search_text(range, new_text, true, selected, cx);
            return;
        }

        if !self.workspace_search_input_active(window) {
            return;
        }

        let range = range_utf16
            .as_ref()
            .map(|range_utf16| workspace_search_range_from_utf16(&self.workspace.state.search_input.query, range_utf16))
            .or_else(|| self.workspace.state.search_input.marked_range.clone())
            .unwrap_or_else(|| self.workspace.state.search_input.selected_range.clone());
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
        if self.ai_prompt_input_active(window) {
            let range = document_search_range_from_utf16(self.ai_prompt_text(), &range_utf16);
            return super::controllers::ai::ai_range_bounds(
                self.ai_prompt_line_layouts(),
                bounds,
                self.ai_prompt_line_height(),
                self.ai_prompt_text(),
                range,
            )
            .or(Some(bounds));
        }

        if self.quick_file_open_input_active(window) {
            return Some(bounds);
        }

        if self.wiki_link_picker_input_active(window) {
            let line = self.wiki_link_picker.input.last_layout.as_ref()?;
            let text = &self.wiki_link_picker.input.query;
            let range = document_search_range_from_utf16(text, &range_utf16);
            return Some(Bounds::from_corners(
                point(bounds.left() + line.x_for_index(range.start), bounds.top()),
                point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
            ));
        }

        if self.workspace_name_input_active(window) {
            let dialog = self.workspace.name_dialog.as_ref()?;
            let line = dialog.input.last_layout.as_ref()?;
            let text = self.workspace_name_text();
            let range = workspace_search_range_from_utf16(&text, &range_utf16);
            return Some(Bounds::from_corners(
                point(bounds.left() + line.x_for_index(range.start), bounds.top()),
                point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
            ));
        }

        if self.document_search_input_active(window) {
            let line = self.search.state.input.last_layout.as_ref()?;
            let text = &self.search.state.input.query;
            let range = document_search_range_from_utf16(text, &range_utf16);
            return Some(Bounds::from_corners(
                point(bounds.left() + line.x_for_index(range.start), bounds.top()),
                point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
            ));
        }

        if !self.workspace_search_input_active(window) {
            return None;
        }

        let line = self.workspace.state.search_input.last_layout.as_ref()?;
        let range = workspace_search_range_from_utf16(&self.workspace.state.search_input.query, &range_utf16);
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
        if self.ai_prompt_input_active(window) {
            return Some(document_search_offset_to_utf16(
                self.ai_prompt_text(),
                self.ai_prompt_offset_for_position(point),
            ));
        }

        if self.quick_file_open_input_active(window) {
            return Some(self.quick_file_open.input.text_len());
        }

        if self.wiki_link_picker_input_active(window) {
            let bounds = self.wiki_link_picker.input.last_bounds?;
            let line = self.wiki_link_picker.input.last_layout.as_ref()?;
            let text = &self.wiki_link_picker.input.query;
            let local = bounds.localize(&point)?;
            let utf8_index = line.index_for_x(local.x - bounds.left())?;
            return Some(document_search_offset_to_utf16(
                text,
                utf8_index.min(text.len()),
            ));
        }

        if self.workspace_name_input_active(window) {
            let dialog = self.workspace.name_dialog.as_ref()?;
            let bounds = dialog.input.last_bounds?;
            let line = dialog.input.last_layout.as_ref()?;
            let text = self.workspace_name_text();
            let local = bounds.localize(&point)?;
            let utf8_index = line.index_for_x(local.x - bounds.left())?;
            return Some(workspace_search_offset_to_utf16(
                &text,
                utf8_index.min(text.len()),
            ));
        }

        if self.document_search_input_active(window) {
            let bounds = self.search.state.input.last_bounds?;
            let line = self.search.state.input.last_layout.as_ref()?;
            let text = &self.search.state.input.query;
            let local = bounds.localize(&point)?;
            let utf8_index = line.index_for_x(local.x - bounds.left())?;
            return Some(document_search_offset_to_utf16(
                text,
                utf8_index.min(text.len()),
            ));
        }

        if !self.workspace_search_input_active(window) {
            return None;
        }

        let bounds = self.workspace.state.search_input.last_bounds?;
        let line = self.workspace.state.search_input.last_layout.as_ref()?;
        let local = bounds.localize(&point)?;
        let utf8_index = line.index_for_x(local.x - bounds.left())?;
        Some(workspace_search_offset_to_utf16(
            &self.workspace.state.search_input.query,
            utf8_index.min(self.workspace.state.search_input.query.len()),
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
