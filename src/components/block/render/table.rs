//! Native and preview table rendering.

use gpui::*;

use super::super::element::InlineTreePreviewTextElement;
use super::super::{Block, BlockEvent};
use super::shared::visible_quote_guides;
use crate::components::{TableAxisKind, TableCellInlineImageSegment, TableCellPosition, TableColumnAlignment, TableColumnLayout, TableData, parse_table_cell_inline_images};
use crate::i18n::{I18nManager, I18nStrings};
use crate::layout::centered_column_width;
use crate::theme::{Theme, ThemeDimensions};

pub(super) const ICON_TABLE_COLUMN_MENU: &str = "icon/toolbar/ellipsis-vertical.svg";
pub(super) const TABLE_COLUMN_RESIZE_HANDLE_WIDTH: f32 = 8.0;

pub(super) fn style_native_table_cell_borders(
    mut cell: Stateful<Div>,
    position: TableCellPosition,
    extent: (usize, usize),
    border_color: Hsla,
    focused: bool,
) -> Stateful<Div> {
    if focused {
        return cell.border(px(1.0)).border_color(border_color);
    }

    let (columns, rows) = extent;
    if position.column + 1 < columns {
        cell = cell.border_r(px(1.0));
    }
    if position.row + 1 < rows {
        cell = cell.border_b(px(1.0));
    }
    cell.border_color(border_color)
}



pub(super) fn effective_table_width(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let centered_width = centered_column_width(viewport_width, d);
    let visible_quote_guides = visible_quote_guides(block);
    let quote_inset = d.quote_padding_left * visible_quote_guides as f32;
    let callout_inset = if block.callout_depth > 0 {
        d.callout_padding_x * 2.0 + d.callout_border_width
    } else {
        0.0
    };

    (centered_width - quote_inset - callout_inset)
        .max((d.table_cell_padding_x * 2.0 + 80.0).max(120.0))
}

impl Block {
    pub(super) fn render_table_cell_inline_images(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        font_weight: FontWeight,
    ) -> Option<AnyElement> {
        let segments = parse_table_cell_inline_images(&self.record.title.serialize_markdown());
        if !segments
            .iter()
            .any(|segment| matches!(segment, TableCellInlineImageSegment::Image { .. }))
        {
            return None;
        }

        let mut children = Vec::new();
        for segment in segments {
            match segment {
                TableCellInlineImageSegment::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    let tree = self.inline_tree_from_markdown_with_context(&text);
                    children.extend(self.render_inline_tree_children(
                        &tree,
                        theme,
                        theme.colors.text_default,
                        theme.typography.text_size,
                        font_weight,
                    ));
                }
                TableCellInlineImageSegment::Image { markdown, syntax } => {
                    if let Some(runtime) = self.image_runtime_for_syntax(syntax) {
                        children.push(self.render_inline_image_content(&runtime, theme, strings));
                    } else {
                        let tree = crate::components::InlineTextTree::plain(markdown);
                        children.extend(self.render_inline_tree_children(
                            &tree,
                            theme,
                            theme.colors.text_default,
                            theme.typography.text_size,
                            font_weight,
                        ));
                    }
                }
            }
        }

        Some(
            div()
                .w_full()
                .min_w(px(0.0))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.0))
                .text_size(px(theme.typography.text_size))
                .line_height(relative(theme.typography.text_line_height))
                .children(children)
                .into_any_element(),
        )
    }
    fn table_preview_cell_justify(
        mut element: Div,
        alignment: TableColumnAlignment,
    ) -> Div {
        element = element.flex();
        match alignment {
            TableColumnAlignment::Left => element.justify_start(),
            TableColumnAlignment::Center => element.justify_center(),
            TableColumnAlignment::Right => element.justify_end(),
        }
    }

    fn table_column_text_align(alignment: TableColumnAlignment) -> TextAlign {
        match alignment {
            TableColumnAlignment::Left => TextAlign::Left,
            TableColumnAlignment::Center => TextAlign::Center,
            TableColumnAlignment::Right => TextAlign::Right,
        }
    }

    fn render_table_preview_cell_content(
        &self,
        cell: &crate::components::InlineTextTree,
        alignment: TableColumnAlignment,
        theme: &Theme,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = cx.global::<I18nManager>().strings_arc();
        if let Some(inline_images) =
            self.render_inline_tree_table_cell_images(cell, theme, &strings, font_weight)
        {
            return Self::table_preview_cell_justify(
                div().w_full().min_w(px(0.0)),
                alignment,
            )
            .child(inline_images)
            .into_any_element();
        }

        div()
            .w_full()
            .min_w(px(0.0))
            .child(
                InlineTreePreviewTextElement::new(
                    cell.clone(),
                    Self::table_column_text_align(alignment),
                    font_weight,
                    theme.colors.text_default,
                    theme.typography.text_size,
                    theme.typography.text_line_height,
                ),
            )
            .into_any_element()
    }

    fn render_inline_tree_table_cell_images(
        &self,
        cell: &crate::components::InlineTextTree,
        theme: &Theme,
        strings: &I18nStrings,
        font_weight: FontWeight,
    ) -> Option<AnyElement> {
        let segments = parse_table_cell_inline_images(&cell.serialize_markdown());
        if !segments
            .iter()
            .any(|segment| matches!(segment, TableCellInlineImageSegment::Image { .. }))
        {
            return None;
        }

        let mut children = Vec::new();
        for segment in segments {
            match segment {
                TableCellInlineImageSegment::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    let tree = self.inline_tree_from_markdown_with_context(&text);
                    children.extend(self.render_inline_tree_children(
                        &tree,
                        theme,
                        theme.colors.text_default,
                        theme.typography.text_size,
                        font_weight,
                    ));
                }
                TableCellInlineImageSegment::Image { markdown, syntax } => {
                    if let Some(runtime) = self.image_runtime_for_syntax(syntax) {
                        children.push(self.render_inline_image_content(&runtime, theme, strings));
                    } else {
                        let tree = crate::components::InlineTextTree::plain(markdown);
                        children.extend(self.render_inline_tree_children(
                            &tree,
                            theme,
                            theme.colors.text_default,
                            theme.typography.text_size,
                            font_weight,
                        ));
                    }
                }
            }
        }

        Some(
            div()
                .w_full()
                .min_w(px(0.0))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.0))
                .text_size(px(theme.typography.text_size))
                .line_height(relative(theme.typography.text_line_height))
                .children(children)
                .into_any_element(),
        )
    }

    pub(super) fn render_table_data_preview(
        &self,
        table: &TableData,
        table_width: f32,
        table_key: &str,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let column_count = table.column_count();
        let column_layout = TableColumnLayout::for_table(table, table_width, window, theme);
        let row_extent = 1 + table.rows.len();
        let column_extent = column_count;

        let header_row = div().w_full().flex().gap(px(0.0)).children(
            table.header.iter().enumerate().map(|(column, cell)| {
                let position = TableCellPosition { row: 0, column };
                let alignment = table
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableColumnAlignment::Left);
                style_native_table_cell_borders(
                    div()
                        .id(ElementId::Name(
                            format!("table-preview-{table_key}-header-{column}").into(),
                        ))
                        .flex_none()
                        .flex_basis(relative(column_layout.fraction(column)))
                        .w(relative(column_layout.fraction(column)))
                        .h_full()
                        .min_w(px(0.0))
                        .min_h(px(d.table_cell_min_height))
                        .px(px(d.table_cell_padding_x))
                        .py(px(d.table_cell_padding_y))
                        .bg(c.table_header_bg)
                        .text_size(px(t.text_size))
                        .text_color(c.text_default)
                        .line_height(relative(t.text_line_height))
                        .font_weight(FontWeight::MEDIUM)
                        .child(self.render_table_preview_cell_content(
                            cell,
                            alignment,
                            theme,
                            FontWeight::MEDIUM,
                            cx,
                        )),
                    position,
                    (column_extent, row_extent),
                    c.table_border,
                    false,
                )
            }),
        );

        let body_rows = table.rows.iter().enumerate().map(|(body_row_index, row)| {
            let row_index = body_row_index + 1;
            div().w_full().flex().gap(px(0.0)).children(row.iter().enumerate().map(
                |(column, cell)| {
                    let position = TableCellPosition {
                        row: row_index,
                        column,
                    };
                    let alignment = table
                        .alignments
                        .get(column)
                        .copied()
                        .unwrap_or(TableColumnAlignment::Left);
                    style_native_table_cell_borders(
                        div()
                            .id(ElementId::Name(
                                format!(
                                    "table-preview-{table_key}-body-{body_row_index}-{column}"
                                )
                                .into(),
                            ))
                            .flex_none()
                            .flex_basis(relative(column_layout.fraction(column)))
                            .w(relative(column_layout.fraction(column)))
                            .h_full()
                            .min_w(px(0.0))
                            .min_h(px(d.table_cell_min_height))
                            .px(px(d.table_cell_padding_x))
                            .py(px(d.table_cell_padding_y))
                            .bg(c.table_cell_bg)
                            .text_size(px(t.text_size))
                            .text_color(c.text_default)
                            .line_height(relative(t.text_line_height))
                            .child(self.render_table_preview_cell_content(
                                cell,
                                alignment,
                                theme,
                                FontWeight::NORMAL,
                                cx,
                            )),
                        position,
                        (column_extent, row_extent),
                        c.table_border,
                        false,
                    )
                },
            ))
        });

        div()
            .id(ElementId::Name(format!("table-preview-{table_key}").into()))
            .w_full()
            .min_w(px(0.0))
            .relative()
            .flex()
            .flex_col()
            .border(px(1.0))
            .border_color(c.table_border)
            .overflow_hidden()
            .gap(px(0.0))
            .child(header_row)
            .children(body_rows)
            .into_any_element()
    }
    pub(super) fn render_native_table_ui(
        &mut self,
        block_id: ElementId,
        table_width: f32,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let Some(runtime) = self.table_runtime.clone() else {
            return div().into_any_element();
        };

        let column_count = runtime.header.len();
        let column_layout = self
            .record
            .table
            .as_ref()
            .map(|table| TableColumnLayout::for_table(table, table_width, window, theme))
            .unwrap_or_else(|| TableColumnLayout::equal(column_count));
        let column_fractions = (0..column_count)
            .map(|column| column_layout.fraction(column))
            .collect::<Vec<_>>();
        let preview_marker = self.table_axis_preview;
        let selected_marker = self.table_axis_selection;
        let body_row_count = runtime.rows.len();
        let append_extent = px(d.table_append_button_extent);
        let append_inset = px(d.table_append_button_inset);
        let activation_band = px(d.table_append_activation_band);
        let column_append_top = activation_band;
        let column_menu_icon_size = px((t.text_size * 0.85).max(12.0));
        let column_menu_handle_width = px(20.0);
        let column_control_visible = self.table_append_column_hovered;
        let row_control_visible = self.table_append_row_hovered;
        let right_gutter = if column_control_visible {
            append_extent + append_inset
        } else {
            px(0.0)
        };
        let bottom_gutter = if row_control_visible {
            append_extent + append_inset
        } else {
            px(0.0)
        };
        let weak_table_block = cx.entity().downgrade();

        let header_cells = runtime.header;

        let resize_handle_offset = px(TABLE_COLUMN_RESIZE_HANDLE_WIDTH * 0.5);
        let resize_handle_width = px(TABLE_COLUMN_RESIZE_HANDLE_WIDTH);

        let header_row = div().w_full().flex().gap(px(0.0)).children(
            header_cells.into_iter().enumerate().map(|(column, cell)| {
                let menu_block = weak_table_block.clone();
                let resize_block = weak_table_block.clone();
                let resize_fractions = column_fractions.clone();
                let can_resize_column = column + 1 < column_count;
                let mut column_shell = div()
                    .relative()
                    .flex_none()
                    .flex_basis(relative(column_layout.fraction(column)))
                    .w(relative(column_layout.fraction(column)))
                    .h_full()
                    .min_w(px(0.0))
                    .child(cell)
                    .child(
                        div()
                            .id(ElementId::Name(
                                format!("table-column-menu-handle-{}-{}", self.record.id, column)
                                    .into(),
                            ))
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(if can_resize_column {
                                resize_handle_width
                            } else {
                                px(0.0)
                            })
                            .w(column_menu_handle_width)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .opacity(0.55)
                            .hover(|this| this.opacity(0.9))
                            .occlude()
                            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                let _ = menu_block.update(cx, |_block, cx| {
                                    cx.stop_propagation();
                                    cx.emit(BlockEvent::RequestOpenTableAxisMenu {
                                        kind: TableAxisKind::Column,
                                        index: column,
                                        position: event.position,
                                    });
                                });
                            })
                            .child(
                                svg()
                                    .path(ICON_TABLE_COLUMN_MENU)
                                    .size(column_menu_icon_size)
                                    .text_color(c.text_default),
                            ),
                    );

                if can_resize_column {
                    column_shell = column_shell.child(
                        div()
                            .id(ElementId::Name(
                                format!(
                                    "table-column-resize-handle-{}-{}",
                                    self.record.id, column
                                )
                                .into(),
                            ))
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right(-resize_handle_offset)
                            .w(resize_handle_width)
                            .cursor_col_resize()
                            .hover(|this| this.bg(c.table_border.opacity(0.55)))
                            .occlude()
                            .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                                let _ = resize_block.update(cx, |_block, cx| {
                                    cx.stop_propagation();
                                    cx.emit(BlockEvent::RequestStartTableColumnResize {
                                        boundary_index: column,
                                        pointer_x: f32::from(event.position.x),
                                        table_width,
                                        fractions: resize_fractions.clone(),
                                    });
                                });
                            }),
                    );
                }

                column_shell
            }),
        );

        let body_rows = runtime.rows.into_iter().enumerate().map(|(body_row_index, row)| {
            let hover_block = weak_table_block.clone();
            let select_block = weak_table_block.clone();
            let menu_block = weak_table_block.clone();
            let marker = crate::components::TableAxisMarker {
                kind: TableAxisKind::Row,
                index: body_row_index,
            };
            let band_bg = if selected_marker == Some(marker) {
                c.table_axis_selected_bg
            } else if preview_marker == Some(marker) {
                c.table_axis_preview_bg
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            };
            div()
                .relative()
                .w_full()
                .flex()
                .gap(px(0.0))
                .child(
                    div()
                        .id(ElementId::Name(
                            format!(
                                "table-row-axis-band-{}-{}",
                                self.record.id, body_row_index
                            )
                            .into(),
                        ))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(-activation_band)
                        .w(activation_band)
                        .rounded(px(6.0))
                        .bg(band_bg)
                        .cursor_pointer()
                        .on_hover(move |hovered, _window, cx| {
                            let _ = hover_block.update(cx, |_block, cx| {
                                cx.emit(BlockEvent::RequestTableAxisPreview {
                                    kind: TableAxisKind::Row,
                                    index: hovered.then_some(body_row_index),
                                });
                            });
                        })
                        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                            let _ = select_block.update(cx, |_block, cx| {
                                cx.stop_propagation();
                                cx.emit(BlockEvent::RequestSelectTableAxis {
                                    kind: TableAxisKind::Row,
                                    index: body_row_index,
                                });
                            });
                        })
                        .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                            let _ = menu_block.update(cx, |_block, cx| {
                                cx.stop_propagation();
                                cx.emit(BlockEvent::RequestOpenTableAxisMenu {
                                    kind: TableAxisKind::Row,
                                    index: body_row_index,
                                    position: event.position,
                                });
                            });
                        })
                        .occlude(),
                )
                .children(row.into_iter().enumerate().map(|(column, cell)| {
                    div()
                        .flex_none()
                        .flex_basis(relative(column_layout.fraction(column)))
                        .w(relative(column_layout.fraction(column)))
                        .h_full()
                        .min_w(px(0.0))
                        .child(cell)
                }))
        });

        let mut rows = Vec::with_capacity(1 + body_row_count);
        rows.push(header_row.into_any_element());
        rows.extend(body_rows.map(|row| row.into_any_element()));

        let column_edge_band = div()
            .id(ElementId::Name(
                format!("table-append-column-edge-{}", self.record.id).into(),
            ))
            .absolute()
            .top(column_append_top)
            .bottom(bottom_gutter)
            .right(right_gutter)
            .w(activation_band)
            .on_hover(cx.listener(Self::on_table_append_column_edge_hover));

        let row_edge_band = div()
            .id(ElementId::Name(
                format!("table-append-row-edge-{}", self.record.id).into(),
            ))
            .absolute()
            .left_0()
            .right(right_gutter)
            .bottom(bottom_gutter)
            .h(activation_band)
            .on_hover(cx.listener(Self::on_table_append_row_edge_hover));

        let column_control = {
            let base = div()
                .id(ElementId::Name(
                    format!("table-append-column-zone-{}", self.record.id).into(),
                ))
                .absolute()
                .top(column_append_top)
                .bottom(bottom_gutter)
                .right_0()
                .w(right_gutter)
                .on_hover(cx.listener(Self::on_table_append_column_zone_hover));

            if column_control_visible {
                base.child(
                    div()
                        .id(ElementId::Name(
                            format!("table-append-column-button-{}", self.record.id).into(),
                        ))
                        .absolute()
                        .top(append_inset)
                        .bottom_0()
                        .right_0()
                        .w(append_extent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(999.0))
                        .bg(c.table_append_button_bg)
                        .hover(|this| this.bg(c.table_append_button_hover))
                        .cursor_pointer()
                        .text_size(px(t.text_size))
                        .text_color(c.table_append_button_text)
                        .occlude()
                        .on_hover(cx.listener(Self::on_table_append_column_button_hover))
                        .on_click(cx.listener(Self::on_append_table_column))
                        .child("+"),
                )
            } else {
                base
            }
        };

        let row_control = {
            let base = div()
                .id(ElementId::Name(
                    format!("table-append-row-zone-{}", self.record.id).into(),
                ))
                .absolute()
                .left_0()
                .right(right_gutter)
                .bottom_0()
                .h(bottom_gutter)
                .on_hover(cx.listener(Self::on_table_append_row_zone_hover));

            if row_control_visible {
                base.child(
                    div()
                        .id(ElementId::Name(
                            format!("table-append-row-button-{}", self.record.id).into(),
                        ))
                        .absolute()
                        .left(append_inset)
                        .right(append_inset)
                        .bottom_0()
                        .h(append_extent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(999.0))
                        .bg(c.table_append_button_bg)
                        .hover(|this| this.bg(c.table_append_button_hover))
                        .cursor_pointer()
                        .text_size(px(t.text_size))
                        .text_color(c.table_append_button_text)
                        .occlude()
                        .on_hover(cx.listener(Self::on_table_append_row_button_hover))
                        .on_click(cx.listener(Self::on_append_table_row))
                        .child("+"),
                )
            } else {
                base
            }
        };

        div()
            .id(block_id)
            .w_full()
            .relative()
            .flex()
            .flex_col()
            .border(px(1.0))
            .border_color(c.table_border)
            .overflow_hidden()
            .pr(right_gutter)
            .pb(bottom_gutter)
            .gap(px(0.0))
            .children(rows)
            .child(column_edge_band)
            .child(row_edge_band)
            .child(column_control)
            .child(row_control)
            .into_any_element()
    }

    pub(super) fn render_table_block(
        &mut self,
        block_id: ElementId,
        focused_base: Stateful<Div>,
        focused: bool,
        is_placeholder: bool,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
                let Some(_runtime) = self.table_runtime.clone() else {
                    return focused_base
                        .text_size(px(t.text_size))
                        .text_color(c.text_default)
                        .line_height(relative(t.text_line_height))
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
                        .into_any_element();
                };

                let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
                let table_width = effective_table_width(self, viewport_width, d);
                self.render_native_table_ui(block_id, table_width, &theme, window, cx)
    }
}
