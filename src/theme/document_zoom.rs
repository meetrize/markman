//! Per-window document zoom applied to editor content (not chrome/toolbars).

use std::sync::Arc;

use gpui::{App, Global};

use super::{Theme, ThemeDimensions, ThemeManager, ThemeTypography};

/// Runtime document body line height ratio for the editor content currently rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DocumentBodyLineHeight {
    pub ratio: f32,
}

impl Global for DocumentBodyLineHeight {}

impl DocumentBodyLineHeight {
    pub fn new(ratio: f32) -> Self {
        Self {
            ratio: ratio.clamp(1.0, 3.0),
        }
    }
}

/// Minimum document zoom multiplier.
pub const MIN_DOCUMENT_ZOOM: f32 = 0.5;
/// Maximum document zoom multiplier.
pub const MAX_DOCUMENT_ZOOM: f32 = 3.0;
/// Toolbar button zoom step.
pub const DOCUMENT_ZOOM_STEP: f32 = 0.1;
/// Pinch / Ctrl+scroll zoom sensitivity (Y delta in pixels).
const PINCH_ZOOM_SENSITIVITY: f32 = 0.002;

/// Runtime document zoom multiplier for the editor content currently rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DocumentZoom {
    pub multiplier: f32,
}

impl Default for DocumentZoom {
    fn default() -> Self {
        Self { multiplier: 1.0 }
    }
}

impl Global for DocumentZoom {}

impl DocumentZoom {
    pub fn new(multiplier: f32) -> Self {
        Self {
            multiplier: Self::clamp(multiplier),
        }
    }

    pub fn clamp(multiplier: f32) -> f32 {
        multiplier.clamp(MIN_DOCUMENT_ZOOM, MAX_DOCUMENT_ZOOM)
    }

    pub fn zoom_in(current: f32) -> f32 {
        Self::clamp(current + DOCUMENT_ZOOM_STEP)
    }

    pub fn zoom_out(current: f32) -> f32 {
        Self::clamp(current - DOCUMENT_ZOOM_STEP)
    }

    pub fn factor_from_pinch_delta_y(delta_y: f32) -> f32 {
        (1.0 - delta_y * PINCH_ZOOM_SENSITIVITY).clamp(0.92, 1.08)
    }
}

fn scale_f32(value: f32, zoom: f32) -> f32 {
    value * zoom
}

impl ThemeTypography {
    pub(crate) fn scale_document_sizes(&mut self, zoom: f32) {
        self.text_size = scale_f32(self.text_size, zoom);
        self.h1_size = scale_f32(self.h1_size, zoom);
        self.h2_size = scale_f32(self.h2_size, zoom);
        self.h3_size = scale_f32(self.h3_size, zoom);
        self.h4_size = scale_f32(self.h4_size, zoom);
        self.h5_size = scale_f32(self.h5_size, zoom);
        self.h6_size = scale_f32(self.h6_size, zoom);
        self.code_size = scale_f32(self.code_size, zoom);
    }
}

impl ThemeDimensions {
    pub(crate) fn scale_document_layout(&mut self, zoom: f32) {
        self.editor_padding = scale_f32(self.editor_padding, zoom);
        self.block_gap = scale_f32(self.block_gap, zoom);
        self.block_min_height = scale_f32(self.block_min_height, zoom);
        self.block_padding_y = scale_f32(self.block_padding_y, zoom);
        self.block_padding_x = scale_f32(self.block_padding_x, zoom);
        self.nested_block_indent = scale_f32(self.nested_block_indent, zoom);
        self.list_marker_gap = scale_f32(self.list_marker_gap, zoom);
        self.list_marker_width = scale_f32(self.list_marker_width, zoom);
        self.ordered_list_marker_width = scale_f32(self.ordered_list_marker_width, zoom);
        self.task_checkbox_size = scale_f32(self.task_checkbox_size, zoom);
        self.task_checkbox_radius = scale_f32(self.task_checkbox_radius, zoom);
        self.task_checkbox_border_width = scale_f32(self.task_checkbox_border_width, zoom);
        self.task_checkbox_check_size = scale_f32(self.task_checkbox_check_size, zoom);
        self.h1_padding_bottom = scale_f32(self.h1_padding_bottom, zoom);
        self.h1_margin_bottom = scale_f32(self.h1_margin_bottom, zoom);
        self.cursor_width = scale_f32(self.cursor_width, zoom);
        self.underline_thickness = scale_f32(self.underline_thickness, zoom);
        self.h1_border_width = scale_f32(self.h1_border_width, zoom);
        self.quote_border_width = scale_f32(self.quote_border_width, zoom);
        self.quote_padding_left = scale_f32(self.quote_padding_left, zoom);
        self.callout_padding_x = scale_f32(self.callout_padding_x, zoom);
        self.callout_padding_y = scale_f32(self.callout_padding_y, zoom);
        self.callout_body_gap = scale_f32(self.callout_body_gap, zoom);
        self.callout_radius = scale_f32(self.callout_radius, zoom);
        self.callout_border_width = scale_f32(self.callout_border_width, zoom);
        self.callout_header_gap = scale_f32(self.callout_header_gap, zoom);
        self.callout_header_margin_bottom = scale_f32(self.callout_header_margin_bottom, zoom);
        self.footnote_padding_x = scale_f32(self.footnote_padding_x, zoom);
        self.footnote_padding_y = scale_f32(self.footnote_padding_y, zoom);
        self.footnote_radius = scale_f32(self.footnote_radius, zoom);
        self.footnote_badge_padding_x = scale_f32(self.footnote_badge_padding_x, zoom);
        self.footnote_badge_padding_y = scale_f32(self.footnote_badge_padding_y, zoom);
        self.separator_thickness = scale_f32(self.separator_thickness, zoom);
        self.separator_inset_x = scale_f32(self.separator_inset_x, zoom);
        self.separator_margin_y = scale_f32(self.separator_margin_y, zoom);
        self.code_block_padding_y = scale_f32(self.code_block_padding_y, zoom);
        self.code_block_padding_x = scale_f32(self.code_block_padding_x, zoom);
        self.code_bg_pad_x = scale_f32(self.code_bg_pad_x, zoom);
        self.code_bg_pad_y = scale_f32(self.code_bg_pad_y, zoom);
        self.code_bg_radius = scale_f32(self.code_bg_radius, zoom);
        self.code_language_input_width = scale_f32(self.code_language_input_width, zoom);
        self.code_language_input_height = scale_f32(self.code_language_input_height, zoom);
        self.code_language_input_padding_x = scale_f32(self.code_language_input_padding_x, zoom);
        self.code_language_input_padding_y = scale_f32(self.code_language_input_padding_y, zoom);
        self.code_language_input_radius = scale_f32(self.code_language_input_radius, zoom);
        self.code_language_input_border_width =
            scale_f32(self.code_language_input_border_width, zoom);
        self.code_language_input_gap = scale_f32(self.code_language_input_gap, zoom);
        self.table_cell_padding_x = scale_f32(self.table_cell_padding_x, zoom);
        self.table_cell_padding_y = scale_f32(self.table_cell_padding_y, zoom);
        self.table_cell_min_height = scale_f32(self.table_cell_min_height, zoom);
        self.table_append_button_extent = scale_f32(self.table_append_button_extent, zoom);
        self.table_append_button_inset = scale_f32(self.table_append_button_inset, zoom);
        self.table_append_activation_band = scale_f32(self.table_append_activation_band, zoom);
        self.image_radius = scale_f32(self.image_radius, zoom);
        self.image_root_max_height = scale_f32(self.image_root_max_height, zoom);
        self.image_cell_max_height = scale_f32(self.image_cell_max_height, zoom);
        self.image_root_placeholder_height = scale_f32(self.image_root_placeholder_height, zoom);
        self.image_cell_placeholder_height = scale_f32(self.image_cell_placeholder_height, zoom);
        self.image_caption_gap = scale_f32(self.image_caption_gap, zoom);
    }
}

impl Theme {
    pub(crate) fn with_document_zoom(&self, zoom: f32) -> Self {
        if (zoom - 1.0).abs() <= f32::EPSILON {
            return self.clone();
        }
        let mut theme = self.clone();
        theme.typography.scale_document_sizes(zoom);
        theme.dimensions.scale_document_layout(zoom);
        theme
    }

    pub(crate) fn with_document_body_line_height(&self, ratio: f32) -> Self {
        if (self.typography.text_line_height - ratio).abs() <= f32::EPSILON {
            return self.clone();
        }
        let mut theme = self.clone();
        theme.typography.text_line_height = ratio;
        theme
    }
}

impl ThemeManager {
    /// Returns the active theme scaled for document content according to [`DocumentZoom`]
    /// and the current [`DocumentBodyLineHeight`] override.
    pub fn document_theme_arc(&self, app: &App) -> Arc<Theme> {
        let zoom = app
            .try_global::<DocumentZoom>()
            .map(|zoom| zoom.multiplier)
            .unwrap_or(1.0);
        let line_height = app
            .try_global::<DocumentBodyLineHeight>()
            .map(|line_height| line_height.ratio)
            .filter(|ratio| ratio.is_finite() && *ratio > 0.0);
        let mut theme = if (zoom - 1.0).abs() <= f32::EPSILON {
            self.current().clone()
        } else {
            self.current().with_document_zoom(zoom)
        };
        if let Some(ratio) = line_height {
            theme = theme.with_document_body_line_height(ratio);
        }
        Arc::new(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_zoom_clamps_to_bounds() {
        assert_eq!(DocumentZoom::clamp(0.1), MIN_DOCUMENT_ZOOM);
        assert_eq!(DocumentZoom::clamp(10.0), MAX_DOCUMENT_ZOOM);
        assert_eq!(DocumentZoom::clamp(1.0), 1.0);
    }

    #[test]
    fn with_document_zoom_scales_body_text_size() {
        let theme = Theme::default_theme();
        let base_size = theme.typography.text_size;
        let zoomed = theme.with_document_zoom(1.5);
        assert!((zoomed.typography.text_size - base_size * 1.5).abs() < f32::EPSILON);
        assert!((zoomed.dimensions.editor_padding - theme.dimensions.editor_padding * 1.5).abs()
            < f32::EPSILON);
        assert_eq!(
            zoomed.dimensions.format_toolbar_button_height,
            theme.dimensions.format_toolbar_button_height
        );
    }

    #[test]
    fn relative_line_height_scales_with_font_size() {
        use gpui::{AbsoluteLength, DefiniteLength, px, relative, rems};

        let font_size = AbsoluteLength::Pixels(px(24.0));
        let rem_size = px(16.0);
        let relative_height = relative(1.4).to_pixels(font_size, rem_size);
        let absolute_rems_height =
            DefiniteLength::Absolute(rems(1.4).into()).to_pixels(font_size, rem_size);

        assert!((f32::from(relative_height) - 33.6).abs() < f32::EPSILON);
        assert!((f32::from(absolute_rems_height) - 22.4).abs() < f32::EPSILON);
    }
}
