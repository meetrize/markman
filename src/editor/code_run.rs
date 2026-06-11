//! Code-block execution state, dialogs, and background task integration.

use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::*;

use crate::code_runner::{
    CodeBlockRunSnapshot, CodeRunOutcome, CodeRunProgress, CodeRunStatus, kill_child,
    resolve_runner, spawn_code_run,
};
use crate::components::{Block, BlockEvent, BlockKind};
use crate::config::{read_app_preferences, set_code_execution_confirm_shown};
use crate::i18n::I18nManager;
use crate::theme::Theme;

use super::Editor;

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

/// Dialog shown before code execution or when execution is blocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeRunDialogKind {
    FirstTimeConfirm { block_id: EntityId },
    UnsavedConfirm { block_id: EntityId },
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
        let Some(dialog) = self.code_run_dialog else {
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

    fn start_code_block_run(&mut self, block_id: EntityId, cx: &mut Context<Self>) {
        self.stop_active_code_run(cx);

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
        let dialog = self.code_run_dialog?;
        let strings = cx.global::<I18nManager>().strings();
        let (title, message, confirm_label, show_cancel) = match dialog {
            CodeRunDialogKind::FirstTimeConfirm { .. } => (
                strings.code_run_confirm_title.clone(),
                strings.code_run_confirm_message.clone(),
                strings.code_run_confirm_allow.clone(),
                true,
            ),
            CodeRunDialogKind::UnsavedConfirm { .. } => (
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
            _ => false,
        }
    }
}
