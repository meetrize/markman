//! Global show/hide hotkey and application visibility toggling.

use gpui::*;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
struct AppVisibilityState {
    _hotkey_manager: GlobalHotKeyManager,
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl Global for AppVisibilityState {}

/// Register the global ⌘⇧⌥7 hotkey and dispatch visibility toggles immediately.
pub(crate) fn init(cx: &mut App) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    register_visibility_hotkey(cx);
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn register_visibility_hotkey(cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(err) => {
            eprintln!("failed to initialize global visibility hotkey: {err}");
            return;
        }
    };

    let hotkey = HotKey::new(
        Some(Modifiers::SUPER | Modifiers::SHIFT | Modifiers::ALT),
        Code::Digit7,
    );
    let hotkey_id = hotkey.id();
    if let Err(err) = manager.register(hotkey) {
        eprintln!("failed to register global visibility hotkey: {err}");
        return;
    }

    cx.set_global(AppVisibilityState {
        _hotkey_manager: manager,
    });

    cx.spawn(async move |cx| {
        loop {
            let event = cx
                .background_executor()
                .spawn(async { GlobalHotKeyEvent::receiver().recv().ok() })
                .await;

            let Some(event) = event else {
                break;
            };
            if event.id != hotkey_id || event.state != HotKeyState::Pressed {
                continue;
            }
            let _ = cx.update(toggle_application_visibility);
        }
    })
    .detach();
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
                    .update(cx, |_, window, _| !window.is_active(cx).unwrap_or(false))
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
