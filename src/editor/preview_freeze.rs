//! Reduce preview repaint work while the UI is in a high-motion or
//! graph-physics state: suspend cursor blink and skip non-essential
//! overlay sync (e.g. code-run visuals).
//!
//! Corresponds to perf-memory-plan P2.6 (extends the original graph-tab freeze).

use std::time::Duration;

use gpui::*;

use crate::layout::set_preview_motion_freeze;

use super::{Editor, ViewMode};

/// After the last scroll event, wait this long before resuming blink / overlays.
pub(crate) const SCROLL_FREEZE_SETTLE_MS: u64 = 100;

impl Editor {
    /// Graph tab: keep the document preview quiet while physics runs.
    pub(super) fn should_freeze_preview_for_graph(&self) -> bool {
        self.view_mode == ViewMode::Rendered
            && !self.document_dirty
            && self.workspace_graph_tab_active()
            && self.knowledge_graph_view.is_some()
    }

    /// Resize / scroll / drag: pause blink so motion frames are not fighting timers.
    pub(super) fn should_freeze_preview_for_motion(&self) -> bool {
        self.resize_throttle_active
            || self.scroll_freeze_active
            || self.scrollbar_drag.is_some()
            || self.table_column_resize_drag.is_some()
            || self.workspace.resize_drag.is_some()
    }

    pub(super) fn should_freeze_preview(&self) -> bool {
        self.should_freeze_preview_for_graph() || self.should_freeze_preview_for_motion()
    }

    pub(super) fn invalidate_preview_content_cache(&mut self) {
        self.preview_freeze_armed = false;
    }

    pub(super) fn suspend_preview_cursor_blink(&self, cx: &mut Context<Self>) {
        for visible in self.document.visible_blocks() {
            let _ = visible.entity.update(cx, |block, _cx| {
                block.suspend_cursor_blink();
            });
        }
    }

    /// Keep freeze armed while `should_freeze_preview`, and clear it when settled.
    pub(super) fn sync_preview_freeze(&mut self, cx: &mut Context<Self>) {
        let freeze = self.should_freeze_preview();
        set_preview_motion_freeze(freeze);
        if freeze {
            if !self.preview_freeze_armed {
                self.suspend_preview_cursor_blink(cx);
                self.preview_freeze_armed = true;
            }
        } else if self.preview_freeze_armed {
            self.preview_freeze_armed = false;
        }
    }

    /// Arm a short-lived freeze after scroll-wheel / programmatic scroll activity.
    pub(super) fn note_scroll_freeze_activity(&mut self, cx: &mut Context<Self>) {
        if !self.scroll_freeze_active {
            self.scroll_freeze_active = true;
            set_preview_motion_freeze(true);
            self.suspend_preview_cursor_blink(cx);
            self.preview_freeze_armed = true;
        }

        let weak = cx.entity().downgrade();
        self.scroll_freeze_settle_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(SCROLL_FREEZE_SETTLE_MS))
                    .await;
                let _ = weak.update(cx, |editor, cx| {
                    editor.scroll_freeze_settle_task = None;
                    if editor.scroll_freeze_active {
                        editor.scroll_freeze_active = false;
                        // Recompute process flag from remaining freeze reasons.
                        let freeze = editor.should_freeze_preview();
                        set_preview_motion_freeze(freeze);
                        if !freeze {
                            editor.preview_freeze_armed = false;
                        }
                        cx.notify();
                    }
                });
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scroll_settle_is_positive() {
        assert!(super::SCROLL_FREEZE_SETTLE_MS > 0);
    }

    #[test]
    fn motion_freeze_flag_round_trips() {
        crate::layout::set_preview_motion_freeze(false);
        assert!(!crate::layout::preview_motion_freeze_active());
        crate::layout::set_preview_motion_freeze(true);
        assert!(crate::layout::preview_motion_freeze_active());
        crate::layout::set_preview_motion_freeze(false);
    }
}
