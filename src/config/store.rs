//! Persistent app preference storage: TOML load/save, defaults, and merge logic.
//!
//! This module intentionally has no GPUI dependency.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context as _;
use serde::Serialize;

use super::ai_toolbar::{
    AiSelectionToolbarButton, AiSelectionToolbarButtonFile,
    ai_selection_toolbar_buttons_from_toml, default_ai_selection_toolbar_buttons,
    normalize_ai_selection_toolbar_buttons,
};
use super::format_toolbar::{
    FormatToolbarButtonConfig, FormatToolbarButtonConfigFile,
    default_format_toolbar_button_configs, format_toolbar_buttons_from_toml,
    normalize_format_toolbar_button_configs,
};
use super::{MarkmanConfigDirs, read_recent_files};
use crate::components::normalize_shortcut_config;
use crate::i18n::language_id_for_locale_preferences;
use crate::theme::normalize_builtin_theme_id;

pub(crate) const DEFAULT_THEME_ID: &str = "markman";
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
    /// When true, collapsible code blocks start expanded instead of folded when unfocused.
    pub(crate) code_block_default_expanded: bool,
    /// When true, code blocks show a line-number gutter in the editor.
    pub(crate) code_block_show_line_numbers: bool,
    pub(crate) ai: AiPreferences,
    pub(crate) format_toolbar: Vec<FormatToolbarButtonConfig>,
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
    pub(crate) selection_toolbar: Vec<AiSelectionToolbarButton>,
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
            selection_toolbar: default_ai_selection_toolbar_buttons(),
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
            code_block_default_expanded: false,
            code_block_show_line_numbers: true,
            ai: AiPreferences::default(),
            format_toolbar: default_format_toolbar_button_configs(),
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
    code_block: CodeBlockPreferencesFile,
    ai: AiPreferencesFile,
    format_toolbar: Vec<FormatToolbarButtonConfigFile>,
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
struct CodeBlockPreferencesFile {
    default_expanded: bool,
    show_line_numbers: bool,
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
    selection_toolbar: Vec<AiSelectionToolbarButtonFile>,
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
            code_block: CodeBlockPreferencesFile {
                default_expanded: value.code_block_default_expanded,
                show_line_numbers: value.code_block_show_line_numbers,
            },
            ai: AiPreferencesFile {
                provider: value.ai.provider.clone(),
                base_url: value.ai.base_url.clone(),
                model: value.ai.model.clone(),
                api_key_env: value.ai.api_key_env.clone(),
                allow_full_document_context: value.ai.allow_full_document_context,
                allow_workspace_context: value.ai.allow_workspace_context,
                allow_command_context: value.ai.allow_command_context,
                selection_toolbar: value
                    .ai
                    .selection_toolbar
                    .iter()
                    .map(AiSelectionToolbarButtonFile::from)
                    .collect(),
            },
            format_toolbar: value
                .format_toolbar
                .iter()
                .map(FormatToolbarButtonConfigFile::from)
                .collect(),
        }
    }
}

pub(crate) fn read_app_preferences() -> anyhow::Result<AppPreferences> {
    read_app_preferences_with_dirs(&MarkmanConfigDirs::from_system()?)
}

pub(crate) fn read_app_preferences_with_dirs(
    dirs: &MarkmanConfigDirs,
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
    let dirs = MarkmanConfigDirs::from_system()?;
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
    let default_theme_id = normalize_builtin_theme_id(
        value
            .get("theme")
            .and_then(|theme| theme.get("default_theme_id"))
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .unwrap_or(DEFAULT_THEME_ID),
    );
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
    let code_block_default_expanded = value
        .get("code_block")
        .and_then(|section| section.get("default_expanded"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let code_block_show_line_numbers = value
        .get("code_block")
        .and_then(|section| section.get("show_line_numbers"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let ai = ai_preferences_from_toml_value(value);
    let format_toolbar = format_toolbar_buttons_from_toml(Some(value));

    AppPreferences {
        startup_open,
        default_language_id,
        default_theme_id,
        keybindings,
        allow_code_execution,
        code_execution_confirm_shown,
        inline_code_run_in_system_terminal,
        code_block_default_expanded,
        code_block_show_line_numbers,
        ai,
        format_toolbar,
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
        selection_toolbar: ai_selection_toolbar_buttons_from_toml(section),
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

pub(crate) fn load_or_create_app_preferences_with_dirs_and_locales<I, S>(
    dirs: &MarkmanConfigDirs,
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
    save_app_preferences_with_dirs(preferences, &MarkmanConfigDirs::from_system()?)
}

pub(crate) fn save_app_preferences_with_dirs(
    preferences: &AppPreferences,
    dirs: &MarkmanConfigDirs,
) -> anyhow::Result<()> {
    let path = dirs.app_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create '{}'", parent.display()))?;
    }
    let mut normalized = preferences.clone();
    normalized.default_theme_id = normalize_builtin_theme_id(&normalized.default_theme_id);
    let text = toml::to_string_pretty(&PreferencesFile::from(&normalized))?;
    std::fs::write(&path, text).with_context(|| format!("failed to write '{}'", path.display()))
}

pub(crate) fn first_existing_recent_markdown_file() -> Option<PathBuf> {
    let recent_files = read_recent_files().ok()?;
    recent_files.into_iter().find(|path| path.is_file())
}

pub(crate) fn save_preferences_from_window(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    keybindings: BTreeMap<String, Vec<String>>,
    allow_code_execution: bool,
    inline_code_run_in_system_terminal: bool,
    code_block_default_expanded: bool,
    code_block_show_line_numbers: bool,
    ai: AiPreferences,
) -> anyhow::Result<AppPreferences> {
    let dirs = MarkmanConfigDirs::from_system()?;
    save_preferences_from_window_with_dirs(
        startup_open,
        default_theme_id,
        keybindings,
        allow_code_execution,
        inline_code_run_in_system_terminal,
        code_block_default_expanded,
        code_block_show_line_numbers,
        ai,
        &dirs,
    )
}

pub(crate) fn save_preferences_from_window_with_dirs(
    startup_open: StartupOpenPreference,
    default_theme_id: &str,
    keybindings: BTreeMap<String, Vec<String>>,
    allow_code_execution: bool,
    inline_code_run_in_system_terminal: bool,
    code_block_default_expanded: bool,
    code_block_show_line_numbers: bool,
    ai: AiPreferences,
    dirs: &MarkmanConfigDirs,
) -> anyhow::Result<AppPreferences> {
    let mut preferences =
        load_or_create_app_preferences_with_dirs_and_locales(dirs, sys_locale::get_locales())?;
    preferences.startup_open = startup_open;
    preferences.default_theme_id = normalize_builtin_theme_id(default_theme_id);
    preferences.keybindings = normalize_shortcut_config(&keybindings);
    preferences.allow_code_execution = allow_code_execution;
    preferences.inline_code_run_in_system_terminal = inline_code_run_in_system_terminal;
    preferences.code_block_default_expanded = code_block_default_expanded;
    preferences.code_block_show_line_numbers = code_block_show_line_numbers;
    preferences.ai = ai;
    preferences.ai.selection_toolbar =
        normalize_ai_selection_toolbar_buttons(preferences.ai.selection_toolbar);
    preferences.format_toolbar =
        normalize_format_toolbar_button_configs(preferences.format_toolbar);
    save_app_preferences_with_dirs(&preferences, dirs)?;
    Ok(preferences)
}

pub(crate) fn update_app_preferences(
    update: impl FnOnce(&mut AppPreferences),
) -> anyhow::Result<AppPreferences> {
    let mut preferences = load_or_create_app_preferences()?;
    update(&mut preferences);
    preferences.format_toolbar =
        normalize_format_toolbar_button_configs(preferences.format_toolbar);
    save_app_preferences(&preferences)?;
    Ok(preferences)
}

#[cfg(test)]
mod tests {
    use super::{
        AiPreferences, AppPreferences, StartupOpenPreference,
        load_or_create_app_preferences_with_dirs_and_locales, read_app_preferences_with_dirs,
        save_app_preferences_with_dirs, save_preferences_from_window_with_dirs,
    };
    use crate::config::format_toolbar::default_format_toolbar_button_configs;
    use crate::config::MarkmanConfigDirs;
    use std::collections::BTreeMap;

    #[test]
    fn missing_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = MarkmanConfigDirs::from_root(&root);
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
        let dirs = MarkmanConfigDirs::from_root(&root);
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
        assert_eq!(preferences.default_theme_id, "markman-light");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn damaged_preferences_file_returns_defaults() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-damaged-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("temp root should exist");
        let dirs = MarkmanConfigDirs::from_root(&root);
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
        let dirs = MarkmanConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::LastOpenedFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "markman-light".into(),
            keybindings: BTreeMap::new(),
            allow_code_execution: true,
            code_execution_confirm_shown: false,
            inline_code_run_in_system_terminal: false,
            code_block_default_expanded: false,
            code_block_show_line_numbers: true,
            ai: AiPreferences::default(),
            format_toolbar: default_format_toolbar_button_configs(),
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert_eq!(loaded, preferences);

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("open = \"last_opened_file\""));
        assert!(text.contains("default_language_id = \"zh-CN\""));
        assert!(text.contains("default_theme_id = \"markman-light\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_preferences_file_is_created_with_detected_language() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-create-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = MarkmanConfigDirs::from_root(&root);
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
        let dirs = MarkmanConfigDirs::from_root(&root);
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
        assert_eq!(preferences.default_theme_id, "markman-light");
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
        let dirs = MarkmanConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            startup_open: StartupOpenPreference::NewFile,
            default_language_id: "zh-CN".into(),
            default_theme_id: "velotype".into(),
            keybindings: BTreeMap::new(),
            allow_code_execution: true,
            code_execution_confirm_shown: false,
            inline_code_run_in_system_terminal: false,
            code_block_default_expanded: false,
            code_block_show_line_numbers: true,
            ai: AiPreferences::default(),
            format_toolbar: default_format_toolbar_button_configs(),
        };
        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");

        let saved = save_preferences_from_window_with_dirs(
            StartupOpenPreference::LastOpenedFile,
            "velotype-light",
            BTreeMap::from([("save_document".to_string(), vec!["ctrl-alt-s".to_string()])]),
            false,
            true,
            true,
            false,
            AiPreferences::default(),
            &dirs,
        )
        .expect("window preferences should save");
        assert_eq!(saved.default_language_id, "zh-CN");
        assert_eq!(saved.startup_open, StartupOpenPreference::LastOpenedFile);
        assert_eq!(saved.default_theme_id, "markman-light");
        assert_eq!(
            saved.keybindings.get("save_document"),
            Some(&vec!["ctrl-alt-s".to_string()])
        );
        assert!(saved.inline_code_run_in_system_terminal);
        assert!(saved.code_block_default_expanded);
        assert!(!saved.code_block_show_line_numbers);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_code_block_show_line_numbers_preference() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-code-block-line-numbers-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = MarkmanConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            code_block_show_line_numbers: false,
            ..AppPreferences::default()
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert!(!loaded.code_block_show_line_numbers);

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("show_line_numbers = false"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_code_block_default_expanded_preference() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-code-block-expanded-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = MarkmanConfigDirs::from_root(&root);
        let preferences = AppPreferences {
            code_block_default_expanded: true,
            ..AppPreferences::default()
        };

        save_app_preferences_with_dirs(&preferences, &dirs)
            .expect("preferences should save to config.toml");
        let loaded = read_app_preferences_with_dirs(&dirs).expect("preferences should read back");
        assert!(loaded.code_block_default_expanded);

        let text =
            std::fs::read_to_string(dirs.app_config_file()).expect("config.toml should exist");
        assert!(text.contains("default_expanded = true"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn saves_and_reads_inline_code_system_terminal_preference() {
        let root = std::env::temp_dir().join(format!(
            "velotype-preferences-inline-terminal-{}",
            uuid::Uuid::new_v4()
        ));
        let dirs = MarkmanConfigDirs::from_root(&root);
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
}
