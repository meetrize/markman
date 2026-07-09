//! Action handlers dispatched by GPUI's action system when bound keys are
//! pressed on a focused block.  Each handler maps to a named action declared
//! in [`crate::components::actions`] and delegates structural changes to the
//! parent editor via `BlockEvent` emissions.

use std::time::Duration;

use gpui::*;

use super::CollapsedCaretAffinity;
use super::parse_columns_markdown;
use super::{Block, BlockEvent, BlockKind, InlineFormat, InlineTextTree, UndoCaptureKind};
use crate::components::markdown::paste::should_split_plain_multiline_paste;
use crate::input::text_norm::{flatten_paste_to_single_line, normalize_line_endings_lf};
use crate::components::{
    BlockDown, BlockUp, BoldSelection, CodeSelection, Copy, Cut, Delete, DeleteBack,
    End, ExitCodeBlock, FocusNext, FocusPrev, Home, IndentBlock,
    ItalicSelection, MoveLeft, MoveRight, Newline, OutdentBlock, Paste, SelectAll, SelectEnd,
    SelectHome, SelectLeft, SelectRight, UnderlineSelection, WordDeleteBack, WordDeleteForward,
    WordMoveLeft, WordMoveRight, WordSelectLeft, WordSelectRight,
};

impl Block {
    fn is_leaf_quote(&self) -> bool {
        self.kind() == BlockKind::Quote
            && self.children.is_empty()
            && !self.display_text().contains('\n')
    }

    fn is_leaf_callout(&self) -> bool {
        matches!(self.kind(), BlockKind::Callout(_)) && self.children.is_empty()
    }

    fn is_empty_leaf_quote(&self) -> bool {
        self.is_leaf_quote() && self.selected_range.is_empty() && self.is_empty()
    }

    fn downgrade_leaf_callout_to_quote_at_start(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_leaf_callout() || !self.selected_range.is_empty() || self.cursor_offset() != 0 {
            return false;
        }

        let BlockKind::Callout(variant) = self.kind() else {
            return false;
        };
        let header_markdown = variant.header_markdown(&self.record.title.serialize_markdown());
        self.record.kind = BlockKind::Quote;
        self.record
            .set_title(InlineTextTree::from_markdown(&header_markdown));
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();
        self.assign_collapsed_selection_offset(0, CollapsedCaretAffinity::Default, None);
        self.marked_range = None;
        self.cursor_blink_epoch = std::time::Instant::now();
        cx.emit(BlockEvent::Changed);
        cx.notify();
        true
    }

    fn downgrade_empty_leaf_quote_to_paragraph(&mut self, cx: &mut Context<Self>) -> bool {
        if self.is_empty_leaf_quote() {
            self.convert_to_paragraph(cx);
            return true;
        }
        false
    }

    /// If the code block's last line is a bare fence (three or more backticks
    /// or tildes, no info string), returns the byte offset to cut from so the
    /// whole line is removed; otherwise `None`.
    fn trailing_code_fence_line_start(&self) -> Option<usize> {
        let text = self.display_text();
        let line_start = text.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        let is_bare_fence = BlockKind::parse_code_fence_opening(&text[line_start..])
            .is_some_and(|fence| fence.language.is_none());
        // Cut from the preceding newline too, unless the fence is the only line.
        is_bare_fence.then(|| line_start.saturating_sub(1))
    }

    pub(crate) fn on_newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        // Enter is ordered from special editors to rich-text splitting:
        // table/source/code/quote-like blocks keep local newline semantics,
        // while normal rendered blocks emit an editor-level split request.
        if self.is_table_cell() {
            cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: 1 });
            return;
        }

        if self.editor_selection_range.is_some() {
            cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                text: "\n".to_string(),
                selected_range_relative: None,
                mark_inserted_text: false,
                undo_kind: UndoCaptureKind::NonCoalescible,
            });
            return;
        }

        if self.inline_math_source_editing() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            if let Some(trailing) = self.split_inline_math_source_edit_for_newline() {
                self.mark_changed(cx);
                cx.emit(BlockEvent::RequestNewline {
                    trailing,
                    source_already_mutated: true,
                });
            }
            return;
        }

        if self.is_source_raw_mode() {
            if !self.selected_range.is_empty() {
                self.replace_text_in_range(None, "", window, cx);
            }
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if self.kind() == BlockKind::Paragraph
            && self.selected_range.is_empty()
            && self.cursor_offset() == self.visible_len()
            && BlockKind::parse_separator_line(self.display_text())
        {
            self.convert_to_separator(cx);
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: true,
            });
            return;
        }

        if self.kind() == BlockKind::Paragraph
            && self.selected_range.is_empty()
            && self.cursor_offset() == self.visible_len()
            && self.display_text() == "$$"
        {
            self.enter_math_block(cx);
            return;
        }

        if self.kind() == BlockKind::Paragraph
            && self.selected_range.is_empty()
            && self.cursor_offset() == self.visible_len()
            && let Some(fence) = BlockKind::parse_code_fence_opening(self.display_text())
        {
            self.enter_code_block(fence.language, cx);
            return;
        }

        if self.kind().is_separator() {
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: false,
            });
            return;
        }

        if self.kind().is_list_item() && self.selected_range.is_empty() && self.is_empty() {
            cx.emit(BlockEvent::RequestOutdent);
            return;
        }

        if self.kind() == BlockKind::Quote {
            if !self.selected_range.is_empty() {
                self.replace_text_in_range(None, "", window, cx);
            }
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if matches!(self.kind(), BlockKind::Callout(_)) {
            cx.emit(BlockEvent::RequestEnterCalloutBody);
            return;
        }

        // In a code block, Enter inserts a newline into the block content
        // rather than splitting the block.  Pressing Enter on an empty
        // code block exits back to a paragraph.
        if self.kind().is_code_block() {
            if self.selected_range.is_empty() && self.is_empty() {
                self.convert_to_paragraph(cx);
                return;
            }
            // Typing a bare closing fence on the last line and pressing Enter
            // leaves the block, matching source mode.
            if self.selected_range.is_empty()
                && self.cursor_offset() == self.visible_len()
                && let Some(fence_start) = self.trailing_code_fence_line_start()
            {
                let fence_end = self.visible_len();
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                self.replace_text_in_visible_range(fence_start..fence_end, "", None, false, cx);
                cx.emit(BlockEvent::RequestNewline {
                    trailing: InlineTextTree::plain(String::new()),
                    source_already_mutated: true,
                });
                return;
            }
            if !self.selected_range.is_empty() {
                self.replace_text_in_range(None, "", window, cx);
            }
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if self.collapsed_caret_inherits_inline_code_style() {
            if self.skip_inline_code_newline_once {
                self.skip_inline_code_newline_once = false;
                return;
            }
            let modifiers = window.modifiers();
            if modifiers.control
                || modifiers.platform
                || self.last_keydown_modifiers.control
                || self.last_keydown_modifiers.platform
            {
                return;
            }
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }

        if !self.selected_range.is_empty() {
            self.replace_text_in_range(None, "", window, cx);
        }

        let cursor = self.cursor_offset();
        let (leading, trailing) = self.split_title(cursor);
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.record.set_title(leading);
        self.mark_changed(cx);
        let cursor = self.visible_len();
        self.assign_collapsed_selection_offset(cursor, CollapsedCaretAffinity::Default, None);
        self.marked_range = None;
        cx.emit(BlockEvent::RequestNewline {
            trailing,
            source_already_mutated: true,
        });
    }

    pub(crate) fn on_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_table_cell() {
            if self.selected_range.is_empty() {
                let previous = self.previous_boundary(self.cursor_offset());
                if previous == self.cursor_offset() {
                    return;
                }
                self.select_to(previous, cx);
            }
            self.replace_text_in_range(None, "", window, cx);
            return;
        }

        if self.is_source_raw_mode() {
            if self.selected_range.is_empty() {
                self.select_to(self.previous_boundary(self.cursor_offset()), cx);
            }
            self.replace_text_in_range(None, "", window, cx);
            return;
        }

        if self.selected_range.is_empty() && self.cursor_offset() == 0 {
            if self.kind() == BlockKind::Paragraph && self.is_direct_list_child() && self.is_empty()
            {
                cx.emit(BlockEvent::RequestOutdent);
                return;
            }
            if self.is_nested_list_item() {
                cx.emit(BlockEvent::RequestDowngradeNestedListItemToChildParagraph);
                return;
            }
            match self.kind() {
                BlockKind::BulletedListItem
                | BlockKind::TaskListItem { .. }
                | BlockKind::NumberedListItem => {
                    cx.emit(BlockEvent::RequestOutdent);
                    return;
                }
                BlockKind::Heading { .. } => {
                    self.convert_to_paragraph(cx);
                    return;
                }
                BlockKind::Quote => {
                    if self.is_leaf_quote() {
                        self.convert_to_paragraph(cx);
                    }
                    return;
                }
                BlockKind::Callout(_) => {
                    if self.downgrade_leaf_callout_to_quote_at_start(cx) {
                        return;
                    }
                    return;
                }
                BlockKind::Separator => {
                    self.convert_to_paragraph(cx);
                    return;
                }
                BlockKind::CodeBlock { .. } => {
                    self.convert_to_paragraph(cx);
                    return;
                }
                _ => {}
            }
        }

        if self.downgrade_leaf_callout_to_quote_at_start(cx)
            || self.downgrade_empty_leaf_quote_to_paragraph(cx)
        {
            return;
        }

        if self.selected_range.is_empty() && self.display_text().is_empty() {
            cx.emit(BlockEvent::RequestDelete);
            return;
        }

        if self.selected_range.is_empty() && self.cursor_offset() == 0 {
            cx.emit(BlockEvent::RequestMergeIntoPrev {
                content: self.record.title.clone(),
            });
            return;
        }

        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_table_cell() {
            if self.selected_range.is_empty() {
                let next = self.next_boundary(self.cursor_offset());
                if next == self.cursor_offset() {
                    return;
                }
                self.select_to(next, cx);
            }
            self.replace_text_in_range(None, "", window, cx);
            return;
        }

        if self.is_source_raw_mode() {
            if self.selected_range.is_empty() {
                self.select_to(self.next_boundary(self.cursor_offset()), cx);
            }
            self.replace_text_in_range(None, "", window, cx);
            return;
        }

        if self.downgrade_leaf_callout_to_quote_at_start(cx)
            || self.downgrade_empty_leaf_quote_to_paragraph(cx)
        {
            return;
        }

        if self.kind().is_separator() {
            self.convert_to_paragraph(cx);
            return;
        }

        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_word_delete_back(
        &mut self,
        _: &WordDeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            if self.cursor_offset() == 0 {
                // Nothing to the left in this block; defer to grapheme
                // backspace, which handles block merge and downgrades.
                self.on_delete_back(&DeleteBack, window, cx);
                return;
            }
            self.select_to(self.previous_word_start(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_word_delete_forward(
        &mut self,
        _: &WordDeleteForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            if self.cursor_offset() == self.visible_len() {
                // Nothing to the right in this block; defer to grapheme
                // delete, which handles block merge and separator removal.
                self.on_delete(&Delete, window, cx);
                return;
            }
            self.select_to(self.next_word_start(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_indent_block(
        &mut self,
        _: &IndentBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_table_cell() {
            cx.emit(BlockEvent::RequestTableCellMoveHorizontal { delta: 1 });
            return;
        }
        if self.can_adjust_list_nesting() {
            cx.emit(BlockEvent::RequestIndent);
            return;
        }
        if self.kind() == BlockKind::Paragraph || self.kind().is_code_block() {
            self.replace_text_in_range(None, "    ", window, cx);
        }
    }

    pub(crate) fn on_outdent_block(
        &mut self,
        _: &OutdentBlock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_table_cell() {
            cx.emit(BlockEvent::RequestTableCellMoveHorizontal { delta: -1 });
            return;
        }
        if self.can_outdent_list_nesting() {
            cx.emit(BlockEvent::RequestOutdent);
        }
    }

    pub(crate) fn on_block_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_keydown_modifiers = event.keystroke.modifiers;

        if event.keystroke.key != "tab" {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.platform || modifiers.alt || modifiers.function {
            return;
        }

        if self.code_language_focus_handle.is_focused(window) {
            return;
        }

        if modifiers.shift {
            self.on_outdent_block(&OutdentBlock, window, cx);
        } else {
            self.on_indent_block(&IndentBlock, window, cx);
        }
        cx.stop_propagation();
    }

    pub(crate) fn on_focus_prev(
        &mut self,
        _: &FocusPrev,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preferred_x = self.vertical_anchor_x();
        if !self.move_cursor_vertically(-1, preferred_x, cx) {
            if self.is_table_cell() {
                cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: -1 });
                return;
            }
            cx.emit(BlockEvent::RequestFocusPrev {
                preferred_x: Some(f32::from(preferred_x)),
            });
        }
    }

    pub(crate) fn on_focus_next(
        &mut self,
        _: &FocusNext,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preferred_x = self.vertical_anchor_x();
        if !self.move_cursor_vertically(1, preferred_x, cx) {
            if self.is_table_cell() {
                cx.emit(BlockEvent::RequestTableCellMoveVertical { delta: 1 });
                return;
            }
            cx.emit(BlockEvent::RequestFocusNext {
                preferred_x: Some(f32::from(preferred_x)),
            });
        }
    }

    pub(crate) fn on_move_left(
        &mut self,
        _: &MoveLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            if let Some((target, affinity)) = self.projected_move_left_target(self.cursor_offset())
            {
                self.assign_collapsed_selection_offset(target, affinity, None);
                self.cursor_blink_epoch = std::time::Instant::now();
                cx.notify();
            } else {
                self.move_to(self.previous_boundary(self.cursor_offset()), cx);
            }
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    pub(crate) fn on_move_right(
        &mut self,
        _: &MoveRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            if let Some((target, affinity)) =
                self.projected_move_right_target(self.selected_range.end)
            {
                self.assign_collapsed_selection_offset(target, affinity, None);
                self.cursor_blink_epoch = std::time::Instant::now();
                cx.notify();
            } else {
                self.move_to(self.next_boundary(self.selected_range.end), cx);
            }
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    pub(crate) fn on_home(&mut self, _: &Home, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    pub(crate) fn on_end(&mut self, _: &End, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.visible_len(), cx);
    }

    pub(crate) fn on_select_left(
        &mut self,
        _: &SelectLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((target, _)) = self.projected_move_left_target(self.cursor_offset()) {
            self.select_to(target, cx);
        } else {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
    }

    pub(crate) fn on_select_right(
        &mut self,
        _: &SelectRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((target, _)) = self.projected_move_right_target(self.cursor_offset()) {
            self.select_to(target, cx);
        } else {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
    }

    pub(crate) fn on_word_move_left(
        &mut self,
        _: &WordMoveLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.previous_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_word_move_right(
        &mut self,
        _: &WordMoveRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.next_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_word_select_left(
        &mut self,
        _: &WordSelectLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_word_select_right(
        &mut self,
        _: &WordSelectRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_word_start(self.cursor_offset()), cx);
    }

    pub(crate) fn on_block_up(
        &mut self,
        _: &BlockUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(BlockEvent::RequestBlockUp);
    }

    pub(crate) fn on_block_down(
        &mut self,
        _: &BlockDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(BlockEvent::RequestBlockDown);
    }

    pub(crate) fn on_select_all(
        &mut self,
        _: &SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.edit_mode == super::runtime::EditMode::RenderedRich {
            cx.emit(BlockEvent::RequestSelectAllDocument);
            cx.stop_propagation();
            return;
        }
        self.move_to(0, cx);
        self.select_to(self.visible_len(), cx);
    }

    pub(crate) fn on_select_home(
        &mut self,
        _: &SelectHome,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    pub(crate) fn on_select_end(
        &mut self,
        _: &SelectEnd,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.visible_len(), cx);
    }

    pub(crate) fn on_copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_text_selection() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.selected_display_text()));
        }
    }

    pub(crate) fn on_code_block_copy_click(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.record.title.visible_text().to_string(),
        ));
    }

    pub(crate) fn on_code_block_run_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestRunCodeBlock);
    }

    pub(crate) fn on_code_block_run_stop_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestStopCodeBlock);
    }

    pub(crate) fn on_code_block_run_output_toggle_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestToggleCodeRunOutput);
    }

    pub(crate) fn on_code_block_run_output_content_toggle_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestToggleCodeRunOutputContent);
    }

    pub(crate) fn on_code_block_run_output_close_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestCloseCodeRunOutput);
    }

    pub(crate) fn on_code_block_collapse_toggle(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.code_block_is_collapsible() {
            return;
        }
        let focused = self.focus_handle.is_focused(window);
        let collapsed = self.code_block_collapsed(focused);
        self.code_block_collapsed_override = Some(!collapsed);
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn on_code_language_badge_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            cx.emit(BlockEvent::RequestFocus);
            self.focus_handle.focus(window);
            self.code_language_menu_open = true;
        } else {
            self.code_language_menu_open = !self.code_language_menu_open;
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn on_cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_text_selection() {
            return;
        }
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        cx.write_to_clipboard(ClipboardItem::new_string(self.selected_display_text()));
        self.apply_active_text_selection_for_local_edit();
        self.replace_text_in_range(None, "", window, cx);
    }

    pub(crate) fn on_paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind().is_separator() && !self.uses_raw_text_editing() {
            return;
        }

        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // Only rendered rich-text blocks apply paste correction. Raw/code
            // contexts preserve bytes, and table cells flatten newlines so the
            // surrounding table structure is not accidentally split.
            if self.editor_selection_range.is_some() {
                cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                    text,
                    selected_range_relative: None,
                    mark_inserted_text: false,
                    undo_kind: UndoCaptureKind::NonCoalescible,
                });
                return;
            }

            if self.is_table_cell() {
                let flattened = flatten_paste_to_single_line(&text);
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                self.replace_text_in_range(None, &flattened, window, cx);
                return;
            }

            if self.uses_raw_text_editing() {
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                self.replace_text_in_range(None, &text, window, cx);
                return;
            }

            if text.contains('\n') || text.contains('\r') {
                let normalized = normalize_line_endings_lf(&text);
                if self.quote_depth > 0 {
                    self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                    self.replace_text_in_range(None, &normalized, window, cx);
                    return;
                }
                let clean_selected = self.selection_clean_range();
                let (leading, tail) = self.record.title.split_at(clean_selected.start);
                let (_, trailing) =
                    tail.split_at(clean_selected.end.saturating_sub(clean_selected.start));
                let lines = normalized
                    .split('\n')
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                let split_physical_lines = should_split_plain_multiline_paste(&lines);
                cx.emit(BlockEvent::RequestPasteMultiline {
                    leading,
                    lines,
                    trailing,
                    split_physical_lines,
                });
                return;
            }

            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.code_language_menu_open {
            self.code_language_menu_open = false;
            cx.notify();
        }

        if self.showing_rendered_image() {
            self.is_selecting = false;
            self.request_image_edit_expansion();
            if self.focus_handle.is_focused(window) {
                if self.sync_image_focus_state(true) {
                    cx.notify();
                }
            } else {
                cx.emit(BlockEvent::RequestFocus);
            }
            cx.stop_propagation();
            return;
        }

        if event.click_count == 1
            && self.try_handle_link_icon_click(event.position, window, cx)
        {
            self.is_selecting = false;
            cx.stop_propagation();
            return;
        }

        if event.click_count == 1
            && self.try_handle_tag_click(event.position, window, cx)
        {
            self.is_selecting = false;
            cx.stop_propagation();
            return;
        }

        let was_focused = self.focus_handle.is_focused(window);
        let columns_preview_active = self.is_columns_raw_markdown()
            && parse_columns_markdown(self.display_text()).is_some()
            && !self.columns_source_edit;
        if self.is_columns_raw_markdown() && was_focused && !columns_preview_active {
            self.enable_columns_source_edit(cx);
        }
        if self.kind().is_code_block() && self.code_block_is_collapsible() && !was_focused {
            self.code_block_collapsed_override = None;
        }
        if event.click_count >= 2
            && self.try_handle_link_double_click(event.position, window, cx)
        {
            cx.stop_propagation();
            self.is_selecting = false;
            if !was_focused {
                self.focus_handle.focus(window);
                cx.emit(BlockEvent::RequestFocus);
            }
            return;
        }

        if event.click_count >= 2
            && self.try_select_word_or_line_at_click_count(
                event.position,
                event.click_count,
                window,
                cx,
            )
        {
            cx.stop_propagation();
            self.is_selecting = false;
            if !was_focused {
                self.focus_handle.focus(window);
                cx.emit(BlockEvent::RequestFocus);
            }
            return;
        }

        let offset = self.index_for_mouse_position(event.position);

        if was_focused {
            self.is_selecting = true;
            if event.modifiers.shift {
                self.select_to(offset, cx);
            } else {
                self.move_to(offset, cx);
            }
        } else {
            self.is_selecting = true;
            if event.modifiers.shift {
                self.select_to(offset, cx);
            } else {
                self.move_to(offset, cx);
            }
            cx.emit(BlockEvent::RequestFocus);
        }
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = false;
        if event.click_count >= 2 {
            self.try_select_word_or_line_at_click_count(
                event.position,
                event.click_count,
                _window,
                cx,
            );
        } else {
            self.finalize_pointer_word_or_line_selection(_window, cx);
        }
    }

    fn finalize_pointer_word_or_line_selection(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.focus_handle.is_focused(window)
            && self.edit_mode == super::runtime::EditMode::RenderedRich
        {
            self.sync_inline_projection_for_focus(true);
        }
        if !self.shows_text_selection_highlight() && self.editor_selection_range.is_some() {
            self.editor_selection_range = None;
            cx.notify();
        }
    }

    /// Returns `true` when a single click on a link-type icon was handled.
    pub(crate) fn try_handle_link_icon_click(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_source_raw_mode() {
            return false;
        }

        let text_bounds = match self.last_bounds.or(self.interaction_bounds) {
            Some(bounds) => bounds,
            None => return false,
        };
        let lines = match self.last_layout.as_ref() {
            Some(lines) => lines,
            None => return false,
        };

        let Some(link) = super::element::link_action_icon_at_position(
            self,
            lines,
            text_bounds,
            self.last_line_height,
            position,
        ) else {
            return false;
        };

        cx.stop_propagation();
        cx.emit(BlockEvent::RequestOpenLink {
            prompt_target: link.prompt_target,
            open_target: link.open_target,
            is_workspace_file: link.is_workspace_file,
            is_document_relative_file: link.is_document_relative_file,
        });
        true
    }

    /// Returns `true` when a single click on an inline hashtag was handled.
    pub(crate) fn try_handle_tag_click(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_source_raw_mode() {
            return false;
        }

        let text_bounds = match self.last_bounds.or(self.interaction_bounds) {
            Some(bounds) => bounds,
            None => return false,
        };
        let lines = match self.last_layout.as_ref() {
            Some(lines) => lines,
            None => return false,
        };

        let tag = match super::element::tag_at_position(
            self,
            lines,
            text_bounds,
            self.last_line_height,
            position,
        ) {
            Some(tag) => tag.clone(),
            None => return false,
        };

        cx.stop_propagation();
        cx.emit(BlockEvent::RequestFilterByTag {
            name: tag.name.clone(),
        });
        true
    }

    fn cancel_pending_wiki_link_picker_open(&mut self) {
        self.wiki_link_picker_single_click_generation =
            self.wiki_link_picker_single_click_generation.wrapping_add(1);
    }

    /// Returns `true` when a single click on wiki link text was handled.
    pub(crate) fn try_handle_link_single_click(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_source_raw_mode() {
            return false;
        }

        let text_bounds = match self.last_bounds.or(self.interaction_bounds) {
            Some(bounds) => bounds,
            None => return false,
        };
        let lines = match self.last_layout.as_ref() {
            Some(lines) => lines,
            None => return false,
        };

        if super::element::link_action_icon_at_position(
            self,
            lines,
            text_bounds,
            self.last_line_height,
            position,
        )
        .is_some()
        {
            return false;
        }

        let path = match super::element::link_text_at_position(
            self,
            lines,
            text_bounds,
            self.last_line_height,
            position,
        ) {
            Some(link) if link.is_workspace_file => link.open_target.clone(),
            _ => return false,
        };

        let offset = self.index_for_mouse_position(position);
        self.move_to(offset, cx);
        self.sync_inline_projection_for_focus(true);

        if !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window);
            cx.emit(BlockEvent::RequestFocus);
        }

        self.cancel_pending_wiki_link_picker_open();
        let generation = self.wiki_link_picker_single_click_generation;
        let block = cx.entity().downgrade();
        cx.spawn(async move |_this: WeakEntity<Block>, cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = block.update(cx, |block, cx| {
                if block.wiki_link_picker_single_click_generation != generation {
                    return;
                }
                cx.emit(BlockEvent::RequestOpenWikiLinkPicker { path });
            });
        })
        .detach();

        cx.stop_propagation();
        true
    }

    /// Returns `true` when a double click on wiki link text should open the file.
    pub(crate) fn try_handle_link_double_click(
        &mut self,
        position: Point<Pixels>,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_source_raw_mode() {
            return false;
        }

        let text_bounds = match self.last_bounds.or(self.interaction_bounds) {
            Some(bounds) => bounds,
            None => return false,
        };
        let lines = match self.last_layout.as_ref() {
            Some(lines) => lines,
            None => return false,
        };

        if super::element::link_action_icon_at_position(
            self,
            lines,
            text_bounds,
            self.last_line_height,
            position,
        )
        .is_some()
        {
            return false;
        }

        let Some(link) = super::element::link_text_at_position(
            self,
            lines,
            text_bounds,
            self.last_line_height,
            position,
        ) else {
            return false;
        };
        if !link.is_workspace_file {
            return false;
        }

        let prompt_target = link.prompt_target.clone();
        let open_target = link.open_target.clone();
        self.cancel_pending_wiki_link_picker_open();
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestOpenLink {
            prompt_target,
            open_target,
            is_workspace_file: true,
            is_document_relative_file: false,
        });
        true
    }

    /// Returns `true` when a double- or triple-click word/line selection (or
    /// footnote/link activation) was handled.
    pub(crate) fn try_select_word_or_line_at_click_count(
        &mut self,
        position: Point<Pixels>,
        click_count: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let offset = self.index_for_mouse_position(position);
        let text_bounds = self.last_bounds.or(self.interaction_bounds);

        if click_count >= 3 {
            cx.stop_propagation();
            self.select_line_at_offset(offset, cx);
            self.finalize_pointer_word_or_line_selection(window, cx);
            return true;
        }

        if click_count >= 2 {
            let footnote = self
                .last_layout
                .as_ref()
                .zip(text_bounds)
                .and_then(|(lines, bounds)| {
                    super::element::footnote_at_position(
                        self,
                        lines,
                        bounds,
                        self.last_line_height,
                        position,
                    )
                })
                .cloned();
            if let Some(footnote) = footnote {
                cx.stop_propagation();
                cx.emit(BlockEvent::RequestJumpToFootnoteDefinition { id: footnote.id });
                return true;
            }

            let link = self
                .last_layout
                .as_ref()
                .zip(text_bounds)
                .and_then(|(lines, bounds)| {
                    super::element::link_at_position(
                        self,
                        lines,
                        bounds,
                        self.last_line_height,
                        position,
                    )
                })
                .cloned();
            if let Some(link) = link {
                if link.is_workspace_file {
                    self.cancel_pending_wiki_link_picker_open();
                }
                cx.stop_propagation();
                cx.emit(BlockEvent::RequestOpenLink {
                    prompt_target: link.prompt_target,
                    open_target: link.open_target,
                    is_workspace_file: link.is_workspace_file,
                    is_document_relative_file: link.is_document_relative_file,
                });
                return true;
            }

            cx.stop_propagation();
            self.select_word_at_offset(offset, cx);
            self.finalize_pointer_word_or_line_selection(window, cx);
            return true;
        }

        false
    }

    /// Returns `true` when a double- or triple-click word/line selection (or
    /// footnote/link activation) was handled.
    pub(crate) fn try_select_word_or_line_at_click(
        &mut self,
        event: &MouseUpEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.click_count >= 2
            && self.try_handle_link_double_click(event.position, window, cx)
        {
            return true;
        }
        self.try_select_word_or_line_at_click_count(event.position, event.click_count, window, cx)
    }

    pub(crate) fn on_footnote_backref_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if !self.focus_handle.is_focused(window) {
            cx.emit(BlockEvent::RequestFocus);
        }
    }

    pub(crate) fn on_footnote_backref_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.footnote_definition_id() else {
            return;
        };
        cx.stop_propagation();
        cx.emit(BlockEvent::RequestJumpToFootnoteBackref { id });
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_source_raw_mode() && !self.kind().is_code_block() {
            let offset = self.index_for_mouse_position(event.position);
            let next_hover = self
                .inline_spans()
                .iter()
                .find(|span| {
                    span.style.code && span.range.start <= offset && offset <= span.range.end
                })
                .map(|span| span.range.clone());
            if self.inline_code_hover_span != next_hover {
                self.inline_code_hover_span = next_hover;
                cx.notify();
            }
        }

        if self.is_selecting {
            // A stale selecting flag can survive a missed mouse-up. Only extend
            // the selection while the platform still reports an active drag.
            if !event.dragging() {
                self.is_selecting = false;
                cx.notify();
                return;
            }
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    pub(crate) fn on_task_checkbox_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if !self.focus_handle.is_focused(window) {
            cx.emit(BlockEvent::RequestFocus);
        }
    }

    pub(crate) fn on_task_checkbox_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.kind().is_task_list_item() || self.is_source_raw_mode() {
            return;
        }

        cx.stop_propagation();
        cx.emit(BlockEvent::ToggleTaskChecked);
    }

    pub(crate) fn on_append_table_column(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.kind() == BlockKind::Table {
            cx.emit(BlockEvent::RequestAppendTableColumn);
        }
    }

    pub(crate) fn on_append_table_row(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.kind() == BlockKind::Table {
            cx.emit(BlockEvent::RequestAppendTableRow);
        }
    }

    pub(crate) fn on_bold_selection(
        &mut self,
        _: &BoldSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Bold, cx);
    }

    pub(crate) fn on_italic_selection(
        &mut self,
        _: &ItalicSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Italic, cx);
    }

    pub(crate) fn on_underline_selection(
        &mut self,
        _: &UnderlineSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Underline, cx);
    }

    pub(crate) fn on_code_selection(
        &mut self,
        _: &CodeSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_inline_format(InlineFormat::Code, cx);
    }

    pub(crate) fn on_exit_code_block(
        &mut self,
        _: &ExitCodeBlock,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.collapsed_caret_inherits_inline_code_style() {
            self.skip_inline_code_newline_once = true;
            if let Some(span) = self.inline_code_span_at_cursor() {
                cx.emit(BlockEvent::RequestRunInlineCode {
                    span_range: span.range,
                });
            }
            cx.stop_propagation();
            return;
        }

        let exits_multiline_block = self.is_table_cell()
            || self.kind().is_code_block()
            || matches!(
                self.kind(),
                BlockKind::MathBlock
                    | BlockKind::HtmlBlock
                    | BlockKind::MermaidBlock
                    | BlockKind::RawMarkdown
                    | BlockKind::Comment
            );

        if exits_multiline_block {
            cx.emit(BlockEvent::RequestNewline {
                trailing: InlineTextTree::plain(String::new()),
                source_already_mutated: false,
            });
        } else if self.callout_depth > 0 {
            cx.emit(BlockEvent::RequestCalloutBreak);
        } else if self.quote_depth > 0 {
            cx.emit(BlockEvent::RequestQuoteBreak);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Block;
    use crate::components::{BlockKind, BlockRecord, InlineTextTree};
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    async fn multiline_quote_is_not_treated_as_leaf(cx: &mut TestAppContext) {
        let block = cx.new(|cx| Block::with_record(cx, BlockRecord::paragraph(String::new())));

        block.update(cx, |block, cx| {
            block.record.kind = BlockKind::Quote;
            block.record.set_title(InlineTextTree::plain("first\n"));
            block.sync_edit_mode_from_kind();
            block.sync_render_cache();
            cx.notify();

            assert!(!block.is_leaf_quote());
        });
    }
}
