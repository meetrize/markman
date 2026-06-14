//! Document zoom controls for the editor content area.

use std::time::Duration;

use gpui::*;

use crate::config::update_app_preferences;
use crate::theme::DocumentZoom;

use super::Editor;

const DOCUMENT_ZOOM_PERSIST_DEBOUNCE: Duration = Duration::from_millis(400);

/// Applies pinch magnification to the active window when its root view is an [`Editor`].
pub(crate) fn apply_magnification_to_active_editor(cx: &mut App, magnification_delta: f32) {
    if magnification_delta.abs() <= f32::EPSILON {
        return;
    }

    let Some(handle) = cx.active_window() else {
        return;
    };

    let factor = 1.0 + magnification_delta;
    let _ = cx.update_window(handle, move |view: AnyView, _window, cx| {
        if let Ok(entity) = view.downcast::<Editor>() {
            entity.update(cx, |editor, cx| {
                editor.set_document_zoom(editor.document_zoom * factor, cx);
            });
        }
    });
}

impl Editor {
    pub(super) fn set_document_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let zoom = DocumentZoom::clamp(zoom);
        if (self.document_zoom - zoom).abs() <= f32::EPSILON {
            return;
        }
        self.document_zoom = zoom;
        self.schedule_document_zoom_persist(cx);
        cx.notify();
    }

    fn schedule_document_zoom_persist(&mut self, cx: &mut Context<Self>) {
        self.document_zoom_persist_task = None;
        let weak_editor = cx.entity().downgrade();
        self.document_zoom_persist_task = Some(cx.spawn(
            async move |_this: WeakEntity<Editor>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(DOCUMENT_ZOOM_PERSIST_DEBOUNCE)
                    .await;
                let _ = weak_editor.update(cx, |editor, _cx| {
                    editor.document_zoom_persist_task = None;
                    let zoom_x100 = DocumentZoom::zoom_x100_from_multiplier(editor.document_zoom);
                    let _ = update_app_preferences(|preferences| {
                        preferences.document_zoom_x100 = zoom_x100;
                    });
                });
            },
        ));
    }

    pub(super) fn zoom_document_in(&mut self, cx: &mut Context<Self>) {
        self.set_document_zoom(DocumentZoom::zoom_in(self.document_zoom), cx);
    }

    pub(super) fn zoom_document_out(&mut self, cx: &mut Context<Self>) {
        self.set_document_zoom(DocumentZoom::zoom_out(self.document_zoom), cx);
    }

    pub(super) fn on_zoom_document_in_click(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_document_in(cx);
    }

    pub(super) fn on_zoom_document_out_click(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_document_out(cx);
    }

    pub(super) fn scroll_wheel_requests_document_zoom(event: &ScrollWheelEvent) -> bool {
        // macOS trackpad pinch is handled via `magnifyWithEvent:` (see platform/macos_magnify.rs).
        // Ctrl+scroll (and some mice) still arrive as scroll wheel events.
        event.modifiers.control
            || (event.modifiers.platform && event.delta.precise() && !event.modifiers.shift)
    }

    pub(super) fn apply_document_zoom_from_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let line_height = px(16.0);
        let delta = event.delta.pixel_delta(line_height);
        let factor = DocumentZoom::factor_from_pinch_delta_y(f32::from(delta.y));
        self.set_document_zoom(self.document_zoom * factor, cx);
    }
}
