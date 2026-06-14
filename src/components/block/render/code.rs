//! Code block rendering helpers.

use std::ops::Range;

use gpui::*;

use super::super::element::BlockTextElement;
use super::super::{Block, code_highlight_color};
use crate::components::markdown::code_highlight::CodeHighlightSpan;
use crate::components::{HtmlNode, attr_value, highlight_code_block};
use crate::config::read_app_preferences;
use crate::code_runner::{CodeRunStatus, code_run_output_line_count, CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

pub(super) const ICON_CODE_BLOCK_COPY: &str = "icon/toolbar/copy.svg";
pub(super) const ICON_CODE_BLOCK_COLLAPSE: &str = "icon/toolbar/chevrons-down-up.svg";
pub(super) const ICON_CODE_BLOCK_EXPAND: &str = "icon/toolbar/chevrons-up-down.svg";
pub(super) const ICON_CODE_BLOCK_RUN: &str = "icon/toolbar/circle-play.svg";
pub(super) const ICON_CODE_BLOCK_STOP: &str = "icon/toolbar/circle-stop.svg";
const ICON_CODE_BLOCK_CLOSE: &str = "icon/toolbar/x.svg";
pub(super) const ICON_CODE_RUN_OUTPUT_CHEVRON_DOWN: &str = "icon/toolbar/chevron-down.svg";
pub(super) const ICON_CODE_RUN_OUTPUT_CHEVRON_UP: &str = "icon/toolbar/chevron-up.svg";

pub(crate) fn html_code_language(node: &HtmlNode) -> Option<String> {
    let class = attr_value(node, "class")?;
    for token in class.split_whitespace() {
        if let Some(language) = token.strip_prefix("language-") {
            return Some(language.to_string());
        }
    }
    None
}

pub(crate) fn html_pre_code_language(node: &HtmlNode) -> Option<String> {
    node.children
        .iter()
        .find(|child| child.tag_name == "code")
        .and_then(html_code_language)
}

fn render_html_code_highlight_chunks(
    source: &str,
    range: Range<usize>,
    spans: &[CodeHighlightSpan],
    base_color: Hsla,
    colors: &crate::theme::ThemeColors,
    font_size: f32,
) -> Vec<AnyElement> {
    let mut boundaries = vec![range.start, range.end];
    for span in spans {
        if span.range.start < range.end && span.range.end > range.start {
            boundaries.push(span.range.start.max(range.start));
            boundaries.push(span.range.end.min(range.end));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut elements = Vec::new();
    let mut span_idx = 0usize;
    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start >= end {
            continue;
        }

        while span_idx < spans.len() && spans[span_idx].range.end <= start {
            span_idx += 1;
        }
        let color = spans
            .get(span_idx)
            .filter(|span| span.range.start <= start && start < span.range.end)
            .map(|span| code_highlight_color(colors, span.class))
            .unwrap_or(base_color);

        elements.push(
            div()
                .flex_shrink_0()
                .text_size(px(font_size))
                .text_color(color)
                .child(SharedString::from(source[start..end].to_string()))
                .into_any_element(),
        );
    }
    elements
}

impl Block {
    fn render_code_run_output_panel(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        run_lane_width: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let snapshot = &self.code_run_snapshot;
        if !snapshot.shows_output_panel() {
            return div().into_any_element();
        }

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let running = snapshot.status == CodeRunStatus::Running;
        let panel_expanded = snapshot.output_expanded;
        let panel_toggle_icon = if panel_expanded {
            ICON_CODE_RUN_OUTPUT_CHEVRON_UP
        } else {
            ICON_CODE_RUN_OUTPUT_CHEVRON_DOWN
        };
        let icon_size = px((t.code_size - 1.0).max(10.0));
        let action_icon_extent = px(f32::from(icon_size) + 8.0);
        let code_line_height = t.code_size * t.text_line_height;
        let content_line_count = code_run_output_line_count(
            &snapshot.stdout,
            &snapshot.stderr,
            snapshot.error_message.as_deref(),
        );
        let content_collapsible =
            content_line_count > CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES;
        let content_collapsed = content_collapsible && !snapshot.output_content_expanded;
        let collapsed_max_height =
            px(code_line_height * CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES as f32);
        let hidden_line_count = content_line_count
            .saturating_sub(CODE_RUN_OUTPUT_COLLAPSED_VISIBLE_LINES);
        let output_panel_bg = Hsla::from(rgba(0xffffffff));
        // Inset the panel background from the block row edge so it aligns with
        // the code content area's internal horizontal padding.
        let output_panel_edge_inset_right = d.code_block_padding_x;

        let mut text_sections = div().w_full().flex().flex_col().gap(px(6.0));
        if !snapshot.stdout.is_empty() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.code_text)
                    .child(snapshot.stdout.clone()),
            );
        }
        if !snapshot.stderr.is_empty() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(snapshot.stderr.clone()),
            );
        }
        if let Some(error) = snapshot.error_message.as_ref() {
            text_sections = text_sections.child(
                div()
                    .text_size(px(t.code_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(error.clone()),
            );
        }

        let exit_label = snapshot
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| strings.code_run_exit_none.clone());
        let meta = strings
            .code_run_meta_template
            .replace("{exit}", &exit_label)
            .replace("{duration}", &snapshot.duration_ms.to_string());

        let mut body = div().w_full();
        if panel_expanded {
            let mut content_wrapper = div().relative().w_full().child({
                let mut clipped = div().min_w(px(0.0)).w_full().child(text_sections);
                if content_collapsed {
                    clipped = clipped
                        .max_h(collapsed_max_height)
                        .overflow_hidden();
                }
                clipped
            });
            if content_collapsed {
                let expand_label = strings
                    .code_run_output_expand_lines_template
                    .replace("{count}", &hidden_line_count.to_string());
                content_wrapper = content_wrapper
                    .pb(px(code_line_height))
                    .child(
                        div()
                            .id("code-block-run-output-content-expand")
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(code_line_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .bg(output_panel_bg)
                            .border_t(px(1.0))
                            .border_color(c.code_language_input_border.opacity(0.35))
                            .cursor_pointer()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_code_block_run_output_content_toggle_mouse_down),
                            )
                            .child(
                                svg()
                                    .path(ICON_CODE_BLOCK_EXPAND)
                                    .size(icon_size)
                                    .text_color(c.code_language_input_text),
                            )
                            .child(
                                div()
                                    .text_size(px((t.code_size - 1.5).max(9.0)))
                                    .text_color(c.code_language_input_text.opacity(0.85))
                                    .child(expand_label),
                            ),
                    );
            }
            body = body.child(content_wrapper).child(
                div()
                    .mt(px(6.0))
                    .text_size(px((t.code_size - 1.0).max(10.0)))
                    .text_color(c.text_quote)
                    .child(meta),
            );
        }

        let mut actions = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .mr(px(8.0));
        if running {
            actions = actions.child(
                div()
                    .id("code-block-run-stop")
                    .w(action_icon_extent)
                    .h(action_icon_extent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .opacity(0.72)
                    .hover(|this| this.opacity(1.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_code_block_run_stop_mouse_down),
                    )
                    .child(
                        svg()
                            .path(ICON_CODE_BLOCK_STOP)
                            .size(icon_size)
                            .text_color(c.dialog_danger_button_bg),
                    ),
            );
        }
        if panel_expanded && content_collapsible && snapshot.output_content_expanded {
            actions = actions.child(
                div()
                    .id("code-block-run-output-content-collapse")
                    .w(action_icon_extent)
                    .h(action_icon_extent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .opacity(0.72)
                    .hover(|this| this.opacity(1.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::on_code_block_run_output_content_toggle_mouse_down),
                    )
                    .child(
                        svg()
                            .path(ICON_CODE_BLOCK_COLLAPSE)
                            .size(icon_size)
                            .text_color(c.code_language_input_text),
                    ),
            );
        }
        actions = actions.child(
            div()
                .id("code-block-run-output-toggle")
                .w(action_icon_extent)
                .h(action_icon_extent)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(0.72)
                .hover(|this| this.opacity(1.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_code_block_run_output_toggle_mouse_down),
                )
                .child(
                    svg()
                        .path(panel_toggle_icon)
                        .size(icon_size)
                        .text_color(c.code_language_input_text),
                ),
        );
        actions = actions.child(
            div()
                .id("code-block-run-output-close")
                .w(action_icon_extent)
                .h(action_icon_extent)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(0.72)
                .hover(|this| this.opacity(1.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_code_block_run_output_close_mouse_down),
                )
                .child(
                    svg()
                        .path(ICON_CODE_BLOCK_CLOSE)
                        .size(icon_size)
                        .text_color(c.code_language_input_text),
                ),
        );

        let run_lane_spacer = || {
            div()
                .flex_none()
                .flex_shrink_0()
                .w(run_lane_width)
                .bg(output_panel_bg)
        };

        let panel_content_lane = |child: AnyElement| {
            div().flex_grow().min_w(px(0.0)).child(child)
        };

        let panel = div()
            .id("code-block-run-output")
            .w_full()
            .border_t(px(1.0))
            .border_color(c.code_language_input_border.opacity(0.35))
            .bg(output_panel_bg)
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .border_b(px(1.0))
                    .border_color(c.code_language_input_border.opacity(0.35))
                    .child(run_lane_spacer())
                    .child(panel_content_lane(
                        div()
                            .w_full()
                            .py(px(6.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px((t.code_size - 1.0).max(10.0)))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(c.code_language_input_text)
                                    .child(strings.code_run_output_title.clone()),
                            )
                            .child(actions)
                            .into_any_element(),
                    )),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .child(run_lane_spacer())
                    .child(panel_content_lane(
                        div()
                            .w_full()
                            .py(px(if panel_expanded { 8.0 } else { 0.0 }))
                            .child(body)
                            .into_any_element(),
                    )),
            );

        div()
            .w_full()
            .pr(px(output_panel_edge_inset_right))
            .child(panel)
            .into_any_element()
    }
    pub(super) fn render_html_code_block(
        &self,
        source: &str,
        language: Option<&str>,
        theme: &Theme,
        _for_column: bool,
        node_style: super::HtmlNodeVisualStyle,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let font_size = t.code_size;
        let line_height = t.text_line_height;
        let base_color = c.code_text;
        let highlight_spans = highlight_code_block(language, source)
            .map(|result| result.spans)
            .unwrap_or_default();

        let mut line_start = 0usize;
        let mut line_elements = Vec::new();
        for line in source.split('\n') {
            let line_end = line_start + line.len();
            line_elements.push(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .children(render_html_code_highlight_chunks(
                        source,
                        line_start..line_end,
                        &highlight_spans,
                        base_color,
                        c,
                        font_size,
                    )),
            );
            line_start = line_end + 1;
        }

        let mut element = div()
            .w_full()
            .min_w(px(0.0))
            .rounded_sm()
            .bg(c.code_bg)
            .px(px(d.code_block_padding_x))
            .py(px(d.code_block_padding_y))
            .text_size(px(font_size))
            .line_height(relative(line_height))
            .flex()
            .flex_col()
            .items_start()
            .children(line_elements);
        if let Some(bg) = node_style.background {
            element = element.bg(bg);
        }
        element.into_any_element()
    }

    pub(super) fn render_code_block(
        &mut self,
        focused_base: Stateful<Div>,
        focused: bool,
        is_placeholder: bool,
        language: Option<SharedString>,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
                let language_label = language
                    .as_ref()
                    .map(|value| SharedString::from(value.to_string()))
                    .unwrap_or_else(|| SharedString::from(strings.code_language_placeholder.clone()));
                let badge_height = d.code_language_input_height
                    + d.code_language_input_padding_y * 2.0
                    + d.code_language_input_border_width * 2.0;
                let icon_size = px((t.code_size - 1.0).max(10.0));
                let collapsible = self.code_block_is_collapsible();
                let collapsed = self.code_block_collapsed(focused);
                let code_line_height = t.code_size * t.text_line_height;
                let collapsed_max_height =
                    px(code_line_height * super::super::CODE_BLOCK_COLLAPSED_VISIBLE_LINES as f32);
                let mut text_wrapper = div().min_w(px(0.0)).w_full();
                if collapsed {
                    text_wrapper = text_wrapper
                        .max_h(collapsed_max_height)
                        .overflow_hidden();
                }
                let mut code_content = div()
                    .relative()
                    .min_w(px(0.0))
                    .w_full()
                    .child(
                        text_wrapper.child(BlockTextElement::new(cx.entity(), is_placeholder)),
                    );

                if collapsed {
                    code_content = code_content.pb(px(code_line_height));
                    let hidden_lines = self.code_block_hidden_line_count();
                    code_content = code_content.child(
                        div()
                            .id("code-block-expand-bar")
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(code_line_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .bg(c.code_bg.opacity(0.94))
                            .border_t(px(1.0))
                            .border_color(c.code_language_input_border.opacity(0.35))
                            .cursor_pointer()
                            .occlude()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_code_block_collapse_toggle),
                            )
                            .child(
                                svg()
                                    .path(ICON_CODE_BLOCK_EXPAND)
                                    .size(icon_size)
                                    .text_color(c.code_language_input_text),
                            )
                            .child(
                                div()
                                    .text_size(px((t.code_size - 1.5).max(9.0)))
                                    .text_color(c.code_language_input_text.opacity(0.85))
                                    .child(SharedString::from(format!(
                                        "展开 {hidden_lines} 行"
                                    ))),
                            ),
                    );
                }

                {
                    let block = cx.entity().downgrade();
                    let collapse_icon = if collapsed {
                        ICON_CODE_BLOCK_EXPAND
                    } else {
                        ICON_CODE_BLOCK_COLLAPSE
                    };
                    let mut controls = div()
                        .absolute()
                        .top(px(d.code_block_padding_y))
                        .right(px(d.code_block_padding_x))
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(d.code_language_input_gap));

                    if collapsible {
                        controls = controls.child(
                            div()
                                .id("code-block-collapse")
                                .w(px(badge_height))
                                .h(px(badge_height))
                                .flex()
                                .flex_shrink_0()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.code_language_input_radius))
                                .border(px(d.code_language_input_border_width))
                                .border_color(c.code_language_input_border.opacity(0.65))
                                .bg(c.code_language_input_bg.opacity(0.92))
                                .hover(|this| this.bg(c.code_language_input_border.opacity(0.35)))
                                .active(|this| this.opacity(0.92))
                                .cursor_pointer()
                                .occlude()
                                .child(
                                    svg()
                                        .path(collapse_icon)
                                        .size(icon_size)
                                        .text_color(c.code_language_input_text),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_code_block_collapse_toggle),
                                ),
                        );
                    }

                    let language_badge = div()
                        .id("code-block-language")
                        .h(px(badge_height))
                        .flex()
                        .items_center()
                        .px(px(d.code_language_input_padding_x))
                        .rounded(px(d.code_language_input_radius))
                        .border(px(d.code_language_input_border_width))
                        .border_color(c.code_language_input_border.opacity(0.65))
                        .bg(c.code_language_input_bg.opacity(0.92))
                        .hover(|this| this.bg(c.code_language_input_border.opacity(0.35)))
                        .active(|this| this.opacity(0.92))
                        .text_size(icon_size)
                        .text_color(c.code_language_input_text)
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .occlude()
                        .child(language_label)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_code_language_badge_mouse_down),
                        );

                    code_content = code_content.child(
                        controls
                            .child(language_badge)
                            .child(
                                div()
                                    .id("code-block-copy")
                                    .w(px(badge_height))
                                    .h(px(badge_height))
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.code_language_input_radius))
                                    .border(px(d.code_language_input_border_width))
                                    .border_color(c.code_language_input_border.opacity(0.65))
                                    .bg(c.code_language_input_bg.opacity(0.92))
                                    .hover(|this| this.bg(c.code_language_input_border.opacity(0.35)))
                                    .active(|this| this.opacity(0.92))
                                    .cursor_pointer()
                                    .occlude()
                                    .child(
                                        svg()
                                            .path(ICON_CODE_BLOCK_COPY)
                                            .size(icon_size)
                                            .text_color(c.code_language_input_text),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |_, _window, cx| {
                                            cx.stop_propagation();
                                            let _ = block.update(cx, |block, cx| {
                                                block.on_code_block_copy_click(cx);
                                            });
                                        },
                                    ),
                            ),
                    );
                }

                let allow_code_execution = read_app_preferences()
                    .unwrap_or_default()
                    .allow_code_execution;
                let run_lane_width = if allow_code_execution {
                    px(badge_height + 6.0)
                } else {
                    px(0.0)
                };
                let run_icon_top = px(8.0);
                let run_icon_size = px((t.code_size + 3.0).max(14.0));
                let run_snapshot = self.code_run_snapshot.clone();
                let running = run_snapshot.status == CodeRunStatus::Running;

                let code_content_lane = div()
                    .flex_grow()
                    .min_w(px(0.0))
                    .child(code_content);

                let code_row = if allow_code_execution {
                    div()
                        .relative()
                        .w_full()
                        .flex()
                        .flex_row()
                        .child(
                            div()
                                .flex_none()
                                .flex_shrink_0()
                                .w(run_lane_width)
                                .relative()
                                .bg(c.code_language_input_bg)
                                .border_r(px(1.0))
                                .border_color(c.code_language_input_border.opacity(0.35))
                                .child(
                                    div()
                                        .id("code-block-run")
                                        .absolute()
                                        .top(run_icon_top)
                                        .left_0()
                                        .right_0()
                                        .h(px(badge_height))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .opacity(if running { 1.0 } else { 0.72 })
                                        .hover(|this| this.opacity(1.0))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::on_code_block_run_mouse_down),
                                        )
                                        .child(
                                            svg()
                                                .path(ICON_CODE_BLOCK_RUN)
                                                .size(run_icon_size)
                                                .text_color(c.code_language_input_text),
                                        ),
                                ),
                        )
                        .child(code_content_lane)
                } else {
                    div()
                        .relative()
                        .w_full()
                        .child(code_content_lane)
                };

                let mut code_shell = div().w_full().flex().flex_col().child(code_row);
                if run_snapshot.shows_output_panel() {
                    code_shell = code_shell.child(self.render_code_run_output_panel(
                        &theme,
                        &strings,
                        run_lane_width,
                        cx,
                    ));
                }

                focused_base
                    .rounded_sm()
                    .overflow_hidden()
                    .text_size(px(t.code_size))
                    .text_color(c.code_text)
                    .line_height(relative(t.text_line_height))
                    .child(code_shell)
                    .into_any_element()
    }
}
