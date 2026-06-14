//! Sidebar AI chat draft input (multiline textarea + keyboard/mouse handlers).

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui::*;

use super::ai_chat::{AiChatContextMode, AiChatRole};
use super::ai_context::AiContextMode;
use super::controllers::ai::{
    ai_cursor_bounds, ai_range_segments, ai_text_offset_for_position, ai_wrapped_line_height,
};
use super::single_line_input::{
    handle_mouse_down, handle_mouse_move, handle_mouse_up, select_caret_to, text_grapheme_boundary,
    SingleLineArrowKey,
};
use super::Editor;
use crate::components::{Copy, Cut, Delete, DeleteBack, End, Home, MoveLeft, MoveRight, Paste, SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight};
use crate::config::read_app_preferences;
use crate::input::text_norm::normalize_line_endings_lf;
use crate::net::ai::AiChatTurn;

impl Editor {
    pub(in crate::editor) fn ai_chat_input_active(&self, window: &Window) -> bool {
        self.workspace_ai_tab_open() && self.ai_chat.input_focus.is_focused(window)
    }

    pub(in crate::editor) fn ai_chat_draft(&self) -> &str {
        &self.ai_chat.draft
    }

    pub(in crate::editor) fn ai_chat_input_focus_handle(&self) -> FocusHandle {
        self.ai_chat.input_focus.clone()
    }

    pub(in crate::editor) fn ai_chat_marked_range(&self) -> Option<Range<usize>> {
        self.ai_chat.input_marked_range.clone()
    }

    pub(in crate::editor) fn ai_chat_selected_range(&self) -> Range<usize> {
        self.ai_chat.input_selected_range.clone()
    }

    pub(in crate::editor) fn ai_chat_selection_reversed(&self) -> bool {
        self.ai_chat.input_selection_reversed
    }

    pub(in crate::editor) fn ai_chat_cursor_offset(&self) -> usize {
        if self.ai_chat.input_selection_reversed {
            self.ai_chat.input_selected_range.start
        } else {
            self.ai_chat.input_selected_range.end
        }
    }

    pub(in crate::editor) fn ai_chat_offset_for_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.ai_chat.input_last_bounds else {
            return self.ai_chat.draft.len();
        };
        ai_text_offset_for_position(
            &self.ai_chat.input_line_layouts,
            bounds,
            self.ai_chat.input_line_height,
            &self.ai_chat.draft,
            position,
        )
    }

    pub(in crate::editor) fn set_ai_chat_input_layout(
        &mut self,
        lines: Vec<WrappedLine>,
        line_height: Pixels,
        bounds: Bounds<Pixels>,
    ) {
        self.ai_chat.input_line_layouts = lines;
        self.ai_chat.input_line_height = line_height;
        self.ai_chat.input_last_bounds = Some(bounds);
    }

    pub(in crate::editor) fn unmark_ai_chat_draft(&mut self) {
        self.ai_chat.input_marked_range = None;
    }

    pub(in crate::editor) fn sync_ai_chat_cursor_blink(
        &mut self,
        focused: bool,
        cx: &mut Context<Self>,
    ) {
        if !focused {
            self.ai_chat.input_cursor_blink_task = None;
            return;
        }
        if self.ai_chat.input_cursor_blink_task.is_some() {
            return;
        }
        self.ai_chat.input_cursor_blink_task = Some(cx.spawn(
            async |this: WeakEntity<Self>, cx: &mut AsyncApp| loop {
                cx.background_executor().timer(Duration::from_millis(530)).await;
                let should_continue = this
                    .update(cx, |editor, cx| {
                        if editor.ai_chat.input_cursor_blink_task.is_some() {
                            editor.ai_chat.input_cursor_blink_epoch = Instant::now();
                            cx.notify();
                            true
                        } else {
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

    pub(in crate::editor) fn ai_chat_cursor_opacity(&self) -> f32 {
        let elapsed = self.ai_chat.input_cursor_blink_epoch.elapsed().as_secs_f32();
        let phase = elapsed % 1.06;
        if phase < 0.53 { 1.0 } else { 0.0 }
    }

    fn reset_ai_chat_cursor_blink(&mut self) {
        self.ai_chat.input_cursor_blink_epoch = Instant::now();
    }

    pub(in crate::editor) fn on_ai_chat_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.ai_chat.input_focus);
        let offset = self.ai_chat_offset_for_position(event.position);
        handle_mouse_down(
            event.modifiers.shift,
            offset,
            self.ai_chat.draft.len(),
            &mut self.ai_chat.input_selected_range,
            &mut self.ai_chat.input_selection_reversed,
            &mut self.ai_chat.input_marked_range,
            &mut self.ai_chat.input_is_selecting,
        );
        self.reset_ai_chat_cursor_blink();
        cx.notify();
    }

    pub(in crate::editor) fn on_ai_chat_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handle_mouse_up(&mut self.ai_chat.input_is_selecting) {
            cx.notify();
        }
    }

    pub(in crate::editor) fn on_ai_chat_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offset = self.ai_chat_offset_for_position(event.position);
        if handle_mouse_move(
            event.dragging(),
            offset,
            self.ai_chat.draft.len(),
            self.ai_chat.input_is_selecting,
            &mut self.ai_chat.input_selected_range,
            &mut self.ai_chat.input_selection_reversed,
            &mut self.ai_chat.input_marked_range,
            &mut self.ai_chat.input_is_selecting,
        ) {
            self.reset_ai_chat_cursor_blink();
            cx.notify();
        }
    }

    pub(in crate::editor) fn replace_ai_chat_draft(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        mark_composing: bool,
        new_selected: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let start = range.start.min(self.ai_chat.draft.len());
        let end = range.end.min(self.ai_chat.draft.len());
        self.ai_chat.draft.replace_range(start..end, replacement);
        if mark_composing && !replacement.is_empty() {
            self.ai_chat.input_marked_range = Some(start..start + replacement.len());
        } else {
            self.ai_chat.input_marked_range = None;
        }
        self.ai_chat.input_selected_range = new_selected.unwrap_or_else(|| {
            let cursor = start + replacement.len();
            cursor..cursor
        });
        self.ai_chat.input_selection_reversed = false;
        self.reset_ai_chat_cursor_blink();
        cx.notify();
    }

    pub(crate) fn on_ai_chat_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let primary =
            (modifiers.platform || modifiers.control) && !modifiers.alt && !modifiers.function;
        if event.keystroke.key == "enter" && !modifiers.shift && !primary {
            self.send_ai_chat_message(window, cx);
            cx.stop_propagation();
        } else if event.keystroke.key == "enter" {
            let range = self
                .ai_chat
                .input_marked_range
                .clone()
                .unwrap_or_else(|| self.ai_chat.input_selected_range.clone());
            self.replace_ai_chat_draft(range, "\n", false, None, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        let range = if self.ai_chat.input_selected_range.is_empty() {
            let cursor = self.ai_chat_cursor_offset();
            text_grapheme_boundary(&self.ai_chat.draft, cursor, true)..cursor
        } else {
            self.ai_chat.input_selected_range.clone()
        };
        self.replace_ai_chat_draft(range, "", false, None, cx);
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_chat_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        let range = if self.ai_chat.input_selected_range.is_empty() {
            let cursor = self.ai_chat_cursor_offset();
            cursor..text_grapheme_boundary(&self.ai_chat.draft, cursor, false)
        } else {
            self.ai_chat.input_selected_range.clone()
        };
        self.replace_ai_chat_draft(range, "", false, None, cx);
        cx.stop_propagation();
    }

    fn move_ai_chat_caret(&mut self, key: SingleLineArrowKey, cx: &mut Context<Self>) {
        let len = self.ai_chat.draft.len();
        let cursor = self.ai_chat_cursor_offset();
        let next = match key {
            SingleLineArrowKey::MoveLeft | SingleLineArrowKey::SelectLeft => {
                text_grapheme_boundary(&self.ai_chat.draft, cursor, true)
            }
            SingleLineArrowKey::MoveRight | SingleLineArrowKey::SelectRight => {
                text_grapheme_boundary(&self.ai_chat.draft, cursor, false)
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
                    &mut self.ai_chat.input_selected_range,
                    &mut self.ai_chat.input_selection_reversed,
                    &mut self.ai_chat.input_marked_range,
                    next,
                    len,
                );
            }
            _ => {
                self.ai_chat.input_selected_range = next..next;
                self.ai_chat.input_selection_reversed = false;
            }
        }
        self.ai_chat.input_marked_range = None;
        self.reset_ai_chat_cursor_blink();
        cx.notify();
    }

    pub(crate) fn on_ai_chat_move_left(&mut self, _: &MoveLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::MoveLeft, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_move_right(&mut self, _: &MoveRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::MoveRight, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::Home, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::End, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::SelectLeft, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::SelectRight, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_select_home(&mut self, _: &SelectHome, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::SelectHome, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_select_end(&mut self, _: &SelectEnd, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat_input_active(window) {
            self.move_ai_chat_caret(SingleLineArrowKey::SelectEnd, cx);
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_ai_chat_select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        self.ai_chat.input_selected_range = 0..self.ai_chat.draft.len();
        self.ai_chat.input_selection_reversed = false;
        self.ai_chat.input_marked_range = None;
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn on_ai_chat_copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        let range = self.ai_chat.input_selected_range.clone();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.ai_chat.draft[range].to_string()));
        }
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_chat_cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        let range = self.ai_chat.input_selected_range.clone();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.ai_chat.draft[range.clone()].to_string()));
            self.replace_ai_chat_draft(range, "", false, None, cx);
        }
        cx.stop_propagation();
    }

    pub(crate) fn on_ai_chat_paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ai_chat_input_active(window) {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            cx.stop_propagation();
            return;
        };
        let text = normalize_line_endings_lf(&item.text().unwrap_or_default());
        let range = self
            .ai_chat
            .input_marked_range
            .clone()
            .unwrap_or_else(|| self.ai_chat.input_selected_range.clone());
        self.replace_ai_chat_draft(range, &text, false, None, cx);
        cx.stop_propagation();
    }

    pub(in crate::editor) fn send_ai_chat_message(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ai_request_active() {
            return;
        }
        let prompt = self.ai_chat.draft.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.ai_chat.draft.clear();
        self.ai_chat.input_selected_range = 0..0;
        self.ai_chat.input_marked_range = None;

        let user_id = self.ai_chat.next_message_id();
        self.ai_chat.messages.push(super::ai_chat::AiChatMessage {
            id: user_id,
            role: AiChatRole::User,
            content: prompt.clone(),
            streaming: false,
        });

        let history = build_ai_chat_history(&self.ai_chat.messages[..self.ai_chat.messages.len() - 1]);
        let context = match self.collect_ai_chat_send_context(window, cx) {
            Ok(context) => context,
            Err(err) => {
                self.ai_chat.error = Some(err);
                cx.notify();
                return;
            }
        };

        if read_app_preferences().is_err() {
            self.ai_chat.error = Some("Failed to read AI preferences.".to_string());
            cx.notify();
            return;
        }

        self.scroll_ai_chat_to_bottom();
        self.request_ai_chat_completion(prompt, context, history, cx);
    }

    pub(in crate::editor) fn scroll_ai_chat_to_bottom(&mut self) {
        let max = self.ai_chat.scroll_handle.max_offset();
        self.ai_chat
            .scroll_handle
            .set_offset(point(px(0.0), -max.height));
    }
}

pub(in crate::editor) fn ai_chat_context_mode_to_ai_context(mode: AiChatContextMode) -> AiContextMode {
    match mode {
        AiChatContextMode::FullDocument => AiContextMode::FullDocument,
        AiChatContextMode::Selection => AiContextMode::Selection,
        AiChatContextMode::Blank => AiContextMode::Blank,
        AiChatContextMode::Workspace => AiContextMode::Workspace,
        AiChatContextMode::Command => AiContextMode::Command,
    }
}

pub(in crate::editor) fn build_ai_chat_history(messages: &[super::ai_chat::AiChatMessage]) -> Vec<AiChatTurn> {
    messages
        .iter()
        .filter_map(|message| match message.role {
            AiChatRole::User => Some(AiChatTurn {
                role: "user",
                content: message.content.clone(),
            }),
            AiChatRole::Assistant if !message.streaming && !message.content.trim().is_empty() => {
                Some(AiChatTurn {
                    role: "assistant",
                    content: message.content.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

struct AiChatDraftInputElement {
    editor: Entity<Editor>,
    placeholder: SharedString,
}

struct AiChatDraftInputPrepaintState {
    lines: Vec<WrappedLine>,
    line_height: Pixels,
    selection: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl AiChatDraftInputElement {
    fn new(editor: Entity<Editor>, placeholder: SharedString) -> Self {
        Self { editor, placeholder }
    }
}

impl IntoElement for AiChatDraftInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for AiChatDraftInputElement {
    type RequestLayoutState = ();
    type PrepaintState = AiChatDraftInputPrepaintState;

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
        let (focused, draft) = {
            let editor = self.editor.read(cx);
            (
                editor.ai_chat_input_active(window),
                editor.ai_chat_draft().to_string(),
            )
        };
        let is_placeholder = draft.is_empty();
        let text = if is_placeholder {
            self.placeholder.as_ref()
        } else {
            draft.as_str()
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
        let cursor_opacity = self.editor.read(cx).ai_chat_cursor_opacity();
        self.editor.update(cx, |editor, cx| {
            editor.sync_ai_chat_cursor_blink(focused, cx);
        });
        let selected_range = self.editor.read(cx).ai_chat_selected_range();
        let selection = if focused && !is_placeholder && !selected_range.is_empty() {
            ai_range_segments(
                &lines,
                bounds,
                line_height,
                &draft,
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
            let cursor_offset = editor.ai_chat_cursor_offset();
            ai_cursor_bounds(
                &lines,
                bounds,
                line_height,
                &draft,
                cursor_offset,
                px(theme.dimensions.cursor_width),
            )
            .map(|bounds| fill(bounds, theme.colors.cursor.opacity(cursor_opacity)))
        } else {
            None
        };
        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
        self.editor.update(cx, |editor, _cx| {
            editor.set_ai_chat_input_layout(lines.clone(), line_height, bounds);
        });
        AiChatDraftInputPrepaintState {
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
        let focus_handle = self.editor.read(cx).ai_chat_input_focus_handle();
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
                editor.on_ai_chat_mouse_down(event, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let _ = editor_for_up.update(cx, |editor, cx| {
                editor.on_ai_chat_mouse_up(event, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            let _ = editor_for_move.update(cx, |editor, cx| {
                editor.on_ai_chat_mouse_move(event, window, cx);
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

pub(in crate::editor) fn ai_chat_draft_input_element(
    editor: Entity<Editor>,
    placeholder: SharedString,
) -> AnyElement {
    AiChatDraftInputElement::new(editor, placeholder).into_any()
}

#[cfg(test)]
mod tests {
    use super::build_ai_chat_history;
    use crate::editor::ai_chat::{AiChatMessage, AiChatRole};

    #[test]
    fn build_history_skips_streaming_assistant_placeholders() {
        let history = build_ai_chat_history(&[
            AiChatMessage {
                id: 1,
                role: AiChatRole::User,
                content: "hello".to_string(),
                streaming: false,
            },
            AiChatMessage {
                id: 2,
                role: AiChatRole::Assistant,
                content: "world".to_string(),
                streaming: false,
            },
            AiChatMessage {
                id: 3,
                role: AiChatRole::Assistant,
                content: String::new(),
                streaming: true,
            },
        ]);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].content, "hello");
        assert_eq!(history[1].content, "world");
    }
}
