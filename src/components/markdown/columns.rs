//! Velotype `::: columns` block boundary detection and column content parsing.
//!
//! Region boundaries follow the Markdown importer in `editor/document.rs`, including
//! fenced-code awareness via the shared fence module.

use super::fence::{is_closing_fence, is_closing_fence_marker, opening_fence_marker, parse_opening_fence, FenceInfo};

/// One column inside a `::: columns` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnBlock {
    /// Raw `width=` attribute value when present and safe to emit.
    pub width: Option<String>,
    pub markdown: String,
}

pub fn is_columns_block_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let Some(rest) = trimmed.strip_prefix("::: columns") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}

pub fn is_columns_block_end(line: &str) -> bool {
    let trimmed = line.trim_start();
    line.len() - trimmed.len() <= 3 && trimmed.trim_end() == ":::"
}

pub fn collect_columns_block_region(lines: &[impl AsRef<str>], start: usize) -> Option<usize> {
    if !is_columns_block_start(lines.get(start)?.as_ref()) {
        return None;
    }

    let mut index = start + 1;
    let mut active_fence: Option<FenceInfo> = None;
    while index < lines.len() {
        let line = lines[index].as_ref();
        if let Some(fence) = active_fence.as_ref() {
            if is_closing_fence(line, fence) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = parse_opening_fence(line) {
            active_fence = Some(fence);
            index += 1;
            continue;
        }

        if is_columns_block_end(line) {
            return Some(index + 1);
        }
        index += 1;
    }

    None
}

/// Parses column markers and markdown bodies from the interior of a columns block.
pub fn parse_columns_content(lines: &[impl AsRef<str>]) -> Vec<ColumnBlock> {
    let mut columns = Vec::new();
    let mut current_width = None;
    let mut current_lines = Vec::new();
    let mut seen_column = false;
    let mut active_fence: Option<(char, usize)> = None;

    for line in lines {
        let line = line.as_ref();
        if let Some((marker, run_len)) = active_fence {
            current_lines.push(line.to_string());
            if is_closing_fence_marker(line, marker, run_len) {
                active_fence = None;
            }
            continue;
        }

        if let Some(fence) = opening_fence_marker(line) {
            current_lines.push(line.to_string());
            active_fence = Some(fence);
            continue;
        }

        if let Some(width) = parse_column_marker(line) {
            if seen_column {
                columns.push(ColumnBlock {
                    width: current_width.take(),
                    markdown: trim_blank_edges(&current_lines).join("\n"),
                });
                current_lines.clear();
            }
            current_width = width;
            seen_column = true;
            continue;
        }

        if seen_column {
            current_lines.push(line.to_string());
        } else if !line.trim().is_empty() {
            return Vec::new();
        }
    }

    if seen_column {
        columns.push(ColumnBlock {
            width: current_width,
            markdown: trim_blank_edges(&current_lines).join("\n"),
        });
    }

    columns
}

fn trim_blank_edges(lines: &[String]) -> Vec<String> {
    let mut start = 0usize;
    let mut end = lines.len();
    while start < end && lines[start].trim().is_empty() {
        start += 1;
    }
    while end > start && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    lines[start..end].to_vec()
}

fn parse_column_marker(line: &str) -> Option<Option<String>> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let rest = trimmed.strip_prefix("--- column")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    let mut width = None;
    for part in rest.split_whitespace() {
        if let Some(value) = part.strip_prefix("width=")
            && is_safe_column_width(value)
        {
            width = Some(value.to_string());
        }
    }
    Some(width)
}

fn is_safe_column_width(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '%' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::{
        collect_columns_block_region, is_columns_block_end, is_columns_block_start,
        parse_columns_content,
    };

    #[test]
    fn columns_block_start_and_end_detection() {
        assert!(is_columns_block_start("::: columns"));
        assert!(is_columns_block_start("  ::: columns"));
        assert!(!is_columns_block_start("    ::: columns"));
        assert!(!is_columns_block_start("::: column"));
        assert!(is_columns_block_end(":::"));
        assert!(is_columns_block_end("  :::"));
        assert!(!is_columns_block_end("::::"));
    }

    #[test]
    fn collect_columns_region_skips_closing_marker_inside_fenced_code() {
        let lines = [
            "::: columns",
            "--- column",
            "```",
            "::: columns",
            "```",
            ":::",
        ];
        assert_eq!(collect_columns_block_region(&lines, 0), Some(6));
    }

    #[test]
    fn parse_columns_content_extracts_widths_and_markdown() {
        let inner = [
            "--- column width=40%",
            "### Left",
            "",
            "- A",
            "--- column width=60%",
            "Right text",
        ];
        let columns = parse_columns_content(&inner);
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].width.as_deref(), Some("40%"));
        assert!(columns[0].markdown.contains("### Left"));
        assert_eq!(columns[1].markdown, "Right text");
    }

    #[test]
    fn parse_columns_content_rejects_leading_non_blank_before_first_column() {
        assert!(parse_columns_content(&["intro", "--- column", "Body"]).is_empty());
    }

    #[test]
    fn unclosed_columns_block_has_no_region_end() {
        let lines = ["::: columns", "--- column", "Left"];
        assert!(collect_columns_block_region(&lines, 0).is_none());
    }
}
