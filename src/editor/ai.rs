//! Editor AI actions, context collection, and preview application.

use std::borrow::Cow;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::sync::mpsc;
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::{CrossBlockSelection, Editor};
use super::single_line_input::{
    cursor_offset, handle_mouse_down, handle_mouse_move, handle_mouse_up, select_caret_to,
    text_grapheme_boundary, SingleLineArrowKey,
};
use crate::components::{
    AiExpandSelection, AiExplainSelection, AiImproveSelection, AiSummarizeSelection,
    AiTasksSelection, AiTranslateSelection, AskAi, Copy, Cut, Delete, DeleteBack, End, Home,
    MoveLeft, MoveRight, Paste, SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight,
    BlockKind, UndoCaptureKind,
};
use crate::config::ai_toolbar::{AiSelectionToolbarBuiltin, AiSelectionToolbarButton};
use crate::app_menu::dispatch_menu_action;
use crate::components::OpenAiPreferences;
use crate::config::{AiPreferences, read_app_preferences};
use crate::net::ai::{self as ai_client, AiCompletionRequest};
use crate::theme::Theme;

const WORKSPACE_CONTEXT_FILE_LIMIT: usize = 8;
const WORKSPACE_CONTEXT_BYTES_PER_FILE: usize = 1200;

const ICON_AI_TOOLBAR_CONFIG: &str = "icon/toolbar/settings-2.svg";
const ICON_AI_PREVIEW_INSERT: &str = "icon/toolbar/list-plus.svg";
const ICON_AI_PREVIEW_REPLACE: &str = "icon/toolbar/replace.svg";

#[derive(Clone, Debug, PartialEq, Eq)]
enum AiOperation {
    Improve,
    Summarize,
    Expand,
    Explain,
    Tasks,
    Translate,
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
    target: AiTarget,
    result_markdown: String,
}

enum AiStreamEvent {
    Delta(String),
    Done(anyhow::Result<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiPromptContextMode {
    Selection,
    FullDocument,
    Blank,
}

pub(super) struct AiState {
    in_flight: bool,
    streaming_markdown: String,
    streaming_started_at: Instant,
    streaming_progress_task: Option<Task<()>>,
    preview_scroll_handle: ScrollHandle,
    preview_scroll_drag: Option<AiPreviewScrollDrag>,
    preview: Option<AiPreview>,
    error: Option<String>,
    prompt_open: bool,
    prompt_text: String,
    prompt_focus: FocusHandle,
    prompt_selected_range: Range<usize>,
    prompt_selection_reversed: bool,
    prompt_marked_range: Option<Range<usize>>,
    prompt_is_selecting: bool,
    prompt_line_layouts: Vec<WrappedLine>,
    prompt_line_height: Pixels,
    prompt_last_bounds: Option<Bounds<Pixels>>,
    prompt_cursor_blink_epoch: Instant,
    prompt_cursor_blink_task: Option<Task<()>>,
    prompt_context_menu: Option<Point<Pixels>>,
    prompt_context_mode: AiPromptContextMode,
    prompt_context_dropdown_open: bool,
    prompt_has_selection_context: bool,
    prompt_context_dropdown_position: Option<Point<Pixels>>,
    prompt_selection_context: Option<AiContext>,
    prompt_dialog_position: Option<Point<Pixels>>,
    prompt_dialog_drag: Option<AiPromptDialogDrag>,
}

#[derive(Clone, Debug)]
struct AiContext {
    target: AiTarget,
    context_markdown: String,
}

#[derive(Clone, Copy, Debug)]
struct AiPreviewScrollDrag {
    pointer_start_y: f32,
    thumb_start_top: f32,
    track_height: f32,
    thumb_height: f32,
    max_scroll_y: f32,
}

#[derive(Clone, Copy, Debug)]
struct AiPromptDialogDrag {
    pointer_start: Point<Pixels>,
    dialog_start: Point<Pixels>,
}

impl AiState {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            in_flight: false,
            streaming_markdown: String::new(),
            streaming_started_at: Instant::now(),
            streaming_progress_task: None,
            preview_scroll_handle: ScrollHandle::new(),
            preview_scroll_drag: None,
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
            prompt_context_menu: None,
            prompt_context_mode: AiPromptContextMode::FullDocument,
            prompt_context_dropdown_open: false,
            prompt_has_selection_context: false,
            prompt_context_dropdown_position: None,
            prompt_selection_context: None,
            prompt_dialog_position: None,
            prompt_dialog_drag: None,
        }
    }
}

impl AiOperation {
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
            Self::Translate => Cow::Borrowed(
                "Translate the Markdown into Chinese while preserving Markdown formatting, structure, links, and code fences."
            ),
        }
    }
}

impl AiPromptContextMode {
    fn label(self) -> &'static str {
        match self {
            Self::Selection => "引用选中文本",
            Self::FullDocument => "引用全文",
            Self::Blank => "全新对话",
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

    pub(crate) fn on_ai_translate_selection(
        &mut self,
        _: &AiTranslateSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation(AiOperation::Translate, window, cx);
    }

    fn preserve_ai_selection_visuals(&mut self, cx: &mut Context<Self>) {
        if self.cross_block_selection.is_some() {
            self.sync_cross_block_selection_visuals(cx);
            return;
        }
        if let Some(entity_id) = self.active_entity_id
            && let Some(entity) = self.document.block_entity_by_id(entity_id)
        {
            entity.update(cx, |block, cx| {
                if !block.selected_range.is_empty() && block.shows_text_selection_highlight() {
                    block.editor_selection_range = Some(block.selected_range.clone());
                    cx.notify();
                }
            });
        }
    }

    fn clear_ai_selection_visual_preservation(&mut self, cx: &mut Context<Self>) {
        if self.cross_block_selection.is_some() {
            return;
        }
        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, cx| {
                if block.editor_selection_range.take().is_some() {
                    cx.notify();
                }
            });
        }
    }

    fn restore_ai_prompt_edit_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entity_id) = self.active_entity_id
            && let Some(entity) = self.focusable_entity_by_id(entity_id)
        {
            window.focus(&entity.read(cx).focus_handle);
        }
    }

    fn open_ai_prompt_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selection_context = self.collect_selected_ai_context(window, cx);
        self.open_ai_prompt_dialog_with_selection_context(selection_context, window, cx);
    }

    fn open_ai_prompt_dialog_with_selection_context(
        &mut self,
        selection_context: Option<AiContext>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = selection_context.is_some();
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);
        if has_selection {
            self.preserve_ai_selection_visuals(cx);
        }
        self.ai.prompt_open = true;
        self.ai.prompt_text.clear();
        self.ai.prompt_selected_range = 0..0;
        self.ai.prompt_selection_reversed = false;
        self.ai.prompt_marked_range = None;
        self.ai.prompt_is_selecting = false;
        self.ai.prompt_cursor_blink_epoch = Instant::now();
        self.ai.prompt_context_menu = None;
        self.ai.prompt_context_mode = if has_selection {
            AiPromptContextMode::Selection
        } else {
            AiPromptContextMode::FullDocument
        };
        self.ai.prompt_context_dropdown_open = false;
        self.ai.prompt_context_dropdown_position = None;
        self.ai.prompt_has_selection_context = has_selection;
        self.ai.prompt_selection_context = selection_context;
        self.ai.prompt_dialog_drag = None;
        window.focus(&self.ai.prompt_focus);
        cx.notify();
    }

    fn close_ai_prompt_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ai.prompt_open = false;
        self.ai.prompt_marked_range = None;
        self.ai.prompt_is_selecting = false;
        self.ai.prompt_cursor_blink_task = None;
        self.ai.prompt_context_menu = None;
        self.ai.prompt_context_dropdown_open = false;
        self.ai.prompt_context_dropdown_position = None;
        self.ai.prompt_selection_context = None;
        self.ai.prompt_dialog_drag = None;
        self.clear_ai_selection_visual_preservation(cx);
        self.restore_ai_prompt_edit_focus(window, cx);
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
        let mode = self.ai.prompt_context_mode;
        let selection_context = self.ai.prompt_selection_context.clone();
        self.close_ai_prompt_dialog(window, cx);
        self.request_custom_ai_prompt(prompt, mode, selection_context, window, cx);
    }

    fn cancel_ai_prompt_dialog(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_ai_prompt_dialog(window, cx);
    }

    fn on_ai_prompt_backdrop_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_ai_prompt_dialog(window, cx);
    }

    fn on_ai_prompt_panel_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if event.button != MouseButton::Left {
            return;
        }
        let dialog_size = size(px(560.0), px(190.0));
        let viewport = window.viewport_size();
        let current = self.ai.prompt_dialog_position.unwrap_or_else(|| {
            point(
                (viewport.width - dialog_size.width) / 2.0,
                (viewport.height - dialog_size.height) / 2.0,
            )
        });
        self.ai.prompt_dialog_drag = Some(AiPromptDialogDrag {
            pointer_start: event.position,
            dialog_start: current,
        });
        self.ai.prompt_dialog_position = Some(current);
        cx.notify();
    }

    pub(super) fn on_ai_prompt_dialog_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.ai.prompt_dialog_drag else {
            return;
        };
        if !event.dragging() {
            self.ai.prompt_dialog_drag = None;
            cx.notify();
            return;
        }
        let viewport = window.viewport_size();
        let delta = event.position - drag.pointer_start;
        let next = point(
            (drag.dialog_start.x + delta.x).clamp(px(8.0), viewport.width - px(80.0)),
            (drag.dialog_start.y + delta.y).clamp(px(8.0), viewport.height - px(80.0)),
        );
        self.ai.prompt_dialog_position = Some(next);
        cx.notify();
    }

    pub(super) fn on_ai_prompt_dialog_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.prompt_dialog_drag.take().is_some() {
            cx.notify();
        }
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
        lines: Vec<WrappedLine>,
        line_height: Pixels,
        bounds: Bounds<Pixels>,
    ) {
        self.ai.prompt_line_layouts = lines;
        self.ai.prompt_line_height = line_height;
        self.ai.prompt_last_bounds = Some(bounds);
    }

    pub(super) fn ai_prompt_line_layouts(&self) -> &[WrappedLine] {
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
        ai_text_offset_for_position(
            &self.ai.prompt_line_layouts,
            bounds,
            self.ai.prompt_line_height,
            &self.ai.prompt_text,
            position,
        )
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
            self.close_ai_prompt_dialog(window, cx);
            cx.stop_propagation();
        }
    }

    fn confirm_ai_prompt_from_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.ai.prompt_text.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let mode = self.ai.prompt_context_mode;
        let selection_context = self.ai.prompt_selection_context.clone();
        self.close_ai_prompt_dialog(window, cx);
        self.request_custom_ai_prompt(prompt, mode, selection_context, window, cx);
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

    fn collect_selected_ai_context(&self, window: &Window, cx: &App) -> Option<AiContext> {
        if let Some(selection) = self.cross_block_selection
            && let Some(markdown) = self.cross_block_selected_markdown(cx)
        {
            return Some(AiContext {
                target: AiTarget::CrossBlockSelection(selection),
                context_markdown: markdown,
            });
        }
        if let Some(block) = self.focused_edit_target(window, cx) {
            let block_ref = block.read(cx);
            if !block_ref.selected_range.is_empty() {
                let text = block_ref.display_text().to_string();
                let range = block_ref.selected_range.start.min(text.len())
                    ..block_ref.selected_range.end.min(text.len());
                if let Some(selected) = text.get(range.clone()) {
                    return Some(AiContext {
                        target: AiTarget::SingleBlockSelection {
                            entity_id: block.entity_id(),
                            range,
                        },
                        context_markdown: selected.to_string(),
                    });
                }
            }
        }
        None
    }

    fn collect_custom_ai_context(
        &self,
        mode: AiPromptContextMode,
        selection_context: Option<AiContext>,
        window: &Window,
        cx: &App,
    ) -> Result<AiContext, String> {
        match mode {
            AiPromptContextMode::Selection => {
                selection_context
                    .or_else(|| self.ai.prompt_selection_context.clone())
                    .or_else(|| self.collect_selected_ai_context(window, cx))
                    .ok_or_else(|| "当前没有选中文本。".to_string())
            }
            AiPromptContextMode::FullDocument => Ok(AiContext {
                target: AiTarget::FullDocument,
                context_markdown: self.serialized_document_text(cx),
            }),
            AiPromptContextMode::Blank => Ok(AiContext {
                target: AiTarget::InsertOnly {
                    after: self.active_entity_id.map(|id| self.root_ancestor_entity_id(id)),
                },
                context_markdown: String::new(),
            }),
        }
    }

    fn request_custom_ai_prompt(
        &mut self,
        prompt: String,
        mode: AiPromptContextMode,
        selection_context: Option<AiContext>,
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
        let context = match self.collect_custom_ai_context(mode, selection_context, window, cx) {
            Ok(context) => context,
            Err(err) => {
                self.ai.error = Some(err);
                cx.notify();
                return;
            }
        };

        self.request_ai_completion(preferences.ai, prompt, context, cx);
    }

    fn open_ai_prompt_context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.ai.prompt_focus);
        let offset = self.ai_prompt_offset_for_position(event.position);
        if self.ai.prompt_selected_range.is_empty()
            || !self.ai.prompt_selected_range.contains(&offset)
        {
            self.ai.prompt_selected_range = offset..offset;
            self.ai.prompt_selection_reversed = false;
            self.ai.prompt_marked_range = None;
        }
        self.ai.prompt_context_menu = Some(event.position);
        cx.notify();
    }

    fn close_ai_prompt_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.ai.prompt_context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn toggle_ai_prompt_context_dropdown(
        &mut self,
        event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai.prompt_context_dropdown_open = !self.ai.prompt_context_dropdown_open;
        if self.ai.prompt_context_dropdown_open {
            let position = event.position();
            self.ai.prompt_context_dropdown_position = Some(point(position.x, position.y + px(30.0)));
        }
        cx.notify();
    }

    fn select_ai_prompt_context_selection(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai.prompt_has_selection_context {
            return;
        }
        self.ai.prompt_context_mode = AiPromptContextMode::Selection;
        self.ai.prompt_context_dropdown_open = false;
        self.ai.prompt_context_dropdown_position = None;
        cx.notify();
    }

    fn select_ai_prompt_context_full_document(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai.prompt_context_mode = AiPromptContextMode::FullDocument;
        self.ai.prompt_context_dropdown_open = false;
        self.ai.prompt_context_dropdown_position = None;
        cx.notify();
    }

    fn select_ai_prompt_context_blank(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai.prompt_context_mode = AiPromptContextMode::Blank;
        self.ai.prompt_context_dropdown_open = false;
        self.ai.prompt_context_dropdown_position = None;
        cx.notify();
    }

    fn on_ai_prompt_context_menu_copy(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_ai_prompt_copy(&Copy, window, cx);
        self.close_ai_prompt_context_menu(cx);
    }

    fn on_ai_prompt_context_menu_cut(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_ai_prompt_cut(&Cut, window, cx);
        self.close_ai_prompt_context_menu(cx);
    }

    fn on_ai_prompt_context_menu_paste(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_ai_prompt_paste(&Paste, window, cx);
        self.close_ai_prompt_context_menu(cx);
    }

    fn request_ai_operation(
        &mut self,
        operation: AiOperation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_ai_operation_with_instruction(operation, None, window, cx);
    }

    fn request_ai_operation_with_instruction(
        &mut self,
        operation: AiOperation,
        instruction_override: Option<String>,
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

        let worker_operation = operation.clone();
        let instruction = instruction_override
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| worker_operation.instruction().to_string());

        self.request_ai_completion(
            preferences.ai,
            instruction,
            AiContext {
                target: context.target,
                context_markdown,
            },
            cx,
        );
    }

    fn request_ai_selection_toolbar_operation_with_instruction(
        &mut self,
        operation: AiOperation,
        instruction_override: Option<String>,
        selection_context: AiContext,
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
        let worker_operation = operation.clone();
        let instruction = instruction_override
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| worker_operation.instruction().to_string());

        self.request_ai_completion(preferences.ai, instruction, selection_context, cx);
    }

    fn request_ai_completion(
        &mut self,
        ai_preferences: AiPreferences,
        instruction: String,
        context: AiContext,
        cx: &mut Context<Self>,
    ) {
        self.ai.in_flight = true;
        self.ai.streaming_markdown.clear();
        self.ai.streaming_started_at = Instant::now();
        self.ai.preview = None;
        self.ai.error = None;
        self.start_ai_streaming_progress_task(cx);
        let weak_editor = cx.entity().downgrade();
        let target = context.target;
        let context_markdown = context.context_markdown;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let request = AiCompletionRequest {
                preferences: ai_preferences,
                instruction,
                context_markdown,
            };
            let stream_tx = tx.clone();
            let result = ai_client::complete_markdown_streaming(request, move |delta| {
                let _ = stream_tx.send(AiStreamEvent::Delta(delta));
            });
            let _ = tx.send(AiStreamEvent::Done(result));
        });

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut target = Some(target);
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;

                let mut deltas = Vec::new();
                let mut done = None;
                loop {
                    match rx.try_recv() {
                        Ok(AiStreamEvent::Delta(delta)) => deltas.push(delta),
                        Ok(AiStreamEvent::Done(result)) => {
                            done = Some(result);
                            break;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            done = Some(Err(anyhow::anyhow!("AI worker ended early")));
                            break;
                        }
                    }
                }

                let is_done = done.is_some();
                let next_target = if is_done { target.take() } else { None };
                if weak_editor
                    .update(cx, move |editor, cx| {
                        for delta in deltas {
                            editor.ai.streaming_markdown.push_str(&delta);
                        }
                        if let Some(result) = done {
                            editor.ai.in_flight = false;
                            editor.ai.streaming_progress_task = None;
                            match result {
                                Ok(result_markdown) => {
                                    editor.ai.streaming_markdown = result_markdown.clone();
                                    if let Some(target) = next_target {
                                        editor.ai.preview = Some(AiPreview {
                                            target,
                                            result_markdown,
                                        });
                                    }
                                }
                                Err(err) => editor.ai.error = Some(err.to_string()),
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                if is_done {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn start_ai_streaming_progress_task(&mut self, cx: &mut Context<Self>) {
        self.ai.streaming_progress_task = Some(cx.spawn(
            async |this: WeakEntity<Editor>, cx: &mut AsyncApp| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                let should_continue = this
                    .update(cx, |editor, cx| {
                        if editor.ai.in_flight {
                            cx.notify();
                            true
                        } else {
                            editor.ai.streaming_progress_task = None;
                            false
                        }
                    })
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            },
        ));
    }

    fn ai_streaming_progress(&self) -> f32 {
        let elapsed = self.ai.streaming_started_at.elapsed().as_secs_f32();
        (1.0 - (-elapsed / 18.0).exp()).clamp(0.0, 0.96)
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
        self.ai.preview_scroll_drag = None;
        cx.notify();
    }

    fn on_ai_preview_scrollbar_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
        track_height: f32,
        thumb_top: f32,
        thumb_height: f32,
        max_scroll_y: f32,
    ) {
        cx.stop_propagation();
        self.ai.preview_scroll_drag = Some(AiPreviewScrollDrag {
            pointer_start_y: f32::from(event.position.y),
            thumb_start_top: thumb_top,
            track_height,
            thumb_height,
            max_scroll_y,
        });
        cx.notify();
    }

    pub(super) fn on_ai_preview_scrollbar_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.ai.preview_scroll_drag else {
            return;
        };
        if !event.dragging() {
            self.ai.preview_scroll_drag = None;
            cx.notify();
            return;
        }
        let travel = (drag.track_height - drag.thumb_height).max(0.0);
        let delta_y = f32::from(event.position.y) - drag.pointer_start_y;
        let thumb_top = (drag.thumb_start_top + delta_y).clamp(0.0, travel);
        let scroll_y = Self::scroll_offset_for_thumb_top(
            thumb_top,
            drag.track_height,
            drag.thumb_height,
            drag.max_scroll_y,
        );
        let mut offset = self.ai.preview_scroll_handle.offset();
        offset.y = -px(scroll_y);
        self.ai.preview_scroll_handle.set_offset(offset);
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn on_ai_preview_scrollbar_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai.preview_scroll_drag.take().is_some() {
            cx.stop_propagation();
            cx.notify();
        }
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
            AiTarget::CrossBlockSelection(selection) => self
                .cross_block_selection_end_entity_id(selection, cx)
                .map(|entity_id| self.root_ancestor_entity_id(entity_id)),
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
        let block_ref = block.read(cx);
        let bounds = block_ref.last_bounds.or(block_ref.interaction_bounds)?;
        let anchor_bounds = if self.cross_block_selection.is_none() && !block_ref.selected_range.is_empty() {
            block_ref
                .visible_range_bounds(block_ref.selected_range.clone())
                .unwrap_or(bounds)
        } else {
            bounds
        };
        Some(point(
            anchor_bounds.left(),
            px((f32::from(anchor_bounds.top()) - 36.0).max(8.0)),
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
        let selection_context = self.collect_selected_ai_context(window, cx)?;
        let preferences = read_app_preferences().ok()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let editor = cx.entity().downgrade();
        let mut toolbar = div()
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
            .on_mouse_up(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .rounded(px(d.menu_panel_radius))
            .bg(c.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .shadow_lg();

        for (index, button) in preferences
            .ai
            .selection_toolbar
            .iter()
            .filter(|button| button.enabled)
            .enumerate()
        {
            if let Some(element) = self.render_ai_selection_toolbar_button(
                index,
                button,
                theme,
                editor.clone(),
                selection_context.clone(),
            ) {
                toolbar = toolbar.child(element);
            }
        }

        Some(
            toolbar
                .child(
                    div()
                        .id("ai-floating-toolbar-separator")
                        .w(px(1.0))
                        .h(px(18.0))
                        .mx(px(2.0))
                        .flex_shrink_0()
                        .bg(c.dialog_border.opacity(0.45)),
                )
                .child(ai_toolbar_action_button(
                    "ai-floating-toolbar-config",
                    ICON_AI_TOOLBAR_CONFIG.to_string(),
                    "自定义",
                    theme,
                    move |_, cx| {
                        cx.stop_propagation();
                        cx.defer(|cx| {
                            dispatch_menu_action(&OpenAiPreferences, cx);
                        });
                    },
                ))
                .into_any_element(),
        )
    }

    fn render_ai_selection_toolbar_button(
        &self,
        index: usize,
        button: &AiSelectionToolbarButton,
        theme: &Theme,
        editor: WeakEntity<Self>,
        selection_context: AiContext,
    ) -> Option<AnyElement> {
        let label = SharedString::from(button.label.clone());
        let icon = button.resolved_icon().to_string();
        let instruction = button.instruction.clone();
        let action = button.action.clone();
        let button_id = SharedString::from(format!("ai-floating-{index}"));
        Some(match AiSelectionToolbarBuiltin::from_id(&action) {
            Some(AiSelectionToolbarBuiltin::CustomPrompt) => ai_toolbar_action_button(
                button_id,
                icon,
                label,
                theme,
                move |window, cx| {
                    let selection_context = selection_context.clone();
                    let _ = editor.update(cx, |editor, cx| {
                        editor.open_ai_prompt_dialog_with_selection_context(
                            Some(selection_context),
                            window,
                            cx,
                        );
                    });
                },
            )
            .into_any_element(),
            Some(builtin) => {
                let operation = match builtin {
                    AiSelectionToolbarBuiltin::Improve => AiOperation::Improve,
                    AiSelectionToolbarBuiltin::Summarize => AiOperation::Summarize,
                    AiSelectionToolbarBuiltin::Expand => AiOperation::Expand,
                    AiSelectionToolbarBuiltin::Explain => AiOperation::Explain,
                    AiSelectionToolbarBuiltin::Tasks => AiOperation::Tasks,
                    AiSelectionToolbarBuiltin::Translate => AiOperation::Translate,
                    AiSelectionToolbarBuiltin::CustomPrompt => unreachable!(),
                };
                ai_toolbar_action_button(
                    button_id,
                    icon,
                    label,
                    theme,
                    move |_window, cx| {
                        let selection_context = selection_context.clone();
                        let _ = editor.update(cx, |editor, cx| {
                            editor.request_ai_selection_toolbar_operation_with_instruction(
                                operation.clone(),
                                instruction.clone(),
                                selection_context,
                                cx,
                            );
                        });
                    },
                )
                .into_any_element()
            }
            None if action == "prompt" => {
                let prompt = instruction.unwrap_or_default();
                ai_toolbar_action_button(
                    button_id,
                    icon,
                    label,
                    theme,
                    move |window, cx| {
                        let selection_context = selection_context.clone();
                        let _ = editor.update(cx, |editor, cx| {
                            if prompt.trim().is_empty() {
                                editor.ai.error =
                                    Some("该自定义按钮尚未配置 AI 指令。".into());
                                cx.notify();
                                return;
                            }
                            editor.request_custom_ai_prompt(
                                prompt.clone(),
                                AiPromptContextMode::Selection,
                                Some(selection_context),
                                window,
                                cx,
                            );
                        });
                    },
                )
                .into_any_element()
            }
            None => return None,
        })
    }

    pub(super) fn render_ai_prompt_dialog_overlay(
        &self,
        theme: &Theme,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.ai.prompt_open {
            return None;
        }
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let can_submit = !self.ai.prompt_text.trim().is_empty();
        let overlay = div()
            .id("ai-prompt-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .bg(c.dialog_backdrop)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_ai_prompt_backdrop_mouse_down))
            .on_mouse_move(cx.listener(Self::on_ai_prompt_dialog_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_ai_prompt_dialog_mouse_up));
        let dialog = div()
                        .id("ai-prompt-dialog")
                        .w(px(d.dialog_width.max(560.0)))
                        .max_w(relative(0.86))
                        .p(px(10.0))
                        .flex()
                        .flex_col()
                        .relative()
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
                        .on_mouse_down(MouseButton::Left, cx.listener(Self::on_ai_prompt_panel_mouse_down))
                        .child(
                            div()
                                .id("ai-prompt-close")
                                .absolute()
                                .top(px(8.0))
                                .right(px(8.0))
                                .size(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.format_toolbar_button_radius))
                                .bg(c.dialog_surface)
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .cursor_pointer()
                                .child(svg().path("icon/toolbar/x.svg").size(px(16.0)).text_color(c.dialog_secondary_button_text))
                                .on_click(cx.listener(Self::cancel_ai_prompt_dialog)),
                        )
                        .child(
                            div()
                                .id("ai-prompt-input")
                                .h(px(170.0))
                                .px(px(12.0))
                                .pt(px(16.0))
                                .pb(px(44.0))
                                .flex()
                                .relative()
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
                                        ))
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(10.0))
                                        .bottom(px(8.0))
                                        .id("ai-prompt-context-dropdown")
                                        .h(px(28.0))
                                        .px(px(8.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .rounded(px(d.menu_item_radius))
                                        .bg(c.dialog_surface)
                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px((t.dialog_body_size - 1.0).max(11.0)))
                                        .text_color(c.dialog_body)
                                        .child(self.ai.prompt_context_mode.label())
                                        .child(
                                            svg()
                                                .path("icon/toolbar/chevron-down.svg")
                                                .size(px(14.0))
                                                .text_color(c.dialog_secondary_button_text),
                                        )
                                        .on_click(cx.listener(Self::toggle_ai_prompt_context_dropdown)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .right(px(10.0))
                                        .bottom(px(8.0))
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
                        );
        let overlay = if let Some(position) = self.ai.prompt_dialog_position {
            overlay.child(dialog.absolute().left(position.x).top(position.y))
        } else {
            overlay
                .flex()
                .items_center()
                .justify_center()
                .child(dialog)
        };
        let overlay = if let Some(menu) = self.render_ai_prompt_context_menu(theme, cx) {
            overlay.child(menu)
        } else {
            overlay
        };
        let overlay = if let Some(dropdown) = self.render_ai_prompt_context_dropdown(theme, cx) {
            overlay.child(dropdown)
        } else {
            overlay
        };
        Some(overlay.into_any_element())
    }

    fn render_ai_prompt_context_dropdown(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.ai.prompt_context_dropdown_open {
            return None;
        }
        let position = self.ai.prompt_context_dropdown_position?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        Some(
            div()
                .id("ai-prompt-context-options")
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(180.0))
                .p(px(d.menu_panel_padding))
                .flex()
                .flex_col()
                .gap(px(d.menu_panel_gap))
                .occlude()
                .rounded(px(d.menu_panel_radius))
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .bg(c.dialog_surface)
                .shadow_lg()
                .child(ai_prompt_dropdown_item(
                    "ai-context-selection",
                    "引用选中文本",
                    self.ai.prompt_context_mode == AiPromptContextMode::Selection,
                    self.ai.prompt_has_selection_context,
                    theme,
                    cx.listener(Self::select_ai_prompt_context_selection),
                ))
                .child(ai_prompt_dropdown_item(
                    "ai-context-full-document",
                    "引用全文",
                    self.ai.prompt_context_mode == AiPromptContextMode::FullDocument,
                    true,
                    theme,
                    cx.listener(Self::select_ai_prompt_context_full_document),
                ))
                .child(ai_prompt_dropdown_item(
                    "ai-context-blank",
                    "全新对话",
                    self.ai.prompt_context_mode == AiPromptContextMode::Blank,
                    true,
                    theme,
                    cx.listener(Self::select_ai_prompt_context_blank),
                ))
                .into_any_element(),
        )
    }

    fn render_ai_prompt_context_menu(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let position = self.ai.prompt_context_menu?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let has_selection = !self.ai.prompt_selected_range.is_empty();
        let can_paste = cx.read_from_clipboard().and_then(|item| item.text()).is_some();
        Some(
            div()
                .id("ai-prompt-context-menu")
                .absolute()
                .left(position.x)
                .top(position.y)
                .w(px(d.context_menu_panel_width.max(168.0)))
                .p(px(d.menu_panel_padding))
                .flex()
                .flex_col()
                .gap(px(d.menu_panel_gap))
                .occlude()
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .rounded(px(d.menu_panel_radius))
                .shadow_lg()
                .child(ai_prompt_menu_item(
                    "ai-prompt-menu-copy",
                    "复制",
                    has_selection,
                    theme,
                    cx.listener(Self::on_ai_prompt_context_menu_copy),
                ))
                .child(ai_prompt_menu_item(
                    "ai-prompt-menu-cut",
                    "剪切",
                    has_selection,
                    theme,
                    cx.listener(Self::on_ai_prompt_context_menu_cut),
                ))
                .child(ai_prompt_menu_item(
                    "ai-prompt-menu-paste",
                    "粘贴",
                    can_paste,
                    theme,
                    cx.listener(Self::on_ai_prompt_context_menu_paste),
                ))
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
        if !self.ai.in_flight && self.ai.preview.is_none() && self.ai.error.is_none() {
            return None;
        }
        let body = if self.ai.in_flight {
            if self.ai.streaming_markdown.trim().is_empty() {
                "等待模型返回内容...".to_string()
            } else {
                self.ai.streaming_markdown.clone()
            }
        } else if let Some(preview) = &self.ai.preview {
            preview.result_markdown.clone()
        } else {
            self.ai.error.clone().unwrap_or_default()
        };
        let has_preview = self.ai.preview.is_some();
        let progress = self.ai_streaming_progress();
        let progress_width = 0.18 + progress * 0.72;
        let progress_dots = ".".repeat(((self.ai.streaming_started_at.elapsed().as_secs_f32() * 2.4) as usize % 3) + 1);
        let body_lines = body
            .split('\n')
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let preview_scroll_height = 360.0;
        let preview_max_scroll_y = f32::from(
            self.ai
                .preview_scroll_handle
                .max_offset()
                .height
                .max(px(0.0)),
        );
        let preview_scroll_y = (-f32::from(self.ai.preview_scroll_handle.offset().y))
            .clamp(0.0, preview_max_scroll_y);
        let preview_scrollbar = Self::scrollbar_geometry(
            preview_scroll_height,
            preview_max_scroll_y,
            preview_scroll_y,
        );
        let preview_track_height = preview_scrollbar.track_height;
        let preview_thumb_top = preview_scrollbar.thumb_top;
        let preview_thumb_height = preview_scrollbar.thumb_height;
        let show_preview_scrollbar = preview_max_scroll_y > 0.5;
        let editor = cx.entity().downgrade();
        let preview_drag_editor = editor.clone();
        let preview_capture_editor = editor.clone();

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
                .on_mouse_move(cx.listener(Self::on_ai_preview_scrollbar_mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_ai_preview_scrollbar_mouse_up))
                .child(
                    div()
                        .id("ai-preview-dialog")
                        .w(px(d.dialog_width.max(560.0)))
                        .max_w(relative(0.86))
                        .max_h(relative(0.82))
                        .p(px(10.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .overflow_hidden()
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .when(self.ai.in_flight, |this| {
                            this.child(
                                div()
                                    .id("ai-preview-progress")
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px((t.dialog_body_size - 1.0).max(11.0)))
                                            .text_color(c.dialog_muted)
                                            .child(format!("AI 正在实时生成{progress_dots}")),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(3.0))
                                            .rounded(px(999.0))
                                            .overflow_hidden()
                                            .bg(c.dialog_border.opacity(0.55))
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w(relative(progress_width))
                                                    .rounded(px(999.0))
                                                    .bg(c.dialog_primary_button_bg),
                                            ),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .id("ai-preview-result-frame")
                                .relative()
                                .w_full()
                                .h(px(360.0))
                                .min_h(px(0.0))
                                .max_h(px(360.0))
                                .rounded(px(d.menu_item_radius))
                                .bg(c.code_bg)
                                .child(
                                    div()
                                        .id("ai-preview-result-scroll")
                                        .size_full()
                                        .overflow_y_scroll()
                                        .scrollbar_width(px(0.0))
                                        .track_scroll(&self.ai.preview_scroll_handle)
                                        .p(px(12.0))
                                        .pr(px(20.0))
                                        .child(
                                            div()
                                                .w_full()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.0))
                                                .text_size(px(t.dialog_body_size))
                                                .line_height(rems(t.text_line_height))
                                                .text_color(c.code_text)
                                                .children(body_lines.into_iter().map(|line| {
                                                    div()
                                                        .w_full()
                                                        .min_h(px(t.dialog_body_size * t.text_line_height))
                                                        .child(if line.is_empty() { " ".to_string() } else { line })
                                                })),
                                        ),
                                )
                                .when(show_preview_scrollbar, |this| {
                                    this.child(
                                        div()
                                            .id("ai-preview-scrollbar-track")
                                            .absolute()
                                            .top(px(8.0))
                                            .right(px(6.0))
                                            .bottom(px(8.0))
                                            .w(px(d.scrollbar_width.max(6.0)))
                                            .rounded(px(999.0))
                                            .bg(c.dialog_border.opacity(0.7))
                                            .child(
                                                div()
                                                    .id("ai-preview-scrollbar-thumb")
                                                    .absolute()
                                                    .top(px(preview_scrollbar.thumb_top))
                                                    .right(px(0.0))
                                                    .w_full()
                                                    .h(px(preview_scrollbar.thumb_height))
                                                    .rounded(px(999.0))
                                                    .bg(c.dialog_primary_button_bg)
                                                    .cursor_pointer()
                                                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                                        let _ = preview_drag_editor.update(cx, |editor, cx| {
                                                            editor.on_ai_preview_scrollbar_mouse_down(
                                                                event,
                                                                window,
                                                                cx,
                                                                preview_track_height,
                                                                preview_thumb_top,
                                                                preview_thumb_height,
                                                                preview_max_scroll_y,
                                                            );
                                                        });
                                                    })
                                                    .child(
                                                        canvas(
                                                            |_, _, _| (),
                                                            move |_thumb_bounds, _, window, _| {
                                                                window.on_mouse_event({
                                                                    let editor = preview_capture_editor.clone();
                                                                    move |event: &MouseMoveEvent, phase, window, cx| {
                                                                        if !phase.bubble() || !event.dragging() {
                                                                            return;
                                                                        }
                                                                        let _ = editor.update(cx, |editor, cx| {
                                                                            editor.on_ai_preview_scrollbar_mouse_move(
                                                                                event,
                                                                                window,
                                                                                cx,
                                                                            );
                                                                        });
                                                                    }
                                                                });

                                                                window.on_mouse_event({
                                                                    let editor = preview_capture_editor.clone();
                                                                    move |event: &MouseUpEvent, phase, window, cx| {
                                                                        if !phase.bubble() {
                                                                            return;
                                                                        }
                                                                        let _ = editor.update(cx, |editor, cx| {
                                                                            editor.on_ai_preview_scrollbar_mouse_up(
                                                                                event,
                                                                                window,
                                                                                cx,
                                                                            );
                                                                        });
                                                                    }
                                                                });
                                                            },
                                                        )
                                                        .size_full(),
                                                    ),
                                            ),
                                    )
                                }),
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
                                    this.child(ai_dialog_button_with_icon(
                                        "ai-preview-insert",
                                        ICON_AI_PREVIEW_INSERT,
                                        "插入下方",
                                        theme,
                                        cx.listener(Self::apply_ai_preview_insert),
                                    ))
                                    .child(ai_dialog_primary_button_with_icon(
                                        "ai-preview-replace",
                                        ICON_AI_PREVIEW_REPLACE,
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
    id: impl Into<ElementId>,
    icon_path: String,
    label: impl Into<SharedString>,
    theme: &Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
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
        .child(
            svg()
                .path(SharedString::from(icon_path))
                .size(px(14.0))
                .text_color(c.dialog_secondary_button_text),
        )
        .child(label)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window, cx);
        })
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

fn ai_dialog_button_with_icon(
    id: &'static str,
    icon_path: &'static str,
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
        .gap(px(5.0))
        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
        .border(px(d.dialog_border_width))
        .border_color(c.dialog_border)
        .bg(c.dialog_secondary_button_bg)
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .cursor_pointer()
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(c.dialog_secondary_button_text)
        .child(
            svg()
                .path(icon_path)
                .size(px(14.0))
                .text_color(c.dialog_secondary_button_text),
        )
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

fn ai_prompt_menu_item(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    if enabled {
        div()
            .id(id)
            .h(px(d.menu_item_height))
            .px(px(d.menu_item_padding_x))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .text_size(px(d.menu_text_size))
            .font_weight(t.dialog_body_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(label)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .on_click(on_click)
            .into_any_element()
    } else {
        div()
            .id(id)
            .h(px(d.menu_item_height))
            .px(px(d.menu_item_padding_x))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .text_size(px(d.menu_text_size))
            .font_weight(t.dialog_body_weight.to_font_weight())
            .text_color(c.dialog_muted)
            .child(label)
            .into_any_element()
    }
}

fn ai_prompt_dropdown_item(
    id: &'static str,
    label: &'static str,
    selected: bool,
    enabled: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    let mut item = div()
        .id(id)
        .min_h(px(30.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .rounded(px(d.menu_item_radius))
        .bg(if selected { c.selection } else { c.dialog_surface })
        .text_size(px(t.dialog_body_size))
        .text_color(if enabled { c.dialog_body } else { c.dialog_muted })
        .child(label);
    if enabled {
        item = item
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .on_click(on_click);
    }
    item.into_any_element()
}

fn ai_hard_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (index, _) in text.match_indices('\n') {
        ranges.push(start..index);
        start = index + 1;
    }
    ranges.push(start..text.len());
    ranges
}

fn ai_line_index_for_offset(ranges: &[Range<usize>], offset: usize) -> (usize, usize) {
    let clamped = offset.min(ranges.last().map(|range| range.end).unwrap_or(0));
    for (index, range) in ranges.iter().enumerate() {
        if clamped <= range.end {
            return (index, clamped.saturating_sub(range.start));
        }
    }
    (ranges.len().saturating_sub(1), 0)
}

fn ai_wrapped_line_height(line: &WrappedLine, line_height: Pixels) -> Pixels {
    line.size(line_height).height
}

fn ai_wrapped_line_top(lines: &[WrappedLine], line_height: Pixels, line_idx: usize) -> Pixels {
    lines
        .iter()
        .take(line_idx)
        .fold(px(0.0), |height, line| height + ai_wrapped_line_height(line, line_height))
}

fn ai_wrap_boundary_offset(line: &WrappedLine, wrap_idx: usize) -> Option<usize> {
    let boundary = line.wrap_boundaries().get(wrap_idx)?;
    let run = line.unwrapped_layout.runs.get(boundary.run_ix)?;
    let glyph = run.glyphs.get(boundary.glyph_ix)?;
    Some(glyph.index)
}

fn ai_wrapped_row_offsets(line: &WrappedLine) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(line.wrap_boundaries().len() + 2);
    offsets.push(0);
    for wrap_idx in 0..line.wrap_boundaries().len() {
        if let Some(offset) = ai_wrap_boundary_offset(line, wrap_idx) {
            offsets.push(offset.min(line.len()));
        }
    }
    offsets.push(line.len());
    offsets.dedup();
    offsets
}

fn ai_position_for_offset(
    line: &WrappedLine,
    offset: usize,
    line_height: Pixels,
) -> Option<Point<Pixels>> {
    let offsets = ai_wrapped_row_offsets(line);
    for row_idx in 0..offsets.len().saturating_sub(1) {
        let row_start = offsets[row_idx];
        let row_end = offsets[row_idx + 1];
        if offset >= row_start && offset < row_end {
            let row_start_x = line.unwrapped_layout.x_for_index(row_start);
            let x = line.unwrapped_layout.x_for_index(offset) - row_start_x;
            return Some(point(x, line_height * row_idx as f32));
        }
    }
    line.position_for_index(offset, line_height)
}

fn ai_cursor_bounds(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    offset: usize,
    cursor_width: Pixels,
) -> Option<Bounds<Pixels>> {
    let ranges = ai_hard_line_ranges(text);
    let (line_idx, local_offset) = ai_line_index_for_offset(&ranges, offset);
    let line = lines.get(line_idx)?;
    let cursor = ai_position_for_offset(line, local_offset, line_height)?;
    let y = bounds.top() + ai_wrapped_line_top(lines, line_height, line_idx);
    Some(Bounds::new(
        point(bounds.left() + cursor.x, y + cursor.y),
        size(cursor_width, line_height),
    ))
}

fn ai_range_segments(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
) -> Vec<Bounds<Pixels>> {
    if range.is_empty() || lines.is_empty() {
        return Vec::new();
    }
    let ranges = ai_hard_line_ranges(text);
    let (start_line, start_offset) = ai_line_index_for_offset(&ranges, range.start);
    let (end_line, end_offset) = ai_line_index_for_offset(&ranges, range.end);
    let mut segments = Vec::new();
    for line_idx in start_line..=end_line {
        let Some(line) = lines.get(line_idx) else {
            continue;
        };
        let line_start = if line_idx == start_line { start_offset } else { 0 };
        let line_end = if line_idx == end_line { end_offset } else { line.len() };
        let offsets = ai_wrapped_row_offsets(line);
        let line_top = bounds.top() + ai_wrapped_line_top(lines, line_height, line_idx);
        for row_idx in 0..offsets.len().saturating_sub(1) {
            let row_start = offsets[row_idx];
            let row_end = offsets[row_idx + 1];
            let seg_start = line_start.max(row_start).min(row_end);
            let seg_end = line_end.min(row_end).max(row_start);
            if seg_start >= seg_end {
                continue;
            }
            let row_start_x = line.unwrapped_layout.x_for_index(row_start);
            let start_x = line.unwrapped_layout.x_for_index(seg_start) - row_start_x;
            let end_x = line.unwrapped_layout.x_for_index(seg_end) - row_start_x;
            let y = line_top + line_height * row_idx as f32;
            segments.push(Bounds::from_corners(
                point(bounds.left() + start_x, y),
                point(bounds.left() + end_x, y + line_height),
            ));
        }
    }
    segments
}

pub(super) fn ai_range_bounds(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    range: Range<usize>,
) -> Option<Bounds<Pixels>> {
    let segments = ai_range_segments(lines, bounds, line_height, text, range.clone());
    if segments.is_empty() {
        return ai_cursor_bounds(lines, bounds, line_height, text, range.start, px(1.0));
    }
    let mut union = segments[0];
    for segment in segments.iter().skip(1) {
        union = Bounds::from_corners(
            point(union.left().min(segment.left()), union.top().min(segment.top())),
            point(union.right().max(segment.right()), union.bottom().max(segment.bottom())),
        );
    }
    Some(union)
}

fn ai_wrapped_row_for_y(
    line: &WrappedLine,
    line_height: Pixels,
    relative_y: Pixels,
) -> usize {
    let row_count = ai_wrapped_row_offsets(line).len().saturating_sub(1).max(1);
    ((f32::from(relative_y.max(px(0.0))) / f32::from(line_height.max(px(1.0))))
        .floor()
        .max(0.0) as usize)
        .min(row_count.saturating_sub(1))
}

fn ai_text_offset_for_position(
    lines: &[WrappedLine],
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    text: &str,
    position: Point<Pixels>,
) -> usize {
    if lines.is_empty() {
        return text.len();
    }
    let mut top = px(0.0);
    let relative_y = (position.y - bounds.top()).max(px(0.0));
    for (line_idx, line) in lines.iter().enumerate() {
        let height = ai_wrapped_line_height(line, line_height);
        if relative_y < top + height || line_idx + 1 == lines.len() {
            let row_idx = ai_wrapped_row_for_y(line, line_height, relative_y - top);
            let offsets = ai_wrapped_row_offsets(line);
            let row_start = offsets[row_idx];
            let row_end = offsets.get(row_idx + 1).copied().unwrap_or(line.len());
            let row_start_x = line.unwrapped_layout.x_for_index(row_start);
            let x = (position.x - bounds.left()).max(px(0.0)) + row_start_x;
            let mut best = row_start;
            let mut best_dist = Pixels::MAX;
            for offset in row_start..=row_end {
                if !line.text.is_char_boundary(offset.min(line.text.len())) {
                    continue;
                }
                let dist = (line.unwrapped_layout.x_for_index(offset) - x).abs();
                if dist < best_dist {
                    best = offset;
                    best_dist = dist;
                }
            }
            let ranges = ai_hard_line_ranges(text);
            return ranges
                .get(line_idx)
                .map(|range| range.start + best.min(range.len()))
                .unwrap_or(text.len())
                .min(text.len());
        }
        top += height;
    }
    text.len()
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

fn ai_dialog_primary_button_with_icon(
    id: &'static str,
    icon_path: &'static str,
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
        .gap(px(5.0))
        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
        .bg(c.dialog_primary_button_bg)
        .hover(|this| this.bg(c.dialog_primary_button_hover))
        .cursor_pointer()
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(c.dialog_primary_button_text)
        .child(
            svg()
                .path(icon_path)
                .size(px(14.0))
                .text_color(c.dialog_primary_button_text),
        )
        .child(label)
        .on_click(on_click)
}

struct AiPromptTextAreaElement {
    editor: Entity<Editor>,
    placeholder: SharedString,
}

struct AiPromptTextAreaPrepaintState {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
    selection: Vec<PaintQuad>,
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
        let runs = vec![TextRun {
            len: text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let lines = window
            .text_system()
            .shape_text(
                SharedString::from(text.to_string()),
                font_size,
                &runs,
                Some(bounds.size.width.max(px(1.0))),
                None,
            )
            .map(|lines| lines.into_vec())
            .unwrap_or_default();
        let cursor_opacity = self.editor.read(cx).ai_prompt_cursor_opacity();
        self.editor.update(cx, |editor, cx| {
            editor.sync_ai_prompt_cursor_blink(focused, cx);
        });
        let selected_range = self.editor.read(cx).ai_prompt_selected_range();
        let selection = if focused && !is_placeholder && !selected_range.is_empty() {
            ai_range_segments(
                &lines,
                bounds,
                line_height,
                &prompt_text,
                selected_range.clone(),
            )
            .into_iter()
            .map(|bounds| fill(bounds, theme.colors.selection))
            .collect()
        } else {
            Vec::new()
        };
        let cursor = if focused && selected_range.is_empty() && cursor_opacity > 0.02 {
            let editor = self.editor.read(cx);
            let cursor_offset = editor.ai_prompt_cursor_offset();
            ai_cursor_bounds(
                &lines,
                bounds,
                line_height,
                &prompt_text,
                cursor_offset,
                px(theme.dimensions.cursor_width),
            )
            .map(|bounds| fill(bounds, theme.colors.cursor.opacity(cursor_opacity)))
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
            selection,
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
            if event.button == MouseButton::Right {
                cx.stop_propagation();
                let _ = editor_for_down.update(cx, |editor, cx| {
                    editor.open_ai_prompt_context_menu(event, window, cx);
                });
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
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }
        let mut y = bounds.top();
        for line in prepaint.lines.drain(..) {
            line.paint(point(bounds.left(), y), prepaint.line_height, TextAlign::Left, None, window, cx)
                .ok();
            y += ai_wrapped_line_height(&line, prepaint.line_height);
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
