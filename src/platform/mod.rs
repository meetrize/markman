//! Platform integrations (macOS gestures, OS file-open handoff, etc.).

mod open_external;
mod path_prompt;

#[cfg(target_os = "macos")]
mod macos_magnify;

pub use open_external::{init_external_open_handling, install_open_external_files};
pub use path_prompt::prompt_for_paths_with_clipboard_navigation;

#[cfg(target_os = "macos")]
pub fn init_document_gestures(cx: &mut gpui::App) {
    macos_magnify::start_magnify_pump(cx);
}
