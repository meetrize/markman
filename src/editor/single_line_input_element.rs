//! Unified single-line input element with IME support.

use gpui::*;

use super::single_line_input::SingleLineInputTarget;
use super::Editor;
use crate::theme::ThemeManager;

pub(super) struct SingleLineInputElement {
    editor: Entity<Editor>,
    target: SingleLineInputTarget,
    placeholder: SharedString,
}

pub(super) struct SingleLineInputPrepaintState {
    line: Option<ShapedLine>,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
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
    type PrepaintState = SingleLineInputPrepaintState;

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
        let editor = self.editor.read(cx);
        let style = window.text_style();
        let font_size = px(theme.typography.text_size * self.target.font_scale());

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
            };

        let content_len = content.len();

        let (shape_text, runs) = if is_placeholder {
            let mut placeholder_runs = vec![TextRun {
                len: content_len,
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            let max_width = bounds.size.width;
            let truncation_suffix = self.target.truncation_suffix();
            let shape_text = if max_width > px(0.0) && !truncation_suffix.is_empty() {
                let mut line_wrapper = window.text_system().line_wrapper(style.font(), font_size);
                line_wrapper.truncate_line(
                    content,
                    max_width,
                    truncation_suffix,
                    &mut placeholder_runs,
                )
            } else {
                content
            };
            (shape_text, placeholder_runs)
        } else if let Some(ref marked) = marked_range {
            let base_run = TextRun {
                len: content_len,
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = vec![
                TextRun {
                    len: marked.start,
                    ..base_run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(text_color),
                        thickness: px(theme.dimensions.underline_thickness),
                        wavy: false,
                    }),
                    ..base_run.clone()
                },
                TextRun {
                    len: content_len.saturating_sub(marked.end),
                    ..base_run.clone()
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect();
            (content, runs)
        } else {
            (
                content,
                vec![TextRun {
                    len: content_len,
                    font: style.font(),
                    color: text_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
            )
        };

        let line = window
            .text_system()
            .shape_line(shape_text, font_size, &runs, None);
        let line_height = bounds.size.height;
        let padding_top = (line_height - line.ascent - line.descent) / 2.0;
        let text_top = bounds.top() + padding_top;
        let text_bottom = text_top + line.ascent + line.descent;

        let marked = marked_range
            .as_ref()
            .filter(|_| focused && !is_placeholder)
            .map(|marked_range| {
                fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(marked_range.start),
                            text_top,
                        ),
                        point(
                            bounds.left() + line.x_for_index(marked_range.end),
                            text_bottom,
                        ),
                    ),
                    theme.colors.selection.opacity(0.35),
                )
            });

        let selection = if focused && !is_placeholder {
            (!selected_range.is_empty()).then(|| {
                fill(
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
                )
            })
        } else {
            None
        };

        let cursor = if focused && marked_range.is_none() && selected_range.is_empty() {
            let mut cursor_color = theme.colors.cursor;
            cursor_color.a *= 0.85;
            Some(fill(
                Bounds::new(
                    point(
                        bounds.left() + line.x_for_index(cursor_offset),
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
        self.editor.update(cx, |editor, _cx| match self.target {
            SingleLineInputTarget::WorkspaceSearch => {
                editor.set_workspace_search_layout(line.clone(), bounds);
            }
            SingleLineInputTarget::DocumentSearch => {
                editor.set_document_search_layout(line.clone(), bounds);
            }
            SingleLineInputTarget::WorkspaceName => {
                editor.set_workspace_name_layout(line.clone(), bounds);
            }
        });

        SingleLineInputPrepaintState {
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
                });
            }
        });

        if let Some(marked) = prepaint.marked.take() {
            window.paint_quad(marked);
        }

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        if let Some(line) = prepaint.line.take() {
            line.paint(bounds.origin, bounds.size.height, window, cx)
                .ok();
        }

        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
    }
}
