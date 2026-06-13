//! Rendering for [`Block`] via GPUI's high-level [`Render`] trait.
//!
//! Each block kind produces a distinct visual style: H1 has a bottom border,
//! list items render a marker column (bullet / ordinal), and raw Markdown
//! fallback renders as plain text.

use gpui::*;
use pulldown_cmark::{Options as CmarkOptions, Parser as CmarkParser, html as cmark_html};
use std::ops::Range;

use super::element::{BlockTextElement, InlineTreePreviewTextElement};
use super::{
    Block, BlockEvent, BlockKind, CodeHighlightSpan, ImageResolvedSource, ImageRuntime,
    code_highlight_color, highlight_code_block,
};
use crate::components::{
    Editor, HtmlCssColor, HtmlDocument, HtmlNode, HtmlNodeKind, InlineScript, TableAxisHighlight,
    TableAxisKind, TableCellInlineImageSegment, TableCellPosition, TableColumnAlignment,
    TableColumnLayout, TableData, attr_value, collect_table_candidate_region,
    display_math_font_size, inline_math_font_size, is_table_candidate_line,
    parse_display_math_source, parse_html_document, parse_html_image_block,
    parse_mermaid_fence_source, parse_mermaid_fence_start, is_mermaid_closing_fence,
    parse_table_cell_inline_images, parse_table_region, render_display_math_svg,
    render_inline_math_svg, render_mermaid_svg_for_display, resolve_image_source,
    serialize_table_markdown_lines, style_for_node,
};
use crate::code_runner::{
    CodeRunStatus, CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES, code_run_output_line_count,
};
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::{Theme, ThemeDimensions, ThemeManager};

// Unicode bullet glyphs for nested list depths.
const BULLET_FILLED: &str = "\u{2022}";
const BULLET_HOLLOW: &str = "\u{25E6}";
const BULLET_SQUARE: &str = "\u{25A1}";
const TASK_CHECKMARK: &str = "\u{2713}";
const ICON_CODE_BLOCK_COPY: &str = "icon/toolbar/copy.svg";
const ICON_CODE_BLOCK_COLLAPSE: &str = "icon/toolbar/chevrons-down-up.svg";
const ICON_CODE_BLOCK_EXPAND: &str = "icon/toolbar/chevrons-up-down.svg";
const ICON_CODE_BLOCK_RUN: &str = "icon/toolbar/circle-play.svg";
const ICON_CODE_BLOCK_STOP: &str = "icon/toolbar/circle-stop.svg";
const ICON_CODE_BLOCK_CLOSE: &str = "icon/toolbar/x.svg";
const ICON_CODE_RUN_OUTPUT_CHEVRON_DOWN: &str = "icon/toolbar/chevron-down.svg";
const ICON_CODE_RUN_OUTPUT_CHEVRON_UP: &str = "icon/toolbar/chevron-up.svg";
const ICON_TABLE_COLUMN_MENU: &str = "icon/toolbar/ellipsis-vertical.svg";
const TABLE_COLUMN_RESIZE_HANDLE_WIDTH: f32 = 8.0;

fn style_native_table_cell_borders(
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

fn bulleted_list_marker(depth: usize) -> &'static str {
    match depth {
        0 => BULLET_FILLED,
        1 => BULLET_HOLLOW,
        _ => BULLET_SQUARE,
    }
}

#[derive(Debug)]
pub(crate) struct RenderColumn {
    pub width_fraction: Option<f32>,
    pub markdown: String,
}

pub(crate) enum ColumnMarkdownSegment {
    Markdown(String),
    Mermaid(String),
    Table(TableData),
}

pub(crate) fn split_column_markdown_segments(markdown: &str) -> Vec<ColumnMarkdownSegment> {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let mut segments = Vec::new();
    let mut index = 0usize;
    let mut current_lines = Vec::new();
    let mut active_fence: Option<(char, usize)> = None;

    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, run_len)) = active_fence {
            current_lines.push(line.to_string());
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if is_table_candidate_line(line) {
            let trimmed = trim_blank_edges(&current_lines).join("\n");
            if !trimmed.is_empty() {
                segments.push(ColumnMarkdownSegment::Markdown(trimmed));
            }
            current_lines.clear();

            let line_strings = lines.iter().map(|line| (*line).to_string()).collect::<Vec<_>>();
            let end = collect_table_candidate_region(&line_strings, index);
            let region = line_strings[index..end].to_vec();
            if let Some(table) = parse_table_region(&region) {
                segments.push(ColumnMarkdownSegment::Table(table));
            } else {
                current_lines.extend(region);
            }
            index = end;
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            if let Some(mermaid_fence) = parse_mermaid_fence_start(line) {
                let trimmed = trim_blank_edges(&current_lines).join("\n");
                if !trimmed.is_empty() {
                    segments.push(ColumnMarkdownSegment::Markdown(trimmed));
                }
                current_lines.clear();

                let mut end = index + 1;
                while end < lines.len() && !is_mermaid_closing_fence(lines[end], mermaid_fence) {
                    end += 1;
                }
                if end < lines.len() {
                    segments.push(ColumnMarkdownSegment::Mermaid(lines[index..=end].join("\n")));
                    index = end + 1;
                    continue;
                }
            }

            current_lines.push(line.to_string());
            active_fence = Some(fence);
            index += 1;
            continue;
        }

        current_lines.push(line.to_string());
        index += 1;
    }

    let trimmed = trim_blank_edges(&current_lines).join("\n");
    if !trimmed.is_empty() {
        segments.push(ColumnMarkdownSegment::Markdown(trimmed));
    }
    segments
}

pub(crate) fn serialize_column_markdown_segments(segments: &[ColumnMarkdownSegment]) -> String {
    let mut parts = Vec::with_capacity(segments.len());
    for segment in segments {
        parts.push(match segment {
            ColumnMarkdownSegment::Markdown(markdown) => markdown.clone(),
            ColumnMarkdownSegment::Mermaid(raw_fence) => raw_fence.clone(),
            ColumnMarkdownSegment::Table(table) => serialize_table_markdown_lines(table).join("\n"),
        });
    }
    parts.join("\n\n")
}

pub(crate) fn serialize_columns_markdown(columns: &[RenderColumn]) -> String {
    let mut out = String::from("::: columns\n");
    for column in columns {
        out.push_str("--- column");
        if let Some(width_fraction) = column.width_fraction {
            let percent = (width_fraction * 100.0).round();
            out.push_str(&format!(" width={percent:.0}%"));
        }
        out.push('\n');
        if !column.markdown.is_empty() {
            out.push_str(&column.markdown);
            if !column.markdown.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out.push_str(":::\n");
    out
}

pub(crate) fn update_columns_host_table_markdown(
    host_markdown: &str,
    column_index: usize,
    segment_index: usize,
    table: &TableData,
) -> Option<String> {
    let mut columns = parse_columns_markdown(host_markdown)?;
    if column_index >= columns.len() {
        return None;
    }
    let mut segments = split_column_markdown_segments(&columns[column_index].markdown);
    if segment_index >= segments.len() {
        return None;
    }
    segments[segment_index] = ColumnMarkdownSegment::Table(table.clone());
    columns[column_index].markdown = serialize_column_markdown_segments(&segments);
    Some(serialize_columns_markdown(&columns))
}

fn column_mermaid_available_width(
    block: &Block,
    viewport_width: f32,
    column_count: usize,
    width_fraction: f32,
    stacked: bool,
    d: &ThemeDimensions,
) -> f32 {
    let budget = effective_image_width(block, viewport_width, d);
    if stacked {
        budget.max(120.0)
    } else {
        let column_gap = 24.0;
        let total_gap = column_gap * column_count.saturating_sub(1) as f32;
        ((budget - total_gap) * width_fraction).max(120.0)
    }
}

fn mermaid_available_height(viewport_height: f32, d: &ThemeDimensions) -> f32 {
    let reserved_height = d.menu_bar_height
        + d.format_toolbar_button_height
        + d.format_toolbar_padding_y * 2.0
        + d.format_toolbar_border_width
        + d.editor_padding * 2.0
        + d.block_padding_y * 2.0;
    (viewport_height - reserved_height).max(1.0)
}

fn markdown_html_options() -> CmarkOptions {
    let mut options = CmarkOptions::empty();
    options.insert(CmarkOptions::ENABLE_TABLES);
    options.insert(CmarkOptions::ENABLE_FOOTNOTES);
    options.insert(CmarkOptions::ENABLE_TASKLISTS);
    options.insert(CmarkOptions::ENABLE_STRIKETHROUGH);
    options.insert(CmarkOptions::ENABLE_GFM);
    options
}

fn render_markdown_to_html(markdown: &str) -> String {
    let parser = CmarkParser::new_ext(markdown, markdown_html_options());
    let mut html = String::new();
    cmark_html::push_html(&mut html, parser);
    html
}

fn columns_block_has_only_trailing_blank_lines(lines: &[&str], end: usize) -> bool {
    lines[end..].iter().all(|line| line.trim().is_empty())
}

pub(crate) fn parse_columns_markdown(markdown: &str) -> Option<Vec<RenderColumn>> {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    if lines.is_empty() || !is_columns_block_start(lines[0]) {
        return None;
    }
    let end = collect_columns_block_region(&lines, 0)?;
    if !columns_block_has_only_trailing_blank_lines(&lines, end) {
        return None;
    }
    let columns = parse_columns_region(&lines[1..end - 1]);
    (!columns.is_empty()).then_some(columns)
}

fn parse_columns_region(lines: &[&str]) -> Vec<RenderColumn> {
    let mut columns = Vec::new();
    let mut current_width = None;
    let mut current_lines = Vec::new();
    let mut seen_column = false;
    let mut active_fence: Option<(char, usize)> = None;

    for line in lines {
        if let Some((marker, run_len)) = active_fence {
            current_lines.push((*line).to_string());
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            current_lines.push((*line).to_string());
            active_fence = Some(fence);
            continue;
        }

        if let Some(width_fraction) = parse_column_marker(line) {
            if seen_column {
                columns.push(RenderColumn {
                    width_fraction: current_width.take(),
                    markdown: trim_blank_edges(&current_lines).join("\n"),
                });
                current_lines.clear();
            }
            current_width = width_fraction;
            seen_column = true;
            continue;
        }

        if seen_column {
            current_lines.push((*line).to_string());
        } else if !line.trim().is_empty() {
            return Vec::new();
        }
    }

    if seen_column {
        columns.push(RenderColumn {
            width_fraction: current_width,
            markdown: trim_blank_edges(&current_lines).join("\n"),
        });
    }

    columns
}

fn trim_blank_edges(lines: &[String]) -> Vec<String> {
    let mut start = 0usize;
    let mut end = lines.len();
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    lines[start..end].to_vec()
}

fn parse_column_marker(line: &str) -> Option<Option<f32>> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let rest = trimmed.strip_prefix("--- column")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    let mut width_fraction = None;
    for part in rest.split_whitespace() {
        if let Some(value) = part.strip_prefix("width=") {
            width_fraction = parse_column_width_fraction(value);
        }
    }
    Some(width_fraction)
}

fn parse_column_width_fraction(value: &str) -> Option<f32> {
    let percent = value.strip_suffix('%')?.parse::<f32>().ok()?;
    percent.is_finite().then_some((percent / 100.0).clamp(0.05, 1.0))
}

fn is_columns_block_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let Some(rest) = trimmed.strip_prefix("::: columns") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

fn is_columns_block_end(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.len() - trimmed.len() <= 3 && trimmed.trim_end() == ":::"
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }

    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let run_len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (run_len >= 3).then_some((marker, run_len))
}

fn is_closing_fence(line: &str, marker: char, opening_run_len: usize) -> bool {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return false;
    }

    let run_len = trimmed.chars().take_while(|ch| *ch == marker).count();
    run_len >= opening_run_len && trimmed[marker.len_utf8() * run_len..].trim().is_empty()
}

fn collect_columns_block_region(lines: &[&str], start: usize) -> Option<usize> {
    if !is_columns_block_start(lines[start]) {
        return None;
    }

    let mut index = start + 1;
    let mut active_fence: Option<(char, usize)> = None;
    while index < lines.len() {
        let line = lines[index];
        if let Some((marker, run_len)) = active_fence {
            if is_closing_fence(line, marker, run_len) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = opening_fence(line) {
            active_fence = Some(fence);
            index += 1;
            continue;
        }

        if is_columns_block_end(line) {
            return Some(index + 1);
        }
        index += 1;
    }

    None
}

impl Block {
    fn render_code_run_output_panel(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        run_lane_width: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = &self.code_run_snapshot;
        if !snapshot.shows_output_panel() {
            return div().into_any_element();
        }

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let running = snapshot.status == CodeRunStatus::Running;
        let panel_expanded = snapshot.output_expanded;
        let panel_toggle_icon = if panel_expanded {
            ICON_CODE_RUN_OUTPUT_CHEVRON_UP
        } else {
            ICON_CODE_RUN_OUTPUT_CHEVRON_DOWN
        };
        let icon_size = px((t.code_size - 1.0).max(10.0));
        let action_icon_extent = px(f32::from(icon_size) + 8.0);
        let code_line_height = t.code_size * t.text_line_height;
        let content_line_count = code_run_output_line_count(
            &snapshot.stdout,
            &snapshot.stderr,
            snapshot.error_message.as_deref(),
        );
        let content_collapsible =
            content_line_count > CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES;
        let content_collapsed = content_collapsible && !snapshot.output_content_expanded;
        let collapsed_max_height =
            px(code_line_height * CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES as f32);
        let hidden_line_count = content_line_count
            .saturating_sub(CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES);
        let output_panel_bg = Hsla::from(rgba(0xffffffff));
        // Inset the panel background from the block row edge. Code content uses
        // `pr(code_block_padding_x)`; add a little more so the white panel sits
        // slightly farther from the window edge than the code area above.
        let output_panel_edge_inset_right = d.code_block_padding_x + 0.0;

        let mut text_sections = div().w_full().flex().flex_col().gap(px(6.0));
        if !snapshot.stdout.is_empty() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.code_text)
                    .child(snapshot.stdout.clone()),
            );
        }
        if !snapshot.stderr.is_empty() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(snapshot.stderr.clone()),
            );
        }
        if let Some(error) = snapshot.error_message.as_ref() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(error.clone()),
            );
        }

        let exit_label = snapshot
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| strings.code_run_exit_none.clone());
        let meta = strings
            .code_run_meta_template
            .replace("{exit}", &exit_label)
            .replace("{duration}", &snapshot.duration_ms.to_string());

        let mut body = div().w_full();
        if panel_expanded {
            let mut content_wrapper = div().relative().w_full().child({
                let mut clipped = div().min_w(px(0.0)).w_full().child(text_sections);
                if content_collapsed {
                    clipped = clipped
                        .max_h(collapsed_max_height)
                        .overflow_hidden();
                }
                clipped
            });
            if content_collapsed {
                let expand_label = strings
                    .code_run_output_expand_lines_template
                    .replace("{count}", &hidden_line_count.to_string());
                content_wrapper = content_wrapper
                    .pb(px(code_line_height))
                    .child(
                        div()
                            .id("code-block-run-output-content-expand")
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(code_line_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .bg(output_panel_bg)
                            .border_t(px(1.0))
                            .border_color(c.code_language_input_border.opacity(0.35))
                            .cursor_pointer()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_code_block_run_output_content_toggle_mouse_down),
                            )
                            .child(
                                svg()
                                    .path(ICON_CODE_BLOCK_EXPAND)
                                    .size(icon_size)
                                    .text_color(c.code_language_input_text),
                            )
                            .child(
                                div()
                                    .text_size(px((t.code_size - 1.5).max(9.0)))
                                    .text_color(c.code_language_input_text.opacity(0.85))
                                    .child(expand_label),
                            ),
                    );
            }
            body = body.child(content_wrapper).child(
                div()
                    .mt(px(6.0))
                    .text_size(px((t.code_size - 1.0).max(10.0)))
                    .text_color(c.text_quote)
                    .child(meta),
            );
        }

        let mut actions = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .mr(px(8.0));
        if running {
            actions = actions.child(
                div()
                    .id("code-block-run-stop")
                    .w(action_icon_extent)
                    .h(action_icon_extent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .opacity(0.72)
                    .hover(|this| this.opacity(1.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_code_block_run_stop_mouse_down),
                    )
                    .child(
                        svg()
                            .path(ICON_CODE_BLOCK_STOP)
                            .size(icon_size)
                            .text_color(c.dialog_danger_button_bg),
                    ),
            );
        }
        if panel_expanded && content_collapsible && snapshot.output_content_expanded {
            actions = actions.child(
                div()
                    .id("code-block-run-output-content-collapse")
                    .w(action_icon_extent)
                    .h(action_icon_extent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .opacity(0.72)
                    .hover(|this| this.opacity(1.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_code_block_run_output_content_toggle_mouse_down),
                    )
                    .child(
                        svg()
                            .path(ICON_CODE_BLOCK_COLLAPSE)
                            .size(icon_size)
                            .text_color(c.code_language_input_text),
                    ),
            );
        }
        actions = actions.child(
            div()
                .id("code-block-run-output-toggle")
                .w(action_icon_extent)
                .h(action_icon_extent)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(0.72)
                .hover(|this| this.opacity(1.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_code_block_run_output_toggle_mouse_down),
                )
                .child(
                    svg()
                        .path(panel_toggle_icon)
                        .size(icon_size)
                        .text_color(c.code_language_input_text),
                ),
        );
        actions = actions.child(
            div()
                .id("code-block-run-output-close")
                .w(action_icon_extent)
                .h(action_icon_extent)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(0.72)
                .hover(|this| this.opacity(1.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_code_block_run_output_close_mouse_down),
                )
                .child(
                    svg()
                        .path(ICON_CODE_BLOCK_CLOSE)
                        .size(icon_size)
                        .text_color(c.code_language_input_text),
                ),
        );

        let run_lane_spacer = || {
            div()
                .flex_none()
                .flex_shrink_0()
                .w(run_lane_width)
                .bg(output_panel_bg)
        };

        let panel_content_lane = |child: AnyElement| {
            div().flex_grow().min_w(px(0.0)).child(child)
        };

        let panel = div()
            .id("code-block-run-output")
            .w_full()
            .border_t(px(1.0))
            .border_color(c.code_language_input_border.opacity(0.35))
            .bg(output_panel_bg)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .border_b(px(1.0))
                    .border_color(c.code_language_input_border.opacity(0.35))
                    .child(run_lane_spacer())
                    .child(panel_content_lane(
                        div()
                            .w_full()
                            .py(px(6.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px((t.code_size - 1.0).max(10.0)))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.code_language_input_text)
                                    .child(strings.code_run_output_title.clone()),
                            )
                            .child(actions)
                            .into_any_element(),
                    )),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(run_lane_spacer())
                    .child(panel_content_lane(
                        div()
                            .w_full()
                            .py(px(if panel_expanded { 8.0 } else { 0.0 }))
                            .child(body)
                            .into_any_element(),
                    )),
            );

        div()
            .w_full()
            .pr(px(output_panel_edge_inset_right))
            .child(panel)
            .into_any_element()
    }
}

fn fallback_image_label(alt: &str, strings: &I18nStrings) -> SharedString {
    if alt.trim().is_empty() {
        SharedString::from(strings.image_placeholder.clone())
    } else {
        SharedString::from(alt.to_string())
    }
}

fn render_image_placeholder(
    runtime: &ImageRuntime,
    width: Length,
    height: Pixels,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    div()
        .w(width)
        .h(height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(d.image_radius))
        .border(px(1.0))
        .border_color(c.image_placeholder_border)
        .bg(c.image_placeholder_bg)
        .px(px(d.block_padding_x))
        .text_center()
        .text_size(px(t.text_size))
        .text_color(c.image_placeholder_text)
        .child(fallback_image_label(&runtime.alt, strings))
        .into_any_element()
}

fn render_loading_placeholder(
    runtime: &ImageRuntime,
    width: Length,
    height: Pixels,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    div()
        .w(width)
        .h(height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(d.image_radius))
        .border(px(1.0))
        .border_color(c.image_placeholder_border)
        .bg(c.image_placeholder_bg)
        .px(px(d.block_padding_x))
        .text_center()
        .text_size(px(t.code_size))
        .text_color(c.image_placeholder_text)
        .child(if runtime.alt.trim().is_empty() {
            SharedString::from(strings.image_loading_without_alt.clone())
        } else {
            SharedString::from(
                strings
                    .image_loading_with_alt_template
                    .replace("{alt}", &runtime.alt),
            )
        })
        .into_any_element()
}

fn wrap_with_quote_guides(content: AnyElement, quote_depth: usize, theme: &Theme) -> AnyElement {
    if quote_depth == 0 {
        return content;
    }

    let c = &theme.colors;
    let d = &theme.dimensions;
    let guide_offset = d.quote_padding_left;
    let total_padding = guide_offset * quote_depth as f32;

    div()
        .w_full()
        .relative()
        .pl(px(total_padding))
        .child(content)
        .children((0..quote_depth).map(|level| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(guide_offset * level as f32))
                .w(px(d.quote_border_width))
                .bg(c.border_quote)
        }))
        .into_any_element()
}

fn callout_accent_and_background(variant: super::CalloutVariant, theme: &Theme) -> (Hsla, Hsla) {
    let c = &theme.colors;
    match variant {
        super::CalloutVariant::Note => (c.callout_note_border, c.callout_note_bg),
        super::CalloutVariant::Tip => (c.callout_tip_border, c.callout_tip_bg),
        super::CalloutVariant::Important => (c.callout_important_border, c.callout_important_bg),
        super::CalloutVariant::Warning => (c.callout_warning_border, c.callout_warning_bg),
        super::CalloutVariant::Caution => (c.callout_caution_border, c.callout_caution_bg),
    }
}

fn visible_quote_guides(block: &Block) -> usize {
    block.visible_quote_depth
}

fn effective_table_width(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let centered_width = Editor::centered_column_width(viewport_width, d);
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

fn container_image_width_budget(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let centered_width = Editor::centered_column_width(viewport_width, d);
    let visible_quote_guides = visible_quote_guides(block);
    let quote_inset = d.quote_padding_left * visible_quote_guides as f32;
    let callout_inset = if block.callout_depth > 0 {
        d.callout_padding_x * 2.0 + d.callout_border_width
    } else {
        0.0
    };

    centered_width - quote_inset - callout_inset
}

fn effective_image_width(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let list_inset = d.nested_block_indent * block.render_depth as f32;
    (container_image_width_budget(block, viewport_width, d) - d.block_padding_x * 2.0 - list_inset)
        .max(160.0)
}

fn effective_list_item_image_width(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let marker_width = match block.kind() {
        BlockKind::BulletedListItem => d.list_marker_width,
        BlockKind::TaskListItem { .. } => d.list_marker_width.max(d.task_checkbox_size),
        BlockKind::NumberedListItem => d.ordered_list_marker_width,
        _ => 0.0,
    };
    let list_inset = d.nested_block_indent * block.render_depth as f32;

    (container_image_width_budget(block, viewport_width, d)
        - d.block_padding_x * 2.0
        - list_inset
        - marker_width
        - d.list_marker_gap)
        .max(160.0)
}

/// Returns a human-readable list ordinal: numbers at depth 0, lowercase
/// letters at depth 1, and unicode roman numerals at depth 2+.
fn numbered_list_marker(depth: usize, ordinal: usize) -> String {
    match depth {
        0 => format!("{ordinal}."),
        1 => format!("{}.", alphabetic_list_marker(ordinal)),
        _ => format!("{}.", roman_list_marker(ordinal)),
    }
}

/// Expands beyond 26 by wrapping: a...z, a1...z1, a2...z2, ...
fn alphabetic_list_marker(ordinal: usize) -> String {
    const ALPHABET: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

    let ordinal = ordinal.max(1);
    if ordinal <= ALPHABET.len() {
        return char::from(ALPHABET[ordinal - 1]).to_string();
    }

    let wrapped = ordinal - (ALPHABET.len() + 1);
    let letter = char::from(ALPHABET[wrapped % ALPHABET.len()]);
    let suffix = wrapped + 1;
    format!("{letter}{suffix}")
}

/// Converts an ASCII roman numeral string to its unicode ligature equivalents
/// where possible (for example, "III" to a single roman numeral glyph).
fn roman_list_marker(ordinal: usize) -> String {
    let ascii = ascii_roman_numeral(ordinal.max(1));
    let mut index = 0;
    let mut marker = String::new();

    while index < ascii.len() {
        let remaining = &ascii[index..];
        if let Some((token_len, token)) = roman_unicode_token(remaining) {
            marker.push_str(token);
            index += token_len;
        } else {
            break;
        }
    }

    marker
}

fn ascii_roman_numeral(mut ordinal: usize) -> String {
    const MAP: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    for (value, symbol) in MAP {
        while ordinal >= *value {
            result.push_str(symbol);
            ordinal -= *value;
        }
    }
    result
}

fn roman_unicode_token(remaining: &str) -> Option<(usize, &'static str)> {
    const TOKENS: &[(&str, &str)] = &[
        ("XII", "\u{216B}"),
        ("XI", "\u{216A}"),
        ("IX", "\u{2168}"),
        ("VIII", "\u{2167}"),
        ("VII", "\u{2166}"),
        ("VI", "\u{2165}"),
        ("IV", "\u{2163}"),
        ("III", "\u{2162}"),
        ("II", "\u{2161}"),
        ("I", "\u{2160}"),
        ("V", "\u{2164}"),
        ("X", "\u{2169}"),
        ("L", "\u{216C}"),
        ("C", "\u{216D}"),
        ("D", "\u{216E}"),
        ("M", "\u{216F}"),
    ];

    TOKENS.iter().find_map(|(ascii, unicode)| {
        remaining
            .starts_with(ascii)
            .then_some((ascii.len(), *unicode))
    })
}

fn html_children_text(node: &HtmlNode) -> String {
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

fn html_code_language(node: &HtmlNode) -> Option<String> {
    let class = attr_value(node, "class")?;
    for token in class.split_whitespace() {
        if let Some(language) = token.strip_prefix("language-") {
            return Some(language.to_string());
        }
    }
    None
}

fn html_pre_code_language(node: &HtmlNode) -> Option<String> {
    node.children
        .iter()
        .find(|child| child.tag_name == "code")
        .and_then(html_code_language)
}

fn render_html_code_highlight_chunks(
    source: &str,
    range: Range<usize>,
    spans: &[CodeHighlightSpan],
    base_color: Hsla,
    colors: &crate::theme::ThemeColors,
    font_size: f32,
) -> Vec<AnyElement> {
    let mut boundaries = vec![range.start, range.end];
    for span in spans {
        if span.range.start < range.end && span.range.end > range.start {
            boundaries.push(span.range.start.max(range.start));
            boundaries.push(span.range.end.min(range.end));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut elements = Vec::new();
    let mut span_idx = 0usize;
    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start >= end {
            continue;
        }

        while span_idx < spans.len() && spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let color = spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end)
            .map(|span| code_highlight_color(colors, span.class))
            .unwrap_or(base_color);

        elements.push(
            div()
                .flex_shrink_0()
                .text_size(px(font_size))
                .text_color(color)
                .child(SharedString::from(source[start..end].to_string()))
                .into_any_element(),
        );
    }
    elements
}

#[derive(Clone, Copy, Debug)]
struct HtmlComputedStyle {
    color: Hsla,
    font_size: f32,
    root_font_size: f32,
}

#[derive(Clone, Copy, Debug)]
struct HtmlNodeVisualStyle {
    computed: HtmlComputedStyle,
    background: Option<Hsla>,
}

impl HtmlComputedStyle {
    fn root(theme: &Theme) -> Self {
        Self {
            color: theme.colors.text_default,
            font_size: theme.typography.text_size,
            root_font_size: theme.typography.text_size,
        }
    }
}

fn html_css_color_to_hsla(color: HtmlCssColor, current_color: Hsla) -> Hsla {
    match color {
        HtmlCssColor::CurrentColor => current_color,
        HtmlCssColor::Rgba(color) => Hsla::from(Rgba {
            r: color.red as f32 / 255.0,
            g: color.green as f32 / 255.0,
            b: color.blue as f32 / 255.0,
            a: color.alpha.clamp(0.0, 1.0),
        }),
    }
}

fn html_node_visual_style(
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

fn html_document_block_gap(dimensions: &ThemeDimensions, for_column: bool) -> f32 {
    if for_column {
        dimensions.block_gap * 0.15
    } else {
        dimensions.block_gap * 0.4
    }
}

fn html_body_line_height(typography: &crate::theme::ThemeTypography, for_column: bool) -> f32 {
    if for_column {
        1.45
    } else {
        typography.text_line_height
    }
}

fn html_heading_line_height(for_column: bool) -> f32 {
    if for_column {
        1.2
    } else {
        1.25
    }
}

fn html_table_cell_padding_y(dimensions: &ThemeDimensions, for_column: bool) -> f32 {
    if for_column {
        dimensions.table_cell_padding_y * 0.3
    } else {
        dimensions.table_cell_padding_y
    }
}

fn html_table_body_line_height(for_column: bool) -> f32 {
    if for_column {
        1.2
    } else {
        1.45
    }
}

fn html_table_child_nodes(children: &[HtmlNode]) -> impl Iterator<Item = &HtmlNode> {
    children
        .iter()
        .filter(|child| !should_skip_html_flow_child(child))
}

fn html_table_collect_rows<'a>(table: &'a HtmlNode) -> Vec<&'a HtmlNode> {
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

fn html_table_row_cells<'a>(row: &'a HtmlNode) -> impl Iterator<Item = &'a HtmlNode> + 'a {
    html_table_child_nodes(&row.children)
        .filter(|cell| cell.tag_name == "th" || cell.tag_name == "td")
}

fn html_table_column_count(table: &HtmlNode) -> usize {
    html_table_collect_rows(table)
        .iter()
        .map(|row| html_table_row_cells(row).count())
        .max()
        .unwrap_or(1)
        .max(1)
}

fn is_collapsible_html_whitespace(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn should_skip_html_flow_child(node: &HtmlNode) -> bool {
    node.tag_name == "#text" && is_collapsible_html_whitespace(&node.raw_source)
}

fn constrain_html_block_for_column(element: Div, for_column: bool, full_width: bool) -> Div {
    if for_column {
        element.w_full().min_w(px(0.0))
    } else if full_width {
        element.w_full()
    } else {
        element
    }
}

fn html_is_inline_semantic_tag(tag: &str) -> bool {
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

fn html_children_are_plain_text(children: &[HtmlNode]) -> bool {
    children
        .iter()
        .filter(|child| !should_skip_html_flow_child(child))
        .all(|child| child.tag_name == "#text" || html_is_inline_semantic_tag(&child.tag_name))
}

fn html_collect_visible_text(nodes: &[HtmlNode]) -> String {
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

fn html_list_line_height(for_column: bool) -> f32 {
    if for_column {
        1.25
    } else {
        1.45
    }
}

impl Block {
    fn on_html_details_toggle_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.html_details_open = !self.html_details_open;
        cx.stop_propagation();
        cx.notify();
    }

    fn render_image_content(
        &self,
        runtime: &ImageRuntime,
        max_width: Length,
        max_height: Pixels,
        placeholder_height: Pixels,
        theme: &Theme,
        strings: &I18nStrings,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let source = runtime.resolved_source.clone();
        let placeholder_theme = theme.clone();
        let loading_theme = theme.clone();
        let placeholder_strings = strings.clone();
        let loading_strings = strings.clone();
        let runtime_for_fallback = runtime.clone();
        let runtime_for_loading = runtime.clone();

        let image = match source {
            ImageResolvedSource::Local(path) => img(path),
            ImageResolvedSource::Remote(uri) => img(uri),
        }
        .max_w(max_width)
        .max_h(max_height)
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || {
            render_image_placeholder(
                &runtime_for_fallback,
                max_width,
                placeholder_height,
                &placeholder_theme,
                &placeholder_strings,
            )
        })
        .with_loading(move || {
            render_loading_placeholder(
                &runtime_for_loading,
                max_width,
                placeholder_height,
                &loading_theme,
                &loading_strings,
            )
        });

        let mut container = div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(d.image_caption_gap))
            .child(image);

        if let Some(title) = runtime
            .title
            .as_ref()
            .filter(|title| !title.trim().is_empty())
        {
            container = container.child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(t.code_size))
                    .text_color(c.image_caption_text)
                    .child(SharedString::from(title.clone())),
            );
        }

        container.into_any_element()
    }

    fn render_math_content(&self, theme: &Theme) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let raw = self
            .record
            .raw_fallback
            .as_deref()
            .unwrap_or_else(|| self.display_text());

        let Some(source) = parse_display_math_source(raw) else {
            return div()
                .w_full()
                .text_size(px(t.text_size))
                .line_height(rems(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw.to_string()))
                .into_any_element();
        };

        match render_display_math_svg(&source, c.text_default, display_math_font_size(t.text_size))
        {
            Ok(rendered) => div()
                .w_full()
                .flex()
                .justify_center()
                .py(px(d.block_padding_y.max(6.0)))
                .child(
                    img(rendered.path)
                        .max_w(Length::Definite(relative(1.0)))
                        .max_h(px(d.image_root_max_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element(),
            Err(err) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x))
                .py(px(d.block_padding_y))
                .text_size(px(t.text_size))
                .line_height(rems(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw.to_string()))
                .child(
                    div()
                        .text_size(px(t.code_size))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(format!("LaTeX render error: {err}"))),
                )
                .into_any_element(),
        }
    }

    fn render_mermaid_content(&self, theme: &Theme, window: &Window) -> AnyElement {
        let d = &theme.dimensions;
        let raw = self
            .record
            .raw_fallback
            .as_deref()
            .unwrap_or_else(|| self.display_text());
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width.max(px(1.0)));
        let viewport_height = f32::from(viewport.height.max(px(1.0)));
        let available_width = effective_image_width(self, viewport_width, d);
        let available_height = mermaid_available_height(viewport_height, d);
        self.render_mermaid_diagram(
            raw,
            available_width,
            available_height,
            theme,
            ElementId::Name(format!("mermaid-scroll-{}", self.record.id).into()),
        )
    }

    fn render_mermaid_diagram(
        &self,
        raw_fence: &str,
        available_width: f32,
        available_height: f32,
        theme: &Theme,
        scroll_id: ElementId,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let Some(source) = parse_mermaid_fence_source(raw_fence) else {
            return div()
                .w_full()
                .text_size(px(t.text_size))
                .line_height(rems(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw_fence.to_string()))
                .into_any_element();
        };

        match render_mermaid_svg_for_display(&source, available_width, available_height) {
            Ok(rendered) => {
                let display_width = rendered.display_width.max(1.0);
                let display_height = rendered.display_height.max(1.0);
                let image_path = rendered.path.clone();
                let image = move || {
                    img(image_path.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                };
                let content = if display_width <= available_width + 0.5 {
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(image())
                        .into_any_element()
                } else {
                    div()
                        .id(scroll_id)
                        .w_full()
                        .overflow_x_scroll()
                        .scrollbar_width(px(0.0))
                        .child(div().w(px(display_width)).child(image()))
                        .into_any_element()
                };

                div()
                    .w_full()
                    .py(px(d.block_padding_y.max(6.0)))
                    .child(content)
                    .into_any_element()
            }
            Err(err) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x))
                .py(px(d.block_padding_y))
                .text_size(px(t.text_size))
                .line_height(rems(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw_fence.to_string()))
                .child(
                    div()
                        .text_size(px(t.code_size))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(format!("Mermaid render error: {err}"))),
                )
                .into_any_element(),
        }
    }

    fn render_text_or_mixed_inline_visuals(
        &self,
        theme: &Theme,
        focused: bool,
        is_placeholder: bool,
        placeholder_text: Option<SharedString>,
        placeholder_color: Option<Hsla>,
        text_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Mixed inline visuals are display-only. Once focused, the text element
        // takes over so caret movement, projection markers, and IME ranges stay
        // anchored to editable text rather than rendered SVG/script offsets.
        // While document search highlights are active, keep BlockTextElement so
        // highlight overlays share the same text layout as the search query.
        if focused
            || is_placeholder
            || !self.has_mixed_inline_visuals()
            || !self.search_highlight_ranges.is_empty()
        {
            return match placeholder_text {
                Some(placeholder) => BlockTextElement::with_placeholder(
                    cx.entity(),
                    is_placeholder,
                    placeholder,
                    placeholder_color,
                )
                .into_any_element(),
                None => BlockTextElement::new(cx.entity(), is_placeholder).into_any_element(),
            };
        }

        self.render_mixed_inline_visual_runs(theme, text_color, font_size, font_weight)
    }

    fn render_mixed_inline_visual_runs(
        &self,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> AnyElement {
        self.render_inline_tree_runs(
            &self.record.title,
            theme,
            base_color,
            font_size,
            font_weight,
        )
    }

    fn render_inline_tree_runs(
        &self,
        tree: &crate::components::InlineTextTree,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> AnyElement {
        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(0.0))
            .text_size(px(font_size))
            .line_height(rems(theme.typography.text_line_height))
            .children(self.render_inline_tree_children(
                tree,
                theme,
                base_color,
                font_size,
                font_weight,
            ))
            .into_any_element()
    }

    fn render_inline_tree_children(
        &self,
        tree: &crate::components::InlineTextTree,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> Vec<AnyElement> {
        let cache = tree.render_cache();
        let text = cache.visible_text();
        let mut children = Vec::new();
        let mut cursor = 0usize;

        for span in cache.spans() {
            if cursor < span.range.start {
                children.push(self.render_inline_text_segment(
                    &text[cursor..span.range.start],
                    span,
                    theme,
                    base_color,
                    font_size,
                    font_weight,
                ));
            }

            let span_text = &text[span.range.clone()];
            if let Some(math) = span.math.as_ref() {
                children.push(
                    self.render_inline_math_segment(math, span, theme, base_color, font_size),
                );
            } else {
                children.push(self.render_inline_text_segment(
                    span_text,
                    span,
                    theme,
                    base_color,
                    font_size,
                    font_weight,
                ));
            }
            cursor = span.range.end;
        }

        if cursor < text.len() {
            let fallback_span = crate::components::InlineSpan {
                range: cursor..text.len(),
                style: crate::components::InlineStyle::default(),
                html_style: None,
                link: None,
                footnote: None,
                math: None,
            };
            children.push(self.render_inline_text_segment(
                &text[cursor..],
                &fallback_span,
                theme,
                base_color,
                font_size,
                font_weight,
            ));
        }

        children
    }

    fn render_inline_text_segment(
        &self,
        text: &str,
        span: &crate::components::InlineSpan,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> AnyElement {
        if text.is_empty() {
            return div().into_any_element();
        }

        let mut color = if span.link.is_some() || span.footnote.is_some() {
            theme.colors.text_link
        } else {
            base_color
        };
        if let Some(style) = span.html_style
            && let Some(html_color) = style.color
        {
            color = html_css_color_to_hsla(html_color, color);
        }

        let script_offset = match span.style.script {
            InlineScript::Normal => 0.0,
            InlineScript::Superscript => -font_size * 0.28,
            InlineScript::Subscript => font_size * 0.22,
        };
        let display_font_size = if span.style.has_script() {
            (font_size * 0.72).max(6.0)
        } else {
            font_size
        };

        let mut element = div()
            .min_w(px(0.0))
            .text_size(px(display_font_size))
            .line_height(rems(theme.typography.text_line_height))
            .text_color(color)
            .font_weight(if span.style.bold {
                FontWeight::BOLD
            } else {
                font_weight
            })
            .child(SharedString::from(text.to_string()));

        if script_offset != 0.0 {
            element = element.relative().top(px(script_offset));
        }

        if span.style.underline || span.link.is_some() || span.footnote.is_some() {
            element = element.underline();
        }
        if span.style.code {
            element = element
                .rounded(px(theme.dimensions.code_bg_radius))
                .px(px(theme.dimensions.code_bg_pad_x))
                .py(px(theme.dimensions.code_bg_pad_y))
                .bg(theme.colors.code_bg);
        }
        if let Some(style) = span.html_style
            && let Some(background) = style.background_color
        {
            element = element
                .rounded(px(3.0))
                .px(px(2.0))
                .bg(html_css_color_to_hsla(background, color));
        }

        element.into_any_element()
    }

    fn render_inline_math_segment(
        &self,
        math: &crate::components::InlineMath,
        span: &crate::components::InlineSpan,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
    ) -> AnyElement {
        let mut color = base_color;
        if let Some(style) = span.html_style
            && let Some(html_color) = style.color
        {
            color = html_css_color_to_hsla(html_color, color);
        }
        let math_size = inline_math_font_size(font_size);
        match render_inline_math_svg(&math.body, color, math_size) {
            Ok(rendered) => div()
                .flex()
                .items_center()
                .h(px(math_size * 1.65))
                .child(
                    img(rendered.path)
                        .max_h(px(math_size * 1.65))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element(),
            Err(_) => self.render_inline_text_segment(
                &math.source,
                span,
                theme,
                base_color,
                font_size,
                FontWeight::NORMAL,
            ),
        }
    }

    fn render_inline_image_content(
        &self,
        runtime: &ImageRuntime,
        theme: &Theme,
        strings: &I18nStrings,
    ) -> AnyElement {
        let d = &theme.dimensions;
        let source = runtime.resolved_source.clone();
        let max_height = px(d.image_cell_placeholder_height);
        let max_width =
            Length::Definite(px((d.image_cell_placeholder_height * 1.6).max(48.0)).into());
        let placeholder_theme = theme.clone();
        let loading_theme = theme.clone();
        let placeholder_strings = strings.clone();
        let loading_strings = strings.clone();
        let runtime_for_fallback = runtime.clone();
        let runtime_for_loading = runtime.clone();

        let image = match source {
            ImageResolvedSource::Local(path) => img(path),
            ImageResolvedSource::Remote(uri) => img(uri),
        }
        .max_w(max_width)
        .max_h(max_height)
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || {
            render_image_placeholder(
                &runtime_for_fallback,
                max_width,
                max_height,
                &placeholder_theme,
                &placeholder_strings,
            )
        })
        .with_loading(move || {
            render_loading_placeholder(
                &runtime_for_loading,
                max_width,
                max_height,
                &loading_theme,
                &loading_strings,
            )
        });

        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .child(image)
            .into_any_element()
    }

    fn render_table_cell_inline_images(
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
                .line_height(rems(theme.typography.text_line_height))
                .children(children)
                .into_any_element(),
        )
    }

    fn render_html_document(
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
                .line_height(rems(theme.typography.text_line_height))
                .children(children)
                .into_any_element(),
        )
    }

    fn render_table_data_preview(
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
                        .line_height(rems(t.text_line_height))
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
                            .line_height(rems(t.text_line_height))
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

    fn render_column_markdown_content(
        &self,
        markdown: &str,
        available_width: f32,
        available_height: f32,
        theme: &Theme,
        column_key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let segments = split_column_markdown_segments(markdown);
        let block_gap = html_document_block_gap(&theme.dimensions, true);
        div()
            .min_w(px(0.0))
            .w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(block_gap))
            .children(segments.into_iter().enumerate().map(|(index, segment)| {
                match segment {
                    ColumnMarkdownSegment::Markdown(markdown) => {
                        let html = render_markdown_to_html(&markdown);
                        let document = parse_html_document(&html);
                        self.render_html_document(&document, theme, true, cx)
                    }
                    ColumnMarkdownSegment::Mermaid(raw_fence) => self.render_mermaid_diagram(
                        &raw_fence,
                        available_width,
                        available_height,
                        theme,
                        ElementId::Name(format!("mermaid-col-{column_key}-{index}").into()),
                    ),
                    ColumnMarkdownSegment::Table(table) => {
                        let key = format!("{column_key}-{index}");
                        if let Some(table_entity) = self.column_embedded_tables.get(&key).cloned() {
                            table_entity.update(cx, |table_block, _cx| {
                                table_block.embedded_table_layout_width = Some(available_width);
                            });
                            table_entity.into_any_element()
                        } else {
                            self.render_table_data_preview(
                                &table,
                                available_width,
                                &format!("{column_key}-{index}"),
                                theme,
                                window,
                                cx,
                            )
                        }
                    }
                }
            }))
            .into_any_element()
    }

    fn render_columns_markdown(
        &self,
        columns: Vec<RenderColumn>,
        theme: &Theme,
        stacked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let column_count = columns.len().max(1);
        let equal_fraction = 1.0 / column_count as f32;
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width.max(px(1.0)));
        let viewport_height = f32::from(viewport.height.max(px(1.0)));
        let d = &theme.dimensions;
        let available_height = mermaid_available_height(viewport_height, d);
        let mut container = div()
            .w_full()
            .min_w(px(0.0))
            .flex_shrink_0()
            .flex()
            .gap(px(24.0))
            .items_start();
        if stacked {
            container = container.flex_col().items_start();
        }

        container
            .children(columns.into_iter().enumerate().map(|(column_index, column)| {
                let width_fraction = column.width_fraction.unwrap_or(equal_fraction);
                let available_width = column_mermaid_available_width(
                    self,
                    viewport_width,
                    column_count,
                    width_fraction,
                    stacked,
                    d,
                );
                let column_key = format!("{}-{column_index}", self.record.id);
                let mut element = div()
                    .min_w(px(0.0))
                    .w_full()
                    .child(self.render_column_markdown_content(
                        &column.markdown,
                        available_width,
                        available_height,
                        theme,
                        &column_key,
                        window,
                        cx,
                    ));
                element.style().align_self = Some(AlignSelf::FlexStart);
                element.style().flex_grow = Some(0.);
                if stacked {
                    element = element.w_full();
                } else {
                    element = element
                        .flex_basis(relative(width_fraction))
                        .w(relative(width_fraction));
                }
                element.into_any_element()
            }))
            .into_any_element()
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
            return div()
                .min_w(px(0.0))
                .flex_shrink_0()
                .text_size(px(inherited_style.font_size))
                .text_color(inherited_style.color)
                .line_height(rems(body_line_height))
                .child(SharedString::from(node.raw_source.clone()))
                .into_any_element();
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
                html_pre_code_language(node).as_deref(),
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

    fn render_html_code_block(
        &self,
        source: &str,
        language: Option<&str>,
        theme: &Theme,
        _for_column: bool,
        node_style: HtmlNodeVisualStyle,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let font_size = t.code_size;
        let line_height = t.text_line_height;
        let base_color = c.code_text;
        let highlight_spans = highlight_code_block(language, source)
            .map(|result| result.spans)
            .unwrap_or_default();

        let mut line_start = 0usize;
        let mut line_elements = Vec::new();
        for line in source.split('\n') {
            let line_end = line_start + line.len();
            line_elements.push(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .children(render_html_code_highlight_chunks(
                        source,
                        line_start..line_end,
                        &highlight_spans,
                        base_color,
                        c,
                        font_size,
                    )),
            );
            line_start = line_end + 1;
        }

        let mut element = div()
            .w_full()
            .min_w(px(0.0))
            .rounded_sm()
            .bg(c.code_bg)
            .px(px(d.code_block_padding_x))
            .py(px(d.code_block_padding_y))
            .text_size(px(font_size))
            .line_height(rems(line_height))
            .flex()
            .flex_col()
            .items_start()
            .children(line_elements);
        if let Some(bg) = node_style.background {
            element = element.bg(bg);
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

    fn render_shell(
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

    fn render_native_table_ui(
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
}

impl Focusable for Block {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The render method builds the full element tree for a block:
/// - Common wrapper: key_context, track_focus, action handlers, mouse events.
/// - Kind-specific styling: headings get size/weight/border, list items get
///   a flex row with marker + content, everything else renders as plain text.
/// - The [`BlockTextElement`] handles text layout, selection, and cursor.
impl Render for Block {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            BlockKind::CodeBlock { language } => {
                let language_label = language
                    .as_ref()
                    .map(|value| SharedString::from(value.to_string()))
                    .unwrap_or_else(|| SharedString::from(strings.code_language_placeholder.clone()));
                let badge_height = d.code_language_input_height
                    + d.code_language_input_padding_y * 2.0
                    + d.code_language_input_border_width * 2.0;
                let icon_size = px((t.code_size - 1.0).max(10.0));
                let collapsible = self.code_block_is_collapsible();
                let collapsed = self.code_block_collapsed(focused);
                let code_line_height = t.code_size * t.text_line_height;
                let collapsed_max_height =
                    px(code_line_height * super::CODE_BLOCK_COLLAPSED_VISIBLE_LINES as f32);
                let mut text_wrapper = div().min_w(px(0.0)).w_full();
                if collapsed {
                    text_wrapper = text_wrapper
                        .max_h(collapsed_max_height)
                        .overflow_hidden();
                }
                let mut code_content = div()
                    .relative()
                    .min_w(px(0.0))
                    .w_full()
                    .child(
                        text_wrapper.child(BlockTextElement::new(cx.entity(), is_placeholder)),
                    );

                if collapsed {
                    code_content = code_content.pb(px(code_line_height));
                    let hidden_lines = self.code_block_hidden_line_count();
                    code_content = code_content.child(
                        div()
                            .id("code-block-expand-bar")
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(code_line_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .bg(c.code_bg.opacity(0.94))
                            .border_t(px(1.0))
                            .border_color(c.code_language_input_border.opacity(0.35))
                            .cursor_pointer()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_code_block_collapse_toggle),
                            )
                            .child(
                                svg()
                                    .path(ICON_CODE_BLOCK_EXPAND)
                                    .size(icon_size)
                                    .text_color(c.code_language_input_text),
                            )
                            .child(
                                div()
                                    .text_size(px((t.code_size - 1.5).max(9.0)))
                                    .text_color(c.code_language_input_text.opacity(0.85))
                                    .child(SharedString::from(format!(
                                        "展开 {hidden_lines} 行"
                                    ))),
                            ),
                    );
                }

                {
                    let block = cx.entity().downgrade();
                    let collapse_icon = if collapsed {
                        ICON_CODE_BLOCK_EXPAND
                    } else {
                        ICON_CODE_BLOCK_COLLAPSE
                    };
                    let mut controls = div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(d.code_language_input_gap));

                    if collapsible {
                        controls = controls.child(
                            div()
                                .id("code-block-collapse")
                                .w(px(badge_height))
                                .h(px(badge_height))
                                .flex()
                                .flex_shrink_0()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.code_language_input_radius))
                                .border(px(d.code_language_input_border_width))
                                .border_color(c.code_language_input_border.opacity(0.65))
                                .bg(c.code_language_input_bg.opacity(0.92))
                                .hover(|this| this.bg(c.code_language_input_border.opacity(0.35)))
                                .active(|this| this.opacity(0.92))
                                .cursor_pointer()
                                .occlude()
                                .child(
                                    svg()
                                        .path(collapse_icon)
                                        .size(icon_size)
                                        .text_color(c.code_language_input_text),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_code_block_collapse_toggle),
                                ),
                        );
                    }

                    let language_badge = div()
                        .id("code-block-language")
                        .h(px(badge_height))
                        .flex()
                        .items_center()
                        .px(px(d.code_language_input_padding_x))
                        .rounded(px(d.code_language_input_radius))
                        .border(px(d.code_language_input_border_width))
                        .border_color(c.code_language_input_border.opacity(0.65))
                        .bg(c.code_language_input_bg.opacity(0.92))
                        .hover(|this| this.bg(c.code_language_input_border.opacity(0.35)))
                        .active(|this| this.opacity(0.92))
                        .text_size(icon_size)
                        .text_color(c.code_language_input_text)
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .occlude()
                        .child(language_label)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_code_language_badge_mouse_down),
                        );

                    code_content = code_content.child(
                        controls
                            .child(language_badge)
                            .child(
                                div()
                                    .id("code-block-copy")
                                    .w(px(badge_height))
                                    .h(px(badge_height))
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.code_language_input_radius))
                                    .border(px(d.code_language_input_border_width))
                                    .border_color(c.code_language_input_border.opacity(0.65))
                                    .bg(c.code_language_input_bg.opacity(0.92))
                                    .hover(|this| this.bg(c.code_language_input_border.opacity(0.35)))
                                    .active(|this| this.opacity(0.92))
                                    .cursor_pointer()
                                    .occlude()
                                    .child(
                                        svg()
                                            .path(ICON_CODE_BLOCK_COPY)
                                            .size(icon_size)
                                            .text_color(c.code_language_input_text),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |_, _window, cx| {
                                            cx.stop_propagation();
                                            let _ = block.update(cx, |block, cx| {
                                                block.on_code_block_copy_click(cx);
                                            });
                                        },
                                    ),
                            ),
                    );
                }

                let run_lane_width = px(badge_height + 6.0);
                let run_icon_top = px(8.0);
                let run_icon_size = px((t.code_size + 3.0).max(14.0));
                let run_snapshot = self.code_run_snapshot.clone();
                let running = run_snapshot.status == CodeRunStatus::Running;

                let code_row = div()
                    .relative()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .flex_none()
                            .flex_shrink_0()
                            .w(run_lane_width)
                            .relative()
                            .bg(c.code_language_input_bg)
                            .border_r(px(1.0))
                            .border_color(c.code_language_input_border.opacity(0.35))
                            .child(
                                div()
                                    .id("code-block-run")
                                    .absolute()
                                    .top(run_icon_top)
                                    .left_0()
                                    .right_0()
                                    .h(px(badge_height))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .opacity(if running { 1.0 } else { 0.72 })
                                    .hover(|this| this.opacity(1.0))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::on_code_block_run_mouse_down),
                                    )
                                    .child(
                                        svg()
                                            .path(ICON_CODE_BLOCK_RUN)
                                            .size(run_icon_size)
                                            .text_color(c.code_language_input_text),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_grow()
                            .min_w(px(0.0))
                            .py(px(d.code_block_padding_y))
                            .pr(px(d.code_block_padding_x))
                            .child(code_content),
                    );

                let mut code_shell = div().w_full().flex().flex_col().child(code_row);
                if run_snapshot.shows_output_panel() {
                    code_shell = code_shell.child(self.render_code_run_output_panel(
                        &theme,
                        &strings,
                        run_lane_width,
                        cx,
                    ));
                }

                focused_base
                    .rounded_sm()
                    .overflow_hidden()
                    .text_size(px(t.code_size))
                    .text_color(c.code_text)
                    .line_height(rems(t.text_line_height))
                    .child(code_shell)
                    .into_any_element()
            }
            BlockKind::Table => {
                let Some(_runtime) = self.table_runtime.clone() else {
                    return focused_base
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
                        .into_any_element();
                };

                let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
                let table_width = effective_table_width(self, viewport_width, d);
                self.render_native_table_ui(block_id, table_width, &theme, window, cx)
            }
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
            BlockKind::MermaidBlock => {
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                }
                let child = if focused {
                    BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
                } else {
                    self.render_mermaid_content(&theme, window)
                };
                focused_base.w_full().child(child).into_any_element()
            }
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

#[cfg(test)]
mod tests {
    use super::{
        ColumnMarkdownSegment, CmarkParser, HtmlComputedStyle, cmark_html,
        html_children_are_plain_text, html_collect_visible_text, html_node_visual_style,
        html_pre_code_language, html_table_column_count,
        html_table_collect_rows, markdown_html_options, parse_columns_markdown,
        split_column_markdown_segments,
    };
    use crate::components::highlight_code_block;
    use crate::components::{Block, BlockKind, BlockRecord, InlineTextTree, parse_html_document};
    use crate::i18n::I18nManager;
    use crate::theme::{Theme, ThemeManager};
    use gpui::{Hsla, Rgba, TestAppContext};

    fn assert_color_near(color: Hsla, red: u8, green: u8, blue: u8, alpha: u8) {
        let color = Rgba::from(color);
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as i16;
        assert!((channel(color.r) - red as i16).abs() <= 1);
        assert!((channel(color.g) - green as i16).abs() <= 1);
        assert!((channel(color.b) - blue as i16).abs() <= 1);
        assert!((channel(color.a) - alpha as i16).abs() <= 1);
    }

    #[test]
    fn html_collect_visible_text_merges_paragraph_inline_nodes() {
        let parser = CmarkParser::new_ext(
            "这是一段很长的分栏说明文字，用来验证自动换行。",
            markdown_html_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let paragraph = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "p")
            .expect("paragraph");
        assert!(html_children_are_plain_text(&paragraph.children));
        assert_eq!(
            html_collect_visible_text(&paragraph.children),
            "这是一段很长的分栏说明文字，用来验证自动换行。"
        );
    }

    #[test]
    fn cmark_code_fence_html_exposes_language_class() {
        let parser = CmarkParser::new_ext(
            "```rust\nfn main() {}\n```",
            markdown_html_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let pre = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "pre")
            .expect("code block pre");
        assert_eq!(html_pre_code_language(pre).as_deref(), Some("rust"));
        let highlight = highlight_code_block(Some("rust"), "fn main() {}");
        assert!(highlight.is_some());
        assert!(!highlight.unwrap().spans.is_empty());
    }

    #[test]
    fn cmark_table_html_parses_into_aligned_row_structure() {
        let parser = CmarkParser::new_ext(
            "| Metric | Value | Change |\n| --- | ---: | ---: |\n| Page Views | 12,000 | +18% |",
            markdown_html_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let table = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "table")
            .expect("table");
        assert_eq!(html_table_column_count(table), 3);
        let rows = html_table_collect_rows(table);
        assert_eq!(rows.len(), 2);
        assert!(rows[0]
            .children
            .iter()
            .any(|child| child.tag_name == "th"));
        assert!(rows[1]
            .children
            .iter()
            .any(|child| child.tag_name == "td"));
    }

    #[test]
    fn cmark_table_html_inserts_whitespace_between_rows() {
        let parser = CmarkParser::new_ext(
            "| A | B |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |",
            markdown_html_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let table = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "table")
            .expect("table");
        let tbody = table
            .children
            .iter()
            .find(|node| node.tag_name == "tbody")
            .expect("tbody");
        let whitespace_nodes = tbody
            .children
            .iter()
            .filter(|child| {
                child.tag_name == "#text" && child.raw_source.chars().all(char::is_whitespace)
            })
            .count();
        assert!(
            whitespace_nodes > 0,
            "expected whitespace nodes between table rows, got {html:?}"
        );
    }

    #[test]
    fn cmark_ordered_list_html_inserts_whitespace_between_items() {
        let parser = CmarkParser::new_ext(
            "1. First\n2. Second\n3. Third",
            markdown_html_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let ol = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "ol")
            .expect("ordered list");
        let whitespace_nodes = ol
            .children
            .iter()
            .filter(|child| {
                child.tag_name == "#text" && child.raw_source.chars().all(char::is_whitespace)
            })
            .count();
        assert!(
            whitespace_nodes > 0,
            "expected whitespace nodes between list items, got {html:?}"
        );
    }

    #[test]
    fn html_render_style_inherits_color_and_font_size() {
        let theme = Theme::default_theme();
        let doc = parse_html_document(
            "<div style=\"color:blue; font-size:20px\"><span style=\"font-size:120%\">x</span></div>",
        );
        let root = HtmlComputedStyle::root(&theme);
        let parent = html_node_visual_style(&doc.nodes[0], root, &theme);
        let child = html_node_visual_style(&doc.nodes[0].children[0], parent.computed, &theme);

        assert_color_near(parent.computed.color, 0, 0, 255, 255);
        assert_color_near(child.computed.color, 0, 0, 255, 255);
        assert!((child.computed.font_size - 24.0).abs() < 0.01);
    }

    #[test]
    fn parses_columns_markdown_for_live_rendering() {
        let columns = parse_columns_markdown(concat!(
            "::: columns\n",
            "--- column width=40%\n",
            "### Left\n\n",
            "- A\n",
            "- B\n\n",
            "--- column width=60%\n",
            "Right text\n",
            ":::"
        ))
        .expect("columns block");

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].width_fraction, Some(0.4));
        assert_eq!(columns[1].width_fraction, Some(0.6));
        assert!(columns[0].markdown.contains("### Left"));
        assert_eq!(columns[1].markdown, "Right text");
    }

    #[test]
    fn split_column_markdown_extracts_mermaid_fence() {
        let segments = split_column_markdown_segments(concat!(
            "### Chart\n\n",
            "```mermaid\n",
            "flowchart LR\n",
            "A --> B\n",
            "```\n\n",
            "Tail text"
        ));

        assert_eq!(segments.len(), 3);
        match &segments[0] {
            ColumnMarkdownSegment::Markdown(text) => assert!(text.contains("### Chart")),
            _ => panic!("expected markdown segment"),
        }
        match &segments[1] {
            ColumnMarkdownSegment::Mermaid(raw) => {
                assert!(raw.contains("```mermaid"));
                assert!(raw.contains("A --> B"));
            }
            _ => panic!("expected mermaid segment"),
        }
        match &segments[2] {
            ColumnMarkdownSegment::Markdown(text) => assert_eq!(text, "Tail text"),
            _ => panic!("expected markdown segment"),
        }
    }

    #[test]
    fn split_column_markdown_extracts_table() {
        let segments = split_column_markdown_segments(concat!(
            "### Metrics\n\n",
            "| Metric | Value | Change |\n",
            "| --- | ---: | ---: |\n",
            "| Page Views | 12,000 | +18% |\n\n",
            "Tail text"
        ));

        assert_eq!(segments.len(), 3);
        match &segments[0] {
            ColumnMarkdownSegment::Markdown(text) => assert!(text.contains("### Metrics")),
            _ => panic!("expected markdown segment"),
        }
        match &segments[1] {
            ColumnMarkdownSegment::Table(table) => {
                assert_eq!(table.header.len(), 3);
                assert_eq!(table.rows.len(), 1);
                assert_eq!(
                    table.header[0].render_cache().visible_text(),
                    "Metric"
                );
            }
            _ => panic!("expected table segment"),
        }
        match &segments[2] {
            ColumnMarkdownSegment::Markdown(text) => assert_eq!(text, "Tail text"),
            _ => panic!("expected markdown segment"),
        }
    }

    #[test]
    fn columns_live_parser_allows_trailing_blank_lines() {
        let columns = parse_columns_markdown(concat!(
            "::: columns\n",
            "--- column\n",
            "Left\n",
            "--- column\n",
            "Right\n",
            ":::\n"
        ))
        .expect("columns block with trailing newline");

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].markdown, "Left");
        assert_eq!(columns[1].markdown, "Right");
    }

    #[test]
    fn columns_live_parser_requires_closing_marker() {
        let columns = parse_columns_markdown("::: columns\n--- column\nLeft");

        assert!(columns.is_none());
    }

    #[test]
    fn html_render_style_overrides_link_and_mark_defaults() {
        let theme = Theme::default_theme();
        let link_doc = parse_html_document("<a style=\"color:red\">x</a>");
        let link_style =
            html_node_visual_style(&link_doc.nodes[0], HtmlComputedStyle::root(&theme), &theme);
        assert_color_near(link_style.computed.color, 255, 0, 0, 255);

        let mark_doc = parse_html_document("<mark style=\"background-color:#123\">x</mark>");
        let mark_style =
            html_node_visual_style(&mark_doc.nodes[0], HtmlComputedStyle::root(&theme), &theme);
        assert_color_near(mark_style.background.unwrap(), 0x11, 0x22, 0x33, 0xff);
    }

    #[test]
    fn html_render_style_does_not_inherit_background_color() {
        let theme = Theme::default_theme();
        let doc =
            parse_html_document("<div style=\"background-color:#112233\"><span>child</span></div>");
        let root = HtmlComputedStyle::root(&theme);
        let parent = html_node_visual_style(&doc.nodes[0], root, &theme);
        let child = html_node_visual_style(&doc.nodes[0].children[0], parent.computed, &theme);

        assert_color_near(parent.background.unwrap(), 0x11, 0x22, 0x33, 0xff);
        assert!(child.background.is_none());
    }

    #[gpui::test]
    async fn focused_code_block_renders_with_language_badge(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init(cx);
            ThemeManager::init(cx);
        });
        let (block, cx) = cx.add_window_view(|_window, cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::CodeBlock {
                        language: Some("rust".into()),
                    },
                    InlineTextTree::plain("fn main() {}\n"),
                ),
            )
        });

        cx.update(|window, cx| {
            block.update(cx, |block, _cx| {
                block.focus_handle.focus(window);
            });
            window.draw(cx).clear();
        });
        cx.run_until_parked();

        block.read_with(cx, |block, _cx| {
            assert!(block.last_bounds.is_some());
            assert_eq!(block.code_language_text(), "rust");
        });
    }
}
