//! Viewport windowing for long documents: estimate row heights, pick a
//! visible index range, and leave spacers for off-screen content.
//!
//! Heights prefer measured `last_bounds` (and a retained per-entity cache so
//! unfocused Math/Mermaid clearing `last_bounds` does not forget size), then
//! fall back to a wrap estimate. Callout / footnote groups that intersect the
//! window are expanded so chrome stays intact.

use std::collections::HashMap;

use gpui::{App, EntityId, Pixels, px};

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

    pub fn with_spacers(self, extents: &[f32]) -> Self {
        let start = self.start.min(extents.len());
        let end = self.end.min(extents.len()).max(start);
        Self {
            start,
            end,
            top_spacer: extents[..start].iter().sum(),
            bottom_spacer: extents[end..].iter().sum(),
        }
    }
}

/// Row under the viewport top: index + offset within that row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollAnchor {
    pub index: usize,
    pub offset_in_row: f32,
}

pub(crate) fn should_virtualize(visible_len: usize) -> bool {
    visible_len >= VIRTUALIZE_THRESHOLD
}

/// Retain measured heights for visible entities; keep prior values when
/// `last_bounds` was cleared (e.g. unfocused math preview).
pub(crate) fn refresh_measured_block_heights(
    previous: &HashMap<EntityId, f32>,
    visible_blocks: &[VisibleBlock],
    cx: &App,
) -> HashMap<EntityId, f32> {
    let mut next = HashMap::with_capacity(visible_blocks.len());
    for visible in visible_blocks {
        let id = visible.entity.entity_id();
        let fresh = visible.entity.read_with(cx, |block, _cx| {
            block
                .last_bounds
                .map(|bounds| f32::from(bounds.size.height).max(1.0))
        });
        if let Some(height) = fresh.or_else(|| previous.get(&id).copied()) {
            next.insert(id, height);
        }
    }
    next
}

/// Estimate the laid-out content height of one block (excluding inter-row gap).
pub(crate) fn estimate_block_content_height(
    block: &Block,
    measured: Option<f32>,
    line_height: f32,
    min_height: f32,
) -> f32 {
    if let Some(height) = measured {
        return height.max(min_height);
    }
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
    measured_heights: &HashMap<EntityId, f32>,
    line_height: f32,
    min_height: f32,
    cx: &App,
) -> Vec<f32> {
    debug_assert_eq!(visible_blocks.len(), top_gaps.len());
    visible_blocks
        .iter()
        .zip(top_gaps.iter())
        .map(|(visible, &gap)| {
            let id = visible.entity.entity_id();
            let measured = measured_heights.get(&id).copied();
            let content = visible.entity.read_with(cx, |block, _cx| {
                estimate_block_content_height(block, measured, line_height, min_height)
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

    BlockWindow {
        start,
        end,
        top_spacer: 0.0,
        bottom_spacer: 0.0,
    }
    .with_spacers(extents)
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

/// Ensure `index` (e.g. focus / search jump target) stays inside the window.
pub(crate) fn include_index_in_window(
    window: BlockWindow,
    index: usize,
    overscan: usize,
    extents: &[f32],
) -> BlockWindow {
    let len = extents.len();
    if len == 0 {
        return BlockWindow::full(0);
    }
    let index = index.min(len.saturating_sub(1));
    let start = window.start.min(index.saturating_sub(overscan));
    let end = window.end.max((index + 1 + overscan).min(len));
    BlockWindow {
        start,
        end,
        top_spacer: 0.0,
        bottom_spacer: 0.0,
    }
    .with_spacers(extents)
}

pub(crate) fn scroll_anchor_at(extents: &[f32], scroll_y: f32) -> ScrollAnchor {
    let scroll_y = scroll_y.max(0.0);
    let mut y = 0.0f32;
    for (index, &extent) in extents.iter().enumerate() {
        let next = y + extent;
        if scroll_y < next || index + 1 == extents.len() {
            return ScrollAnchor {
                index,
                offset_in_row: (scroll_y - y).clamp(0.0, extent.max(0.0)),
            };
        }
        y = next;
    }
    ScrollAnchor {
        index: 0,
        offset_in_row: 0.0,
    }
}

pub(crate) fn scroll_y_for_anchor(extents: &[f32], anchor: ScrollAnchor) -> f32 {
    let prefix: f32 = extents.iter().take(anchor.index.min(extents.len())).sum();
    let row = extents
        .get(anchor.index)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    prefix + anchor.offset_in_row.clamp(0.0, row)
}

pub(crate) fn scroll_offset_for_row_with_padding(
    extents: &[f32],
    index: usize,
    padding: f32,
) -> Pixels {
    let y: f32 = extents.iter().take(index.min(extents.len())).sum();
    -px((y - padding).max(0.0))
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
        assert!(window.end > window.start);
        assert!(window.top_spacer > 0.0);
        assert!(window.bottom_spacer > 0.0);
        let mid: f32 = extents[window.start..window.end].iter().sum();
        assert!((window.top_spacer + mid + window.bottom_spacer - 1000.0).abs() < 0.1);
    }

    #[test]
    fn expand_keeps_callout_group_intact() {
        let anchor = uuid::Uuid::new_v4();
        let callouts = vec![None, Some(anchor), Some(anchor), Some(anchor), None];
        let footnotes = vec![None; 5];
        let (start, end) = expand_window_for_spacing_groups(2, 3, &callouts, &footnotes);
        assert_eq!(start, 1);
        assert_eq!(end, 4);
    }

    #[test]
    fn empty_extents_yield_empty_window() {
        assert_eq!(
            compute_block_window(&[], 0.0, 100.0, 2),
            BlockWindow::full(0)
        );
    }

    #[test]
    fn include_index_expands_window() {
        let extents = vec![10.0; 40];
        let window = compute_block_window(&extents, 0.0, 50.0, 1);
        assert!(window.end < 20);
        let expanded = include_index_in_window(window, 25, 2, &extents);
        assert!(expanded.start <= 23);
        assert!(expanded.end > 25);
    }

    #[test]
    fn scroll_anchor_round_trips_through_height_change() {
        let old = vec![20.0; 10];
        let anchor = scroll_anchor_at(&old, 55.0);
        assert_eq!(anchor.index, 2);
        assert!((anchor.offset_in_row - 15.0).abs() < 0.01);

        // Rows above the anchor grew; restoring the anchor keeps the same content.
        let mut new_extents = old.clone();
        new_extents[0] = 40.0;
        new_extents[1] = 40.0;
        let restored = scroll_y_for_anchor(&new_extents, anchor);
        assert!((restored - (40.0 + 40.0 + 15.0)).abs() < 0.01);
    }
}
