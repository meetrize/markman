//! Shared icon buttons for editor toolbars.

use gpui::*;

use crate::theme::Theme;

/// Square toolbar icon button styled like format-toolbar history controls.
pub fn toolbar_icon_button(
    id: impl Into<ElementId>,
    theme: &Theme,
    icon_path: impl Into<SharedString>,
    active: bool,
    disabled: bool,
    tooltip: impl Into<SharedString>,
    muted_disabled_icon: bool,
) -> Stateful<Div> {
    let _tooltip = tooltip.into();
    let c = &theme.colors;
    let d = &theme.dimensions;
    let icon_size = px(d.format_toolbar_icon_size);
    let button_size = px(d.format_toolbar_button_height);
    let bg = if active {
        c.selection.opacity(0.35)
    } else {
        c.dialog_surface
    };
    let hover_bg = if active {
        c.selection.opacity(0.5)
    } else {
        c.dialog_secondary_button_hover
    };
    let icon_color = if disabled && muted_disabled_icon {
        c.dialog_muted
    } else {
        c.dialog_secondary_button_text
    };

    let mut button = div()
        .id(id)
        .w(button_size)
        .h(button_size)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(d.format_toolbar_button_radius))
        .bg(bg)
        .child(
            svg()
                .path(icon_path.into())
                .size(icon_size)
                .text_color(icon_color),
        );

    if disabled {
        button = button.opacity(0.45);
    } else {
        button = button
            .hover(|this| this.bg(hover_bg))
            .active(|this| this.opacity(0.92))
            .cursor_pointer();
    }

    button
}

/// Compact icon+label button for floating AI selection toolbars.
pub fn toolbar_icon_label_button(
    id: impl Into<ElementId>,
    icon_path: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
    tooltip: impl Into<SharedString>,
) -> Stateful<Div> {
    let _tooltip = tooltip.into();
    let label = label.into();
    let c = &theme.colors;
    let d = &theme.dimensions;
    div()
        .id(id)
        .h(px(26.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .rounded(px(d.format_toolbar_button_radius))
        .bg(c.dialog_surface)
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .active(|this| this.opacity(0.92))
        .cursor_pointer()
        .text_size(px(12.0))
        .text_color(c.dialog_secondary_button_text)
        .child(
            svg()
                .path(icon_path.into())
                .size(px(14.0))
                .text_color(c.dialog_secondary_button_text),
        )
        .child(label)
}
