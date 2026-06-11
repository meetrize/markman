//! Workspace file-tree context menu and file-system operations.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::*;

use super::Editor;
use super::workspace_name_input::WorkspaceNameInputElement;
use crate::components::{
    Copy, Cut, Delete, DeleteBack, End, Home, MoveLeft, MoveRight, Paste, SelectAll, SelectEnd,
    SelectHome, SelectLeft, SelectRight,
};
use crate::i18n::I18nManager;
use crate::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceFileMenuTarget {
    Directory(PathBuf),
    MarkdownFile(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkspaceFileContextMenuState {
    position: Point<Pixels>,
    target: WorkspaceFileMenuTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceNameDialogKind {
    NewFolder { parent: PathBuf },
    NewMarkdown { parent: PathBuf },
    Rename { path: PathBuf },
}

#[derive(Clone, Debug)]
pub(super) struct WorkspaceNameDialogState {
    pub(super) kind: WorkspaceNameDialogKind,
    pub(super) name: String,
    pub(super) marked_range: Option<Range<usize>>,
    pub(super) selected_range: Range<usize>,
    pub(super) selection_reversed: bool,
}

impl Editor {
    pub(super) fn workspace_name_input_active(&self, window: &Window) -> bool {
        self.workspace_name_dialog.is_some() && self.workspace_name_focus.is_focused(window)
    }

    pub(super) fn workspace_name_focus_handle(&self) -> FocusHandle {
        self.workspace_name_focus.clone()
    }

    pub(super) fn workspace_name_text(&self) -> String {
        self.workspace_name_dialog
            .as_ref()
            .map(|dialog| dialog.name.clone())
            .unwrap_or_default()
    }

    pub(super) fn workspace_name_marked_range(&self) -> Option<Range<usize>> {
        self.workspace_name_dialog
            .as_ref()
            .and_then(|dialog| dialog.marked_range.clone())
    }

    pub(super) fn workspace_name_selected_range(&self) -> Range<usize> {
        self.workspace_name_dialog
            .as_ref()
            .map(|dialog| dialog.selected_range.clone())
            .unwrap_or(0..0)
    }

    pub(super) fn workspace_name_cursor_offset(&self) -> usize {
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return 0;
        };
        if dialog.selection_reversed {
            dialog.selected_range.start
        } else {
            dialog.selected_range.end
        }
    }

    pub(super) fn set_workspace_name_layout(
        &mut self,
        line: ShapedLine,
        bounds: Bounds<Pixels>,
    ) {
        self.workspace_name_last_layout = Some(line);
        self.workspace_name_last_bounds = Some(bounds);
    }

    pub(super) fn workspace_name_index_for_mouse_position(
        &self,
        position: Point<Pixels>,
    ) -> usize {
        let text = self.workspace_name_text();
        if text.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (
            self.workspace_name_last_bounds.as_ref(),
            self.workspace_name_last_layout.as_ref(),
        ) else {
            return text.len();
        };

        if position.x <= bounds.left() {
            return 0;
        }
        if position.x >= bounds.right() {
            return text.len();
        }

        line.closest_index_for_x(position.x - bounds.left())
            .min(text.len())
    }

    pub(super) fn open_workspace_file_context_menu(
        &mut self,
        position: Point<Pixels>,
        target: WorkspaceFileMenuTarget,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_files_panel_active() {
            return;
        }

        self.close_workspace_file_context_menu(cx);
        self.close_workspace_name_dialog(cx);
        self.context_menu = None;
        self.context_menu_submenu_close_task = None;
        self.workspace_file_context_menu = Some(WorkspaceFileContextMenuState { position, target });
        cx.notify();
    }

    pub(super) fn close_workspace_file_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace_file_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn close_workspace_name_dialog(&mut self, cx: &mut Context<Self>) {
        if self.workspace_name_dialog.take().is_some() {
            self.workspace_name_is_selecting = false;
            cx.notify();
        }
    }

    pub(super) fn on_dismiss_workspace_file_context_menu(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_file_context_menu(cx);
    }

    pub(super) fn on_dismiss_workspace_name_dialog(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_name_dialog(cx);
    }

    fn workspace_file_menu_target(&self) -> Option<WorkspaceFileMenuTarget> {
        self.workspace_file_context_menu
            .as_ref()
            .map(|menu| menu.target.clone())
    }

    fn is_workspace_tree_root(&self, path: &Path) -> bool {
        self.workspace_is_tree_root(path)
    }

    fn refresh_workspace_tree_after_fs_change(&mut self, cx: &mut Context<Self>) {
        self.workspace_refresh_file_tree(cx);
    }

    fn select_workspace_file_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.workspace_select_file_path(path, cx);
    }

    fn expand_workspace_path_ancestors(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.workspace_expand_path(path, cx);
    }

    fn open_workspace_name_dialog(
        &mut self,
        kind: WorkspaceNameDialogKind,
        default_name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_file_context_menu(cx);
        let len = default_name.len();
        self.workspace_name_dialog = Some(WorkspaceNameDialogState {
            kind,
            name: default_name,
            marked_range: None,
            selected_range: 0..len,
            selection_reversed: false,
        });
        window.focus(&self.workspace_name_focus);
        cx.notify();
    }

    pub(super) fn on_workspace_file_menu_new_folder(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = self.workspace_file_menu_parent_dir() else {
            return;
        };
        let default_name = cx
            .global::<I18nManager>()
            .strings()
            .workspace_default_folder_name
            .clone();
        self.open_workspace_name_dialog(
            WorkspaceNameDialogKind::NewFolder { parent },
            default_name,
            window,
            cx,
        );
    }

    pub(super) fn on_workspace_file_menu_new_markdown(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = self.workspace_file_menu_parent_dir() else {
            return;
        };
        let default_name = cx
            .global::<I18nManager>()
            .strings()
            .workspace_default_file_name
            .clone();
        self.open_workspace_name_dialog(
            WorkspaceNameDialogKind::NewMarkdown { parent },
            default_name,
            window,
            cx,
        );
    }

    pub(super) fn on_workspace_file_menu_rename(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.workspace_file_menu_target_path() else {
            return;
        };
        let default_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.open_workspace_name_dialog(
            WorkspaceNameDialogKind::Rename { path },
            default_name,
            window,
            cx,
        );
    }

    pub(super) fn on_workspace_file_menu_delete(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.workspace_file_menu_target_path() else {
            return;
        };
        if let Some(WorkspaceFileMenuTarget::Directory(dir_path)) =
            self.workspace_file_context_menu.as_ref().map(|menu| menu.target.clone())
        {
            if self.is_workspace_tree_root(&dir_path) {
                return;
            }
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let strings = cx.global::<I18nManager>().strings_arc();
        let message = strings
            .workspace_delete_confirm_message
            .replace("{name}", &name);
        let title = strings.workspace_delete_confirm_title.clone();
        let delete_label = strings.workspace_menu_delete.clone();
        let cancel_label = strings.drop_replace_cancel.clone();
        self.close_workspace_file_context_menu(cx);

        let weak_editor = cx.entity().downgrade();
        let path_to_delete = path.clone();
        let prompt = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&message),
            &[delete_label.as_str(), cancel_label.as_str()],
            cx,
        );
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(choice) = prompt.await else {
                return;
            };
            if choice != 0 {
                return;
            }
            let delete_result = if path_to_delete.is_dir() {
                std::fs::remove_dir_all(&path_to_delete)
            } else {
                std::fs::remove_file(&path_to_delete)
            };
            match delete_result {
                Ok(()) => {
                    let _ = weak_editor.update(cx, |editor, cx| {
                        editor.apply_workspace_delete_success(&path_to_delete, cx);
                    });
                }
                Err(err) => {
                    let detail = err.to_string();
                    let _ = cx.update_window(
                        window_handle,
                        move |_view: AnyView, window: &mut Window, cx: &mut App| {
                            let strings = cx.global::<I18nManager>().strings().clone();
                            let buttons = [strings.info_dialog_ok.as_str()];
                            let _ = window.prompt(
                                PromptLevel::Critical,
                                &strings.workspace_operation_failed_title,
                                Some(&detail),
                                &buttons,
                                cx,
                            );
                        },
                    );
                }
            }
        })
        .detach();
    }

    pub(super) fn on_workspace_file_menu_copy_path(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.workspace_file_menu_target_path() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(path.to_string_lossy().into_owned()));
        self.close_workspace_file_context_menu(cx);
    }

    pub(super) fn on_workspace_file_menu_reveal(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.workspace_file_menu_target_path() else {
            return;
        };
        reveal_in_file_manager(&path);
        self.close_workspace_file_context_menu(cx);
    }

    pub(super) fn on_workspace_file_menu_refresh(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_file_context_menu(cx);
        self.refresh_workspace_tree_after_fs_change(cx);
    }

    pub(super) fn on_confirm_workspace_name_dialog(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_workspace_name_dialog(window, cx);
    }

    fn confirm_workspace_name_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.clone() else {
            return;
        };
        let trimmed = dialog.name.trim();
        if trimmed.is_empty() {
            return;
        }

        let result = match dialog.kind.clone() {
            WorkspaceNameDialogKind::NewFolder { parent } => {
                create_directory(&parent, trimmed).map(|path| (path, false))
            }
            WorkspaceNameDialogKind::NewMarkdown { parent } => {
                create_markdown_file(&parent, trimmed).map(|path| (path, true))
            }
            WorkspaceNameDialogKind::Rename { path } => {
                rename_workspace_path(&path, trimmed).map(|new_path| (new_path, false))
            }
        };

        match result {
            Ok((new_path, open_file)) => {
                let rename_from = match dialog.kind {
                    WorkspaceNameDialogKind::Rename { path } => Some(path),
                    _ => None,
                };
                self.close_workspace_name_dialog(cx);
                self.apply_workspace_fs_success(new_path, open_file, rename_from, window, cx);
            }
            Err(err) => self.show_workspace_fs_error(&err, window, cx),
        }
    }

    pub(super) fn on_cancel_workspace_name_dialog(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_workspace_name_dialog(cx);
    }

    fn apply_workspace_delete_success(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.file_path.as_deref() == Some(path) {
            self.file_path = None;
            self.pending_window_title_refresh = true;
        }
        if self.workspace_folder_root_is(path) {
            self.workspace_clear_folder_root_if(path, cx);
        }
        self.workspace_clear_file_selection_if(path, cx);
        self.refresh_workspace_tree_after_fs_change(cx);
    }

    fn apply_workspace_fs_success(
        &mut self,
        path: PathBuf,
        open_file: bool,
        rename_from: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(old_path) = rename_from {
            if self.file_path.as_deref() == Some(old_path.as_path()) {
                self.file_path = Some(path.clone());
                self.pending_window_title_refresh = true;
            }
            if self.workspace_folder_root_is(old_path.as_path()) {
                self.workspace_set_folder_root(path.clone(), cx);
                let _ = crate::config::record_last_workspace_folder(&path);
            }
            self.workspace_clear_file_selection_if(old_path.as_path(), cx);
        }

        self.expand_workspace_path_ancestors(&path, cx);
        self.refresh_workspace_tree_after_fs_change(cx);
        self.select_workspace_file_path(path.clone(), cx);
        if open_file {
            self.open_workspace_file(path, window, cx);
        }
    }

    fn show_workspace_fs_error(&mut self, detail: &str, window: &mut Window, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings_arc();
        let title = strings.workspace_operation_failed_title.clone();
        let ok = strings.info_dialog_ok.clone();
        let detail = detail.to_string();
        let buttons = [ok.as_str()];
        let _ = window.prompt(PromptLevel::Critical, &title, Some(&detail), &buttons, cx);
    }

    fn workspace_file_menu_parent_dir(&self) -> Option<PathBuf> {
        match self.workspace_file_menu_target()? {
            WorkspaceFileMenuTarget::Directory(path) => Some(path),
            WorkspaceFileMenuTarget::MarkdownFile(path) => path.parent().map(Path::to_path_buf),
        }
    }

    fn workspace_file_menu_target_path(&self) -> Option<PathBuf> {
        match self.workspace_file_menu_target()? {
            WorkspaceFileMenuTarget::Directory(path) | WorkspaceFileMenuTarget::MarkdownFile(path) => {
                Some(path)
            }
        }
    }

    pub(super) fn replace_workspace_name_dialog_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        marked: bool,
        selected: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.workspace_name_dialog.as_mut() else {
            return;
        };
        let start = range.start.min(dialog.name.len());
        let end = range.end.min(dialog.name.len());
        dialog.name.replace_range(start..end, new_text);
        let cursor = start + new_text.len();
        dialog.selected_range = selected.unwrap_or(cursor..cursor);
        dialog.marked_range = marked.then(|| dialog.selected_range.clone());
        cx.notify();
    }

    fn workspace_name_move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.as_mut() else {
            return;
        };
        let offset = offset.min(dialog.name.len());
        dialog.selected_range = offset..offset;
        dialog.marked_range = None;
        dialog.selection_reversed = false;
        self.workspace_name_is_selecting = false;
        cx.notify();
    }

    fn workspace_name_select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.as_mut() else {
            return;
        };
        super::workspace::extend_single_line_selection(
            &mut dialog.selected_range,
            &mut dialog.selection_reversed,
            offset,
            dialog.name.len(),
        );
        dialog.marked_range = None;
        cx.notify();
    }

    fn workspace_name_delete_backward(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        if !dialog.selected_range.is_empty() {
            self.replace_workspace_name_dialog_text(
                dialog.selected_range.clone(),
                "",
                false,
                None,
                cx,
            );
            return;
        }

        let cursor = dialog.selected_range.end;
        if cursor == 0 {
            return;
        }
        let previous = super::workspace::workspace_text_grapheme_boundary(&dialog.name, cursor, true);
        self.replace_workspace_name_dialog_text(previous..cursor, "", false, Some(previous..previous), cx);
    }

    fn workspace_name_delete_forward(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        if !dialog.selected_range.is_empty() {
            self.replace_workspace_name_dialog_text(
                dialog.selected_range.clone(),
                "",
                false,
                None,
                cx,
            );
            return;
        }

        let cursor = dialog.selected_range.end;
        if cursor >= dialog.name.len() {
            return;
        }
        let next = super::workspace::workspace_text_grapheme_boundary(&dialog.name, cursor, false);
        self.replace_workspace_name_dialog_text(cursor..next, "", false, Some(cursor..cursor), cx);
    }

    fn workspace_name_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let Some(dialog) = self.workspace_name_dialog.as_ref() else {
                return;
            };
            let range = if dialog.selected_range.is_empty() {
                dialog.selected_range.clone()
            } else {
                dialog.selected_range.clone()
            };
            self.replace_workspace_name_dialog_text(range, &text, false, None, cx);
        }
    }

    fn workspace_name_copy_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        if dialog.selected_range.is_empty() {
            return;
        }
        let selected = dialog.name[dialog.selected_range.clone()].to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(selected));
    }

    fn workspace_name_cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        self.workspace_name_copy_to_clipboard(cx);
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        if !dialog.selected_range.is_empty() {
            self.replace_workspace_name_dialog_text(dialog.selected_range.clone(), "", false, None, cx);
        }
    }

    fn workspace_name_select_all_text(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        let len = dialog.name.len();
        self.workspace_name_move_to(0, cx);
        self.workspace_name_select_to(len, cx);
    }

    pub(super) fn render_workspace_file_context_menu_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace_file_context_menu.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let is_directory = matches!(menu.target, WorkspaceFileMenuTarget::Directory(_));
        let is_root = matches!(&menu.target, WorkspaceFileMenuTarget::Directory(path) if self.is_workspace_tree_root(path));

        let mut items: Vec<AnyElement> = Vec::new();
        if is_directory {
            items.push(menu_item(
                "workspace-file-menu-new-md",
                s.workspace_menu_new_file.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_new_markdown),
                c,
                d,
                t,
            ));
            items.push(menu_item(
                "workspace-file-menu-new-folder",
                s.workspace_menu_new_folder.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_new_folder),
                c,
                d,
                t,
            ));
            items.push(menu_separator(c, d));
            if !is_root {
                items.push(menu_item(
                    "workspace-file-menu-rename",
                    s.workspace_menu_rename.clone(),
                    false,
                    cx.listener(Self::on_workspace_file_menu_rename),
                    c,
                    d,
                    t,
                ));
                items.push(menu_item(
                    "workspace-file-menu-delete",
                    s.workspace_menu_delete.clone(),
                    true,
                    cx.listener(Self::on_workspace_file_menu_delete),
                    c,
                    d,
                    t,
                ));
                items.push(menu_separator(c, d));
            }
            items.push(menu_item(
                "workspace-file-menu-copy-path",
                s.workspace_menu_copy_path.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_copy_path),
                c,
                d,
                t,
            ));
            items.push(menu_item(
                "workspace-file-menu-reveal",
                s.workspace_menu_reveal_in_file_manager.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_reveal),
                c,
                d,
                t,
            ));
            items.push(menu_item(
                "workspace-file-menu-refresh",
                s.workspace_menu_refresh.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_refresh),
                c,
                d,
                t,
            ));
        } else {
            items.push(menu_item(
                "workspace-file-menu-rename",
                s.workspace_menu_rename.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_rename),
                c,
                d,
                t,
            ));
            items.push(menu_item(
                "workspace-file-menu-delete",
                s.workspace_menu_delete.clone(),
                true,
                cx.listener(Self::on_workspace_file_menu_delete),
                c,
                d,
                t,
            ));
            items.push(menu_separator(c, d));
            items.push(menu_item(
                "workspace-file-menu-copy-path",
                s.workspace_menu_copy_path.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_copy_path),
                c,
                d,
                t,
            ));
            items.push(menu_item(
                "workspace-file-menu-reveal",
                s.workspace_menu_reveal_in_file_manager.clone(),
                false,
                cx.listener(Self::on_workspace_file_menu_reveal),
                c,
                d,
                t,
            ));
        }

        let panel_x = menu.position.x;
        let panel_y = menu.position.y;
        let panel_width = px(d.context_menu_panel_width.max(168.0));

        Some(
            div()
                .id("workspace-file-menu-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_workspace_file_context_menu),
                )
                .child(
                    div()
                        .id("workspace-file-menu-panel")
                        .absolute()
                        .left(panel_x)
                        .top(panel_y)
                        .w(panel_width)
                        .p(px(d.menu_panel_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.menu_panel_gap))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                            cx.stop_propagation()
                        })
                        .children(items),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_workspace_name_dialog_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.workspace_name_dialog.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let title = match dialog.kind {
            WorkspaceNameDialogKind::NewFolder { .. } => s.workspace_dialog_new_folder_title.clone(),
            WorkspaceNameDialogKind::NewMarkdown { .. } => s.workspace_dialog_new_file_title.clone(),
            WorkspaceNameDialogKind::Rename { .. } => s.workspace_dialog_rename_title.clone(),
        };
        let name_focus = self.workspace_name_focus.clone();

        Some(
            div()
                .id("workspace-name-dialog-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_dismiss_workspace_name_dialog),
                )
                .child(
                    div()
                        .id("workspace-name-dialog")
                        .w(px(d.dialog_width.min(420.0)))
                        .max_w(relative(1.0))
                        .p(px(d.dialog_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.dialog_gap))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .track_focus(&self.workspace_name_focus)
                        .key_context("BlockEditor")
                        .on_key_down(cx.listener(Self::on_workspace_name_dialog_key_down))
                        .on_action(cx.listener(Self::on_workspace_name_delete_back))
                        .on_action(cx.listener(Self::on_workspace_name_delete_forward))
                        .on_action(cx.listener(Self::on_workspace_name_paste))
                        .on_action(cx.listener(Self::on_workspace_name_copy))
                        .on_action(cx.listener(Self::on_workspace_name_cut))
                        .on_action(cx.listener(Self::on_workspace_name_select_all))
                        .on_action(cx.listener(Self::on_workspace_name_move_left))
                        .on_action(cx.listener(Self::on_workspace_name_move_right))
                        .on_action(cx.listener(Self::on_workspace_name_home))
                        .on_action(cx.listener(Self::on_workspace_name_end))
                        .on_action(cx.listener(Self::on_workspace_name_select_left))
                        .on_action(cx.listener(Self::on_workspace_name_select_right))
                        .on_action(cx.listener(Self::on_workspace_name_select_home))
                        .on_action(cx.listener(Self::on_workspace_name_select_end))
                        .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                            cx.stop_propagation();
                            window.focus(&name_focus);
                        })
                        .child(
                            div()
                                .text_size(px(t.dialog_title_size))
                                .font_weight(t.dialog_title_weight.to_font_weight())
                                .text_color(c.dialog_title)
                                .child(title),
                        )
                        .child(
                            div()
                                .id("workspace-name-dialog-input")
                                .w_full()
                                .h(px(28.0))
                                .px(px(8.0))
                                .flex()
                                .items_center()
                                .rounded(px(4.0))
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .bg(c.dialog_secondary_button_bg)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .h_full()
                                        .overflow_hidden()
                                        .child(WorkspaceNameInputElement::new(cx.entity())),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_workspace_name_mouse_down),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(Self::on_workspace_name_mouse_up),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(Self::on_workspace_name_mouse_up),
                                )
                                .on_mouse_move(cx.listener(Self::on_workspace_name_mouse_move)),
                        )
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(d.dialog_gap))
                                .child(dialog_button(
                                    "workspace-name-dialog-cancel",
                                    s.drop_replace_cancel.clone(),
                                    false,
                                    cx.listener(Self::on_cancel_workspace_name_dialog),
                                    c,
                                    d,
                                    t,
                                ))
                                .child(dialog_button(
                                    "workspace-name-dialog-confirm",
                                    s.info_dialog_ok.clone(),
                                    false,
                                    cx.listener(Self::on_confirm_workspace_name_dialog),
                                    c,
                                    d,
                                    t,
                                )),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn on_workspace_name_dialog_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }

        let modifiers = &event.keystroke.modifiers;
        if super::workspace::workspace_search_primary_shortcut_modifiers(modifiers) {
            match event.keystroke.key.as_str() {
                "v" => {
                    self.workspace_name_paste_from_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                "c" => {
                    self.workspace_name_copy_to_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                "x" => {
                    self.workspace_name_cut_to_clipboard(cx);
                    cx.stop_propagation();
                    return;
                }
                "a" => {
                    self.workspace_name_select_all_text(cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_workspace_name_dialog(cx);
                cx.stop_propagation();
            }
            "enter" if !event.keystroke.modifiers.platform => {
                self.confirm_workspace_name_dialog(window, cx);
                cx.stop_propagation();
            }
            "backspace" => {
                self.workspace_name_delete_backward(cx);
                cx.stop_propagation();
            }
            "delete" => {
                self.workspace_name_delete_forward(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(super) fn on_workspace_name_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.workspace_name_focus);
        let offset = self.workspace_name_index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.workspace_name_select_to(offset, cx);
        } else {
            self.workspace_name_move_to(offset, cx);
        }
        self.workspace_name_is_selecting = true;
    }

    pub(super) fn on_workspace_name_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_name_is_selecting {
            self.workspace_name_is_selecting = false;
            cx.notify();
        }
    }

    pub(super) fn on_workspace_name_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_is_selecting || !self.workspace_name_input_active(window) {
            return;
        }
        if !event.dragging() {
            self.workspace_name_is_selecting = false;
            cx.notify();
            return;
        }
        self.workspace_name_select_to(
            self.workspace_name_index_for_mouse_position(event.position),
            cx,
        );
    }

    pub(super) fn on_workspace_name_delete_back(
        &mut self,
        _: &DeleteBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_delete_backward(cx);
    }

    pub(super) fn on_workspace_name_delete_forward(
        &mut self,
        _: &Delete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_delete_forward(cx);
    }

    pub(super) fn on_workspace_name_paste(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_paste_from_clipboard(cx);
    }

    pub(super) fn on_workspace_name_copy(
        &mut self,
        _: &Copy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_copy_to_clipboard(cx);
    }

    pub(super) fn on_workspace_name_cut(
        &mut self,
        _: &Cut,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_cut_to_clipboard(cx);
    }

    pub(super) fn on_workspace_name_select_all(
        &mut self,
        _: &SelectAll,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_select_all_text(cx);
    }

    pub(super) fn on_workspace_name_move_left(
        &mut self,
        _: &MoveLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        if dialog.selected_range.is_empty() {
            let previous = super::workspace::workspace_text_grapheme_boundary(
                &dialog.name,
                self.workspace_name_cursor_offset(),
                true,
            );
            self.workspace_name_move_to(previous, cx);
        } else {
            self.workspace_name_move_to(dialog.selected_range.start, cx);
        }
    }

    pub(super) fn on_workspace_name_move_right(
        &mut self,
        _: &MoveRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        if dialog.selected_range.is_empty() {
            let next = super::workspace::workspace_text_grapheme_boundary(
                &dialog.name,
                self.workspace_name_cursor_offset(),
                false,
            );
            self.workspace_name_move_to(next, cx);
        } else {
            self.workspace_name_move_to(dialog.selected_range.end, cx);
        }
    }

    pub(super) fn on_workspace_name_home(
        &mut self,
        _: &Home,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_move_to(0, cx);
    }

    pub(super) fn on_workspace_name_end(
        &mut self,
        _: &End,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let len = self
            .workspace_name_dialog
            .as_ref()
            .map(|dialog| dialog.name.len())
            .unwrap_or(0);
        self.workspace_name_move_to(len, cx);
    }

    pub(super) fn on_workspace_name_select_left(
        &mut self,
        _: &SelectLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        self.workspace_name_select_to(
            super::workspace::workspace_text_grapheme_boundary(
                &dialog.name,
                self.workspace_name_cursor_offset(),
                true,
            ),
            cx,
        );
    }

    pub(super) fn on_workspace_name_select_right(
        &mut self,
        _: &SelectRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let Some(dialog) = self.workspace_name_dialog.as_ref() else {
            return;
        };
        self.workspace_name_select_to(
            super::workspace::workspace_text_grapheme_boundary(
                &dialog.name,
                self.workspace_name_cursor_offset(),
                false,
            ),
            cx,
        );
    }

    pub(super) fn on_workspace_name_select_home(
        &mut self,
        _: &SelectHome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        self.workspace_name_select_to(0, cx);
    }

    pub(super) fn on_workspace_name_select_end(
        &mut self,
        _: &SelectEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace_name_input_active(window) {
            return;
        }
        cx.stop_propagation();
        let len = self
            .workspace_name_dialog
            .as_ref()
            .map(|dialog| dialog.name.len())
            .unwrap_or(0);
        self.workspace_name_select_to(len, cx);
    }
}

fn menu_item(
    id: &'static str,
    label: String,
    danger: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    c: &crate::theme::ThemeColors,
    d: &crate::theme::ThemeDimensions,
    t: &crate::theme::ThemeTypography,
) -> AnyElement {
    div()
        .id(id)
        .h(px(d.menu_item_height))
        .px(px(d.menu_item_padding_x))
        .flex()
        .items_center()
        .rounded(px(d.menu_item_radius))
        .bg(c.dialog_surface)
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .active(|this| this.opacity(0.92))
        .cursor_pointer()
        .text_size(px(d.menu_text_size))
        .font_weight(t.dialog_body_weight.to_font_weight())
        .text_color(if danger {
            c.dialog_danger_button_bg
        } else {
            c.dialog_secondary_button_text
        })
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

fn menu_separator(c: &crate::theme::ThemeColors, d: &crate::theme::ThemeDimensions) -> AnyElement {
    div()
        .w_full()
        .h(px(d.dialog_border_width))
        .bg(c.dialog_border)
        .into_any_element()
}

fn dialog_button(
    id: &'static str,
    label: String,
    primary: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    c: &crate::theme::ThemeColors,
    d: &crate::theme::ThemeDimensions,
    t: &crate::theme::ThemeTypography,
) -> AnyElement {
    div()
        .id(id)
        .px(px(d.dialog_button_padding_x))
        .py(px(6.0))
        .rounded(px(d.menu_item_radius))
        .cursor_pointer()
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(if primary {
            c.dialog_primary_button_text
        } else {
            c.dialog_secondary_button_text
        })
        .bg(if primary {
            c.dialog_primary_button_bg
        } else {
            c.dialog_secondary_button_bg
        })
        .hover(|this| {
            this.bg(if primary {
                c.dialog_primary_button_hover
            } else {
                c.dialog_secondary_button_hover
            })
        })
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

fn sanitize_file_name(name: &str) -> String {
    name.trim()
        .replace('/', "-")
        .replace('\\', "-")
        .chars()
        .filter(|ch| *ch != '\0')
        .collect::<String>()
        .trim_end_matches('.')
        .to_string()
}

fn unique_path_in_parent(parent: &Path, base_name: &str) -> PathBuf {
    let candidate = parent.join(base_name);
    if !candidate.exists() {
        return candidate;
    }

    let (stem, extension) = match base_name.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() && !ext.contains('/') && !ext.contains('\\') => {
            (stem.to_string(), Some(format!(".{ext}")))
        }
        _ => (base_name.to_string(), None),
    };

    for index in 2..=999 {
        let next_name = match &extension {
            Some(ext) => format!("{stem} {index}{ext}"),
            None => format!("{stem} {index}"),
        };
        let next_path = parent.join(&next_name);
        if !next_path.exists() {
            return next_path;
        }
    }
    parent.join(format!("{stem}-{}", uuid::Uuid::new_v4()))
}

fn create_directory(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = sanitize_file_name(name);
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    let path = unique_path_in_parent(parent, &name);
    std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
    Ok(path)
}

fn create_markdown_file(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let mut name = sanitize_file_name(name);
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    if !name.to_ascii_lowercase().ends_with(".md") {
        name.push_str(".md");
    }
    let path = unique_path_in_parent(parent, &name);
    std::fs::write(&path, "").map_err(|err| err.to_string())?;
    Ok(path)
}

fn rename_workspace_path(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_name = sanitize_file_name(new_name);
    if new_name.is_empty() {
        return Err("name cannot be empty".into());
    }
    let Some(parent) = path.parent() else {
        return Err("missing parent directory".into());
    };
    let new_path = parent.join(&new_name);
    if new_path.exists() {
        return Err("target already exists".into());
    }
    std::fs::rename(path, &new_path).map_err(|err| err.to_string())?;
    Ok(new_path)
}

fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(path).to_path_buf()
        };
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::{create_directory, create_markdown_file, unique_path_in_parent};

    #[test]
    fn unique_path_appends_suffix_for_existing_entries() {
        let root = std::env::temp_dir().join(format!("velotype-ws-fs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(root.join("notes")).expect("create notes");

        let next = unique_path_in_parent(&root, "notes");
        assert_eq!(next, root.join("notes 2"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_markdown_file_adds_md_extension() {
        let root = std::env::temp_dir().join(format!("velotype-ws-fs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");

        let path = create_markdown_file(&root, "note").expect("create md");
        assert_eq!(path, root.join("note.md"));
        assert!(path.is_file());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_directory_makes_unique_folder() {
        let root = std::env::temp_dir().join(format!("velotype-ws-fs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(root.join("draft")).expect("create draft");

        let path = create_directory(&root, "draft").expect("create folder");
        assert_eq!(path, root.join("draft 2"));
        assert!(path.is_dir());

        let _ = std::fs::remove_dir_all(root);
    }
}
