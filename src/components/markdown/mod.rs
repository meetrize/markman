//! Markdown syntax models and parse/serialize helpers shared by editor blocks.

pub(crate) mod columns;
pub(crate) mod fence;
pub(crate) mod code_highlight;
pub(crate) mod escape;
pub(crate) mod footnote;
pub(crate) mod html;
pub(crate) mod image;
pub mod inline;
pub(crate) mod link;
pub(crate) mod parser;
pub(crate) mod paste;
pub(crate) mod source_format;
pub mod table;

pub use escape::{escape_html_attr, escape_html_text};
pub use columns::{
    collect_columns_block_region, is_columns_block_start, parse_columns_content,
};
pub use fence::{
    FenceInfo, is_closing_fence, is_closing_fence_marker, opening_fence_marker,
    parse_opening_fence,
};
pub(crate) use fence::strip_fence_indent;
pub use parser::gfm_parser;
