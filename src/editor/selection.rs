//! Editor-level selection spanning multiple rendered blocks.

use std::collections::HashMap;
use std::ops::Range;
use std::time::Instant;

use gpui::*;

use super::{
    ai_context, CrossBlockDrag, CrossBlockSelection, CrossBlockSelectionEndpoint, Editor,
    SourceTargetMapping, UndoSelectionSnapshot, ViewMode,
};
use crate::components::markdown::inline::clamp_to_char_boundary;
use crate::components::{
    Block, BlockKind, Copy, Cut, Delete, DeleteBack, UndoCaptureKind,
    serialize_table_markdown_lines,
};

fn clamp_source_byte_range(source: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_to_char_boundary(source, range.start.min(source.len()));
    let end = clamp_to_char_boundary(source, range.end.min(source.len()));
    start.min(end)..start.max(end)
}

/// Cross-block selection with endpoints ordered by visible block position.
#[derive(Clone, Copy)]
struct NormalizedCrossBlockSelection {
    start: CrossBlockSelectionEndpoint,
    end: CrossBlockSelectionEndpoint,
    start_index: usize,
    end_index: usize,
    reversed: bool,
}

impl Editor {
    fn clear_cross_block_selection_visuals(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, cx| {
                if block.editor_selection_range.take().is_some() {
                    changed = true;
                    cx.notify();
                }
            });
        }
        changed
    }

    pub(super) fn clear_cross_block_selection(&mut self, cx: &mut Context<Self>) {
        let had_selection = self.cross_block_selection.take().is_some();
        self.cross_block_drag = None;
        let changed_visuals = self.clear_cross_block_selection_visuals(cx);
        let changed = had_selection || changed_visuals;
        if changed {
            cx.notify();
        }
    }

    pub(super) fn select_all_document(&mut self, cx: &mut Context<Self>) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        let visible_blocks = self.document.visible_blocks().to_vec();
        let Some(first) = visible_blocks.first() else {
            return;
        };
        let Some(last) = visible_blocks.last() else {
            return;
        };

        let first_entity = first.entity.clone();
        let last_entity = last.entity.clone();
        let last_len = last_entity.read(cx).visible_len();

        for visible in &visible_blocks {
            visible.entity.update(cx, |block, cx| {
                block.selected_range = 0..0;
                block.selection_reversed = false;
                block.marked_range = None;
                block.vertical_motion_x = None;
                cx.notify();
            });
        }

        self.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: first_entity.entity_id(),
                offset: 0,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: last_entity.entity_id(),
                offset: last_len,
            },
        });
        self.cross_block_drag = None;
        self.sync_cross_block_selection_visuals(cx);
        self.focus_block(last_entity.entity_id());
        last_entity.update(cx, |block, cx| {
            block.selected_range = last_len..last_len;
            block.cursor_blink_epoch = Instant::now();
            cx.notify();
        });
        cx.notify();
    }

    fn begin_cross_block_drag_at_point(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let had_selection = self.cross_block_selection.take().is_some();
        let changed_visuals = self.clear_cross_block_selection_visuals(cx);
        let changed = had_selection || changed_visuals;
        self.cross_block_drag = self
            .cross_block_endpoint_for_point(position, cx)
            .map(|anchor| CrossBlockDrag { anchor });
        if changed {
            cx.notify();
        }
    }

    pub(super) fn on_editor_capture_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            cx.propagate();
            return;
        }

        if self.view_mode != ViewMode::Rendered {
            cx.propagate();
            return;
        }

        if event.click_count >= 2
            && self.apply_rendered_word_or_line_selection_on_mouse_down(event, window, cx)
        {
            cx.stop_propagation();
            return;
        }

        self.begin_cross_block_drag_at_point(event.position, cx);
        cx.propagate();
    }

    pub(super) fn on_editor_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let Some(drag) = self.cross_block_drag else {
            return;
        };
        let Some(focus) = self.cross_block_endpoint_for_point(event.position, cx) else {
            return;
        };

        if self.cross_block_selection.is_none() && drag.anchor.entity_id == focus.entity_id {
            if let Some(entity) = self.document.block_entity_by_id(drag.anchor.entity_id) {
                if entity.read(cx).is_selecting {
                    return;
                }
            }
        }

        let selection = CrossBlockSelection {
            anchor: drag.anchor,
            focus,
        };
        if self.cross_block_selection_is_empty(selection) {
            self.cross_block_selection = None;
        } else {
            self.cross_block_selection = Some(selection);
        }
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
    }

    pub(super) fn on_editor_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view_mode == ViewMode::Rendered && event.click_count >= 2 {
            self.apply_rendered_word_or_line_selection(event, window, cx);
        }
        self.cross_block_drag = None;
        self.end_block_pointer_selection_sessions(cx);
    }

    fn apply_rendered_word_or_line_selection_on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(endpoint) = self.cross_block_endpoint_for_point(event.position, cx) else {
            return false;
        };
        let Some(entity) = self.document.block_entity_by_id(endpoint.entity_id) else {
            return false;
        };

        self.cross_block_drag = None;
        self.cross_block_selection = None;
        self.sync_cross_block_selection_visuals(cx);

        let handled = entity.update(cx, |block, cx| {
            if !block.focus_handle.is_focused(window) {
                block.focus_handle.focus(window);
            }
            if event.click_count >= 2
                && block.try_handle_link_double_click(event.position, window, cx)
            {
                return true;
            }
            block.try_select_word_or_line_at_click_count(
                event.position,
                event.click_count,
                window,
                cx,
            )
        });
        if !handled {
            return false;
        }

        self.focus_block(endpoint.entity_id);
        cx.notify();
        true
    }

    fn apply_rendered_word_or_line_selection(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.cross_block_endpoint_for_point(event.position, cx) else {
            return;
        };
        let Some(entity) = self.document.block_entity_by_id(endpoint.entity_id) else {
            return;
        };

        self.cross_block_selection = None;
        self.sync_cross_block_selection_visuals(cx);

        let handled = entity.update(cx, |block, cx| {
            if !block.focus_handle.is_focused(window) {
                block.focus_handle.focus(window);
            }
            block.try_select_word_or_line_at_click(event, window, cx)
        });
        if !handled {
            return;
        }

        self.focus_block(endpoint.entity_id);
        cx.notify();
    }

    pub(super) fn on_copy_capture(
        &mut self,
        _: &Copy,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(markdown) = self.cross_block_selected_markdown(cx) else {
            cx.propagate();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        cx.stop_propagation();
    }

    pub(super) fn on_cut_capture(&mut self, _: &Cut, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(markdown) = self.cross_block_selected_markdown(cx) else {
            cx.propagate();
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(markdown));
        self.delete_cross_block_selection(cx);
        cx.stop_propagation();
    }

    pub(super) fn on_delete_capture(
        &mut self,
        _: &Delete,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.delete_cross_block_selection(cx) {
            cx.propagate();
            return;
        }
        cx.stop_propagation();
    }

    pub(super) fn on_delete_back_capture(
        &mut self,
        _: &DeleteBack,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.delete_cross_block_selection(cx) {
            cx.propagate();
            return;
        }
        cx.stop_propagation();
    }

    pub(super) fn cross_block_source_selection_snapshot(
        &self,
        cx: &App,
    ) -> Option<UndoSelectionSnapshot> {
        let normalized = self.normalized_cross_block_selection(cx)?;
        let range = self.cross_block_source_range_for_normalized(normalized, cx)?;
        Some(UndoSelectionSnapshot {
            range,
            reversed: normalized.reversed,
        })
    }

    pub(super) fn apply_cross_block_selection_snapshot_if_possible(
        &mut self,
        snapshot: &UndoSelectionSnapshot,
        cx: &mut Context<Self>,
    ) -> bool {
        if snapshot.range.is_empty() {
            return false;
        }

        let mappings = self.build_source_target_mappings(cx);
        let Some(start) = self.endpoint_for_source_offset(snapshot.range.start, &mappings, cx)
        else {
            return false;
        };
        let Some(end) = self.endpoint_for_source_offset(snapshot.range.end, &mappings, cx) else {
            return false;
        };
        let Some(start_index) = self.document.visible_index_for_entity_id(start.entity_id) else {
            return false;
        };
        let Some(end_index) = self.document.visible_index_for_entity_id(end.entity_id) else {
            return false;
        };
        if start_index == end_index {
            return false;
        }

        self.cross_block_selection = Some(if snapshot.reversed {
            CrossBlockSelection {
                anchor: end,
                focus: start,
            }
        } else {
            CrossBlockSelection {
                anchor: start,
                focus: end,
            }
        });
        self.cross_block_drag = None;
        self.sync_cross_block_selection_visuals(cx);
        let focus = if snapshot.reversed { start } else { end };
        self.focus_block(focus.entity_id);
        cx.notify();
        true
    }

    fn cross_block_endpoint_for_point(
        &self,
        position: Point<Pixels>,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let mut previous: Option<(Entity<Block>, Bounds<Pixels>)> = None;
        for visible in self.document.visible_blocks() {
            let entity = visible.entity.clone();
            let block = entity.read(cx);
            let bounds = block.last_bounds.or(block.interaction_bounds);
            let Some(bounds) = bounds else {
                continue;
            };

            if position.y < bounds.top() {
                if let Some((previous, _)) = previous {
                    let offset = previous.read(cx).visible_len();
                    return Some(CrossBlockSelectionEndpoint {
                        entity_id: previous.entity_id(),
                        offset,
                    });
                }
                return Some(CrossBlockSelectionEndpoint {
                    entity_id: entity.entity_id(),
                    offset: 0,
                });
            }

            if position.y <= bounds.bottom() {
                let offset = entity.read(cx).index_for_mouse_position(position);
                return Some(CrossBlockSelectionEndpoint {
                    entity_id: entity.entity_id(),
                    offset,
                });
            }

            previous = Some((entity, bounds));
        }

        previous.map(|(entity, _)| CrossBlockSelectionEndpoint {
            entity_id: entity.entity_id(),
            offset: entity.read(cx).visible_len(),
        })
    }

    fn cross_block_selection_is_empty(&self, selection: CrossBlockSelection) -> bool {
        let Some(anchor_index) = self
            .document
            .visible_index_for_entity_id(selection.anchor.entity_id)
        else {
            return true;
        };
        let Some(focus_index) = self
            .document
            .visible_index_for_entity_id(selection.focus.entity_id)
        else {
            return true;
        };
        anchor_index == focus_index && selection.anchor.offset == selection.focus.offset
    }

    fn normalized_cross_block_selection(&self, cx: &App) -> Option<NormalizedCrossBlockSelection> {
        let selection = self.cross_block_selection?;
        self.normalize_cross_block_selection(selection, cx)
    }

    fn normalize_cross_block_selection(
        &self,
        selection: CrossBlockSelection,
        cx: &App,
    ) -> Option<NormalizedCrossBlockSelection> {
        let anchor = self.clamp_cross_block_endpoint(selection.anchor, cx)?;
        let focus = self.clamp_cross_block_endpoint(selection.focus, cx)?;
        let anchor_index = self
            .document
            .visible_index_for_entity_id(anchor.entity_id)?;
        let focus_index = self.document.visible_index_for_entity_id(focus.entity_id)?;
        let reversed = focus_index < anchor_index
            || (focus_index == anchor_index && focus.offset < anchor.offset);
        let (start, end, start_index, end_index) = if reversed {
            (focus, anchor, focus_index, anchor_index)
        } else {
            (anchor, focus, anchor_index, focus_index)
        };
        if start_index == end_index && start.offset == end.offset {
            return None;
        }
        Some(NormalizedCrossBlockSelection {
            start,
            end,
            start_index,
            end_index,
            reversed,
        })
    }

    pub(super) fn cross_block_selection_end_entity_id(
        &self,
        selection: CrossBlockSelection,
        cx: &App,
    ) -> Option<EntityId> {
        self.normalize_cross_block_selection(selection, cx)
            .map(|selection| selection.end.entity_id)
    }

    fn clamp_cross_block_endpoint(
        &self,
        endpoint: CrossBlockSelectionEndpoint,
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let entity = self.document.block_entity_by_id(endpoint.entity_id)?;
        let block = entity.read(cx);
        let text = block.display_text();
        let len = block.visible_len();
        Some(CrossBlockSelectionEndpoint {
            entity_id: endpoint.entity_id,
            offset: clamp_to_char_boundary(text, endpoint.offset.min(len)),
        })
    }

    pub(super) fn sync_cross_block_selection_visuals(&mut self, cx: &mut Context<Self>) {
        let normalized = self.normalized_cross_block_selection(cx);
        let visible_blocks = self.document.visible_blocks().to_vec();
        for (index, visible) in visible_blocks.into_iter().enumerate() {
            let next_range = normalized.and_then(|selection| {
                if index < selection.start_index || index > selection.end_index {
                    return None;
                }
                let block = visible.entity.read(cx);
                let len = block.visible_len();
                let range = if selection.start_index == selection.end_index {
                    selection.start.offset.min(len)..selection.end.offset.min(len)
                } else if index == selection.start_index {
                    selection.start.offset.min(len)..len
                } else if index == selection.end_index {
                    0..selection.end.offset.min(len)
                } else {
                    0..len
                };
                (!range.is_empty()).then_some(range)
            });

            visible.entity.update(cx, |block, cx| {
                let next_range = next_range.filter(|_| block.shows_text_selection_highlight());
                if block.editor_selection_range != next_range {
                    block.editor_selection_range = next_range.clone();
                    cx.notify();
                }
            });
        }
    }

    fn source_mapping_by_entity_id(&self, cx: &App) -> HashMap<EntityId, SourceTargetMapping> {
        self.build_source_target_mappings(cx)
            .into_iter()
            .map(|mapping| (mapping.entity.entity_id(), mapping))
            .collect()
    }

    fn endpoint_source_offset(
        &self,
        endpoint: CrossBlockSelectionEndpoint,
        mappings: &HashMap<EntityId, SourceTargetMapping>,
        cx: &App,
    ) -> Option<usize> {
        let mapping = mappings.get(&endpoint.entity_id)?;
        let block = mapping.entity.read(cx);
        let visible_len = block.visible_len();
        if endpoint.offset == 0 {
            return Some(mapping.full_source_range.start);
        }
        if endpoint.offset >= visible_len {
            return Some(mapping.full_source_range.end);
        }
        let markdown_offset = block
            .current_range_to_markdown_range(endpoint.offset..endpoint.offset)
            .start;
        let max_content = mapping.content_to_source.len().saturating_sub(1);
        Some(
            mapping.full_source_range.start
                + mapping.content_to_source[markdown_offset.min(max_content)],
        )
    }

    pub(super) fn endpoint_for_source_offset(
        &self,
        offset: usize,
        mappings: &[SourceTargetMapping],
        cx: &App,
    ) -> Option<CrossBlockSelectionEndpoint> {
        let mapping = mappings.iter().min_by_key(|mapping| {
            Self::source_offset_distance(&mapping.full_source_range, offset)
        })?;
        let local = if offset <= mapping.full_source_range.start {
            0
        } else if offset >= mapping.full_source_range.end {
            mapping.full_source_range.len()
        } else {
            offset - mapping.full_source_range.start
        };
        let content_offset =
            mapping.source_to_content[local.min(mapping.source_to_content.len().saturating_sub(1))];
        let block = mapping.entity.read(cx);
        Some(CrossBlockSelectionEndpoint {
            entity_id: mapping.entity.entity_id(),
            offset: block.markdown_offset_to_current_offset(content_offset),
        })
    }

    pub(super) fn clear_search_match_highlight(&mut self, cx: &mut Context<Self>) {
        let had_source_range = self.search.match_source_range.take().is_some();
        let mut changed = had_source_range;
        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, cx| {
                if !block.search_highlight_ranges.is_empty() {
                    block.search_highlight_ranges.clear();
                    block.search_highlight_active_range = None;
                    changed = true;
                    cx.notify();
                }
            });
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn refresh_search_match_highlights(&mut self, cx: &mut Context<Self>) {
        if self.search.match_source_range.is_some() {
            self.sync_search_match_highlights(cx);
            cx.notify();
        }
    }

    fn sync_search_match_highlights(&mut self, cx: &mut Context<Self>) {
        let source_range = self.search.match_source_range.clone();
        let mappings = self.build_source_target_mappings(cx);
        let visible_blocks = self.document.visible_blocks().to_vec();
        for visible in visible_blocks {
            let entity_id = visible.entity.entity_id();
            let next_range = source_range.as_ref().and_then(|range| {
                let mapping = mappings
                    .iter()
                    .find(|mapping| mapping.entity.entity_id() == entity_id)?;
                let overlap_start = range.start.max(mapping.full_source_range.start);
                let overlap_end = range.end.min(mapping.full_source_range.end);
                if overlap_start >= overlap_end {
                    return None;
                }
                let start = self.endpoint_for_source_offset(overlap_start, &mappings, cx)?;
                let end = self.endpoint_for_source_offset(overlap_end, &mappings, cx)?;
                if start.entity_id != entity_id || end.entity_id != entity_id {
                    return None;
                }
                Some(start.offset.min(end.offset)..start.offset.max(end.offset))
            });

            let next_ranges = next_range.map(|range| vec![range]).unwrap_or_default();
            visible.entity.update(cx, |block, cx| {
                let active = None;
                if block.search_highlight_ranges != next_ranges
                    || block.search_highlight_active_range != active
                {
                    block.search_highlight_ranges = next_ranges;
                    block.search_highlight_active_range = active;
                    cx.notify();
                }
            });
        }
    }

    pub(super) fn jump_to_source_line_with_query(
        &mut self,
        line: usize,
        query: &str,
        preview: &str,
        match_start_byte: Option<usize>,
        raw_file_len: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        if line == 0 {
            return false;
        }

        let source = self.last_stable_source_text.as_str();
        let Some(source_range) = resolve_search_match_in_source(
            source,
            query,
            line,
            preview,
            match_start_byte,
            raw_file_len,
        ) else {
            return false;
        };

        let mappings = self.build_source_target_mappings(cx);
        let Some(start) = self.endpoint_for_source_offset(source_range.start, &mappings, cx) else {
            return false;
        };
        let Some(end) = self.endpoint_for_source_offset(source_range.end, &mappings, cx) else {
            return false;
        };

        if start.entity_id == end.entity_id {
            let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
                return false;
            };
            let selection = start.offset.min(end.offset)..start.offset.max(end.offset);
            block.update(cx, |block, cx| {
                block.selected_range = selection.clone();
                block.selection_reversed = false;
                block.marked_range = None;
                block.vertical_motion_x = None;
                block.cursor_blink_epoch = Instant::now();
                cx.notify();
            });
            self.focus_block(start.entity_id);
        } else {
            let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
                return false;
            };
            block.update(cx, |block, cx| {
                block.selected_range = start.offset..start.offset;
                block.selection_reversed = false;
                block.marked_range = None;
                block.vertical_motion_x = None;
                block.cursor_blink_epoch = Instant::now();
                cx.notify();
            });
            self.focus_block(start.entity_id);
        }

        self.search.match_source_range = Some(source_range);
        self.sync_search_match_highlights(cx);
        cx.notify();
        true
    }

    fn cross_block_source_range_for_normalized(
        &self,
        selection: NormalizedCrossBlockSelection,
        cx: &App,
    ) -> Option<Range<usize>> {
        let mappings = self.source_mapping_by_entity_id(cx);
        let start = self.endpoint_source_offset(selection.start, &mappings, cx)?;
        let end = self.endpoint_source_offset(selection.end, &mappings, cx)?;
        Some(start.min(end)..start.max(end))
    }

    pub(in crate::editor) fn selection_source_byte_range(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Range<usize>> {
        if let Some(normalized) = self.normalized_cross_block_selection(cx) {
            return self.cross_block_source_range_for_normalized(normalized, cx);
        }
        if let Some(block) = self.focused_edit_target(window, cx) {
            let block_ref = block.read(cx);
            if let Some(range) = ai_context::block_text_selection_range(block_ref) {
                let mappings = self.source_mapping_by_entity_id(cx);
                let start = CrossBlockSelectionEndpoint {
                    entity_id: block.entity_id(),
                    offset: range.start,
                };
                let end = CrossBlockSelectionEndpoint {
                    entity_id: block.entity_id(),
                    offset: range.end,
                };
                let start_byte = self.endpoint_source_offset(start, &mappings, cx)?;
                let end_byte = self.endpoint_source_offset(end, &mappings, cx)?;
                return Some(start_byte.min(end_byte)..start_byte.max(end_byte));
            }
        }
        None
    }

    pub(in crate::editor) fn selection_source_line_range(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<(usize, usize)> {
        let source = self.serialized_document_text(cx);
        let byte_range = self.selection_source_byte_range(window, cx)?;
        if byte_range.is_empty() {
            return None;
        }
        let start_line = source_line_number_at(&source, byte_range.start);
        let last_byte = if byte_range.end > byte_range.start {
            byte_range.end.saturating_sub(1)
        } else {
            byte_range.start
        };
        let end_line = source_line_number_at(&source, last_byte);
        Some((start_line, end_line))
    }

    pub(in crate::editor) fn document_file_display_name(&self) -> Option<String> {
        self.file_path.as_ref().and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
    }

    fn rebuild_after_cross_block_source_edit(&mut self, source: String, cx: &mut Context<Self>) {
        match self.view_mode {
            ViewMode::Rendered => {
                let mut roots = Self::build_root_blocks_from_markdown(cx, &source);
                if roots.is_empty() {
                    roots.push(Self::new_block(
                        cx,
                        crate::components::BlockRecord::paragraph(String::new()),
                    ));
                }
                self.document.replace_roots(roots, cx);
                self.rebuild_table_runtimes(cx);
                self.rebuild_image_runtimes(cx);
            }
            ViewMode::Source => {
                let block = Self::new_block(
                    cx,
                    crate::components::BlockRecord::paragraph(source.clone()),
                );
                block.update(cx, |block, _cx| block.set_source_document_mode());
                self.document.replace_roots(vec![block], cx);
                self.table_cells.clear();
            }
        }
    }

    fn apply_marked_source_range(&mut self, source_range: Range<usize>, cx: &mut Context<Self>) {
        if source_range.is_empty() {
            return;
        }
        let mappings = self.build_source_target_mappings(cx);
        let Some(start) = self.endpoint_for_source_offset(source_range.start, &mappings, cx) else {
            return;
        };
        let Some(end) = self.endpoint_for_source_offset(source_range.end, &mappings, cx) else {
            return;
        };
        if start.entity_id != end.entity_id {
            return;
        }
        let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
            return;
        };
        block.update(cx, |block, cx| {
            block.marked_range = Some(start.offset.min(end.offset)..start.offset.max(end.offset));
            cx.notify();
        });
    }

    pub(super) fn replace_cross_block_selection_with_text(
        &mut self,
        new_text: &str,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        undo_kind: UndoCaptureKind,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(selection) = self.normalized_cross_block_selection(cx) else {
            return false;
        };
        let Some(source_range) = self.cross_block_source_range_for_normalized(selection, cx) else {
            return false;
        };

        self.prepare_undo_capture(undo_kind, cx);
        let mut source = self.current_document_source(cx);
        let start = source_range.start.min(source.len());
        let end = source_range.end.min(source.len());
        source.replace_range(start..end, new_text);
        self.cross_block_selection = None;
        self.cross_block_drag = None;

        let inserted_start = start;
        let inserted_end = inserted_start + new_text.len();
        let selected_source_range = selected_range_relative
            .map(|relative| {
                inserted_start + relative.start.min(new_text.len())
                    ..inserted_start + relative.end.min(new_text.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        let marked_source_range =
            (mark_inserted_text && !new_text.is_empty()).then_some(inserted_start..inserted_end);

        self.rebuild_after_cross_block_source_edit(source, cx);
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: selected_source_range,
                reversed: false,
            },
            cx,
        );
        if let Some(marked_source_range) = marked_source_range {
            self.apply_marked_source_range(marked_source_range, cx);
        }
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        self.sync_cross_block_selection_visuals(cx);
        self.request_active_block_scroll_into_view(cx);
        cx.notify();
        true
    }

    pub(super) fn cross_block_selected_markdown(&self, cx: &App) -> Option<String> {
        let selection = self.normalized_cross_block_selection(cx)?;
        let source = self.current_document_source(cx);
        let mappings = self.source_mapping_by_entity_id(cx);
        let visible = self.document.visible_blocks();
        let mut chunks = Vec::new();

        for index in selection.start_index..=selection.end_index {
            let entity = visible.get(index)?.entity.clone();
            let block = entity.read(cx);
            let len = block.visible_len();
            let range = if selection.start_index == selection.end_index {
                selection.start.offset.min(len)..selection.end.offset.min(len)
            } else if index == selection.start_index {
                selection.start.offset.min(len)..len
            } else if index == selection.end_index {
                0..selection.end.offset.min(len)
            } else {
                0..len
            };
            let full_block = range.start == 0
                && range.end == len
                && (selection.start_index != selection.end_index || len > 0);
            let include_empty_middle =
                len == 0 && selection.start_index < index && index < selection.end_index;
            if range.is_empty() && !include_empty_middle {
                continue;
            }
            chunks.push(self.markdown_chunk_for_block(
                &entity,
                range,
                full_block || include_empty_middle,
                &source,
                &mappings,
                cx,
            ));
        }

        Some(chunks.join("\n"))
    }

    fn markdown_chunk_for_block(
        &self,
        entity: &Entity<Block>,
        range: Range<usize>,
        full_block: bool,
        source: &str,
        mappings: &HashMap<EntityId, SourceTargetMapping>,
        cx: &App,
    ) -> String {
        if let Some(mapping) = mappings.get(&entity.entity_id()) {
            if full_block {
                return source[mapping.full_source_range.clone()].to_string();
            }

            let start = self
                .endpoint_source_offset(
                    CrossBlockSelectionEndpoint {
                        entity_id: entity.entity_id(),
                        offset: range.start,
                    },
                    mappings,
                    cx,
                )
                .unwrap_or(mapping.full_source_range.start);
            let end = self
                .endpoint_source_offset(
                    CrossBlockSelectionEndpoint {
                        entity_id: entity.entity_id(),
                        offset: range.end,
                    },
                    mappings,
                    cx,
                )
                .unwrap_or(mapping.full_source_range.end);
            let range = clamp_source_byte_range(source, start..end);
            return source[range].to_string();
        }

        let block = entity.read(cx);
        if full_block {
            return match block.kind() {
                BlockKind::Table => block
                    .record
                    .table
                    .as_ref()
                    .map(serialize_table_markdown_lines)
                    .map(|lines| lines.join("\n"))
                    .unwrap_or_default(),
                _ => block
                    .record
                    .markdown_line(block.render_depth, block.list_ordinal),
            };
        }

        let markdown = block.record.title.serialize_markdown();
        let markdown_range = block.current_range_to_markdown_range(range);
        markdown
            .get(markdown_range)
            .map(ToOwned::to_owned)
            .unwrap_or_default()
    }

    pub(super) fn delete_cross_block_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.normalized_cross_block_selection(cx) else {
            return false;
        };
        let Some(source_range) = self.cross_block_source_range_for_normalized(selection, cx) else {
            return false;
        };
        if source_range.is_empty() {
            return false;
        }

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let mut source = self.current_document_source(cx);
        let range = clamp_source_byte_range(&source, source_range);
        source.replace_range(range.clone(), "");
        let start = range.start;
        self.cross_block_selection = None;
        self.cross_block_drag = None;

        self.rebuild_after_cross_block_source_edit(source, cx);

        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: start..start,
                reversed: false,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        self.sync_cross_block_selection_visuals(cx);
        cx.notify();
        true
    }

    /// Jump the caret to the start of a 0-based source line index (as used by the outline tree).
    pub(super) fn jump_to_source_line_index(
        &mut self,
        line_index: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let source = self.serialized_document_text(cx);
        let Some(byte_offset) = source_line_index_start_offset(&source, line_index) else {
            return false;
        };

        let mappings = self.build_source_target_mappings(cx);
        let Some(start) = self.endpoint_for_source_offset(byte_offset, &mappings, cx) else {
            return false;
        };
        let Some(block) = self.focusable_entity_by_id(start.entity_id) else {
            return false;
        };

        block.update(cx, |block, cx| {
            block.selected_range = start.offset..start.offset;
            block.selection_reversed = false;
            block.marked_range = None;
            block.vertical_motion_x = None;
            block.cursor_blink_epoch = Instant::now();
            cx.notify();
        });
        self.focus_block(start.entity_id);
        self.clear_search_match_highlight(cx);
        cx.notify();
        true
    }
}

fn source_line_index_start_offset(source: &str, line_index: usize) -> Option<usize> {
    source_line_start_offset(source, line_index + 1)
}

fn source_line_start_offset(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }

    let mut current_line = 1usize;
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            current_line += 1;
            if current_line == line {
                return Some(index + 1);
            }
        }
    }

    (current_line == line).then_some(source.len())
}

fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut index = offset.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn source_line_number_at(source: &str, offset: usize) -> usize {
    let offset = floor_char_boundary(source, offset);
    source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn all_query_matches_in_source(source: &str, query: &str) -> Vec<Range<usize>> {
    super::search_match::find_case_insensitive_ranges(source, query)
}

fn resolve_search_match_in_source(
    source: &str,
    query: &str,
    target_line: usize,
    preview: &str,
    match_start_byte: Option<usize>,
    raw_file_len: Option<usize>,
) -> Option<Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let matches = all_query_matches_in_source(source, query);
    if matches.is_empty() {
        return None;
    }
    if matches.len() == 1 {
        return Some(matches[0].clone());
    }

    let byte_hint = match (match_start_byte, raw_file_len) {
        (Some(byte), Some(len)) if len > 0 => Some(byte * source.len() / len),
        _ => None,
    };

    matches
        .into_iter()
        .min_by_key(|range| {
            let line = source_line_number_at(source, range.start);
            let line_dist = line.abs_diff(target_line);
            let byte_dist = byte_hint
                .map(|hint| range.start.abs_diff(hint))
                .unwrap_or(usize::MAX / 4);
            let preview_dist = if preview.is_empty() {
                0
            } else {
                let line_start = source_line_start_offset(source, line).unwrap_or(0);
                let line_end = source[line_start..]
                    .find('\n')
                    .map(|index| line_start + index)
                    .unwrap_or(source.len());
                let line_text = &source[line_start..line_end];
                if line_text.to_lowercase().contains(&preview.to_lowercase()) {
                    0
                } else {
                    1
                }
            };
            (line_dist, preview_dist, byte_dist)
        })
        .map(|range| {
            let end = range.end;
            range.start..end
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gpui::{AppContext, Bounds, Context, TestAppContext, point, px, size};

    use super::{CrossBlockSelection, CrossBlockSelectionEndpoint, Editor, source_line_index_start_offset, source_line_number_at};
    use crate::components::{Cut, Undo, UndoCaptureKind};
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;

    fn init_editor_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init(cx);
            crate::components::init_with_keybindings(cx, &BTreeMap::new());
        });
    }

    fn redraw(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }

    fn set_selection(
        editor: &mut Editor,
        start_index: usize,
        start_offset: usize,
        end_index: usize,
        end_offset: usize,
        cx: &mut Context<Editor>,
    ) {
        let visible = editor.document.visible_blocks().to_vec();
        let start = visible[start_index].entity.entity_id();
        let end = visible[end_index].entity.entity_id();
        editor.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: start,
                offset: start_offset,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: end,
                offset: end_offset,
            },
        });
        editor.sync_cross_block_selection_visuals(cx);
    }

    fn assign_visible_block_bounds(editor: &mut Editor, cx: &mut Context<Editor>) {
        for (index, visible) in editor
            .document
            .visible_blocks()
            .to_vec()
            .into_iter()
            .enumerate()
        {
            visible.entity.update(cx, move |block, _cx| {
                block.last_bounds = Some(Bounds::new(
                    point(px(0.0), px(index as f32 * 32.0)),
                    size(px(400.0), px(24.0)),
                ));
            });
        }
    }

    #[test]
    fn source_line_index_start_offset_matches_outline_line_indices() {
        let source = "# Root\n\n## Child\n\n# Next";
        assert_eq!(source_line_index_start_offset(source, 0), Some(0));
        assert_eq!(source_line_index_start_offset(source, 2), Some(8));
        assert_eq!(source_line_index_start_offset(source, 4), Some(18));
    }

    #[test]
    fn source_line_number_at_accepts_mid_utf8_byte_offset() {
        let source = "第一行\n加入对话功能\n第三行";
        let anchor = source.find('功').expect("find 功");
        let mid = anchor + 1;
        assert!(!source.is_char_boundary(mid));
        assert_eq!(source_line_number_at(source, mid), 2);
    }

    #[test]
    fn mouse_down_starts_cross_block_drag_after_clearing_old_selection() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            assign_visible_block_bounds(editor, cx);
            set_selection(editor, 0, 0, 2, 2, cx);
            assert!(editor.cross_block_selection.is_some());
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .any(|visible| visible.entity.read(cx).editor_selection_range.is_some())
            );

            editor.begin_cross_block_drag_at_point(point(px(8.0), px(4.0)), cx);

            assert!(editor.cross_block_selection.is_none());
            assert!(editor.cross_block_drag.is_some());
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .all(|visible| visible.entity.read(cx).editor_selection_range.is_none())
            );
        });
        cx.quit();
    }

    #[test]
    fn typing_replaces_cross_block_selection_with_plain_text() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "X",
                None,
                false,
                UndoCaptureKind::CoalescibleText,
                cx
            ));

            assert_eq!(editor.document.markdown_text(cx), "alXmma");
            assert!(editor.cross_block_selection.is_none());
            assert!(editor.cross_block_drag.is_none());
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.selected_range, 3..3);
            assert!(block.marked_range.is_none());
        });
        cx.quit();
    }

    #[test]
    fn ime_composition_replaces_cross_block_selection_and_marks_inserted_text() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "ni",
                Some(2..2),
                true,
                UndoCaptureKind::CoalescibleText,
                cx
            ));

            assert_eq!(editor.document.markdown_text(cx), "alnimma");
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.selected_range, 4..4);
            assert_eq!(block.marked_range, Some(2..4));
            assert!(block.editor_selection_range.is_none());
        });
        cx.quit();
    }

    #[test]
    fn cross_block_selection_marks_heading_visual_ranges() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| {
            Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
        });

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 0, 0, 3, cx);

            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.editor_selection_range, Some(0..3));
        });
        cx.quit();
    }

    #[test]
    fn cross_block_selection_marks_visual_ranges_and_copies_markdown() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "alpha **bold**\n\n- item\n\n![alt](image.png)".to_string(),
                None,
            )
        });

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);

            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("alpha **bold**\n- item\n![alt](image.png)")
            );
            for visible in visible {
                let block = visible.entity.read(cx);
                assert_eq!(block.editor_selection_range, Some(0..block.visible_len()));
            }
        });
        cx.quit();
    }

    #[test]
    fn cross_block_cut_writes_markdown_deletes_range_and_undo_restores() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let original = "alpha\n\nbeta\n\ngamma";
        let (editor, cx) = cx.add_window_view({
            let original = original.to_string();
            move |_window, cx| Editor::from_markdown(cx, original.clone(), None)
        });
        redraw(cx);

        editor.update(cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("pha\nbeta\nga")
            );
        });
        redraw(cx);

        cx.dispatch_action(Cut);
        redraw(cx);

        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("pha\nbeta\nga")
        );
        assert_eq!(
            editor.read_with(cx, |editor, cx| editor.document.markdown_text(cx)),
            "almma"
        );

        cx.dispatch_action(Undo);
        redraw(cx);

        assert_eq!(
            editor.read_with(cx, |editor, cx| editor.document.markdown_text(cx)),
            original
        );
        editor.read_with(cx, |editor, cx| {
            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("pha\nbeta\nga")
            );
        });
        cx.quit();
    }
}
