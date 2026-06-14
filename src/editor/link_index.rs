//! Workspace-wide inline wiki link index built from Markdown files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::components::markdown::inline::locate_wiki_link_in_str;
use crate::components::{is_closing_fence_marker, opening_fence_marker, BlockKind};

use super::markdown_files::{collect_markdown_files, is_markdown_file};

/// One occurrence of a wiki link inside a workspace markdown file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkOccurrence {
    pub source_path: PathBuf,
    pub target_path: String,
    pub line: usize,
    pub preview: String,
    pub match_start_byte: usize,
    pub raw_file_len: usize,
}

/// Aggregated workspace wiki link index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkspaceLinkIndex {
    /// Source file → outgoing links.
    pub by_source: BTreeMap<PathBuf, Vec<LinkOccurrence>>,
    /// Target path (as written in `[[...]]`) → backlinks.
    pub by_target: BTreeMap<String, Vec<LinkOccurrence>>,
    pub revision: u64,
}

pub(crate) fn extract_links_from_markdown(
    content: &str,
    source_path: &Path,
) -> Vec<LinkOccurrence> {
    let raw_file_len = content.len();
    let mut results = Vec::new();
    let mut active_fence: Option<(char, usize)> = None;
    let mut line_number = 1usize;
    let mut line_start = 0usize;

    for segment in content.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\n', '\r']);

        if let Some((marker, run_len)) = active_fence {
            if is_closing_fence_marker(line, marker, run_len) {
                active_fence = None;
            }
            line_start += segment.len();
            line_number += 1;
            continue;
        }

        if let Some(fence) = opening_fence_marker(line) {
            active_fence = Some(fence);
            line_start += segment.len();
            line_number += 1;
            continue;
        }

        if BlockKind::parse_atx_heading_line(line).is_some() {
            line_start += segment.len();
            line_number += 1;
            continue;
        }

        let mut search = 0usize;
        while search < line.len() {
            let Some((start, end, target_path)) = locate_wiki_link_in_str(line, search) else {
                break;
            };
            results.push(LinkOccurrence {
                source_path: source_path.to_path_buf(),
                target_path,
                line: line_number,
                preview: line.trim().to_string(),
                match_start_byte: line_start + start,
                raw_file_len,
            });
            search = end;
        }

        line_start += segment.len();
        line_number += 1;
    }

    results
}

fn remove_path_from_index(index: &mut WorkspaceLinkIndex, path: &Path) {
    index.by_source.remove(path);
    index.by_target.retain(|_, occurrences| {
        occurrences.retain(|occurrence| occurrence.source_path != path);
        !occurrences.is_empty()
    });
}

fn rebuild_by_target(by_source: &BTreeMap<PathBuf, Vec<LinkOccurrence>>) -> BTreeMap<String, Vec<LinkOccurrence>> {
    let mut by_target: BTreeMap<String, Vec<LinkOccurrence>> = BTreeMap::new();
    for occurrences in by_source.values() {
        for occurrence in occurrences {
            by_target
                .entry(occurrence.target_path.clone())
                .or_default()
                .push(occurrence.clone());
        }
    }
    by_target
}

pub(crate) fn build_workspace_link_index(root: &Path) -> WorkspaceLinkIndex {
    let mut files = Vec::new();
    collect_markdown_files(root, &mut files);
    files.sort();

    let mut by_source: BTreeMap<PathBuf, Vec<LinkOccurrence>> = BTreeMap::new();
    for path in files {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let occurrences = extract_links_from_markdown(&content, &path);
        if !occurrences.is_empty() {
            by_source.insert(path, occurrences);
        }
    }

    let by_target = rebuild_by_target(&by_source);
    WorkspaceLinkIndex {
        by_source,
        by_target,
        revision: 1,
    }
}

pub(crate) fn refresh_link_index_for_file(
    index: &mut WorkspaceLinkIndex,
    path: &Path,
    content: &str,
) {
    remove_path_from_index(index, path);

    if is_markdown_file(path) {
        let occurrences = extract_links_from_markdown(content, path);
        if occurrences.is_empty() {
            index.by_source.remove(path);
        } else {
            index.by_source.insert(path.to_path_buf(), occurrences);
        }
    }

    index.by_target = rebuild_by_target(&index.by_source);
    index.revision = index.revision.saturating_add(1);
}

#[cfg(test)]
pub(crate) fn remove_file_from_link_index(index: &mut WorkspaceLinkIndex, path: &Path) {
    remove_path_from_index(index, path);
    index.revision = index.revision.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const FIXTURE: &str = "\
# Title
see [[a.md]] and [[b/c.md]]
```markdown
[[ignored.md]]
```
also [[note.md]]
";

    #[test]
    fn extract_links_finds_multiple_targets_and_skips_code_blocks() {
        let path = PathBuf::from("/tmp/notes/sample.md");
        let links = extract_links_from_markdown(FIXTURE, &path);

        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target_path, "a.md");
        assert_eq!(links[0].line, 2);
        assert_eq!(links[1].target_path, "b/c.md");
        assert_eq!(links[2].target_path, "note.md");
        assert_eq!(links[2].line, 6);
        assert!(links.iter().all(|link| link.source_path == path));
    }

    #[test]
    fn build_index_groups_by_source_and_target() {
        let dir = std::env::temp_dir().join(format!(
            "markman-link-index-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let root = dir.as_path();
        let note_a = root.join("a.md");
        let note_b = root.join("b.md");
        std::fs::write(&note_a, "link [[b.md]]\n").expect("write a");
        std::fs::write(&note_b, "back [[a.md]]\n").expect("write b");

        let index = build_workspace_link_index(root);
        assert_eq!(index.by_source.len(), 2);
        assert_eq!(index.by_target.get("b.md").map(Vec::len), Some(1));
        assert_eq!(index.by_target.get("a.md").map(Vec::len), Some(1));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_replaces_previous_file_entries() {
        let path = PathBuf::from("/tmp/notes/update.md");
        let mut index = WorkspaceLinkIndex::default();
        refresh_link_index_for_file(&mut index, &path, "old [[a.md]]\n");
        refresh_link_index_for_file(&mut index, &path, "new [[b.md]]\n");

        assert!(index.by_target.get("a.md").is_none());
        assert_eq!(index.by_target.get("b.md").map(Vec::len), Some(1));
        assert_eq!(index.by_source.get(&path).map(Vec::len), Some(1));
        assert_eq!(index.revision, 2);
    }

    #[test]
    fn remove_file_from_link_index_drops_all_occurrences() {
        let path = PathBuf::from("/tmp/notes/remove.md");
        let mut index = WorkspaceLinkIndex::default();
        refresh_link_index_for_file(&mut index, &path, "note [[x.md]]\n");
        assert_eq!(index.by_target.get("x.md").map(Vec::len), Some(1));

        remove_file_from_link_index(&mut index, &path);
        assert!(index.by_source.is_empty());
        assert!(index.by_target.is_empty());
    }
}
