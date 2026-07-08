//! Shared workspace file search helpers for quick-open and wiki-link pickers.

use std::path::{Path, PathBuf};

use super::markdown_files::should_skip_workspace_entry_name;

/// A workspace file row for flat fuzzy-search results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileSearchResult {
    pub path: PathBuf,
    pub label: String,
    pub detail: String,
}

/// Hierarchical node for tree-mode file browsing.
#[derive(Clone, Debug)]
pub(super) struct FileTreeNode {
    pub id: String,
    pub label: String,
    /// Workspace-root-relative path for file leaves.
    pub relative_path: Option<String>,
    pub is_directory: bool,
    pub children: Vec<FileTreeNode>,
}

/// One flattened row in tree mode (directory header or selectable file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FileTreeRow {
    Directory {
        depth: usize,
        node_id: String,
        label: String,
        expanded: bool,
    },
    File {
        depth: usize,
        relative_path: String,
        label: String,
        detail: String,
    },
}

pub(super) fn should_skip_entry_name(name: &str) -> bool {
    should_skip_workspace_entry_name(name)
}

/// Recursively collects all regular files under `root`, sorted by relative path.
pub(super) fn collect_all_files_recursive(root: Option<&Path>) -> Vec<PathBuf> {
    let Some(root) = root else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_files_recursive_inner(root, root, &mut paths);
    paths.sort();
    paths
}

fn collect_files_recursive_inner(root: &Path, dir: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        if should_skip_entry_name(&file_name) {
            continue;
        }
        if path.is_dir() {
            collect_files_recursive_inner(root, &path, paths);
        } else if path.is_file() {
            paths.push(path);
        }
    }
}

pub(super) fn build_workspace_file_tree(root: Option<&Path>) -> Option<FileTreeNode> {
    let root = root?;
    Some(scan_tree_dir(root, root))
}

fn scan_tree_dir(root: &Path, dir: &Path) -> FileTreeNode {
    let mut children = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default();
            if should_skip_entry_name(&file_name) {
                continue;
            }
            if path.is_dir() {
                children.push(scan_tree_dir(root, &path));
            } else if path.is_file() {
                children.push(FileTreeNode {
                    id: tree_node_id(&path),
                    label: file_name.into_owned(),
                    relative_path: Some(file_display_label(Some(root), &path)),
                    is_directory: false,
                    children: Vec::new(),
                });
            }
        }
    }

    children.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });

    FileTreeNode {
        id: tree_node_id(dir),
        label: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.to_string_lossy().into_owned()),
        relative_path: None,
        is_directory: true,
        children,
    }
}

fn tree_node_id(path: &Path) -> String {
    format!("tree:{}", path.to_string_lossy())
}

pub(super) fn flatten_file_tree(
    node: &FileTreeNode,
    expanded: &std::collections::HashSet<String>,
    depth: usize,
    root: Option<&Path>,
    rows: &mut Vec<FileTreeRow>,
) {
    if node.is_directory {
        let expanded_flag = expanded.contains(&node.id);
        rows.push(FileTreeRow::Directory {
            depth,
            node_id: node.id.clone(),
            label: node.label.clone(),
            expanded: expanded_flag,
        });
        if expanded_flag {
            for child in &node.children {
                flatten_file_tree(child, expanded, depth + 1, root, rows);
            }
        }
        return;
    }

    if let Some(relative_path) = node.relative_path.as_ref() {
        rows.push(FileTreeRow::File {
            depth,
            relative_path: relative_path.clone(),
            label: node.label.clone(),
            detail: file_parent_label(root, &PathBuf::from(relative_path)),
        });
    }
}

pub(super) fn filter_files_fuzzy(
    all_files: &[PathBuf],
    root: Option<&Path>,
    query: &str,
) -> Vec<FileSearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut scored: Vec<(usize, FileSearchResult)> = all_files
        .iter()
        .filter_map(|path| {
            let label = file_display_label(root, path);
            let label_lower = label.to_lowercase();
            let detail = file_parent_label(root, path);
            let path_str = path.to_string_lossy().to_lowercase();
            let score = fuzzy_match_score(&path_str, &query_lower);
            let name_score = fuzzy_match_score(&label_lower, &query_lower);
            let final_score = if name_score < score || score >= usize::MAX / 2 {
                name_score
            } else {
                score
            };
            (final_score < usize::MAX).then(|| {
                (
                    final_score,
                    FileSearchResult {
                        path: path.clone(),
                        label,
                        detail,
                    },
                )
            })
        })
        .collect();

    scored.sort_by(|(s1, r1), (s2, r2)| s1.cmp(s2).then_with(|| r1.label.cmp(&r2.label)));
    scored.into_iter().map(|(_, result)| result).collect()
}

pub(super) fn file_display_label(root: Option<&Path>, path: &Path) -> String {
    if let Some(root) = root {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    } else {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }
}

pub(super) fn file_parent_label(root: Option<&Path>, path: &Path) -> String {
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            return String::new();
        }
        file_display_label(root, parent)
    } else {
        String::new()
    }
}

pub(super) fn fuzzy_match_score(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }

    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();

    let mut ni = 0;
    let mut hi = 0;
    let mut score = 0usize;
    let mut last_match = None;

    while ni < needle.len() && hi < haystack.len() {
        if haystack[hi] == needle[ni] {
            if let Some(lm) = last_match {
                if hi == lm + 1 {
                    score = score.saturating_sub(10);
                }
            } else {
                score += hi;
            }
            if hi == 0
                || haystack[hi - 1] == b'/'
                || haystack[hi - 1] == b'_'
                || haystack[hi - 1] == b'-'
                || haystack[hi - 1] == b'.'
            {
                score = score.saturating_sub(20);
            }
            last_match = Some(hi);
            ni += 1;
        } else {
            score += 1;
        }
        hi += 1;
    }

    if ni == needle.len() {
        score
    } else {
        usize::MAX
    }
}
