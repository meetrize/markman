//! Columns block parsing and preview rendering.

use gpui::*;

use super::super::Block;
use super::shared::{column_mermaid_available_width, mermaid_available_height};
use super::html_block::html_document_block_gap;
use crate::components::{
    TableData, collect_columns_block_region, collect_table_candidate_region,
    is_closing_fence_marker, is_columns_block_start, is_mermaid_closing_fence,
    is_table_candidate_line, opening_fence_marker, parse_columns_content,
    parse_column_width_fraction, parse_html_document, parse_mermaid_fence_start,
    parse_table_region, serialize_table_markdown_lines, trim_column_markdown_lines,
};
use crate::components::gfm_parser_options;
use crate::theme::Theme;
use pulldown_cmark::{Parser as CmarkParser, html as cmark_html};

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
            if is_closing_fence_marker(line, marker, run_len) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if is_table_candidate_line(line) {
            let trimmed = trim_column_markdown_lines(&current_lines).join("\n");
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

        if let Some(fence) = opening_fence_marker(line) {
            if let Some(mermaid_fence) = parse_mermaid_fence_start(line) {
                let trimmed = trim_column_markdown_lines(&current_lines).join("\n");
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

    let trimmed = trim_column_markdown_lines(&current_lines).join("\n");
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

pub(crate) fn render_markdown_to_html(markdown: &str) -> String {
    let parser = CmarkParser::new_ext(markdown, gfm_parser_options());
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
    let columns = parse_columns_content(&lines[1..end - 1])
        .into_iter()
        .map(|column| RenderColumn {
            width_fraction: column
                .width
                .as_deref()
                .and_then(parse_column_width_fraction),
            markdown: column.markdown,
        })
        .collect::<Vec<_>>();
    (!columns.is_empty()).then_some(columns)
}

impl Block {
    pub(super) fn render_column_markdown_content(
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
                        window.scale_factor(),
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

    pub(super) fn render_columns_markdown(
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
        let c = &theme.colors;
        let d = &theme.dimensions;
        let available_height = mermaid_available_height(viewport_height, d);
        let mut container = div()
            .w_full()
            .min_w(px(0.0))
            .flex_shrink_0()
            .flex()
            .gap(px(d.callout_body_gap.max(16.0)))
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
                    .id(ElementId::Name(format!("column-{column_key}").into()))
                    .min_w(px(0.0))
                    .w_full()
                    .rounded(px(d.callout_radius))
                    .bg(c.callout_note_bg)
                    .border(px(1.0))
                    .border_color(c.table_border.opacity(0.28))
                    .px(px(d.callout_padding_x))
                    .py(px(d.callout_padding_y))
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
                    element = element.w_full().mb(px(d.callout_body_gap.max(12.0)));
                } else {
                    element = element
                        .flex_basis(relative(width_fraction))
                        .w(relative(width_fraction));
                }
                element.into_any_element()
            }))
            .into_any_element()
    }
}
