//! Undo history and selection snapshot restoration.

use super::*;

impl Editor {
    pub(super) fn empty_selection_snapshot() -> UndoSelectionSnapshot {
        UndoSelectionSnapshot {
            range: 0..0,
            reversed: false,
        }
    }

    pub(super) fn capture_source_selection_snapshot(&self, cx: &App) -> UndoSelectionSnapshot {
        if let Some(snapshot) = self.cross_block_source_selection_snapshot(cx) {
            return snapshot;
        }

        if self.view_mode == ViewMode::Source {
            return self
                .document
                .first_root()
                .map(|block| {
                    let block_ref = block.read(cx);
                    UndoSelectionSnapshot {
                        range: block_ref.selected_range.clone(),
                        reversed: block_ref.selection_reversed,
                    }
                })
                .unwrap_or_else(Self::empty_selection_snapshot);
        }

        let Some(target) = self.current_edit_target_from_state(cx) else {
            return self.last_selection_snapshot.clone();
        };
        let Some(mapping) = self
            .build_source_target_mappings(cx)
            .into_iter()
            .find(|mapping| mapping.entity.entity_id() == target.entity_id())
        else {
            return self.last_selection_snapshot.clone();
        };

        let selected_range = target.read(cx).selected_range.clone();
        let content_range = target
            .read(cx)
            .current_range_to_markdown_range(selected_range);
        let max_offset = mapping.content_to_source.len().saturating_sub(1);
        let start = mapping.full_source_range.start
            + mapping.content_to_source[content_range.start.min(max_offset)];
        let end = mapping.full_source_range.start
            + mapping.content_to_source[content_range.end.min(max_offset)];

        UndoSelectionSnapshot {
            range: start..end,
            reversed: target.read(cx).selection_reversed,
        }
    }

    pub(super) fn capture_history_entry(&self, kind: UndoCaptureKind, cx: &App) -> HistoryEntry {
        HistoryEntry {
            source_text: self.current_document_source(cx),
            selection: self.capture_source_selection_snapshot(cx),
            timestamp: Instant::now(),
            kind,
        }
    }

    pub(super) fn capture_stable_history_entry(
        &self,
        kind: UndoCaptureKind,
        cx: &App,
    ) -> HistoryEntry {
        HistoryEntry {
            source_text: self.last_stable_source_text.clone(),
            // Selection is captured live so undo restores the caret as it was
            // when the edit began; document text stays on the last stable snapshot.
            selection: self.capture_source_selection_snapshot(cx),
            timestamp: Instant::now(),
            kind,
        }
    }

    pub(super) fn prepare_undo_capture(&mut self, kind: UndoCaptureKind, cx: &mut Context<Self>) {
        if self.history_restore_in_progress || self.pending_undo_capture.is_some() {
            return;
        }
        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.capture_history_entry(kind, cx),
        });
    }

    pub(super) fn prepare_undo_capture_from_stable_snapshot(
        &mut self,
        kind: UndoCaptureKind,
        cx: &App,
    ) {
        if self.history_restore_in_progress || self.pending_undo_capture.is_some() {
            return;
        }
        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.capture_stable_history_entry(kind, cx),
        });
    }

    pub(super) fn refresh_stable_document_snapshot(&mut self, cx: &App) {
        self.last_selection_snapshot = self.capture_source_selection_snapshot(cx);
        self.last_stable_source_text = self.current_document_source(cx);
    }

    pub(super) fn finalize_pending_undo_capture(&mut self, cx: &mut Context<Self>) {
        if self.history_restore_in_progress {
            self.pending_undo_capture = None;
            return;
        }

        let Some(pending) = self.pending_undo_capture.take() else {
            self.refresh_stable_document_snapshot(cx);
            return;
        };

        let current_source = self.current_document_source(cx);
        if current_source == pending.snapshot.source_text {
            self.refresh_stable_document_snapshot(cx);
            return;
        }

        let should_merge = matches!(pending.snapshot.kind, UndoCaptureKind::CoalescibleText)
            && self.undo_history.last().is_some_and(|entry| {
                matches!(entry.kind, UndoCaptureKind::CoalescibleText)
                    && pending
                        .snapshot
                        .timestamp
                        .saturating_duration_since(entry.timestamp)
                        <= Self::HISTORY_COALESCE_WINDOW
            });
        if !should_merge {
            self.undo_history.push(pending.snapshot);
            self.redo_history.clear();
            Self::trim_history_stack(&mut self.undo_history);
        }
        self.refresh_stable_document_snapshot(cx);
    }

    pub(super) fn apply_selection_snapshot_in_current_mode(
        &mut self,
        snapshot: &UndoSelectionSnapshot,
        cx: &mut Context<Self>,
    ) {
        match self.view_mode {
            ViewMode::Source => {
                let Some(block) = self.document.first_root().cloned() else {
                    return;
                };
                let len = block.read(cx).visible_len();
                let selected_range = snapshot.range.start.min(len)..snapshot.range.end.min(len);
                block.update(cx, move |block, cx| {
                    block.selected_range = selected_range.clone();
                    block.selection_reversed = snapshot.reversed;
                    block.marked_range = None;
                    block.vertical_motion_x = None;
                    block.cursor_blink_epoch = Instant::now();
                    cx.notify();
                });
                self.pending_focus = Some(block.entity_id());
                self.active_entity_id = Some(block.entity_id());
            }
            ViewMode::Rendered => {
                if self.apply_cross_block_selection_snapshot_if_possible(snapshot, cx) {
                    return;
                }

                let mappings = self.build_source_target_mappings(cx);
                let exact_mapping = mappings.iter().find(|mapping| {
                    let contains_start = Self::source_range_contains(
                        &mapping.full_source_range,
                        snapshot.range.start,
                    );
                    let contains_end =
                        Self::source_range_contains(&mapping.full_source_range, snapshot.range.end);
                    if !contains_start || !contains_end {
                        return false;
                    }
                    let local_start = snapshot
                        .range
                        .start
                        .saturating_sub(mapping.full_source_range.start);
                    let local_end = snapshot
                        .range
                        .end
                        .saturating_sub(mapping.full_source_range.start);
                    let content_start = mapping.source_to_content
                        [local_start.min(mapping.source_to_content.len().saturating_sub(1))];
                    let content_end = mapping.source_to_content
                        [local_end.min(mapping.source_to_content.len().saturating_sub(1))];
                    let max_content = mapping.content_to_source.len().saturating_sub(1);
                    mapping.content_to_source[content_start.min(max_content)] == local_start
                        && mapping.content_to_source[content_end.min(max_content)] == local_end
                });

                if let Some(mapping) = exact_mapping {
                    let local_start = snapshot.range.start - mapping.full_source_range.start;
                    let local_end = snapshot.range.end - mapping.full_source_range.start;
                    let content_start = mapping.source_to_content[local_start];
                    let content_end = mapping.source_to_content[local_end];
                    let selected_range = mapping
                        .entity
                        .read(cx)
                        .markdown_range_to_current_range(content_start..content_end);
                    mapping.entity.update(cx, move |block, cx| {
                        block.selected_range = selected_range.clone();
                        block.selection_reversed = snapshot.reversed;
                        block.marked_range = None;
                        block.vertical_motion_x = None;
                        block.cursor_blink_epoch = Instant::now();
                        cx.notify();
                    });
                    self.pending_focus = Some(mapping.entity.entity_id());
                    self.active_entity_id = Some(mapping.entity.entity_id());
                    return;
                }

                let caret_offset = snapshot.range.end;
                let best = mappings.iter().min_by_key(|mapping| {
                    Self::source_offset_distance(&mapping.full_source_range, caret_offset)
                });
                let Some(mapping) = best else {
                    self.pending_focus = self.first_focusable_entity_id(cx);
                    self.active_entity_id = self.pending_focus;
                    return;
                };
                let local_source = if caret_offset <= mapping.full_source_range.start {
                    0
                } else if caret_offset >= mapping.full_source_range.end {
                    mapping.full_source_range.len()
                } else {
                    caret_offset - mapping.full_source_range.start
                };
                let content_offset = mapping.source_to_content
                    [local_source.min(mapping.source_to_content.len().saturating_sub(1))];
                let current_offset = mapping
                    .entity
                    .read(cx)
                    .markdown_offset_to_current_offset(content_offset);
                mapping.entity.update(cx, move |block, cx| {
                    block.assign_collapsed_selection_offset(
                        current_offset,
                        crate::components::CollapsedCaretAffinity::Default,
                        None,
                    );
                    block.marked_range = None;
                    block.cursor_blink_epoch = Instant::now();
                    cx.notify();
                });
                self.pending_focus = Some(mapping.entity.entity_id());
                self.active_entity_id = Some(mapping.entity.entity_id());
            }
        }
    }

    pub(super) fn source_range_contains(range: &std::ops::Range<usize>, offset: usize) -> bool {
        if range.start == range.end {
            offset == range.start
        } else {
            offset >= range.start && offset <= range.end
        }
    }

    pub(super) fn source_offset_distance(range: &std::ops::Range<usize>, offset: usize) -> usize {
        if Self::source_range_contains(range, offset) {
            0
        } else if offset < range.start {
            range.start - offset
        } else {
            offset.saturating_sub(range.end)
        }
    }

    pub(super) fn restore_history_entry(&mut self, entry: &HistoryEntry, cx: &mut Context<Self>) {
        match self.view_mode {
            ViewMode::Rendered => {
                let mut roots = Self::build_root_blocks_from_markdown(cx, &entry.source_text);
                if roots.is_empty() {
                    roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
                }
                self.document.replace_roots(roots, cx);
                self.rebuild_table_runtimes(cx);
                self.rebuild_image_runtimes(cx);
            }
            ViewMode::Source => {
                let block = Self::new_block(cx, BlockRecord::paragraph(entry.source_text.clone()));
                block.update(cx, |block, _cx| block.set_source_document_mode());
                self.document.replace_roots(vec![block], cx);
                self.table_cells.clear();
            }
        }

        self.apply_selection_snapshot_in_current_mode(&entry.selection, cx);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
        self.refresh_stable_document_snapshot(cx);
    }

    pub(super) fn normalize_rendered_quote_structure(&mut self, cx: &mut Context<Self>) {
        if self.view_mode != ViewMode::Rendered {
            return;
        }

        let selection_snapshot = self.capture_source_selection_snapshot(cx);
        let source = self.document.markdown_text(cx);
        let mut roots = Self::build_root_blocks_from_markdown(cx, &source);
        if roots.is_empty() {
            roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
        }
        self.document.replace_roots(roots, cx);
        self.rebuild_table_runtimes(cx);
        self.rebuild_image_runtimes(cx);
        self.apply_selection_snapshot_in_current_mode(&selection_snapshot, cx);
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        self.last_scroll_viewport_size = None;
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo_history.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo_history.is_empty()
    }

    fn push_redo_entry(&mut self, cx: &App) {
        self.redo_history
            .push(self.capture_history_entry(UndoCaptureKind::NonCoalescible, cx));
        Self::trim_history_stack(&mut self.redo_history);
    }

    fn push_undo_entry(&mut self, cx: &App) {
        self.undo_history
            .push(self.capture_history_entry(UndoCaptureKind::NonCoalescible, cx));
        Self::trim_history_stack(&mut self.undo_history);
    }

    /// Drops oldest entries until the stack fits both the entry count and the
    /// total `source_text` byte budget. Always retains the newest entry.
    fn trim_history_stack(stack: &mut Vec<HistoryEntry>) {
        Self::trim_history_stack_with_limits(
            stack,
            Self::HISTORY_LIMIT,
            Self::HISTORY_BYTE_BUDGET,
        );
    }

    fn trim_history_stack_with_limits(
        stack: &mut Vec<HistoryEntry>,
        entry_limit: usize,
        byte_budget: usize,
    ) {
        if entry_limit == 0 {
            stack.clear();
            return;
        }
        if stack.len() > entry_limit {
            let overflow = stack.len() - entry_limit;
            stack.drain(0..overflow);
        }

        let mut total_bytes: usize = stack.iter().map(|entry| entry.source_text.len()).sum();
        while stack.len() > 1 && total_bytes > byte_budget {
            let removed = stack.remove(0);
            total_bytes = total_bytes.saturating_sub(removed.source_text.len());
        }
    }

    pub(super) fn undo_document(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.undo_history.pop() else {
            return;
        };

        self.pending_undo_capture = None;
        self.history_restore_in_progress = true;
        self.clear_cross_block_selection(cx);
        self.push_redo_entry(cx);
        self.restore_history_entry(&entry, cx);
        self.history_restore_in_progress = false;
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }

    pub(super) fn redo_document(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.redo_history.pop() else {
            return;
        };

        self.pending_undo_capture = None;
        self.history_restore_in_progress = true;
        self.clear_cross_block_selection(cx);
        self.push_undo_entry(cx);
        self.restore_history_entry(&entry, cx);
        self.history_restore_in_progress = false;
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod history_budget_tests {
    use super::super::{
        Editor, HistoryEntry, UndoCaptureKind, UndoSelectionSnapshot,
    };
    use std::time::Instant;

    fn entry(text: impl Into<String>) -> HistoryEntry {
        HistoryEntry {
            source_text: text.into(),
            selection: UndoSelectionSnapshot {
                range: 0..0,
                reversed: false,
            },
            timestamp: Instant::now(),
            kind: UndoCaptureKind::NonCoalescible,
        }
    }

    #[test]
    fn trim_drops_oldest_when_over_entry_limit() {
        let mut stack = vec![entry("a"), entry("b"), entry("c"), entry("d")];
        Editor::trim_history_stack_with_limits(&mut stack, 2, usize::MAX);
        assert_eq!(
            stack
                .iter()
                .map(|e| e.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "d"]
        );
    }

    #[test]
    fn trim_drops_oldest_when_over_byte_budget() {
        let mut stack = vec![entry("aaaa"), entry("bbbb"), entry("cc")];
        Editor::trim_history_stack_with_limits(&mut stack, 10, 6);
        assert_eq!(
            stack
                .iter()
                .map(|e| e.source_text.as_str())
                .collect::<Vec<_>>(),
            vec!["bbbb", "cc"]
        );
        assert!(stack.iter().map(|e| e.source_text.len()).sum::<usize>() <= 6);
    }

    #[test]
    fn trim_keeps_newest_entry_even_if_over_budget() {
        let mut stack = vec![entry("x".repeat(100))];
        Editor::trim_history_stack_with_limits(&mut stack, 10, 8);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].source_text.len(), 100);
    }
}
