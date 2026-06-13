//! Search within the current document: highlight all matches and F3 navigation.

use std::collections::HashMap;
use std::ops::Range;

use gpui::*;

use super::single_line_input::{
    SingleLineInputTarget, handle_mouse_down, handle_mouse_move, handle_mouse_up,
    index_for_mouse_position, move_caret_to, primary_shortcut_modifiers, select_caret_to,
    text_grapheme_boundary,
};
use super::single_line_input_element::SingleLineInputElement;
use super::search_match::find_case_insensitive_ranges;
use super::toolbar_button::toolbar_icon_button;
use super::Editor;
use super::ViewMode;
use crate::components::{
    Copy, Cut, Delete, DeleteBack, End, FindNextInDocument, FindPreviousInDocument, Home,
    MoveLeft, MoveRight, Paste, SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight,
    ToggleDocumentSearch,
};
use crate::i18n::I18nManager;
use crate::input::single_line_field::SingleLineFieldState;
use crate::theme::Theme;

const ICON_SEARCH: &str = "icon/toolbar/search.svg";
const ICON_CHEVRON_DOWN: &str = "icon/workspace/chevron-down.svg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DocumentSearchMatch {
    entity_id: EntityId,
    range: Range<usize>,
}

#[derive(Default)]
pub(super) struct DocumentSearchState {
    pub open: bool,
    pub input: SingleLineFieldState,
    pub matches: Vec<DocumentSearchMatch>,
    pub match_index: Option<usize>,
    pub scroll_entity_id: Option<EntityId>,
}

impl Editor {
    pub(super) fn document_search_input_active(&self, window: &Window) -> bool {
        self.search.state.open && self.search.focus.is_focused(window)
    }

    pub(super) fn document_search_query_is_empty(&self) -> bool {
        self.search.state.input.query.is_empty()
    }

    pub(super) fn document_search_display_text(&self, placeholder: &SharedString) -> SharedString {
        if self.search.state.input.query.is_empty() {
            placeholder.clone()
        } else {
            self.search.state.input.query.clone().into()
        }
    }

    pub(super) fn document_search_marked_range(&self) -> Option<Range<usize>> {
        self.search.state.input.marked_range.clone()
    }

    pub(super) fn document_search_selected_range(&self) -> Range<usize> {
        self.search.state.input.selected_range.clone()
    }

    pub(super) fn document_search_cursor_offset(&self) -> usize {
        self.search.state.input.cursor_offset()
    }

    pub(super) fn set_document_search_layout(
        &mut self,
        line: ShapedLine,
        bounds: Bounds<Pixels>,
    ) {
        self.search.state.input.set_layout(line, bounds);
    }

    pub(super) fn on_toggle_document_search(
        &mut self,
        _: &ToggleDocumentSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_document_search(window, cx);
    }

    pub(super) fn on_toggle_document_search_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_document_search(window, cx);
    }

    pub(super) fn on_find_next_in_document(
        &mut self,
        _: &FindNextInDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_next_document_match(cx);
    }

    pub(super) fn on_find_previous_in_document(
        &mut self,
        _: &FindPreviousInDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find_previous_document_match(cx);
    }

    pub(super) fn toggle_document_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.state.open = !self.search.state.open;
        if self.search.state.open {
            self.sync_document_search_selection();
            window.focus(&self.search.focus);
            self.run_document_search(cx);
        } else {
            self.close_document_search(cx);
        }
        cx.notify();
    }

    pub(super) fn close_document_search(&mut self, cx: &mut Context<Self>) {
        if !self.search.state.open && self.search.state.input.query.is_empty() {
            self.clear_document_search_highlights(cx);
            return;
        }
        self.search.state.open = false;
        self.search.state.input.clear();
        self.search.state.matches.clear();
        self.search.state.match_index = None;
        self.search.state.scroll_entity_id = None;
        self.clear_document_search_highlights(cx);
        self.close_single_line_input_context_menu(cx);
        cx.notify();
    }

    fn sync_document_search_selection(&mut self) {
        self.search.state.input.sync_caret_to_end();
    }

    pub(super) fn run_document_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.state.input.query.trim();
        if query.is_empty() {
            self.search.state.matches.clear();
            self.search.state.match_index = None;
            self.clear_document_search_highlights(cx);
            cx.notify();
            return;
        }

        self.search.match_source_range = None;
        self.collect_document_search_matches(cx);
        self.search.state.match_index = if self.search.state.matches.is_empty() {
            None
        } else {
            Some(0)
        };
        self.apply_document_search_highlights(cx);
        if self.search.state.match_index.is_some() {
            self.jump_to_document_search_match(self.search.state.match_index, cx);
        } else {
            cx.notify();
        }
    }

    pub(super) fn find_next_document_match(&mut self, cx: &mut Context<Self>) {
        if self.search.state.matches.is_empty() {
            if self.search.state.open && !self.search.state.input.query.trim().is_empty() {
                self.run_document_search(cx);
            }
            return;
        }
        let len = self.search.state.matches.len();
        let next = self
            .search
            .state
            .match_index
            .map(|index| (index + 1) % len)
            .unwrap_or(0);
        self.jump_to_document_search_match(Some(next), cx);
    }

    pub(super) fn find_previous_document_match(&mut self, cx: &mut Context<Self>) {
        if self.search.state.matches.is_empty() {
            return;
        }
        let len = self.search.state.matches.len();
        let previous = self
            .search
            .state
            .match_index
            .map(|index| (index + len - 1) % len)
            .unwrap_or(len - 1);
        self.jump_to_document_search_match(Some(previous), cx);
    }

    fn jump_to_document_search_match(
        &mut self,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = index else {
            return;
        };
        let Some(match_record) = self.search.state.matches.get(index).cloned() else {
            return;
        };

        self.search.state.match_index = Some(index);
        self.apply_document_search_highlights(cx);
        self.scroll_document_search_match_into_view(match_record.entity_id, cx);
        cx.notify();
    }

    fn scroll_document_search_match_into_view(
        &mut self,
        entity_id: EntityId,
        cx: &App,
    ) {
        if self.view_mode != ViewMode::Source {
            if let Some(idx) = self.document.visible_index_for_entity_id(entity_id) {
                self.scroll_handle.scroll_to_item(idx);
            }
        }

        let Some(block_entity) = self.document.block_entity_by_id(entity_id) else {
            self.search.state.scroll_entity_id = Some(entity_id);
            self.pending_scroll_recheck_after_layout = true;
            return;
        };

        let Some(bounds) = block_entity.read_with(cx, |block, _| block.last_bounds) else {
            self.search.state.scroll_entity_id = Some(entity_id);
            self.pending_scroll_recheck_after_layout = true;
            return;
        };

        self.search.state.scroll_entity_id = None;
        let viewport = self.scroll_handle.bounds();
        let padding = px(20.0);
        let top_limit = viewport.top() + padding;
        let bottom_limit = viewport.bottom() - padding;
        let mut offset = self.scroll_handle.offset();
        let mut changed = false;

        if bounds.top() < top_limit {
            offset.y += top_limit - bounds.top();
            changed = true;
        } else if bounds.bottom() > bottom_limit {
            offset.y -= bounds.bottom() - bottom_limit;
            changed = true;
        }

        if changed {
            let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            self.scroll_handle.set_offset(offset);
        }
    }

    pub(super) fn retry_document_search_scroll(&mut self, cx: &App) {
        let Some(entity_id) = self.search.state.scroll_entity_id else {
            return;
        };
        self.scroll_document_search_match_into_view(entity_id, cx);
    }

    pub(super) fn clear_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, cx| {
                if !block.search_highlight_ranges.is_empty()
                    || block.search_highlight_active_range.is_some()
                {
                    block.search_highlight_ranges.clear();
                    block.search_highlight_active_range = None;
                    changed = true;
                    cx.notify();
                }
            });
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn refresh_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        if !self.search.state.open || self.search.state.input.query.trim().is_empty() {
            return;
        }
        let current_match = self
            .search
            .state
            .match_index
            .and_then(|index| self.search.state.matches.get(index).cloned());
        self.collect_document_search_matches(cx);
        self.search.state.match_index = if let Some(current) = current_match {
            self.search
                .state
                .matches
                .iter()
                .position(|candidate| {
                    candidate.entity_id == current.entity_id && candidate.range == current.range
                })
                .or_else(|| {
                    self.search.state.matches.iter().position(|candidate| {
                        candidate.entity_id == current.entity_id
                            && candidate.range.start >= current.range.start
                    })
                })
        } else if self.search.state.matches.is_empty() {
            None
        } else {
            Some(0)
        };
        self.apply_document_search_highlights(cx);
        if let Some(index) = self.search.state.match_index {
            self.jump_to_document_search_match(Some(index), cx);
        } else {
            cx.notify();
        }
    }

    fn collect_document_search_matches(&mut self, cx: &App) {
        let query = self.search.state.input.query.trim();
        let mut matches = Vec::new();
        for visible in self.document.visible_blocks() {
            let entity_id = visible.entity.entity_id();
            let text = visible.entity.read(cx).display_text();
            for range in find_case_insensitive_ranges(text, query) {
                matches.push(DocumentSearchMatch { entity_id, range });
            }
        }
        self.search.state.matches = matches;
    }

    fn apply_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        let query = self.search.state.input.query.trim();
        if query.is_empty() {
            self.clear_document_search_highlights(cx);
            return;
        }

        let active_match = self
            .search
            .state
            .match_index
            .and_then(|index| self.search.state.matches.get(index).cloned());
        let mut by_block: HashMap<EntityId, Vec<Range<usize>>> = HashMap::new();
        for match_record in &self.search.state.matches {
            by_block
                .entry(match_record.entity_id)
                .or_default()
                .push(match_record.range.clone());
        }

        for visible in self.document.visible_blocks().to_vec() {
            let entity_id = visible.entity.entity_id();
            let ranges = by_block.remove(&entity_id).unwrap_or_default();
            let active = active_match
                .as_ref()
                .filter(|record| record.entity_id == entity_id)
                .map(|record| record.range.clone());
            visible.entity.update(cx, |block, cx| {
                if block.search_highlight_ranges != ranges
                    || block.search_highlight_active_range != active
                {
                    block.search_highlight_ranges = ranges;
                    block.search_highlight_active_range = active;
                    cx.notify();
                }
            });
        }
    }

    pub(super) fn render_document_search_bar(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.search.state.open {
            return None;
        }

        let c = &theme.colors;
        let strings = cx.global::<I18nManager>().strings_arc();
        let placeholder: SharedString = strings.document_search_placeholder.clone().into();
        let match_count = self.search.state.matches.len();
        let next_enabled = match_count > 0;
        let current = self
            .search
            .state
            .match_index
            .map(|index| index + 1)
            .unwrap_or(0);
        let status = if self.search.state.input.query.trim().is_empty() {
            strings.document_search_status_empty.clone()
        } else if match_count == 0 {
            strings.document_search_no_matches.clone()
        } else {
            strings
                .document_search_status
                .replace("{current}", &current.to_string())
                .replace("{total}", &match_count.to_string())
        };
        let editor_for_next = cx.entity();

        Some(
            div()
                .id("document-search-bar")
                .w_full()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(theme.dimensions.format_toolbar_padding_x))
                .py(px(6.0))
                .bg(c.dialog_surface)
                .border_b(px(theme.dimensions.format_toolbar_border_width))
                .border_color(c.dialog_border.opacity(0.65))
                .track_focus(&self.search.focus)
                .key_context("BlockEditor")
                .on_key_down(cx.listener(Self::on_document_search_key_down))
                .on_action(cx.listener(Self::on_document_search_delete_back))
                .on_action(cx.listener(Self::on_document_search_delete_forward))
                .on_action(cx.listener(Self::on_document_search_paste))
                .on_action(cx.listener(Self::on_document_search_copy))
                .on_action(cx.listener(Self::on_document_search_cut))
                .on_action(cx.listener(Self::on_document_search_select_all))
                .on_action(cx.listener(Self::on_document_search_move_left))
                .on_action(cx.listener(Self::on_document_search_move_right))
                .on_action(cx.listener(Self::on_document_search_home))
                .on_action(cx.listener(Self::on_document_search_end))
                .on_action(cx.listener(Self::on_document_search_select_left))
                .on_action(cx.listener(Self::on_document_search_select_right))
                .on_action(cx.listener(Self::on_document_search_select_home))
                .on_action(cx.listener(Self::on_document_search_select_end))
                .child(
                    div()
                        .id("document-search-input")
                        .h(px(28.0))
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .rounded(px(theme.dimensions.format_toolbar_button_radius))
                        .border(px(1.0))
                        .border_color(c.dialog_border.opacity(0.75))
                        .bg(c.editor_background)
                        .child(
                            svg()
                                .path(ICON_SEARCH)
                                .size(px(theme.dimensions.format_toolbar_icon_size))
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
                                    SingleLineInputTarget::DocumentSearch,
                                    placeholder,
                                )),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(theme.typography.text_size * 0.82))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(status)),
                )
                .child({
                    let mut button = toolbar_icon_button(
                        "document-search-next",
                        theme,
                        ICON_CHEVRON_DOWN,
                        false,
                        !next_enabled,
                        "",
                        true,
                    );
                    if next_enabled {
                        button = button.on_mouse_down(
                            MouseButton::Left,
                            move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = editor_for_next.update(cx, |editor, cx| {
                                    editor.find_next_document_match(cx);
                                });
                            },
                        );
                    }
                    button
                })
                .into_any_element(),
        )
    }

    pub(crate) fn on_document_search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search.state.open || !self.document_search_input_active(window) {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if primary_shortcut_modifiers(&modifiers) {
            match event.keystroke.key.as_str() {
                "v" => {
                    self.document_search_paste_from_clipboard(cx);
                    cx.stop_propagation();
                }
                "c" => {
                    self.document_search_copy_to_clipboard(cx);
                    cx.stop_propagation();
                }
                "x" => {
                    self.document_search_cut_to_clipboard(cx);
                    cx.stop_propagation();
                }
                "a" => {
                    self.document_search_select_all_text(cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.close_document_search(cx);
            }
            "enter" => {
                cx.stop_propagation();
                self.find_next_document_match(cx);
            }
            "backspace" => {
                cx.stop_propagation();
                self.document_search_delete_backward(cx);
            }
            "delete" => {
                cx.stop_propagation();
                self.document_search_delete_forward(cx);
            }
            _ => {}
        }
    }

    pub(crate) fn on_document_search_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.search.focus);
        let text_len = self.search.state.input.query.len();
        let offset = self.document_search_index_for_mouse_position(event.position);
        let input = &mut self.search.state.input;
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

    pub(super) fn document_search_prepare_context_menu(
        &mut self,
        position: Point<Pixels>,
    ) {
        let offset = self.document_search_index_for_mouse_position(position);
        let input = &mut self.search.state.input;
        super::single_line_input::prepare_context_menu_selection(
            &mut input.selected_range,
            &mut input.selection_reversed,
            &mut input.marked_range,
            &mut input.is_selecting,
            offset,
            input.query.len(),
        );
    }

    pub(crate) fn on_document_search_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handle_mouse_up(&mut self.search.state.input.is_selecting) {
            cx.notify();
        }
    }

    pub(crate) fn on_document_search_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        let text_len = self.search.state.input.query.len();
        let offset = self.document_search_index_for_mouse_position(event.position);
        if handle_mouse_move(
            event.dragging(),
            offset,
            text_len,
            self.search.state.input.is_selecting,
            &mut self.search.state.input.selected_range,
            &mut self.search.state.input.selection_reversed,
            &mut self.search.state.input.marked_range,
            &mut self.search.state.input.is_selecting,
        ) {
            cx.notify();
        }
    }

    pub(super) fn replace_document_search_text(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        marked: bool,
        selected_range: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let query = &mut self.search.state.input.query;
        let start = range.start.min(query.len());
        let end = range.end.min(query.len());
        query.replace_range(start..end, replacement);
        self.search.state.input.marked_range = marked.then(|| {
            let marked_start = start;
            let marked_end = start + replacement.len();
            marked_start..marked_end
        });
        if let Some(selected_range) = selected_range {
            self.search.state.input.selected_range = selected_range;
        } else {
            let cursor = start + replacement.len();
            self.search.state.input.selected_range = cursor..cursor;
        }
        self.search.state.input.selection_reversed = false;
        self.run_document_search(cx);
    }

    pub(super) fn document_search_index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        index_for_mouse_position(
            self.search.state.input.query.len(),
            self.search.state.input.last_bounds.as_ref(),
            self.search.state.input.last_layout.as_ref(),
            position,
        )
    }

    fn document_search_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        move_caret_to(
            &mut self.search.state.input.selected_range,
            &mut self.search.state.input.selection_reversed,
            &mut self.search.state.input.marked_range,
            &mut self.search.state.input.is_selecting,
            offset,
            self.search.state.input.query.len(),
        );
        cx.notify();
    }

    fn document_search_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        select_caret_to(
            &mut self.search.state.input.selected_range,
            &mut self.search.state.input.selection_reversed,
            &mut self.search.state.input.marked_range,
            offset,
            self.search.state.input.query.len(),
        );
        cx.notify();
    }

    fn document_search_delete_backward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.search.state.input.marked_range.clone() {
            let cursor = marked.start;
            self.replace_document_search_text(marked, "", false, Some(cursor..cursor), cx);
            return;
        }

        let selected = self.search.state.input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor == 0 {
                return;
            }
            let previous =
                text_grapheme_boundary(&self.search.state.input.query, cursor, true);
            previous..cursor
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_document_search_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn document_search_delete_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(marked) = self.search.state.input.marked_range.clone() {
            let cursor = marked.start;
            self.replace_document_search_text(marked, "", false, Some(cursor..cursor), cx);
            return;
        }

        let query_len = self.search.state.input.query.len();
        let selected = self.search.state.input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor >= query_len {
                return;
            }
            let next = text_grapheme_boundary(&self.search.state.input.query, cursor, false);
            cursor..next
        } else {
            selected
        };

        let cursor = delete_range.start;
        self.replace_document_search_text(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn document_search_replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = if self.search.state.input.selected_range.is_empty() {
            let cursor = self.document_search_cursor_offset();
            cursor..cursor
        } else {
            self.search.state.input.selected_range.clone()
        };
        let cursor = range.start + text.len();
        self.replace_document_search_text(range, text, false, Some(cursor..cursor), cx);
    }

    pub(super) fn document_search_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = super::single_line_input::sanitize_pasted_text(&text);
            self.document_search_replace_selection(&text, cx);
        }
    }

    pub(super) fn document_search_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.search.state.input.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.search.state.input.query
                    [self.search.state.input.selected_range.clone()]
                    .to_string(),
            ));
        }
    }

    pub(super) fn document_search_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        self.document_search_copy_to_clipboard(cx);
        if !self.search.state.input.selected_range.is_empty() {
            self.document_search_replace_selection("", cx);
        }
    }

    fn document_search_select_all_text(&mut self, cx: &mut Context<Self>) {
        self.document_search_move_to(0, cx);
        self.document_search_select_to(self.search.state.input.query.len(), cx);
    }

    pub(crate) fn on_document_search_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_delete_backward(cx);
    }

    pub(crate) fn on_document_search_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_delete_forward(cx);
    }

    pub(crate) fn on_document_search_paste(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_paste_from_clipboard(cx);
    }

    pub(crate) fn on_document_search_copy(
        &mut self,
        _: &Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_copy_to_clipboard(cx);
    }

    pub(crate) fn on_document_search_cut(
        &mut self,
        _: &Cut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_cut_to_clipboard(cx);
    }

    pub(crate) fn on_document_search_select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_select_all_text(cx);
    }

    pub(crate) fn on_document_search_move_left(
        &mut self,
        _: &MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        if self.search.state.input.selected_range.is_empty() {
            let previous = text_grapheme_boundary(
                &self.search.state.input.query,
                self.document_search_cursor_offset(),
                true,
            );
            self.document_search_move_to(previous, cx);
        } else {
            self.document_search_move_to(self.search.state.input.selected_range.start, cx);
        }
    }

    pub(crate) fn on_document_search_move_right(
        &mut self,
        _: &MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        if self.search.state.input.selected_range.is_empty() {
            let next = text_grapheme_boundary(
                &self.search.state.input.query,
                self.document_search_cursor_offset(),
                false,
            );
            self.document_search_move_to(next, cx);
        } else {
            self.document_search_move_to(self.search.state.input.selected_range.end, cx);
        }
    }

    pub(crate) fn on_document_search_home(
        &mut self,
        _: &Home,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_move_to(0, cx);
    }

    pub(crate) fn on_document_search_end(
        &mut self,
        _: &End,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_move_to(self.search.state.input.query.len(), cx);
    }

    pub(crate) fn on_document_search_select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_select_to(
            text_grapheme_boundary(
                &self.search.state.input.query,
                self.document_search_cursor_offset(),
                true,
            ),
            cx,
        );
    }

    pub(crate) fn on_document_search_select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_select_to(
            text_grapheme_boundary(
                &self.search.state.input.query,
                self.document_search_cursor_offset(),
                false,
            ),
            cx,
        );
    }

    pub(crate) fn on_document_search_select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_select_to(0, cx);
    }

    pub(crate) fn on_document_search_select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.document_search_select_to(self.search.state.input.query.len(), cx);
    }
}

pub(super) fn document_search_offset_to_utf16(text: &str, offset: usize) -> usize {
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

pub(super) fn document_search_range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    document_search_offset_to_utf16(text, range.start)
        ..document_search_offset_to_utf16(text, range.end)
}

pub(super) fn document_search_range_from_utf16(
    text: &str,
    range_utf16: &Range<usize>,
) -> Range<usize> {
    document_search_offset_from_utf16(text, range_utf16.start)
        ..document_search_offset_from_utf16(text, range_utf16.end)
}

fn document_search_offset_from_utf16(text: &str, target_utf16: usize) -> usize {
    let mut utf16_count = 0;
    for (byte_index, ch) in text.char_indices() {
        if utf16_count >= target_utf16 {
            return byte_index;
        }
        utf16_count += ch.len_utf16();
    }
    text.len()
}
