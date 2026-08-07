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
            payload: HistoryPayload::Full {
                source_text: self.current_document_source(cx),
            },
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
            payload: HistoryPayload::Full {
                source_text: self.last_stable_source_text.clone(),
            },
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
            anchor: None,
        });
    }

    pub(super) fn prepare_undo_capture_from_stable_snapshot_for_block(
        &mut self,
        block: Option<&Entity<Block>>,
        kind: UndoCaptureKind,
        cx: &App,
    ) {
        if self.history_restore_in_progress || self.pending_undo_capture.is_some() {
            return;
        }

        let anchor = block.and_then(|entity| {
            if self.view_mode != ViewMode::Rendered {
                return None;
            }
            let block_id = entity.read(cx).record.id;
            if !entity.read(cx).children.is_empty() {
                return None;
            }
            if !Self::kind_supports_scoped_block_undo(&entity.read(cx).record.kind) {
                return None;
            }
            // Only record identity here. GPUI may deliver PrepareUndo after the
            // edit mutates the block, so before-markdown is recovered at finalize
            // from last_stable_source_text via document diff.
            Some(HistoryBlockAnchor {
                block_id,
                entity_id: entity.entity_id(),
            })
        });

        self.pending_undo_capture = Some(PendingUndoCapture {
            snapshot: self.capture_stable_history_entry(kind, cx),
            anchor,
        });
    }

    fn kind_supports_scoped_block_undo(kind: &BlockKind) -> bool {
        matches!(
            kind,
            BlockKind::Paragraph
                | BlockKind::Heading { .. }
                | BlockKind::Comment
                | BlockKind::RawMarkdown
        )
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

        let HistoryPayload::Full {
            source_text: before_source,
        } = &pending.snapshot.payload
        else {
            // Pending captures are always Full; keep the type exhaustive.
            self.refresh_stable_document_snapshot(cx);
            return;
        };
        let before_source = before_source.clone();

        let current_source = self.current_document_source(cx);
        if current_source == before_source {
            self.refresh_stable_document_snapshot(cx);
            return;
        }

        let payload = self.classify_history_payload(
            &before_source,
            &current_source,
            pending.anchor.as_ref(),
            cx,
        );
        let entry = HistoryEntry {
            payload,
            selection: pending.snapshot.selection,
            timestamp: pending.snapshot.timestamp,
            kind: pending.snapshot.kind,
        };

        let should_merge = matches!(entry.kind, UndoCaptureKind::CoalescibleText)
            && self.undo_history.last().is_some_and(|last| {
                matches!(last.kind, UndoCaptureKind::CoalescibleText)
                    && same_history_coalesce_target(last, &entry)
                    && entry
                        .timestamp
                        .saturating_duration_since(last.timestamp)
                        <= Self::HISTORY_COALESCE_WINDOW
            });
        if !should_merge {
            self.undo_history.push(entry);
            self.redo_history.clear();
            Self::trim_history_stack(&mut self.undo_history);
        }
        self.refresh_stable_document_snapshot(cx);
    }

    fn classify_history_payload(
        &self,
        before_source: &str,
        after_source: &str,
        anchor: Option<&HistoryBlockAnchor>,
        cx: &App,
    ) -> HistoryPayload {
        let Some(anchor) = anchor else {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        };
        if self.view_mode != ViewMode::Rendered {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        }

        let Some(entity) = self.document.block_entity_by_id(anchor.entity_id) else {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        };
        let live_id = entity.read(cx).record.id;
        if live_id != anchor.block_id
            || !entity.read(cx).children.is_empty()
            || !Self::kind_supports_scoped_block_undo(&entity.read(cx).record.kind)
        {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        }

        let after_md = self.document.block_markdown_text(&entity, cx);
        if after_md.is_empty() {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        }

        // Locate the edited block's after-markdown uniquely, then recover the
        // before slice from the same surrounding prefix/suffix in before_source.
        let mut after_matches = after_source.match_indices(&after_md);
        let Some((after_pos, _)) = after_matches.next() else {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        };
        if after_matches.next().is_some() {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        }

        let prefix = &after_source[..after_pos];
        let suffix = &after_source[after_pos + after_md.len()..];
        if !before_source.starts_with(prefix)
            || !before_source.ends_with(suffix)
            || before_source.len() < prefix.len() + suffix.len()
        {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        }
        let before_md =
            before_source[prefix.len()..before_source.len() - suffix.len()].to_string();
        let mut reconstructed =
            String::with_capacity(prefix.len() + after_md.len() + suffix.len());
        reconstructed.push_str(prefix);
        reconstructed.push_str(&after_md);
        reconstructed.push_str(suffix);
        if reconstructed != after_source {
            return HistoryPayload::Full {
                source_text: before_source.to_string(),
            };
        }

        HistoryPayload::BlockText {
            block_id: anchor.block_id,
            block_markdown: before_md,
        }
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
        match &entry.payload {
            HistoryPayload::BlockText {
                block_id,
                block_markdown,
            } => {
                if self.try_restore_block_text_history(*block_id, block_markdown, cx) {
                    self.apply_selection_snapshot_in_current_mode(&entry.selection, cx);
                    self.pending_scroll_active_block_into_view = true;
                    self.pending_scroll_recheck_after_layout = true;
                    self.refresh_stable_document_snapshot(cx);
                    return;
                }
                // Scoped restore failed (entity gone / kind changed) — no full
                // document is stored on BlockText entries, so rebuild from the
                // current document with the block slice swapped is impossible.
                // Fall through by synthesizing a full snapshot from last_stable
                // is also wrong. Best effort: keep document and still apply selection.
                self.apply_selection_snapshot_in_current_mode(&entry.selection, cx);
                self.refresh_stable_document_snapshot(cx);
            }
            HistoryPayload::Full { source_text } => {
                match self.view_mode {
                    ViewMode::Rendered => {
                        let mut roots = Self::build_root_blocks_from_markdown(cx, source_text);
                        if roots.is_empty() {
                            roots.push(Self::new_block(cx, BlockRecord::paragraph(String::new())));
                        }
                        self.document.replace_roots(roots, cx);
                        self.rebuild_table_runtimes(cx);
                        self.rebuild_image_runtimes(cx);
                    }
                    ViewMode::Source => {
                        let block =
                            Self::new_block(cx, BlockRecord::paragraph(source_text.clone()));
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
        }
    }

    fn try_restore_block_text_history(
        &mut self,
        block_id: uuid::Uuid,
        block_markdown: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.view_mode != ViewMode::Rendered {
            return false;
        }
        let Some(entity) = self.document.block_entity_by_record_id(block_id, cx) else {
            return false;
        };
        if !entity.read(cx).children.is_empty() {
            return false;
        }
        if !Self::kind_supports_scoped_block_undo(&entity.read(cx).record.kind) {
            return false;
        }

        // Parse the stored slice as a tiny document and copy title/kind back.
        let mut roots = Self::build_root_blocks_from_markdown(cx, block_markdown);
        if roots.len() != 1 {
            return false;
        }
        let temp = roots.pop().expect("checked len");
        if !temp.read(cx).children.is_empty() {
            return false;
        }
        let (kind, title, raw_fallback) = temp.read_with(cx, |block, _cx| {
            (
                block.record.kind.clone(),
                block.record.title.clone(),
                block.record.raw_fallback.clone(),
            )
        });
        if !Self::kind_supports_scoped_block_undo(&kind) {
            return false;
        }

        entity.update(cx, |block, cx| {
            block.record.kind = kind;
            block.record.set_title(title);
            block.record.raw_fallback = raw_fallback;
            block.sync_edit_mode_from_kind();
            block.sync_render_cache();
            block.clear_shape_cache();
            cx.notify();
        });
        true
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

    fn push_redo_entry(&mut self, undone: &HistoryEntry, cx: &App) {
        self.redo_history
            .push(self.capture_inverse_history_entry(undone, cx));
        Self::trim_history_stack(&mut self.redo_history);
    }

    fn push_undo_entry(&mut self, redone: &HistoryEntry, cx: &App) {
        self.undo_history
            .push(self.capture_inverse_history_entry(redone, cx));
        Self::trim_history_stack(&mut self.undo_history);
    }

    /// Capture the live state that redo/undo should return to after applying `peer`.
    fn capture_inverse_history_entry(&self, peer: &HistoryEntry, cx: &App) -> HistoryEntry {
        if let Some(block_id) = peer.payload.block_id()
            && self.view_mode == ViewMode::Rendered
            && let Some(entity) = self.document.block_entity_by_record_id(block_id, cx)
            && entity.read(cx).children.is_empty()
            && Self::kind_supports_scoped_block_undo(&entity.read(cx).record.kind)
        {
            let block_markdown = self.document.block_markdown_text(&entity, cx);
            if !block_markdown.is_empty() {
                return HistoryEntry {
                    payload: HistoryPayload::BlockText {
                        block_id,
                        block_markdown,
                    },
                    selection: self.capture_source_selection_snapshot(cx),
                    timestamp: Instant::now(),
                    kind: UndoCaptureKind::NonCoalescible,
                };
            }
        }
        self.capture_history_entry(UndoCaptureKind::NonCoalescible, cx)
    }

    /// Drops oldest entries until the stack fits both the entry count and the
    /// retained-bytes budget. Always retains the newest entry.
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

        let mut total_bytes: usize = stack.iter().map(HistoryEntry::retained_bytes).sum();
        while stack.len() > 1 && total_bytes > byte_budget {
            let removed = stack.remove(0);
            total_bytes = total_bytes.saturating_sub(removed.retained_bytes());
        }
    }

    pub(super) fn undo_document(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.undo_history.pop() else {
            return;
        };

        self.pending_undo_capture = None;
        self.history_restore_in_progress = true;
        self.clear_cross_block_selection(cx);
        self.push_redo_entry(&entry, cx);
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
        self.push_undo_entry(&entry, cx);
        self.restore_history_entry(&entry, cx);
        self.history_restore_in_progress = false;
        self.mark_dirty(cx);
        self.sync_table_axis_visuals(cx);
        self.dismiss_contextual_overlays(cx);
        cx.notify();
    }
}

fn same_history_coalesce_target(left: &HistoryEntry, right: &HistoryEntry) -> bool {
    match (&left.payload, &right.payload) {
        (
            HistoryPayload::BlockText {
                block_id: left_id, ..
            },
            HistoryPayload::BlockText {
                block_id: right_id, ..
            },
        ) => left_id == right_id,
        (HistoryPayload::Full { .. }, HistoryPayload::Full { .. }) => true,
        _ => false,
    }
}

#[cfg(test)]
mod history_budget_tests {
    use super::super::{
        Editor, HistoryEntry, HistoryPayload, UndoCaptureKind, UndoSelectionSnapshot,
    };
    use std::time::Instant;

    fn entry(text: impl Into<String>) -> HistoryEntry {
        HistoryEntry {
            payload: HistoryPayload::Full {
                source_text: text.into(),
            },
            selection: UndoSelectionSnapshot {
                range: 0..0,
                reversed: false,
            },
            timestamp: Instant::now(),
            kind: UndoCaptureKind::NonCoalescible,
        }
    }

    fn block_entry(markdown: impl Into<String>) -> HistoryEntry {
        HistoryEntry {
            payload: HistoryPayload::BlockText {
                block_id: uuid::Uuid::nil(),
                block_markdown: markdown.into(),
            },
            selection: UndoSelectionSnapshot {
                range: 0..0,
                reversed: false,
            },
            timestamp: Instant::now(),
            kind: UndoCaptureKind::CoalescibleText,
        }
    }

    fn payload_text(entry: &HistoryEntry) -> &str {
        match &entry.payload {
            HistoryPayload::Full { source_text } => source_text.as_str(),
            HistoryPayload::BlockText { block_markdown, .. } => block_markdown.as_str(),
        }
    }

    #[test]
    fn trim_drops_oldest_when_over_entry_limit() {
        let mut stack = vec![entry("a"), entry("b"), entry("c"), entry("d")];
        Editor::trim_history_stack_with_limits(&mut stack, 2, usize::MAX);
        assert_eq!(
            stack.iter().map(payload_text).collect::<Vec<_>>(),
            vec!["c", "d"]
        );
    }

    #[test]
    fn trim_drops_oldest_when_over_byte_budget() {
        let mut stack = vec![entry("aaaa"), entry("bbbb"), entry("cc")];
        Editor::trim_history_stack_with_limits(&mut stack, 10, 6);
        assert_eq!(
            stack.iter().map(payload_text).collect::<Vec<_>>(),
            vec!["bbbb", "cc"]
        );
        assert!(stack.iter().map(HistoryEntry::retained_bytes).sum::<usize>() <= 6);
    }

    #[test]
    fn trim_keeps_newest_entry_even_if_over_budget() {
        let mut stack = vec![entry("x".repeat(100))];
        Editor::trim_history_stack_with_limits(&mut stack, 10, 8);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].retained_bytes(), 100);
    }

    #[test]
    fn trim_counts_block_text_bytes() {
        let mut stack = vec![block_entry("aaaa"), block_entry("bbbb"), block_entry("cc")];
        Editor::trim_history_stack_with_limits(&mut stack, 10, 6);
        assert_eq!(stack.len(), 2);
        assert!(stack.iter().map(HistoryEntry::retained_bytes).sum::<usize>() <= 6);
    }
}
