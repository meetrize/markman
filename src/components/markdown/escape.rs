//! HTML text and attribute escaping shared by Markdown render and export paths.

/// Escapes characters that are special in HTML text nodes.
pub fn escape_html_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Escapes characters that are special inside double-quoted HTML attributes.
pub fn escape_html_attr(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::{escape_html_attr, escape_html_text};

    #[test]
    fn escape_html_text_handles_special_characters() {
        assert_eq!(
            escape_html_text(r#"&<>"'"#),
            "&amp;&lt;&gt;&quot;&#39;"
        );
    }

    #[test]
    fn escape_html_text_preserves_plain_text() {
        assert_eq!(escape_html_text("hello world"), "hello world");
    }

    #[test]
    fn escape_html_text_empty_string() {
        assert_eq!(escape_html_text(""), "");
    }

    #[test]
    fn escape_html_attr_handles_quotes_and_ampersands() {
        assert_eq!(
            escape_html_attr(r#"say "hi" & bye"#),
            "say &quot;hi&quot; &amp; bye"
        );
    }

    #[test]
    fn escape_html_attr_escapes_angle_brackets_in_attribute_values() {
        assert_eq!(escape_html_attr("<tag>"), "&lt;tag>");
    }

    #[test]
    fn escape_html_attr_empty_string() {
        assert_eq!(escape_html_attr(""), "");
    }
}
