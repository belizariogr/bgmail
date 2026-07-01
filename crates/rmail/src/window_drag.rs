//! Portable helper to start an interactive window move (dragging the window by a
//! custom region such as the top toolbar).
//!
//! On Linux and Windows GPUI exposes `Window::start_window_move`, which is all we
//! need. On macOS GPUI 0.2 doesn't implement it and its content view consumes the
//! mouse events (so `movableByWindowBackground` never kicks in), so we fall back
//! to AppKit's `performWindowDragWithEvent:` using the native window handle.

use gpui::{Bounds, Pixels, Size, Window, WindowBounds};

/// Whether the window has reached its final size, so the deferred UI can lay out
/// (see `RootView::content_ready`). `actual` is the platform's real window size
/// and `viewport` is GPUI's cached drawable size; they match once GPUI has
/// processed the latest resize. When a maximized open is expected we also wait
/// for the OS to report the window as maximized, so the UI is never revealed at
/// the pre-maximize size.
// Only the Windows open sequence polls this at runtime; the test build exercises
// it directly. Other platforms compile but don't call it.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn window_layout_settled(
    expect_maximized: bool,
    is_maximized: bool,
    viewport: Size<Pixels>,
    actual: Size<Pixels>,
) -> bool {
    let synced = viewport == actual;
    synced && (!expect_maximized || is_maximized)
}

/// Re-posts the `WM_SIZE` the OS sends on resize so GPUI re-reads the real window
/// size into its cached viewport. Needed on Windows because the maximize-on-open
/// is applied asynchronously and that single `WM_SIZE` can be dropped during the
/// busy open sequence, leaving GPUI laid out at the small base size. Re-posting
/// it (with the current client rect) re-syncs GPUI without changing the window
/// state, so the window stays truly maximized. No-op on other platforms.
#[cfg(target_os = "windows")]
pub fn nudge_window_resize(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, IsZoomed, PostMessageW, SIZE_MAXIMIZED, SIZE_RESTORED, WM_SIZE,
    };

    // Fully qualified to disambiguate from GPUI's inherent `Window::window_handle`.
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut core::ffi::c_void);

    // SAFETY: `hwnd` is the live handle for `window`, used only synchronously
    // here. `GetClientRect`/`IsZoomed` are read-only queries and `PostMessageW`
    // merely re-queues the same `WM_SIZE` the OS itself posts on resize, so GPUI
    // re-reads the real client size into its viewport. No ownership is transferred
    // and nothing escapes this call.
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }
        let width = (rect.right - rect.left).max(1) as u32 & 0xFFFF;
        let height = (rect.bottom - rect.top).max(1) as u32 & 0xFFFF;
        let kind = if IsZoomed(hwnd).as_bool() {
            SIZE_MAXIMIZED
        } else {
            SIZE_RESTORED
        };
        let lparam = LPARAM(((height << 16) | width) as isize);
        let _ = PostMessageW(Some(hwnd), WM_SIZE, WPARAM(kind as usize), lparam);
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn nudge_window_resize(_window: &Window) {}

/// Hides or reveals the window without changing its layout, so the first paint
/// (and, on Windows, the asynchronous maximize) can happen off-screen and the
/// window only appears once fully rendered — avoiding the open flicker. Windows
/// uses DWM cloaking (invisible to the compositor while keeping its maximized
/// size/state, unlike `SW_HIDE`); macOS uses the window's `alphaValue` (still
/// composited and rendering, just transparent). No-op on other platforms.
#[cfg(target_os = "windows")]
pub fn set_window_cloaked(window: &Window, cloaked: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};

    // Fully qualified to disambiguate from GPUI's inherent `Window::window_handle`.
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut core::ffi::c_void);
    // `DWMWA_CLOAK` takes a `BOOL` (a 4-byte `i32`); 1 cloaks, 0 reveals.
    let value: i32 = cloaked as i32;

    // SAFETY: `hwnd` is the live handle for `window`, used only synchronously
    // here. `DwmSetWindowAttribute` reads `cbattribute` bytes from `pvattribute`
    // (a local `i32` we keep alive across the call) to toggle the cloak flag; it
    // transfers no ownership and nothing escapes this call.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            &value as *const i32 as *const core::ffi::c_void,
            core::mem::size_of::<i32>() as u32,
        );
    }
}

/// macOS implementation: toggle the window's `alphaValue` (0 hides, 1 reveals).
/// The window stays ordered front and keeps rendering while transparent, so when
/// we restore the alpha the finished content appears at once.
#[cfg(target_os = "macos")]
pub fn set_window_cloaked(window: &Window, cloaked: bool) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // Fully qualified to disambiguate from GPUI's inherent `Window::window_handle`.
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let alpha: f64 = if cloaked { 0.0 } else { 1.0 };

    // SAFETY: `ns_view` points to the window's live `NSView` (valid while the
    // window is open; touched only synchronously here). `-[NSView window]` and
    // `-[NSWindow setAlphaValue:]` transfer no ownership, so no retain/release
    // bookkeeping is required.
    unsafe {
        let ns_view = handle.ns_view.as_ptr() as *mut Object;
        let ns_window: *mut Object = msg_send![ns_view, window];
        if ns_window.is_null() {
            return;
        }
        let _: () = msg_send![ns_window, setAlphaValue: alpha];
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn set_window_cloaked(_window: &Window, _cloaked: bool) {}

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

#[cfg(test)]
mod tests {
    use super::window_layout_settled;
    use gpui::{px, size};

    #[test]
    fn settled_requires_viewport_to_match_window() {
        let stale = size(px(800.0), px(600.0));
        let real = size(px(1920.0), px(1040.0));
        // A viewport smaller than the real window means GPUI hasn't caught up yet.
        assert!(!window_layout_settled(false, false, stale, real));
        assert!(window_layout_settled(false, false, real, real));
    }

    #[test]
    fn settled_waits_for_maximize_when_expected() {
        let real = size(px(1920.0), px(1040.0));
        // Synced but not maximized yet: keep waiting when a maximized open is expected.
        assert!(!window_layout_settled(true, false, real, real));
        assert!(window_layout_settled(true, true, real, real));
        // No maximize expected: matching sizes is enough.
        assert!(window_layout_settled(false, false, real, real));
    }
}
