//! Viewport windowing for long documents: estimate row heights, pick a
//! visible index range, and leave spacers for off-screen content.
//!
//! This is an MVP (perf-memory-plan P2.1–P2.2). Heights prefer measured
//! `last_bounds` when available and otherwise fall back to a wrap estimate.
//! Callout / footnote groups that intersect the window are expanded so chrome
//! stays intact.

use gpui::{App, Pixels, px};

use crate::components::Block;

use super::tree::VisibleBlock;

/// Documents at or above this visible-block count use viewport windowing.
pub(crate) const VIRTUALIZE_THRESHOLD: usize = 100;
/// Extra rows rendered above and below the viewport.
pub(crate) const OVERSCAN_ROWS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BlockWindow {
    pub start: usize,
    pub end: usize,
    pub top_spacer: f32,
    pub bottom_spacer: f32,
}

impl BlockWindow {
    pub fn full(len: usize) -> Self {
        Self {
            start: 0,
            end: len,
            top_spacer: 0.0,
            bottom_spacer: 0.0,
        }
    }
}

pub(crate) fn should_virtualize(visible_len: usize) -> bool {
    visible_len >= VIRTUALIZE_THRESHOLD
}

/// Estimate the laid-out content height of one block (excluding inter-row gap).
pub(crate) fn estimate_block_content_height(
    block: &Block,
    line_height: f32,
    min_height: f32,
) -> f32 {
    if let Some(bounds) = block.last_bounds {
        return f32::from(bounds.size.height).max(min_height);
    }

    let text = block.display_text();
    if text.is_empty() {
        return min_height;
    }

    let newline_count = text.bytes().filter(|&b| b == b'\n').count();
    // Rough wrap estimate when we have not measured this block yet.
    const APPROX_CHARS_PER_LINE: usize = 72;
    let wrapped_extra = text.len() / APPROX_CHARS_PER_LINE;
    let lines = (newline_count + 1 + wrapped_extra).max(1);
    ((lines as f32) * line_height).max(min_height)
}

/// Per visible-block row extent: leading gap + content height.
pub(crate) fn build_row_extents(
    visible_blocks: &[VisibleBlock],
    top_gaps: &[f32],
    line_height: f32,
    min_height: f32,
    cx: &App,
) -> Vec<f32> {
    debug_assert_eq!(visible_blocks.len(), top_gaps.len());
    visible_blocks
        .iter()
        .zip(top_gaps.iter())
        .map(|(visible, &gap)| {
            let content = visible.entity.read_with(cx, |block, _cx| {
                estimate_block_content_height(block, line_height, min_height)
            });
            gap + content
        })
        .collect()
}

pub(crate) fn compute_block_window(
    extents: &[f32],
    scroll_y: f32,
    viewport_height: f32,
    overscan: usize,
) -> BlockWindow {
    let len = extents.len();
    if len == 0 {
        return BlockWindow::full(0);
    }

    let scroll_y = scroll_y.max(0.0);
    let viewport_bottom = scroll_y + viewport_height.max(1.0);

    let mut prefix = 0.0f32;
    let mut start = 0usize;
    // Skip rows that end at or before the viewport top.
    while start < len && prefix + extents[start] <= scroll_y {
        prefix += extents[start];
        start += 1;
    }

    let mut end = start;
    let mut covered = prefix;
    while end < len && covered < viewport_bottom {
        covered += extents[end];
        end += 1;
    }

    start = start.saturating_sub(overscan);
    end = (end + overscan).min(len);

    let top_spacer: f32 = extents[..start].iter().sum();
    let bottom_spacer: f32 = extents[end..].iter().sum();

    BlockWindow {
        start,
        end,
        top_spacer,
        bottom_spacer,
    }
}

/// Expand `[start, end)` so callout / footnote groups are not split.
pub(crate) fn expand_window_for_spacing_groups(
    start: usize,
    end: usize,
    callout_anchors: &[Option<uuid::Uuid>],
    footnote_anchors: &[Option<uuid::Uuid>],
) -> (usize, usize) {
    let len = callout_anchors.len();
    if len == 0 || start >= end {
        return (start.min(len), end.min(len));
    }

    let mut start = start.min(len);
    let mut end = end.min(len);

    if let Some(anchor) = callout_anchors.get(start).and_then(|a| *a) {
        while start > 0 && callout_anchors[start - 1] == Some(anchor) {
            start -= 1;
        }
    }
    if let Some(anchor) = footnote_anchors.get(start).and_then(|a| *a) {
        while start > 0 && footnote_anchors[start - 1] == Some(anchor) {
            start -= 1;
        }
    }

    if end > 0 {
        if let Some(anchor) = callout_anchors.get(end - 1).and_then(|a| *a) {
            while end < len && callout_anchors[end] == Some(anchor) {
                end += 1;
            }
        }
        if let Some(anchor) = footnote_anchors.get(end - 1).and_then(|a| *a) {
            while end < len && footnote_anchors[end] == Some(anchor) {
                end += 1;
            }
        }
    }

    (start, end)
}

pub(crate) fn scroll_offset_for_row(extents: &[f32], index: usize) -> Pixels {
    let y: f32 = extents.iter().take(index.min(extents.len())).sum();
    -px(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_window_covers_viewport_with_overscan() {
        let extents = vec![20.0; 50];
        let window = compute_block_window(&extents, 100.0, 80.0, 2);
        // scroll 100 → first visible index 5, overscan 2 → start 3
        assert_eq!(window.start, 3);
        // covers through ~180px content from prefix 60 → indices 3..11 + overscan
        assert!(window.end > window.start);
        assert!(window.top_spacer > 0.0);
        assert!(window.bottom_spacer > 0.0);
        let mid: f32 = extents[window.start..window.end].iter().sum();
        assert!((window.top_spacer + mid + window.bottom_spacer - 1000.0).abs() < 0.1);
    }

    #[test]
    fn expand_keeps_callout_group_intact() {
        let anchor = uuid::Uuid::new_v4();
        let callouts = vec![
            None,
            Some(anchor),
            Some(anchor),
            Some(anchor),
            None,
        ];
        let footnotes = vec![None; 5];
        let (start, end) = expand_window_for_spacing_groups(2, 3, &callouts, &footnotes);
        assert_eq!(start, 1);
        assert_eq!(end, 4);
    }

    #[test]
    fn empty_extents_yield_empty_window() {
        assert_eq!(compute_block_window(&[], 0.0, 100.0, 2), BlockWindow::full(0));
    }
}
