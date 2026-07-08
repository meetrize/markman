//! Shared Markdown file enumeration for workspace search and tag indexing.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md"))
}

/// Returns true for directories that should be skipped during workspace scans.
pub(crate) fn should_skip_workspace_entry_name(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }

    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "vendor"
            | "__pycache__"
            | "coverage"
            | ".pnpm"
    )
}

pub(crate) fn collect_markdown_files(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        if should_skip_workspace_entry_name(&file_name) {
            continue;
        }
        if path.is_dir() {
            collect_markdown_files(&path, files);
        } else if is_markdown_file(&path) {
            files.push(path);
        }
    }
}
