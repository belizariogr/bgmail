//! Portable helper to start an interactive window move (dragging the window by a
//! custom region such as the top toolbar).
//!
//! On Linux and Windows GPUI exposes `Window::start_window_move`, which is all we
//! need. On macOS GPUI 0.2 doesn't implement it and its content view consumes the
//! mouse events (so `movableByWindowBackground` never kicks in), so we fall back
//! to AppKit's `performWindowDragWithEvent:` using the native window handle.

use gpui::{Bounds, Pixels, Size, Window, WindowBounds};

/// Picks the `WindowBounds` to open the main window with.
///
/// On Windows/Linux, opening with `Maximized` while the window is still hidden
/// makes it appear maximized immediately (no flicker) with the OS maximized
/// status; `restored` is the frame to restore to. On macOS, GPUI's zoom is
/// asynchronous (and animates), so to avoid any restore-then-maximize flicker we
/// open windowed *directly at the saved maximized frame* (`maxed`); the drag
/// handler then restores to `restored` manually.
#[cfg(target_os = "macos")]
pub fn initial_window_bounds(
    restored: Bounds<Pixels>,
    maximized: bool,
    maxed: Bounds<Pixels>,
) -> WindowBounds {
    if maximized {
        WindowBounds::Windowed(maxed)
    } else {
        WindowBounds::Windowed(restored)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn initial_window_bounds(
    restored: Bounds<Pixels>,
    maximized: bool,
    _maxed: Bounds<Pixels>,
) -> WindowBounds {
    if maximized {
        WindowBounds::Maximized(restored)
    } else {
        WindowBounds::Windowed(restored)
    }
}

/// Begins dragging the window in response to the in-flight mouse interaction.
///
/// On Windows/Linux the OS already restores a maximized window when its title bar
/// is dragged, so `maximized`/`restore_size` are unused here.
#[cfg(not(target_os = "macos"))]
pub fn start_window_drag(window: &Window, _maximized: bool, _restore_size: Size<Pixels>) {
    window.start_window_move();
}

/// Foundation geometry structs (bottom-left origin, `CGFloat` == `f64` on
/// 64-bit macOS). Declared locally to avoid pulling in the `cocoa` crate.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

/// macOS implementation: ask AppKit to drag the window using the current event.
///
/// If the window is currently maximized, first restore it to `restore_size`,
/// repositioned so the cursor keeps the same relative spot on the title bar, then
/// hand off to AppKit's drag. This mirrors Windows, where dragging a maximized
/// window restores it. The restore is done with an explicit `setFrame` (no
/// `zoom:`), so it matches the size we persist and never relies on AppKit's
/// remembered zoom frame.
#[cfg(target_os = "macos")]
pub fn start_window_drag(window: &Window, maximized: bool, restore_size: Size<Pixels>) {
    use objc::runtime::{Object, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // Fully qualified to disambiguate from GPUI's inherent `Window::window_handle`.
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };

    // SAFETY: `ns_view` points to the window's live `NSView` (valid while the
    // window is open; we only touch it synchronously during event handling).
    // The AppKit messages used here (`-[NSView window]`, `-[NSWindow frame]`,
    // `+[NSEvent mouseLocation]`, `-[NSWindow setFrame:display:]`,
    // `+[NSApplication sharedApplication]`, `-[NSApplication currentEvent]` and
    // `-[NSWindow performWindowDragWithEvent:]`) transfer no ownership, so no
    // retain/release bookkeeping is required.
    unsafe {
        let ns_view = handle.ns_view.as_ptr() as *mut Object;
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }

        let restore_w = f32::from(restore_size.width) as f64;
        let restore_h = f32::from(restore_size.height) as f64;
        if maximized && restore_w > 0.0 && restore_h > 0.0 {
            // Current (maximized) frame and cursor, to keep the cursor at the same
            // relative position over the title bar after restoring.
            let maxed: NSRect = msg_send![ns_window, frame];
            let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            let rel_x = if maxed.size.width > 0.0 {
                (mouse.x - maxed.origin.x) / maxed.size.width
            } else {
                0.5
            };
            // Distance from the top edge, as a fraction of the height.
            let rel_top = if maxed.size.height > 0.0 {
                ((maxed.origin.y + maxed.size.height) - mouse.y) / maxed.size.height
            } else {
                0.0
            };

            let new_top = mouse.y + rel_top * restore_h;
            let new_frame = NSRect {
                origin: NSPoint {
                    x: mouse.x - rel_x * restore_w,
                    y: new_top - restore_h,
                },
                size: NSSize {
                    width: restore_w,
                    height: restore_h,
                },
            };
            let _: () = msg_send![ns_window, setFrame: new_frame display: YES];
        }

        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut Object = msg_send![app, currentEvent];
        if event.is_null() {
            return;
        }
        let _: () = msg_send![ns_window, performWindowDragWithEvent: event];
    }
}
