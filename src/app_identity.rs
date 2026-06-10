//! Shared application identity used by platform windowing and packaging.

/// Reverse-DNS application id used by GPUI, desktop launchers, and bundles.
pub(crate) const VELOTYPE_APP_ID: &str = "com.manyougz.Velotype";

/// User-visible application name shown in window titles and menus.
pub(crate) const APP_DISPLAY_NAME: &str = "Markman";

pub(crate) fn app_window_title(document_label: Option<&str>) -> String {
    match document_label {
        Some(label) => format!("{APP_DISPLAY_NAME} - {label}"),
        None => APP_DISPLAY_NAME.to_string(),
    }
}

pub(crate) fn app_version_line() -> String {
    format!("{APP_DISPLAY_NAME} {}", env!("CARGO_PKG_VERSION"))
}
