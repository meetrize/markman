//! Markdown formatting toolbar shown above the editor content area.

use gpui::*;

use crate::components::markdown::source_format::{MarkdownToolbarAction, apply_markdown_toolbar_action};
use crate::components::{AskAi, Block, BlockKind, BlockRecord, InlineTextTree, UndoCaptureKind};
use crate::theme::Theme;

use super::Editor;
use super::ViewMode;

const ICON_UNDO: &str = "icon/toolbar/undo-2.svg";
const ICON_REDO: &str = "icon/toolbar/redo-2.svg";
const ICON_BOLD: &str = "icon/toolbar/bold.svg";
const ICON_ITALIC: &str = "icon/toolbar/italic.svg";
const ICON_HEADING_1: &str = "icon/toolbar/heading-1.svg";
const ICON_HEADING_2: &str = "icon/toolbar/heading-2.svg";
const ICON_HEADING_3: &str = "icon/toolbar/heading-3.svg";
const ICON_LIST_ORDERED: &str = "icon/toolbar/list-ordered.svg";
const ICON_LIST_BULLET: &str = "icon/toolbar/list-bullet.svg";
const ICON_CODE: &str = "icon/toolbar/code.svg";
const ICON_SQUARE_CODE: &str = "icon/toolbar/square-code.svg";
const ICON_LINK: &str = "icon/toolbar/link.svg";
const ICON_QUOTE: &str = "icon/toolbar/quote.svg";
const ICON_TABLE: &str = "icon/toolbar/table.svg";
const ICON_TODO: &str = "icon/toolbar/square-check-big.svg";
const ICON_HORIZONTAL_RULE: &str = "icon/toolbar/minus.svg";
const ICON_IMAGE: &str = "icon/toolbar/image.svg";
const ICON_TABLE_OF_CONTENTS: &str = "icon/toolbar/table-of-contents.svg";
const ICON_VIEW_SOURCE: &str = "icon/toolbar/view-source.svg";
const ICON_VIEW_RENDERED: &str = "icon/toolbar/view-rendered.svg";
const ICON_SAVE: &str = "icon/toolbar/save.svg";
const ICON_AUTO_SAVE: &str = "icon/toolbar/auto-save.svg";
const ICON_SEARCH: &str = "icon/toolbar/search.svg";
const ICON_AI_CUSTOM: &str = "icon/toolbar/sparkles.svg";

enum FormatToolbarItem {
    Action(MarkdownToolbarAction),
    Separator,
}

impl Editor {
    pub(super) fn apply_markdown_toolbar_format(
        &mut self,
        action: MarkdownToolbarAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == MarkdownToolbarAction::Table && self.view_mode == ViewMode::Rendered {
            self.open_table_insert_from_toolbar(window, cx);
            return;
        }

        if action == MarkdownToolbarAction::CodeBlock && self.view_mode == ViewMode::Rendered {
            if let Some(block) = self.focused_edit_target(window, cx) {
                block.update(cx, |block, cx| {
                    block.apply_rendered_toolbar_format(action, cx);
                });
            } else {
                self.append_code_block_from_toolbar(cx);
            }
            self.mark_dirty(cx);
            cx.notify();
            return;
        }

        // Block-insertion actions: in rendered mode, insert native blocks;
        // in source mode, insert raw Markdown text.
        if matches!(
            action,
            MarkdownToolbarAction::Todo
                | MarkdownToolbarAction::HorizontalRule
                | MarkdownToolbarAction::Image
                | MarkdownToolbarAction::TableOfContents
        ) {
            if self.view_mode == ViewMode::Rendered {
                self.apply_rendered_block_insert(action, window, cx);
            } else {
                self.apply_source_view_toolbar_format(action, cx);
            }
            return;
        }

        if self.view_mode == ViewMode::Source {
            self.apply_source_view_toolbar_format(action, cx);
            return;
        }

        let Some(block) = self.focused_edit_target(window, cx) else {
            return;
        };

        block.update(cx, |block, cx| {
            block.apply_rendered_toolbar_format(action, cx);
        });
        self.mark_dirty(cx);
        cx.notify();
    }

    fn apply_source_view_toolbar_format(
        &mut self,
        action: MarkdownToolbarAction,
        cx: &mut Context<Self>,
    ) {
        let Some(block_entity) = self
            .document
            .visible_blocks()
            .first()
            .map(|visible| visible.entity.clone())
        else {
            return;
        };

        block_entity.update(cx, |block, cx| {
            if !block.show_source_line_numbers() {
                return;
            }

            block.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            let text = block.display_text().to_string();
            let selection = block.selected_range.clone();
            let (next_text, next_selection) =
                apply_markdown_toolbar_action(&text, selection, action);
            block.replace_text_in_visible_range(
                0..text.len(),
                &next_text,
                Some(next_selection.clone()),
                false,
                cx,
            );
            block.selected_range = next_selection;
            block.selection_reversed = false;
            block.marked_range = None;
            block.cursor_blink_epoch = std::time::Instant::now();
            cx.emit(crate::components::BlockEvent::Changed);
            cx.notify();
        });
        self.mark_dirty(cx);
        cx.notify();
    }

    pub(super) fn render_markdown_format_toolbar(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let icon_color = c.dialog_secondary_button_text;
        let icon_size = px(d.format_toolbar_icon_size);
        let editor = cx.entity().downgrade();

        let items = [
            FormatToolbarItem::Action(MarkdownToolbarAction::Bold),
            FormatToolbarItem::Action(MarkdownToolbarAction::Italic),
            FormatToolbarItem::Separator,
            FormatToolbarItem::Action(MarkdownToolbarAction::Heading1),
            FormatToolbarItem::Action(MarkdownToolbarAction::Heading2),
            FormatToolbarItem::Action(MarkdownToolbarAction::Heading3),
            FormatToolbarItem::Separator,
            FormatToolbarItem::Action(MarkdownToolbarAction::OrderedList),
            FormatToolbarItem::Action(MarkdownToolbarAction::UnorderedList),
            FormatToolbarItem::Separator,
            FormatToolbarItem::Action(MarkdownToolbarAction::Code),
            FormatToolbarItem::Action(MarkdownToolbarAction::CodeBlock),
            FormatToolbarItem::Action(MarkdownToolbarAction::Link),
            FormatToolbarItem::Action(MarkdownToolbarAction::Quote),
            FormatToolbarItem::Separator,
            FormatToolbarItem::Action(MarkdownToolbarAction::Todo),
            FormatToolbarItem::Action(MarkdownToolbarAction::Image),
            FormatToolbarItem::Separator,
            FormatToolbarItem::Action(MarkdownToolbarAction::HorizontalRule),
            FormatToolbarItem::Action(MarkdownToolbarAction::TableOfContents),
            FormatToolbarItem::Separator,
            FormatToolbarItem::Action(MarkdownToolbarAction::Table),
        ];

        let view_mode_icon = match self.view_mode {
            ViewMode::Rendered => ICON_VIEW_SOURCE,
            ViewMode::Source => ICON_VIEW_RENDERED,
        };
        let auto_save_enabled = self.auto_save_enabled;
        let auto_save_bg = if auto_save_enabled {
            c.selection.opacity(0.35)
        } else {
            c.dialog_surface
        };
        let auto_save_hover_bg = if auto_save_enabled {
            c.selection.opacity(0.5)
        } else {
            c.dialog_secondary_button_hover
        };
        let document_search_open = self.document_search.open;
        let document_search_bg = if document_search_open {
            c.selection.opacity(0.35)
        } else {
            c.dialog_surface
        };
        let document_search_hover_bg = if document_search_open {
            c.selection.opacity(0.5)
        } else {
            c.dialog_secondary_button_hover
        };
        let can_undo = self.can_undo();
        let can_redo = self.can_redo();
        let can_save = self.document_dirty;

        div()
            .id("markdown-format-toolbar")
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px(px(d.format_toolbar_padding_x))
            .py(px(d.format_toolbar_padding_y))
            .bg(c.dialog_surface)
            .border_b(px(d.format_toolbar_border_width))
            .border_color(c.dialog_border.opacity(0.65))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(d.format_toolbar_gap))
                    .child(Self::render_history_toolbar_button(
                        "undo-toolbar-button",
                        ICON_UNDO,
                        can_undo,
                        theme,
                        editor.clone(),
                        Self::undo_document,
                    ))
                    .child(Self::render_history_toolbar_button(
                        "redo-toolbar-button",
                        ICON_REDO,
                        can_redo,
                        theme,
                        editor.clone(),
                        Self::redo_document,
                    ))
                    .child(
                        div()
                            .id("markdown-format-history-separator")
                            .w(px(d.format_toolbar_separator_width))
                            .h(px(d.format_toolbar_separator_height))
                            .mx(px(d.format_toolbar_separator_margin_x))
                            .flex_shrink_0()
                            .bg(c.dialog_border.opacity(0.45)),
                    )
                    .children(items.into_iter().enumerate().map(|(index, item)| {
                        match item {
                            FormatToolbarItem::Separator => div()
                                .id(("markdown-format-separator", index))
                                .w(px(d.format_toolbar_separator_width))
                                .h(px(d.format_toolbar_separator_height))
                                .mx(px(d.format_toolbar_separator_margin_x))
                                .flex_shrink_0()
                                .bg(c.dialog_border.opacity(0.45))
                                .into_any_element(),
                            FormatToolbarItem::Action(action) => {
                                let icon_path = format_toolbar_icon_path(action);
                                let button_editor = editor.clone();
                                div()
                                    .id(("markdown-format-button", index))
                                    .w(px(d.format_toolbar_button_height))
                                    .h(px(d.format_toolbar_button_height))
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.format_toolbar_button_radius))
                                    .bg(c.dialog_surface)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .active(|this| this.opacity(0.92))
                                    .cursor_pointer()
                                    .child(
                                        svg()
                                            .path(icon_path)
                                            .size(icon_size)
                                            .text_color(icon_color),
                                    )
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        let _ = button_editor.update(cx, |editor, cx| {
                                            editor.apply_markdown_toolbar_format(action, window, cx);
                                        });
                                    })
                                    .into_any_element()
                            }
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(d.format_toolbar_gap))
                    .child({
                        let button_editor = editor.clone();
                        div()
                            .id("ai-toolbar-button")
                            .h(px(d.format_toolbar_button_height))
                            .px(px(10.0))
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .rounded(px(d.format_toolbar_button_radius))
                            .bg(c.dialog_surface)
                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                            .active(|this| this.opacity(0.92))
                            .cursor_pointer()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(icon_color)
                            .child(
                                svg()
                                    .path(ICON_AI_CUSTOM)
                                    .size(px(d.format_toolbar_icon_size))
                                    .text_color(icon_color),
                            )
                            .child("AI")
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                let _ = button_editor.update(cx, |editor, cx| {
                                    editor.on_ask_ai(&AskAi, window, cx);
                                });
                            })
                    })
                    .child(
                        div()
                            .id("document-search-toggle")
                            .w(px(d.format_toolbar_button_height))
                            .h(px(d.format_toolbar_button_height))
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.format_toolbar_button_radius))
                            .bg(document_search_bg)
                            .hover(|this| this.bg(document_search_hover_bg))
                            .active(|this| this.opacity(0.92))
                            .cursor_pointer()
                            .child(
                                svg()
                                    .path(ICON_SEARCH)
                                    .size(icon_size)
                                    .text_color(icon_color),
                            )
                            .on_click(cx.listener(Self::on_toggle_document_search_click)),
                    )
                    .child(Self::render_history_toolbar_button(
                        "save-toolbar-button",
                        ICON_SAVE,
                        can_save,
                        theme,
                        editor.clone(),
                        Self::request_save_document,
                    ))
                    .child(
                        div()
                            .id("auto-save-toggle")
                            .w(px(d.format_toolbar_button_height))
                            .h(px(d.format_toolbar_button_height))
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.format_toolbar_button_radius))
                            .bg(auto_save_bg)
                            .hover(|this| this.bg(auto_save_hover_bg))
                            .active(|this| this.opacity(0.92))
                            .cursor_pointer()
                            .child(
                                svg()
                                    .path(ICON_AUTO_SAVE)
                                    .size(icon_size)
                                    .text_color(icon_color),
                            )
                            .on_click(cx.listener(Self::on_toggle_auto_save)),
                    )
                    .child(
                        div()
                            .id("view-mode-toggle")
                            .w(px(d.format_toolbar_button_height))
                            .h(px(d.format_toolbar_button_height))
                            .flex()
                            .flex_shrink_0()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.format_toolbar_button_radius))
                            .bg(c.dialog_surface)
                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                            .active(|this| this.opacity(0.92))
                            .cursor_pointer()
                            .child(
                                svg()
                                    .path(view_mode_icon)
                                    .size(icon_size)
                                    .text_color(icon_color),
                            )
                            .on_click(cx.listener(Self::on_toggle_view_mode)),
                    ),
            )
    }

    fn render_history_toolbar_button(
        id: &'static str,
        icon_path: &'static str,
        enabled: bool,
        theme: &Theme,
        editor: WeakEntity<Self>,
        action: fn(&mut Self, &mut Context<Self>),
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let icon_color = c.dialog_secondary_button_text;
        let icon_size = px(d.format_toolbar_icon_size);
        let mut button = div()
            .id(id)
            .w(px(d.format_toolbar_button_height))
            .h(px(d.format_toolbar_button_height))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded(px(d.format_toolbar_button_radius))
            .bg(c.dialog_surface)
            .child(
                svg()
                    .path(icon_path)
                    .size(icon_size)
                    .text_color(icon_color),
            );

        if enabled {
            button = button
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .active(|this| this.opacity(0.92))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = editor.update(cx, |editor, cx| {
                        action(editor, cx);
                    });
                });
        } else {
            button = button.opacity(0.45);
        }

        button
    }

    fn append_code_block_from_toolbar(&mut self, cx: &mut Context<Self>) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let record = BlockRecord::new(
            BlockKind::CodeBlock {
                language: Some(SharedString::from("javascript")),
            },
            InlineTextTree::plain(String::new()),
        );
        let new_block = Self::new_block(cx, record);
        self.document
            .insert_blocks_at(None, self.document.root_count(), vec![new_block.clone()], cx);
        self.focus_block(new_block.entity_id());
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }

    fn apply_rendered_block_insert(
        &mut self,
        action: MarkdownToolbarAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MarkdownToolbarAction::Todo => {
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                let task1 = Self::new_block(
                    cx,
                    BlockRecord::with_plain_text(
                        BlockKind::TaskListItem { checked: false },
                        "待办事项 1",
                    ),
                );
                let task2 = Self::new_block(
                    cx,
                    BlockRecord::with_plain_text(
                        BlockKind::TaskListItem { checked: false },
                        "待办事项 2",
                    ),
                );
                let blocks = vec![task1.clone(), task2];
                self.insert_blocks_after_focused(blocks, window, cx);
                self.focus_block(task1.entity_id());
                self.mark_dirty(cx);
                self.request_active_block_scroll_into_view(cx);
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            MarkdownToolbarAction::HorizontalRule => {
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                let separator = Self::new_block(
                    cx,
                    BlockRecord::new(
                        BlockKind::Separator,
                        InlineTextTree::plain(String::new()),
                    ),
                );
                let blocks = vec![separator.clone()];
                self.insert_blocks_after_focused(blocks, window, cx);
                self.focus_block(separator.entity_id());
                self.mark_dirty(cx);
                self.request_active_block_scroll_into_view(cx);
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            MarkdownToolbarAction::Image => {
                if let Some(block) = self.focused_edit_target(window, cx) {
                    block.update(cx, |block, cx| {
                        block.insert_image_markdown(cx);
                    });
                } else {
                    self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                    let paragraph = Self::new_block(
                        cx,
                        BlockRecord::with_plain_text(
                            BlockKind::Paragraph,
                            "![alt text](https://vcg03.cfp.cn/creative/vcg/800/new/VCG41N1224074145.jpg)",
                        ),
                    );
                    self.document.insert_blocks_at(
                        None,
                        self.document.root_count(),
                        vec![paragraph.clone()],
                        cx,
                    );
                    self.focus_block(paragraph.entity_id());
                    self.finalize_pending_undo_capture(cx);
                }
                self.mark_dirty(cx);
                cx.notify();
            }
            MarkdownToolbarAction::TableOfContents => {
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                let heading = Self::new_block(
                    cx,
                    BlockRecord::with_plain_text(BlockKind::Heading { level: 2 }, "目录"),
                );
                let item1 = Self::new_block(
                    cx,
                    BlockRecord::with_plain_text(
                        BlockKind::BulletedListItem,
                        "[章节 1](#)",
                    ),
                );
                let item2 = Self::new_block(
                    cx,
                    BlockRecord::with_plain_text(
                        BlockKind::BulletedListItem,
                        "[章节 2](#)",
                    ),
                );
                let blocks = vec![heading.clone(), item1, item2];
                self.insert_blocks_after_focused(blocks, window, cx);
                self.focus_block(heading.entity_id());
                self.mark_dirty(cx);
                self.request_active_block_scroll_into_view(cx);
                self.finalize_pending_undo_capture(cx);
                cx.notify();
            }
            _ => {}
        }
    }

    fn insert_blocks_after_focused(
        &mut self,
        blocks: Vec<Entity<Block>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focused) = self.focused_edit_target(window, cx) {
            let focused_id = focused.entity_id();
            if let Some(location) = self.document.find_block_location(focused_id) {
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    blocks,
                    cx,
                );
                return;
            }
        }
        self.document
            .insert_blocks_at(None, self.document.root_count(), blocks, cx);
    }
}

fn format_toolbar_icon_path(action: MarkdownToolbarAction) -> &'static str {
    match action {
        MarkdownToolbarAction::Bold => ICON_BOLD,
        MarkdownToolbarAction::Italic => ICON_ITALIC,
        MarkdownToolbarAction::Heading1 => ICON_HEADING_1,
        MarkdownToolbarAction::Heading2 => ICON_HEADING_2,
        MarkdownToolbarAction::Heading3 => ICON_HEADING_3,
        MarkdownToolbarAction::OrderedList => ICON_LIST_ORDERED,
        MarkdownToolbarAction::UnorderedList => ICON_LIST_BULLET,
        MarkdownToolbarAction::Code => ICON_CODE,
        MarkdownToolbarAction::CodeBlock => ICON_SQUARE_CODE,
        MarkdownToolbarAction::Link => ICON_LINK,
        MarkdownToolbarAction::Quote => ICON_QUOTE,
        MarkdownToolbarAction::Table => ICON_TABLE,
        MarkdownToolbarAction::Todo => ICON_TODO,
        MarkdownToolbarAction::HorizontalRule => ICON_HORIZONTAL_RULE,
        MarkdownToolbarAction::Image => ICON_IMAGE,
        MarkdownToolbarAction::TableOfContents => ICON_TABLE_OF_CONTENTS,
    }
}
