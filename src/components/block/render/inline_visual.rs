//! Mixed inline text, math, and image visual runs.

use gpui::*;

use super::super::element::BlockTextElement;
use super::super::Block;
use super::shared::html_css_color_to_hsla;
use crate::components::InlineScript;
use crate::theme::Theme;

/// Marker-pen tint for `==highlight==` in div-based inline preview runs.
fn inline_highlight_background(_theme: &Theme) -> Hsla {
    Hsla::from(rgba(0xffe06699))
}

impl Block {
    pub(super) fn render_text_or_mixed_inline_visuals(
        &self,
        theme: &Theme,
        focused: bool,
        is_placeholder: bool,
        placeholder_text: Option<SharedString>,
        placeholder_color: Option<Hsla>,
        text_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Mixed inline visuals are display-only. Once focused, the text element
        // takes over so caret movement, projection markers, and IME ranges stay
        // anchored to editable text rather than rendered SVG/script offsets.
        // While document search highlights are active, keep BlockTextElement so
        // highlight overlays share the same text layout as the search query.
        // Unfocused blocks with highlight/math/script use div-based preview runs.
        if focused
            || is_placeholder
            || !self.search_highlight_ranges.is_empty()
        {
            return match placeholder_text {
                Some(placeholder) => BlockTextElement::with_placeholder(
                    cx.entity(),
                    is_placeholder,
                    placeholder,
                    placeholder_color,
                )
                .into_any_element(),
                None => BlockTextElement::new(cx.entity(), is_placeholder).into_any_element(),
            };
        }

        if self.has_mixed_inline_visuals() {
            return self.render_mixed_inline_visual_runs(theme, text_color, font_size, font_weight);
        }

        BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
    }

    pub(super) fn render_mixed_inline_visual_runs(
        &self,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> AnyElement {
        self.render_inline_tree_runs(
            &self.record.title,
            theme,
            base_color,
            font_size,
            font_weight,
        )
    }

    pub(super) fn render_inline_tree_runs(
        &self,
        tree: &crate::components::InlineTextTree,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> AnyElement {
        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(0.0))
            .text_size(px(font_size))
            .line_height(relative(theme.typography.text_line_height))
            .children(self.render_inline_tree_children(
                tree,
                theme,
                base_color,
                font_size,
                font_weight,
            ))
            .into_any_element()
    }

    pub(super) fn render_inline_tree_children(
        &self,
        tree: &crate::components::InlineTextTree,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> Vec<AnyElement> {
        let cache = tree.render_cache();
        let text = cache.visible_text();
        let mut children = Vec::new();
        let mut cursor = 0usize;

        for span in cache.spans() {
            if cursor < span.range.start {
                let gap_span = crate::components::InlineSpan {
                    range: cursor..span.range.start,
                    style: crate::components::InlineStyle::default(),
                    html_style: None,
                    link: None,
                    footnote: None,
                    math: None,
                    tag: None,
                };
                children.push(self.render_inline_text_segment(
                    &text[cursor..span.range.start],
                    &gap_span,
                    theme,
                    base_color,
                    font_size,
                    font_weight,
                ));
            }

            let span_text = &text[span.range.clone()];
            if let Some(math) = span.math.as_ref() {
                children.push(
                    self.render_inline_math_segment(math, span, theme, base_color, font_size),
                );
            } else {
                children.push(self.render_inline_text_segment(
                    span_text,
                    span,
                    theme,
                    base_color,
                    font_size,
                    font_weight,
                ));
            }
            cursor = span.range.end;
        }

        if cursor < text.len() {
            let fallback_span = crate::components::InlineSpan {
                range: cursor..text.len(),
                style: crate::components::InlineStyle::default(),
                html_style: None,
                link: None,
                footnote: None,
                math: None,
                tag: None,
            };
            children.push(self.render_inline_text_segment(
                &text[cursor..],
                &fallback_span,
                theme,
                base_color,
                font_size,
                font_weight,
            ));
        }

        children
    }

    pub(super) fn render_inline_text_segment(
        &self,
        text: &str,
        span: &crate::components::InlineSpan,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
    ) -> AnyElement {
        if text.is_empty() {
            return div().into_any_element();
        }

        let mut color = if span.tag.is_some() {
            theme.colors.text_tag
        } else if span.link.is_some() || span.footnote.is_some() {
            theme.colors.text_link
        } else {
            base_color
        };
        if let Some(style) = span.html_style
            && let Some(html_color) = style.color
        {
            color = html_css_color_to_hsla(html_color, color);
        }

        let script_offset = match span.style.script {
            InlineScript::Normal => 0.0,
            InlineScript::Superscript => -font_size * 0.28,
            InlineScript::Subscript => font_size * 0.22,
        };
        let display_font_size = if span.style.has_script() {
            (font_size * 0.72).max(6.0)
        } else {
            font_size
        };

        let mut element = div()
            .min_w(px(0.0))
            .text_size(px(display_font_size))
            .line_height(relative(theme.typography.text_line_height))
            .text_color(color)
            .font_weight(if span.style.bold {
                FontWeight::BOLD
            } else {
                font_weight
            })
            .child(SharedString::from(text.to_string()));

        if script_offset != 0.0 {
            element = element.relative().top(px(script_offset));
        }

        if span.style.underline || span.link.is_some() || span.footnote.is_some() {
            element = element.underline();
        }
        if span.tag.is_some() {
            element = element
                .rounded(px(theme.dimensions.code_bg_radius))
                .px(px(theme.dimensions.code_bg_pad_x))
                .py(px(theme.dimensions.code_bg_pad_y))
                .bg(theme.colors.tag_background);
        } else if span.style.code {
            element = element
                .rounded(px(theme.dimensions.code_bg_radius))
                .px(px(theme.dimensions.code_bg_pad_x))
                .py(px(theme.dimensions.code_bg_pad_y))
                .bg(theme.colors.code_bg);
        }
        if span.style.highlight {
            element = element
                .rounded(px(3.0))
                .px(px(2.0))
                .bg(inline_highlight_background(theme));
        }
        if let Some(style) = span.html_style
            && let Some(background) = style.background_color
        {
            element = element
                .rounded(px(3.0))
                .px(px(2.0))
                .bg(html_css_color_to_hsla(background, color));
        }

        element.into_any_element()
    }

}
