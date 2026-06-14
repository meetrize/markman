//! Attribute-based inline Markdown tree for block titles and table cells.
//!
//! The runtime model stores only text fragments and formatting attributes.
//! Markdown markers are parsed at the I/O boundary and regenerated on save,
//! which keeps editing operations focused on text ranges instead of raw
//! delimiter strings.

use std::ops::Range;

use super::footnote::{
    InlineFootnoteHit, InlineFootnoteReference,
    superscript_ordinal,
};
use super::html::HtmlInlineStyle;
use super::link::LinkReferenceDefinitions;
use crate::input::text_norm::normalize_line_endings_lf;


mod delimiter;
mod emoji;
mod fragment;
mod hashtag;
mod html;
mod link_image;
mod math;
mod normalize;
mod wiki_link;
mod serialize;
mod style;

pub use fragment::{
    InlineFragment, InlineInsertionAttributes, InlineLink, InlineLinkHit, InlineMath, InlineSpan,
    InlineTagHit, TextCursor,
};
#[allow(unused_imports)]
pub use fragment::InlineTag;
pub(crate) use normalize::clamp_to_char_boundary;
pub(crate) use hashtag::{locate_hashtag_in_str, normalize_tag_name};
pub(crate) use wiki_link::locate_wiki_link_in_str;
pub use style::{InlineScript, InlineStyle};
pub(crate) use style::StyleFlag;
pub(crate) use serialize::can_use_markdown_script_delimiters;
pub(crate) use emoji::resolve_emoji_shortcode;

/// Flattens the fragment tree into a visible text string plus a list of
/// [`InlineSpan`]s.  Also maintains bidirectional mapping tables between
/// visible offsets and fragment positions, used by the IME subsystem.
#[derive(Clone, Debug, Default)]
pub struct InlineRenderCache {
    visible_text: String,
    spans: Vec<InlineSpan>,
    #[allow(dead_code)]
    visible_to_tree: Vec<TextCursor>,
    #[allow(dead_code)]
    tree_to_visible: Vec<usize>,
}

/// Bidirectional offset map between source Markdown and visible inline text.
impl InlineRenderCache {
    pub fn from_tree(tree: &InlineTextTree) -> Self {
        let mut visible_text = String::new();
        let mut spans = Vec::new();
        let mut visible_to_tree = vec![TextCursor::default(); tree.visible_len() + 1];
        let mut tree_to_visible = Vec::with_capacity(tree.fragments.len() + 1);
        let mut visible_offset = 0;

        for (fragment_index, fragment) in tree.fragments.iter().enumerate() {
            tree_to_visible.push(visible_offset);
            let fragment_start = visible_offset;
            visible_text.push_str(&fragment.text);
            let fragment_len = fragment.text.len();
            if fragment_len > 0 {
                spans.push(InlineSpan {
                    range: fragment_start..fragment_start + fragment_len,
                    style: fragment.style,
                    html_style: fragment.html_style,
                    link: fragment.link.as_ref().map(InlineLink::hit),
                    footnote: fragment
                        .footnote
                        .as_ref()
                        .and_then(InlineFootnoteReference::hit),
                    math: fragment.math.clone(),
                    tag: fragment.tag.as_ref().map(|tag| InlineTagHit {
                        name: tag.name.clone(),
                        source: tag.source.clone(),
                    }),
                });
            }

            for byte_offset in 0..=fragment_len {
                visible_to_tree[fragment_start + byte_offset] = TextCursor {
                    fragment_index,
                    byte_offset,
                };
            }

            visible_offset += fragment_len;
        }

        tree_to_visible.push(visible_offset);
        if tree.fragments.is_empty() {
            visible_to_tree[0] = TextCursor::default();
        }

        Self {
            visible_text,
            spans,
            visible_to_tree,
            tree_to_visible,
        }
    }

    pub fn visible_text(&self) -> &str {
        &self.visible_text
    }

    pub fn spans(&self) -> &[InlineSpan] {
        &self.spans
    }

    pub fn visible_len(&self) -> usize {
        self.visible_text.len()
    }

    pub fn style_at(&self, offset: usize) -> InlineStyle {
        self.spans
            .iter()
            .find(|span| span.range.start <= offset && offset < span.range.end)
            .map(|span| span.style)
            .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn html_style_at(&self, offset: usize) -> Option<HtmlInlineStyle> {
        self.spans
            .iter()
            .find(|span| span.range.start <= offset && offset < span.range.end)
            .and_then(|span| span.html_style)
    }

    #[allow(dead_code)]
    pub fn link_at(&self, offset: usize) -> Option<&str> {
        self.link_hit_at(offset).map(|hit| hit.open_target.as_str())
    }

    pub fn link_hit_at(&self, offset: usize) -> Option<&InlineLinkHit> {
        self.spans
            .iter()
            .find(|span| span.range.start <= offset && offset < span.range.end)
            .and_then(|span| span.link.as_ref())
    }

    #[allow(dead_code)]
    pub fn footnote_hit_at(&self, offset: usize) -> Option<&InlineFootnoteHit> {
        self.spans
            .iter()
            .find(|span| span.range.start <= offset && offset < span.range.end)
            .and_then(|span| span.footnote.as_ref())
    }

    #[allow(dead_code)]
    pub fn inline_math_at(&self, offset: usize) -> Option<&InlineMath> {
        self.spans
            .iter()
            .find(|span| span.range.start <= offset && offset < span.range.end)
            .and_then(|span| span.math.as_ref())
    }
}

/// A sequence of [`InlineFragment`]s representing inline-formatted text.
///
/// This is the core data structure for block titles.  It supports:
/// - Building from raw Markdown (auto-parsing bold/italic/underline markers)
/// - Bidirectional Markdown serialization with optimal delimiter choice
/// - Splitting at arbitrary byte offsets (used for Enter key, paste)
/// - Toggling inline styles on arbitrary ranges
///
/// The serialization uses a Viterbi-like DP optimization to choose between
/// Markdown and HTML delimiter variants, avoiding ambiguous `****` runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineTextTree {
    pub(crate) fragments: Vec<InlineFragment>,
}

impl InlineTextTree {
    pub fn plain(text: impl Into<String>) -> Self {
        Self::from_fragments(vec![InlineFragment {
            text: text.into(),
            style: InlineStyle::default(),
            html_style: None,
            link: None,
            footnote: None,
            math: None,
            emoji: None,
            tag: None,
        }])
    }

    /// Parse marker-based Markdown into the internal fragment representation.
    ///
    /// Markers (`**`, `*`, `<u>`, `<strong>`, `<em>`) are consumed and
    /// converted to [`InlineStyle`] flags on adjacent fragments.  The
    /// markers themselves are never stored — the tree holds only text
    /// content and style attributes.
    pub fn from_markdown(markdown: &str) -> Self {
        Self::from_markdown_with_link_references(markdown, &LinkReferenceDefinitions::default())
    }

    pub fn from_markdown_with_link_references(
        markdown: &str,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> Self {
        let mut tree = Self::plain(markdown)
            .normalize_inline_syntax_with_link_references(reference_definitions)
            .tree;
        tree.normalize_code_spans();
        tree
    }

    /// Code-span content normalization:
    /// - CRLF/CR line endings are normalized to LF so inline code can render
    ///   across hard lines in the editor.
    /// - If the content is not entirely spaces and both starts AND ends with
    ///   a single space, those two spaces are stripped.
    fn normalize_code_spans(&mut self) {
        for fragment in &mut self.fragments {
            if fragment.style.code && !fragment.text.is_empty() {
                let mut s = normalize_line_endings_lf(&fragment.text);
                let all_space = s.chars().all(|c| c == ' ');
                if !all_space && s.starts_with(' ') && s.ends_with(' ') {
                    s.remove(0);
                    s.pop();
                }
                fragment.text = s;
            }
        }
        self.normalize_fragments();
    }

    pub fn from_fragments(fragments: Vec<InlineFragment>) -> Self {
        let mut tree = Self { fragments };
        tree.normalize_fragments();
        tree
    }

    pub fn visible_text(&self) -> String {
        let mut text = String::new();
        for fragment in &self.fragments {
            text.push_str(&fragment.text);
        }
        text
    }

    pub fn visible_len(&self) -> usize {
        self.fragments
            .iter()
            .map(|fragment| fragment.text.len())
            .sum()
    }

    pub(crate) fn has_source_preserving_links(&self) -> bool {
        self.fragments.iter().any(|fragment| {
            fragment
                .link
                .as_ref()
                .is_some_and(InlineLink::is_source_preserving)
                || fragment.footnote.is_some()
                || fragment.math.is_some()
        })
    }

    pub(crate) fn has_inline_math(&self) -> bool {
        self.fragments
            .iter()
            .any(|fragment| fragment.math.is_some())
    }

    pub(crate) fn has_mixed_inline_visuals(&self) -> bool {
        self.fragments.iter().any(|fragment| {
            fragment.math.is_some()
                || fragment.style.has_script()
                || fragment.style.highlight
        })
    }

    pub(crate) fn has_footnote_references(&self) -> bool {
        self.fragments
            .iter()
            .any(|fragment| fragment.footnote.is_some())
    }

    pub(crate) fn apply_footnote_reference_state(
        &mut self,
        mut resolve: impl FnMut(&str) -> Option<(usize, usize)>,
    ) {
        for fragment in &mut self.fragments {
            let Some(footnote) = fragment.footnote.as_mut() else {
                continue;
            };
            if let Some((ordinal, occurrence_index)) = resolve(&footnote.id) {
                footnote.ordinal = Some(ordinal);
                footnote.occurrence_index = occurrence_index;
                fragment.text = superscript_ordinal(ordinal);
            } else {
                footnote.ordinal = None;
                footnote.occurrence_index = 0;
                fragment.text = footnote.raw_markdown();
            }
        }
        self.normalize_fragments();
    }

    pub fn render_cache(&self) -> InlineRenderCache {
        InlineRenderCache::from_tree(self)
    }

    /// Serialize fragments back to Markdown text with optimal delimiter choices.
    ///
    /// Each fragment's style flags determine which markers surround its text.
    /// This is the export side of the I/O boundary; the internal fragment
    /// representation never stores raw marker characters.
    pub fn serialize_markdown(&self) -> String {
        self.markdown_offset_map().markdown
    }

    pub(crate) fn markdown_offset_map(&self) -> serialize::InlineMarkdownOffsetMap {
        if self.fragments.is_empty() {
            return serialize::InlineMarkdownOffsetMap {
                markdown: String::new(),
                visible_to_markdown: vec![0],
                markdown_to_visible: vec![0],
            };
        }

        let mut output = String::new();
        let mut visible_to_markdown = vec![0; self.visible_len() + 1];
        let mut markdown_to_visible = vec![0];
        let mut visible_cursor = 0usize;
        let mut index = 0usize;
        while index < self.fragments.len() {
            if let Some(footnote) = self.fragments[index].footnote.clone() {
                let raw_markdown = footnote.raw_markdown();
                let raw_len = raw_markdown.len();
                let run_visible_len = self.fragments[index].text.len();
                let run_start = output.len();
                output.push_str(&raw_markdown);
                let run_end = output.len();

                for local_visible in 0..=run_visible_len {
                    let mapped = if run_visible_len == 0 {
                        0
                    } else {
                        (raw_len * local_visible) / run_visible_len
                    };
                    visible_to_markdown[visible_cursor + local_visible] = run_start + mapped;
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=raw_len {
                    let mapped = if raw_len == 0 {
                        0
                    } else {
                        (run_visible_len * local_markdown) / raw_len
                    };
                    markdown_to_visible[run_start + local_markdown] = visible_cursor + mapped;
                }

                visible_cursor += run_visible_len;
                index += 1;
                continue;
            }

            if let Some(emoji) = self.fragments[index].emoji.clone() {
                let raw_markdown = emoji.source;
                let raw_len = raw_markdown.len();
                let run_visible_len = self.fragments[index].text.len();
                let run_start = output.len();
                output.push_str(&raw_markdown);
                let run_end = output.len();

                for local_visible in 0..=run_visible_len {
                    let mapped = if run_visible_len == 0 {
                        0
                    } else {
                        (raw_len * local_visible) / run_visible_len
                    };
                    visible_to_markdown[visible_cursor + local_visible] = run_start + mapped;
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=raw_len {
                    let mapped = if raw_len == 0 {
                        0
                    } else {
                        (run_visible_len * local_markdown) / raw_len
                    };
                    markdown_to_visible[run_start + local_markdown] = visible_cursor + mapped;
                }

                visible_cursor += run_visible_len;
                index += 1;
                continue;
            }

            if let Some(tag) = self.fragments[index].tag.clone() {
                let raw_markdown = tag.source;
                let raw_len = raw_markdown.len();
                let run_visible_len = self.fragments[index].text.len();
                let run_start = output.len();
                output.push_str(&raw_markdown);
                let run_end = output.len();

                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        run_start + local_visible.min(raw_len);
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=raw_len {
                    markdown_to_visible[run_start + local_markdown] =
                        visible_cursor + local_markdown.min(run_visible_len);
                }

                visible_cursor += run_visible_len;
                index += 1;
                continue;
            }

            if let Some(math) = self.fragments[index].math.clone() {
                let raw_markdown = math.source;
                let raw_len = raw_markdown.len();
                let run_visible_len = self.fragments[index].text.len();
                let run_start = output.len();
                output.push_str(&raw_markdown);
                let run_end = output.len();

                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        run_start + local_visible.min(raw_len);
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=raw_len {
                    markdown_to_visible[run_start + local_markdown] =
                        visible_cursor + local_markdown.min(run_visible_len);
                }

                visible_cursor += run_visible_len;
                index += 1;
                continue;
            }

            let link = self.fragments[index].link.clone();
            let mut end = index + 1;
            while end < self.fragments.len()
                && self.fragments[end].link == link
                && self.fragments[end].footnote.is_none()
                && self.fragments[end].math.is_none()
                && self.fragments[end].emoji.is_none()
                && self.fragments[end].tag.is_none()
            {
                end += 1;
            }

            let run_map =
                serialize::serialize_fragment_run_markdown_with_offset_map(&self.fragments[index..end]);
            if let Some(link) = link {
                let run_visible_len = run_map.visible_to_markdown.len().saturating_sub(1);
                let link_start = output.len();
                let editable_text = link.editable_text();
                output.push_str(link.open_marker());
                output.push_str(run_map.markdown());
                if let Some(middle_marker) = link.middle_marker() {
                    output.push_str(middle_marker);
                }
                if let Some(editable_text) = editable_text.as_deref() {
                    output.push_str(editable_text);
                }
                output.push_str(link.close_marker());
                let link_end = output.len();
                let label_markdown_start = link_start + link.open_marker().len();

                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        label_markdown_start + run_map.visible_to_markdown_offset(local_visible);
                }

                markdown_to_visible.resize(link_end + 1, visible_cursor);
                for local in 0..=link.open_marker().len() {
                    markdown_to_visible[link_start + local] = visible_cursor;
                }
                for local_markdown in 0..run_map.markdown().len() {
                    markdown_to_visible[label_markdown_start + local_markdown] =
                        visible_cursor + run_map.markdown_to_visible_offset(local_markdown);
                }

                let label_markdown_end = label_markdown_start + run_map.markdown().len();
                markdown_to_visible[label_markdown_end] = visible_cursor + run_visible_len;

                let suffix_start = label_markdown_end;
                let suffix_len = link.middle_marker().map(str::len).unwrap_or(0)
                    + editable_text.as_ref().map(String::len).unwrap_or(0)
                    + link.close_marker().len();
                for local in 0..=suffix_len {
                    markdown_to_visible[suffix_start + local] = visible_cursor + run_visible_len;
                }
                visible_cursor += run_visible_len;
            } else {
                let run_start = output.len();
                output.push_str(run_map.markdown());
                let run_end = output.len();

                let run_visible_len = run_map.visible_to_markdown.len().saturating_sub(1);
                for local_visible in 0..=run_visible_len {
                    visible_to_markdown[visible_cursor + local_visible] =
                        run_start + run_map.visible_to_markdown_offset(local_visible);
                }

                markdown_to_visible.resize(run_end + 1, visible_cursor);
                for local_markdown in 0..=run_map.markdown().len() {
                    markdown_to_visible[run_start + local_markdown] =
                        visible_cursor + run_map.markdown_to_visible_offset(local_markdown);
                }
                visible_cursor += run_visible_len;
            }

            index = end;
        }

        serialize::InlineMarkdownOffsetMap {
            markdown: output,
            visible_to_markdown,
            markdown_to_visible,
        }
    }
}

impl InlineTextTree {
    pub fn split_at(&self, offset: usize) -> (Self, Self) {
        let clamped = offset.min(self.visible_len());
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut consumed = 0;

        for fragment in &self.fragments {
            let fragment_len = fragment.text.len();
            let fragment_start = consumed;
            let fragment_end = fragment_start + fragment_len;

            if clamped <= fragment_start {
                right.push(fragment.clone());
            } else if clamped >= fragment_end {
                left.push(fragment.clone());
            } else {
                let split_offset = normalize::clamp_to_char_boundary(&fragment.text, clamped - fragment_start);
                if split_offset > 0 {
                    left.push(InlineFragment {
                        text: fragment.text[..split_offset].to_string(),
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                        emoji: None,
                        tag: None,
                    });
                }
                if split_offset < fragment_len {
                    right.push(InlineFragment {
                        text: fragment.text[split_offset..].to_string(),
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                        emoji: None,
                        tag: None,
                    });
                }
            }

            consumed = fragment_end;
        }

        (Self::from_fragments(left), Self::from_fragments(right))
    }

    pub fn append_tree(&mut self, other: Self) {
        self.fragments.extend(other.fragments);
        self.normalize_fragments();
    }

    pub(crate) fn replace_fragment_range(
        &mut self,
        range: Range<usize>,
        replacement: Vec<InlineFragment>,
    ) {
        self.fragments.splice(range, replacement);
        self.normalize_fragments();
    }

    pub fn remove_visible_prefix(&mut self, prefix_len: usize) {
        let (_, tail) = self.split_at(prefix_len);
        *self = tail;
    }

    pub fn attributes_for_insertion_at(&self, offset: usize) -> InlineInsertionAttributes {
        if self.fragments.is_empty() {
            return InlineInsertionAttributes::default();
        }

        let clamped = offset.min(self.visible_len());
        let mut consumed = 0;

        for (index, fragment) in self.fragments.iter().enumerate() {
            let fragment_len = fragment.text.len();
            let fragment_start = consumed;
            let fragment_end = fragment_start + fragment_len;

            if fragment_start < clamped && clamped < fragment_end {
                return InlineInsertionAttributes {
                    style: fragment.style,
                    html_style: fragment.html_style,
                    link: fragment.link.clone(),
                    footnote: fragment.footnote.clone(),
                    math: None,
                };
            }

            // Typing at a delimited-fragment boundary should produce plain
            // text, not extend the span past its visible closing/opening
            // marker when the caret is outside.
            if clamped == fragment_end && index + 1 == self.fragments.len() {
                return if fragment.style.code || fragment.style.strikethrough {
                    InlineInsertionAttributes::default()
                } else {
                    InlineInsertionAttributes {
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                    }
                };
            }

            if clamped == fragment_start && index == 0 {
                return if fragment.style.code || fragment.style.strikethrough {
                    InlineInsertionAttributes::default()
                } else {
                    InlineInsertionAttributes {
                        style: fragment.style,
                        html_style: fragment.html_style,
                        link: fragment.link.clone(),
                        footnote: fragment.footnote.clone(),
                        math: None,
                    }
                };
            }

            consumed = fragment_end;
        }

        InlineInsertionAttributes::default()
    }

    pub fn toggle_bold(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Bold)
    }

    pub fn toggle_italic(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Italic)
    }

    pub fn toggle_underline(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Underline)
    }

    #[allow(dead_code)]
    pub fn toggle_strikethrough(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Strikethrough)
    }

    pub fn toggle_code(&mut self, range: Range<usize>) -> bool {
        self.toggle_style(range, StyleFlag::Code)
    }

    pub fn unwrap_styles_on_fragments(&mut self, targets: &[(usize, StyleFlag)]) {
        if targets.is_empty() {
            return;
        }

        for (fragment_index, flag) in targets {
            if let Some(fragment) = self.fragments.get_mut(*fragment_index) {
                fragment.style = style::set_style_flag(fragment.style, *flag, false);
            }
        }
        self.normalize_fragments();
    }

    #[allow(dead_code)]
    pub fn replace_visible_range(
        &self,
        range: Range<usize>,
        new_text: &str,
        inserted_attributes: InlineInsertionAttributes,
    ) -> InlineEditResult {
        self.replace_visible_range_with_link_references(
            range,
            new_text,
            inserted_attributes,
            &LinkReferenceDefinitions::default(),
        )
    }

    pub fn replace_visible_range_with_link_references(
        &self,
        range: Range<usize>,
        new_text: &str,
        inserted_attributes: InlineInsertionAttributes,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> InlineEditResult {
        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        let (before, tail) = self.split_at(clamped_start);
        let (_, after) = tail.split_at(clamped_end.saturating_sub(clamped_start));

        let mut temp = before;
        if !new_text.is_empty() {
            temp.fragments.push(InlineFragment {
                text: new_text.to_string(),
                style: inserted_attributes.style,
                html_style: inserted_attributes.html_style,
                link: inserted_attributes.link,
                footnote: inserted_attributes.footnote,
                math: inserted_attributes.math,
                emoji: None,
                tag: None,
            });
        }
        temp.append_tree(after);
        temp.normalize_fragments();
        temp.normalize_inline_syntax_with_link_references(reference_definitions)
    }

    /// Like `replace_visible_range` but skips marker normalization so
    /// that backticks, stars, and other delimiters are stored as-is.
    /// Used for source-mode editing where the text must remain raw.
    pub fn replace_visible_range_raw(
        &self,
        range: Range<usize>,
        new_text: &str,
        inserted_attributes: InlineInsertionAttributes,
    ) -> InlineEditResult {
        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        let (before, tail) = self.split_at(clamped_start);
        let (_, after) = tail.split_at(clamped_end.saturating_sub(clamped_start));

        let mut temp = before;
        if !new_text.is_empty() {
            temp.fragments.push(InlineFragment {
                text: new_text.to_string(),
                style: inserted_attributes.style,
                html_style: inserted_attributes.html_style,
                link: inserted_attributes.link,
                footnote: inserted_attributes.footnote,
                math: inserted_attributes.math,
                emoji: None,
                tag: None,
            });
        }
        temp.append_tree(after);
        temp.normalize_fragments();
        let len = temp.visible_len();
        InlineEditResult {
            tree: InlineTextTree::from_fragments(temp.fragments),
            visible_to_normalized: (0..=len).collect(),
        }
    }

    /// Core marker-to-style normalizer: scans the fragment text for
    /// delimiter sequences (`**`, `*`, `<u>`, etc.), removes them, and
    /// applies the corresponding [`InlineStyle`] to the text between
    /// matching pairs.  Unmatched delimiters are emitted as literal text.
    #[allow(dead_code)]
    pub fn normalize_inline_syntax(&self) -> InlineEditResult {
        self.normalize_inline_syntax_with_link_references(&LinkReferenceDefinitions::default())
    }

    pub fn normalize_inline_syntax_with_link_references(
        &self,
        reference_definitions: &LinkReferenceDefinitions,
    ) -> InlineEditResult {
        let visible_text = self.visible_text();
        let tokens = normalize::flatten_tokens(&self.fragments);
        let mut builder = normalize::NormalizeBuilder::new(visible_text.len());
        let _ = normalize::parse_until(
            &tokens,
            0,
            None,
            InlineStyle::default(),
            None,
            &mut builder,
            false,
            reference_definitions,
        );
        InlineEditResult {
            tree: InlineTextTree::from_fragments(builder.fragments),
            visible_to_normalized: builder.visible_to_normalized,
        }
    }

    fn toggle_style(&mut self, range: Range<usize>, flag: StyleFlag) -> bool {
        if range.is_empty() {
            return false;
        }

        let clamped_start = range.start.min(self.visible_len());
        let clamped_end = range.end.min(self.visible_len());
        if clamped_start >= clamped_end {
            return false;
        }

        let (before, tail) = self.split_at(clamped_start);
        let (mut middle, after) = tail.split_at(clamped_end - clamped_start);
        let should_remove = middle
            .fragments
            .iter()
            .all(|fragment| style::style_flag_enabled(fragment.style, flag));

        for fragment in &mut middle.fragments {
            fragment.style = style::set_style_flag(fragment.style, flag, !should_remove);
        }
        middle.normalize_fragments();

        let mut next = before;
        next.append_tree(middle);
        next.append_tree(after);
        *self = next;
        true
    }

    fn normalize_fragments(&mut self) {
        let mut normalized: Vec<InlineFragment> = Vec::new();
        for fragment in self.fragments.drain(..) {
            if fragment.text.is_empty() {
                continue;
            }

            if let Some(last) = normalized.last_mut()
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
                continue;
            }

            normalized.push(fragment);
        }
        self.fragments = normalized;
    }
}
/// Result of a visible-text replacement operation, containing the
/// normalized tree and a mapping from pre-edit visible offsets to
/// post-edit tree offsets.
#[derive(Clone, Debug)]
pub struct InlineEditResult {
    pub tree: InlineTextTree,
    visible_to_normalized: Vec<usize>,
}

impl InlineEditResult {
    pub fn map_offset(&self, offset: usize) -> usize {
        let mapped = self
            .visible_to_normalized
            .get(offset.min(self.visible_to_normalized.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        clamp_to_char_boundary(&self.tree.visible_text(), mapped)
    }

    pub fn map_range(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.tree.visible_text();
        let start = clamp_to_char_boundary(&text, self.map_offset(range.start));
        let end = clamp_to_char_boundary(&text, self.map_offset(range.end));
        start..end.max(start)
    }
}
#[cfg(test)]
mod tests {
    use super::{
        InlineFragment, InlineInsertionAttributes, InlineLinkHit, InlineScript, InlineStyle,
        InlineTextTree, LinkReferenceDefinitions, StyleFlag,
    };
    use super::fragment::InlineMathDelimiter;
    use crate::components::HtmlCssColor;

    #[test]
    fn map_offset_after_chinese_hashtag_stays_on_char_boundary() {
        let tree = InlineTextTree::from_markdown("话题 #工作");
        let result = tree.replace_visible_range(
            "话题 #工作".len().. "话题 #工作".len(),
            "。",
            InlineInsertionAttributes::default(),
        );
        let mapped = result.map_offset("话题 #工作".len());
        assert!(result.tree.visible_text().is_char_boundary(mapped));
    }

    #[test]
    fn parses_supported_styles_and_serializes_canonically() {
        let tree = InlineTextTree::from_markdown("1**23**4*56*7<u>89</u>0***ab***<u>*cd*</u>");
        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(tree.visible_text(), "1234567890abcd");
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn parses_underscore_emphasis_and_canonicalizes_to_asterisks() {
        let tree = InlineTextTree::from_markdown("_a_ __b__");

        assert_eq!(tree.visible_text(), "a b");
        assert_eq!(tree.serialize_markdown(), "*a* **b**");
    }

    #[test]
    fn emphasis_delimiters_surrounded_by_spaces_stay_literal() {
        let tree = InlineTextTree::from_markdown("* a * _ b _");

        assert_eq!(tree.visible_text(), "* a * _ b _");
        assert_eq!(tree.serialize_markdown(), "\\* a \\* \\_ b \\_");
    }

    #[test]
    fn preserves_unclosed_markers_as_literal_text() {
        let tree = InlineTextTree::from_markdown("1**234");

        assert_eq!(tree.visible_text(), "1**234");
        assert_eq!(tree.serialize_markdown(), "1\\*\\*234");
    }

    #[test]
    fn empty_emphasis_spans_stay_literal() {
        // `**`, `* *`, or `**word` must not be swallowed as an empty emphasis
        // span; the markers stay literal until a non-empty body is closed.
        for input in ["*", "**", "***", "****", "~~~~", "__"] {
            let tree = InlineTextTree::from_markdown(input);
            assert_eq!(tree.visible_text(), input, "input {input:?} lost markers");
        }

        let leading = InlineTextTree::from_markdown("**word");
        assert_eq!(leading.visible_text(), "**word");
        assert_eq!(leading.serialize_markdown(), "\\*\\*word");

        let trailing = InlineTextTree::from_markdown("**word*");
        assert_eq!(trailing.visible_text(), "**word*");
    }

    #[test]
    fn non_empty_emphasis_still_parses_after_empty_guard() {
        let bold = InlineTextTree::from_markdown("**word**");
        assert_eq!(bold.visible_text(), "word");
        assert_eq!(bold.serialize_markdown(), "**word**");

        let italic = InlineTextTree::from_markdown("*a*");
        assert_eq!(italic.visible_text(), "a");
        assert_eq!(italic.serialize_markdown(), "*a*");

        let single_char_bold = InlineTextTree::from_markdown("**a**");
        assert_eq!(single_char_bold.visible_text(), "a");
        assert_eq!(single_char_bold.serialize_markdown(), "**a**");

        let bold_italic = InlineTextTree::from_markdown("***x***");
        assert_eq!(bold_italic.visible_text(), "x");
        let spans = bold_italic.render_cache();
        assert!(
            spans
                .spans()
                .iter()
                .all(|span| span.style.bold && span.style.italic)
        );
    }

    #[test]
    fn unclosed_multichar_opener_stays_fully_literal() {
        // While typing `**bold**`, the intermediate `**bold*` must stay literal;
        // otherwise the second `*` opens an italic span and the bold is lost.
        let partial = InlineTextTree::from_markdown("**bold*");
        assert_eq!(partial.visible_text(), "**bold*");
        assert!(
            partial
                .render_cache()
                .spans()
                .iter()
                .all(|span| !span.style.italic && !span.style.bold),
            "`**bold*` must be plain literal, not italic"
        );

        // The completed marker still resolves to bold (not italic).
        let complete = InlineTextTree::from_markdown("**bold**");
        assert_eq!(complete.visible_text(), "bold");
        assert!(
            complete
                .render_cache()
                .spans()
                .iter()
                .all(|span| span.style.bold && !span.style.italic),
            "`**bold**` must be bold, not italic"
        );

        // A genuine single-`*` italic opener is unaffected by the multi-char rule.
        let italic = InlineTextTree::from_markdown("*word*");
        assert_eq!(italic.visible_text(), "word");
        assert!(
            italic
                .render_cache()
                .spans()
                .iter()
                .all(|span| span.style.italic && !span.style.bold),
            "`*word*` must stay italic"
        );

        // Other unclosed multi-char openers stay literal as a unit too.
        for input in ["__bold_", "~~strike~"] {
            let tree = InlineTextTree::from_markdown(input);
            assert_eq!(tree.visible_text(), input, "input {input:?} lost markers");
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| !span.style.italic
                        && !span.style.bold
                        && !span.style.strikethrough),
                "input {input:?} should be plain literal"
            );
        }
    }

    #[test]
    fn empty_code_span_is_unaffected_by_emphasis_guard() {
        // The empty-emphasis guard must not touch code spans. `*` inside a code
        // span stays literal and the span round-trips.
        let tree = InlineTextTree::from_markdown("`*`");
        assert_eq!(tree.visible_text(), "*");
        assert_eq!(tree.serialize_markdown(), "`*`");
    }

    #[test]
    fn preserves_escaped_marker_sequences_as_literal_text() {
        let tree = InlineTextTree::from_markdown("\\*\\*\\<u>text\\</u>\\\\");

        assert_eq!(tree.visible_text(), "**<u>text</u>\\");
        assert_eq!(tree.serialize_markdown(), "\\*\\*\\<u>text\\</u>\\\\");
    }

    #[test]
    fn preserves_tibetan_spaces_through_inline_round_trip() {
        let markdown = "༄༅།།དཔལ་ལྡན་རྩ་བའི་བླ་མ་རིན་པོ་ཆེ།། བདག་གི་སྤྱི་བོར་པདྨའི་གདན་བཞུགས་ནས།། ";
        let tree = InlineTextTree::from_markdown(markdown);
        let serialized = tree.serialize_markdown();

        assert_eq!(tree.visible_text(), markdown);
        assert!(tree.visible_text().contains("།། བདག"));
        assert!(tree.visible_text().ends_with(' '));
        assert_eq!(serialized, markdown);
        assert_eq!(
            InlineTextTree::from_markdown(&serialized).visible_text(),
            markdown
        );
    }

    #[test]
    fn preserves_chinese_spaces_through_inline_round_trip() {
        let markdown = "中文 文本 ";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn toggle_style_operates_on_selected_slice_only() {
        let mut tree = InlineTextTree::plain("123");
        assert!(tree.toggle_bold(1..3));
        assert_eq!(tree.serialize_markdown(), "1**23**");

        assert!(tree.toggle_bold(2..3));
        assert_eq!(tree.serialize_markdown(), "1**2**3");
    }

    #[test]
    fn replaces_visible_range_and_normalizes_manual_markdown_input() {
        let tree = InlineTextTree::plain(String::new());
        let result =
            tree.replace_visible_range(0..0, "**bold**", InlineInsertionAttributes::default());

        assert_eq!(result.tree.visible_text(), "bold");
        assert_eq!(result.map_offset(8), 4);
        assert_eq!(result.tree.serialize_markdown(), "**bold**");
    }

    #[test]
    fn renders_nested_marks_without_storing_markers_in_text() {
        let tree = InlineTextTree::from_markdown("**<u>*TEST*</u>**");
        let cache = tree.render_cache();

        assert_eq!(cache.visible_text(), "TEST");
        assert_eq!(
            cache.style_at(0),
            InlineStyle {
                bold: true,
                italic: true,
                underline: true,
                strikethrough: false,
                code: false,
                highlight: false,
                script: InlineScript::Normal,
            }
        );
    }

    #[test]
    fn replace_visible_range_raw_preserves_markers_as_literal_text() {
        let tree = InlineTextTree::plain("alpha");
        let result = tree.replace_visible_range_raw(
            5..5,
            "**`<u>x</u>`**",
            InlineInsertionAttributes::default(),
        );

        assert_eq!(result.tree.visible_text(), "alpha**`<u>x</u>`**");
        assert_eq!(
            result.tree.serialize_markdown(),
            "alpha\\*\\*\\`\\<u>x\\</u>\\`\\*\\*"
        );
    }

    #[test]
    fn unwrap_code_fragments_keeps_text_and_removes_code_style() {
        let mut tree = InlineTextTree::from_markdown("before `code` after");
        tree.unwrap_styles_on_fragments(&[(1, StyleFlag::Code)]);

        assert_eq!(tree.visible_text(), "before code after");
        let cache = tree.render_cache();
        assert!(!cache.style_at(7).code);
        assert_eq!(tree.serialize_markdown(), "before code after");
    }

    #[test]
    fn parses_and_serializes_strikethrough() {
        let tree = InlineTextTree::from_markdown("~~text~~");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "text");
        assert!(cache.style_at(0).strikethrough);
        assert_eq!(tree.serialize_markdown(), "~~text~~");
    }

    #[test]
    fn parses_and_serializes_superscript() {
        let tree = InlineTextTree::from_markdown("x^2^");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "x2");
        assert_eq!(cache.style_at(1).script, InlineScript::Superscript);
        assert_eq!(tree.serialize_markdown(), "x^2^");
    }

    #[test]
    fn parses_and_serializes_subscript_without_conflicting_with_strikethrough() {
        let tree = InlineTextTree::from_markdown("H~2~O and ~~old~~");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "H2O and old");
        assert_eq!(cache.style_at(1).script, InlineScript::Subscript);
        assert!(cache.style_at("H2O and ".len()).strikethrough);
        assert_eq!(tree.serialize_markdown(), "H~2~O and ~~old~~");
    }

    #[test]
    fn script_markers_require_ascii_context_and_ascii_body() {
        for markdown in ["\\^2^", "\\~2~", "汉^2^", "H~二~O", "`x^2^ H~2~O`"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| span.style.script == InlineScript::Normal),
                "{markdown} should not produce script spans"
            );
        }
    }

    #[test]
    fn inline_html_sup_and_sub_map_to_script_style() {
        let tree = InlineTextTree::from_markdown("x<sup>2</sup> and H<sub>2</sub>O");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "x2 and H2O");
        assert_eq!(cache.style_at(1).script, InlineScript::Superscript);
        assert_eq!(
            cache.style_at("x2 and H".len()).script,
            InlineScript::Subscript
        );
        assert_eq!(tree.serialize_markdown(), "x^2^ and H~2~O");

        let standalone = InlineTextTree::from_markdown("<sup>2</sup>");
        assert_eq!(standalone.serialize_markdown(), "<sup>2</sup>");
    }

    #[test]
    fn unmatched_strikethrough_markers_stay_literal() {
        let tree = InlineTextTree::from_markdown("~~text");
        assert_eq!(tree.visible_text(), "~~text");
        assert_eq!(tree.serialize_markdown(), "\\~\\~text");
    }

    #[test]
    fn toggle_strikethrough_operates_on_selected_slice_only() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_strikethrough(1..4));
        assert!(tree.toggle_strikethrough(2..4));

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "1~~2~~34");
        assert_eq!(tree, reparsed);
    }

    #[test]
    fn insertion_at_outer_end_of_terminal_strikethrough_is_plain_text() {
        let tree = InlineTextTree::from_markdown("~~123~~");
        let result = tree.replace_visible_range(
            tree.visible_len()..tree.visible_len(),
            "456",
            tree.attributes_for_insertion_at(tree.visible_len()),
        );
        assert_eq!(result.tree.serialize_markdown(), "~~123~~456");
    }

    #[test]
    fn insertion_at_outer_start_of_terminal_strikethrough_is_plain_text() {
        let tree = InlineTextTree::from_markdown("~~123~~");
        let result = tree.replace_visible_range(0..0, "0", tree.attributes_for_insertion_at(0));
        assert_eq!(result.tree.serialize_markdown(), "0~~123~~");
    }

    #[test]
    fn serializes_partial_underline_removal_without_ambiguous_star_runs() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_bold(1..4));
        assert!(tree.toggle_underline(1..4));
        assert!(tree.toggle_italic(1..4));
        assert!(tree.toggle_underline(2..4));

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "1**<u>*2*</u>*34***");
        assert!(!serialized.contains("*****34"));
        assert_eq!(reparsed.visible_text(), "1234");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn parses_inline_links_autolinks_and_preserves_other_unsupported_inline_syntax() {
        let markdown =
            "[link](http://example.com) ![alt](/img.png) <http://example.com/> <span>x</span>";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(
            tree.visible_text(),
            "link ![alt](/img.png) http://example.com/ <span>x</span>"
        );
        assert_eq!(tree.render_cache().link_at(0), Some("http://example.com"));
        assert_eq!(
            tree.render_cache().link_at("link ![alt](/img.png) ".len()),
            Some("http://example.com/")
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_dollar_inline_math_as_source_preserving_fragment() {
        let markdown = "before $x^2$ after";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let math_start = "before ".len();
        let math = cache
            .inline_math_at(math_start)
            .expect("inline math span should be recorded");

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(math.source, "$x^2$");
        assert_eq!(math.body, "x^2");
        assert_eq!(math.delimiter, InlineMathDelimiter::Dollar);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_paren_inline_math_as_source_preserving_fragment() {
        let markdown = "before \\(\\frac{1}{2}\\) after";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let math_start = "before ".len();
        let math = cache
            .inline_math_at(math_start)
            .expect("inline math span should be recorded");

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(math.source, "\\(\\frac{1}{2}\\)");
        assert_eq!(math.body, "\\frac{1}{2}");
        assert_eq!(math.delimiter, InlineMathDelimiter::Paren);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn rejects_conservative_inline_math_cases() {
        for markdown in ["\\$x$", "$ x $", "$", "$x\ny$", "cost $12$"] {
            let tree = InlineTextTree::from_markdown(markdown);
            assert!(
                tree.render_cache()
                    .spans()
                    .iter()
                    .all(|span| span.math.is_none()),
                "{markdown:?} should stay plain text"
            );
        }
    }

    #[test]
    fn inline_math_does_not_parse_inside_code_spans() {
        let tree = InlineTextTree::from_markdown("`$x$` and $y$");
        let cache = tree.render_cache();

        assert!(cache.style_at(0).code);
        assert!(cache.inline_math_at(0).is_none());
        assert!(cache.inline_math_at("$x$ and ".len()).is_some());
        assert_eq!(tree.serialize_markdown(), "`$x$` and $y$");
    }

    #[test]
    fn parses_inline_local_markdown_file_link_as_document_relative() {
        let markdown = "[方案设计](ai-chat-implementation.zh-CN.md)";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "方案设计");
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "ai-chat-implementation.zh-CN.md".to_string(),
                open_target: "ai-chat-implementation.zh-CN.md".to_string(),
                is_workspace_file: false,
                is_document_relative_file: true,
            })
        );
    }

    #[test]
    fn parses_inline_link_title_without_polluting_open_target() {
        let markdown = "[ABC](https://abc.com \"https://abc.com\")";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "ABC");
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "https://abc.com".to_string(),
                open_target: "https://abc.com".to_string(),
                is_workspace_file: false,
                is_document_relative_file: false,
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_span_style_as_inline_html_not_link() {
        let markdown = "留意<span style='color:blue;'>磁盘预留空间、系统环境变量</span>等问题";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let span_start = "留意".len();

        assert_eq!(tree.visible_text(), "留意磁盘预留空间、系统环境变量等问题");
        assert_eq!(cache.link_at(span_start), None);
        assert!(matches!(
            cache.html_style_at(span_start).and_then(|style| style.color),
            Some(HtmlCssColor::Rgba(color))
                if color.red == 0 && color.green == 0 && color.blue == 255
        ));
        assert_eq!(cache.html_style_at(0), None);
        assert_eq!(
            tree.serialize_markdown(),
            "留意<span style=\"color: rgba(0,0,255,1.000);\">磁盘预留空间、系统环境变量</span>等问题"
        );
    }

    #[test]
    fn inline_span_style_allows_nested_markdown_code() {
        let markdown = "<span style='color:blue;'>英伟达驱动`CUDA+cuDNN`</span>";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let code_start = "英伟达驱动".len();

        assert_eq!(tree.visible_text(), "英伟达驱动CUDA+cuDNN");
        assert!(cache.style_at(code_start).code);
        assert!(matches!(
            cache.html_style_at(code_start).and_then(|style| style.color),
            Some(HtmlCssColor::Rgba(color))
                if color.red == 0 && color.green == 0 && color.blue == 255
        ));

        let reparsed = InlineTextTree::from_markdown(&tree.serialize_markdown());
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn html_like_tags_are_not_autolinks_when_unsafe_or_unclosed() {
        let unclosed = InlineTextTree::from_markdown("<span style='color:blue;'>x");
        assert_eq!(unclosed.visible_text(), "<span style='color:blue;'>x");
        assert_eq!(unclosed.render_cache().link_at(0), None);

        let script = InlineTextTree::from_markdown("<script>alert(1)</script>");
        assert_eq!(script.visible_text(), "<script>alert(1)</script>");
        assert_eq!(script.render_cache().link_at(0), None);
    }

    #[test]
    fn parses_reference_style_links_with_definitions_and_preserves_syntax() {
        let markdown = "[reference link][ref-link]";
        let definitions =
            super::super::link::parse_link_reference_definitions("[ref-link]: https://example.com");
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "reference link");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.com"));
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_reference_style_links_with_generic_normalized_labels() {
        let markdown = "[reference link][Ref   Links]";
        let definitions = super::super::link::parse_link_reference_definitions(
            "[ref links]: https://example.com",
        );
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "reference link");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.com"));
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "Ref   Links".to_string(),
                open_target: "https://example.com".to_string(),
                is_workspace_file: false,
                is_document_relative_file: false,
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_collapsed_reference_style_links_with_definitions() {
        let markdown = "[collapsed reference][]";
        let definitions = super::super::link::parse_link_reference_definitions(
            "[collapsed reference]: https://example.org",
        );
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "collapsed reference");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.org"));
        assert_eq!(
            tree.serialize_markdown(),
            "[collapsed reference][collapsed reference]"
        );
    }

    #[test]
    fn parses_shortcut_reference_style_links_with_definitions() {
        let markdown = "[shortcut reference]";
        let definitions = super::super::link::parse_link_reference_definitions(
            "[shortcut reference]: https://example.net",
        );
        let tree = InlineTextTree::from_markdown_with_link_references(markdown, &definitions);

        assert_eq!(tree.visible_text(), "shortcut reference");
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.net"));
        assert_eq!(
            tree.serialize_markdown(),
            "[shortcut reference][shortcut reference]"
        );
    }

    #[test]
    fn resolves_reference_link_examples_from_test_markdown() {
        let markdown = include_str!("../../../../test.md");
        let definitions = super::super::link::parse_link_reference_definitions(markdown);
        let tree = InlineTextTree::from_markdown_with_link_references(
            "[reference link][ref-link] [collapsed reference][] [shortcut reference]",
            &definitions,
        );

        assert_eq!(
            tree.visible_text(),
            "reference link collapsed reference shortcut reference"
        );
        assert_eq!(tree.render_cache().link_at(0), Some("https://example.com"));
        assert_eq!(
            tree.render_cache().link_at("reference link ".len()),
            Some("https://example.org")
        );
        assert_eq!(
            tree.render_cache()
                .link_at("reference link collapsed reference ".len()),
            Some("https://example.net")
        );
    }

    #[test]
    fn unresolved_reference_style_links_remain_literal_text() {
        let markdown = "[reference link][missing]";
        let tree = InlineTextTree::from_markdown_with_link_references(
            markdown,
            &LinkReferenceDefinitions::default(),
        );

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn unresolved_shortcut_reference_links_remain_literal_text() {
        let markdown = "[shortcut reference]";
        let tree = InlineTextTree::from_markdown_with_link_references(
            markdown,
            &LinkReferenceDefinitions::default(),
        );

        assert_eq!(tree.visible_text(), markdown);
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn shortcut_reference_detection_does_not_consume_images_as_links() {
        let definitions = super::super::link::parse_link_reference_definitions(
            "[alt]: https://example.com/not-an-image-link",
        );
        let tree = InlineTextTree::from_markdown_with_link_references("![alt]", &definitions);

        assert_eq!(tree.visible_text(), "![alt]");
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), "![alt]");
    }

    #[test]
    fn shortcut_reference_detection_does_not_rewrite_reference_images() {
        let definitions = super::super::link::parse_link_reference_definitions(
            "[img]: https://example.com/image.png",
        );
        let tree =
            InlineTextTree::from_markdown_with_link_references("![cover][img]", &definitions);

        assert_eq!(tree.visible_text(), "![cover][img]");
        assert_eq!(tree.render_cache().link_at(0), None);
        assert_eq!(tree.serialize_markdown(), "![cover][img]");
    }

    #[test]
    fn parses_mailto_autolinks_and_preserves_syntax() {
        let markdown = "<mailto:test@example.com>";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "mailto:test@example.com");
        assert_eq!(
            tree.render_cache().link_at(0),
            Some("mailto:test@example.com")
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_any_standalone_autolink_and_preserves_syntax() {
        let markdown = "<ref2>";
        let tree = InlineTextTree::from_markdown(markdown);

        assert_eq!(tree.visible_text(), "ref2");
        assert_eq!(tree.render_cache().link_at(0), Some("ref2"));
        assert_eq!(
            tree.render_cache().link_hit_at(0),
            Some(&InlineLinkHit {
                prompt_target: "ref2".to_string(),
                open_target: "ref2".to_string(),
                is_workspace_file: false,
                is_document_relative_file: false,
            })
        );
        assert_eq!(tree.serialize_markdown(), markdown);
    }

    #[test]
    fn parses_nested_inline_marks_inside_link_label() {
        let tree = InlineTextTree::from_markdown("[**go** now](https://example.com)");
        let cache = tree.render_cache();

        assert_eq!(tree.visible_text(), "go now");
        assert_eq!(cache.link_at(0), Some("https://example.com"));
        assert!(cache.style_at(0).bold);
        assert_eq!(
            tree.serialize_markdown(),
            "[**go** now](https://example.com)"
        );
    }

    #[test]
    fn serializes_partial_bold_removal_without_ambiguous_star_runs() {
        let mut tree = InlineTextTree::plain("1234");
        assert!(tree.toggle_bold(1..4));
        assert!(tree.toggle_italic(1..4));
        assert!(tree.toggle_bold(2..4));

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "1***2***<em>34</em>");
        assert_eq!(reparsed.visible_text(), "1234");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    // --- inline code tests ---

    #[test]
    fn parses_backtick_as_code_style() {
        let tree = InlineTextTree::from_markdown("a `code` b");
        let cache = tree.render_cache();

        assert_eq!(cache.visible_text(), "a code b");
        // "code" at offset 2 should have code style
        let style = cache.style_at(2);
        assert!(style.code, "expected code=true at offset 2");
        assert!(!style.bold);
    }

    #[test]
    fn backtick_content_preserves_markers_as_literal() {
        // Inside a code span, ** and * are literal, not parsed as bold/italic.
        let tree = InlineTextTree::from_markdown("`**not bold**`");
        let cache = tree.render_cache();

        assert_eq!(cache.visible_text(), "**not bold**");
        let style = cache.style_at(0);
        assert!(style.code);
        assert!(!style.bold);
        assert!(!style.italic);
    }

    #[test]
    fn unclosed_backtick_is_literal() {
        let tree = InlineTextTree::from_markdown("a `b");
        assert_eq!(tree.visible_text(), "a `b");
        assert_eq!(tree.serialize_markdown(), "a \\`b");
    }

    #[test]
    fn toggle_code_on_selection() {
        let mut tree = InlineTextTree::plain("hello world");
        assert!(tree.toggle_code(0..5)); // "hello"
        assert_eq!(tree.serialize_markdown(), "`hello` world");
    }

    #[test]
    fn toggle_code_twice_removes_code() {
        let mut tree = InlineTextTree::plain("hello world");
        assert!(tree.toggle_code(0..5));
        assert!(tree.toggle_code(0..5)); // toggle back
        assert_eq!(tree.serialize_markdown(), "hello world");
    }

    #[test]
    fn code_round_trips_through_serialization() {
        let tree = InlineTextTree::from_markdown("a `code` b");
        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(serialized, "a `code` b");
        assert_eq!(reparsed.visible_text(), "a code b");
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn code_inside_bold_text() {
        // `**bold `code` more**` — bold wraps around a code span.
        let tree = InlineTextTree::from_markdown("**bold `code` more**");
        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);

        assert_eq!(tree.visible_text(), "bold code more");
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
    }

    #[test]
    fn consecutive_backticks_treated_as_literal() {
        // Per CommonMark: a backtick run that has no matching closing run
        // is treated as literal text.
        let tree = InlineTextTree::from_markdown("``");
        // Two backticks with no closing -> literal (run_len=2, no matching close).
        assert_eq!(tree.visible_text(), "``");
        assert!(!tree.render_cache().style_at(0).code);
    }

    #[test]
    fn variable_length_backtick_run() {
        // `` `` `x` ``` `` (run_len=1 with 'x', matching close of run_len=1)
        let tree = InlineTextTree::from_markdown("`x`");
        assert_eq!(tree.visible_text(), "x");
        assert!(tree.render_cache().style_at(0).code);

        // ``` `` `` `` `` (run_len=2, content "a", run_len=2 close)
        let tree2 = InlineTextTree::from_markdown("``a``");
        assert_eq!(tree2.visible_text(), "a");
        assert!(tree2.render_cache().style_at(0).code);
    }

    #[test]
    fn code_span_content_normalization() {
        // Leading/trailing single space is stripped.
        let tree = InlineTextTree::from_markdown("` hello `");
        assert_eq!(tree.visible_text(), "hello");
        assert!(tree.render_cache().style_at(0).code);

        // All-space content is preserved (no stripping per spec).
        let tree2 = InlineTextTree::from_markdown("`   `");
        assert_eq!(tree2.visible_text(), "   ");
    }

    #[test]
    fn code_span_newline_is_preserved_as_hard_line() {
        let tree = InlineTextTree::from_markdown("`a\nb`");
        assert_eq!(tree.visible_text(), "a\nb");

        let cache = tree.render_cache();
        assert_eq!(cache.spans().len(), 1);
        assert_eq!(cache.spans()[0].range, 0..3);
        assert!(cache.spans()[0].style.code);
        assert_eq!(tree.serialize_markdown(), "`a\nb`");
    }

    #[test]
    fn code_span_blank_line_stays_inside_single_code_span() {
        let tree = InlineTextTree::from_markdown("`line 1\n\nline 2`");
        assert_eq!(tree.visible_text(), "line 1\n\nline 2");

        let cache = tree.render_cache();
        assert_eq!(cache.spans().len(), 1);
        assert_eq!(cache.spans()[0].range, 0.."line 1\n\nline 2".len());
        assert!(cache.spans()[0].style.code);
        assert_eq!(tree.serialize_markdown(), "`line 1\n\nline 2`");
    }

    #[test]
    fn code_span_content_keeps_inline_markers_literal() {
        let tree = InlineTextTree::from_markdown("`*[x] [link](x) \\\\`");

        assert_eq!(tree.visible_text(), "*[x] [link](x) \\\\");
        let cache = tree.render_cache();
        assert_eq!(cache.spans().len(), 1);
        assert!(cache.spans()[0].style.code);
        assert!(cache.spans()[0].link.is_none());
        assert!(!cache.spans()[0].style.bold);
        assert!(!cache.spans()[0].style.italic);
    }

    #[test]
    fn parses_literal_backtick_runs_with_unambiguous_delimiters() {
        let markdown = "`` ` `` and ``` `` ``` and ```` ``` ````";
        let tree = InlineTextTree::from_markdown(markdown);
        let cache = tree.render_cache();
        let code_ranges = cache
            .spans()
            .iter()
            .filter(|span| span.style.code)
            .map(|span| span.range.clone())
            .collect::<Vec<_>>();

        assert_eq!(tree.visible_text(), "` and `` and ```");
        assert_eq!(code_ranges, vec![0..1, 6..8, 13..16]);
        assert!(!cache.style_at("` ".len()).code);
        assert!(!cache.style_at("` and `` ".len()).code);

        let serialized = tree.serialize_markdown();
        let reparsed = InlineTextTree::from_markdown(&serialized);
        assert_eq!(reparsed.visible_text(), tree.visible_text());
        assert_eq!(reparsed.render_cache().spans(), cache.spans());
    }

    #[test]
    fn serializes_code_spans_with_safe_backtick_delimiters_and_padding() {
        for text in [" leading", "trailing ", "`tick", "tick`", "`", "``", "   "] {
            let tree = InlineTextTree::from_fragments(vec![InlineFragment {
                text: text.to_string(),
                style: InlineStyle {
                    code: true,
                    ..InlineStyle::default()
                },
                html_style: None,
                link: None,
                footnote: None,
                math: None,
                emoji: None,
                tag: None,
            }]);
            let serialized = tree.serialize_markdown();
            let reparsed = InlineTextTree::from_markdown(&serialized);

            assert_eq!(
                reparsed.visible_text(),
                text,
                "serialized as {serialized:?}"
            );
            assert_eq!(reparsed.render_cache().spans(), tree.render_cache().spans());
        }
    }

    #[test]
    fn source_to_rendered_round_trip_preserves_code_span() {
        // Simulate Source -> Rendered: raw markdown -> from_markdown parses it.
        let raw = "`123`";
        let tree = InlineTextTree::from_markdown(raw);
        assert_eq!(tree.visible_text(), "123");
        assert!(tree.render_cache().style_at(0).code);

        // Serialize back: must produce valid markdown.
        let serialized = tree.serialize_markdown();
        assert_eq!(serialized, "`123`");

        // Re-parse: must produce same result.
        let reparsed = InlineTextTree::from_markdown(&serialized);
        assert_eq!(reparsed.visible_text(), "123");
        assert!(reparsed.render_cache().style_at(0).code);
    }

    #[test]
    fn raw_text_with_backticks_not_double_escaped() {
        // Simulate the Source block's display_text() path.
        let raw = "`123`";
        // display_text() returns raw text as-is; from_markdown re-parses.
        let parsed = InlineTextTree::from_markdown(raw);
        assert_eq!(parsed.visible_text(), "123");

        // A second round-trip should NOT escape or double the backticks.
        let serialized = parsed.serialize_markdown();
        assert_eq!(serialized, "`123`");
        let reparsed = InlineTextTree::from_markdown(&serialized);
        assert_eq!(reparsed.visible_text(), "123");
    }

    #[test]
    fn escaped_backtick_in_code() {
        let tree = InlineTextTree::from_markdown("\\`not code\\`");
        assert_eq!(tree.visible_text(), "`not code`");
        // Escaped backticks are literal, not code delimiters.
        let cache = tree.render_cache();
        assert!(!cache.style_at(0).code);
        assert_eq!(tree.serialize_markdown(), "\\`not code\\`");
    }

    #[test]
    fn parses_highlight_syntax() {
        let tree = InlineTextTree::from_markdown("before ==highlighted== after");
        assert_eq!(tree.visible_text(), "before highlighted after");
        assert!(tree.render_cache().style_at("before ".len()).highlight);
        assert_eq!(tree.serialize_markdown(), "before ==highlighted== after");
    }

    #[test]
    fn parses_emoji_shortcodes() {
        let tree = InlineTextTree::from_markdown(":smile: :+1: :rocket: :cn:");
        assert_eq!(tree.visible_text(), "😄 👍 🚀 🇨🇳");
        assert_eq!(tree.serialize_markdown(), ":smile: :+1: :rocket: :cn:");
    }

    #[test]
    fn unknown_emoji_shortcodes_stay_literal() {
        let tree = InlineTextTree::from_markdown(":unknownemoji:");
        assert_eq!(tree.visible_text(), ":unknownemoji:");
    }
}
