//! Unified single-line input element with IME support.

use gpui::*;

use super::single_line_input::SingleLineInputTarget;
use super::Editor;
use crate::input::single_line_field_element::{
    paint_single_line_field, prepaint_single_line_field, request_single_line_field_layout,
    SingleLineFieldElementStyle, SingleLineFieldPrepaint, SingleLineFieldView,
};
use crate::theme::ThemeManager;

pub(super) struct SingleLineInputElement {
    editor: Entity<Editor>,
    target: SingleLineInputTarget,
    placeholder: SharedString,
}

impl SingleLineInputElement {
    pub(super) fn new(
        editor: Entity<Editor>,
        target: SingleLineInputTarget,
        placeholder: SharedString,
    ) -> Self {
        Self {
            editor,
            target,
            placeholder,
        }
    }
}

impl IntoElement for SingleLineInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SingleLineInputElement {
    type RequestLayoutState = ();
    type PrepaintState = SingleLineFieldPrepaint;

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
        (request_single_line_field_layout(window, cx), ())
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
        let editor = self.editor.read(cx);

        let (content, is_placeholder, text_color, focused, marked_range, selected_range, cursor_offset) =
            match self.target {
                SingleLineInputTarget::WorkspaceSearch => {
                    let is_placeholder = editor.workspace_search_query_is_empty();
                    let text_color = if is_placeholder {
                        theme.colors.dialog_muted
                    } else {
                        theme.colors.text_default
                    };
                    (
                        editor.workspace_search_display_text(&self.placeholder),
                        is_placeholder,
                        text_color,
                        editor.workspace_search_input_active(window),
                        editor.workspace_search_marked_range(),
                        editor.workspace_search_selected_range(),
                        editor.workspace_search_cursor_offset(),
                    )
                }
                SingleLineInputTarget::DocumentSearch => {
                    let is_placeholder = editor.document_search_query_is_empty();
                    let text_color = if is_placeholder {
                        theme.colors.dialog_muted
                    } else {
                        theme.colors.text_default
                    };
                    (
                        editor.document_search_display_text(&self.placeholder),
                        is_placeholder,
                        text_color,
                        editor.document_search_input_active(window),
                        editor.document_search_marked_range(),
                        editor.document_search_selected_range(),
                        editor.document_search_cursor_offset(),
                    )
                }
                SingleLineInputTarget::WorkspaceName => {
                    let text = editor.workspace_name_text();
                    (
                        SharedString::from(text),
                        false,
                        theme.colors.dialog_body,
                        editor.workspace_name_input_active(window),
                        editor.workspace_name_marked_range(),
                        editor.workspace_name_selected_range(),
                        editor.workspace_name_cursor_offset(),
                    )
                }
                SingleLineInputTarget::QuickFileOpen => {
                    let is_placeholder = editor.quick_file_open_query_is_empty();
                    let text_color = if is_placeholder {
                        theme.colors.dialog_muted
                    } else {
                        theme.colors.text_default
                    };
                    (
                        editor.quick_file_open_display_text(&self.placeholder),
                        is_placeholder,
                        text_color,
                        editor.quick_file_open_input_active(window),
                        None,
                        0..0,
                        editor.quick_file_open_cursor_offset(),
                    )
                }
            };

        let prepaint = prepaint_single_line_field(
            bounds,
            window,
            cx,
            &theme,
            &SingleLineFieldView {
                content,
                is_placeholder,
                text_color,
                focused,
                marked_range,
                selected_range,
                cursor_offset,
            },
            &SingleLineFieldElementStyle {
                font_scale: self.target.font_scale(),
                truncation_suffix: self.target.truncation_suffix(),
                marked_underline_in_runs: true,
            },
        );

        self.editor.update(cx, |editor, _cx| match self.target {
            SingleLineInputTarget::WorkspaceSearch => {
                editor.set_workspace_search_layout(prepaint.line.clone(), bounds);
            }
            SingleLineInputTarget::DocumentSearch => {
                editor.set_document_search_layout(prepaint.line.clone(), bounds);
            }
            SingleLineInputTarget::WorkspaceName => {
                editor.set_workspace_name_layout(prepaint.line.clone(), bounds);
            }
            SingleLineInputTarget::QuickFileOpen => {}
        });

        prepaint
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
        paint_single_line_field(bounds, prepaint, window, cx);

        let focus_handle = self.editor.read(cx).single_line_input_focus_handle(self.target);
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
        let target = self.target;
        let input_bounds = bounds;
        window.on_mouse_event({
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !input_bounds.contains(&event.position) {
                    return;
                }
                if event.button == MouseButton::Right {
                    cx.stop_propagation();
                    editor_for_down.update(cx, |editor, cx| {
                        editor.on_single_line_input_context_menu_mouse_down(
                            target, event, window, cx,
                        );
                    });
                    return;
                }
                if event.button != MouseButton::Left {
                    return;
                }
                cx.stop_propagation();
                editor_for_down.update(cx, |editor, cx| match target {
                    SingleLineInputTarget::WorkspaceSearch => {
                        editor.on_workspace_search_mouse_down(event, window, cx);
                    }
                    SingleLineInputTarget::DocumentSearch => {
                        editor.on_document_search_mouse_down(event, window, cx);
                    }
                    SingleLineInputTarget::WorkspaceName => {
                        editor.on_workspace_name_mouse_down(event, window, cx);
                    }
                    SingleLineInputTarget::QuickFileOpen => {}
                });
            }
        });
        window.on_mouse_event({
            move |event: &MouseUpEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                editor_for_up.update(cx, |editor, cx| match target {
                    SingleLineInputTarget::WorkspaceSearch => {
                        editor.on_workspace_search_mouse_up(event, _window, cx);
                    }
                    SingleLineInputTarget::DocumentSearch => {
                        editor.on_document_search_mouse_up(event, _window, cx);
                    }
                    SingleLineInputTarget::WorkspaceName => {
                        editor.on_workspace_name_mouse_up(event, _window, cx);
                    }
                    SingleLineInputTarget::QuickFileOpen => {}
                });
            }
        });
        window.on_mouse_event({
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                editor_for_move.update(cx, |editor, cx| match target {
                    SingleLineInputTarget::WorkspaceSearch => {
                        editor.on_workspace_search_mouse_move(event, window, cx);
                    }
                    SingleLineInputTarget::DocumentSearch => {
                        editor.on_document_search_mouse_move(event, window, cx);
                    }
                    SingleLineInputTarget::WorkspaceName => {
                        editor.on_workspace_name_mouse_move(event, window, cx);
                    }
                    SingleLineInputTarget::QuickFileOpen => {}
                });
            }
        });
    }
}
