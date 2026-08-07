//! Throttle layout work while the editor viewport width is changing
//! (window drag-resize). Coarse wrap quantization + reduced overscan
//! apply until the width settles, then a full reflow runs.
//!
//! Corresponds to perf-memory-plan P2.4.

use std::time::Duration;

use gpui::*;

use crate::layout::set_resize_throttle_active;

use super::Editor;

/// After the last width change, wait this long before fine reflow.
pub(crate) const RESIZE_SETTLE_MS: u64 = 120;
/// Overscan while width is actively changing (vs [`super::virtualization::OVERSCAN_ROWS`]).
pub(crate) const OVERSCAN_ROWS_WHILE_RESIZING: usize = 2;

impl Editor {
    pub(super) fn virtualization_overscan_rows(&self) -> usize {
        if self.resize_throttle_active {
            OVERSCAN_ROWS_WHILE_RESIZING
        } else {
            super::virtualization::OVERSCAN_ROWS
        }
    }

    /// Detect continuous width changes and arm throttle until settle.
    pub(super) fn sync_resize_throttle(
        &mut self,
        viewport_size: Size<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let width = f32::from(viewport_size.width);
        let width_changed = match self.last_layout_viewport_width {
            Some(previous) => (previous - width).abs() > 0.5,
            None => false,
        };
        self.last_layout_viewport_width = Some(width);

        if !width_changed {
            return;
        }

        // Short docs are cheap; keep exact wrap. Long / virtualized docs benefit most.
        if !super::virtualization::should_virtualize(self.document.visible_blocks().len()) {
            if self.resize_throttle_active {
                self.clear_resize_throttle(cx);
            }
            return;
        }

        self.arm_resize_throttle(cx);
    }

    fn arm_resize_throttle(&mut self, cx: &mut Context<Self>) {
        if !self.resize_throttle_active {
            self.resize_throttle_active = true;
            set_resize_throttle_active(true);
        }

        let weak = cx.entity().downgrade();
        self.resize_settle_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(RESIZE_SETTLE_MS))
                    .await;
                let _ = weak.update(cx, |editor, cx| {
                    editor.clear_resize_throttle(cx);
                    cx.notify();
                });
            },
        ));
    }

    fn clear_resize_throttle(&mut self, _cx: &mut Context<Self>) {
        self.resize_settle_task = None;
        if self.resize_throttle_active {
            self.resize_throttle_active = false;
            set_resize_throttle_active(false);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn settle_constant_is_positive() {
        assert!(super::RESIZE_SETTLE_MS > 0);
        assert!(super::OVERSCAN_ROWS_WHILE_RESIZING < 10);
    }
}
