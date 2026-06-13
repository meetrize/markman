//! Markdown formatting toolbar shown above the editor content area.

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::components::markdown::source_format::{MarkdownToolbarAction, apply_markdown_toolbar_action};
use crate::components::{AskAi, Block, BlockKind, BlockRecord, InlineTextTree, UndoCaptureKind, toolbar_icon_button};
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
const ICON_WORKFLOW: &str = "icon/toolbar/workflow.svg";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MermaidTemplate {
    Flowchart,
    MindMap,
    Sequence,
    Gantt,
    State,
    Class,
}

impl MermaidTemplate {
    fn label(self) -> &'static str {
        match self {
            Self::Flowchart => "流程图",
            Self::MindMap => "思维导图",
            Self::Sequence => "时序图",
            Self::Gantt => "甘特图",
            Self::State => "状态图",
            Self::Class => "类图",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::Flowchart => "flowchart TD\n    A[开始] --> B{是否满足条件?}\n    B -- 是 --> C[执行任务]\n    B -- 否 --> D[调整方案]\n    C --> E[结束]\n    D --> B",
            Self::MindMap => "mindmap\n  root((项目计划))\n    目标\n      业务目标\n      用户目标\n    阶段\n      调研\n      设计\n      开发\n      发布\n    风险\n      需求变化\n      时间紧张",
            Self::Sequence => "sequenceDiagram\n    participant 用户\n    participant 前端\n    participant 服务端\n    用户->>前端: 提交请求\n    前端->>服务端: 调用 API\n    服务端-->>前端: 返回结果\n    前端-->>用户: 展示结果",
            Self::Gantt => "gantt\n    title 项目排期\n    dateFormat  YYYY-MM-DD\n    section 准备\n    需求调研      :a1, 2026-01-01, 3d\n    原型设计      :a2, after a1, 4d\n    section 实施\n    开发实现      :b1, after a2, 7d\n    测试验收      :b2, after b1, 3d",
            Self::State => "stateDiagram-v2\n    [*] --> 待处理\n    待处理 --> 处理中: 开始\n    处理中 --> 已完成: 完成\n    处理中 --> 待处理: 退回\n    已完成 --> [*]",
            Self::Class => "classDiagram\n    class 用户 {\n      +String 姓名\n      +登录()\n    }\n    class 订单 {\n      +String 编号\n      +支付()\n    }\n    用户 \"1\" --> \"*\" 订单",
        }
    }

    fn fenced(self) -> String {
        format!("```mermaid\n{}\n```", self.source())
    }
}

const MERMAID_TEMPLATES: [MermaidTemplate; 6] = [
    MermaidTemplate::Flowchart,
    MermaidTemplate::MindMap,
    MermaidTemplate::Sequence,
    MermaidTemplate::Gantt,
    MermaidTemplate::State,
    MermaidTemplate::Class,
];

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

    fn toggle_mermaid_template_menu_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.mermaid_template_menu_position = if self.mermaid_template_menu_position.is_some() {
            None
        } else {
            Some(position)
        };
        cx.notify();
    }

    fn insert_mermaid_template(
        &mut self,
        template: MermaidTemplate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mermaid_template_menu_position = None;
        let markdown = template.fenced();
        if self.view_mode == ViewMode::Source {
            self.insert_mermaid_template_in_source(&markdown, cx);
        } else {
            self.insert_mermaid_template_in_rendered(&markdown, window, cx);
        }
    }

    fn insert_mermaid_template_in_source(&mut self, markdown: &str, cx: &mut Context<Self>) {
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
            let prefix = if selection.start == 0 {
                String::new()
            } else if text[..selection.start].ends_with("\n\n") {
                String::new()
            } else if text[..selection.start].ends_with('\n') {
                "\n".to_string()
            } else {
                "\n\n".to_string()
            };
            let suffix = if selection.end == text.len() || text[selection.end..].starts_with('\n') {
                "\n".to_string()
            } else {
                "\n\n".to_string()
            };
            let insertion = format!("{prefix}{markdown}{suffix}");
            let cursor = selection.start + insertion.len();
            block.replace_text_in_visible_range(selection, &insertion, Some(cursor..cursor), true, cx);
            block.selection_reversed = false;
            block.marked_range = None;
            block.cursor_blink_epoch = std::time::Instant::now();
            cx.emit(crate::components::BlockEvent::Changed);
            cx.notify();
        });
        self.mark_dirty(cx);
        cx.notify();
    }

    fn insert_mermaid_template_in_rendered(
        &mut self,
        markdown: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let block = Self::new_block(cx, BlockRecord::mermaid(markdown));
        self.insert_blocks_after_focused(vec![block.clone()], window, cx);
        self.focus_block(block.entity_id());
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        self.finalize_pending_undo_capture(cx);
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
        let document_search_open = self.search.state.open;
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
                    }))
                    .child(Self::render_mermaid_template_button(
                        theme,
                        editor.clone(),
                        self.mermaid_template_menu_position.is_some(),
                    )),
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
                    .child({
                        let _button_editor = editor.clone();
                        toolbar_icon_button(
                            "document-search-toggle",
                            theme,
                            ICON_SEARCH,
                            document_search_open,
                            false,
                            "",
                            false,
                        )
                        .on_click(cx.listener(Self::on_toggle_document_search_click))
                    })
                    .child(Self::render_history_toolbar_button(
                        "save-toolbar-button",
                        ICON_SAVE,
                        can_save,
                        theme,
                        editor.clone(),
                        Self::request_save_document,
                    ))
                    .child(
                        toolbar_icon_button(
                            "auto-save-toggle",
                            theme,
                            ICON_AUTO_SAVE,
                            auto_save_enabled,
                            false,
                            "",
                            false,
                        )
                        .on_click(cx.listener(Self::on_toggle_auto_save)),
                    )
                    .child(
                        toolbar_icon_button(
                            "view-mode-toggle",
                            theme,
                            view_mode_icon,
                            false,
                            false,
                            "",
                            false,
                        )
                        .on_click(cx.listener(Self::on_toggle_view_mode)),
                    ),
            )
    }

    fn render_mermaid_template_button(
        theme: &Theme,
        editor: WeakEntity<Self>,
        menu_open: bool,
    ) -> impl IntoElement {
        let d = &theme.dimensions;
        let menu_offset_y = px(d.format_toolbar_button_height + 6.0);
        toolbar_icon_button(
            "mermaid-template-toolbar-button",
            theme,
            ICON_WORKFLOW,
            menu_open,
            false,
            "",
            false,
        )
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                cx.stop_propagation();
                let position = point(event.position.x, event.position.y + menu_offset_y);
                let _ = editor.update(cx, |editor, cx| {
                    editor.toggle_mermaid_template_menu_at(position, cx);
                });
            })
    }

    pub(super) fn render_mermaid_template_menu_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let position = self.mermaid_template_menu_position?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let editor = cx.entity().downgrade();
        Some(
            div()
                .id("mermaid-template-menu")
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(132.0))
                .p(px(4.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .rounded(px(d.menu_panel_radius))
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .shadow_lg()
                .occlude()
                .children(MERMAID_TEMPLATES.into_iter().enumerate().map(|(index, template)| {
                    let item_editor = editor.clone();
                    div()
                        .id(("mermaid-template-menu-item", index))
                        .h(px(26.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .text_size(px(12.0))
                        .text_color(c.text_default)
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .cursor_pointer()
                        .child(template.label())
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = item_editor.update(cx, |editor, cx| {
                                editor.insert_mermaid_template(template, window, cx);
                            });
                        })
                }))
                .into_any_element(),
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
        toolbar_icon_button(id, theme, icon_path, false, !enabled, "", false).when(enabled, |this| {
            this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = editor.update(cx, |editor, cx| {
                    action(editor, cx);
                });
            })
        })
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
