//! Search within the current document: highlight all matches and F3 navigation.

use std::collections::HashMap;
use std::ops::Range;
use std::time::Instant;

use gpui::*;
use unicode_segmentation::UnicodeSegmentation;

use super::document_search_input::DocumentSearchInputElement;
use super::search_match::find_case_insensitive_ranges;
use super::Editor;
use crate::components::{FindNextInDocument, FindPreviousInDocument, ToggleDocumentSearch};
use crate::i18n::I18nManager;
use crate::theme::Theme;

const ICON_SEARCH: &str = "icon/toolbar/search.svg";

#[derive(Default)]
pub(super) struct DocumentSearchState {
    pub open: bool,
    pub query: String,
    pub marked_range: Option<Range<usize>>,
    pub selected_range: Range<usize>,
    pub selection_reversed: bool,
    pub is_selecting: bool,
    pub last_layout: Option<ShapedLine>,
    pub last_bounds: Option<Bounds<Pixels>>,
    pub matches: Vec<Range<usize>>,
    pub match_index: Option<usize>,
}

impl Editor {
    pub(super) fn document_search_input_active(&self, window: &Window) -> bool {
        self.document_search.open && self.document_search_focus.is_focused(window)
    }

    pub(super) fn document_search_query_is_empty(&self) -> bool {
        self.document_search.query.is_empty()
    }

    pub(super) fn document_search_display_text(&self, placeholder: &SharedString) -> SharedString {
        if self.document_search.query.is_empty() {
            placeholder.clone()
        } else {
            self.document_search.query.clone().into()
        }
    }

    pub(super) fn document_search_marked_range(&self) -> Option<Range<usize>> {
        self.document_search.marked_range.clone()
    }

    pub(super) fn document_search_selected_range(&self) -> Range<usize> {
        self.document_search.selected_range.clone()
    }

    pub(super) fn document_search_cursor_offset(&self) -> usize {
        if self.document_search.selection_reversed {
            self.document_search.selected_range.start
        } else {
            self.document_search.selected_range.end
        }
    }

    pub(super) fn document_search_focus_handle(&self) -> FocusHandle {
        self.document_search_focus.clone()
    }

    pub(super) fn set_document_search_layout(
        &mut self,
        line: ShapedLine,
        bounds: Bounds<Pixels>,
    ) {
        self.document_search.last_layout = Some(line);
        self.document_search.last_bounds = Some(bounds);
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
        self.document_search.open = !self.document_search.open;
        if self.document_search.open {
            self.sync_document_search_selection();
            window.focus(&self.document_search_focus);
            self.run_document_search(cx);
        } else {
            self.close_document_search(cx);
        }
        cx.notify();
    }

    pub(super) fn close_document_search(&mut self, cx: &mut Context<Self>) {
        if !self.document_search.open && self.document_search.query.is_empty() {
            self.clear_document_search_highlights(cx);
            return;
        }
        self.document_search.open = false;
        self.document_search.query.clear();
        self.document_search.marked_range = None;
        self.document_search.selected_range = 0..0;
        self.document_search.matches.clear();
        self.document_search.match_index = None;
        self.clear_document_search_highlights(cx);
        cx.notify();
    }

    fn sync_document_search_selection(&mut self) {
        let len = self.document_search.query.len();
        self.document_search.selected_range = len..len;
        self.document_search.selection_reversed = false;
    }

    pub(super) fn run_document_search(&mut self, cx: &mut Context<Self>) {
        let query = self.document_search.query.trim();
        if query.is_empty() {
            self.document_search.matches.clear();
            self.document_search.match_index = None;
            self.clear_document_search_highlights(cx);
            cx.notify();
            return;
        }

        let source = self.current_document_source(cx);
        self.document_search.matches = find_case_insensitive_ranges(&source, query);
        self.document_search.match_index = if self.document_search.matches.is_empty() {
            None
        } else {
            Some(0)
        };
        self.search_match_source_range = None;
        self.sync_document_search_highlights(cx);
        if self.document_search.match_index.is_some() {
            self.jump_to_document_search_match(self.document_search.match_index, cx);
        }
        cx.notify();
    }

    pub(super) fn find_next_document_match(&mut self, cx: &mut Context<Self>) {
        if self.document_search.matches.is_empty() {
            if self.document_search.open && !self.document_search.query.trim().is_empty() {
                self.run_document_search(cx);
            }
            return;
        }
        let len = self.document_search.matches.len();
        let next = self
            .document_search
            .match_index
            .map(|index| (index + 1) % len)
            .unwrap_or(0);
        self.jump_to_document_search_match(Some(next), cx);
    }

    pub(super) fn find_previous_document_match(&mut self, cx: &mut Context<Self>) {
        if self.document_search.matches.is_empty() {
            return;
        }
        let len = self.document_search.matches.len();
        let previous = self
            .document_search
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
        let Some(source_range) = self.document_search.matches.get(index).cloned() else {
            return;
        };

        let mappings = self.build_source_target_mappings(cx);
        let Some(start) = self.endpoint_for_source_offset(source_range.start, &mappings, cx) else {
            return;
        };
        let Some(end) = self.endpoint_for_source_offset(source_range.end, &mappings, cx) else {
            return;
        };

        self.document_search.match_index = Some(index);

        if start.entity_id == end.entity_id {
            let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
                return;
            };
            let selection = start.offset.min(end.offset)..start.offset.max(end.offset);
            block.update(cx, |block, cx| {
                block.selected_range = selection.clone();
                block.selection_reversed = false;
                block.marked_range = None;
                block.vertical_motion_x = None;
                block.cursor_blink_epoch = Instant::now();
                cx.notify();
            });
            self.focus_block(start.entity_id);
        } else {
            let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
                return;
            };
            block.update(cx, |block, cx| {
                block.selected_range = start.offset..start.offset;
                block.selection_reversed = false;
                block.marked_range = None;
                block.vertical_motion_x = None;
                block.cursor_blink_epoch = Instant::now();
                cx.notify();
            });
            self.focus_block(start.entity_id);
        }

        self.search_match_source_range = Some(source_range);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    pub(super) fn clear_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, cx| {
                if !block.search_highlight_ranges.is_empty() {
                    block.search_highlight_ranges.clear();
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
        if !self.document_search.open || self.document_search.query.trim().is_empty() {
            return;
        }
        let current_source_range = self
            .document_search
            .match_index
            .and_then(|index| self.document_search.matches.get(index).cloned());
        let query = self.document_search.query.clone();
        let source = self.current_document_source(cx);
        self.document_search.matches = find_case_insensitive_ranges(&source, &query);
        self.document_search.match_index = if let Some(range) = current_source_range {
            self.document_search
                .matches
                .iter()
                .position(|candidate| *candidate == range)
                .or_else(|| {
                    self.document_search
                        .matches
                        .iter()
                        .position(|candidate| candidate.start >= range.start)
                })
        } else if self.document_search.matches.is_empty() {
            None
        } else {
            Some(0)
        };
        self.sync_document_search_highlights(cx);
        if let Some(index) = self.document_search.match_index {
            self.jump_to_document_search_match(Some(index), cx);
        } else {
            cx.notify();
        }
    }

    fn sync_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        let query = self.document_search.query.trim();
        if query.is_empty() {
            self.clear_document_search_highlights(cx);
            return;
        }

        let source = self.current_document_source(cx);
        let matches = find_case_insensitive_ranges(&source, query);
        self.document_search.matches = matches.clone();

        let mappings = self.build_source_target_mappings(cx);
        let mut by_block: HashMap<EntityId, Vec<Range<usize>>> = HashMap::new();

        for match_range in matches {
            let Some(start) = self.endpoint_for_source_offset(match_range.start, &mappings, cx) else {
                continue;
            };
            let Some(end) = self.endpoint_for_source_offset(match_range.end, &mappings, cx) else {
                continue;
            };
            if start.entity_id != end.entity_id {
                continue;
            }
            by_block
                .entry(start.entity_id)
                .or_default()
                .push(start.offset.min(end.offset)..start.offset.max(end.offset));
        }

        for visible in self.document.visible_blocks().to_vec() {
            let entity_id = visible.entity.entity_id();
            let ranges = by_block.remove(&entity_id).unwrap_or_default();
            visible.entity.update(cx, |block, cx| {
                if block.search_highlight_ranges != ranges {
                    block.search_highlight_ranges = ranges;
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
        if !self.document_search.open {
            return None;
        }

        let c = &theme.colors;
        let strings = cx.global::<I18nManager>().strings_arc();
        let placeholder: SharedString = strings.document_search_placeholder.clone().into();
        let match_count = self.document_search.matches.len();
        let current = self
            .document_search
            .match_index
            .map(|index| index + 1)
            .unwrap_or(0);
        let status = if self.document_search.query.trim().is_empty() {
            strings.document_search_status_empty.clone()
        } else if match_count == 0 {
            strings.document_search_no_matches.clone()
        } else {
            strings
                .document_search_status
                .replace("{current}", &current.to_string())
                .replace("{total}", &match_count.to_string())
        };

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
                                .child(DocumentSearchInputElement::new(
                                    cx.entity(),
                                    placeholder,
                                ))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_document_search_mouse_down),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(Self::on_document_search_mouse_up),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(Self::on_document_search_mouse_up),
                                )
                                .on_mouse_move(cx.listener(Self::on_document_search_mouse_move)),
                        ),
                )
                .on_key_down(cx.listener(Self::on_document_search_key_down))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(theme.typography.text_size * 0.82))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(status)),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn on_document_search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search.open || !self.document_search_input_active(window) {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if document_search_primary_shortcut_modifiers(modifiers) {
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
        window.focus(&self.document_search_focus);
        let offset = self.document_search_index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.document_search_select_to(offset, cx);
        } else {
            self.document_search_move_to(offset, cx);
        }
    }

    pub(crate) fn on_document_search_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.document_search.is_selecting = false;
        cx.notify();
    }

    pub(crate) fn on_document_search_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.document_search.is_selecting || !self.document_search_input_active(window) {
            return;
        }
        self.document_search_select_to(
            self.document_search_index_for_mouse_position(event.position),
            cx,
        );
    }

    pub(super) fn replace_document_search_text(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        marked: bool,
        selected_range: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let query = &mut self.document_search.query;
        let start = range.start.min(query.len());
        let end = range.end.min(query.len());
        query.replace_range(start..end, replacement);
        self.document_search.marked_range = marked.then(|| {
            let marked_start = start;
            let marked_end = start + replacement.len();
            marked_start..marked_end
        });
        if let Some(selected_range) = selected_range {
            self.document_search.selected_range = selected_range;
        } else {
            let cursor = start + replacement.len();
            self.document_search.selected_range = cursor..cursor;
        }
        self.document_search.selection_reversed = false;
        self.run_document_search(cx);
    }

    fn document_search_index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let query = &self.document_search.query;
        if query.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (
            self.document_search.last_bounds.as_ref(),
            self.document_search.last_layout.as_ref(),
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
    }

    fn document_search_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.document_search.query.len());
        self.document_search.selected_range = clamped..clamped;
        self.document_search.selection_reversed = false;
        cx.notify();
    }

    fn document_search_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let clamped = offset.min(self.document_search.query.len());
        if self.document_search.selection_reversed {
            self.document_search.selected_range.start = clamped;
        } else {
            self.document_search.selected_range.end = clamped;
        }
        if self.document_search.selected_range.end < self.document_search.selected_range.start {
            self.document_search.selection_reversed =
                !self.document_search.selection_reversed;
            self.document_search.selected_range = self.document_search.selected_range.end
                ..self.document_search.selected_range.start;
        }
        cx.notify();
    }

    fn document_search_delete_backward(&mut self, cx: &mut Context<Self>) {
        let cursor = self.document_search_cursor_offset();
        if !self.document_search.selected_range.is_empty() {
            self.replace_document_search_text(
                self.document_search.selected_range.clone(),
                "",
                false,
                Some(cursor..cursor),
                cx,
            );
            return;
        }
        if cursor == 0 {
            return;
        }
        let previous = document_text_grapheme_boundary(&self.document_search.query, cursor, true);
        self.replace_document_search_text(previous..cursor, "", false, Some(previous..previous), cx);
    }

    fn document_search_delete_forward(&mut self, cx: &mut Context<Self>) {
        let cursor = self.document_search_cursor_offset();
        if !self.document_search.selected_range.is_empty() {
            self.replace_document_search_text(
                self.document_search.selected_range.clone(),
                "",
                false,
                Some(cursor..cursor),
                cx,
            );
            return;
        }
        let query_len = self.document_search.query.len();
        if cursor >= query_len {
            return;
        }
        let next = document_text_grapheme_boundary(&self.document_search.query, cursor, false);
        self.replace_document_search_text(cursor..next, "", false, Some(cursor..cursor), cx);
    }

    fn document_search_replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = if self.document_search.selected_range.is_empty() {
            let cursor = self.document_search_cursor_offset();
            cursor..cursor
        } else {
            self.document_search.selected_range.clone()
        };
        let cursor = range.start + text.len();
        self.replace_document_search_text(range, text, false, Some(cursor..cursor), cx);
    }

    fn document_search_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.document_search_replace_selection(&text, cx);
        }
    }

    fn document_search_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if !self.document_search.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.document_search.query
                    [self.document_search.selected_range.clone()]
                    .to_string(),
            ));
        }
    }

    fn document_search_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        self.document_search_copy_to_clipboard(cx);
        if !self.document_search.selected_range.is_empty() {
            self.document_search_replace_selection("", cx);
        }
    }

    fn document_search_select_all_text(&mut self, cx: &mut Context<Self>) {
        self.document_search_move_to(0, cx);
        self.document_search_select_to(self.document_search.query.len(), cx);
    }
}

fn document_search_primary_shortcut_modifiers(modifiers: Modifiers) -> bool {
    modifiers.platform
}

fn document_text_grapheme_boundary(text: &str, offset: usize, backward: bool) -> usize {
    if backward {
        text.grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    } else {
        text.grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(text.len())
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
