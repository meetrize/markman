//! Rendering for [`Block`] via GPUI's high-level [`Render`] trait.
//!
//! Each block kind produces a distinct visual style: H1 has a bottom border,
//! list items render a marker column (bullet / ordinal), and raw Markdown
//! fallback renders as plain text.
mod code;
mod columns;
mod html_block;
mod image;
mod inline_visual;
mod math;
mod mermaid;
mod shared;
mod table;
mod text_blocks;

pub(crate) use columns::{
    ColumnMarkdownSegment, parse_columns_markdown,
    split_column_markdown_segments, update_columns_host_table_markdown,
};
pub(crate) use html_block::HtmlNodeVisualStyle;

#[cfg(test)]
mod tests {
    use super::columns::{
        ColumnMarkdownSegment, parse_columns_markdown, split_column_markdown_segments,
    };
    use super::html_block::{
        html_children_are_plain_text, html_collect_visible_text, html_node_visual_style,
        html_table_column_count, html_table_collect_rows, HtmlComputedStyle,
    };
    use super::code::{html_pre_code_language};
    use pulldown_cmark::{Parser as CmarkParser, html as cmark_html};
    use crate::components::gfm_parser_options;
    use crate::components::highlight_code_block;
    use crate::components::{Block, BlockKind, BlockRecord, InlineTextTree, parse_html_document};
    use crate::i18n::I18nManager;
    use crate::theme::{Theme, ThemeManager};
    use gpui::{Hsla, Rgba, TestAppContext};

    fn assert_color_near(color: Hsla, red: u8, green: u8, blue: u8, alpha: u8) {
        let color = Rgba::from(color);
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as i16;
        assert!((channel(color.r) - red as i16).abs() <= 1);
        assert!((channel(color.g) - green as i16).abs() <= 1);
        assert!((channel(color.b) - blue as i16).abs() <= 1);
        assert!((channel(color.a) - alpha as i16).abs() <= 1);
    }

    #[test]
    fn html_collect_visible_text_merges_paragraph_inline_nodes() {
        let parser = CmarkParser::new_ext(
            "这是一段很长的分栏说明文字，用来验证自动换行。",
            gfm_parser_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let paragraph = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "p")
            .expect("paragraph");
        assert!(html_children_are_plain_text(&paragraph.children));
        assert_eq!(
            html_collect_visible_text(&paragraph.children),
            "这是一段很长的分栏说明文字，用来验证自动换行。"
        );
    }

    #[test]
    fn cmark_code_fence_html_exposes_language_class() {
        let parser = CmarkParser::new_ext(
            "```rust\nfn main() {}\n```",
            gfm_parser_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let pre = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "pre")
            .expect("code block pre");
        assert_eq!(html_pre_code_language(pre).as_deref(), Some("rust"));
        let highlight = highlight_code_block(Some("rust"), "fn main() {}");
        assert!(highlight.is_some());
        assert!(!highlight.unwrap().spans.is_empty());
    }

    #[test]
    fn cmark_table_html_parses_into_aligned_row_structure() {
        let parser = CmarkParser::new_ext(
            "| Metric | Value | Change |\n| --- | ---: | ---: |\n| Page Views | 12,000 | +18% |",
            gfm_parser_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let table = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "table")
            .expect("table");
        assert_eq!(html_table_column_count(table), 3);
        let rows = html_table_collect_rows(table);
        assert_eq!(rows.len(), 2);
        assert!(rows[0]
            .children
            .iter()
            .any(|child| child.tag_name == "th"));
        assert!(rows[1]
            .children
            .iter()
            .any(|child| child.tag_name == "td"));
    }

    #[test]
    fn cmark_table_html_inserts_whitespace_between_rows() {
        let parser = CmarkParser::new_ext(
            "| A | B |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |",
            gfm_parser_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let table = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "table")
            .expect("table");
        let tbody = table
            .children
            .iter()
            .find(|node| node.tag_name == "tbody")
            .expect("tbody");
        let whitespace_nodes = tbody
            .children
            .iter()
            .filter(|child| {
                child.tag_name == "#text" && child.raw_source.chars().all(char::is_whitespace)
            })
            .count();
        assert!(
            whitespace_nodes > 0,
            "expected whitespace nodes between table rows, got {html:?}"
        );
    }

    #[test]
    fn cmark_ordered_list_html_inserts_whitespace_between_items() {
        let parser = CmarkParser::new_ext(
            "1. First\n2. Second\n3. Third",
            gfm_parser_options(),
        );
        let mut html = String::new();
        cmark_html::push_html(&mut html, parser);
        let doc = parse_html_document(&html);
        let ol = doc
            .nodes
            .iter()
            .find(|node| node.tag_name == "ol")
            .expect("ordered list");
        let whitespace_nodes = ol
            .children
            .iter()
            .filter(|child| {
                child.tag_name == "#text" && child.raw_source.chars().all(char::is_whitespace)
            })
            .count();
        assert!(
            whitespace_nodes > 0,
            "expected whitespace nodes between list items, got {html:?}"
        );
    }

    #[test]
    fn html_render_style_inherits_color_and_font_size() {
        let theme = Theme::default_theme();
        let doc = parse_html_document(
            "<div style=\"color:blue; font-size:20px\"><span style=\"font-size:120%\">x</span></div>",
        );
        let root = HtmlComputedStyle::root(&theme);
        let parent = html_node_visual_style(&doc.nodes[0], root, &theme);
        let child = html_node_visual_style(&doc.nodes[0].children[0], parent.computed, &theme);

        assert_color_near(parent.computed.color, 0, 0, 255, 255);
        assert_color_near(child.computed.color, 0, 0, 255, 255);
        assert!((child.computed.font_size - 24.0).abs() < 0.01);
    }

    #[test]
    fn parses_columns_markdown_for_live_rendering() {
        let columns = parse_columns_markdown(concat!(
            "::: columns\n",
            "--- column width=40%\n",
            "### Left\n\n",
            "- A\n",
            "- B\n\n",
            "--- column width=60%\n",
            "Right text\n",
            ":::"
        ))
        .expect("columns block");

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].width_fraction, Some(0.4));
        assert_eq!(columns[1].width_fraction, Some(0.6));
        assert!(columns[0].markdown.contains("### Left"));
        assert_eq!(columns[1].markdown, "Right text");
    }

    #[test]
    fn split_column_markdown_extracts_mermaid_fence() {
        let segments = split_column_markdown_segments(concat!(
            "### Chart\n\n",
            "```mermaid\n",
            "flowchart LR\n",
            "A --> B\n",
            "```\n\n",
            "Tail text"
        ));

        assert_eq!(segments.len(), 3);
        match &segments[0] {
            ColumnMarkdownSegment::Markdown(text) => assert!(text.contains("### Chart")),
            _ => panic!("expected markdown segment"),
        }
        match &segments[1] {
            ColumnMarkdownSegment::Mermaid(raw) => {
                assert!(raw.contains("```mermaid"));
                assert!(raw.contains("A --> B"));
            }
            _ => panic!("expected mermaid segment"),
        }
        match &segments[2] {
            ColumnMarkdownSegment::Markdown(text) => assert_eq!(text, "Tail text"),
            _ => panic!("expected markdown segment"),
        }
    }

    #[test]
    fn split_column_markdown_extracts_table() {
        let segments = split_column_markdown_segments(concat!(
            "### Metrics\n\n",
            "| Metric | Value | Change |\n",
            "| --- | ---: | ---: |\n",
            "| Page Views | 12,000 | +18% |\n\n",
            "Tail text"
        ));

        assert_eq!(segments.len(), 3);
        match &segments[0] {
            ColumnMarkdownSegment::Markdown(text) => assert!(text.contains("### Metrics")),
            _ => panic!("expected markdown segment"),
        }
        match &segments[1] {
            ColumnMarkdownSegment::Table(table) => {
                assert_eq!(table.header.len(), 3);
                assert_eq!(table.rows.len(), 1);
                assert_eq!(
                    table.header[0].render_cache().visible_text(),
                    "Metric"
                );
            }
            _ => panic!("expected table segment"),
        }
        match &segments[2] {
            ColumnMarkdownSegment::Markdown(text) => assert_eq!(text, "Tail text"),
            _ => panic!("expected markdown segment"),
        }
    }

    #[test]
    fn columns_live_parser_allows_trailing_blank_lines() {
        let columns = parse_columns_markdown(concat!(
            "::: columns\n",
            "--- column\n",
            "Left\n",
            "--- column\n",
            "Right\n",
            ":::\n"
        ))
        .expect("columns block with trailing newline");

        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].markdown, "Left");
        assert_eq!(columns[1].markdown, "Right");
    }

    #[test]
    fn columns_live_parser_requires_closing_marker() {
        let columns = parse_columns_markdown("::: columns\n--- column\nLeft");

        assert!(columns.is_none());
    }

    #[test]
    fn html_render_style_overrides_link_and_mark_defaults() {
        let theme = Theme::default_theme();
        let link_doc = parse_html_document("<a style=\"color:red\">x</a>");
        let link_style =
            html_node_visual_style(&link_doc.nodes[0], HtmlComputedStyle::root(&theme), &theme);
        assert_color_near(link_style.computed.color, 255, 0, 0, 255);

        let mark_doc = parse_html_document("<mark style=\"background-color:#123\">x</mark>");
        let mark_style =
            html_node_visual_style(&mark_doc.nodes[0], HtmlComputedStyle::root(&theme), &theme);
        assert_color_near(mark_style.background.unwrap(), 0x11, 0x22, 0x33, 0xff);
    }

    #[test]
    fn html_render_style_does_not_inherit_background_color() {
        let theme = Theme::default_theme();
        let doc =
            parse_html_document("<div style=\"background-color:#112233\"><span>child</span></div>");
        let root = HtmlComputedStyle::root(&theme);
        let parent = html_node_visual_style(&doc.nodes[0], root, &theme);
        let child = html_node_visual_style(&doc.nodes[0].children[0], parent.computed, &theme);

        assert_color_near(parent.background.unwrap(), 0x11, 0x22, 0x33, 0xff);
        assert!(child.background.is_none());
    }

    #[gpui::test]
    async fn focused_code_block_renders_with_language_badge(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init(cx);
        });
        let (block, cx) = cx.add_window_view(|_window, cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::CodeBlock {
                        language: Some("rust".into()),
                    },
                    InlineTextTree::plain("fn main() {}\n"),
                ),
            )
        });

        cx.update(|window, cx| {
            block.update(cx, |block, _cx| {
                block.focus_handle.focus(window);
            });
            window.draw(cx).clear();
        });
        cx.run_until_parked();

        block.read_with(cx, |block, _cx| {
            assert!(block.last_bounds.is_some());
            assert_eq!(block.code_language_text(), "rust");
        });
    }
}
