//! AI toolbar text inputs in preferences, modeled after document search.

use std::ops::Range;

use gpui::*;

use crate::components::{
    Copy, Cut, Delete, DeleteBack, End, Home, MoveLeft, MoveRight, Paste, SelectAll, SelectEnd,
    SelectHome, SelectLeft, SelectRight,
};
use crate::input::single_line::{
    self, cursor_offset, handle_mouse_down, handle_mouse_move, handle_mouse_up,
    index_for_mouse_position, move_caret_to, primary_shortcut_modifiers, sanitize_pasted_text,
    select_caret_to, text_grapheme_boundary,
};
use crate::theme::{Theme, ThemeManager};

use super::{PreferencesNav, PreferencesWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolbarTextField {
    Label(usize),
    Instruction(usize),
}

#[derive(Debug)]
pub(crate) struct ToolbarTextInputState {
    pub active_field: Option<ToolbarTextField>,
    pub marked_range: Option<Range<usize>>,
    pub selected_range: Range<usize>,
    pub selection_reversed: bool,
    pub is_selecting: bool,
    pub last_layout: Option<ShapedLine>,
    pub last_bounds: Option<Bounds<Pixels>>,
}

impl ToolbarTextInputState {
    pub(crate) fn new() -> Self {
        Self {
            active_field: None,
            marked_range: None,
            selected_range: 0..0,
            selection_reversed: false,
            is_selecting: false,
            last_layout: None,
            last_bounds: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.active_field = None;
        self.marked_range = None;
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.is_selecting = false;
        self.last_layout = None;
        self.last_bounds = None;
    }
}

impl PreferencesWindow {
    pub(super) fn toolbar_text_input_active(&self, window: &Window) -> bool {
        self.nav == PreferencesNav::Ai
            && self.toolbar_text_input.active_field.is_some()
            && self.toolbar_text_focus.is_focused(window)
    }

    pub(super) fn render_toolbar_text_field(
        &self,
        id: impl Into<ElementId>,
        field: ToolbarTextField,
        placeholder: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        div()
            .id(id)
            .h(px(28.0))
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.editor_background)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .overflow_hidden()
                    .child(ToolbarTextInputElement::new(
                        cx.entity(),
                        field,
                        SharedString::from(placeholder),
                    )),
            )
    }

    pub(super) fn render_toolbar_text_input_shell(
        &self,
        _theme: &Theme,
        cx: &mut Context<Self>,
        child: impl IntoElement,
    ) -> Div {
        div()
            .w_full()
            .track_focus(&self.toolbar_text_focus)
            .key_context("BlockEditor")
            .on_key_down(cx.listener(Self::on_toolbar_text_key_down))
            .on_action(cx.listener(Self::on_toolbar_text_delete_back))
            .on_action(cx.listener(Self::on_toolbar_text_delete_forward))
            .on_action(cx.listener(Self::on_toolbar_text_paste))
            .on_action(cx.listener(Self::on_toolbar_text_copy))
            .on_action(cx.listener(Self::on_toolbar_text_cut))
            .on_action(cx.listener(Self::on_toolbar_text_select_all))
            .on_action(cx.listener(Self::on_toolbar_text_move_left))
            .on_action(cx.listener(Self::on_toolbar_text_move_right))
            .on_action(cx.listener(Self::on_toolbar_text_home))
            .on_action(cx.listener(Self::on_toolbar_text_end))
            .on_action(cx.listener(Self::on_toolbar_text_select_left))
            .on_action(cx.listener(Self::on_toolbar_text_select_right))
            .on_action(cx.listener(Self::on_toolbar_text_select_home))
            .on_action(cx.listener(Self::on_toolbar_text_select_end))
            .child(child)
    }

    pub(super) fn toolbar_text_for_field(&self, field: ToolbarTextField) -> &str {
        match field {
            ToolbarTextField::Label(index) => self
                .ai
                .selection_toolbar
                .get(index)
                .map(|button| button.label.as_str())
                .unwrap_or(""),
            ToolbarTextField::Instruction(index) => self
                .ai
                .selection_toolbar
                .get(index)
                .and_then(|button| button.instruction.as_deref())
                .unwrap_or(""),
        }
    }

    fn toolbar_text_is_empty(&self, field: ToolbarTextField) -> bool {
        self.toolbar_text_for_field(field).is_empty()
    }

    fn toolbar_text_display(
        &self,
        field: ToolbarTextField,
        placeholder: &SharedString,
    ) -> SharedString {
        if self.toolbar_text_is_empty(field) {
            placeholder.clone()
        } else {
            SharedString::from(self.toolbar_text_for_field(field).to_string())
        }
    }

    fn toolbar_text_cursor_offset(&self) -> usize {
        cursor_offset(
            &self.toolbar_text_input.selected_range,
            self.toolbar_text_input.selection_reversed,
        )
    }

    fn toolbar_text_index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(field) = self.toolbar_text_input.active_field else {
            return 0;
        };
        index_for_mouse_position(
            self.toolbar_text_for_field(field).len(),
            self.toolbar_text_input.last_bounds.as_ref(),
            self.toolbar_text_input.last_layout.as_ref(),
            position,
        )
    }

    pub(super) fn set_toolbar_text_layout(&mut self, line: ShapedLine, bounds: Bounds<Pixels>) {
        self.toolbar_text_input.last_layout = Some(line);
        self.toolbar_text_input.last_bounds = Some(bounds);
    }

    pub(super) fn on_toolbar_text_mouse_down(
        &mut self,
        field: ToolbarTextField,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.clear_ai_input_state();
        self.toolbar_text_input.active_field = Some(field);
        window.focus(&self.toolbar_text_focus);
        let text_len = self.toolbar_text_for_field(field).len();
        let offset = self.toolbar_text_index_for_mouse_position(event.position);
        handle_mouse_down(
            event.modifiers.shift,
            offset,
            text_len,
            &mut self.toolbar_text_input.selected_range,
            &mut self.toolbar_text_input.selection_reversed,
            &mut self.toolbar_text_input.marked_range,
            &mut self.toolbar_text_input.is_selecting,
        );
        cx.notify();
    }

    pub(super) fn on_toolbar_text_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if handle_mouse_up(&mut self.toolbar_text_input.is_selecting) {
            cx.notify();
        }
    }

    pub(super) fn on_toolbar_text_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        let text_len = self.toolbar_text_for_field(field).len();
        let offset = self.toolbar_text_index_for_mouse_position(event.position);
        if handle_mouse_move(
            event.dragging(),
            offset,
            text_len,
            self.toolbar_text_input.is_selecting,
            &mut self.toolbar_text_input.selected_range,
            &mut self.toolbar_text_input.selection_reversed,
            &mut self.toolbar_text_input.marked_range,
            &mut self.toolbar_text_input.is_selecting,
        ) {
            cx.notify();
        }
    }

    fn toolbar_text_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        let text_len = self.toolbar_text_for_field(field).len();
        move_caret_to(
            &mut self.toolbar_text_input.selected_range,
            &mut self.toolbar_text_input.selection_reversed,
            &mut self.toolbar_text_input.marked_range,
            &mut self.toolbar_text_input.is_selecting,
            offset,
            text_len,
        );
        cx.notify();
    }

    fn toolbar_text_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        let text_len = self.toolbar_text_for_field(field).len();
        select_caret_to(
            &mut self.toolbar_text_input.selected_range,
            &mut self.toolbar_text_input.selection_reversed,
            &mut self.toolbar_text_input.marked_range,
            offset,
            text_len,
        );
        cx.notify();
    }

    fn toolbar_text_replace(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        marked: bool,
        selected_range: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        let text = match field {
            ToolbarTextField::Label(index) => {
                if self.ai.selection_toolbar.len() <= index {
                    return;
                }
                &mut self.ai.selection_toolbar[index].label
            }
            ToolbarTextField::Instruction(index) => {
                if self.ai.selection_toolbar.len() <= index {
                    return;
                }
                let button = &mut self.ai.selection_toolbar[index];
                if button.instruction.is_none() {
                    button.instruction = Some(String::new());
                }
                button.instruction.as_mut().expect("instruction initialized")
            }
        };
        let start = range.start.min(text.len());
        let end = range.end.min(text.len());
        text.replace_range(start..end, replacement);
        self.toolbar_text_input.marked_range = marked.then(|| {
            let marked_start = start;
            let marked_end = start + replacement.len();
            marked_start..marked_end
        });
        if let Some(selected_range) = selected_range {
            self.toolbar_text_input.selected_range = selected_range;
        } else {
            let cursor = start + replacement.len();
            self.toolbar_text_input.selected_range = cursor..cursor;
        }
        self.toolbar_text_input.selection_reversed = false;
        cx.notify();
    }

    fn toolbar_text_delete_backward(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        if let Some(marked) = self.toolbar_text_input.marked_range.clone() {
            let cursor = marked.start;
            self.toolbar_text_replace(marked, "", false, Some(cursor..cursor), cx);
            return;
        }
        let text = self.toolbar_text_for_field(field).to_string();
        let selected = self.toolbar_text_input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor == 0 {
                return;
            }
            let previous = text_grapheme_boundary(&text, cursor, true);
            previous..cursor
        } else {
            selected
        };
        let cursor = delete_range.start;
        self.toolbar_text_replace(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn toolbar_text_delete_forward(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        if let Some(marked) = self.toolbar_text_input.marked_range.clone() {
            let cursor = marked.start;
            self.toolbar_text_replace(marked, "", false, Some(cursor..cursor), cx);
            return;
        }
        let text = self.toolbar_text_for_field(field).to_string();
        let text_len = text.len();
        let selected = self.toolbar_text_input.selected_range.clone();
        let delete_range = if selected.is_empty() {
            let cursor = selected.end;
            if cursor >= text_len {
                return;
            }
            let next = text_grapheme_boundary(&text, cursor, false);
            cursor..next
        } else {
            selected
        };
        let cursor = delete_range.start;
        self.toolbar_text_replace(delete_range, "", false, Some(cursor..cursor), cx);
    }

    fn toolbar_text_replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = if self.toolbar_text_input.selected_range.is_empty() {
            let cursor = self.toolbar_text_cursor_offset();
            cursor..cursor
        } else {
            self.toolbar_text_input.selected_range.clone()
        };
        let cursor = range.start + text.len();
        self.toolbar_text_replace(range, text, false, Some(cursor..cursor), cx);
    }

    fn toolbar_text_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = sanitize_pasted_text(&text);
            self.toolbar_text_replace_selection(&text, cx);
        }
    }

    fn toolbar_text_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        if !self.toolbar_text_input.selected_range.is_empty() {
            let text = self.toolbar_text_for_field(field);
            cx.write_to_clipboard(ClipboardItem::new_string(
                text[self.toolbar_text_input.selected_range.clone()].to_string(),
            ));
        }
    }

    fn toolbar_text_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        self.toolbar_text_copy_to_clipboard(cx);
        if !self.toolbar_text_input.selected_range.is_empty() {
            self.toolbar_text_replace_selection("", cx);
        }
    }

    fn toolbar_text_select_all_text(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.toolbar_text_input.active_field else {
            return;
        };
        self.toolbar_text_move_to(0, cx);
        self.toolbar_text_select_to(self.toolbar_text_for_field(field).len(), cx);
    }

    pub(super) fn on_toolbar_text_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }

        let modifiers = event.keystroke.modifiers;
        if primary_shortcut_modifiers(&modifiers) {
            match event.keystroke.key.as_str() {
                "v" => {
                    self.toolbar_text_paste_from_clipboard(cx);
                    cx.stop_propagation();
                }
                "c" => {
                    self.toolbar_text_copy_to_clipboard(cx);
                    cx.stop_propagation();
                }
                "x" => {
                    self.toolbar_text_cut_to_clipboard(cx);
                    cx.stop_propagation();
                }
                "a" => {
                    self.toolbar_text_select_all_text(cx);
                    cx.stop_propagation();
                }
                _ => {}
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "backspace" => {
                cx.stop_propagation();
                self.toolbar_text_delete_backward(cx);
            }
            "delete" => {
                cx.stop_propagation();
                self.toolbar_text_delete_forward(cx);
            }
            _ => {}
        }
    }

    pub(super) fn on_toolbar_text_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_delete_backward(cx);
    }

    pub(super) fn on_toolbar_text_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_delete_forward(cx);
    }

    pub(super) fn on_toolbar_text_paste(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_paste_from_clipboard(cx);
    }

    pub(super) fn on_toolbar_text_copy(
        &mut self,
        _: &Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_copy_to_clipboard(cx);
    }

    pub(super) fn on_toolbar_text_cut(
        &mut self,
        _: &Cut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_cut_to_clipboard(cx);
    }

    pub(super) fn on_toolbar_text_select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_select_all_text(cx);
    }

    pub(super) fn on_toolbar_text_move_left(
        &mut self,
        _: &MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let field = self.toolbar_text_input.active_field.expect("active field");
        let text = self.toolbar_text_for_field(field).to_string();
        if self.toolbar_text_input.selected_range.is_empty() {
            let previous = text_grapheme_boundary(&text, self.toolbar_text_cursor_offset(), true);
            self.toolbar_text_move_to(previous, cx);
        } else {
            self.toolbar_text_move_to(self.toolbar_text_input.selected_range.start, cx);
        }
    }

    pub(super) fn on_toolbar_text_move_right(
        &mut self,
        _: &MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let field = self.toolbar_text_input.active_field.expect("active field");
        let text = self.toolbar_text_for_field(field).to_string();
        if self.toolbar_text_input.selected_range.is_empty() {
            let next = text_grapheme_boundary(&text, self.toolbar_text_cursor_offset(), false);
            self.toolbar_text_move_to(next, cx);
        } else {
            self.toolbar_text_move_to(self.toolbar_text_input.selected_range.end, cx);
        }
    }

    pub(super) fn on_toolbar_text_home(
        &mut self,
        _: &Home,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_move_to(0, cx);
    }

    pub(super) fn on_toolbar_text_end(
        &mut self,
        _: &End,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let field = self.toolbar_text_input.active_field.expect("active field");
        self.toolbar_text_move_to(self.toolbar_text_for_field(field).len(), cx);
    }

    pub(super) fn on_toolbar_text_select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let field = self.toolbar_text_input.active_field.expect("active field");
        let text = self.toolbar_text_for_field(field).to_string();
        let previous = text_grapheme_boundary(&text, self.toolbar_text_cursor_offset(), true);
        self.toolbar_text_select_to(previous, cx);
    }

    pub(super) fn on_toolbar_text_select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let field = self.toolbar_text_input.active_field.expect("active field");
        let text = self.toolbar_text_for_field(field).to_string();
        let next = text_grapheme_boundary(&text, self.toolbar_text_cursor_offset(), false);
        self.toolbar_text_select_to(next, cx);
    }

    pub(super) fn on_toolbar_text_select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.toolbar_text_select_to(0, cx);
    }

    pub(super) fn on_toolbar_text_select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.toolbar_text_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let field = self.toolbar_text_input.active_field.expect("active field");
        self.toolbar_text_select_to(self.toolbar_text_for_field(field).len(), cx);
    }

    pub(super) fn clear_toolbar_text_input_state(&mut self) {
        self.toolbar_text_input.clear();
    }

    pub(super) fn toolbar_text_replace_for_ime(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        mark_inserted_text: bool,
        selected_after: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        self.toolbar_text_replace(
            range,
            new_text,
            mark_inserted_text,
            selected_after,
            cx,
        );
    }
}

struct ToolbarTextInputElement {
    preferences: Entity<PreferencesWindow>,
    field: ToolbarTextField,
    placeholder: SharedString,
}

struct ToolbarTextInputPrepaintState {
    line: Option<ShapedLine>,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl ToolbarTextInputElement {
    fn new(
        preferences: Entity<PreferencesWindow>,
        field: ToolbarTextField,
        placeholder: SharedString,
    ) -> Self {
        Self {
            preferences,
            field,
            placeholder,
        }
    }
}

impl IntoElement for ToolbarTextInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ToolbarTextInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ToolbarTextInputPrepaintState;

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
        let theme = cx.global::<ThemeManager>().current_arc();
        let preferences = self.preferences.read(cx);
        let focused = preferences.toolbar_text_input.active_field == Some(self.field)
            && preferences.toolbar_text_focus.is_focused(window);
        let is_placeholder = preferences.toolbar_text_is_empty(self.field);
        let content = preferences.toolbar_text_display(self.field, &self.placeholder);
        let text_color = if is_placeholder {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };
        let style = window.text_style();
        let font_size = px(theme.typography.text_size * 0.78);
        let runs = vec![TextRun {
            len: content.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let line = window.text_system().shape_line(content, font_size, &runs, None);
        let line_height = bounds.size.height;
        let padding_top = (line_height - line.ascent - line.descent) / 2.0;
        let text_top = bounds.top() + padding_top;
        let text_bottom = text_top + line.ascent + line.descent;

        let marked = preferences
            .toolbar_text_input
            .marked_range
            .as_ref()
            .filter(|_| focused && !is_placeholder)
            .map(|range| {
                fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(range.start), text_top),
                        point(bounds.left() + line.x_for_index(range.end), text_bottom),
                    ),
                    theme.colors.selection.opacity(0.35),
                )
            });
        let selected_range = if preferences.toolbar_text_input.active_field == Some(self.field) {
            preferences.toolbar_text_input.selected_range.clone()
        } else {
            0..0
        };
        let selection = if focused && !is_placeholder && !selected_range.is_empty() {
            Some(fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(selected_range.start),
                        text_top,
                    ),
                    point(
                        bounds.left() + line.x_for_index(selected_range.end),
                        text_bottom,
                    ),
                ),
                theme.colors.selection.opacity(0.35),
            ))
        } else {
            None
        };
        let cursor = if focused
            && preferences.toolbar_text_input.marked_range.is_none()
            && selected_range.is_empty()
        {
            let mut cursor_color = theme.colors.cursor;
            cursor_color.a *= 0.85;
            Some(fill(
                Bounds::new(
                    point(
                        bounds.left()
                            + line.x_for_index(single_line::cursor_offset(
                                &selected_range,
                                preferences.toolbar_text_input.selection_reversed,
                            )),
                        text_top,
                    ),
                    size(px(theme.dimensions.cursor_width), text_bottom - text_top),
                ),
                cursor_color,
            ))
        } else {
            None
        };
        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
        if preferences.toolbar_text_input.active_field == Some(self.field) {
            self.preferences.update(cx, |preferences, _cx| {
                preferences.set_toolbar_text_layout(line.clone(), bounds);
            });
        }

        ToolbarTextInputPrepaintState {
            line: Some(line),
            selection,
            cursor,
            marked,
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

        let focus_handle = self.preferences.read(cx).toolbar_text_focus.clone();
        if focus_handle.is_focused(window)
            && self.preferences.read(cx).toolbar_text_input.active_field == Some(self.field)
        {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.preferences.clone()),
                cx,
            );
        }

        let preferences_for_down = self.preferences.clone();
        let preferences_for_up = self.preferences.clone();
        let preferences_for_move = self.preferences.clone();
        let field = self.field;
        let input_bounds = bounds;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !input_bounds.contains(&event.position) {
                return;
            }
            if event.button != MouseButton::Left {
                return;
            }
            preferences_for_down.update(cx, |preferences, cx| {
                preferences.on_toolbar_text_mouse_down(field, event, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            preferences_for_up.update(cx, |preferences, cx| {
                preferences.on_toolbar_text_mouse_up(event, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            preferences_for_move.update(cx, |preferences, cx| {
                preferences.on_toolbar_text_mouse_move(event, window, cx);
            });
        });

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        if let Some(marked) = prepaint.marked.take() {
            window.paint_quad(marked);
        }
        if let Some(line) = prepaint.line.take() {
            line.paint(bounds.origin, bounds.size.height, window, cx).ok();
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
    }
}
