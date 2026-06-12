//! Persistent app preferences and the preferences window.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

use anyhow::Context as _;
use gpui::prelude::FluentBuilder;
use gpui::*;
use serde::Serialize;

use super::{VelotypeConfigDirs, read_recent_files};
use crate::components::{
    CloseWindow, QuitApplication, ShortcutCategory, ShortcutCommand, ShortcutDefinition,
    install_keybindings, normalize_shortcut_config, normalize_shortcut_keys, resolved_shortcut_keys,
    shortcut_conflict_for, shortcut_definitions,
};
use crate::app_identity::app_window_title;
use crate::i18n::{I18nManager, language_id_for_locale_preferences};
use crate::theme::{Theme, ThemeCatalogEntry, ThemeManager};
use crate::window_chrome::{
    custom_titlebar_height, render_custom_titlebar, velotype_window_options,
};

const DEFAULT_THEME_ID: &str = "velotype";
const DEFAULT_LANGUAGE_ID: &str = "en-US";

/// Startup document selection stored in `config.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupOpenPreference {
    NewFile,
    LastOpenedFile,
}

impl StartupOpenPreference {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NewFile => "new_file",
            Self::LastOpenedFile => "last_opened_file",
        }
    }

    fn from_str(value: &str) -> Self {
        match value {
            "last_opened_file" => Self::LastOpenedFile,
            _ => Self::NewFile,
        }
    }
}

/// User preferences persisted under the app config directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppPreferences {
    pub(crate) startup_open: StartupOpenPreference,
    pub(crate) default_language_id: String,
    pub(crate) default_theme_id: String,
    pub(crate) keybindings: BTreeMap<String, Vec<String>>,
    /// When false, code-block run buttons are disabled.
    pub(crate) allow_code_execution: bool,
    /// Set after the user accepts the first-run code execution warning.
    pub(crate) code_execution_confirm_shown: bool,
    /// When true, inline code runs open in the system terminal instead of the in-app popover runner.
    pub(crate) inline_code_run_in_system_terminal: bool,
    pub(crate) ai: AiPreferences,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiPreferences {
    pub(crate) provider: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) api_key_env: String,
    pub(crate) allow_full_document_context: bool,
    pub(crate) allow_workspace_context: bool,
    pub(crate) allow_command_context: bool,
}

impl Default for AiPreferences {
    fn default() -> Self {
        Self {
            provider: "openai-compatible".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            allow_full_document_context: false,
            allow_workspace_context: false,
            allow_command_context: false,
        }
    }
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            startup_open: StartupOpenPreference::NewFile,
            default_language_id: DEFAULT_LANGUAGE_ID.into(),
            default_theme_id: DEFAULT_THEME_ID.into(),
            keybindings: BTreeMap::new(),
            allow_code_execution: true,
            code_execution_confirm_shown: false,
            inline_code_run_in_system_terminal: false,
            ai: AiPreferences::default(),
        }
    }
}

#[derive(Serialize)]
struct PreferencesFile {
    startup: StartupPreferencesFile,
    language: LanguagePreferencesFile,
    theme: ThemePreferencesFile,
    keybindings: BTreeMap<String, Vec<String>>,
    code_execution: CodeExecutionPreferencesFile,
    ai: AiPreferencesFile,
}

#[derive(Serialize)]
struct StartupPreferencesFile {
    open: String,
}

#[derive(Serialize)]
struct LanguagePreferencesFile {
    default_language_id: String,
}

#[derive(Serialize)]
struct ThemePreferencesFile {
    default_theme_id: String,
}

#[derive(Serialize)]
struct CodeExecutionPreferencesFile {
    allow: bool,
    confirm_shown: bool,
    inline_code_system_terminal: bool,
}

#[derive(Serialize)]
struct AiPreferencesFile {
    provider: String,
    base_url: String,
    model: String,
    api_key_env: String,
    allow_full_document_context: bool,
    allow_workspace_context: bool,
    allow_command_context: bool,
}

impl From<&AppPreferences> for PreferencesFile {
    fn from(value: &AppPreferences) -> Self {
        Self {
            startup: StartupPreferencesFile {
                open: value.startup_open.as_str().into(),
            },
            language: LanguagePreferencesFile {
                default_language_id: value.default_language_id.clone(),
            },
            theme: ThemePreferencesFile {
                default_theme_id: value.default_theme_id.clone(),
            },
            keybindings: normalize_shortcut_config(&value.keybindings),
            code_execution: CodeExecutionPreferencesFile {
                allow: value.allow_code_execution,
                confirm_shown: value.code_execution_confirm_shown,
                inline_code_system_terminal: value.inline_code_run_in_system_terminal,
            },
            ai: AiPreferencesFile {
                provider: value.ai.provider.clone(),
                base_url: value.ai.base_url.clone(),
                model: value.ai.model.clone(),
                api_key_env: value.ai.api_key_env.clone(),
                allow_full_document_context: value.ai.allow_full_document_context,
                allow_workspace_context: value.ai.allow_workspace_context,
                allow_command_context: value.ai.allow_command_context,
            },
        }
    }
}

pub(crate) fn read_app_preferences() -> anyhow::Result<AppPreferences> {
    read_app_preferences_with_dirs(&VelotypeConfigDirs::from_system()?)
}

pub(crate) fn read_app_preferences_with_dirs(
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let path = dirs.app_config_file();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppPreferences::default());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return Ok(AppPreferences::default());
    };

    Ok(app_preferences_from_toml_value(&value, DEFAULT_LANGUAGE_ID))
}

pub(crate) fn load_or_create_app_preferences() -> anyhow::Result<AppPreferences> {
    let dirs = VelotypeConfigDirs::from_system()?;
    load_or_create_app_preferences_with_dirs_and_locales(&dirs, sys_locale::get_locales())
}

fn app_preferences_from_toml_value(
    value: &toml::Value,
    fallback_language_id: &str,
) -> AppPreferences {
    let startup_open = value
        .get("startup")
        .and_then(|startup| startup.get("open"))
        .and_then(|open| open.as_str())
        .map(StartupOpenPreference::from_str)
        .unwrap_or(StartupOpenPreference::NewFile);
    let default_language_id = value
        .get("language")
        .and_then(|language| language.get("default_language_id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(fallback_language_id)
        .to_string();
    let default_theme_id = value
        .get("theme")
        .and_then(|theme| theme.get("default_theme_id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(DEFAULT_THEME_ID)
        .to_string();
    let keybindings = value
        .get("keybindings")
        .and_then(|keybindings| keybindings.as_table())
        .map(|table| {
            table
                .iter()
                .filter_map(|(key, value)| {
                    let keys = value
                        .as_array()?
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect::<Vec<_>>();
                    Some((key.clone(), keys))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .map(|keybindings| normalize_shortcut_config(&keybindings))
        .unwrap_or_default();

    let allow_code_execution = value
        .get("code_execution")
        .and_then(|section| section.get("allow"))
        .and_then(|allow| allow.as_bool())
        .unwrap_or(true);
    let code_execution_confirm_shown = value
        .get("code_execution")
        .and_then(|section| section.get("confirm_shown"))
        .and_then(|confirm| confirm.as_bool())
        .unwrap_or(false);
    let inline_code_run_in_system_terminal = value
        .get("code_execution")
        .and_then(|section| section.get("inline_code_system_terminal"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let ai = ai_preferences_from_toml_value(value);

    AppPreferences {
        startup_open,
        default_language_id,
        default_theme_id,
        keybindings,
        allow_code_execution,
        code_execution_confirm_shown,
        inline_code_run_in_system_terminal,
        ai,
    }
}

fn read_trimmed_string(section: Option<&toml::Value>, key: &str, fallback: &str) -> String {
    section
        .and_then(|section| section.get(key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn ai_preferences_from_toml_value(value: &toml::Value) -> AiPreferences {
    let defaults = AiPreferences::default();
    let section = value.get("ai");
    AiPreferences {
        provider: read_trimmed_string(section, "provider", &defaults.provider),
        base_url: read_trimmed_string(section, "base_url", &defaults.base_url),
        model: read_trimmed_string(section, "model", &defaults.model),
        api_key_env: read_trimmed_string(section, "api_key_env", &defaults.api_key_env),
        allow_full_document_context: section
            .and_then(|section| section.get("allow_full_document_context"))
            .and_then(|value| value.as_bool())
            .unwrap_or(defaults.allow_full_document_context),
        allow_workspace_context: section
            .and_then(|section| section.get("allow_workspace_context"))
            .and_then(|value| value.as_bool())
            .unwrap_or(defaults.allow_workspace_context),
        allow_command_context: section
            .and_then(|section| section.get("allow_command_context"))
            .and_then(|value| value.as_bool())
            .unwrap_or(defaults.allow_command_context),
    }
}

pub(crate) fn set_code_execution_confirm_shown() -> anyhow::Result<()> {
    update_app_preferences(|preferences| {
        preferences.code_execution_confirm_shown = true;
    })?;
    Ok(())
}

fn detected_language_id_from_locales<I, S>(locales: I) -> &'static str
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    language_id_for_locale_preferences(locales)
}

fn load_or_create_app_preferences_with_dirs_and_locales<I, S>(
    dirs: &VelotypeConfigDirs,
    locales: I,
) -> anyhow::Result<AppPreferences>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let detected_language_id = detected_language_id_from_locales(locales);
    let path = dirs.app_config_file();
    let preferences = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<toml::Value>(&text)
            .map(|value| app_preferences_from_toml_value(&value, detected_language_id))
            .unwrap_or_else(|_| AppPreferences {
                default_language_id: detected_language_id.into(),
                ..AppPreferences::default()
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => AppPreferences {
            default_language_id: detected_language_id.into(),
            ..AppPreferences::default()
        },
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read '{}'", path.display()));
        }
    };
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

pub(crate) fn save_app_preferences(preferences: &AppPreferences) -> anyhow::Result<()> {
    save_app_preferences_with_dirs(preferences, &VelotypeConfigDirs::from_system()?)
}

pub(crate) fn save_app_preferences_with_dirs(
    preferences: &AppPreferences,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<()> {
    let path = dirs.app_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let text = toml::to_string_pretty(&PreferencesFile::from(preferences))?;
    std::fs::write(&path, text).with_context(|| format!("failed to write '{}'", path.display()))
}

pub(crate) fn first_existing_recent_markdown_file() -> Option<PathBuf> {
    let recent_files = read_recent_files().ok()?;
    recent_files.into_iter().find(|path| path.is_file())
}

pub(crate) fn apply_configured_language(cx: &mut App, language_id: &str) -> anyhow::Result<bool> {
    let mut applied = false;
    let changed = cx.update_global::<I18nManager, _>(|i18n_manager, _cx| {
        let changed = i18n_manager.set_language_by_id(language_id);
        applied = changed || i18n_manager.current_language_id() == language_id;
        changed
    });
    if !applied {
        return Ok(false);
    }
    update_app_preferences(|preferences| {
        preferences.default_language_id = language_id.into();
    })?;
    Ok(changed)
}

pub(crate) fn apply_configured_theme(cx: &mut App, theme_id: &str) -> anyhow::Result<bool> {
    let mut applied = false;
    let changed = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        let changed = theme_manager.set_theme_by_id(theme_id);
        applied = changed || theme_manager.current_theme_id() == theme_id;
        changed
    });
    if !applied {
        return Ok(false);
    }
    update_app_preferences(|preferences| {
        preferences.default_theme_id = theme_id.into();
    })?;
    Ok(changed)
}

pub(crate) fn import_language_config_and_select(
    cx: &mut App,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<String> {
    let imported_id = cx.update_global::<I18nManager, _>(|i18n_manager, _cx| {
        i18n_manager.import_language_config(path)
    })?;
    update_app_preferences(|preferences| {
        preferences.default_language_id = imported_id.clone();
    })?;
    Ok(imported_id)
}

pub(crate) fn import_theme_config_and_select(
    cx: &mut App,
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<String> {
    let imported_id = cx.update_global::<ThemeManager, _>(|theme_manager, _cx| {
        theme_manager.import_theme_config(path)
    })?;
    update_app_preferences(|preferences| {
        preferences.default_theme_id = imported_id.clone();
    })?;
    Ok(imported_id)
}

pub(crate) fn save_preferences_from_window(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    keybindings: BTreeMap<String, Vec<String>>,
    allow_code_execution: bool,
    inline_code_run_in_system_terminal: bool,
    ai: AiPreferences,
) -> anyhow::Result<AppPreferences> {
    let dirs = VelotypeConfigDirs::from_system()?;
    save_preferences_from_window_with_dirs(
        startup_open,
        default_theme_id,
        keybindings,
        allow_code_execution,
        inline_code_run_in_system_terminal,
        ai,
        &dirs,
    )
}

fn save_preferences_from_window_with_dirs(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    keybindings: BTreeMap<String, Vec<String>>,
    allow_code_execution: bool,
    inline_code_run_in_system_terminal: bool,
    ai: AiPreferences,
    dirs: &VelotypeConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let mut preferences =
        load_or_create_app_preferences_with_dirs_and_locales(dirs, sys_locale::get_locales())?;
    preferences.startup_open = startup_open;
    preferences.default_theme_id = default_theme_id.into();
    preferences.keybindings = normalize_shortcut_config(&keybindings);
    preferences.allow_code_execution = allow_code_execution;
    preferences.inline_code_run_in_system_terminal = inline_code_run_in_system_terminal;
    preferences.ai = ai;
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

fn update_app_preferences(
    update: impl FnOnce(&mut AppPreferences),
) -> anyhow::Result<AppPreferences> {
    let mut preferences = load_or_create_app_preferences()?;
    update(&mut preferences);
    save_app_preferences(&preferences)?;
    Ok(preferences)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreferencesNav {
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
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    is_selecting: bool,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
}

impl AiPreferenceInputState {
    fn new(cx: &mut Context<PreferencesWindow>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            active_field: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            is_selecting: false,
            last_layout: None,
            last_bounds: None,
        }
    }
}

/// Independent preferences window view.
pub(crate) struct PreferencesWindow {
    nav: PreferencesNav,
    startup_open: StartupOpenPreference,
    allow_code_execution: bool,
    inline_code_run_in_system_terminal: bool,
    ai: AiPreferences,
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
}

impl PreferencesWindow {
    fn new(
        preferences: AppPreferences,
        theme_options: Vec<ThemeCatalogEntry>,
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
            nav: PreferencesNav::File,
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
        }
    }

    fn selected_theme_name(&self) -> String {
        self.theme_options
            .iter()
            .find(|entry| entry.id == self.selected_theme_id)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "Velotype".into())
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
        self.clear_ai_input_state();
        cx.notify();
    }

    fn set_nav_theme(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.nav = PreferencesNav::Theme;
        self.startup_dropdown_open = false;
        self.theme_dropdown_open = false;
        self.recording_shortcut = None;
        self.clear_ai_input_state();
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
        self.clear_ai_input_state();
        cx.notify();
    }

    fn clear_ai_input_state(&mut self) {
        self.ai_input.active_field = None;
        self.ai_input.selected_range = 0..0;
        self.ai_input.selection_reversed = false;
        self.ai_input.marked_range = None;
        self.ai_input.is_selecting = false;
        self.ai_input.last_layout = None;
        self.ai_input.last_bounds = None;
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
        self.ai_input.selected_range = selected_after.unwrap_or(inserted_end..inserted_end);
        self.ai_input.selection_reversed = false;
        self.ai_input.marked_range = if mark_inserted_text && !new_text.is_empty() {
            Some(start..inserted_end)
        } else {
            None
        };
        self.ai_input.is_selecting = false;
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
            self.ai_input.selected_range = text_len..text_len;
            self.ai_input.selection_reversed = false;
            self.ai_input.marked_range = None;
        }
        self.ai_input.active_field = Some(field);
        window.focus(&self.ai_input.focus_handle);
        if shift {
            extend_selection(
                &mut self.ai_input.selected_range,
                &mut self.ai_input.selection_reversed,
                offset,
                text_len,
            );
        } else {
            self.ai_input.selected_range = offset.min(text_len)..offset.min(text_len);
            self.ai_input.selection_reversed = false;
        }
        self.ai_input.marked_range = None;
        self.ai_input.is_selecting = true;
        cx.notify();
    }

    fn finish_ai_input_selection(&mut self, cx: &mut Context<Self>) {
        if self.ai_input.is_selecting {
            self.ai_input.is_selecting = false;
            cx.notify();
        }
    }

    fn drag_ai_input_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(field) = self.ai_input.active_field else {
            return;
        };
        if !self.ai_input.is_selecting {
            return;
        }
        let text_len = self.ai_field_text(field).len();
        extend_selection(
            &mut self.ai_input.selected_range,
            &mut self.ai_input.selection_reversed,
            offset,
            text_len,
        );
        self.ai_input.marked_range = None;
        cx.notify();
    }

    fn set_ai_input_layout(&mut self, field: AiPreferenceField, line: ShapedLine, bounds: Bounds<Pixels>) {
        if self.ai_input.active_field == Some(field) {
            self.ai_input.last_layout = Some(line);
            self.ai_input.last_bounds = Some(bounds);
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
                let range = if self.ai_input.selected_range.is_empty() {
                    let cursor = caret_offset(&self.ai_input.selected_range, self.ai_input.selection_reversed);
                    previous_char_boundary(&text, cursor)..cursor
                } else {
                    self.ai_input.selected_range.clone()
                };
                self.replace_ai_field_text(field, range, "", false, None, cx);
                true
            }
            "delete" => {
                let range = if self.ai_input.selected_range.is_empty() {
                    let cursor = caret_offset(&self.ai_input.selected_range, self.ai_input.selection_reversed);
                    cursor..next_char_boundary(&text, cursor)
                } else {
                    self.ai_input.selected_range.clone()
                };
                self.replace_ai_field_text(field, range, "", false, None, cx);
                true
            }
            "left" => {
                let cursor = caret_offset(&self.ai_input.selected_range, self.ai_input.selection_reversed);
                let next = previous_char_boundary(&text, cursor);
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.selected_range, &mut self.ai_input.selection_reversed, next, len);
                } else {
                    self.ai_input.selected_range = next..next;
                    self.ai_input.selection_reversed = false;
                }
                self.ai_input.marked_range = None;
                cx.notify();
                true
            }
            "right" => {
                let cursor = caret_offset(&self.ai_input.selected_range, self.ai_input.selection_reversed);
                let next = next_char_boundary(&text, cursor);
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.selected_range, &mut self.ai_input.selection_reversed, next, len);
                } else {
                    self.ai_input.selected_range = next..next;
                    self.ai_input.selection_reversed = false;
                }
                self.ai_input.marked_range = None;
                cx.notify();
                true
            }
            "home" => {
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.selected_range, &mut self.ai_input.selection_reversed, 0, len);
                } else {
                    self.ai_input.selected_range = 0..0;
                    self.ai_input.selection_reversed = false;
                }
                self.ai_input.marked_range = None;
                cx.notify();
                true
            }
            "end" => {
                if modifiers.shift {
                    extend_selection(&mut self.ai_input.selected_range, &mut self.ai_input.selection_reversed, len, len);
                } else {
                    self.ai_input.selected_range = len..len;
                    self.ai_input.selection_reversed = false;
                }
                self.ai_input.marked_range = None;
                cx.notify();
                true
            }
            "a" if primary => {
                self.ai_input.selected_range = 0..len;
                self.ai_input.selection_reversed = false;
                self.ai_input.marked_range = None;
                cx.notify();
                true
            }
            "c" if primary => {
                if !self.ai_input.selected_range.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        text[self.ai_input.selected_range.clone()].to_string(),
                    ));
                }
                true
            }
            "x" if primary => {
                if !self.ai_input.selected_range.is_empty() {
                    let range = self.ai_input.selected_range.clone();
                    cx.write_to_clipboard(ClipboardItem::new_string(text[range.clone()].to_string()));
                    self.replace_ai_field_text(field, range, "", false, None, cx);
                }
                true
            }
            "v" if primary => {
                if let Some(value) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let range = self
                        .ai_input
                        .marked_range
                        .clone()
                        .unwrap_or_else(|| self.ai_input.selected_range.clone());
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
        id: &'static str,
        field: AiPreferenceField,
        placeholder: &'static str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        div()
            .w(px(280.0))
            .min_h(px(36.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .overflow_hidden()
            .id(id)
            .track_focus(&self.ai_input.focus_handle)
            .child(
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
        let selection = if focused && !is_placeholder && !preferences.ai_input.selected_range.is_empty() {
            Some(fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(preferences.ai_input.selected_range.start),
                        text_top,
                    ),
                    point(
                        bounds.left() + line.x_for_index(preferences.ai_input.selected_range.end),
                        text_bottom,
                    ),
                ),
                theme.colors.selection.opacity(0.35),
            ))
        } else {
            None
        };
        let cursor = if focused
            && preferences.ai_input.marked_range.is_none()
            && preferences.ai_input.selected_range.is_empty()
        {
            Some(fill(
                Bounds::new(
                    point(
                        bounds.left()
                            + line.x_for_index(caret_offset(
                                &preferences.ai_input.selected_range,
                                preferences.ai_input.selection_reversed,
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
        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        Some(UTF16Selection {
            range: range_to_utf16(self.ai_field_text(field), &self.ai_input.selected_range),
            reversed: self.ai_input.selection_reversed,
        })
    }

    fn marked_text_range(&self, window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        self.ai_input
            .marked_range
            .as_ref()
            .map(|range| range_to_utf16(self.ai_field_text(field), range))
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.ai_input_active(window) {
            self.ai_input.marked_range = None;
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            .or_else(|| self.ai_input.marked_range.clone())
            .unwrap_or_else(|| self.ai_input.selected_range.clone());
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
            .or_else(|| self.ai_input.marked_range.clone())
            .unwrap_or_else(|| self.ai_input.selected_range.clone());
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
        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        let line = self.ai_input.last_layout.as_ref()?;
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
        if !self.ai_input_active(window) {
            return None;
        }
        let field = self.ai_input.active_field?;
        let bounds = self.ai_input.last_bounds?;
        let line = self.ai_input.last_layout.as_ref()?;
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
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
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

fn open_preferences_window_with_state(
    cx: &mut App,
    preferences: AppPreferences,
    theme_options: Vec<ThemeCatalogEntry>,
    title: String,
) -> WindowHandle<PreferencesWindow> {
    let bounds = Bounds::centered(None, size(px(720.0), px(480.0)), cx);
    let window_title = SharedString::from(app_window_title(Some(title.as_str())));
    let handle = cx
        .open_window(
            velotype_window_options(window_title, bounds),
            move |_window, cx| {
                cx.new(move |cx| PreferencesWindow::new(preferences, theme_options, cx))
            },
        )
        .expect("preferences window should open");

    handle
        .update(cx, |preferences, window, _cx| {
            window.activate_window();
            preferences.focus_handle.focus(window);
        })
        .expect("newly opened preferences window should be updateable");

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
    open_preferences_window_with_state(cx, preferences, theme_options, title)
}

#[cfg(test)]
mod tests {
    use super::{
        AiPreferences, AppPreferences, StartupOpenPreference,
        load_or_create_app_preferences_with_dirs_and_locales, open_preferences_window_with_state,
        read_app_preferences_with_dirs, save_app_preferences_with_dirs,
        save_preferences_from_window_with_dirs,
    };
    use crate::config::VelotypeConfigDirs;
    use crate::i18n::I18nManager;
    use crate::theme::{ThemeCatalogEntry, ThemeManager};
    use gpui::TestAppContext;
    use std::collections::BTreeMap;

    fn init_preferences_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init_with_theme_id(cx, "velotype");
            crate::components::init(cx);
        });
    }

    fn default_theme_options() -> Vec<ThemeCatalogEntry> {
        vec![ThemeCatalogEntry {
            id: "velotype".into(),
            name: "Velotype".into(),
        }]
    }

    #[test]
    fn missing_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("missing preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn partial_or_invalid_preferences_fall_back_by_field() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-partial-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "not-valid"

                [theme]
                default_theme_id = "velotype-light"
            "#,
        )
        .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("partial preferences should load");
        assert_eq!(preferences.startup_open, StartupOpenPreference::NewFile);
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velotype-light");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn damaged_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-damaged-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(dirs.app_config_file(), "not = [valid")
            .expect("preferences should be written");

        let preferences =
            read_app_preferences_with_dirs(&dirs).expect("damaged preferences should load");
        assert_eq!(preferences, AppPreferences::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_preferences() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-save-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::LastOpenedFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velotype-light".into(),
            keybindings: BTreeMap::new(),
            allow_code_execution: true,
            code_execution_confirm_shown: false,
            inline_code_run_in_system_terminal: false,
            ai: AiPreferences::default(),
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert_eq!(loaded, preferences);

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("open = \"last_opened_file\""));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        assert!(text.contains("default_theme_id = \"velotype-light\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_preferences_file_is_created_with_detected_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-create-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["zh-HK"])
            .expect("preferences should be created");
        assert_eq!(preferences.default_language_id, "zh-CN");
        assert!(dirs.app_config_file().exists());
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("[language]"));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_preferences_are_normalized_with_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-legacy-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = VelotypeConfigDirs::from_root(&root);
        std::fs::write(
            dirs.app_config_file(),
            r#"
                [startup]
                open = "last_opened_file"

                [theme]
                default_theme_id = "velotype-light"
            "#,
        )
        .expect("legacy preferences should be written");

        let preferences = load_or_create_app_preferences_with_dirs_and_locales(&dirs, ["en-GB"])
            .expect("legacy preferences should normalize");
        assert_eq!(
            preferences.startup_open,
            StartupOpenPreference::LastOpenedFile
        );
        assert_eq!(preferences.default_language_id, "en-US");
        assert_eq!(preferences.default_theme_id, "velotype-light");
        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("[language]"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saving_preferences_window_preserves_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-window-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::NewFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velotype".into(),
            keybindings: BTreeMap::new(),
            allow_code_execution: true,
            code_execution_confirm_shown: false,
            inline_code_run_in_system_terminal: false,
            ai: AiPreferences::default(),
        };
        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");

        let saved = save_preferences_from_window_with_dirs(
            StartupOpenPreference::LastOpenedFile,
            "velotype-light",
            BTreeMap::from([("save_document".to_string(), vec!["ctrl-alt-s".to_string()])]),
            false,
            true,
            AiPreferences::default(),
            &dirs,
        )
        .expect("window preferences should save");
        assert_eq!(saved.default_language_id, "zh-CN");
        assert_eq!(saved.startup_open, StartupOpenPreference::LastOpenedFile);
        assert_eq!(saved.default_theme_id, "velotype-light");
        assert_eq!(
            saved.keybindings.get("save_document"),
            Some(&vec!["ctrl-alt-s".to_string()])
        );
        assert!(saved.inline_code_run_in_system_terminal);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_inline_code_system_terminal_preference() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-inline-terminal-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = VelotypeConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            inline_code_run_in_system_terminal: true,
            ..AppPreferences::default()
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert!(loaded.inline_code_run_in_system_terminal);

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("inline_code_system_terminal = true"));
        let _ = std::fs::remove_dir_all(root);
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
