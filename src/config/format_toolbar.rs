//! Markdown format toolbar button configuration.

use serde::Serialize;

/// One configurable button on the markdown format toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormatToolbarButtonConfig {
    pub id: String,
    pub enabled: bool,
}

impl FormatToolbarButtonConfig {
    pub(crate) fn new(id: impl Into<String>, enabled: bool) -> Self {
        Self {
            id: id.into(),
            enabled,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct FormatToolbarButtonConfigFile {
    id: String,
    enabled: bool,
}

impl From<&FormatToolbarButtonConfig> for FormatToolbarButtonConfigFile {
    fn from(value: &FormatToolbarButtonConfig) -> Self {
        Self {
            id: value.id.clone(),
            enabled: value.enabled,
        }
    }
}

const DEFAULT_FORMAT_TOOLBAR_BUTTON_IDS: &[&str] = &[
    "undo",
    "redo",
    "bold",
    "italic",
    "heading1",
    "heading2",
    "heading3",
    "ordered_list",
    "unordered_list",
    "code",
    "code_block",
    "link",
    "quote",
    "todo",
    "image",
    "horizontal_rule",
    "table_of_contents",
    "table",
    "mermaid",
    "ai",
    "document_search",
    "save",
    "auto_save",
    "zoom_out",
    "zoom_in",
    "view_mode",
];

pub(crate) fn is_valid_format_toolbar_button_id(id: &str) -> bool {
    DEFAULT_FORMAT_TOOLBAR_BUTTON_IDS.contains(&id)
}

pub(crate) fn default_format_toolbar_button_configs() -> Vec<FormatToolbarButtonConfig> {
    DEFAULT_FORMAT_TOOLBAR_BUTTON_IDS
        .iter()
        .map(|id| FormatToolbarButtonConfig::new(*id, true))
        .collect()
}

pub(crate) fn normalize_format_toolbar_button_configs(
    configs: Vec<FormatToolbarButtonConfig>,
) -> Vec<FormatToolbarButtonConfig> {
    let defaults = default_format_toolbar_button_configs();
    if configs.is_empty() {
        return defaults;
    }

    let mut result = Vec::with_capacity(defaults.len());
    let mut seen = std::collections::HashSet::new();

    for config in configs {
        let id = config.id.trim();
        if id.is_empty() || !is_valid_format_toolbar_button_id(id) {
            continue;
        }
        if !seen.insert(id.to_string()) {
            continue;
        }
        result.push(FormatToolbarButtonConfig::new(id, config.enabled));
    }

    for default in defaults {
        if !seen.contains(&default.id) {
            result.push(default);
        }
    }

    result
}

pub(crate) fn format_toolbar_buttons_from_toml(
    root: Option<&toml::Value>,
) -> Vec<FormatToolbarButtonConfig> {
    let Some(items) = root
        .and_then(|value| value.get("format_toolbar"))
        .and_then(|value| value.as_array())
    else {
        return default_format_toolbar_button_configs();
    };

    let buttons = items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.trim();
            if id.is_empty() || !is_valid_format_toolbar_button_id(id) {
                return None;
            }
            let enabled = item
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            Some(FormatToolbarButtonConfig::new(id, enabled))
        })
        .collect::<Vec<_>>();

    normalize_format_toolbar_button_configs(buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_falls_back_to_defaults() {
        let configs = normalize_format_toolbar_button_configs(vec![]);
        assert_eq!(configs.len(), default_format_toolbar_button_configs().len());
        assert!(configs.iter().all(|config| config.enabled));
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let configs = normalize_format_toolbar_button_configs(vec![
            FormatToolbarButtonConfig::new("unknown", true),
            FormatToolbarButtonConfig::new("bold", false),
        ]);
        assert_eq!(configs.len(), default_format_toolbar_button_configs().len());
        assert_eq!(
            configs
                .iter()
                .find(|config| config.id == "bold")
                .map(|config| config.enabled),
            Some(false)
        );
    }
}
