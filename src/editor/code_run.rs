//! Code-block and inline-code execution state, dialogs, and background tasks.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::*;

use crate::code_runner::{
    CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES, CodeBlockRunSnapshot, CodeRunOutcome, CodeRunProgress,
    CodeRunStatus, code_run_output_line_count, kill_child,
    resolve_runner, spawn_code_run, spawn_inline_shell_run,
};
use crate::components::{Block, BlockEvent, BlockKind};
use crate::config::{read_app_preferences, set_code_execution_confirm_shown};
use crate::i18n::I18nManager;
use crate::theme::Theme;

use super::Editor;
use super::ViewMode;

const ICON_INLINE_CODE_RUN: &str = "icon/toolbar/circle-play.svg";
const ICON_INLINE_CODE_RUN_STOP: &str = "icon/toolbar/circle-stop.svg";
const ICON_INLINE_CODE_RUN_CLOSE: &str = "icon/toolbar/x.svg";
const INLINE_CODE_RUN_VIEWPORT_MARGIN: f32 = 8.0;

/// Mutable run state owned by the editor for one code block.
#[derive(Clone, Debug)]
pub(crate) struct CodeBlockRunState {
    pub status: CodeRunStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub output_expanded: bool,
    pub output_content_expanded: bool,
    pub error_message: Option<String>,
}

impl Default for CodeBlockRunState {
    fn default() -> Self {
        Self {
            status: CodeRunStatus::Idle,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            duration_ms: 0,
            output_expanded: false,
            output_content_expanded: false,
            error_message: None,
        }
    }
}

impl CodeBlockRunState {
    fn snapshot(&self) -> CodeBlockRunSnapshot {
        CodeBlockRunSnapshot {
            status: self.status,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            exit_code: self.exit_code,
            duration_ms: self.duration_ms,
            output_expanded: self.output_expanded,
            output_content_expanded: self.output_content_expanded,
            error_message: self.error_message.clone(),
        }
    }

    fn reset_for_run(&mut self) {
        self.status = CodeRunStatus::Running;
        self.stdout.clear();
        self.stderr.clear();
        self.exit_code = None;
        self.duration_ms = 0;
        self.error_message = None;
        self.output_expanded = true;
        self.output_content_expanded = false;
    }

    fn apply_outcome(&mut self, outcome: CodeRunOutcome) {
        self.stdout = outcome.stdout;
        self.stderr = outcome.stderr;
        self.exit_code = outcome.exit_code;
        self.duration_ms = outcome.duration_ms;
        let failed = outcome.error_message.is_some();
        self.error_message = outcome.error_message;
        self.status = if outcome.cancelled {
            CodeRunStatus::Cancelled
        } else if failed {
            CodeRunStatus::Failed
        } else {
            CodeRunStatus::Done
        };
    }
}

pub(crate) struct ActiveCodeRunControl {
    block_id: EntityId,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
}

/// Identifies one inline code span eligible for execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InlineCodeRunTarget {
    pub block_id: EntityId,
    pub span_range: Range<usize>,
}

pub(crate) struct ActiveInlineCodeRunControl {
    target: InlineCodeRunTarget,
    cancel: Arc<AtomicBool>,
    child_slot: Arc<std::sync::Mutex<Option<Child>>>,
}

/// Dialog shown before code execution or when execution is blocked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CodeRunDialogKind {
    FirstTimeConfirm { block_id: EntityId },
    UnsavedConfirm { block_id: EntityId },
    FirstTimeConfirmInline { target: InlineCodeRunTarget },
    UnsavedConfirmInline { target: InlineCodeRunTarget },
    Disabled,
    UnsupportedLanguage,
}

impl Editor {
    pub(super) fn sync_code_run_visuals(&self, cx: &mut Context<Self>) {
        for visible in self.document.visible_blocks() {
            let entity_id = visible.entity.entity_id();
            let snapshot = self
                .code_runs
                .get(&entity_id)
                .map(CodeBlockRunState::snapshot)
                .unwrap_or_default();
            visible.entity.update(cx, |block, cx| {
                block.set_code_run_snapshot(snapshot);
                cx.notify();
            });
        }
    }

    pub(crate) fn request_code_block_run(&mut self, block_id: EntityId, cx: &mut Context<Self>) {
        if self
            .active_code_run
            .as_ref()
            .is_some_and(|active| active.block_id == block_id)
        {
            self.stop_active_code_run(cx);
            return;
        }

        let preferences = read_app_preferences().unwrap_or_default();
        if !preferences.allow_code_execution {
            self.code_run_dialog = Some(CodeRunDialogKind::Disabled);
            cx.notify();
            return;
        }

        let needs_unsaved_confirm = self.document_dirty || self.file_path.is_none();
        if !preferences.code_execution_confirm_shown {
            self.code_run_dialog = Some(CodeRunDialogKind::FirstTimeConfirm { block_id });
            cx.notify();
            return;
        }
        if needs_unsaved_confirm {
            self.code_run_dialog = Some(CodeRunDialogKind::UnsavedConfirm { block_id });
            cx.notify();
            return;
        }

        self.start_code_block_run(block_id, cx);
    }

    pub(crate) fn on_confirm_code_run_dialog(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.code_run_dialog.clone() else {
            return;
        };
        match dialog {
            CodeRunDialogKind::FirstTimeConfirm { block_id } => {
                if set_code_execution_confirm_shown().is_err() {
                    return;
                }
                self.code_run_dialog = None;
                self.request_code_block_run(block_id, cx);
            }
            CodeRunDialogKind::UnsavedConfirm { block_id } => {
                self.code_run_dialog = None;
                self.start_code_block_run(block_id, cx);
            }
            CodeRunDialogKind::FirstTimeConfirmInline { target } => {
                if set_code_execution_confirm_shown().is_err() {
                    return;
                }
                self.code_run_dialog = None;
                self.request_inline_code_run(target, None, cx);
            }
            CodeRunDialogKind::UnsavedConfirmInline { target } => {
                self.code_run_dialog = None;
                self.start_inline_code_run(target, None, cx);
            }
            CodeRunDialogKind::Disabled | CodeRunDialogKind::UnsupportedLanguage => {
                self.code_run_dialog = None;
                cx.notify();
            }
        }
    }

    pub(crate) fn on_cancel_code_run_dialog(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code_run_dialog = None;
        cx.notify();
    }

    pub(crate) fn on_toggle_code_run_output(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        let entry = self.code_runs.entry(block_id).or_default();
        entry.output_expanded = !entry.output_expanded;
        cx.notify();
    }

    pub(crate) fn on_toggle_code_run_output_content(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        let entry = self.code_runs.entry(block_id).or_default();
        entry.output_content_expanded = !entry.output_content_expanded;
        cx.notify();
    }

    pub(crate) fn on_stop_code_block_run(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_code_run
            .as_ref()
            .is_some_and(|active| active.block_id == block_id)
        {
            self.stop_active_code_run(cx);
        }
    }

    pub(crate) fn on_close_code_block_run_output(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_code_run
            .as_ref()
            .is_some_and(|active| active.block_id == block_id)
        {
            self.stop_active_code_run(cx);
        }
        self.code_runs.remove(&block_id);
        cx.notify();
    }

    pub(crate) fn request_inline_code_run(
        &mut self,
        target: InlineCodeRunTarget,
        source: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_inline_code_run
            .as_ref()
            .is_some_and(|active| active.target == target)
        {
            self.stop_active_inline_code_run(cx);
            return;
        }

        let preferences = read_app_preferences().unwrap_or_default();
        if !preferences.allow_code_execution {
            self.code_run_dialog = Some(CodeRunDialogKind::Disabled);
            cx.notify();
            return;
        }

        let needs_unsaved_confirm = self.document_dirty || self.file_path.is_none();
        if !preferences.code_execution_confirm_shown {
            self.code_run_dialog = Some(CodeRunDialogKind::FirstTimeConfirmInline { target });
            cx.notify();
            return;
        }
        if needs_unsaved_confirm {
            self.code_run_dialog = Some(CodeRunDialogKind::UnsavedConfirmInline { target });
            cx.notify();
            return;
        }

        self.start_inline_code_run(target, source, cx);
    }

    pub(crate) fn inline_code_run_state(
        &self,
        target: &InlineCodeRunTarget,
    ) -> Option<&CodeBlockRunState> {
        self.inline_code_runs.get(target)
    }

    #[cfg(test)]
    pub(crate) fn inline_code_run_statuses_for_test(&self) -> Vec<CodeRunStatus> {
        self.inline_code_runs
            .values()
            .map(|state| state.status)
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn code_run_dialog_for_test(&self) -> Option<CodeRunDialogKind> {
        self.code_run_dialog.clone()
    }

    fn start_inline_code_run(
        &mut self,
        target: InlineCodeRunTarget,
        source: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.stop_active_code_run(cx);
        self.stop_active_inline_code_run(cx);

        let Some(block) = self.focusable_entity_by_id(target.block_id) else {
            return;
        };
        let source = source.unwrap_or_else(|| {
            block.read(cx)
                .inline_code_source_for_visible_span(&target.span_range)
                .unwrap_or_default()
        });
        if source.is_empty() {
            return;
        }

        let work_dir = self.code_run_work_dir();

        let state = self.inline_code_runs.entry(target.clone()).or_default();
        state.reset_for_run();
        self.inline_code_run_popover = Some(target.clone());
        cx.notify();

        let cancel = Arc::new(AtomicBool::new(false));
        let child_slot = Arc::new(std::sync::Mutex::new(None));
        self.active_inline_code_run = Some(ActiveInlineCodeRunControl {
            target: target.clone(),
            cancel: cancel.clone(),
            child_slot: child_slot.clone(),
        });

        let (tx, rx) = mpsc::unbounded();
        let _join = spawn_inline_shell_run(&source, &work_dir, cancel, child_slot, tx);

        cx.spawn(async move |this, cx| {
            let mut rx = rx;
            while let Some(progress) = rx.next().await {
                match progress {
                    CodeRunProgress::StdoutChunk(chunk) => {
                        let _ = this.update(cx, |editor, cx| {
                            if let Some(state) = editor.inline_code_runs.get_mut(&target) {
                                state.stdout.push_str(&chunk);
                                cx.notify();
                            }
                        });
                    }
                    CodeRunProgress::StderrChunk(chunk) => {
                        let _ = this.update(cx, |editor, cx| {
                            if let Some(state) = editor.inline_code_runs.get_mut(&target) {
                                state.stderr.push_str(&chunk);
                                cx.notify();
                            }
                        });
                    }
                    CodeRunProgress::Finished(outcome) => {
                        let _ = this.update(cx, |editor, cx| {
                            editor.finish_inline_code_run(target, outcome, cx);
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn finish_inline_code_run(
        &mut self,
        target: InlineCodeRunTarget,
        outcome: CodeRunOutcome,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.inline_code_runs.get_mut(&target) {
            state.apply_outcome(outcome);
        }
        if self
            .active_inline_code_run
            .as_ref()
            .is_some_and(|active| active.target == target)
        {
            self.active_inline_code_run = None;
        }
        cx.notify();
    }

    fn stop_active_inline_code_run(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.active_inline_code_run.take() else {
            return;
        };
        active.cancel.store(true, Ordering::SeqCst);
        kill_child(&active.child_slot);
        if let Some(state) = self.inline_code_runs.get_mut(&active.target) {
            state.status = CodeRunStatus::Cancelled;
        }
        cx.notify();
    }

    pub(crate) fn on_stop_inline_code_run(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_inline_code_run
            .as_ref()
            .is_some_and(|active| active.target.block_id == block_id)
        {
            self.stop_active_inline_code_run(cx);
        }
    }

    pub(crate) fn on_close_inline_code_run_output(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_inline_code_run
            .as_ref()
            .is_some_and(|active| active.target.block_id == block_id)
        {
            self.stop_active_inline_code_run(cx);
        }
        self.inline_code_runs
            .retain(|target, _| target.block_id != block_id);
        if self
            .inline_code_run_popover
            .as_ref()
            .is_some_and(|target| target.block_id == block_id)
        {
            self.inline_code_run_popover = None;
        }
        cx.notify();
    }

    pub(crate) fn on_toggle_inline_code_run_output_content(
        &mut self,
        target: InlineCodeRunTarget,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.inline_code_runs.get_mut(&target) {
            state.output_content_expanded = !state.output_content_expanded;
            cx.notify();
        }
    }

    pub(super) fn dismiss_inline_code_run_popover(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.inline_code_run_popover.take() else {
            return false;
        };
        if self
            .active_inline_code_run
            .as_ref()
            .is_some_and(|active| active.target == target)
        {
            self.stop_active_inline_code_run(cx);
        }
        self.inline_code_runs.remove(&target);
        cx.notify();
        true
    }

    pub(super) fn prune_stale_inline_code_runs(
        &mut self,
        block_id: EntityId,
        cx: &mut Context<Self>,
    ) {
        let valid_ranges = self
            .focusable_entity_by_id(block_id)
            .map(|block| {
                block
                    .read(cx)
                    .inline_spans()
                    .iter()
                    .filter(|span| span.style.code)
                    .map(|span| span.range.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let mut changed = false;
        self.inline_code_runs.retain(|target, _| {
            if target.block_id != block_id {
                return true;
            }
            let keep = valid_ranges.contains(&target.span_range);
            changed |= !keep;
            keep
        });
        if self.inline_code_run_popover.as_ref().is_some_and(|target| {
            target.block_id == block_id && !valid_ranges.contains(&target.span_range)
        }) {
            self.inline_code_run_popover = None;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn start_code_block_run(&mut self, block_id: EntityId, cx: &mut Context<Self>) {
        self.stop_active_code_run(cx);
        self.stop_active_inline_code_run(cx);

        let Some(block) = self.focusable_entity_by_id(block_id) else {
            return;
        };
        let block_ref = block.read(cx);
        let BlockKind::CodeBlock { language } = &block_ref.record.kind else {
            return;
        };
        let language = language
            .as_ref()
            .map(|value| value.to_string())
            .unwrap_or_default();
        if resolve_runner(&language).is_none() {
            self.code_run_dialog = Some(CodeRunDialogKind::UnsupportedLanguage);
            cx.notify();
            return;
        }

        let source = block_ref.display_text().to_string();
        let work_dir = self.code_run_work_dir();

        let state = self.code_runs.entry(block_id).or_default();
        state.reset_for_run();
        cx.notify();

        let cancel = Arc::new(AtomicBool::new(false));
        let child_slot = Arc::new(std::sync::Mutex::new(None));
        self.active_code_run = Some(ActiveCodeRunControl {
            block_id,
            cancel: cancel.clone(),
            child_slot: child_slot.clone(),
        });

        let (tx, rx) = mpsc::unbounded();
        let _join = spawn_code_run(&language, &source, &work_dir, cancel, child_slot, tx);

        cx.spawn(async move |this, cx| {
            let mut rx = rx;
            while let Some(progress) = rx.next().await {
                match progress {
                    CodeRunProgress::StdoutChunk(chunk) => {
                        let _ = this.update(cx, |editor, cx| {
                            if let Some(state) = editor.code_runs.get_mut(&block_id) {
                                state.stdout.push_str(&chunk);
                                cx.notify();
                            }
                        });
                    }
                    CodeRunProgress::StderrChunk(chunk) => {
                        let _ = this.update(cx, |editor, cx| {
                            if let Some(state) = editor.code_runs.get_mut(&block_id) {
                                state.stderr.push_str(&chunk);
                                cx.notify();
                            }
                        });
                    }
                    CodeRunProgress::Finished(outcome) => {
                        let _ = this.update(cx, |editor, cx| {
                            editor.finish_code_block_run(block_id, outcome, cx);
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn finish_code_block_run(
        &mut self,
        block_id: EntityId,
        outcome: CodeRunOutcome,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = self.code_runs.get_mut(&block_id) {
            state.apply_outcome(outcome);
        }
        if self
            .active_code_run
            .as_ref()
            .is_some_and(|active| active.block_id == block_id)
        {
            self.active_code_run = None;
        }
        cx.notify();
    }

    fn stop_active_code_run(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.active_code_run.take() else {
            return;
        };
        active.cancel.store(true, Ordering::SeqCst);
        kill_child(&active.child_slot);
        if let Some(state) = self.code_runs.get_mut(&active.block_id) {
            state.status = CodeRunStatus::Cancelled;
        }
        cx.notify();
    }

    fn code_run_work_dir(&self) -> PathBuf {
        self.file_path
            .as_ref()
            .and_then(|path| path.parent().map(PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(super) fn render_code_run_dialog_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let dialog = self.code_run_dialog.clone()?;
        let strings = cx.global::<I18nManager>().strings();
        let (title, message, confirm_label, show_cancel) = match dialog {
            CodeRunDialogKind::FirstTimeConfirm { .. }
            | CodeRunDialogKind::FirstTimeConfirmInline { .. } => (
                strings.code_run_confirm_title.clone(),
                strings.code_run_confirm_message.clone(),
                strings.code_run_confirm_allow.clone(),
                true,
            ),
            CodeRunDialogKind::UnsavedConfirm { .. }
            | CodeRunDialogKind::UnsavedConfirmInline { .. } => (
                strings.code_run_unsaved_title.clone(),
                strings.code_run_unsaved_message.clone(),
                strings.code_run_unsaved_confirm.clone(),
                true,
            ),
            CodeRunDialogKind::Disabled => (
                strings.code_run_disabled_title.clone(),
                strings.code_run_disabled_message.clone(),
                strings.info_dialog_ok.clone(),
                false,
            ),
            CodeRunDialogKind::UnsupportedLanguage => (
                strings.code_run_unsupported_title.clone(),
                strings.code_run_unsupported_message.clone(),
                strings.info_dialog_ok.clone(),
                false,
            ),
        };

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let cancel_label = strings.code_run_confirm_cancel.clone();

        Some(
            div()
                .id("code-run-dialog-overlay")
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
                        .w(px(d.dialog_width))
                        .max_w(relative(1.0))
                        .flex()
                        .flex_col()
                        .gap(px(d.dialog_gap))
                        .p(px(d.dialog_padding))
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
                                .text_size(px(t.dialog_body_size))
                                .font_weight(t.dialog_body_weight.to_font_weight())
                                .line_height(rems(t.text_line_height))
                                .text_color(c.dialog_body)
                                .child(message),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(d.dialog_button_gap))
                                .children(if show_cancel {
                                    vec![
                                        div()
                                            .id("cancel-code-run-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .border(px(d.dialog_border_width))
                                            .border_color(c.dialog_border)
                                            .bg(c.dialog_secondary_button_bg)
                                            .hover(|this| {
                                                this.bg(c.dialog_secondary_button_hover)
                                            })
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_secondary_button_text)
                                            .child(cancel_label)
                                            .on_click(cx.listener(Self::on_cancel_code_run_dialog))
                                            .into_any_element(),
                                        div()
                                            .id("confirm-code-run-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .bg(c.dialog_primary_button_bg)
                                            .hover(|this| this.bg(c.dialog_primary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_primary_button_text)
                                            .child(confirm_label)
                                            .on_click(cx.listener(Self::on_confirm_code_run_dialog))
                                            .into_any_element(),
                                    ]
                                } else {
                                    vec![div()
                                        .id("confirm-code-run-dialog")
                                        .h(px(d.dialog_button_height))
                                        .px(px(d.dialog_button_padding_x))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                        .bg(c.dialog_primary_button_bg)
                                        .hover(|this| this.bg(c.dialog_primary_button_hover))
                                        .active(|this| this.opacity(0.92))
                                        .cursor_pointer()
                                        .text_size(px(t.dialog_button_size))
                                        .font_weight(t.dialog_button_weight.to_font_weight())
                                        .text_color(c.dialog_primary_button_text)
                                        .child(confirm_label)
                                        .on_click(cx.listener(Self::on_confirm_code_run_dialog))
                                        .into_any_element()]
                                }),
                        ),
                ),
        )
    }

    fn inline_code_run_anchor_bounds(
        &self,
        target: &InlineCodeRunTarget,
        cx: &App,
    ) -> Option<Bounds<Pixels>> {
        let block = self.focusable_entity_by_id(target.block_id)?;
        block
            .read(cx)
            .visible_range_bounds(target.span_range.clone())
    }

    pub(super) fn render_inline_code_run_button_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(self.view_mode, ViewMode::Rendered) {
            return None;
        }
        let block = self.focused_edit_target(window, cx)?;
        let block_ref = block.read(cx);
        if block_ref.is_source_raw_mode() || block_ref.kind().is_code_block() {
            return None;
        }
        let span = block_ref.inline_code_run_action_span()?;
        let span_bounds = block_ref.visible_range_bounds(span.range.clone())?;
        let theme = cx.global::<crate::theme::ThemeManager>().current_arc();
        let t = &theme.typography;
        let icon_size = px((t.code_size + 1.0).max(12.0));
        let button_size = px(f32::from(icon_size) + 8.0);
        let button_x = f32::from(span_bounds.right()) + 4.0;
        let button_y = f32::from(span_bounds.top());

        let block_entity = block.clone();
        Some(
            div()
                .id("inline-code-run-button")
                .absolute()
                .left(px(button_x))
                .top(px(button_y))
                .w(button_size)
                .h(button_size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .bg(theme.colors.code_bg)
                .border(px(1.0))
                .border_color(theme.colors.code_language_input_border.opacity(0.45))
                .opacity(0.88)
                .hover(|this| this.opacity(1.0))
                .cursor_pointer()
                .occlude()
                .on_mouse_down(MouseButton::Left, {
                    let block_entity = block_entity.clone();
                    let span_range = span.range.clone();
                    cx.listener(move |editor, _, _, cx| {
                        editor.on_block_event(
                            block_entity.clone(),
                            &BlockEvent::RequestRunInlineCode { span_range: span_range.clone() },
                            cx,
                        );
                    })
                })
                .child(
                    svg()
                        .path(ICON_INLINE_CODE_RUN)
                        .size(icon_size)
                        .text_color(theme.colors.code_language_input_text),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_inline_code_run_popover_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let target = self.inline_code_run_popover.clone()?;
        let state = self.inline_code_runs.get(&target)?;
        if state.status == CodeRunStatus::Idle {
            return None;
        }
        let anchor = self.inline_code_run_anchor_bounds(&target, cx)?;
        let strings = cx.global::<I18nManager>().strings();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let running = state.status == CodeRunStatus::Running;
        let icon_size = px((t.code_size - 1.0).max(10.0));
        let action_icon_extent = px(f32::from(icon_size) + 8.0);
        let code_line_height = t.code_size * t.text_line_height;
        let content_line_count =
            code_run_output_line_count(&state.stdout, &state.stderr, state.error_message.as_deref());
        let content_collapsible =
            content_line_count > CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES;
        let content_collapsed = content_collapsible && !state.output_content_expanded;
        let collapsed_max_height =
            px(code_line_height * CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES as f32);
        let hidden_line_count = content_line_count
            .saturating_sub(CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES);

        let mut text_sections = div().w_full().flex().flex_col().gap(px(4.0));
        if !state.stdout.is_empty() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.code_text)
                    .child(state.stdout.clone()),
            );
        }
        if !state.stderr.is_empty() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(state.stderr.clone()),
            );
        }
        if let Some(error) = state.error_message.as_ref() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(error.clone()),
            );
        }

        let exit_label = state
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| strings.code_run_exit_none.clone());
        let meta = strings
            .code_run_meta_template
            .replace("{exit}", &exit_label)
            .replace("{duration}", &state.duration_ms.to_string());

        let mut body = div().w_full();
        let mut content_wrapper = div().relative().w_full().child({
            let mut clipped = div().min_w(px(0.0)).w_full().child(text_sections);
            if content_collapsed {
                clipped = clipped.max_h(collapsed_max_height).overflow_hidden();
            }
            clipped
        });
        if content_collapsed {
            let expand_label = strings
                .code_run_output_expand_lines_template
                .replace("{count}", &hidden_line_count.to_string());
            let expand_target = target.clone();
            content_wrapper = content_wrapper.child(
                div()
                    .mt(px(4.0))
                    .text_size(px((t.code_size - 1.5).max(9.0)))
                    .text_color(c.text_quote)
                    .cursor_pointer()
                    .child(expand_label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _, _, cx| {
                            editor.on_toggle_inline_code_run_output_content(expand_target.clone(), cx);
                        }),
                    ),
            );
        }
        body = body.child(content_wrapper).child(
            div()
                .mt(px(6.0))
                .text_size(px((t.code_size - 1.0).max(10.0)))
                .text_color(c.text_quote)
                .child(meta),
        );

        let mut actions = div().flex().items_center().gap(px(4.0));
        if running {
            let stop_block_id = target.block_id;
            actions = actions.child(
                div()
                    .id("inline-code-run-stop")
                    .w(action_icon_extent)
                    .h(action_icon_extent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .opacity(0.72)
                    .hover(|this| this.opacity(1.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _, _, cx| {
                            editor.on_stop_inline_code_run(stop_block_id, cx);
                        }),
                    )
                    .child(
                        svg()
                            .path(ICON_INLINE_CODE_RUN_STOP)
                            .size(icon_size)
                            .text_color(c.dialog_danger_button_bg),
                    ),
            );
        }
        actions = actions.child(
            div()
                .id("inline-code-run-close")
                .w(action_icon_extent)
                .h(action_icon_extent)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(0.72)
                .hover(|this| this.opacity(1.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|editor, _, _, cx| {
                        editor.dismiss_inline_code_run_popover(cx);
                    }),
                )
                .child(
                    svg()
                        .path(ICON_INLINE_CODE_RUN_CLOSE)
                        .size(icon_size)
                        .text_color(c.code_language_input_text),
                ),
        );

        let panel_width = 320.0f32;
        let panel_height = 120.0f32;
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);
        let below_y = f32::from(anchor.bottom()) + 6.0;
        let above_y = f32::from(anchor.top()) - panel_height - 6.0;
        let space_below = viewport_height - below_y - INLINE_CODE_RUN_VIEWPORT_MARGIN;
        let space_above = f32::from(anchor.top()) - INLINE_CODE_RUN_VIEWPORT_MARGIN;
        let open_upward = space_below < panel_height && space_above > space_below;
        let mut panel_y = if open_upward { above_y } else { below_y };
        panel_y = panel_y
            .max(INLINE_CODE_RUN_VIEWPORT_MARGIN)
            .min((viewport_height - panel_height - INLINE_CODE_RUN_VIEWPORT_MARGIN)
                .max(INLINE_CODE_RUN_VIEWPORT_MARGIN));
        let mut panel_x = f32::from(anchor.left());
        panel_x = panel_x
            .max(INLINE_CODE_RUN_VIEWPORT_MARGIN)
            .min((viewport_width - panel_width - INLINE_CODE_RUN_VIEWPORT_MARGIN)
                .max(INLINE_CODE_RUN_VIEWPORT_MARGIN));

        Some(
            div()
                .id("inline-code-run-popover-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|editor, _, _, cx| {
                        editor.dismiss_inline_code_run_popover(cx);
                    }),
                )
                .child(
                    div()
                        .id("inline-code-run-popover")
                        .absolute()
                        .left(px(panel_x))
                        .top(px(panel_y))
                        .w(px(panel_width))
                        .max_w(px(viewport_width - INLINE_CODE_RUN_VIEWPORT_MARGIN * 2.0))
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .p(px(d.menu_panel_padding))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_size(px((t.code_size - 1.0).max(10.0)))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(c.dialog_title)
                                        .child(if running {
                                            strings.code_run_stop.clone()
                                        } else {
                                            strings.inline_code_run_output_title.clone()
                                        }),
                                )
                                .child(actions),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }

    pub(crate) fn handle_block_code_run_event(
        &mut self,
        block: &Entity<Block>,
        event: &BlockEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        match event {
            BlockEvent::RequestRunCodeBlock => {
                self.request_code_block_run(block.entity_id(), cx);
                true
            }
            BlockEvent::RequestStopCodeBlock => {
                self.on_stop_code_block_run(block.entity_id(), cx);
                true
            }
            BlockEvent::RequestToggleCodeRunOutput => {
                self.on_toggle_code_run_output(block.entity_id(), cx);
                true
            }
            BlockEvent::RequestToggleCodeRunOutputContent => {
                self.on_toggle_code_run_output_content(block.entity_id(), cx);
                true
            }
            BlockEvent::RequestCloseCodeRunOutput => {
                self.on_close_code_block_run_output(block.entity_id(), cx);
                true
            }
            BlockEvent::RequestRunInlineCode { span_range } => {
                let block_id = block.entity_id();
                let block_ref = block.read(cx);
                let source = block_ref.inline_code_source_for_visible_span(span_range);
                self.request_inline_code_run(
                    InlineCodeRunTarget {
                        block_id,
                        span_range: span_range.clone(),
                    },
                    source,
                    cx,
                );
                true
            }
            BlockEvent::RequestStopInlineCode => {
                self.on_stop_inline_code_run(block.entity_id(), cx);
                true
            }
            BlockEvent::RequestCloseInlineCodeRunOutput => {
                self.on_close_inline_code_run_output(block.entity_id(), cx);
                true
            }
            _ => false,
        }
    }
}
