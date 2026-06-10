//! Markdown formatting toolbar shown above the editor content area.

use gpui::*;

use crate::components::markdown::source_format::{MarkdownToolbarAction, apply_markdown_toolbar_action};
use crate::components::UndoCaptureKind;
use crate::theme::Theme;

use super::Editor;
use super::ViewMode;

const ICON_BOLD: &str = "icon/toolbar/bold.svg";
const ICON_ITALIC: &str = "icon/toolbar/italic.svg";
const ICON_HEADING_1: &str = "icon/toolbar/heading-1.svg";
const ICON_HEADING_2: &str = "icon/toolbar/heading-2.svg";
const ICON_HEADING_3: &str = "icon/toolbar/heading-3.svg";
const ICON_LIST_ORDERED: &str = "icon/toolbar/list-ordered.svg";
const ICON_LIST_BULLET: &str = "icon/toolbar/list-bullet.svg";
const ICON_CODE: &str = "icon/toolbar/code.svg";
const ICON_LINK: &str = "icon/toolbar/link.svg";
const ICON_QUOTE: &str = "icon/toolbar/quote.svg";
const ICON_TABLE: &str = "icon/toolbar/table.svg";
const ICON_VIEW_SOURCE: &str = "icon/toolbar/view-source.svg";
const ICON_VIEW_RENDERED: &str = "icon/toolbar/view-rendered.svg";
const ICON_AUTO_SAVE: &str = "icon/toolbar/auto-save.svg";
const ICON_SEARCH: &str = "icon/toolbar/search.svg";

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
            FormatToolbarItem::Action(MarkdownToolbarAction::Link),
            FormatToolbarItem::Action(MarkdownToolbarAction::Quote),
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
        MarkdownToolbarAction::Link => ICON_LINK,
        MarkdownToolbarAction::Quote => ICON_QUOTE,
        MarkdownToolbarAction::Table => ICON_TABLE,
    }
}
