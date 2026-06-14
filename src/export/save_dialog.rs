//! Platform-specific save dialogs for export.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use futures::channel::oneshot;
use gpui::AsyncApp;

/// Prompts for an export destination without a native overwrite confirmation.
#[cfg(target_os = "macos")]
pub(crate) async fn prompt_export_save_path(
    cx: &AsyncApp,
    directory: PathBuf,
    suggested_name: Option<String>,
) -> Result<Option<PathBuf>> {
    let (tx, rx) = oneshot::channel();
    cx.foreground_executor()
        .spawn(async move {
            let result = prompt_export_save_path_on_main_thread(&directory, suggested_name.as_deref());
            let _ = tx.send(result);
        })
        .detach();
    rx.await.unwrap_or(Ok(None))
}

#[cfg(target_os = "macos")]
fn prompt_export_save_path_on_main_thread(
    directory: &Path,
    suggested_name: Option<&str>,
) -> Result<Option<PathBuf>> {
    use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
    use objc2_foundation::{MainThreadMarker, NSURL, NSString};

    let mtm = MainThreadMarker::new().context("export save panel must run on the main thread")?;
    let panel = NSSavePanel::savePanel(mtm);

    if !directory.as_os_str().is_empty() {
        let directory_string = NSString::from_str(&directory.to_string_lossy());
        let directory_url = NSURL::fileURLWithPath_isDirectory(&directory_string, true);
        panel.setDirectoryURL(Some(&directory_url));
    }

    if let Some(suggested_name) = suggested_name {
        panel.setNameFieldStringValue(&NSString::from_str(suggested_name));
    }

    if panel.runModal() != NSModalResponseOK {
        return Ok(None);
    }

    let url = panel.URL().context("export save panel did not return a file URL")?;
    let path = url
        .path()
        .context("export save panel returned a non-file URL")?
        .to_string();
    Ok(Some(PathBuf::from(path)))
}
