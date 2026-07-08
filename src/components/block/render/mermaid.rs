//! Mermaid diagram block rendering.

use gpui::*;

use super::super::element::BlockTextElement;
use super::shared::{effective_image_width, mermaid_available_height};
use super::super::Block;
use crate::components::{parse_mermaid_fence_source, render_mermaid_svg_for_display};
use crate::theme::Theme;

impl Block {
    fn render_mermaid_content(&self, theme: &Theme, window: &Window) -> AnyElement {
        let d = &theme.dimensions;
        let raw = self
            .record
            .raw_fallback
            .as_deref()
            .unwrap_or_else(|| self.display_text());
        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width.max(px(1.0)));
        let viewport_height = f32::from(viewport.height.max(px(1.0)));
        let available_width = effective_image_width(self, viewport_width, d);
        let available_height = mermaid_available_height(viewport_height, d);
        self.render_mermaid_diagram(
            raw,
            available_width,
            available_height,
            window.scale_factor(),
            theme,
            ElementId::Name(format!("mermaid-scroll-{}", self.record.id).into()),
        )
    }

    pub(super) fn render_mermaid_diagram(
        &self,
        raw_fence: &str,
        available_width: f32,
        available_height: f32,
        device_scale: f32,
        theme: &Theme,
        scroll_id: ElementId,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let Some(source) = parse_mermaid_fence_source(raw_fence) else {
            return div()
                .w_full()
                .text_size(px(t.text_size))
                .line_height(relative(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw_fence.to_string()))
                .into_any_element();
        };

        match render_mermaid_svg_for_display(
            &source,
            available_width,
            available_height,
            device_scale,
        ) {
            Ok(rendered) => {
                let display_width = rendered.display_width.max(1.0);
                let display_height = rendered.display_height.max(1.0);
                let image_path = rendered.path.clone();
                let image = move || {
                    img(image_path.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                };
                let content = if display_width <= available_width + 0.5 {
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(image())
                        .into_any_element()
                } else {
                    div()
                        .id(scroll_id)
                        .w_full()
                        .overflow_x_scroll()
                        .scrollbar_width(px(0.0))
                        .child(div().w(px(display_width)).child(image()))
                        .into_any_element()
                };

                div()
                    .w_full()
                    .py(px(d.block_padding_y.max(6.0)))
                    .child(content)
                    .into_any_element()
            }
            Err(err) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x))
                .py(px(d.block_padding_y))
                .text_size(px(t.text_size))
                .line_height(relative(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw_fence.to_string()))
                .child(
                    div()
                        .text_size(px(t.code_size))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(format!("Mermaid render error: {err}"))),
                )
                .into_any_element(),
        }
    }

    pub(super) fn render_mermaid_block(
        &mut self,
        focused_base: Stateful<Div>,
        focused: bool,
        is_placeholder: bool,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                }
                let child = if focused {
                    BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
                } else {
                    self.render_mermaid_content(&theme, window)
                };
                focused_base.w_full().child(child).into_any_element()
    }
}
