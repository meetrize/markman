//! Shared icon buttons for editor toolbars.

use gpui::*;

use crate::theme::{Theme, ThemeColors, ThemeTypography};

/// Square toolbar icon button styled like format-toolbar history controls.
pub(in crate::editor) fn toolbar_icon_button(
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

#[derive(Clone, Copy, Debug)]
pub(in crate::editor) struct ToolbarIconLabelStyle {
    pub height: f32,
    pub horizontal_padding: f32,
    pub icon_size: f32,
    pub bold_label: bool,
}

impl ToolbarIconLabelStyle {
    pub(in crate::editor) fn floating_toolbar() -> Self {
        Self {
            height: 26.0,
            horizontal_padding: 8.0,
            icon_size: 14.0,
            bold_label: false,
        }
    }

    pub(in crate::editor) fn format_toolbar(theme: &Theme) -> Self {
        Self {
            height: theme.dimensions.format_toolbar_button_height,
            horizontal_padding: 10.0,
            icon_size: theme.dimensions.format_toolbar_icon_size,
            bold_label: true,
        }
    }
}

/// Compact icon+label button for floating AI selection toolbars.
pub(in crate::editor) fn toolbar_icon_label_button(
    id: impl Into<ElementId>,
    icon_path: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
    tooltip: impl Into<SharedString>,
) -> Stateful<Div> {
    toolbar_icon_label_button_styled(
        id,
        icon_path,
        label,
        theme,
        tooltip,
        ToolbarIconLabelStyle::floating_toolbar(),
    )
}

pub(in crate::editor) fn toolbar_icon_label_button_styled(
    id: impl Into<ElementId>,
    icon_path: impl Into<SharedString>,
    label: impl Into<SharedString>,
    theme: &Theme,
    tooltip: impl Into<SharedString>,
    style: ToolbarIconLabelStyle,
) -> Stateful<Div> {
    let _tooltip = tooltip.into();
    let label = label.into();
    let c = &theme.colors;
    let d = &theme.dimensions;
    let mut button = div()
        .id(id)
        .h(px(style.height))
        .px(px(style.horizontal_padding))
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
                .size(px(style.icon_size))
                .text_color(c.dialog_secondary_button_text),
        )
        .child(label);
    if style.bold_label {
        button = button.font_weight(FontWeight::BOLD);
    }
    button
}

pub(in crate::editor) fn ai_toolbar_action_button(
    id: impl Into<ElementId>,
    icon_path: String,
    label: impl Into<SharedString>,
    theme: &Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    toolbar_icon_label_button(id, icon_path, label, theme, "")
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window, cx);
        })
}

struct EditorToolbarTooltip {
    text: SharedString,
    colors: ThemeColors,
    typography: ThemeTypography,
}

impl Render for EditorToolbarTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.colors;
        let t = &self.typography;
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(4.0))
            .bg(c.dialog_surface)
            .border(px(1.0))
            .border_color(c.dialog_border.opacity(0.75))
            .text_size(px(t.text_size * 0.75))
            .text_color(c.text_default)
            .child(self.text.clone())
    }
}

fn editor_toolbar_tooltip(
    text: impl Into<SharedString>,
    theme: &Theme,
) -> impl Fn(&mut Window, &mut App) -> AnyView + Clone + 'static {
    let text = text.into();
    let colors = theme.colors.clone();
    let typography = theme.typography.clone();
    move |_window, cx| {
        cx.new(|_cx| EditorToolbarTooltip {
            text: text.clone(),
            colors: colors.clone(),
            typography: typography.clone(),
        })
        .into()
    }
}

/// Icon-only button for floating AI selection toolbars, with hover tooltip.
pub(in crate::editor) fn ai_toolbar_icon_button(
    id: impl Into<ElementId>,
    icon_path: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    theme: &Theme,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let style = ToolbarIconLabelStyle::floating_toolbar();
    let c = &theme.colors;
    let d = &theme.dimensions;
    div()
        .id(id)
        .size(px(style.height))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(px(d.format_toolbar_button_radius))
        .bg(c.dialog_surface)
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .active(|this| this.opacity(0.92))
        .cursor_pointer()
        .tooltip(editor_toolbar_tooltip(tooltip, theme))
        .child(
            svg()
                .path(icon_path.into())
                .size(px(style.icon_size))
                .text_color(c.dialog_secondary_button_text),
        )
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            action(window, cx);
        })
}
