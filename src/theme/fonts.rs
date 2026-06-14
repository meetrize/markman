//! Editor font family resolution and GPUI font construction.

use std::sync::OnceLock;

use gpui::{Font, FontFallbacks, font};

/// Stored preference value: resolve to the platform default monospace stack.
pub const FONT_SYSTEM_MONO: &str = "__system_mono__";
/// Stored preference value: GPUI system UI font.
pub const FONT_SYSTEM_UI: &str = ".SystemUIFont";

/// A selectable font family shown in preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontPreset {
    pub id: &'static str,
}

pub const FONT_PRESETS: &[FontPreset] = &[
    FontPreset {
        id: FONT_SYSTEM_MONO,
    },
    FontPreset {
        id: FONT_SYSTEM_UI,
    },
    FontPreset {
        id: "Menlo",
    },
    FontPreset {
        id: "SF Mono",
    },
    FontPreset {
        id: "JetBrains Mono",
    },
    FontPreset {
        id: "Consolas",
    },
    FontPreset {
        id: "Fira Code",
    },
];

/// Default preview (rendered) font preference.
pub fn default_preview_font_family() -> String {
    FONT_SYSTEM_MONO.into()
}

/// Default source-mode font preference.
pub fn default_source_font_family() -> String {
    FONT_SYSTEM_UI.into()
}

/// Resolves a stored preference or theme value to a concrete GPUI font family name.
pub fn resolve_font_family(family: &str) -> String {
    match family.trim() {
        "" | FONT_SYSTEM_MONO => platform_monospace_font().into(),
        FONT_SYSTEM_UI => ".SystemUIFont".into(),
        other => other.into(),
    }
}

fn platform_monospace_font() -> &'static str {
    match std::env::consts::OS {
        "macos" => "Menlo",
        "windows" => "Consolas",
        _ => "DejaVu Sans Mono",
    }
}

fn tibetan_font_fallbacks_for_target_os(target_os: &str) -> Vec<String> {
    let families = match target_os {
        "windows" => &[
            "Microsoft Himalaya",
            "Noto Serif Tibetan",
            "Noto Sans Tibetan",
            "BabelStone Tibetan",
        ][..],
        "macos" => &["Kailasa", "Noto Serif Tibetan", "Noto Sans Tibetan"][..],
        _ => &[
            "Noto Serif Tibetan",
            "Noto Sans Tibetan",
            "Microsoft Himalaya",
            "Kailasa",
            "BabelStone Tibetan",
        ][..],
    };
    families
        .iter()
        .map(|family| (*family).to_string())
        .collect()
}

/// Builds an editor font for the given stored family id or concrete family name.
pub fn editor_font(family: &str) -> Font {
    static FALLBACKS: OnceLock<FontFallbacks> = OnceLock::new();
    let fallbacks = FALLBACKS
        .get_or_init(|| {
            FontFallbacks::from_fonts(tibetan_font_fallbacks_for_target_os(std::env::consts::OS))
        })
        .clone();
    let mut font = font(resolve_font_family(family));
    font.fallbacks = Some(fallbacks);
    font
}

/// Human-readable label for a stored font family id.
pub fn font_preset_label(family: &str, system_mono: &str, system_ui: &str) -> String {
    match family {
        FONT_SYSTEM_MONO => system_mono.into(),
        FONT_SYSTEM_UI => system_ui.into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_mono_resolves_to_platform_monospace() {
        assert_eq!(
            resolve_font_family(FONT_SYSTEM_MONO),
            platform_monospace_font()
        );
    }

    #[test]
    fn system_ui_resolves_to_gpui_system_font() {
        assert_eq!(resolve_font_family(FONT_SYSTEM_UI), ".SystemUIFont");
    }

    #[test]
    fn custom_family_is_passthrough() {
        assert_eq!(
            resolve_font_family("JetBrains Mono"),
            "JetBrains Mono"
        );
    }
}
