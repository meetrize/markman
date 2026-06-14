//! Workspace-wide inline hashtag index built from Markdown files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::components::markdown::inline::{locate_hashtag_in_str, normalize_tag_name};
use crate::components::{is_closing_fence_marker, opening_fence_marker, BlockKind};

use super::markdown_files::{collect_markdown_files, is_markdown_file};

/// One occurrence of a tag inside a workspace markdown file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TagOccurrence {
    pub path: PathBuf,
    pub line: usize,
    pub preview: String,
    pub match_start_byte: usize,
    pub raw_file_len: usize,
}

/// Aggregated workspace tag index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WorkspaceTagIndex {
    pub by_tag: BTreeMap<String, Vec<TagOccurrence>>,
    pub counts: BTreeMap<String, usize>,
    pub revision: u64,
}

pub(crate) fn extract_tags_from_markdown(
    content: &str,
    path: &Path,
) -> Vec<(String, TagOccurrence)> {
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
            let Some((start, end, tag)) = locate_hashtag_in_str(line, search) else {
                break;
            };
            let canonical = normalize_tag_name(&tag.name);
            results.push((
                canonical,
                TagOccurrence {
                    path: path.to_path_buf(),
                    line: line_number,
                    preview: line.trim().to_string(),
                    match_start_byte: line_start + start,
                    raw_file_len,
                },
            ));
            search = end;
        }

        line_start += segment.len();
        line_number += 1;
    }

    results
}

fn rebuild_counts(by_tag: &BTreeMap<String, Vec<TagOccurrence>>) -> BTreeMap<String, usize> {
    by_tag
        .iter()
        .map(|(tag, occurrences)| (tag.clone(), occurrences.len()))
        .collect()
}

fn remove_path_from_index(index: &mut WorkspaceTagIndex, path: &Path) {
    index.by_tag.retain(|_, occurrences| {
        occurrences.retain(|occurrence| occurrence.path != path);
        !occurrences.is_empty()
    });
    index.counts = rebuild_counts(&index.by_tag);
}

pub(crate) fn build_workspace_tag_index(root: &Path) -> WorkspaceTagIndex {
    let mut files = Vec::new();
    collect_markdown_files(root, &mut files);
    files.sort();

    let mut by_tag: BTreeMap<String, Vec<TagOccurrence>> = BTreeMap::new();
    for path in files {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        for (canonical, occurrence) in extract_tags_from_markdown(&content, &path) {
            by_tag.entry(canonical).or_default().push(occurrence);
        }
    }

    let counts = rebuild_counts(&by_tag);
    WorkspaceTagIndex {
        by_tag,
        counts,
        revision: 1,
    }
}

pub(crate) fn refresh_tag_index_for_file(
    index: &mut WorkspaceTagIndex,
    path: &Path,
    content: &str,
) {
    remove_path_from_index(index, path);

    if is_markdown_file(path) {
        for (canonical, occurrence) in extract_tags_from_markdown(content, path) {
            index.by_tag.entry(canonical).or_default().push(occurrence);
        }
    }

    index.counts = rebuild_counts(&index.by_tag);
    index.revision = index.revision.saturating_add(1);
}

#[cfg(test)]
pub(crate) fn remove_file_from_tag_index(index: &mut WorkspaceTagIndex, path: &Path) {
    remove_path_from_index(index, path);
    index.revision = index.revision.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const FIXTURE: &str = "\
# Title not a tag
paragraph with #rust and #project/alpha
```python
#not-a-tag
```
color #fff ok
#2024 allowed
";

    #[test]
    fn extract_tags_skips_headings_code_blocks_and_hex_colors() {
        let path = PathBuf::from("/tmp/notes/sample.md");
        let tags: BTreeMap<_, _> = extract_tags_from_markdown(FIXTURE, &path)
            .into_iter()
            .map(|(canonical, occurrence)| (canonical, occurrence.preview))
            .collect();

        assert_eq!(tags.get("rust").map(String::as_str), Some("paragraph with #rust and #project/alpha"));
        assert_eq!(
            tags.get("project/alpha").map(String::as_str),
            Some("paragraph with #rust and #project/alpha")
        );
        assert_eq!(tags.get("2024").map(String::as_str), Some("#2024 allowed"));
        assert_eq!(tags.len(), 3);
        assert!(!tags.contains_key("fff"));
        assert!(!tags.contains_key("not-a-tag"));
    }

    #[test]
    fn refresh_merges_case_insensitive_canonical_tags() {
        let path = PathBuf::from("/tmp/notes/case.md");
        let mut index = WorkspaceTagIndex::default();
        refresh_tag_index_for_file(
            &mut index,
            &path,
            "alpha #Rust\nbeta #rust\n",
        );

        assert_eq!(index.counts.get("rust"), Some(&2));
        assert_eq!(index.by_tag.get("rust").map(Vec::len), Some(2));
    }

    #[test]
    fn remove_file_from_tag_index_drops_all_occurrences() {
        let path = PathBuf::from("/tmp/notes/remove.md");
        let mut index = WorkspaceTagIndex::default();
        refresh_tag_index_for_file(&mut index, &path, "note #rust\n");
        assert_eq!(index.counts.get("rust"), Some(&1));

        remove_file_from_tag_index(&mut index, &path);
        assert!(index.by_tag.is_empty());
        assert!(index.counts.is_empty());
    }

    #[test]
    fn refresh_replaces_previous_file_entries() {
        let path = PathBuf::from("/tmp/notes/update.md");
        let mut index = WorkspaceTagIndex::default();
        refresh_tag_index_for_file(&mut index, &path, "old #rust\n");
        refresh_tag_index_for_file(&mut index, &path, "new #go\n");

        assert!(index.by_tag.get("rust").is_none());
        assert_eq!(index.counts.get("go"), Some(&1));
        assert_eq!(index.revision, 2);
    }
}
