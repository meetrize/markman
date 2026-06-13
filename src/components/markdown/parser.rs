//! Shared pulldown-cmark parser configuration for GFM Markdown.

use pulldown_cmark::{Options, Parser};

/// Returns the GFM parser options used by block preview and HTML export.
pub fn gfm_parser_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_GFM);
    options
}

/// Builds a GFM-enabled pulldown-cmark parser for the given Markdown source.
pub fn gfm_parser<'input>(markdown: &'input str) -> Parser<'input> {
    Parser::new_ext(markdown, gfm_parser_options())
}

#[cfg(test)]
mod tests {
    use super::gfm_parser_options;
    use pulldown_cmark::Options;

    #[test]
    fn gfm_parser_options_enable_expected_extensions() {
        let options = gfm_parser_options();
        assert!(options.contains(Options::ENABLE_TABLES));
        assert!(options.contains(Options::ENABLE_FOOTNOTES));
        assert!(options.contains(Options::ENABLE_TASKLISTS));
        assert!(options.contains(Options::ENABLE_STRIKETHROUGH));
        assert!(options.contains(Options::ENABLE_GFM));
    }
}
