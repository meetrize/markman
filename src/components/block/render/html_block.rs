//! Native HTML block preview rendering.

use std::path::Path;

use gpui::*;

use super::super::{Block, ImageRuntime};
use super::code;
use super::shared::html_css_color_to_hsla;
use crate::components::{
    BlockEvent, HtmlDocument, HtmlNode, HtmlNodeKind, ImageReferenceDefinitions, TableColumnLayout,
    attr_value, parse_html_image_block, parse_standalone_image, parse_standalone_link_wrapped_image,
    resolve_image_source, style_for_node,
};
use crate::components::markdown::inline::InlineLinkHit;
use crate::components::markdown::link::{HtmlTextLineSegment, parse_html_text_line_segments};
use crate::i18n::I18nManager;
use crate::theme::{Theme, ThemeDimensions};

const BULLET_FILLED: &str = "\u{2022}";

pub(super) fn html_children_text(node: &HtmlNode) -> String {
    if node.children.is_empty() {
        return node.raw_source.clone();
    }

    let mut text = String::new();
    for child in &node.children {
        if child.tag_name == "br" {
            text.push('\n');
        } else {
            text.push_str(&html_children_text(child));
        }
    }
    text
}


#[derive(Clone, Copy, Debug)]
pub(super) struct HtmlComputedStyle {
    pub(super) color: Hsla,
    pub(super) font_size: f32,
    pub(super) root_font_size: f32,
    pub(super) text_align: TextAlign,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HtmlNodeVisualStyle {
    pub(super) computed: HtmlComputedStyle,
    pub(super) background: Option<Hsla>,
}

impl HtmlComputedStyle {
    pub(crate) fn root(theme: &Theme) -> Self {
        Self {
            color: theme.colors.text_default,
            font_size: theme.typography.text_size,
            root_font_size: theme.typography.text_size,
            text_align: TextAlign::Left,
        }
    }
}

pub(super) fn html_block_align(node: &HtmlNode) -> TextAlign {
    match attr_value(node, "align")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

pub(super) fn try_standalone_image_line(
    line: &str,
    base_dir: Option<&Path>,
    reference_definitions: &ImageReferenceDefinitions,
) -> Option<(String, String)> {
    let trimmed = line.trim();
    let syntax = parse_standalone_link_wrapped_image(trimmed)
        .or_else(|| parse_standalone_image(trimmed))?;
    let resolved = syntax.resolve_target(reference_definitions)?;
    let _ = resolve_image_source(&resolved.src, base_dir);
    Some((syntax.alt, resolved.src))
}

pub(super) fn html_node_visual_style(
    node: &HtmlNode,
    parent: HtmlComputedStyle,
    theme: &Theme,
) -> HtmlNodeVisualStyle {
    let c = &theme.colors;
    let t = &theme.typography;
    let mut computed = parent;
    let mut background = None;

    match node.tag_name.as_str() {
        "a" => computed.color = c.text_link,
        "blockquote" => computed.color = c.text_quote,
        "code" | "kbd" | "pre" => {
            computed.color = c.code_text;
            computed.font_size = t.code_size;
            background = Some(c.code_bg);
        }
        "mark" => background = Some(c.comment_bg),
        "figcaption" => {
            computed.color = c.image_caption_text;
            computed.font_size = t.code_size;
        }
        "small" | "sup" | "sub" => computed.font_size = (computed.font_size * 0.8).max(6.0),
        "th" => background = Some(c.table_header_bg),
        "td" => background = Some(c.table_cell_bg),
        _ => {}
    }

    let inline_style = style_for_node(node);
    if let Some(color) = inline_style.color {
        computed.color = html_css_color_to_hsla(color, computed.color);
    }
    if let Some(font_size) = inline_style.font_size {
        computed.font_size = font_size.resolve(computed.font_size, computed.root_font_size);
    }
    if let Some(color) = inline_style.background_color {
        background = Some(html_css_color_to_hsla(color, computed.color));
    }

    HtmlNodeVisualStyle {
        computed,
        background,
    }
}

pub(super) fn html_document_block_gap(dimensions: &ThemeDimensions, for_column: bool) -> f32 {
    if for_column {
        dimensions.callout_body_gap.max(8.0)
    } else {
        dimensions.block_gap * 0.4
    }
}

pub(super) fn html_column_heading_margin_bottom(dimensions: &ThemeDimensions) -> f32 {
    (dimensions.callout_header_margin_bottom * 1.75).max(10.0)
}

pub(super) fn html_body_line_height(typography: &crate::theme::ThemeTypography, for_column: bool) -> f32 {
    if for_column {
        1.45
    } else {
        typography.text_line_height
    }
}

pub(super) fn html_heading_line_height(for_column: bool) -> f32 {
    if for_column {
        1.2
    } else {
        1.25
    }
}

pub(super) fn html_table_cell_padding_y(dimensions: &ThemeDimensions, for_column: bool) -> f32 {
    if for_column {
        dimensions.table_cell_padding_y * 0.3
    } else {
        dimensions.table_cell_padding_y
    }
}

pub(super) fn html_table_body_line_height(for_column: bool) -> f32 {
    if for_column {
        1.2
    } else {
        1.45
    }
}

pub(super) fn html_table_child_nodes(children: &[HtmlNode]) -> impl Iterator<Item = &HtmlNode> {
    children
        .iter()
        .filter(|child| !should_skip_html_flow_child(child))
}

pub(super) fn html_table_collect_rows<'a>(table: &'a HtmlNode) -> Vec<&'a HtmlNode> {
    let mut rows = Vec::new();
    for child in html_table_child_nodes(&table.children) {
        match child.tag_name.as_str() {
            "thead" | "tbody" | "tfoot" => {
                for row in html_table_child_nodes(&child.children) {
                    if row.tag_name == "tr" {
                        rows.push(row);
                    }
                }
            }
            "tr" => rows.push(child),
            _ => {}
        }
    }
    rows
}

pub(super) fn html_table_row_cells<'a>(row: &'a HtmlNode) -> impl Iterator<Item = &'a HtmlNode> + 'a {
    html_table_child_nodes(&row.children)
        .filter(|cell| cell.tag_name == "th" || cell.tag_name == "td")
}

pub(super) fn html_table_column_count(table: &HtmlNode) -> usize {
    html_table_collect_rows(table)
        .iter()
        .map(|row| html_table_row_cells(row).count())
        .max()
        .unwrap_or(1)
        .max(1)
}

pub(super) fn is_collapsible_html_whitespace(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

pub(super) fn should_skip_html_flow_child(node: &HtmlNode) -> bool {
    node.tag_name == "#text" && is_collapsible_html_whitespace(&node.raw_source)
}

pub(super) fn constrain_html_block_for_column(element: Div, for_column: bool, full_width: bool) -> Div {
    if for_column {
        element.w_full().min_w(px(0.0))
    } else if full_width {
        element.w_full()
    } else {
        element
    }
}

pub(super) fn html_is_inline_semantic_tag(tag: &str) -> bool {
    matches!(
        tag,
        "strong"
            | "b"
            | "em"
            | "i"
            | "span"
            | "abbr"
            | "dfn"
            | "time"
            | "u"
            | "ins"
            | "del"
            | "small"
            | "sup"
            | "sub"
            | "a"
            | "mark"
            | "code"
            | "kbd"
            | "q"
    )
}

pub(super) fn html_children_are_plain_text(children: &[HtmlNode]) -> bool {
    children
        .iter()
        .filter(|child| !should_skip_html_flow_child(child))
        .all(|child| child.tag_name == "#text" || html_is_inline_semantic_tag(&child.tag_name))
}

pub(super) fn html_collect_visible_text(nodes: &[HtmlNode]) -> String {
    let mut text = String::new();
    for node in nodes {
        if should_skip_html_flow_child(node) {
            continue;
        }
        match node.tag_name.as_str() {
            "#text" => text.push_str(&node.raw_source),
            "br" => text.push('\n'),
            tag if html_is_inline_semantic_tag(tag) => {
                text.push_str(&html_collect_visible_text(&node.children));
            }
            _ => {}
        }
    }
    text
}

pub(super) fn html_list_line_height(for_column: bool) -> f32 {
    if for_column {
        1.25
    } else {
        1.45
    }
}

impl Block {
    pub(super) fn on_html_details_toggle_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.html_details_open = !self.html_details_open;
        cx.stop_propagation();
        cx.notify();
    }
    pub(super) fn render_html_document(
        &self,
        document: &HtmlDocument,
        theme: &Theme,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        if !document.is_semantic() {
            let mut element = div()
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x))
                .py(px(d.block_padding_y))
                .text_size(px(t.code_size))
                .text_color(c.text_default)
                .child(SharedString::from(document.raw_source.clone()));
            if for_column {
                element = element.w_full().min_w(px(0.0));
            } else {
                element = element.w_full();
            }
            return element.into_any_element();
        }

        let block_gap = html_document_block_gap(d, for_column);
        let body_line_height = html_body_line_height(t, for_column);
        let element = div()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(block_gap))
            .line_height(rems(body_line_height))
            .children(
                document
                    .nodes
                    .iter()
                    .filter(|node| !should_skip_html_flow_child(node))
                    .map(|node| {
                        self.render_html_node(
                            node,
                            theme,
                            HtmlComputedStyle::root(theme),
                            for_column,
                            cx,
                        )
                    }),
            );
        constrain_html_block_for_column(element, for_column, !for_column).into_any_element()
    }

    fn render_html_node(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let body_line_height = html_body_line_height(t, for_column);

        if node.kind == HtmlNodeKind::RawTextBlock {
            return div()
                .w_full()
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x * 0.6))
                .py(px(d.block_padding_y * 0.6))
                .text_size(px(t.code_size))
                .text_color(c.text_default)
                .child(SharedString::from(node.raw_source.clone()))
                .into_any_element();
        }

        if node.tag_name == "#text" {
            return self.render_html_text_node(
                &node.raw_source,
                theme,
                inherited_style,
                for_column,
                body_line_height,
                cx,
            );
        }

        let node_style = html_node_visual_style(node, inherited_style, theme);
        match node.tag_name.as_str() {
            "strong" | "b" => {
                self.render_html_inline_container(node, theme, node_style, FontWeight::BOLD, for_column, body_line_height, cx)
            }
            "em" | "i" | "span" | "abbr" | "dfn" | "time" | "u" | "ins" | "del" | "small"
            | "sup" | "sub" | "a" => {
                self.render_html_inline_container(node, theme, node_style, FontWeight::NORMAL, for_column, body_line_height, cx)
            }
            "mark" => {
                self.render_html_inline_container(node, theme, node_style, FontWeight::NORMAL, for_column, body_line_height, cx)
            }
            "code" | "kbd" => {
                let mut element =
                    div()
                        .flex()
                        .rounded(px(4.0))
                        .px(px(4.0))
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .line_height(rems(body_line_height))
                        .children(
                            node
                                .children
                                .iter()
                                .filter(|child| !should_skip_html_flow_child(child))
                                .map(|child| {
                                    self.render_html_node(child, theme, node_style.computed, for_column, cx)
                                }),
                        );
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "q" => {
                let mut element = div()
                    .flex()
                    .text_size(px(node_style.computed.font_size))
                    .text_color(node_style.computed.color)
                    .line_height(rems(body_line_height))
                    .children([
                        div().child("\u{201C}").into_any_element(),
                        div()
                            .children(node.children.iter().map(|child| {
                                self.render_html_node(child, theme, node_style.computed, for_column, cx)
                            }))
                            .into_any_element(),
                        div().child("\u{201D}").into_any_element(),
                    ]);
                if let Some(bg) = node_style.background {
                    element = element.bg(bg).rounded(px(3.0)).px(px(2.0));
                }
                element.into_any_element()
            }
            "br" => div().child("\n").into_any_element(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let (size, weight) = match node.tag_name.as_str() {
                    "h1" => (t.h1_size, FontWeight::BOLD),
                    "h2" => (t.h2_size, FontWeight::BOLD),
                    "h3" => (t.h3_size, FontWeight::SEMIBOLD),
                    "h4" => (t.h4_size, FontWeight::SEMIBOLD),
                    "h5" => (t.h5_size, FontWeight::MEDIUM),
                    _ => (t.h6_size, FontWeight::MEDIUM),
                };
                let mut element = div()
                    .w_full()
                    .min_w(px(0.0))
                    .text_size(px(size))
                    .text_color(node_style.computed.color)
                    .font_weight(weight)
                    .line_height(rems(html_heading_line_height(for_column)))
                    .child(if for_column && html_children_are_plain_text(&node.children) {
                        div()
                            .w_full()
                            .min_w(px(0.0))
                            .child(SharedString::from(html_collect_visible_text(&node.children)))
                            .into_any_element()
                    } else {
                        div()
                            .w_full()
                            .min_w(px(0.0))
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .children(node.children.iter().map(|child| {
                                self.render_html_node(child, theme, node_style.computed, for_column, cx)
                            }))
                            .into_any_element()
                    });
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                if for_column {
                    element = element.mb(px(html_column_heading_margin_bottom(d)));
                }
                element.into_any_element()
            }
            "p" => {
                let element = if for_column {
                    self.render_html_inline_flow(
                        &node.children,
                        theme,
                        node_style.computed,
                        node_style.computed.font_size,
                        node_style.computed.color,
                        body_line_height,
                        true,
                        cx,
                    )
                } else {
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_start()
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .line_height(rems(body_line_height))
                        .children(self.render_html_compact_children(
                            &node.children,
                            theme,
                            node_style.computed,
                            for_column,
                            for_column,
                            cx,
                        ))
                        .into_any_element()
                };
                if let Some(bg) = node_style.background {
                    div().bg(bg).child(element).into_any_element()
                } else {
                    element
                }
            }
            "ul" | "ol" => {
                let list_gap = if for_column {
                    0.0
                } else {
                    d.block_gap * 0.25
                };
                let list_line_height = if for_column {
                    html_list_line_height(true)
                } else {
                    body_line_height
                };
                let list_children: Vec<_> = node
                    .children
                    .iter()
                    .filter(|child| !should_skip_html_flow_child(child))
                    .collect();
                let mut element = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .text_size(px(node_style.computed.font_size))
                    .text_color(node_style.computed.color)
                    .line_height(rems(list_line_height));
                if list_gap > 0.0 {
                    element = element.gap(px(list_gap));
                }
                element = element.children(list_children.iter().enumerate().map(|(index, child)| {
                        if child.tag_name == "li" {
                            self.render_html_list_item(
                                child,
                                theme,
                                node_style.computed,
                                node.tag_name == "ol",
                                index + 1,
                                for_column,
                                list_line_height,
                                cx,
                            )
                        } else {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        }
                    }));
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "li" => self.render_html_list_item(
                node,
                theme,
                node_style.computed,
                false,
                1,
                for_column,
                body_line_height,
                cx,
            ),
            "hr" => div()
                .w_full()
                .h(px(d.separator_thickness))
                .my(px(d.separator_margin_y))
                .bg(c.separator_color)
                .rounded(px(999.0))
                .into_any_element(),
            "blockquote" => {
                let quote_gap = if for_column {
                    d.block_gap * 0.12
                } else {
                    d.block_gap * 0.25
                };
                let mut element =
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(quote_gap))
                        .pl(px(d.quote_padding_left))
                        .border_l(px(d.quote_border_width))
                        .border_color(c.border_quote)
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .line_height(rems(body_line_height))
                        .children(
                            node
                                .children
                                .iter()
                                .filter(|child| !should_skip_html_flow_child(child))
                                .map(|child| {
                                    self.render_html_node(child, theme, node_style.computed, for_column, cx)
                                }),
                        );
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "pre" => self.render_html_code_block(
                &html_children_text(node),
                code::html_pre_code_language(node).as_deref(),
                theme,
                for_column,
                node_style,
            ),
            "img" => self.render_html_image(node, theme, node_style, cx),
            "table" => self.render_html_table(node, theme, node_style, for_column, cx),
            "thead" | "tbody" | "tfoot" => {
                let table_line_height = html_table_body_line_height(for_column);
                let mut element =
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_start()
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .line_height(rems(table_line_height))
                        .children(html_table_child_nodes(&node.children).map(|child| {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        }));
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "tr" => self.render_html_table_row(node, theme, node_style, for_column, cx),
            "th" | "td" => {
                let cell_pad_y = html_table_cell_padding_y(d, for_column);
                let table_line_height = html_table_body_line_height(for_column);
                let mut element =
                    div()
                        .min_w(px(0.0))
                        .flex_grow()
                        .border(px(1.0))
                        .border_color(c.table_border)
                        .px(px(d.table_cell_padding_x))
                        .py(px(cell_pad_y))
                        .font_weight(if node.tag_name == "th" {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .child(if for_column {
                            self.render_html_inline_flow(
                                &node.children,
                                theme,
                                node_style.computed,
                                node_style.computed.font_size,
                                node_style.computed.color,
                                table_line_height,
                                true,
                                cx,
                            )
                        } else {
                            div()
                                .flex()
                                .flex_wrap()
                                .items_start()
                                .text_size(px(node_style.computed.font_size))
                                .text_color(node_style.computed.color)
                                .line_height(rems(body_line_height))
                                .children(self.render_html_compact_children(
                                    &node.children,
                                    theme,
                                    node_style.computed,
                                    for_column,
                                    for_column,
                                    cx,
                                ))
                                .into_any_element()
                        });
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "details" => self.render_html_details(node, theme, node_style, for_column, cx),
            "summary" => {
                let mut element =
                    div()
                        .w_full()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .children(node.children.iter().map(|child| {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        }));
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "figure" => {
                let mut element =
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(d.image_caption_gap))
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .children(node.children.iter().map(|child| {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        }));
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "figcaption" => {
                let mut element =
                    div()
                        .w_full()
                        .text_center()
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .children(node.children.iter().map(|child| {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        }));
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            "div" => {
                let align = html_block_align(node);
                let mut child_style = node_style.computed;
                child_style.text_align = align;
                let mut element = div()
                    .w_full()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(d.block_gap * 0.25))
                    .text_size(px(node_style.computed.font_size))
                    .text_color(node_style.computed.color)
                    .line_height(rems(body_line_height))
                    .children(
                        node
                            .children
                            .iter()
                            .filter(|child| !should_skip_html_flow_child(child))
                            .map(|child| {
                                self.render_html_node(child, theme, child_style, for_column, cx)
                            }),
                    );
                element = match align {
                    TextAlign::Center => element.items_center(),
                    TextAlign::Right => element.items_end(),
                    TextAlign::Left => element.items_start(),
                };
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
            _ => {
                let mut element =
                    div()
                        .w_full()
                        .text_size(px(node_style.computed.font_size))
                        .text_color(node_style.computed.color)
                        .children(node.children.iter().map(|child| {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        }));
                if let Some(bg) = node_style.background {
                    element = element.bg(bg);
                }
                element.into_any_element()
            }
        }
    }

    fn render_html_inline_container(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        node_style: HtmlNodeVisualStyle,
        weight: FontWeight,
        for_column: bool,
        body_line_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut element = div()
            .flex()
            .min_w(px(0.0))
            .text_size(px(node_style.computed.font_size))
            .text_color(node_style.computed.color)
            .font_weight(weight)
            .line_height(rems(body_line_height))
            .children(
                node.children
                    .iter()
                    .filter(|child| !should_skip_html_flow_child(child))
                    .map(|child| self.render_html_node(child, theme, node_style.computed, for_column, cx)),
            );
        if let Some(bg) = node_style.background {
            element = element.bg(bg).rounded(px(3.0)).px(px(2.0));
        }
        match node.tag_name.as_str() {
            "sup" => {
                element = element
                    .relative()
                    .top(px(-node_style.computed.font_size * 0.28))
            }
            "sub" => {
                element = element
                    .relative()
                    .top(px(node_style.computed.font_size * 0.22))
            }
            _ => {}
        }
        element.into_any_element()
    }


    fn render_html_inline_flow(
        &self,
        children: &[HtmlNode],
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        font_size: f32,
        color: Hsla,
        body_line_height: f32,
        flatten_paragraphs: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if html_children_are_plain_text(children) {
            return div()
                .w_full()
                .min_w(px(0.0))
                .text_size(px(font_size))
                .text_color(color)
                .line_height(rems(body_line_height))
                .child(SharedString::from(html_collect_visible_text(children)))
                .into_any_element();
        }

        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .items_start()
            .text_size(px(font_size))
            .text_color(color)
            .line_height(rems(body_line_height))
            .children(self.render_html_compact_children(
                children,
                theme,
                inherited_style,
                true,
                flatten_paragraphs,
                cx,
            ))
            .into_any_element()
    }

    fn render_html_compact_children(
        &self,
        children: &[HtmlNode],
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        for_column: bool,
        flatten_paragraphs: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut elements = Vec::new();
        for child in children {
            if should_skip_html_flow_child(child) {
                continue;
            }
            if flatten_paragraphs && child.tag_name == "p" {
                for grandchild in &child.children {
                    elements.push(self.render_html_node(
                        grandchild,
                        theme,
                        inherited_style,
                        for_column,
                        cx,
                    ));
                }
                continue;
            }
            elements.push(self.render_html_node(
                child,
                theme,
                inherited_style,
                for_column,
                cx,
            ));
        }
        elements
    }

    fn render_html_list_item(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        ordered: bool,
        ordinal: usize,
        for_column: bool,
        body_line_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let marker = if ordered {
            format!("{ordinal}.")
        } else {
            BULLET_FILLED.to_string()
        };
        let node_style = html_node_visual_style(node, inherited_style, theme);
        let d = &theme.dimensions;
        let marker_width = if ordered {
            d.ordered_list_marker_width
        } else {
            20.0
        };
        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .items_start()
            .gap(px(d.list_marker_gap))
            .text_size(px(node_style.computed.font_size))
            .text_color(node_style.computed.color)
            .line_height(rems(body_line_height))
            .child(
                div()
                    .min_w(px(marker_width))
                    .flex_shrink_0()
                    .text_color(theme.colors.dialog_muted)
                    .line_height(rems(body_line_height))
                    .child(marker),
            )
            .child(
                if for_column {
                    self.render_html_inline_flow(
                        &node.children,
                        theme,
                        node_style.computed,
                        node_style.computed.font_size,
                        node_style.computed.color,
                        body_line_height,
                        true,
                        cx,
                    )
                } else {
                    div()
                        .min_w(px(0.0))
                        .flex_grow()
                        .flex()
                        .flex_wrap()
                        .items_start()
                        .children(self.render_html_compact_children(
                            &node.children,
                            theme,
                            node_style.computed,
                            for_column,
                            for_column,
                            cx,
                        ))
                        .into_any_element()
                },
            )
            .into_any_element()
    }

    fn render_html_text_node(
        &self,
        raw_source: &str,
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        for_column: bool,
        body_line_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let trimmed = raw_source.trim();
        if !raw_source.contains('\n')
            && let Some((alt, src)) = try_standalone_image_line(
                trimmed,
                self.image_base_dir(),
                &self.image_reference_definitions(),
            )
        {
            return self.render_html_markdown_image_line(alt, src, theme, cx);
        }

        let mut elements = Vec::new();
        for line in raw_source.split('\n') {
            if is_collapsible_html_whitespace(line) {
                continue;
            }
            if let Some((alt, src)) = try_standalone_image_line(
                line,
                self.image_base_dir(),
                &self.image_reference_definitions(),
            ) {
                elements.push(self.render_html_markdown_image_line(alt, src, theme, cx));
            } else {
                elements.push(self.render_html_markdown_text_line(
                    line,
                    theme,
                    inherited_style,
                    body_line_height,
                    cx,
                ));
            }
        }

        if elements.is_empty() {
            return div().into_any_element();
        }
        if elements.len() == 1 {
            return elements.into_iter().next().expect("single element");
        }

        let d = &theme.dimensions;
        let mut container = div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap(px(d.block_gap * 0.25));
        container = match inherited_style.text_align {
            TextAlign::Center => container.items_center(),
            TextAlign::Right => container.items_end(),
            TextAlign::Left => container.items_start(),
        };
        constrain_html_block_for_column(container, for_column, false)
            .children(elements)
            .into_any_element()
    }

    fn render_html_markdown_text_line(
        &self,
        line: &str,
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        body_line_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let segments = parse_html_text_line_segments(line);
        if segments.len() == 1 {
            match &segments[0] {
                HtmlTextLineSegment::Text(text) => {
                    return div()
                        .min_w(px(0.0))
                        .flex_shrink_0()
                        .text_size(px(inherited_style.font_size))
                        .text_color(inherited_style.color)
                        .line_height(rems(body_line_height))
                        .text_align(inherited_style.text_align)
                        .child(SharedString::from(text.clone()))
                        .into_any_element();
                }
                HtmlTextLineSegment::Link { label, hit } => {
                    return self.render_html_markdown_link(
                        label,
                        hit,
                        theme,
                        inherited_style,
                        body_line_height,
                        cx,
                    );
                }
            }
        }

        div()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .items_start()
            .text_size(px(inherited_style.font_size))
            .text_color(inherited_style.color)
            .line_height(rems(body_line_height))
            .text_align(inherited_style.text_align)
            .children(segments.iter().map(|segment| match segment {
                HtmlTextLineSegment::Text(text) => div()
                    .flex_shrink_0()
                    .child(SharedString::from(text.clone()))
                    .into_any_element(),
                HtmlTextLineSegment::Link { label, hit } => self.render_html_markdown_link(
                    label,
                    hit,
                    theme,
                    inherited_style,
                    body_line_height,
                    cx,
                ),
            }))
            .into_any_element()
    }

    fn render_html_markdown_link(
        &self,
        label: &str,
        hit: &InlineLinkHit,
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        body_line_height: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let hit = hit.clone();
        let block = cx.entity().downgrade();
        div()
            .flex_shrink_0()
            .text_size(px(inherited_style.font_size))
            .line_height(rems(body_line_height))
            .text_color(c.text_link)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                cx.stop_propagation();
                let _ = block.update(cx, |_block, cx| {
                    cx.emit(BlockEvent::RequestOpenLink {
                        prompt_target: hit.prompt_target.clone(),
                        open_target: hit.open_target.clone(),
                        is_workspace_file: hit.is_workspace_file,
                        is_document_relative_file: hit.is_document_relative_file,
                    });
                });
            })
            .child(SharedString::from(label.to_string()))
            .into_any_element()
    }

    fn render_html_markdown_image_line(
        &self,
        alt: String,
        src: String,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let runtime = ImageRuntime {
            alt,
            src: src.clone(),
            title: None,
            resolved_source: resolve_image_source(&src, self.image_base_dir()),
        };
        let strings = cx.global::<I18nManager>().strings_arc();
        self.render_image_content(
            &runtime,
            Length::Definite(relative(1.0)),
            px(theme.dimensions.image_root_max_height),
            px(theme.dimensions.image_root_placeholder_height),
            theme,
            &strings,
        )
    }

    fn render_html_image(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        node_style: HtmlNodeVisualStyle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let parsed_image = parse_html_image_block(&node.raw_source);
        let src = parsed_image
            .as_ref()
            .map(|image| image.src.as_str())
            .or_else(|| attr_value(node, "src"))
            .filter(|src| !src.trim().is_empty());
        let Some(src) = src else {
            let mut element = div()
                .text_size(px(node_style.computed.font_size))
                .text_color(node_style.computed.color)
                .child(SharedString::from(node.raw_source.clone()));
            if let Some(bg) = node_style.background {
                element = element.bg(bg);
            }
            return element.into_any_element();
        };
        let alt = parsed_image
            .as_ref()
            .map(|image| image.alt.clone())
            .unwrap_or_else(|| attr_value(node, "alt").unwrap_or_default().to_string());
        let zoom = parsed_image
            .as_ref()
            .map(|image| image.zoom_factor())
            .unwrap_or(1.0);
        let runtime = ImageRuntime {
            alt,
            src: src.to_string(),
            title: None,
            resolved_source: resolve_image_source(src, self.image_base_dir()),
        };
        let strings = cx.global::<I18nManager>().strings_arc();
        let content = self.render_image_content(
            &runtime,
            Length::Definite(relative(zoom)),
            px(theme.dimensions.image_root_max_height * zoom),
            px(theme.dimensions.image_root_placeholder_height * zoom),
            theme,
            &strings,
        );
        if let Some(bg) = node_style.background {
            div().w_full().bg(bg).child(content).into_any_element()
        } else {
            content
        }
    }

    fn render_html_table(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        node_style: HtmlNodeVisualStyle,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let column_count = html_table_column_count(node);
        let column_layout = TableColumnLayout::equal(column_count);
        let rows = html_table_collect_rows(node);
        let table_line_height = html_table_body_line_height(for_column);
        let mut element = div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .items_start()
            .border(px(1.0))
            .border_color(theme.colors.table_border)
            .text_size(px(node_style.computed.font_size))
            .text_color(node_style.computed.color)
            .line_height(rems(table_line_height))
            .children(rows.iter().map(|row| {
                self.render_html_table_row_with_layout(
                    row,
                    &column_layout,
                    theme,
                    node_style,
                    for_column,
                    cx,
                )
            }));
        if let Some(bg) = node_style.background {
            element = element.bg(bg);
        }
        element.into_any_element()
    }

    fn render_html_table_row_with_layout(
        &self,
        row: &HtmlNode,
        column_layout: &TableColumnLayout,
        theme: &Theme,
        node_style: HtmlNodeVisualStyle,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut element = div()
            .w_full()
            .flex()
            .items_start()
            .text_size(px(node_style.computed.font_size))
            .text_color(node_style.computed.color)
            .children(html_table_row_cells(row).enumerate().map(|(column, cell)| {
                self.render_html_table_cell(
                    cell,
                    column,
                    column_layout,
                    theme,
                    node_style.computed,
                    for_column,
                    cx,
                )
            }));
        if let Some(bg) = node_style.background {
            element = element.bg(bg);
        }
        element.into_any_element()
    }

    fn render_html_table_cell(
        &self,
        cell: &HtmlNode,
        column: usize,
        column_layout: &TableColumnLayout,
        theme: &Theme,
        inherited_style: HtmlComputedStyle,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let body_line_height = html_body_line_height(&theme.typography, for_column);
        let cell_pad_y = html_table_cell_padding_y(d, for_column);
        let table_line_height = html_table_body_line_height(for_column);
        let node_style = html_node_visual_style(cell, inherited_style, theme);
        let column_fraction = column_layout.fraction(column);
        let mut element = div()
            .min_w(px(0.0))
            .flex_shrink_0()
            .flex_basis(relative(column_fraction))
            .w(relative(column_fraction))
            .border(px(1.0))
            .border_color(c.table_border)
            .px(px(d.table_cell_padding_x))
            .py(px(cell_pad_y))
            .font_weight(if cell.tag_name == "th" {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .child(if for_column {
                self.render_html_inline_flow(
                    &cell.children,
                    theme,
                    node_style.computed,
                    node_style.computed.font_size,
                    node_style.computed.color,
                    table_line_height,
                    true,
                    cx,
                )
            } else {
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .text_size(px(node_style.computed.font_size))
                    .text_color(node_style.computed.color)
                    .line_height(rems(body_line_height))
                    .children(self.render_html_compact_children(
                        &cell.children,
                        theme,
                        node_style.computed,
                        for_column,
                        for_column,
                        cx,
                    ))
                    .into_any_element()
            });
        if let Some(bg) = node_style.background {
            element = element.bg(bg);
        }
        element.into_any_element()
    }

    fn render_html_table_row(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        node_style: HtmlNodeVisualStyle,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut element = div()
            .w_full()
            .flex()
            .items_start()
            .text_size(px(node_style.computed.font_size))
            .text_color(node_style.computed.color)
            .children(html_table_child_nodes(&node.children).map(|child| {
                self.render_html_node(child, theme, node_style.computed, for_column, cx)
            }));
        if let Some(bg) = node_style.background {
            element = element.bg(bg);
        }
        element.into_any_element()
    }

    fn render_html_details(
        &self,
        node: &HtmlNode,
        theme: &Theme,
        node_style: HtmlNodeVisualStyle,
        for_column: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_open = attr_value(node, "open").is_some() || self.html_details_open;
        let summary = node
            .children
            .iter()
            .find(|child| child.tag_name == "summary");
        let body = node
            .children
            .iter()
            .filter(|child| child.tag_name != "summary");

        let mut container = div()
            .w_full()
            .rounded_sm()
            .border(px(1.0))
            .border_color(theme.colors.table_border)
            .px(px(theme.dimensions.block_padding_x))
            .py(px(theme.dimensions.block_padding_y))
            .text_size(px(node_style.computed.font_size))
            .text_color(node_style.computed.color)
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap(px(theme.dimensions.list_marker_gap))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_html_details_toggle_mouse_down),
                    )
                    .child(if is_open { "\u{25BE}" } else { "\u{25B8}" })
                    .children(summary.into_iter().map(|summary| {
                        self.render_html_node(summary, theme, node_style.computed, for_column, cx)
                    })),
            );
        if let Some(bg) = node_style.background {
            container = container.bg(bg);
        }

        if is_open {
            container =
                container.child(
                    div()
                        .w_full()
                        .pt(px(theme.dimensions.block_padding_y))
                        .children(body.map(|child| {
                            self.render_html_node(child, theme, node_style.computed, for_column, cx)
                        })),
                );
        }

        container.into_any_element()
    }

}
