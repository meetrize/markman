//! Code-block syntax highlighting support.

use std::collections::HashMap;
use std::ops::Range;
#[cfg(feature = "code-highlight-core")]
use std::sync::LazyLock;

use gpui::Hsla;
#[cfg(feature = "code-highlight-core")]
use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

use crate::theme::ThemeColors;

/// Canonical language key used by the syntax-highlighting registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CodeLanguageKey {
    /// Rust source code.
    Rust,
    /// JavaScript without JSX.
    JavaScript,
    /// JavaScript with JSX syntax.
    JavaScriptJsx,
    /// TypeScript without TSX.
    TypeScript,
    /// TypeScript with TSX syntax.
    TypeScriptTsx,
    /// JSON data.
    Json,
    /// Markdown source.
    Markdown,
    /// POSIX-like shell scripts.
    Bash,
    /// C source code.
    C,
    /// C++ source code.
    Cpp,
    /// C# source code.
    CSharp,
    /// CSS stylesheets.
    Css,
    /// Go source code.
    Go,
    /// HTML markup.
    Html,
    /// Java source code.
    Java,
    /// PHP source code.
    Php,
    /// Python source code.
    Python,
    /// Ruby source code.
    Ruby,
    /// YAML configuration.
    Yaml,
    /// TOML configuration.
    Toml,
    /// Mermaid diagram source.
    Mermaid,
    /// Plain text or unknown language fallback.
    PlainText,
}

/// Semantic highlight classes mapped onto theme colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeHighlightClass {
    /// Comment text.
    Comment,
    /// Language keyword or reserved word.
    Keyword,
    /// String literal.
    String,
    /// Numeric literal.
    Number,
    /// Type name.
    Type,
    /// Function or callable identifier.
    Function,
    /// Constant identifier.
    Constant,
    /// Variable identifier.
    Variable,
    /// Object or record property.
    Property,
    /// Operator token.
    Operator,
    /// Punctuation token.
    Punctuation,
    /// Markdown ATX/setext H1 heading content.
    MarkdownHeading1,
    /// Markdown ATX/setext H2 heading content.
    MarkdownHeading2,
    /// Markdown ATX/setext H3 heading content.
    MarkdownHeading3,
    /// Markdown ATX/setext H4 heading content.
    MarkdownHeading4,
    /// Markdown ATX/setext H5 heading content.
    MarkdownHeading5,
    /// Markdown ATX/setext H6 heading content.
    MarkdownHeading6,
    /// Markdown emphasis / italic inline span.
    MarkdownEmphasis,
    /// Markdown strong / bold inline span.
    MarkdownStrong,
    /// Markdown inline code, code block, or literal text.
    MarkdownLiteral,
    /// Markdown link destination or autolink URI.
    MarkdownUri,
    /// Markdown link label or reference text.
    MarkdownReference,
}

/// Highlighted byte range inside a code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodeHighlightSpan {
    pub(crate) range: Range<usize>,
    pub(crate) class: CodeHighlightClass,
}

/// Highlight result cached on a code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodeHighlightResult {
    pub(crate) language: CodeLanguageKey,
    pub(crate) spans: Vec<CodeHighlightSpan>,
}

/// Language aliases accepted from fenced-code info strings.
#[derive(Clone, Copy)]
struct LanguageDescriptor {
    key: CodeLanguageKey,
    aliases: &'static [&'static str],
}

const LANGUAGE_DESCRIPTORS: &[LanguageDescriptor] = &[
    LanguageDescriptor {
        key: CodeLanguageKey::Rust,
        aliases: &["rust", "rs"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::JavaScript,
        aliases: &["javascript", "js"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::JavaScriptJsx,
        aliases: &["jsx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::TypeScript,
        aliases: &["typescript", "ts"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::TypeScriptTsx,
        aliases: &["tsx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Json,
        aliases: &["json"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Markdown,
        aliases: &["markdown", "md"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Bash,
        aliases: &["bash", "sh", "shell", "zsh"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::C,
        aliases: &["c", "h"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Cpp,
        aliases: &["cpp", "cxx", "cc", "hpp", "hxx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::CSharp,
        aliases: &["csharp", "cs", "c#"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Css,
        aliases: &["css"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Go,
        aliases: &["go", "golang"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Html,
        aliases: &["html"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Java,
        aliases: &["java"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Php,
        aliases: &["php"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Python,
        aliases: &["python", "py"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Ruby,
        aliases: &["ruby", "rb"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Yaml,
        aliases: &["yaml", "yml"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Toml,
        aliases: &["toml"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::PlainText,
        aliases: &["text", "txt", "plain"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Mermaid,
        aliases: &["mermaid"],
    },
];

const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
    "markdown.heading.1",
    "markdown.heading.2",
    "markdown.heading.3",
    "markdown.heading.4",
    "markdown.heading.5",
    "markdown.heading.6",
    "markdown.emphasis",
    "markdown.strong",
    "markdown.literal",
    "markdown.uri",
    "markdown.reference",
    "markdown.punctuation",
    "markdown.escape",
    "text.literal",
    "text.emphasis",
    "text.strong",
    "text.uri",
    "text.reference",
];

/// Block-level Markdown highlight query with per-level heading captures.
const MARKDOWN_BLOCK_HIGHLIGHTS_QUERY: &str = r#"
(atx_heading
  (atx_h1_marker)
  (inline) @markdown.heading.1)

(atx_heading
  (atx_h2_marker)
  (inline) @markdown.heading.2)

(atx_heading
  (atx_h3_marker)
  (inline) @markdown.heading.3)

(atx_heading
  (atx_h4_marker)
  (inline) @markdown.heading.4)

(atx_heading
  (atx_h5_marker)
  (inline) @markdown.heading.5)

(atx_heading
  (atx_h6_marker)
  (inline) @markdown.heading.6)

[
  (atx_h1_marker)
  (atx_h2_marker)
  (atx_h3_marker)
  (atx_h4_marker)
  (atx_h5_marker)
  (atx_h6_marker)
  (setext_h1_underline)
  (setext_h2_underline)
  (list_marker_plus)
  (list_marker_minus)
  (list_marker_star)
  (list_marker_dot)
  (list_marker_parenthesis)
  (thematic_break)
  (block_quote_marker)
  (fenced_code_block_delimiter)
] @markdown.punctuation

[
  (link_title)
  (indented_code_block)
  (fenced_code_block)
] @markdown.literal

(code_fence_content) @none

(link_destination) @markdown.uri

(link_label) @markdown.reference

(block_continuation) @markdown.punctuation

(backslash_escape) @markdown.escape
"#;

/// Lazily built tree-sitter highlighter registry.
#[cfg(feature = "code-highlight-core")]
struct CodeHighlightRegistry {
    configs: HashMap<CodeLanguageKey, HighlightConfiguration>,
    #[cfg(feature = "code-highlight-official")]
    markdown_inline: Option<HighlightConfiguration>,
}

#[cfg(feature = "code-highlight-core")]
static CODE_HIGHLIGHT_REGISTRY: LazyLock<CodeHighlightRegistry> =
    LazyLock::new(CodeHighlightRegistry::new);

#[cfg(feature = "code-highlight-core")]
impl CodeHighlightRegistry {
    fn new() -> Self {
        let mut configs = HashMap::new();
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Rust, build_rust_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::JavaScript,
            build_javascript_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::JavaScriptJsx,
            build_jsx_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::TypeScript,
            build_typescript_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::TypeScriptTsx,
            build_tsx_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Json, build_json_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::Markdown,
            build_markdown_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Bash, build_bash_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::C, build_c_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Cpp, build_cpp_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::CSharp, build_csharp_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Css, build_css_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Go, build_go_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Html, build_html_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Java, build_java_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Php, build_php_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Python, build_python_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Ruby, build_ruby_config());
        #[cfg(feature = "code-highlight-config")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Yaml, build_yaml_config());
        #[cfg(feature = "code-highlight-config")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Toml, build_toml_config());
        Self {
            configs,
            #[cfg(feature = "code-highlight-official")]
            markdown_inline: build_markdown_inline_config(),
        }
    }

    fn config_for(&self, key: CodeLanguageKey) -> Option<&HighlightConfiguration> {
        self.configs.get(&key)
    }

    fn config_for_injection(&self, language_name: &str) -> Option<&HighlightConfiguration> {
        if language_name == "markdown_inline" {
            #[cfg(feature = "code-highlight-official")]
            {
                return self.markdown_inline.as_ref();
            }
            #[cfg(not(feature = "code-highlight-official"))]
            {
                return None;
            }
        }

        if let Some(key) = resolve_code_language_key(Some(language_name)) {
            return self.config_for(key);
        }

        match language_name {
            "c_sharp" => self.config_for(CodeLanguageKey::CSharp),
            _ => None,
        }
    }
}

#[cfg(feature = "code-highlight-core")]
fn maybe_insert_config(
    configs: &mut HashMap<CodeLanguageKey, HighlightConfiguration>,
    key: CodeLanguageKey,
    config: Option<HighlightConfiguration>,
) {
    if let Some(config) = config {
        configs.insert(key, config);
    }
}

#[cfg(feature = "code-highlight-core")]
fn configure_highlights(
    language: tree_sitter::Language,
    name: &'static str,
    highlights_query: &str,
    injections_query: &str,
    locals_query: &str,
) -> Option<HighlightConfiguration> {
    let mut config = HighlightConfiguration::new(
        language,
        name,
        highlights_query,
        injections_query,
        locals_query,
    )
    .ok()?;
    config.configure(HIGHLIGHT_NAMES);
    Some(config)
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
fn build_rust_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
}

fn build_javascript_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
}

fn build_jsx_config() -> Option<HighlightConfiguration> {
    let query = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    configure_highlights(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        &query,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
}

fn build_typescript_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "typescript",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
}

fn build_tsx_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "tsx",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
}

fn build_json_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_json::LANGUAGE.into(),
        "json",
        tree_sitter_json::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_markdown_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_md::LANGUAGE.into(),
        "markdown",
        MARKDOWN_BLOCK_HIGHLIGHTS_QUERY,
        tree_sitter_md::INJECTION_QUERY_BLOCK,
        "",
    )
}

fn build_markdown_inline_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_md::INLINE_LANGUAGE.into(),
        "markdown_inline",
        tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
        tree_sitter_md::INJECTION_QUERY_INLINE,
        "",
    )
}

fn build_bash_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_bash::LANGUAGE.into(),
        "bash",
        tree_sitter_bash::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_c_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_c::LANGUAGE.into(),
        "c",
        tree_sitter_c::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_cpp_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_cpp::LANGUAGE.into(),
        "cpp",
        tree_sitter_cpp::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_csharp_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_c_sharp::LANGUAGE.into(),
        "c_sharp",
        tree_sitter_c_sharp::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_css_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_css::LANGUAGE.into(),
        "css",
        tree_sitter_css::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_go_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_go::LANGUAGE.into(),
        "go",
        tree_sitter_go::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_html_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_html::LANGUAGE.into(),
        "html",
        tree_sitter_html::HIGHLIGHTS_QUERY,
        tree_sitter_html::INJECTIONS_QUERY,
        "",
    )
}

fn build_java_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_java::LANGUAGE.into(),
        "java",
        tree_sitter_java::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_php_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_php::LANGUAGE_PHP.into(),
        "php",
        tree_sitter_php::HIGHLIGHTS_QUERY,
        tree_sitter_php::INJECTIONS_QUERY,
        "",
    )
}

fn build_python_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_python::LANGUAGE.into(),
        "python",
        tree_sitter_python::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_ruby_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_ruby::LANGUAGE.into(),
        "ruby",
        tree_sitter_ruby::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_ruby::LOCALS_QUERY,
    )
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-config"))]
fn build_yaml_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_yaml::LANGUAGE.into(),
        "yaml",
        tree_sitter_yaml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-config"))]
fn build_toml_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_toml::LANGUAGE.into(),
        "toml",
        tree_sitter_toml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn descriptor_for_language(language: &str) -> Option<&'static LanguageDescriptor> {
    LANGUAGE_DESCRIPTORS.iter().find(|descriptor| {
        descriptor
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(language))
    })
}

pub(crate) fn resolve_code_language_key(language: Option<&str>) -> Option<CodeLanguageKey> {
    let normalized = language?
        .split_whitespace()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    descriptor_for_language(normalized).map(|descriptor| descriptor.key)
}

pub(crate) fn highlight_code_block(
    language: Option<&str>,
    source: &str,
) -> Option<CodeHighlightResult> {
    let key = resolve_code_language_key(language)?;

    #[cfg(feature = "code-highlight-core")]
    if let Some(config) = CODE_HIGHLIGHT_REGISTRY.config_for(key) {
        let registry = &*CODE_HIGHLIGHT_REGISTRY;
        let mut highlighter = Highlighter::new();
        let events = match highlighter.highlight(
            config,
            source.as_bytes(),
            None,
            |language_name| registry.config_for_injection(language_name),
        ) {
            Ok(events) => events,
            Err(_) => {
                return Some(CodeHighlightResult {
                    language: key,
                    spans: Vec::new(),
                });
            }
        };

        let mut spans = Vec::new();
        let mut active = Vec::new();
        for event in events {
            let Ok(event) = event else {
                return Some(CodeHighlightResult {
                    language: key,
                    spans: Vec::new(),
                });
            };

            match event {
                HighlightEvent::Source { start, end } => {
                    if let Some(class) = active.last().copied() {
                        push_highlight_span(&mut spans, start..end, class);
                    }
                }
                HighlightEvent::HighlightStart(highlight) => {
                    if let Some(class) = class_for_highlight(highlight) {
                        active.push(class);
                    }
                }
                HighlightEvent::HighlightEnd => {
                    active.pop();
                }
            }
        }

        if key == CodeLanguageKey::Markdown {
            if let Some(inline_config) = registry.config_for_injection("markdown_inline") {
                augment_markdown_inline_spans(source, inline_config, &mut spans);
            }
        }

        return Some(CodeHighlightResult {
            language: key,
            spans,
        });
    }

    Some(CodeHighlightResult {
        language: key,
        spans: Vec::new(),
    })
}

fn push_highlight_span(
    spans: &mut Vec<CodeHighlightSpan>,
    range: Range<usize>,
    class: CodeHighlightClass,
) {
    if range.start >= range.end {
        return;
    }

    if let Some(last) = spans.last_mut()
        && last.class == class
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }

    spans.push(CodeHighlightSpan { range, class });
}

/// Prefer the shortest highlight span covering `offset` so nested Markdown marks win.
pub(crate) fn code_highlight_class_at(
    spans: &[CodeHighlightSpan],
    offset: usize,
) -> Option<CodeHighlightClass> {
    spans
        .iter()
        .filter(|span| span.range.start <= offset && offset < span.range.end)
        .min_by_key(|span| span.range.len())
        .map(|span| span.class)
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
fn append_config_highlight_spans(
    config: &HighlightConfiguration,
    source: &str,
    byte_offset: usize,
    spans: &mut Vec<CodeHighlightSpan>,
) {
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(config, source.as_bytes(), None, |_| None) else {
        return;
    };

    let mut active = Vec::new();
    for event in events {
        let Ok(event) = event else {
            return;
        };
        match event {
            HighlightEvent::Source { start, end } => {
                if let Some(class) = active.last().copied() {
                    push_highlight_span(spans, byte_offset + start..byte_offset + end, class);
                }
            }
            HighlightEvent::HighlightStart(highlight) => {
                if let Some(class) = class_for_highlight(highlight) {
                    active.push(class);
                }
            }
            HighlightEvent::HighlightEnd => {
                active.pop();
            }
        }
    }
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
fn augment_markdown_inline_spans(
    source: &str,
    inline_config: &HighlightConfiguration,
    spans: &mut Vec<CodeHighlightSpan>,
) {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_md::LANGUAGE.into())
        .is_err()
    {
        return;
    }
    let Some(tree) = parser.parse(source, None) else {
        return;
    };

    collect_markdown_inline_spans(
        tree.root_node(),
        source,
        inline_config,
        spans,
    );
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
fn collect_markdown_inline_spans(
    node: tree_sitter::Node<'_>,
    source: &str,
    inline_config: &HighlightConfiguration,
    spans: &mut Vec<CodeHighlightSpan>,
) {
    if node.kind() == "inline" {
        let start = node.start_byte();
        let end = node.end_byte();
        if start < end && end <= source.len() {
            append_config_highlight_spans(
                inline_config,
                &source[start..end],
                start,
                spans,
            );
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_markdown_inline_spans(child, source, inline_config, spans);
    }
}

#[cfg(feature = "code-highlight-core")]
fn class_for_highlight(highlight: Highlight) -> Option<CodeHighlightClass> {
    let name = HIGHLIGHT_NAMES.get(highlight.0)?;
    Some(match *name {
        "comment" => CodeHighlightClass::Comment,
        "keyword" | "tag" => CodeHighlightClass::Keyword,
        "string" | "string.special" | "embedded" => CodeHighlightClass::String,
        "number" => CodeHighlightClass::Number,
        "type" | "type.builtin" | "module" => CodeHighlightClass::Type,
        "function" | "function.builtin" | "constructor" => CodeHighlightClass::Function,
        "constant" | "constant.builtin" => CodeHighlightClass::Constant,
        "variable" | "variable.builtin" | "variable.parameter" => CodeHighlightClass::Variable,
        "property" | "property.builtin" | "attribute" => CodeHighlightClass::Property,
        "operator" => CodeHighlightClass::Operator,
        "punctuation" | "punctuation.bracket" | "punctuation.delimiter" | "punctuation.special"
        | "markdown.punctuation" => CodeHighlightClass::Punctuation,
        "markdown.heading.1" => CodeHighlightClass::MarkdownHeading1,
        "markdown.heading.2" => CodeHighlightClass::MarkdownHeading2,
        "markdown.heading.3" => CodeHighlightClass::MarkdownHeading3,
        "markdown.heading.4" => CodeHighlightClass::MarkdownHeading4,
        "markdown.heading.5" => CodeHighlightClass::MarkdownHeading5,
        "markdown.heading.6" => CodeHighlightClass::MarkdownHeading6,
        "markdown.emphasis" => CodeHighlightClass::MarkdownEmphasis,
        "markdown.strong" => CodeHighlightClass::MarkdownStrong,
        "markdown.literal" => CodeHighlightClass::MarkdownLiteral,
        "markdown.uri" => CodeHighlightClass::MarkdownUri,
        "markdown.reference" | "text.reference" => CodeHighlightClass::MarkdownReference,
        "markdown.escape" | "string.escape" => CodeHighlightClass::String,
        "text.literal" => CodeHighlightClass::MarkdownLiteral,
        "text.emphasis" => CodeHighlightClass::MarkdownEmphasis,
        "text.strong" => CodeHighlightClass::MarkdownStrong,
        "text.uri" => CodeHighlightClass::MarkdownUri,
        _ => return None,
    })
}

pub(crate) fn code_highlight_color(colors: &ThemeColors, class: CodeHighlightClass) -> Hsla {
    match class {
        CodeHighlightClass::Comment => colors.code_syntax_comment,
        CodeHighlightClass::Keyword => colors.code_syntax_keyword,
        CodeHighlightClass::String => colors.code_syntax_string,
        CodeHighlightClass::Number => colors.code_syntax_number,
        CodeHighlightClass::Type => colors.code_syntax_type,
        CodeHighlightClass::Function => colors.code_syntax_function,
        CodeHighlightClass::Constant => colors.code_syntax_constant,
        CodeHighlightClass::Variable => colors.code_syntax_variable,
        CodeHighlightClass::Property => colors.code_syntax_property,
        CodeHighlightClass::Operator => colors.code_syntax_operator,
        CodeHighlightClass::Punctuation => colors.code_syntax_punctuation,
        CodeHighlightClass::MarkdownHeading1 => colors.code_syntax_constant,
        CodeHighlightClass::MarkdownHeading2 => colors.code_syntax_function,
        CodeHighlightClass::MarkdownHeading3 => colors.code_syntax_type,
        CodeHighlightClass::MarkdownHeading4 => colors.code_syntax_keyword,
        CodeHighlightClass::MarkdownHeading5 => colors.code_syntax_string,
        CodeHighlightClass::MarkdownHeading6 => colors.code_syntax_comment,
        CodeHighlightClass::MarkdownEmphasis => colors.text_quote,
        CodeHighlightClass::MarkdownStrong => colors.code_syntax_constant,
        CodeHighlightClass::MarkdownLiteral => colors.code_syntax_string,
        CodeHighlightClass::MarkdownUri => colors.text_link,
        CodeHighlightClass::MarkdownReference => colors.code_syntax_type,
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeLanguageKey, highlight_code_block, resolve_code_language_key};

    #[test]
    fn balanced_bundle_aliases_resolve_to_expected_keys() {
        assert_eq!(
            resolve_code_language_key(Some("rust")),
            Some(CodeLanguageKey::Rust)
        );
        assert_eq!(
            resolve_code_language_key(Some("js")),
            Some(CodeLanguageKey::JavaScript)
        );
        assert_eq!(
            resolve_code_language_key(Some("jsx")),
            Some(CodeLanguageKey::JavaScriptJsx)
        );
        assert_eq!(
            resolve_code_language_key(Some("ts")),
            Some(CodeLanguageKey::TypeScript)
        );
        assert_eq!(
            resolve_code_language_key(Some("tsx")),
            Some(CodeLanguageKey::TypeScriptTsx)
        );
        assert_eq!(
            resolve_code_language_key(Some("sh")),
            Some(CodeLanguageKey::Bash)
        );
        assert_eq!(
            resolve_code_language_key(Some("hpp")),
            Some(CodeLanguageKey::Cpp)
        );
        assert_eq!(
            resolve_code_language_key(Some("c#")),
            Some(CodeLanguageKey::CSharp)
        );
        assert_eq!(
            resolve_code_language_key(Some("golang")),
            Some(CodeLanguageKey::Go)
        );
        assert_eq!(
            resolve_code_language_key(Some("py")),
            Some(CodeLanguageKey::Python)
        );
        assert_eq!(
            resolve_code_language_key(Some("rb")),
            Some(CodeLanguageKey::Ruby)
        );
        assert_eq!(
            resolve_code_language_key(Some("yml")),
            Some(CodeLanguageKey::Yaml)
        );
        assert_eq!(
            resolve_code_language_key(Some("plain")),
            Some(CodeLanguageKey::PlainText)
        );
        assert_eq!(
            resolve_code_language_key(Some("mermaid")),
            Some(CodeLanguageKey::Mermaid)
        );
        assert_eq!(resolve_code_language_key(Some("unknown")), None);
    }

    #[test]
    fn plain_fallback_languages_produce_empty_spans() {
        let mermaid = highlight_code_block(Some("mermaid"), "graph TD;\nA-->B")
            .expect("known plain fallback should still produce a result");
        assert_eq!(mermaid.language, CodeLanguageKey::Mermaid);
        assert!(mermaid.spans.is_empty());

        let text = highlight_code_block(Some("text"), "just text")
            .expect("plain text should still produce a result");
        assert_eq!(text.language, CodeLanguageKey::PlainText);
        assert!(text.spans.is_empty());
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn markdown_highlight_configuration_builds() {
        use tree_sitter_highlight::HighlightConfiguration;

        match HighlightConfiguration::new(
            tree_sitter_md::LANGUAGE.into(),
            "markdown",
            super::MARKDOWN_BLOCK_HIGHLIGHTS_QUERY,
            tree_sitter_md::INJECTION_QUERY_BLOCK,
            "",
        ) {
            Ok(_) => {}
            Err(error) => panic!("markdown block highlight query failed: {error}"),
        }
        match HighlightConfiguration::new(
            tree_sitter_md::INLINE_LANGUAGE.into(),
            "markdown_inline",
            tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
            tree_sitter_md::INJECTION_QUERY_INLINE,
            "",
        ) {
            Ok(_) => {}
            Err(error) => panic!("markdown inline highlight query failed: {error}"),
        }
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn markdown_fenced_code_block_has_nested_highlight_spans() {
        use super::{CodeHighlightClass, CodeLanguageKey};

        let source = "# Title\n\n```rust\nfn main() {}\n```\n";
        let result = highlight_code_block(Some("markdown"), source)
            .expect("markdown should produce a result");
        assert_eq!(result.language, CodeLanguageKey::Markdown);
        let has_keyword = result.spans.iter().any(|span| {
            span.class == CodeHighlightClass::Keyword
                && source[span.range.clone()].contains("fn")
        });
        assert!(
            has_keyword,
            "expected nested rust highlighting in markdown fenced block, spans: {:?}",
            result.spans
        );
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn markdown_headings_use_distinct_highlight_classes() {
        use super::{CodeHighlightClass, CodeLanguageKey};

        let source = "# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6\n";
        let result = highlight_code_block(Some("md"), source).expect("markdown should highlight");
        assert_eq!(result.language, CodeLanguageKey::Markdown);
        for (expected, label) in [
            (CodeHighlightClass::MarkdownHeading1, "H1"),
            (CodeHighlightClass::MarkdownHeading2, "H2"),
            (CodeHighlightClass::MarkdownHeading3, "H3"),
            (CodeHighlightClass::MarkdownHeading4, "H4"),
            (CodeHighlightClass::MarkdownHeading5, "H5"),
            (CodeHighlightClass::MarkdownHeading6, "H6"),
        ] {
            assert!(
                result.spans.iter().any(|span| span.class == expected),
                "expected {label} heading span, got {:?}",
                result.spans
            );
        }
    }


    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn markdown_paragraph_inline_receives_strong_highlight() {
        use super::{CodeHighlightClass, CodeLanguageKey};

        let source = "# Title\n\nSome **bold** text\n";
        let result = highlight_code_block(Some("markdown"), source)
            .expect("markdown should highlight");
        assert_eq!(result.language, CodeLanguageKey::Markdown);
        assert!(
            result
                .spans
                .iter()
                .any(|span| span.class == CodeHighlightClass::MarkdownStrong),
            "expected strong inline span, got {:?}",
            result.spans
        );
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn markdown_block_formats_use_semantic_highlight_classes() {
        use super::{CodeHighlightClass, CodeLanguageKey};

        let source = "- list item\n\n> quoted text\n\n[label]: https://example.com\n";
        let result = highlight_code_block(Some("md"), source).expect("markdown should highlight");
        assert_eq!(result.language, CodeLanguageKey::Markdown);
        assert!(
            result
                .spans
                .iter()
                .any(|span| span.class == CodeHighlightClass::Punctuation),
            "expected list/quote punctuation span, got {:?}",
            result.spans
        );
        assert!(
            result
                .spans
                .iter()
                .any(|span| span.class == CodeHighlightClass::MarkdownReference),
            "expected reference link span, got {:?}",
            result.spans
        );
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn default_official_highlight_bundle_produces_spans() {
        let samples = [
            ("rust", "fn main() {\n    let value: i32 = 42;\n}\n"),
            ("js", "function greet(name) { return `hi ${name}`; }\n"),
            ("jsx", "const App = () => <div className=\"x\">Hi</div>;\n"),
            (
                "ts",
                "type User = { id: number };\nconst user: User = { id: 1 };\n",
            ),
            (
                "tsx",
                "const App = (): JSX.Element => <button>OK</button>;\n",
            ),
            ("json", "{\n  \"answer\": 42\n}\n"),
            ("md", "# Heading\n\n`code`\n"),
            ("bash", "echo \"hello\"\nif [ -f file ]; then echo ok; fi\n"),
            ("c", "int main(void) { return 0; }\n"),
            ("cpp", "class Box { public: int value = 1; };\n"),
            (
                "csharp",
                "class App { static void Main() { var x = 1; } }\n",
            ),
            ("css", "body { color: #fff; display: grid; }\n"),
            ("go", "package main\nfunc main() { println(\"hi\") }\n"),
            ("html", "<div class=\"card\"><span>Hi</span></div>\n"),
            (
                "java",
                "class App { int add(int a, int b) { return a + b; } }\n",
            ),
            ("php", "<?php echo \"hi\"; $x = 1; ?>\n"),
            ("python", "def double(x):\n    return x * 2\n"),
            ("ruby", "def hello(name)\n  puts \"Hi #{name}\"\nend\n"),
        ];

        for (language, sample) in samples {
            let result = highlight_code_block(Some(language), sample)
                .expect("known language should produce a result");
            assert!(
                !result.spans.is_empty(),
                "expected non-empty spans for {language}"
            );
        }
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-config"))]
    #[test]
    fn config_language_bundle_produces_spans() {
        let yaml = highlight_code_block(Some("yaml"), "key:\n  - value\n")
            .expect("yaml should produce a result");
        assert!(!yaml.spans.is_empty());

        let toml = highlight_code_block(
            Some("toml"),
            "[package]\nname = \"velotype\"\nversion = \"0.1.0\"\n",
        )
        .expect("toml should produce a result");
        assert!(!toml.spans.is_empty());
    }
}
