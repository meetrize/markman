//! Format toolbar context menu and customization dialog.

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::config::format_toolbar::{
    default_format_toolbar_button_configs, normalize_format_toolbar_button_configs,
};
use crate::config::{read_app_preferences, update_app_preferences};
use crate::editor::format_toolbar_overflow::FormatToolbarControl;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

use super::format_toolbar_overflow::format_toolbar_controls_from_config;
use super::Editor;

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
const ICON_ZOOM_IN: &str = "icon/toolbar/zoom-in.svg";
const ICON_ZOOM_OUT: &str = "icon/toolbar/zoom-out.svg";
const ICON_SAVE: &str = "icon/toolbar/save.svg";
const ICON_AUTO_SAVE: &str = "icon/toolbar/auto-save.svg";
const ICON_SEARCH: &str = "icon/toolbar/search.svg";
const ICON_AI_CUSTOM: &str = "icon/toolbar/sparkles.svg";
const ICON_WORKFLOW: &str = "icon/toolbar/workflow.svg";

impl Editor {
    pub(super) fn resolved_format_toolbar_controls(&self) -> Vec<FormatToolbarControl> {
        let configs = read_app_preferences()
            .map(|preferences| preferences.format_toolbar)
            .unwrap_or_else(|_| default_format_toolbar_button_configs());
        format_toolbar_controls_from_config(&configs)
    }

    pub(super) fn on_format_toolbar_context_menu_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Right {
            return;
        }
        cx.stop_propagation();
        self.close_format_toolbar_overflow_menu(cx);
        self.format_toolbar_context_menu = Some(event.position);
        cx.notify();
    }

    pub(super) fn close_format_toolbar_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.format_toolbar_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn open_format_toolbar_customize_dialog(&mut self, cx: &mut Context<Self>) {
        self.format_toolbar_context_menu = None;
        let draft = read_app_preferences()
            .map(|preferences| preferences.format_toolbar)
            .unwrap_or_else(|_| default_format_toolbar_button_configs());
        self.format_toolbar_customize_dialog =
            Some(normalize_format_toolbar_button_configs(draft));
        cx.notify();
    }

    pub(super) fn close_format_toolbar_customize_dialog(&mut self, cx: &mut Context<Self>) {
        if self.format_toolbar_customize_dialog.take().is_some() {
            cx.notify();
        }
    }

    fn on_open_format_toolbar_customize_dialog(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_format_toolbar_customize_dialog(cx);
    }

    fn on_dismiss_format_toolbar_context_menu(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_format_toolbar_context_menu(cx);
    }

    fn on_dismiss_format_toolbar_customize_dialog(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_format_toolbar_customize_dialog(cx);
    }

    fn on_cancel_format_toolbar_customize_dialog(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_format_toolbar_customize_dialog(cx);
    }

    fn on_confirm_format_toolbar_customize_dialog(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.format_toolbar_customize_dialog.take() else {
            return;
        };
        let normalized = normalize_format_toolbar_button_configs(draft);
        if let Err(err) = update_app_preferences(|preferences| {
            preferences.format_toolbar = normalized;
        }) {
            eprintln!("failed to save format toolbar preferences: {err}");
        }
        cx.notify();
    }

    fn on_reset_format_toolbar_customize_dialog(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.format_toolbar_customize_dialog =
            Some(default_format_toolbar_button_configs());
        cx.notify();
    }

    fn move_format_toolbar_customize_button(
        &mut self,
        index: usize,
        delta: isize,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.format_toolbar_customize_dialog.as_mut() else {
            return;
        };
        let target = index as isize + delta;
        if target < 0 || target as usize >= draft.len() {
            return;
        }
        draft.swap(index, target as usize);
        cx.notify();
    }

    fn toggle_format_toolbar_customize_button(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.format_toolbar_customize_dialog.as_mut() else {
            return;
        };
        if let Some(button) = draft.get_mut(index) {
            button.enabled = !button.enabled;
            cx.notify();
        }
    }

    pub(super) fn render_format_toolbar_context_menu_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let position = self.format_toolbar_context_menu?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let panel_width = px(d.context_menu_panel_width.max(168.0));

        Some(
            div()
                .id("format-toolbar-context-menu-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_format_toolbar_context_menu),
                )
                .child(
                    div()
                        .id("format-toolbar-context-menu-panel")
                        .absolute()
                        .left(position.x)
                        .top(position.y)
                        .w(panel_width)
                        .p(px(d.menu_panel_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.menu_panel_gap))
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
                                .id("format-toolbar-context-menu-customize")
                                .w_full()
                                .h(px(d.menu_item_height))
                                .px(px(d.menu_item_padding_x))
                                .flex()
                                .items_center()
                                .rounded(px(d.menu_item_radius))
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .cursor_pointer()
                                .text_size(px(d.menu_text_size))
                                .font_weight(t.dialog_body_weight.to_font_weight())
                                .text_color(c.dialog_body)
                                .child(s.format_toolbar_customize_menu.clone())
                                .on_click(cx.listener(Self::on_open_format_toolbar_customize_dialog)),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_format_toolbar_customize_dialog_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self.format_toolbar_customize_dialog.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let button_count = draft.len();

        let mut rows = div().flex().flex_col().gap(px(8.0)).w_full();
        for (index, button) in draft.iter().enumerate() {
            let enabled_label = if button.enabled {
                s.format_toolbar_customize_enabled.clone()
            } else {
                s.format_toolbar_customize_disabled.clone()
            };
            let icon_path = format_toolbar_button_icon_path(&button.id);
            let label = format_toolbar_button_label(&s, &button.id);

            rows = rows.child(
                div()
                    .id(("format-toolbar-customize-item", index))
                    .w_full()
                    .p(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(d.menu_item_radius))
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .bg(c.dialog_secondary_button_bg)
                    .child(
                        div()
                            .id(("format-toolbar-customize-up", index))
                            .size(px(26.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.menu_item_radius))
                            .when(index > 0, |this| {
                                this.bg(c.dialog_surface)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .on_click({
                                        let index = index;
                                        cx.listener(move |this, _, _, cx| {
                                            this.move_format_toolbar_customize_button(index, -1, cx);
                                        })
                                    })
                            })
                            .when(index == 0, |this| this.opacity(0.35))
                            .child(
                                svg()
                                    .path("icon/toolbar/chevron-up.svg")
                                    .size(px(14.0))
                                    .text_color(c.dialog_secondary_button_text),
                            ),
                    )
                    .child(
                        div()
                            .id(("format-toolbar-customize-down", index))
                            .size(px(26.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.menu_item_radius))
                            .when(index + 1 < button_count, |this| {
                                this.bg(c.dialog_surface)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .on_click({
                                        let index = index;
                                        cx.listener(move |this, _, _, cx| {
                                            this.move_format_toolbar_customize_button(index, 1, cx);
                                        })
                                    })
                            })
                            .when(index + 1 >= button_count, |this| this.opacity(0.35))
                            .child(
                                svg()
                                    .path("icon/toolbar/chevron-down.svg")
                                    .size(px(14.0))
                                    .text_color(c.dialog_secondary_button_text),
                            ),
                    )
                    .child(
                        div()
                            .id(("format-toolbar-customize-enabled", index))
                            .h(px(26.0))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.menu_item_radius))
                            .bg(c.dialog_surface)
                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                            .cursor_pointer()
                            .text_size(px(12.0))
                            .text_color(c.dialog_secondary_button_text)
                            .child(enabled_label)
                            .on_click({
                                let index = index;
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_format_toolbar_customize_button(index, cx);
                                })
                            }),
                    )
                    .child(
                        div()
                            .size(px(26.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                svg()
                                    .path(icon_path)
                                    .size(px(14.0))
                                    .text_color(c.dialog_secondary_button_text),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(t.dialog_body_size))
                            .text_color(c.dialog_body)
                            .child(label),
                    ),
            );
        }

        Some(
            div()
                .id("format-toolbar-customize-dialog-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_format_toolbar_customize_dialog),
                )
                .child(
                    div()
                        .w_full()
                        .px(px(d.editor_padding))
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("format-toolbar-customize-dialog")
                                .w(px(520.0))
                                .max_w(relative(1.0))
                                .max_h(relative(0.82))
                                .p(px(d.dialog_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.dialog_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.dialog_radius))
                                .shadow_lg()
                                .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                                    cx.stop_propagation();
                                })
                                .child(
                                    div()
                                        .text_size(px(t.dialog_title_size))
                                        .font_weight(t.dialog_title_weight.to_font_weight())
                                        .text_color(c.dialog_title)
                                        .child(s.format_toolbar_customize_title.clone()),
                                )
                                .child(
                                    div()
                                        .id("format-toolbar-customize-list")
                                        .w_full()
                                        .flex_1()
                                        .min_h(px(0.0))
                                        .max_h(px(420.0))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .child(rows),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
                                        .gap(px(d.dialog_button_gap))
                                        .child(
                                            div()
                                                .id("format-toolbar-customize-reset")
                                                .h(px(d.dialog_button_height))
                                                .px(px(d.dialog_button_padding_x))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                .border(px(d.dialog_border_width))
                                                .border_color(c.dialog_border)
                                                .bg(c.dialog_secondary_button_bg)
                                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                                .cursor_pointer()
                                                .text_size(px(t.dialog_button_size))
                                                .font_weight(t.dialog_button_weight.to_font_weight())
                                                .text_color(c.dialog_secondary_button_text)
                                                .child(s.format_toolbar_customize_reset.clone())
                                                .on_click(cx.listener(Self::on_reset_format_toolbar_customize_dialog)),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .gap(px(d.dialog_button_gap))
                                                .child(
                                                    div()
                                                        .id("format-toolbar-customize-cancel")
                                                        .h(px(d.dialog_button_height))
                                                        .px(px(d.dialog_button_padding_x))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                        .border(px(d.dialog_border_width))
                                                        .border_color(c.dialog_border)
                                                        .bg(c.dialog_secondary_button_bg)
                                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                                        .cursor_pointer()
                                                        .text_size(px(t.dialog_button_size))
                                                        .font_weight(t.dialog_button_weight.to_font_weight())
                                                        .text_color(c.dialog_secondary_button_text)
                                                        .child(s.table_insert_cancel.clone())
                                                        .on_click(cx.listener(Self::on_cancel_format_toolbar_customize_dialog)),
                                                )
                                                .child(
                                                    div()
                                                        .id("format-toolbar-customize-save")
                                                        .h(px(d.dialog_button_height))
                                                        .px(px(d.dialog_button_padding_x))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                                        .bg(c.dialog_primary_button_bg)
                                                        .hover(|this| this.bg(c.dialog_primary_button_hover))
                                                        .cursor_pointer()
                                                        .text_size(px(t.dialog_button_size))
                                                        .font_weight(t.dialog_button_weight.to_font_weight())
                                                        .text_color(c.dialog_primary_button_text)
                                                        .child(s.preferences_save.clone())
                                                        .on_click(cx.listener(Self::on_confirm_format_toolbar_customize_dialog)),
                                                ),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn format_toolbar_button_label(strings: &I18nStrings, id: &str) -> String {
    match id {
        "undo" => strings.preferences_shortcut_undo.clone(),
        "redo" => strings.preferences_shortcut_redo.clone(),
        "bold" => strings.format_toolbar_bold.clone(),
        "italic" => strings.format_toolbar_italic.clone(),
        "heading1" => strings.format_toolbar_heading1.clone(),
        "heading2" => strings.format_toolbar_heading2.clone(),
        "heading3" => strings.format_toolbar_heading3.clone(),
        "ordered_list" => strings.format_toolbar_ordered_list.clone(),
        "unordered_list" => strings.format_toolbar_unordered_list.clone(),
        "code" => strings.format_toolbar_code.clone(),
        "code_block" => strings.format_toolbar_code_block.clone(),
        "link" => strings.format_toolbar_link.clone(),
        "quote" => strings.format_toolbar_quote.clone(),
        "todo" => strings.format_toolbar_todo.clone(),
        "image" => strings.format_toolbar_image.clone(),
        "horizontal_rule" => strings.format_toolbar_horizontal_rule.clone(),
        "table_of_contents" => strings.format_toolbar_table_of_contents.clone(),
        "table" => strings.table_insert_title.clone(),
        "mermaid" => strings.format_toolbar_mermaid.clone(),
        "ai" => strings.format_toolbar_ai.clone(),
        "document_search" => strings.format_toolbar_document_search.clone(),
        "save" => strings.menu_save.clone(),
        "auto_save" => strings.format_toolbar_auto_save.clone(),
        "zoom_out" => strings.format_toolbar_zoom_out.clone(),
        "zoom_in" => strings.format_toolbar_zoom_in.clone(),
        "view_mode" => strings.format_toolbar_view_mode.clone(),
        _ => id.to_string(),
    }
}

fn format_toolbar_button_icon_path(id: &str) -> SharedString {
    SharedString::from(match id {
        "undo" => ICON_UNDO,
        "redo" => ICON_REDO,
        "bold" => ICON_BOLD,
        "italic" => ICON_ITALIC,
        "heading1" => ICON_HEADING_1,
        "heading2" => ICON_HEADING_2,
        "heading3" => ICON_HEADING_3,
        "ordered_list" => ICON_LIST_ORDERED,
        "unordered_list" => ICON_LIST_BULLET,
        "code" => ICON_CODE,
        "code_block" => ICON_SQUARE_CODE,
        "link" => ICON_LINK,
        "quote" => ICON_QUOTE,
        "todo" => ICON_TODO,
        "image" => ICON_IMAGE,
        "horizontal_rule" => ICON_HORIZONTAL_RULE,
        "table_of_contents" => ICON_TABLE_OF_CONTENTS,
        "table" => ICON_TABLE,
        "mermaid" => ICON_WORKFLOW,
        "ai" => ICON_AI_CUSTOM,
        "document_search" => ICON_SEARCH,
        "save" => ICON_SAVE,
        "auto_save" => ICON_AUTO_SAVE,
        "zoom_out" => ICON_ZOOM_OUT,
        "zoom_in" => ICON_ZOOM_IN,
        "view_mode" => ICON_VIEW_SOURCE,
        _ => ICON_BOLD,
    })
}
