//! Document column width calculations for centered editor content.

use crate::theme::ThemeDimensions;

/// Linearly interpolates the editor content width ratio based on viewport
/// width. The column stays full-width until `centered_shrink_start`, then
/// shrinks to `centered_min_ratio` at `centered_shrink_end`.
pub fn centered_column_ratio(viewport_width: f32, dimensions: &ThemeDimensions) -> f32 {
    if viewport_width <= dimensions.centered_shrink_start {
        return 1.0;
    }

    let t = ((viewport_width - dimensions.centered_shrink_start)
        / (dimensions.centered_shrink_end - dimensions.centered_shrink_start))
        .clamp(0.0, 1.0);
    1.0 - t * (1.0 - dimensions.centered_min_ratio)
}

pub fn centered_column_width(viewport_width: f32, dimensions: &ThemeDimensions) -> f32 {
    let available_content_width = (viewport_width - dimensions.editor_padding * 2.0).max(1.0);
    let centered_ratio = centered_column_ratio(viewport_width, dimensions);
    (available_content_width * centered_ratio)
        .max(320.0)
        .min(available_content_width)
}

#[cfg(test)]
mod tests {
    use crate::theme::Theme;

    use super::{centered_column_ratio, centered_column_width};

    #[test]
    fn centered_column_ratio_stays_full_before_shrink_start() {
        let theme = Theme::default_theme();
        assert_eq!(centered_column_ratio(900.0, &theme.dimensions), 1.0);
        assert_eq!(
            centered_column_ratio(theme.dimensions.centered_shrink_start, &theme.dimensions),
            1.0
        );
    }

    #[test]
    fn centered_column_ratio_stays_full_at_large_viewports() {
        let theme = Theme::default_theme();
        let ratio =
            centered_column_ratio(theme.dimensions.centered_shrink_end, &theme.dimensions);
        assert!((ratio - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn centered_column_width_respects_minimum() {
        let theme = Theme::default_theme();
        let width = centered_column_width(400.0, &theme.dimensions);
        assert!(width >= 320.0);
    }
}
