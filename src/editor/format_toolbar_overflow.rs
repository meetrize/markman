//! Responsive layout for the markdown format toolbar.

use crate::components::markdown::source_format::MarkdownToolbarAction;
use crate::config::format_toolbar::{
    FormatToolbarButtonConfig, normalize_format_toolbar_button_configs,
};
use crate::theme::ThemeDimensions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FormatToolbarControl {
    Undo,
    Redo,
    HistorySeparator,
    Format(MarkdownToolbarAction),
    FormatSeparator,
    MermaidTemplate,
    Ai,
    DocumentSearch,
    Save,
    AutoSave,
    ZoomOut,
    ZoomIn,
    ViewMode,
}

impl FormatToolbarControl {
    pub(super) fn config_id(self) -> Option<&'static str> {
        match self {
            Self::Undo => Some("undo"),
            Self::Redo => Some("redo"),
            Self::Format(MarkdownToolbarAction::Bold) => Some("bold"),
            Self::Format(MarkdownToolbarAction::Italic) => Some("italic"),
            Self::Format(MarkdownToolbarAction::Heading1) => Some("heading1"),
            Self::Format(MarkdownToolbarAction::Heading2) => Some("heading2"),
            Self::Format(MarkdownToolbarAction::Heading3) => Some("heading3"),
            Self::Format(MarkdownToolbarAction::OrderedList) => Some("ordered_list"),
            Self::Format(MarkdownToolbarAction::UnorderedList) => Some("unordered_list"),
            Self::Format(MarkdownToolbarAction::Code) => Some("code"),
            Self::Format(MarkdownToolbarAction::CodeBlock) => Some("code_block"),
            Self::Format(MarkdownToolbarAction::Link) => Some("link"),
            Self::Format(MarkdownToolbarAction::Quote) => Some("quote"),
            Self::Format(MarkdownToolbarAction::Todo) => Some("todo"),
            Self::Format(MarkdownToolbarAction::Image) => Some("image"),
            Self::Format(MarkdownToolbarAction::HorizontalRule) => Some("horizontal_rule"),
            Self::Format(MarkdownToolbarAction::TableOfContents) => Some("table_of_contents"),
            Self::Format(MarkdownToolbarAction::Table) => Some("table"),
            Self::MermaidTemplate => Some("mermaid"),
            Self::Ai => Some("ai"),
            Self::DocumentSearch => Some("document_search"),
            Self::Save => Some("save"),
            Self::AutoSave => Some("auto_save"),
            Self::ZoomOut => Some("zoom_out"),
            Self::ZoomIn => Some("zoom_in"),
            Self::ViewMode => Some("view_mode"),
            Self::HistorySeparator | Self::FormatSeparator => None,
        }
    }

    pub(super) fn from_config_id(id: &str) -> Option<Self> {
        match id {
            "undo" => Some(Self::Undo),
            "redo" => Some(Self::Redo),
            "bold" => Some(Self::Format(MarkdownToolbarAction::Bold)),
            "italic" => Some(Self::Format(MarkdownToolbarAction::Italic)),
            "heading1" => Some(Self::Format(MarkdownToolbarAction::Heading1)),
            "heading2" => Some(Self::Format(MarkdownToolbarAction::Heading2)),
            "heading3" => Some(Self::Format(MarkdownToolbarAction::Heading3)),
            "ordered_list" => Some(Self::Format(MarkdownToolbarAction::OrderedList)),
            "unordered_list" => Some(Self::Format(MarkdownToolbarAction::UnorderedList)),
            "code" => Some(Self::Format(MarkdownToolbarAction::Code)),
            "code_block" => Some(Self::Format(MarkdownToolbarAction::CodeBlock)),
            "link" => Some(Self::Format(MarkdownToolbarAction::Link)),
            "quote" => Some(Self::Format(MarkdownToolbarAction::Quote)),
            "todo" => Some(Self::Format(MarkdownToolbarAction::Todo)),
            "image" => Some(Self::Format(MarkdownToolbarAction::Image)),
            "horizontal_rule" => Some(Self::Format(MarkdownToolbarAction::HorizontalRule)),
            "table_of_contents" => Some(Self::Format(MarkdownToolbarAction::TableOfContents)),
            "table" => Some(Self::Format(MarkdownToolbarAction::Table)),
            "mermaid" => Some(Self::MermaidTemplate),
            "ai" => Some(Self::Ai),
            "document_search" => Some(Self::DocumentSearch),
            "save" => Some(Self::Save),
            "auto_save" => Some(Self::AutoSave),
            "zoom_out" => Some(Self::ZoomOut),
            "zoom_in" => Some(Self::ZoomIn),
            "view_mode" => Some(Self::ViewMode),
            _ => None,
        }
    }
}

pub(super) struct FormatToolbarLayout {
    pub visible: Vec<FormatToolbarControl>,
    pub overflow: Vec<FormatToolbarControl>,
}

pub(super) fn default_format_toolbar_controls() -> Vec<FormatToolbarControl> {
    format_toolbar_controls_from_config(&crate::config::format_toolbar::default_format_toolbar_button_configs())
}

pub(super) fn format_toolbar_controls_from_config(
    configs: &[FormatToolbarButtonConfig],
) -> Vec<FormatToolbarControl> {
    let normalized = normalize_format_toolbar_button_configs(configs.to_vec());
    let mut controls = Vec::new();
    let mut last_group: Option<u8> = None;

    for config in normalized.iter().filter(|config| config.enabled) {
        let Some(control) = FormatToolbarControl::from_config_id(&config.id) else {
            continue;
        };
        let group = format_toolbar_group(control);
        if let Some(last) = last_group {
            if last != group {
                controls.push(if last == 0 {
                    FormatToolbarControl::HistorySeparator
                } else {
                    FormatToolbarControl::FormatSeparator
                });
            }
        }
        controls.push(control);
        last_group = Some(group);
    }

    controls
}

pub(super) fn compute_format_toolbar_layout(
    controls: &[FormatToolbarControl],
    available_width: f32,
    dimensions: &ThemeDimensions,
) -> FormatToolbarLayout {
    let slots = controls.to_vec();
    if !available_width.is_finite() || available_width <= 0.0 {
        return FormatToolbarLayout {
            visible: slots,
            overflow: Vec::new(),
        };
    }

    let mut hidden = Vec::new();
    let mut visible = slots.clone();

    loop {
        strip_orphan_separators(&mut visible);
        let row_width = toolbar_row_width(&visible, dimensions, !hidden.is_empty());
        if row_width <= available_width || visible.is_empty() {
            break;
        }

        let Some((remove_idx, _)) = visible
            .iter()
            .enumerate()
            .filter(|(_, control)| is_overflow_candidate(**control))
            .min_by_key(|(_, control)| overflow_priority(**control))
        else {
            break;
        };

        let removed = visible.remove(remove_idx);
        hidden.push(removed);
    }

    hidden.sort_by_key(|control| slot_index(&slots, *control));

    FormatToolbarLayout { visible, overflow: hidden }
}

fn slot_index(slots: &[FormatToolbarControl], control: FormatToolbarControl) -> usize {
    slots
        .iter()
        .position(|slot| *slot == control)
        .unwrap_or(usize::MAX)
}

fn is_overflow_candidate(control: FormatToolbarControl) -> bool {
    !matches!(
        control,
        FormatToolbarControl::HistorySeparator | FormatToolbarControl::FormatSeparator
    )
}

fn strip_orphan_separators(visible: &mut Vec<FormatToolbarControl>) {
    loop {
        let mut changed = false;
        if visible.first() == Some(&FormatToolbarControl::FormatSeparator)
            || visible.first() == Some(&FormatToolbarControl::HistorySeparator)
        {
            visible.remove(0);
            changed = true;
        }
        if visible.last() == Some(&FormatToolbarControl::FormatSeparator) {
            visible.pop();
            changed = true;
        }
        let len = visible.len();
        let mut index = 1;
        while index < len {
            if visible[index] == FormatToolbarControl::FormatSeparator
                && visible[index - 1] == FormatToolbarControl::FormatSeparator
            {
                visible.remove(index);
                changed = true;
                break;
            }
            index += 1;
        }
        if !changed {
            break;
        }
    }
}

fn toolbar_row_width(
    visible: &[FormatToolbarControl],
    dimensions: &ThemeDimensions,
    include_overflow_button: bool,
) -> f32 {
    let padding = dimensions.format_toolbar_padding_x * 2.0;
    if visible.is_empty() && !include_overflow_button {
        return padding;
    }

    let mut width = padding;
    for (index, control) in visible.iter().enumerate() {
        if index > 0 {
            width += dimensions.format_toolbar_gap;
        }
        width += control_width(*control, dimensions);
    }

    if include_overflow_button {
        if !visible.is_empty() {
            width += dimensions.format_toolbar_gap;
        }
        width += dimensions.format_toolbar_button_height;
    }

    width
}

fn format_toolbar_group(control: FormatToolbarControl) -> u8 {
    match control {
        FormatToolbarControl::Undo | FormatToolbarControl::Redo => 0,
        FormatToolbarControl::Format(MarkdownToolbarAction::Bold)
        | FormatToolbarControl::Format(MarkdownToolbarAction::Italic) => 1,
        FormatToolbarControl::Format(MarkdownToolbarAction::Heading1)
        | FormatToolbarControl::Format(MarkdownToolbarAction::Heading2)
        | FormatToolbarControl::Format(MarkdownToolbarAction::Heading3) => 2,
        FormatToolbarControl::Format(MarkdownToolbarAction::OrderedList)
        | FormatToolbarControl::Format(MarkdownToolbarAction::UnorderedList) => 3,
        FormatToolbarControl::Format(MarkdownToolbarAction::Code)
        | FormatToolbarControl::Format(MarkdownToolbarAction::CodeBlock)
        | FormatToolbarControl::Format(MarkdownToolbarAction::Link)
        | FormatToolbarControl::Format(MarkdownToolbarAction::Quote) => 4,
        FormatToolbarControl::Format(MarkdownToolbarAction::Todo)
        | FormatToolbarControl::Format(MarkdownToolbarAction::Image) => 5,
        FormatToolbarControl::Format(MarkdownToolbarAction::HorizontalRule)
        | FormatToolbarControl::Format(MarkdownToolbarAction::TableOfContents) => 6,
        FormatToolbarControl::Format(MarkdownToolbarAction::Table)
        | FormatToolbarControl::MermaidTemplate => 7,
        FormatToolbarControl::Ai
        | FormatToolbarControl::DocumentSearch
        | FormatToolbarControl::Save
        | FormatToolbarControl::AutoSave
        | FormatToolbarControl::ZoomOut
        | FormatToolbarControl::ZoomIn
        | FormatToolbarControl::ViewMode => 8,
        FormatToolbarControl::HistorySeparator | FormatToolbarControl::FormatSeparator => 9,
    }
}

fn control_width(control: FormatToolbarControl, dimensions: &ThemeDimensions) -> f32 {
    match control {
        FormatToolbarControl::HistorySeparator | FormatToolbarControl::FormatSeparator => {
            dimensions.format_toolbar_separator_width
                + dimensions.format_toolbar_separator_margin_x * 2.0
        }
        FormatToolbarControl::Ai => {
            10.0 * 2.0 + dimensions.format_toolbar_icon_size + 4.0 + 14.0
        }
        _ => dimensions.format_toolbar_button_height,
    }
}

fn overflow_priority(control: FormatToolbarControl) -> u16 {
    match control {
        FormatToolbarControl::Format(MarkdownToolbarAction::Table) => 0,
        FormatToolbarControl::Format(MarkdownToolbarAction::TableOfContents) => 1,
        FormatToolbarControl::Format(MarkdownToolbarAction::HorizontalRule) => 2,
        FormatToolbarControl::Format(MarkdownToolbarAction::Image) => 3,
        FormatToolbarControl::Format(MarkdownToolbarAction::Todo) => 4,
        FormatToolbarControl::Format(MarkdownToolbarAction::Quote) => 5,
        FormatToolbarControl::Format(MarkdownToolbarAction::Link) => 6,
        FormatToolbarControl::Format(MarkdownToolbarAction::CodeBlock) => 7,
        FormatToolbarControl::Format(MarkdownToolbarAction::Code) => 8,
        FormatToolbarControl::Format(MarkdownToolbarAction::UnorderedList) => 9,
        FormatToolbarControl::Format(MarkdownToolbarAction::OrderedList) => 10,
        FormatToolbarControl::Format(MarkdownToolbarAction::Heading3) => 11,
        FormatToolbarControl::Format(MarkdownToolbarAction::Heading2) => 12,
        FormatToolbarControl::Format(MarkdownToolbarAction::Heading1) => 13,
        FormatToolbarControl::Format(MarkdownToolbarAction::Italic) => 14,
        FormatToolbarControl::Format(MarkdownToolbarAction::Bold) => 15,
        FormatToolbarControl::MermaidTemplate => 16,
        FormatToolbarControl::Ai => 17,
        FormatToolbarControl::DocumentSearch => 18,
        FormatToolbarControl::Save => 19,
        FormatToolbarControl::AutoSave => 20,
        FormatToolbarControl::ZoomOut => 21,
        FormatToolbarControl::ZoomIn => 22,
        FormatToolbarControl::ViewMode => 23,
        FormatToolbarControl::Undo => 24,
        FormatToolbarControl::Redo => 25,
        FormatToolbarControl::HistorySeparator | FormatToolbarControl::FormatSeparator => 26,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn narrow_toolbar_moves_low_priority_controls_to_overflow() {
        let theme = Theme::default_theme();
        let controls = default_format_toolbar_controls();
        let layout = compute_format_toolbar_layout(&controls, 220.0, &theme.dimensions);
        assert!(layout.overflow.contains(&FormatToolbarControl::Format(
            MarkdownToolbarAction::Table
        )));
        assert!(!layout.visible.is_empty());
    }

    #[test]
    fn wide_toolbar_keeps_all_controls_visible() {
        let theme = Theme::default_theme();
        let controls = default_format_toolbar_controls();
        let layout = compute_format_toolbar_layout(&controls, 2000.0, &theme.dimensions);
        assert!(layout.overflow.is_empty());
        assert_eq!(
            layout.visible.len(),
            default_format_toolbar_controls().len()
        );
    }
}
