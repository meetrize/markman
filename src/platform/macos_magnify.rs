//! macOS trackpad pinch-to-zoom bridge for GPUI 0.2, which does not handle
//! `magnifyWithEvent:` on its `GPUIView` class.

use std::sync::Mutex;
use std::time::Duration;

use gpui::{App, AsyncApp};
use objc::runtime::{Class, Object};
use objc::{msg_send, sel, sel_impl};

use crate::editor::document_zoom::apply_magnification_to_active_editor;

static PENDING_MAGNIFICATIONS: Mutex<Vec<f32>> = Mutex::new(Vec::new());

fn push_magnification(delta: f32) {
    if let Ok(mut pending) = PENDING_MAGNIFICATIONS.lock() {
        pending.push(delta);
    }
}

fn drain_pending_magnifications() -> Vec<f32> {
    PENDING_MAGNIFICATIONS
        .lock()
        .map(|mut pending| pending.drain(..).collect())
        .unwrap_or_default()
}

extern "C" fn magnify_with_event(_this: &Object, _sel: objc::runtime::Sel, event: *mut Object) {
    let magnification: f64 = unsafe { msg_send![event, magnification] };
    push_magnification(magnification as f32);
}

fn install_gpuiview_magnify_handler() {
    let Some(class) = Class::get("GPUIView") else {
        eprintln!("GPUIView class not found; trackpad pinch zoom unavailable");
        return;
    };

    let added = unsafe {
        let sel = sel!(magnifyWithEvent:);
        let imp: objc::runtime::Imp =
            std::mem::transmute(magnify_with_event as *const () as *const std::ffi::c_void);
        objc::runtime::class_addMethod(
            class as *const Class as *mut Class,
            sel,
            imp,
            c"v@:@".as_ptr(),
        )
    };
    if !added {
        // Method may already be present if init runs more than once.
    }
}

pub fn start_magnify_pump(cx: &mut App) {
    install_gpuiview_magnify_handler();

    let background_executor = cx.background_executor().clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            background_executor.timer(Duration::from_millis(8)).await;
            let deltas = drain_pending_magnifications();
            if deltas.is_empty() {
                continue;
            }
            let combined = deltas.into_iter().sum::<f32>();
            let _ = cx.update(|app| apply_magnification_to_active_editor(app, combined));
        }
    })
    .detach();
}
