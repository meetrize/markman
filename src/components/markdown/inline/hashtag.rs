//! Obsidian-style inline hashtags: `#tag` / `#project/alpha`.

use crate::components::markdown::html::HtmlInlineStyle;

use super::fragment::{InlineFragment, InlineTag};
use super::link_image::tokens_to_string;
use super::normalize::{CharToken, NormalizeBuilder};
use super::style::InlineStyle;

pub(crate) fn is_valid_tag_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/') || (!ch.is_ascii() && ch.is_alphanumeric())
}

pub(crate) fn is_hex_color_tag(body: &str) -> bool {
    (body.len() == 3 || body.len() == 6) && body.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub(crate) fn normalize_tag_name(name: &str) -> String {
    name.to_lowercase()
}

pub(crate) fn tag_from_body(body: &str) -> Option<InlineTag> {
    if body.is_empty() || body.starts_with('/') || body.ends_with('/') {
        return None;
    }
    if is_hex_color_tag(body) {
        return None;
    }
    Some(InlineTag {
        name: body.to_string(),
        source: format!("#{body}"),
    })
}

fn tag_body_len_in_str(text: &str, body_start: usize) -> usize {
    let mut end = body_start;
    for (offset, ch) in text[body_start..].char_indices() {
        if !is_valid_tag_char(ch) {
            break;
        }
        end = body_start + offset + ch.len_utf8();
    }
    end.saturating_sub(body_start)
}

pub(crate) fn locate_hashtag_in_str(line: &str, start: usize) -> Option<(usize, usize, InlineTag)> {
    let mut index = start;
    while index < line.len() {
        if line.as_bytes().get(index) != Some(&b'#') {
            index += 1;
            continue;
        }
        if index > 0 && line.as_bytes()[index - 1] == b'\\' {
            index += 1;
            continue;
        }

        let body_start = index + '#'.len_utf8();
        if body_start >= line.len() {
            return None;
        }
        if line[body_start..].starts_with(' ') {
            index += 1;
            continue;
        }

        let body_len = tag_body_len_in_str(line, body_start);
        if body_len == 0 {
            index += 1;
            continue;
        }

        let body = &line[body_start..body_start + body_len];
        if let Some(tag) = tag_from_body(body) {
            return Some((index, body_start + body_len, tag));
        }

        index += 1;
    }

    None
}

#[cfg(test)]
pub(crate) fn extract_tags_from_line(line: &str, line_start_byte: usize) -> Vec<(InlineTag, usize)> {
    let mut tags = Vec::new();
    let mut search = 0usize;
    while search < line.len() {
        let Some((start, end, tag)) = locate_hashtag_in_str(line, search) else {
            break;
        };
        tags.push((tag, line_start_byte + start));
        search = end;
    }
    tags
}

pub(crate) fn locate_hashtag(tokens: &[CharToken], index: usize) -> Option<(usize, usize)> {
    if tokens.get(index)?.ch != '#' {
        return None;
    }

    let body_start = index + 1;
    if body_start >= tokens.len() {
        return None;
    }
    if tokens[body_start].ch.is_whitespace() {
        return None;
    }

    let mut cursor = body_start;
    while cursor < tokens.len() && is_valid_tag_char(tokens[cursor].ch) {
        cursor += 1;
    }

    if cursor == body_start {
        return None;
    }

    let body = tokens_to_string(&tokens[body_start..cursor]);
    if tag_from_body(&body).is_none() {
        return None;
    }

    Some((body_start, cursor - 1))
}

pub(crate) fn parse_hashtag(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) -> Option<usize> {
    let (body_start, end_index) = locate_hashtag(tokens, index)?;
    let body = tokens_to_string(&tokens[body_start..=end_index]);
    let tag = tag_from_body(&body)?;
    let source = tag.source.clone();

    let fragment = InlineFragment {
        text: source.clone(),
        style: extra_style,
        html_style: extra_html_style,
        link: None,
        footnote: None,
        math: None,
        emoji: None,
        tag: Some(tag),
    };

    let normalized_start = builder.normalized_len;
    let visible_len = fragment.text.len();

    for token in &tokens[index..=end_index] {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] =
                normalized_start + (token.source_range.start + delta - tokens[index].source_range.start);
        }
    }

    builder.normalized_len += visible_len;
    if fragment.tag.is_some() {
        builder.fragments.push(fragment);
    } else if let Some(last) = builder.fragments.last_mut()
        && last.style == fragment.style
        && last.html_style == fragment.html_style
        && last.link.is_none()
        && last.footnote.is_none()
        && last.math.is_none()
        && last.emoji.is_none()
        && last.tag.is_none()
    {
        last.text.push_str(&fragment.text);
    } else {
        builder.fragments.push(fragment);
    }

    Some(end_index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::markdown::inline::InlineTextTree;

    #[test]
    fn parses_inline_hashtag_with_visible_hash_prefix() {
        let tree = InlineTextTree::from_markdown("See #rust here.");
        assert_eq!(tree.visible_text(), "See #rust here.");
        assert_eq!(tree.serialize_markdown(), "See #rust here.");
        assert!(tree
            .fragments
            .iter()
            .any(|fragment| fragment.tag.as_ref().is_some_and(|tag| tag.name == "rust")));
    }

    #[test]
    fn hex_color_literals_stay_plain_text() {
        let tree = InlineTextTree::from_markdown("color #fff and #1a2b3c ok");
        assert_eq!(tree.visible_text(), "color #fff and #1a2b3c ok");
        assert!(tree.fragments.iter().all(|fragment| fragment.tag.is_none()));
    }

    #[test]
    fn numeric_non_hex_tag_is_allowed() {
        let tree = InlineTextTree::from_markdown("year #2024");
        assert!(tree
            .fragments
            .iter()
            .any(|fragment| fragment.tag.as_ref().is_some_and(|tag| tag.name == "2024")));
    }

    #[test]
    fn hierarchical_tag_parses() {
        let tree = InlineTextTree::from_markdown("#project/alpha");
        assert!(tree
            .fragments
            .iter()
            .any(|fragment| fragment.tag.as_ref().is_some_and(|tag| tag.name == "project/alpha")));
    }

    #[test]
    fn invalid_tag_boundaries_stay_literal() {
        let tree = InlineTextTree::from_markdown("#/bad and #bad/");
        assert!(tree.fragments.iter().all(|fragment| fragment.tag.is_none()));
    }

    #[test]
    fn escaped_hashtag_stays_literal() {
        let tree = InlineTextTree::from_markdown(r"\#tag");
        assert!(tree.fragments.iter().all(|fragment| fragment.tag.is_none()));
    }

    #[test]
    fn code_span_hashtag_stays_literal() {
        let tree = InlineTextTree::from_markdown("use `#tag`");
        assert!(tree.fragments.iter().all(|fragment| fragment.tag.is_none()));
    }

    #[test]
    fn normalize_tag_name_is_lowercase() {
        assert_eq!(normalize_tag_name("Project/Alpha"), "project/alpha");
    }

    #[test]
    fn tag_name_preserves_input_case_in_fragment() {
        let tree = InlineTextTree::from_markdown("#Rust");
        let tag = tree
            .fragments
            .iter()
            .find_map(|fragment| fragment.tag.as_ref())
            .expect("tag fragment");
        assert_eq!(tag.name, "Rust");
        assert_eq!(normalize_tag_name(&tag.name), "rust");
        assert_eq!(tag.source, "#Rust");
    }

    #[test]
    fn chinese_tag_parses() {
        let tree = InlineTextTree::from_markdown("笔记 #工作");
        assert!(tree
            .fragments
            .iter()
            .any(|fragment| fragment.tag.as_ref().is_some_and(|tag| tag.name == "工作")));
    }

    #[test]
    fn multiple_tags_on_same_line_are_independent_fragments() {
        let tree = InlineTextTree::from_markdown("a #rust b #go c");
        let tags: Vec<_> = tree
            .fragments
            .iter()
            .filter_map(|fragment| fragment.tag.as_ref())
            .map(|tag| tag.name.as_str())
            .collect();
        assert_eq!(tags, vec!["rust", "go"]);
    }

    #[test]
    fn spaced_hash_prefix_stays_literal_in_inline() {
        let tree = InlineTextTree::from_markdown("# Title");
        assert!(tree.fragments.iter().all(|fragment| fragment.tag.is_none()));
        assert_eq!(tree.visible_text(), "# Title");
    }

    #[test]
    fn extract_tags_from_line_finds_multiple_tags() {
        let tags = extract_tags_from_line("a #rust b #go", 0);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].0.name, "rust");
        assert_eq!(tags[1].0.name, "go");
    }

    #[test]
    fn tag_span_preserves_adjacent_bold_round_trip() {
        let markdown = "See **bold** #rust";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn bold_tag_fragment_keeps_bold_style_for_rendering() {
        let tree = InlineTextTree::from_markdown("**#bold-tag**");
        let tag_fragment = tree
            .fragments
            .iter()
            .find(|fragment| fragment.tag.is_some())
            .expect("tag fragment");
        assert!(tag_fragment.style.bold);
        assert_eq!(tag_fragment.tag.as_ref().unwrap().name, "bold-tag");
    }

    #[test]
    fn render_cache_exposes_all_tag_spans_on_one_line() {
        let tree = InlineTextTree::from_markdown("topics #rust #go #java end");
        let tag_spans: Vec<_> = tree
            .render_cache()
            .spans()
            .iter()
            .filter(|span| span.tag.is_some())
            .map(|span| span.tag.as_ref().unwrap().name.clone())
            .collect();
        assert_eq!(tag_spans, vec!["rust", "go", "java"]);
    }

    #[test]
    fn three_digit_hex_tokens_are_not_tags() {
        let tree = InlineTextTree::from_markdown("#rust #123 #go");
        let tag_names: Vec<_> = tree
            .fragments
            .iter()
            .filter_map(|fragment| fragment.tag.as_ref())
            .map(|tag| tag.name.as_str())
            .collect();
        assert_eq!(tag_names, vec!["rust", "go"]);
    }

    #[test]
    fn render_cache_exposes_tag_span() {
        let tree = InlineTextTree::from_markdown("See #rust here");
        let cache = tree.render_cache();
        let tag_spans: Vec<_> = cache
            .spans()
            .iter()
            .filter(|span| span.tag.is_some())
            .collect();
        assert_eq!(tag_spans.len(), 1);
        assert_eq!(tag_spans[0].range, "See ".len().."See #rust".len());
        assert_eq!(tag_spans[0].tag.as_ref().unwrap().name, "rust");
    }
}
