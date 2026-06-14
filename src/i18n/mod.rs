//! Localised UI strings and runtime language selection.
//!
//! This module owns language packs, system-locale matching, and the global
//! manager used by menus and editor UI. Visual styling remains in `theme`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, bail};
use gpui::{App, Global};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::config::{
    MarkmanConfigDirs, catalog, object_without_empty_values, prune_empty_json_values,
    read_json_or_jsonc, sanitize_config_file_stem,
};
use crate::config::catalog::ConfigCatalog;

/// All localisable UI strings for the editor.
#[derive(Debug, Clone, Serialize)]
pub struct I18nStrings {
    /// Marker prepended to the window title when the document is dirty.
    pub dirty_title_marker: String,
    /// Title of the unsaved-changes dialog.
    pub unsaved_changes_title: String,
    /// Body message of the unsaved-changes dialog.
    pub unsaved_changes_message: String,
    /// Label for the "save and close" button.
    pub unsaved_changes_save_and_close: String,
    /// Label for the "discard and close" button.
    pub unsaved_changes_discard_and_close: String,
    /// Label for the "keep editing" button.
    pub unsaved_changes_cancel: String,
    /// Title of the dropped-file replacement dialog.
    pub drop_replace_title: String,
    /// Body message of the dropped-file replacement dialog.
    pub drop_replace_message: String,
    /// Label for saving before replacing the current document.
    pub drop_replace_save_and_replace: String,
    /// Label for replacing the current document without saving.
    pub drop_replace_discard_and_replace: String,
    /// Label for cancelling a dropped-file replacement.
    pub drop_replace_cancel: String,
    /// Prompt detail shown when no supported Markdown file was dropped.
    pub drop_no_markdown_file_message: String,
    /// Label for dismissing simple informational dialogs.
    pub info_dialog_ok: String,
    /// Title of the placeholder update-check dialog.
    pub help_check_updates_title: String,
    /// Body text shown while an update check is running.
    pub help_check_updates_message: String,
    /// Title shown when a newer version is available.
    pub update_available_title: String,
    /// Message template for newer-version prompts. Supports `{current}` and `{latest}`.
    pub update_available_message_template: String,
    /// Title shown when the running app is already current.
    pub update_up_to_date_title: String,
    /// Message template for up-to-date prompts. Supports `{current}` and `{latest}`.
    pub update_up_to_date_message_template: String,
    /// Title shown when an update check fails.
    pub update_failed_title: String,
    /// Message template for update-check failures. Supports `{error}`.
    pub update_failed_message_template: String,
    /// Button label for opening the GitHub Releases page.
    pub update_open_release: String,
    /// Button label for dismissing an available-update prompt.
    pub update_later: String,
    /// Title of the About dialog.
    pub help_about_title: String,
    /// Supplemental About dialog text shown below the app name and version.
    pub help_about_message: String,
    /// Label for the project repository link in the About dialog.
    pub help_about_github_label: String,
    /// Star request shown in the About dialog.
    pub help_about_star_message: String,
    /// Top-level File menu label.
    pub menu_file: String,
    /// Top-level Export menu label.
    pub menu_export: String,
    /// Top-level Language menu label.
    pub menu_language: String,
    /// Top-level Theme menu label.
    pub menu_theme: String,
    /// Top-level Workspace menu label.
    pub menu_workspace: String,
    /// Top-level Help menu label.
    pub menu_help: String,
    /// Language menu item for importing a custom language pack.
    pub menu_add_language_config: String,
    /// Theme menu item for importing a custom theme pack.
    pub menu_add_theme_config: String,
    /// File menu item for opening a new window.
    pub menu_new_window: String,
    /// File menu item for closing the current window.
    pub menu_close_window: String,
    /// File menu item for opening Markdown files.
    pub menu_open_file: String,
    /// File menu item for opening a local folder as workspace root.
    pub menu_open_folder: String,
    /// File menu item for opening a recent file submenu.
    pub menu_open_recent_file: String,
    /// File menu item for opening app preferences.
    pub menu_preferences: String,
    /// Placeholder item shown when no recent files are recorded.
    pub menu_no_recent_files: String,
    /// File menu item for saving the current document.
    pub menu_save: String,
    /// File menu item for saving the current document to a new path.
    pub menu_save_as: String,
    /// File menu item for quitting the app.
    pub menu_quit: String,
    /// Export menu item for writing an HTML document.
    pub menu_export_html: String,
    /// Export menu item for writing a PDF document.
    pub menu_export_pdf: String,
    /// Help menu item for checking updates.
    pub menu_check_updates: String,
    /// Help menu item for showing About information.
    pub menu_about: String,
    /// Help menu item for installing the CLI tool (symlink to /usr/local/bin).
    pub menu_install_cli_tool: String,
    /// Help menu item for uninstalling the CLI tool.
    pub menu_uninstall_cli_tool: String,
    /// Workspace menu item for opening or closing the workspace drawer.
    pub menu_toggle_workspace: String,
    /// Native file-dialog prompt for opening Markdown files.
    pub open_markdown_files_prompt: String,
    /// Native folder-dialog prompt for opening a workspace folder.
    pub open_folder_prompt: String,
    /// Native file-dialog prompt for importing a language pack.
    pub add_language_config_prompt: String,
    /// Native file-dialog prompt for importing a theme pack.
    pub add_theme_config_prompt: String,
    /// Title of the open-file failure prompt.
    pub open_failed_title: String,
    /// Title shown when a recent file path no longer exists.
    pub recent_file_missing_title: String,
    /// Message template for missing recent files. Supports `{path}`.
    pub recent_file_missing_message_template: String,
    /// Title of the save failure prompt.
    pub save_failed_title: String,
    /// Title of the export failure prompt.
    pub export_failed_title: String,
    /// Title shown when an export target already exists.
    pub export_overwrite_title: String,
    /// Body message shown when an export target already exists. Supports `{path}`.
    pub export_overwrite_message: String,
    /// Confirm button for replacing an existing export file.
    pub export_overwrite_confirm: String,
    /// Title of the custom configuration import failure prompt.
    pub config_import_failed_title: String,
    /// Preferences window title.
    pub preferences_window_title: String,
    /// File preferences navigation label.
    pub preferences_nav_file: String,
    /// Theme preferences navigation label.
    pub preferences_nav_theme: String,
    /// Shortcut preferences navigation label.
    pub preferences_nav_shortcuts: String,
    /// Startup option field label.
    pub preferences_startup_option: String,
    /// Startup option for creating a new Markdown document.
    pub preferences_startup_new_file: String,
    /// Startup option for opening the last opened Markdown document.
    pub preferences_startup_last_opened_file: String,
    /// Theme preference field label.
    pub preferences_local_theme: String,
    /// Save button label in the preferences window.
    pub preferences_save: String,
    /// Cancel button label in the preferences window.
    pub preferences_cancel: String,
    /// Title shown when preferences cannot be saved.
    pub preferences_save_failed_title: String,
    pub preferences_shortcuts_group_file: String,
    pub preferences_shortcuts_group_edit: String,
    pub preferences_shortcuts_group_navigation: String,
    pub preferences_shortcuts_group_formatting: String,
    pub preferences_shortcuts_group_block: String,
    pub preferences_shortcuts_group_other: String,
    pub preferences_shortcut_record: String,
    pub preferences_shortcut_reset: String,
    pub preferences_shortcut_recording: String,
    pub preferences_shortcut_conflict_template: String,
    pub preferences_shortcut_invalid_template: String,
    pub preferences_shortcut_newline: String,
    pub preferences_shortcut_delete_back: String,
    pub preferences_shortcut_delete: String,
    pub preferences_shortcut_word_delete_back: String,
    pub preferences_shortcut_word_delete_forward: String,
    pub preferences_shortcut_focus_prev: String,
    pub preferences_shortcut_focus_next: String,
    pub preferences_shortcut_move_left: String,
    pub preferences_shortcut_move_right: String,
    pub preferences_shortcut_word_move_left: String,
    pub preferences_shortcut_word_move_right: String,
    pub preferences_shortcut_home: String,
    pub preferences_shortcut_end: String,
    pub preferences_shortcut_block_up: String,
    pub preferences_shortcut_block_down: String,
    pub preferences_shortcut_select_left: String,
    pub preferences_shortcut_select_right: String,
    pub preferences_shortcut_word_select_left: String,
    pub preferences_shortcut_word_select_right: String,
    pub preferences_shortcut_select_home: String,
    pub preferences_shortcut_select_end: String,
    pub preferences_shortcut_select_all: String,
    pub preferences_shortcut_copy: String,
    pub preferences_shortcut_cut: String,
    pub preferences_shortcut_paste: String,
    pub preferences_shortcut_undo: String,
    pub preferences_shortcut_redo: String,
    pub preferences_shortcut_bold_selection: String,
    pub preferences_shortcut_italic_selection: String,
    pub preferences_shortcut_underline_selection: String,
    pub preferences_shortcut_code_selection: String,
    pub preferences_shortcut_indent_block: String,
    pub preferences_shortcut_outdent_block: String,
    pub preferences_shortcut_exit_code_block: String,
    pub preferences_shortcut_save_document: String,
    pub preferences_shortcut_save_document_as: String,
    pub preferences_shortcut_new_window: String,
    pub preferences_shortcut_open_file: String,
    pub preferences_shortcut_quit_application: String,
    pub preferences_shortcut_close_window: String,
    pub preferences_shortcut_dismiss_transient_ui: String,
    pub preferences_shortcut_toggle_view_mode: String,
    pub preferences_shortcut_toggle_workspace: String,
    pub preferences_shortcut_find_next_in_document: String,
    pub preferences_shortcut_find_previous_in_document: String,
    pub preferences_shortcut_quick_file_open: String,
    pub preferences_shortcut_open_workspace_search: String,
    /// Workspace drawer Files tab.
    pub workspace_tab_files: String,
    /// Workspace drawer Outline tab.
    pub workspace_tab_outline: String,
    /// Workspace drawer Tags tab.
    pub workspace_tab_tags: String,
    /// Workspace drawer Graph tab.
    pub workspace_tab_graph: String,
    /// Workspace drawer AI chat tab.
    pub workspace_tab_ai: String,
    /// Sidebar AI chat: new conversation button.
    pub workspace_ai_new_chat: String,
    /// Sidebar AI chat: open AI preferences.
    pub workspace_ai_settings: String,
    /// Sidebar AI chat: send button.
    pub workspace_ai_send: String,
    /// Sidebar AI chat: empty state hint.
    pub workspace_ai_empty: String,
    /// Sidebar AI chat: empty state when AI is not configured.
    pub workspace_ai_empty_no_api: String,
    /// Sidebar AI chat: empty state when prior request failed.
    pub workspace_ai_empty_error: String,
    /// Sidebar AI chat: draft input placeholder.
    pub workspace_ai_input_placeholder: String,
    /// Sidebar AI chat context mode: selection.
    pub workspace_ai_context_selection: String,
    /// Sidebar AI chat context mode: full document.
    pub workspace_ai_context_full: String,
    /// Sidebar AI chat context mode: blank.
    pub workspace_ai_context_blank: String,
    /// Sidebar AI chat context mode: workspace.
    pub workspace_ai_context_workspace: String,
    /// Sidebar AI chat context mode: code block.
    pub workspace_ai_context_command: String,
    /// Sidebar AI chat: open AI settings CTA.
    pub workspace_ai_error_no_api: String,
    /// Sidebar AI chat: copy assistant reply.
    pub workspace_ai_copy: String,
    /// Sidebar AI chat: insert assistant reply into document.
    pub workspace_ai_insert: String,
    /// Sidebar AI chat: untitled document label for pinned references.
    pub workspace_ai_untitled_document: String,
    /// Message shown when the workspace tag index is empty.
    pub workspace_empty_tags: String,
    /// Message shown while the knowledge graph is building.
    pub workspace_graph_building: String,
    /// Message shown when the knowledge graph has nothing to display.
    pub workspace_graph_empty: String,
    /// Toolbar button to fit the graph viewport.
    pub workspace_graph_fit_view: String,
    /// Toolbar button to recompute graph layout.
    pub workspace_graph_reset_layout: String,
    /// Toolbar button to open graph in a new window.
    pub workspace_graph_popout: String,
    /// Window title for the knowledge graph popout.
    pub workspace_graph_window_title: String,
    /// Toggle: keep graph nodes from overlapping.
    pub workspace_graph_mutual_repulsion: String,
    /// Toggle: velocity-based node collisions while dragging.
    pub workspace_graph_physics_collisions: String,
    /// Toolbar button to reduce edge crossings in the graph layout.
    pub workspace_graph_uncross_crossings: String,
    /// Graph filter: show only connected nodes.
    pub workspace_graph_filter_connected: String,
    /// Graph filter: show all markdown nodes.
    pub workspace_graph_filter_all: String,
    /// Toggle label to sort tags by name ascending.
    pub workspace_tag_sort_by_name: String,
    /// Toggle label to sort tags by count descending.
    pub workspace_tag_sort_by_count: String,
    /// Title above tag occurrence references; `{tag}` is replaced with `#name`.
    pub workspace_tag_occurrences_title: String,
    /// Workspace search input placeholder.
    pub workspace_search_placeholder: String,
    /// Message shown when workspace search finds no matches.
    pub workspace_search_no_results: String,
    /// Message shown when workspace search has no root directory.
    pub workspace_search_no_root: String,
    /// Title shown when no Markdown file path is available for workspace mode.
    pub workspace_no_file_title: String,
    /// Message shown when no Markdown file path is available for workspace mode.
    pub workspace_no_file_message: String,
    /// Message shown when a workspace directory has no visible Markdown files.
    pub workspace_empty_files: String,
    /// Message shown when the current document has no headings.
    pub workspace_empty_outline: String,
    /// Title shown when the workspace file tree cannot be scanned.
    pub workspace_scan_failed_title: String,
    /// Workspace file-tree context menu: create Markdown file.
    pub workspace_menu_new_file: String,
    /// Workspace file-tree context menu: create folder.
    pub workspace_menu_new_folder: String,
    /// Workspace file-tree context menu: rename item.
    pub workspace_menu_rename: String,
    /// Workspace file-tree context menu: delete item.
    pub workspace_menu_delete: String,
    /// Workspace file-tree context menu: copy absolute path.
    pub workspace_menu_copy_path: String,
    /// Workspace file-tree context menu: reveal in system file manager.
    pub workspace_menu_reveal_in_file_manager: String,
    /// Workspace file-tree context menu: refresh tree.
    pub workspace_menu_refresh: String,
    /// Dialog title for creating a Markdown file in the workspace tree.
    pub workspace_dialog_new_file_title: String,
    /// Dialog title for creating a folder in the workspace tree.
    pub workspace_dialog_new_folder_title: String,
    /// Dialog title for renaming a workspace tree item.
    pub workspace_dialog_rename_title: String,
    /// Confirmation title before deleting a workspace tree item.
    pub workspace_delete_confirm_title: String,
    /// Confirmation message before deleting a workspace tree item. Supports `{name}`.
    pub workspace_delete_confirm_message: String,
    /// Title shown when a workspace file operation fails.
    pub workspace_operation_failed_title: String,
    /// Default name for a new folder in the workspace tree.
    pub workspace_default_folder_name: String,
    /// Default name for a new Markdown file in the workspace tree.
    pub workspace_default_file_name: String,
    /// Title of the link-opening confirmation prompt.
    pub open_link_title: String,
    /// Confirm button for the link-opening prompt.
    pub open_link_open: String,
    /// Cancel button for the link-opening prompt.
    pub open_link_cancel: String,
    /// Compact label shown when rendered mode can switch to source mode.
    pub view_mode_source: String,
    /// Hover label shown when rendered mode can switch to source mode.
    pub view_mode_switch_to_source: String,
    /// Compact label shown when source mode can switch to rendered mode.
    pub view_mode_rendered: String,
    /// Hover label shown when source mode can switch to rendered mode.
    pub view_mode_switch_to_rendered: String,
    /// Markdown toolbar button label for bold formatting.
    pub format_toolbar_bold: String,
    /// Markdown toolbar button label for italic formatting.
    pub format_toolbar_italic: String,
    /// Markdown toolbar button label for heading formatting.
    pub format_toolbar_heading: String,
    /// Markdown toolbar button label for ordered-list formatting.
    pub format_toolbar_ordered_list: String,
    /// Markdown toolbar button label for unordered-list formatting.
    pub format_toolbar_unordered_list: String,
    /// Markdown toolbar button label for inline-code formatting.
    pub format_toolbar_code: String,
    /// Markdown toolbar button label for link formatting.
    pub format_toolbar_link: String,
    /// Markdown toolbar button label for blockquote formatting.
    pub format_toolbar_quote: String,
    /// Markdown toolbar button label for todo/task-list insertion.
    pub format_toolbar_todo: String,
    /// Markdown toolbar button label for horizontal rule insertion.
    pub format_toolbar_horizontal_rule: String,
    /// Markdown toolbar button label for image insertion.
    pub format_toolbar_image: String,
    /// Markdown toolbar button label for table-of-contents insertion.
    pub format_toolbar_table_of_contents: String,
    /// Mermaid template menu label for flowchart diagrams.
    pub mermaid_template_flowchart: String,
    /// Mermaid template menu label for mind-map diagrams.
    pub mermaid_template_mind_map: String,
    /// Mermaid template menu label for sequence diagrams.
    pub mermaid_template_sequence: String,
    /// Mermaid template menu label for Gantt charts.
    pub mermaid_template_gantt: String,
    /// Mermaid template menu label for state diagrams.
    pub mermaid_template_state: String,
    /// Mermaid template menu label for class diagrams.
    pub mermaid_template_class: String,
    /// Placeholder for the in-document search field.
    pub document_search_placeholder: String,
    /// Status text when document search is active: "{current}/{total}".
    pub document_search_status: String,
    /// Status text when document search finds no matches.
    pub document_search_no_matches: String,
    /// Status text before a document search query is entered.
    pub document_search_status_empty: String,
    /// Root context-menu insert label.
    pub context_menu_insert: String,
    /// Editor context menu: add selection to sidebar AI chat.
    pub context_menu_add_to_ai_chat: String,
    /// Insert submenu item for tables.
    pub context_menu_table: String,
    /// Table-axis menu item for left-aligning a column.
    pub table_axis_align_column_left: String,
    /// Table-axis menu item for center-aligning a column.
    pub table_axis_align_column_center: String,
    /// Table-axis menu item for right-aligning a column.
    pub table_axis_align_column_right: String,
    /// Table-axis menu item for moving a column left.
    pub table_axis_move_column_left: String,
    /// Table-axis menu item for moving a column right.
    pub table_axis_move_column_right: String,
    /// Table-axis menu item for deleting a column.
    pub table_axis_delete_column: String,
    /// Table-axis menu item for moving a row up.
    pub table_axis_move_row_up: String,
    /// Table-axis menu item for moving a row down.
    pub table_axis_move_row_down: String,
    /// Table-axis menu item for deleting a row.
    pub table_axis_delete_row: String,
    /// Title of the table-insert dialog.
    pub table_insert_title: String,
    /// Body text of the table-insert dialog.
    pub table_insert_description: String,
    /// Label for table body rows in the table-insert dialog.
    pub table_insert_body_rows: String,
    /// Label for table columns in the table-insert dialog.
    pub table_insert_columns: String,
    /// Cancel button in the table-insert dialog.
    pub table_insert_cancel: String,
    /// Confirm button in the table-insert dialog.
    pub table_insert_confirm: String,
    /// Placeholder label for rendered images without alt text.
    pub image_placeholder: String,
    /// Loading label for rendered images without alt text.
    pub image_loading_without_alt: String,
    /// Loading label template for rendered images with alt text; `{alt}` is replaced.
    pub image_loading_with_alt_template: String,
    /// Placeholder shown in the code-block language input when no language is set.
    pub code_language_placeholder: String,
    /// Title for the first-run code execution confirmation dialog.
    pub code_run_confirm_title: String,
    /// Body text for the first-run code execution confirmation dialog.
    pub code_run_confirm_message: String,
    /// Confirm button in the first-run code execution dialog.
    pub code_run_confirm_allow: String,
    /// Cancel button in the first-run code execution dialog.
    pub code_run_confirm_cancel: String,
    /// Title when running code from an unsaved document.
    pub code_run_unsaved_title: String,
    /// Body text when running code from an unsaved document.
    pub code_run_unsaved_message: String,
    /// Confirm button for running from an unsaved document.
    pub code_run_unsaved_confirm: String,
    /// Cancel button for running from an unsaved document.
    pub code_run_unsaved_cancel: String,
    /// Title when code execution is disabled in preferences.
    pub code_run_disabled_title: String,
    /// Body text when code execution is disabled in preferences.
    pub code_run_disabled_message: String,
    /// Title when the code-block language cannot be executed.
    pub code_run_unsupported_title: String,
    /// Body text when the code-block language cannot be executed.
    pub code_run_unsupported_message: String,
    /// Header label for the collapsible run-output panel.
    pub code_run_output_title: String,
    /// Expand label for the run-output panel.
    pub code_run_output_expand: String,
    /// Collapse label for the run-output panel.
    pub code_run_output_collapse: String,
    /// Stop button label while a run is active.
    pub code_run_stop: String,
    /// Close button label for the run-output panel.
    pub code_run_close: String,
    /// Expand-long-output label; `{count}` is replaced with hidden line count.
    pub code_run_output_expand_lines_template: String,
    /// Footer template for exit code and duration; `{exit}` and `{duration}` are replaced.
    pub code_run_meta_template: String,
    /// Placeholder exit code when the process did not report one.
    pub code_run_exit_none: String,
    /// Tooltip for the inline code run button.
    pub inline_code_run_tooltip: String,
    /// Title for the inline code run output popover.
    pub inline_code_run_output_title: String,
    /// Message shown when inline code was opened in the system terminal.
    pub inline_code_run_opened_in_terminal: String,
    /// Preferences label for allowing code execution.
    pub preferences_allow_code_execution_label: String,
    /// Preferences value when code execution is enabled.
    pub preferences_allow_code_execution_on: String,
    /// Preferences value when code execution is disabled.
    pub preferences_allow_code_execution_off: String,
    /// Preferences label for running inline code in the system terminal.
    pub preferences_inline_code_system_terminal_label: String,
    pub quick_file_open_placeholder: String,
}

/// Partial string set used by JSON language packs.
#[derive(Debug, Default, Deserialize)]
struct I18nStringsDe {
    dirty_title_marker: Option<String>,
    unsaved_changes_title: Option<String>,
    unsaved_changes_message: Option<String>,
    unsaved_changes_save_and_close: Option<String>,
    unsaved_changes_discard_and_close: Option<String>,
    unsaved_changes_cancel: Option<String>,
    drop_replace_title: Option<String>,
    drop_replace_message: Option<String>,
    drop_replace_save_and_replace: Option<String>,
    drop_replace_discard_and_replace: Option<String>,
    drop_replace_cancel: Option<String>,
    drop_no_markdown_file_message: Option<String>,
    info_dialog_ok: Option<String>,
    help_check_updates_title: Option<String>,
    help_check_updates_message: Option<String>,
    update_available_title: Option<String>,
    update_available_message_template: Option<String>,
    update_up_to_date_title: Option<String>,
    update_up_to_date_message_template: Option<String>,
    update_failed_title: Option<String>,
    update_failed_message_template: Option<String>,
    update_open_release: Option<String>,
    update_later: Option<String>,
    help_about_title: Option<String>,
    help_about_message: Option<String>,
    help_about_github_label: Option<String>,
    help_about_star_message: Option<String>,
    menu_file: Option<String>,
    menu_export: Option<String>,
    menu_language: Option<String>,
    menu_theme: Option<String>,
    menu_workspace: Option<String>,
    menu_help: Option<String>,
    menu_add_language_config: Option<String>,
    menu_add_theme_config: Option<String>,
    menu_new_window: Option<String>,
    menu_close_window: Option<String>,
    menu_open_file: Option<String>,
    menu_open_folder: Option<String>,
    menu_open_recent_file: Option<String>,
    menu_preferences: Option<String>,
    menu_no_recent_files: Option<String>,
    menu_save: Option<String>,
    menu_save_as: Option<String>,
    menu_quit: Option<String>,
    menu_export_html: Option<String>,
    menu_export_pdf: Option<String>,
    menu_check_updates: Option<String>,
    menu_about: Option<String>,
    menu_install_cli_tool: Option<String>,
    menu_uninstall_cli_tool: Option<String>,
    menu_toggle_workspace: Option<String>,
    open_markdown_files_prompt: Option<String>,
    open_folder_prompt: Option<String>,
    add_language_config_prompt: Option<String>,
    add_theme_config_prompt: Option<String>,
    open_failed_title: Option<String>,
    recent_file_missing_title: Option<String>,
    recent_file_missing_message_template: Option<String>,
    save_failed_title: Option<String>,
    export_failed_title: Option<String>,
    export_overwrite_title: Option<String>,
    export_overwrite_message: Option<String>,
    export_overwrite_confirm: Option<String>,
    config_import_failed_title: Option<String>,
    preferences_window_title: Option<String>,
    preferences_nav_file: Option<String>,
    preferences_nav_theme: Option<String>,
    preferences_nav_shortcuts: Option<String>,
    preferences_startup_option: Option<String>,
    preferences_startup_new_file: Option<String>,
    preferences_startup_last_opened_file: Option<String>,
    preferences_local_theme: Option<String>,
    preferences_save: Option<String>,
    preferences_cancel: Option<String>,
    preferences_save_failed_title: Option<String>,
    preferences_shortcuts_group_file: Option<String>,
    preferences_shortcuts_group_edit: Option<String>,
    preferences_shortcuts_group_navigation: Option<String>,
    preferences_shortcuts_group_formatting: Option<String>,
    preferences_shortcuts_group_block: Option<String>,
    preferences_shortcuts_group_other: Option<String>,
    preferences_shortcut_record: Option<String>,
    preferences_shortcut_reset: Option<String>,
    preferences_shortcut_recording: Option<String>,
    preferences_shortcut_conflict_template: Option<String>,
    preferences_shortcut_invalid_template: Option<String>,
    preferences_shortcut_newline: Option<String>,
    preferences_shortcut_delete_back: Option<String>,
    preferences_shortcut_delete: Option<String>,
    preferences_shortcut_word_delete_back: Option<String>,
    preferences_shortcut_word_delete_forward: Option<String>,
    preferences_shortcut_focus_prev: Option<String>,
    preferences_shortcut_focus_next: Option<String>,
    preferences_shortcut_move_left: Option<String>,
    preferences_shortcut_move_right: Option<String>,
    preferences_shortcut_word_move_left: Option<String>,
    preferences_shortcut_word_move_right: Option<String>,
    preferences_shortcut_home: Option<String>,
    preferences_shortcut_end: Option<String>,
    preferences_shortcut_block_up: Option<String>,
    preferences_shortcut_block_down: Option<String>,
    preferences_shortcut_select_left: Option<String>,
    preferences_shortcut_select_right: Option<String>,
    preferences_shortcut_word_select_left: Option<String>,
    preferences_shortcut_word_select_right: Option<String>,
    preferences_shortcut_select_home: Option<String>,
    preferences_shortcut_select_end: Option<String>,
    preferences_shortcut_select_all: Option<String>,
    preferences_shortcut_copy: Option<String>,
    preferences_shortcut_cut: Option<String>,
    preferences_shortcut_paste: Option<String>,
    preferences_shortcut_undo: Option<String>,
    preferences_shortcut_redo: Option<String>,
    preferences_shortcut_bold_selection: Option<String>,
    preferences_shortcut_italic_selection: Option<String>,
    preferences_shortcut_underline_selection: Option<String>,
    preferences_shortcut_code_selection: Option<String>,
    preferences_shortcut_indent_block: Option<String>,
    preferences_shortcut_outdent_block: Option<String>,
    preferences_shortcut_exit_code_block: Option<String>,
    preferences_shortcut_save_document: Option<String>,
    preferences_shortcut_save_document_as: Option<String>,
    preferences_shortcut_new_window: Option<String>,
    preferences_shortcut_open_file: Option<String>,
    preferences_shortcut_quit_application: Option<String>,
    preferences_shortcut_close_window: Option<String>,
    preferences_shortcut_dismiss_transient_ui: Option<String>,
    preferences_shortcut_toggle_view_mode: Option<String>,
    preferences_shortcut_toggle_workspace: Option<String>,
    preferences_shortcut_find_next_in_document: Option<String>,
    preferences_shortcut_find_previous_in_document: Option<String>,
    preferences_shortcut_quick_file_open: Option<String>,
    preferences_shortcut_open_workspace_search: Option<String>,
    workspace_tab_files: Option<String>,
    workspace_tab_outline: Option<String>,
    workspace_tab_tags: Option<String>,
    workspace_tab_graph: Option<String>,
    workspace_tab_ai: Option<String>,
    workspace_ai_new_chat: Option<String>,
    workspace_ai_settings: Option<String>,
    workspace_ai_send: Option<String>,
    workspace_ai_empty: Option<String>,
    workspace_ai_empty_no_api: Option<String>,
    workspace_ai_empty_error: Option<String>,
    workspace_ai_input_placeholder: Option<String>,
    workspace_ai_context_selection: Option<String>,
    workspace_ai_context_full: Option<String>,
    workspace_ai_context_blank: Option<String>,
    workspace_ai_context_workspace: Option<String>,
    workspace_ai_context_command: Option<String>,
    workspace_ai_error_no_api: Option<String>,
    workspace_ai_copy: Option<String>,
    workspace_ai_insert: Option<String>,
    workspace_ai_untitled_document: Option<String>,
    workspace_empty_tags: Option<String>,
    workspace_graph_building: Option<String>,
    workspace_graph_empty: Option<String>,
    workspace_graph_fit_view: Option<String>,
    workspace_graph_reset_layout: Option<String>,
    workspace_graph_popout: Option<String>,
    workspace_graph_window_title: Option<String>,
    workspace_graph_mutual_repulsion: Option<String>,
    workspace_graph_physics_collisions: Option<String>,
    workspace_graph_uncross_crossings: Option<String>,
    workspace_graph_filter_connected: Option<String>,
    workspace_graph_filter_all: Option<String>,
    workspace_tag_sort_by_name: Option<String>,
    workspace_tag_sort_by_count: Option<String>,
    workspace_tag_occurrences_title: Option<String>,
    workspace_search_placeholder: Option<String>,
    workspace_search_no_results: Option<String>,
    workspace_search_no_root: Option<String>,
    workspace_no_file_title: Option<String>,
    workspace_no_file_message: Option<String>,
    workspace_empty_files: Option<String>,
    workspace_empty_outline: Option<String>,
    workspace_scan_failed_title: Option<String>,
    workspace_menu_new_file: Option<String>,
    workspace_menu_new_folder: Option<String>,
    workspace_menu_rename: Option<String>,
    workspace_menu_delete: Option<String>,
    workspace_menu_copy_path: Option<String>,
    workspace_menu_reveal_in_file_manager: Option<String>,
    workspace_menu_refresh: Option<String>,
    workspace_dialog_new_file_title: Option<String>,
    workspace_dialog_new_folder_title: Option<String>,
    workspace_dialog_rename_title: Option<String>,
    workspace_delete_confirm_title: Option<String>,
    workspace_delete_confirm_message: Option<String>,
    workspace_operation_failed_title: Option<String>,
    workspace_default_folder_name: Option<String>,
    workspace_default_file_name: Option<String>,
    open_link_title: Option<String>,
    open_link_open: Option<String>,
    open_link_cancel: Option<String>,
    view_mode_source: Option<String>,
    view_mode_switch_to_source: Option<String>,
    view_mode_rendered: Option<String>,
    view_mode_switch_to_rendered: Option<String>,
    format_toolbar_bold: Option<String>,
    format_toolbar_italic: Option<String>,
    format_toolbar_heading: Option<String>,
    format_toolbar_ordered_list: Option<String>,
    format_toolbar_unordered_list: Option<String>,
    format_toolbar_code: Option<String>,
    format_toolbar_link: Option<String>,
    format_toolbar_quote: Option<String>,
    format_toolbar_todo: Option<String>,
    format_toolbar_horizontal_rule: Option<String>,
    format_toolbar_image: Option<String>,
    format_toolbar_table_of_contents: Option<String>,
    mermaid_template_flowchart: Option<String>,
    mermaid_template_mind_map: Option<String>,
    mermaid_template_sequence: Option<String>,
    mermaid_template_gantt: Option<String>,
    mermaid_template_state: Option<String>,
    mermaid_template_class: Option<String>,
    document_search_placeholder: Option<String>,
    document_search_status: Option<String>,
    document_search_no_matches: Option<String>,
    document_search_status_empty: Option<String>,
    context_menu_insert: Option<String>,
    context_menu_add_to_ai_chat: Option<String>,
    context_menu_table: Option<String>,
    table_axis_align_column_left: Option<String>,
    table_axis_align_column_center: Option<String>,
    table_axis_align_column_right: Option<String>,
    table_axis_move_column_left: Option<String>,
    table_axis_move_column_right: Option<String>,
    table_axis_delete_column: Option<String>,
    table_axis_move_row_up: Option<String>,
    table_axis_move_row_down: Option<String>,
    table_axis_delete_row: Option<String>,
    table_insert_title: Option<String>,
    table_insert_description: Option<String>,
    table_insert_body_rows: Option<String>,
    table_insert_columns: Option<String>,
    table_insert_cancel: Option<String>,
    table_insert_confirm: Option<String>,
    image_placeholder: Option<String>,
    image_loading_without_alt: Option<String>,
    image_loading_with_alt_template: Option<String>,
    code_language_placeholder: Option<String>,
    code_run_confirm_title: Option<String>,
    code_run_confirm_message: Option<String>,
    code_run_confirm_allow: Option<String>,
    code_run_confirm_cancel: Option<String>,
    code_run_unsaved_title: Option<String>,
    code_run_unsaved_message: Option<String>,
    code_run_unsaved_confirm: Option<String>,
    code_run_unsaved_cancel: Option<String>,
    code_run_disabled_title: Option<String>,
    code_run_disabled_message: Option<String>,
    code_run_unsupported_title: Option<String>,
    code_run_unsupported_message: Option<String>,
    code_run_output_title: Option<String>,
    code_run_output_expand: Option<String>,
    code_run_output_collapse: Option<String>,
    code_run_stop: Option<String>,
    code_run_close: Option<String>,
    code_run_output_expand_lines_template: Option<String>,
    code_run_meta_template: Option<String>,
    code_run_exit_none: Option<String>,
    inline_code_run_tooltip: Option<String>,
    inline_code_run_output_title: Option<String>,
    inline_code_run_opened_in_terminal: Option<String>,
    preferences_allow_code_execution_label: Option<String>,
    preferences_allow_code_execution_on: Option<String>,
    preferences_allow_code_execution_off: Option<String>,
    preferences_inline_code_system_terminal_label: Option<String>,
    quick_file_open_placeholder: Option<String>,
}

const I18N_STRING_KEYS: &[&str] = &[
    "dirty_title_marker",
    "unsaved_changes_title",
    "unsaved_changes_message",
    "unsaved_changes_save_and_close",
    "unsaved_changes_discard_and_close",
    "unsaved_changes_cancel",
    "drop_replace_title",
    "drop_replace_message",
    "drop_replace_save_and_replace",
    "drop_replace_discard_and_replace",
    "drop_replace_cancel",
    "drop_no_markdown_file_message",
    "info_dialog_ok",
    "help_check_updates_title",
    "help_check_updates_message",
    "update_available_title",
    "update_available_message_template",
    "update_up_to_date_title",
    "update_up_to_date_message_template",
    "update_failed_title",
    "update_failed_message_template",
    "update_open_release",
    "update_later",
    "help_about_title",
    "help_about_message",
    "help_about_github_label",
    "help_about_star_message",
    "menu_file",
    "menu_export",
    "menu_language",
    "menu_theme",
    "menu_workspace",
    "menu_help",
    "menu_add_language_config",
    "menu_add_theme_config",
    "menu_new_window",
    "menu_close_window",
    "menu_open_file",
    "menu_open_folder",
    "menu_open_recent_file",
    "menu_preferences",
    "menu_no_recent_files",
    "menu_save",
    "menu_save_as",
    "menu_quit",
    "menu_export_html",
    "menu_export_pdf",
    "menu_check_updates",
    "menu_about",
    "menu_install_cli_tool",
    "menu_uninstall_cli_tool",
    "menu_toggle_workspace",
    "open_markdown_files_prompt",
    "open_folder_prompt",
    "add_language_config_prompt",
    "add_theme_config_prompt",
    "open_failed_title",
    "recent_file_missing_title",
    "recent_file_missing_message_template",
    "save_failed_title",
    "export_failed_title",
    "export_overwrite_title",
    "export_overwrite_message",
    "export_overwrite_confirm",
    "config_import_failed_title",
    "preferences_window_title",
    "preferences_nav_file",
    "preferences_nav_theme",
    "preferences_nav_shortcuts",
    "preferences_startup_option",
    "preferences_startup_new_file",
    "preferences_startup_last_opened_file",
    "preferences_local_theme",
    "preferences_save",
    "preferences_cancel",
    "preferences_save_failed_title",
    "preferences_shortcuts_group_file",
    "preferences_shortcuts_group_edit",
    "preferences_shortcuts_group_navigation",
    "preferences_shortcuts_group_formatting",
    "preferences_shortcuts_group_block",
    "preferences_shortcuts_group_other",
    "preferences_shortcut_record",
    "preferences_shortcut_reset",
    "preferences_shortcut_recording",
    "preferences_shortcut_conflict_template",
    "preferences_shortcut_invalid_template",
    "preferences_shortcut_newline",
    "preferences_shortcut_delete_back",
    "preferences_shortcut_delete",
    "preferences_shortcut_word_delete_back",
    "preferences_shortcut_word_delete_forward",
    "preferences_shortcut_focus_prev",
    "preferences_shortcut_focus_next",
    "preferences_shortcut_move_left",
    "preferences_shortcut_move_right",
    "preferences_shortcut_word_move_left",
    "preferences_shortcut_word_move_right",
    "preferences_shortcut_home",
    "preferences_shortcut_end",
    "preferences_shortcut_block_up",
    "preferences_shortcut_block_down",
    "preferences_shortcut_select_left",
    "preferences_shortcut_select_right",
    "preferences_shortcut_word_select_left",
    "preferences_shortcut_word_select_right",
    "preferences_shortcut_select_home",
    "preferences_shortcut_select_end",
    "preferences_shortcut_select_all",
    "preferences_shortcut_copy",
    "preferences_shortcut_cut",
    "preferences_shortcut_paste",
    "preferences_shortcut_undo",
    "preferences_shortcut_redo",
    "preferences_shortcut_bold_selection",
    "preferences_shortcut_italic_selection",
    "preferences_shortcut_underline_selection",
    "preferences_shortcut_code_selection",
    "preferences_shortcut_indent_block",
    "preferences_shortcut_outdent_block",
    "preferences_shortcut_exit_code_block",
    "preferences_shortcut_save_document",
    "preferences_shortcut_save_document_as",
    "preferences_shortcut_new_window",
    "preferences_shortcut_open_file",
    "preferences_shortcut_quit_application",
    "preferences_shortcut_close_window",
    "preferences_shortcut_dismiss_transient_ui",
    "preferences_shortcut_toggle_view_mode",
    "preferences_shortcut_toggle_workspace",
    "preferences_shortcut_find_next_in_document",
    "preferences_shortcut_find_previous_in_document",
    "preferences_shortcut_quick_file_open",
    "preferences_shortcut_open_workspace_search",
    "workspace_tab_files",
    "workspace_tab_outline",
    "workspace_tab_tags",
    "workspace_tab_graph",
    "workspace_tab_ai",
    "workspace_ai_new_chat",
    "workspace_ai_settings",
    "workspace_ai_send",
    "workspace_ai_empty",
    "workspace_ai_empty_no_api",
    "workspace_ai_empty_error",
    "workspace_ai_input_placeholder",
    "workspace_ai_context_selection",
    "workspace_ai_context_full",
    "workspace_ai_context_blank",
    "workspace_ai_context_workspace",
    "workspace_ai_context_command",
    "workspace_ai_error_no_api",
    "workspace_ai_copy",
    "workspace_ai_insert",
    "workspace_ai_untitled_document",
    "workspace_empty_tags",
    "workspace_graph_building",
    "workspace_graph_empty",
    "workspace_graph_fit_view",
    "workspace_graph_reset_layout",
    "workspace_graph_popout",
    "workspace_graph_window_title",
    "workspace_graph_mutual_repulsion",
    "workspace_graph_physics_collisions",
    "workspace_graph_uncross_crossings",
    "workspace_graph_filter_connected",
    "workspace_graph_filter_all",
    "workspace_tag_sort_by_name",
    "workspace_tag_sort_by_count",
    "workspace_tag_occurrences_title",
    "workspace_search_placeholder",
    "workspace_search_no_results",
    "workspace_search_no_root",
    "workspace_no_file_title",
    "workspace_no_file_message",
    "workspace_empty_files",
    "workspace_empty_outline",
    "workspace_scan_failed_title",
    "workspace_menu_new_file",
    "workspace_menu_new_folder",
    "workspace_menu_rename",
    "workspace_menu_delete",
    "workspace_menu_copy_path",
    "workspace_menu_reveal_in_file_manager",
    "workspace_menu_refresh",
    "workspace_dialog_new_file_title",
    "workspace_dialog_new_folder_title",
    "workspace_dialog_rename_title",
    "workspace_delete_confirm_title",
    "workspace_delete_confirm_message",
    "workspace_operation_failed_title",
    "workspace_default_folder_name",
    "workspace_default_file_name",
    "open_link_title",
    "open_link_open",
    "open_link_cancel",
    "view_mode_source",
    "view_mode_switch_to_source",
    "view_mode_rendered",
    "view_mode_switch_to_rendered",
    "format_toolbar_bold",
    "format_toolbar_italic",
    "format_toolbar_heading",
    "format_toolbar_ordered_list",
    "format_toolbar_unordered_list",
    "format_toolbar_code",
    "format_toolbar_link",
    "format_toolbar_quote",
    "format_toolbar_todo",
    "format_toolbar_horizontal_rule",
    "format_toolbar_image",
    "format_toolbar_table_of_contents",
    "mermaid_template_flowchart",
    "mermaid_template_mind_map",
    "mermaid_template_sequence",
    "mermaid_template_gantt",
    "mermaid_template_state",
    "mermaid_template_class",
    "document_search_placeholder",
    "document_search_status",
    "document_search_no_matches",
    "document_search_status_empty",
    "context_menu_insert",
    "context_menu_add_to_ai_chat",
    "context_menu_table",
    "table_axis_align_column_left",
    "table_axis_align_column_center",
    "table_axis_align_column_right",
    "table_axis_move_column_left",
    "table_axis_move_column_right",
    "table_axis_delete_column",
    "table_axis_move_row_up",
    "table_axis_move_row_down",
    "table_axis_delete_row",
    "table_insert_title",
    "table_insert_description",
    "table_insert_body_rows",
    "table_insert_columns",
    "table_insert_cancel",
    "table_insert_confirm",
    "image_placeholder",
    "image_loading_without_alt",
    "image_loading_with_alt_template",
    "code_language_placeholder",
    "code_run_confirm_title",
    "code_run_confirm_message",
    "code_run_confirm_allow",
    "code_run_confirm_cancel",
    "code_run_unsaved_title",
    "code_run_unsaved_message",
    "code_run_unsaved_confirm",
    "code_run_unsaved_cancel",
    "code_run_disabled_title",
    "code_run_disabled_message",
    "code_run_unsupported_title",
    "code_run_unsupported_message",
    "code_run_output_title",
    "code_run_output_expand",
    "code_run_output_collapse",
    "code_run_stop",
    "code_run_close",
    "code_run_output_expand_lines_template",
    "code_run_meta_template",
    "code_run_exit_none",
    "inline_code_run_tooltip",
    "inline_code_run_output_title",
    "inline_code_run_opened_in_terminal",
    "preferences_allow_code_execution_label",
    "preferences_allow_code_execution_on",
    "preferences_allow_code_execution_off",
    "preferences_inline_code_system_terminal_label",
    "quick_file_open_placeholder",
];

impl I18nStringsDe {
    fn into_strings(self, defaults: I18nStrings) -> I18nStrings {
        I18nStrings {
            dirty_title_marker: self
                .dirty_title_marker
                .unwrap_or(defaults.dirty_title_marker),
            unsaved_changes_title: self
                .unsaved_changes_title
                .unwrap_or(defaults.unsaved_changes_title),
            unsaved_changes_message: self
                .unsaved_changes_message
                .unwrap_or(defaults.unsaved_changes_message),
            unsaved_changes_save_and_close: self
                .unsaved_changes_save_and_close
                .unwrap_or(defaults.unsaved_changes_save_and_close),
            unsaved_changes_discard_and_close: self
                .unsaved_changes_discard_and_close
                .unwrap_or(defaults.unsaved_changes_discard_and_close),
            unsaved_changes_cancel: self
                .unsaved_changes_cancel
                .unwrap_or(defaults.unsaved_changes_cancel),
            drop_replace_title: self
                .drop_replace_title
                .unwrap_or(defaults.drop_replace_title),
            drop_replace_message: self
                .drop_replace_message
                .unwrap_or(defaults.drop_replace_message),
            drop_replace_save_and_replace: self
                .drop_replace_save_and_replace
                .unwrap_or(defaults.drop_replace_save_and_replace),
            drop_replace_discard_and_replace: self
                .drop_replace_discard_and_replace
                .unwrap_or(defaults.drop_replace_discard_and_replace),
            drop_replace_cancel: self
                .drop_replace_cancel
                .unwrap_or(defaults.drop_replace_cancel),
            drop_no_markdown_file_message: self
                .drop_no_markdown_file_message
                .unwrap_or(defaults.drop_no_markdown_file_message),
            info_dialog_ok: self.info_dialog_ok.unwrap_or(defaults.info_dialog_ok),
            help_check_updates_title: self
                .help_check_updates_title
                .unwrap_or(defaults.help_check_updates_title),
            help_check_updates_message: self
                .help_check_updates_message
                .unwrap_or(defaults.help_check_updates_message),
            update_available_title: self
                .update_available_title
                .unwrap_or(defaults.update_available_title),
            update_available_message_template: self
                .update_available_message_template
                .unwrap_or(defaults.update_available_message_template),
            update_up_to_date_title: self
                .update_up_to_date_title
                .unwrap_or(defaults.update_up_to_date_title),
            update_up_to_date_message_template: self
                .update_up_to_date_message_template
                .unwrap_or(defaults.update_up_to_date_message_template),
            update_failed_title: self
                .update_failed_title
                .unwrap_or(defaults.update_failed_title),
            update_failed_message_template: self
                .update_failed_message_template
                .unwrap_or(defaults.update_failed_message_template),
            update_open_release: self
                .update_open_release
                .unwrap_or(defaults.update_open_release),
            update_later: self.update_later.unwrap_or(defaults.update_later),
            help_about_title: self.help_about_title.unwrap_or(defaults.help_about_title),
            help_about_message: self
                .help_about_message
                .unwrap_or(defaults.help_about_message),
            help_about_github_label: self
                .help_about_github_label
                .unwrap_or(defaults.help_about_github_label),
            help_about_star_message: self
                .help_about_star_message
                .unwrap_or(defaults.help_about_star_message),
            menu_file: self.menu_file.unwrap_or(defaults.menu_file),
            menu_export: self.menu_export.unwrap_or(defaults.menu_export),
            menu_language: self.menu_language.unwrap_or(defaults.menu_language),
            menu_theme: self.menu_theme.unwrap_or(defaults.menu_theme),
            menu_workspace: self.menu_workspace.unwrap_or(defaults.menu_workspace),
            menu_help: self.menu_help.unwrap_or(defaults.menu_help),
            menu_add_language_config: self
                .menu_add_language_config
                .unwrap_or(defaults.menu_add_language_config),
            menu_add_theme_config: self
                .menu_add_theme_config
                .unwrap_or(defaults.menu_add_theme_config),
            menu_new_window: self.menu_new_window.unwrap_or(defaults.menu_new_window),
            menu_close_window: self.menu_close_window.unwrap_or(defaults.menu_close_window),
            menu_open_file: self.menu_open_file.unwrap_or(defaults.menu_open_file),
            menu_open_folder: self
                .menu_open_folder
                .unwrap_or(defaults.menu_open_folder),
            menu_open_recent_file: self
                .menu_open_recent_file
                .unwrap_or(defaults.menu_open_recent_file),
            menu_preferences: self.menu_preferences.unwrap_or(defaults.menu_preferences),
            menu_no_recent_files: self
                .menu_no_recent_files
                .unwrap_or(defaults.menu_no_recent_files),
            menu_save: self.menu_save.unwrap_or(defaults.menu_save),
            menu_save_as: self.menu_save_as.unwrap_or(defaults.menu_save_as),
            menu_quit: self.menu_quit.unwrap_or(defaults.menu_quit),
            menu_export_html: self.menu_export_html.unwrap_or(defaults.menu_export_html),
            menu_export_pdf: self.menu_export_pdf.unwrap_or(defaults.menu_export_pdf),
            menu_check_updates: self
                .menu_check_updates
                .unwrap_or(defaults.menu_check_updates),
            menu_about: self.menu_about.unwrap_or(defaults.menu_about),
            menu_install_cli_tool: self
                .menu_install_cli_tool
                .unwrap_or(defaults.menu_install_cli_tool),
            menu_uninstall_cli_tool: self
                .menu_uninstall_cli_tool
                .unwrap_or(defaults.menu_uninstall_cli_tool),
            menu_toggle_workspace: self
                .menu_toggle_workspace
                .unwrap_or(defaults.menu_toggle_workspace),
            open_markdown_files_prompt: self
                .open_markdown_files_prompt
                .unwrap_or(defaults.open_markdown_files_prompt),
            open_folder_prompt: self
                .open_folder_prompt
                .unwrap_or(defaults.open_folder_prompt),
            add_language_config_prompt: self
                .add_language_config_prompt
                .unwrap_or(defaults.add_language_config_prompt),
            add_theme_config_prompt: self
                .add_theme_config_prompt
                .unwrap_or(defaults.add_theme_config_prompt),
            open_failed_title: self.open_failed_title.unwrap_or(defaults.open_failed_title),
            recent_file_missing_title: self
                .recent_file_missing_title
                .unwrap_or(defaults.recent_file_missing_title),
            recent_file_missing_message_template: self
                .recent_file_missing_message_template
                .unwrap_or(defaults.recent_file_missing_message_template),
            save_failed_title: self.save_failed_title.unwrap_or(defaults.save_failed_title),
            export_failed_title: self
                .export_failed_title
                .unwrap_or(defaults.export_failed_title),
            export_overwrite_title: self
                .export_overwrite_title
                .unwrap_or(defaults.export_overwrite_title),
            export_overwrite_message: self
                .export_overwrite_message
                .unwrap_or(defaults.export_overwrite_message),
            export_overwrite_confirm: self
                .export_overwrite_confirm
                .unwrap_or(defaults.export_overwrite_confirm),
            config_import_failed_title: self
                .config_import_failed_title
                .unwrap_or(defaults.config_import_failed_title),
            preferences_window_title: self
                .preferences_window_title
                .unwrap_or(defaults.preferences_window_title),
            preferences_nav_file: self
                .preferences_nav_file
                .unwrap_or(defaults.preferences_nav_file),
            preferences_nav_theme: self
                .preferences_nav_theme
                .unwrap_or(defaults.preferences_nav_theme),
            preferences_nav_shortcuts: self
                .preferences_nav_shortcuts
                .unwrap_or(defaults.preferences_nav_shortcuts),
            preferences_startup_option: self
                .preferences_startup_option
                .unwrap_or(defaults.preferences_startup_option),
            preferences_startup_new_file: self
                .preferences_startup_new_file
                .unwrap_or(defaults.preferences_startup_new_file),
            preferences_startup_last_opened_file: self
                .preferences_startup_last_opened_file
                .unwrap_or(defaults.preferences_startup_last_opened_file),
            preferences_local_theme: self
                .preferences_local_theme
                .unwrap_or(defaults.preferences_local_theme),
            preferences_save: self.preferences_save.unwrap_or(defaults.preferences_save),
            preferences_cancel: self
                .preferences_cancel
                .unwrap_or(defaults.preferences_cancel),
            preferences_save_failed_title: self
                .preferences_save_failed_title
                .unwrap_or(defaults.preferences_save_failed_title),
            preferences_shortcuts_group_file: self
                .preferences_shortcuts_group_file
                .unwrap_or(defaults.preferences_shortcuts_group_file),
            preferences_shortcuts_group_edit: self
                .preferences_shortcuts_group_edit
                .unwrap_or(defaults.preferences_shortcuts_group_edit),
            preferences_shortcuts_group_navigation: self
                .preferences_shortcuts_group_navigation
                .unwrap_or(defaults.preferences_shortcuts_group_navigation),
            preferences_shortcuts_group_formatting: self
                .preferences_shortcuts_group_formatting
                .unwrap_or(defaults.preferences_shortcuts_group_formatting),
            preferences_shortcuts_group_block: self
                .preferences_shortcuts_group_block
                .unwrap_or(defaults.preferences_shortcuts_group_block),
            preferences_shortcuts_group_other: self
                .preferences_shortcuts_group_other
                .unwrap_or(defaults.preferences_shortcuts_group_other),
            preferences_shortcut_record: self
                .preferences_shortcut_record
                .unwrap_or(defaults.preferences_shortcut_record),
            preferences_shortcut_reset: self
                .preferences_shortcut_reset
                .unwrap_or(defaults.preferences_shortcut_reset),
            preferences_shortcut_recording: self
                .preferences_shortcut_recording
                .unwrap_or(defaults.preferences_shortcut_recording),
            preferences_shortcut_conflict_template: self
                .preferences_shortcut_conflict_template
                .unwrap_or(defaults.preferences_shortcut_conflict_template),
            preferences_shortcut_invalid_template: self
                .preferences_shortcut_invalid_template
                .unwrap_or(defaults.preferences_shortcut_invalid_template),
            preferences_shortcut_newline: self
                .preferences_shortcut_newline
                .unwrap_or(defaults.preferences_shortcut_newline),
            preferences_shortcut_delete_back: self
                .preferences_shortcut_delete_back
                .unwrap_or(defaults.preferences_shortcut_delete_back),
            preferences_shortcut_delete: self
                .preferences_shortcut_delete
                .unwrap_or(defaults.preferences_shortcut_delete),
            preferences_shortcut_word_delete_back: self
                .preferences_shortcut_word_delete_back
                .unwrap_or(defaults.preferences_shortcut_word_delete_back),
            preferences_shortcut_word_delete_forward: self
                .preferences_shortcut_word_delete_forward
                .unwrap_or(defaults.preferences_shortcut_word_delete_forward),
            preferences_shortcut_focus_prev: self
                .preferences_shortcut_focus_prev
                .unwrap_or(defaults.preferences_shortcut_focus_prev),
            preferences_shortcut_focus_next: self
                .preferences_shortcut_focus_next
                .unwrap_or(defaults.preferences_shortcut_focus_next),
            preferences_shortcut_move_left: self
                .preferences_shortcut_move_left
                .unwrap_or(defaults.preferences_shortcut_move_left),
            preferences_shortcut_move_right: self
                .preferences_shortcut_move_right
                .unwrap_or(defaults.preferences_shortcut_move_right),
            preferences_shortcut_word_move_left: self
                .preferences_shortcut_word_move_left
                .unwrap_or(defaults.preferences_shortcut_word_move_left),
            preferences_shortcut_word_move_right: self
                .preferences_shortcut_word_move_right
                .unwrap_or(defaults.preferences_shortcut_word_move_right),
            preferences_shortcut_home: self
                .preferences_shortcut_home
                .unwrap_or(defaults.preferences_shortcut_home),
            preferences_shortcut_end: self
                .preferences_shortcut_end
                .unwrap_or(defaults.preferences_shortcut_end),
            preferences_shortcut_block_up: self
                .preferences_shortcut_block_up
                .unwrap_or(defaults.preferences_shortcut_block_up),
            preferences_shortcut_block_down: self
                .preferences_shortcut_block_down
                .unwrap_or(defaults.preferences_shortcut_block_down),
            preferences_shortcut_select_left: self
                .preferences_shortcut_select_left
                .unwrap_or(defaults.preferences_shortcut_select_left),
            preferences_shortcut_select_right: self
                .preferences_shortcut_select_right
                .unwrap_or(defaults.preferences_shortcut_select_right),
            preferences_shortcut_word_select_left: self
                .preferences_shortcut_word_select_left
                .unwrap_or(defaults.preferences_shortcut_word_select_left),
            preferences_shortcut_word_select_right: self
                .preferences_shortcut_word_select_right
                .unwrap_or(defaults.preferences_shortcut_word_select_right),
            preferences_shortcut_select_home: self
                .preferences_shortcut_select_home
                .unwrap_or(defaults.preferences_shortcut_select_home),
            preferences_shortcut_select_end: self
                .preferences_shortcut_select_end
                .unwrap_or(defaults.preferences_shortcut_select_end),
            preferences_shortcut_select_all: self
                .preferences_shortcut_select_all
                .unwrap_or(defaults.preferences_shortcut_select_all),
            preferences_shortcut_copy: self
                .preferences_shortcut_copy
                .unwrap_or(defaults.preferences_shortcut_copy),
            preferences_shortcut_cut: self
                .preferences_shortcut_cut
                .unwrap_or(defaults.preferences_shortcut_cut),
            preferences_shortcut_paste: self
                .preferences_shortcut_paste
                .unwrap_or(defaults.preferences_shortcut_paste),
            preferences_shortcut_undo: self
                .preferences_shortcut_undo
                .unwrap_or(defaults.preferences_shortcut_undo),
            preferences_shortcut_redo: self
                .preferences_shortcut_redo
                .unwrap_or(defaults.preferences_shortcut_redo),
            preferences_shortcut_bold_selection: self
                .preferences_shortcut_bold_selection
                .unwrap_or(defaults.preferences_shortcut_bold_selection),
            preferences_shortcut_italic_selection: self
                .preferences_shortcut_italic_selection
                .unwrap_or(defaults.preferences_shortcut_italic_selection),
            preferences_shortcut_underline_selection: self
                .preferences_shortcut_underline_selection
                .unwrap_or(defaults.preferences_shortcut_underline_selection),
            preferences_shortcut_code_selection: self
                .preferences_shortcut_code_selection
                .unwrap_or(defaults.preferences_shortcut_code_selection),
            preferences_shortcut_indent_block: self
                .preferences_shortcut_indent_block
                .unwrap_or(defaults.preferences_shortcut_indent_block),
            preferences_shortcut_outdent_block: self
                .preferences_shortcut_outdent_block
                .unwrap_or(defaults.preferences_shortcut_outdent_block),
            preferences_shortcut_exit_code_block: self
                .preferences_shortcut_exit_code_block
                .unwrap_or(defaults.preferences_shortcut_exit_code_block),
            preferences_shortcut_save_document: self
                .preferences_shortcut_save_document
                .unwrap_or(defaults.preferences_shortcut_save_document),
            preferences_shortcut_save_document_as: self
                .preferences_shortcut_save_document_as
                .unwrap_or(defaults.preferences_shortcut_save_document_as),
            preferences_shortcut_new_window: self
                .preferences_shortcut_new_window
                .unwrap_or(defaults.preferences_shortcut_new_window),
            preferences_shortcut_open_file: self
                .preferences_shortcut_open_file
                .unwrap_or(defaults.preferences_shortcut_open_file),
            preferences_shortcut_quit_application: self
                .preferences_shortcut_quit_application
                .unwrap_or(defaults.preferences_shortcut_quit_application),
            preferences_shortcut_close_window: self
                .preferences_shortcut_close_window
                .unwrap_or(defaults.preferences_shortcut_close_window),
            preferences_shortcut_dismiss_transient_ui: self
                .preferences_shortcut_dismiss_transient_ui
                .unwrap_or(defaults.preferences_shortcut_dismiss_transient_ui),
            preferences_shortcut_toggle_view_mode: self
                .preferences_shortcut_toggle_view_mode
                .unwrap_or(defaults.preferences_shortcut_toggle_view_mode),
            preferences_shortcut_toggle_workspace: self
                .preferences_shortcut_toggle_workspace
                .unwrap_or(defaults.preferences_shortcut_toggle_workspace),
            preferences_shortcut_find_next_in_document: self
                .preferences_shortcut_find_next_in_document
                .unwrap_or(defaults.preferences_shortcut_find_next_in_document),
            preferences_shortcut_find_previous_in_document: self
                .preferences_shortcut_find_previous_in_document
                .unwrap_or(defaults.preferences_shortcut_find_previous_in_document),
            preferences_shortcut_quick_file_open: self
                .preferences_shortcut_quick_file_open
                .unwrap_or(defaults.preferences_shortcut_quick_file_open),
            preferences_shortcut_open_workspace_search: self
                .preferences_shortcut_open_workspace_search
                .unwrap_or(defaults.preferences_shortcut_open_workspace_search),
            workspace_tab_files: self
                .workspace_tab_files
                .unwrap_or(defaults.workspace_tab_files),
            workspace_tab_outline: self
                .workspace_tab_outline
                .unwrap_or(defaults.workspace_tab_outline),
            workspace_tab_tags: self
                .workspace_tab_tags
                .unwrap_or(defaults.workspace_tab_tags),
            workspace_tab_graph: self
                .workspace_tab_graph
                .unwrap_or(defaults.workspace_tab_graph),
            workspace_tab_ai: self
                .workspace_tab_ai
                .unwrap_or(defaults.workspace_tab_ai),
            workspace_ai_new_chat: self
                .workspace_ai_new_chat
                .unwrap_or(defaults.workspace_ai_new_chat),
            workspace_ai_settings: self
                .workspace_ai_settings
                .unwrap_or(defaults.workspace_ai_settings),
            workspace_ai_send: self
                .workspace_ai_send
                .unwrap_or(defaults.workspace_ai_send),
            workspace_ai_empty: self
                .workspace_ai_empty
                .unwrap_or(defaults.workspace_ai_empty),
            workspace_ai_empty_no_api: self
                .workspace_ai_empty_no_api
                .unwrap_or(defaults.workspace_ai_empty_no_api),
            workspace_ai_empty_error: self
                .workspace_ai_empty_error
                .unwrap_or(defaults.workspace_ai_empty_error),
            workspace_ai_input_placeholder: self
                .workspace_ai_input_placeholder
                .unwrap_or(defaults.workspace_ai_input_placeholder),
            workspace_ai_context_selection: self
                .workspace_ai_context_selection
                .unwrap_or(defaults.workspace_ai_context_selection),
            workspace_ai_context_full: self
                .workspace_ai_context_full
                .unwrap_or(defaults.workspace_ai_context_full),
            workspace_ai_context_blank: self
                .workspace_ai_context_blank
                .unwrap_or(defaults.workspace_ai_context_blank),
            workspace_ai_context_workspace: self
                .workspace_ai_context_workspace
                .unwrap_or(defaults.workspace_ai_context_workspace),
            workspace_ai_context_command: self
                .workspace_ai_context_command
                .unwrap_or(defaults.workspace_ai_context_command),
            workspace_ai_error_no_api: self
                .workspace_ai_error_no_api
                .unwrap_or(defaults.workspace_ai_error_no_api),
            workspace_ai_copy: self
                .workspace_ai_copy
                .unwrap_or(defaults.workspace_ai_copy),
            workspace_ai_insert: self
                .workspace_ai_insert
                .unwrap_or(defaults.workspace_ai_insert),
            workspace_ai_untitled_document: self
                .workspace_ai_untitled_document
                .unwrap_or(defaults.workspace_ai_untitled_document),
            workspace_empty_tags: self
                .workspace_empty_tags
                .unwrap_or(defaults.workspace_empty_tags),
            workspace_graph_building: self
                .workspace_graph_building
                .unwrap_or(defaults.workspace_graph_building),
            workspace_graph_empty: self
                .workspace_graph_empty
                .unwrap_or(defaults.workspace_graph_empty),
            workspace_graph_fit_view: self
                .workspace_graph_fit_view
                .unwrap_or(defaults.workspace_graph_fit_view),
            workspace_graph_reset_layout: self
                .workspace_graph_reset_layout
                .unwrap_or(defaults.workspace_graph_reset_layout),
            workspace_graph_popout: self
                .workspace_graph_popout
                .unwrap_or(defaults.workspace_graph_popout),
            workspace_graph_window_title: self
                .workspace_graph_window_title
                .unwrap_or(defaults.workspace_graph_window_title),
            workspace_graph_mutual_repulsion: self
                .workspace_graph_mutual_repulsion
                .unwrap_or(defaults.workspace_graph_mutual_repulsion),
            workspace_graph_physics_collisions: self
                .workspace_graph_physics_collisions
                .unwrap_or(defaults.workspace_graph_physics_collisions),
            workspace_graph_uncross_crossings: self
                .workspace_graph_uncross_crossings
                .unwrap_or(defaults.workspace_graph_uncross_crossings),
            workspace_graph_filter_connected: self
                .workspace_graph_filter_connected
                .unwrap_or(defaults.workspace_graph_filter_connected),
            workspace_graph_filter_all: self
                .workspace_graph_filter_all
                .unwrap_or(defaults.workspace_graph_filter_all),
            workspace_tag_sort_by_name: self
                .workspace_tag_sort_by_name
                .unwrap_or(defaults.workspace_tag_sort_by_name),
            workspace_tag_sort_by_count: self
                .workspace_tag_sort_by_count
                .unwrap_or(defaults.workspace_tag_sort_by_count),
            workspace_tag_occurrences_title: self
                .workspace_tag_occurrences_title
                .unwrap_or(defaults.workspace_tag_occurrences_title),
            workspace_search_placeholder: self
                .workspace_search_placeholder
                .unwrap_or(defaults.workspace_search_placeholder),
            workspace_search_no_results: self
                .workspace_search_no_results
                .unwrap_or(defaults.workspace_search_no_results),
            workspace_search_no_root: self
                .workspace_search_no_root
                .unwrap_or(defaults.workspace_search_no_root),
            workspace_no_file_title: self
                .workspace_no_file_title
                .unwrap_or(defaults.workspace_no_file_title),
            workspace_no_file_message: self
                .workspace_no_file_message
                .unwrap_or(defaults.workspace_no_file_message),
            workspace_empty_files: self
                .workspace_empty_files
                .unwrap_or(defaults.workspace_empty_files),
            workspace_empty_outline: self
                .workspace_empty_outline
                .unwrap_or(defaults.workspace_empty_outline),
            workspace_scan_failed_title: self
                .workspace_scan_failed_title
                .unwrap_or(defaults.workspace_scan_failed_title),
            workspace_menu_new_file: self
                .workspace_menu_new_file
                .unwrap_or(defaults.workspace_menu_new_file),
            workspace_menu_new_folder: self
                .workspace_menu_new_folder
                .unwrap_or(defaults.workspace_menu_new_folder),
            workspace_menu_rename: self
                .workspace_menu_rename
                .unwrap_or(defaults.workspace_menu_rename),
            workspace_menu_delete: self
                .workspace_menu_delete
                .unwrap_or(defaults.workspace_menu_delete),
            workspace_menu_copy_path: self
                .workspace_menu_copy_path
                .unwrap_or(defaults.workspace_menu_copy_path),
            workspace_menu_reveal_in_file_manager: self
                .workspace_menu_reveal_in_file_manager
                .unwrap_or(defaults.workspace_menu_reveal_in_file_manager),
            workspace_menu_refresh: self
                .workspace_menu_refresh
                .unwrap_or(defaults.workspace_menu_refresh),
            workspace_dialog_new_file_title: self
                .workspace_dialog_new_file_title
                .unwrap_or(defaults.workspace_dialog_new_file_title),
            workspace_dialog_new_folder_title: self
                .workspace_dialog_new_folder_title
                .unwrap_or(defaults.workspace_dialog_new_folder_title),
            workspace_dialog_rename_title: self
                .workspace_dialog_rename_title
                .unwrap_or(defaults.workspace_dialog_rename_title),
            workspace_delete_confirm_title: self
                .workspace_delete_confirm_title
                .unwrap_or(defaults.workspace_delete_confirm_title),
            workspace_delete_confirm_message: self
                .workspace_delete_confirm_message
                .unwrap_or(defaults.workspace_delete_confirm_message),
            workspace_operation_failed_title: self
                .workspace_operation_failed_title
                .unwrap_or(defaults.workspace_operation_failed_title),
            workspace_default_folder_name: self
                .workspace_default_folder_name
                .unwrap_or(defaults.workspace_default_folder_name),
            workspace_default_file_name: self
                .workspace_default_file_name
                .unwrap_or(defaults.workspace_default_file_name),
            open_link_title: self.open_link_title.unwrap_or(defaults.open_link_title),
            open_link_open: self.open_link_open.unwrap_or(defaults.open_link_open),
            open_link_cancel: self.open_link_cancel.unwrap_or(defaults.open_link_cancel),
            view_mode_source: self.view_mode_source.unwrap_or(defaults.view_mode_source),
            view_mode_switch_to_source: self
                .view_mode_switch_to_source
                .unwrap_or(defaults.view_mode_switch_to_source),
            view_mode_rendered: self
                .view_mode_rendered
                .unwrap_or(defaults.view_mode_rendered),
            view_mode_switch_to_rendered: self
                .view_mode_switch_to_rendered
                .unwrap_or(defaults.view_mode_switch_to_rendered),
            format_toolbar_bold: self
                .format_toolbar_bold
                .unwrap_or(defaults.format_toolbar_bold),
            format_toolbar_italic: self
                .format_toolbar_italic
                .unwrap_or(defaults.format_toolbar_italic),
            format_toolbar_heading: self
                .format_toolbar_heading
                .unwrap_or(defaults.format_toolbar_heading),
            format_toolbar_ordered_list: self
                .format_toolbar_ordered_list
                .unwrap_or(defaults.format_toolbar_ordered_list),
            format_toolbar_unordered_list: self
                .format_toolbar_unordered_list
                .unwrap_or(defaults.format_toolbar_unordered_list),
            format_toolbar_code: self
                .format_toolbar_code
                .unwrap_or(defaults.format_toolbar_code),
            format_toolbar_link: self
                .format_toolbar_link
                .unwrap_or(defaults.format_toolbar_link),
            format_toolbar_quote: self
                .format_toolbar_quote
                .unwrap_or(defaults.format_toolbar_quote),
            format_toolbar_todo: self
                .format_toolbar_todo
                .unwrap_or(defaults.format_toolbar_todo),
            format_toolbar_horizontal_rule: self
                .format_toolbar_horizontal_rule
                .unwrap_or(defaults.format_toolbar_horizontal_rule),
            format_toolbar_image: self
                .format_toolbar_image
                .unwrap_or(defaults.format_toolbar_image),
            format_toolbar_table_of_contents: self
                .format_toolbar_table_of_contents
                .unwrap_or(defaults.format_toolbar_table_of_contents),
            mermaid_template_flowchart: self
                .mermaid_template_flowchart
                .unwrap_or(defaults.mermaid_template_flowchart),
            mermaid_template_mind_map: self
                .mermaid_template_mind_map
                .unwrap_or(defaults.mermaid_template_mind_map),
            mermaid_template_sequence: self
                .mermaid_template_sequence
                .unwrap_or(defaults.mermaid_template_sequence),
            mermaid_template_gantt: self
                .mermaid_template_gantt
                .unwrap_or(defaults.mermaid_template_gantt),
            mermaid_template_state: self
                .mermaid_template_state
                .unwrap_or(defaults.mermaid_template_state),
            mermaid_template_class: self
                .mermaid_template_class
                .unwrap_or(defaults.mermaid_template_class),
            document_search_placeholder: self
                .document_search_placeholder
                .unwrap_or(defaults.document_search_placeholder),
            document_search_status: self
                .document_search_status
                .unwrap_or(defaults.document_search_status),
            document_search_no_matches: self
                .document_search_no_matches
                .unwrap_or(defaults.document_search_no_matches),
            document_search_status_empty: self
                .document_search_status_empty
                .unwrap_or(defaults.document_search_status_empty),
            context_menu_insert: self
                .context_menu_insert
                .unwrap_or(defaults.context_menu_insert),
            context_menu_add_to_ai_chat: self
                .context_menu_add_to_ai_chat
                .unwrap_or(defaults.context_menu_add_to_ai_chat),
            context_menu_table: self
                .context_menu_table
                .unwrap_or(defaults.context_menu_table),
            table_axis_align_column_left: self
                .table_axis_align_column_left
                .unwrap_or(defaults.table_axis_align_column_left),
            table_axis_align_column_center: self
                .table_axis_align_column_center
                .unwrap_or(defaults.table_axis_align_column_center),
            table_axis_align_column_right: self
                .table_axis_align_column_right
                .unwrap_or(defaults.table_axis_align_column_right),
            table_axis_move_column_left: self
                .table_axis_move_column_left
                .unwrap_or(defaults.table_axis_move_column_left),
            table_axis_move_column_right: self
                .table_axis_move_column_right
                .unwrap_or(defaults.table_axis_move_column_right),
            table_axis_delete_column: self
                .table_axis_delete_column
                .unwrap_or(defaults.table_axis_delete_column),
            table_axis_move_row_up: self
                .table_axis_move_row_up
                .unwrap_or(defaults.table_axis_move_row_up),
            table_axis_move_row_down: self
                .table_axis_move_row_down
                .unwrap_or(defaults.table_axis_move_row_down),
            table_axis_delete_row: self
                .table_axis_delete_row
                .unwrap_or(defaults.table_axis_delete_row),
            table_insert_title: self
                .table_insert_title
                .unwrap_or(defaults.table_insert_title),
            table_insert_description: self
                .table_insert_description
                .unwrap_or(defaults.table_insert_description),
            table_insert_body_rows: self
                .table_insert_body_rows
                .unwrap_or(defaults.table_insert_body_rows),
            table_insert_columns: self
                .table_insert_columns
                .unwrap_or(defaults.table_insert_columns),
            table_insert_cancel: self
                .table_insert_cancel
                .unwrap_or(defaults.table_insert_cancel),
            table_insert_confirm: self
                .table_insert_confirm
                .unwrap_or(defaults.table_insert_confirm),
            image_placeholder: self.image_placeholder.unwrap_or(defaults.image_placeholder),
            image_loading_without_alt: self
                .image_loading_without_alt
                .unwrap_or(defaults.image_loading_without_alt),
            image_loading_with_alt_template: self
                .image_loading_with_alt_template
                .unwrap_or(defaults.image_loading_with_alt_template),
            code_language_placeholder: self
                .code_language_placeholder
                .unwrap_or(defaults.code_language_placeholder),
            code_run_confirm_title: self
                .code_run_confirm_title
                .unwrap_or(defaults.code_run_confirm_title),
            code_run_confirm_message: self
                .code_run_confirm_message
                .unwrap_or(defaults.code_run_confirm_message),
            code_run_confirm_allow: self
                .code_run_confirm_allow
                .unwrap_or(defaults.code_run_confirm_allow),
            code_run_confirm_cancel: self
                .code_run_confirm_cancel
                .unwrap_or(defaults.code_run_confirm_cancel),
            code_run_unsaved_title: self
                .code_run_unsaved_title
                .unwrap_or(defaults.code_run_unsaved_title),
            code_run_unsaved_message: self
                .code_run_unsaved_message
                .unwrap_or(defaults.code_run_unsaved_message),
            code_run_unsaved_confirm: self
                .code_run_unsaved_confirm
                .unwrap_or(defaults.code_run_unsaved_confirm),
            code_run_unsaved_cancel: self
                .code_run_unsaved_cancel
                .unwrap_or(defaults.code_run_unsaved_cancel),
            code_run_disabled_title: self
                .code_run_disabled_title
                .unwrap_or(defaults.code_run_disabled_title),
            code_run_disabled_message: self
                .code_run_disabled_message
                .unwrap_or(defaults.code_run_disabled_message),
            code_run_unsupported_title: self
                .code_run_unsupported_title
                .unwrap_or(defaults.code_run_unsupported_title),
            code_run_unsupported_message: self
                .code_run_unsupported_message
                .unwrap_or(defaults.code_run_unsupported_message),
            code_run_output_title: self
                .code_run_output_title
                .unwrap_or(defaults.code_run_output_title),
            code_run_output_expand: self
                .code_run_output_expand
                .unwrap_or(defaults.code_run_output_expand),
            code_run_output_collapse: self
                .code_run_output_collapse
                .unwrap_or(defaults.code_run_output_collapse),
            code_run_stop: self.code_run_stop.unwrap_or(defaults.code_run_stop),
            code_run_close: self.code_run_close.unwrap_or(defaults.code_run_close),
            code_run_output_expand_lines_template: self
                .code_run_output_expand_lines_template
                .unwrap_or(defaults.code_run_output_expand_lines_template),
            code_run_meta_template: self
                .code_run_meta_template
                .unwrap_or(defaults.code_run_meta_template),
            code_run_exit_none: self
                .code_run_exit_none
                .unwrap_or(defaults.code_run_exit_none),
            inline_code_run_tooltip: self
                .inline_code_run_tooltip
                .unwrap_or(defaults.inline_code_run_tooltip),
            inline_code_run_output_title: self
                .inline_code_run_output_title
                .unwrap_or(defaults.inline_code_run_output_title),
            inline_code_run_opened_in_terminal: self
                .inline_code_run_opened_in_terminal
                .unwrap_or(defaults.inline_code_run_opened_in_terminal),
            preferences_allow_code_execution_label: self
                .preferences_allow_code_execution_label
                .unwrap_or(defaults.preferences_allow_code_execution_label),
            preferences_allow_code_execution_on: self
                .preferences_allow_code_execution_on
                .unwrap_or(defaults.preferences_allow_code_execution_on),
            preferences_allow_code_execution_off: self
                .preferences_allow_code_execution_off
                .unwrap_or(defaults.preferences_allow_code_execution_off),
            preferences_inline_code_system_terminal_label: self
                .preferences_inline_code_system_terminal_label
                .unwrap_or(defaults.preferences_inline_code_system_terminal_label),
            quick_file_open_placeholder: self
                .quick_file_open_placeholder
                .unwrap_or(defaults.quick_file_open_placeholder),
        }
    }
}

impl<'de> Deserialize<'de> for I18nStrings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = I18nStringsDe::deserialize(deserializer)?;
        Ok(raw.into_strings(I18nStrings::en_us()))
    }
}

impl I18nStrings {
    /// Built-in Simplified Chinese UI strings.
    pub fn zh_cn() -> Self {
        Self {
            dirty_title_marker: "\u{00B7}".into(),
            unsaved_changes_title: "不保存并关闭？".into(),
            unsaved_changes_message: "此文档有未保存的更改。关闭前保存可避免丢失最新编辑。".into(),
            unsaved_changes_save_and_close: "保存并关闭".into(),
            unsaved_changes_discard_and_close: "放弃并关闭".into(),
            unsaved_changes_cancel: "继续编辑".into(),
            drop_replace_title: "替换当前文档？".into(),
            drop_replace_message: "当前文档有未保存的更改。替换前保存可避免丢失最新编辑。".into(),
            drop_replace_save_and_replace: "保存并替换".into(),
            drop_replace_discard_and_replace: "直接替换".into(),
            drop_replace_cancel: "取消".into(),
            drop_no_markdown_file_message:
                "请拖入 Markdown 文件（.md 或 .markdown）以在当前窗口打开。".into(),
            info_dialog_ok: "确定".into(),
            help_check_updates_title: "检查更新".into(),
            help_check_updates_message: "正在检查 Markman 的最新版本...".into(),
            update_available_title: "发现新版本".into(),
            update_available_message_template:
                "当前版本：{current}\n最新版本：{latest}\n是否前往 GitHub Releases 下载？".into(),
            update_up_to_date_title: "已是最新版本".into(),
            update_up_to_date_message_template: "当前版本：{current}\n远程版本：{latest}".into(),
            update_failed_title: "检查更新失败".into(),
            update_failed_message_template: "无法完成在线更新检查：{error}".into(),
            update_open_release: "前往下载".into(),
            update_later: "稍后".into(),
            help_about_title: "关于 Markman".into(),
            help_about_message: "作者：manyougz".into(),
            help_about_github_label: "GitHub".into(),
            help_about_star_message: "如果本项目对您有帮助，那不妨给本项目一颗 Star⭐，十分感谢！"
                .into(),
            menu_file: "文件".into(),
            menu_export: "导出".into(),
            menu_language: "语言".into(),
            menu_theme: "主题".into(),
            menu_workspace: "工作区".into(),
            menu_help: "帮助".into(),
            menu_add_language_config: "添加语言配置".into(),
            menu_add_theme_config: "添加主题配置".into(),
            menu_new_window: "新建窗口".into(),
            menu_close_window: "关闭窗口".into(),
            menu_open_file: "打开文件".into(),
            menu_open_folder: "打开文件夹".into(),
            menu_open_recent_file: "打开最近文件".into(),
            menu_preferences: "偏好设置".into(),
            menu_no_recent_files: "无最近文件".into(),
            menu_save: "保存".into(),
            menu_save_as: "另存为".into(),
            menu_quit: "退出".into(),
            menu_export_html: "HTML".into(),
            menu_export_pdf: "PDF".into(),
            menu_check_updates: "检查更新".into(),
            menu_about: "关于 Markman".into(),
            menu_install_cli_tool: "安装CLI命令".into(),
            menu_uninstall_cli_tool: "卸载CLI命令".into(),
            menu_toggle_workspace: "切换工作区".into(),
            open_markdown_files_prompt: "打开 Markdown 文件".into(),
            open_folder_prompt: "选择文件夹".into(),
            add_language_config_prompt: "选择语言配置文件".into(),
            add_theme_config_prompt: "选择主题配置文件".into(),
            open_failed_title: "打开失败".into(),
            recent_file_missing_title: "最近文件不存在".into(),
            recent_file_missing_message_template: "此最近文件已经不存在，已从记录中移除：\n{path}"
                .into(),
            save_failed_title: "保存失败".into(),
            export_failed_title: "导出失败".into(),
            export_overwrite_title: "文件已存在".into(),
            export_overwrite_message: "「{path}」已存在，是否要覆盖？".into(),
            export_overwrite_confirm: "覆盖".into(),
            config_import_failed_title: "配置导入失败".into(),
            preferences_window_title: "偏好设置".into(),
            preferences_nav_file: "文件".into(),
            preferences_nav_theme: "主题".into(),
            preferences_nav_shortcuts: "快捷键".into(),
            preferences_startup_option: "启动选项".into(),
            preferences_startup_new_file: "新 md 文件".into(),
            preferences_startup_last_opened_file: "上一次打开的 md 文件".into(),
            preferences_local_theme: "本地主题".into(),
            preferences_save: "保存".into(),
            preferences_cancel: "取消".into(),
            preferences_save_failed_title: "保存偏好设置失败".into(),
            preferences_shortcuts_group_file: "文件".into(),
            preferences_shortcuts_group_edit: "编辑".into(),
            preferences_shortcuts_group_navigation: "移动与选择".into(),
            preferences_shortcuts_group_formatting: "格式化".into(),
            preferences_shortcuts_group_block: "块操作".into(),
            preferences_shortcuts_group_other: "其他".into(),
            preferences_shortcut_record: "录制".into(),
            preferences_shortcut_reset: "重置".into(),
            preferences_shortcut_recording: "按下快捷键...".into(),
            preferences_shortcut_conflict_template: "该快捷键已被“{command}”使用".into(),
            preferences_shortcut_invalid_template: "无法使用快捷键“{shortcut}”".into(),
            preferences_shortcut_newline: "换行".into(),
            preferences_shortcut_delete_back: "向前删除".into(),
            preferences_shortcut_delete: "向后删除".into(),
            preferences_shortcut_word_delete_back: "向前删除单词".into(),
            preferences_shortcut_word_delete_forward: "向后删除单词".into(),
            preferences_shortcut_focus_prev: "上移".into(),
            preferences_shortcut_focus_next: "下移".into(),
            preferences_shortcut_move_left: "光标左移".into(),
            preferences_shortcut_move_right: "光标右移".into(),
            preferences_shortcut_word_move_left: "按词左移".into(),
            preferences_shortcut_word_move_right: "按词右移".into(),
            preferences_shortcut_home: "行首".into(),
            preferences_shortcut_end: "行尾".into(),
            preferences_shortcut_block_up: "上一块开头".into(),
            preferences_shortcut_block_down: "下一块开头".into(),
            preferences_shortcut_select_left: "向左选择".into(),
            preferences_shortcut_select_right: "向右选择".into(),
            preferences_shortcut_word_select_left: "向左选择单词".into(),
            preferences_shortcut_word_select_right: "向右选择单词".into(),
            preferences_shortcut_select_home: "选择到行首".into(),
            preferences_shortcut_select_end: "选择到行尾".into(),
            preferences_shortcut_select_all: "全选".into(),
            preferences_shortcut_copy: "复制".into(),
            preferences_shortcut_cut: "剪切".into(),
            preferences_shortcut_paste: "粘贴".into(),
            preferences_shortcut_undo: "撤销".into(),
            preferences_shortcut_redo: "重做".into(),
            preferences_shortcut_bold_selection: "加粗".into(),
            preferences_shortcut_italic_selection: "斜体".into(),
            preferences_shortcut_underline_selection: "下划线".into(),
            preferences_shortcut_code_selection: "行内代码".into(),
            preferences_shortcut_indent_block: "缩进块".into(),
            preferences_shortcut_outdent_block: "取消缩进块".into(),
            preferences_shortcut_exit_code_block: "退出代码块".into(),
            preferences_shortcut_save_document: "保存文档".into(),
            preferences_shortcut_save_document_as: "另存为".into(),
            preferences_shortcut_new_window: "新建窗口".into(),
            preferences_shortcut_open_file: "打开文件".into(),
            preferences_shortcut_quit_application: "退出应用".into(),
            preferences_shortcut_close_window: "关闭窗口".into(),
            preferences_shortcut_dismiss_transient_ui: "关闭临时界面".into(),
            preferences_shortcut_toggle_view_mode: "切换视图模式".into(),
            preferences_shortcut_toggle_workspace: "切换工作区".into(),
            preferences_shortcut_find_next_in_document: "查找下一个".into(),
            preferences_shortcut_find_previous_in_document: "查找上一个".into(),
            preferences_shortcut_quick_file_open: "快速打开文件".into(),
            preferences_shortcut_open_workspace_search: "打开全局搜索".into(),
            workspace_tab_files: "文件".into(),
            workspace_tab_outline: "大纲".into(),
            workspace_tab_tags: "标签".into(),
            workspace_tab_graph: "图谱".into(),
            workspace_tab_ai: "AI".into(),
            workspace_ai_new_chat: "新建对话".into(),
            workspace_ai_settings: "AI 配置".into(),
            workspace_ai_send: "发送".into(),
            workspace_ai_empty: "输入问题开始对话".into(),
            workspace_ai_empty_no_api: "未检测到 AI 配置".into(),
            workspace_ai_empty_error: "请检查 AI 配置后输入问题".into(),
            workspace_ai_input_placeholder: "输入消息，Enter 发送".into(),
            workspace_ai_context_selection: "引用选中文本".into(),
            workspace_ai_context_full: "引用全文".into(),
            workspace_ai_context_blank: "全新对话".into(),
            workspace_ai_context_workspace: "引用工作区".into(),
            workspace_ai_context_command: "引用代码块".into(),
            workspace_ai_error_no_api: "打开 AI 配置".into(),
            workspace_ai_copy: "复制".into(),
            workspace_ai_insert: "插入".into(),
            workspace_ai_untitled_document: "未命名文档".into(),
            workspace_empty_tags: "工作区中没有行内标签".into(),
            workspace_graph_building: "正在构建知识图谱…".into(),
            workspace_graph_empty: "工作区中没有可显示的图谱节点".into(),
            workspace_graph_fit_view: "适应窗口".into(),
            workspace_graph_reset_layout: "重置布局".into(),
            workspace_graph_popout: "在新窗口中打开".into(),
            workspace_graph_window_title: "知识图谱".into(),
            workspace_graph_mutual_repulsion: "互相排斥".into(),
            workspace_graph_physics_collisions: "物理碰撞".into(),
            workspace_graph_uncross_crossings: "去除交叉".into(),
            workspace_graph_filter_connected: "已连接".into(),
            workspace_graph_filter_all: "全部".into(),
            workspace_tag_sort_by_name: "按名称排序".into(),
            workspace_tag_sort_by_count: "按引用数排序".into(),
            workspace_tag_occurrences_title: "{tag} 的引用".into(),
            workspace_search_placeholder: "搜索文件名或内容…".into(),
            workspace_search_no_results: "未找到匹配项".into(),
            workspace_search_no_root: "请先打开文件夹或 Markdown 文件".into(),
            workspace_no_file_title: "未打开 Markdown 文件".into(),
            workspace_no_file_message: "打开或保存一个 .md 文件后，工作区会使用该文件所在目录。"
                .into(),
            workspace_empty_files: "没有可显示的 Markdown 文件".into(),
            workspace_empty_outline: "当前文档没有标题".into(),
            workspace_scan_failed_title: "无法读取工作区".into(),
            workspace_menu_new_file: "新建 Markdown 文件".into(),
            workspace_menu_new_folder: "新建文件夹".into(),
            workspace_menu_rename: "重命名".into(),
            workspace_menu_delete: "删除".into(),
            workspace_menu_copy_path: "复制路径".into(),
            workspace_menu_reveal_in_file_manager: "在 Finder 中显示".into(),
            workspace_menu_refresh: "刷新".into(),
            workspace_dialog_new_file_title: "新建 Markdown 文件".into(),
            workspace_dialog_new_folder_title: "新建文件夹".into(),
            workspace_dialog_rename_title: "重命名".into(),
            workspace_delete_confirm_title: "确认删除".into(),
            workspace_delete_confirm_message: "确定要删除“{name}”吗？此操作无法撤销。".into(),
            workspace_operation_failed_title: "文件操作失败".into(),
            workspace_default_folder_name: "新建文件夹".into(),
            workspace_default_file_name: "未命名.md".into(),
            open_link_title: "打开链接？".into(),
            open_link_open: "打开".into(),
            open_link_cancel: "取消".into(),
            view_mode_source: "源码".into(),
            view_mode_switch_to_source: "切换到源码".into(),
            view_mode_rendered: "渲染".into(),
            view_mode_switch_to_rendered: "切换到渲染".into(),
            format_toolbar_bold: "B".into(),
            format_toolbar_italic: "I".into(),
            format_toolbar_heading: "H".into(),
            format_toolbar_ordered_list: "1.".into(),
            format_toolbar_unordered_list: "•".into(),
            format_toolbar_code: "`".into(),
            format_toolbar_link: "[]".into(),
            format_toolbar_quote: ">".into(),
            format_toolbar_todo: "待办".into(),
            format_toolbar_horizontal_rule: "分割线".into(),
            format_toolbar_image: "图片".into(),
            format_toolbar_table_of_contents: "目录".into(),
            mermaid_template_flowchart: "流程图".into(),
            mermaid_template_mind_map: "思维导图".into(),
            mermaid_template_sequence: "时序图".into(),
            mermaid_template_gantt: "甘特图".into(),
            mermaid_template_state: "状态图".into(),
            mermaid_template_class: "类图".into(),
            document_search_placeholder: "搜索本文档…".into(),
            document_search_status: "{current}/{total}".into(),
            document_search_no_matches: "无匹配".into(),
            document_search_status_empty: "输入关键词".into(),
            context_menu_insert: "插入".into(),
            context_menu_add_to_ai_chat: "加入对话".into(),
            context_menu_table: "表格".into(),
            table_axis_align_column_left: "左对齐此列".into(),
            table_axis_align_column_center: "居中此列".into(),
            table_axis_align_column_right: "右对齐此列".into(),
            table_axis_move_column_left: "向左移动此列".into(),
            table_axis_move_column_right: "向右移动此列".into(),
            table_axis_delete_column: "删除此列".into(),
            table_axis_move_row_up: "向上移动此行".into(),
            table_axis_move_row_down: "向下移动此行".into(),
            table_axis_delete_row: "删除此行".into(),
            table_insert_title: "插入表格".into(),
            table_insert_description: "创建 1 个表头行，并配置正文行数与列数。".into(),
            table_insert_body_rows: "正文行数".into(),
            table_insert_columns: "列数".into(),
            table_insert_cancel: "取消".into(),
            table_insert_confirm: "插入".into(),
            image_placeholder: "图片".into(),
            image_loading_without_alt: "正在加载图片...".into(),
            image_loading_with_alt_template: "正在加载 {alt}".into(),
            code_language_placeholder: "语言".into(),
            code_run_confirm_title: "允许运行代码？".into(),
            code_run_confirm_message:
                "Markman 会在本机执行代码块中的脚本。请仅在信任的来源上运行代码。".into(),
            code_run_confirm_allow: "允许运行".into(),
            code_run_confirm_cancel: "取消".into(),
            code_run_unsaved_title: "从未保存的文档运行代码？".into(),
            code_run_unsaved_message:
                "当前文档尚未保存到磁盘。运行结果基于编辑器中的内容，可能与已保存文件不一致。".into(),
            code_run_unsaved_confirm: "仍然运行".into(),
            code_run_unsaved_cancel: "取消".into(),
            code_run_disabled_title: "代码运行已禁用".into(),
            code_run_disabled_message: "可在「偏好设置 → 文件」中开启“允许运行代码”。".into(),
            code_run_unsupported_title: "无法运行此语言".into(),
            code_run_unsupported_message:
                "当前仅支持 bash、sh、python 和 javascript 代码块。".into(),
            code_run_output_title: "输出".into(),
            code_run_output_expand: "展开".into(),
            code_run_output_collapse: "收起".into(),
            code_run_stop: "停止".into(),
            code_run_close: "关闭".into(),
            code_run_output_expand_lines_template: "展开 {count} 行".into(),
            code_run_meta_template: "退出码：{exit} · 耗时：{duration} ms".into(),
            code_run_exit_none: "—".into(),
            inline_code_run_tooltip: "运行行内代码".into(),
            inline_code_run_output_title: "行内代码输出".into(),
            inline_code_run_opened_in_terminal: "已在系统终端中打开".into(),
            preferences_allow_code_execution_label: "允许运行代码".into(),
            preferences_allow_code_execution_on: "已开启".into(),
            preferences_allow_code_execution_off: "已关闭".into(),
            preferences_inline_code_system_terminal_label: "行内代码在系统终端中执行".into(),
            quick_file_open_placeholder: "搜索文件名…".into(),
        }
    }

    /// Built-in English UI strings.
    pub fn en_us() -> Self {
        Self {
            dirty_title_marker: "\u{00B7}".into(),
            unsaved_changes_title: "Close without saving?".into(),
            unsaved_changes_message:
                "This document has unsaved changes. Save before closing to avoid losing your latest edits."
                    .into(),
            unsaved_changes_save_and_close: "Save and Close".into(),
            unsaved_changes_discard_and_close: "Discard and Close".into(),
            unsaved_changes_cancel: "Keep Editing".into(),
            drop_replace_title: "Replace current document?".into(),
            drop_replace_message:
                "This document has unsaved changes. Save before replacing it with the dropped file to avoid losing edits."
                    .into(),
            drop_replace_save_and_replace: "Save and Replace".into(),
            drop_replace_discard_and_replace: "Replace Without Saving".into(),
            drop_replace_cancel: "Cancel".into(),
            drop_no_markdown_file_message:
                "Drop a Markdown file (.md or .markdown) to open it in this window.".into(),
            info_dialog_ok: "OK".into(),
            help_check_updates_title: "Check for Updates".into(),
            help_check_updates_message: "Checking the latest Markman version...".into(),
            update_available_title: "Update Available".into(),
            update_available_message_template:
                "Current version: {current}\nLatest version: {latest}\nOpen GitHub Releases to download it?"
                    .into(),
            update_up_to_date_title: "You're Up to Date".into(),
            update_up_to_date_message_template:
                "Current version: {current}\nRemote version: {latest}".into(),
            update_failed_title: "Update Check Failed".into(),
            update_failed_message_template: "Unable to complete the online update check: {error}"
                .into(),
            update_open_release: "Open Releases".into(),
            update_later: "Later".into(),
            help_about_title: "About Markman".into(),
            help_about_message: "Author: manyougz".into(),
            help_about_github_label: "GitHub".into(),
            help_about_star_message:
                "If this project helps you, consider giving it a Star⭐. Thank you!".into(),
            menu_file: "File".into(),
            menu_export: "Export".into(),
            menu_language: "Language".into(),
            menu_theme: "Theme".into(),
            menu_workspace: "Workspace".into(),
            menu_help: "Help".into(),
            menu_add_language_config: "Add Language Config".into(),
            menu_add_theme_config: "Add Theme Config".into(),
            menu_new_window: "New Window".into(),
            menu_close_window: "Close Window".into(),
            menu_open_file: "Open File".into(),
            menu_open_folder: "Open Folder".into(),
            menu_open_recent_file: "Open Recent File".into(),
            menu_preferences: "Preferences".into(),
            menu_no_recent_files: "No Recent Files".into(),
            menu_save: "Save".into(),
            menu_save_as: "Save As".into(),
            menu_quit: "Quit".into(),
            menu_export_html: "HTML".into(),
            menu_export_pdf: "PDF".into(),
            menu_check_updates: "Check for Updates".into(),
            menu_about: "About Markman".into(),
            menu_install_cli_tool: "Install CLI Command".into(),
            menu_uninstall_cli_tool: "Uninstall CLI Command".into(),
            menu_toggle_workspace: "Toggle Workspace".into(),
            open_markdown_files_prompt: "Open Markdown Files".into(),
            open_folder_prompt: "Choose a Folder".into(),
            add_language_config_prompt: "Choose Language Config".into(),
            add_theme_config_prompt: "Choose Theme Config".into(),
            open_failed_title: "Open Failed".into(),
            recent_file_missing_title: "Recent File Missing".into(),
            recent_file_missing_message_template:
                "This recent file no longer exists and has been removed:\n{path}".into(),
            save_failed_title: "Save Failed".into(),
            export_failed_title: "Export Failed".into(),
            export_overwrite_title: "File Already Exists".into(),
            export_overwrite_message: "\"{path}\" already exists. Replace it?".into(),
            export_overwrite_confirm: "Replace".into(),
            config_import_failed_title: "Config Import Failed".into(),
            preferences_window_title: "Preferences".into(),
            preferences_nav_file: "File".into(),
            preferences_nav_theme: "Theme".into(),
            preferences_nav_shortcuts: "Shortcuts".into(),
            preferences_startup_option: "Startup Option".into(),
            preferences_startup_new_file: "New Markdown File".into(),
            preferences_startup_last_opened_file: "Last Opened Markdown File".into(),
            preferences_local_theme: "Local Theme".into(),
            preferences_save: "Save".into(),
            preferences_cancel: "Cancel".into(),
            preferences_save_failed_title: "Save Preferences Failed".into(),
            preferences_shortcuts_group_file: "File".into(),
            preferences_shortcuts_group_edit: "Edit".into(),
            preferences_shortcuts_group_navigation: "Move and Select".into(),
            preferences_shortcuts_group_formatting: "Formatting".into(),
            preferences_shortcuts_group_block: "Block Operations".into(),
            preferences_shortcuts_group_other: "Other".into(),
            preferences_shortcut_record: "Record".into(),
            preferences_shortcut_reset: "Reset".into(),
            preferences_shortcut_recording: "Press shortcut...".into(),
            preferences_shortcut_conflict_template: "This shortcut is already used by {command}"
                .into(),
            preferences_shortcut_invalid_template: "Cannot use shortcut {shortcut}".into(),
            preferences_shortcut_newline: "Newline".into(),
            preferences_shortcut_delete_back: "Delete Backward".into(),
            preferences_shortcut_delete: "Delete Forward".into(),
            preferences_shortcut_word_delete_back: "Word Delete Backward".into(),
            preferences_shortcut_word_delete_forward: "Word Delete Forward".into(),
            preferences_shortcut_focus_prev: "Move Up".into(),
            preferences_shortcut_focus_next: "Move Down".into(),
            preferences_shortcut_move_left: "Move Left".into(),
            preferences_shortcut_move_right: "Move Right".into(),
            preferences_shortcut_word_move_left: "Word Move Left".into(),
            preferences_shortcut_word_move_right: "Word Move Right".into(),
            preferences_shortcut_home: "Line Start".into(),
            preferences_shortcut_end: "Line End".into(),
            preferences_shortcut_block_up: "Block Up".into(),
            preferences_shortcut_block_down: "Block Down".into(),
            preferences_shortcut_select_left: "Select Left".into(),
            preferences_shortcut_select_right: "Select Right".into(),
            preferences_shortcut_word_select_left: "Word Select Left".into(),
            preferences_shortcut_word_select_right: "Word Select Right".into(),
            preferences_shortcut_select_home: "Select to Line Start".into(),
            preferences_shortcut_select_end: "Select to Line End".into(),
            preferences_shortcut_select_all: "Select All".into(),
            preferences_shortcut_copy: "Copy".into(),
            preferences_shortcut_cut: "Cut".into(),
            preferences_shortcut_paste: "Paste".into(),
            preferences_shortcut_undo: "Undo".into(),
            preferences_shortcut_redo: "Redo".into(),
            preferences_shortcut_bold_selection: "Bold".into(),
            preferences_shortcut_italic_selection: "Italic".into(),
            preferences_shortcut_underline_selection: "Underline".into(),
            preferences_shortcut_code_selection: "Inline Code".into(),
            preferences_shortcut_indent_block: "Indent Block".into(),
            preferences_shortcut_outdent_block: "Outdent Block".into(),
            preferences_shortcut_exit_code_block: "Exit Code Block".into(),
            preferences_shortcut_save_document: "Save Document".into(),
            preferences_shortcut_save_document_as: "Save Document As".into(),
            preferences_shortcut_new_window: "New Window".into(),
            preferences_shortcut_open_file: "Open File".into(),
            preferences_shortcut_quit_application: "Quit Application".into(),
            preferences_shortcut_close_window: "Close Window".into(),
            preferences_shortcut_dismiss_transient_ui: "Dismiss Temporary UI".into(),
            preferences_shortcut_toggle_view_mode: "Toggle View Mode".into(),
            preferences_shortcut_toggle_workspace: "Toggle Workspace".into(),
            preferences_shortcut_find_next_in_document: "Find Next".into(),
            preferences_shortcut_find_previous_in_document: "Find Previous".into(),
            preferences_shortcut_quick_file_open: "Quick File Open".into(),
            preferences_shortcut_open_workspace_search: "Open Global Search".into(),
            workspace_tab_files: "Files".into(),
            workspace_tab_outline: "Outline".into(),
            workspace_tab_tags: "Tags".into(),
            workspace_tab_graph: "Graph".into(),
            workspace_tab_ai: "AI".into(),
            workspace_ai_new_chat: "New chat".into(),
            workspace_ai_settings: "AI settings".into(),
            workspace_ai_send: "Send".into(),
            workspace_ai_empty: "Ask a question to start".into(),
            workspace_ai_empty_no_api: "AI is not configured".into(),
            workspace_ai_empty_error: "Check AI settings and try again".into(),
            workspace_ai_input_placeholder: "Type a message, Enter to send".into(),
            workspace_ai_context_selection: "Selection".into(),
            workspace_ai_context_full: "Full document".into(),
            workspace_ai_context_blank: "Blank chat".into(),
            workspace_ai_context_workspace: "Workspace".into(),
            workspace_ai_context_command: "Code block".into(),
            workspace_ai_error_no_api: "Open AI settings".into(),
            workspace_ai_copy: "Copy".into(),
            workspace_ai_insert: "Insert".into(),
            workspace_ai_untitled_document: "Untitled".into(),
            workspace_empty_tags: "No inline tags in this workspace".into(),
            workspace_graph_building: "Building knowledge graph…".into(),
            workspace_graph_empty: "No graph nodes to display in this workspace".into(),
            workspace_graph_fit_view: "Fit view".into(),
            workspace_graph_reset_layout: "Reset layout".into(),
            workspace_graph_popout: "Open in new window".into(),
            workspace_graph_window_title: "Knowledge Graph".into(),
            workspace_graph_mutual_repulsion: "Repel".into(),
            workspace_graph_physics_collisions: "Physics".into(),
            workspace_graph_uncross_crossings: "Uncross edges".into(),
            workspace_graph_filter_connected: "Connected".into(),
            workspace_graph_filter_all: "All".into(),
            workspace_tag_sort_by_name: "Sort by name".into(),
            workspace_tag_sort_by_count: "Sort by count".into(),
            workspace_tag_occurrences_title: "References to {tag}".into(),
            workspace_search_placeholder: "Search file names or content…".into(),
            workspace_search_no_results: "No matches found".into(),
            workspace_search_no_root: "Open a folder or Markdown file first".into(),
            workspace_no_file_title: "No Markdown File Open".into(),
            workspace_no_file_message:
                "Open or save a .md file to use its folder as the workspace.".into(),
            workspace_empty_files: "No Markdown files to show".into(),
            workspace_empty_outline: "This document has no headings".into(),
            workspace_scan_failed_title: "Unable to Read Workspace".into(),
            workspace_menu_new_file: "New Markdown File".into(),
            workspace_menu_new_folder: "New Folder".into(),
            workspace_menu_rename: "Rename".into(),
            workspace_menu_delete: "Delete".into(),
            workspace_menu_copy_path: "Copy Path".into(),
            workspace_menu_reveal_in_file_manager: "Reveal in Finder".into(),
            workspace_menu_refresh: "Refresh".into(),
            workspace_dialog_new_file_title: "New Markdown File".into(),
            workspace_dialog_new_folder_title: "New Folder".into(),
            workspace_dialog_rename_title: "Rename".into(),
            workspace_delete_confirm_title: "Confirm Delete".into(),
            workspace_delete_confirm_message:
                "Delete “{name}”? This action cannot be undone.".into(),
            workspace_operation_failed_title: "File Operation Failed".into(),
            workspace_default_folder_name: "New Folder".into(),
            workspace_default_file_name: "Untitled.md".into(),
            open_link_title: "Open link?".into(),
            open_link_open: "Open".into(),
            open_link_cancel: "Cancel".into(),
            view_mode_source: "Source".into(),
            view_mode_switch_to_source: "Switch to Source".into(),
            view_mode_rendered: "Rendered".into(),
            view_mode_switch_to_rendered: "Switch to Rendered".into(),
            format_toolbar_bold: "B".into(),
            format_toolbar_italic: "I".into(),
            format_toolbar_heading: "H".into(),
            format_toolbar_ordered_list: "1.".into(),
            format_toolbar_unordered_list: "•".into(),
            format_toolbar_code: "`".into(),
            format_toolbar_link: "[]".into(),
            format_toolbar_quote: ">".into(),
            format_toolbar_todo: "Todo".into(),
            format_toolbar_horizontal_rule: "---".into(),
            format_toolbar_image: "Img".into(),
            format_toolbar_table_of_contents: "TOC".into(),
            mermaid_template_flowchart: "Flowchart".into(),
            mermaid_template_mind_map: "Mind Map".into(),
            mermaid_template_sequence: "Sequence".into(),
            mermaid_template_gantt: "Gantt".into(),
            mermaid_template_state: "State".into(),
            mermaid_template_class: "Class".into(),
            document_search_placeholder: "Search this document…".into(),
            document_search_status: "{current}/{total}".into(),
            document_search_no_matches: "No matches".into(),
            document_search_status_empty: "Type to search".into(),
            context_menu_insert: "Insert".into(),
            context_menu_add_to_ai_chat: "Add to Chat".into(),
            context_menu_table: "Table".into(),
            table_axis_align_column_left: "Align Column Left".into(),
            table_axis_align_column_center: "Align Column Center".into(),
            table_axis_align_column_right: "Align Column Right".into(),
            table_axis_move_column_left: "Move Column Left".into(),
            table_axis_move_column_right: "Move Column Right".into(),
            table_axis_delete_column: "Delete Column".into(),
            table_axis_move_row_up: "Move Row Up".into(),
            table_axis_move_row_down: "Move Row Down".into(),
            table_axis_delete_row: "Delete Row".into(),
            table_insert_title: "Insert Table".into(),
            table_insert_description:
                "Create one header row and configure body rows and columns.".into(),
            table_insert_body_rows: "Body Rows".into(),
            table_insert_columns: "Columns".into(),
            table_insert_cancel: "Cancel".into(),
            table_insert_confirm: "Insert".into(),
            image_placeholder: "Image".into(),
            image_loading_without_alt: "Loading image...".into(),
            image_loading_with_alt_template: "Loading {alt}".into(),
            code_language_placeholder: "language".into(),
            code_run_confirm_title: "Allow Code Execution?".into(),
            code_run_confirm_message:
                "Markman will run scripts from code blocks on this machine. Only run code you trust."
                    .into(),
            code_run_confirm_allow: "Allow".into(),
            code_run_confirm_cancel: "Cancel".into(),
            code_run_unsaved_title: "Run Code from Unsaved Document?".into(),
            code_run_unsaved_message:
                "This document has not been saved to disk. The run uses the current editor content, which may differ from any saved file."
                    .into(),
            code_run_unsaved_confirm: "Run Anyway".into(),
            code_run_unsaved_cancel: "Cancel".into(),
            code_run_disabled_title: "Code Execution Disabled".into(),
            code_run_disabled_message:
                "Enable “Allow code execution” in Preferences → File to run code blocks.".into(),
            code_run_unsupported_title: "Language Not Runnable".into(),
            code_run_unsupported_message:
                "Only bash, sh, python, and javascript code blocks can be run.".into(),
            code_run_output_title: "Output".into(),
            code_run_output_expand: "Expand".into(),
            code_run_output_collapse: "Collapse".into(),
            code_run_stop: "Stop".into(),
            code_run_close: "Close".into(),
            code_run_output_expand_lines_template: "Expand {count} lines".into(),
            code_run_meta_template: "Exit code: {exit} · Duration: {duration} ms".into(),
            code_run_exit_none: "—".into(),
            inline_code_run_tooltip: "Run inline code".into(),
            inline_code_run_output_title: "Inline code output".into(),
            inline_code_run_opened_in_terminal: "Opened in the system terminal".into(),
            preferences_allow_code_execution_label: "Allow code execution".into(),
            preferences_allow_code_execution_on: "Enabled".into(),
            preferences_allow_code_execution_off: "Disabled".into(),
            preferences_inline_code_system_terminal_label: "Run inline code in the system terminal".into(),
            quick_file_open_placeholder: "Search files by name…".into(),
        }
    }

    /// Returns a built-in string set for a supported language id.
    pub fn for_language_id(language_id: &str) -> Option<Self> {
        match language_id {
            "zh-CN" => Some(Self::zh_cn()),
            "en-US" => Some(Self::en_us()),
            _ => None,
        }
    }
}

/// Metadata for a selectable UI language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageCatalogEntry {
    pub id: String,
    pub name: String,
}

const BUILTIN_LANGUAGE_ZH_CN_ID: &str = "zh-CN";
const BUILTIN_LANGUAGE_ZH_CN_NAME: &str = "简体中文";
const BUILTIN_LANGUAGE_EN_US_ID: &str = "en-US";
const BUILTIN_LANGUAGE_EN_US_NAME: &str = "English";

fn builtin_language_catalog() -> Vec<LanguageCatalogEntry> {
    vec![
        LanguageCatalogEntry {
            id: BUILTIN_LANGUAGE_ZH_CN_ID.into(),
            name: BUILTIN_LANGUAGE_ZH_CN_NAME.into(),
        },
        LanguageCatalogEntry {
            id: BUILTIN_LANGUAGE_EN_US_ID.into(),
            name: BUILTIN_LANGUAGE_EN_US_NAME.into(),
        },
    ]
}

struct LanguageCatalog;

impl ConfigCatalog for LanguageCatalog {
    fn builtin_ids() -> &'static [&'static str] {
        &[BUILTIN_LANGUAGE_ZH_CN_ID, BUILTIN_LANGUAGE_EN_US_ID]
    }
}

/// A JSON language pack with metadata and fallback-completed strings.
#[derive(Debug, Clone, Serialize)]
pub struct I18nLanguagePack {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub strings: I18nStrings,
}

#[derive(Debug, Deserialize)]
struct I18nLanguagePackDe {
    id: String,
    name: Option<String>,
    author: Option<String>,
    description: Option<String>,
    version: Option<String>,
    homepage: Option<String>,
    license: Option<String>,
    #[serde(default)]
    strings: I18nStringsDe,
}

impl I18nLanguagePack {
    /// Parses a language pack from JSON text.
    #[cfg(test)]
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let mut value: Value = serde_json::from_str(json)?;
        prune_empty_json_values(&mut value);
        Self::from_value(value)
    }

    fn from_value(value: Value) -> anyhow::Result<Self> {
        let raw: I18nLanguagePackDe = serde_json::from_value(value)?;
        Ok(Self::from_partial(raw))
    }

    fn from_partial(raw: I18nLanguagePackDe) -> Self {
        let fallback = I18nStrings::for_language_id(&raw.id).unwrap_or_else(I18nStrings::en_us);
        let name = raw
            .name
            .unwrap_or_else(|| language_name_for_id(&raw.id).unwrap_or(&raw.id).to_string());
        Self {
            id: raw.id,
            name,
            author: raw.author,
            description: raw.description,
            version: raw.version,
            homepage: raw.homepage,
            license: raw.license,
            strings: raw.strings.into_strings(fallback),
        }
    }
}

fn language_name_for_id(language_id: &str) -> Option<&'static str> {
    match language_id {
        BUILTIN_LANGUAGE_ZH_CN_ID => Some(BUILTIN_LANGUAGE_ZH_CN_NAME),
        BUILTIN_LANGUAGE_EN_US_ID => Some(BUILTIN_LANGUAGE_EN_US_NAME),
        _ => None,
    }
}

fn is_builtin_language_id(language_id: &str) -> bool {
    catalog::is_builtin_id(language_id, LanguageCatalog::builtin_ids())
}

fn is_valid_custom_language_id(language_id: &str) -> bool {
    !language_id.trim().is_empty()
        && language_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        && language_id.chars().any(|ch| ch.is_ascii_alphabetic())
}

/// Selects a built-in language id from preferred system locales.
pub fn language_id_for_locale_preferences<I, S>(locales: I) -> &'static str
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    locales
        .into_iter()
        .find_map(|locale| language_id_for_locale(locale.as_ref()))
        .unwrap_or(BUILTIN_LANGUAGE_EN_US_ID)
}

fn language_id_for_locale(locale: &str) -> Option<&'static str> {
    let locale = locale.trim();
    if locale.is_empty() {
        return None;
    }

    let no_encoding = locale
        .split_once('.')
        .map_or(locale, |(locale, _encoding)| locale);
    let no_modifier = no_encoding
        .split_once('@')
        .map_or(no_encoding, |(locale, _modifier)| locale);
    let locale = no_modifier.replace('_', "-");
    let language = locale.split('-').next()?.to_ascii_lowercase();
    if !language.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }

    match language.as_str() {
        "zh" => Some(BUILTIN_LANGUAGE_ZH_CN_ID),
        "en" => Some(BUILTIN_LANGUAGE_EN_US_ID),
        _ => None,
    }
}

/// Global singleton that holds the current UI language strings.
pub struct I18nManager {
    current_language_id: String,
    strings: Arc<I18nStrings>,
    custom_languages: Vec<I18nLanguagePack>,
    language_catalog: Vec<LanguageCatalogEntry>,
}

impl Global for I18nManager {}

impl Default for I18nManager {
    fn default() -> Self {
        Self::new_with_language_id(BUILTIN_LANGUAGE_EN_US_ID)
    }
}

impl I18nManager {
    /// Installs a specific UI language into GPUI's global state.
    pub fn init_with_language_id(cx: &mut App, language_id: &str) {
        let mut manager = Self::new_with_language_id(BUILTIN_LANGUAGE_EN_US_ID);
        if let Ok(dirs) = MarkmanConfigDirs::from_system()
            && let Err(err) = manager.load_custom_languages_from_dirs(&dirs)
        {
            eprintln!("failed to load custom languages: {err}");
        }
        let _ = manager.set_language_by_id(language_id);
        cx.set_global(manager);
    }

    /// Creates a manager with a known language id, falling back to English.
    pub fn new_with_language_id(language_id: &str) -> Self {
        let current_language_id = if I18nStrings::for_language_id(language_id).is_some() {
            language_id
        } else {
            BUILTIN_LANGUAGE_EN_US_ID
        };
        Self {
            current_language_id: current_language_id.into(),
            strings: Arc::new(
                I18nStrings::for_language_id(current_language_id)
                    .unwrap_or_else(I18nStrings::en_us),
            ),
            custom_languages: Vec::new(),
            language_catalog: builtin_language_catalog(),
        }
    }

    /// Returns the identifier of the currently active UI language.
    pub fn current_language_id(&self) -> &str {
        &self.current_language_id
    }

    /// Returns the strings for the currently active UI language.
    pub fn strings(&self) -> &I18nStrings {
        &self.strings
    }

    /// Returns an `Arc` clone of the currently active strings — O(1), no
    /// per-field copy. Use this in hot render paths instead of cloning the
    /// whole `I18nStrings` struct (137 `String` fields).
    pub fn strings_arc(&self) -> Arc<I18nStrings> {
        self.strings.clone()
    }

    /// Returns all built-in and imported UI languages exposed in the menu.
    pub fn available_languages(&self) -> &[LanguageCatalogEntry] {
        &self.language_catalog
    }

    /// Activates a UI language by identifier.
    pub fn set_language_by_id(&mut self, language_id: &str) -> bool {
        let strings = if let Some(strings) = I18nStrings::for_language_id(language_id) {
            strings
        } else if let Some(pack) = self
            .custom_languages
            .iter()
            .find(|pack| pack.id == language_id)
        {
            pack.strings.clone()
        } else {
            return false;
        };
        let changed = self.current_language_id != language_id;
        self.current_language_id = language_id.into();
        self.strings = Arc::new(strings);
        changed
    }

    /// Imports a user language pack, persists a normalized copy, and activates it.
    pub fn import_language_config(&mut self, path: impl AsRef<Path>) -> anyhow::Result<String> {
        let dirs = MarkmanConfigDirs::from_system()?;
        self.import_language_config_with_dirs(path, &dirs)
    }

    fn import_language_config_with_dirs(
        &mut self,
        path: impl AsRef<Path>,
        dirs: &MarkmanConfigDirs,
    ) -> anyhow::Result<String> {
        let raw = read_json_or_jsonc(path.as_ref())?;
        let (pack, normalized) = custom_language_pack_from_value(raw)?;
        let file_name = format!("{}.json", sanitize_config_file_stem(&pack.id));
        catalog::persist_normalized_json_config(&dirs.languages_dir(), &file_name, &normalized)?;
        let imported_id = pack.id.clone();
        self.upsert_custom_language(pack);
        self.set_language_by_id(&imported_id);
        Ok(imported_id)
    }

    fn load_custom_languages_from_dirs(&mut self, dirs: &MarkmanConfigDirs) -> anyhow::Result<()> {
        let mut loaded =
            catalog::scan_json_config_dir(&dirs.languages_dir(), "language", |_path, value| {
                custom_language_pack_from_value(value).map(|(pack, _)| pack)
            })?;
        loaded.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        for pack in loaded {
            self.upsert_custom_language(pack);
        }
        Ok(())
    }

    fn upsert_custom_language(&mut self, pack: I18nLanguagePack) {
        catalog::upsert_by_id(&mut self.custom_languages, pack, |pack| pack.id.as_str());
        self.rebuild_language_catalog();
    }

    fn rebuild_language_catalog(&mut self) {
        let mut catalog = builtin_language_catalog();
        catalog.extend(
            self.custom_languages
                .iter()
                .map(|pack| LanguageCatalogEntry {
                    id: pack.id.clone(),
                    name: pack.name.clone(),
                }),
        );
        self.language_catalog = catalog;
    }
}

fn custom_language_pack_from_value(mut value: Value) -> anyhow::Result<(I18nLanguagePack, Value)> {
    prune_empty_json_values(&mut value);
    let Value::Object(object) = value else {
        bail!("language config must be a JSON object");
    };
    let object = object_without_empty_values(object);
    let id = required_string(&object, "id")?;
    if is_builtin_language_id(&id) {
        bail!("custom language id '{id}' would override a built-in language");
    }
    if !is_valid_custom_language_id(&id) {
        bail!("custom language id '{id}' contains unsupported characters");
    }
    let name = required_string(&object, "name")?;
    let mut normalized_object = Map::new();
    normalized_object.insert("id".into(), Value::String(id.clone()));
    normalized_object.insert("name".into(), Value::String(name));
    for key in ["author", "description", "version", "homepage", "license"] {
        if let Some(value) = object.get(key) {
            normalized_object.insert(key.into(), value.clone());
        }
    }
    if let Some(strings) = object.get("strings").and_then(Value::as_object) {
        let mut normalized_strings = Map::new();
        for key in I18N_STRING_KEYS {
            if let Some(value) = strings.get(*key) {
                normalized_strings.insert((*key).into(), value.clone());
            }
        }
        if !normalized_strings.is_empty() {
            normalized_object.insert("strings".into(), Value::Object(normalized_strings));
        }
    }
    let normalized = Value::Object(normalized_object);
    let pack = I18nLanguagePack::from_value(normalized.clone())
        .with_context(|| format!("failed to parse language config '{id}'"))?;
    Ok((pack, normalized))
}

fn required_string(object: &Map<String, Value>, key: &str) -> anyhow::Result<String> {
    let Some(value) = object.get(key) else {
        bail!("missing required field '{key}'");
    };
    let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        bail!("field '{key}' must be a non-empty string");
    };
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::{I18nLanguagePack, I18nManager, I18nStrings, language_id_for_locale_preferences};
    use crate::config::MarkmanConfigDirs;
    use crate::theme::ThemeManager;

    #[test]
    fn built_in_chinese_strings_are_utf8() {
        let strings = I18nStrings::zh_cn();
        assert_eq!(strings.menu_file, "文件");
        assert_eq!(strings.menu_export, "导出");
        assert_eq!(strings.menu_language, "语言");
        assert_eq!(strings.save_failed_title, "保存失败");
        assert_eq!(strings.export_failed_title, "导出失败");
        assert_eq!(strings.view_mode_switch_to_source, "切换到源码");
        assert_eq!(strings.context_menu_insert, "插入");
        assert_eq!(strings.table_insert_title, "插入表格");
        assert_eq!(strings.image_loading_without_alt, "正在加载图片...");
        assert_eq!(
            strings.help_check_updates_message,
            "正在检查 Markman 的最新版本..."
        );
        assert_eq!(strings.update_open_release, "前往下载");
        assert_eq!(strings.help_about_github_label, "GitHub");
        assert_eq!(
            strings.help_about_star_message,
            "如果本项目对您有帮助，那不妨给本项目一颗 Star⭐，十分感谢！"
        );
    }

    #[test]
    fn manager_switches_builtin_languages() {
        let mut manager = I18nManager::default();
        assert_eq!(manager.current_language_id(), "en-US");
        assert_eq!(manager.strings().menu_file, "File");
        assert_eq!(manager.strings().menu_export, "Export");

        assert!(manager.set_language_by_id("zh-CN"));
        assert_eq!(manager.current_language_id(), "zh-CN");
        assert_eq!(manager.strings().menu_file, "文件");
        assert_eq!(manager.strings().menu_export, "导出");
        assert!(!manager.set_language_by_id("zh-CN"));
        assert!(!manager.set_language_by_id("missing"));
    }

    #[test]
    fn language_catalog_contains_chinese_and_english() {
        let manager = I18nManager::default();
        let ids = manager
            .available_languages()
            .iter()
            .map(|entry| (entry.id.as_str(), entry.name.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![("zh-CN", "简体中文"), ("en-US", "English")]);
    }

    #[test]
    fn manager_can_be_constructed_with_known_language() {
        let manager = I18nManager::new_with_language_id("zh-CN");
        assert_eq!(manager.current_language_id(), "zh-CN");
        assert_eq!(manager.strings().menu_file, "文件");

        let fallback = I18nManager::new_with_language_id("missing");
        assert_eq!(fallback.current_language_id(), "en-US");
        assert_eq!(fallback.strings().menu_file, "File");
    }

    #[test]
    fn theme_switch_does_not_modify_selected_language() {
        let mut theme_manager = ThemeManager::default();
        let mut i18n_manager = I18nManager::new_with_language_id("zh-CN");

        assert!(theme_manager.set_theme_by_id("markman"));
        assert!(!i18n_manager.set_language_by_id("missing"));

        assert_eq!(theme_manager.current_theme_id(), "markman");
        assert_eq!(i18n_manager.current_language_id(), "zh-CN");
        assert_eq!(i18n_manager.strings().menu_file, "文件");
    }

    #[test]
    fn locale_preferences_map_to_builtin_languages() {
        assert_eq!(language_id_for_locale_preferences(["zh-CN"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["zh-HK"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["zh-Hant-TW"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["zh_SG.UTF-8"]), "zh-CN");
        assert_eq!(language_id_for_locale_preferences(["en-US"]), "en-US");
        assert_eq!(language_id_for_locale_preferences(["en_GB.UTF-8"]), "en-US");
        assert_eq!(
            language_id_for_locale_preferences(["fr-FR", "zh-CN"]),
            "zh-CN"
        );
        assert_eq!(
            language_id_for_locale_preferences(Vec::<&str>::new()),
            "en-US"
        );
        assert_eq!(language_id_for_locale_preferences(["fr-FR"]), "en-US");
        assert_eq!(language_id_for_locale_preferences(["!!!"]), "en-US");
    }

    #[test]
    fn imports_jsonc_language_pack_and_persists_normalized_json() {
        let root = std::env::temp_dir().join(format!("velotype-i18n-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let source = root.join("language.jsonc");
        std::fs::write(
            &source,
            r#"{
                // Required metadata.
                "id": "ja-JP",
                "name": "日本語",
                "author": "",
                "strings": {
                    "menu_file": "ファイル",
                    "menu_export": ""
                }
            }"#,
        )
        .expect("language config should be written");

        let dirs = MarkmanConfigDirs::from_root(&root);
        let mut manager = I18nManager::default();
        let imported_id = manager
            .import_language_config_with_dirs(&source, &dirs)
            .expect("language config should import");

        assert_eq!(imported_id, "ja-JP");
        assert_eq!(manager.current_language_id(), "ja-JP");
        assert_eq!(manager.strings().menu_file, "ファイル");
        assert_eq!(manager.strings().menu_export, "Export");
        assert!(
            manager
                .available_languages()
                .iter()
                .any(|entry| entry.id == "ja-JP" && entry.name == "日本語")
        );

        let normalized = std::fs::read_to_string(dirs.languages_dir().join("ja-JP.json"))
            .expect("normalized language config should exist");
        assert!(normalized.contains("\"menu_file\": \"ファイル\""));
        assert!(!normalized.contains("menu_export"));
        assert!(!normalized.contains("author"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn custom_language_cannot_override_builtin_language_id() {
        let root = std::env::temp_dir().join(format!("velotype-i18n-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let source = root.join("language.json");
        std::fs::write(
            &source,
            r#"{
                "id": "en-US",
                "name": "Override",
                "strings": { "menu_file": "Override" }
            }"#,
        )
        .expect("language config should be written");

        let dirs = MarkmanConfigDirs::from_root(&root);
        let mut manager = I18nManager::default();
        let err = manager
            .import_language_config_with_dirs(&source, &dirs)
            .expect_err("built-in language ids should be rejected");
        assert!(err.to_string().contains("built-in language"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn language_pack_json_falls_back_for_missing_strings() {
        let pack = I18nLanguagePack::from_json(
            r#"{
                "id": "zh-CN",
                "name": "简体中文",
                "strings": {
                    "menu_file": "文件菜单",
                    "unsaved_changes_hint": "legacy hint",
                    "drop_replace_hint": "legacy hint",
                    "unknown_field": "ignored"
                }
            }"#,
        )
        .expect("language pack should load");

        assert_eq!(pack.id, "zh-CN");
        assert_eq!(pack.name, "简体中文");
        assert_eq!(pack.strings.menu_file, "文件菜单");
        assert_eq!(pack.strings.menu_export, "导出");
        assert_eq!(pack.strings.info_dialog_ok, "确定");
        assert_eq!(pack.strings.update_open_release, "前往下载");
        assert_eq!(pack.strings.help_about_github_label, "GitHub");
        assert_eq!(
            pack.strings.help_about_star_message,
            "如果本项目对您有帮助，那不妨给本项目一颗 Star⭐，十分感谢！"
        );
    }

    #[test]
    fn unknown_language_pack_falls_back_to_english_strings() {
        let pack = I18nLanguagePack::from_json(
            r#"{
                "id": "fr-FR",
                "strings": {
                    "menu_file": "Fichier"
                }
            }"#,
        )
        .expect("language pack should load");

        assert_eq!(pack.id, "fr-FR");
        assert_eq!(pack.name, "fr-FR");
        assert_eq!(pack.strings.menu_file, "Fichier");
        assert_eq!(pack.strings.menu_export, "Export");
        assert_eq!(pack.strings.info_dialog_ok, "OK");
        assert_eq!(pack.strings.update_open_release, "Open Releases");
        assert_eq!(pack.strings.menu_open_recent_file, "Open Recent File");
        assert_eq!(pack.strings.menu_no_recent_files, "No Recent Files");
        assert_eq!(
            pack.strings.recent_file_missing_title,
            "Recent File Missing"
        );
        assert_eq!(pack.strings.help_about_github_label, "GitHub");
        assert_eq!(
            pack.strings.help_about_star_message,
            "If this project helps you, consider giving it a Star⭐. Thank you!"
        );
    }
}
