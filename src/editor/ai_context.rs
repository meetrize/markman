//! Shared AI context collection for popup dialog and sidebar chat.

use std::ops::Range;

use gpui::*;

use super::{CrossBlockSelection, Editor};
use crate::components::Block;

/// Context mode aligned with popup prompt and sidebar chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiContextMode {
    Selection,
    #[default]
    FullDocument,
    Blank,
    /// Sidebar extension; full collection deferred to later steps.
    #[allow(dead_code)]
    Workspace,
    /// Sidebar extension; full collection deferred to later steps.
    #[allow(dead_code)]
    Command,
}

impl AiContextMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Selection => "引用选中文本",
            Self::FullDocument => "引用全文",
            Self::Blank => "全新对话",
            Self::Workspace => "引用工作区",
            Self::Command => "引用代码块",
        }
    }
}

/// Lightweight snapshot of editor context for AI requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiContextSnapshot {
    pub context_markdown: String,
    pub target_label: String,
    /// Basename of the document file when the snapshot was taken.
    pub source_file_name: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

impl AiContextSnapshot {
    pub fn format_reference_label(&self, untitled_document: &str) -> Option<String> {
        if let Some(label) = self.reference_display_label() {
            return Some(label);
        }
        if let (Some(start), Some(end)) = (self.start_line, self.end_line) {
            let file = self
                .source_file_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or(untitled_document);
            return Some(if start == end {
                format!("{file} L{start}")
            } else {
                format!("{file} L{start}-{end}")
            });
        }
        self.source_file_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                (!self.context_markdown.trim().is_empty()).then(|| self.target_label.clone())
            })
    }

    pub fn reference_display_label(&self) -> Option<String> {
        let file = self.source_file_name.as_deref()?;
        let start = self.start_line?;
        let end = self.end_line?;
        Some(if start == end {
            format!("{file} L{start}")
        } else {
            format!("{file} L{start}-{end}")
        })
    }
}

/// Resolved target for popup insert/replace flows.
#[derive(Clone, Debug)]
pub(in crate::editor) enum AiContextTarget {
    CrossBlock(CrossBlockSelection),
    SingleBlock {
        entity_id: EntityId,
        range: Range<usize>,
    },
    FullDocument,
    InsertOnly {
        after: Option<EntityId>,
    },
}

#[derive(Clone, Debug)]
pub(in crate::editor) struct AiCollectedContext {
    pub target: AiContextTarget,
    pub context_markdown: String,
    pub target_label: String,
}

impl AiCollectedContext {
    pub fn snapshot(&self) -> AiContextSnapshot {
        AiContextSnapshot {
            context_markdown: self.context_markdown.clone(),
            target_label: self.target_label.clone(),
            source_file_name: None,
            start_line: None,
            end_line: None,
        }
    }
}

/// Snapshot current editor selection for sidebar AI chat, including file and line range.
pub(in crate::editor) fn snapshot_editor_selection_context(
    editor: &Editor,
    window: &Window,
    cx: &App,
) -> Option<AiContextSnapshot> {
    let collected = collect_selection_context(editor, None, window, cx)?;
    let mut snapshot = collected.snapshot();
    snapshot.source_file_name = editor.document_file_display_name();
    if let Some((start, end)) = editor.selection_source_line_range(window, cx) {
        snapshot.start_line = Some(start);
        snapshot.end_line = Some(end);
    }
    Some(snapshot)
}

/// Collect context for a specific mode (popup prompt / sidebar).
#[allow(dead_code)]
pub fn collect_editor_ai_context(
    editor: &Editor,
    mode: AiContextMode,
    selection_override: Option<&AiContextSnapshot>,
    window: &Window,
    cx: &App,
) -> Result<AiContextSnapshot, String> {
    collect_editor_ai_context_full(editor, mode, selection_override, window, cx)
        .map(|context| context.snapshot())
}

pub(in crate::editor) fn collect_editor_ai_context_full(
    editor: &Editor,
    mode: AiContextMode,
    selection_override: Option<&AiContextSnapshot>,
    window: &Window,
    cx: &App,
) -> Result<AiCollectedContext, String> {
    match mode {
        AiContextMode::Selection => collect_selection_context(editor, selection_override, window, cx)
            .ok_or_else(|| "当前没有选中文本。".to_string()),
        AiContextMode::FullDocument => Ok(AiCollectedContext {
            target: AiContextTarget::FullDocument,
            context_markdown: editor.serialized_document_text(cx),
            target_label: "全文".to_string(),
        }),
        AiContextMode::Blank => Ok(AiCollectedContext {
            target: AiContextTarget::InsertOnly {
                after: editor
                    .active_entity_id
                    .map(|id| editor.root_ancestor_entity_id(id)),
            },
            context_markdown: String::new(),
            target_label: "空白".to_string(),
        }),
        AiContextMode::Workspace => Ok(AiCollectedContext {
            target: AiContextTarget::InsertOnly {
                after: editor
                    .active_entity_id
                    .map(|id| editor.root_ancestor_entity_id(id)),
            },
            context_markdown: String::new(),
            target_label: "工作区".to_string(),
        }),
        AiContextMode::Command => Ok(AiCollectedContext {
            target: AiContextTarget::InsertOnly {
                after: editor
                    .active_entity_id
                    .map(|id| editor.root_ancestor_entity_id(id)),
            },
            context_markdown: String::new(),
            target_label: "代码块".to_string(),
        }),
    }
}

/// Auto-detect context for toolbar quick actions (selection → block → full document).
#[allow(dead_code)]
pub fn collect_editor_ai_context_auto(
    editor: &Editor,
    allow_full_document_context: bool,
    window: &Window,
    cx: &App,
) -> Result<AiContextSnapshot, String> {
    collect_editor_ai_context_auto_full(editor, allow_full_document_context, window, cx)
        .map(|context| context.snapshot())
}

pub(in crate::editor) fn collect_editor_ai_context_auto_full(
    editor: &Editor,
    allow_full_document_context: bool,
    window: &Window,
    cx: &App,
) -> Result<AiCollectedContext, String> {
    if editor.cross_block_selection.is_some()
        && let Some(markdown) = editor.cross_block_selected_markdown(cx)
    {
        return Ok(AiCollectedContext {
            target: AiContextTarget::CrossBlock(editor.cross_block_selection.expect(
                "cross_block_selection checked above",
            )),
            context_markdown: markdown,
            target_label: "选中文本".to_string(),
        });
    }

    if let Some(block) = editor.focused_edit_target(window, cx) {
        let block_ref = block.read(cx);
        if !block_ref.selected_range.is_empty() {
            let text = block_ref.display_text().to_string();
            let range = block_ref.selected_range.start.min(text.len())
                ..block_ref.selected_range.end.min(text.len());
            if let Some(selected) = text.get(range.clone()) {
                return Ok(AiCollectedContext {
                    target: AiContextTarget::SingleBlock {
                        entity_id: block.entity_id(),
                        range,
                    },
                    context_markdown: selected.to_string(),
                    target_label: "选中文本".to_string(),
                });
            }
        }
        return Ok(AiCollectedContext {
            target: AiContextTarget::InsertOnly {
                after: Some(editor.root_ancestor_entity_id(block.entity_id())),
            },
            context_markdown: block_ref.display_text().to_string(),
            target_label: "当前块".to_string(),
        });
    }

    if !allow_full_document_context {
        return Err(
            "Select text first, or enable full document context in AI preferences.".into(),
        );
    }
    Ok(AiCollectedContext {
        target: AiContextTarget::FullDocument,
        context_markdown: editor.serialized_document_text(cx),
        target_label: "全文".to_string(),
    })
}

pub(in crate::editor) fn block_text_selection_range(block: &Block) -> Option<Range<usize>> {
    if !block.selected_range.is_empty() {
        Some(block.selected_range.clone())
    } else {
        block.editor_selection_range.clone()
    }
}

pub(in crate::editor) fn block_has_visible_text_selection(block: &Block) -> bool {
    block_text_selection_range(block).is_some()
}

fn collect_selection_context(
    editor: &Editor,
    selection_override: Option<&AiContextSnapshot>,
    window: &Window,
    cx: &App,
) -> Option<AiCollectedContext> {
    if let Some(override_) = selection_override {
        if override_.context_markdown.trim().is_empty() {
            return None;
        }
        return Some(AiCollectedContext {
            target: selection_target_from_editor(editor, window, cx).unwrap_or(
                AiContextTarget::InsertOnly { after: None },
            ),
            context_markdown: override_.context_markdown.clone(),
            target_label: override_.target_label.clone(),
        });
    }

    if editor.cross_block_selection.is_some()
        && let Some(markdown) = editor.cross_block_selected_markdown(cx)
    {
        return Some(AiCollectedContext {
            target: AiContextTarget::CrossBlock(editor.cross_block_selection.expect(
                "cross_block_selection checked above",
            )),
            context_markdown: markdown,
            target_label: "选中文本".to_string(),
        });
    }
    if let Some(block) = editor.focused_edit_target(window, cx) {
        let block_ref = block.read(cx);
        if let Some(range) = block_text_selection_range(block_ref) {
            let text = block_ref.display_text().to_string();
            let range = range.start.min(text.len())..range.end.min(text.len());
            if let Some(selected) = text.get(range.clone()) {
                return Some(AiCollectedContext {
                    target: AiContextTarget::SingleBlock {
                        entity_id: block.entity_id(),
                        range,
                    },
                    context_markdown: selected.to_string(),
                    target_label: "选中文本".to_string(),
                });
            }
        }
    }
    None
}

fn selection_target_from_editor(
    editor: &Editor,
    window: &Window,
    cx: &App,
) -> Option<AiContextTarget> {
    if editor.cross_block_selection.is_some()
        && editor.cross_block_selected_markdown(cx).is_some()
    {
        return Some(AiContextTarget::CrossBlock(
            editor.cross_block_selection.expect("cross_block_selection checked above"),
        ));
    }
    if let Some(block) = editor.focused_edit_target(window, cx) {
        let block_ref = block.read(cx);
        if !block_ref.selected_range.is_empty() {
            let text = block_ref.display_text().to_string();
            let range = block_ref.selected_range.start.min(text.len())
                ..block_ref.selected_range.end.min(text.len());
            if text.get(range.clone()).is_some() {
                return Some(AiContextTarget::SingleBlock {
                    entity_id: block.entity_id(),
                    range,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::AiContextSnapshot;

    #[test]
    fn reference_display_label_formats_single_and_multi_line_ranges() {
        let single = AiContextSnapshot {
            context_markdown: "hello".into(),
            target_label: "选中文本".into(),
            source_file_name: Some("note.md".into()),
            start_line: Some(3),
            end_line: Some(3),
        };
        assert_eq!(
            single.reference_display_label().as_deref(),
            Some("note.md L3")
        );
        assert_eq!(
            single.format_reference_label("未命名文档").as_deref(),
            Some("note.md L3")
        );

        let multi = AiContextSnapshot {
            context_markdown: "hello\nworld".into(),
            target_label: "选中文本".into(),
            source_file_name: Some("note.md".into()),
            start_line: Some(2),
            end_line: Some(4),
        };
        assert_eq!(
            multi.reference_display_label().as_deref(),
            Some("note.md L2-4")
        );
    }

    #[test]
    fn format_reference_label_uses_untitled_document_when_no_file_name() {
        let snapshot = AiContextSnapshot {
            context_markdown: "hello".into(),
            target_label: "选中文本".into(),
            source_file_name: None,
            start_line: Some(5),
            end_line: Some(7),
        };
        assert_eq!(
            snapshot.format_reference_label("Untitled").as_deref(),
            Some("Untitled L5-7")
        );
    }
}
