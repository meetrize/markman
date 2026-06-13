//! Editor-level overlay for the code-block language picker.

use gpui::*;

use crate::components::{Block, CODE_LANGUAGE_MENU_OPTIONS};
use crate::theme::Theme;

use super::Editor;

const VIEWPORT_MARGIN: f32 = 8.0;

impl Editor {
    pub(super) fn render_code_language_menu_overlay(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let block = self.code_language_menu_block(cx)?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let icon_size = px((theme.typography.code_size - 1.0).max(10.0));

        let bounds = block
            .read(cx)
            .last_bounds
            .or(block.read(cx).interaction_bounds)?;
        let anchor_right = f32::from(bounds.right());
        let anchor_top = f32::from(bounds.top());
        let current_language = block.read(cx).code_language_text().to_string();

        let badge_height = d.code_language_input_height
            + d.code_language_input_padding_y * 2.0
            + d.code_language_input_border_width * 2.0;
        let panel_width = d.code_language_input_width;
        let item_count = CODE_LANGUAGE_MENU_OPTIONS.len();
        let menu_height = d.menu_panel_padding * 2.0
            + item_count as f32 * d.menu_item_height
            + (item_count.saturating_sub(1) as f32) * d.menu_panel_gap;
        let gap = d.code_language_input_gap;

        let viewport = window.viewport_size();
        let viewport_width = f32::from(viewport.width);
        let viewport_height = f32::from(viewport.height);

        let below_y = anchor_top + badge_height + gap;
        let above_y = anchor_top - menu_height - gap;
        let space_below = viewport_height - below_y - VIEWPORT_MARGIN;
        let space_above = anchor_top - VIEWPORT_MARGIN;
        let open_upward = space_below < menu_height && space_above > space_below;

        let mut panel_y = if open_upward { above_y } else { below_y };
        panel_y = panel_y
            .max(VIEWPORT_MARGIN)
            .min((viewport_height - menu_height - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN));

        let mut panel_x = anchor_right - panel_width;
        panel_x = panel_x
            .max(VIEWPORT_MARGIN)
            .min((viewport_width - panel_width - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN));

        let block_entity = block.clone();
        let items = CODE_LANGUAGE_MENU_OPTIONS
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let selected = current_language == *option;
                let option_label = SharedString::from(*option);
                let item_block = block_entity.clone();
                div()
                    .id(("code-block-language-item", index))
                    .min_h(px(d.menu_item_height))
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .rounded(px(d.menu_item_radius))
                    .cursor_pointer()
                    .bg(if selected {
                        c.selection.opacity(0.35)
                    } else {
                        c.dialog_surface
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .text_size(icon_size)
                    .text_color(c.code_language_input_text)
                    .child(option_label.clone())
                    .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                        cx.stop_propagation();
                        let language = option_label.clone();
                        let _ = item_block.update(cx, |block, cx| {
                            block.set_code_language(&language, cx);
                        });
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        Some(
            div()
                .id("code-block-language-menu-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_context_menu_overlay),
                )
                .child(
                    div()
                        .id("code-block-language-menu")
                        .absolute()
                        .left(px(panel_x))
                        .top(px(panel_y))
                        .min_w(px(panel_width))
                        .p(px(d.menu_panel_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.menu_panel_gap))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                            cx.stop_propagation();
                        })
                        .children(items),
                )
                .into_any_element(),
        )
    }

    pub(super) fn code_language_menu_block(&self, cx: &App) -> Option<Entity<Block>> {
        for visible in self.document.flatten_visible_blocks() {
            if visible.entity.read(cx).code_language_menu_open {
                return Some(visible.entity.clone());
            }
        }
        for binding in self.table_cells.values() {
            if binding.cell.read(cx).code_language_menu_open {
                return Some(binding.cell.clone());
            }
        }
        None
    }
}
