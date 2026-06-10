//! Workspace search field with IME support for CJK input.

use gpui::*;

use super::Editor;
use crate::theme::ThemeManager;

pub(super) struct WorkspaceSearchInputElement {
    editor: Entity<Editor>,
    placeholder: SharedString,
}

pub(super) struct WorkspaceSearchInputPrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl WorkspaceSearchInputElement {
    pub(super) fn new(editor: Entity<Editor>, placeholder: SharedString) -> Self {
        Self { editor, placeholder }
    }
}

impl IntoElement for WorkspaceSearchInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for WorkspaceSearchInputElement {
    type RequestLayoutState = ();
    type PrepaintState = WorkspaceSearchInputPrepaintState;

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
        style.size.height = px(24.0).max(window.line_height()).into();
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
        let content = editor.workspace_search_display_text(&self.placeholder);
        let is_placeholder = editor.workspace_search_query_is_empty();
        let focused = editor.workspace_search_input_active(window);
        let style = window.text_style();
        let text_color = if is_placeholder {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };
        let base_run = TextRun {
            len: content.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let runs = if let Some(marked_range) = editor
            .workspace_search_marked_range()
            .filter(|_| !is_placeholder)
        {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..base_run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(text_color),
                        thickness: px(theme.dimensions.underline_thickness),
                        wavy: false,
                    }),
                    ..base_run.clone()
                },
                TextRun {
                    len: content.len().saturating_sub(marked_range.end),
                    ..base_run.clone()
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![base_run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(content, font_size, &runs, None);
        let line_height = bounds.size.height;

        let marked = editor
            .workspace_search_marked_range()
            .filter(|_| focused && !is_placeholder)
            .map(|marked_range| {
                fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(marked_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(marked_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    theme.colors.selection.opacity(0.35),
                )
            });

        let cursor = if focused && editor.workspace_search_marked_range().is_none() {
            let cursor_offset = editor.workspace_search_cursor_offset();
            let mut cursor_color = theme.colors.cursor;
            cursor_color.a *= 0.85;
            Some(fill(
                Bounds::new(
                    point(
                        bounds.left() + line.x_for_index(cursor_offset),
                        bounds.top(),
                    ),
                    size(px(theme.dimensions.cursor_width), line_height),
                ),
                cursor_color,
            ))
        } else {
            None
        };

        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
        self.editor.update(cx, |editor, _cx| {
            editor.set_workspace_search_layout(line.clone(), bounds);
        });

        WorkspaceSearchInputPrepaintState {
            line: Some(line),
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
        if let Some(hitbox) = prepaint.hitbox.as_ref() {
            if hitbox.is_hovered(window) {
                window.set_cursor_style(CursorStyle::IBeam, hitbox);
            }
        }

        let focus_handle = self.editor.read(cx).workspace_search_focus_handle();
        if focus_handle.is_focused(window) {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.editor.clone()),
                cx,
            );
        }

        if let Some(marked) = prepaint.marked.take() {
            window.paint_quad(marked);
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
