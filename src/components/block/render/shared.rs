//! Shared rendering helpers used across block kind modules.

use gpui::*;

use super::super::{Block, BlockKind, CalloutVariant};
use crate::components::HtmlCssColor;
use crate::layout::centered_column_width;
use crate::theme::{Theme, ThemeDimensions};

// Unicode bullet glyphs for nested list depths.
const BULLET_FILLED: &str = "\u{2022}";
const BULLET_HOLLOW: &str = "\u{25E6}";
const BULLET_SQUARE: &str = "\u{25A1}";
pub(super) fn bulleted_list_marker(depth: usize) -> &'static str {
    match depth {
        0 => BULLET_FILLED,
        1 => BULLET_HOLLOW,
        _ => BULLET_SQUARE,
    }
}
pub(super) fn column_mermaid_available_width(
    block: &Block,
    viewport_width: f32,
    column_count: usize,
    width_fraction: f32,
    stacked: bool,
    d: &ThemeDimensions,
) -> f32 {
    let budget = effective_image_width(block, viewport_width, d);
    if stacked {
        budget.max(120.0)
    } else {
        let column_gap = 24.0;
        let total_gap = column_gap * column_count.saturating_sub(1) as f32;
        ((budget - total_gap) * width_fraction).max(120.0)
    }
}

pub(crate) fn mermaid_available_height(viewport_height: f32, d: &ThemeDimensions) -> f32 {
    let reserved_height = d.menu_bar_height
        + d.format_toolbar_button_height
        + d.format_toolbar_padding_y * 2.0
        + d.format_toolbar_border_width
        + d.editor_padding * 2.0
        + d.block_padding_y * 2.0;
    (viewport_height - reserved_height).max(1.0)
}
pub(super) fn wrap_with_quote_guides(content: AnyElement, quote_depth: usize, theme: &Theme) -> AnyElement {
    if quote_depth == 0 {
        return content;
    }

    let c = &theme.colors;
    let d = &theme.dimensions;
    let guide_offset = d.quote_padding_left;
    let total_padding = guide_offset * quote_depth as f32;

    div()
        .w_full()
        .relative()
        .pl(px(total_padding))
        .child(content)
        .children((0..quote_depth).map(|level| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(guide_offset * level as f32))
                .w(px(d.quote_border_width))
                .bg(c.border_quote)
        }))
        .into_any_element()
}

pub(super) fn callout_accent_and_background(variant: CalloutVariant, theme: &Theme) -> (Hsla, Hsla) {
    let c = &theme.colors;
    match variant {
        CalloutVariant::Note => (c.callout_note_border, c.callout_note_bg),
        CalloutVariant::Tip => (c.callout_tip_border, c.callout_tip_bg),
        CalloutVariant::Important => (c.callout_important_border, c.callout_important_bg),
        CalloutVariant::Warning => (c.callout_warning_border, c.callout_warning_bg),
        CalloutVariant::Caution => (c.callout_caution_border, c.callout_caution_bg),
    }
}

pub(crate) fn visible_quote_guides(block: &Block) -> usize {
    block.visible_quote_depth
}

pub(super) fn container_image_width_budget(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let centered_width = centered_column_width(viewport_width, d);
    let visible_quote_guides = visible_quote_guides(block);
    let quote_inset = d.quote_padding_left * visible_quote_guides as f32;
    let callout_inset = if block.callout_depth > 0 {
        d.callout_padding_x * 2.0 + d.callout_border_width
    } else {
        0.0
    };

    centered_width - quote_inset - callout_inset
}

pub(crate) fn effective_image_width(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let list_inset = d.nested_block_indent * block.render_depth as f32;
    (container_image_width_budget(block, viewport_width, d) - d.block_padding_x * 2.0 - list_inset)
        .max(160.0)
}

pub(super) fn effective_list_item_image_width(block: &Block, viewport_width: f32, d: &ThemeDimensions) -> f32 {
    let marker_width = match block.kind() {
        BlockKind::BulletedListItem => d.list_marker_width,
        BlockKind::TaskListItem { .. } => d.list_marker_width.max(d.task_checkbox_size),
        BlockKind::NumberedListItem => d.ordered_list_marker_width,
        _ => 0.0,
    };
    let list_inset = d.nested_block_indent * block.render_depth as f32;

    (container_image_width_budget(block, viewport_width, d)
        - d.block_padding_x * 2.0
        - list_inset
        - marker_width
        - d.list_marker_gap)
        .max(160.0)
}

/// Returns a human-readable list ordinal: numbers at depth 0, lowercase
/// letters at depth 1, and unicode roman numerals at depth 2+.
pub(super) fn numbered_list_marker(depth: usize, ordinal: usize) -> String {
    match depth {
        0 => format!("{ordinal}."),
        1 => format!("{}.", alphabetic_list_marker(ordinal)),
        _ => format!("{}.", roman_list_marker(ordinal)),
    }
}

/// Expands beyond 26 by wrapping: a...z, a1...z1, a2...z2, ...
fn alphabetic_list_marker(ordinal: usize) -> String {
    const ALPHABET: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

    let ordinal = ordinal.max(1);
    if ordinal <= ALPHABET.len() {
        return char::from(ALPHABET[ordinal - 1]).to_string();
    }

    let wrapped = ordinal - (ALPHABET.len() + 1);
    let letter = char::from(ALPHABET[wrapped % ALPHABET.len()]);
    let suffix = wrapped + 1;
    format!("{letter}{suffix}")
}

/// Converts an ASCII roman numeral string to its unicode ligature equivalents
/// where possible (for example, "III" to a single roman numeral glyph).
fn roman_list_marker(ordinal: usize) -> String {
    let ascii = ascii_roman_numeral(ordinal.max(1));
    let mut index = 0;
    let mut marker = String::new();

    while index < ascii.len() {
        let remaining = &ascii[index..];
        if let Some((token_len, token)) = roman_unicode_token(remaining) {
            marker.push_str(token);
            index += token_len;
        } else {
            break;
        }
    }

    marker
}

fn ascii_roman_numeral(mut ordinal: usize) -> String {
    const MAP: &[(usize, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    for (value, symbol) in MAP {
        while ordinal >= *value {
            result.push_str(symbol);
            ordinal -= *value;
        }
    }
    result
}

fn roman_unicode_token(remaining: &str) -> Option<(usize, &'static str)> {
    const TOKENS: &[(&str, &str)] = &[
        ("XII", "\u{216B}"),
        ("XI", "\u{216A}"),
        ("IX", "\u{2168}"),
        ("VIII", "\u{2167}"),
        ("VII", "\u{2166}"),
        ("VI", "\u{2165}"),
        ("IV", "\u{2163}"),
        ("III", "\u{2162}"),
        ("II", "\u{2161}"),
        ("I", "\u{2160}"),
        ("V", "\u{2164}"),
        ("X", "\u{2169}"),
        ("L", "\u{216C}"),
        ("C", "\u{216D}"),
        ("D", "\u{216E}"),
        ("M", "\u{216F}"),
    ];

    TOKENS.iter().find_map(|(ascii, unicode)| {
        remaining
            .starts_with(ascii)
            .then_some((ascii.len(), *unicode))
    })
}
pub(super) fn html_css_color_to_hsla(color: HtmlCssColor, current_color: Hsla) -> Hsla {
    match color {
        HtmlCssColor::CurrentColor => current_color,
        HtmlCssColor::Rgba(color) => Hsla::from(Rgba {
            r: color.red as f32 / 255.0,
            g: color.green as f32 / 255.0,
            b: color.blue as f32 / 255.0,
            a: color.alpha.clamp(0.0, 1.0),
        }),
    }
}
