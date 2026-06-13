//! Shared UI components and Markdown editing primitives.

mod actions;
mod block;
pub(crate) mod latex;
pub(crate) mod markdown;
pub(crate) mod mermaid;

#[cfg(test)]
pub use crate::editor::Editor;

#[allow(unused_imports)]
pub(crate) use crate::editor::InfoDialogKind;
pub use actions::*;
pub use block::*;
pub(crate) use markdown::{
    collect_columns_block_region, escape_html_text, gfm_parser, gfm_parser_options,
    is_columns_block_start, parse_columns_content, FenceInfo, is_closing_fence,
    is_closing_fence_marker, opening_fence_marker, parse_opening_fence, strip_fence_indent,
};
pub(crate) use markdown::columns::{parse_column_width_fraction, trim_column_markdown_lines};
pub(crate) use block::{
    ColumnMarkdownSegment, parse_columns_markdown, split_column_markdown_segments,
    update_columns_host_table_markdown,
};
#[allow(unused_imports)]
pub(crate) use latex::*;
#[allow(unused_imports)]
pub(crate) use markdown::code_highlight::*;
#[allow(unused_imports)]
pub(crate) use markdown::footnote::*;
#[allow(unused_imports)]
pub(crate) use markdown::html::*;
#[allow(unused_imports)]
pub(crate) use markdown::image::*;
#[allow(unused_imports)]
pub use markdown::inline::*;
#[allow(unused_imports)]
pub(crate) use markdown::link::*;
pub use markdown::table::*;
#[allow(unused_imports)]
pub(crate) use mermaid::*;
