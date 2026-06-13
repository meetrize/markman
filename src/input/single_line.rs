//! Shared selection, mouse interaction, and keyboard helpers for single-line inputs.

use std::ops::Range;

use gpui::*;
use unicode_segmentation::GraphemeCursor;

pub(crate) fn primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    (modifiers.platform || modifiers.control) && !modifiers.alt && !modifiers.function
}

pub(crate) fn extend_single_line_selection(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    offset: usize,
    text_len: usize,
) {
    let offset = offset.min(text_len);
    if *selection_reversed {
        selected_range.start = offset;
    } else {
        selected_range.end = offset;
    }
    if selected_range.end < selected_range.start {
        *selection_reversed = !*selection_reversed;
        *selected_range = selected_range.end..selected_range.start;
    }
}

pub(crate) fn cursor_offset(selected_range: &Range<usize>, selection_reversed: bool) -> usize {
    if selection_reversed {
        selected_range.start
    } else {
        selected_range.end
    }
}

pub(crate) fn move_caret_to(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting: &mut bool,
    offset: usize,
    text_len: usize,
) {
    let offset = offset.min(text_len);
    *selected_range = offset..offset;
    *selection_reversed = false;
    *marked_range = None;
    *is_selecting = false;
}

pub(crate) fn select_caret_to(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    offset: usize,
    text_len: usize,
) {
    extend_single_line_selection(selected_range, selection_reversed, offset, text_len);
    *marked_range = None;
}

pub(crate) fn index_for_mouse_position(
    text_len: usize,
    bounds: Option<&Bounds<Pixels>>,
    line: Option<&ShapedLine>,
    position: Point<Pixels>,
) -> usize {
    if text_len == 0 {
        return 0;
    }

    let (Some(bounds), Some(line)) = (bounds, line) else {
        return text_len;
    };

    if position.x <= bounds.left() {
        return 0;
    }
    if position.x >= bounds.right() {
        return text_len;
    }

    line.closest_index_for_x(position.x - bounds.left())
        .min(text_len)
}

pub(crate) fn handle_mouse_down(
    shift: bool,
    offset: usize,
    text_len: usize,
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting: &mut bool,
) {
    if shift {
        select_caret_to(
            selected_range,
            selection_reversed,
            marked_range,
            offset,
            text_len,
        );
    } else {
        move_caret_to(
            selected_range,
            selection_reversed,
            marked_range,
            is_selecting,
            offset,
            text_len,
        );
    }
    *is_selecting = true;
}

pub(crate) fn handle_mouse_move(
    dragging: bool,
    offset: usize,
    text_len: usize,
    is_selecting: bool,
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    marked_range: &mut Option<Range<usize>>,
    is_selecting_flag: &mut bool,
) -> bool {
    if !is_selecting {
        return false;
    }
    if !dragging {
        *is_selecting_flag = false;
        return true;
    }
    select_caret_to(
        selected_range,
        selection_reversed,
        marked_range,
        offset,
        text_len,
    );
    true
}

pub(crate) fn handle_mouse_up(is_selecting_flag: &mut bool) -> bool {
    if *is_selecting_flag {
        *is_selecting_flag = false;
        return true;
    }
    false
}

pub(crate) fn sanitize_pasted_text(text: &str) -> String {
    super::text_norm::flatten_paste_to_single_line(text)
}

pub(crate) fn text_grapheme_boundary(text: &str, offset: usize, backward: bool) -> usize {
    let offset = offset.min(text.len());
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    if backward {
        cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
    } else {
        cursor
            .next_boundary(text, 0)
            .ok()
            .flatten()
            .unwrap_or(text.len())
    }
}
