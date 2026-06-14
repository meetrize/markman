//! Platform integrations (macOS gestures, OS file-open handoff, etc.).

mod open_external;

#[cfg(target_os = "macos")]
mod macos_magnify;

pub use open_external::{init_external_open_drain, install_open_external_files};

#[cfg(target_os = "macos")]
pub fn init_document_gestures(cx: &mut gpui::App) {
    macos_magnify::start_magnify_pump(cx);
}
