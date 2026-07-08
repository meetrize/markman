//! Background filesystem watcher that keeps the workspace file tree in sync.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use gpui::*;

use super::markdown_files::should_skip_workspace_entry_name;
use super::Editor;

const WATCH_DEBOUNCE: Duration = Duration::from_millis(300);
const WATCH_POLL: Duration = Duration::from_millis(50);

impl Editor {
    pub(super) fn sync_workspace_file_watcher(&mut self, cx: &mut Context<Self>) {
        if !self.workspace_panel_is_open() {
            self.stop_workspace_file_watcher();
            return;
        }

        let next_root = self.effective_workspace_root();
        if self.workspace.file_watch_root == next_root && self.workspace.file_watch_task.is_some() {
            return;
        }

        self.stop_workspace_file_watcher();

        let Some(root) = next_root else {
            return;
        };

        self.workspace.file_watch_root = Some(root.clone());
        let editor = cx.entity().downgrade();
        self.workspace.file_watch_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                run_workspace_file_watcher(editor, root, cx).await;
            },
        ));
    }

    fn stop_workspace_file_watcher(&mut self) {
        self.workspace.file_watch_task = None;
        self.workspace.file_watch_root = None;
    }
}

struct WorkspaceWatchStop(Arc<AtomicBool>);

impl Drop for WorkspaceWatchStop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn spawn_workspace_watch_thread(
    root: PathBuf,
    stop: Arc<AtomicBool>,
) -> Option<mpsc::Receiver<()>> {
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

    let (event_tx, event_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (notify_tx, notify_rx) = mpsc::channel();
        let Ok(mut watcher) = RecommendedWatcher::new(notify_tx, Config::default()) else {
            return;
        };
        if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
            return;
        }

        while !stop.load(Ordering::Relaxed) {
            match notify_rx.recv_timeout(WATCH_POLL) {
                Ok(Ok(event)) => {
                    if is_relevant_workspace_event(&root, &event) {
                        let _ = event_tx.send(());
                    }
                }
                Ok(Err(err)) => {
                    eprintln!("workspace file watcher error: {err}");
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    Some(event_rx)
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
async fn run_workspace_file_watcher(
    editor: WeakEntity<Editor>,
    root: PathBuf,
    cx: &mut AsyncApp,
) {
    let stop = Arc::new(AtomicBool::new(false));
    let _guard = WorkspaceWatchStop(stop.clone());
    let Some(event_rx) = spawn_workspace_watch_thread(root.clone(), stop) else {
        return;
    };

    let mut pending_refresh = false;
    let mut last_event = Instant::now();

    loop {
        cx.background_executor().timer(WATCH_POLL).await;

        while event_rx.try_recv().is_ok() {
            pending_refresh = true;
            last_event = Instant::now();
        }

        if pending_refresh && last_event.elapsed() >= WATCH_DEBOUNCE {
            pending_refresh = false;
            let root_for_refresh = root.clone();
            let _ = editor.update(cx, |editor, cx| {
                if editor.effective_workspace_root().as_deref() != Some(root_for_refresh.as_path()) {
                    return;
                }
                editor.workspace_refresh_file_tree(cx);
            });
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
async fn run_workspace_file_watcher(
    _editor: WeakEntity<Editor>,
    _root: PathBuf,
    _cx: &mut AsyncApp,
) {
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn is_relevant_workspace_event(root: &Path, event: &notify::Event) -> bool {
    use notify::EventKind;

    match &event.kind {
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => {}
        _ => return false,
    }

    event.paths.iter().any(|path| {
        path.starts_with(root) && !path_is_in_skipped_workspace_dir(path)
    })
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn path_is_in_skipped_workspace_dir(path: &Path) -> bool {
    path.components().any(|component| {
        if let std::path::Component::Normal(name) = component {
            should_skip_workspace_entry_name(&name.to_string_lossy())
        } else {
            false
        }
    })
}
