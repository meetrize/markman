//! Contextual file picker shown while editing `[[wiki-link]]` paths.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::document_search::document_search_range_from_utf16;
use super::file_search::{
    FileSearchResult, FileTreeNode, FileTreeRow, build_workspace_file_tree,
    collect_all_files_recursive, filter_files_fuzzy, flatten_file_tree,
};
use super::single_line_input::{
    SingleLineInputTarget, handle_mouse_down, handle_mouse_move, handle_mouse_up,
    index_for_mouse_position, move_caret_to, primary_shortcut_modifiers, select_caret_to,
    text_grapheme_boundary,
};
use super::single_line_input_element::SingleLineInputElement;
use super::Editor;
use crate::components::{
    Copy, Cut, Delete, DeleteBack, End, Home, MoveLeft, MoveRight, Paste, SelectAll, SelectEnd,
    SelectHome, SelectLeft, SelectRight,
};
use crate::i18n::I18nStrings;
use crate::input::single_line_field::SingleLineFieldState;
use crate::theme::{Theme, ThemeColors, ThemeTypography};

const VIEWPORT_MARGIN: f32 = 8.0;
const PANEL_WIDTH: f32 = 420.0;
const ROW_HEIGHT: f32 = 28.0;
const MAX_VISIBLE_ROWS: usize = 10;
const INPUT_HEIGHT: f32 = 32.0;

enum WikiLinkPickerAcceptAction {
    Apply(String),
    Toggle(String),
}

#[derive(Clone, Debug)]
pub(super) struct WikiLinkPickerState {
    pub open: bool,
    pub block_entity_id: Option<EntityId>,
    pub input: SingleLineFieldState,
    pub all_files: Vec<PathBuf>,
    pub file_tree: Option<FileTreeNode>,
    pub tree_expanded: HashSet<String>,
    pub visible_rows: Vec<FileTreeRow>,
    pub filter_results: Vec<FileSearchResult>,
    pub filtering: bool,
    pub selection: usize,
    pub scroll_top: usize,
    pub focus_handle: FocusHandle,
    /// After the user confirms a file, keep the picker closed while the caret
    /// remains inside the same wiki link.
    pub suppress_auto_open_for: Option<EntityId>,
}

impl WikiLinkPickerState {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            open: false,
            block_entity_id: None,
            input: SingleLineFieldState::new(),
            all_files: Vec::new(),
            file_tree: None,
            tree_expanded: HashSet::new(),
            visible_rows: Vec::new(),
            filter_results: Vec::new(),
            filtering: false,
            selection: 0,
            scroll_top: 0,
            focus_handle: cx.focus_handle(),
            suppress_auto_open_for: None,
        }
    }
}

impl Editor {
    fn sync_wiki_link_picker_suppression(&mut self, window: &Window, cx: &App) {
        let Some(suppressed) = self.wiki_link_picker.suppress_auto_open_for else {
            return;
        };
        let still_suppressed = self
            .document
            .block_entity_by_id(suppressed)
            .zip(self.focused_edit_target(window, cx))
            .filter(|(block, focused)| focused.entity_id() == block.entity_id())
            .and_then(|(block, _)| block.read(cx).wiki_link_edit_context())
            .is_some();
        if !still_suppressed {
            self.wiki_link_picker.suppress_auto_open_for = None;
        }
    }

    fn resolve_wiki_link_picker_session(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(EntityId, String)> {
        if self.wiki_link_picker.open {
            let entity_id = self.wiki_link_picker.block_entity_id?;
            if self.document.block_entity_by_id(entity_id).is_none() {
                return None;
            }

            if self.wiki_link_picker.focus_handle.is_focused(window) {
                return Some((entity_id, self.wiki_link_picker.input.query.clone()));
            }

            if let Some(focused) = self.focused_edit_target(window, cx) {
                if focused.entity_id() != entity_id {
                    return None;
                }
                if let Some(context) = focused.read(cx).wiki_link_edit_context() {
                    return Some((entity_id, context.path));
                }
                return None;
            }

            // Focus is transitioning (e.g. clicking the search box). The block
            // clears inline projection on blur, so do not consult wiki context here.
            return Some((entity_id, self.wiki_link_picker.input.query.clone()));
        }

        self.focused_edit_target(window, cx).and_then(|block| {
            if self.wiki_link_picker.suppress_auto_open_for == Some(block.entity_id()) {
                return None;
            }
            block
                .read(cx)
                .wiki_link_edit_context()
                .map(|context| (block.entity_id(), context.path))
        })
    }

    pub(super) fn sync_wiki_link_picker(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sync_wiki_link_picker_suppression(window, cx);
        let Some((entity_id, path)) = self.resolve_wiki_link_picker_session(window, cx) else {
            self.close_wiki_link_picker(cx);
            return;
        };

        if !self.wiki_link_picker.open
            || self.wiki_link_picker.block_entity_id != Some(entity_id)
        {
            self.open_wiki_link_picker(entity_id, &path, cx);
            return;
        }

        if !self.wiki_link_picker_input_active(window) && self.wiki_link_picker.input.query != path
        {
            self.wiki_link_picker.input.query = path;
            self.wiki_link_picker.input.sync_caret_to_end();
            self.refresh_wiki_link_picker_results(cx);
        }
    }

    pub(crate) fn on_dismiss_wiki_link_picker_overlay(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(entity_id) = self.wiki_link_picker.block_entity_id {
            self.wiki_link_picker.suppress_auto_open_for = Some(entity_id);
        }
        self.close_wiki_link_picker(cx);
        cx.stop_propagation();
    }

    pub(super) fn request_open_wiki_link_picker(
        &mut self,
        block_entity_id: EntityId,
        path: String,
        cx: &mut Context<Self>,
    ) {
        self.pending_wiki_link_picker = Some((block_entity_id, path));
        cx.notify();
    }

    pub(super) fn sync_pending_wiki_link_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((entity_id, path)) = self.pending_wiki_link_picker.take() else {
            return;
        };
        self.show_wiki_link_picker_for_block(entity_id, &path, window, cx);
    }

    pub(super) fn show_wiki_link_picker_for_block(
        &mut self,
        block_entity_id: EntityId,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.wiki_link_picker.suppress_auto_open_for = None;
        self.focus_block(block_entity_id);
        self.open_wiki_link_picker(block_entity_id, path, cx);
        window.focus(&self.wiki_link_picker.focus_handle);
    }

    fn open_wiki_link_picker(
        &mut self,
        block_entity_id: EntityId,
        path: &str,
        cx: &mut Context<Self>,
    ) {
        let root = self.effective_workspace_root();
        let picker = &mut self.wiki_link_picker;
        picker.open = true;
        picker.block_entity_id = Some(block_entity_id);
        picker.input.query = path.to_string();
        picker.input.sync_caret_to_end();
        picker.input.clear_selection_and_layout();
        picker.all_files = collect_all_files_recursive(root.as_deref());
        picker.file_tree = build_workspace_file_tree(root.as_deref());
        if let Some(tree) = picker.file_tree.as_ref() {
            picker.tree_expanded.insert(tree.id.clone());
        }
        picker.selection = 0;
        picker.scroll_top = 0;
        self.refresh_wiki_link_picker_results(cx);
        cx.notify();
    }

    pub(super) fn close_wiki_link_picker(&mut self, cx: &mut Context<Self>) {
        if !self.wiki_link_picker.open {
            return;
        }
        let picker = &mut self.wiki_link_picker;
        picker.open = false;
        picker.block_entity_id = None;
        picker.input.clear();
        picker.all_files.clear();
        picker.file_tree = None;
        picker.tree_expanded.clear();
        picker.visible_rows.clear();
        picker.filter_results.clear();
        picker.filtering = false;
        cx.notify();
    }

    pub(super) fn wiki_link_picker_input_active(&self, window: &Window) -> bool {
        self.wiki_link_picker.open && self.wiki_link_picker.focus_handle.is_focused(window)
    }

    pub(super) fn wiki_link_picker_query_is_empty(&self) -> bool {
        self.wiki_link_picker.input.query.is_empty()
    }

    pub(super) fn wiki_link_picker_display_text(&self, placeholder: &SharedString) -> SharedString {
        if self.wiki_link_picker.input.query.is_empty() {
            placeholder.clone()
        } else {
            self.wiki_link_picker.input.query.clone().into()
        }
    }

    pub(super) fn wiki_link_picker_marked_range(&self) -> Option<Range<usize>> {
        self.wiki_link_picker.input.marked_range.clone()
    }

    pub(super) fn wiki_link_picker_selected_range(&self) -> Range<usize> {
        self.wiki_link_picker.input.selected_range.clone()
    }

    pub(super) fn wiki_link_picker_cursor_offset(&self) -> usize {
        self.wiki_link_picker.input.cursor_offset()
    }

    pub(super) fn set_wiki_link_picker_layout(
        &mut self,
        line: ShapedLine,
        bounds: Bounds<Pixels>,
    ) {
        self.wiki_link_picker.input.set_layout(line, bounds);
    }

    fn refresh_wiki_link_picker_results(&mut self, cx: &mut Context<Self>) {
        let root = self.effective_workspace_root();
        let query = self.wiki_link_picker.input.query.trim().to_string();
        let file_tree = self.wiki_link_picker.file_tree.clone();
        let tree_expanded = self.wiki_link_picker.tree_expanded.clone();
        let all_files = self.wiki_link_picker.all_files.clone();
        let picker = &mut self.wiki_link_picker;

        if query.is_empty() {
            picker.filtering = false;
            picker.filter_results.clear();
            picker.visible_rows.clear();
            if let Some(tree) = file_tree.as_ref() {
                flatten_file_tree(
                    tree,
                    &tree_expanded,
                    0,
                    root.as_deref(),
                    &mut picker.visible_rows,
                );
            }
        } else {
            picker.filtering = true;
            picker.visible_rows.clear();
            picker.filter_results = filter_files_fuzzy(&all_files, root.as_deref(), &query);
        }

        picker.selection = 0;
        picker.scroll_top = 0;
        cx.notify();
    }

    pub(super) fn replace_wiki_link_picker_text(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        marked: bool,
        selected_range: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let query = &mut self.wiki_link_picker.input.query;
        let start = range.start.min(query.len());
        let end = range.end.min(query.len());
        query.replace_range(start..end, replacement);
        self.wiki_link_picker.input.marked_range = marked.then(|| {
            let marked_start = start;
            let marked_end = start + replacement.len();
            marked_start..marked_end
        });
        if let Some(selected_range) = selected_range {
            self.wiki_link_picker.input.selected_range = selected_range;
        } else {
            let cursor = start + replacement.len();
            self.wiki_link_picker.input.selected_range = cursor..cursor;
        }
        self.wiki_link_picker.input.selection_reversed = false;
        self.refresh_wiki_link_picker_results(cx);
    }

    pub(super) fn wiki_link_picker_index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        index_for_mouse_position(
            self.wiki_link_picker.input.query.len(),
            self.wiki_link_picker.input.last_bounds.as_ref(),
            self.wiki_link_picker.input.last_layout.as_ref(),
            position,
        )
    }

    fn wiki_link_picker_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        move_caret_to(
            &mut self.wiki_link_picker.input.selected_range,
            &mut self.wiki_link_picker.input.selection_reversed,
            &mut self.wiki_link_picker.input.marked_range,
            &mut self.wiki_link_picker.input.is_selecting,
            offset,
            self.wiki_link_picker.input.query.len(),
        );
        cx.notify();
    }

    fn wiki_link_picker_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        select_caret_to(
            &mut self.wiki_link_picker.input.selected_range,
            &mut self.wiki_link_picker.input.selection_reversed,
            &mut self.wiki_link_picker.input.marked_range,
            offset,
            self.wiki_link_picker.input.query.len(),
        );
        cx.notify();
    }

    fn wiki_link_picker_delete_backward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.wiki_link_picker.input.marked_range.clone() {
            let cursor = marked.start;
            self.replace_wiki_link_picker_text(marked, "", false, Some(cursor..cursor), cx);
            return;
        }

        let selected = self.wiki_link_picker.input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor == 0 {
                return;
            }
            let previous =
                text_grapheme_boundary(&self.wiki_link_picker.input.query, cursor, true);
            previous..cursor
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_wiki_link_picker_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn wiki_link_picker_delete_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.wiki_link_picker.input.marked_range.clone() {
            let cursor = marked.start;
            self.replace_wiki_link_picker_text(marked, "", false, Some(cursor..cursor), cx);
            return;
        }

        let query_len = self.wiki_link_picker.input.query.len();
        let selected = self.wiki_link_picker.input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor >= query_len {
                return;
            }
            let next = text_grapheme_boundary(&self.wiki_link_picker.input.query, cursor, false);
            cursor..next
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_wiki_link_picker_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn wiki_link_picker_replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = if self.wiki_link_picker.input.selected_range.is_empty() {
            let cursor = self.wiki_link_picker_cursor_offset();
            cursor..cursor
        } else {
            self.wiki_link_picker.input.selected_range.clone()
        };
        let cursor = range.start + text.len();
        self.replace_wiki_link_picker_text(range, text, false, Some(cursor..cursor), cx);
    }

    pub(super) fn wiki_link_picker_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = super::single_line_input::sanitize_pasted_text(&text);
            self.wiki_link_picker_replace_selection(&text, cx);
        }
    }

    pub(super) fn wiki_link_picker_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.wiki_link_picker.input.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.wiki_link_picker.input.query
                    [self.wiki_link_picker.input.selected_range.clone()]
                    .to_string(),
            ));
        }
    }

    pub(super) fn wiki_link_picker_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        self.wiki_link_picker_copy_to_clipboard(cx);
        if !self.wiki_link_picker.input.selected_range.is_empty() {
            self.wiki_link_picker_replace_selection("", cx);
        }
    }

    fn wiki_link_picker_select_all_text(&mut self, cx: &mut Context<Self>) {
        self.wiki_link_picker_move_to(0, cx);
        self.wiki_link_picker_select_to(self.wiki_link_picker.input.query.len(), cx);
    }

    fn wiki_link_picker_row_count(&self) -> usize {
        let picker = &self.wiki_link_picker;
        if picker.filtering {
            picker.filter_results.len()
        } else {
            picker.visible_rows.len()
        }
    }

    pub(super) fn replace_wiki_link_picker_from_utf16(
        &mut self,
        range_utf16: Option<&Range<usize>>,
        new_text: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.wiki_link_picker_input_active(window) {
            return false;
        }
        let text = &self.wiki_link_picker.input.query;
        let range = range_utf16
            .as_ref()
            .map(|range| document_search_range_from_utf16(text, range))
            .or_else(|| self.wiki_link_picker.input.marked_range.clone())
            .unwrap_or_else(|| self.wiki_link_picker.input.selected_range.clone());
        self.replace_wiki_link_picker_text(range, new_text, false, None, cx);
        true
    }

    pub(super) fn wiki_link_picker_select_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let count = self.wiki_link_picker_row_count();
        if count == 0 {
            return;
        }
        let index = index.min(count - 1);
        let picker = &mut self.wiki_link_picker;
        picker.selection = index;
        if index < picker.scroll_top {
            picker.scroll_top = index;
        } else if index >= picker.scroll_top + MAX_VISIBLE_ROWS {
            picker.scroll_top = index.saturating_sub(MAX_VISIBLE_ROWS - 1);
        }
        cx.notify();
    }

    pub(super) fn wiki_link_picker_select_file(
        &mut self,
        row_index: usize,
        relative_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.wiki_link_picker_select_row(row_index, cx);
        self.wiki_link_picker_apply_relative_path(relative_path, window, cx);
    }

    pub(super) fn wiki_link_picker_apply_arrow(&mut self, direction: isize, cx: &mut Context<Self>) {
        let count = self.wiki_link_picker_row_count();
        if count == 0 {
            return;
        }
        let picker = &mut self.wiki_link_picker;
        let new_index =
            (picker.selection as isize + direction).rem_euclid(count as isize) as usize;
        picker.selection = new_index;
        if new_index < picker.scroll_top {
            picker.scroll_top = new_index;
        } else if new_index >= picker.scroll_top + MAX_VISIBLE_ROWS {
            picker.scroll_top = new_index.saturating_sub(MAX_VISIBLE_ROWS - 1);
        }
        cx.notify();
    }

    pub(super) fn wiki_link_picker_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let action = {
            let picker = &self.wiki_link_picker;
            if picker.filtering {
                picker
                    .filter_results
                    .get(picker.selection)
                    .map(|result| WikiLinkPickerAcceptAction::Apply(result.label.clone()))
            } else {
                match picker.visible_rows.get(picker.selection) {
                    Some(FileTreeRow::File { relative_path, .. }) => {
                        Some(WikiLinkPickerAcceptAction::Apply(relative_path.clone()))
                    }
                    Some(FileTreeRow::Directory { node_id, .. }) => {
                        Some(WikiLinkPickerAcceptAction::Toggle(node_id.clone()))
                    }
                    None => None,
                }
            }
        };

        match action {
            Some(WikiLinkPickerAcceptAction::Apply(path)) => {
                self.wiki_link_picker_apply_relative_path(&path, window, cx);
            }
            Some(WikiLinkPickerAcceptAction::Toggle(node_id)) => {
                self.wiki_link_picker_toggle_directory(&node_id, cx);
            }
            None => {}
        }
    }

    pub(super) fn wiki_link_picker_toggle_directory(
        &mut self,
        node_id: &str,
        cx: &mut Context<Self>,
    ) {
        let picker = &mut self.wiki_link_picker;
        if picker.tree_expanded.contains(node_id) {
            picker.tree_expanded.remove(node_id);
        } else {
            picker.tree_expanded.insert(node_id.to_string());
        }
        self.refresh_wiki_link_picker_results(cx);
    }

    pub(super) fn wiki_link_picker_apply_relative_path(
        &mut self,
        relative_path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entity_id) = self.wiki_link_picker.block_entity_id else {
            return;
        };
        let Some(block) = self.document.block_entity_by_id(entity_id) else {
            return;
        };

        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        block.update(cx, |block, cx| {
            if block.projection.is_none() {
                block.sync_inline_projection_for_focus(true);
            }
            block.apply_wiki_link_path(relative_path, cx);
            cx.emit(crate::components::BlockEvent::Changed);
        });
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);

        self.wiki_link_picker.suppress_auto_open_for = Some(entity_id);
        self.close_wiki_link_picker(cx);
        window.focus(&block.read(cx).focus_handle);
    }

    pub(super) fn on_wiki_link_picker_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker.open || !self.wiki_link_picker_input_active(window) {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if primary_shortcut_modifiers(&modifiers) {
            match event.keystroke.key.as_str() {
                "v" => {
                    self.wiki_link_picker_paste_from_clipboard(cx);
                    cx.stop_propagation();
                }
                "c" => {
                    self.wiki_link_picker_copy_to_clipboard(cx);
                    cx.stop_propagation();
                }
                "x" => {
                    self.wiki_link_picker_cut_to_clipboard(cx);
                    cx.stop_propagation();
                }
                "a" => {
                    self.wiki_link_picker_select_all_text(cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                if let Some(block) = self.focused_edit_target(window, cx) {
                    window.focus(&block.read(cx).focus_handle);
                }
                cx.notify();
                cx.stop_propagation();
            }
            "enter" => {
                self.wiki_link_picker_accept(window, cx);
                cx.stop_propagation();
            }
            "backspace" => {
                self.wiki_link_picker_delete_backward(cx);
                cx.stop_propagation();
            }
            "delete" => {
                self.wiki_link_picker_delete_forward(cx);
                cx.stop_propagation();
            }
            "up" => {
                self.wiki_link_picker_apply_arrow(-1, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.wiki_link_picker_apply_arrow(1, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(crate) fn on_wiki_link_picker_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.wiki_link_picker.focus_handle);
        let text_len = self.wiki_link_picker.input.query.len();
        let offset = self.wiki_link_picker_index_for_mouse_position(event.position);
        let input = &mut self.wiki_link_picker.input;
        handle_mouse_down(
            event.modifiers.shift,
            offset,
            text_len,
            &mut input.selected_range,
            &mut input.selection_reversed,
            &mut input.marked_range,
            &mut input.is_selecting,
        );
        cx.notify();
    }

    pub(super) fn wiki_link_picker_prepare_context_menu(&mut self, position: Point<Pixels>) {
        let offset = self.wiki_link_picker_index_for_mouse_position(position);
        let input = &mut self.wiki_link_picker.input;
        super::single_line_input::prepare_context_menu_selection(
            &mut input.selected_range,
            &mut input.selection_reversed,
            &mut input.marked_range,
            &mut input.is_selecting,
            offset,
            input.query.len(),
        );
    }

    pub(crate) fn on_wiki_link_picker_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handle_mouse_up(&mut self.wiki_link_picker.input.is_selecting) {
            cx.notify();
        }
    }

    pub(crate) fn on_wiki_link_picker_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        let text_len = self.wiki_link_picker.input.query.len();
        let offset = self.wiki_link_picker_index_for_mouse_position(event.position);
        if handle_mouse_move(
            event.dragging(),
            offset,
            text_len,
            self.wiki_link_picker.input.is_selecting,
            &mut self.wiki_link_picker.input.selected_range,
            &mut self.wiki_link_picker.input.selection_reversed,
            &mut self.wiki_link_picker.input.marked_range,
            &mut self.wiki_link_picker.input.is_selecting,
        ) {
            cx.notify();
        }
    }

    pub(crate) fn on_wiki_link_picker_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_delete_backward(cx);
    }

    pub(crate) fn on_wiki_link_picker_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_delete_forward(cx);
    }

    pub(crate) fn on_wiki_link_picker_paste(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_paste_from_clipboard(cx);
    }

    pub(crate) fn on_wiki_link_picker_copy(
        &mut self,
        _: &Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_copy_to_clipboard(cx);
    }

    pub(crate) fn on_wiki_link_picker_cut(
        &mut self,
        _: &Cut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_cut_to_clipboard(cx);
    }

    pub(crate) fn on_wiki_link_picker_select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_select_all_text(cx);
    }

    pub(crate) fn on_wiki_link_picker_move_left(
        &mut self,
        _: &MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        if self.wiki_link_picker.input.selected_range.is_empty() {
            let previous = text_grapheme_boundary(
                &self.wiki_link_picker.input.query,
                self.wiki_link_picker_cursor_offset(),
                true,
            );
            self.wiki_link_picker_move_to(previous, cx);
        } else {
            self.wiki_link_picker_move_to(self.wiki_link_picker.input.selected_range.start, cx);
        }
    }

    pub(crate) fn on_wiki_link_picker_move_right(
        &mut self,
        _: &MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        if self.wiki_link_picker.input.selected_range.is_empty() {
            let next = text_grapheme_boundary(
                &self.wiki_link_picker.input.query,
                self.wiki_link_picker_cursor_offset(),
                false,
            );
            self.wiki_link_picker_move_to(next, cx);
        } else {
            self.wiki_link_picker_move_to(self.wiki_link_picker.input.selected_range.end, cx);
        }
    }

    pub(crate) fn on_wiki_link_picker_home(
        &mut self,
        _: &Home,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_move_to(0, cx);
    }

    pub(crate) fn on_wiki_link_picker_end(
        &mut self,
        _: &End,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_move_to(self.wiki_link_picker.input.query.len(), cx);
    }

    pub(crate) fn on_wiki_link_picker_select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_select_to(
            text_grapheme_boundary(
                &self.wiki_link_picker.input.query,
                self.wiki_link_picker_cursor_offset(),
                true,
            ),
            cx,
        );
    }

    pub(crate) fn on_wiki_link_picker_select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_select_to(
            text_grapheme_boundary(
                &self.wiki_link_picker.input.query,
                self.wiki_link_picker_cursor_offset(),
                false,
            ),
            cx,
        );
    }

    pub(crate) fn on_wiki_link_picker_select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_select_to(0, cx);
    }

    pub(crate) fn on_wiki_link_picker_select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.wiki_link_picker_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.wiki_link_picker_select_to(self.wiki_link_picker.input.query.len(), cx);
    }

    pub(super) fn render_wiki_link_picker_overlay(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.wiki_link_picker.open {
            return None;
        }

        let block = self
            .wiki_link_picker
            .block_entity_id
            .and_then(|id| self.document.block_entity_by_id(id))?;
        let bounds = block
            .read(cx)
            .last_bounds
            .or(block.read(cx).interaction_bounds)?;

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let anchor_left = f32::from(bounds.left());
        let anchor_bottom = f32::from(bounds.bottom());
        let picker = &self.wiki_link_picker;
        let editor = cx.entity().downgrade();
        let focus_handle = picker.focus_handle.clone();

        let row_count = if picker.filtering {
            picker.filter_results.len()
        } else {
            picker.visible_rows.len()
        };
        let list_height = (row_count.min(MAX_VISIBLE_ROWS) as f32 * ROW_HEIGHT).max(ROW_HEIGHT);
        let panel_height = INPUT_HEIGHT + 8.0 + list_height + d.menu_panel_padding * 2.0;

        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);

        let below_y = anchor_bottom + 4.0;
        let above_y = f32::from(bounds.top()) - panel_height - 4.0;
        let space_below = viewport_height - below_y - VIEWPORT_MARGIN;
        let open_upward = space_below < panel_height && above_y > space_below;

        let mut panel_y = if open_upward { above_y } else { below_y };
        panel_y = panel_y
            .max(VIEWPORT_MARGIN)
            .min((viewport_height - panel_height - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN));

        let mut panel_x = anchor_left;
        panel_x = panel_x
            .max(VIEWPORT_MARGIN)
            .min((viewport_width - PANEL_WIDTH - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN));

        let mut rows = Vec::new();
        if picker.filtering {
            let end = (picker.scroll_top + MAX_VISIBLE_ROWS).min(picker.filter_results.len());
            for index in picker.scroll_top..end {
                let result = &picker.filter_results[index];
                let selected = index == picker.selection;
                rows.push(render_file_row(
                    index,
                    selected,
                    0,
                    result.label.clone(),
                    result.detail.clone(),
                    result.label.clone(),
                    editor.clone(),
                    c,
                    t,
                ));
            }
        } else {
            let mut file_index = 0usize;
            let end = (picker.scroll_top + MAX_VISIBLE_ROWS).min(picker.visible_rows.len());
            for (index, row) in picker.visible_rows[picker.scroll_top..end]
                .iter()
                .enumerate()
            {
                let row_index = picker.scroll_top + index;
                let selected = row_index == picker.selection;
                match row {
                    FileTreeRow::Directory {
                        depth,
                        node_id,
                        label,
                        expanded,
                    } => {
                        let chevron = if *expanded { "▾" } else { "▸" };
                        let node_id = node_id.clone();
                        let toggle_editor = editor.clone();
                        rows.push(
                            div()
                                .id(("wiki-link-dir", row_index))
                                .h(px(ROW_HEIGHT))
                                .w_full()
                                .pl(px(8.0 + *depth as f32 * 12.0))
                                .pr(px(8.0))
                                .flex()
                                .items_center()
                                .gap(px(4.0))
                                .rounded(px(4.0))
                                .bg(if selected {
                                    c.selection.opacity(0.35)
                                } else {
                                    c.dialog_surface
                                })
                                .cursor_pointer()
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .child(
                                    div()
                                        .text_size(px(t.text_size * 0.82))
                                        .text_color(c.dialog_muted)
                                        .child(chevron),
                                )
                                .child(
                                    div()
                                        .text_size(px(t.text_size * 0.88))
                                        .text_color(c.dialog_muted)
                                        .child(label.clone()),
                                )
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let node_id = node_id.clone();
                                    let row_index = row_index;
                                    let _ = toggle_editor.update(cx, |editor, cx| {
                                        editor.wiki_link_picker_select_row(row_index, cx);
                                        editor.wiki_link_picker_toggle_directory(&node_id, cx);
                                    });
                                    cx.stop_propagation();
                                })
                                .into_any_element(),
                        );
                    }
                    FileTreeRow::File {
                        depth,
                        relative_path,
                        label,
                        detail,
                    } => {
                        rows.push(render_file_row(
                            row_index,
                            selected,
                            *depth,
                            label.clone(),
                            detail.clone(),
                            relative_path.clone(),
                            editor.clone(),
                            c,
                            t,
                        ));
                        file_index += 1;
                    }
                }
            }
            let _ = file_index;
        }

        let placeholder: SharedString = if picker.input.is_empty() {
            strings.quick_file_open_placeholder.clone().into()
        } else if picker.filtering && picker.filter_results.is_empty() {
            strings.workspace_search_no_results.clone().into()
        } else {
            SharedString::default()
        };

        Some(
            div()
                .id("wiki-link-picker-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_wiki_link_picker_overlay),
                )
                .child(
                    div()
                        .id("wiki-link-picker-panel")
                        .absolute()
                        .left(px(panel_x))
                        .top(px(panel_y))
                        .w(px(PANEL_WIDTH))
                        .p(px(d.menu_panel_padding))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                            cx.stop_propagation();
                        })
                        .child(
                            div()
                                .id("wiki-link-picker-input")
                                .h(px(INPUT_HEIGHT))
                                .w_full()
                                .px(px(10.0))
                                .flex()
                                .items_center()
                                .rounded(px(6.0))
                                .bg(c.dialog_secondary_button_bg)
                                .border(px(1.0))
                                .border_color(c.dialog_border)
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation();
                                })
                                .track_focus(&focus_handle)
                                .key_context("BlockEditor")
                                .on_key_down(cx.listener(Self::on_wiki_link_picker_key_down))
                                .on_action(cx.listener(Self::on_wiki_link_picker_delete_back))
                                .on_action(cx.listener(Self::on_wiki_link_picker_delete_forward))
                                .on_action(cx.listener(Self::on_wiki_link_picker_paste))
                                .on_action(cx.listener(Self::on_wiki_link_picker_copy))
                                .on_action(cx.listener(Self::on_wiki_link_picker_cut))
                                .on_action(cx.listener(Self::on_wiki_link_picker_select_all))
                                .on_action(cx.listener(Self::on_wiki_link_picker_move_left))
                                .on_action(cx.listener(Self::on_wiki_link_picker_move_right))
                                .on_action(cx.listener(Self::on_wiki_link_picker_home))
                                .on_action(cx.listener(Self::on_wiki_link_picker_end))
                                .on_action(cx.listener(Self::on_wiki_link_picker_select_left))
                                .on_action(cx.listener(Self::on_wiki_link_picker_select_right))
                                .on_action(cx.listener(Self::on_wiki_link_picker_select_home))
                                .on_action(cx.listener(Self::on_wiki_link_picker_select_end))
                                .child(SingleLineInputElement::new(
                                    cx.entity().clone(),
                                    SingleLineInputTarget::WikiLinkPicker,
                                    placeholder,
                                )),
                        )
                        .child(
                            div()
                                .id("wiki-link-picker-results")
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .children(rows),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn render_file_row(
    row_index: usize,
    selected: bool,
    depth: usize,
    label: String,
    detail: String,
    relative_path: String,
    editor: WeakEntity<Editor>,
    c: &ThemeColors,
    t: &ThemeTypography,
) -> AnyElement {
    div()
        .id(("wiki-link-file", row_index))
        .h(px(ROW_HEIGHT))
        .w_full()
        .pl(px(8.0 + depth as f32 * 12.0))
        .pr(px(8.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(4.0))
        .bg(if selected {
            c.selection.opacity(0.35)
        } else {
            c.dialog_surface
        })
        .cursor_pointer()
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(t.text_size * 0.88))
                .text_color(c.text_default)
                .child(label),
        )
        .when(!detail.is_empty(), |this| {
            this.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(t.text_size * 0.72))
                    .text_color(c.dialog_muted)
                    .child(detail),
            )
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            let path = relative_path.clone();
            let index = row_index;
            let _ = editor.update(cx, |editor, cx| {
                editor.wiki_link_picker_select_file(index, &path, window, cx);
            });
            cx.stop_propagation();
        })
        .into_any_element()
}
