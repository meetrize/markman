//! Standard Markdown formatting helpers for source-mode toolbar actions.

use std::ops::Range;

/// Default link label inserted when the toolbar link action has no selection.
pub(crate) const DEFAULT_LINK_TEXT: &str = "link text";
/// Default URL inserted when the toolbar link action has no selection.
pub(crate) const DEFAULT_LINK_URL: &str = "https://example.com";
/// Default alt text inserted when the toolbar image action has no selection.
pub(crate) const DEFAULT_IMAGE_ALT_TEXT: &str = "alt text";
/// Default image URL inserted when the toolbar image action has no selection.
pub(crate) const DEFAULT_IMAGE_URL: &str =
    "https://vcg03.cfp.cn/creative/vcg/800/new/VCG41N1224074145.jpg";

/// Toolbar actions that insert or toggle standard Markdown syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkdownToolbarAction {
    Bold,
    Italic,
    Heading1,
    Heading2,
    Heading3,
    OrderedList,
    UnorderedList,
    Code,
    CodeBlock,
    Link,
    Quote,
    Table,
    Todo,
    HorizontalRule,
    Image,
    TableOfContents,
}

/// Applies a toolbar action to `text` at `selection`, returning the updated
/// document and a new UTF-8 byte selection range.
pub(crate) fn apply_markdown_toolbar_action(
    text: &str,
    selection: Range<usize>,
    action: MarkdownToolbarAction,
) -> (String, Range<usize>) {
    match action {
        MarkdownToolbarAction::Bold => toggle_inline_wrap(text, selection, "**", "**"),
        MarkdownToolbarAction::Italic => toggle_inline_wrap(text, selection, "*", "*"),
        MarkdownToolbarAction::Code => toggle_inline_wrap(text, selection, "`", "`"),
        MarkdownToolbarAction::CodeBlock => insert_code_fence(text, selection, "javascript"),
        MarkdownToolbarAction::Link => apply_link_format(text, selection),
        MarkdownToolbarAction::Heading1 => toggle_line_prefix(text, selection, "# "),
        MarkdownToolbarAction::Heading2 => toggle_line_prefix(text, selection, "## "),
        MarkdownToolbarAction::Heading3 => toggle_line_prefix(text, selection, "### "),
        MarkdownToolbarAction::OrderedList => toggle_line_prefix(text, selection, "1. "),
        MarkdownToolbarAction::UnorderedList => toggle_line_prefix(text, selection, "- "),
        MarkdownToolbarAction::Quote => toggle_line_prefix(text, selection, "> "),
        MarkdownToolbarAction::Table => insert_markdown_table(text, selection),
        MarkdownToolbarAction::Todo => insert_todo_template(text, selection),
        MarkdownToolbarAction::HorizontalRule => insert_horizontal_rule(text, selection),
        MarkdownToolbarAction::Image => apply_image_format(text, selection),
        MarkdownToolbarAction::TableOfContents => insert_table_of_contents(text, selection),
    }
}

fn clamp_range(text: &str, range: Range<usize>) -> Range<usize> {
    let len = text.len();
    range.start.min(len)..range.end.min(len)
}

fn toggle_inline_wrap(
    text: &str,
    selection: Range<usize>,
    prefix: &str,
    suffix: &str,
) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);

    if selection.is_empty() {
        let mut next = text.to_string();
        next.insert_str(selection.start, &format!("{prefix}{suffix}"));
        let cursor = selection.start + prefix.len();
        return (next, cursor..cursor);
    }

    let before_start = selection.start.saturating_sub(prefix.len());
    let after_end = (selection.end + suffix.len()).min(text.len());
    if text.get(before_start..selection.start) == Some(prefix)
        && text.get(selection.end..after_end) == Some(suffix)
    {
        let mut next = String::with_capacity(text.len() - prefix.len() - suffix.len());
        next.push_str(&text[..before_start]);
        next.push_str(&text[selection.clone()]);
        next.push_str(&text[after_end..]);
        let start = before_start;
        let end = start + selection.len();
        return (next, start..end);
    }

    let mut next = String::with_capacity(text.len() + prefix.len() + suffix.len());
    next.push_str(&text[..selection.start]);
    next.push_str(prefix);
    next.push_str(&text[selection.clone()]);
    next.push_str(suffix);
    next.push_str(&text[selection.end..]);
    let start = selection.start + prefix.len();
    let end = start + selection.len();
    (next, start..end)
}

/// Inserts or wraps a Markdown link at `selection`, returning the updated text
/// and a UTF-8 byte range selecting the URL portion.
pub(crate) fn apply_link_format(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let (link_text, url) = if selection.is_empty() {
        (
            DEFAULT_LINK_TEXT.to_string(),
            DEFAULT_LINK_URL.to_string(),
        )
    } else {
        let selected = text[selection.clone()].to_string();
        (selected.clone(), selected)
    };
    let replacement = format!("[{link_text}]({url})");

    let mut next = String::with_capacity(text.len() + replacement.len() - selection.len());
    next.push_str(&text[..selection.start]);
    next.push_str(&replacement);
    next.push_str(&text[selection.end..]);

    let url_start = selection.start + link_text.len() + 3;
    let url_end = url_start + url.len();
    (next, url_start..url_end)
}

fn line_range_for_selection(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = text[..selection.start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let end = text[selection.end..]
        .find('\n')
        .map(|index| selection.end + index)
        .unwrap_or(text.len());
    start..end
}

fn toggle_line_prefix(text: &str, selection: Range<usize>, prefix: &str) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let line_range = line_range_for_selection(text, &selection);
    let line = &text[line_range.clone()];

    let (updated_line, line_delta) = if prefix == "# "
        || prefix == "## "
        || prefix == "### "
    {
        toggle_heading_line_at_level(line, prefix)
    } else if prefix == "1. " {
        toggle_numbered_list_line(line)
    } else if prefix == "- " {
        toggle_unordered_list_line(line)
    } else if prefix == "> " {
        toggle_quote_line(line)
    } else {
        (format!("{prefix}{line}"), prefix.len() as isize)
    };

    let mut next = String::with_capacity(text.len() + updated_line.len().saturating_sub(line.len()));
    next.push_str(&text[..line_range.start]);
    next.push_str(&updated_line);
    next.push_str(&text[line_range.end..]);

    let selection_shift = if line_delta >= 0 {
        line_delta as usize
    } else {
        0
    };
    let selection_shrink = if line_delta < 0 {
        (-line_delta) as usize
    } else {
        0
    };

    let new_start = selection
        .start
        .saturating_add(selection_shift)
        .saturating_sub(if selection.start > line_range.start {
            selection_shrink
        } else {
            0
        });
    let new_end = selection
        .end
        .saturating_add(selection_shift)
        .saturating_sub(if selection.end > line_range.start {
            selection_shrink
        } else {
            0
        });
    (next, new_start.min(new_end)..new_start.max(new_end))
}

fn toggle_heading_line_at_level(line: &str, prefix: &str) -> (String, isize) {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    let leading_spaces = &line[..leading];
    let content = strip_atx_heading_content(trimmed);

    if trimmed.starts_with(prefix) {
        let new_line = format!("{leading_spaces}{content}");
        let delta = new_line.len() as isize - line.len() as isize;
        return (new_line, delta);
    }

    let new_line = format!("{leading_spaces}{prefix}{content}");
    let delta = new_line.len() as isize - line.len() as isize;
    (new_line, delta)
}

fn strip_atx_heading_content(line: &str) -> &str {
    let hash_count = line.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&hash_count) {
        return line[hash_count..].strip_prefix(' ').unwrap_or(&line[hash_count..]);
    }
    line
}

fn insert_code_fence(text: &str, selection: Range<usize>, language: &str) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let prefix = if selection.start == 0 {
        String::new()
    } else if text[..selection.start].ends_with("\n\n") {
        String::new()
    } else if text[..selection.start].ends_with('\n') {
        "\n".to_string()
    } else {
        "\n\n".to_string()
    };
    let suffix = if selection.end == text.len() || text[selection.end..].starts_with('\n') {
        "\n".to_string()
    } else {
        "\n\n".to_string()
    };
    let fence_body = format!("```{language}\n\n```");
    let insertion = format!("{prefix}{fence_body}{suffix}");
    let insert_at = selection.start;

    let mut next = String::with_capacity(text.len() + insertion.len());
    next.push_str(&text[..insert_at]);
    next.push_str(&insertion);
    next.push_str(&text[selection.end..]);

    let cursor = insert_at + prefix.len() + language.len() + 4;
    (next, cursor..cursor)
}

fn insert_markdown_table(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    use crate::components::markdown::table::{TableData, serialize_table_markdown_lines};

    let selection = clamp_range(text, selection);
    let table_text = serialize_table_markdown_lines(&TableData::new_empty(2, 2)).join("\n");
    let prefix = if selection.start == 0 {
        String::new()
    } else if text[..selection.start].ends_with("\n\n") {
        String::new()
    } else if text[..selection.start].ends_with('\n') {
        "\n".to_string()
    } else {
        "\n\n".to_string()
    };
    let suffix = if selection.end == text.len() || text[selection.end..].starts_with('\n') {
        "\n".to_string()
    } else {
        "\n\n".to_string()
    };
    let insertion = format!("{prefix}{table_text}{suffix}");
    let insert_at = selection.start;

    let mut next = String::with_capacity(text.len() + insertion.len());
    next.push_str(&text[..insert_at]);
    next.push_str(&insertion);
    next.push_str(&text[selection.end..]);

    let cursor = insert_at + prefix.len() + 2;
    (next, cursor..cursor)
}

fn toggle_numbered_list_line(line: &str) -> (String, isize) {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    let leading_spaces = &line[..leading];

    if let Some(rest) = strip_ordered_list_prefix(trimmed) {
        let removed = (trimmed.len() - rest.len()) as isize;
        return (
            format!("{leading_spaces}{rest}"),
            -removed,
        );
    }

    (
        format!("{leading_spaces}1. {trimmed}"),
        3,
    )
}

fn strip_ordered_list_prefix(value: &str) -> Option<&str> {
    let mut digits = 0usize;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
            if digits > 9 {
                return None;
            }
            continue;
        }
        if ch == '.' && digits > 0 {
            return value[digits + 1..].strip_prefix(' ');
        }
        return None;
    }
    None
}

fn toggle_unordered_list_line(line: &str) -> (String, isize) {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    let leading_spaces = &line[..leading];

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return (
            format!("{leading_spaces}{rest}"),
            -2,
        );
    }

    (
        format!("{leading_spaces}- {trimmed}"),
        2,
    )
}

fn toggle_quote_line(line: &str) -> (String, isize) {
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    let leading_spaces = &line[..leading];

    if let Some(rest) = trimmed.strip_prefix("> ") {
        return (
            format!("{leading_spaces}{rest}"),
            -2,
        );
    }

    if trimmed == ">" {
        return (leading_spaces.to_string(), -1);
    }

    (
        format!("{leading_spaces}> {trimmed}"),
        2,
    )
}

fn insert_todo_template(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let template = "- [ ] 待办事项 1\n- [ ] 待办事项 2";
    let prefix = block_insert_prefix(text, &selection);
    let suffix = block_insert_suffix(text, &selection);
    let insertion = format!("{prefix}{template}{suffix}");

    let mut next = String::with_capacity(text.len() + insertion.len());
    next.push_str(&text[..selection.start]);
    next.push_str(&insertion);
    next.push_str(&text[selection.end..]);

    let cursor = selection.start + prefix.len() + 6;
    (next, cursor..cursor)
}

fn insert_horizontal_rule(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let prefix = block_insert_prefix(text, &selection);
    let suffix = block_insert_suffix(text, &selection);
    let insertion = format!("{prefix}---{suffix}");

    let mut next = String::with_capacity(text.len() + insertion.len());
    next.push_str(&text[..selection.start]);
    next.push_str(&insertion);
    next.push_str(&text[selection.end..]);

    let cursor = selection.start + prefix.len() + 3;
    (next, cursor..cursor)
}

/// Inserts or wraps a Markdown image at `selection`, returning the updated text
/// and a UTF-8 byte range selecting the URL portion.
pub(crate) fn apply_image_format(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let (alt_text, url) = if selection.is_empty() {
        (
            DEFAULT_IMAGE_ALT_TEXT.to_string(),
            DEFAULT_IMAGE_URL.to_string(),
        )
    } else {
        let selected = text[selection.clone()].to_string();
        (selected.clone(), selected)
    };
    let replacement = format!("![{alt_text}]({url})");

    let mut next = String::with_capacity(text.len() + replacement.len() - selection.len());
    next.push_str(&text[..selection.start]);
    next.push_str(&replacement);
    next.push_str(&text[selection.end..]);

    let url_start = selection.start + alt_text.len() + 4;
    let url_end = url_start + url.len();
    (next, url_start..url_end)
}

/// Extracts the replacement snippet and post-edit selection from a formatted result.
pub(crate) fn toolbar_replacement_from_formatted_text(
    original: &str,
    selection: Range<usize>,
    formatted: (String, Range<usize>),
) -> (String, Range<usize>) {
    let (new_text, post_selection) = formatted;
    let replacement_end = new_text.len() - (original.len() - selection.end);
    let replacement = new_text[selection.start..replacement_end].to_string();
    (replacement, post_selection)
}

fn insert_table_of_contents(text: &str, selection: Range<usize>) -> (String, Range<usize>) {
    let selection = clamp_range(text, selection);
    let template = "## 目录\n\n- [章节 1](#)\n- [章节 2](#)";
    let prefix = block_insert_prefix(text, &selection);
    let suffix = block_insert_suffix(text, &selection);
    let insertion = format!("{prefix}{template}{suffix}");

    let mut next = String::with_capacity(text.len() + insertion.len());
    next.push_str(&text[..selection.start]);
    next.push_str(&insertion);
    next.push_str(&text[selection.end..]);

    let cursor = selection.start + prefix.len() + 6;
    (next, cursor..cursor)
}

fn block_insert_prefix(text: &str, selection: &Range<usize>) -> String {
    if selection.start == 0 {
        String::new()
    } else if text[..selection.start].ends_with("\n\n") {
        String::new()
    } else if text[..selection.start].ends_with('\n') {
        "\n".to_string()
    } else {
        "\n\n".to_string()
    }
}

fn block_insert_suffix(text: &str, selection: &Range<usize>) -> String {
    if selection.end == text.len() || text[selection.end..].starts_with('\n') {
        "\n".to_string()
    } else {
        "\n\n".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_wraps_selection() {
        let (text, range) =
            apply_markdown_toolbar_action("hello world", 6..11, MarkdownToolbarAction::Bold);
        assert_eq!(text, "hello **world**");
        assert_eq!(range, 8..13);
    }

    #[test]
    fn bold_unwraps_existing_markers() {
        let (text, range) =
            apply_markdown_toolbar_action("hello **world**", 8..13, MarkdownToolbarAction::Bold);
        assert_eq!(text, "hello world");
        assert_eq!(range, 6..11);
    }

    #[test]
    fn bold_inserts_empty_markers_at_cursor() {
        let (text, range) =
            apply_markdown_toolbar_action("hello", 5..5, MarkdownToolbarAction::Bold);
        assert_eq!(text, "hello****");
        assert_eq!(range, 7..7);
    }

    #[test]
    fn heading_prefixes_current_line() {
        let (text, range) =
            apply_markdown_toolbar_action("Title\nBody", 0..5, MarkdownToolbarAction::Heading1);
        assert_eq!(text, "# Title\nBody");
        assert_eq!(range, 2..7);
    }

    #[test]
    fn heading2_prefixes_current_line() {
        let (text, _) =
            apply_markdown_toolbar_action("Title", 0..5, MarkdownToolbarAction::Heading2);
        assert_eq!(text, "## Title");
    }

    #[test]
    fn heading_toggles_off_existing_prefix() {
        let (text, range) = apply_markdown_toolbar_action(
            "# Title\nBody",
            2..7,
            MarkdownToolbarAction::Heading1,
        );
        assert_eq!(text, "Title\nBody");
        assert_eq!(range, 0..5);
    }

    #[test]
    fn heading3_switches_existing_level() {
        let (text, _) = apply_markdown_toolbar_action(
            "## Title",
            0..8,
            MarkdownToolbarAction::Heading3,
        );
        assert_eq!(text, "### Title");
    }

    #[test]
    fn unordered_list_prefixes_line() {
        let (text, _) =
            apply_markdown_toolbar_action("item", 0..4, MarkdownToolbarAction::UnorderedList);
        assert_eq!(text, "- item");
    }

    #[test]
    fn link_wraps_selection_with_selected_text_as_url() {
        let (text, range) =
            apply_markdown_toolbar_action("click here", 0..10, MarkdownToolbarAction::Link);
        assert_eq!(text, "[click here](click here)");
        assert_eq!(range, 13..23);
    }

    #[test]
    fn link_without_selection_uses_placeholder_text_and_url() {
        let (text, range) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::Link);
        assert_eq!(text, "Hello[link text](https://example.com)");
        assert_eq!(range, 17..36);
    }

    #[test]
    fn table_inserts_pipe_table_template() {
        let (text, _) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::Table);
        assert!(text.contains("| --- | --- |"));
        assert!(text.starts_with("Hello\n\n|"));
    }

    #[test]
    fn quote_prefixes_line() {
        let (text, _) =
            apply_markdown_toolbar_action("quoted", 0..6, MarkdownToolbarAction::Quote);
        assert_eq!(text, "> quoted");
    }

    #[test]
    fn code_block_inserts_javascript_fence() {
        let (text, range) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::CodeBlock);
        assert_eq!(text, "Hello\n\n```javascript\n\n```\n");
        assert_eq!(range, 21..21);
    }

    #[test]
    fn todo_inserts_template() {
        let (text, _) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::Todo);
        assert!(text.starts_with("Hello\n\n- [ ]"));
        assert!(text.contains("待办事项 1"));
        assert!(text.contains("待办事项 2"));
    }

    #[test]
    fn horizontal_rule_inserts_dashes() {
        let (text, _) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::HorizontalRule);
        assert!(text.starts_with("Hello\n\n---"));
    }

    #[test]
    fn image_wraps_selection_with_alt_and_url() {
        let (text, range) =
            apply_markdown_toolbar_action("photo", 0..5, MarkdownToolbarAction::Image);
        assert_eq!(text, "![photo](photo)");
        assert_eq!(range, 9..14);
    }

    #[test]
    fn image_without_selection_uses_placeholder() {
        let (text, range) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::Image);
        assert_eq!(text, "Hello![alt text](https://vcg03.cfp.cn/creative/vcg/800/new/VCG41N1224074145.jpg)");
        assert_eq!(range, 17..79);
    }

    #[test]
    fn table_of_contents_inserts_template() {
        let (text, _) =
            apply_markdown_toolbar_action("Hello", 5..5, MarkdownToolbarAction::TableOfContents);
        assert!(text.contains("## 目录"));
        assert!(text.contains("- [章节 1](#)"));
        assert!(text.contains("- [章节 2](#)"));
    }

    #[test]
    fn toolbar_replacement_extracts_inserted_snippet() {
        let original = "click here";
        let selection = 0..10;
        let formatted = apply_link_format(original, selection.clone());
        let (replacement, url_range) =
            toolbar_replacement_from_formatted_text(original, selection, formatted);
        assert_eq!(replacement, "[click here](click here)");
        assert_eq!(url_range, 13..23);
    }
}
