//! Sidebar AI chat panel state (messages, draft, context mode).

use std::ops::Range;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::ai_chat_input::{ai_chat_context_mode_to_ai_context, ai_chat_draft_input_element};
use super::ai_context::{self, AiContextMode, AiContextSnapshot};
use super::controllers::ai::ai_prompt_dropdown_item;
use super::Editor;
use crate::config::{read_app_preferences, AiPreferences};
use crate::i18n::I18nStrings;
use crate::net::ai::{
    self as ai_client, AiChatCompletionRequest, AiChatTurn, DEFAULT_SYSTEM_PROMPT,
};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::editor) enum AiChatRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::editor) struct AiChatMessage {
    pub id: u64,
    pub role: AiChatRole,
    pub content: String,
    /// True while the last assistant message is still streaming.
    pub streaming: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(in crate::editor) enum AiChatContextMode {
    #[default]
    FullDocument,
    Selection,
    Blank,
    /// Available when `allow_workspace_context` is enabled in preferences.
    Workspace,
    /// Available when `allow_command_context` is enabled and focus is in a code block.
    Command,
}

pub(in crate::editor) struct AiChatPanelState {
    pub messages: Vec<AiChatMessage>,
    pub draft: String,
    pub context_mode: AiChatContextMode,
    pub has_selection_context: bool,
    pub pinned_selection_context: Option<AiContextSnapshot>,
    pub in_flight: bool,
    pub error: Option<String>,
    pub scroll_handle: ScrollHandle,
    pub input_focus: FocusHandle,
    pub input_selected_range: Range<usize>,
    pub input_selection_reversed: bool,
    pub input_marked_range: Option<Range<usize>>,
    pub input_is_selecting: bool,
    pub input_cursor_blink_epoch: Instant,
    pub input_line_layouts: Vec<WrappedLine>,
    pub input_line_height: Pixels,
    pub input_last_bounds: Option<Bounds<Pixels>>,
    pub input_cursor_blink_task: Option<Task<()>>,
    next_message_id: u64,
}

impl AiChatPanelState {
    pub(in crate::editor) fn new(cx: &mut Context<super::Editor>) -> Self {
        Self {
            messages: Vec::new(),
            draft: String::new(),
            context_mode: AiChatContextMode::default(),
            has_selection_context: false,
            pinned_selection_context: None,
            in_flight: false,
            error: None,
            scroll_handle: ScrollHandle::new(),
            input_focus: cx.focus_handle(),
            input_selected_range: 0..0,
            input_selection_reversed: false,
            input_marked_range: None,
            input_is_selecting: false,
            input_cursor_blink_epoch: Instant::now(),
            input_line_layouts: Vec::new(),
            input_line_height: px(20.0),
            input_last_bounds: None,
            input_cursor_blink_task: None,
            next_message_id: 1,
        }
    }

    pub(in crate::editor) fn clear_conversation(&mut self) {
        self.messages.clear();
        self.draft.clear();
        self.error = None;
        self.in_flight = false;
        self.context_mode = AiChatContextMode::Blank;
        self.pinned_selection_context = None;
        self.input_selected_range = 0..0;
        self.input_selection_reversed = false;
        self.input_marked_range = None;
        self.input_is_selecting = false;
        self.input_cursor_blink_task = None;
    }

    pub(in crate::editor) fn next_message_id(&mut self) -> u64 {
        let id = self.next_message_id;
        self.next_message_id = self.next_message_id.saturating_add(1);
        id
    }
}

enum AiChatStreamEvent {
    Delta(String),
    Done(anyhow::Result<String>),
}

pub(in crate::editor) fn sidebar_chat_system_prompt() -> String {
    format!(
        "{DEFAULT_SYSTEM_PROMPT}\n\nYou are in the sidebar chat panel. Maintain conversational continuity across turns."
    )
}

pub(in crate::editor) fn build_ai_chat_completion_request(
    ai_preferences: AiPreferences,
    user_prompt: String,
    context: &AiContextSnapshot,
    history: Vec<AiChatTurn>,
) -> AiChatCompletionRequest {
    let context_markdown = if history.is_empty() && !context.context_markdown.trim().is_empty() {
        Some(context.context_markdown.clone())
    } else {
        None
    };
    let mut turns = history;
    turns.push(AiChatTurn {
        role: "user",
        content: user_prompt,
    });
    AiChatCompletionRequest {
        preferences: ai_preferences,
        system_prompt: sidebar_chat_system_prompt(),
        turns,
        context_markdown,
    }
}

pub(in crate::editor) fn ai_chat_context_label(
    mode: AiChatContextMode,
    strings: &I18nStrings,
) -> &str {
    match mode {
        AiChatContextMode::Selection => &strings.workspace_ai_context_selection,
        AiChatContextMode::FullDocument => &strings.workspace_ai_context_full,
        AiChatContextMode::Blank => &strings.workspace_ai_context_blank,
        AiChatContextMode::Workspace => &strings.workspace_ai_context_workspace,
        AiChatContextMode::Command => &strings.workspace_ai_context_command,
    }
}

impl Editor {
    pub(in crate::editor) fn ai_request_active(&self) -> bool {
        self.ai.in_flight() || self.ai_chat.in_flight
    }

    pub(in crate::editor) fn request_ai_chat_completion(
        &mut self,
        user_prompt: String,
        context: AiContextSnapshot,
        history: Vec<AiChatTurn>,
        cx: &mut Context<Self>,
    ) {
        if self.ai_request_active() {
            return;
        }

        let ai_preferences = match read_app_preferences() {
            Ok(preferences) => preferences.ai,
            Err(err) => {
                self.ai_chat.error = Some(format!("Failed to read AI preferences: {err}"));
                cx.notify();
                return;
            }
        };

        self.ai_chat.in_flight = true;
        self.ai_chat.error = None;
        let assistant_id = self.ai_chat.next_message_id();
        self.ai_chat.messages.push(AiChatMessage {
            id: assistant_id,
            role: AiChatRole::Assistant,
            content: String::new(),
            streaming: true,
        });

        let request =
            build_ai_chat_completion_request(ai_preferences, user_prompt, &context, history);
        let weak_editor = cx.entity().downgrade();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let stream_tx = tx.clone();
            let result = ai_client::complete_chat_streaming(request, move |delta| {
                let _ = stream_tx.send(AiChatStreamEvent::Delta(delta));
            });
            let _ = tx.send(AiChatStreamEvent::Done(result));
        });

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;

                let mut deltas = Vec::new();
                let mut done = None;
                loop {
                    match rx.try_recv() {
                        Ok(AiChatStreamEvent::Delta(delta)) => deltas.push(delta),
                        Ok(AiChatStreamEvent::Done(result)) => {
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
                if weak_editor
                    .update(cx, move |editor, cx| {
                        if let Some(last) = editor.ai_chat.messages.last_mut() {
                            let had_deltas = !deltas.is_empty();
                            for delta in deltas {
                                last.content.push_str(&delta);
                            }
                            if had_deltas {
                                editor.scroll_ai_chat_to_bottom();
                            }
                        }
                        if let Some(result) = done {
                            editor.ai_chat.in_flight = false;
                            if let Some(last) = editor.ai_chat.messages.last_mut() {
                                last.streaming = false;
                            }
                            match result {
                                Ok(final_content) => {
                                    if let Some(last) = editor.ai_chat.messages.last_mut() {
                                        last.content = final_content;
                                    }
                                }
                                Err(err) => {
                                    editor.ai_chat.error = Some(err.to_string());
                                    if let Some(last) = editor.ai_chat.messages.last()
                                        && last.content.trim().is_empty()
                                    {
                                        editor.ai_chat.messages.pop();
                                    }
                                }
                            }
                            editor.scroll_ai_chat_to_bottom();
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

    pub(in crate::editor) fn refresh_ai_chat_context_availability(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let live_selection = ai_context::collect_editor_ai_context(
            self,
            AiContextMode::Selection,
            None,
            window,
            cx,
        )
        .is_ok();
        self.ai_chat.has_selection_context =
            self.ai_chat.pinned_selection_context.is_some() || live_selection;
    }

    pub(in crate::editor) fn apply_ai_chat_selection_snapshot(
        &mut self,
        snapshot: ai_context::AiContextSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_chat.pinned_selection_context = Some(snapshot);
        self.ai_chat.context_mode = AiChatContextMode::Selection;
        self.ai_chat.has_selection_context = true;
        self.open_ai_chat_tab(window, cx);
        cx.notify();
    }

    pub(in crate::editor) fn add_selection_to_ai_chat(
        &mut self,
        snapshot: Option<AiContextSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = snapshot.or_else(|| {
            self.preserve_ai_selection_visuals(cx);
            ai_context::snapshot_editor_selection_context(self, window, cx)
        });
        if let Some(snapshot) = snapshot {
            self.apply_ai_chat_selection_snapshot(snapshot, window, cx);
        }
    }

    pub(in crate::editor) fn clear_ai_chat_pinned_selection(&mut self, cx: &mut Context<Self>) {
        self.ai_chat.pinned_selection_context = None;
        if self.ai_chat.context_mode == AiChatContextMode::Selection {
            self.ai_chat.context_mode = AiChatContextMode::FullDocument;
        }
        cx.notify();
    }

    pub(in crate::editor) fn collect_ai_chat_send_context(
        &self,
        window: &Window,
        cx: &App,
    ) -> Result<AiContextSnapshot, String> {
        match self.ai_chat.context_mode {
            AiChatContextMode::Selection => {
                if let Some(pinned) = self.ai_chat.pinned_selection_context.as_ref() {
                    if pinned.context_markdown.trim().is_empty() {
                        return Err("当前没有选中文本。".to_string());
                    }
                    return Ok(pinned.clone());
                }
                let mode = ai_chat_context_mode_to_ai_context(AiChatContextMode::Selection);
                ai_context::collect_editor_ai_context(self, mode, None, window, cx)
            }
            AiChatContextMode::FullDocument => Ok(AiContextSnapshot {
                context_markdown: self.serialized_document_text(cx),
                target_label: self
                    .document_file_display_name()
                    .map(|file| format!("{file} 全文"))
                    .unwrap_or_else(|| "全文".to_string()),
                source_file_name: self.document_file_display_name(),
                start_line: None,
                end_line: None,
            }),
            AiChatContextMode::Workspace => {
                let workspace_context = self
                    .ai_workspace_context()
                    .filter(|context| !context.trim().is_empty())
                    .ok_or_else(|| "工作区中没有可用的上下文。".to_string())?;
                Ok(AiContextSnapshot {
                    context_markdown: workspace_context,
                    target_label: "工作区".to_string(),
                    source_file_name: None,
                    start_line: None,
                    end_line: None,
                })
            }
            AiChatContextMode::Blank | AiChatContextMode::Command => {
                let mode = ai_chat_context_mode_to_ai_context(self.ai_chat.context_mode);
                let mut snapshot =
                    ai_context::collect_editor_ai_context(self, mode, None, window, cx)?;
                let preferences = read_app_preferences()
                    .map_err(|err| format!("Failed to read AI preferences: {err}"))?;
                if self.ai_chat.context_mode == AiChatContextMode::Command
                    && preferences.ai.allow_command_context
                    && let Some(command_context) = self.ai_command_context(window, cx)
                {
                    if !snapshot.context_markdown.trim().is_empty() {
                        snapshot
                            .context_markdown
                            .push_str("\n\n---\nCommand/code context:\n\n");
                    }
                    snapshot.context_markdown.push_str(&command_context);
                }
                Ok(snapshot)
            }
        }
    }

    fn ai_chat_workspace_context_available(&self) -> bool {
        self.effective_workspace_root().is_some()
            && self
                .ai_workspace_context()
                .is_some_and(|context| !context.trim().is_empty())
    }

    fn ai_chat_command_context_available(&self, window: &Window, cx: &App) -> bool {
        read_app_preferences()
            .ok()
            .is_some_and(|preferences| preferences.ai.allow_command_context)
            && self.ai_command_context(window, cx).is_some()
    }

    fn select_ai_chat_context_mode(
        &mut self,
        mode: AiChatContextMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match mode {
            AiChatContextMode::FullDocument
            | AiChatContextMode::Workspace
            | AiChatContextMode::Blank => {
                self.ai_chat.pinned_selection_context = None;
            }
            _ => {}
        }
        self.ai_chat.context_mode = mode;
        self.refresh_ai_chat_context_availability(window, cx);
        cx.notify();
    }

    fn ai_chat_context_mode_chip(
        &self,
        id: &'static str,
        label: &str,
        mode: AiChatContextMode,
        enabled: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.ai_chat.context_mode == mode;
        ai_prompt_dropdown_item(
            id,
            label,
            selected,
            enabled,
            theme,
            cx.listener(move |editor, _, window, cx| {
                if !enabled {
                    return;
                }
                editor.select_ai_chat_context_mode(mode, window, cx);
            }),
        )
    }

    fn ai_chat_full_document_context_label(&self, strings: &I18nStrings) -> String {
        self.document_file_display_name()
            .map(|file| format!("{file} · {}", strings.workspace_ai_context_full))
            .unwrap_or_else(|| strings.workspace_ai_context_full.clone())
    }

    fn ai_chat_workspace_context_label(&self, strings: &I18nStrings) -> String {
        self.effective_workspace_root()
            .and_then(|root| {
                root.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| format!("{name} · {}", strings.workspace_ai_context_workspace))
            })
            .unwrap_or_else(|| strings.workspace_ai_context_workspace.clone())
    }

    pub(super) fn render_ai_chat_panel(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Self>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use super::toolbar_button::toolbar_icon_button;

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let new_chat_editor = editor.clone();
        let send_editor = editor.clone();
        let can_send = !self.ai_chat.draft.trim().is_empty() && !self.ai_chat.in_flight;
        let entity = cx.entity();

        let header = div()
            .id("ai-chat-panel-header")
            .w_full()
            .px(px(4.0))
            .pb(px(6.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.0))
            .child(
                toolbar_icon_button(
                    "ai-chat-panel-settings",
                    theme,
                    "icon/toolbar/settings-2.svg",
                    false,
                    false,
                    &strings.workspace_ai_settings,
                    false,
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    crate::config::open_preferences_window_to_ai(cx);
                }),
            )
            .child(
                div()
                    .id("ai-chat-panel-new-chat")
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(d.format_toolbar_button_radius))
                    .text_sm()
                    .text_color(c.dialog_secondary_button_text)
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .child(strings.workspace_ai_new_chat.clone())
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = new_chat_editor.update(cx, |editor, cx| {
                            editor.ai_chat.clear_conversation();
                            cx.notify();
                        });
                    }),
            );

        let preferences_ready = read_app_preferences().is_ok();
        let selection_context_available = self.ai_chat.has_selection_context;
        let workspace_context_available = self.ai_chat_workspace_context_available();
        let command_context_available = self.ai_chat_command_context_available(window, cx);

        let pinned_reference_label = (self.ai_chat.context_mode == AiChatContextMode::Selection)
            .then(|| {
                self.ai_chat
                    .pinned_selection_context
                    .as_ref()
                    .and_then(|context| {
                        context.format_reference_label(&strings.workspace_ai_untitled_document)
                    })
            })
            .flatten();

        let messages = if self.ai_chat.messages.is_empty() {
            let mut empty = div()
                .id("ai-chat-empty-state")
                .py(px(24.0))
                .px(px(8.0))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0))
                .text_sm()
                .text_color(c.dialog_muted)
                .text_center();
            if !preferences_ready {
                empty = empty
                    .child(strings.workspace_ai_empty_no_api.clone())
                    .child(
                        div()
                            .id("ai-chat-open-preferences")
                            .px(px(10.0))
                            .py(px(4.0))
                            .rounded(px(d.menu_item_radius))
                            .bg(c.dialog_secondary_button_bg)
                            .text_color(c.dialog_secondary_button_text)
                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                            .cursor_pointer()
                            .child(strings.workspace_ai_error_no_api.clone())
                            .on_click(|_, _, cx| {
                                crate::config::open_preferences_window_to_ai(cx);
                            }),
                    );
            } else if self.ai_chat.error.is_some() {
                empty = empty.child(strings.workspace_ai_empty_error.clone());
            } else {
                empty = empty.child(strings.workspace_ai_empty.clone());
            }
            vec![empty.into_any()]
        } else {
            self.ai_chat
                .messages
                .iter()
                .map(|message| {
                    let is_user = message.role == AiChatRole::User;
                    let is_assistant = message.role == AiChatRole::Assistant;
                    let content = message.content.clone();
                    let copy_content = content.clone();
                    let insert_content = content.clone();
                    let insert_editor = editor.clone();
                    let copy_label = strings.workspace_ai_copy.clone();
                    let insert_label = strings.workspace_ai_insert.clone();
                    let bubble = div()
                        .max_w(relative(0.92))
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .bg(if is_user {
                            c.selection.opacity(0.35)
                        } else {
                            c.editor_background
                        })
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border.opacity(0.6))
                        .text_size(px((t.dialog_body_size - 1.0).max(11.0)))
                        .text_color(c.text_default)
                        .child(content.clone());
                    let actions = if is_assistant && !content.trim().is_empty() && !message.streaming {
                        Some(
                            div()
                                .id(("ai-chat-message-actions", message.id))
                                .mt(px(4.0))
                                .flex()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .id(("ai-chat-copy", message.id))
                                        .text_size(px((t.dialog_body_size - 2.0).max(10.0)))
                                        .text_color(c.dialog_muted)
                                        .hover(|this| {
                                            this.text_color(c.text_link).cursor_pointer()
                                        })
                                        .child(copy_label)
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy_content.clone(),
                                            ));
                                        }),
                                )
                                .child(
                                    div()
                                        .id(("ai-chat-insert", message.id))
                                        .text_size(px((t.dialog_body_size - 2.0).max(10.0)))
                                        .text_color(c.dialog_muted)
                                        .hover(|this| {
                                            this.text_color(c.text_link).cursor_pointer()
                                        })
                                        .child(insert_label)
                                        .on_click(move |_, _, cx| {
                                            let markdown = insert_content.clone();
                                            let _ = insert_editor.update(cx, |editor, cx| {
                                                editor.insert_markdown_after_cursor(&markdown, cx);
                                            });
                                        }),
                                )
                                .into_any(),
                        )
                    } else {
                        None
                    };
                    div()
                        .id(("ai-chat-message", message.id))
                        .mb(px(8.0))
                        .w_full()
                        .flex()
                        .flex_col()
                        .when(is_user, |this| this.items_end())
                        .child(bubble)
                        .children(actions)
                        .into_any()
                })
                .collect()
        };

        let error_banner = self.ai_chat.error.as_ref().map(|error| {
            div()
                .id("ai-chat-error")
                .mb(px(6.0))
                .px(px(8.0))
                .py(px(6.0))
                .rounded(px(4.0))
                .bg(c.callout_warning_bg.opacity(0.45))
                .text_size(px((t.dialog_body_size - 1.0).max(11.0)))
                .text_color(c.callout_warning_border)
                .child(error.clone())
        });

        let message_list = div()
            .id("ai-chat-message-list")
            .w_full()
            .flex()
            .flex_col()
            .children(messages);

        let input_area = div()
            .id("ai-chat-input-area")
            .w_full()
            .px(px(4.0))
            .pt(px(6.0))
            .border_t(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .flex()
            .flex_col()
            .gap(px(6.0))
            .key_context("BlockEditor")
            .track_focus(&self.ai_chat.input_focus)
            .on_key_down(cx.listener(Self::on_ai_chat_key_down))
            .on_action(cx.listener(Self::on_ai_chat_delete_back))
            .on_action(cx.listener(Self::on_ai_chat_delete_forward))
            .on_action(cx.listener(Self::on_ai_chat_paste))
            .on_action(cx.listener(Self::on_ai_chat_copy))
            .on_action(cx.listener(Self::on_ai_chat_cut))
            .on_action(cx.listener(Self::on_ai_chat_select_all))
            .on_action(cx.listener(Self::on_ai_chat_move_left))
            .on_action(cx.listener(Self::on_ai_chat_move_right))
            .on_action(cx.listener(Self::on_ai_chat_home))
            .on_action(cx.listener(Self::on_ai_chat_end))
            .on_action(cx.listener(Self::on_ai_chat_select_left))
            .on_action(cx.listener(Self::on_ai_chat_select_right))
            .on_action(cx.listener(Self::on_ai_chat_select_home))
            .on_action(cx.listener(Self::on_ai_chat_select_end))
            .child(
                div()
                    .id("ai-chat-draft-input")
                    .h(px(72.0))
                    .px(px(8.0))
                    .py(px(6.0))
                    .rounded(px(d.menu_item_radius))
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .bg(c.editor_background)
                    .overflow_hidden()
                    .child(ai_chat_draft_input_element(
                        entity,
                        SharedString::from(strings.workspace_ai_input_placeholder.clone()),
                    )),
            )
            .when(pinned_reference_label.is_some(), |this| {
                let label = pinned_reference_label.clone().unwrap_or_default();
                this.child(
                    div()
                        .id("ai-chat-pinned-reference")
                        .w_full()
                        .px(px(4.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            svg()
                                .path("icon/toolbar/sparkles.svg")
                                .size(px(12.0))
                                .text_color(c.dialog_muted),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_size(px((t.dialog_body_size - 2.0).max(10.0)))
                                .text_color(c.dialog_muted)
                                .child(label),
                        )
                        .child(
                            div()
                                .id("ai-chat-clear-pinned-reference")
                                .flex_shrink_0()
                                .px(px(4.0))
                                .text_size(px((t.dialog_body_size - 2.0).max(10.0)))
                                .text_color(c.dialog_muted)
                                .hover(|this| {
                                    this.text_color(c.text_link).cursor_pointer()
                                })
                                .child("×")
                                .on_click(cx.listener(|editor, _, _, cx| {
                                    editor.clear_ai_chat_pinned_selection(cx);
                                })),
                        ),
                )
            })
            .child(
                div()
                    .id("ai-chat-context-modes")
                    .w_full()
                    .px(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(self.ai_chat_context_mode_chip(
                        "ai-chat-context-selection",
                        &strings.workspace_ai_context_selection,
                        AiChatContextMode::Selection,
                        selection_context_available,
                        theme,
                        cx,
                    ))
                    .child(self.ai_chat_context_mode_chip(
                        "ai-chat-context-full-document",
                        &self.ai_chat_full_document_context_label(strings),
                        AiChatContextMode::FullDocument,
                        true,
                        theme,
                        cx,
                    ))
                    .child(self.ai_chat_context_mode_chip(
                        "ai-chat-context-workspace",
                        &self.ai_chat_workspace_context_label(strings),
                        AiChatContextMode::Workspace,
                        workspace_context_available,
                        theme,
                        cx,
                    ))
                    .child(self.ai_chat_context_mode_chip(
                        "ai-chat-context-blank",
                        ai_chat_context_label(AiChatContextMode::Blank, strings),
                        AiChatContextMode::Blank,
                        true,
                        theme,
                        cx,
                    ))
                    .child(self.ai_chat_context_mode_chip(
                        "ai-chat-context-command",
                        ai_chat_context_label(AiChatContextMode::Command, strings),
                        AiChatContextMode::Command,
                        command_context_available,
                        theme,
                        cx,
                    )),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(6.0))
                    .child(
                        div()
                            .id("ai-chat-send")
                            .h(px(28.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(d.menu_item_radius))
                            .bg(if can_send {
                                c.dialog_primary_button_bg
                            } else {
                                c.dialog_secondary_button_bg
                            })
                            .text_size(px((t.dialog_body_size - 1.0).max(11.0)))
                            .text_color(if can_send {
                                c.dialog_primary_button_text
                            } else {
                                c.dialog_muted
                            })
                            .when(can_send, |this| {
                                this.hover(|this| this.bg(c.dialog_primary_button_hover))
                                    .cursor_pointer()
                            })
                            .child(strings.workspace_ai_send.clone())
                            .on_click(move |_, window, cx| {
                                if !can_send {
                                    return;
                                }
                                let _ = send_editor.update(cx, |editor, cx| {
                                    editor.send_ai_chat_message(window, cx);
                                });
                            }),
                    ),
            );

        div()
            .id("ai-chat-panel")
            .relative()
            .w_full()
            .h_full()
            .min_h(px(160.0))
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("ai-chat-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.ai_chat.scroll_handle)
                    .px(px(4.0))
                    .py(px(4.0))
                    .children(error_banner)
                    .child(message_list),
            )
            .child(input_area)
            .into_any()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_ai_chat_completion_request, sidebar_chat_system_prompt, AiContextSnapshot,
    };
    use crate::config::AiPreferences;
    use crate::net::ai::{AiChatTurn, DEFAULT_SYSTEM_PROMPT};

    fn sample_preferences() -> AiPreferences {
        AiPreferences {
            provider: "openai-compatible".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            model: "gpt-test".to_string(),
            api_key_env: "TEST_KEY".to_string(),
            allow_full_document_context: true,
            allow_workspace_context: false,
            allow_command_context: false,
            selection_toolbar: Vec::new(),
        }
    }

    #[test]
    fn sidebar_system_prompt_extends_default() {
        assert!(sidebar_chat_system_prompt().starts_with(DEFAULT_SYSTEM_PROMPT));
        assert!(sidebar_chat_system_prompt().contains("sidebar chat panel"));
    }

    #[test]
    fn build_request_attaches_context_only_on_first_turn() {
        let context = AiContextSnapshot {
            context_markdown: "doc body".to_string(),
            target_label: "full document".to_string(),
            source_file_name: None,
            start_line: None,
            end_line: None,
        };
        let first = build_ai_chat_completion_request(
            sample_preferences(),
            "hello".to_string(),
            &context,
            vec![],
        );
        assert_eq!(first.context_markdown.as_deref(), Some("doc body"));

        let follow_up = build_ai_chat_completion_request(
            sample_preferences(),
            "again".to_string(),
            &context,
            vec![
                AiChatTurn {
                    role: "user",
                    content: "hello".to_string(),
                },
                AiChatTurn {
                    role: "assistant",
                    content: "hi".to_string(),
                },
            ],
        );
        assert!(follow_up.context_markdown.is_none());
        assert_eq!(first.turns.len(), 1);
        assert_eq!(first.turns[0].content, "hello");
        assert_eq!(follow_up.turns.len(), 3);
        assert_eq!(follow_up.turns[2].content, "again");
    }
}
