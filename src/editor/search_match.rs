//! Case-insensitive search helpers that preserve UTF-8 char boundaries.

use std::ops::Range;

fn chars_case_equal(a: char, b: char) -> bool {
    a.to_lowercase().eq(b.to_lowercase())
}

/// All non-overlapping case-insensitive matches as byte ranges in `haystack`.
pub(crate) fn find_case_insensitive_ranges(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }

    let needle_chars: Vec<char> = needle.chars().collect();
    let n = needle_chars.len();
    let indices: Vec<(usize, char)> = haystack.char_indices().collect();
    if indices.len() < n {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for i in 0..=indices.len() - n {
        if (0..n).all(|j| chars_case_equal(indices[i + j].1, needle_chars[j])) {
            let start = indices[i].0;
            let (last_byte, last_char) = indices[i + n - 1];
            let end = last_byte + last_char.len_utf8();
            matches.push(start..end);
        }
    }
    matches
}

pub(crate) fn find_case_insensitive_start(haystack: &str, needle: &str) -> usize {
    find_case_insensitive_ranges(haystack, needle)
        .into_iter()
        .next()
        .map(|range| range.start)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{find_case_insensitive_ranges, find_case_insensitive_start};

    #[test]
    fn finds_ascii_case_insensitive_match() {
        let ranges = find_case_insensitive_ranges("Hello World", "world");
        assert_eq!(ranges, vec![6..11]);
    }

    #[test]
    fn finds_cjk_match_without_panic() {
        let haystack = "河南南阳";
        let ranges = find_case_insensitive_ranges(haystack, "南");
        assert_eq!(ranges.len(), 2);
        assert!(haystack.is_char_boundary(ranges[0].start));
        assert!(haystack.is_char_boundary(ranges[0].end));
        assert!(haystack.is_char_boundary(ranges[1].start));
        assert!(haystack.is_char_boundary(ranges[1].end));
    }

    #[test]
    fn keyword_start_skips_partial_utf8_bytes() {
        assert_eq!(find_case_insensitive_start("河南", "南"), 3);
    }
}
