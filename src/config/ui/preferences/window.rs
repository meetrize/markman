//! GPUI preferences window and form controls.

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;

use super::toolbar_text_input::{ToolbarTextField, ToolbarTextInputState};
use crate::config::ai_toolbar::{
    AiSelectionToolbarBuiltin, AiSelectionToolbarButton, AI_TOOLBAR_ICON_OPTIONS,
    default_ai_selection_toolbar_buttons,
};
use crate::app_identity::app_window_title;
use crate::components::{
    CloseWindow, QuitApplication, ShortcutCategory, ShortcutCommand, ShortcutDefinition,
    install_keybindings, normalize_shortcut_config, normalize_shortcut_keys, resolved_shortcut_keys,
    shortcut_conflict_for, shortcut_definitions,
};
use crate::config::store::{
    AiPreferences, AppPreferences, DEFAULT_THEME_ID, StartupOpenPreference, read_app_preferences, save_preferences_from_window,
};
use crate::editor::Editor;
use crate::i18n::I18nManager;
use crate::input::single_line_field::SingleLineFieldState;
use crate::input::text_norm::flatten_paste_to_single_line;
use crate::theme::{Theme, ThemeCatalogEntry, ThemeManager};
use crate::window_chrome::{
    custom_titlebar_height, render_custom_titlebar, velotype_window_options,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreferencesNav {
    File,
    Theme,
    Ai,
    Shortcuts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiPreferenceField {
    Provider,
    BaseUrl,
    Model,
    ApiKeyEnv,
}

#[derive(Debug)]
struct AiPreferenceInputState {
    focus_handle: FocusHandle,
    active_field: Option<AiPreferenceField>,
    input: SingleLineFieldState,
}

impl AiPreferenceInputState {
    fn new(cx: &mut Context<PreferencesWindow>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            active_field: None,
            input: SingleLineFieldState::new(),
        }
    }
}

/// Independent preferences window view.
pub(crate) struct PreferencesWindow {
    pub(in crate::config::ui::preferences) nav: PreferencesNav,
    startup_open: StartupOpenPreference,
    allow_code_execution: bool,
    inline_code_run_in_system_terminal: bool,
    pub(in crate::config::ui::preferences) ai: AiPreferences,
    selected_theme_id: String,
    keybindings: BTreeMap<String, Vec<String>>,
    saved_startup_open: StartupOpenPreference,
    saved_allow_code_execution: bool,
    saved_inline_code_run_in_system_terminal: bool,
    saved_ai: AiPreferences,
    saved_theme_id: String,
    saved_keybindings: BTreeMap<String, Vec<String>>,
    theme_options: Vec<ThemeCatalogEntry>,
    focus_handle: FocusHandle,
    startup_dropdown_open: bool,
    theme_dropdown_open: bool,
    recording_shortcut: Option<ShortcutCommand>,
    shortcut_error: Option<String>,
    ai_input: AiPreferenceInputState,
    pub(in crate::config::ui::preferences) toolbar_text_focus: FocusHandle,
    pub(in crate::config::ui::preferences) toolbar_text_input: ToolbarTextInputState,
    pub(in crate::config::ui::preferences) toolbar_icon_dropdown_open: Option<usize>,
}

impl PreferencesWindow {
    fn new(
        preferences: AppPreferences,
        theme_options: Vec<ThemeCatalogEntry>,
        initial_nav: PreferencesNav,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected_theme_id = if theme_options
            .iter()
            .any(|entry| entry.id == preferences.default_theme_id)
        {
            preferences.default_theme_id
        } else {
            DEFAULT_THEME_ID.into()
        };
        let startup_open = preferences.startup_open;
        let allow_code_execution = preferences.allow_code_execution;
        let inline_code_run_in_system_terminal = preferences.inline_code_run_in_system_terminal;
        let ai = preferences.ai;
        let keybindings = preferences.keybindings;
        Self {
            nav: initial_nav,
            startup_open,
            allow_code_execution,
            inline_code_run_in_system_terminal,
            ai: ai.clone(),
            selected_theme_id: selected_theme_id.clone(),
            keybindings: keybindings.clone(),
            saved_startup_open: startup_open,
            saved_allow_code_execution: allow_code_execution,
            saved_inline_code_run_in_system_terminal: inline_code_run_in_system_terminal,
            saved_ai: ai,
            saved_theme_id: selected_theme_id,
            saved_keybindings: keybindings,
            theme_options,
            focus_handle: cx.focus_handle(),
            startup_dropdown_open: false,
            theme_dropdown_open: false,
            recording_shortcut: None,
            shortcut_error: None,
            ai_input: AiPreferenceInputState::new(cx),
            toolbar_text_focus: cx.focus_handle(),
            toolbar_text_input: ToolbarTextInputState::new(),
            toolbar_icon_dropdown_open: None,
        }
    }

    fn selected_theme_name(&self) -> String {
        self.theme_options
            .iter()
            .find(|entry| entry.id == self.selected_theme_id)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "Markman".into())
    }

    fn has_unsaved_changes(&self) -> bool {
        self.startup_open != self.saved_startup_open
            || self.allow_code_execution != self.saved_allow_code_execution
            || self.inline_code_run_in_system_terminal
                != self.saved_inline_code_run_in_system_terminal
            || self.ai != self.saved_ai
            || self.selected_theme_id != self.saved_theme_id
            || normalize_shortcut_config(&self.keybindings)
                != normalize_shortcut_config(&self.saved_keybindings)
    }

    fn set_nav_file(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::File;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.recording_shortcut = None;
        self.clear_all_ai_text_input_state();
        cx.notify();
    }

    fn set_nav_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Theme;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.recording_shortcut = None;
        self.clear_all_ai_text_input_state();
        cx.notify();
    }

    fn set_nav_ai(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Ai;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.recording_shortcut = None;
        cx.notify();
    }

    fn set_nav_shortcuts(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Shortcuts;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.shortcut_error = None;
        self.clear_all_ai_text_input_state();
        cx.notify();
    }

    fn show_ai_settings(&mut self, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Ai;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.recording_shortcut = None;
        self.toolbar_icon_dropdown_open = None;
        self.clear_all_ai_text_input_state();
        cx.notify();
    }

    pub(in crate::config::ui::preferences) fn clear_ai_input_state(&mut self) {
        self.ai_input.active_field = None;
        self.ai_input.input.clear_selection_and_layout();
    }

    fn clear_all_ai_text_input_state(&mut self) {
        self.clear_ai_input_state();
        self.clear_toolbar_text_input_state();
    }

    fn ai_field_text(&self, field: AiPreferenceField) -> &str {
        match field {
            AiPreferenceField::Provider => &self.ai.provider,
            AiPreferenceField::BaseUrl => &self.ai.base_url,
            AiPreferenceField::Model => &self.ai.model,
            AiPreferenceField::ApiKeyEnv => &self.ai.api_key_env,
        }
    }

    fn ai_field_text_mut(&mut self, field: AiPreferenceField) -> &mut String {
        match field {
            AiPreferenceField::Provider => &mut self.ai.provider,
            AiPreferenceField::BaseUrl => &mut self.ai.base_url,
            AiPreferenceField::Model => &mut self.ai.model,
            AiPreferenceField::ApiKeyEnv => &mut self.ai.api_key_env,
        }
    }

    fn ai_input_active(&self, window: &Window) -> bool {
        self.nav == PreferencesNav::Ai && self.ai_input.focus_handle.is_focused(window)
    }

    fn replace_ai_field_text(
        &mut self,
        field: AiPreferenceField,
        range: Range<usize>,
        new_text: &str,
        mark_inserted_text: bool,
        selected_after: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let text = self.ai_field_text_mut(field);
        let start = range.start.min(text.len());
        let end = range.end.min(text.len());
        text.replace_range(start..end, new_text);
        let inserted_end = start + new_text.len();
        self.ai_input.input.selected_range = selected_after.unwrap_or(inserted_end..inserted_end);
        self.ai_input.input.selection_reversed = false;
        self.ai_input.input.marked_range = if mark_inserted_text && !new_text.is_empty() {
            Some(start..inserted_end)
        } else {
            None
        };
        self.ai_input.input.is_selecting = false;
        cx.notify();
    }

    fn activate_ai_input_at(
        &mut self,
        field: AiPreferenceField,
        offset: usize,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text_len = self.ai_field_text(field).len();
        if self.ai_input.active_field != Some(field) {
            self.ai_input.input.selected_range = text_len..text_len;
            self.ai_input.input.selection_reversed = false;
            self.ai_input.input.marked_range = None;
        }
        self.ai_input.active_field = Some(field);
        self.clear_toolbar_text_input_state();
        window.focus(&self.ai_input.focus_handle);
        if shift {
            extend_selection(
                &mut self.ai_input.input.selected_range,
                &mut self.ai_input.input.selection_reversed,
                offset,
                text_len,
            );
        } else {
            self.ai_input.input.selected_range = offset.min(text_len)..offset.min(text_len);
            self.ai_input.input.selection_reversed = false;
        }
        self.ai_input.input.marked_range = None;
        self.ai_input.input.is_selecting = true;
        cx.notify();
    }

    fn finish_ai_input_selection(&mut self, cx: &mut Context<Self>) {
        if self.ai_input.input.is_selecting {
            self.ai_input.input.is_selecting = false;
            cx.notify();
        }
    }

    fn drag_ai_input_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(field) = self.ai_input.active_field else {
            return;
        };
        if !self.ai_input.input.is_selecting {
            return;
        }
        let text_len = self.ai_field_text(field).len();
        extend_selection(
            &mut self.ai_input.input.selected_range,
            &mut self.ai_input.input.selection_reversed,
            offset,
            text_len,
        );
        self.ai_input.input.marked_range = None;
        cx.notify();
    }

    fn set_ai_input_layout(&mut self, field: AiPreferenceField, line: ShapedLine, bounds: Bounds<Pixels>) {
        if self.ai_input.active_field == Some(field) {
            self.ai_input.input.set_layout(line, bounds);
        }
    }

    fn on_preferences_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.recording_shortcut.is_some() {
            self.capture_shortcut_key(event, window, cx);
            return;
        }
        if !self.ai_input_active(window) {
            return;
        }
        if self.handle_ai_input_key_down(event, cx) {
            cx.stop_propagation();
        }
    }

    fn handle_ai_input_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(field) = self.ai_input.active_field else {
            return false;
        };
        if event.is_held {
            return true;
        }
        let modifiers = event.keystroke.modifiers;
        let primary = (modifiers.platform || modifiers.control) && !modifiers.alt && !modifiers.function;
        let text = self.ai_field_text(field).to_string();
        let len = text.len();
        match event.keystroke.key.as_str() {
            "backspace" => {
                let range = if self.ai_input.input.selected_range.is_empty() {
                    let cursor = caret_offset(&self.ai_input.input.selected_range, self.ai_input.input.selection_reversed);
                    previous_char_boundary(&text, cursor)..cursor
                } else {
                    self.ai_input.input.selected_range.clone()
                };
                self.replace_ai_field_text(field, range, "", false, None, cx);
                true
            }
            "delete" => {
                let range = if self.ai_input.input.selected_range.is_empty() {
                    let cursor = caret_offset(&self.ai_input.input.selected_range, self.ai_input.input.selection_reversed);
                    cursor..next_char_boundary(&text, cursor)
                } else {
                    self.ai_input.input.selected_range.clone()
                };
                self.replace_ai_field_text(field, range, "", false, None, cx);
                true
            }
            "left" => {
                let cursor = caret_offset(&self.ai_input.input.selected_range, self.ai_input.input.selection_reversed);
                let next = previous_char_boundary(&text, cursor);
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.input.selected_range, &mut self.ai_input.input.selection_reversed, next, len);
                } else {
                    self.ai_input.input.selected_range = next..next;
                    self.ai_input.input.selection_reversed = false;
                }
                self.ai_input.input.marked_range = None;
                cx.notify();
                true
            }
            "right" => {
                let cursor = caret_offset(&self.ai_input.input.selected_range, self.ai_input.input.selection_reversed);
                let next = next_char_boundary(&text, cursor);
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.input.selected_range, &mut self.ai_input.input.selection_reversed, next, len);
                } else {
                    self.ai_input.input.selected_range = next..next;
                    self.ai_input.input.selection_reversed = false;
                }
                self.ai_input.input.marked_range = None;
                cx.notify();
                true
            }
            "home" => {
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.input.selected_range, &mut self.ai_input.input.selection_reversed, 0, len);
                } else {
                    self.ai_input.input.selected_range = 0..0;
                    self.ai_input.input.selection_reversed = false;
                }
                self.ai_input.input.marked_range = None;
                cx.notify();
                true
            }
            "end" => {
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.input.selected_range, &mut self.ai_input.input.selection_reversed, len, len);
                } else {
                    self.ai_input.input.selected_range = len..len;
                    self.ai_input.input.selection_reversed = false;
                }
                self.ai_input.input.marked_range = None;
                cx.notify();
                true
            }
            "a" if primary => {
                self.ai_input.input.selected_range = 0..len;
                self.ai_input.input.selection_reversed = false;
                self.ai_input.input.marked_range = None;
                cx.notify();
                true
            }
            "c" if primary => {
                if !self.ai_input.input.selected_range.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        text[self.ai_input.input.selected_range.clone()].to_string(),
                    ));
                }
                true
            }
            "x" if primary => {
                if !self.ai_input.input.selected_range.is_empty() {
                    let range = self.ai_input.input.selected_range.clone();
                    cx.write_to_clipboard(ClipboardItem::new_string(text[range.clone()].to_string()));
                    self.replace_ai_field_text(field, range, "", false, None, cx);
                }
                true
            }
            "v" if primary => {
                if let Some(value) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let range = self
                        .ai_input
                        .input
                        .marked_range
                        .clone()
                        .unwrap_or_else(|| self.ai_input.input.selected_range.clone());
                    self.replace_ai_field_text(field, range, &sanitize_single_line_text(&value), false, None, cx);
                }
                true
            }
            _ => false,
        }
    }

    fn toggle_startup_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.startup_dropdown_open = !self.startup_dropdown_open;
        self.theme_dropdown_open = false;
        cx.notify();
    }

    fn toggle_theme_dropdown(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.theme_dropdown_open = !self.theme_dropdown_open;
        self.startup_dropdown_open = false;
        cx.notify();
    }

    fn cancel(&mut self, _: &ClickEvent, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn on_titlebar_close(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if event.standard_click() {
            window.remove_window();
        }
    }

    fn on_quit_application(
        &mut self,
        _: &QuitApplication,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::app_menu::request_quit_application(cx);
        cx.stop_propagation();
    }

    fn on_close_window(
        &mut self,
        _: &CloseWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.remove_window();
        cx.stop_propagation();
    }

    fn save(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_unsaved_changes() {
            return;
        }

        let preferences = match save_preferences_from_window(
            self.startup_open,
            &self.selected_theme_id,
            self.keybindings.clone(),
            self.allow_code_execution,
            self.inline_code_run_in_system_terminal,
            self.ai.clone(),
        ) {
            Ok(preferences) => preferences,
            Err(err) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let ok = strings.info_dialog_ok;
                let buttons = [ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Critical,
                    &strings.preferences_save_failed_title,
                    Some(&err.to_string()),
                    &buttons,
                    cx,
                );
                return;
            }
        };

        self.apply_saved_preferences(preferences, window, cx);
    }

    fn apply_saved_preferences(
        &mut self,
        preferences: AppPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme_changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
            theme_manager.set_theme_by_id(&preferences.default_theme_id)
        });
        if !theme_changed {
            let _ = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
                theme_manager.set_theme_by_id(DEFAULT_THEME_ID)
            });
        }
        cx.clear_key_bindings();
        install_keybindings(cx, &preferences.keybindings);
        crate::app_menu::install_menus(cx);
        cx.refresh_windows();
        window.activate_window();
        self.focus_handle.focus(window);
        self.saved_startup_open = self.startup_open;
        self.saved_allow_code_execution = self.allow_code_execution;
        self.saved_inline_code_run_in_system_terminal = self.inline_code_run_in_system_terminal;
        self.saved_ai = self.ai.clone();
        self.saved_theme_id = self.selected_theme_id.clone();
        self.saved_keybindings = normalize_shortcut_config(&self.keybindings);
        cx.notify();
    }

    fn nav_button(
        &self,
        id: &'static str,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .h(px(34.0))
            .w(px(156.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_end()
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(if selected {
                c.dialog_primary_button_text
            } else {
                c.dialog_body
            })
            .bg(if selected {
                c.dialog_primary_button_bg
            } else {
                c.dialog_secondary_button_bg
            })
            .hover(move |this| {
                this.bg(if selected {
                    c.dialog_primary_button_hover
                } else {
                    c.dialog_secondary_button_hover
                })
            })
            .id(id)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    fn dropdown_button(
        id: &'static str,
        label: String,
        theme: &Theme,
        on_click: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .w(px(280.0))
            .min_h(px(36.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_between()
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.editor_background)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .id(id)
            .child(label)
            .child("v")
            .on_click(cx.listener(on_click))
    }

    fn dropdown_item(
        id: impl Into<ElementId>,
        label: String,
        selected: bool,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .w(px(280.0))
            .min_h(px(30.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .cursor_pointer()
            .bg(if selected {
                c.selection
            } else {
                c.dialog_surface
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .text_size(px(t.dialog_body_size))
            .text_color(c.dialog_body)
            .id(id)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    fn labeled_row(&self, label: &str, control: impl IntoElement, theme: &Theme) -> Div {
        let c = &theme.colors;
        let t = &theme.typography;
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(280.0))
                    .text_size(px(t.dialog_body_size))
                    .font_weight(t.dialog_button_weight.to_font_weight())
                    .text_color(c.dialog_title)
                    .child(SharedString::from(label.to_string())),
            )
            .child(control)
    }

    fn render_startup_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected = match self.startup_open {
            StartupOpenPreference::NewFile => strings.preferences_startup_new_file.clone(),
            StartupOpenPreference::LastOpenedFile => {
                strings.preferences_startup_last_opened_file.clone()
            }
        };
        let mut dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-startup-dropdown",
                selected,
                theme,
                Self::toggle_startup_dropdown,
                cx,
            ));
        if self.startup_dropdown_open {
            let new_file_label = strings.preferences_startup_new_file.clone();
            let last_file_label = strings.preferences_startup_last_opened_file.clone();
            dropdown = dropdown
                .child(Self::dropdown_item(
                    "preferences-startup-new-file",
                    new_file_label,
                    self.startup_open == StartupOpenPreference::NewFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::NewFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ))
                .child(Self::dropdown_item(
                    "preferences-startup-last-opened-file",
                    last_file_label,
                    self.startup_open == StartupOpenPreference::LastOpenedFile,
                    theme,
                    |this, _, _, cx| {
                        this.startup_open = StartupOpenPreference::LastOpenedFile;
                        this.startup_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
        }
        let allow_label = if self.allow_code_execution {
            strings.preferences_allow_code_execution_on.clone()
        } else {
            strings.preferences_allow_code_execution_off.clone()
        };
        let system_terminal_label = if self.inline_code_run_in_system_terminal {
            strings.preferences_allow_code_execution_on.clone()
        } else {
            strings.preferences_allow_code_execution_off.clone()
        };

        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(self.labeled_row(&strings.preferences_startup_option, dropdown, theme))
            .child({
                self.labeled_row(
                    &strings.preferences_allow_code_execution_label,
                    Self::dropdown_button(
                        "preferences-allow-code-execution",
                        allow_label,
                        theme,
                        |this, _, _, cx| {
                            this.allow_code_execution = !this.allow_code_execution;
                            cx.notify();
                        },
                        cx,
                    ),
                    theme,
                )
            })
            .child({
                self.labeled_row(
                    &strings.preferences_inline_code_system_terminal_label,
                    Self::dropdown_button(
                        "preferences-inline-code-system-terminal",
                        system_terminal_label,
                        theme,
                        |this, _, _, cx| {
                            this.inline_code_run_in_system_terminal =
                                !this.inline_code_run_in_system_terminal;
                            cx.notify();
                        },
                        cx,
                    ),
                    theme,
                )
            })
    }

    fn render_theme_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut dropdown = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(Self::dropdown_button(
                "preferences-theme-dropdown",
                self.selected_theme_name(),
                theme,
                Self::toggle_theme_dropdown,
                cx,
            ));
        if self.theme_dropdown_open {
            for (index, entry) in self.theme_options.clone().into_iter().enumerate() {
                let selected = entry.id == self.selected_theme_id;
                dropdown = dropdown.child(Self::dropdown_item(
                    ("preferences-theme-option", index),
                    entry.name,
                    selected,
                    theme,
                    move |this, _, _, cx| {
                        this.selected_theme_id = entry.id.clone();
                        this.theme_dropdown_open = false;
                        cx.notify();
                    },
                    cx,
                ));
            }
        }
        self.labeled_row(&strings.preferences_local_theme, dropdown, theme)
    }

    fn render_ai_text_field(
        &self,
        id: impl Into<ElementId>,
        field: AiPreferenceField,
        placeholder: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
        full_width: bool,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let mut field_container = div()
            .min_h(px(if full_width { 30.0 } else { 36.0 }))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .overflow_hidden()
            .id(id)
            .track_focus(&self.ai_input.focus_handle);
        field_container = if full_width {
            field_container.w_full()
        } else {
            field_container.w(px(280.0))
        };
        field_container.child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .overflow_hidden()
                    .child(AiPreferenceInputElement::new(
                        cx.entity(),
                        field,
                        SharedString::from(placeholder),
                    )),
            )
    }

    fn render_ai_page(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let enabled = |value| {
            if value {
                "Enabled".to_string()
            } else {
                "Disabled".to_string()
            }
        };

        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(self.labeled_row(
                "Provider",
                self.render_ai_text_field(
                    "preferences-ai-provider",
                    AiPreferenceField::Provider,
                    "openai-compatible",
                    theme,
                    cx,
                    false,
                ),
                theme,
            ))
            .child(self.labeled_row(
                "Base URL",
                self.render_ai_text_field(
                    "preferences-ai-base-url",
                    AiPreferenceField::BaseUrl,
                    "https://api.openai.com/v1",
                    theme,
                    cx,
                    false,
                ),
                theme,
            ))
            .child(self.labeled_row(
                "Model",
                self.render_ai_text_field(
                    "preferences-ai-model",
                    AiPreferenceField::Model,
                    "gpt-4o-mini",
                    theme,
                    cx,
                    false,
                ),
                theme,
            ))
            .child(self.labeled_row(
                "API key or env",
                self.render_ai_text_field(
                    "preferences-ai-api-key-env",
                    AiPreferenceField::ApiKeyEnv,
                    "OPENAI_API_KEY",
                    theme,
                    cx,
                    false,
                ),
                theme,
            ))
            .child(self.labeled_row(
                "Allow full document context",
                Self::dropdown_button(
                    "preferences-ai-full-document",
                    enabled(self.ai.allow_full_document_context),
                    theme,
                    |this, _, _, cx| {
                        this.ai.allow_full_document_context =
                            !this.ai.allow_full_document_context;
                        cx.notify();
                    },
                    cx,
                ),
                theme,
            ))
            .child(self.labeled_row(
                "Allow workspace context",
                Self::dropdown_button(
                    "preferences-ai-workspace",
                    enabled(self.ai.allow_workspace_context),
                    theme,
                    |this, _, _, cx| {
                        this.ai.allow_workspace_context = !this.ai.allow_workspace_context;
                        cx.notify();
                    },
                    cx,
                ),
                theme,
            ))
            .child(self.labeled_row(
                "Allow command context",
                Self::dropdown_button(
                    "preferences-ai-command",
                    enabled(self.ai.allow_command_context),
                    theme,
                    |this, _, _, cx| {
                        this.ai.allow_command_context = !this.ai.allow_command_context;
                        cx.notify();
                    },
                    cx,
                ),
                theme,
            ))
            .child({
                let toolbar_config = self.render_ai_toolbar_config(theme, cx);
                self.render_toolbar_text_input_shell(
                    theme,
                    cx,
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.colors.dialog_title)
                                .child("选区工具栏"),
                        )
                        .child(toolbar_config)
                        .child(
                            div()
                                .flex()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .id("preferences-ai-toolbar-add")
                                        .h(px(30.0))
                                        .px(px(12.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(theme.dimensions.menu_item_radius))
                                        .border(px(theme.dimensions.dialog_border_width))
                                        .border_color(theme.colors.dialog_border)
                                        .bg(theme.colors.dialog_secondary_button_bg)
                                        .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px(12.0))
                                        .text_color(theme.colors.dialog_secondary_button_text)
                                        .child("添加按钮")
                                        .on_click(cx.listener(Self::add_ai_toolbar_button)),
                                )
                                .child(
                                    div()
                                        .id("preferences-ai-toolbar-reset")
                                        .h(px(30.0))
                                        .px(px(12.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(theme.dimensions.menu_item_radius))
                                        .border(px(theme.dimensions.dialog_border_width))
                                        .border_color(theme.colors.dialog_border)
                                        .bg(theme.colors.dialog_secondary_button_bg)
                                        .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px(12.0))
                                        .text_color(theme.colors.dialog_secondary_button_text)
                                        .child("恢复默认")
                                        .on_click(cx.listener(Self::reset_ai_toolbar_buttons)),
                                ),
                        ),
                )
            })
    }

    fn render_ai_toolbar_config(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let mut rows = div().flex().flex_col().gap(px(8.0)).w_full();
        let button_count = self.ai.selection_toolbar.len();
        for index in 0..button_count {
            let button = self.ai.selection_toolbar[index].clone();
            let enabled_label = if button.enabled {
                "启用".to_string()
            } else {
                "禁用".to_string()
            };
            let button_icon = button.resolved_icon().to_string();
            let show_instruction = button.action != AiSelectionToolbarBuiltin::CustomPrompt.id()
                && (button.is_custom_action()
                    || button.instruction.is_some()
                    || AiSelectionToolbarBuiltin::from_id(&button.action).is_some());
            let icon_dropdown_open = self.toolbar_icon_dropdown_open == Some(index);

            let mut row = div()
                .id(("preferences-ai-toolbar-item", index))
                .w_full()
                .p(px(10.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .rounded(px(d.menu_item_radius))
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .bg(c.dialog_secondary_button_bg)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .id(("preferences-ai-toolbar-up", index))
                                .size(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.menu_item_radius))
                                .when(index > 0, |this| {
                                    this.bg(c.dialog_surface)
                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .on_click({
                                            let index = index;
                                            cx.listener(move |this, _, _, cx| {
                                                this.move_ai_toolbar_button(index, -1, cx);
                                            })
                                        })
                                })
                                .when(index == 0, |this| this.opacity(0.35))
                                .child(
                                    svg()
                                        .path("icon/toolbar/chevron-up.svg")
                                        .size(px(14.0))
                                        .text_color(c.dialog_secondary_button_text),
                                ),
                        )
                        .child(
                            div()
                                .id(("preferences-ai-toolbar-down", index))
                                .size(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.menu_item_radius))
                                .when(index + 1 < button_count, |this| {
                                    this.bg(c.dialog_surface)
                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .on_click({
                                            let index = index;
                                            cx.listener(move |this, _, _, cx| {
                                                this.move_ai_toolbar_button(index, 1, cx);
                                            })
                                        })
                                })
                                .when(index + 1 >= button_count, |this| this.opacity(0.35))
                                .child(
                                    svg()
                                        .path("icon/toolbar/chevron-down.svg")
                                        .size(px(14.0))
                                        .text_color(c.dialog_secondary_button_text),
                                ),
                        )
                        .child(
                            div()
                                .id(("preferences-ai-toolbar-enabled", index))
                                .h(px(26.0))
                                .px(px(8.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.menu_item_radius))
                                .bg(c.dialog_surface)
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .cursor_pointer()
                                .text_size(px(12.0))
                                .text_color(c.dialog_secondary_button_text)
                                .child(enabled_label)
                                .on_click({
                                    let index = index;
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_ai_toolbar_button_enabled(index, cx);
                                    })
                                }),
                        )
                        .child(
                            div()
                                .id(("preferences-ai-toolbar-icon", index))
                                .size(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(d.menu_item_radius))
                                .bg(c.dialog_surface)
                                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                .cursor_pointer()
                                .child(
                                    svg()
                                        .path(SharedString::from(button_icon.clone()))
                                        .size(px(14.0))
                                        .text_color(c.dialog_secondary_button_text),
                                )
                                .on_click({
                                    let index = index;
                                    cx.listener(move |this, _, _, cx| {
                                        this.toggle_ai_toolbar_icon_dropdown(index, cx);
                                    })
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(self.render_toolbar_text_field(
                                    ("preferences-ai-toolbar-label", index),
                                    ToolbarTextField::Label(index),
                                    "按钮名称",
                                    theme,
                                    cx,
                                )),
                        )
                        .when(button.is_removable(), |this| {
                            this.child(
                                div()
                                    .id(("preferences-ai-toolbar-remove", index))
                                    .size(px(26.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .child(
                                        svg()
                                            .path("icon/toolbar/x.svg")
                                            .size(px(14.0))
                                            .text_color(c.dialog_secondary_button_text),
                                    )
                                    .on_click({
                                        let index = index;
                                        cx.listener(move |this, _, _, cx| {
                                            this.remove_ai_toolbar_button(index, cx);
                                        })
                                    }),
                            )
                        }),
                );

            if icon_dropdown_open {
                row = row.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .children(AI_TOOLBAR_ICON_OPTIONS.iter().enumerate().map(
                            |(icon_index, icon_path)| {
                                let selected = button.icon == *icon_path;
                                div()
                                    .id(("ai-toolbar-icon-option", index * 100 + icon_index))
                                    .size(px(28.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.menu_item_radius))
                                    .bg(if selected {
                                        c.selection.opacity(0.35)
                                    } else {
                                        c.dialog_surface
                                    })
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .child(
                                        svg()
                                            .path(*icon_path)
                                            .size(px(14.0))
                                            .text_color(c.dialog_secondary_button_text),
                                    )
                                    .on_click({
                                        let index = index;
                                        let icon_path = (*icon_path).to_string();
                                        cx.listener(move |this, _, _, cx| {
                                            this.select_ai_toolbar_icon(index, icon_path.clone(), cx);
                                        })
                                    })
                            },
                        )),
                );
            }

            if show_instruction {
                row = row.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(c.dialog_muted)
                                .child(if button.is_custom_action() {
                                    "AI 指令"
                                } else {
                                    "指令覆盖（可选）"
                                }),
                        )
                        .child(self.render_toolbar_text_field(
                            ("preferences-ai-toolbar-instruction", index),
                            ToolbarTextField::Instruction(index),
                            if button.is_custom_action() {
                                "例如：将选中文本翻译为英文"
                            } else {
                                "留空则使用内置指令"
                            },
                            theme,
                            cx,
                        )),
                );
            }

            rows = rows.child(row);
        }
        rows
    }

    fn move_ai_toolbar_button(&mut self, index: usize, delta: isize, cx: &mut Context<Self>) {
        let target = index as isize + delta;
        if target < 0 || target as usize >= self.ai.selection_toolbar.len() {
            return;
        }
        self.ai
            .selection_toolbar
            .swap(index, target as usize);
        self.toolbar_icon_dropdown_open = None;
        cx.notify();
    }

    fn toggle_ai_toolbar_button_enabled(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(button) = self.ai.selection_toolbar.get_mut(index) {
            button.enabled = !button.enabled;
            cx.notify();
        }
    }

    fn toggle_ai_toolbar_icon_dropdown(&mut self, index: usize, cx: &mut Context<Self>) {
        self.toolbar_icon_dropdown_open = if self.toolbar_icon_dropdown_open == Some(index) {
            None
        } else {
            Some(index)
        };
        cx.notify();
    }

    fn select_ai_toolbar_icon(&mut self, index: usize, icon: String, cx: &mut Context<Self>) {
        if let Some(button) = self.ai.selection_toolbar.get_mut(index) {
            button.icon = icon;
        }
        self.toolbar_icon_dropdown_open = None;
        cx.notify();
    }

    fn remove_ai_toolbar_button(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.ai.selection_toolbar.len() {
            return;
        }
        if !self.ai.selection_toolbar[index].is_removable() {
            return;
        }
        self.ai.selection_toolbar.remove(index);
        self.toolbar_icon_dropdown_open = None;
        self.clear_all_ai_text_input_state();
        cx.notify();
    }

    fn add_ai_toolbar_button(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai.selection_toolbar.push(AiSelectionToolbarButton::new_custom(
            "新按钮".into(),
            "Describe what this button should do with the selected Markdown.".into(),
        ));
        self.toolbar_icon_dropdown_open = None;
        cx.notify();
    }

    fn reset_ai_toolbar_buttons(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai.selection_toolbar = default_ai_selection_toolbar_buttons();
        self.toolbar_icon_dropdown_open = None;
        self.clear_all_ai_text_input_state();
        cx.notify();
    }

    fn shortcut_category_label(
        category: ShortcutCategory,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match category {
            ShortcutCategory::File => strings.preferences_shortcuts_group_file.clone(),
            ShortcutCategory::Edit => strings.preferences_shortcuts_group_edit.clone(),
            ShortcutCategory::Navigation => strings.preferences_shortcuts_group_navigation.clone(),
            ShortcutCategory::Formatting => strings.preferences_shortcuts_group_formatting.clone(),
            ShortcutCategory::Block => strings.preferences_shortcuts_group_block.clone(),
            ShortcutCategory::Other => strings.preferences_shortcuts_group_other.clone(),
        }
    }

    fn shortcut_command_label(
        command: ShortcutCommand,
        strings: &crate::i18n::I18nStrings,
    ) -> String {
        match command {
            ShortcutCommand::Newline => strings.preferences_shortcut_newline.clone(),
            ShortcutCommand::DeleteBack => strings.preferences_shortcut_delete_back.clone(),
            ShortcutCommand::Delete => strings.preferences_shortcut_delete.clone(),
            ShortcutCommand::WordDeleteBack => {
                strings.preferences_shortcut_word_delete_back.clone()
            }
            ShortcutCommand::WordDeleteForward => {
                strings.preferences_shortcut_word_delete_forward.clone()
            }
            ShortcutCommand::FocusPrev => strings.preferences_shortcut_focus_prev.clone(),
            ShortcutCommand::FocusNext => strings.preferences_shortcut_focus_next.clone(),
            ShortcutCommand::MoveLeft => strings.preferences_shortcut_move_left.clone(),
            ShortcutCommand::MoveRight => strings.preferences_shortcut_move_right.clone(),
            ShortcutCommand::WordMoveLeft => strings.preferences_shortcut_word_move_left.clone(),
            ShortcutCommand::WordMoveRight => strings.preferences_shortcut_word_move_right.clone(),
            ShortcutCommand::Home => strings.preferences_shortcut_home.clone(),
            ShortcutCommand::End => strings.preferences_shortcut_end.clone(),
            ShortcutCommand::BlockUp => strings.preferences_shortcut_block_up.clone(),
            ShortcutCommand::BlockDown => strings.preferences_shortcut_block_down.clone(),
            ShortcutCommand::SelectLeft => strings.preferences_shortcut_select_left.clone(),
            ShortcutCommand::SelectRight => strings.preferences_shortcut_select_right.clone(),
            ShortcutCommand::WordSelectLeft => {
                strings.preferences_shortcut_word_select_left.clone()
            }
            ShortcutCommand::WordSelectRight => {
                strings.preferences_shortcut_word_select_right.clone()
            }
            ShortcutCommand::SelectHome => strings.preferences_shortcut_select_home.clone(),
            ShortcutCommand::SelectEnd => strings.preferences_shortcut_select_end.clone(),
            ShortcutCommand::SelectAll => strings.preferences_shortcut_select_all.clone(),
            ShortcutCommand::Copy => strings.preferences_shortcut_copy.clone(),
            ShortcutCommand::Cut => strings.preferences_shortcut_cut.clone(),
            ShortcutCommand::Paste => strings.preferences_shortcut_paste.clone(),
            ShortcutCommand::Undo => strings.preferences_shortcut_undo.clone(),
            ShortcutCommand::Redo => strings.preferences_shortcut_redo.clone(),
            ShortcutCommand::BoldSelection => strings.preferences_shortcut_bold_selection.clone(),
            ShortcutCommand::ItalicSelection => {
                strings.preferences_shortcut_italic_selection.clone()
            }
            ShortcutCommand::UnderlineSelection => {
                strings.preferences_shortcut_underline_selection.clone()
            }
            ShortcutCommand::CodeSelection => strings.preferences_shortcut_code_selection.clone(),
            ShortcutCommand::IndentBlock => strings.preferences_shortcut_indent_block.clone(),
            ShortcutCommand::OutdentBlock => strings.preferences_shortcut_outdent_block.clone(),
            ShortcutCommand::ExitCodeBlock => strings.preferences_shortcut_exit_code_block.clone(),
            ShortcutCommand::SaveDocument => strings.preferences_shortcut_save_document.clone(),
            ShortcutCommand::SaveDocumentAs => {
                strings.preferences_shortcut_save_document_as.clone()
            }
            ShortcutCommand::NewWindow => strings.preferences_shortcut_new_window.clone(),
            ShortcutCommand::OpenFile => strings.preferences_shortcut_open_file.clone(),
            ShortcutCommand::QuitApplication => {
                strings.preferences_shortcut_quit_application.clone()
            }
            ShortcutCommand::CloseWindow => strings.preferences_shortcut_close_window.clone(),
            ShortcutCommand::DismissTransientUi => {
                strings.preferences_shortcut_dismiss_transient_ui.clone()
            }
            ShortcutCommand::ToggleViewMode => {
                strings.preferences_shortcut_toggle_view_mode.clone()
            }
            ShortcutCommand::ToggleWorkspace => {
                strings.preferences_shortcut_toggle_workspace.clone()
            }
            ShortcutCommand::FindNextInDocument => {
                strings.preferences_shortcut_find_next_in_document.clone()
            }
            ShortcutCommand::FindPreviousInDocument => {
                strings.preferences_shortcut_find_previous_in_document.clone()
            }
            ShortcutCommand::QuickFileOpen => {
                strings.preferences_shortcut_quick_file_open.clone()
            }
            ShortcutCommand::OpenWorkspaceSearch => {
                strings.preferences_shortcut_open_workspace_search.clone()
            }
            ShortcutCommand::AskAi => "Ask AI".to_string(),
        }
    }

    fn format_template(template: &str, key: &str, value: &str) -> String {
        template.replace(key, value)
    }

    fn begin_recording_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording_shortcut = Some(command);
        self.shortcut_error = None;
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn reset_shortcut(
        &mut self,
        command: ShortcutCommand,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            self.keybindings.remove(definition.id);
        }
        if self.recording_shortcut == Some(command) {
            self.recording_shortcut = None;
        }
        self.shortcut_error = None;
        cx.notify();
    }

    fn capture_shortcut_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(command) = self.recording_shortcut else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }

        let key = event.keystroke.unparse();
        if key == "escape" {
            self.recording_shortcut = None;
            self.shortcut_error = None;
            cx.notify();
            return;
        }

        let Some(keys) = normalize_shortcut_keys(std::slice::from_ref(&key)) else {
            let strings = cx.global::<I18nManager>().strings();
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_invalid_template,
                "{shortcut}",
                &key,
            ));
            cx.notify();
            return;
        };

        if let Some(conflict) = shortcut_conflict_for(command, &keys, &self.keybindings) {
            let strings = cx.global::<I18nManager>().strings();
            let label = Self::shortcut_command_label(conflict.command, strings);
            self.shortcut_error = Some(Self::format_template(
                &strings.preferences_shortcut_conflict_template,
                "{command}",
                &label,
            ));
            cx.notify();
            return;
        }

        if let Some(definition) = shortcut_definitions()
            .iter()
            .find(|definition| definition.command == command)
        {
            let defaults = definition
                .default_keys
                .iter()
                .map(|key| key.to_string())
                .collect::<Vec<_>>();
            if keys == defaults {
                self.keybindings.remove(definition.id);
            } else {
                self.keybindings.insert(definition.id.to_string(), keys);
            }
        }
        self.recording_shortcut = None;
        self.shortcut_error = None;
        cx.notify();
    }

    fn shortcut_chip(label: &str, theme: &Theme) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .min_w(px(58.0))
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.menu_item_radius - 1.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.code_bg)
            .text_size(px((t.dialog_body_size - 1.0).max(10.0)))
            .text_color(c.code_text)
            .child(SharedString::from(label.to_string()))
    }

    fn shortcut_action_button(
        id: impl Into<ElementId>,
        label: String,
        theme: &Theme,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        div()
            .id(id)
            .h(px(28.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.dialog_radius - 5.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px((t.dialog_button_size - 1.0).max(10.0)))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(label)
            .on_click(cx.listener(on_click))
    }

    fn render_shortcut_row(
        &self,
        definition: ShortcutDefinition,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let is_recording = self.recording_shortcut == Some(definition.command);
        let keys = resolved_shortcut_keys(&self.keybindings, definition.command);
        let label = Self::shortcut_command_label(definition.command, strings);
        let command = definition.command;

        let mut chips = div().flex().flex_wrap().gap(px(6.0));
        if is_recording {
            chips = chips.child(Self::shortcut_chip(
                &strings.preferences_shortcut_recording,
                theme,
            ));
        } else {
            for key in keys {
                chips = chips.child(Self::shortcut_chip(&key, theme));
            }
        }

        div()
            .w_full()
            .min_h(px(42.0))
            .px(px(10.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .child(
                div()
                    .min_w(px(144.0))
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_body)
                    .child(label),
            )
            .child(div().flex_1().child(chips))
            .child(
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-record", definition.command as u32),
                        strings.preferences_shortcut_record.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.begin_recording_shortcut(command, event, window, cx)
                        },
                        cx,
                    ))
                    .child(Self::shortcut_action_button(
                        ("preferences-shortcut-reset", definition.command as u32),
                        strings.preferences_shortcut_reset.clone(),
                        theme,
                        move |this, event, window, cx| {
                            this.reset_shortcut(command, event, window, cx)
                        },
                        cx,
                    )),
            )
    }

    fn render_shortcuts_page(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        cx: &mut Context<Self>,
    ) -> Div {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let mut content = div()
            .id("preferences-shortcuts-scroll")
            .w_full()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .pr(px(4.0));

        let categories = [
            ShortcutCategory::File,
            ShortcutCategory::Edit,
            ShortcutCategory::Navigation,
            ShortcutCategory::Formatting,
            ShortcutCategory::Block,
            ShortcutCategory::Other,
        ];

        for category in categories {
            let mut group = div().w_full().flex().flex_col().gap(px(8.0)).child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(t.dialog_body_size))
                            .font_weight(t.dialog_button_weight.to_font_weight())
                            .text_color(c.dialog_title)
                            .child(Self::shortcut_category_label(category, strings)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(d.dialog_border_width.max(1.0)))
                            .bg(c.dialog_border),
                    ),
            );
            for definition in shortcut_definitions()
                .iter()
                .copied()
                .filter(|definition| definition.category == category)
            {
                group = group.child(self.render_shortcut_row(definition, theme, strings, cx));
            }
            content = content.child(group);
        }

        let mut page = div()
            .w_full()
            .h_full()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0));
        if let Some(error) = &self.shortcut_error {
            page = page.child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_danger_button_bg)
                    .child(error.clone()),
            );
        }
        page.child(content)
    }
}

struct AiPreferenceInputElement {
    preferences: Entity<PreferencesWindow>,
    field: AiPreferenceField,
    placeholder: SharedString,
}

impl AiPreferenceInputElement {
    fn new(
        preferences: Entity<PreferencesWindow>,
        field: AiPreferenceField,
        placeholder: SharedString,
    ) -> Self {
        Self {
            preferences,
            field,
            placeholder,
        }
    }
}

struct AiPreferenceInputPrepaintState {
    line: Option<ShapedLine>,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
    hitbox: Option<Hitbox>,
}

impl IntoElement for AiPreferenceInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for AiPreferenceInputElement {
    type RequestLayoutState = ();
    type PrepaintState = AiPreferenceInputPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<ThemeManager>().current_arc();
        let preferences = self.preferences.read(cx);
        let focused = preferences.ai_input.active_field == Some(self.field)
            && preferences.ai_input.focus_handle.is_focused(window);
        let text = preferences.ai_field_text(self.field);
        let is_placeholder = text.is_empty();
        let content = if is_placeholder {
            self.placeholder.clone()
        } else {
            SharedString::from(text.to_string())
        };
        let text_color = if is_placeholder {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };
        let style = window.text_style();
        let font_size = px(theme.typography.text_size * 0.9);
        let runs = vec![TextRun {
            len: content.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        let line = window.text_system().shape_line(content, font_size, &runs, None);
        let line_height = bounds.size.height;
        let padding_top = (line_height - line.ascent - line.descent) / 2.0;
        let text_top = bounds.top() + padding_top;
        let text_bottom = text_top + line.ascent + line.descent;

        let marked = preferences
            .ai_input
            .input
            .marked_range
            .as_ref()
            .filter(|_| focused && !is_placeholder)
            .map(|range| {
                fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(range.start), text_top),
                        point(bounds.left() + line.x_for_index(range.end), text_bottom),
                    ),
                    theme.colors.selection.opacity(0.35),
                )
            });
        let selection = if focused && !is_placeholder && !preferences.ai_input.input.selected_range.is_empty() {
            Some(fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(preferences.ai_input.input.selected_range.start),
                        text_top,
                    ),
                    point(
                        bounds.left() + line.x_for_index(preferences.ai_input.input.selected_range.end),
                        text_bottom,
                    ),
                ),
                theme.colors.selection.opacity(0.35),
            ))
        } else {
            None
        };
        let cursor = if focused
            && preferences.ai_input.input.marked_range.is_none()
            && preferences.ai_input.input.selected_range.is_empty()
        {
            Some(fill(
                Bounds::new(
                    point(
                        bounds.left()
                            + line.x_for_index(caret_offset(
                                &preferences.ai_input.input.selected_range,
                                preferences.ai_input.input.selection_reversed,
                            )),
                        text_top,
                    ),
                    size(px(theme.dimensions.cursor_width), text_bottom - text_top),
                ),
                theme.colors.cursor,
            ))
        } else {
            None
        };
        let hitbox = Some(window.insert_hitbox(bounds, HitboxBehavior::Normal));
        self.preferences.update(cx, |preferences, _cx| {
            preferences.set_ai_input_layout(self.field, line.clone(), bounds);
        });
        AiPreferenceInputPrepaintState {
            line: Some(line),
            selection,
            cursor,
            marked,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(hitbox) = prepaint.hitbox.as_ref()
            && hitbox.is_hovered(window)
        {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }
        let focus_handle = self.preferences.read(cx).ai_input.focus_handle.clone();
        if focus_handle.is_focused(window)
            && self.preferences.read(cx).ai_input.active_field == Some(self.field)
        {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.preferences.clone()),
                cx,
            );
        }

        let preferences_for_down = self.preferences.clone();
        let preferences_for_up = self.preferences.clone();
        let preferences_for_move = self.preferences.clone();
        let field = self.field;
        let input_bounds = bounds;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !input_bounds.contains(&event.position) {
                return;
            }
            if event.button != MouseButton::Left {
                return;
            }
            cx.stop_propagation();
            let offset = preferences_for_down
                .read(cx)
                .ai_input
                .input
                .last_layout
                .as_ref()
                .and_then(|line| {
                    let x = event.position.x - input_bounds.left();
                    line.index_for_x(x)
                })
                .unwrap_or_else(|| preferences_for_down.read(cx).ai_field_text(field).len());
            preferences_for_down.update(cx, |preferences, cx| {
                preferences.activate_ai_input_at(field, offset, event.modifiers.shift, window, cx);
            });
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            preferences_for_up.update(cx, |preferences, cx| {
                preferences.finish_ai_input_selection(cx);
            });
        });
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _window, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }
            let offset = preferences_for_move
                .read(cx)
                .ai_input
                .input
                .last_layout
                .as_ref()
                .and_then(|line| {
                    let x = event.position.x - input_bounds.left();
                    line.index_for_x(x)
                })
                .unwrap_or_else(|| preferences_for_move.read(cx).ai_field_text(field).len());
            preferences_for_move.update(cx, |preferences, cx| {
                preferences.drag_ai_input_to(offset, cx);
            });
        });

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        if let Some(marked) = prepaint.marked.take() {
            window.paint_quad(marked);
        }
        if let Some(line) = prepaint.line.take() {
            line.paint(bounds.origin, bounds.size.height, window, cx).ok();
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
    }
}

impl EntityInputHandler for PreferencesWindow {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field?;
            let text = self.toolbar_text_for_field(field);
            let range = range_from_utf16(text, &range_utf16);
            actual_range.replace(range_to_utf16(text, &range));
            return Some(text[range].to_string());
        }

        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        let text = self.ai_field_text(field);
        let range = range_from_utf16(text, &range_utf16);
        actual_range.replace(range_to_utf16(text, &range));
        Some(text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field?;
            return Some(UTF16Selection {
                range: range_to_utf16(
                    self.toolbar_text_for_field(field),
                    &self.toolbar_text_input.input.selected_range,
                ),
                reversed: self.toolbar_text_input.input.selection_reversed,
            });
        }

        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        Some(UTF16Selection {
            range: range_to_utf16(self.ai_field_text(field), &self.ai_input.input.selected_range),
            reversed: self.ai_input.input.selection_reversed,
        })
    }

    fn marked_text_range(&self, window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field?;
            return self
                .toolbar_text_input
                .input
                .marked_range
                .as_ref()
                .map(|range| range_to_utf16(self.toolbar_text_for_field(field), range));
        }

        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        self.ai_input
            .input
            .marked_range
            .as_ref()
            .map(|range| range_to_utf16(self.ai_field_text(field), range))
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.toolbar_text_input_active(window) {
            self.toolbar_text_input.input.marked_range = None;
            return;
        }

        if self.ai_input_active(window) {
            self.ai_input.input.marked_range = None;
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field.expect("active field");
            let text = self.toolbar_text_for_field(field).to_string();
            let range = range_utf16
                .as_ref()
                .map(|range| range_from_utf16(&text, range))
                .or_else(|| self.toolbar_text_input.input.marked_range.clone())
                .unwrap_or_else(|| self.toolbar_text_input.input.selected_range.clone());
            self.toolbar_text_replace_for_ime(
                range,
                &sanitize_single_line_text(new_text),
                false,
                None,
                cx,
            );
            return;
        }

        if !self.ai_input_active(window) {
            return;
        }
        let Some(field) = self.ai_input.active_field else {
            return;
        };
        let text = self.ai_field_text(field).to_string();
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&text, range))
            .or_else(|| self.ai_input.input.marked_range.clone())
            .unwrap_or_else(|| self.ai_input.input.selected_range.clone());
        self.replace_ai_field_text(field, range, &sanitize_single_line_text(new_text), false, None, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field.expect("active field");
            let text = self.toolbar_text_for_field(field).to_string();
            let replacement = sanitize_single_line_text(new_text);
            let range = range_utf16
                .as_ref()
                .map(|range| range_from_utf16(&text, range))
                .or_else(|| self.toolbar_text_input.input.marked_range.clone())
                .unwrap_or_else(|| self.toolbar_text_input.input.selected_range.clone());
            let selected_after = new_selected_range_utf16
                .as_ref()
                .map(|range| range_from_utf16(&replacement, range))
                .map(|relative| relative.start + range.start..relative.end + range.start);
            self.toolbar_text_replace_for_ime(range, &replacement, true, selected_after, cx);
            return;
        }

        if !self.ai_input_active(window) {
            return;
        }
        let Some(field) = self.ai_input.active_field else {
            return;
        };
        let text = self.ai_field_text(field).to_string();
        let replacement = sanitize_single_line_text(new_text);
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&text, range))
            .or_else(|| self.ai_input.input.marked_range.clone())
            .unwrap_or_else(|| self.ai_input.input.selected_range.clone());
        let selected_after = new_selected_range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&replacement, range))
            .map(|relative| relative.start + range.start..relative.end + range.start);
        self.replace_ai_field_text(field, range, &replacement, true, selected_after, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field?;
            let line = self.toolbar_text_input.input.last_layout.as_ref()?;
            let range = range_from_utf16(self.toolbar_text_for_field(field), &range_utf16);
            return Some(Bounds::from_corners(
                point(bounds.left() + line.x_for_index(range.start), bounds.top()),
                point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
            ));
        }

        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        let line = self.ai_input.input.last_layout.as_ref()?;
        let range = range_from_utf16(self.ai_field_text(field), &range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.toolbar_text_input_active(window) {
            let field = self.toolbar_text_input.active_field?;
            let bounds = self.toolbar_text_input.input.last_bounds?;
            let line = self.toolbar_text_input.input.last_layout.as_ref()?;
            let local = bounds.localize(&point)?;
            let offset = line.index_for_x(local.x - bounds.left())?;
            return Some(offset_to_utf16(self.toolbar_text_for_field(field), offset));
        }

        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        let bounds = self.ai_input.input.last_bounds?;
        let line = self.ai_input.input.last_layout.as_ref()?;
        let local = bounds.localize(&point)?;
        let offset = line.index_for_x(local.x - bounds.left())?;
        Some(offset_to_utf16(self.ai_field_text(field), offset))
    }
}

fn caret_offset(range: &Range<usize>, reversed: bool) -> usize {
    if reversed {
        range.start
    } else {
        range.end
    }
}

fn extend_selection(
    selected_range: &mut Range<usize>,
    selection_reversed: &mut bool,
    offset: usize,
    text_len: usize,
) {
    let offset = offset.min(text_len);
    if *selection_reversed {
        selected_range.start = offset;
    } else {
        selected_range.end = offset;
    }
    if selected_range.end < selected_range.start {
        *selection_reversed = !*selection_reversed;
        *selected_range = selected_range.end..selected_range.start;
    }
}

fn previous_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 {
        offset -= 1;
        if text.is_char_boundary(offset) {
            return offset;
        }
    }
    0
}

fn next_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    if offset >= text.len() {
        return text.len();
    }
    offset += 1;
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

fn sanitize_single_line_text(text: &str) -> String {
    flatten_paste_to_single_line(text)
}

fn offset_to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].encode_utf16().count()
}

fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(text, range.start)..offset_to_utf16(text, range.end)
}

fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    let start = byte_offset_from_utf16(text, range.start);
    let end = byte_offset_from_utf16(text, range.end);
    start.min(end)..start.max(end)
}

fn byte_offset_from_utf16(text: &str, target: usize) -> usize {
    if target == 0 {
        return 0;
    }
    let mut utf16 = 0usize;
    for (offset, ch) in text.char_indices() {
        if utf16 >= target {
            return offset;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

impl Render for PreferencesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeManager>().current().clone();
        let strings = cx.global::<I18nManager>().strings().clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let can_save = self.has_unsaved_changes();
        let window_title = SharedString::from(app_window_title(Some(
            strings.preferences_window_title.as_str(),
        )));
        window.set_window_title(window_title.as_ref());
        let titlebar_height = custom_titlebar_height(window, d);

        let content = div()
            .size_full()
            .pt(px(titlebar_height))
            .flex()
            .key_context("Preferences")
            .track_focus(&self.focus_handle)
            .capture_action(cx.listener(Self::on_quit_application))
            .capture_action(cx.listener(Self::on_close_window))
            .on_key_down(cx.listener(Self::on_preferences_key_down))
            .bg(c.editor_background)
            .text_color(c.dialog_body)
            .child(
                div()
                    .w(relative(0.3))
                    .h_full()
                    .pr(px(20.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .border_r(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(self.nav_button(
                                "preferences-nav-file",
                                strings.preferences_nav_file.clone(),
                                self.nav == PreferencesNav::File,
                                &theme,
                                Self::set_nav_file,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-theme",
                                strings.preferences_nav_theme.clone(),
                                self.nav == PreferencesNav::Theme,
                                &theme,
                                Self::set_nav_theme,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-ai",
                                "AI".to_string(),
                                self.nav == PreferencesNav::Ai,
                                &theme,
                                Self::set_nav_ai,
                                cx,
                            ))
                            .child(self.nav_button(
                                "preferences-nav-shortcuts",
                                strings.preferences_nav_shortcuts.clone(),
                                self.nav == PreferencesNav::Shortcuts,
                                &theme,
                                Self::set_nav_shortcuts,
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .w(relative(0.7))
                    .h_full()
                    .p(px(d.dialog_padding))
                    .flex()
                    .flex_col()
                    .gap(px(d.dialog_gap))
                    .child(
                        div()
                            .w_full()
                            .flex_1()
                            .min_h(px(0.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(d.dialog_gap * 1.5))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(t.dialog_title_size))
                                    .font_weight(t.dialog_title_weight.to_font_weight())
                                    .text_color(c.dialog_title)
                                    .child(match self.nav {
                                        PreferencesNav::File => {
                                            strings.preferences_nav_file.clone()
                                        }
                                        PreferencesNav::Theme => {
                                            strings.preferences_nav_theme.clone()
                                        }
                                        PreferencesNav::Ai => "AI".to_string(),
                                        PreferencesNav::Shortcuts => {
                                            strings.preferences_nav_shortcuts.clone()
                                        }
                                    }),
                            )
                            .child(match self.nav {
                                PreferencesNav::File => div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(self.render_startup_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::Theme => div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(self.render_theme_page(&theme, &strings, cx))
                                    .into_any_element(),
                                PreferencesNav::Ai => div()
                                    .id("preferences-ai-scroll")
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .overflow_y_scroll()
                                    .flex()
                                    .items_start()
                                    .justify_center()
                                    .child(self.render_ai_page(&theme, cx))
                                    .into_any_element(),
                                PreferencesNav::Shortcuts => div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .child(self.render_shortcuts_page(&theme, &strings, cx))
                                    .into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .gap(px(d.dialog_button_gap))
                            .child(
                                div()
                                    .id("preferences-cancel")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .cursor_pointer()
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(c.dialog_secondary_button_text)
                                    .child(strings.preferences_cancel.clone())
                                    .on_click(cx.listener(Self::cancel)),
                            )
                            .child(
                                div()
                                    .id("preferences-save")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .border(px(if can_save { 0.0 } else { d.dialog_border_width }))
                                    .border_color(c.dialog_border)
                                    .bg(if can_save {
                                        c.dialog_primary_button_bg
                                    } else {
                                        c.dialog_secondary_button_bg
                                    })
                                    .hover(move |this| {
                                        if can_save {
                                            this.bg(c.dialog_primary_button_hover)
                                        } else {
                                            this.bg(c.dialog_secondary_button_bg)
                                        }
                                    })
                                    .when(can_save, |this| this.cursor_pointer())
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(if can_save {
                                        c.dialog_primary_button_text
                                    } else {
                                        c.dialog_secondary_button_text
                                    })
                                    .child(strings.preferences_save.clone())
                                    .on_click(cx.listener(Self::save)),
                            ),
                    ),
            );

        let root = div()
            .size_full()
            .relative()
            .bg(c.editor_background)
            .child(content);

        if let Some(titlebar) = render_custom_titlebar(
            "preferences-titlebar",
            window_title,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            root.child(titlebar)
        } else {
            root
        }
    }
}

fn existing_preferences_window(cx: &App) -> Option<WindowHandle<PreferencesWindow>> {
    cx.windows()
        .iter()
        .find_map(|window| window.downcast::<PreferencesWindow>())
}

fn preferences_window_bounds(cx: &mut App) -> Bounds<Pixels> {
    let window_size = size(px(720.0), px(480.0));
    if let Some(parent_bounds) = cx
        .active_window()
        .and_then(|window| window.downcast::<Editor>())
        .and_then(|editor| editor.update(cx, |_, window, _| window.bounds()).ok())
    {
        Bounds::centered_at(parent_bounds.center(), window_size)
    } else {
        Bounds::centered(None, window_size, cx)
    }
}

fn activate_preferences_handle(handle: &WindowHandle<PreferencesWindow>, cx: &mut App) {
    cx.activate(true);
    let handle_for_followup = handle.clone();
    let _ = handle.update(cx, |preferences, window, cx| {
        window.activate_window();
        preferences.focus_handle.focus(window);
        window.defer(cx, move |window, cx| {
            cx.activate(true);
            window.activate_window();
            let _ = handle_for_followup.update(cx, |preferences, window, _cx| {
                preferences.focus_handle.focus(window);
            });
        });
    });

    let handle_for_spawn = handle.clone();
    cx.spawn(async move |cx| {
        for delay in [50_u64, 150] {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let _ = cx.update(|cx| {
                cx.activate(true);
                let _ = handle_for_spawn.update(cx, |preferences, window, _cx| {
                    window.activate_window();
                    preferences.focus_handle.focus(window);
                });
            });
        }
    })
    .detach();
}

pub(crate) fn open_preferences_window_with_state(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
    initial_nav: PreferencesNav,
) -> WindowHandle<PreferencesWindow> {
    let bounds = preferences_window_bounds(cx);
    let window_title = SharedString::from(app_window_title(Some(title.as_str())));
    let mut options = velotype_window_options(window_title, bounds);
    options.focus = true;
    options.show = true;
    let handle = cx
        .open_window(options, move |_window, cx| {
            cx.new(move |cx| {
                PreferencesWindow::new(preferences, theme_options, initial_nav, cx)
            })
        })
        .expect("preferences window should open");

    activate_preferences_handle(&handle, cx);
    handle
}

pub(crate) fn open_preferences_window(cx: &mut App) -> WindowHandle<PreferencesWindow> {
    let preferences = match read_app_preferences() {
        Ok(preferences) => preferences,
        Err(err) => {
            eprintln!("failed to read app preferences: {err}");
            AppPreferences::default()
        }
    };
    let theme_options = cx.global::<ThemeManager>().available_themes().to_vec();
    let title = cx
        .global::<I18nManager>()
        .strings()
        .preferences_window_title
        .clone();
    open_preferences_window_with_state(
        cx,
        preferences,
        theme_options,
        title,
        PreferencesNav::File,
    )
}

pub(crate) fn open_preferences_window_to_ai(cx: &mut App) {
    if let Some(existing) = existing_preferences_window(cx) {
        let _ = existing.update(cx, |preferences, _window, cx| {
            preferences.show_ai_settings(cx);
        });
        activate_preferences_handle(&existing, cx);
        return;
    }

    let preferences = match read_app_preferences() {
        Ok(preferences) => preferences,
        Err(err) => {
            eprintln!("failed to read app preferences: {err}");
            AppPreferences::default()
        }
    };
    let theme_options = cx.global::<ThemeManager>().available_themes().to_vec();
    let title = cx
        .global::<I18nManager>()
        .strings()
        .preferences_window_title
        .clone();
    let _ = open_preferences_window_with_state(
        cx,
        preferences,
        theme_options,
        title,
        PreferencesNav::Ai,
    );
}

#[cfg(test)]
mod tests {
    use super::{open_preferences_window_with_state, PreferencesNav, StartupOpenPreference};
    use crate::config::store::AppPreferences;
    use crate::i18n::I18nManager;
    use crate::theme::{ThemeCatalogEntry, ThemeManager};
    use gpui::TestAppContext;
    use std::collections::BTreeMap;

    fn init_preferences_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init_with_theme_id(cx, "markman");
            crate::components::init_with_keybindings(cx, &BTreeMap::new());
        });
    }

    fn default_theme_options() -> Vec<ThemeCatalogEntry> {
        vec![ThemeCatalogEntry {
            id: "markman".into(),
            name: "Markman".into(),
        }]
    }

    #[gpui::test]
    async fn preferences_window_activates_and_focuses_on_open(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
                PreferencesNav::File,
            )
        });
        cx.run_until_parked();

        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should be updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should be updateable")
        );
    }

    #[gpui::test]
    async fn preferences_dirty_state_tracks_draft_changes(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
                PreferencesNav::File,
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, _window, _cx| {
                assert!(!preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                preferences.startup_open = StartupOpenPreference::NewFile;
                assert!(!preferences.has_unsaved_changes());

                preferences
                    .keybindings
                    .insert("save_document".into(), vec!["ctrl-alt-s".into()]);
                assert!(preferences.has_unsaved_changes());
            })
            .expect("preferences window should be updateable");
    }

    #[gpui::test]
    async fn applying_saved_preferences_keeps_window_open_and_focused(cx: &mut TestAppContext) {
        init_preferences_test_app(cx);

        let handle = cx.update(|cx| {
            open_preferences_window_with_state(
                cx,
                AppPreferences::default(),
                default_theme_options(),
                "Preferences".into(),
                PreferencesNav::File,
            )
        });
        cx.run_until_parked();

        handle
            .update(cx, |preferences, window, cx| {
                preferences.startup_open = StartupOpenPreference::LastOpenedFile;
                assert!(preferences.has_unsaved_changes());
                let saved = AppPreferences {
                    startup_open: StartupOpenPreference::LastOpenedFile,
                    ..AppPreferences::default()
                };
                preferences.apply_saved_preferences(saved, window, cx);
            })
            .expect("preferences window should be updateable");
        cx.run_until_parked();

        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
        let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
        assert_eq!(active_window.window_id(), handle.window_id());
        assert!(
            handle
                .update(cx, |preferences, window, _cx| preferences
                    .focus_handle
                    .is_focused(window))
                .expect("preferences window should remain updateable")
        );
        assert!(
            !handle
                .update(cx, |preferences, _window, _cx| preferences
                    .has_unsaved_changes())
                .expect("preferences window should remain updateable")
        );
    }
}
