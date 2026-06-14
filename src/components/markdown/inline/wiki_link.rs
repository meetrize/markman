//! Obsidian-style wiki links: `[[path/to/file]]`.

use crate::components::markdown::html::HtmlInlineStyle;

use super::fragment::{InlineFragment, InlineLink};
use super::link_image::tokens_to_string;
use super::normalize::{CharToken, NormalizeBuilder};
use super::style::InlineStyle;

/// Finds the next `[[...]]` in a line starting at `search_from`.
///
/// Returns `(start_byte, end_byte, path)` where `start_byte`/`end_byte` are UTF-8
/// offsets into `line` spanning the full `[[path]]` source range.
pub(crate) fn locate_wiki_link_in_str(
    line: &str,
    search_from: usize,
) -> Option<(usize, usize, String)> {
    if search_from > line.len() || !line.is_char_boundary(search_from) {
        return None;
    }

    let mut index = search_from;
    while index < line.len() {
        if line[index..].starts_with("[[") {
            let path_start = index + 2;
            let mut cursor = path_start;
            while cursor < line.len() {
                if line[cursor..].starts_with("]]") {
                    let path = line[path_start..cursor].trim().to_string();
                    if path.is_empty() {
                        index += 2;
                        break;
                    }
                    return Some((index, cursor + 2, path));
                }
                cursor += line[cursor..].chars().next()?.len_utf8();
            }

            if cursor >= line.len() {
                return None;
            }
            index += 2;
        } else {
            index += line[index..].chars().next().map_or(1, |c| c.len_utf8());
        }
    }

    None
}

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

    #[test]
    fn locate_wiki_link_in_str_handles_cjk_prefix() {
        use super::locate_wiki_link_in_str;

        let line = "用户参考 [[note.md]] 结束";
        assert_eq!(
            locate_wiki_link_in_str(line, 0),
            Some((13, 24, "note.md".to_string()))
        );
        assert!(locate_wiki_link_in_str(line, 1).is_none());
    }

    #[test]
    fn locate_wiki_link_in_str_finds_multiple_links_on_one_line() {
        use super::locate_wiki_link_in_str;

        let line = "see [[a.md]] and [[b/c.md]]";
        let first = locate_wiki_link_in_str(line, 0).expect("first link");
        assert_eq!(first, (4, 12, "a.md".to_string()));

        let second = locate_wiki_link_in_str(line, first.1).expect("second link");
        assert_eq!(second, (17, 27, "b/c.md".to_string()));
        assert!(locate_wiki_link_in_str(line, second.1).is_none());
    }

    #[test]
    fn locate_wiki_link_in_str_skips_empty_brackets() {
        use super::locate_wiki_link_in_str;

        assert!(locate_wiki_link_in_str("broken [[ ]] link", 0).is_none());
        assert_eq!(
            locate_wiki_link_in_str("ok [[note.md]]", 0),
            Some((3, 14, "note.md".to_string()))
        );
    }
}
