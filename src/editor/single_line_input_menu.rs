//! Right-click context menu for unified single-line inputs.

use gpui::*;

use super::single_line_input::SingleLineInputTarget;
use super::Editor;
use crate::i18n::I18nManager;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SingleLineInputContextMenuState {
    pub target: SingleLineInputTarget,
    pub position: Point<Pixels>,
}

impl Editor {
    pub(super) fn single_line_input_is_visible(&self, target: SingleLineInputTarget) -> bool {
        match target {
            SingleLineInputTarget::WorkspaceSearch => self.workspace_search_is_open(),
            SingleLineInputTarget::DocumentSearch => self.search.state.open,
            SingleLineInputTarget::WorkspaceName => self.workspace.name_dialog.is_some(),
            SingleLineInputTarget::QuickFileOpen => self.quick_file_open.open,
        }
    }

    fn single_line_input_has_selection(&self, target: SingleLineInputTarget) -> bool {
        match target {
            SingleLineInputTarget::WorkspaceSearch => self.workspace_search_has_selection(),
            SingleLineInputTarget::DocumentSearch => {
                !self.search.state.input.selected_range.is_empty()
            }
            SingleLineInputTarget::WorkspaceName => self
                .workspace.name_dialog
                .as_ref()
                .is_some_and(|dialog| !dialog.input.selected_range.is_empty()),
            SingleLineInputTarget::QuickFileOpen => false,
        }
    }

    fn single_line_input_can_paste(&self, cx: &App) -> bool {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .is_some()
    }

    pub(super) fn close_single_line_input_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.single_line_input_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn on_single_line_input_context_menu_mouse_down(
        &mut self,
        target: SingleLineInputTarget,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Right || !self.single_line_input_is_visible(target) {
            return;
        }

        cx.stop_propagation();
        self.context_menu = None;
        self.close_workspace_file_context_menu(cx);

        window.focus(&self.single_line_input_focus_handle(target));
        match target {
            SingleLineInputTarget::WorkspaceSearch => {
                self.workspace_search_prepare_context_menu(event.position);
            }
            SingleLineInputTarget::DocumentSearch => {
                self.document_search_prepare_context_menu(event.position);
            }
            SingleLineInputTarget::WorkspaceName => {
                self.workspace_name_prepare_context_menu(event.position);
            }
            SingleLineInputTarget::QuickFileOpen => {}
        }

        self.single_line_input_context_menu = Some(SingleLineInputContextMenuState {
            target,
            position: event.position,
        });
        cx.notify();
    }

    pub(super) fn on_dismiss_single_line_input_context_menu(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_single_line_input_context_menu(cx);
    }

    fn single_line_input_perform_copy(&mut self, target: SingleLineInputTarget, cx: &mut Context<Self>) {
        match target {
            SingleLineInputTarget::WorkspaceSearch => {
                self.workspace_search_copy_to_clipboard(cx);
            }
            SingleLineInputTarget::DocumentSearch => {
                self.document_search_copy_to_clipboard(cx);
            }
            SingleLineInputTarget::WorkspaceName => {
                self.workspace_name_copy_to_clipboard(cx);
            }
            SingleLineInputTarget::QuickFileOpen => {}
        }
    }

    fn single_line_input_perform_cut(&mut self, target: SingleLineInputTarget, cx: &mut Context<Self>) {
        match target {
            SingleLineInputTarget::WorkspaceSearch => {
                self.workspace_search_cut_to_clipboard(cx);
            }
            SingleLineInputTarget::DocumentSearch => {
                self.document_search_cut_to_clipboard(cx);
            }
            SingleLineInputTarget::WorkspaceName => {
                self.workspace_name_cut_to_clipboard(cx);
            }
            SingleLineInputTarget::QuickFileOpen => {}
        }
    }

    fn single_line_input_perform_paste(&mut self, target: SingleLineInputTarget, cx: &mut Context<Self>) {
        match target {
            SingleLineInputTarget::WorkspaceSearch => {
                self.workspace_search_paste_from_clipboard(cx);
            }
            SingleLineInputTarget::DocumentSearch => {
                self.document_search_paste_from_clipboard(cx);
            }
            SingleLineInputTarget::WorkspaceName => {
                self.workspace_name_paste_from_clipboard(cx);
            }
            SingleLineInputTarget::QuickFileOpen => {}
        }
    }

    pub(super) fn on_single_line_input_menu_copy(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.single_line_input_context_menu else {
            return;
        };
        self.single_line_input_perform_copy(menu.target, cx);
        self.close_single_line_input_context_menu(cx);
    }

    pub(super) fn on_single_line_input_menu_cut(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.single_line_input_context_menu else {
            return;
        };
        self.single_line_input_perform_cut(menu.target, cx);
        self.close_single_line_input_context_menu(cx);
    }

    pub(super) fn on_single_line_input_menu_paste(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.single_line_input_context_menu else {
            return;
        };
        self.single_line_input_perform_paste(menu.target, cx);
        self.close_single_line_input_context_menu(cx);
    }

    pub(super) fn render_single_line_input_context_menu_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.single_line_input_context_menu.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let has_selection = self.single_line_input_has_selection(menu.target);
        let can_paste = self.single_line_input_can_paste(cx);

        let items = vec![
            menu_item(
                "single-line-input-menu-copy",
                s.preferences_shortcut_copy.clone(),
                has_selection,
                cx.listener(Self::on_single_line_input_menu_copy),
                c,
                d,
                t,
            ),
            menu_item(
                "single-line-input-menu-cut",
                s.preferences_shortcut_cut.clone(),
                has_selection,
                cx.listener(Self::on_single_line_input_menu_cut),
                c,
                d,
                t,
            ),
            menu_item(
                "single-line-input-menu-paste",
                s.preferences_shortcut_paste.clone(),
                can_paste,
                cx.listener(Self::on_single_line_input_menu_paste),
                c,
                d,
                t,
            ),
        ];

        let panel_x = menu.position.x;
        let panel_y = menu.position.y;
        let panel_width = px(d.context_menu_panel_width.max(168.0));

        Some(
            div()
                .id("single-line-input-menu-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_single_line_input_context_menu),
                )
                .child(
                    div()
                        .id("single-line-input-menu-panel")
                        .absolute()
                        .left(panel_x)
                        .top(panel_y)
                        .w(panel_width)
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
}

fn menu_item(
    id: &'static str,
    label: String,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    c: &crate::theme::ThemeColors,
    d: &crate::theme::ThemeDimensions,
    t: &crate::theme::ThemeTypography,
) -> AnyElement {
    if enabled {
        div()
            .id(id)
            .h(px(d.menu_item_height))
            .px(px(d.menu_item_padding_x))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .text_size(px(d.menu_text_size))
            .font_weight(t.dialog_body_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(label)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .on_click(on_click)
            .into_any_element()
    } else {
        div()
            .id(id)
            .h(px(d.menu_item_height))
            .px(px(d.menu_item_padding_x))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .text_size(px(d.menu_text_size))
            .font_weight(t.dialog_body_weight.to_font_weight())
            .text_color(c.dialog_muted)
            .child(label)
            .into_any_element()
    }
}
