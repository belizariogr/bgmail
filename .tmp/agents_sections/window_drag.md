### `src/window_drag.rs`

#### Types / constants

- **`NSPoint`** (private, L200)
  - Signature: `struct NSPoint { x: f64, y: f64 }` (`#[cfg(target_os = "macos")]`)
  - Purpose: Local Foundation point struct for macOS window drag math.
  - Behavior: Used in `start_window_drag` frame calculations without pulling in `cocoa`.

- **`NSSize`** (private, L208)
  - Signature: `struct NSSize { width: f64, height: f64 }` (macOS)
  - Purpose: Local Foundation size struct for macOS restore geometry.
  - Behavior: Paired with `NSPoint` inside `NSRect`.

- **`NSRect`** (private, L216)
  - Signature: `struct NSRect { origin: NSPoint, size: NSSize }` (macOS)
  - Purpose: Local Foundation rect for explicit maximized-to-restored frame placement.
  - Behavior: Passed to `-[NSWindow setFrame:display:]`.

#### Functions / methods

##### Context: `module`

- **`window_layout_settled`** (pub, L20)
  - Signature: `pub fn window_layout_settled(expect_maximized: bool, is_maximized: bool, viewport: Size<Pixels>, actual: Size<Pixels>) -> bool`
  - Purpose: Decides when deferred UI (`RootView::content_ready`) may reveal the window.
  - Behavior: Requires GPUI viewport size to match platform window size; when a maximized open is expected, also waits until `is_maximized` is true.

- **`nudge_window_resize`** (pub, L37)
  - Signature: `pub fn nudge_window_resize(window: &Window)` (Windows)
  - Purpose: Re-syncs GPUI viewport after async maximize-on-open.
  - Behavior: Reads Win32 client rect and zoom state, then re-posts `WM_SIZE` with current dimensions so GPUI picks up the maximized client size without changing window state.

- **`nudge_window_resize`** (pub, L77)
  - Signature: `pub fn nudge_window_resize(_window: &Window)` (non-Windows)
  - Purpose: Portable no-op stub for the resize nudge API.
  - Behavior: Empty body on platforms that do not need WM_SIZE re-posting.

- **`set_window_cloaked`** (pub, L86)
  - Signature: `pub fn set_window_cloaked(window: &Window, cloaked: bool)` (Windows)
  - Purpose: Hides window visually during first paint without altering layout state.
  - Behavior: Sets DWM cloak attribute via `DwmSetWindowAttribute(DWMWA_CLOAK)` so compositor skips the window while it stays maximized/rendering off-screen.

- **`set_window_cloaked`** (pub, L120)
  - Signature: `pub fn set_window_cloaked(window: &Window, cloaked: bool)` (macOS)
  - Purpose: Hides window visually during first paint on macOS.
  - Behavior: Sets `NSWindow` alpha to 0 when cloaked and 1 when revealed; window keeps rendering while transparent.

- **`set_window_cloaked`** (pub, L150)
  - Signature: `pub fn set_window_cloaked(_window: &Window, _cloaked: bool)` (other platforms)
  - Purpose: Portable no-op for cloak API on Linux etc.
  - Behavior: Empty body.

- **`initial_window_bounds`** (pub, L161)
  - Signature: `pub fn initial_window_bounds(restored: Bounds<Pixels>, maximized: bool, maxed: Bounds<Pixels>) -> WindowBounds` (macOS)
  - Purpose: Chooses open bounds avoiding restore-then-maximize flicker on macOS.
  - Behavior: When maximized, opens windowed directly at saved maximized frame (`maxed`); otherwise uses `restored`.

- **`initial_window_bounds`** (pub, L174)
  - Signature: `pub fn initial_window_bounds(restored: Bounds<Pixels>, maximized: bool, _maxed: Bounds<Pixels>) -> WindowBounds` (non-macOS)
  - Purpose: Chooses open bounds for Windows/Linux maximize behavior.
  - Behavior: Returns `WindowBounds::Maximized(restored)` when maximized, else `WindowBounds::Windowed(restored)`.

- **`start_window_drag`** (pub, L191)
  - Signature: `pub fn start_window_drag(window: &Window, _maximized: bool, _restore_size: Size<Pixels>)` (non-macOS)
  - Purpose: Begins interactive window move from a custom drag region.
  - Behavior: Delegates to GPUI `window.start_window_move()`; OS restores maximized windows automatically when dragging.

- **`start_window_drag`** (pub, L230)
  - Signature: `pub fn start_window_drag(window: &Window, maximized: bool, restore_size: Size<Pixels>)` (macOS)
  - Purpose: AppKit-backed window drag when GPUI move is unavailable.
  - Behavior: When maximized with valid restore size, computes new frame keeping cursor relative position on title bar, applies `setFrame`, then calls `performWindowDragWithEvent:` on the current event.

- **`settled_requires_viewport_to_match_window`** (private, L305)
  - Signature: `fn settled_requires_viewport_to_match_window()` (test)
  - Purpose: Tests viewport/actual size matching requirement.
  - Behavior: Smaller viewport than actual returns false; equal sizes return true when maximize not expected.

- **`settled_waits_for_maximize_when_expected`** (private, L314)
  - Signature: `fn settled_waits_for_maximize_when_expected()` (test)
  - Purpose: Tests maximize wait logic in `window_layout_settled`.
  - Behavior: When `expect_maximized`, requires `is_maximized` even if sizes already match.
