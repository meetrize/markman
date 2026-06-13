//! CommonMark fenced code block parsing shared across import, render, and export.
//!
//! Opening/closing rules follow the Markdown importer in `editor/document.rs`:
//! up to three spaces of indent, exact closing run length, and info-string validation
//! via [`BlockKind::parse_code_fence_opening`].

use gpui::SharedString;

use crate::components::BlockKind;

/// Opening fence metadata: marker character, run length, and info string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceInfo {
    pub marker: char,
    pub run_len: usize,
    pub info: String,
}

impl FenceInfo {
    pub fn language(&self) -> Option<SharedString> {
        if self.info.is_empty() {
            None
        } else {
            Some(self.info.clone().into())
        }
    }
}

/// Strips up to three leading spaces so block markers may be mildly indented.
pub(crate) fn strip_fence_indent(line: &str) -> Option<&str> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    (indent <= 3).then_some(&line[indent..])
}

/// Parses an opening fence line using the document importer indent rules.
pub fn parse_opening_fence(line: &str) -> Option<FenceInfo> {
    let opening = BlockKind::parse_code_fence_opening(strip_fence_indent(line)?.trim_end())?;
    Some(FenceInfo {
        marker: opening.ch,
        run_len: opening.len,
        info: opening
            .language
            .map(|language| language.to_string())
            .unwrap_or_default(),
    })
}

/// Returns true when `line` closes `opener` with the same marker run length.
pub fn is_closing_fence(line: &str, opener: &FenceInfo) -> bool {
    let Some(trimmed) = strip_fence_indent(line).map(str::trim_end) else {
        return false;
    };
    if !trimmed.starts_with(opener.marker) {
        return false;
    }
    let run_len = trimmed.chars().take_while(|&c| c == opener.marker).count();
    if run_len != opener.run_len {
        return false;
    }
    trimmed[opener.marker.len_utf8() * run_len..].trim().is_empty()
}

/// Parses an opening fence when the marker must start the line (`trim_end` only).
///
/// Reference-definition scanning treats any leading whitespace as "not a fence line".
pub fn parse_opening_fence_unindented(line: &str) -> Option<FenceInfo> {
    let opening = BlockKind::parse_code_fence_opening(line.trim_end())?;
    Some(FenceInfo {
        marker: opening.ch,
        run_len: opening.len,
        info: opening
            .language
            .map(|language| language.to_string())
            .unwrap_or_default(),
    })
}

/// Closing check for [`parse_opening_fence_unindented`] lines (no indent strip).
pub fn is_closing_fence_unindented(line: &str, opener: &FenceInfo) -> bool {
    let trimmed = line.trim_end();
    if !trimmed.starts_with(opener.marker) {
        return false;
    }
    let run_len = trimmed.chars().take_while(|&c| c == opener.marker).count();
    run_len == opener.run_len && trimmed[opener.marker.len_utf8() * run_len..].trim().is_empty()
}

/// Marker and run length when only fence boundaries are needed.
pub fn opening_fence_marker(line: &str) -> Option<(char, usize)> {
    parse_opening_fence(line).map(|fence| (fence.marker, fence.run_len))
}

/// Closing check when only marker and run length are tracked (document rules).
pub fn is_closing_fence_marker(line: &str, marker: char, run_len: usize) -> bool {
    is_closing_fence(
        line,
        &FenceInfo {
            marker,
            run_len,
            info: String::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{is_closing_fence, parse_opening_fence, parse_opening_fence_unindented};

    #[test]
    fn closing_fence_must_match_exact_opening_run_length() {
        let opener = parse_opening_fence("````rust").expect("opening fence");

        assert!(is_closing_fence("````", &opener));
        assert!(is_closing_fence("  ````   ", &opener));
        assert!(!is_closing_fence("```", &opener));
        assert!(!is_closing_fence("`````", &opener));
    }

    #[test]
    fn fence_detection_rejects_indent_beyond_three_spaces() {
        assert!(parse_opening_fence("    ```rust").is_none());

        let opener = parse_opening_fence("```rust").expect("opening fence");
        assert!(!is_closing_fence("    ```", &opener));
    }

    #[test]
    fn unindented_opening_requires_fence_at_line_start() {
        assert!(parse_opening_fence_unindented("  ```rust").is_none());
        assert!(parse_opening_fence("  ```rust").is_some());
    }
}
