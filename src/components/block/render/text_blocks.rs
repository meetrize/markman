//! Block kind dispatch and shell wrapper.

use gpui::*;

use super::super::element::BlockTextElement;
use super::super::{Block, BlockKind};
use super::shared::{bulleted_list_marker, callout_accent_and_background, effective_image_width, effective_list_item_image_width, numbered_list_marker, visible_quote_guides, wrap_with_quote_guides};
use super::table::style_native_table_cell_borders;
use crate::components::{TableAxisHighlight, parse_columns_markdown};
use crate::i18n::I18nManager;
use crate::theme::{ThemeDimensions, ThemeManager};

const TASK_CHECKMARK: &str = "\u{2713}";

impl Block {
    pub(super) fn render_shell(
        &self,
        block_id: ElementId,
        source_mode: bool,
        cursor_style: CursorStyle,
        padding_left: f32,
        padding_right: f32,
        dimensions: &ThemeDimensions,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let base = div()
            .id(block_id)
            .key_context("BlockEditor")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_exit_code_block))
            .on_action(cx.listener(Self::on_newline))
            .on_action(cx.listener(Self::on_delete_back))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_word_delete_back))
            .on_action(cx.listener(Self::on_word_delete_forward))
            .on_action(cx.listener(Self::on_focus_prev))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_word_move_left))
            .on_action(cx.listener(Self::on_word_move_right))
            .on_action(cx.listener(Self::on_home))
            .on_action(cx.listener(Self::on_end))
            .on_action(cx.listener(Self::on_block_up))
            .on_action(cx.listener(Self::on_block_down))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_word_select_left))
            .on_action(cx.listener(Self::on_word_select_right))
            .on_action(cx.listener(Self::on_select_home))
            .on_action(cx.listener(Self::on_select_end))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_key_down(cx.listener(Self::on_block_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_w(px(0.0))
            .flex_shrink_0()
            .min_h(px(dimensions.block_min_height))
            .py(px(dimensions.block_padding_y))
            .pl(px(padding_left))
            .pr(px(padding_right))
            .cursor(cursor_style);

        if source_mode {
            base
        } else {
            base.on_action(cx.listener(Self::on_indent_block))
                .on_action(cx.listener(Self::on_outdent_block))
                .on_action(cx.listener(Self::on_bold_selection))
                .on_action(cx.listener(Self::on_italic_selection))
                .on_action(cx.listener(Self::on_underline_selection))
                .on_action(cx.listener(Self::on_code_selection))
        }
    }
    pub(super) fn render_block_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus_handle.is_focused(window);
        let code_language_focused = self.code_language_focus_handle.is_focused(window);
        let input_active = focused || code_language_focused;
        if self.sync_image_focus_state(focused) {
            cx.notify();
        }
        if self.sync_code_language_menu_for_focus(input_active) {
            cx.notify();
        }

        let showing_rendered_image = self.showing_rendered_image();
        if self.sync_inline_math_source_edit_for_focus(focused && !showing_rendered_image) {
            cx.notify();
        }
        self.sync_inline_projection_for_focus(
            focused && !showing_rendered_image && !self.inline_math_source_editing(),
        );

        if input_active && self.cursor_blink_task.is_none() {
            self.start_cursor_blink(cx);
        } else         if !input_active && self.cursor_blink_task.is_some() {
            self.cursor_blink_task = None;
        }

        let block_id = ElementId::Name(format!("block-{}", self.record.id).into());
        let is_placeholder =
            focused && self.display_text().is_empty() && self.marked_range.is_none();

        let theme = cx.global::<ThemeManager>().current_arc();
        let strings = cx.global::<I18nManager>().strings_arc();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let depth_padding = d.block_padding_x + d.nested_block_indent * self.render_depth as f32;

        if self.embedded_column_table {
            let table_width = self
                .embedded_table_layout_width
                .unwrap_or(120.0)
                .max(120.0);
            return self.render_native_table_ui(block_id, table_width, &theme, window, cx);
        }

        if self.is_table_cell() {
            let position = self.table_cell_position().expect("table cell position");
            let extent = self
                .table_cell_extent
                .unwrap_or((position.column + 1, position.row + 1));
            let is_header = position.is_header();
            let highlight = self.table_axis_highlight;
            let base_bg = if is_header {
                c.table_header_bg
            } else {
                c.table_cell_bg
            };
            let bg = match highlight {
                TableAxisHighlight::None => base_bg,
                TableAxisHighlight::Preview => c.table_axis_preview_bg,
                TableAxisHighlight::Selected => c.table_axis_selected_bg,
            };
            let border_color = if focused {
                c.table_cell_active_outline
            } else {
                match highlight {
                    TableAxisHighlight::None => c.table_border,
                    TableAxisHighlight::Preview => c.table_axis_preview_bg,
                    TableAxisHighlight::Selected => c.table_axis_selected_bg,
                }
            };
            let cell_base = style_native_table_cell_borders(
                self.render_shell(
                    block_id,
                    false,
                    if showing_rendered_image {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::IBeam
                    },
                    0.0,
                    0.0,
                    d,
                    cx,
                )
                .w_full()
                .h_full()
                .min_h(px(d.table_cell_min_height))
                .px(px(d.table_cell_padding_x))
                .py(px(d.table_cell_padding_y))
                .bg(bg)
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(rems(t.text_line_height)),
                position,
                extent,
                border_color,
                focused,
            );

            let cell_base = if is_header {
                cell_base.font_weight(FontWeight::MEDIUM)
            } else {
                cell_base
            };

            if showing_rendered_image && let Some(runtime) = self.image_runtime() {
                return cell_base
                    .child(self.render_image_content(
                        runtime,
                        Length::Definite(relative(1.0)),
                        px(d.image_cell_max_height),
                        px(d.image_cell_placeholder_height),
                        &theme,
                        &strings,
                    ))
                    .into_any_element();
            }

            if !focused
                && let Some(inline_images) = self.render_table_cell_inline_images(
                    &theme,
                    &strings,
                    if is_header {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    },
                )
            {
                return cell_base.child(inline_images).into_any_element();
            }

            return cell_base
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_default,
                    t.text_size,
                    if is_header {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    },
                    cx,
                ))
                .into_any_element();
        }

        // Source-mode rendering: raw text with no formatting.
        let rendered_columns = if self.kind() == BlockKind::RawMarkdown {
            parse_columns_markdown(self.display_text())
        } else {
            None
        };
        let columns_preview_active = rendered_columns.is_some()
            && (!focused || !self.columns_source_edit);
        if !focused && self.columns_source_edit {
            self.columns_source_edit = false;
        }

        if self.is_source_raw_mode()
            && !columns_preview_active
            && (focused
                || (rendered_columns.is_none()
                    && !matches!(
                        self.kind(),
                        BlockKind::HtmlBlock | BlockKind::MathBlock | BlockKind::MermaidBlock
                    )))
        {
            if focused && self.cursor_blink_task.is_none() {
                self.start_cursor_blink(cx);
            } else if !focused && self.cursor_blink_task.is_some() {
                self.cursor_blink_task = None;
            }
            let source_base = self
                .render_shell(
                    block_id.clone(),
                    true,
                    CursorStyle::IBeam,
                    d.block_padding_x,
                    d.block_padding_x,
                    d,
                    cx,
                )
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(rems(t.text_line_height));

            let source_base = if self.kind() == BlockKind::Comment {
                source_base.bg(c.comment_bg).rounded_sm()
            } else if focused {
                source_base.bg(c.source_mode_block_bg).rounded_sm()
            } else {
                source_base
            };

            return source_base
                .child(BlockTextElement::new(cx.entity(), is_placeholder))
                .into_any_element();
        }

        let focused_base = self.render_shell(
            block_id.clone(),
            false,
            if showing_rendered_image {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            },
            if self.kind().is_separator() {
                depth_padding + d.separator_inset_x
            } else {
                depth_padding
            },
            if self.kind().is_separator() {
                d.block_padding_x + d.separator_inset_x
            } else {
                d.block_padding_x
            },
            d,
            cx,
        );

        if showing_rendered_image && self.kind() == BlockKind::Paragraph {
            let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
            let max_width = px(effective_image_width(self, viewport_width, d));
            if let Some(runtime) = self.image_runtime() {
                return focused_base
                    .child(self.render_image_content(
                        runtime,
                        max_width.into(),
                        px(d.image_root_max_height),
                        px(d.image_root_placeholder_height),
                        &theme,
                        &strings,
                    ))
                    .into_any_element();
            }
        }

        let content = match self.kind() {
            BlockKind::Separator => focused_base
                .py(px(d.separator_margin_y))
                .child(
                    div()
                        .w_full()
                        .h(px(d.separator_thickness))
                        .bg(c.separator_color)
                        .rounded(px(999.0)),
                )
                .into_any_element(),
            BlockKind::Heading { level: 1 } => focused_base
                .text_size(px(t.h1_size))
                .font_weight(t.h1_weight.to_font_weight())
                .text_color(c.text_h1)
                .pb(px(d.h1_padding_bottom))
                .mb(px(d.h1_margin_bottom))
                .border_b(px(d.h1_border_width))
                .border_color(c.border_h1)
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h1,
                    t.h1_size,
                    t.h1_weight.to_font_weight(),
                    cx,
                ))
                .into_any_element(),
            BlockKind::Heading { level: 2 } => focused_base
                .text_size(px(t.h2_size))
                .font_weight(t.h2_weight.to_font_weight())
                .text_color(c.text_h2)
                .pb(px(d.h1_padding_bottom))
                .mb(px(d.h1_margin_bottom))
                .border_b(px(d.h1_border_width))
                .border_color(c.border_h2)
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h2,
                    t.h2_size,
                    t.h2_weight.to_font_weight(),
                    cx,
                ))
                .into_any_element(),
            BlockKind::Heading { level: 3 } => focused_base
                .text_size(px(t.h3_size))
                .font_weight(t.h3_weight.to_font_weight())
                .text_color(c.text_h3)
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h3,
                    t.h3_size,
                    t.h3_weight.to_font_weight(),
                    cx,
                ))
                .into_any_element(),
            BlockKind::Heading { level: 4 } => focused_base
                .text_size(px(t.h4_size))
                .font_weight(t.h4_weight.to_font_weight())
                .text_color(c.text_h4)
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h4,
                    t.h4_size,
                    t.h4_weight.to_font_weight(),
                    cx,
                ))
                .into_any_element(),
            BlockKind::Heading { level: 5 } => focused_base
                .text_size(px(t.h5_size))
                .font_weight(t.h5_weight.to_font_weight())
                .text_color(c.text_h5)
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h5,
                    t.h5_size,
                    t.h5_weight.to_font_weight(),
                    cx,
                ))
                .into_any_element(),
            BlockKind::Heading { level: 6 } => focused_base
                .text_size(px(t.h6_size))
                .font_weight(t.h6_weight.to_font_weight())
                .text_color(c.text_h6)
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h6,
                    t.h6_size,
                    t.h6_weight.to_font_weight(),
                    cx,
                ))
                .into_any_element(),
            BlockKind::BulletedListItem => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(rems(t.text_line_height))
                .w_full()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(d.list_marker_gap))
                .children([
                    div()
                        .min_w(px(d.list_marker_width))
                        .child(SharedString::new(bulleted_list_marker(self.render_depth))),
                    if showing_rendered_image {
                        let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
                        let max_width =
                            px(effective_list_item_image_width(self, viewport_width, d));
                        if let Some(runtime) = self.image_runtime() {
                            div().flex_grow().child(self.render_image_content(
                                runtime,
                                max_width.into(),
                                px(d.image_root_max_height),
                                px(d.image_root_placeholder_height),
                                &theme,
                                &strings,
                            ))
                        } else {
                            div().min_w(px(0.0)).flex_grow().child(
                                self.render_text_or_mixed_inline_visuals(
                                    &theme,
                                    focused,
                                    is_placeholder,
                                    None,
                                    None,
                                    c.text_default,
                                    t.text_size,
                                    FontWeight::NORMAL,
                                    cx,
                                ),
                            )
                        }
                    } else {
                        div().min_w(px(0.0)).flex_grow().child(
                            self.render_text_or_mixed_inline_visuals(
                                &theme,
                                focused,
                                is_placeholder,
                                None,
                                None,
                                c.text_default,
                                t.text_size,
                                FontWeight::NORMAL,
                                cx,
                            ),
                        )
                    },
                ])
                .into_any_element(),
            BlockKind::TaskListItem { checked } => {
                let marker_width = d.list_marker_width.max(d.task_checkbox_size);
                let first_line_height = t.text_size * t.text_line_height;
                focused_base
                    .text_size(px(t.text_size))
                    .text_color(c.text_default)
                    .line_height(rems(t.text_line_height))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(d.list_marker_gap))
                    .children([
                        div()
                            .min_w(px(marker_width))
                            .h(px(first_line_height))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .size(px(d.task_checkbox_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.task_checkbox_radius))
                                    .border(px(d.task_checkbox_border_width))
                                    .border_color(c.task_checkbox_border)
                                    .bg(if checked {
                                        c.task_checkbox_checked_bg
                                    } else {
                                        c.task_checkbox_bg
                                    })
                                    .text_size(px(d.task_checkbox_check_size))
                                    .text_color(c.task_checkbox_check)
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::on_task_checkbox_mouse_down),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(Self::on_task_checkbox_mouse_up),
                                    )
                                    .child(if checked {
                                        SharedString::new(TASK_CHECKMARK)
                                    } else {
                                        SharedString::new("")
                                    }),
                            ),
                        if showing_rendered_image {
                            let viewport_width =
                                f32::from(window.viewport_size().width.max(px(1.0)));
                            let max_width =
                                px(effective_list_item_image_width(self, viewport_width, d));
                            if let Some(runtime) = self.image_runtime() {
                                div().flex_grow().child(self.render_image_content(
                                    runtime,
                                    max_width.into(),
                                    px(d.image_root_max_height),
                                    px(d.image_root_placeholder_height),
                                    &theme,
                                    &strings,
                                ))
                            } else {
                                div().min_w(px(0.0)).flex_grow().child(
                                    self.render_text_or_mixed_inline_visuals(
                                        &theme,
                                        focused,
                                        is_placeholder,
                                        None,
                                        None,
                                        c.text_default,
                                        t.text_size,
                                        FontWeight::NORMAL,
                                        cx,
                                    ),
                                )
                            }
                        } else {
                            div().min_w(px(0.0)).flex_grow().child(
                                self.render_text_or_mixed_inline_visuals(
                                    &theme,
                                    focused,
                                    is_placeholder,
                                    None,
                                    None,
                                    c.text_default,
                                    t.text_size,
                                    FontWeight::NORMAL,
                                    cx,
                                ),
                            )
                        },
                    ])
                    .into_any_element()
            }
            BlockKind::NumberedListItem => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(rems(t.text_line_height))
                .w_full()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(d.list_marker_gap))
                .children([
                    div()
                        .min_w(px(d.ordered_list_marker_width))
                        .child(SharedString::from(numbered_list_marker(
                            self.render_depth,
                            self.list_ordinal.unwrap_or(1),
                        ))),
                    if showing_rendered_image {
                        let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
                        let max_width =
                            px(effective_list_item_image_width(self, viewport_width, d));
                        if let Some(runtime) = self.image_runtime() {
                            div().flex_grow().child(self.render_image_content(
                                runtime,
                                max_width.into(),
                                px(d.image_root_max_height),
                                px(d.image_root_placeholder_height),
                                &theme,
                                &strings,
                            ))
                        } else {
                            div().min_w(px(0.0)).flex_grow().child(
                                self.render_text_or_mixed_inline_visuals(
                                    &theme,
                                    focused,
                                    is_placeholder,
                                    None,
                                    None,
                                    c.text_default,
                                    t.text_size,
                                    FontWeight::NORMAL,
                                    cx,
                                ),
                            )
                        }
                    } else {
                        div().min_w(px(0.0)).flex_grow().child(
                            self.render_text_or_mixed_inline_visuals(
                                &theme,
                                focused,
                                is_placeholder,
                                None,
                                None,
                                c.text_default,
                                t.text_size,
                                FontWeight::NORMAL,
                                cx,
                            ),
                        )
                    },
                ])
                .into_any_element(),
            BlockKind::Quote => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_quote)
                .line_height(rems(t.text_line_height))
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_quote,
                    t.text_size,
                    FontWeight::NORMAL,
                    cx,
                ))
                .into_any_element(),
            BlockKind::Callout(variant) => {
                let (accent, _) = callout_accent_and_background(variant, &theme);
                let title_is_empty = self.record.title.visible_text().is_empty();
                let show_static_default_label = title_is_empty && !focused;
                let header_label = SharedString::from(variant.label());
                let header_text = if show_static_default_label {
                    div()
                        .text_size(px(t.text_size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent)
                        .child(header_label.clone())
                        .into_any_element()
                } else {
                    div()
                        .min_w(px(0.0))
                        .flex_grow()
                        .text_size(px(t.text_size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent)
                        .child(self.render_text_or_mixed_inline_visuals(
                            &theme,
                            focused,
                            is_placeholder,
                            Some(header_label),
                            Some(accent),
                            accent,
                            t.text_size,
                            FontWeight::SEMIBOLD,
                            cx,
                        ))
                        .into_any_element()
                };

                focused_base
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(d.callout_header_gap))
                    .child(
                        div()
                            .text_size(px(t.text_size))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent)
                            .child(variant.icon()),
                    )
                    .child(header_text)
                    .into_any_element()
            }
            BlockKind::FootnoteDefinition => {
                let ordinal = self.footnote_definition_ordinal();
                let badge = ordinal
                    .map(|ordinal| ordinal.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let badge_text_size = px((t.code_size - 1.0).max(10.0));
                let header = focused_base
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(d.list_marker_gap))
                    .text_size(px(t.code_size))
                    .text_color(c.text_quote)
                    .child(
                        div()
                            .px(px(d.footnote_badge_padding_x))
                            .py(px(d.footnote_badge_padding_y))
                            .rounded(px(999.0))
                            .bg(c.footnote_badge_bg)
                            .text_size(badge_text_size)
                            .text_color(c.footnote_badge_text)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_grow()
                            .text_color(c.text_quote)
                            .child(self.render_text_or_mixed_inline_visuals(
                                &theme,
                                focused,
                                is_placeholder,
                                None,
                                None,
                                c.text_quote,
                                t.code_size,
                                FontWeight::NORMAL,
                                cx,
                            )),
                    );

                if self.footnote_definition_has_backref() {
                    header
                        .child(
                            div()
                                .text_color(c.footnote_backref)
                                .hover(|this| this.text_color(c.text_link))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_footnote_backref_mouse_down),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(Self::on_footnote_backref_mouse_up),
                                )
                                .child("\u{21A9}"),
                        )
                        .into_any_element()
                } else {
                    header.into_any_element()
                }
            }
            BlockKind::CodeBlock { language } => self.render_code_block(
                focused_base,
                focused,
                is_placeholder,
                language,
                &theme,
                &strings,
                cx,
            ),
            BlockKind::Table => self.render_table_block(
                block_id,
                focused_base,
                focused,
                is_placeholder,
                &theme,
                window,
                cx,
            ),
            BlockKind::HtmlBlock => {
                let html = self.record.html.as_ref().cloned().unwrap_or_else(|| {
                    crate::components::parse_html_document(
                        self.record
                            .raw_fallback
                            .as_deref()
                            .unwrap_or_else(|| self.display_text()),
                    )
                });
                focused_base
                    .text_size(px(t.text_size))
                    .text_color(c.text_default)
                    .line_height(rems(t.text_line_height))
                    .child(self.render_html_document(&html, &theme, false, cx))
                    .into_any_element()
            }
            BlockKind::MathBlock => {
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                }
                let child = if focused {
                    BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
                } else {
                    self.render_math_content(&theme)
                };
                focused_base.w_full().child(child).into_any_element()
            }
            BlockKind::MermaidBlock => self.render_mermaid_block(
                focused_base,
                focused,
                is_placeholder,
                &theme,
                window,
                cx,
            ),
            BlockKind::RawMarkdown if rendered_columns.is_some() && columns_preview_active => {
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                    self.interaction_bounds = None;
                }
                let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
                div()
                    .id(block_id)
                    .w_full()
                    .min_w(px(0.0))
                    .child(self.render_columns_markdown(
                        rendered_columns.unwrap_or_default(),
                        &theme,
                        viewport_width <= 768.0,
                        window,
                        cx,
                    ))
                    .into_any_element()
            }
            BlockKind::Paragraph
            | BlockKind::Comment
            | BlockKind::RawMarkdown
            | BlockKind::Heading { .. } => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(rems(t.text_line_height))
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_default,
                    t.text_size,
                    FontWeight::NORMAL,
                    cx,
                ))
                .into_any_element(),
        };

        wrap_with_quote_guides(content, visible_quote_guides(self), &theme)
    }
}

impl Render for Block {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_block_body(window, cx)
    }
}

impl Focusable for Block {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
