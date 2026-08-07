//! Shared layout calculations used by the editor and block rendering.

mod document_column;
pub(crate) mod resize_throttle;

pub use document_column::centered_column_width;
pub(crate) use resize_throttle::{
    WRAP_QUANTUM_PX_WHILE_RESIZING, resize_throttle_active, set_resize_throttle_active,
};
