use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::*;

use super::{Block, InlineFootnoteHit, InlineLinkHit, code_highlight_color};
use crate::components::{HtmlCssColor, InlineTextTree};
use crate::theme::{ThemeColors, ThemeManager};

const SOURCE_LINE_NUMBER_MIN_DIGITS: usize = 2;
const SOURCE_LINE_NUMBER_GAP: f32 = 12.0;
const SOURCE_LINE_NUMBER_DIGIT_WIDTH_RATIO: f32 = 0.62;

const LINK_ACTION_ICON_WIKI: &str = "icon/workspace/markdown.svg";
const LINK_ACTION_ICON_EXTERNAL: &str = "icon/toolbar/link.svg";
/// Preview mode: gap from icon background right edge to visible link text left edge.
const LINK_ACTION_ICON_TEXT_GAP: f32 = 2.0;
const LINK_ACTION_ICON_SIZE_RATIO: f32 = 0.68;
/// Projection mode: gap from icon background right edge to opening delimiter left edge (`[[`, `[`, `<`).
/// Applied in `link_action_icon_layout()` as `bg_right = anchor_x - trailing_gap`.
const LINK_ACTION_ICON_MARKER_GAP: f32 = 3.0;
const LINK_ACTION_ICON_BG_PAD: f32 = 1.5;
/// Inline links: gap from preceding content right edge to icon background left edge.
const LINK_ACTION_ICON_PRECEDING_GAP: f32 = 2.0;
const LINK_ACTION_ICON_BG_RADIUS: f32 = 3.0;
const LINK_ACTION_ICON_VISUAL_INSET: f32 = 1.5;
const LINK_ACTION_ICON_BG_OPACITY: f32 = 0.14;

fn source_line_count(text: &str) -> usize {
    text.split('\n').count().max(1)
}

fn source_line_number_gutter_width(line_count: usize, font_size: Pixels) -> Pixels {
    let digits = line_count
        .max(1)
        .to_string()
        .len()
        .max(SOURCE_LINE_NUMBER_MIN_DIGITS);
    px(digits as f32 * f32::from(font_size) * SOURCE_LINE_NUMBER_DIGIT_WIDTH_RATIO)
        + px(SOURCE_LINE_NUMBER_GAP)
}

fn source_text_bounds(bounds: Bounds<Pixels>, gutter_width: Pixels) -> Bounds<Pixels> {
    if gutter_width <= px(0.0) {
        return bounds;
    }

    let max_gutter = (f32::from(bounds.size.width) - 1.0).max(0.0);
    let gutter_width = px(f32::from(gutter_width).min(max_gutter));
    Bounds::new(
        point(bounds.origin.x + gutter_width, bounds.origin.y),
        size(
            (bounds.size.width - gutter_width).max(px(1.0)),
            bounds.size.height,
        ),
    )
}

fn source_line_number_tops(lines: &[WrappedLine], line_height: Pixels) -> Vec<Pixels> {
    let mut tops = Vec::with_capacity(lines.len());
    let mut y = Pixels::default();
    for line in lines {
        tops.push(y);
        y += wrapped_line_height(line, line_height);
    }
    tops
}

fn push_search_highlight_boundaries(boundaries: &mut Vec<usize>, ranges: &[Range<usize>]) {
    for range in ranges {
        boundaries.push(range.start);
        boundaries.push(range.end);
    }
}

fn segment_overlaps_range(start: usize, end: usize, range: &Range<usize>) -> bool {
    start < range.end && range.start < end
}

fn segment_in_search_highlight(start: usize, end: usize, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|range| segment_overlaps_range(start, end, range))
}

fn build_text_runs(
    input: &Block,
    display_text: &SharedString,
    base_run: &TextRun,
    underline_thickness: Pixels,
    link_color: Hsla,
    code_bg: Hsla,
    search_highlight_bg: Hsla,
    search_highlight_active_bg: Hsla,
    show_inline_code_backgrounds: bool,
) -> Vec<TextRun> {
    let spans = input.inline_spans();
    let mut boundaries = vec![0, display_text.len()];
    for span in spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    if let Some(marked_range) = input.marked_range.as_ref() {
        boundaries.push(marked_range.start);
        boundaries.push(marked_range.end);
    }
    push_search_highlight_boundaries(&mut boundaries, &input.search_highlight_ranges);
    if let Some(active_range) = input.search_highlight_active_range.as_ref() {
        boundaries.push(active_range.start);
        boundaries.push(active_range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let marked_range = input.marked_range.as_ref();
    let search_highlight_ranges = &input.search_highlight_ranges;
    let search_highlight_active_range = input.search_highlight_active_range.as_ref();
    let mut runs = Vec::new();
    let mut span_idx = 0usize;
    for boundary_pair in boundaries.windows(2) {
        let start = boundary_pair[0];
        let end = boundary_pair[1];
        if start >= end {
            continue;
        }

        // Spans are stored in ascending order and boundaries are sorted, so
        // we can advance a single index instead of re-scanning per boundary.
        while span_idx < spans.len() && spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let active_span = spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end);

        let inline_style = active_span.map(|s| s.style).unwrap_or_default();
        let html_style = active_span.and_then(|s| s.html_style);
        let is_link = active_span.map(|s| s.link.is_some()).unwrap_or(false);
        let is_footnote = active_span.map(|s| s.footnote.is_some()).unwrap_or(false);
        let is_marked = marked_range
            .map(|range| start < range.end && range.start < end)
            .unwrap_or(false);
        let is_search_highlight =
            segment_in_search_highlight(start, end, search_highlight_ranges);
        let is_active_search_highlight = search_highlight_active_range
            .is_some_and(|range| segment_overlaps_range(start, end, range));

        let mut font = base_run.font.clone();
        if inline_style.bold && font.weight < FontWeight::BOLD {
            font.weight = FontWeight::BOLD;
        }
        if inline_style.italic {
            font.style = FontStyle::Italic;
        }

        let mut run_color = if is_link || is_footnote {
            link_color
        } else {
            base_run.color
        };
        if let Some(style) = html_style
            && let Some(color) = style.color
        {
            run_color = html_css_color_to_hsla(color, run_color);
        }
        let underline = (inline_style.underline || is_marked || is_link || is_footnote).then_some(
            UnderlineStyle {
                color: Some(run_color),
                thickness: underline_thickness,
                wavy: false,
            },
        );
        let strikethrough = inline_style.strikethrough.then_some(StrikethroughStyle {
            color: Some(run_color),
            thickness: underline_thickness,
        });

        let mut background_color = if show_inline_code_backgrounds && inline_style.code {
            Some(code_bg)
        } else {
            base_run.background_color
        };
        if is_active_search_highlight {
            background_color = Some(search_highlight_active_bg);
        } else if is_search_highlight {
            background_color = Some(search_highlight_bg);
        }
        if let Some(style) = html_style
            && let Some(color) = style.background_color
        {
            background_color = Some(html_css_color_to_hsla(color, run_color));
        }

        runs.push(TextRun {
            len: end - start,
            font,
            color: run_color,
            background_color,
            underline,
            strikethrough,
        });
    }

    if runs.is_empty() {
        vec![base_run.clone()]
    } else {
        runs
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

fn build_code_text_runs(
    input: &Block,
    display_text: &SharedString,
    base_run: &TextRun,
    underline_thickness: Pixels,
    colors: &ThemeColors,
    search_highlight_bg: Hsla,
    search_highlight_active_bg: Hsla,
) -> Vec<TextRun> {
    let highlight_spans = input
        .code_highlight_result()
        .map(|r| r.spans.as_slice())
        .unwrap_or(&[]);
    let mut boundaries = vec![0, display_text.len()];
    for span in highlight_spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    if let Some(marked_range) = input.marked_range.as_ref() {
        boundaries.push(marked_range.start);
        boundaries.push(marked_range.end);
    }
    push_search_highlight_boundaries(&mut boundaries, &input.search_highlight_ranges);
    if let Some(active_range) = input.search_highlight_active_range.as_ref() {
        boundaries.push(active_range.start);
        boundaries.push(active_range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let marked_range = input.marked_range.as_ref();
    let search_highlight_ranges = &input.search_highlight_ranges;
    let search_highlight_active_range = input.search_highlight_active_range.as_ref();
    let mut runs = Vec::new();
    let mut span_idx = 0usize;
    for boundary_pair in boundaries.windows(2) {
        let start = boundary_pair[0];
        let end = boundary_pair[1];
        if start >= end {
            continue;
        }

        let is_marked = marked_range
            .map(|range| start < range.end && range.start < end)
            .unwrap_or(false);
        let is_search_highlight =
            segment_in_search_highlight(start, end, search_highlight_ranges);
        let is_active_search_highlight = search_highlight_active_range
            .is_some_and(|range| segment_overlaps_range(start, end, range));
        while span_idx < highlight_spans.len() && highlight_spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let run_color = highlight_spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end)
            .map(|span| code_highlight_color(colors, span.class))
            .unwrap_or(base_run.color);

        runs.push(TextRun {
            len: end - start,
            font: base_run.font.clone(),
            color: run_color,
            background_color: if is_active_search_highlight {
                Some(search_highlight_active_bg)
            } else if is_search_highlight {
                Some(search_highlight_bg)
            } else {
                base_run.background_color
            },
            underline: is_marked.then_some(UnderlineStyle {
                color: Some(run_color),
                thickness: underline_thickness,
                wavy: false,
            }),
            strikethrough: None,
        });
    }

    if runs.is_empty() {
        vec![base_run.clone()]
    } else {
        runs
    }
}

/// Compute byte ranges of each hard-line (`\n`-separated) segment in the
/// visible text.  Index `i` in the returned Vec corresponds to the `i`-th
/// WrappedLine produced by `shape_text`.
pub(super) fn hard_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (idx, _) in text.match_indices('\n') {
        ranges.push(start..idx);
        start = idx + 1;
    }
    ranges.push(start..text.len());
    ranges
}

/// Map a flat visible-text offset to `(line_index, offset_within_line)`.
pub(super) fn line_index_for_offset(ranges: &[Range<usize>], offset: usize) -> (usize, usize) {
    let clamped = offset.min(ranges.last().map(|r| r.end).unwrap_or(0));
    for (i, range) in ranges.iter().enumerate() {
        if clamped <= range.end {
            return (i, clamped.saturating_sub(range.start));
        }
    }
    let last = ranges.len() - 1;
    (last, ranges[last].len())
}

pub(crate) fn aligned_line_left(
    line: &WrappedLine,
    bounds: Bounds<Pixels>,
    align: TextAlign,
) -> Pixels {
    let slack = (bounds.size.width - line.width()).max(px(0.0));
    match align {
        TextAlign::Left => bounds.left(),
        TextAlign::Center => bounds.left() + slack / 2.0,
        TextAlign::Right => bounds.left() + slack,
    }
}

pub(super) fn wrapped_line_height(line: &WrappedLine, line_height: Pixels) -> Pixels {
    line.size(line_height).height
}

pub(super) fn wrapped_line_top(
    lines: &[WrappedLine],
    line_height: Pixels,
    line_idx: usize,
) -> Pixels {
    lines.iter().take(line_idx).fold(px(0.0), |height, line| {
        height + wrapped_line_height(line, line_height)
    })
}

pub(super) fn wrapped_line_for_y(
    lines: &[WrappedLine],
    line_height: Pixels,
    relative_y: Pixels,
) -> Option<(usize, Pixels)> {
    if lines.is_empty() {
        return None;
    }

    let mut top = px(0.0);
    for (line_idx, line) in lines.iter().enumerate() {
        let height = wrapped_line_height(line, line_height);
        if relative_y < top + height || line_idx + 1 == lines.len() {
            return Some((line_idx, (relative_y - top).max(px(0.0))));
        }
        top += height;
    }

    Some((lines.len() - 1, px(0.0)))
}

fn wrap_boundary_offset(line: &WrappedLine, wrap_idx: usize) -> Option<usize> {
    let boundary = line.wrap_boundaries().get(wrap_idx)?;
    let run = line.unwrapped_layout.runs.get(boundary.run_ix)?;
    let glyph = run.glyphs.get(boundary.glyph_ix)?;
    Some(glyph.index)
}

fn wrapped_row_offsets(line: &WrappedLine) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(line.wrap_boundaries().len() + 2);
    offsets.push(0);
    for wrap_idx in 0..line.wrap_boundaries().len() {
        if let Some(offset) = wrap_boundary_offset(line, wrap_idx) {
            offsets.push(offset.min(line.len()));
        }
    }
    offsets.push(line.len());
    offsets.dedup();
    offsets
}

fn wrapped_row_origin_x(
    line: &WrappedLine,
    bounds: Bounds<Pixels>,
    align: TextAlign,
    row_start: usize,
    row_end: usize,
) -> Pixels {
    let row_width =
        line.unwrapped_layout.x_for_index(row_end) - line.unwrapped_layout.x_for_index(row_start);
    let align_width = line.width();
    let slack = (align_width - row_width).max(px(0.0));
    let line_left = aligned_line_left(line, bounds, align);
    match align {
        TextAlign::Left => line_left,
        TextAlign::Center => line_left + slack / 2.0,
        TextAlign::Right => line_left + slack,
    }
}

pub(super) fn position_for_offset(
    line: &WrappedLine,
    offset: usize,
    line_height: Pixels,
    prefer_next_wrap_start: bool,
) -> Option<Point<Pixels>> {
    let offsets = wrapped_row_offsets(line);
    for row_idx in 0..offsets.len().saturating_sub(1) {
        let row_start = offsets[row_idx];
        let row_end = offsets[row_idx + 1];
        let is_start_of_wrapped_row = prefer_next_wrap_start && row_idx > 0 && offset == row_start;
        let is_end_of_line = offset >= line.len();
        let is_in_row = offset >= row_start && offset < row_end;
        let is_end_of_last_row = is_end_of_line && row_end == line.len() && offset == line.len();
        if is_start_of_wrapped_row || is_in_row || is_end_of_last_row {
            let row_start_x = line.unwrapped_layout.x_for_index(row_start);
            let x_index = if is_end_of_line {
                line.unwrapped_layout.x_for_index(line.len())
            } else {
                line.unwrapped_layout.x_for_index(offset)
            };
            let x = x_index - row_start_x;
            return Some(point(x, line_height * row_idx as f32));
        }
    }

    line.position_for_index(offset.min(line.len()), line_height)
}

pub(super) fn cursor_bounds_for_offset(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    offset: usize,
    align: TextAlign,
    cursor_width: Pixels,
    link_insets: &[LinkIconTextInset],
) -> Option<Bounds<Pixels>> {
    let ranges = hard_line_ranges(text);
    let (line_idx, offset_in_line) = line_index_for_offset(&ranges, offset);
    let layout = lines.get(line_idx)?;
    let origin_x = aligned_line_left(layout, bounds, align);
    let cursor_pos = position_for_offset(layout, offset_in_line, line_height, true)?;
    let x_shift = link_icon_x_shift_for_offset(lines, text, offset, link_insets);
    let y_offset = bounds.top() + wrapped_line_top(lines, line_height, line_idx);
    Some(Bounds::new(
        point(origin_x + cursor_pos.x + x_shift, y_offset + cursor_pos.y),
        size(cursor_width, line_height),
    ))
}

pub(super) fn offset_for_mouse_position(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    align: TextAlign,
    position: Point<Pixels>,
    link_insets: &[LinkIconTextInset],
) -> usize {
    if text.is_empty() || lines.is_empty() {
        return 0;
    }

    if position.y < bounds.top() {
        return 0;
    }
    if position.y > bounds.bottom() {
        return text.len();
    }

    let ranges = hard_line_ranges(text);
    let relative_y = position.y - bounds.top();
    let Some((line_idx, _y_in_line)) = wrapped_line_for_y(lines, line_height, relative_y) else {
        return 0;
    };

    let hard_range = &ranges[line_idx];

    // Match caret geometry from `cursor_bounds_for_offset` on the active hard line.
    let mut best_offset = hard_range.start;
    let mut best_distance = px(f32::MAX);
    let y_tolerance = line_height * 0.6;
    for abs_offset in hard_range.start..=hard_range.end.min(text.len()) {
        let Some(cursor) = cursor_bounds_for_offset(
            lines,
            bounds,
            line_height,
            text,
            abs_offset,
            align,
            px(1.0),
            link_insets,
        ) else {
            continue;
        };
        let dy = (cursor.center().y - position.y).abs();
        if dy > y_tolerance {
            continue;
        }
        let distance = (cursor.center().x - position.x).abs();
        if distance < best_distance
            || (distance == best_distance && abs_offset < best_offset)
        {
            best_distance = distance;
            best_offset = abs_offset;
        }
    }

    best_offset.min(text.len())
}

pub(super) fn range_bounds(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
    align: TextAlign,
    link_insets: &[LinkIconTextInset],
) -> Option<Bounds<Pixels>> {
    let segments =
        range_segment_bounds_with_link_insets(lines, bounds, line_height, text, range.clone(), align, link_insets);
    if segments.is_empty() {
        return cursor_bounds_for_offset(
            lines,
            bounds,
            line_height,
            text,
            range.start,
            align,
            px(1.0),
            link_insets,
        );
    }

    let mut union = segments[0];
    for segment in segments.iter().skip(1) {
        union = Bounds::from_corners(
            point(
                union.left().min(segment.left()),
                union.top().min(segment.top()),
            ),
            point(
                union.right().max(segment.right()),
                union.bottom().max(segment.bottom()),
            ),
        );
    }
    Some(union)
}

/// Horizontal shift applied from `anchor_offset` through the end of its hard line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LinkIconTextInset {
    anchor_offset: usize,
    extra_x: Pixels,
}

pub(super) fn link_icon_x_shift_for_offset(
    lines: &[WrappedLine],
    text: &str,
    offset: usize,
    insets: &[LinkIconTextInset],
) -> Pixels {
    link_icon_x_shift_for_offset_at(lines, text, offset, insets, true)
}

fn link_icon_x_shift_for_offset_at(
    lines: &[WrappedLine],
    text: &str,
    offset: usize,
    insets: &[LinkIconTextInset],
    inclusive: bool,
) -> Pixels {
    if insets.is_empty() {
        return px(0.0);
    }

    let ranges = hard_line_ranges(text);
    let (line_idx, _) = line_index_for_offset(&ranges, offset);
    if lines.get(line_idx).is_none() {
        return px(0.0);
    }

    insets
        .iter()
        .filter(|inset| {
            let (inset_line, _) = line_index_for_offset(&ranges, inset.anchor_offset);
            if inset_line != line_idx {
                return false;
            }
            if inclusive {
                inset.anchor_offset <= offset
            } else {
                inset.anchor_offset < offset
            }
        })
        .map(|inset| inset.extra_x)
        .fold(px(0.0), |total, extra| total + extra)
}

fn range_segment_bounds_for_hard_line(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    line_idx: usize,
    start_offset: usize,
    end_offset: usize,
    align: TextAlign,
    hard_line_start: usize,
    link_insets: &[LinkIconTextInset],
    text: &str,
) -> Vec<Bounds<Pixels>> {
    let Some(line) = lines.get(line_idx) else {
        return Vec::new();
    };
    let ranges = hard_line_ranges(text);
    let line_top = bounds.top() + wrapped_line_top(lines, line_height, line_idx);
    let offsets = wrapped_row_offsets(line);
    let mut segments = Vec::new();

    for row_idx in 0..offsets.len().saturating_sub(1) {
        let row_start = offsets[row_idx];
        let row_end = offsets[row_idx + 1];
        let seg_start = start_offset.max(row_start).min(row_end);
        let seg_end = end_offset.min(row_end).max(row_start);
        if seg_start >= seg_end {
            continue;
        }

        let row_start_x = line.unwrapped_layout.x_for_index(row_start);
        let origin_x = wrapped_row_origin_x(line, bounds, align, row_start, row_end);
        let y = line_top + line_height * row_idx as f32;

        let mut boundaries = vec![seg_start, seg_end];
        for inset in link_insets {
            let (inset_line, inset_local) =
                line_index_for_offset(&ranges, inset.anchor_offset);
            if inset_line != line_idx {
                continue;
            }
            if inset_local <= seg_start || inset_local >= seg_end {
                continue;
            }
            boundaries.push(inset_local);
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        for boundary_pair in boundaries.windows(2) {
            let part_start = boundary_pair[0];
            let part_end = boundary_pair[1];
            if part_start >= part_end {
                continue;
            }
            let start_shift = link_icon_x_shift_for_offset(
                lines,
                text,
                hard_line_start + part_start,
                link_insets,
            );
            let end_shift = link_icon_x_shift_for_offset_at(
                lines,
                text,
                hard_line_start + part_end,
                link_insets,
                false,
            );
            let raw_start = line.unwrapped_layout.x_for_index(part_start) - row_start_x;
            let raw_end = line.unwrapped_layout.x_for_index(part_end) - row_start_x;
            let start_x = px(f32::from(raw_start) + f32::from(start_shift));
            let end_x = px(f32::from(raw_end) + f32::from(end_shift));
            segments.push(Bounds::from_corners(
                point(origin_x + start_x, y),
                point(origin_x + end_x, y + line_height),
            ));
        }
    }

    segments
}

pub(super) fn range_segment_bounds(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
    align: TextAlign,
) -> Vec<Bounds<Pixels>> {
    range_segment_bounds_with_link_insets(lines, bounds, line_height, text, range, align, &[])
}

pub(super) fn range_segment_bounds_with_link_insets(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
    align: TextAlign,
    link_insets: &[LinkIconTextInset],
) -> Vec<Bounds<Pixels>> {
    if range.start >= range.end || lines.is_empty() {
        return Vec::new();
    }

    let ranges = hard_line_ranges(text);
    let (start_line, start_offset) = line_index_for_offset(&ranges, range.start);
    let (end_line, end_offset) = line_index_for_offset(&ranges, range.end);
    let mut segments = Vec::new();

    for line_idx in start_line..=end_line {
        let hard_range = &ranges[line_idx];
        let line_start = if line_idx == start_line {
            start_offset
        } else {
            0
        };
        let line_end = if line_idx == end_line {
            end_offset
        } else {
            hard_range.len()
        };
        segments.extend(range_segment_bounds_for_hard_line(
            lines,
            bounds,
            line_height,
            line_idx,
            line_start,
            line_end,
            align,
            hard_range.start,
            link_insets,
            text,
        ));
    }

    segments
}

fn point_inside_bounds(bounds: Bounds<Pixels>, position: Point<Pixels>) -> bool {
    position.x >= bounds.left()
        && position.x < bounds.right()
        && position.y >= bounds.top()
        && position.y < bounds.bottom()
}

fn link_action_icon_size(line_height: Pixels) -> Pixels {
    px(f32::from(line_height) * LINK_ACTION_ICON_SIZE_RATIO)
        .max(px(10.0))
        .min(px(16.0))
}

fn link_action_icon_slot_width(line_height: Pixels, trailing_gap: Pixels) -> Pixels {
    link_action_icon_chrome_width(line_height) + trailing_gap
}

fn link_action_icon_chrome_width(line_height: Pixels) -> Pixels {
    link_action_icon_size(line_height) + px(LINK_ACTION_ICON_BG_PAD) * 2.0
}

fn link_opening_marker_start(text: &str, span_start: usize, is_wiki: bool) -> Option<usize> {
    if is_wiki {
        if span_start >= 2 && text.get(span_start - 2..span_start) == Some("[[") {
            return Some(span_start - 2);
        }
        return None;
    }
    if span_start >= 2 && text.get(span_start - 2..span_start) == Some("[[") {
        return None;
    }
    if span_start >= 1 {
        match text.as_bytes()[span_start - 1] {
            b'[' | b'<' => Some(span_start - 1),
            _ => None,
        }
    } else {
        None
    }
}

fn wiki_link_opening_marker_start(text: &str, span_start: usize) -> Option<usize> {
    if span_start >= 2 && text.get(span_start - 2..span_start) == Some("[[") {
        Some(span_start - 2)
    } else {
        None
    }
}

/// Display offset used as the layout anchor (left edge of the gap target).
///
/// - Wiki projection (`[[path]]`): anchor at `[[` so `MARKER_GAP` separates icon and brackets.
/// - Wiki preview (`path`): anchor at path text so `TEXT_GAP` separates icon and path.
/// - External projection (`[text](url)`, `<url>`): anchor at `[` / `<`.
/// - External preview (visible URL or label): anchor at link text.
fn link_icon_anchor_offset(input: &Block, span_range: &Range<usize>, link: &InlineLinkHit) -> usize {
    let text = input.display_text();
    if link.is_workspace_file {
        if let Some(marker) = wiki_link_opening_marker_start(text, span_range.start) {
            return marker;
        }
        return span_range.start;
    }

    if let Some(marker) = link_opening_marker_start(text, span_range.start, false) {
        return marker;
    }
    if let Some(anchor) = input.projected_link_icon_anchor_offset(span_range, false) {
        return anchor;
    }
    span_range.start
}

/// Trailing gap paired with [`link_icon_anchor_offset`].
/// When the anchor sits before span text (opening delimiters), use [`LINK_ACTION_ICON_MARKER_GAP`].
fn link_icon_trailing_gap(_link: &InlineLinkHit, anchor_offset: usize, span_start: usize) -> Pixels {
    if anchor_offset < span_start {
        px(LINK_ACTION_ICON_MARKER_GAP)
    } else {
        px(LINK_ACTION_ICON_TEXT_GAP)
    }
}

fn link_icon_layout_bounds(text_bounds: Bounds<Pixels>, link_gutter: Pixels) -> Bounds<Pixels> {
    if link_gutter <= px(0.0) {
        return text_bounds;
    }

    Bounds::new(
        point(text_bounds.left() - link_gutter, text_bounds.top()),
        size((text_bounds.size.width + link_gutter).max(px(1.0)), text_bounds.size.height),
    )
}

fn link_content_gutter_width(
    input: &Block,
    line_height: Pixels,
    source_line_number_gutter_width: Pixels,
) -> Pixels {
    source_line_number_gutter_width + block_link_icon_gutter(input, line_height)
}

fn offset_is_leading_on_hard_line(text: &str, offset: usize) -> bool {
    if offset > text.len() {
        return false;
    }
    let line_start = text[..offset].rfind('\n').map(|index| index + 1).unwrap_or(0);
    text[line_start..offset].chars().all(|ch| ch.is_whitespace())
}

fn block_link_icon_gutter(input: &Block, line_height: Pixels) -> Pixels {
    if input.is_source_raw_mode() {
        return px(0.0);
    }

    let text = input.display_text();
    let reserve_leading_gutter = input.text_align() == TextAlign::Left;
    let mut slot = px(0.0);

    if reserve_leading_gutter && first_hard_line_starts_with_link(input) {
        slot = slot.max(link_action_icon_slot_width(
            line_height,
            block_link_icon_trailing_gap(text),
        ));
    }

    for span in input.inline_spans() {
        let Some(link) = span.link.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }
        let anchor = link_icon_anchor_offset(input, &span.range, link);
        if !reserve_leading_gutter || !offset_is_leading_on_hard_line(text, anchor) {
            continue;
        }
        let gap = link_icon_trailing_gap(link, anchor, span.range.start);
        slot = slot.max(link_action_icon_slot_width(line_height, gap));
    }

    if slot <= px(0.0) && reserve_leading_gutter && first_hard_line_starts_with_link(input) {
        slot = link_action_icon_slot_width(line_height, block_link_icon_trailing_gap(text));
    }

    slot
}

fn block_link_icon_trailing_gap(text: &str) -> Pixels {
    if line_starts_with_wiki_link_marker(text) || line_starts_with_projected_external_link_marker(text)
    {
        return px(LINK_ACTION_ICON_MARKER_GAP);
    }
    px(LINK_ACTION_ICON_TEXT_GAP)
}

fn line_starts_with_wiki_link_marker(text: &str) -> bool {
    text.starts_with("[[")
}

fn line_starts_with_projected_external_link_marker(text: &str) -> bool {
    (text.starts_with('[') && !text.starts_with("[[")) || text.starts_with('<')
}

fn first_hard_line_starts_with_link(input: &Block) -> bool {
    input.first_hard_line_starts_with_link()
}

fn link_action_icon_background_bounds(
    icon_bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Bounds<Pixels> {
    let pad = px(LINK_ACTION_ICON_BG_PAD);
    let mut bg = Bounds::from_corners(
        point(icon_bounds.left() - pad, icon_bounds.top() - pad),
        point(icon_bounds.right() + pad, icon_bounds.bottom() + pad),
    );
    let max_height = line_height - px(2.0);
    if bg.size.height > max_height {
        let trim = (bg.size.height - max_height) / 2.0;
        bg.origin.y += trim;
        bg.size.height = max_height;
    }
    bg
}

fn link_action_icon_svg_bounds(icon_bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let inset = px(LINK_ACTION_ICON_VISUAL_INSET);
    Bounds::from_corners(
        point(icon_bounds.left() + inset, icon_bounds.top() + inset),
        point(icon_bounds.right() - inset, icon_bounds.bottom() - inset),
    )
}

/// Layout for a link-type icon immediately before link text or opening delimiters.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LinkActionIconLayout {
    paint_bounds: Bounds<Pixels>,
    /// When the icon cannot fit in the leading gutter, link text hits start after this width.
    clamped_text_inset: Pixels,
}

fn link_action_icon_layout(
    anchor_x: Pixels,
    vertical_segment: Bounds<Pixels>,
    layout_bounds: Bounds<Pixels>,
    line_height: Pixels,
    trailing_gap: Pixels,
    pin_leading: bool,
    min_icon_left: Pixels,
) -> LinkActionIconLayout {
    let icon_size = link_action_icon_size(line_height);
    let bg_pad = px(LINK_ACTION_ICON_BG_PAD);

    // Spacing is ONLY controlled here: chrome right = anchor_x - trailing_gap.
    let bg_right = anchor_x - trailing_gap;
    let paint_right = bg_right - bg_pad;
    let paint_left = paint_right - icon_size;

    // Line-start links: align icon to the row/content left edge inside the reserved gutter.
    // Inline links: icon background left edge starts at preceding content right + gap.
    // `min_icon_left` is the preceding content right edge (including whitespace before anchor).
    let min_bg_left = min_icon_left + px(LINK_ACTION_ICON_PRECEDING_GAP);
    let min_paint_left = min_bg_left + bg_pad;
    let final_paint_left = if pin_leading {
        // Anchor-relative: keep icon adjacent to link text / delimiters instead of
        // pinning to the reserved gutter's far left (which creates a large visual gap
        // when text is center/right aligned or the anchor sits away from text_bounds.left()).
        paint_left.max(layout_bounds.left())
    } else {
        paint_left.max(min_paint_left)
    };

    let chrome_right = final_paint_left + icon_size + bg_pad;
    let clamped_text_inset = if pin_leading {
        // Leading gutter already reserves horizontal space; never shift text again.
        px(0.0)
    } else if chrome_right + trailing_gap > anchor_x + px(0.5) {
        (chrome_right + trailing_gap).max(anchor_x) - anchor_x
    } else {
        px(0.0)
    };

    let icon_top = vertical_segment.top()
        + px((f32::from(line_height) - f32::from(icon_size)) / 2.0);
    LinkActionIconLayout {
        paint_bounds: Bounds::new(point(final_paint_left, icon_top), size(icon_size, icon_size)),
        clamped_text_inset,
    }
}

fn content_right_before_anchor(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    anchor_offset: usize,
    align: TextAlign,
    link_insets: &[LinkIconTextInset],
) -> Pixels {
    let line_start = text[..anchor_offset]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    if anchor_offset <= line_start {
        return bounds.left();
    }

    range_segment_bounds_with_link_insets(
        lines,
        bounds,
        line_height,
        text,
        line_start..anchor_offset,
        align,
        link_insets,
    )
    .last()
    .map(|segment| segment.right())
    .unwrap_or(bounds.left())
}

pub(super) fn compute_link_icon_text_insets(
    input: &Block,
    lines: &[WrappedLine],
    layout_bounds: Bounds<Pixels>,
    text_bounds: Bounds<Pixels>,
    line_height: Pixels,
    align: TextAlign,
) -> Vec<LinkIconTextInset> {
    if input.is_source_raw_mode() || input.display_text().is_empty() {
        return Vec::new();
    }

    let text = input.display_text();
    let mut link_spans: Vec<_> = input
        .inline_spans()
        .iter()
        .filter(|span| span.link.is_some() && !span.range.is_empty())
        .collect();
    link_spans.sort_by_key(|span| {
        link_icon_anchor_offset(input, &span.range, span.link.as_ref().expect("link span"))
    });

    let mut insets = Vec::new();
    for span in link_spans {
        let link = span.link.as_ref().expect("link span");
        let anchor_offset = link_icon_anchor_offset(input, &span.range, link);
        if offset_is_leading_on_hard_line(text, anchor_offset) {
            continue;
        }
        let Some(layout) = link_action_icon_layout_for_span(
            input,
            lines,
            layout_bounds,
            text_bounds,
            line_height,
            text,
            span.range.clone(),
            link,
            align,
            &insets,
        ) else {
            continue;
        };
        if layout.clamped_text_inset > px(0.5) {
            let duplicate = insets
                .iter()
                .any(|inset| inset.anchor_offset == anchor_offset);
            if !duplicate {
                insets.push(LinkIconTextInset {
                    anchor_offset,
                    extra_x: layout.clamped_text_inset,
                });
            }
        }
    }

    insets
}

fn link_action_icon_layout_for_span(
    input: &Block,
    lines: &[WrappedLine],
    layout_bounds: Bounds<Pixels>,
    text_bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    span_range: Range<usize>,
    link: &InlineLinkHit,
    align: TextAlign,
    link_insets: &[LinkIconTextInset],
) -> Option<LinkActionIconLayout> {
    let segments = range_segment_bounds_with_link_insets(
        lines,
        text_bounds,
        line_height,
        text,
        span_range.clone(),
        align,
        link_insets,
    );
    let vertical_segment = segments.first()?;
    let anchor_offset = link_icon_anchor_offset(input, &span_range, link);
    if anchor_offset >= text.len() {
        return None;
    }
    let anchor_end = if anchor_offset < span_range.start {
        span_range.start.min(text.len())
    } else {
        (anchor_offset + 1).min(text.len())
    };
    if anchor_end <= anchor_offset {
        return None;
    }
    let anchor_segments = range_segment_bounds(
        lines,
        text_bounds,
        line_height,
        text,
        anchor_offset..anchor_end,
        align,
    );
    let anchor_x = anchor_segments.first()?.left();
    let trailing_gap = link_icon_trailing_gap(link, anchor_offset, span_range.start);
    let pin_leading = offset_is_leading_on_hard_line(text, anchor_offset);
    let min_icon_left = if pin_leading {
        layout_bounds.left()
    } else {
        content_right_before_anchor(
            lines,
            text_bounds,
            line_height,
            text,
            anchor_offset,
            align,
            link_insets,
        )
        .max(layout_bounds.left())
    };
    Some(link_action_icon_layout(
        anchor_x,
        *vertical_segment,
        layout_bounds,
        line_height,
        trailing_gap,
        pin_leading,
        min_icon_left,
    ))
}

/// Geometry for painting a link-type icon immediately before link text.
#[derive(Clone, Debug)]
pub(crate) struct LinkActionIconPaint {
    pub svg_bounds: Bounds<Pixels>,
    pub background_bounds: Bounds<Pixels>,
    pub icon_path: SharedString,
    pub color: Hsla,
}

fn link_text_segment_hit_bounds(
    segment: Bounds<Pixels>,
    segment_index: usize,
    icon_layout: Option<LinkActionIconLayout>,
    anchor_x: Pixels,
) -> Bounds<Pixels> {
    if segment_index == 0
        && let Some(layout) = icon_layout
        && layout.clamped_text_inset > px(0.0)
    {
        let reserved_end = anchor_x + layout.clamped_text_inset;
        return Bounds::from_corners(
            point(reserved_end.max(segment.left()), segment.top()),
            point(segment.right(), segment.bottom()),
        );
    }
    segment
}

fn collect_link_action_icons(
    input: &Block,
    lines: &[WrappedLine],
    layout_bounds: Bounds<Pixels>,
    text_bounds: Bounds<Pixels>,
    line_height: Pixels,
    link_color: Hsla,
) -> Vec<LinkActionIconPaint> {
    if input.is_source_raw_mode() || input.display_text().is_empty() {
        return Vec::new();
    }

    let text = input.display_text();
    let align = input.text_align();
    let link_insets = compute_link_icon_text_insets(
        input,
        lines,
        layout_bounds,
        text_bounds,
        line_height,
        align,
    );
    let mut icons = Vec::new();
    for span in input.inline_spans() {
        let Some(link) = span.link.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }
        let Some(layout) = link_action_icon_layout_for_span(
            input,
            lines,
            layout_bounds,
            text_bounds,
            line_height,
            text,
            span.range.clone(),
            link,
            align,
            &link_insets,
        ) else {
            continue;
        };
        let paint_bounds = layout.paint_bounds;
        let icon_path = if link.is_workspace_file {
            LINK_ACTION_ICON_WIKI
        } else {
            LINK_ACTION_ICON_EXTERNAL
        };
        icons.push(LinkActionIconPaint {
            svg_bounds: link_action_icon_svg_bounds(paint_bounds),
            background_bounds: link_action_icon_background_bounds(paint_bounds, line_height),
            icon_path: icon_path.into(),
            color: link_color,
        });
    }
    icons
}

fn paint_wrapped_line_with_link_insets(
    line: &WrappedLine,
    line_idx: usize,
    origin: Point<Pixels>,
    text_bounds: Bounds<Pixels>,
    line_height: Pixels,
    align: TextAlign,
    text: &str,
    link_insets: &[LinkIconTextInset],
    window: &mut Window,
    cx: &mut App,
) {
    let ranges = hard_line_ranges(text);
    let hard_range = ranges.get(line_idx);
    let line_insets: Vec<&LinkIconTextInset> = hard_range
        .map(|hard_range| {
            link_insets
                .iter()
                .filter(|inset| {
                    inset.anchor_offset >= hard_range.start && inset.anchor_offset < hard_range.end
                })
                .collect()
        })
        .unwrap_or_default();

    if line_insets.is_empty() {
        line.paint(origin, line_height, TextAlign::Left, None, window, cx).ok();
        return;
    }

    let row_offsets = wrapped_row_offsets(line);
    for row_idx in 0..row_offsets.len().saturating_sub(1) {
        let row_start = row_offsets[row_idx];
        let row_end = row_offsets[row_idx + 1];
        let row_origin_x = wrapped_row_origin_x(line, text_bounds, align, row_start, row_end);
        let row_top_y = origin.y + line_height * row_idx as f32;

        let mut row_insets: Vec<(usize, Pixels)> = line_insets
            .iter()
            .filter_map(|inset| {
                let (_, inset_local) = line_index_for_offset(&ranges, inset.anchor_offset);
                if inset_local < row_start || inset_local >= row_end {
                    return None;
                }
                Some((inset_local, inset.extra_x))
            })
            .collect();
        row_insets.sort_unstable_by_key(|(offset, _)| *offset);

        let mut cursor = row_start;
        let mut x_shift = line_insets
            .iter()
            .filter_map(|inset| {
                let (_, inset_local) = line_index_for_offset(&ranges, inset.anchor_offset);
                (inset_local < row_start).then_some(inset.extra_x)
            })
            .fold(px(0.0), |total, extra| total + extra);
        for (inset_local, extra) in row_insets {
            if cursor < inset_local {
                paint_wrapped_row_segment(
                    line,
                    origin,
                    row_origin_x,
                    row_top_y,
                    row_start,
                    cursor,
                    inset_local,
                    x_shift,
                    line_height,
                    window,
                    cx,
                );
            }
            x_shift += extra;
            cursor = inset_local;
        }
        if cursor < row_end {
            paint_wrapped_row_segment(
                line,
                origin,
                row_origin_x,
                row_top_y,
                row_start,
                cursor,
                row_end,
                x_shift,
                line_height,
                window,
                cx,
            );
        }
    }
}

fn paint_wrapped_row_segment(
    line: &WrappedLine,
    origin: Point<Pixels>,
    row_origin_x: Pixels,
    row_top_y: Pixels,
    row_start: usize,
    seg_start: usize,
    seg_end: usize,
    x_shift: Pixels,
    line_height: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    if seg_start >= seg_end {
        return;
    }

    let row_start_x = line.unwrapped_layout.x_for_index(row_start);
    let start_x = line.unwrapped_layout.x_for_index(seg_start) - row_start_x;
    let end_x = line.unwrapped_layout.x_for_index(seg_end) - row_start_x;
    let clip = Bounds::from_corners(
        point(row_origin_x + start_x + x_shift, row_top_y),
        point(row_origin_x + end_x + x_shift, row_top_y + line_height),
    );
    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
        line.paint(
            point(row_origin_x + x_shift - row_start_x, origin.y),
            line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        )
        .ok();
    });
}

pub(crate) fn link_action_icon_at_position(
    input: &Block,
    lines: &[WrappedLine],
    text_bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<InlineLinkHit> {
    if input.is_source_raw_mode()
        || input.display_text().is_empty()
        || lines.is_empty()
        || position.y < text_bounds.top()
        || position.y >= text_bounds.bottom()
    {
        return None;
    }

    let text = input.display_text();
    let align = input.text_align();
    let link_gutter = block_link_icon_gutter(input, line_height);
    let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
    let link_insets = compute_link_icon_text_insets(
        input,
        lines,
        layout_bounds,
        text_bounds,
        line_height,
        align,
    );

    for span in input.inline_spans() {
        let Some(link) = span.link.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }
        let Some(icon_layout) = link_action_icon_layout_for_span(
            input,
            lines,
            layout_bounds,
            text_bounds,
            line_height,
            text,
            span.range.clone(),
            link,
            align,
            &link_insets,
        ) else {
            continue;
        };
        if point_inside_bounds(
            link_action_icon_background_bounds(icon_layout.paint_bounds, line_height),
            position,
        ) {
            return Some(link.clone());
        }
    }

    None
}

pub(crate) fn link_text_at_position<'a>(
    input: &'a Block,
    lines: &[WrappedLine],
    text_bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<&'a InlineLinkHit> {
    if link_action_icon_at_position(input, lines, text_bounds, line_height, position).is_some() {
        return None;
    }

    if input.is_source_raw_mode()
        || input.display_text().is_empty()
        || lines.is_empty()
        || position.y < text_bounds.top()
        || position.y >= text_bounds.bottom()
    {
        return None;
    }

    let text = input.display_text();
    let align = input.text_align();
    let link_gutter = block_link_icon_gutter(input, line_height);
    let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
    let link_insets = compute_link_icon_text_insets(
        input,
        lines,
        layout_bounds,
        text_bounds,
        line_height,
        align,
    );

    for span in input.inline_spans() {
        let Some(link) = span.link.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }

        let icon_layout = link_action_icon_layout_for_span(
            input,
            lines,
            layout_bounds,
            text_bounds,
            line_height,
            text,
            span.range.clone(),
            link,
            align,
            &link_insets,
        );
        let anchor_offset = link_icon_anchor_offset(input, &span.range, link);
        let anchor_end = if anchor_offset < span.range.start {
            span.range.start.min(text.len())
        } else {
            (anchor_offset + 1).min(text.len())
        };
        let anchor_x = range_segment_bounds(
            lines,
            text_bounds,
            line_height,
            text,
            anchor_offset..anchor_end.max(anchor_offset + 1),
            align,
        )
        .first()
        .map(|segment| segment.left())
        .unwrap_or(text_bounds.left());

        for (segment_index, segment) in range_segment_bounds_with_link_insets(
            lines,
            text_bounds,
            line_height,
            text,
            span.range.clone(),
            align,
            &link_insets,
        )
        .into_iter()
        .enumerate()
        {
            let hit_bounds = link_text_segment_hit_bounds(
                segment,
                segment_index,
                icon_layout,
                anchor_x,
            );
            if hit_bounds.left() >= hit_bounds.right() {
                continue;
            }
            if point_inside_bounds(hit_bounds, position) {
                return Some(link);
            }
        }
    }

    None
}

pub(crate) fn link_at_position<'a>(
    input: &'a Block,
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<&'a InlineLinkHit> {
    if input.is_source_raw_mode()
        || input.display_text().is_empty()
        || lines.is_empty()
        || position.y < bounds.top()
        || position.y >= bounds.bottom()
    {
        return None;
    }

    let text = input.display_text();
    let align = input.text_align();

    for span in input.inline_spans() {
        let Some(link) = span.link.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }

        for link_bounds in
            range_segment_bounds(lines, bounds, line_height, text, span.range.clone(), align)
        {
            if point_inside_bounds(link_bounds, position) {
                return Some(link);
            }
        }
    }

    None
}

pub(crate) fn footnote_at_position<'a>(
    input: &'a Block,
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    position: Point<Pixels>,
) -> Option<&'a InlineFootnoteHit> {
    if input.is_source_raw_mode()
        || input.display_text().is_empty()
        || lines.is_empty()
        || position.y < bounds.top()
        || position.y >= bounds.bottom()
    {
        return None;
    }

    let text = input.display_text();
    let align = input.text_align();

    for span in input.inline_spans() {
        let Some(footnote) = span.footnote.as_ref() else {
            continue;
        };
        if span.range.is_empty() {
            continue;
        }

        for footnote_bounds in
            range_segment_bounds(lines, bounds, line_height, text, span.range.clone(), align)
        {
            if point_inside_bounds(footnote_bounds, position) {
                return Some(footnote);
            }
        }
    }

    None
}

type BlockTextLayoutState = Rc<RefCell<Option<(SharedString, Pixels, Vec<WrappedLine>)>>>;

fn shape_block_display_lines(
    input: &Block,
    display_text: &SharedString,
    is_placeholder: bool,
    placeholder_text: Option<&SharedString>,
    placeholder_color: Option<Hsla>,
    font_size: Pixels,
    text_wrap_width: Option<Pixels>,
    window: &Window,
    theme: &crate::theme::Theme,
) -> Vec<WrappedLine> {
    let show_inline_code_backgrounds = !input.is_source_raw_mode();
    let style = window.text_style();
    let (display_text, text_color): (SharedString, Hsla) = if is_placeholder {
        (
            placeholder_text
                .cloned()
                .unwrap_or_else(|| theme.placeholders.empty_editing.clone().into()),
            placeholder_color.unwrap_or(theme.colors.text_placeholder),
        )
    } else {
        (display_text.clone(), style.color)
    };

    let run = TextRun {
        len: display_text.len(),
        font: style.font(),
        color: text_color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let search_highlight_bg = theme.colors.selection.opacity(0.35);
    let search_highlight_active_bg = theme.colors.selection.opacity(0.7);

    let runs: Vec<TextRun> = if !is_placeholder {
        if input.kind().is_code_block() {
            build_code_text_runs(
                input,
                &display_text,
                &run,
                px(theme.dimensions.underline_thickness),
                &theme.colors,
                search_highlight_bg,
                search_highlight_active_bg,
            )
        } else {
            build_text_runs(
                input,
                &display_text,
                &run,
                px(theme.dimensions.underline_thickness),
                theme.colors.text_link,
                theme.colors.code_bg,
                search_highlight_bg,
                search_highlight_active_bg,
                show_inline_code_backgrounds,
            )
        }
    } else {
        vec![run]
    };

    window
        .text_system()
        .shape_text(display_text, font_size, &runs, text_wrap_width, None)
        .map(|lines| lines.into_vec())
        .unwrap_or_default()
}

/// Custom low-level [`Element`] that renders a block's inline-formatted
/// text with selection highlights and a blinking cursor.
///
/// Supports multi-line text (used by code blocks) via hard `\n` breaks.
/// Each `\n` produces a separate `WrappedLine` from the text shaper.
pub struct BlockTextElement {
    input: Entity<Block>,
    is_placeholder: bool,
    placeholder_text: Option<SharedString>,
    placeholder_color: Option<Hsla>,
}

impl BlockTextElement {
    pub fn new(input: Entity<Block>, is_placeholder: bool) -> Self {
        Self {
            input,
            is_placeholder,
            placeholder_text: None,
            placeholder_color: None,
        }
    }

    pub fn with_placeholder(
        input: Entity<Block>,
        is_placeholder: bool,
        placeholder_text: SharedString,
        placeholder_color: Option<Hsla>,
    ) -> Self {
        Self {
            input,
            is_placeholder,
            placeholder_text: Some(placeholder_text),
            placeholder_color,
        }
    }
}

/// Prepared text layout and paint geometry for one `BlockTextElement` frame.
pub struct PrepaintState {
    lines: Vec<WrappedLine>,
    source_line_numbers: Vec<ShapedLine>,
    source_line_number_gutter_width: Pixels,
    link_icon_gutter_width: Pixels,
    link_icon_text_insets: Vec<LinkIconTextInset>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    search_highlights: Vec<PaintQuad>,
    search_active_highlights: Vec<PaintQuad>,
    code_backgrounds: Vec<PaintQuad>,
    link_action_icons: Vec<LinkActionIconPaint>,
    line_height: Pixels,
    hitbox: Hitbox,
}

impl IntoElement for BlockTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BlockTextElement {
    type RequestLayoutState = BlockTextLayoutState;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let shared_text = input.shared_display_text();
        let is_placeholder = self.is_placeholder;
        let show_inline_code_backgrounds = !input.is_source_raw_mode();
        let show_line_number_gutter = input.show_line_number_gutter();
        let source_line_count = source_line_count(shared_text.as_ref());
        let style = window.text_style();

        let (display_text, text_color): (SharedString, Hsla) = if is_placeholder {
            (
                self.placeholder_text
                    .clone()
                    .unwrap_or_else(|| theme.placeholders.empty_editing.clone().into()),
                self.placeholder_color
                    .unwrap_or(theme.colors.text_placeholder),
            )
        } else {
            (shared_text, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let search_highlight_bg = theme.colors.selection.opacity(0.35);
        let search_highlight_active_bg = theme.colors.selection.opacity(0.7);

        let runs: Vec<TextRun> = if !is_placeholder {
            if input.kind().is_code_block() {
                build_code_text_runs(
                    input,
                    &display_text,
                    &run,
                    px(theme.dimensions.underline_thickness),
                    &theme.colors,
                    search_highlight_bg,
                    search_highlight_active_bg,
                )
            } else {
                build_text_runs(
                    input,
                    &display_text,
                    &run,
                    px(theme.dimensions.underline_thickness),
                    theme.colors.text_link,
                    theme.colors.code_bg,
                    search_highlight_bg,
                    search_highlight_active_bg,
                    show_inline_code_backgrounds,
                )
            }
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let source_line_number_gutter_width = show_line_number_gutter
            .then(|| source_line_number_gutter_width(source_line_count, font_size))
            .unwrap_or(px(0.0));

        let link_icon_gutter = if !is_placeholder {
            block_link_icon_gutter(input, line_height)
        } else {
            px(0.0)
        };

        let shared_cache = Rc::new(RefCell::new(None));
        let shared_cache_clone = shared_cache.clone();

        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.min_size.width = link_icon_gutter.max(px(0.0)).into();
        layout_style.max_size.width = relative(1.).into();

        let layout_id = window.request_measured_layout(
            layout_style,
            move |known_dimensions, available_space, window, _cx| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(x) => Some(x),
                    AvailableSpace::MinContent => Some(px(1.0)),
                    AvailableSpace::MaxContent => Some(window.viewport_size().width.max(px(1.0))),
                });
                let text_wrap_width = wrap_width.map(|width| {
                    (width - source_line_number_gutter_width - link_icon_gutter).max(px(1.0))
                });

                let lines = window
                    .text_system()
                    .shape_text(
                        display_text.clone(),
                        font_size,
                        &runs,
                        text_wrap_width,
                        None,
                    )
                    .map(|lines| lines.into_vec())
                    .unwrap_or_default();

                let mut total_size: Size<Pixels> = Size::default();
                for line in &lines {
                    let ls = line.size(line_height);
                    total_size.height += ls.height;
                    total_size.width = total_size.width.max(ls.width);
                }
                total_size.width += source_line_number_gutter_width + link_icon_gutter;
                *shared_cache_clone.borrow_mut() =
                    Some((display_text.clone(), link_icon_gutter, lines));
                total_size
            },
        );

        (layout_id, shared_cache)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let show_selection_highlight = input.shows_text_selection_highlight();
        let editor_selection_range = input
            .editor_selection_range
            .as_ref()
            .filter(|range| !range.is_empty())
            .filter(|_| show_selection_highlight)
            .cloned();
        let selected_range = if show_selection_highlight {
            editor_selection_range
                .clone()
                .unwrap_or_else(|| input.selected_range.clone())
        } else {
            input.selected_range.clone()
        };
        let cursor = input.cursor_offset();
        let line_height = window.line_height();
        let focused = input.focus_handle.is_focused(window);
        let show_inline_code_backgrounds = !input.is_source_raw_mode();
        let show_line_number_gutter = input.show_line_number_gutter();
        let show_code_block_gutter = input.show_code_block_line_number_gutter();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());

        let shared_display_text = if self.is_placeholder {
            self.placeholder_text
                .clone()
                .unwrap_or_else(|| theme.placeholders.empty_editing.clone().into())
        } else {
            input.shared_display_text()
        };
        let link_icon_gutter = if self.is_placeholder || input.is_source_raw_mode() {
            px(0.0)
        } else {
            block_link_icon_gutter(input, line_height)
        };

        let lines = match request_layout.borrow_mut().take() {
            Some((cached_text, cached_gutter, lines))
                if cached_text.as_ref() == shared_display_text.as_ref()
                    && cached_gutter == link_icon_gutter =>
            {
                lines
            }
            _ => {
                let source_line_number_gutter_width = show_line_number_gutter
                    .then(|| {
                        source_line_number_gutter_width(
                            source_line_count(shared_display_text.as_ref()),
                            font_size,
                        )
                    })
                    .unwrap_or(px(0.0));
                let text_wrap_width = (bounds.size.width
                    - source_line_number_gutter_width
                    - link_icon_gutter)
                    .max(px(1.0));
                shape_block_display_lines(
                    input,
                    &shared_display_text,
                    self.is_placeholder,
                    self.placeholder_text.as_ref(),
                    self.placeholder_color,
                    font_size,
                    Some(text_wrap_width),
                    window,
                    &theme,
                )
            }
        };

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let source_line_number_gutter_width = show_line_number_gutter
            .then(|| source_line_number_gutter_width(lines.len().max(1), font_size))
            .unwrap_or(px(0.0));
        let content_gutter_width =
            link_content_gutter_width(input, line_height, source_line_number_gutter_width);
        let text_bounds = source_text_bounds(bounds, content_gutter_width);
        let icon_layout_bounds = link_icon_layout_bounds(text_bounds, link_icon_gutter);
        let text_align = input.text_align();
        let text = input.display_text();
        let link_icon_text_insets = if self.is_placeholder || input.is_source_raw_mode() {
            Vec::new()
        } else {
            compute_link_icon_text_insets(
                input,
                &lines,
                icon_layout_bounds,
                text_bounds,
                line_height,
                text_align,
            )
        };
        let source_line_numbers = if show_line_number_gutter {
            let run_color = if show_code_block_gutter {
                theme.colors.code_language_input_text.opacity(0.55)
            } else {
                theme.colors.text_placeholder
            };
            (1..=lines.len().max(1))
                .map(|line_number| {
                    let label = line_number.to_string();
                    window.text_system().shape_line(
                        SharedString::from(label.clone()),
                        font_size,
                        &[TextRun {
                            len: label.len(),
                            font: style.font(),
                            color: run_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        let cursor_opacity = input.cursor_opacity();
        let cursor_color = {
            let mut c = theme.colors.cursor;
            c.a *= cursor_opacity;
            c
        };
        let cursor_width = theme.dimensions.cursor_width;
        let selection_color = theme.colors.selection;

        let (selection_quads, cursor_quad) =
            if (focused || editor_selection_range.is_some()) && !lines.is_empty() {
                if self.is_placeholder {
                    // Placeholder: cursor after the placeholder text
                    let layout = &lines[0];
                    let origin_x = aligned_line_left(layout, text_bounds, text_align);
                    let cursor_pos = layout
                        .position_for_index(0, line_height)
                        .unwrap_or_default();
                    (
                        vec![],
                        Some(fill(
                            Bounds::new(
                                point(origin_x + cursor_pos.x, text_bounds.top() + cursor_pos.y),
                                size(px(cursor_width), line_height),
                            ),
                            cursor_color,
                        )),
                    )
                } else if selected_range.is_empty() || !show_selection_highlight {
                    // No selection overlay (headings) or collapsed caret.
                    (
                        vec![],
                        cursor_bounds_for_offset(
                            &lines,
                            text_bounds,
                            line_height,
                            text,
                            cursor,
                            text_align,
                            px(cursor_width),
                            &link_icon_text_insets,
                        )
                        .map(|bounds| fill(bounds, cursor_color)),
                    )
                } else {
                    let quads = range_segment_bounds_with_link_insets(
                        &lines,
                        text_bounds,
                        line_height,
                        text,
                        selected_range,
                        text_align,
                        &link_icon_text_insets,
                    )
                    .into_iter()
                    .map(|bounds| fill(bounds, selection_color))
                    .collect();
                    (quads, None)
                }
            } else {
                (vec![], None)
            };

        // Compute code-span background quads with rounded corners and padding.
        let mut code_quads = Vec::new();
        let mut search_quads = Vec::new();
        let mut search_active_quads = Vec::new();
        if !self.is_placeholder {
            let search_bg = theme.colors.selection.opacity(0.35);
            let search_active_bg = theme.colors.selection.opacity(0.7);
            if !input.search_highlight_ranges.is_empty() {
                for range in &input.search_highlight_ranges {
                    for segment in range_segment_bounds_with_link_insets(
                        &lines,
                        text_bounds,
                        line_height,
                        text,
                        range.clone(),
                        text_align,
                        &link_icon_text_insets,
                    ) {
                        search_quads.push(fill(segment, search_bg));
                    }
                }
            }
            if let Some(active) = input.search_highlight_active_range.as_ref() {
                for segment in range_segment_bounds_with_link_insets(
                    &lines,
                    text_bounds,
                    line_height,
                    text,
                    active.clone(),
                    text_align,
                    &link_icon_text_insets,
                ) {
                    search_active_quads.push(fill(segment, search_active_bg));
                }
            }
        }
        if show_inline_code_backgrounds && !self.is_placeholder {
            let code_color = theme.colors.code_bg;
            let pad_x = px(theme.dimensions.code_bg_pad_x);
            let pad_y = px(theme.dimensions.code_bg_pad_y);
            let radius = px(theme.dimensions.code_bg_radius);
            for span in input.inline_spans() {
                if !span.style.code || span.range.is_empty() {
                    continue;
                }
                for segment in range_segment_bounds_with_link_insets(
                    &lines,
                    text_bounds,
                    line_height,
                    text,
                    span.range.clone(),
                    text_align,
                    &link_icon_text_insets,
                ) {
                    let quad_bounds = Bounds::from_corners(
                        point(segment.left() - pad_x, segment.top() - pad_y),
                        point(segment.right() + pad_x, segment.bottom() + pad_y),
                    );
                    code_quads.push({
                        let mut q = fill(quad_bounds, code_color);
                        q.corner_radii = Corners::all(radius);
                        q
                    });
                }
            }
        }

        let link_action_icons = if self.is_placeholder || input.is_source_raw_mode() {
            Vec::new()
        } else {
            collect_link_action_icons(
                input,
                &lines,
                icon_layout_bounds,
                text_bounds,
                line_height,
                theme.colors.text_link,
            )
        };

        if !self.is_placeholder && !input.is_source_raw_mode() {
            self.input.update(cx, |input, _cx| {
                input.last_link_icon_text_insets = link_icon_text_insets.clone();
            });
        }

        PrepaintState {
            lines,
            source_line_numbers,
            source_line_number_gutter_width,
            link_icon_gutter_width: link_icon_gutter,
            link_icon_text_insets,
            cursor: cursor_quad,
            selection: selection_quads,
            search_highlights: search_quads,
            search_active_highlights: search_active_quads,
            code_backgrounds: code_quads,
            link_action_icons,
            line_height,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, hovered_link) = {
            let input = self.input.read(cx);
            let text_bounds = source_text_bounds(
                bounds,
                prepaint.source_line_number_gutter_width + prepaint.link_icon_gutter_width,
            );
            let mouse_position = window.mouse_position();
            let hovered_link = !self.is_placeholder
                && !input.is_source_raw_mode()
                && prepaint.hitbox.is_hovered(window)
                && (link_text_at_position(
                    input,
                    &prepaint.lines,
                    text_bounds,
                    prepaint.line_height,
                    mouse_position,
                )
                .is_some()
                    || link_action_icon_at_position(
                        input,
                        &prepaint.lines,
                        text_bounds,
                        prepaint.line_height,
                        mouse_position,
                    )
                    .is_some());
            (input.focus_handle.clone(), hovered_link)
        };

        if hovered_link {
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }

        if focus_handle.is_focused(window) {
            let text_bounds = source_text_bounds(
                bounds,
                prepaint.source_line_number_gutter_width + prepaint.link_icon_gutter_width,
            );
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(text_bounds, self.input.clone()),
                cx,
            );
        }

        let text_bounds = source_text_bounds(
            bounds,
            prepaint.source_line_number_gutter_width + prepaint.link_icon_gutter_width,
        );
        let link_click_bounds =
            link_icon_layout_bounds(text_bounds, prepaint.link_icon_gutter_width);
        let input_entity = self.input.clone();
        let focus_handle_for_click = focus_handle.clone();
        let input_entity_for_wiki = self.input.clone();
        window.on_mouse_event({
            let link_click_bounds = link_click_bounds;
            move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || event.click_count != 1
                    || !link_click_bounds.contains(&event.position)
                {
                    return;
                }
                input_entity_for_wiki.update(cx, |block, cx| {
                    block.try_handle_link_single_click(event.position, window, cx);
                });
            }
        });
        window.on_mouse_event({
            let text_bounds_for_click = text_bounds;
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || event.click_count < 2
                    || !text_bounds_for_click.contains(&event.position)
                {
                    return;
                }
                input_entity.update(cx, |block, cx| {
                    if block.try_select_word_or_line_at_click_count(
                        event.position,
                        event.click_count,
                        window,
                        cx,
                    ) && !focus_handle_for_click.is_focused(window)
                    {
                        block.focus_handle.focus(window);
                        cx.emit(crate::components::BlockEvent::RequestFocus);
                    }
                });
            }
        });

        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        if input.show_code_block_line_number_gutter()
            && prepaint.source_line_number_gutter_width > px(0.0)
        {
            let gutter_bounds = Bounds::new(
                bounds.origin,
                size(prepaint.source_line_number_gutter_width, bounds.size.height),
            );
            window.paint_quad(fill(
                gutter_bounds,
                theme.colors.code_language_input_bg,
            ));
            let code_bounds = Bounds::new(
                point(
                    bounds.origin.x + prepaint.source_line_number_gutter_width,
                    bounds.origin.y,
                ),
                size(
                    (bounds.size.width - prepaint.source_line_number_gutter_width).max(px(0.0)),
                    bounds.size.height,
                ),
            );
            window.paint_quad(fill(code_bounds, theme.colors.code_bg));
        }

        // Paint code backgrounds behind text.
        for code_bg in prepaint.code_backgrounds.drain(..) {
            window.paint_quad(code_bg);
        }

        for search_bg in prepaint.search_highlights.drain(..) {
            window.paint_quad(search_bg);
        }
        for search_bg in prepaint.search_active_highlights.drain(..) {
            window.paint_quad(search_bg);
        }

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        let line_height = prepaint.line_height;
        let lines = std::mem::take(&mut prepaint.lines);
        let text_align = self.input.read(cx).text_align();
        let display_text = self.input.read(cx).display_text().to_string();
        let link_insets = prepaint.link_icon_text_insets.clone();

        for icon in prepaint.link_action_icons.drain(..) {
            let mut background_color = icon.color;
            background_color.a *= LINK_ACTION_ICON_BG_OPACITY;
            let mut background = fill(icon.background_bounds, background_color);
            background.corner_radii = Corners::all(px(LINK_ACTION_ICON_BG_RADIUS));
            window.paint_quad(background);
            let _ = window.paint_svg(
                icon.svg_bounds,
                icon.icon_path,
                TransformationMatrix::default(),
                icon.color,
                cx,
            );
        }

        let line_number_tops = source_line_number_tops(&lines, line_height);
        let line_number_gap = px(SOURCE_LINE_NUMBER_GAP);
        let line_numbers = std::mem::take(&mut prepaint.source_line_numbers);
        for (line_number, y_offset) in line_numbers.iter().zip(line_number_tops.iter()) {
            let line_number_width = line_number.x_for_index(line_number.len());
            line_number
                .paint(
                    point(
                        text_bounds.left() - line_number_gap - line_number_width,
                        bounds.origin.y + *y_offset,
                    ),
                    line_height,
                    window,
                    cx,
                )
                .ok();
        }

        let mut y_offset = Pixels::default();
        for (line_idx, line) in lines.iter().enumerate() {
            let origin_x = aligned_line_left(line, text_bounds, text_align);
            let origin = point(origin_x, text_bounds.origin.y + y_offset);
            if link_insets.is_empty() {
                line.paint(origin, line_height, TextAlign::Left, None, window, cx)
                    .ok();
            } else {
                paint_wrapped_line_with_link_insets(
                    line,
                    line_idx,
                    origin,
                    text_bounds,
                    line_height,
                    text_align,
                    &display_text,
                    &link_insets,
                    window,
                    cx,
                );
            }
            y_offset += wrapped_line_height(line, line_height);
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(lines);
            input.last_bounds = Some(text_bounds);
            input.interaction_bounds = Some(text_bounds);
            input.last_line_height = line_height;
            input.last_link_icon_text_insets = prepaint.link_icon_text_insets.clone();
        });
    }
}

fn build_inline_tree_preview_runs(
    tree: &InlineTextTree,
    display_text: &str,
    base_run: &TextRun,
    underline_thickness: Pixels,
    link_color: Hsla,
    code_bg: Hsla,
) -> Vec<TextRun> {
    let cache = tree.render_cache();
    let spans = cache.spans();
    let mut boundaries = vec![0, display_text.len()];
    for span in spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut runs = Vec::new();
    let mut span_idx = 0usize;
    for boundary_pair in boundaries.windows(2) {
        let start = boundary_pair[0];
        let end = boundary_pair[1];
        if start >= end {
            continue;
        }

        while span_idx < spans.len() && spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let active_span = spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end);

        let inline_style = active_span.map(|span| span.style).unwrap_or_default();
        let html_style = active_span.and_then(|span| span.html_style);
        let is_link = active_span
            .map(|span| span.link.is_some())
            .unwrap_or(false);
        let is_footnote = active_span
            .map(|span| span.footnote.is_some())
            .unwrap_or(false);

        let mut font = base_run.font.clone();
        if inline_style.bold && font.weight < FontWeight::BOLD {
            font.weight = FontWeight::BOLD;
        }
        if inline_style.italic {
            font.style = FontStyle::Italic;
        }

        let mut run_color = if is_link || is_footnote {
            link_color
        } else {
            base_run.color
        };
        if let Some(style) = html_style
            && let Some(color) = style.color
        {
            run_color = html_css_color_to_hsla(color, run_color);
        }
        let underline = (inline_style.underline || is_link || is_footnote).then_some(UnderlineStyle {
            color: Some(run_color),
            thickness: underline_thickness,
            wavy: false,
        });
        let strikethrough = inline_style.strikethrough.then_some(StrikethroughStyle {
            color: Some(run_color),
            thickness: underline_thickness,
        });

        let mut background_color = if inline_style.code {
            Some(code_bg)
        } else {
            base_run.background_color
        };
        if let Some(style) = html_style
            && let Some(color) = style.background_color
        {
            background_color = Some(html_css_color_to_hsla(color, run_color));
        }

        runs.push(TextRun {
            len: end - start,
            font,
            color: run_color,
            background_color,
            underline,
            strikethrough,
        });
    }

    if runs.is_empty() {
        vec![base_run.clone()]
    } else {
        runs
    }
}

/// Read-only shaped text preview for inline trees inside narrow containers
/// such as column table cells.
pub(crate) struct InlineTreePreviewTextElement {
    tree: InlineTextTree,
    text_align: TextAlign,
    font_weight: FontWeight,
    text_color: Hsla,
    font_size: f32,
    line_height_rems: f32,
}

impl InlineTreePreviewTextElement {
    pub fn new(
        tree: InlineTextTree,
        text_align: TextAlign,
        font_weight: FontWeight,
        text_color: Hsla,
        font_size: f32,
        line_height_rems: f32,
    ) -> Self {
        Self {
            tree,
            text_align,
            font_weight,
            text_color,
            font_size,
            line_height_rems,
        }
    }
}


impl IntoElement for InlineTreePreviewTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InlineTreePreviewTextElement {
    type RequestLayoutState = Rc<RefCell<Option<Vec<WrappedLine>>>>;
    type PrepaintState = (Vec<WrappedLine>, Pixels);

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let theme = cx.global::<ThemeManager>().current_arc();
        let cache = self.tree.render_cache();
        let display_text = SharedString::from(cache.visible_text().to_string());
        let style = window.text_style();
        let mut font = style.font();
        if self.font_weight > font.weight {
            font.weight = self.font_weight;
        }
        let font_size = px(self.font_size);
        let line_height = px(self.font_size * self.line_height_rems);
        let base_run = TextRun {
            len: display_text.len(),
            font,
            color: self.text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = build_inline_tree_preview_runs(
            &self.tree,
            display_text.as_ref(),
            &base_run,
            px(theme.dimensions.underline_thickness),
            theme.colors.text_link,
            theme.colors.code_bg,
        );

        let shared_lines = Rc::new(RefCell::new(None));
        let shared_lines_clone = shared_lines.clone();
        let display_text_for_layout = display_text.clone();

        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.min_size.width = px(0.0).into();
        layout_style.max_size.width = relative(1.).into();

        let layout_id = window.request_measured_layout(
            layout_style,
            move |known_dimensions, available_space, window, _cx| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(x) => Some(x),
                    AvailableSpace::MinContent => Some(px(1.0)),
                    AvailableSpace::MaxContent => Some(window.viewport_size().width.max(px(1.0))),
                });
                let text_wrap_width = wrap_width.map(|width| width.max(px(1.0)));

                match window.text_system().shape_text(
                    display_text_for_layout.clone(),
                    font_size,
                    &runs,
                    text_wrap_width,
                    None,
                ) {
                    Ok(lines) => {
                        let mut total_size: Size<Pixels> = Size::default();
                        for line in &lines {
                            let line_size = line.size(line_height);
                            total_size.height += line_size.height;
                            total_size.width = total_size.width.max(line_size.width);
                        }
                        *shared_lines_clone.borrow_mut() = Some(lines.into_vec());
                        total_size
                    }
                    Err(_) => Size::default(),
                }
            },
        );

        (layout_id, shared_lines)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let line_height = px(self.font_size * self.line_height_rems);
        let lines = request_layout.borrow_mut().take().unwrap_or_default();
        (lines, line_height)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (lines, line_height) = prepaint;
        let mut y_offset = Pixels::default();
        for line in lines {
            let origin_x = aligned_line_left(line, bounds, self.text_align);
            line.paint(
                point(origin_x, bounds.origin.y + y_offset),
                *line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
            y_offset += wrapped_line_height(line, *line_height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        block_link_icon_gutter, compute_link_icon_text_insets, cursor_bounds_for_offset,
        first_hard_line_starts_with_link, link_action_icon_at_position,
        link_action_icon_background_bounds, link_action_icon_layout_for_span,
        link_action_icon_slot_width, link_at_position, link_icon_anchor_offset,
        link_icon_layout_bounds, link_text_at_position, offset_for_mouse_position,
        range_segment_bounds, range_segment_bounds_with_link_insets,
        source_line_number_gutter_width, source_line_number_tops, source_text_bounds,
        wrapped_line_height, LINK_ACTION_ICON_BG_PAD, LINK_ACTION_ICON_MARKER_GAP,
        LINK_ACTION_ICON_PRECEDING_GAP, LINK_ACTION_ICON_TEXT_GAP,
    };
    use crate::components::{Block, BlockKind, BlockRecord, InlineTextTree, TableCellPosition};
    use gpui::{
        AppContext, Bounds, Hsla, SharedString, TestAppContext, TextAlign, TextRun,
        VisualTestContext, font, point, px, rgba, size,
    };

    fn shaped_lines(
        text: &str,
        width: gpui::Pixels,
        cx: &mut VisualTestContext,
    ) -> Vec<gpui::WrappedLine> {
        cx.update(|window, _app| {
            window
                .text_system()
                .shape_text(
                    text.to_string().into(),
                    px(16.0),
                    &[TextRun {
                        len: text.len(),
                        font: font(".SystemUIFont"),
                        color: Hsla::from(rgba(0xffffffff)),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    Some(width),
                    None,
                )
                .expect("text should shape")
                .into_vec()
        })
    }

    #[test]
    fn source_line_number_gutter_grows_with_digit_count() {
        let one_digit = source_line_number_gutter_width(9, px(16.0));
        let two_digits = source_line_number_gutter_width(10, px(16.0));
        let three_digits = source_line_number_gutter_width(100, px(16.0));

        assert_eq!(one_digit, two_digits);
        assert!(three_digits > two_digits);
    }

    #[test]
    fn source_text_bounds_are_offset_by_gutter_width() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(300.0), px(120.0)));
        let text_bounds = source_text_bounds(bounds, px(48.0));

        assert_eq!(text_bounds.left(), px(58.0));
        assert_eq!(text_bounds.top(), px(20.0));
        assert_eq!(text_bounds.size.width, px(252.0));
        assert_eq!(text_bounds.size.height, px(120.0));
    }

    #[gpui::test]
    async fn source_line_number_tops_follow_soft_wrapped_hard_lines(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let lines = shaped_lines(
            "this line should wrap before the next hard line\nsecond",
            px(92.0),
            cx,
        );
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "first hard line should soft-wrap"
        );

        let tops = source_line_number_tops(&lines, px(20.0));
        assert_eq!(tops[0], px(0.0));
        assert_eq!(tops[1], wrapped_line_height(&lines[0], px(20.0)));
    }

    #[gpui::test]
    async fn link_hit_matches_only_rendered_link_text(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link](https://example.com)"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let (hit, miss_right) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            let hit = point((start.x + end.x) / 2.0, px(10.0));
            let miss_right = point(end.x + px(24.0), px(10.0));
            (
                link_at_position(block, &lines, bounds, px(20.0), hit)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), miss_right)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(hit, Some("https://example.com".to_string()));
        assert_eq!(miss_right, None);
    }

    #[gpui::test]
    async fn link_action_icon_hit_is_separate_from_link_text(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("See [[docs/readme.md]] here."),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let line_height = px(20.0);
        let (icon_hit, text_hit, icon_excludes_text, icon_after_preceding, icon_before_link) =
            block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("wiki link span should exist");
            let link = span.link.as_ref().expect("wiki link");
            let link_gutter = block_link_icon_gutter(block, line_height);
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
            let insets = compute_link_icon_text_insets(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                block.text_align(),
            );
            let icon_layout = link_action_icon_layout_for_span(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                &display_text,
                span.range.clone(),
                link,
                block.text_align(),
                &insets,
            )
            .expect("inline wiki icon layout should exist");
            let icon_bg = link_action_icon_background_bounds(icon_layout.paint_bounds, line_height);
            let icon_position = icon_bg.center();
            let text_segments = range_segment_bounds_with_link_insets(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                span.range.clone(),
                block.text_align(),
                &insets,
            );
            let text_segment = text_segments.first().expect("link text segment");
            let text_position = point(
                (text_segment.left() + text_segment.right()) / 2.0,
                px(10.0),
            );
            let anchor = link_icon_anchor_offset(block, &span.range, link);
            let preceding_right = range_segment_bounds(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                {
                    let line_start = display_text[..anchor]
                        .rfind('\n')
                        .map(|index| index + 1)
                        .unwrap_or(0);
                    line_start..anchor
                },
                block.text_align(),
            )
            .last()
            .map(|segment| segment.right())
            .unwrap_or(text_bounds.left());
            let trailing_gap = if anchor < span.range.start {
                px(LINK_ACTION_ICON_MARKER_GAP)
            } else {
                px(LINK_ACTION_ICON_TEXT_GAP)
            };
            let icon_before_link =
                icon_bg.right() + trailing_gap <= text_segment.left() + px(0.5);
            (
                link_action_icon_at_position(block, &lines, text_bounds, line_height, icon_position)
                    .map(|link| link.open_target.clone()),
                link_text_at_position(block, &lines, text_bounds, line_height, text_position)
                    .map(|link| link.open_target.clone()),
                link_text_at_position(block, &lines, text_bounds, line_height, icon_position).is_none(),
                icon_bg.left() + px(0.5)
                    >= preceding_right + px(LINK_ACTION_ICON_PRECEDING_GAP),
                icon_before_link,
            )
        });

        assert_eq!(icon_hit, Some("docs/readme.md".to_string()));
        assert_eq!(text_hit, Some("docs/readme.md".to_string()));
        assert!(icon_excludes_text);
        assert!(icon_after_preceding, "icon should not overlap preceding text");
        assert!(icon_before_link, "icon should not overlap link text");
    }

    #[gpui::test]
    async fn first_hard_line_link_requests_leading_icon_gutter(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let leading = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[[docs/readme.md]]"),
                ),
            )
        });
        let inline = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("See [[docs/readme.md]]"),
                ),
            )
        });
        leading.read_with(cx, |block, _cx| {
            assert!(first_hard_line_starts_with_link(block));
        });
        inline.read_with(cx, |block, _cx| {
            assert!(!first_hard_line_starts_with_link(block));
        });
    }

    #[test]
    fn link_icon_layout_bounds_includes_leading_gutter_slot() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
        let gutter = px(20.0);
        let text_bounds = source_text_bounds(bounds, gutter);
        let icon_click = point(text_bounds.left() - gutter / 2.0, px(10.0));
        assert!(!text_bounds.contains(&icon_click));
        assert!(link_icon_layout_bounds(text_bounds, gutter).contains(&icon_click));
    }

    #[gpui::test]
    async fn link_action_icon_stays_inside_text_bounds_at_line_start(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[[docs/readme.md]]"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let line_height = px(20.0);
        block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let link_gutter = block_link_icon_gutter(block, line_height);
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let gutter = link_action_icon_slot_width(line_height, px(LINK_ACTION_ICON_TEXT_GAP));
            let icon_position = point(text_bounds.left() - gutter / 2.0, px(10.0));
            let icon_hit = link_action_icon_at_position(
                block,
                &lines,
                text_bounds,
                line_height,
                icon_position,
            );
            assert!(icon_hit.is_some());
            assert!(
                link_text_at_position(block, &lines, text_bounds, line_height, icon_position)
                    .is_none()
            );
        });
    }

    #[gpui::test]
    async fn wiki_projected_icon_gaps_from_opening_brackets_not_path_text(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[[docs/readme.md]]"),
                ),
            )
        });

        block.update(cx, |block, _cx| {
            block.selected_range = 2..2;
            block.sync_inline_projection_for_focus(true);
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        assert!(
            display_text.starts_with("[["),
            "wiki link should expand to projected syntax"
        );

        let line_height = px(20.0);
        let link_gutter = block.read_with(cx, |block, _cx| {
            block_link_icon_gutter(block, line_height)
        });
        let wrap_width = px(320.0) - link_gutter;
        let lines = shaped_lines(&display_text, wrap_width, cx);

        block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("wiki span should exist");
            let link = span.link.as_ref().expect("wiki link hit");
            let layout = link_action_icon_layout_for_span(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                &display_text,
                span.range.clone(),
                link,
                block.text_align(),
                &[],
            )
            .expect("wiki icon layout should exist");
            let icon_bg = link_action_icon_background_bounds(layout.paint_bounds, line_height);
            assert!(
                link_gutter > px(0.0),
                "line-start wiki should reserve leading icon gutter"
            );
            assert!(
                layout.paint_bounds.left() >= layout_bounds.left() - px(0.5)
                    && layout.paint_bounds.left()
                        <= layout_bounds.left() + px(LINK_ACTION_ICON_BG_PAD) + px(0.5),
                "line-start icon should sit at anchor gap inside reserved gutter"
            );
            assert!(
                icon_bg.left() >= bounds.left() - px(LINK_ACTION_ICON_BG_PAD) - px(0.5),
                "icon must stay inside block bounds at left edge"
            );
            assert!(text_bounds.left() >= bounds.left() + link_gutter - px(0.5));
            let marker_left = range_segment_bounds(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                0..2,
                block.text_align(),
            )
            .first()
            .expect("opening [[ segment")
            .left();

            assert!(
                icon_bg.right() + px(LINK_ACTION_ICON_MARKER_GAP) <= marker_left + px(0.5),
                "icon background right ({:?}) + MARKER_GAP should not overlap [[ left ({:?})",
                icon_bg.right(),
                marker_left,
            );
        });
    }

    #[gpui::test]
    async fn link_hit_respects_center_alignment(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            let mut block = Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link](https://example.com)"),
                ),
            );
            block.set_table_cell_mode(
                TableCellPosition { row: 0, column: 0 },
                crate::components::TableColumnAlignment::Center,
                (1, 1),
            );
            block
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(240.0), cx);
        let (miss_left, hit_center) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(240.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let origin_x = super::aligned_line_left(layout, bounds, block.text_align());
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            let miss_left = point(origin_x - px(12.0), px(10.0));
            let hit_center = point(origin_x + (start.x + end.x) / 2.0, px(10.0));
            (
                link_at_position(block, &lines, bounds, px(20.0), miss_left)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), hit_center)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(miss_left, None);
        assert_eq!(hit_center, Some("https://example.com".to_string()));
    }

    #[gpui::test]
    async fn text_runs_apply_inline_html_color_and_background(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(
                        "before <span style='color:blue;background-color:#ff0'>marked</span>",
                    ),
                ),
            )
        });

        block.read_with(cx, |block, _cx| {
            let display_text: SharedString = block.display_text().to_string().into();
            let base_run = TextRun {
                len: display_text.len(),
                font: font(".SystemUIFont"),
                color: Hsla::from(rgba(0xffffffff)),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = super::build_text_runs(
                block,
                &display_text,
                &base_run,
                px(1.0),
                Hsla::from(rgba(0x0066ccff)),
                Hsla::from(rgba(0x111111ff)),
                Hsla::from(rgba(0xffff0033)),
                Hsla::from(rgba(0xffff0066)),
                true,
            );
            let marked_run = runs.last().expect("styled text should create a final run");

            assert_eq!(block.display_text(), "before marked");
            assert_eq!(marked_run.len, "marked".len());
            assert_eq!(marked_run.color, Hsla::from(rgba(0x0000ffff)));
            assert_eq!(
                marked_run.background_color,
                Some(Hsla::from(rgba(0xffff00ff)))
            );
        });
    }

    #[gpui::test]
    async fn soft_wrapped_range_segments_stay_within_wrap_width(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let lines = shaped_lines(text, px(80.0), cx);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "test text should soft-wrap"
        );

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(80.0), px(120.0)));
        let segments = super::range_segment_bounds(
            &lines,
            bounds,
            px(20.0),
            text,
            0..text.len(),
            TextAlign::Left,
        );

        assert!(segments.len() > 1);
        for segment in segments {
            assert!(segment.left() >= bounds.left());
            assert!(segment.right() <= bounds.right() + px(0.5));
        }
    }

    #[gpui::test]
    async fn wrapped_link_hit_matches_only_visible_segments(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let label = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(&format!("[{label}](https://example.com)")),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(80.0), cx);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "link text should soft-wrap"
        );

        let (hit, miss_right) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(80.0), px(120.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let segments = super::range_segment_bounds(
                &lines,
                bounds,
                px(20.0),
                &display_text,
                span.range.clone(),
                block.text_align(),
            );
            assert!(segments.len() > 1);
            let second_segment = segments[1];
            let hit = point(
                (second_segment.left() + second_segment.right()) / 2.0,
                (second_segment.top() + second_segment.bottom()) / 2.0,
            );
            let miss_right = point(second_segment.right() + px(24.0), hit.y);
            (
                link_at_position(block, &lines, bounds, px(20.0), hit)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), miss_right)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(hit, Some("https://example.com".to_string()));
        assert_eq!(miss_right, None);
    }

    #[gpui::test]
    async fn wrapped_hard_line_top_accumulates_soft_wrap_height(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz\nnext";
        let lines = shaped_lines(text, px(80.0), cx);
        assert_eq!(lines.len(), 2);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "first hard line should soft-wrap"
        );

        let first_height = lines[0].size(px(20.0)).height;
        assert!(first_height > px(20.0));
        assert_eq!(super::wrapped_line_top(&lines, px(20.0), 1), first_height);
    }

    #[gpui::test]
    async fn line_start_external_link_icon_sits_close_to_link_text(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link text](https://example.com)"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        assert_eq!(display_text, "link text");
        let line_height = px(20.0);
        let link_gutter = block.read_with(cx, |block, _cx| {
            block_link_icon_gutter(block, line_height)
        });
        assert!(link_gutter > px(0.0));
        let wrap_width = px(320.0) - link_gutter;
        let lines = shaped_lines(&display_text, wrap_width, cx);

        block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("external link span");
            let link = span.link.as_ref().expect("link hit");
            let insets = compute_link_icon_text_insets(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                block.text_align(),
            );
            assert!(insets.is_empty(), "line-start links should not shift text");
            let layout = link_action_icon_layout_for_span(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                &display_text,
                span.range.clone(),
                link,
                block.text_align(),
                &insets,
            )
            .expect("external link icon layout");
            assert_eq!(
                layout.clamped_text_inset,
                px(0.0),
                "leading gutter should avoid text inset"
            );
            let icon_bg = link_action_icon_background_bounds(layout.paint_bounds, line_height);
            let text_segments = range_segment_bounds(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                span.range.clone(),
                block.text_align(),
            );
            let text_segment = text_segments.first().expect("link text segment");
            let gap = text_segment.left() - icon_bg.right();
            assert!(
                gap <= px(LINK_ACTION_ICON_TEXT_GAP) + px(0.5),
                "icon-to-text gap ({gap:?}) should be at most TEXT_GAP"
            );
        });
    }

    #[gpui::test]
    async fn offset_for_mouse_matches_cursor_bounds_with_link_insets(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("See [link text](https://example.com) here."),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let line_height = px(20.0);
        let link_gutter = block.read_with(cx, |block, _cx| {
            block_link_icon_gutter(block, line_height)
        });
        let wrap_width = px(320.0) - link_gutter;
        let lines = shaped_lines(&display_text, wrap_width, cx);

        block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
            let insets = compute_link_icon_text_insets(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                block.text_align(),
            );
            assert!(
                !insets.is_empty(),
                "inline external link should reserve text inset"
            );
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("external link span");
            let mid_offset = (span.range.start + span.range.end) / 2;
            let cursor = cursor_bounds_for_offset(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                mid_offset,
                block.text_align(),
                px(1.0),
                &insets,
            )
            .expect("cursor bounds");
            let mouse_offset = offset_for_mouse_position(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                block.text_align(),
                cursor.center(),
                &insets,
            );
            assert_eq!(
                mouse_offset, mid_offset,
                "drag hit-testing should match painted cursor position"
            );
        });
    }

    #[gpui::test]
    async fn projected_line_start_external_link_cursor_reaches_line_end(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let markdown = "[link text](https://example.com)";
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(markdown),
                ),
            )
        });

        block.update(cx, |block, cx| {
            block.selected_range = 2..2;
            block.sync_inline_projection_for_focus(true);
            block.move_to(block.visible_len(), cx);
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let line_height = px(20.0);
        let link_gutter = block.read_with(cx, |block, _cx| {
            block_link_icon_gutter(block, line_height)
        });
        let wrap_width = px(320.0) - link_gutter;
        let lines = shaped_lines(&display_text, wrap_width, cx);
        block.read_with(cx, |block, _cx| {
            let visible_len = block.visible_len();
            let cursor_offset = block.cursor_offset();
            assert_eq!(display_text, markdown);
            assert_eq!(visible_len, markdown.len());
            assert_eq!(
                cursor_offset, visible_len,
                "End should place caret after closing paren"
            );

            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
            let insets = compute_link_icon_text_insets(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                block.text_align(),
            );
            let end_cursor = cursor_bounds_for_offset(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                cursor_offset,
                block.text_align(),
                px(1.0),
                &insets,
            )
            .expect("end cursor bounds");
            let text_end_segments = range_segment_bounds_with_link_insets(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                visible_len.saturating_sub(1)..visible_len,
                block.text_align(),
                &insets,
            );
            let text_end = text_end_segments.last().expect("last char segment");
            assert!(
                (end_cursor.left() - text_end.right()).abs() <= px(1.0),
                "cursor ({:?}) should align with text end ({:?})",
                end_cursor.left(),
                text_end.right(),
            );
        });
    }

    #[gpui::test]
    async fn projected_inline_external_link_cursor_and_mouse_match_at_line_end(
        cx: &mut TestAppContext,
    ) {
        let cx = cx.add_empty_window();
        let markdown = "See [link text](https://example.com) here.";
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(markdown),
                ),
            )
        });

        block.update(cx, |block, cx| {
            block.selected_range = 4..4;
            block.sync_inline_projection_for_focus(true);
            block.move_to(block.visible_len(), cx);
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let line_height = px(20.0);
        let link_gutter = block.read_with(cx, |block, _cx| {
            block_link_icon_gutter(block, line_height)
        });
        let wrap_width = px(320.0) - link_gutter;
        let lines = shaped_lines(&display_text, wrap_width, cx);
        block.read_with(cx, |block, _cx| {
            let visible_len = block.visible_len();
            let cursor_offset = block.cursor_offset();
            assert_eq!(cursor_offset, visible_len);

            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let text_bounds = source_text_bounds(bounds, link_gutter);
            let layout_bounds = link_icon_layout_bounds(text_bounds, link_gutter);
            let insets = compute_link_icon_text_insets(
                block,
                &lines,
                layout_bounds,
                text_bounds,
                line_height,
                block.text_align(),
            );
            assert!(
                !insets.is_empty(),
                "inline projected link should reserve text inset"
            );

            let end_cursor = cursor_bounds_for_offset(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                cursor_offset,
                block.text_align(),
                px(1.0),
                &insets,
            )
            .expect("end cursor bounds");
            let text_end_segments = range_segment_bounds_with_link_insets(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                visible_len.saturating_sub(1)..visible_len,
                block.text_align(),
                &insets,
            );
            let text_end = text_end_segments.last().expect("last char segment");
            assert!(
                (end_cursor.left() - text_end.right()).abs() <= px(1.0),
                "cursor ({:?}) should align with text end ({:?})",
                end_cursor.left(),
                text_end.right(),
            );

            let mouse_offset = offset_for_mouse_position(
                &lines,
                text_bounds,
                line_height,
                &display_text,
                block.text_align(),
                end_cursor.center(),
                &insets,
            );
            assert_eq!(mouse_offset, cursor_offset);
        });
    }
}
