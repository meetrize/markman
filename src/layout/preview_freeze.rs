//! Process-wide preview motion freeze flag shared by the editor and blocks.
//!
//! While set, focused blocks must not arm cursor-blink timers so resize /
//! fast scroll / graph-tab freeze actually stick across re-renders.

use std::sync::atomic::{AtomicBool, Ordering};

static PREVIEW_MOTION_FREEZE: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_preview_motion_freeze(active: bool) {
    PREVIEW_MOTION_FREEZE.store(active, Ordering::Relaxed);
}

pub(crate) fn preview_motion_freeze_active() -> bool {
    PREVIEW_MOTION_FREEZE.load(Ordering::Relaxed)
}
