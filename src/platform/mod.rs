//! macOS platform integrations.

#[cfg(target_os = "macos")]
mod macos_magnify;

#[cfg(target_os = "macos")]
pub fn init_document_gestures(cx: &mut gpui::App) {
    macos_magnify::start_magnify_pump(cx);
}
