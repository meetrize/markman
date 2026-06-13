//! Image block and inline image rendering.

use gpui::*;

use super::super::{Block, ImageRuntime};
use crate::components::ImageResolvedSource;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

fn fallback_image_label(alt: &str, strings: &I18nStrings) -> SharedString {
    if alt.trim().is_empty() {
        SharedString::from(strings.image_placeholder.clone())
    } else {
        SharedString::from(alt.to_string())
    }
}

fn render_image_placeholder(
    runtime: &ImageRuntime,
    width: Length,
    height: Pixels,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    div()
        .w(width)
        .h(height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(d.image_radius))
        .border(px(1.0))
        .border_color(c.image_placeholder_border)
        .bg(c.image_placeholder_bg)
        .px(px(d.block_padding_x))
        .text_center()
        .text_size(px(t.text_size))
        .text_color(c.image_placeholder_text)
        .child(fallback_image_label(&runtime.alt, strings))
        .into_any_element()
}

fn render_loading_placeholder(
    runtime: &ImageRuntime,
    width: Length,
    height: Pixels,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    div()
        .w(width)
        .h(height)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(d.image_radius))
        .border(px(1.0))
        .border_color(c.image_placeholder_border)
        .bg(c.image_placeholder_bg)
        .px(px(d.block_padding_x))
        .text_center()
        .text_size(px(t.code_size))
        .text_color(c.image_placeholder_text)
        .child(if runtime.alt.trim().is_empty() {
            SharedString::from(strings.image_loading_without_alt.clone())
        } else {
            SharedString::from(
                strings
                    .image_loading_with_alt_template
                    .replace("{alt}", &runtime.alt),
            )
        })
        .into_any_element()
}
impl Block {
    pub(super) fn render_image_content(
        &self,
        runtime: &ImageRuntime,
        max_width: Length,
        max_height: Pixels,
        placeholder_height: Pixels,
        theme: &Theme,
        strings: &I18nStrings,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let source = runtime.resolved_source.clone();
        let placeholder_theme = theme.clone();
        let loading_theme = theme.clone();
        let placeholder_strings = strings.clone();
        let loading_strings = strings.clone();
        let runtime_for_fallback = runtime.clone();
        let runtime_for_loading = runtime.clone();

        let image = match source {
            ImageResolvedSource::Local(path) => img(path),
            ImageResolvedSource::Remote(uri) => img(uri),
        }
        .max_w(max_width)
        .max_h(max_height)
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || {
            render_image_placeholder(
                &runtime_for_fallback,
                max_width,
                placeholder_height,
                &placeholder_theme,
                &placeholder_strings,
            )
        })
        .with_loading(move || {
            render_loading_placeholder(
                &runtime_for_loading,
                max_width,
                placeholder_height,
                &loading_theme,
                &loading_strings,
            )
        });

        let mut container = div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(d.image_caption_gap))
            .child(image);

        if let Some(title) = runtime
            .title
            .as_ref()
            .filter(|title| !title.trim().is_empty())
        {
            container = container.child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(t.code_size))
                    .text_color(c.image_caption_text)
                    .child(SharedString::from(title.clone())),
            );
        }

        container.into_any_element()
    }
    pub(super) fn render_inline_image_content(
        &self,
        runtime: &ImageRuntime,
        theme: &Theme,
        strings: &I18nStrings,
    ) -> AnyElement {
        let d = &theme.dimensions;
        let source = runtime.resolved_source.clone();
        let max_height = px(d.image_cell_placeholder_height);
        let max_width =
            Length::Definite(px((d.image_cell_placeholder_height * 1.6).max(48.0)).into());
        let placeholder_theme = theme.clone();
        let loading_theme = theme.clone();
        let placeholder_strings = strings.clone();
        let loading_strings = strings.clone();
        let runtime_for_fallback = runtime.clone();
        let runtime_for_loading = runtime.clone();

        let image = match source {
            ImageResolvedSource::Local(path) => img(path),
            ImageResolvedSource::Remote(uri) => img(uri),
        }
        .max_w(max_width)
        .max_h(max_height)
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || {
            render_image_placeholder(
                &runtime_for_fallback,
                max_width,
                max_height,
                &placeholder_theme,
                &placeholder_strings,
            )
        })
        .with_loading(move || {
            render_loading_placeholder(
                &runtime_for_loading,
                max_width,
                max_height,
                &loading_theme,
                &loading_strings,
            )
        });

        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .child(image)
            .into_any_element()
    }


}
