//! AI selection floating toolbar button configuration.

use serde::Serialize;

/// Built-in AI selection toolbar actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiSelectionToolbarBuiltin {
    CustomPrompt,
    Improve,
    Summarize,
    Expand,
    Explain,
    Tasks,
}

impl AiSelectionToolbarBuiltin {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::CustomPrompt => "custom_prompt",
            Self::Improve => "improve",
            Self::Summarize => "summarize",
            Self::Expand => "expand",
            Self::Explain => "explain",
            Self::Tasks => "tasks",
        }
    }

    pub(crate) fn from_id(value: &str) -> Option<Self> {
        match value {
            "custom_prompt" => Some(Self::CustomPrompt),
            "improve" => Some(Self::Improve),
            "summarize" => Some(Self::Summarize),
            "expand" => Some(Self::Expand),
            "explain" => Some(Self::Explain),
            "tasks" => Some(Self::Tasks),
            _ => None,
        }
    }

    pub(crate) fn default_label(self) -> &'static str {
        match self {
            Self::CustomPrompt => "自定义",
            Self::Improve => "润色",
            Self::Summarize => "总结",
            Self::Expand => "扩写",
            Self::Explain => "解释",
            Self::Tasks => "任务",
        }
    }

    pub(crate) fn default_icon(self) -> &'static str {
        match self {
            Self::CustomPrompt => "icon/toolbar/sparkles.svg",
            Self::Improve => "icon/toolbar/wand-sparkles.svg",
            Self::Summarize => "icon/toolbar/list-collapse.svg",
            Self::Expand => "icon/toolbar/maximize-2.svg",
            Self::Explain => "icon/toolbar/circle-help.svg",
            Self::Tasks => "icon/toolbar/list-checks.svg",
        }
    }
}

/// One configurable button on the AI selection floating toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiSelectionToolbarButton {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub enabled: bool,
    /// Built-in action id, or `"prompt"` for user-defined AI actions.
    pub action: String,
    /// Optional instruction override for built-ins, or required prompt for custom buttons.
    pub instruction: Option<String>,
}

impl AiSelectionToolbarButton {
    pub(crate) fn from_builtin(builtin: AiSelectionToolbarBuiltin) -> Self {
        Self {
            id: builtin.id().into(),
            label: builtin.default_label().into(),
            icon: builtin.default_icon().into(),
            enabled: true,
            action: builtin.id().into(),
            instruction: None,
        }
    }

    pub(crate) fn is_custom_action(&self) -> bool {
        self.action == "prompt"
    }

    pub(crate) fn is_removable(&self) -> bool {
        self.is_custom_action()
    }

    pub(crate) fn resolved_icon(&self) -> &str {
        if self.icon.trim().is_empty() {
            if let Some(builtin) = AiSelectionToolbarBuiltin::from_id(&self.action) {
                return builtin.default_icon();
            }
            return "icon/toolbar/sparkles.svg";
        }
        self.icon.as_str()
    }

    pub(crate) fn new_custom(label: String, instruction: String) -> Self {
        Self {
            id: format!("custom-{}", uuid::Uuid::new_v4()),
            label,
            icon: "icon/toolbar/sparkles.svg".into(),
            enabled: true,
            action: "prompt".into(),
            instruction: Some(instruction),
        }
    }
}

pub(crate) fn default_ai_selection_toolbar_buttons() -> Vec<AiSelectionToolbarButton> {
    vec![
        AiSelectionToolbarButton::from_builtin(AiSelectionToolbarBuiltin::CustomPrompt),
        AiSelectionToolbarButton::from_builtin(AiSelectionToolbarBuiltin::Improve),
        AiSelectionToolbarButton::from_builtin(AiSelectionToolbarBuiltin::Summarize),
        AiSelectionToolbarButton::from_builtin(AiSelectionToolbarBuiltin::Expand),
        AiSelectionToolbarButton::from_builtin(AiSelectionToolbarBuiltin::Explain),
        AiSelectionToolbarButton::from_builtin(AiSelectionToolbarBuiltin::Tasks),
    ]
}

pub(crate) fn normalize_ai_selection_toolbar_buttons(
    buttons: Vec<AiSelectionToolbarButton>,
) -> Vec<AiSelectionToolbarButton> {
    let mut normalized = if buttons.is_empty() {
        default_ai_selection_toolbar_buttons()
    } else {
        buttons
    };
    for button in &mut normalized {
        if button.label.trim().is_empty() {
            if let Some(builtin) = AiSelectionToolbarBuiltin::from_id(&button.action) {
                button.label = builtin.default_label().into();
            } else if button.label.trim().is_empty() {
                button.label = "新按钮".into();
            }
        }
        if button.icon.trim().is_empty() {
            if let Some(builtin) = AiSelectionToolbarBuiltin::from_id(&button.action) {
                button.icon = builtin.default_icon().into();
            } else {
                button.icon = "icon/toolbar/sparkles.svg".into();
            }
        }
        if button.id.trim().is_empty() {
            button.id = if button.is_custom_action() {
                format!("custom-{}", uuid::Uuid::new_v4())
            } else {
                button.action.clone()
            };
        }
    }
    normalized
}

pub(crate) fn ai_selection_toolbar_buttons_from_toml(
    section: Option<&toml::Value>,
) -> Vec<AiSelectionToolbarButton> {
    let Some(items) = section
        .and_then(|section| section.get("selection_toolbar"))
        .and_then(|value| value.as_array())
    else {
        return default_ai_selection_toolbar_buttons();
    };

    let buttons = items
        .iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let action = item
                .get("action")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(id);
            let label = item
                .get("label")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    AiSelectionToolbarBuiltin::from_id(action)
                        .map(|builtin| builtin.default_label().to_string())
                        .unwrap_or_else(|| "新按钮".to_string())
                });
            let icon = item
                .get("icon")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    AiSelectionToolbarBuiltin::from_id(action)
                        .map(|builtin| builtin.default_icon().to_string())
                        .unwrap_or_else(|| "icon/toolbar/sparkles.svg".to_string())
                });
            let enabled = item
                .get("enabled")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            let instruction = item
                .get("instruction")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Some(AiSelectionToolbarButton {
                id: id.to_string(),
                label,
                icon,
                enabled,
                action: action.to_string(),
                instruction,
            })
        })
        .collect::<Vec<_>>();

    normalize_ai_selection_toolbar_buttons(buttons)
}

#[derive(Serialize)]
pub(crate) struct AiSelectionToolbarButtonFile {
    id: String,
    label: String,
    icon: String,
    enabled: bool,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instruction: Option<String>,
}

impl From<&AiSelectionToolbarButton> for AiSelectionToolbarButtonFile {
    fn from(value: &AiSelectionToolbarButton) -> Self {
        Self {
            id: value.id.clone(),
            label: value.label.clone(),
            icon: value.icon.clone(),
            enabled: value.enabled,
            action: value.action.clone(),
            instruction: value.instruction.clone(),
        }
    }
}

pub(crate) const AI_TOOLBAR_ICON_OPTIONS: &[&str] = &[
    "icon/toolbar/sparkles.svg",
    "icon/toolbar/wand-sparkles.svg",
    "icon/toolbar/list-collapse.svg",
    "icon/toolbar/maximize-2.svg",
    "icon/toolbar/circle-help.svg",
    "icon/toolbar/list-checks.svg",
    "icon/toolbar/bold.svg",
    "icon/toolbar/quote.svg",
    "icon/toolbar/code.svg",
];

#[cfg(test)]
mod tests {
    use super::{
        AiSelectionToolbarBuiltin,
        ai_selection_toolbar_buttons_from_toml, default_ai_selection_toolbar_buttons,
        normalize_ai_selection_toolbar_buttons,
    };

    #[test]
    fn default_toolbar_contains_all_builtins() {
        let buttons = default_ai_selection_toolbar_buttons();
        assert_eq!(buttons.len(), 6);
        assert!(buttons.iter().all(|button| button.enabled));
        assert_eq!(buttons[0].action, AiSelectionToolbarBuiltin::CustomPrompt.id());
    }

    #[test]
    fn empty_toolbar_falls_back_to_defaults() {
        let buttons = normalize_ai_selection_toolbar_buttons(vec![]);
        assert_eq!(buttons.len(), 6);
    }

    #[test]
    fn reads_toolbar_buttons_from_toml() {
        let value = toml::from_str::<toml::Value>(
            r#"
            [[ai.selection_toolbar]]
            id = "improve"
            label = "改写"
            action = "improve"
            enabled = false

            [[ai.selection_toolbar]]
            id = "custom-1"
            label = "翻译"
            action = "prompt"
            instruction = "Translate to English"
            icon = "icon/toolbar/sparkles.svg"
            "#,
        )
        .expect("toml should parse");
        let buttons = ai_selection_toolbar_buttons_from_toml(value.get("ai"));
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0].label, "改写");
        assert!(!buttons[0].enabled);
        assert_eq!(buttons[1].instruction.as_deref(), Some("Translate to English"));
    }
}
