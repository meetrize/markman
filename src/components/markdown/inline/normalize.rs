//! Normalization.

use std::ops::Range;

use crate::components::markdown::footnote::{
    InlineFootnoteReference, parse_inline_footnote_reference,
};
use crate::components::markdown::html::HtmlInlineStyle;
use crate::components::markdown::link::LinkReferenceDefinitions;

use super::delimiter::Delimiter;
use super::emoji::parse_emoji_shortcode;
use super::fragment::InlineFragment;
use super::fragment::InlineMath;
use super::html::{merge_html_styles, parse_inline_html_container};
use super::link_image::{parse_autolink, parse_inline_link, tokens_to_string};
use super::math::parse_inline_math;
use super::style::InlineStyle;
use super::wiki_link::{locate_wiki_link, parse_wiki_link};
use super::hashtag::parse_hashtag;

pub(crate) struct CharToken {
    pub(crate) ch: char,
    pub(crate) style: InlineStyle,
    pub(crate) html_style: Option<HtmlInlineStyle>,
    pub(crate) source_range: Range<usize>,
}

/// Result of parsing a delimited inline region.
pub(crate) struct ParseResult {
    next_index: usize,
    closed: bool,
}

/// Builds the output fragments during normalization (marker parsing).
/// Keeps track of the visible-to-normalized offset mapping so that
/// selections and cursors can be mapped to the normalized tree.
pub(crate) struct NormalizeBuilder {
    pub(crate) fragments: Vec<InlineFragment>,
    pub(crate) visible_to_normalized: Vec<usize>,
    pub(crate) normalized_len: usize,
}

impl NormalizeBuilder {
    pub(crate) fn new(input_len: usize) -> Self {
        Self {
            fragments: Vec::new(),
            visible_to_normalized: vec![0; input_len + 1],
            normalized_len: 0,
        }
    }

    pub(crate) fn drop_token(&mut self, token: &CharToken) {
        for boundary in token.source_range.start..=token.source_range.end {
            self.visible_to_normalized[boundary] = self.normalized_len;
        }
    }

    pub(crate) fn emit_token(
        &mut self,
        token: &CharToken,
        extra_style: InlineStyle,
        html_style: Option<HtmlInlineStyle>,
    ) {
        let mut style = token.style;
        if extra_style.bold {
            style.bold = true;
        }
        if extra_style.italic {
            style.italic = true;
        }
        if extra_style.underline {
            style.underline = true;
        }
        if extra_style.strikethrough {
            style.strikethrough = true;
        }
        if extra_style.code {
            style.code = true;
        }
        if extra_style.highlight {
            style.highlight = true;
        }
        if extra_style.has_script() {
            style.script = extra_style.script;
        }
        let html_style = merge_html_styles(html_style, token.html_style);

        let text = token.ch.to_string();
        let start = self.normalized_len;
        for boundary in token.source_range.start..=token.source_range.end {
            self.visible_to_normalized[boundary] = start + (boundary - token.source_range.start);
        }
        self.normalized_len += text.len();

        if let Some(last) = self.fragments.last_mut()
            && last.style == style
            && last.html_style == html_style
            && last.link.is_none()
            && last.footnote.is_none()
            && last.math.is_none()
            && last.emoji.is_none()
            && last.tag.is_none()
        {
            last.text.push_str(&text);
            return;
        }

        self.fragments.push(InlineFragment {
            text,
            style,
            html_style,
            link: None,
            footnote: None,
            math: None,
            emoji: None,
            tag: None,
        });
    }

    pub(crate) fn emit_inline_math(
        &mut self,
        tokens: &[CharToken],
        math: InlineMath,
        extra_style: InlineStyle,
        extra_html_style: Option<HtmlInlineStyle>,
    ) {
        let source_start = tokens
            .first()
            .map(|token| token.source_range.start)
            .unwrap_or(0);
        let normalized_start = self.normalized_len;
        let source = math.source.clone();
        let visible_len = source.len();

        for token in tokens {
            let token_len = token.source_range.len();
            for delta in 0..=token_len {
                self.visible_to_normalized[token.source_range.start + delta] =
                    normalized_start + (token.source_range.start + delta - source_start);
            }
        }

        self.normalized_len += visible_len;
        self.fragments.push(InlineFragment {
            text: source,
            style: extra_style,
            html_style: extra_html_style,
            link: None,
            footnote: None,
            math: Some(math),
            emoji: None,
            tag: None,
        });
    }
}

pub(crate) fn flatten_tokens(fragments: &[InlineFragment]) -> Vec<CharToken> {
    let mut tokens = Vec::new();
    let mut visible_offset = 0;

    for fragment in fragments {
        for ch in fragment.text.chars() {
            let len = ch.len_utf8();
            tokens.push(CharToken {
                ch,
                style: fragment.style,
                html_style: fragment.html_style,
                source_range: visible_offset..visible_offset + len,
            });
            visible_offset += len;
        }
    }

    tokens
}

/// Recursive-descent parser that consumes [`CharToken`]s and reconstructs
/// the normalized inline tree.  Matching delimiters are consumed (dropped);
/// unmatched ones are emitted as literal text.  Nested styles are handled by
/// recursive calls that accumulate `extra_style`.
pub(crate) fn parse_until(
    tokens: &[CharToken],
    mut index: usize,
    end_delimiter: Option<Delimiter>,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
    inside_code: bool,
    reference_definitions: &LinkReferenceDefinitions,
) -> ParseResult {
    let body_start = index;
    while index < tokens.len() {
        // Check for closing delimiter.
        if let Some(ref end_delim) = end_delimiter {
            let mut closed = match end_delim {
                Delimiter::CodeMarkdown { run_len } => {
                    tokens[index].ch == '`' && backtick_run_len(tokens, index) == *run_len
                }
                Delimiter::SuperscriptMarkdown => {
                    tokens[index].ch == '^' && can_close_emphasis(tokens, index)
                }
                Delimiter::SubscriptMarkdown => {
                    is_single_tilde_delimiter(tokens, index) && can_close_emphasis(tokens, index)
                }
                _ => {
                    matches_sequence(tokens, index, &end_delim.close())
                        && can_close_emphasis(tokens, index)
                }
            };

            // Emphasis spans must enclose at least one character; reject a
            // close at the very start of the body so empty spans stay literal.
            if closed && index == body_start && emphasis_requires_body(*end_delim) {
                closed = false;
            }

            if closed {
                let close_len = end_delim.close().chars().count();
                for token in &tokens[index..index + close_len] {
                    builder.drop_token(token);
                }
                return ParseResult {
                    next_index: index + close_len,
                    closed: true,
                };
            }
        }

        if !inside_code
            && let Some(next_index) =
                parse_inline_math(tokens, index, extra_style, extra_html_style, builder)
        {
            index = next_index;
            continue;
        }

        if !inside_code
            && tokens[index].ch == '\\'
            && let Some(escaped_len) = escaped_sequence_token_len(tokens, index)
        {
            builder.drop_token(&tokens[index]);
            let escaped_start = index + 1;
            let escaped_end = escaped_start + escaped_len;
            for token in &tokens[escaped_start..escaped_end] {
                builder.emit_token(token, extra_style, extra_html_style);
            }
            index = escaped_end;
            continue;
        }

        // Inside a code span, all text (including markers) is literal.
        if !inside_code {
            if tokens[index].ch == '['
                && tokens.get(index + 1).is_some_and(|token| token.ch == '[')
            {
                if let Some(next_index) =
                    parse_wiki_link(tokens, index, extra_style, extra_html_style, builder)
                {
                    index = next_index;
                    continue;
                }

                if let Some((_path_start, end_index)) = locate_wiki_link(tokens, index) {
                    for token in &tokens[index..=end_index] {
                        builder.emit_token(token, extra_style, extra_html_style);
                    }
                    index = end_index + 1;
                    continue;
                }
            }

            if tokens[index].ch == '['
                && let Some(next_index) =
                    parse_footnote_reference(tokens, index, extra_style, extra_html_style, builder)
            {
                index = next_index;
                continue;
            }

            if let Some(next_index) = parse_inline_link(
                tokens,
                index,
                extra_style,
                extra_html_style,
                builder,
                reference_definitions,
            ) {
                index = next_index;
                continue;
            }

            if tokens[index].ch == '<'
                && let Some(next_index) = parse_inline_html_container(
                    tokens,
                    index,
                    extra_style,
                    extra_html_style,
                    builder,
                    reference_definitions,
                )
            {
                index = next_index;
                continue;
            }

            if tokens[index].ch == '<'
                && let Some(next_index) = parse_autolink(
                    tokens,
                    index,
                    extra_style,
                    extra_html_style,
                    builder,
                    reference_definitions,
                )
            {
                index = next_index;
                continue;
            }

            if tokens[index].ch == ':'
                && let Some(next_index) = parse_emoji_shortcode(
                    tokens,
                    index,
                    extra_style,
                    extra_html_style,
                    builder,
                )
            {
                index = next_index;
                continue;
            }

            if tokens[index].ch == '#'
                && let Some(next_index) =
                    parse_hashtag(tokens, index, extra_style, extra_html_style, builder)
            {
                index = next_index;
                continue;
            }

            if let Some(delimiter) = match_open_delimiter(tokens, index) {
                if has_closing_delimiter(tokens, index, delimiter) {
                    for token in &tokens[index..index + delimiter.token_len()] {
                        builder.drop_token(token);
                    }
                    let inner_start = index + delimiter.token_len();
                    let is_code_delim = matches!(delimiter, Delimiter::CodeMarkdown { .. });
                    let parsed = parse_until(
                        tokens,
                        inner_start,
                        Some(delimiter),
                        extra_style.apply(delimiter),
                        extra_html_style,
                        builder,
                        is_code_delim,
                        reference_definitions,
                    );
                    if parsed.closed {
                        index = parsed.next_index;
                        continue;
                    }
                } else if delimiter.token_len() > 1 {
                    // Keep an unclosed multi-character opener (`**`, `__`, `~~`,
                    // backtick run) literal as one unit. Emitting just its first
                    // char would let the rest open a shorter span (e.g. `**bold*`
                    // -> `*` + italic `bold`), which is committed on every
                    // keystroke and loses the intended bold.
                    for token in &tokens[index..index + delimiter.token_len()] {
                        builder.emit_token(token, extra_style, extra_html_style);
                    }
                    index += delimiter.token_len();
                    continue;
                }
            }
        }

        builder.emit_token(&tokens[index], extra_style, extra_html_style);
        index += 1;
    }

    ParseResult {
        next_index: tokens.len(),
        closed: false,
    }
}

pub(crate) fn token_is_backslash_escaped(tokens: &[CharToken], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    let mut cursor = index;
    let mut slash_count = 0usize;
    while cursor > 0 && tokens[cursor - 1].ch == '\\' {
        slash_count += 1;
        cursor -= 1;
    }
    slash_count % 2 == 1
}

pub(crate) fn parse_footnote_reference(
    tokens: &[CharToken],
    index: usize,
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
    builder: &mut NormalizeBuilder,
) -> Option<usize> {
    if tokens.get(index)?.ch != '[' || tokens.get(index + 1)?.ch != '^' {
        return None;
    }

    let mut cursor = index + 2;
    let end_index = loop {
        let token = tokens.get(cursor)?;
        if token.ch == '\\' {
            cursor += 2;
            continue;
        }
        if token.ch == ']' {
            break cursor;
        }
        cursor += 1;
    };

    let raw_markdown = tokens_to_string(&tokens[index..=end_index]);
    let id = parse_inline_footnote_reference(&raw_markdown)?;
    let fragments = vec![InlineFragment {
        text: raw_markdown.clone(),
        style: extra_style,
        html_style: extra_html_style,
        link: None,
        footnote: Some(InlineFootnoteReference {
            id,
            ordinal: None,
            occurrence_index: 0,
        }),
        math: None,
        emoji: None,
        tag: None,
    }];

    let normalized_start = builder.normalized_len;
    let visible_len = raw_markdown.len();
    let normalized_end = normalized_start + visible_len;
    for token in &tokens[index..=end_index] {
        let token_len = token.source_range.len();
        for delta in 0..=token_len {
            builder.visible_to_normalized[token.source_range.start + delta] = normalized_start
                + (token.source_range.start + delta - tokens[index].source_range.start);
        }
    }

    for fragment in fragments {
        builder.normalized_len += fragment.text.len();
        if let Some(last) = builder.fragments.last_mut()
            && last.style == fragment.style
            && last.html_style == fragment.html_style
            && last.link == fragment.link
            && last.footnote == fragment.footnote
            && last.math.is_none()
            && fragment.math.is_none()
            && last.emoji.is_none()
            && fragment.emoji.is_none()
            && last.tag.is_none()
            && fragment.tag.is_none()
        {
            last.text.push_str(&fragment.text);
        } else {
            builder.fragments.push(fragment);
        }
    }

    for boundary in tokens[end_index].source_range.end..=tokens[end_index].source_range.end {
        builder.visible_to_normalized[boundary] = normalized_end;
    }

    Some(end_index + 1)
}

pub(crate) fn apply_extra_style_to_fragments(
    fragments: &mut [InlineFragment],
    extra_style: InlineStyle,
    extra_html_style: Option<HtmlInlineStyle>,
) {
    for fragment in fragments {
        if extra_style.bold {
            fragment.style.bold = true;
        }
        if extra_style.italic {
            fragment.style.italic = true;
        }
        if extra_style.underline {
            fragment.style.underline = true;
        }
        if extra_style.strikethrough {
            fragment.style.strikethrough = true;
        }
        if extra_style.code {
            fragment.style.code = true;
        }
        if extra_style.highlight {
            fragment.style.highlight = true;
        }
        if extra_style.has_script() {
            fragment.style.script = extra_style.script;
        }
        fragment.html_style = merge_html_styles(extra_html_style, fragment.html_style);
    }
}

pub(crate) fn match_open_delimiter(tokens: &[CharToken], index: usize) -> Option<Delimiter> {
    if matches_sequence(tokens, index, "<strong>") {
        Some(Delimiter::BoldHtml)
    } else if matches_sequence(tokens, index, "<em>") {
        Some(Delimiter::ItalicHtml)
    } else if matches_sequence(tokens, index, "<u>") {
        Some(Delimiter::Underline)
    } else if matches_sequence(tokens, index, "~~") {
        Some(Delimiter::StrikethroughMarkdown)
    } else if matches_sequence(tokens, index, "==") {
        Some(Delimiter::HighlightMarkdown)
    } else if matches_sequence(tokens, index, "^") && can_open_script(tokens, index, '^') {
        Some(Delimiter::SuperscriptMarkdown)
    } else if is_single_tilde_delimiter(tokens, index) && can_open_script(tokens, index, '~') {
        Some(Delimiter::SubscriptMarkdown)
    } else if matches_sequence(tokens, index, "**") && can_open_emphasis(tokens, index, 2) {
        Some(Delimiter::BoldMarkdown { marker: '*' })
    } else if matches_sequence(tokens, index, "__") && can_open_emphasis(tokens, index, 2) {
        Some(Delimiter::BoldMarkdown { marker: '_' })
    } else if matches_sequence(tokens, index, "*") && can_open_emphasis(tokens, index, 1) {
        Some(Delimiter::ItalicMarkdown { marker: '*' })
    } else if matches_sequence(tokens, index, "_") && can_open_emphasis(tokens, index, 1) {
        Some(Delimiter::ItalicMarkdown { marker: '_' })
    } else if tokens[index].ch == '`' {
        // Count the run of consecutive backticks.
        let run_len = backtick_run_len(tokens, index);
        // A backtick run is only a valid opener if it is NOT immediately
        // followed by another backtick (no double-counting).
        if run_len > 0 {
            Some(Delimiter::CodeMarkdown { run_len })
        } else {
            None
        }
    } else {
        None
    }
}

/// Returns the length of the consecutive backtick run starting at `index`.
pub(crate) fn backtick_run_len(tokens: &[CharToken], index: usize) -> usize {
    let mut len = 0;
    while index + len < tokens.len() && tokens[index + len].ch == '`' {
        len += 1;
    }
    // A backtick run is only valid if it's not immediately preceded by an
    // additional backtick (the run must start at `index`).
    if index > 0 && tokens[index - 1].ch == '`' {
        return 0;
    }
    len
}

pub(crate) fn has_closing_delimiter(tokens: &[CharToken], index: usize, delimiter: Delimiter) -> bool {
    let skip = delimiter.token_len();
    let close_str = delimiter.close();

    // For code spans we look for a matching-length backtick run;
    // for emphasis we just scan for the close string.
    if let Delimiter::CodeMarkdown { .. } = delimiter {
        let mut cursor = index + skip;
        while cursor < tokens.len() {
            if tokens[cursor].ch == '\\'
                && let Some(escaped_len) = escaped_sequence_token_len(tokens, cursor)
            {
                cursor += 1 + escaped_len;
                continue;
            }

            if tokens[cursor].ch == '`' && backtick_run_len(tokens, cursor) == skip {
                return true;
            }

            cursor += 1;
        }
        return false;
    }

    if matches!(
        delimiter,
        Delimiter::SuperscriptMarkdown | Delimiter::SubscriptMarkdown
    ) {
        let marker = match delimiter {
            Delimiter::SuperscriptMarkdown => '^',
            Delimiter::SubscriptMarkdown => '~',
            _ => unreachable!(),
        };
        return locate_script_close(tokens, index + skip, marker).is_some();
    }

    let body_start = index + skip;
    let requires_body = emphasis_requires_body(delimiter);
    let mut cursor = body_start;
    while cursor < tokens.len() {
        if tokens[cursor].ch == '\\'
            && let Some(escaped_len) = escaped_sequence_token_len(tokens, cursor)
        {
            cursor += 1 + escaped_len;
            continue;
        }

        if matches_sequence(tokens, cursor, &close_str) {
            // Emphasis spans must enclose at least one character; a close
            // sitting immediately after the open (e.g. `**` or `*` `*`) is an
            // empty span and is treated as literal text instead.
            if requires_body && cursor == body_start {
                cursor += 1;
                continue;
            }
            return true;
        }

        cursor += 1;
    }

    false
}

/// Whether `delimiter` requires a non-empty body. Emphasis and strikethrough
/// markers must enclose at least one character; code spans may be empty and
/// script markers already constrain their bodies elsewhere.
pub(crate) fn emphasis_requires_body(delimiter: Delimiter) -> bool {
    matches!(
        delimiter,
        Delimiter::BoldMarkdown { .. }
            | Delimiter::ItalicMarkdown { .. }
            | Delimiter::StrikethroughMarkdown
            | Delimiter::HighlightMarkdown
            | Delimiter::HighlightHtml
            | Delimiter::BoldHtml
            | Delimiter::ItalicHtml
            | Delimiter::Underline
    )
}

pub(crate) fn locate_script_close(tokens: &[CharToken], mut cursor: usize, marker: char) -> Option<usize> {
    let body_start = cursor;
    while cursor < tokens.len() {
        if tokens[cursor].ch == '\\'
            && let Some(escaped_len) = escaped_sequence_token_len(tokens, cursor)
        {
            cursor += 1 + escaped_len;
            continue;
        }

        let is_close = if marker == '~' {
            is_single_tilde_delimiter(tokens, cursor)
        } else {
            tokens[cursor].ch == marker
        };
        if is_close {
            return valid_script_body(tokens, body_start, cursor).then_some(cursor);
        }

        cursor += 1;
    }

    None
}

pub(crate) fn valid_script_body(tokens: &[CharToken], start: usize, end: usize) -> bool {
    start < end
        && tokens[start..end]
            .iter()
            .all(|token| token.ch.is_ascii_alphanumeric())
}

pub(crate) fn is_single_tilde_delimiter(tokens: &[CharToken], index: usize) -> bool {
    tokens.get(index).is_some_and(|token| token.ch == '~')
        && index
            .checked_sub(1)
            .and_then(|prev| tokens.get(prev))
            .is_none_or(|token| token.ch != '~')
        && tokens.get(index + 1).is_none_or(|token| token.ch != '~')
}

pub(crate) fn matches_sequence(tokens: &[CharToken], index: usize, sequence: &str) -> bool {
    sequence
        .chars()
        .enumerate()
        .all(|(offset, ch)| tokens.get(index + offset).is_some_and(|t| t.ch == ch))
}

pub(crate) fn escaped_sequence_token_len(tokens: &[CharToken], index: usize) -> Option<usize> {
    let next_index = index + 1;
    if next_index >= tokens.len() {
        return None;
    }

    if matches_sequence(tokens, next_index, "</strong>") {
        Some(9)
    } else if matches_sequence(tokens, next_index, "<strong>") {
        Some(8)
    } else if matches_sequence(tokens, next_index, "</em>") {
        Some(5)
    } else if matches_sequence(tokens, next_index, "<em>") {
        Some(4)
    } else if matches_sequence(tokens, next_index, "</u>") {
        Some(4)
    } else if matches_sequence(tokens, next_index, "<u>") {
        Some(3)
    } else if matches_sequence(tokens, next_index, "\\")
        || matches_sequence(tokens, next_index, "*")
        || matches_sequence(tokens, next_index, "_")
        || matches_sequence(tokens, next_index, "~")
        || matches_sequence(tokens, next_index, "[")
        || matches_sequence(tokens, next_index, "]")
        || matches_sequence(tokens, next_index, "`")
        || matches_sequence(tokens, next_index, "^")
        || matches_sequence(tokens, next_index, "#")
    {
        Some(1)
    } else {
        None
    }
}

pub(crate) fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    if text.is_char_boundary(clamped) {
        return clamped;
    }

    let mut boundary = clamped;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

pub(crate) fn can_open_emphasis(tokens: &[CharToken], index: usize, len: usize) -> bool {
    tokens
        .get(index + len)
        .map(|token| !token.ch.is_whitespace())
        .unwrap_or(false)
}

pub(crate) fn can_open_script(tokens: &[CharToken], index: usize, marker: char) -> bool {
    if token_is_backslash_escaped(tokens, index) {
        return false;
    }

    if marker == '~' && !is_single_tilde_delimiter(tokens, index) {
        return false;
    }

    index > 0
        && tokens[index - 1].ch.is_ascii_alphanumeric()
        && tokens
            .get(index + 1)
            .is_some_and(|token| token.ch.is_ascii_alphanumeric())
}

pub(crate) fn can_close_emphasis(tokens: &[CharToken], index: usize) -> bool {
    index > 0 && !tokens[index - 1].ch.is_whitespace()
}

