//! Process-wide resize-throttle flags shared by the editor and block layout.

use std::sync::atomic::{AtomicBool, Ordering};

/// While the editor viewport width is changing, snap wrap width to this
/// quantum so `shape_text` cache hits across consecutive resize frames.
pub(crate) const WRAP_QUANTUM_PX_WHILE_RESIZING: i32 = 16;

static RESIZE_THROTTLE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_resize_throttle_active(active: bool) {
    RESIZE_THROTTLE_ACTIVE.store(active, Ordering::Relaxed);
}

pub(crate) fn resize_throttle_active() -> bool {
    RESIZE_THROTTLE_ACTIVE.load(Ordering::Relaxed)
}
