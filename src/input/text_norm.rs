//! Shared text normalization helpers for line endings and single-line paste input.

/// Converts CRLF and lone CR line endings to LF.
pub(crate) fn normalize_line_endings_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Flattens pasted text to a single line by replacing line breaks with spaces.
pub(crate) fn flatten_paste_to_single_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::{flatten_paste_to_single_line, normalize_line_endings_lf};

    #[test]
    fn normalize_line_endings_lf_converts_crlf() {
        assert_eq!(normalize_line_endings_lf("a\r\nb"), "a\nb");
    }

    #[test]
    fn normalize_line_endings_lf_converts_cr() {
        assert_eq!(normalize_line_endings_lf("a\rb"), "a\nb");
    }

    #[test]
    fn normalize_line_endings_lf_preserves_lf() {
        assert_eq!(normalize_line_endings_lf("a\nb"), "a\nb");
    }

    #[test]
    fn flatten_paste_to_single_line_converts_crlf() {
        assert_eq!(flatten_paste_to_single_line("a\r\nb"), "a b");
    }

    #[test]
    fn flatten_paste_to_single_line_converts_lf_and_cr() {
        assert_eq!(flatten_paste_to_single_line("a\nb\rc"), "a b c");
    }

    #[test]
    fn flatten_paste_to_single_line_preserves_plain_text() {
        assert_eq!(flatten_paste_to_single_line("hello"), "hello");
    }
}
