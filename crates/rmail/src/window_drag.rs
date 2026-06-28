//! Portable helper to start an interactive window move (dragging the window by a
//! custom region such as the top toolbar).
//!
//! On Linux and Windows GPUI exposes `Window::start_window_move`, which is all we
//! need. On macOS GPUI 0.2 doesn't implement it and its content view consumes the
//! mouse events (so `movableByWindowBackground` never kicks in), so we fall back
//! to AppKit's `performWindowDragWithEvent:` using the native window handle.

use gpui::Window;

/// Begins dragging the window in response to the in-flight mouse interaction.
#[cfg(not(target_os = "macos"))]
pub fn start_window_drag(window: &Window) {
    window.start_window_move();
}

/// macOS implementation: ask AppKit to drag the window using the current event.
#[cfg(target_os = "macos")]
pub fn start_window_drag(_window: &Window) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // Fully qualified to disambiguate from GPUI's inherent `Window::window_handle`.
    let Ok(handle) = HasWindowHandle::window_handle(_window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };

    // SAFETY: `ns_view` points to the window's live `NSView` (valid while the
    // window is open; we only touch it synchronously during event handling).
    // `-[NSView window]`, `+[NSApplication sharedApplication]`,
    // `-[NSApplication currentEvent]` and `-[NSWindow performWindowDragWithEvent:]`
    // are standard AppKit messages that transfer no ownership, so no retain/
    // release bookkeeping is required.
    unsafe {
        let ns_view = handle.ns_view.as_ptr() as *mut Object;
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut Object = msg_send![app, currentEvent];
        if event.is_null() {
            return;
        }
        let _: () = msg_send![ns_window, performWindowDragWithEvent: event];
    }
}
