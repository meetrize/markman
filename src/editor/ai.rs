//! Editor AI actions, context collection, and preview application.

use std::ops::Range;
use std::path::{Path, PathBuf};

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::{CrossBlockSelection, Editor};
use crate::components::{
    AiExpandSelection, AiExplainSelection, AiImproveSelection, AiSummarizeSelection,
    AiTasksSelection, AskAi, BlockKind, UndoCaptureKind,
};
use crate::config::read_app_preferences;
use crate::net::ai::{self as ai_client, AiCompletionRequest};
use crate::theme::Theme;

const WORKSPACE_CONTEXT_FILE_LIMIT: usize = 8;
const WORKSPACE_CONTEXT_BYTES_PER_FILE: usize = 1200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiOperation {
    Improve,
    Summarize,
    Expand,
    Explain,
    Tasks,
}

#[derive(Clone, Debug)]
enum AiTarget {
    CrossBlockSelection(CrossBlockSelection),
    SingleBlockSelection {
        entity_id: EntityId,
        range: Range<usize>,
    },
    FullDocument,
    InsertOnly {
        after: Option<EntityId>,
    },
}

#[derive(Clone, Debug)]
struct AiPreview {
    operation: AiOperation,
    target: AiTarget,
    result_markdown: String,
}

#[derive(Default)]
pub(super) struct AiState {
    in_flight: bool,
    preview: Option<AiPreview>,
    error: Option<String>,
}

struct AiContext {
    target: AiTarget,
    context_markdown: String,
}

impl AiOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Improve => "Improve writing",
            Self::Summarize => "Summarize",
            Self::Expand => "Expand",
            Self::Explain => "Explain",
            Self::Tasks => "Turn into tasks",
        }
    }

    fn instruction(self) -> &'static str {
        match self {
            Self::Improve => {
                "Polish the Markdown while preserving meaning, structure, links, and code fences."
            }
            Self::Summarize => "Summarize the Markdown into concise notes with useful bullets.",
            Self::Expand => "Expand the Markdown with helpful detail while keeping the same topic.",
            Self::Explain => "Explain the Markdown clearly for a personal knowledge base note.",
            Self::Tasks => {
                "Convert the Markdown into an actionable task list using GitHub-flavored task items."
            }
        }
    }
}

impl Editor {
    pub(crate) fn on_ask_ai(
        &mut self,
        _: &AskAi,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Improve, window, cx);
    }

    pub(crate) fn on_ai_improve_selection(
        &mut self,
        _: &AiImproveSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Improve, window, cx);
    }

    pub(crate) fn on_ai_summarize_selection(
        &mut self,
        _: &AiSummarizeSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Summarize, window, cx);
    }

    pub(crate) fn on_ai_expand_selection(
        &mut self,
        _: &AiExpandSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Expand, window, cx);
    }

    pub(crate) fn on_ai_explain_selection(
        &mut self,
        _: &AiExplainSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Explain, window, cx);
    }

    pub(crate) fn on_ai_tasks_selection(
        &mut self,
        _: &AiTasksSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Tasks, window, cx);
    }

    fn request_ai_operation(
        &mut self,
        operation: AiOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.in_flight {
            return;
        }
        let preferences = match read_app_preferences() {
            Ok(preferences) => preferences,
            Err(err) => {
                self.ai.error = Some(format!("Failed to read AI preferences: {err}"));
                cx.notify();
                return;
            }
        };
        let context = match self.collect_ai_context(window, preferences.ai.allow_full_document_context, cx) {
            Ok(context) => context,
            Err(err) => {
                self.ai.error = Some(err);
                cx.notify();
                return;
            }
        };
        let mut context_markdown = context.context_markdown;
        if preferences.ai.allow_workspace_context {
            if let Some(workspace_context) = self.ai_workspace_context() {
                context_markdown.push_str("\n\n---\nWorkspace context:\n\n");
                context_markdown.push_str(&workspace_context);
            }
        }
        if preferences.ai.allow_command_context {
            if let Some(command_context) = self.ai_command_context(window, cx) {
                context_markdown.push_str("\n\n---\nCommand/code context:\n\n");
                context_markdown.push_str(&command_context);
            }
        }

        self.ai.in_flight = true;
        self.ai.preview = None;
        self.ai.error = None;
        let weak_editor = cx.entity().downgrade();
        let target = context.target;
        let (tx, rx) = oneshot::channel();
        std::thread::spawn(move || {
            let result = ai_client::complete_markdown(AiCompletionRequest {
                preferences: preferences.ai,
                instruction: operation.instruction().to_string(),
                context_markdown,
            });
            let _ = tx.send(result);
        });

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = rx
                .map(|result| {
                    result.unwrap_or_else(|_| Err(anyhow::anyhow!("AI worker ended early")))
                })
                .await;
            let _ = weak_editor.update(cx, move |editor, cx| {
                editor.ai.in_flight = false;
                match result {
                    Ok(result_markdown) => {
                        editor.ai.preview = Some(AiPreview {
                            operation,
                            target,
                            result_markdown,
                        });
                    }
                    Err(err) => editor.ai.error = Some(err.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn collect_ai_context(
        &self,
        window: &Window,
        allow_full_document_context: bool,
        cx: &App,
    ) -> Result<AiContext, String> {
        if let Some(selection) = self.cross_block_selection {
            if let Some(markdown) = self.cross_block_selected_markdown(cx) {
                return Ok(AiContext {
                    target: AiTarget::CrossBlockSelection(selection),
                    context_markdown: markdown,
                });
            }
        }

        if let Some(block) = self.focused_edit_target(window, cx) {
            let block_ref = block.read(cx);
            if !block_ref.selected_range.is_empty() {
                let text = block_ref.display_text().to_string();
                let range = block_ref.selected_range.start.min(text.len())
                    ..block_ref.selected_range.end.min(text.len());
                if let Some(selected) = text.get(range.clone()) {
                    return Ok(AiContext {
                        target: AiTarget::SingleBlockSelection {
                            entity_id: block.entity_id(),
                            range,
                        },
                        context_markdown: selected.to_string(),
                    });
                }
            }
            return Ok(AiContext {
                target: AiTarget::InsertOnly {
                    after: Some(self.root_ancestor_entity_id(block.entity_id())),
                },
                context_markdown: block_ref.display_text().to_string(),
            });
        }

        if !allow_full_document_context {
            return Err("Select text first, or enable full document context in AI preferences.".into());
        }
        Ok(AiContext {
            target: AiTarget::FullDocument,
            context_markdown: self.serialized_document_text(cx),
        })
    }

    fn ai_workspace_context(&self) -> Option<String> {
        let root = self.workspace_root_for_ai()?;
        let mut files = Vec::new();
        collect_markdown_files(&root, &mut files);
        files.sort();
        let mut output = String::new();
        for path in files.into_iter().take(WORKSPACE_CONTEXT_FILE_LIMIT) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let relative = path.strip_prefix(&root).unwrap_or(&path);
            output.push_str(&format!("## {}\n", relative.display()));
            output.push_str(first_markdown_excerpt(&text));
            output.push_str("\n\n");
        }
        (!output.trim().is_empty()).then_some(output)
    }

    fn ai_command_context(&self, window: &Window, cx: &App) -> Option<String> {
        let block = self.focused_edit_target(window, cx)?;
        let block = block.read(cx);
        match block.kind() {
            BlockKind::CodeBlock { language } => {
                let language = language
                    .as_ref()
                    .map(|language| language.as_ref())
                    .unwrap_or("text");
                Some(format!("```{language}\n{}\n```", block.display_text()))
            }
            _ => None,
        }
    }

    fn apply_ai_preview_replace(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.ai.preview.take() else {
            return;
        };
        self.apply_ai_result(preview.target, preview.result_markdown, true, cx);
    }

    fn apply_ai_preview_insert(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(preview) = self.ai.preview.take() else {
            return;
        };
        self.insert_ai_result_after_target(preview.target, &preview.result_markdown, cx);
    }

    fn dismiss_ai_preview(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.ai.preview = None;
        self.ai.error = None;
        cx.notify();
    }

    fn apply_ai_result(
        &mut self,
        target: AiTarget,
        result_markdown: String,
        allow_replace_full_document: bool,
        cx: &mut Context<Self>,
    ) {
        match target {
            AiTarget::CrossBlockSelection(selection) => {
                self.cross_block_selection = Some(selection);
                self.replace_cross_block_selection_with_text(
                    &result_markdown,
                    None,
                    true,
                    UndoCaptureKind::NonCoalescible,
                    cx,
                );
            }
            AiTarget::SingleBlockSelection { entity_id, range } => {
                if let Some(block) = self.focusable_entity_by_id(entity_id) {
                    block.update(cx, |block, cx| {
                        block.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                        block.replace_text_in_visible_range(range, &result_markdown, None, true, cx);
                    });
                    self.mark_dirty(cx);
                    cx.notify();
                }
            }
            AiTarget::FullDocument if allow_replace_full_document => {
                let roots = Self::build_root_blocks_from_markdown(cx, &result_markdown);
                if !roots.is_empty() {
                    self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                    self.document.replace_roots(roots, cx);
                    self.rebuild_table_runtimes(cx);
                    self.rebuild_image_runtimes(cx);
                    self.mark_dirty(cx);
                    self.finalize_pending_undo_capture(cx);
                    cx.notify();
                }
            }
            AiTarget::FullDocument | AiTarget::InsertOnly { .. } => {
                self.insert_ai_result_after_target(target, &result_markdown, cx);
            }
        }
    }

    fn insert_ai_result_after_target(
        &mut self,
        target: AiTarget,
        result_markdown: &str,
        cx: &mut Context<Self>,
    ) {
        let blocks = Self::build_root_blocks_from_markdown(cx, result_markdown);
        if blocks.is_empty() {
            return;
        }
        let after = match target {
            AiTarget::SingleBlockSelection { entity_id, .. } => Some(self.root_ancestor_entity_id(entity_id)),
            AiTarget::CrossBlockSelection(selection) => Some(self.root_ancestor_entity_id(selection.focus.entity_id)),
            AiTarget::InsertOnly { after } => after,
            AiTarget::FullDocument => self.active_entity_id.map(|id| self.root_ancestor_entity_id(id)),
        };
        let index = after
            .and_then(|entity_id| self.document.find_block_location(entity_id))
            .map(|location| location.index + 1)
            .unwrap_or_else(|| self.document.root_count());
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.document.insert_blocks_at(None, index, blocks, cx);
        self.rebuild_table_runtimes(cx);
        self.rebuild_image_runtimes(cx);
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }

    pub(super) fn render_ai_preview_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let title = if self.ai.in_flight {
            "AI is thinking".to_string()
        } else if let Some(preview) = &self.ai.preview {
            preview.operation.label().to_string()
        } else if self.ai.error.is_some() {
            "AI request failed".to_string()
        } else {
            return None;
        };
        let body = if self.ai.in_flight {
            "Generating Markdown preview...".to_string()
        } else if let Some(preview) = &self.ai.preview {
            preview.result_markdown.clone()
        } else {
            self.ai.error.clone().unwrap_or_default()
        };
        let has_preview = self.ai.preview.is_some();

        Some(
            div()
                .id("ai-preview-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .child(
                    div()
                        .id("ai-preview-dialog")
                        .w(px(d.dialog_width.max(560.0)))
                        .max_w(relative(0.86))
                        .max_h(relative(0.82))
                        .p(px(d.dialog_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.dialog_gap))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .child(
                            div()
                                .text_size(px(t.dialog_title_size))
                                .font_weight(t.dialog_title_weight.to_font_weight())
                                .text_color(c.dialog_title)
                                .child(title),
                        )
                        .child(
                            div()
                                .id("ai-preview-result-scroll")
                                .flex_1()
                                .min_h(px(120.0))
                                .max_h(px(360.0))
                                .overflow_y_scroll()
                                .p(px(12.0))
                                .rounded(px(d.menu_item_radius))
                                .bg(c.code_bg)
                                .text_size(px(t.dialog_body_size))
                                .text_color(c.code_text)
                                .child(body),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(d.dialog_button_gap))
                                .child(ai_dialog_button(
                                    "ai-preview-cancel",
                                    "Cancel",
                                    theme,
                                    cx.listener(Self::dismiss_ai_preview),
                                ))
                                .when(has_preview, |this| {
                                    this.child(ai_dialog_button(
                                        "ai-preview-insert",
                                        "Insert below",
                                        theme,
                                        cx.listener(Self::apply_ai_preview_insert),
                                    ))
                                    .child(ai_dialog_primary_button(
                                        "ai-preview-replace",
                                        "Replace",
                                        theme,
                                        cx.listener(Self::apply_ai_preview_replace),
                                    ))
                                }),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn ai_dialog_button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    div()
        .id(id)
        .h(px(d.dialog_button_height))
        .px(px(d.dialog_button_padding_x))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
        .border(px(d.dialog_border_width))
        .border_color(c.dialog_border)
        .bg(c.dialog_secondary_button_bg)
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .cursor_pointer()
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(c.dialog_secondary_button_text)
        .child(label)
        .on_click(on_click)
}

fn ai_dialog_primary_button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    div()
        .id(id)
        .h(px(d.dialog_button_height))
        .px(px(d.dialog_button_padding_x))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
        .bg(c.dialog_primary_button_bg)
        .hover(|this| this.bg(c.dialog_primary_button_hover))
        .cursor_pointer()
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(c.dialog_primary_button_text)
        .child(label)
        .on_click(on_click)
}

fn collect_markdown_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.') || name == "target")
        {
            continue;
        }
        if path.is_dir() {
            collect_markdown_files(&path, out);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "md" | "markdown"))
        {
            out.push(path);
        }
    }
}

fn first_markdown_excerpt(text: &str) -> &str {
    if text.len() <= WORKSPACE_CONTEXT_BYTES_PER_FILE {
        return text;
    }
    let mut end = WORKSPACE_CONTEXT_BYTES_PER_FILE;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
