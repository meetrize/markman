//! Reference-style links and autolink helpers.

use std::collections::HashMap;
use std::str::FromStr;

use gpui::http_client::Uri;

use super::fence::{
    is_closing_fence_unindented, parse_opening_fence_unindented, FenceInfo as ScanFenceInfo,
};
use super::image::normalize_reference_label;

/// Active fenced code block while scanning for link reference definitions.
type ScanFence = ScanFenceInfo;

/// HTML block start that suppresses reference-definition scanning.
enum HtmlBlockStart {
    /// HTML comment beginning with `<!--`.
    Comment,
    /// HTML tag block whose closing behavior depends on the tag.
    Tag {
        name: String,
        self_closing: bool,
        closes_same_line: bool,
    },
}

/// Global reference definition for reference-style links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkReferenceDefinition {
    pub(crate) destination: String,
    #[allow(dead_code)]
    pub(crate) title: Option<String>,
}

pub(crate) type LinkReferenceDefinitions = HashMap<String, LinkReferenceDefinition>;

pub(crate) fn parse_link_reference_definitions(markdown: &str) -> LinkReferenceDefinitions {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let normalized_lines = lines
        .iter()
        .map(|line| strip_reference_scan_container_prefixes(line).to_string())
        .collect::<Vec<_>>();
    let normalized_refs = normalized_lines
        .iter()
        .map(|line| line.as_str())
        .collect::<Vec<_>>();
    let mut definitions = LinkReferenceDefinitions::new();
    let mut index = 0usize;
    let mut active_fence = None;
    let mut active_html_tag: Option<String> = None;
    let mut active_html_comment = false;

    while index < lines.len() {
        let line = normalized_refs[index];

        if let Some(ref fence) = active_fence {
            if is_reference_scan_closing_fence(line, fence) {
                active_fence = None;
            }
            index += 1;
            continue;
        }

        if active_html_comment {
            if line.contains("-->") || line.trim().is_empty() {
                active_html_comment = false;
            }
            index += 1;
            continue;
        }

        if let Some(tag_name) = active_html_tag.clone() {
            if line.trim().is_empty()
                || parse_reference_scan_html_close_tag_name(line).as_deref() == Some(&tag_name)
            {
                active_html_tag = None;
            }
            index += 1;
            continue;
        }

        if let Some(fence) = parse_reference_scan_opening_fence(line) {
            if !is_reference_scan_closing_fence(line, &fence) {
                active_fence = Some(fence);
            }
            index += 1;
            continue;
        }

        if let Some(html_start) = parse_reference_scan_html_block_start(line) {
            match html_start {
                HtmlBlockStart::Comment => {
                    if !line.contains("-->") {
                        active_html_comment = true;
                    }
                }
                HtmlBlockStart::Tag {
                    name,
                    self_closing,
                    closes_same_line,
                } => {
                    if !self_closing && !closes_same_line {
                        active_html_tag = Some(name);
                    }
                }
            }
            index += 1;
            continue;
        }

        let Some((label, definition, consumed)) =
            parse_link_reference_definition(&normalized_refs, index)
        else {
            index += 1;
            continue;
        };

        definitions.entry(label).or_insert(definition);
        index += consumed;
    }

    definitions
}

pub(crate) fn is_supported_autolink_target(target: &str) -> bool {
    if target
        .strip_prefix("mailto:")
        .is_some_and(|address| !address.is_empty() && address.contains('@'))
    {
        return true;
    }

    Uri::from_str(target)
        .ok()
        .and_then(|uri| uri.scheme_str().map(str::to_owned))
        .is_some_and(|scheme| matches!(scheme.as_str(), "http" | "https"))
}

/// Returns the path portion of an inline link destination, without `#fragment` or `?query`.
pub(crate) fn link_destination_path_part(destination: &str) -> &str {
    destination.split(['#', '?']).next().unwrap_or(destination)
}

/// One parsed segment from a Markdown-like line inside an HTML text node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HtmlTextLineSegment {
    /// Plain text between links.
    Text(String),
    /// Inline link label with resolved open-link metadata.
    Link {
        label: String,
        hit: crate::components::markdown::inline::InlineLinkHit,
    },
}

/// Parses inline Markdown links and plain text from one HTML text line.
pub(crate) fn parse_html_text_line_segments(line: &str) -> Vec<HtmlTextLineSegment> {
    use crate::components::markdown::inline::InlineLink;

    let mut segments = Vec::new();
    let mut text_start = 0usize;
    let mut cursor = 0usize;

    while cursor < line.len() {
        if line[cursor..].starts_with('[') && !line[cursor..].starts_with("![") {
            if let Some((label, destination, title, end)) = parse_inline_link_at(line, cursor) {
                if text_start < cursor {
                    segments.push(HtmlTextLineSegment::Text(
                        line[text_start..cursor].to_string(),
                    ));
                }
                let link = InlineLink::Inline {
                    destination,
                    title,
                };
                segments.push(HtmlTextLineSegment::Link {
                    label,
                    hit: link.hit(),
                });
                cursor = end;
                text_start = end;
                continue;
            }
        }

        cursor += line[cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    }

    if text_start < line.len() {
        segments.push(HtmlTextLineSegment::Text(line[text_start..].to_string()));
    }

    if segments.is_empty() {
        vec![HtmlTextLineSegment::Text(String::new())]
    } else {
        segments
    }
}

fn parse_inline_link_at(
    line: &str,
    start: usize,
) -> Option<(String, String, Option<String>, usize)> {
    if !line.get(start..)?.starts_with('[') || line[start..].starts_with("![") {
        return None;
    }

    let label_end = find_unescaped_char(line, start + 1, b']')?;
    let label = unescape_ascii_punctuation(&line[start + 1..label_end]);
    if line.as_bytes().get(label_end + 1) != Some(&b'(') {
        return None;
    }

    let close = find_unescaped_char(line, label_end + 2, b')')?;
    let (destination, title) = parse_link_target(&line[label_end + 2..close])?;
    Some((label, destination, title, close + 1))
}

/// True when `[label](destination)` should open a workspace file instead of a browser URL.
pub(crate) fn is_local_file_link_destination(destination: &str) -> bool {
    let destination = destination.trim();
    if destination.is_empty() || destination.starts_with('#') {
        return false;
    }
    if is_supported_autolink_target(destination) {
        return false;
    }
    if destination.starts_with("//") || destination.contains("://") {
        return false;
    }
    !link_destination_path_part(destination).is_empty()
}

fn parse_link_reference_definition(
    lines: &[&str],
    start: usize,
) -> Option<(String, LinkReferenceDefinition, usize)> {
    let line = lines.get(start)?;
    let trimmed_end = line.trim_end();
    let leading_spaces = trimmed_end.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &trimmed_end[leading_spaces..];
    if !rest.starts_with('[') {
        return None;
    }

    let label_end = find_unescaped_char(rest, 1, b']')?;
    if rest.as_bytes().get(label_end + 1) != Some(&b':') {
        return None;
    }

    let label = normalize_reference_label(&rest[1..label_end])?;
    let mut target = rest[label_end + 2..].trim_start().to_string();
    let mut consumed = 1usize;

    if let Some(next_line) = lines.get(start + 1)
        && is_reference_definition_title_continuation(next_line)
    {
        if !target.is_empty() {
            target.push(' ');
        }
        target.push_str(next_line.trim());
        consumed += 1;
    }

    let (destination, title) = parse_link_target(&target)?;
    Some((
        label,
        LinkReferenceDefinition { destination, title },
        consumed,
    ))
}

fn strip_reference_scan_container_prefixes(mut line: &str) -> &str {
    loop {
        let original = line;
        if let Some(rest) = strip_reference_scan_quote_prefix(line) {
            line = rest;
            continue;
        }
        if let Some(rest) = strip_reference_scan_list_marker(line) {
            line = rest;
            continue;
        }
        if line == original {
            return line;
        }
    }
}

fn strip_reference_scan_quote_prefix(line: &str) -> Option<&str> {
    let leading_spaces = line.bytes().take_while(|b| *b == b' ').count();
    if leading_spaces > 3 {
        return None;
    }

    let rest = &line[leading_spaces..];
    if !rest.starts_with('>') {
        return None;
    }

    Some(rest[1..].strip_prefix(' ').unwrap_or(&rest[1..]))
}

fn strip_reference_scan_list_marker(line: &str) -> Option<&str> {
    let indent_bytes = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(char::len_utf8)
        .sum::<usize>();
    let rest = &line[indent_bytes..];

    if let Some(marker) = rest.chars().next()
        && matches!(marker, '-' | '*' | '+')
    {
        let after_marker = &rest[marker.len_utf8()..];
        return after_marker
            .strip_prefix(' ')
            .or_else(|| after_marker.strip_prefix('\t'));
    }

    let digit_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if !(1..=9).contains(&digit_len) {
        return None;
    }

    let marker = *rest.as_bytes().get(digit_len)?;
    if !matches!(marker, b'.' | b')') {
        return None;
    }

    let separator = *rest.as_bytes().get(digit_len + 1)?;
    if !matches!(separator, b' ' | b'\t') {
        return None;
    }

    Some(&rest[digit_len + 2..])
}

fn parse_reference_scan_opening_fence(line: &str) -> Option<ScanFence> {
    parse_opening_fence_unindented(line)
}

fn is_reference_scan_closing_fence(line: &str, opener: &ScanFence) -> bool {
    is_closing_fence_unindented(line, opener)
}

fn parse_reference_scan_html_block_start(line: &str) -> Option<HtmlBlockStart> {
    let rest = line.trim_start().trim_end();
    if rest.starts_with("<!--") {
        return Some(HtmlBlockStart::Comment);
    }

    let tagged = rest.strip_prefix('<')?;
    if tagged.starts_with('/') {
        return None;
    }

    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    if name_len == 0 {
        return None;
    }

    let name = &tagged[..name_len];
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t' | '/') {
        return None;
    }

    Some(HtmlBlockStart::Tag {
        name: name.to_string(),
        self_closing: rest.ends_with("/>"),
        closes_same_line: rest.contains(&format!("</{name}>")),
    })
}

fn parse_reference_scan_html_close_tag_name(line: &str) -> Option<String> {
    let rest = line.trim_start().trim_end();
    let tagged = rest.strip_prefix("</")?;
    let name_len = tagged
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .count();
    if name_len == 0 {
        return None;
    }

    let name = &tagged[..name_len];
    let suffix = &tagged[name_len..];
    let next = suffix.chars().next()?;
    if !matches!(next, '>' | ' ' | '\t') {
        return None;
    }

    Some(name.to_string())
}

pub(crate) fn parse_link_target(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() {
        return None;
    }

    if inner.ends_with('"') {
        let close_quote = inner.len() - 1;
        if !is_escaped(inner, close_quote)
            && let Some(open_quote) = find_open_title_quote(inner, close_quote)
        {
            let destination = inner[..open_quote.saturating_sub(1)].trim_end();
            let title = inner[open_quote + 1..close_quote].to_string();
            if destination.is_empty() {
                return None;
            }
            return Some((normalize_link_destination(destination), Some(title)));
        }
    }

    Some((normalize_link_destination(inner), None))
}

fn normalize_link_destination(destination: &str) -> String {
    let destination = unescape_ascii_punctuation(destination);
    if destination.starts_with('<')
        && destination.ends_with('>')
        && is_supported_autolink_target(&destination[1..destination.len() - 1])
    {
        destination[1..destination.len() - 1].to_string()
    } else {
        destination
    }
}

fn unescape_ascii_punctuation(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|next| next.is_ascii_punctuation()) {
            output.push(chars.next().expect("peeked punctuation must exist"));
        } else {
            output.push(ch);
        }
    }
    output
}

fn is_reference_definition_title_continuation(line: &str) -> bool {
    let indent_bytes = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(char::len_utf8)
        .sum::<usize>();
    if indent_bytes == 0 {
        return false;
    }

    let trimmed = line[indent_bytes..].trim();
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
}

fn find_open_title_quote(input: &str, close_quote: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    (0..close_quote).rev().find(|&index| {
        bytes[index] == b'"'
            && !is_escaped(input, index)
            && index > 0
            && bytes[index - 1].is_ascii_whitespace()
    })
}

fn find_unescaped_char(input: &str, start: usize, target: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    (start..bytes.len()).find(|&index| bytes[index] == target && !is_escaped(input, index))
}

fn is_escaped(input: &str, index: usize) -> bool {
    if index == 0 {
        return false;
    }

    let bytes = input.as_bytes();
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if bytes[cursor] == b'\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::{
        LinkReferenceDefinition, is_supported_autolink_target, parse_link_reference_definitions,
    };

    #[test]
    fn parses_link_reference_definitions_with_title_and_first_wins() {
        let definitions = parse_link_reference_definitions(
            "[Ref Link]: https://first.example \"Caption\"\n[ref link]: https://second.example",
        );
        assert_eq!(
            definitions.get("ref link"),
            Some(&LinkReferenceDefinition {
                destination: "https://first.example".to_string(),
                title: Some("Caption".to_string()),
            })
        );
    }

    #[test]
    fn parses_container_scoped_link_reference_definitions_and_skips_raw_blocks() {
        let definitions = parse_link_reference_definitions(
            [
                "> [quoted ref]: https://quoted.example \"Quoted\"",
                "- [list ref]: https://list.example",
                "1) [ordered ref]: https://ordered.example",
                "> ```md",
                "> [code ref]: https://ignored-code.example",
                "> ```",
                "",
                "<div>",
                "[html ref]: https://ignored-html.example",
                "</div>",
            ]
            .join("\n")
            .as_str(),
        );

        assert_eq!(
            definitions.get("quoted ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://quoted.example".to_string(),
                title: Some("Quoted".to_string()),
            })
        );
        assert_eq!(
            definitions.get("list ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://list.example".to_string(),
                title: None,
            })
        );
        assert_eq!(
            definitions.get("ordered ref"),
            Some(&LinkReferenceDefinition {
                destination: "https://ordered.example".to_string(),
                title: None,
            })
        );
        assert!(!definitions.contains_key("code ref"));
        assert!(!definitions.contains_key("html ref"));
    }

    #[test]
    fn supports_http_https_and_mailto_autolinks() {
        assert!(is_supported_autolink_target("https://example.com"));
        assert!(is_supported_autolink_target("http://example.com"));
        assert!(is_supported_autolink_target("mailto:test@example.com"));
        assert!(!is_supported_autolink_target("./relative/path"));
        assert!(!is_supported_autolink_target("span>x</span"));
    }

    #[test]
    fn detects_local_markdown_link_destinations() {
        use super::is_local_file_link_destination;

        assert!(is_local_file_link_destination("ai-chat-implementation.zh-CN.md"));
        assert!(is_local_file_link_destination("./development.zh-CN.md"));
        assert!(is_local_file_link_destination("../README.md"));
        assert!(is_local_file_link_destination("other.md#section"));
        assert!(!is_local_file_link_destination("https://example.com"));
        assert!(!is_local_file_link_destination("mailto:test@example.com"));
        assert!(!is_local_file_link_destination("#anchor"));
    }

    #[test]
    fn parses_html_text_line_inline_links_and_text() {
        use super::{HtmlTextLineSegment, parse_html_text_line_segments};
        use crate::components::markdown::inline::InlineLinkHit;

        let segments = parse_html_text_line_segments(
            "[English](../README.md) | [中文](README.zh-CN.md)",
        );
        assert_eq!(segments.len(), 3);
        assert_eq!(
            segments[0],
            HtmlTextLineSegment::Link {
                label: "English".to_string(),
                hit: InlineLinkHit {
                    prompt_target: "../README.md".to_string(),
                    open_target: "../README.md".to_string(),
                    is_workspace_file: false,
                    is_document_relative_file: true,
                },
            }
        );
        assert_eq!(segments[1], HtmlTextLineSegment::Text(" | ".to_string()));
        assert_eq!(
            segments[2],
            HtmlTextLineSegment::Link {
                label: "中文".to_string(),
                hit: InlineLinkHit {
                    prompt_target: "README.zh-CN.md".to_string(),
                    open_target: "README.zh-CN.md".to_string(),
                    is_workspace_file: false,
                    is_document_relative_file: true,
                },
            }
        );
    }
}
