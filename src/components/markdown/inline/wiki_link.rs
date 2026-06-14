//! Obsidian-style wiki links: `[[path/to/file]]`.

use crate::components::markdown::html::HtmlInlineStyle;

use super::fragment::{InlineFragment, InlineLink};
use super::link_image::tokens_to_string;
use super::normalize::{CharToken, NormalizeBuilder};
use super::style::InlineStyle;

/// Returns `(path_token_start, closing_bracket_end_index)` when `[[...]]` is well-formed.
pub(crate) fn locate_wiki_link(tokens: &[CharToken], index: usize) -> Option<(usize, usize)> {
    if tokens.get(index)?.ch != '[' || tokens.get(index + 1)?.ch != '[' {
        return None;
    }

    let mut cursor = index + 2;
    while cursor < tokens.len() {
        if tokens[cursor].ch == ']' && tokens.get(cursor + 1).is_some_and(|token| token.ch == ']')
        {
            return Some((index + 2, cursor + 1));
        }
        cursor += 1;
    }

    None
}

pub(crate) fn parse_wiki_link(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) -> Option<usize> {
    let (path_start, end_index) = locate_wiki_link(tokens, index)?;
    let path = tokens_to_string(&tokens[path_start..end_index - 1]).trim().to_string();
    if path.is_empty() {
        return None;
    }

    let link = InlineLink::WikiLink {
        path: path.clone(),
    };
    let fragment = InlineFragment {
        text: path,
        style: extra_style,
        html_style: extra_html_style,
        link: Some(link),
        footnote: None,
        math: None,
        emoji: None,
        tag: None,
    };

    let normalized_start = builder.normalized_len;
    let path_len = fragment.text.len();

    for boundary in tokens[index].source_range.start..=tokens[index + 1].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_start;
    }

    let mut local_boundary = 0usize;
    for token in &tokens[path_start..end_index - 1] {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] =
                normalized_start + local_boundary + delta;
        }
        local_boundary += token_len;
    }

    let normalized_end = normalized_start + path_len;
    for token in &tokens[end_index - 1..=end_index] {
        for boundary in token.source_range.start..=token.source_range.end {
            builder.visible_to_normalized[boundary] = normalized_end;
        }
    }

    builder.normalized_len += path_len;
    if let Some(last) = builder.fragments.last_mut()
        && last.style == fragment.style
        && last.html_style == fragment.html_style
        && last.link == fragment.link
        && last.footnote.is_none()
        && last.math.is_none()
        && fragment.emoji.is_none()
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
    use crate::components::markdown::inline::{InlineLinkHit, InlineTextTree};

    #[test]
    fn parses_wiki_link_with_workspace_relative_path() {
        let tree = InlineTextTree::from_markdown("See [[docs/README.zh-CN.md]] for details.");
        assert_eq!(tree.visible_text(), "See docs/README.zh-CN.md for details.");
        assert_eq!(
            tree.render_cache().link_hit_at("See ".len()),
            Some(&InlineLinkHit {
                prompt_target: "docs/README.zh-CN.md".to_string(),
                open_target: "docs/README.zh-CN.md".to_string(),
                is_workspace_file: true,
            })
        );
        assert_eq!(
            tree.serialize_markdown(),
            "See [[docs/README.zh-CN.md]] for details."
        );
    }

    #[test]
    fn empty_wiki_link_stays_literal() {
        let markdown = "broken [[ ]] link";
        let tree = InlineTextTree::from_markdown(markdown);
        assert_eq!(tree.visible_text(), markdown);
        assert!(tree
            .render_cache()
            .spans()
            .iter()
            .all(|span| span.link.is_none()));
    }
}
