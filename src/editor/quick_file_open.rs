//! Quick-file-open overlay (Command+P / Ctrl+P) that fuzzy-matches files
//! in the current workspace root and opens the selected file.
//!
//! The overlay presents a centred modal with a search input and a scrollable
//! list of ranked results. Arrow keys move the highlight; Enter opens the file;
//! Escape dismisses the overlay.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::*;

use super::Editor;
use super::single_line_input::SingleLineInputTarget;
use super::single_line_input_element::SingleLineInputElement;
use crate::components::QuickFileOpen;
use crate::config::read_recent_files;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

/// State for the quick-file-open modal overlay.
#[derive(Clone, Debug)]
pub(super) struct QuickFileOpenState {
    /// Whether the overlay is currently shown.
    pub(super) open: bool,
    /// Current search query entered by the user.
    pub(super) query: String,
    /// All files in the workspace root (collected once on opening).
    pub(super) all_files: Vec<PathBuf>,
    /// Files filtered and ranked by the current query.
    pub(super) results: Vec<QuickFileOpenResult>,
    /// Index of the currently highlighted result (0-based).
    pub(super) selection: usize,
    /// Width of the overlay panel in pixels.
    pub(super) panel_width: f32,
    /// Maximum number of visible result rows before scrolling.
    pub(super) max_visible_rows: usize,
    /// Scroll offset into the result list.
    pub(super) scroll_top: usize,
    /// Focus handle for the search input.
    pub(super) focus_handle: FocusHandle,
}

/// A single matched file in the quick-open results list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct QuickFileOpenResult {
    pub(super) path: PathBuf,
    /// Display label (file name relative to workspace root).
    pub(super) label: String,
    /// Parent-directory breadcrumb shown as muted detail.
    pub(super) detail: String,
    /// Match score for ranking (lower = better).
    pub(super) score: usize,
}

impl QuickFileOpenState {
    pub(super) fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            open: false,
            query: String::new(),
            all_files: Vec::new(),
            results: Vec::new(),
            selection: 0,
            panel_width: 560.0,
            max_visible_rows: 12,
            scroll_top: 0,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl Editor {
    const QUICK_FILE_OPEN_ROW_HEIGHT: f32 = 32.0;
    const QUICK_FILE_OPEN_PANEL_BG: Hsla = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.94,
        a: 1.0,
    };
    const QUICK_FILE_OPEN_INPUT_BG: Hsla = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.90,
        a: 1.0,
    };
    const QUICK_FILE_OPEN_ROW_HOVER: Hsla = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.82,
        a: 1.0,
    };
    const QUICK_FILE_OPEN_BORDER: Hsla = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.80,
        a: 1.0,
    };
    const QUICK_FILE_OPEN_TEXT: Hsla = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.15,
        a: 1.0,
    };
    const QUICK_FILE_OPEN_TEXT_MUTED: Hsla = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.45,
        a: 1.0,
    };

    /// Action handler for Command+P / Ctrl+P.
    pub(super) fn on_quick_file_open(
        &mut self,
        _: &QuickFileOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quick_file_open.open {
            self.close_quick_file_open(cx);
        } else {
            self.open_quick_file_open(window, cx);
        }
        cx.stop_propagation();
    }

    /// Opens the quick-file-open overlay and populates the file list from
    /// the current workspace root (or the current file's parent directory).
    pub(super) fn open_quick_file_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_contextual_overlays(cx);
        let root = self.effective_workspace_root();

        // Load recent files for when query is empty.
        let recent = read_recent_files().unwrap_or_default();
        let recent: Vec<PathBuf> = recent.into_iter().take(10).collect();

        // Collect all files (not just Markdown) in the workspace root.
        let all_files = collect_all_files(root.as_deref());

        let state = &mut self.quick_file_open;
        state.open = true;
        state.query.clear();
        state.all_files = all_files;

        // When query is empty, show recent files at top.
        let root_ref = root.as_deref();
        state.results = recent
            .iter()
            .map(|path| QuickFileOpenResult {
                path: path.clone(),
                label: file_display_label(root_ref, path),
                detail: file_parent_label(root_ref, path),
                score: 0,
            })
            .collect();
        state.selection = 0;
        state.scroll_top = 0;
        window.focus(&self.quick_file_open.focus_handle);
        cx.notify();
    }

    /// Whether the quick-file-open search input is currently active.
    pub(super) fn quick_file_open_input_active(&self, window: &mut Window) -> bool {
        self.quick_file_open.open && self.quick_file_open.focus_handle.is_focused(window)
    }

    /// Whether the search query is currently empty.
    pub(super) fn quick_file_open_query_is_empty(&self) -> bool {
        self.quick_file_open.query.is_empty()
    }

    /// Returns the current search query string.
    pub(super) fn quick_file_open_query(&self) -> &str {
        &self.quick_file_open.query
    }

    /// Returns the display text for the input element.
    pub(super) fn quick_file_open_display_text(&self, placeholder: &str) -> SharedString {
        if self.quick_file_open.query.is_empty() {
            SharedString::from(placeholder.to_string())
        } else {
            SharedString::from(self.quick_file_open.query.clone())
        }
    }

    /// Replaces text in the quick-file-open search query.
    pub(super) fn replace_quick_file_open_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.quick_file_open;
        let end = range.end.min(state.query.len());
        let start = range.start.min(end);
        state.query.replace_range(start..end, new_text);
        self.refresh_quick_file_open_results(cx);
    }
    /// Closes the quick-file-open overlay.
    pub(super) fn close_quick_file_open(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if !self.quick_file_open.open {
            return;
        }
        self.quick_file_open.open = false;
        self.quick_file_open.query.clear();
        self.quick_file_open.results.clear();
        self.quick_file_open.all_files.clear();
        self.pending_focus = self.first_focusable_entity_id(cx);
        
        cx.notify();
    }

    /// Runs the fuzzy search and updates the result list.
    fn refresh_quick_file_open_results(&mut self, cx: &mut Context<Self>) {
        // Take a copy of the query before borrowing self.quick_file_open mutably.
        let query = self.quick_file_open.query.trim().to_string();
        let all_files = self.quick_file_open.all_files.clone();
        let root = self.effective_workspace_root();

        let results: Vec<QuickFileOpenResult> = if query.is_empty() {
            // When query is empty, show recent files.
            let recent = read_recent_files().unwrap_or_default();
            let root_ref = root.as_deref();
            recent
                .into_iter()
                .take(10)
                .map(|path| QuickFileOpenResult {
                    path: path.clone(),
                    label: file_display_label(root_ref, &path),
                    detail: file_parent_label(root_ref, &path),
                    score: 0,
                })
                .collect()
        } else {
            let query_lower = query.to_lowercase();
            let mut scored: Vec<(usize, QuickFileOpenResult)> = all_files
                .iter()
                .filter_map(|path| {
                    let label = file_display_label(root.as_deref(), path);
                    let label_lower = label.to_lowercase();
                    let detail = file_parent_label(root.as_deref(), path);

                    // Score: full-path match (for subdir filtering)
                    let path_str = path.to_string_lossy().to_lowercase();
                    let score = fuzzy_match_score(&path_str, &query_lower);

                    // Boost filename-only matches
                    let name_score = fuzzy_match_score(&label_lower, &query_lower);
                    let final_score = if name_score < score || score >= usize::MAX / 2 {
                        name_score
                    } else {
                        score
                    };

                    Some((
                        final_score,
                        QuickFileOpenResult {
                            path: path.clone(),
                            label,
                            detail,
                            score: final_score,
                        },
                    ))
                })
                .collect();

            scored.sort_by(|(s1, r1), (s2, r2)| {
                s1.cmp(s2).then_with(|| r1.label.cmp(&r2.label))
            });

            scored.into_iter().map(|(_, r)| r).collect()
        };

        let state = &mut self.quick_file_open;
        state.results = results;
        state.selection = if state.results.is_empty() { 0 } else { 0 };
        state.scroll_top = 0;
        cx.notify();
    }

    /// Apply arrow key navigation in the quick-file-open results.
    pub(super) fn quick_file_open_apply_arrow(
        &mut self,
        direction: isize,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.quick_file_open;
        if state.results.is_empty() {
            return;
        }
        let count = state.results.len() as isize;
        let new_index = (state.selection as isize + direction).rem_euclid(count) as usize;
        state.selection = new_index;

        // Scroll to keep the selection visible.
        if new_index < state.scroll_top {
            state.scroll_top = new_index;
        } else if new_index >= state.scroll_top + state.max_visible_rows {
            state.scroll_top = new_index.saturating_sub(state.max_visible_rows - 1);
        }
        cx.notify();
    }

    /// Opens the currently selected file in the results.
    pub(super) fn quick_file_open_accept(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &self.quick_file_open;
        let Some(result) = state.results.get(state.selection) else {
            return;
        };
        let path = result.path.clone();
        self.close_quick_file_open(cx);
        self.open_workspace_file(path, window, cx);
        cx.notify();
    }

    /// Handles key-down events when the quick-file-open input is focused.
    pub(super) fn on_quick_file_open_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.quick_file_open.open || !self.quick_file_open.focus_handle.is_focused(window) {
            return;
        }

        let modifiers = &event.keystroke.modifiers;
        if primary_shortcut_modifiers(modifiers) {
            if event.keystroke.key.as_str() == "v" {
                self.quick_file_open_paste_from_clipboard(cx);
                cx.stop_propagation();
                return;
            }
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_quick_file_open(cx);
                cx.stop_propagation();
            }
            "enter" => {
                self.quick_file_open_accept(window, cx);
                cx.stop_propagation();
            }
            "backspace" => {
                self.quick_file_open_delete_backward(cx);
                cx.stop_propagation();
            }
            "delete" => {
                self.quick_file_open_delete_forward(cx);
                cx.stop_propagation();
            }
            "up" => {
                self.quick_file_open_apply_arrow(-1, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.quick_file_open_apply_arrow(1, cx);
                cx.stop_propagation();
            }
            _ => {
                // Text input is handled by EntityInputHandler.
            }
        }
    }

    fn quick_file_open_paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text: String = text
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c: &char| !c.is_control() || *c == ' ')
                .collect();
            if !text.is_empty() {
                let end = self.quick_file_open.query.len();
                self.replace_quick_file_open_text(end..end, &text, cx);
            }
        }
    }

    fn quick_file_open_delete_backward(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.quick_file_open;
        if !state.query.is_empty() {
            state.query.pop();
            self.refresh_quick_file_open_results(cx);
        }
    }

    fn quick_file_open_delete_forward(&mut self, _cx: &mut Context<Self>) {
        // Single-line input — delete forward is a no-op at end.
    }

    /// Renders the quick-file-open overlay.
    pub(super) fn render_quick_file_open_overlay(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.quick_file_open.open {
            return None;
        }

        let state = &self.quick_file_open;
        let t = &theme.typography;
        let d = &theme.dimensions;
        let editor = cx.entity().downgrade();

        let panel_width = state.panel_width;
        let result_row_height = Self::QUICK_FILE_OPEN_ROW_HEIGHT;
        let results_px = 8.0;
        let results_py = 4.0;

        // Build visible result rows (with virtual scrolling).
        let mut result_rows = Vec::new();
        let end = (state.scroll_top + state.max_visible_rows).min(state.results.len());
        for index in state.scroll_top..end {
            let result = &state.results[index];
            let is_selected = index == state.selection;
            let path = result.path.clone();
            let open_editor = editor.clone();
            let label = result.label.clone();
            let detail = result.detail.clone();

            let row_bg = if is_selected {
                Self::QUICK_FILE_OPEN_ROW_HOVER
            } else {
                Hsla {
                    h: 0.0,
                    s: 0.0,
                    l: 0.0,
                    a: 0.0,
                }
            };

            result_rows.push(
                div()
                    .w_full()
                    .h(px(result_row_height))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(px(6.0))
                    .bg(row_bg)
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                        let open_path = path.clone();
                        let _ = open_editor.update(cx, |editor, cx| {
                            editor.close_quick_file_open(cx);
                            editor.open_workspace_file(open_path, window, cx);
                        });
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(px(t.text_size))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(Self::QUICK_FILE_OPEN_TEXT)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(t.text_size * 0.78))
                            .text_color(Self::QUICK_FILE_OPEN_TEXT_MUTED)
                            .child(detail),
                    )
                    .into_any_element(),
            );
        }

        let total_height = state.results.len() as f32 * (result_row_height + results_py);
        let visible_results_height =
            ((state.max_visible_rows as f32).min(state.results.len() as f32) * (result_row_height + results_py))
                .max(0.0);
        let scroll_offset = state.scroll_top as f32 * (result_row_height + results_py);

        // Placeholder text.
        let placeholder: &str = if state.query.is_empty() {
            &strings.quick_file_open_placeholder
        } else if state.results.is_empty() {
            &strings.workspace_search_no_results
        } else {
            ""
        };
        let placeholder_owned: SharedString = SharedString::from(placeholder.to_string());

        let search_focus = state.focus_handle.clone();
        let close_editor = editor.clone();

        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_start()
                .justify_center()
                .pt(px(120.0))
                .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                    let _ = close_editor.update(cx, |editor, cx| {
                        editor.close_quick_file_open(cx);
                    });
                })
                .child(
                    div()
                        .w(px(panel_width))
                        .flex()
                        .flex_col()
                        .bg(Self::QUICK_FILE_OPEN_PANEL_BG)
                        .rounded(px(d.dialog_radius))
                        .shadow_md()
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_event, _, cx| {
                            cx.stop_propagation();
                        })
                        // Search input row.
                        .child(
                            div()
                                .w_full()
                                .px(px(results_px))
                                .pt(px(8.0))
                                .pb(px(6.0))
                                .border_b_1()
                                .border_color(Self::QUICK_FILE_OPEN_BORDER)
                                .track_focus(&search_focus)
                                .key_context("BlockEditor")
                                .on_key_down(cx.listener(Self::on_quick_file_open_key_down))
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(32.0))
                                        .min_w(px(0.0))
                                        .overflow_hidden()
                                        .px(px(8.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .rounded(px(4.0))
                                        .border(px(1.0))
                                        .border_color(Self::QUICK_FILE_OPEN_BORDER)
                                        .bg(Self::QUICK_FILE_OPEN_INPUT_BG)
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.0))
                                                .h_full()
                                                .overflow_hidden()
                                                .child(SingleLineInputElement::new(
                                                    cx.entity(),
                                                    SingleLineInputTarget::QuickFileOpen,
                                                    placeholder_owned.clone(),
                                                )),
                                        ),
                                ),
                        )
                        // Results area.
                        .child(
                            div()
                                .w_full()
                                .h(px(visible_results_height))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(total_height))
                                        .absolute()
                                        .top(px(-scroll_offset))
                                        .left_0()
                                        .flex()
                                        .flex_col()
                                        .gap(px(results_py))
                                        .px(px(results_px))
                                        .children(result_rows),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Recursively collects all regular files under `root`, sorted.
fn collect_all_files(root: Option<&Path>) -> Vec<PathBuf> {
    let Some(root) = root else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip hidden files/dirs and system files.
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        if file_name.starts_with('.') || file_name == "node_modules" || file_name == "target" {
            continue;
        }
        if path.is_dir() {
            // Shallow: only go one level deep for common project patterns.
            paths.extend(collect_files_shallow(&path));
        } else if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

/// Collects only files (no subdirectories) from a directory, skipping
/// hidden and common ignorable directories.
fn collect_files_shallow(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        if file_name.starts_with('.') || file_name == "node_modules" || file_name == "target" {
            continue;
        }
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

/// Produces a display label relative to the workspace root.
fn file_display_label(root: Option<&Path>, path: &Path) -> String {
    if let Some(root) = root {
        path.strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }
}

/// Produces a parent-directory breadcrumb relative to the workspace root.
fn file_parent_label(root: Option<&Path>, path: &Path) -> String {
    if let Some(parent) = path.parent() {
        if let Some(root) = root {
            if parent == root {
                return String::new();
            }
            parent
                .strip_prefix(root)
                .unwrap_or(parent)
                .to_string_lossy()
                .into_owned()
        } else {
            parent
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    } else {
        String::new()
    }
}

/// Simple fuzzy match scoring.
/// Returns a score where 0 is a perfect prefix match and higher values
/// are worse matches. Returns `usize::MAX` for no match.
fn fuzzy_match_score(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }

    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();

    let mut ni = 0;
    let mut hi = 0;
    let mut score = 0usize;
    let mut last_match = None;

    while ni < needle.len() && hi < haystack.len() {
        if haystack[hi] == needle[ni] {
            // Bonus for consecutive matches and separator transitions.
            if let Some(lm) = last_match {
                if hi == lm + 1 {
                    // Consecutive match bonus — reduce score.
                    score = score.saturating_sub(10);
                }
            } else {
                // First match — penalize position (later = worse).
                score += hi;
            }
            // Bonus if match is at a separator boundary.
            if hi == 0
                || haystack[hi - 1] == b'/'
                || haystack[hi - 1] == b'_'
                || haystack[hi - 1] == b'-'
                || haystack[hi - 1] == b'.'
            {
                score = score.saturating_sub(20);
            }
            last_match = Some(hi);
            ni += 1;
        } else {
            // Gap penalty.
            score += 1;
        }
        hi += 1;
    }

    if ni == needle.len() {
        score
    } else {
        usize::MAX
    }
}

/// Utility to check for primary shortcut modifiers (cmd on macOS, ctrl otherwise).
#[cfg(target_os = "macos")]
fn primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    modifiers.platform
}

#[cfg(not(target_os = "macos"))]
fn primary_shortcut_modifiers(modifiers: &Modifiers) -> bool {
    modifiers.control
}
