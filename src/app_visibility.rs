//! Global show/hide hotkey and application visibility toggling.

use std::collections::{BTreeMap, HashSet};

use gpui::*;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::HotKey,
};

use crate::components::{ShortcutCommand, resolved_shortcut_keys};

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
struct AppVisibilityState {
    hotkey_manager: GlobalHotKeyManager,
    registered_hotkeys: Vec<HotKey>,
    registered_hotkey_ids: HashSet<u32>,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl Global for AppVisibilityState {}

/// Register global visibility hotkeys from preferences and dispatch toggles immediately.
pub(crate) fn init(cx: &mut App, keybindings: &BTreeMap<String, Vec<String>>) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    register_visibility_hotkeys(cx, keybindings);
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = (cx, keybindings);
}

/// Re-register global visibility hotkeys after preferences change.
pub(crate) fn update_hotkeys(cx: &mut App, keybindings: &BTreeMap<String, Vec<String>>) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        if cx.try_global::<AppVisibilityState>().is_some() {
            let state = cx.global_mut::<AppVisibilityState>();
            let _ = state.hotkey_manager.unregister_all(&state.registered_hotkeys);
            state.registered_hotkeys.clear();
            state.registered_hotkey_ids.clear();
            register_hotkeys_into_state(state, keybindings);
            return;
        }
        register_visibility_hotkeys(cx, keybindings);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = (cx, keybindings);
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn register_visibility_hotkeys(cx: &mut App, keybindings: &BTreeMap<String, Vec<String>>) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(err) => {
            eprintln!("failed to initialize global visibility hotkey: {err}");
            return;
        }
    };

    let mut state = AppVisibilityState {
        hotkey_manager: manager,
        registered_hotkeys: Vec::new(),
        registered_hotkey_ids: HashSet::new(),
    };
    register_hotkeys_into_state(&mut state, keybindings);
    if state.registered_hotkeys.is_empty() {
        eprintln!("no valid global visibility hotkeys were registered");
        return;
    }

    cx.set_global(state);

    cx.spawn(async move |cx| {
        loop {
            let event = cx
                .background_executor()
                .spawn(async { GlobalHotKeyEvent::receiver().recv().ok() })
                .await;

            let Some(event) = event else {
                break;
            };
            if event.state != HotKeyState::Pressed {
                continue;
            }
            let should_toggle = cx
                .update(|cx| {
                    cx.global::<AppVisibilityState>()
                        .registered_hotkey_ids
                        .contains(&event.id)
                })
                .unwrap_or(false);
            if !should_toggle {
                continue;
            }
            let _ = cx.update(toggle_application_visibility);
        }
    })
    .detach();
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn register_hotkeys_into_state(
    state: &mut AppVisibilityState,
    keybindings: &BTreeMap<String, Vec<String>>,
) {
    let keys = resolved_shortcut_keys(keybindings, ShortcutCommand::ToggleApplicationVisibility);
    for key in keys {
        let Some(hotkey) = keystroke_to_hotkey(&key) else {
            eprintln!("failed to convert visibility hotkey '{key}'");
            continue;
        };
        if let Err(err) = state.hotkey_manager.register(hotkey) {
            eprintln!("failed to register global visibility hotkey '{key}': {err}");
            continue;
        }
        state.registered_hotkey_ids.insert(hotkey.id());
        state.registered_hotkeys.push(hotkey);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn keystroke_to_hotkey(key: &str) -> Option<HotKey> {
    let keystroke = Keystroke::parse(key).ok()?;
    let mut parts = Vec::new();
    if keystroke.modifiers.shift {
        parts.push("shift");
    }
    if keystroke.modifiers.control {
        parts.push("control");
    }
    if keystroke.modifiers.alt {
        parts.push("alt");
    }
    if keystroke.modifiers.platform {
        parts.push("super");
    }
    parts.push(keystroke.key.as_str());
    parts.join("+").parse().ok()
}

/// Hide the application when it is visible, or show and focus it when hidden.
pub(crate) fn toggle_application_visibility(cx: &mut App) {
    if application_is_hidden(cx) {
        show_application(cx);
    } else {
        hide_application(cx);
    }
}

fn show_application(cx: &mut App) {
    cx.activate(true);
    let window = cx
        .active_window()
        .or_else(|| cx.windows().last().copied());
    if let Some(window) = window {
        let _ = window.update(cx, |_, window, _| {
            window.activate_window();
        });
    }
}

fn hide_application(cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        cx.hide();
    }

    #[cfg(not(target_os = "macos"))]
    if let Some(window) = cx.active_window().or_else(|| cx.windows().last().copied()) {
        let _ = window.update(cx, |_, window, _| {
            window.minimize_window();
        });
    }
}

fn application_is_hidden(cx: &mut App) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = cx;
        return macos_application_is_hidden();
    }

    #[cfg(not(target_os = "macos"))]
    {
        cx.windows().is_empty()
            || cx.windows().iter().all(|window| {
                window
                    .update(cx, |_, window, _| !window.is_window_active())
                    .unwrap_or(true)
            })
    }
}

#[cfg(target_os = "macos")]
fn macos_application_is_hidden() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    NSApplication::sharedApplication(mtm).isHidden()
}
