//! Last editor session: file path, scroll offset, and caret position.

use std::path::PathBuf;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use super::MarkmanConfigDirs;

/// Persisted editor session restored on the next application launch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct EditorSessionState {
    pub file_path: PathBuf,
    pub scroll_offset_y: f32,
    pub selection_start: usize,
    pub selection_end: usize,
    pub selection_reversed: bool,
    pub view_mode: String,
}

pub(crate) fn read_editor_session_state() -> anyhow::Result<Option<EditorSessionState>> {
    read_editor_session_state_with_dirs(&MarkmanConfigDirs::from_system()?)
}

pub(crate) fn record_editor_session_state(state: &EditorSessionState) -> anyhow::Result<()> {
    record_editor_session_state_with_dirs(state, &MarkmanConfigDirs::from_system()?)
}

pub(crate) fn first_existing_editor_session() -> Option<EditorSessionState> {
    let session = read_editor_session_state().ok().flatten()?;
    if session.file_path.is_file() {
        Some(session)
    } else {
        None
    }
}

pub(crate) fn read_editor_session_state_with_dirs(
    dirs: &MarkmanConfigDirs,
) -> anyhow::Result<Option<EditorSessionState>> {
    let path = dirs.last_session_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };

    let state = toml::from_str::<EditorSessionState>(&text)
        .with_context(|| format!("failed to parse '{}'", path.display()))?;
    if state.file_path.to_string_lossy().trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state))
}

pub(crate) fn record_editor_session_state_with_dirs(
    state: &EditorSessionState,
    dirs: &MarkmanConfigDirs,
) -> anyhow::Result<()> {
    if state.file_path.to_string_lossy().trim().is_empty() {
        anyhow::bail!("session file path cannot be empty");
    }

    let path = dirs.last_session_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let text = toml::to_string_pretty(state)
        .with_context(|| "failed to serialize editor session state")?;
    std::fs::write(&path, text).with_context(|| format!("failed to write '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        EditorSessionState, read_editor_session_state_with_dirs,
        record_editor_session_state_with_dirs,
    };
    use crate::config::MarkmanConfigDirs;

    #[test]
    fn records_and_reads_editor_session_state() {
        let root = std::env::temp_dir().join(format!(
            "markman-session-save-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        let dirs = MarkmanConfigDirs::from_root(&root);
        let file = root.join("notes.md");
        std::fs::write(&file, "# hello").expect("write fixture file");

        let state = EditorSessionState {
            file_path: file.clone(),
            scroll_offset_y: 128.5,
            selection_start: 3,
            selection_end: 7,
            selection_reversed: true,
            view_mode: "source".into(),
        };
        record_editor_session_state_with_dirs(&state, &dirs).expect("record session");

        let loaded =
            read_editor_session_state_with_dirs(&dirs).expect("read session").expect("session");
        assert_eq!(loaded, state);

        let text = std::fs::read_to_string(dirs.last_session_file()).expect("session file");
        assert!(text.contains("scroll_offset_y = 128.5"));
        assert!(text.contains("view_mode = \"source\""));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_session_file_returns_none() {
        let root = std::env::temp_dir().join(format!(
            "markman-session-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = MarkmanConfigDirs::from_root(&root);
        assert!(
            read_editor_session_state_with_dirs(&dirs)
                .expect("read missing session")
                .is_none()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
