//! Shared path parsing for clipboard and typed navigation.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

pub fn normalize_pasted_path(text: &str) -> Option<PathBuf> {
    let path = parse_path_text(text)?;
    if path.exists() {
        return path.canonicalize().ok().or(Some(path));
    }

    let parent_exists = path.parent().is_some_and(|parent| parent.exists());
    parent_exists.then_some(path)
}

pub fn resolve_navigation_target(path: &Path, pick_files: bool) -> (PathBuf, Option<String>) {
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
        let dir = std::env::temp_dir().join(format!("markmemo-path-parse-{name}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn normalize_pasted_path_expands_tilde() {
        let home = BaseDirs::new().expect("home").home_dir().to_path_buf();
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

        assert_eq!(resolve_navigation_target(&dir, true), (dir.clone(), None));
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
