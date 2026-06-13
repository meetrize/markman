//! Shared GPUI prepaint/paint for single-line text field elements.

use std::ops::Range;

use gpui::*;

use crate::theme::Theme;

/// View-model passed into the shared single-line field renderer.
pub(crate) struct SingleLineFieldView {
    pub content: SharedString,
    pub is_placeholder: bool,
    pub text_color: Hsla,
    pub focused: bool,
    pub marked_range: Option<Range<usize>>,
    pub selected_range: Range<usize>,
    pub cursor_offset: usize,
}

pub(crate) struct SingleLineFieldElementStyle {
    pub font_scale: f32,
    pub truncation_suffix: &'static str,
    /// When true, IME marked text is underlined in the shaped line runs.
    pub marked_underline_in_runs: bool,
}

pub(crate) struct SingleLineFieldPrepaint {
    pub line: ShapedLine,
    pub selection: Option<PaintQuad>,
    pub cursor: Option<PaintQuad>,
    pub marked: Option<PaintQuad>,
    pub hitbox: Hitbox,
}

pub(crate) fn request_single_line_field_layout(
    window: &mut Window,
    cx: &mut App,
) -> LayoutId {
    let mut style = Style::default();
    style.size.width = relative(1.).into();
    style.size.height = relative(1.).into();
    window.request_layout(style, [], cx)
}

pub(crate) fn prepaint_single_line_field(
    bounds: Bounds<Pixels>,
    window: &mut Window,
    _cx: &App,
    theme: &Theme,
    view: &SingleLineFieldView,
    style: &SingleLineFieldElementStyle,
) -> SingleLineFieldPrepaint {
    let window_style = window.text_style();
    let font_size = px(theme.typography.text_size * style.font_scale);
    let content_len = view.content.len();

    let (shape_text, runs) = if view.is_placeholder {
        let mut placeholder_runs = vec![TextRun {
            len: content_len,
            font: window_style.font(),
            color: view.text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let max_width = bounds.size.width;
        let shape_text = if max_width > px(0.0) && !style.truncation_suffix.is_empty() {
            let mut line_wrapper = window.text_system().line_wrapper(window_style.font(), font_size);
            line_wrapper.truncate_line(
                view.content.clone(),
                max_width,
                style.truncation_suffix,
                &mut placeholder_runs,
            )
        } else {
            view.content.clone()
        };
        (shape_text, placeholder_runs)
    } else if style.marked_underline_in_runs
        && let Some(ref marked) = view.marked_range
    {
        let base_run = TextRun {
            len: content_len,
            font: window_style.font(),
            color: view.text_color,
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
                    color: Some(view.text_color),
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
        (view.content.clone(), runs)
    } else {
        (
            view.content.clone(),
            vec![TextRun {
                len: content_len,
                font: window_style.font(),
                color: view.text_color,
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

    let marked = view
        .marked_range
        .as_ref()
        .filter(|_| view.focused && !view.is_placeholder)
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

    let selection = if view.focused && !view.is_placeholder && !view.selected_range.is_empty() {
        Some(fill(
            Bounds::from_corners(
                point(
                    bounds.left() + line.x_for_index(view.selected_range.start),
                    text_top,
                ),
                point(
                    bounds.left() + line.x_for_index(view.selected_range.end),
                    text_bottom,
                ),
            ),
            theme.colors.selection.opacity(0.35),
        ))
    } else {
        None
    };

    let cursor = if view.focused
        && view.marked_range.is_none()
        && view.selected_range.is_empty()
    {
        let mut cursor_color = theme.colors.cursor;
        cursor_color.a *= 0.85;
        Some(fill(
            Bounds::new(
                point(
                    bounds.left() + line.x_for_index(view.cursor_offset),
                    text_top,
                ),
                size(px(theme.dimensions.cursor_width), text_bottom - text_top),
            ),
            cursor_color,
        ))
    } else {
        None
    };

    let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

    SingleLineFieldPrepaint {
        line,
        selection,
        cursor,
        marked,
        hitbox,
    }
}

pub(crate) fn paint_single_line_field(
    bounds: Bounds<Pixels>,
    prepaint: &mut SingleLineFieldPrepaint,
    window: &mut Window,
    cx: &mut App,
) {
    if prepaint.hitbox.is_hovered(window) {
        window.set_cursor_style(CursorStyle::IBeam, &prepaint.hitbox);
    }

    if let Some(marked) = prepaint.marked.take() {
        window.paint_quad(marked);
    }

    if let Some(selection) = prepaint.selection.take() {
        window.paint_quad(selection);
    }

    prepaint
        .line
        .paint(bounds.origin, bounds.size.height, window, cx)
        .ok();

    if let Some(cursor) = prepaint.cursor.take() {
        window.paint_quad(cursor);
    }
}
