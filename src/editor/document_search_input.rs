//! In-document search field with IME support for CJK input.

use gpui::*;

use super::Editor;
use crate::theme::ThemeManager;

const DOCUMENT_SEARCH_FONT_SCALE: f32 = 0.78;
const DOCUMENT_SEARCH_TRUNCATION_SUFFIX: &str = "…";

pub(super) struct DocumentSearchInputElement {
    editor: Entity<Editor>,
    placeholder: SharedString,
}

pub(super) struct DocumentSearchInputPrepaintState {
    line: Option<ShapedLine>,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl DocumentSearchInputElement {
    pub(super) fn new(editor: Entity<Editor>, placeholder: SharedString) -> Self {
        Self { editor, placeholder }
    }
}

impl IntoElement for DocumentSearchInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DocumentSearchInputElement {
    type RequestLayoutState = ();
    type PrepaintState = DocumentSearchInputPrepaintState;

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
        let content = editor.document_search_display_text(&self.placeholder);
        let is_placeholder = editor.document_search_query_is_empty();
        let focused = editor.document_search_input_active(window);
        let style = window.text_style();
        let font_size = px(theme.typography.text_size * DOCUMENT_SEARCH_FONT_SCALE);
        let text_color = if is_placeholder {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };

        let (shape_text, runs) = if is_placeholder {
            let mut placeholder_runs = vec![TextRun {
                len: content.len(),
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }];
            let max_width = bounds.size.width;
            let shape_text = if max_width > px(0.0) {
                let mut line_wrapper = window.text_system().line_wrapper(style.font(), font_size);
                line_wrapper.truncate_line(
                    content,
                    max_width,
                    DOCUMENT_SEARCH_TRUNCATION_SUFFIX,
                    &mut placeholder_runs,
                )
            } else {
                content
            };
            (shape_text, placeholder_runs)
        } else if let Some(marked_range) = editor.document_search_marked_range() {
            let base_run = TextRun {
                len: content.len(),
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = vec![
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
            .collect();
            (content, runs)
        } else {
            let query_len = content.len();
            (
                content,
                vec![TextRun {
                    len: query_len,
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

        let marked = editor
            .document_search_marked_range()
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
            let selected_range = editor.document_search_selected_range();
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

        let cursor = if focused
            && editor.document_search_marked_range().is_none()
            && editor.document_search_selected_range().is_empty()
        {
            let cursor_offset = editor.document_search_cursor_offset();
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
        self.editor.update(cx, |editor, _cx| {
            editor.set_document_search_layout(line.clone(), bounds);
        });

        DocumentSearchInputPrepaintState {
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

        let focus_handle = self.editor.read(cx).document_search_focus_handle();
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
