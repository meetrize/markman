//! In-app file/folder picker with global clipboard path navigation (⌘V).

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::Result;
use directories::BaseDirs;
use futures::channel::oneshot;
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::markdown_files::should_skip_workspace_entry_name;
use super::single_line_input::SingleLineInputTarget;
use super::single_line_input_element::SingleLineInputElement;
use super::Editor;
use crate::components::Paste;
use crate::i18n::I18nStrings;
use crate::input::single_line_field::SingleLineFieldState;
use crate::platform::{normalize_pasted_path, resolve_navigation_target};
use crate::theme::Theme;

const FOLDER_ICON: &str = "icon/workspace/folder.svg";
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
const FILE_ICON: &str = "icon/workspace/files.svg";
const PARENT_ICON: &str = "icon/workspace/chevron-right.svg";

#[derive(Debug)]
pub(crate) struct PathPickerRequest {
    pub files: bool,
    pub directories: bool,
    pub multiple: bool,
    pub title: String,
    pub completion: oneshot::Sender<Result<Option<Vec<PathBuf>>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PathPickerEntry {
    Parent,
    Directory(PathBuf, String),
    File(PathBuf, String),
}

#[derive(Debug)]
pub(super) struct PathPickerState {
    pub(super) open: bool,
    files: bool,
    directories: bool,
    multiple: bool,
    title: String,
    current_dir: PathBuf,
    entries: Vec<PathPickerEntry>,
    selection: usize,
    scroll_top: usize,
    selected_paths: HashSet<PathBuf>,
    pub(super) input: SingleLineFieldState,
    pub(super) focus_handle: FocusHandle,
    completion: Option<oneshot::Sender<Result<Option<Vec<PathBuf>>>>>,
}

impl PathPickerState {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            open: false,
            files: false,
            directories: false,
            multiple: false,
            title: String::new(),
            current_dir: PathBuf::new(),
            entries: Vec::new(),
            selection: 0,
            scroll_top: 0,
            selected_paths: HashSet::new(),
            input: SingleLineFieldState::new(),
            focus_handle: cx.focus_handle(),
            completion: None,
        }
    }
}

impl Editor {
    const PATH_PICKER_PANEL_WIDTH: f32 = 640.0;
    const PATH_PICKER_MAX_VISIBLE_ROWS: usize = 14;
    const PATH_PICKER_ROW_HEIGHT: f32 = 30.0;

    pub(crate) fn open_path_picker(
        &mut self,
        request: PathPickerRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path_picker.open {
            self.cancel_path_picker(cx);
        }

        self.dismiss_contextual_overlays(cx);

        let PathPickerRequest {
            files,
            directories,
            multiple,
            title,
            completion,
        } = request;

        let current_dir = self.path_picker_initial_directory();
        let state = &mut self.path_picker;
        state.open = true;
        state.files = files;
        state.directories = directories;
        state.multiple = multiple;
        state.title = title;
        state.current_dir = current_dir;
        state.selected_paths.clear();
        state.selection = 0;
        state.scroll_top = 0;
        state.completion = Some(completion);
        state.input.clear();
        state.input.replace_text(0..0, &state.current_dir.to_string_lossy());
        refresh_path_picker_entries(state);

        if let Some(path) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| normalize_pasted_path(&text))
        {
            self.path_picker_navigate_to(&path, None, cx);
        }

        window.focus(&self.path_picker.focus_handle);
        cx.notify();
    }

    fn path_picker_initial_directory(&self) -> PathBuf {
        if let Some(root) = self.effective_workspace_root() {
            return root;
        }
        BaseDirs::new()
            .map(|dirs| dirs.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    pub(super) fn path_picker_is_open(&self) -> bool {
        self.path_picker.open
    }

    pub(super) fn close_path_picker(&mut self, cx: &mut Context<Self>) {
        if !self.path_picker.open {
            return;
        }
        self.path_picker.open = false;
        self.path_picker.entries.clear();
        self.path_picker.selected_paths.clear();
        self.path_picker.input.clear();
        self.path_picker.completion = None;
        self.pending_focus = self.first_focusable_entity_id(cx);
        cx.notify();
    }

    pub(super) fn cancel_path_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(tx) = self.path_picker.completion.take() {
            let _ = tx.send(Ok(None));
        }
        self.close_path_picker(cx);
    }

    fn confirm_path_picker(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if let Some(tx) = self.path_picker.completion.take() {
            let _ = tx.send(Ok(Some(paths)));
        }
        self.close_path_picker(cx);
    }

    fn path_picker_sync_input_to_current_dir(&mut self, cx: &mut Context<Self>) {
        let path = self.path_picker.current_dir.to_string_lossy().to_string();
        self.path_picker.input.clear();
        self.path_picker.input.replace_text(0..0, &path);
        cx.notify();
    }

    fn path_picker_navigate_to(
        &mut self,
        path: &Path,
        select_name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let pick_files = self.path_picker.files;
        let (dir, file_name) = resolve_navigation_target(path, pick_files);
        if !dir.is_dir() {
            return;
        }

        let state = &mut self.path_picker;
        state.current_dir = dir;
        state.selected_paths.clear();
        refresh_path_picker_entries(state);

        if let Some(name) = select_name.or(file_name) {
            if let Some(index) = state.entries.iter().position(|entry| {
                matches!(
                    entry,
                    PathPickerEntry::File(path, label) if label == &name || path.file_name().is_some_and(|n| n.to_string_lossy() == name)
                )
            }) {
                state.selection = index;
                path_picker_scroll_to_selection(state);
            }
        }

        self.path_picker_sync_input_to_current_dir(cx);
    }

    fn path_picker_navigate_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let Some(path) = normalize_pasted_path(&text) else {
            return;
        };
        self.path_picker_navigate_to(&path, None, cx);
    }

    fn path_picker_navigate_from_input(&mut self, cx: &mut Context<Self>) {
        let query = self.path_picker.input.query.trim().to_string();
        if query.is_empty() {
            return;
        }
        if let Some(path) = normalize_pasted_path(&query) {
            self.path_picker_navigate_to(&path, None, cx);
            return;
        }
        let candidate = self.path_picker.current_dir.join(&query);
        if candidate.exists() {
            self.path_picker_navigate_to(&candidate, None, cx);
        }
    }

    fn path_picker_activate_selection(&mut self, cx: &mut Context<Self>) {
        let entry = self.path_picker.entries.get(self.path_picker.selection).cloned();
        let Some(entry) = entry else {
            return;
        };

        match entry {
            PathPickerEntry::Parent => {
                if let Some(parent) = self.path_picker.current_dir.parent() {
                    let parent = parent.to_path_buf();
                    self.path_picker.current_dir = parent;
                    refresh_path_picker_entries(&mut self.path_picker);
                    self.path_picker.selection = 0;
                    self.path_picker_sync_input_to_current_dir(cx);
                }
            }
            PathPickerEntry::Directory(path, _) => {
                if self.path_picker.directories && !self.path_picker.files {
                    self.confirm_path_picker(vec![path], cx);
                } else {
                    self.path_picker_navigate_to(&path, None, cx);
                }
            }
            PathPickerEntry::File(path, _) => {
                if self.path_picker.multiple {
                    if self.path_picker.selected_paths.contains(&path) {
                        self.path_picker.selected_paths.remove(&path);
                    } else {
                        self.path_picker.selected_paths.insert(path);
                    }
                    cx.notify();
                } else {
                    self.confirm_path_picker(vec![path], cx);
                }
            }
        }
    }

    fn path_picker_confirm_current(&mut self, cx: &mut Context<Self>) {
        if self.path_picker.multiple && !self.path_picker.selected_paths.is_empty() {
            let paths: Vec<_> = self.path_picker.selected_paths.iter().cloned().collect();
            self.confirm_path_picker(paths, cx);
            return;
        }

        if self.path_picker.directories && !self.path_picker.files {
            let path = self
                .path_picker
                .entries
                .get(self.path_picker.selection)
                .and_then(|entry| match entry {
                    PathPickerEntry::Directory(path, _) => Some(path.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| self.path_picker.current_dir.clone());
            self.confirm_path_picker(vec![path], cx);
            return;
        }

        let entry = self.path_picker.entries.get(self.path_picker.selection).cloned();
        match entry {
            Some(PathPickerEntry::File(path, _)) => self.confirm_path_picker(vec![path], cx),
            _ => {}
        }
    }

    fn path_picker_toggle_selection_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.path_picker.entries.get(index).cloned() else {
            return;
        };
        if let PathPickerEntry::File(path, _) = entry {
            if self.path_picker.selected_paths.contains(&path) {
                self.path_picker.selected_paths.remove(&path);
            } else {
                self.path_picker.selected_paths.insert(path);
            }
            self.path_picker.selection = index;
            cx.notify();
        }
    }

    pub(crate) fn on_path_picker_backdrop_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_path_picker(cx);
    }

    pub(crate) fn on_path_picker_confirm_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.path_picker_confirm_current(cx);
    }

    pub(crate) fn on_path_picker_cancel_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.cancel_path_picker(cx);
    }

    pub(super) fn path_picker_input_active(&self, window: &mut Window) -> bool {
        self.path_picker.open && self.path_picker.focus_handle.is_focused(window)
    }

    pub(super) fn path_picker_query_is_empty(&self) -> bool {
        self.path_picker.input.is_empty()
    }

    pub(super) fn path_picker_cursor_offset(&self) -> usize {
        self.path_picker.input.text_len()
    }

    pub(super) fn path_picker_display_text(&self, placeholder: &str) -> SharedString {
        if self.path_picker.input.is_empty() {
            SharedString::from(placeholder.to_string())
        } else {
            SharedString::from(self.path_picker.input.query.clone())
        }
    }

    pub(super) fn replace_path_picker_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        self.path_picker.input.replace_text(range, new_text);
        cx.notify();
    }

    pub(super) fn replace_path_picker_from_utf16(
        &mut self,
        range_utf16: Option<&Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.path_picker_input_active(window) {
            return false;
        }
        let len = self.path_picker.input.text_len();
        let range = range_utf16
            .map(|r| r.start.min(len)..r.end.min(len))
            .unwrap_or(len..len);
        self.replace_path_picker_text(range, new_text, cx);
        true
    }

    pub(super) fn path_picker_apply_arrow(&mut self, direction: isize, cx: &mut Context<Self>) {
        let state = &mut self.path_picker;
        if state.entries.is_empty() {
            return;
        }
        let count = state.entries.len() as isize;
        let new_index = (state.selection as isize + direction).rem_euclid(count) as usize;
        state.selection = new_index;
        path_picker_scroll_to_selection(state);
        cx.notify();
    }

    pub(crate) fn on_path_picker_navigate_paste(
        &mut self,
        _: &Paste,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_picker.open {
            return;
        }
        cx.stop_propagation();
        self.path_picker_navigate_from_clipboard(cx);
    }

    pub(super) fn on_path_picker_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_picker.open {
            return;
        }

        let modifiers = &event.keystroke.modifiers;
        if primary_shortcut_modifiers(modifiers) && event.keystroke.key.as_str() == "v" {
            self.path_picker_navigate_from_clipboard(cx);
            cx.stop_propagation();
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                self.cancel_path_picker(cx);
                cx.stop_propagation();
            }
            "enter" => {
                if self.path_picker_input_active(window) {
                    let typed = self.path_picker.input.query.trim();
                    let current = self.path_picker.current_dir.to_string_lossy();
                    if typed != current.as_ref() {
                        self.path_picker_navigate_from_input(cx);
                        cx.stop_propagation();
                        return;
                    }
                }
                self.path_picker_activate_selection(cx);
                cx.stop_propagation();
            }
            "backspace" => {
                if self.path_picker_input_active(window) {
                    if self.path_picker.input.delete_backward() {
                        cx.notify();
                    }
                    cx.stop_propagation();
                }
            }
            "up" => {
                self.path_picker_apply_arrow(-1, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.path_picker_apply_arrow(1, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    pub(super) fn render_path_picker_overlay(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.path_picker.open {
            return None;
        }

        let state = &self.path_picker;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let title = if state.title.is_empty() {
            if state.directories && !state.files {
                strings.open_folder_prompt.clone()
            } else {
                strings.open_markdown_files_prompt.clone()
            }
        } else {
            state.title.clone()
        };

        let row_height = Self::PATH_PICKER_ROW_HEIGHT;
        let row_gap = 2.0;
        let end = (state.scroll_top + Self::PATH_PICKER_MAX_VISIBLE_ROWS).min(state.entries.len());
        let mut rows = Vec::new();

        for index in state.scroll_top..end {
            let entry = &state.entries[index];
            let is_selected = index == state.selection;
            let (label, icon, path_for_action) = match entry {
                PathPickerEntry::Parent => (
                    strings.path_picker_parent.clone(),
                    PARENT_ICON.to_string(),
                    None,
                ),
                PathPickerEntry::Directory(path, name) => {
                    (name.clone(), FOLDER_ICON.to_string(), Some(path.clone()))
                }
                PathPickerEntry::File(path, name) => {
                    let icon = if path
                        .extension()
                        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("md"))
                    {
                        MARKDOWN_ICON
                    } else {
                        FILE_ICON
                    };
                    (name.clone(), icon.to_string(), Some(path.clone()))
                }
            };

            let is_checked = path_for_action
                .as_ref()
                .is_some_and(|path| state.selected_paths.contains(path));
            let row_bg = if is_selected {
                c.dialog_secondary_button_hover
            } else {
                Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 0.0,
                }
            };

            let open_editor = editor.clone();
            let entry_index = index;
            let entry_kind = entry.clone();

            rows.push(
                div()
                    .w_full()
                    .h(px(row_height))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(6.0))
                    .bg(row_bg)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                        let open_editor = open_editor.clone();
                        let entry_kind = entry_kind.clone();
                        let multi = event.modifiers.platform || event.modifiers.control;
                        let _ = open_editor.update(cx, |editor, cx| {
                            editor.path_picker.selection = entry_index;
                            if multi {
                                editor.path_picker_toggle_selection_at(entry_index, cx);
                                return;
                            }
                            match entry_kind {
                                PathPickerEntry::Parent => {
                                    editor.path_picker_activate_selection(cx);
                                }
                                PathPickerEntry::Directory(path, _) => {
                                    if editor.path_picker.directories && !editor.path_picker.files {
                                        editor.confirm_path_picker(vec![path], cx);
                                    } else {
                                        editor.path_picker_navigate_to(&path, None, cx);
                                    }
                                }
                                PathPickerEntry::File(path, _) => {
                                    if editor.path_picker.multiple {
                                        editor.path_picker_toggle_selection_at(entry_index, cx);
                                    } else {
                                        editor.confirm_path_picker(vec![path], cx);
                                    }
                                }
                            }
                        });
                    })
                    .child(
                        svg()
                            .path(icon)
                            .size(px(14.0))
                            .flex_shrink_0()
                            .text_color(c.dialog_body),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(px(t.text_size))
                            .text_color(c.dialog_body)
                            .child(label),
                    )
                    .when(is_checked, |this| {
                        this.child(
                            div()
                                .text_size(px(t.text_size * 0.85))
                                .text_color(c.dialog_muted)
                                .child("✓"),
                        )
                    })
                    .into_any_element(),
            );
        }

        let visible_height = ((Self::PATH_PICKER_MAX_VISIBLE_ROWS as f32)
            .min(state.entries.len() as f32)
            * (row_height + row_gap))
            .max(row_height);
        let total_height = state.entries.len() as f32 * (row_height + row_gap);
        let scroll_offset = state.scroll_top as f32 * (row_height + row_gap);

        let placeholder = SharedString::from(strings.path_picker_path_placeholder.clone());
        let focus_handle = state.focus_handle.clone();

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_path_picker_backdrop_mouse_down))
                .child(
                    div()
                        .w(px(Self::PATH_PICKER_PANEL_WIDTH))
                        .max_w(relative(1.0))
                        .flex()
                        .flex_col()
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.dialog_radius))
                        .shadow_lg()
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_event, _, cx| {
                            cx.stop_propagation();
                        })
                        .track_focus(&focus_handle)
                        .key_context("BlockEditor")
                        .on_key_down(cx.listener(Self::on_path_picker_key_down))
                        .on_action(cx.listener(Self::on_path_picker_navigate_paste))
                        .child(
                            div()
                                .px(px(d.dialog_padding))
                                .pt(px(d.dialog_padding))
                                .pb(px(8.0))
                                .text_size(px(t.dialog_title_size))
                                .font_weight(t.dialog_title_weight.to_font_weight())
                                .text_color(c.dialog_title)
                                .child(title),
                        )
                        .child(
                            div()
                                .px(px(d.dialog_padding))
                                .pb(px(8.0))
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(32.0))
                                        .px(px(8.0))
                                        .flex()
                                        .items_center()
                                        .rounded(px(6.0))
                                        .border(px(d.dialog_border_width))
                                        .border_color(c.dialog_border)
                                        .bg(c.dialog_secondary_button_bg)
                                        .child(
                                            SingleLineInputElement::new(
                                                cx.entity(),
                                                SingleLineInputTarget::PathPicker,
                                                placeholder,
                                            ),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(visible_height))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(total_height.max(visible_height)))
                                        .absolute()
                                        .top(px(-scroll_offset))
                                        .left_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(row_gap))
                                        .px(px(d.dialog_padding))
                                        .children(rows),
                                ),
                        )
                        .child(
                            div()
                                .px(px(d.dialog_padding))
                                .py(px(12.0))
                                .flex()
                                .justify_end()
                                .gap(px(d.dialog_button_gap))
                                .child(
                                    div()
                                        .h(px(d.dialog_button_height))
                                        .px(px(d.dialog_button_padding_x))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                        .border(px(d.dialog_border_width))
                                        .border_color(c.dialog_border)
                                        .bg(c.dialog_secondary_button_bg)
                                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px(t.dialog_button_size))
                                        .text_color(c.dialog_body)
                                        .child(strings.open_link_cancel.clone())
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::on_path_picker_cancel_mouse_down),
                                        ),
                                )
                                .child(
                                    div()
                                        .h(px(d.dialog_button_height))
                                        .px(px(d.dialog_button_padding_x))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                        .bg(c.dialog_primary_button_bg)
                                        .hover(|this| this.bg(c.dialog_primary_button_hover))
                                        .cursor_pointer()
                                        .text_size(px(t.dialog_button_size))
                                        .text_color(c.dialog_primary_button_text)
                                        .child(strings.open_link_open.clone())
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::on_path_picker_confirm_mouse_down),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn refresh_path_picker_entries(state: &mut PathPickerState) {
    let dir = state.current_dir.clone();
    let mut entries = Vec::new();
    if dir.parent().is_some() {
        entries.push(PathPickerEntry::Parent);
    }

    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if should_skip_workspace_entry_name(&name) {
                continue;
            }
            if path.is_dir() {
                if state.directories || state.files {
                    dirs.push((path, name));
                }
            } else if path.is_file() && state.files {
                files.push((path, name));
            }
        }
    }

    dirs.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    files.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

    for (path, name) in dirs {
        entries.push(PathPickerEntry::Directory(path, name));
    }
    for (path, name) in files {
        entries.push(PathPickerEntry::File(path, name));
    }

    state.entries = entries;
    if state.selection >= state.entries.len() {
        state.selection = state.entries.len().saturating_sub(1);
    }
    path_picker_scroll_to_selection(state);
}

fn path_picker_scroll_to_selection(state: &mut PathPickerState) {
    if state.selection < state.scroll_top {
        state.scroll_top = state.selection;
    } else if state.selection
        >= state.scroll_top + Editor::PATH_PICKER_MAX_VISIBLE_ROWS
    {
        state.scroll_top = state
            .selection
            .saturating_sub(Editor::PATH_PICKER_MAX_VISIBLE_ROWS - 1);
    }
}

#[cfg(target_os = "macos")]
fn primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    modifiers.platform
}

#[cfg(not(target_os = "macos"))]
fn primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    modifiers.control
}
