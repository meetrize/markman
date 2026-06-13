//! Native table runtime installation and table-editing operations.

use super::*;
use crate::components::{
    ColumnMarkdownSegment, parse_columns_markdown, split_column_markdown_segments,
    update_columns_host_table_markdown,
};

impl Editor {
    pub(crate) fn new_table_block(cx: &mut Context<Self>, table: TableData) -> Entity<Block> {
        Self::new_block(cx, BlockRecord::table(table))
    }

    pub(super) fn install_table_runtime_for_block(
        &mut self,
        table_block: &Entity<Block>,
        table: &TableData,
        cx: &mut Context<Self>,
    ) {
        let columns = table.column_count();
        let rows = 1 + table.rows.len();
        let extent = (columns, rows);

        let header = table
            .header
            .iter()
            .cloned()
            .enumerate()
            .map(|(column, title)| {
                let alignment = table
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableColumnAlignment::Left);
                let position = TableCellPosition { row: 0, column };
                let cell = Self::new_table_cell_block(cx, title, position, alignment, extent);
                self.table_cells.insert(
                    cell.entity_id(),
                    TableCellBinding {
                        table_block: table_block.clone(),
                        cell: cell.clone(),
                        position,
                    },
                );
                cell
            })
            .collect::<Vec<_>>();

        let rows = table
            .rows
            .iter()
            .cloned()
            .enumerate()
            .map(|(body_row_index, row)| {
                row.into_iter()
                    .enumerate()
                    .map(|(column, title)| {
                        let alignment = table
                            .alignments
                            .get(column)
                            .copied()
                            .unwrap_or(TableColumnAlignment::Left);
                        let position = TableCellPosition {
                            row: body_row_index + 1,
                            column,
                        };
                        let cell =
                            Self::new_table_cell_block(cx, title, position, alignment, extent);
                        self.table_cells.insert(
                            cell.entity_id(),
                            TableCellBinding {
                                table_block: table_block.clone(),
                                cell: cell.clone(),
                                position,
                            },
                        );
                        cell
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        table_block.update(cx, {
            let runtime = TableRuntime { header, rows };
            move |block, _cx| block.set_table_runtime(runtime.clone())
        });
    }

    pub(super) fn rebuild_table_runtimes(&mut self, cx: &mut Context<Self>) {
        self.table_cells.clear();
        self.table_axis_preview = None;
        let visible = self.document.visible_blocks().to_vec();
        for block in &visible {
            block
                .entity
                .update(cx, |block, _cx| block.clear_table_runtime());
        }
        for visible in visible {
            let Some(table) = visible.entity.read(cx).record.table.clone() else {
                continue;
            };
            if visible.entity.read(cx).kind() == BlockKind::Table {
                self.install_table_runtime_for_block(&visible.entity, &table, cx);
            }
        }
        self.rebuild_column_embedded_tables(cx);
        self.rebuild_image_runtimes(cx);
        self.sync_table_axis_visuals(cx);
    }

    pub(super) fn rebuild_column_embedded_tables(&mut self, cx: &mut Context<Self>) {
        let visible = self.document.visible_blocks().to_vec();
        for visible in visible {
            let host = visible.entity.clone();
            if !host.read(cx).is_columns_raw_markdown() || host.read(cx).columns_source_edit {
                continue;
            }

            let Some(columns) = parse_columns_markdown(host.read(cx).display_text()) else {
                host.update(cx, |block, _cx| block.column_embedded_tables.clear());
                continue;
            };

            let host_id = host.read(cx).record.id.clone();
            let mut expected_keys = std::collections::HashSet::new();

            for (column_index, column) in columns.iter().enumerate() {
                let segments = split_column_markdown_segments(&column.markdown);
                for (segment_index, segment) in segments.iter().enumerate() {
                    let ColumnMarkdownSegment::Table(table) = segment else {
                        continue;
                    };
                    let key = format!("{host_id}-{column_index}-{segment_index}");
                    expected_keys.insert(key.clone());

                    let table_block = if let Some(existing) =
                        host.read(cx).column_embedded_tables.get(&key).cloned()
                    {
                        existing
                    } else {
                        let table_block = Self::new_block(cx, BlockRecord::table(table.clone()));
                        table_block.update(cx, |block, _cx| {
                            block.embedded_column_table = true;
                            block.column_table_host = Some(host.clone());
                            block.column_table_host_column_index = column_index;
                            block.column_table_segment_index = segment_index;
                        });
                        host.update(cx, |block, _cx| {
                            block.column_embedded_tables
                                .insert(key.clone(), table_block.clone());
                        });
                        table_block
                    };

                    let stored = table_block.read(cx).record.table.clone();
                    if stored.as_ref() != Some(table) {
                        table_block.update(cx, |block, _cx| {
                            block.record.table = Some(table.clone());
                        });
                        self.install_table_runtime_for_block(&table_block, table, cx);
                    } else if table_block.read(cx).table_runtime.is_none() {
                        self.install_table_runtime_for_block(&table_block, table, cx);
                    }
                    self.sync_runtime_context_for_block(
                        &table_block,
                        self.image_base_dir().as_deref(),
                        cx,
                    );
                }
            }

            host.update(cx, |block, _cx| {
                block
                    .column_embedded_tables
                    .retain(|key, _| expected_keys.contains(key));
            });
        }
    }

    pub(super) fn sync_column_embedded_table_to_host(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        let (host, column_index, segment_index, table) = table_block.read_with(cx, |block, _cx| {
            (
                block.column_table_host.clone(),
                block.column_table_host_column_index,
                block.column_table_segment_index,
                block.record.table.clone(),
            )
        });
        let Some(host) = host else {
            return;
        };
        let Some(table) = table else {
            return;
        };
        let host_markdown = host.read(cx).display_text().to_string();
        let Some(new_markdown) = update_columns_host_table_markdown(
            &host_markdown,
            column_index,
            segment_index,
            &table,
        ) else {
            return;
        };
        if new_markdown == host_markdown {
            return;
        }
        host.update(cx, |host_block, cx| {
            host_block.record.title = InlineTextTree::plain(&new_markdown);
            host_block.record.raw_fallback = Some(new_markdown);
            host_block.sync_render_cache();
            cx.notify();
        });
        self.mark_dirty(cx);
    }

    pub(super) fn sync_table_record_from_runtime(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
            return;
        };
        let (alignments, column_width_fractions) = table_block
            .read(cx)
            .record
            .table
            .as_ref()
            .map(|table| {
                (
                    table.alignments.clone(),
                    table.column_width_fractions.clone(),
                )
            })
            .unwrap_or_default();
        let header = runtime
            .header
            .iter()
            .map(|cell| cell.read(cx).record.title.clone())
            .collect::<Vec<_>>();
        let rows = runtime
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.read(cx).record.title.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(TableData {
                header,
                rows,
                alignments,
                column_width_fractions,
            });
        });
        if table_block.read(cx).embedded_column_table {
            self.sync_column_embedded_table_to_host(table_block, cx);
        }
    }

    pub(crate) fn start_table_column_resize_drag(
        &mut self,
        table_block: &Entity<Block>,
        boundary_index: usize,
        pointer_x: f32,
        table_width: f32,
        start_fractions: Vec<f32>,
        cx: &mut Context<Self>,
    ) {
        let Some(table) = table_block.read(cx).record.table.as_ref() else {
            return;
        };
        let columns = table.column_count();
        if columns <= 1
            || boundary_index + 1 >= columns
            || start_fractions.len() != columns
        {
            return;
        }

        let theme = cx.global::<ThemeManager>().current_arc();
        table_block.update(cx, |block, _cx| {
            if let Some(table) = block.record.table.as_mut() {
                table.set_column_width_fractions(start_fractions.clone());
            }
        });
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.table_column_resize_drag = Some(super::TableColumnResizeDragSession {
            table_block: table_block.clone(),
            boundary_index,
            start_pointer_x: pointer_x,
            start_fractions,
            table_width: table_width.max(1.0),
            min_column_width: minimum_table_column_width(&theme),
        });
        cx.notify();
    }

    pub(crate) fn update_table_column_resize_drag(
        &mut self,
        pointer_x: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.table_column_resize_drag.clone() else {
            return;
        };
        let left = drag.boundary_index;
        let right = left + 1;
        let min_fraction = (drag.min_column_width / drag.table_width).clamp(0.05, 0.95);
        let combined = drag.start_fractions[left] + drag.start_fractions[right];
        let delta_fraction = (pointer_x - drag.start_pointer_x) / drag.table_width;
        let next_left =
            (drag.start_fractions[left] + delta_fraction).clamp(min_fraction, combined - min_fraction);
        let next_right = combined - next_left;

        let mut fractions = drag.start_fractions.clone();
        fractions[left] = next_left;
        fractions[right] = next_right;

        drag.table_block.update(cx, |block, _cx| {
            if let Some(table) = block.record.table.as_mut() {
                table.set_column_width_fractions(fractions);
            }
        });
        cx.notify();
    }

    pub(crate) fn end_table_column_resize_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.table_column_resize_drag.take() else {
            return;
        };
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        let _ = drag;
        cx.notify();
    }

    pub(super) fn append_table_column(
        &mut self,
        table_block: &Entity<Block>,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);

        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        let alignment = table
            .alignments
            .last()
            .copied()
            .unwrap_or(TableColumnAlignment::Left);
        table.append_column(alignment);

        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        if let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.header.last())
        {
            self.focus_block(cell.entity_id());
        }
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn append_table_row(&mut self, table_block: &Entity<Block>, cx: &mut Context<Self>) {
        self.sync_table_record_from_runtime(table_block, cx);

        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.append_row();

        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        if let Some(cell) = table_block
            .read(cx)
            .table_runtime
            .as_ref()
            .and_then(|runtime| runtime.rows.last())
            .and_then(|row| row.first())
        {
            self.focus_block(cell.entity_id());
        }
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn preview_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let preview = index.map(|index| TableAxisSelection {
            table_block_id,
            kind,
            index,
        });
        self.set_table_axis_preview(preview, cx);
    }

    pub(super) fn select_table_axis(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let selection = TableAxisSelection {
            table_block_id,
            kind,
            index,
        };
        self.set_table_axis_preview(Some(selection), cx);
        self.set_table_axis_selection(Some(selection), cx);
    }

    pub(super) fn open_table_axis_menu(
        &mut self,
        table_block_id: EntityId,
        kind: TableAxisKind,
        index: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.select_table_axis(table_block_id, kind, index, cx);
        if let Some(selection) = self.table_axis_selection {
            self.open_table_axis_context_menu(position, selection, cx);
        }
    }

    pub(super) fn set_table_column_alignment(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        alignment: TableColumnAlignment,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.set_column_alignment(column, alignment);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Column,
            index: column,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(table_block, TableCellPosition { row: 0, column }, cx);
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn move_table_row(
        &mut self,
        table_block: &Entity<Block>,
        row_index: usize,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let next_row = if delta < 0 {
            row_index.checked_sub(delta.unsigned_abs() as usize)
        } else {
            row_index.checked_add(delta as usize)
        };
        let Some(next_row) = next_row else {
            return;
        };
        if next_row >= table.rows.len() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.swap_body_rows(row_index, next_row);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Row,
            index: next_row,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: next_row + 1,
                column: 0,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn move_table_column(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        delta: i32,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        let next_column = if delta < 0 {
            column.checked_sub(delta.unsigned_abs() as usize)
        } else {
            column.checked_add(delta as usize)
        };
        let Some(next_column) = next_column else {
            return;
        };
        if next_column >= table.column_count() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.swap_columns(column, next_column);
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Column,
            index: next_column,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: next_column,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn delete_table_row(
        &mut self,
        table_block: &Entity<Block>,
        row_index: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        if table.rows.len() <= 1 || row_index >= table.rows.len() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_body_row(row_index);
        let focus_row = row_index.min(table.rows.len().saturating_sub(1));
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Row,
            index: focus_row,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: focus_row + 1,
                column: 0,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn delete_table_column(
        &mut self,
        table_block: &Entity<Block>,
        column: usize,
        cx: &mut Context<Self>,
    ) {
        self.sync_table_record_from_runtime(table_block, cx);
        let Some(mut table) = table_block.read(cx).record.table.clone() else {
            return;
        };
        if table.column_count() <= 1 || column >= table.column_count() {
            return;
        }
        let started_local_capture = if self.pending_undo_capture.is_none() {
            self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
            true
        } else {
            false
        };
        table.remove_column(column);
        let focus_column = column.min(table.column_count().saturating_sub(1));
        table_block.update(cx, move |block, _cx| {
            block.record.table = Some(table.clone());
        });
        self.rebuild_table_runtimes(cx);
        let selection = TableAxisSelection {
            table_block_id: table_block.entity_id(),
            kind: TableAxisKind::Column,
            index: focus_column,
        };
        self.set_table_axis_selection(Some(selection), cx);
        self.focus_table_cell_position(
            table_block,
            TableCellPosition {
                row: 0,
                column: focus_column,
            },
            cx,
        );
        self.mark_dirty(cx);
        self.request_active_block_scroll_into_view(cx);
        if started_local_capture {
            self.finalize_pending_undo_capture(cx);
        }
        cx.notify();
    }

    pub(super) fn table_axis_marker(selection: TableAxisSelection) -> TableAxisMarker {
        TableAxisMarker {
            kind: selection.kind,
            index: selection.index,
        }
    }

    pub(super) fn clear_table_axis_preview(&mut self, cx: &mut Context<Self>) {
        if self.table_axis_preview.take().is_some() {
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn clear_table_axis_selection(&mut self, cx: &mut Context<Self>) {
        if self.table_axis_selection.take().is_some() {
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn set_table_axis_preview(
        &mut self,
        preview: Option<TableAxisSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.table_axis_preview != preview {
            self.table_axis_preview = preview;
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn set_table_axis_selection(
        &mut self,
        selection: Option<TableAxisSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.table_axis_selection != selection {
            self.table_axis_selection = selection;
            self.sync_table_axis_visuals(cx);
        }
    }

    pub(super) fn table_axis_selection_valid(
        &self,
        selection: TableAxisSelection,
        cx: &App,
    ) -> bool {
        let Some(table_block) = self.table_block_by_id(selection.table_block_id, cx) else {
            return false;
        };
        let Some(runtime) = table_block.read(cx).table_runtime.as_ref() else {
            return false;
        };
        match selection.kind {
            TableAxisKind::Column => selection.index < runtime.header.len(),
            TableAxisKind::Row => selection.index < runtime.rows.len(),
        }
    }

    pub(super) fn normalize_table_axis_state(&mut self, cx: &mut Context<Self>) {
        if let Some(selection) = self.table_axis_selection
            && !self.table_axis_selection_valid(selection, cx)
        {
            self.table_axis_selection = None;
        }
        if let Some(preview) = self.table_axis_preview
            && !self.table_axis_selection_valid(preview, cx)
        {
            self.table_axis_preview = None;
        }
    }

    pub(super) fn sync_table_axis_visuals(&mut self, cx: &mut Context<Self>) {
        self.normalize_table_axis_state(cx);

        let visible_tables = self
            .document
            .flatten_visible_blocks()
            .into_iter()
            .filter(|visible| visible.entity.read(cx).kind() == BlockKind::Table)
            .map(|visible| visible.entity)
            .collect::<Vec<_>>();
        let embedded_tables = self
            .document
            .flatten_visible_blocks()
            .into_iter()
            .filter(|visible| visible.entity.read(cx).is_columns_raw_markdown())
            .flat_map(|visible| {
                visible
                    .entity
                    .read(cx)
                    .column_embedded_tables
                    .values()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let table_blocks = visible_tables
            .into_iter()
            .chain(embedded_tables)
            .collect::<Vec<_>>();

        for table_block in &table_blocks {
            let block_id = table_block.entity_id();
            let preview_marker = self
                .table_axis_preview
                .filter(|selection| selection.table_block_id == block_id)
                .map(Self::table_axis_marker);
            let selected_marker = self
                .table_axis_selection
                .filter(|selection| selection.table_block_id == block_id)
                .map(Self::table_axis_marker);

            table_block.update(cx, move |block, cx| {
                block.set_table_axis_visual_state(preview_marker, selected_marker);
                cx.notify();
            });

            let Some(runtime) = table_block.read(cx).table_runtime.clone() else {
                continue;
            };

            let selected = self
                .table_axis_selection
                .filter(|selection| selection.table_block_id == block_id);
            let preview = self
                .table_axis_preview
                .filter(|selection| selection.table_block_id == block_id);

            let mut apply_highlight = |cell: &Entity<Block>, row: usize, column: usize| {
                let highlight = if selected.is_some_and(|selection| match selection.kind {
                    TableAxisKind::Column => selection.index == column,
                    TableAxisKind::Row => selection.index == row.saturating_sub(1),
                }) {
                    TableAxisHighlight::Selected
                } else if preview.is_some_and(|selection| match selection.kind {
                    TableAxisKind::Column => selection.index == column,
                    TableAxisKind::Row => selection.index == row.saturating_sub(1),
                }) {
                    TableAxisHighlight::Preview
                } else {
                    TableAxisHighlight::None
                };

                cell.update(cx, move |block, cx| {
                    block.set_table_axis_highlight(highlight);
                    cx.notify();
                });
            };

            for (column, cell) in runtime.header.iter().enumerate() {
                apply_highlight(cell, 0, column);
            }
            for (body_row_index, row) in runtime.rows.iter().enumerate() {
                for (column, cell) in row.iter().enumerate() {
                    apply_highlight(cell, body_row_index + 1, column);
                }
            }
        }
    }
}
