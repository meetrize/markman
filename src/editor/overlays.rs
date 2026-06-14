//! Editor overlay stack: collects active overlays and renders them in priority order.

use gpui::*;

use super::{Editor, ViewMode};
use crate::code_runner::CodeRunStatus;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

/// Overlay layers rendered above the main editor content, in stack order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditorOverlayKind {
    ContextMenu,
    AiToolbar,
    CodeLanguageMenu,
    WikiLinkPicker,
    MermaidTemplateMenu,
    FormatToolbarOverflowMenu,
    FormatToolbarContextMenu,
    FormatToolbarCustomizeDialog,
    WorkspaceFileContextMenu,
    WorkspaceNameDialog,
    SingleLineInputContextMenu,
    TableInsertDialog,
    InlineCodeRunGroup,
    AiPreview,
    AiPromptDialog,
    ModalExclusiveGroup,
}

impl Editor {
    pub(super) fn collect_active_overlays(&self, window: &Window, cx: &App) -> Vec<EditorOverlayKind> {
        let mut overlays = Vec::new();

        if self.context_menu.is_some() {
            overlays.push(EditorOverlayKind::ContextMenu);
        }
        if self.ai_floating_toolbar_active(window, cx) {
            overlays.push(EditorOverlayKind::AiToolbar);
        }
        if self.code_language_menu_block(cx).is_some() {
            overlays.push(EditorOverlayKind::CodeLanguageMenu);
        }
        if self.wiki_link_picker.open {
            overlays.push(EditorOverlayKind::WikiLinkPicker);
        }
        if self.mermaid_template_menu_position.is_some() {
            overlays.push(EditorOverlayKind::MermaidTemplateMenu);
        }
        if self.format_toolbar_overflow_menu_position.is_some() {
            overlays.push(EditorOverlayKind::FormatToolbarOverflowMenu);
        }
        if self.format_toolbar_context_menu.is_some() {
            overlays.push(EditorOverlayKind::FormatToolbarContextMenu);
        }
        if self.format_toolbar_customize_dialog.is_some() {
            overlays.push(EditorOverlayKind::FormatToolbarCustomizeDialog);
        }
        if self.workspace.file_context_menu.is_some() {
            overlays.push(EditorOverlayKind::WorkspaceFileContextMenu);
        }
        if self.workspace.name_dialog.is_some() {
            overlays.push(EditorOverlayKind::WorkspaceNameDialog);
        }
        if self.single_line_input_context_menu.is_some() {
            overlays.push(EditorOverlayKind::SingleLineInputContextMenu);
        }
        if self.table_insert_dialog.is_some() {
            overlays.push(EditorOverlayKind::TableInsertDialog);
        }
        if self.inline_code_run_overlay_group_active(window, cx) {
            overlays.push(EditorOverlayKind::InlineCodeRunGroup);
        }
        if self.ai_preview_overlay_active() {
            overlays.push(EditorOverlayKind::AiPreview);
        }
        if self.ai_prompt_dialog_active() {
            overlays.push(EditorOverlayKind::AiPromptDialog);
        }
        if self.modal_exclusive_group_active() {
            overlays.push(EditorOverlayKind::ModalExclusiveGroup);
        }

        overlays
    }

    pub(super) fn render_overlay(
        &self,
        kind: EditorOverlayKind,
        theme: &Theme,
        strings: &I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match kind {
            EditorOverlayKind::ContextMenu => {
                self.render_context_menu_overlay(theme, window, cx)
            }
            EditorOverlayKind::AiToolbar => self.render_ai_floating_toolbar(theme, window, cx),
            EditorOverlayKind::CodeLanguageMenu => {
                self.render_code_language_menu_overlay(theme, window, cx)
            }
            EditorOverlayKind::WikiLinkPicker => {
                self.render_wiki_link_picker_overlay(theme, strings, window, cx)
            }
            EditorOverlayKind::MermaidTemplateMenu => {
                self.render_mermaid_template_menu_overlay(theme, cx)
            }
            EditorOverlayKind::FormatToolbarOverflowMenu => {
                self.render_format_toolbar_overflow_menu_overlay(theme, cx)
            }
            EditorOverlayKind::FormatToolbarContextMenu => {
                self.render_format_toolbar_context_menu_overlay(theme, cx)
            }
            EditorOverlayKind::FormatToolbarCustomizeDialog => {
                self.render_format_toolbar_customize_dialog_overlay(theme, cx)
            }
            EditorOverlayKind::WorkspaceFileContextMenu => {
                self.render_workspace_file_context_menu_overlay(theme, cx)
            }
            EditorOverlayKind::WorkspaceNameDialog => {
                self.render_workspace_name_dialog_overlay(theme, cx)
            }
            EditorOverlayKind::SingleLineInputContextMenu => {
                self.render_single_line_input_context_menu_overlay(theme, cx)
            }
            EditorOverlayKind::TableInsertDialog => {
                self.render_table_insert_dialog_overlay(theme, cx)
            }
            EditorOverlayKind::InlineCodeRunGroup => {
                if let Some(popover) =
                    self.render_inline_code_run_popover_overlay(theme, window, cx)
                {
                    return Some(popover);
                }
                self.render_inline_code_run_button_overlay(window, cx)
            }
            EditorOverlayKind::AiPreview => self.render_ai_preview_overlay(theme, cx),
            EditorOverlayKind::AiPromptDialog => {
                self.render_ai_prompt_dialog_overlay(theme, window, cx)
            }
            EditorOverlayKind::ModalExclusiveGroup => self.render_modal_exclusive_group(
                theme, strings, cx,
            ),
        }
    }

    pub(super) fn render_overlay_stack(
        &self,
        base: Div,
        theme: &Theme,
        strings: &I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.collect_active_overlays(window, cx)
            .into_iter()
            .fold(base, |base, kind| {
                if let Some(element) = self.render_overlay(kind, theme, strings, window, cx) {
                    base.child(element)
                } else {
                    base
                }
            })
    }

    fn modal_exclusive_group_active(&self) -> bool {
        self.info_dialog.is_some()
            || self.code_run_dialog.is_some()
            || self.show_drop_replace_dialog
            || self.show_unsaved_changes_dialog
            || self.quick_file_open.open
    }

    fn render_modal_exclusive_group(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(kind) = self.info_dialog {
            return Some(self.render_info_dialog_overlay(theme, kind, cx).into_any_element());
        }
        if let Some(dialog) = self.render_code_run_dialog_overlay(theme, cx) {
            return Some(dialog.into_any_element());
        }
        if self.show_drop_replace_dialog {
            return Some(self.render_drop_replace_overlay(theme, cx).into_any_element());
        }
        if self.show_unsaved_changes_dialog {
            return Some(self.render_unsaved_changes_overlay(theme, cx).into_any_element());
        }
        self.render_quick_file_open_overlay(theme, strings, cx)
    }

    pub(super) fn inline_code_run_overlay_group_active(&self, window: &Window, cx: &App) -> bool {
        if let Some(target) = self.inline_code_run_popover.as_ref() {
            if self
                .inline_code_runs
                .get(target)
                .is_some_and(|state| state.status != CodeRunStatus::Idle)
                && self.inline_code_run_anchor_bounds(target, cx).is_some()
            {
                return true;
            }
        }
        self.inline_code_run_button_active(window, cx)
    }

    pub(super) fn inline_code_run_button_active(&self, window: &Window, cx: &App) -> bool {
        if !matches!(self.view_mode, ViewMode::Rendered) {
            return false;
        }
        let Some(block) = self.focused_edit_target(window, cx) else {
            return false;
        };
        let block_ref = block.read(cx);
        if block_ref.is_source_raw_mode() || block_ref.kind().is_code_block() {
            return false;
        }
        block_ref.inline_code_run_action_span().is_some()
    }
}
