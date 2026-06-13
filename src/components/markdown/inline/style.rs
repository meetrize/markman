//! Inline styles.



/// Bitfield of active inline formatting flags for a span of text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub highlight: bool,
    pub script: InlineScript,
}

/// Vertical script style for simple Markdown extension syntax.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineScript {
    #[default]
    Normal,
    Superscript,
    Subscript,
}

impl InlineStyle {
    pub fn with_bold(self) -> Self {
        Self { bold: true, ..self }
    }

    pub fn with_italic(self) -> Self {
        Self {
            italic: true,
            ..self
        }
    }

    pub fn with_underline(self) -> Self {
        Self {
            underline: true,
            ..self
        }
    }

    pub fn with_strikethrough(self) -> Self {
        Self {
            strikethrough: true,
            ..self
        }
    }

    pub fn with_code(self) -> Self {
        Self { code: true, ..self }
    }

    pub fn with_highlight(self) -> Self {
        Self {
            highlight: true,
            ..self
        }
    }

    pub fn with_superscript(self) -> Self {
        Self {
            script: InlineScript::Superscript,
            ..self
        }
    }

    pub fn with_subscript(self) -> Self {
        Self {
            script: InlineScript::Subscript,
            ..self
        }
    }

    pub fn has_script(self) -> bool {
        self.script != InlineScript::Normal
    }

    pub(crate) fn apply(self, delimiter: super::delimiter::Delimiter) -> Self {
        match delimiter {
            super::delimiter::Delimiter::BoldMarkdown { .. } | super::delimiter::Delimiter::BoldHtml => self.with_bold(),
            super::delimiter::Delimiter::ItalicMarkdown { .. } | super::delimiter::Delimiter::ItalicHtml => self.with_italic(),
            super::delimiter::Delimiter::Underline => self.with_underline(),
            super::delimiter::Delimiter::StrikethroughMarkdown => self.with_strikethrough(),
            super::delimiter::Delimiter::CodeMarkdown { .. } => self.with_code(),
            super::delimiter::Delimiter::HighlightMarkdown | super::delimiter::Delimiter::HighlightHtml => {
                self.with_highlight()
            }
            super::delimiter::Delimiter::SuperscriptMarkdown | super::delimiter::Delimiter::SuperscriptHtml => self.with_superscript(),
            super::delimiter::Delimiter::SubscriptMarkdown | super::delimiter::Delimiter::SubscriptHtml => self.with_subscript(),
        }
    }
}

/// Inline style flag addressable by editing commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StyleFlag {
    /// Bold text.
    Bold,
    /// Italic text.
    Italic,
    /// Underlined text.
    Underline,
    /// Strikethrough text.
    Strikethrough,
    /// Inline code text.
    Code,
    /// Highlighted text.
    Highlight,
    /// Superscript text.
    Superscript,
    /// Subscript text.
    Subscript,
}
pub(crate) fn style_flag_enabled(style: InlineStyle, flag: StyleFlag) -> bool {
    match flag {
        StyleFlag::Bold => style.bold,
        StyleFlag::Italic => style.italic,
        StyleFlag::Underline => style.underline,
        StyleFlag::Strikethrough => style.strikethrough,
        StyleFlag::Code => style.code,
        StyleFlag::Highlight => style.highlight,
        StyleFlag::Superscript => style.script == InlineScript::Superscript,
        StyleFlag::Subscript => style.script == InlineScript::Subscript,
    }
}

pub(crate) fn set_style_flag(mut style: InlineStyle, flag: StyleFlag, enabled: bool) -> InlineStyle {
    match flag {
        StyleFlag::Bold => style.bold = enabled,
        StyleFlag::Italic => style.italic = enabled,
        StyleFlag::Underline => style.underline = enabled,
        StyleFlag::Strikethrough => style.strikethrough = enabled,
        StyleFlag::Code => style.code = enabled,
        StyleFlag::Highlight => style.highlight = enabled,
        StyleFlag::Superscript => {
            style.script = if enabled {
                InlineScript::Superscript
            } else if style.script == InlineScript::Superscript {
                InlineScript::Normal
            } else {
                style.script
            }
        }
        StyleFlag::Subscript => {
            style.script = if enabled {
                InlineScript::Subscript
            } else if style.script == InlineScript::Subscript {
                InlineScript::Normal
            } else {
                style.script
            }
        }
    }
    style
}
