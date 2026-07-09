//! Persist and restore the last edited file, scroll offset, and caret position.

use gpui::*;

use super::{Editor, UndoSelectionSnapshot, ViewMode};
use crate::config::session::EditorSessionState;

impl Editor {
    pub(crate) fn capture_session_state(&self, cx: &App) -> Option<EditorSessionState> {
        let file_path = self.file_path.clone()?;
        if !file_path.is_file() {
            return None;
        }

        let selection = self.capture_source_selection_snapshot(cx);
        let scroll_y = (-f32::from(self.scroll_handle.offset().y)).max(0.0);

        Some(EditorSessionState {
            file_path,
            scroll_offset_y: scroll_y,
            selection_start: selection.range.start,
            selection_end: selection.range.end,
            selection_reversed: selection.reversed,
            view_mode: match self.view_mode {
                ViewMode::Rendered => "rendered".into(),
                ViewMode::Source => "source".into(),
            },
        })
    }

    pub(crate) fn persist_session_state(&self, cx: &App) {
        if let Some(state) = self.capture_session_state(cx) {
            if let Err(err) = crate::config::session::record_editor_session_state(&state) {
                eprintln!("failed to persist editor session: {err}");
            }
        }
    }

    pub(crate) fn restore_session_state(
        &mut self,
        session: &EditorSessionState,
        cx: &mut Context<Self>,
    ) {
        let want_source = session.view_mode == "source";
        if want_source != (self.view_mode == ViewMode::Source) {
            self.toggle_view_mode(cx);
        }

        let snapshot = UndoSelectionSnapshot {
            range: session.selection_start..session.selection_end,
            reversed: session.selection_reversed,
        };
        self.apply_selection_snapshot_in_current_mode(&snapshot, cx);
        self.pending_scroll_active_block_into_view = false;
        self.pending_session_scroll_y = Some(session.scroll_offset_y.max(0.0));
        self.session_scroll_restore_pending = true;
        self.pending_scroll_recheck_after_layout = true;
        self.refresh_stable_document_snapshot(cx);
        cx.notify();
    }

    pub(super) fn apply_pending_session_scroll(&mut self, cx: &mut Context<Self>) {
        if !self.session_scroll_restore_pending {
            return;
        }

        let Some(scroll_y) = self.pending_session_scroll_y else {
            self.session_scroll_restore_pending = false;
            return;
        };

        let max_scroll_y = f32::from(self.scroll_handle.max_offset().height.max(px(0.0)));
        if max_scroll_y <= 0.0 {
            self.pending_scroll_recheck_after_layout = true;
            cx.notify();
            return;
        }

        let clamped = scroll_y.clamp(0.0, max_scroll_y);
        let mut offset = self.scroll_handle.offset();
        offset.y = px(-clamped);
        self.scroll_handle.set_offset(offset);
        self.pending_session_scroll_y = None;
        self.session_scroll_restore_pending = false;
    }
}
