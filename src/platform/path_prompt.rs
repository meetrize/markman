//! Native open-panel helpers with clipboard path navigation on macOS.

use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::channel::oneshot;
use gpui::{App, PathPromptOptions};

/// Opens the platform file/folder picker. On macOS, ⌘V pastes a clipboard path
/// and navigates the panel to that location without leaving the dialog.
pub fn prompt_for_paths_with_clipboard_navigation(
    cx: &App,
    options: PathPromptOptions,
) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = oneshot::channel();
        cx.foreground_executor()
            .spawn(async move {
                let result = prompt_for_paths_on_main_thread(options);
                let _ = tx.send(result);
            })
            .detach();
        return rx;
    }

    #[cfg(not(target_os = "macos"))]
    {
        cx.prompt_for_paths(options)
    }
}

#[cfg(target_os = "macos")]
fn prompt_for_paths_on_main_thread(options: PathPromptOptions) -> Result<Option<Vec<PathBuf>>> {
    use std::ptr::NonNull;

    use anyhow::Context as _;
    use block2::RcBlock;
    use objc2_app_kit::{
        NSModalResponseOK, NSOpenPanel, NSEvent, NSEventMask, NSEventModifierFlags,
    };
    use objc2_foundation::{MainThreadMarker, NSString};

    let mtm =
        MainThreadMarker::new().context("open panel must run on the main thread")?;
    let panel = NSOpenPanel::openPanel(mtm);

    panel.setCanChooseDirectories(options.directories);
    panel.setCanChooseFiles(options.files);
    panel.setAllowsMultipleSelection(options.multiple);
    panel.setCanCreateDirectories(true);
    panel.setResolvesAliases(false);

    if let Some(prompt) = options.prompt.as_ref() {
        panel.setPrompt(Some(&NSString::from_str(prompt)));
    }

    let pick_files = options.files;
    if let Some(path) = path_parse::read_clipboard_path() {
        apply_navigation(&panel, &path, pick_files);
    }

    let panel_for_monitor = panel.clone();
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let flags = unsafe { event.as_ref().modifierFlags() };
        if !flags.contains(NSEventModifierFlags::Command) {
            return event.as_ptr();
        }

        let chars = unsafe { event.as_ref().charactersIgnoringModifiers() };
        let Some(chars) = chars else {
            return event.as_ptr();
        };
        let key = chars.to_string();
        if !key.eq_ignore_ascii_case("v") {
            return event.as_ptr();
        }

        if let Some(path) = path_parse::read_clipboard_path() {
            apply_navigation(&panel_for_monitor, &path, pick_files);
            return std::ptr::null_mut();
        }

        event.as_ptr()
    });

    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
    };

    let response = panel.runModal();

    if let Some(monitor) = monitor {
        unsafe { NSEvent::removeMonitor(&monitor) };
    }

    if response != NSModalResponseOK {
        return Ok(None);
    }

    let mut paths = Vec::new();
    for url in panel.URLs().iter() {
        if !url.isFileURL() {
            continue;
        }
        let Some(path) = url.path() else {
            continue;
        };
        paths.push(PathBuf::from(path.to_string()));
    }

    Ok(Some(paths))
}

#[cfg(target_os = "macos")]
fn apply_navigation(panel: &objc2_app_kit::NSOpenPanel, path: &Path, pick_files: bool) {
    use objc2_foundation::{NSURL, NSString};

    let (directory, file_name) = path_parse::resolve_navigation_target(path, pick_files);
    if directory.as_os_str().is_empty() || !directory.exists() {
        return;
    }

    let directory_string = NSString::from_str(&directory.to_string_lossy());
    let directory_url = NSURL::fileURLWithPath_isDirectory(&directory_string, true);
    panel.setDirectoryURL(Some(&directory_url));

    if let Some(file_name) = file_name {
        panel.setNameFieldStringValue(&NSString::from_str(&file_name));
    }
}

mod path_parse {
    use std::path::{Path, PathBuf};

    use directories::BaseDirs;

    #[cfg(target_os = "macos")]
    pub(crate) fn read_clipboard_path() -> Option<PathBuf> {
        use objc2_app_kit::{
            NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeString,
        };

        let pasteboard = NSPasteboard::generalPasteboard();

        if let Some(file_url) =
            unsafe { pasteboard.stringForType(NSPasteboardTypeFileURL) }
        {
            if let Some(path) = normalize_pasted_path(&file_url.to_string()) {
                return Some(path);
            }
        }

        let text = unsafe { pasteboard.stringForType(NSPasteboardTypeString) }?;
        normalize_pasted_path(&text.to_string())
    }

    pub(crate) fn normalize_pasted_path(text: &str) -> Option<PathBuf> {
        let path = parse_path_text(text)?;
        if path.exists() {
            return path.canonicalize().ok().or(Some(path));
        }

        let parent_exists = path.parent().is_some_and(|parent| parent.exists());
        parent_exists.then_some(path)
    }

    pub(crate) fn resolve_navigation_target(
        path: &Path,
        pick_files: bool,
    ) -> (PathBuf, Option<String>) {
        if path.is_dir() {
            return (path.to_path_buf(), None);
        }

        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
        let file_name = pick_files
            .then(|| path.file_name().map(|name| name.to_string_lossy().into_owned()))
            .flatten();
        (parent, file_name)
    }

    fn parse_path_text(text: &str) -> Option<PathBuf> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        let unquoted = trimmed.trim_matches('"').trim_matches('\'');
        if let Some(rest) = unquoted.strip_prefix("file://") {
            let url = format!("file://{rest}");
            return url::Url::parse(&url).ok()?.to_file_path().ok();
        }

        Some(expand_tilde(unquoted))
    }

    fn expand_tilde(path: &str) -> PathBuf {
        if let Some(rest) = path.strip_prefix("~/") {
            if let Some(home) = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
                return home.join(rest);
            }
        }

        if path == "~" {
            if let Some(home) = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
                return home;
            }
        }

        PathBuf::from(path)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};

        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

        fn temp_dir(name: &str) -> PathBuf {
            let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("markmemo-path-prompt-{name}-{id}"));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            dir
        }

        #[test]
        fn normalize_pasted_path_expands_tilde() {
            let home = BaseDirs::new()
                .expect("home")
                .home_dir()
                .to_path_buf();
            assert_eq!(
                normalize_pasted_path("~/").expect("path"),
                expand_tilde("~/")
            );
            assert!(normalize_pasted_path(&home.to_string_lossy()).is_some());
        }

        #[test]
        fn normalize_pasted_path_accepts_existing_parent() {
            let dir = temp_dir("parent");
            let child = dir.join("missing.md");
            assert_eq!(normalize_pasted_path(&child.to_string_lossy()), Some(child));
        }

        #[test]
        fn resolve_navigation_target_for_file_and_directory() {
            let dir = temp_dir("resolve");
            let file = dir.join("note.md");
            fs::write(&file, b"#").expect("write");

            assert_eq!(
                resolve_navigation_target(&dir, true),
                (dir.clone(), None)
            );
            assert_eq!(
                resolve_navigation_target(&file, true),
                (dir, Some("note.md".into()))
            );
            assert_eq!(
                resolve_navigation_target(&file, false),
                (file.parent().unwrap().to_path_buf(), None)
            );
        }
    }
}
