//! Editor AI actions, context collection, and preview application.

use std::borrow::Cow;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::{CrossBlockSelection, Editor};
use super::single_line_input::{
    cursor_offset, handle_mouse_down, handle_mouse_move, handle_mouse_up, select_caret_to,
    text_grapheme_boundary, SingleLineArrowKey,
};
use crate::components::{
    AiExpandSelection, AiExplainSelection, AiImproveSelection, AiSummarizeSelection,
    AiTasksSelection, AskAi, Copy, Cut, Delete, DeleteBack, End, Home, MoveLeft, MoveRight, Paste,
    SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight, BlockKind, UndoCaptureKind,
};
use crate::config::read_app_preferences;
use crate::net::ai::{self as ai_client, AiCompletionRequest};
use crate::theme::Theme;

const WORKSPACE_CONTEXT_FILE_LIMIT: usize = 8;
const WORKSPACE_CONTEXT_BYTES_PER_FILE: usize = 1200;

const ICON_AI_CUSTOM: &str = "icon/toolbar/sparkles.svg";
const ICON_AI_IMPROVE: &str = "icon/toolbar/wand-sparkles.svg";
const ICON_AI_SUMMARIZE: &str = "icon/toolbar/list-collapse.svg";
const ICON_AI_EXPAND: &str = "icon/toolbar/maximize-2.svg";
const ICON_AI_EXPLAIN: &str = "icon/toolbar/circle-help.svg";
const ICON_AI_TASKS: &str = "icon/toolbar/list-checks.svg";

#[derive(Clone, Debug, PartialEq, Eq)]
enum AiOperation {
    Improve,
    Summarize,
    Expand,
    Explain,
    Tasks,
    Custom(String),
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

pub(super) struct AiState {
    in_flight: bool,
    preview: Option<AiPreview>,
    error: Option<String>,
    prompt_open: bool,
    prompt_text: String,
    prompt_focus: FocusHandle,
    prompt_selected_range: Range<usize>,
    prompt_selection_reversed: bool,
    prompt_marked_range: Option<Range<usize>>,
    prompt_is_selecting: bool,
    prompt_line_layouts: Vec<(usize, ShapedLine)>,
    prompt_line_height: Pixels,
    prompt_last_bounds: Option<Bounds<Pixels>>,
    prompt_cursor_blink_epoch: Instant,
    prompt_cursor_blink_task: Option<Task<()>>,
}

struct AiContext {
    target: AiTarget,
    context_markdown: String,
}

impl AiState {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            in_flight: false,
            preview: None,
            error: None,
            prompt_open: false,
            prompt_text: String::new(),
            prompt_focus: cx.focus_handle(),
            prompt_selected_range: 0..0,
            prompt_selection_reversed: false,
            prompt_marked_range: None,
            prompt_is_selecting: false,
            prompt_line_layouts: Vec::new(),
            prompt_line_height: px(20.0),
            prompt_last_bounds: None,
            prompt_cursor_blink_epoch: Instant::now(),
            prompt_cursor_blink_task: None,
        }
    }
}

impl AiOperation {
    fn label(&self) -> Cow<'static, str> {
        match self {
            Self::Improve => Cow::Borrowed("润色"),
            Self::Summarize => Cow::Borrowed("总结"),
            Self::Expand => Cow::Borrowed("扩写"),
            Self::Explain => Cow::Borrowed("解释"),
            Self::Tasks => Cow::Borrowed("转任务"),
            Self::Custom(_) => Cow::Borrowed("自定义 AI"),
        }
    }

    fn instruction(&self) -> Cow<'_, str> {
        match self {
            Self::Improve => Cow::Borrowed(
                "Polish the Markdown while preserving meaning, structure, links, and code fences."
            ),
            Self::Summarize => Cow::Borrowed("Summarize the Markdown into concise notes with useful bullets."),
            Self::Expand => Cow::Borrowed("Expand the Markdown with helpful detail while keeping the same topic."),
            Self::Explain => Cow::Borrowed("Explain the Markdown clearly for a personal knowledge base note."),
            Self::Tasks => Cow::Borrowed(
                "Convert the Markdown into an actionable task list using GitHub-flavored task items."
            ),
            Self::Custom(prompt) => Cow::Owned(prompt.clone()),
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
        self.open_ai_prompt_dialog(window, cx);
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

    fn open_ai_prompt_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);
        self.ai.prompt_open = true;
        self.ai.prompt_text.clear();
        self.ai.prompt_selected_range = 0..0;
        self.ai.prompt_selection_reversed = false;
        self.ai.prompt_marked_range = None;
        self.ai.prompt_is_selecting = false;
        self.ai.prompt_cursor_blink_epoch = Instant::now();
        window.focus(&self.ai.prompt_focus);
        cx.notify();
    }

    fn close_ai_prompt_dialog(&mut self, cx: &mut Context<Self>) {
        self.ai.prompt_open = false;
        self.ai.prompt_marked_range = None;
        self.ai.prompt_is_selecting = false;
        self.ai.prompt_cursor_blink_task = None;
        cx.notify();
    }

    fn confirm_ai_prompt_dialog(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = self.ai.prompt_text.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.close_ai_prompt_dialog(cx);
        self.request_ai_operation(AiOperation::Custom(prompt), window, cx);
    }

    fn cancel_ai_prompt_dialog(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_ai_prompt_dialog(cx);
    }

    pub(super) fn ai_prompt_focus_handle(&self) -> FocusHandle {
        self.ai.prompt_focus.clone()
    }

    pub(super) fn ai_prompt_input_active(&self, window: &Window) -> bool {
        self.ai.prompt_open && self.ai.prompt_focus.is_focused(window)
    }

    pub(super) fn sync_ai_prompt_cursor_blink(
        &mut self,
        focused: bool,
        cx: &mut Context<Self>,
    ) {
        if focused && self.ai.prompt_cursor_blink_task.is_none() {
            self.ai.prompt_cursor_blink_epoch = Instant::now();
            self.ai.prompt_cursor_blink_task = Some(cx.spawn(
                async |this: WeakEntity<Editor>, cx: &mut AsyncApp| loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(33))
                        .await;
                    if this
                        .update(cx, |editor, cx| {
                            if editor.ai.prompt_cursor_blink_epoch.elapsed().as_secs_f32() >= 0.5 {
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                },
            ));
        } else if !focused && self.ai.prompt_cursor_blink_task.is_some() {
            self.ai.prompt_cursor_blink_task = None;
        }
    }

    fn ai_prompt_cursor_opacity(&self) -> f32 {
        let elapsed = self.ai.prompt_cursor_blink_epoch.elapsed().as_secs_f32();
        if elapsed < 0.5 {
            return 1.0;
        }
        let t = elapsed - 0.5;
        ((t * std::f32::consts::TAU).cos() * 0.5 + 0.5).clamp(0.0, 1.0)
    }

    fn reset_ai_prompt_cursor_blink(&mut self) {
        self.ai.prompt_cursor_blink_epoch = Instant::now();
    }

    pub(super) fn ai_prompt_is_open(&self) -> bool {
        self.ai.prompt_open
    }

    pub(super) fn ai_prompt_text(&self) -> &str {
        &self.ai.prompt_text
    }

    pub(super) fn ai_prompt_marked_range(&self) -> Option<Range<usize>> {
        self.ai.prompt_marked_range.clone()
    }

    pub(super) fn ai_prompt_selected_range(&self) -> Range<usize> {
        self.ai.prompt_selected_range.clone()
    }

    pub(super) fn ai_prompt_selection_reversed(&self) -> bool {
        self.ai.prompt_selection_reversed
    }

    pub(super) fn ai_prompt_cursor_offset(&self) -> usize {
        cursor_offset(
            &self.ai.prompt_selected_range,
            self.ai.prompt_selection_reversed,
        )
    }

    pub(super) fn set_ai_prompt_layout(
        &mut self,
        lines: Vec<(usize, ShapedLine)>,
        line_height: Pixels,
        bounds: Bounds<Pixels>,
    ) {
        self.ai.prompt_line_layouts = lines;
        self.ai.prompt_line_height = line_height;
        self.ai.prompt_last_bounds = Some(bounds);
    }

    pub(super) fn ai_prompt_line_layouts(&self) -> &[(usize, ShapedLine)] {
        &self.ai.prompt_line_layouts
    }

    pub(super) fn ai_prompt_line_height(&self) -> Pixels {
        self.ai.prompt_line_height
    }

    pub(super) fn ai_prompt_offset_for_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.ai.prompt_last_bounds else {
            return self.ai.prompt_text.len();
        };
        if self.ai.prompt_line_layouts.is_empty() {
            return self.ai.prompt_text.len();
        }
        let local_y = (position.y - bounds.top()).max(px(0.0));
        let line_index = (f32::from(local_y) / f32::from(self.ai.prompt_line_height.max(px(1.0))))
            .floor()
            .max(0.0) as usize;
        let line_index = line_index.min(self.ai.prompt_line_layouts.len().saturating_sub(1));
        let (start, line) = &self.ai.prompt_line_layouts[line_index];
        let local_x = position.x - bounds.left();
        let line_offset = line.closest_index_for_x(local_x.max(px(0.0)));
        (*start + line_offset).min(self.ai.prompt_text.len())
    }

    pub(super) fn unmark_ai_prompt_text(&mut self) {
        self.ai.prompt_marked_range = None;
    }

    pub(super) fn on_ai_prompt_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.ai.prompt_focus);
        let offset = self.ai_prompt_offset_for_position(event.position);
        handle_mouse_down(
            event.modifiers.shift,
            offset,
            self.ai.prompt_text.len(),
            &mut self.ai.prompt_selected_range,
            &mut self.ai.prompt_selection_reversed,
            &mut self.ai.prompt_marked_range,
            &mut self.ai.prompt_is_selecting,
        );
        self.reset_ai_prompt_cursor_blink();
        cx.notify();
    }

    pub(super) fn on_ai_prompt_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handle_mouse_up(&mut self.ai.prompt_is_selecting) {
            cx.notify();
        }
    }

    pub(super) fn on_ai_prompt_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.ai_prompt_offset_for_position(event.position);
        if handle_mouse_move(
            event.dragging(),
            offset,
            self.ai.prompt_text.len(),
            self.ai.prompt_is_selecting,
            &mut self.ai.prompt_selected_range,
            &mut self.ai.prompt_selection_reversed,
            &mut self.ai.prompt_marked_range,
            &mut self.ai.prompt_is_selecting,
        ) {
            self.reset_ai_prompt_cursor_blink();
            cx.notify();
        }
    }

    pub(super) fn replace_ai_prompt_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        mark_inserted_text: bool,
        selected_after: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let replacement = normalize_multiline_text(new_text);
        let start = range.start.min(self.ai.prompt_text.len());
        let end = range.end.min(self.ai.prompt_text.len());
        self.ai.prompt_text.replace_range(start..end, &replacement);
        let inserted_end = start + replacement.len();
        self.ai.prompt_selected_range = selected_after.unwrap_or(inserted_end..inserted_end);
        self.ai.prompt_selection_reversed = false;
        self.ai.prompt_marked_range = if mark_inserted_text && !replacement.is_empty() {
            Some(start..inserted_end)
        } else {
            None
        };
        self.reset_ai_prompt_cursor_blink();
        cx.notify();
    }

    pub(crate) fn on_ai_prompt_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let primary = (modifiers.platform || modifiers.control) && !modifiers.alt && !modifiers.function;
        if event.keystroke.key == "enter" && primary {
            self.confirm_ai_prompt_from_keyboard(window, cx);
            cx.stop_propagation();
        } else if event.keystroke.key == "enter" {
            let range = self
                .ai
                .prompt_marked_range
                .clone()
                .unwrap_or_else(|| self.ai.prompt_selected_range.clone());
            self.replace_ai_prompt_text(range, "\n", false, None, cx);
            cx.stop_propagation();
        } else if event.keystroke.key == "escape" {
            self.close_ai_prompt_dialog(cx);
            cx.stop_propagation();
        }
    }

    fn confirm_ai_prompt_from_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.ai.prompt_text.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.close_ai_prompt_dialog(cx);
        self.request_ai_operation(AiOperation::Custom(prompt), window, cx);
    }

    pub(crate) fn on_ai_prompt_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        let range = if self.ai.prompt_selected_range.is_empty() {
            let cursor = self.ai_prompt_cursor_offset();
            text_grapheme_boundary(&self.ai.prompt_text, cursor, true)..cursor
        } else {
            self.ai.prompt_selected_range.clone()
        };
        self.replace_ai_prompt_text(range, "", false, None, cx);
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_prompt_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        let range = if self.ai.prompt_selected_range.is_empty() {
            let cursor = self.ai_prompt_cursor_offset();
            cursor..text_grapheme_boundary(&self.ai.prompt_text, cursor, false)
        } else {
            self.ai.prompt_selected_range.clone()
        };
        self.replace_ai_prompt_text(range, "", false, None, cx);
        cx.stop_propagation();
    }

    fn move_ai_prompt_caret(&mut self, key: SingleLineArrowKey, cx: &mut Context<Self>) {
        let len = self.ai.prompt_text.len();
        let cursor = self.ai_prompt_cursor_offset();
        let next = match key {
            SingleLineArrowKey::MoveLeft | SingleLineArrowKey::SelectLeft => {
                text_grapheme_boundary(&self.ai.prompt_text, cursor, true)
            }
            SingleLineArrowKey::MoveRight | SingleLineArrowKey::SelectRight => {
                text_grapheme_boundary(&self.ai.prompt_text, cursor, false)
            }
            SingleLineArrowKey::Home | SingleLineArrowKey::SelectHome => 0,
            SingleLineArrowKey::End | SingleLineArrowKey::SelectEnd => len,
        };
        match key {
            SingleLineArrowKey::SelectLeft
            | SingleLineArrowKey::SelectRight
            | SingleLineArrowKey::SelectHome
            | SingleLineArrowKey::SelectEnd => {
                select_caret_to(
                    &mut self.ai.prompt_selected_range,
                    &mut self.ai.prompt_selection_reversed,
                    &mut self.ai.prompt_marked_range,
                    next,
                    len,
                );
            }
            _ => {
                self.ai.prompt_selected_range = next..next;
                self.ai.prompt_selection_reversed = false;
                self.ai.prompt_marked_range = None;
            }
        }
        self.reset_ai_prompt_cursor_blink();
        cx.notify();
    }

    fn on_ai_prompt_arrow_action(
        &mut self,
        key: SingleLineArrowKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        self.move_ai_prompt_caret(key, cx);
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_prompt_move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::MoveLeft, window, cx);
    }

    pub(crate) fn on_ai_prompt_move_right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::MoveRight, window, cx);
    }

    pub(crate) fn on_ai_prompt_home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::Home, window, cx);
    }

    pub(crate) fn on_ai_prompt_end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::End, window, cx);
    }

    pub(crate) fn on_ai_prompt_select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::SelectLeft, window, cx);
    }

    pub(crate) fn on_ai_prompt_select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::SelectRight, window, cx);
    }

    pub(crate) fn on_ai_prompt_select_home(&mut self, _: &SelectHome, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::SelectHome, window, cx);
    }

    pub(crate) fn on_ai_prompt_select_end(&mut self, _: &SelectEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.on_ai_prompt_arrow_action(SingleLineArrowKey::SelectEnd, window, cx);
    }

    pub(crate) fn on_ai_prompt_select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        self.ai.prompt_selected_range = 0..self.ai.prompt_text.len();
        self.ai.prompt_selection_reversed = false;
        self.ai.prompt_marked_range = None;
        cx.notify();
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_prompt_copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        if !self.ai.prompt_selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.ai.prompt_text[self.ai.prompt_selected_range.clone()].to_string(),
            ));
        }
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_prompt_cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        if !self.ai.prompt_selected_range.is_empty() {
            let range = self.ai.prompt_selected_range.clone();
            cx.write_to_clipboard(ClipboardItem::new_string(self.ai.prompt_text[range.clone()].to_string()));
            self.replace_ai_prompt_text(range, "", false, None, cx);
        }
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_prompt_paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_prompt_input_active(window) {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let range = self
            .ai
            .prompt_marked_range
            .clone()
            .unwrap_or_else(|| self.ai.prompt_selected_range.clone());
        self.replace_ai_prompt_text(range, &text, false, None, cx);
        cx.stop_propagation();
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
        let worker_operation = operation.clone();
        std::thread::spawn(move || {
            let result = ai_client::complete_markdown(AiCompletionRequest {
                preferences: preferences.ai,
                instruction: worker_operation.instruction().to_string(),
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

    fn ai_selection_toolbar_position(&self, window: &Window, cx: &App) -> Option<Point<Pixels>> {
        let has_selection = self.cross_block_selected_markdown(cx).is_some()
            || self
                .focused_edit_target(window, cx)
                .is_some_and(|block| !block.read(cx).selected_range.is_empty());
        if !has_selection {
            return None;
        }
        let entity_id = self
            .cross_block_selection
            .map(|selection| selection.anchor.entity_id)
            .or(self.active_entity_id)?;
        let block = self.document.block_entity_by_id(entity_id)?;
        let bounds = block.read(cx).last_bounds.or(block.read(cx).interaction_bounds)?;
        Some(point(
            bounds.left(),
            px((f32::from(bounds.top()) - 42.0).max(8.0)),
        ))
    }

    pub(super) fn render_ai_floating_toolbar(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.ai.prompt_open || self.ai.preview.is_some() || self.ai.in_flight || self.ai.error.is_some() {
            return None;
        }
        let position = self.ai_selection_toolbar_position(window, cx)?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        Some(
            div()
                .id("ai-selection-toolbar")
                .absolute()
                .left(position.x)
                .top(position.y)
                .h(px(34.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .occlude()
                .rounded(px(d.menu_panel_radius))
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .shadow_lg()
                .child(ai_toolbar_action_button(
                    "ai-floating-improve",
                    ICON_AI_IMPROVE,
                    "润色",
                    theme,
                    cx.listener(Self::on_context_menu_ai_improve),
                ))
                .child(ai_toolbar_action_button(
                    "ai-floating-summarize",
                    ICON_AI_SUMMARIZE,
                    "总结",
                    theme,
                    cx.listener(Self::on_context_menu_ai_summarize),
                ))
                .child(ai_toolbar_action_button(
                    "ai-floating-expand",
                    ICON_AI_EXPAND,
                    "扩写",
                    theme,
                    cx.listener(Self::on_context_menu_ai_expand),
                ))
                .child(ai_toolbar_action_button(
                    "ai-floating-explain",
                    ICON_AI_EXPLAIN,
                    "解释",
                    theme,
                    cx.listener(Self::on_context_menu_ai_explain),
                ))
                .child(ai_toolbar_action_button(
                    "ai-floating-tasks",
                    ICON_AI_TASKS,
                    "任务",
                    theme,
                    cx.listener(Self::on_context_menu_ai_tasks),
                ))
                .into_any_element(),
        )
    }

    pub(super) fn render_ai_prompt_dialog_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.ai.prompt_open {
            return None;
        }
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let can_submit = !self.ai.prompt_text.trim().is_empty();
        Some(
            div()
                .id("ai-prompt-overlay")
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
                        .id("ai-prompt-dialog")
                        .w(px(d.dialog_width.max(560.0)))
                        .max_w(relative(0.86))
                        .p(px(d.dialog_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.dialog_gap))
                        .key_context("BlockEditor")
                        .track_focus(&self.ai.prompt_focus)
                        .on_key_down(cx.listener(Self::on_ai_prompt_key_down))
                        .on_action(cx.listener(Self::on_ai_prompt_delete_back))
                        .on_action(cx.listener(Self::on_ai_prompt_delete_forward))
                        .on_action(cx.listener(Self::on_ai_prompt_paste))
                        .on_action(cx.listener(Self::on_ai_prompt_copy))
                        .on_action(cx.listener(Self::on_ai_prompt_cut))
                        .on_action(cx.listener(Self::on_ai_prompt_select_all))
                        .on_action(cx.listener(Self::on_ai_prompt_move_left))
                        .on_action(cx.listener(Self::on_ai_prompt_move_right))
                        .on_action(cx.listener(Self::on_ai_prompt_home))
                        .on_action(cx.listener(Self::on_ai_prompt_end))
                        .on_action(cx.listener(Self::on_ai_prompt_select_left))
                        .on_action(cx.listener(Self::on_ai_prompt_select_right))
                        .on_action(cx.listener(Self::on_ai_prompt_select_home))
                        .on_action(cx.listener(Self::on_ai_prompt_select_end))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.0))
                                .text_size(px(t.dialog_title_size))
                                .font_weight(t.dialog_title_weight.to_font_weight())
                                .text_color(c.dialog_title)
                                .child(svg().path(ICON_AI_CUSTOM).size(px(18.0)).text_color(c.dialog_title))
                                .child("自定义 AI 提示词"),
                        )
                        .child(
                            div()
                                .text_size(px(t.dialog_body_size))
                                .text_color(c.dialog_body)
                                .child("将引用当前选区或当前块作为上下文。请输入你希望 AI 执行的指令。"),
                        )
                        .child(
                            div()
                                .id("ai-prompt-input")
                                .h(px(140.0))
                                .px(px(12.0))
                                .flex()
                                .items_center()
                                .rounded(px(d.menu_item_radius))
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .bg(c.editor_background)
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .h_full()
                                        .overflow_hidden()
                                        .child(AiPromptTextAreaElement::new(
                                            cx.entity(),
                                            SharedString::from("例如：把这段内容改写得更正式"),
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(d.dialog_button_gap))
                                .child(ai_dialog_button(
                                    "ai-prompt-cancel",
                                    "取消",
                                    theme,
                                    cx.listener(Self::cancel_ai_prompt_dialog),
                                ))
                                .child(if can_submit {
                                    ai_dialog_primary_button(
                                        "ai-prompt-submit",
                                        "发送",
                                        theme,
                                        cx.listener(Self::confirm_ai_prompt_dialog),
                                    )
                                    .into_any_element()
                                } else {
                                    ai_dialog_disabled_button("ai-prompt-submit-disabled", "发送", theme)
                                }),
                        ),
                )
                .into_any_element(),
        )
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
            "AI 正在思考".to_string()
        } else if let Some(preview) = &self.ai.preview {
            preview.operation.label().to_string()
        } else if self.ai.error.is_some() {
            "AI 请求失败".to_string()
        } else {
            return None;
        };
        let body = if self.ai.in_flight {
            "正在生成 Markdown 预览...".to_string()
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
                                    "取消",
                                    theme,
                                    cx.listener(Self::dismiss_ai_preview),
                                ))
                                .when(has_preview, |this| {
                                    this.child(ai_dialog_button(
                                        "ai-preview-insert",
                                        "插入下方",
                                        theme,
                                        cx.listener(Self::apply_ai_preview_insert),
                                    ))
                                    .child(ai_dialog_primary_button(
                                        "ai-preview-replace",
                                        "替换",
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

fn ai_toolbar_action_button(
    id: &'static str,
    icon_path: &'static str,
    label: &'static str,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    div()
        .id(id)
        .h(px(26.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .rounded(px(d.format_toolbar_button_radius))
        .bg(c.dialog_surface)
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .active(|this| this.opacity(0.92))
        .cursor_pointer()
        .text_size(px(12.0))
        .text_color(c.dialog_secondary_button_text)
        .child(svg().path(icon_path).size(px(14.0)).text_color(c.dialog_secondary_button_text))
        .child(label)
        .on_click(on_click)
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

fn ai_dialog_disabled_button(id: &'static str, label: &'static str, theme: &Theme) -> AnyElement {
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
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(c.dialog_muted)
        .child(label)
        .into_any_element()
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

struct AiPromptTextAreaElement {
    editor: Entity<Editor>,
    placeholder: SharedString,
}

struct AiPromptTextAreaPrepaintState {
    lines: Vec<(usize, ShapedLine)>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl AiPromptTextAreaElement {
    fn new(editor: Entity<Editor>, placeholder: SharedString) -> Self {
        Self { editor, placeholder }
    }
}

impl IntoElement for AiPromptTextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for AiPromptTextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = AiPromptTextAreaPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<crate::theme::ThemeManager>().current_arc();
        let (focused, prompt_text) = {
            let editor = self.editor.read(cx);
            (
                editor.ai_prompt_input_active(window),
                editor.ai_prompt_text().to_string(),
            )
        };
        let is_placeholder = prompt_text.is_empty();
        let text = if is_placeholder {
            self.placeholder.as_ref()
        } else {
            prompt_text.as_str()
        };
        let text_color = if is_placeholder {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };
        let font_size = px(theme.typography.text_size * 0.9);
        let line_height = px((f32::from(font_size) * 1.45).max(18.0));
        let style = window.text_style();
        let mut offset = 0usize;
        let mut lines = Vec::new();
        for line_text in text.split('\n') {
            let content = SharedString::from(line_text.to_string());
            let runs = vec![TextRun {
                len: content.len(),
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            lines.push((offset, window.text_system().shape_line(content, font_size, &runs, None)));
            offset += line_text.len() + 1;
        }
        if lines.is_empty() {
            let runs = vec![TextRun {
                len: 0,
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            lines.push((0, window.text_system().shape_line(SharedString::from(""), font_size, &runs, None)));
        }
        let cursor_opacity = self.editor.read(cx).ai_prompt_cursor_opacity();
        self.editor.update(cx, |editor, cx| {
            editor.sync_ai_prompt_cursor_blink(focused, cx);
        });
        let cursor = if focused && !is_placeholder && cursor_opacity > 0.02 {
            let editor = self.editor.read(cx);
            let cursor_offset = editor.ai_prompt_cursor_offset();
            let mut y = bounds.top();
            let mut cursor = None;
            for (start, line) in &lines {
                let end = start + line.len();
                if cursor_offset <= end {
                    let local_offset = cursor_offset.saturating_sub(*start).min(line.len());
                    cursor = Some(fill(
                        Bounds::new(
                            point(bounds.left() + line.x_for_index(local_offset), y),
                            size(px(theme.dimensions.cursor_width), line_height),
                        ),
                        theme.colors.cursor.opacity(cursor_opacity),
                    ));
                    break;
                }
                y += line_height;
            }
            cursor
        } else {
            None
        };
        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
        self.editor.update(cx, |editor, _cx| {
            editor.set_ai_prompt_layout(lines.clone(), line_height, bounds);
        });
        AiPromptTextAreaPrepaintState {
            lines,
            line_height,
            cursor,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(hitbox) = prepaint.hitbox.as_ref()
            && hitbox.is_hovered(window)
        {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }
        let focus_handle = self.editor.read(cx).ai_prompt_focus_handle();
        if focus_handle.is_focused(window) {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.editor.clone()),
                cx,
            );
        }
        let editor_for_down = self.editor.clone();
        let editor_for_up = self.editor.clone();
        let editor_for_move = self.editor.clone();
        let input_bounds = bounds;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !input_bounds.contains(&event.position) {
                return;
            }
            if event.button != MouseButton::Left {
                return;
            }
            cx.stop_propagation();
            let _ = editor_for_down.update(cx, |editor, cx| {
                editor.on_ai_prompt_mouse_down(event, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let _ = editor_for_up.update(cx, |editor, cx| {
                editor.on_ai_prompt_mouse_up(event, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            let _ = editor_for_move.update(cx, |editor, cx| {
                editor.on_ai_prompt_mouse_move(event, window, cx);
            });
        });
        let mut y = bounds.top();
        for (_, line) in prepaint.lines.drain(..) {
            line.paint(point(bounds.left(), y), prepaint.line_height, window, cx)
                .ok();
            y += prepaint.line_height;
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
    }
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

fn normalize_multiline_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
