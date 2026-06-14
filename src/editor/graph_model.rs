//! Knowledge graph model built from workspace tag and wiki link indexes.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use super::link_index::WorkspaceLinkIndex;
use super::markdown_files::{collect_markdown_files, is_markdown_file};
use super::tag_index::WorkspaceTagIndex;

/// Stable node identifier in the knowledge graph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct GraphNodeId(String);

impl GraphNodeId {
    pub(crate) fn document(relative_path: &str) -> Self {
        Self(format!("doc:{relative_path}"))
    }

    pub(crate) fn tag(name: &str) -> Self {
        Self(format!("tag:{name}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GraphNodeKind {
    Document {
        path: PathBuf,
        relative_path: String,
        label: String,
    },
    Tag {
        name: String,
        count: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GraphFilter {
    /// Show every markdown document node plus tag nodes.
    All,
    /// Show only nodes that participate in at least one edge (default).
    #[default]
    ConnectedOnly,
}

pub(crate) fn apply_graph_filter(graph: &KnowledgeGraph, filter: GraphFilter) -> KnowledgeGraph {
    match filter {
        GraphFilter::All => graph.clone(),
        GraphFilter::ConnectedOnly => {
            if graph.edges.is_empty() {
                return KnowledgeGraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    broken_wiki_links: graph.broken_wiki_links,
                    revision: graph.revision,
                };
            }

            let connected: HashSet<_> = graph
                .edges
                .iter()
                .flat_map(|edge| [edge.source.clone(), edge.target.clone()])
                .collect();
            let nodes: Vec<_> = graph
                .nodes
                .iter()
                .filter(|node| connected.contains(&node.id))
                .cloned()
                .collect();
            let node_ids: HashSet<_> = nodes.iter().map(|node| node.id.clone()).collect();
            let edges: Vec<_> = graph
                .edges
                .iter()
                .filter(|edge| {
                    node_ids.contains(&edge.source) && node_ids.contains(&edge.target)
                })
                .cloned()
                .collect();

            KnowledgeGraph {
                nodes,
                edges,
                broken_wiki_links: graph.broken_wiki_links,
                revision: graph.revision,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GraphEdgeKind {
    Tagged,
    WikiLink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GraphNode {
    pub id: GraphNodeId,
    pub kind: GraphNodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GraphEdge {
    pub source: GraphNodeId,
    pub target: GraphNodeId,
    pub kind: GraphEdgeKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KnowledgeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub broken_wiki_links: usize,
    pub revision: u64,
}

pub(crate) fn build_knowledge_graph(
    workspace_root: &Path,
    tag_index: &WorkspaceTagIndex,
    link_index: &WorkspaceLinkIndex,
) -> KnowledgeGraph {
    let mut document_paths: BTreeMap<String, PathBuf> = BTreeMap::new();

    let mut files = Vec::new();
    collect_markdown_files(workspace_root, &mut files);
    for path in files {
        if let Some(relative) = relative_markdown_path(workspace_root, &path) {
            document_paths.insert(relative, path);
        }
    }

    for occurrences in tag_index.by_tag.values() {
        for occurrence in occurrences {
            if let Some(relative) = relative_markdown_path(workspace_root, &occurrence.path) {
                document_paths
                    .entry(relative)
                    .or_insert_with(|| occurrence.path.clone());
            }
        }
    }

    for occurrences in link_index.by_source.values() {
        for occurrence in occurrences {
            if let Some(relative) = relative_markdown_path(workspace_root, &occurrence.source_path)
            {
                document_paths
                    .entry(relative)
                    .or_insert_with(|| occurrence.source_path.clone());
            }
        }
    }

    let mut nodes: Vec<GraphNode> = document_paths
        .iter()
        .map(|(relative_path, path)| GraphNode {
            id: GraphNodeId::document(relative_path),
            kind: GraphNodeKind::Document {
                path: path.clone(),
                relative_path: relative_path.clone(),
                label: document_label(relative_path),
            },
        })
        .collect();

    for (name, count) in &tag_index.counts {
        nodes.push(GraphNode {
            id: GraphNodeId::tag(name),
            kind: GraphNodeKind::Tag {
                name: name.clone(),
                count: *count,
            },
        });
    }

    nodes.sort_by(|left, right| left.id.cmp(&right.id));

    let mut edges = BTreeSet::new();
    let mut broken_wiki_links = 0usize;

    for (tag_name, occurrences) in &tag_index.by_tag {
        let mut tagged_sources = BTreeSet::new();
        for occurrence in occurrences {
            if let Some(relative) = relative_markdown_path(workspace_root, &occurrence.path) {
                tagged_sources.insert(relative);
            }
        }
        let tag_id = GraphNodeId::tag(tag_name);
        for relative in tagged_sources {
            edges.insert((
                GraphNodeId::document(&relative),
                tag_id.clone(),
                GraphEdgeKind::Tagged,
            ));
        }
    }

    for occurrences in link_index.by_source.values() {
        let mut wiki_pairs = BTreeSet::new();
        for occurrence in occurrences {
            let Some(source_relative) =
                relative_markdown_path(workspace_root, &occurrence.source_path)
            else {
                continue;
            };
            let Some(target_path) =
                resolve_wiki_target(workspace_root, &occurrence.target_path)
            else {
                broken_wiki_links += 1;
                continue;
            };
            let Some(target_relative) = relative_markdown_path(workspace_root, &target_path) else {
                broken_wiki_links += 1;
                continue;
            };
            wiki_pairs.insert((source_relative, target_relative));
        }
        for (source_relative, target_relative) in wiki_pairs {
            edges.insert((
                GraphNodeId::document(&source_relative),
                GraphNodeId::document(&target_relative),
                GraphEdgeKind::WikiLink,
            ));
        }
    }

    let edges = edges
        .into_iter()
        .map(|(source, target, kind)| GraphEdge {
            source,
            target,
            kind,
        })
        .collect();

    KnowledgeGraph {
        nodes,
        edges,
        broken_wiki_links,
        revision: tag_index
            .revision
            .wrapping_mul(1_000)
            .wrapping_add(link_index.revision),
    }
}

pub(crate) fn workspace_document_node_id(
    workspace_root: &Path,
    absolute_path: &Path,
) -> Option<GraphNodeId> {
    relative_markdown_path(workspace_root, absolute_path)
        .map(|relative| GraphNodeId::document(&relative))
}

fn relative_markdown_path(workspace_root: &Path, absolute: &Path) -> Option<String> {
    absolute
        .strip_prefix(workspace_root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn document_label(relative_path: &str) -> String {
    Path::new(relative_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| relative_path.to_string())
}

fn resolve_wiki_target(workspace_root: &Path, target_path: &str) -> Option<PathBuf> {
    let trimmed = target_path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let direct = workspace_root.join(trimmed);
    if direct.is_file() && is_markdown_file(&direct) {
        return Some(direct);
    }

    if !trimmed.ends_with(".md") {
        let with_md = workspace_root.join(format!("{trimmed}.md"));
        if with_md.is_file() {
            return Some(with_md);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::editor::link_index::{refresh_link_index_for_file, WorkspaceLinkIndex};
    use crate::editor::tag_index::{refresh_tag_index_for_file, WorkspaceTagIndex};

    fn temp_workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "markman-graph-model-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn build_graph_merges_shared_tag_and_wiki_link() {
        let root = temp_workspace("shared-tag-wiki");
        let note_a = root.join("a.md");
        let note_b = root.join("b.md");
        std::fs::write(&note_a, "topic #shared\nsee [[b.md]]\n").expect("write a");
        std::fs::write(&note_b, "also #shared\n").expect("write b");

        let mut tag_index = WorkspaceTagIndex::default();
        refresh_tag_index_for_file(&mut tag_index, &note_a, "topic #shared\nsee [[b.md]]\n");
        refresh_tag_index_for_file(&mut tag_index, &note_b, "also #shared\n");

        let mut link_index = WorkspaceLinkIndex::default();
        refresh_link_index_for_file(
            &mut link_index,
            &note_a,
            "topic #shared\nsee [[b.md]]\n",
        );

        let graph = build_knowledge_graph(&root, &tag_index, &link_index);

        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.edges.len(), 3);
        assert_eq!(graph.broken_wiki_links, 0);
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::Tagged
                && edge.source == GraphNodeId::document("a.md")
                && edge.target == GraphNodeId::tag("shared")
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::Tagged
                && edge.source == GraphNodeId::document("b.md")
                && edge.target == GraphNodeId::tag("shared")
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == GraphEdgeKind::WikiLink
                && edge.source == GraphNodeId::document("a.md")
                && edge.target == GraphNodeId::document("b.md")
        }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_tag_in_same_file_produces_single_edge() {
        let root = temp_workspace("duplicate-tag");
        let note = root.join("note.md");
        std::fs::write(&note, "#rust and #rust again\n").expect("write note");

        let mut tag_index = WorkspaceTagIndex::default();
        refresh_tag_index_for_file(&mut tag_index, &note, "#rust and #rust again\n");

        let link_index = WorkspaceLinkIndex::default();
        let graph = build_knowledge_graph(&root, &tag_index, &link_index);

        assert_eq!(tag_index.counts.get("rust"), Some(&2));
        assert_eq!(
            graph
                .edges
                .iter()
                .filter(|edge| edge.kind == GraphEdgeKind::Tagged)
                .count(),
            1
        );
        assert_eq!(
            graph.edges[0],
            GraphEdge {
                source: GraphNodeId::document("note.md"),
                target: GraphNodeId::tag("rust"),
                kind: GraphEdgeKind::Tagged,
            }
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn connected_only_filter_hides_isolated_documents() {
        let root = temp_workspace("connected-filter");
        let note_a = root.join("a.md");
        let note_b = root.join("b.md");
        let note_c = root.join("c.md");
        std::fs::write(&note_a, "see [[b.md]]\n").expect("write a");
        std::fs::write(&note_b, "linked back\n").expect("write b");
        std::fs::write(&note_c, "isolated\n").expect("write c");

        let tag_index = WorkspaceTagIndex::default();
        let mut link_index = WorkspaceLinkIndex::default();
        refresh_link_index_for_file(&mut link_index, &note_a, "see [[b.md]]\n");

        let graph = build_knowledge_graph(&root, &tag_index, &link_index);
        let filtered = apply_graph_filter(&graph, GraphFilter::ConnectedOnly);

        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(filtered.nodes.len(), 2);
        assert!(filtered
            .nodes
            .iter()
            .all(|node| node.id != GraphNodeId::document("c.md")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_wiki_links_are_counted_and_skipped() {
        let root = temp_workspace("broken-wiki");
        let note = root.join("note.md");
        std::fs::write(&note, "missing [[ghost.md]]\n").expect("write note");

        let tag_index = WorkspaceTagIndex::default();
        let mut link_index = WorkspaceLinkIndex::default();
        refresh_link_index_for_file(&mut link_index, &note, "missing [[ghost.md]]\n");

        let graph = build_knowledge_graph(&root, &tag_index, &link_index);

        assert_eq!(graph.broken_wiki_links, 1);
        assert!(graph
            .edges
            .iter()
            .all(|edge| edge.kind != GraphEdgeKind::WikiLink));

        let _ = std::fs::remove_dir_all(&root);
    }
}
