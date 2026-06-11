//! Rendered-mode Markdown toolbar actions for individual blocks.

use gpui::*;

use crate::components::markdown::source_format::MarkdownToolbarAction;
use crate::components::{BlockKind, InlineFormat, UndoCaptureKind};

use super::Block;

impl Block {
    pub(crate) fn apply_rendered_toolbar_format(
        &mut self,
        action: MarkdownToolbarAction,
        cx: &mut Context<Self>,
    ) {
        if self.uses_raw_text_editing() {
            return;
        }

        match action {
            MarkdownToolbarAction::Bold => {
                if self.selected_range.is_empty() {
                    self.insert_inline_markdown_markers("**", "**", cx);
                } else {
                    self.toggle_inline_format(InlineFormat::Bold, cx);
                }
            }
            MarkdownToolbarAction::Italic => {
                if self.selected_range.is_empty() {
                    self.insert_inline_markdown_markers("*", "*", cx);
                } else {
                    self.toggle_inline_format(InlineFormat::Italic, cx);
                }
            }
            MarkdownToolbarAction::Code => {
                if self.selected_range.is_empty() {
                    self.insert_inline_markdown_markers("`", "`", cx);
                } else {
                    self.toggle_inline_format(InlineFormat::Code, cx);
                }
            }
            MarkdownToolbarAction::CodeBlock => {
                self.convert_block_to_code_block(SharedString::from("javascript"), cx);
            }
            MarkdownToolbarAction::Link => {
                self.insert_link_markdown(cx);
            }
            MarkdownToolbarAction::Heading1 => {
                self.convert_block_to_heading(1, cx);
            }
            MarkdownToolbarAction::Heading2 => {
                self.convert_block_to_heading(2, cx);
            }
            MarkdownToolbarAction::Heading3 => {
                self.convert_block_to_heading(3, cx);
            }
            MarkdownToolbarAction::OrderedList => {
                self.convert_block_to_list_item(BlockKind::NumberedListItem, cx);
            }
            MarkdownToolbarAction::UnorderedList => {
                self.convert_block_to_list_item(BlockKind::BulletedListItem, cx);
            }
            MarkdownToolbarAction::Quote => {
                self.convert_block_to_quote(cx);
            }
            MarkdownToolbarAction::Table => {}
        }
    }

    fn insert_inline_markdown_markers(
        &mut self,
        prefix: &str,
        suffix: &str,
        cx: &mut Context<Self>,
    ) {
        let cursor = self.cursor_offset();
        let insert = format!("{prefix}{suffix}");
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.replace_text_in_visible_range(
            cursor..cursor,
            &insert,
            Some(prefix.len()..prefix.len()),
            false,
            cx,
        );
    }

    fn insert_link_markdown(&mut self, cx: &mut Context<Self>) {
        let selection = self.selected_range.clone();
        let text = self.display_text();
        let link_text = if selection.is_empty() {
            "link text".to_string()
        } else {
            text[selection.clone()].to_string()
        };
        let replacement = format!("[{link_text}](https://example.com)");
        let url_start = selection.start + link_text.len() + 3;
        let url_end = url_start + "https://example.com".len();
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.replace_text_in_visible_range(
            selection,
            &replacement,
            Some(url_start..url_end),
            false,
            cx,
        );
    }

    fn convert_block_to_heading(&mut self, level: u8, cx: &mut Context<Self>) {
        if matches!(self.kind(), BlockKind::Heading { level: existing } if existing == level) {
            self.convert_to_paragraph(cx);
            return;
        }
        if !matches!(
            self.kind(),
            BlockKind::Paragraph
                | BlockKind::BulletedListItem
                | BlockKind::NumberedListItem
                | BlockKind::TaskListItem { .. }
        ) {
            return;
        }
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.record.kind = BlockKind::Heading { level };
        self.mark_changed(cx);
    }

    fn convert_block_to_list_item(&mut self, kind: BlockKind, cx: &mut Context<Self>) {
        if self.kind() == kind {
            self.convert_to_paragraph(cx);
            return;
        }
        if !matches!(
            self.kind(),
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::BulletedListItem
                | BlockKind::NumberedListItem
                | BlockKind::TaskListItem { .. }
                | BlockKind::Quote
        ) {
            return;
        }
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.record.kind = kind;
        self.mark_changed(cx);
    }

    fn convert_block_to_quote(&mut self, cx: &mut Context<Self>) {
        if self.kind() == BlockKind::Quote {
            self.convert_to_paragraph(cx);
            return;
        }
        if !matches!(
            self.kind(),
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::BulletedListItem
                | BlockKind::NumberedListItem
                | BlockKind::TaskListItem { .. }
        ) {
            return;
        }
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.record.kind = BlockKind::Quote;
        self.mark_changed(cx);
    }

    fn convert_block_to_code_block(&mut self, language: SharedString, cx: &mut Context<Self>) {
        if matches!(
            self.kind(),
            BlockKind::CodeBlock {
                language: existing
            } if existing
                .as_ref()
                .is_some_and(|value| value.as_ref() == language.as_ref())
        ) {
            self.convert_to_paragraph(cx);
            return;
        }
        if !matches!(
            self.kind(),
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::BulletedListItem
                | BlockKind::NumberedListItem
                | BlockKind::TaskListItem { .. }
                | BlockKind::Quote
        ) {
            return;
        }
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.record.kind = BlockKind::CodeBlock {
            language: Some(language),
        };
        self.mark_changed(cx);
    }
}
