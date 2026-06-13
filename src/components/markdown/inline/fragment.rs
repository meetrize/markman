//! Fragment types.

use std::ops::Range;

use crate::components::markdown::footnote::{
    InlineFootnoteHit, InlineFootnoteReference,
};
use crate::components::markdown::html::HtmlInlineStyle;

use super::style::InlineStyle;

/// A contiguous run of text with a uniform [`InlineStyle`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineFragment {
    pub text: String,
    pub style: InlineStyle,
    pub html_style: Option<HtmlInlineStyle>,
    pub link: Option<InlineLink>,
    pub footnote: Option<InlineFootnoteReference>,
    pub math: Option<InlineMath>,
}

/// Source-preserving inline LaTeX math metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineMath {
    /// Full Markdown source, including `$...$` or `\(...\)` delimiters.
    pub source: String,
    /// LaTeX body between the inline math delimiters.
    pub body: String,
    /// Delimiter form used by the source.
    pub delimiter: InlineMathDelimiter,
}

/// Supported inline math delimiter syntaxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineMathDelimiter {
    /// Dollar-delimited inline math: `$...$`.
    Dollar,
    /// Parenthesis-delimited inline math: `\(...\)`.
    Paren,
}

/// Link metadata attached to a formatted inline text fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineLink {
    /// Inline destination and optional title from `[label](destination "title")`.
    Inline {
        destination: String,
        title: Option<String>,
    },
    /// Reference-style link resolved from `[label][ref]`-style syntax.
    Reference { label: String, destination: String },
    /// Autolink target from `<scheme:target>` or email-like syntax.
    Autolink { target: String },
}

/// Link target pair used by hit-testing and open-link prompts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineLinkHit {
    pub prompt_target: String,
    pub open_target: String,
}

/// A cursor inside the inline text tree.
///
/// `fragment_index` identifies the fragment and `byte_offset` addresses a byte
/// boundary inside that fragment's text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCursor {
    pub fragment_index: usize,
    pub byte_offset: usize,
}

/// A visible-text range with its associated [`InlineStyle`], used by
/// the render cache to build styled text runs for the text system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSpan {
    pub range: Range<usize>,
    pub style: InlineStyle,
    pub html_style: Option<HtmlInlineStyle>,
    pub link: Option<InlineLinkHit>,
    pub footnote: Option<InlineFootnoteHit>,
    pub math: Option<InlineMath>,
}

/// Fragment attributes inherited by inserted text at a caret position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineInsertionAttributes {
    pub style: InlineStyle,
    pub html_style: Option<HtmlInlineStyle>,
    pub link: Option<InlineLink>,
    pub footnote: Option<InlineFootnoteReference>,
    pub math: Option<InlineMath>,
}
