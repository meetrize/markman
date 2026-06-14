//! Handle files opened by the OS (macOS Open With, Linux file-manager handoff, etc.).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use gpui::{App, Application, AsyncApp};

use crate::app_menu;
use crate::config::store::AppPreferences;

static PENDING_EXTERNAL_OPEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

const EXTERNAL_OPEN_POLL_INTERVAL: Duration = Duration::from_millis(16);
const EXTERNAL_OPEN_WAIT_TIMEOUT: Duration = Duration::from_millis(200);

/// Register a handler for platform file-open requests (`application:openURLs:` on macOS).
pub fn install_open_external_files(application: &Application) {
    application.on_open_urls(move |urls| {
        let paths = markdown_paths_from_urls(urls);
        if paths.is_empty() {
            return;
        }
        if let Ok(mut pending) = PENDING_EXTERNAL_OPEN.lock() {
            pending.extend(paths);
        }
    });
}

/// Drain pending OS file-open requests and optionally defer the default startup window.
pub fn init_external_open_handling(cx: &mut App, deferred_startup: Option<AppPreferences>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        if let Some(preferences) = deferred_startup {
            let external_paths =
                wait_for_external_open_paths(cx, EXTERNAL_OPEN_WAIT_TIMEOUT).await;
            let _ = cx.update(|app| {
                if external_paths.is_empty() {
                    app_menu::open_default_startup_window(app, &preferences);
                } else {
                    app_menu::open_markdown_files_from_external(app, &external_paths);
                }
                app_menu::install_menus(app);
                app.refresh_windows();
            });
        }

        loop {
            cx.background_executor()
                .timer(EXTERNAL_OPEN_POLL_INTERVAL)
                .await;
            let paths = take_pending_external_open_paths();
            if paths.is_empty() {
                continue;
            }
            let _ = cx.update(|app| app_menu::open_markdown_files_from_external(app, &paths));
        }
    })
    .detach();
}

async fn wait_for_external_open_paths(cx: &mut AsyncApp, timeout: Duration) -> Vec<PathBuf> {
    let mut waited = Duration::ZERO;
    while waited < timeout {
        if has_pending_external_open_paths() {
            return take_pending_external_open_paths();
        }
        cx.background_executor()
            .timer(EXTERNAL_OPEN_POLL_INTERVAL)
            .await;
        waited += EXTERNAL_OPEN_POLL_INTERVAL;
    }
    take_pending_external_open_paths()
}

fn has_pending_external_open_paths() -> bool {
    PENDING_EXTERNAL_OPEN
        .lock()
        .is_ok_and(|pending| !pending.is_empty())
}

fn take_pending_external_open_paths() -> Vec<PathBuf> {
    PENDING_EXTERNAL_OPEN
        .lock()
        .map(|mut pending| pending.drain(..).collect())
        .unwrap_or_default()
}

fn markdown_paths_from_urls(urls: Vec<String>) -> Vec<PathBuf> {
    urls.into_iter()
        .filter_map(|url| path_from_open_url(&url))
        .filter(|path| crate::editor::Editor::is_markdown_file_path(path))
        .collect()
}

fn path_from_open_url(url: &str) -> Option<PathBuf> {
    if let Ok(parsed) = url::Url::parse(url) {
        if parsed.scheme() == "file" {
            return parsed.to_file_path().ok();
        }
    }

    let path = PathBuf::from(url);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_url_parses_to_path() {
        let path = path_from_open_url("file:///tmp/example.md").expect("file url");
        assert_eq!(path, PathBuf::from("/tmp/example.md"));
    }

    #[test]
    fn ignores_non_file_urls() {
        assert!(path_from_open_url("https://example.com/doc.md").is_none());
    }

    #[test]
    fn accepts_existing_raw_path() {
        let path = std::env::temp_dir().join("markman-open-external-test.md");
        std::fs::write(&path, "# test").unwrap();
        let parsed = path_from_open_url(&path.display().to_string()).expect("raw path");
        assert_eq!(parsed, path);
        let _ = std::fs::remove_file(path);
    }
}
