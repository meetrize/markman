//! Experimental: reduce preview repaint work while the knowledge graph tab is
//! active so graph drag physics is not competing with full document re-layout.

use gpui::*;

use super::{Editor, ViewMode};

impl Editor {
    /// Preview is considered settled when the document is clean and the graph tab is shown.
    pub(super) fn should_freeze_preview_for_graph(&self) -> bool {
        self.view_mode == ViewMode::Rendered
            && !self.document_dirty
            && self.workspace_graph_tab_active()
            && self.knowledge_graph_view.is_some()
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

    pub(super) fn arm_preview_freeze_if_needed(
        &mut self,
        freeze_preview: bool,
        cx: &mut Context<Self>,
    ) {
        if freeze_preview && !self.preview_freeze_armed {
            self.suspend_preview_cursor_blink(cx);
            self.preview_freeze_armed = true;
        }
    }
}
