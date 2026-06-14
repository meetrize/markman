//! Theme configuration and global theme access.
//!
//! Themes are JSON-serializable so editor colors, spacing, and typography can
//! be swapped without changing the runtime logic.

mod document_zoom;
mod fonts;
mod theme;
pub use document_zoom::*;
pub use fonts::*;
pub use theme::*;
