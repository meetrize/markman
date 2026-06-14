//! Markdown formatting toolbar shown above the editor content area.

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::components::markdown::source_format::{MarkdownToolbarAction, apply_markdown_toolbar_action};
use crate::components::{AskAi, Block, BlockKind, BlockRecord, InlineTextTree, UndoCaptureKind};
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

use super::format_toolbar_overflow::{
    FormatToolbarControl, compute_format_toolbar_layout,
    format_toolbar_separator_belongs_to_right_section,
    is_format_toolbar_right_section_control,
};
use super::toolbar_button::{
    toolbar_icon_button, toolbar_icon_label_button_styled, ToolbarIconLabelStyle,
};
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
const ICON_ZOOM_IN: &str = "icon/toolbar/zoom-in.svg";
const ICON_ZOOM_OUT: &str = "icon/toolbar/zoom-out.svg";
const ICON_SAVE: &str = "icon/toolbar/save.svg";
const ICON_AUTO_SAVE: &str = "icon/toolbar/auto-save.svg";
const ICON_SEARCH: &str = "icon/toolbar/search.svg";
const ICON_AI_CUSTOM: &str = "icon/toolbar/sparkles.svg";
const ICON_WORKFLOW: &str = "icon/toolbar/workflow.svg";
const ICON_OVERFLOW: &str = "icon/toolbar/chevron-down.svg";

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
    fn label(self, strings: &I18nStrings) -> String {
        match self {
            Self::Flowchart => strings.mermaid_template_flowchart.clone(),
            Self::MindMap => strings.mermaid_template_mind_map.clone(),
            Self::Sequence => strings.mermaid_template_sequence.clone(),
            Self::Gantt => strings.mermaid_template_gantt.clone(),
            Self::State => strings.mermaid_template_state.clone(),
            Self::Class => strings.mermaid_template_class.clone(),
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
        self.format_toolbar_overflow_menu_position = None;
        self.mermaid_template_menu_position = if self.mermaid_template_menu_position.is_some() {
            None
        } else {
            Some(position)
        };
        cx.notify();
    }

    fn toggle_format_toolbar_overflow_menu_at(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.mermaid_template_menu_position = None;
        self.format_toolbar_overflow_menu_position =
            if self.format_toolbar_overflow_menu_position.is_some() {
                None
            } else {
                Some(position)
            };
        cx.notify();
    }

    pub(super) fn close_format_toolbar_overflow_menu(&mut self, cx: &mut Context<Self>) {
        if self.format_toolbar_overflow_menu_position.take().is_some() {
            cx.notify();
        }
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
        let editor = cx.entity().downgrade();
        let all_controls = self.resolved_format_toolbar_controls();
        let layout = compute_format_toolbar_layout(&all_controls, self.format_toolbar_width, d);
        let view_mode_icon = match self.view_mode {
            ViewMode::Rendered => ICON_VIEW_SOURCE,
            ViewMode::Source => ICON_VIEW_RENDERED,
        };
        let toolbar_state = FormatToolbarRenderState {
            theme,
            editor: editor.clone(),
            view_mode_icon,
            auto_save_enabled: self.auto_save_enabled,
            document_search_open: self.search.state.open,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            can_save: self.document_dirty,
            mermaid_menu_open: self.mermaid_template_menu_position.is_some(),
            overflow_menu_open: self.format_toolbar_overflow_menu_position.is_some(),
        };
        let mut overflow_button_rendered = false;
        let mut left_toolbar_children = Vec::new();
        let mut right_toolbar_children = Vec::new();

        for (index, control) in all_controls.iter().copied().enumerate() {
            if layout.visible.contains(&control) {
                let element = Self::render_format_toolbar_control(control, &toolbar_state, cx)
                    .into_any_element();
                if is_format_toolbar_right_section_control(control)
                    || matches!(
                        control,
                        FormatToolbarControl::HistorySeparator
                            | FormatToolbarControl::FormatSeparator
                    )
                        && format_toolbar_separator_belongs_to_right_section(
                            &all_controls,
                            index,
                            &layout.visible,
                        )
                {
                    right_toolbar_children.push(element);
                } else {
                    left_toolbar_children.push(element);
                }
            } else if layout.overflow.contains(&control) && !overflow_button_rendered {
                right_toolbar_children.push(
                    Self::render_format_toolbar_overflow_button(&toolbar_state).into_any_element(),
                );
                overflow_button_rendered = true;
            }
        }

        if !overflow_button_rendered && !layout.overflow.is_empty() {
            right_toolbar_children.push(
                Self::render_format_toolbar_overflow_button(&toolbar_state).into_any_element(),
            );
        }

        let width_probe_editor = editor;
        div()
            .id("markdown-format-toolbar")
            .w_full()
            .flex_shrink_0()
            .relative()
            .px(px(d.format_toolbar_padding_x))
            .py(px(d.format_toolbar_padding_y))
            .bg(c.dialog_surface)
            .border_b(px(d.format_toolbar_border_width))
            .border_color(c.dialog_border.opacity(0.65))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::on_format_toolbar_context_menu_mouse_down),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let width = f32::from(bounds.size.width);
                        let _ = width_probe_editor.update(cx, |editor, cx| {
                            if (editor.format_toolbar_width - width).abs() > 0.5 {
                                editor.format_toolbar_width = width;
                                cx.notify();
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(d.format_toolbar_gap))
                            .overflow_hidden()
                            .children(left_toolbar_children),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(d.format_toolbar_gap))
                            .flex_shrink_0()
                            .children(right_toolbar_children),
                    ),
            )
    }

    fn render_format_toolbar_overflow_button(state: &FormatToolbarRenderState<'_>) -> impl IntoElement {
        let d = &state.theme.dimensions;
        let menu_offset_y = px(d.format_toolbar_button_height + 6.0);
        let editor = state.editor.clone();
        toolbar_icon_button(
            "format-toolbar-overflow-button",
            state.theme,
            ICON_OVERFLOW,
            state.overflow_menu_open,
            false,
            "",
            false,
        )
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            cx.stop_propagation();
            let position = point(event.position.x, event.position.y + menu_offset_y);
            let _ = editor.update(cx, |editor, cx| {
                editor.toggle_format_toolbar_overflow_menu_at(position, cx);
            });
        })
    }

    fn render_format_toolbar_control(
        control: FormatToolbarControl,
        state: &FormatToolbarRenderState<'_>,
        cx: &mut Context<Editor>,
    ) -> impl IntoElement {
        let c = &state.theme.colors;
        let d = &state.theme.dimensions;
        match control {
            FormatToolbarControl::Undo => Self::render_history_toolbar_button(
                "undo-toolbar-button",
                ICON_UNDO,
                state.can_undo,
                state.theme,
                state.editor.clone(),
                Self::undo_document,
            )
            .into_any_element(),
            FormatToolbarControl::Redo => Self::render_history_toolbar_button(
                "redo-toolbar-button",
                ICON_REDO,
                state.can_redo,
                state.theme,
                state.editor.clone(),
                Self::redo_document,
            )
            .into_any_element(),
            FormatToolbarControl::HistorySeparator => div()
                .id("markdown-format-history-separator")
                .w(px(d.format_toolbar_separator_width))
                .h(px(d.format_toolbar_separator_height))
                .mx(px(d.format_toolbar_separator_margin_x))
                .flex_shrink_0()
                .bg(c.dialog_border.opacity(0.45))
                .into_any_element(),
            FormatToolbarControl::FormatSeparator => div()
                .id("markdown-format-separator")
                .w(px(d.format_toolbar_separator_width))
                .h(px(d.format_toolbar_separator_height))
                .mx(px(d.format_toolbar_separator_margin_x))
                .flex_shrink_0()
                .bg(c.dialog_border.opacity(0.45))
                .into_any_element(),
            FormatToolbarControl::Format(action) => {
                let icon_path = format_toolbar_icon_path(action);
                let button_editor = state.editor.clone();
                toolbar_icon_button(
                    format_action_element_id(action),
                    state.theme,
                    icon_path,
                    false,
                    false,
                    "",
                    false,
                )
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = button_editor.update(cx, |editor, cx| {
                        editor.apply_markdown_toolbar_format(action, window, cx);
                    });
                })
                .into_any_element()
            }
            FormatToolbarControl::MermaidTemplate => Self::render_mermaid_template_button(
                state.theme,
                state.editor.clone(),
                state.mermaid_menu_open,
            )
            .into_any_element(),
            FormatToolbarControl::Ai => {
                let button_editor = state.editor.clone();
                toolbar_icon_label_button_styled(
                    "ai-toolbar-button",
                    ICON_AI_CUSTOM,
                    "AI",
                    state.theme,
                    "",
                    ToolbarIconLabelStyle::format_toolbar(state.theme),
                )
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = button_editor.update(cx, |editor, cx| {
                        editor.on_ask_ai(&AskAi, window, cx);
                    });
                })
                .into_any_element()
            }
            FormatToolbarControl::DocumentSearch => toolbar_icon_button(
                "document-search-toggle",
                state.theme,
                ICON_SEARCH,
                state.document_search_open,
                false,
                "",
                false,
            )
            .on_click(cx.listener(Self::on_toggle_document_search_click))
            .into_any_element(),
            FormatToolbarControl::Save => Self::render_history_toolbar_button(
                "save-toolbar-button",
                ICON_SAVE,
                state.can_save,
                state.theme,
                state.editor.clone(),
                Self::request_save_document,
            )
            .into_any_element(),
            FormatToolbarControl::AutoSave => toolbar_icon_button(
                "auto-save-toggle",
                state.theme,
                ICON_AUTO_SAVE,
                state.auto_save_enabled,
                false,
                "",
                false,
            )
            .on_click(cx.listener(Self::on_toggle_auto_save))
            .into_any_element(),
            FormatToolbarControl::ZoomOut => toolbar_icon_button(
                "document-zoom-out",
                state.theme,
                ICON_ZOOM_OUT,
                false,
                false,
                "",
                false,
            )
            .on_click(cx.listener(Self::on_zoom_document_out_click))
            .into_any_element(),
            FormatToolbarControl::ZoomIn => toolbar_icon_button(
                "document-zoom-in",
                state.theme,
                ICON_ZOOM_IN,
                false,
                false,
                "",
                false,
            )
            .on_click(cx.listener(Self::on_zoom_document_in_click))
            .into_any_element(),
            FormatToolbarControl::ViewMode => toolbar_icon_button(
                "view-mode-toggle",
                state.theme,
                state.view_mode_icon,
                false,
                false,
                "",
                false,
            )
            .on_click(cx.listener(Self::on_toggle_view_mode))
            .into_any_element(),
        }
    }

    pub(super) fn render_format_toolbar_overflow_menu_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let position = self.format_toolbar_overflow_menu_position?;
        let all_controls = self.resolved_format_toolbar_controls();
        let layout = compute_format_toolbar_layout(&all_controls, self.format_toolbar_width, &theme.dimensions);
        if layout.overflow.is_empty() {
            return None;
        }

        let c = &theme.colors;
        let d = &theme.dimensions;
        let editor = cx.entity().downgrade();
        let view_mode_icon = match self.view_mode {
            ViewMode::Rendered => ICON_VIEW_SOURCE,
            ViewMode::Source => ICON_VIEW_RENDERED,
        };
        let toolbar_state = FormatToolbarRenderState {
            theme,
            editor: editor.clone(),
            view_mode_icon,
            auto_save_enabled: self.auto_save_enabled,
            document_search_open: self.search.state.open,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            can_save: self.document_dirty,
            mermaid_menu_open: self.mermaid_template_menu_position.is_some(),
            overflow_menu_open: true,
        };

        Some(
            div()
                .id("format-toolbar-overflow-menu")
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(d.format_toolbar_button_height + 8.0))
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
                .children(
                    layout
                        .overflow
                        .into_iter()
                        .filter(|control| {
                            !matches!(
                                control,
                                FormatToolbarControl::HistorySeparator
                                    | FormatToolbarControl::FormatSeparator
                            )
                        })
                        .enumerate()
                        .map(|(index, control)| {
                            let item_editor = editor.clone();
                            let menu_offset_y = px(d.format_toolbar_button_height + 6.0);
                            Self::render_format_toolbar_overflow_menu_item(
                                index,
                                control,
                                &toolbar_state,
                            )
                            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                cx.stop_propagation();
                                let _ = item_editor.update(cx, |editor, cx| {
                                    editor.close_format_toolbar_overflow_menu(cx);
                                    if control == FormatToolbarControl::MermaidTemplate {
                                        editor.toggle_mermaid_template_menu_at(
                                            point(
                                                event.position.x,
                                                event.position.y + menu_offset_y,
                                            ),
                                            cx,
                                        );
                                    } else {
                                        editor.activate_format_toolbar_control(
                                            control,
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            })
                        }),
                )
                .into_any_element(),
        )
    }

    fn render_format_toolbar_overflow_menu_item(
        index: usize,
        control: FormatToolbarControl,
        state: &FormatToolbarRenderState<'_>,
    ) -> Stateful<Div> {
        let c = &state.theme.colors;
        let d = &state.theme.dimensions;
        let (icon_path, active, disabled, muted_disabled_icon) =
            format_toolbar_overflow_menu_item_visuals(control, state);

        div()
            .id(("format-toolbar-overflow-menu-item", index))
            .w(px(d.format_toolbar_button_height))
            .h(px(d.format_toolbar_button_height))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .when(!disabled, |this| {
                this.hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
            })
            .when(disabled, |this| this.opacity(0.45))
            .child(
                svg()
                    .path(icon_path)
                    .size(px(d.format_toolbar_icon_size))
                    .text_color(if disabled && muted_disabled_icon {
                        c.dialog_muted
                    } else if active {
                        c.text_default
                    } else {
                        c.dialog_secondary_button_text
                    }),
            )
    }

    fn activate_format_toolbar_control(
        &mut self,
        control: FormatToolbarControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match control {
            FormatToolbarControl::Undo => Self::undo_document(self, cx),
            FormatToolbarControl::Redo => Self::redo_document(self, cx),
            FormatToolbarControl::Format(action) => {
                self.apply_markdown_toolbar_format(action, window, cx);
            }
            FormatToolbarControl::Ai => self.on_ask_ai(&AskAi, window, cx),
            FormatToolbarControl::DocumentSearch => self.toggle_document_search(window, cx),
            FormatToolbarControl::Save => Self::request_save_document(self, cx),
            FormatToolbarControl::AutoSave => {
                self.on_toggle_auto_save(&ClickEvent::default(), window, cx);
            }
            FormatToolbarControl::ZoomOut => self.zoom_document_out(cx),
            FormatToolbarControl::ZoomIn => self.zoom_document_in(cx),
            FormatToolbarControl::ViewMode => self.toggle_view_mode(cx),
            FormatToolbarControl::MermaidTemplate
            | FormatToolbarControl::HistorySeparator
            | FormatToolbarControl::FormatSeparator => {}
        }
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
        let strings = cx.global::<I18nManager>().strings().clone();
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
                        .child(template.label(&strings))
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

struct FormatToolbarRenderState<'a> {
    theme: &'a Theme,
    editor: WeakEntity<Editor>,
    view_mode_icon: &'static str,
    auto_save_enabled: bool,
    document_search_open: bool,
    can_undo: bool,
    can_redo: bool,
    can_save: bool,
    mermaid_menu_open: bool,
    overflow_menu_open: bool,
}

fn format_action_element_id(action: MarkdownToolbarAction) -> SharedString {
    SharedString::from(format!(
        "markdown-format-{}",
        format_action_element_key(action)
    ))
}

fn format_action_element_key(action: MarkdownToolbarAction) -> &'static str {
    match action {
        MarkdownToolbarAction::Bold => "bold",
        MarkdownToolbarAction::Italic => "italic",
        MarkdownToolbarAction::Heading1 => "heading-1",
        MarkdownToolbarAction::Heading2 => "heading-2",
        MarkdownToolbarAction::Heading3 => "heading-3",
        MarkdownToolbarAction::OrderedList => "ordered-list",
        MarkdownToolbarAction::UnorderedList => "unordered-list",
        MarkdownToolbarAction::Code => "code",
        MarkdownToolbarAction::CodeBlock => "code-block",
        MarkdownToolbarAction::Link => "link",
        MarkdownToolbarAction::Quote => "quote",
        MarkdownToolbarAction::Table => "table",
        MarkdownToolbarAction::Todo => "todo",
        MarkdownToolbarAction::HorizontalRule => "horizontal-rule",
        MarkdownToolbarAction::Image => "image",
        MarkdownToolbarAction::TableOfContents => "table-of-contents",
    }
}

fn format_toolbar_overflow_menu_item_visuals(
    control: FormatToolbarControl,
    state: &FormatToolbarRenderState<'_>,
) -> (&'static str, bool, bool, bool) {
    match control {
        FormatToolbarControl::Undo => (ICON_UNDO, false, !state.can_undo, false),
        FormatToolbarControl::Redo => (ICON_REDO, false, !state.can_redo, false),
        FormatToolbarControl::Format(action) => (format_toolbar_icon_path(action), false, false, false),
        FormatToolbarControl::MermaidTemplate => (ICON_WORKFLOW, state.mermaid_menu_open, false, false),
        FormatToolbarControl::Ai => (ICON_AI_CUSTOM, false, false, false),
        FormatToolbarControl::DocumentSearch => {
            (ICON_SEARCH, state.document_search_open, false, false)
        }
        FormatToolbarControl::Save => (ICON_SAVE, false, !state.can_save, false),
        FormatToolbarControl::AutoSave => (ICON_AUTO_SAVE, state.auto_save_enabled, false, false),
        FormatToolbarControl::ZoomOut => (ICON_ZOOM_OUT, false, false, false),
        FormatToolbarControl::ZoomIn => (ICON_ZOOM_IN, false, false, false),
        FormatToolbarControl::ViewMode => (state.view_mode_icon, false, false, false),
        FormatToolbarControl::HistorySeparator | FormatToolbarControl::FormatSeparator => {
            (ICON_OVERFLOW, false, true, false)
        }
    }
}
