//! Code-block runtime cache management.

use super::*;
use crate::input::text_norm::flatten_paste_to_single_line;

use crate::config::read_app_preferences;

/// Number of code lines shown before a collapsible block folds.
pub(crate) const CODE_BLOCK_COLLAPSED_VISIBLE_LINES: usize = 3;

/// Common code-block languages shown in the floating language picker.
pub(crate) const CODE_LANGUAGE_MENU_OPTIONS: &[&str] = &[
    "javascript",
    "typescript",
    "python",
    "bash",
    "rust",
    "go",
    "json",
    "markdown",
    "html",
    "css",
    "java",
    "cpp",
    "yaml",
    "text",
];

fn normalize_code_language_input(text: &str) -> String {
    flatten_paste_to_single_line(text).trim().to_string()
}

impl Block {
    pub(crate) fn code_block_line_count(text: &str) -> usize {
        if text.is_empty() {
            1
        } else {
            text.split('\n').count()
        }
    }

    pub(crate) fn code_block_is_collapsible(&self) -> bool {
        self.kind().is_code_block()
            && Self::code_block_line_count(self.display_text()) > CODE_BLOCK_COLLAPSED_VISIBLE_LINES
    }

    pub(crate) fn code_block_collapsed(&self, focused: bool) -> bool {
        if !self.code_block_is_collapsible() {
            return false;
        }
        match self.code_block_collapsed_override {
            Some(collapsed) => collapsed,
            None => {
                let default_expanded = read_app_preferences()
                    .map(|preferences| preferences.code_block_default_expanded)
                    .unwrap_or(false);
                if default_expanded {
                    false
                } else {
                    !focused
                }
            }
        }
    }

    pub(crate) fn code_block_hidden_line_count(&self) -> usize {
        Self::code_block_line_count(self.display_text())
            .saturating_sub(CODE_BLOCK_COLLAPSED_VISIBLE_LINES)
    }

    pub(crate) fn show_code_block_line_number_gutter(&self) -> bool {
        self.kind().is_code_block()
            && !self.is_source_raw_mode()
            && read_app_preferences()
                .map(|preferences| preferences.code_block_show_line_numbers)
                .unwrap_or(true)
    }

    pub(crate) fn show_line_number_gutter(&self) -> bool {
        self.show_source_line_numbers || self.show_code_block_line_number_gutter()
    }

    pub(crate) fn code_highlight_result(&self) -> Option<&CodeHighlightResult> {
        self.code_highlight.as_ref()
    }

    pub(super) fn sync_code_highlight(&mut self) {
        self.code_highlight = match &self.record.kind {
            BlockKind::CodeBlock { language } => highlight_code_block(
                language.as_deref().map(|value| &**value),
                self.render_cache.visible_text(),
            ),
            _ => None,
        };
    }

    pub(crate) fn code_language_text(&self) -> &str {
        match &self.record.kind {
            BlockKind::CodeBlock {
                language: Some(language),
            } => language.as_ref(),
            _ => "",
        }
    }

    pub(crate) fn code_language_range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        Self::utf8_range_to_utf16_in(self.code_language_text(), range)
    }

    pub(crate) fn code_language_range_from_utf16(
        &self,
        range_utf16: &Range<usize>,
    ) -> Range<usize> {
        Self::utf16_range_to_utf8_in(self.code_language_text(), range_utf16)
    }

    pub(crate) fn replace_code_language_text_in_range(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.kind().is_code_block() {
            return;
        }

        self.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);

        let current = self.code_language_text().to_string();
        let range = range.start.min(current.len())..range.end.min(current.len());
        let inserted = flatten_paste_to_single_line(new_text);
        let mut raw_next = String::new();
        raw_next.push_str(&current[..range.start]);
        raw_next.push_str(&inserted);
        raw_next.push_str(&current[range.end..]);

        let trimmed_start = raw_next.len() - raw_next.trim_start().len();
        let normalized = normalize_code_language_input(&raw_next);
        let normalized_len = normalized.len();
        let raw_inserted_end = range.start + inserted.len();
        let next_cursor = selected_range_relative
            .as_ref()
            .map(|relative| range.start + relative.end)
            .unwrap_or(raw_inserted_end)
            .saturating_sub(trimmed_start)
            .min(normalized_len);
        let next_selection = selected_range_relative
            .as_ref()
            .map(|relative| {
                let start = (range.start + relative.start)
                    .saturating_sub(trimmed_start)
                    .min(normalized_len);
                let end = (range.start + relative.end)
                    .saturating_sub(trimmed_start)
                    .min(normalized_len);
                start.min(end)..start.max(end)
            })
            .unwrap_or_else(|| next_cursor..next_cursor);
        let next_marked = if mark_inserted_text && !inserted.is_empty() {
            let start = range
                .start
                .saturating_sub(trimmed_start)
                .min(normalized_len);
            let end = raw_inserted_end
                .saturating_sub(trimmed_start)
                .min(normalized_len);
            (start < end).then_some(start..end)
        } else {
            None
        };

        let old_language = match &self.record.kind {
            BlockKind::CodeBlock { language } => language.clone(),
            _ => None,
        };
        self.record.kind = BlockKind::CodeBlock {
            language: (!normalized.is_empty()).then(|| SharedString::from(normalized)),
        };
        self.code_language_selected_range = next_selection;
        self.code_language_selection_reversed = selected_range_relative
            .as_ref()
            .is_some_and(|relative| relative.end < relative.start);
        self.code_language_marked_range = next_marked;
        self.cursor_blink_epoch = Instant::now();
        self.sync_code_highlight();

        let next_language = match &self.record.kind {
            BlockKind::CodeBlock { language } => language.clone(),
            _ => None,
        };
        if old_language != next_language {
            cx.emit(BlockEvent::Changed);
        }
        cx.notify();
    }

    pub(crate) fn set_code_language(&mut self, language: &str, cx: &mut Context<Self>) {
        if !self.kind().is_code_block() {
            return;
        }

        let range = 0..self.code_language_text().len();
        self.replace_code_language_text_in_range(range, language, None, false, cx);
        self.code_language_menu_open = false;
    }

    pub(crate) fn dismiss_code_language_menu(&mut self) -> bool {
        if self.code_language_menu_open {
            self.code_language_menu_open = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn sync_code_language_menu_for_focus(&mut self, active: bool) -> bool {
        if !active && self.code_language_menu_open {
            self.code_language_menu_open = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn set_code_run_snapshot(
        &mut self,
        snapshot: crate::code_runner::CodeBlockRunSnapshot,
    ) {
        self.code_run_snapshot = snapshot;
    }
}
