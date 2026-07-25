# crates/bgmail/src/ — Source API

## Purpose

Source for the `bgmail` binary: app entry, views, webview/CEF, locale, config, and OS helpers.

## Ownership

- Owns: All modules under `crates/bgmail/src/`.
- Does not own: crate-level dependency/feature policy (parent `.dox/crates/bgmail/AGENTS.md`).

## Local Contracts

- Every function/method in this tree is listed under **API Catalog** with purpose
  and behavior. Update the matching entry when changing signatures or semantics.
- Prefer rustdoc on public items; DOX purpose/behavior should stay aligned with
  rustdoc.
- Visibility in entries (`pub` / `private`) reflects the source item.

## Work Guidance

- After adding/removing/renaming a function, update this catalog in the same
  change.
- Do not weaken parent DOX contracts from root/`crates/`/`bgmail/`.

## Verification

- `cargo test -p bgmail`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

_624 functions/methods documented._

### `src/actions.rs`

#### Types / constants

- **actions `Quit`, `ToggleCommandPalette`, `ComposeNew`, `OpenSettings`, `ToggleSidebar`, `MessageDelete`, `MessageDeletePermanent`, `MessageRestore`, `MessageArchive`, `MessageMarkJunk`, `MessageToggleFlag`, `ComposeSend`, `ComposeAttach`, `ComposeDiscard`, `ComposeClose`**: GPUI action types declared via the `actions!` macro under the `bgmail` namespace. Each is a zero-sized dispatch token wired to menus, key bindings, and `cx.on_action` handlers in `main.rs`.
- **struct `MoveMessageToFolder`**: Parameterized action carrying a folder storage path (`path: SharedString`). Dispatched when the user picks a move target from menus or the command palette.

#### Functions / methods

_(No functions — only action types.)_

### `src/app_menus.rs`

#### Types / constants

- **`MenuSurface`** (pub, L25)
  - Signature: `pub enum MenuSurface { Main, Compose }`
  - Purpose: Identifies which window's command set should drive the global menu bar.
  - Behavior: `Main` reflects the mail reader (File, Edit, View, Message menus). `Compose` replaces those with compose-specific File/Edit menus while a compose window is key.

- **`ActiveMenuSurface`** (pub, L32)
  - Signature: `pub struct ActiveMenuSurface(pub MenuSurface)`
  - Purpose: GPUI global that records the menu layout last installed by a sync function.
  - Behavior: Stored via `cx.set_global` when `sync_main_menus` or `sync_compose_menus` runs. Read by `active_menu_surface`, defaulting to `Main` when unset.

#### Functions / methods

##### Context: `module`

- **`app_menu`** (private, L36)
  - Signature: `fn app_menu() -> Menu`
  - Purpose: Builds the application menu (macOS app menu / first menu slot).
  - Behavior: Returns a menu named "BGMail" with About (no-op), Services OS submenu, separator, and Quit bound to `Quit`.

- **`edit_menu`** (private, L49)
  - Signature: `fn edit_menu() -> Menu`
  - Purpose: Builds the standard Edit menu shared by main and compose surfaces.
  - Behavior: Wires Undo, Redo, Cut, Copy, Paste, and Select All to GPUI OS actions (`OsAction::*`) with `NoAction` placeholders.

- **`build_menus`** (pub, L65)
  - Signature: `pub fn build_menus(ctx: &CommandContext, language: Language) -> Vec<Menu>`
  - Purpose: Builds the full menu tree for the main mail window.
  - Behavior: Assembles App, File (compose + settings), Edit, View (sidebar toggle + command palette), and Message menus. Message actions are added only when `commands::command_enabled` allows them; move-to-folder items come from `ctx.move_targets()` filtered the same way. Flag label switches between flag/unflag based on `ctx.message_starred()`. Move targets appear in a submenu when non-empty.

- **`build_compose_menus`** (pub, L158)
  - Signature: `pub fn build_compose_menus(language: Language) -> Vec<Menu>`
  - Purpose: Builds menus while a compose window is key.
  - Behavior: Returns App, a File menu (Send, Attach, Close, Discard), and Edit. Omits View and Message menus entirely.

- **`active_menu_surface`** (pub, L177)
  - Signature: `pub fn active_menu_surface(cx: &App) -> MenuSurface`
  - Purpose: Reads which menu surface is currently active.
  - Behavior: Returns the inner `MenuSurface` from the `ActiveMenuSurface` global, or `MenuSurface::Main` if the global is missing.

- **`push_if_enabled`** (private, L183)
  - Signature: `fn push_if_enabled<A: gpui::Action>(items: &mut Vec<MenuItem>, label: &'static str, action: A, id: CommandId, ctx: &CommandContext)`
  - Purpose: Conditionally appends a localized action item to a menu list.
  - Behavior: Calls `commands::command_enabled(&id, ctx)`; when true, pushes `MenuItem::action(label, action)`.

- **`sync_main_menus`** (pub, L196)
  - Signature: `pub fn sync_main_menus(cx: &mut App, ctx: &CommandContext, language: Language)`
  - Purpose: Refreshes the global menu bar for the main mail window.
  - Behavior: Sets `ActiveMenuSurface(Main)` and replaces GPUI menus with `build_menus(ctx, language)`.

- **`sync_compose_menus`** (pub, L202)
  - Signature: `pub fn sync_compose_menus(cx: &mut App, language: Language)`
  - Purpose: Refreshes the global menu bar for the compose window.
  - Behavior: Sets `ActiveMenuSurface(Compose)` and installs `build_compose_menus(language)`.

- **`sync_menus`** (pub, L208)
  - Signature: `pub fn sync_menus(cx: &mut App, ctx: &CommandContext, language: Language)`
  - Purpose: Convenience alias to refresh main-window menus from a command context.
  - Behavior: Delegates to `sync_main_menus`.

- **`menu_has_action_label`** (private, L214)
  - Signature: `fn menu_has_action_label(menu: &Menu, label: &str) -> bool` (`#[cfg(test)]`)
  - Purpose: Test helper that checks whether a menu tree contains an action with an exact label.
  - Behavior: Recursively walks `MenuItem::Action` and `MenuItem::Submenu` entries, comparing action names to `label`.

- **`ctx_with_inbox_message`** (private, L229)
  - Signature: `fn ctx_with_inbox_message() -> CommandContext` (tests)
  - Purpose: Builds a `CommandContext` with a selected inbox message and folder metadata.
  - Behavior: Constructs a synthetic `MessageDetail` in INBOX and a single-folder map so message-menu tests have enabled delete/move actions.

- **`message_menu_includes_delete_when_message_selected`** (private, L261)
  - Signature: `fn message_menu_includes_delete_when_message_selected()` (test)
  - Purpose: Asserts delete appears in the Message menu when a message is selected.
  - Behavior: Builds menus from `ctx_with_inbox_message()` and checks the localized delete label via `menu_has_action_label`.

- **`message_menu_omits_delete_without_selection`** (private, L274)
  - Signature: `fn message_menu_omits_delete_without_selection()` (test)
  - Purpose: Asserts delete is omitted when nothing is selected.
  - Behavior: Builds menus from `CommandContext::default()` and expects delete label absent.

- **`file_menu_always_includes_compose`** (private, L287)
  - Signature: `fn file_menu_always_includes_compose()` (test)
  - Purpose: Asserts compose is always available from File.
  - Behavior: Finds the File menu and checks for the localized compose window title action.

- **`compose_menus_include_send_attach_close_and_discard`** (private, L300)
  - Signature: `fn compose_menus_include_send_attach_close_and_discard()` (test)
  - Purpose: Validates compose surface File menu contents and absence of reader-only menus.
  - Behavior: Checks Send, Attach, Close, and Discard labels exist; asserts Message and View menus are not present.

### `src/cef_osr.rs`

#### Types / constants

- **const `EVENTFLAG_SHIFT_DOWN`** (private, L46): CEF `cef_event_flags_t` bit for Shift held during mouse/keyboard events.
- **const `EVENTFLAG_CONTROL_DOWN`** (private, L47): CEF event flag for Control held.
- **const `EVENTFLAG_ALT_DOWN`** (private, L48): CEF event flag for Alt held.
- **const `EVENTFLAG_LEFT_MOUSE_BUTTON`** (private, L49): CEF event flag OR'd into move/wheel events while the left button is down.
- **const `EVENTFLAG_MIDDLE_MOUSE_BUTTON`** (private, L50): CEF event flag for middle button held.
- **const `EVENTFLAG_RIGHT_MOUSE_BUTTON`** (private, L51): CEF event flag for right button held.
- **const `EVENTFLAG_COMMAND_DOWN`** (private, L52): CEF event flag for Meta/Command held.
- **const `VK_BACK`** (private, L55): Windows virtual-key code for Backspace, used in CEF `KeyEvent::windows_key_code`.
- **const `VK_TAB`** (private, L56): Virtual-key code for Tab.
- **const `VK_RETURN`** (private, L57): Virtual-key code for Enter/Return.
- **const `VK_ESCAPE`** (private, L58): Virtual-key code for Escape.
- **const `VK_SPACE`** (private, L59): Virtual-key code for Space.
- **const `VK_PRIOR`** (private, L60): Virtual-key code for Page Up.
- **const `VK_NEXT`** (private, L61): Virtual-key code for Page Down.
- **const `VK_END`** (private, L62): Virtual-key code for End.
- **const `VK_HOME`** (private, L63): Virtual-key code for Home.
- **const `VK_LEFT`** (private, L64): Virtual-key code for Left arrow.
- **const `VK_UP`** (private, L65): Virtual-key code for Up arrow.
- **const `VK_RIGHT`** (private, L66): Virtual-key code for Right arrow.
- **const `VK_DOWN`** (private, L67): Virtual-key code for Down arrow.
- **const `VK_DELETE`** (private, L68): Virtual-key code for Delete.
- **enum `MouseButton`** (pub, L74): Logical mouse button (Left, Right, Middle) forwarded to CEF; independent of GPUI's `MouseButton`.
- **static `BROWSER_CREATE_COUNT`** (private, L91): Atomic counter of successful windowless browser creations; used to detect accidental re-creation.
- **struct `CefRuntime`** (private, L103): Owns the CEF `App`, process `Args`, and a `ready` cell that must outlive `cef::initialize`.
- **struct `BgApp`** (private, L212): CEF app state; holds the `ready` cell passed to the browser-process handler.
- **struct `BgBrowserProcess`** (private, L267): Browser-process handler state; sets `ready` when CEF context initializes.
- **type `FrameBuffer`** (private, L287): Tuple `(width, height, Vec<u8>)` of a BGRA paint buffer from CEF `on_paint`.
- **struct `BgRender`** (private, L290): Render-handler state: shared frame mutex, logical view size, and device scale factor.
- **struct `BgDisplay`** (private, L389): Display-handler state: host event channel and localized download-notification text.
- **struct `BgContextMenu`** (private, L483): Marker type for the context-menu handler that suppresses native Chromium menus.
- **struct `BgLoad`** (private, L526): Load-handler state: host channel for redraw requests after main-frame load.
- **struct `BgRequest`** (private, L563): Marker type for the request handler that opens external URLs in the system browser.
- **const `IPC_CONSOLE_PREFIX`** (private, L632): Prefix (`__BGMAIL_IPC__`) prepended to bridged IPC messages logged via `console.log`.
- **struct `OsrBrowser`** (pub, L750): Windowless CEF browser rendering one e-mail body off-screen; owns paint buffer and GPUI texture state.

#### Functions / methods


- **`browser_create_count`** (pub, L96)
  - Signature: `pub fn browser_create_count() -> u64`
  - Purpose: Returns how many [`OsrBrowser`] instances have been created since process start.
  - Behavior: Loads `BROWSER_CREATE_COUNT` with relaxed ordering. Intended for leak diagnostics and tests; message switches must not increment it.
- **`build_app`** (private, L111)
  - Signature: `fn build_app(ready: Rc<Cell<bool>>) -> App`
  - Purpose: Builds the CEF `App` with command-line tweaks and a browser-process handler wired to `ready`.
  - Behavior: Wraps `BgApp { ready }` in `AppBuilder::new`. Does not initialize CEF.
- **`run_if_subprocess`** (pub, L123)
  - Signature: `pub fn run_if_subprocess() -> bool`
  - Purpose: Detects and runs CEF sub-processes so `main` can exit early without starting GPUI.
  - Behavior: Calls `execute_process`; returns `true` when `--type=` is present (sub-process ran to completion). Returns `false` in the browser process (`execute_process` returns -1) so boot continues.
- **`initialize`** (pub, L152)
  - Signature: `pub fn initialize() -> bool`
  - Purpose: Initializes CEF for windowless OSR with an external message pump.
  - Behavior: Idempotent: returns `true` immediately if already initialized. On success stores `CefRuntime` in thread-local `CEF` and enables windowless rendering, external pump, and no-sandbox. Returns `false` if `cef::initialize` fails.
- **`pump`** (pub, L189)
  - Signature: `pub fn pump()`
  - Purpose: Advances CEF's message loop one tick.
  - Behavior: Calls `do_message_loop_work` only when CEF is initialized; otherwise no-op.
- **`shutdown_cef`** (pub, L196)
  - Signature: `pub fn shutdown_cef()`
  - Purpose: Shuts down CEF on app quit.
  - Behavior: Takes and drops `CefRuntime` from thread-local storage, then calls `cef::shutdown`. No-op if never initialized.
- **`is_ready`** (pub, L205)
  - Signature: `pub fn is_ready() -> bool`
  - Purpose: Reports whether CEF context initialization finished and browsers can be created.
  - Behavior: Returns `true` when thread-local `CEF` exists and its `ready` cell is set by `on_context_initialized`.
- **`expected_physical_size`** (private, L360)
  - Signature: `fn expected_physical_size(width: i32, height: i32, scale: f32) -> (u32, u32)`
  - Purpose: Computes the physical pixel dimensions CEF should paint for a logical view size and scale factor.
  - Behavior: Clamps scale to ≥0.01, rounds `width×scale` and `height×scale` to `u32`. Used to detect stale frames after resize.
- **`frame_matches_view`** (private, L370)
  - Signature: `fn frame_matches_view(frame_w: u32, frame_h: u32, expected_w: u32, expected_h: u32) -> bool`
  - Purpose: Checks whether a painted buffer matches the expected view size within rounding tolerance.
  - Behavior: Returns `true` when width and height each differ by at most 1 pixel from expected.
- **`store_paint_buffer`** (private, L377)
  - Signature: `fn store_paint_buffer(slot: &mut Option<FrameBuffer>, width: u32, height: u32, src: &[u8])`
  - Purpose: Stores a CEF `on_paint` buffer, reusing allocation when dimensions unchanged.
  - Behavior: Copies into existing `Vec` when `(w,h,len)` match; otherwise allocates a new `(width, height, src.to_vec())` tuple.
- **`map_cef_cursor`** (private, L439)
  - Signature: `fn map_cef_cursor(type_: CursorType) -> WebviewCursor`
  - Purpose: Maps CEF cursor types to portable [`WebviewCursor`] for GPUI.
  - Behavior: Compares raw cursor enum values; maps I-beam, hand, resize variants, etc. Falls back to `Arrow` for unmapped types (including default pointer).
- **`handle_ipc`** (private, L637)
  - Signature: `fn handle_ipc(message: &str, to_host: &Sender<HostEvent>, notify_body: &str)`
  - Purpose: Routes bridged document IPC from the console bridge to host events or local actions.
  - Behavior: Parses via [`parse_ipc_message`]. Hover, copy, image-shown, body-mousedown, and palette go to `to_host` via `try_send`. Open-external and download-image run on the main thread (`open_in_new_window`, `download_image`). Unknown messages are ignored.
- **`open_in_new_window`** (private, L663)
  - Signature: `fn open_in_new_window(url: &str)`
  - Purpose: Opens a link or image outside the reader webview.
  - Behavior: External `http`/`https`/`mailto` URLs open via `open::that_detached`. Base64 `data:` images are written to a temp file and opened by the OS default app. Other URLs are no-op.
- **`download_image`** (private, L677)
  - Signature: `fn download_image(url: &str, _notify_body: &str)`
  - Purpose: Saves an image to Downloads without a file dialog.
  - Behavior: Decodes base64 `data:` URIs and writes to a unique path under [`downloads_dir`]. Non-data remote URLs fall back to opening in the browser. Silently returns on missing home dir or I/O failure.
- **`persist_temp_file`** (private, L696)
  - Signature: `fn persist_temp_file(extension: &str, bytes: &[u8]) -> Option<std::path::PathBuf>`
  - Purpose: Materializes inline image bytes to a uniquely named temp file for OS viewing.
  - Behavior: Builds `bgmail-image-{nanos}.{extension}` in the system temp dir, writes bytes, returns path on success; `None` on clock or I/O failure.
- **`ipc_shim_script`** (private, L713)
  - Signature: `fn ipc_shim_script() -> String`
  - Purpose: Builds the inline script that defines `window.ipc.postMessage` via prefixed `console.log`.
  - Behavior: Returns an HTML `<script>` tag embedding `IPC_CONSOLE_PREFIX` so [`DisplayHandler::on_console_message`] can intercept messages.
- **`compose_document`** (private, L723)
  - Signature: `fn compose_document(html: &str) -> String`
  - Purpose: Injects IPC shim and content script into a rendered e-mail HTML document.
  - Behavior: Inserts shim immediately after `<head>` (or prepends if no head). Inserts [`CONTENT_SCRIPT`] immediately before `</body>` (or appends). Returns the augmented document unchanged if markers are absent.
- **`data_url`** (private, L740)
  - Signature: `fn data_url(html: &str) -> String`
  - Purpose: Encodes a composed HTML document as a `data:text/html` URL for CEF navigation.
  - Behavior: Calls `compose_document`, then percent-encodes the result with `urlencoding::encode`.
- **`modifier_flags`** (pub, L1133)
  - Signature: `pub fn modifier_flags(shift: bool, control: bool, alt: bool, meta: bool) -> u32`
  - Purpose: Converts GPUI modifier booleans to CEF `event_flags` bitmask.
  - Behavior: ORs `EVENTFLAG_*_DOWN` bits for each true modifier; returns 0 when none are set.
- **`windows_virtual_key`** (private, L1152)
  - Signature: `fn windows_virtual_key(key: &str) -> Option<i32>`
  - Purpose: Maps GPUI key names to Windows virtual-key codes for CEF key events.
  - Behavior: Handles named keys (backspace, arrows, etc.) and single ASCII letters/digits/symbols. Returns `None` for unknown multi-char keys or non-ASCII single chars.
- **`key_characters`** (private, L1186)
  - Signature: `fn key_characters(key: &str, key_char: Option<&str>, modifiers: u32) -> (u16, u16)`
  - Purpose: Builds CEF character and unmodified-character fields for key events.
  - Behavior: Unmodified comes from `key_char`, else first char of `key`. When Control or Command is down and the char is A–Z/a–z, character becomes ASCII control code (Ctrl+C → 3); otherwise character equals unmodified.

##### Context: `App` (CEF handler)

- **`on_before_command_line_processing`** (private, L222)
  - Signature: `fn on_before_command_line_processing(&self, _process_type: Option<&CefString>, command_line: Option<&mut CommandLine>)`
  - Purpose: Applies reader-specific Chromium command-line switches before process launch.
  - Behavior: Disables extensions, background networking, smooth scrolling, background throttling/occlusion, and on-device ML features. No-op when `command_line` is `None`.
- **`browser_process_handler`** (private, L258)
  - Signature: `fn browser_process_handler(&self) -> Option<BrowserProcessHandler>`
  - Purpose: Supplies the browser-process handler that signals CEF readiness.
  - Behavior: Returns `BrowserProcessHandlerBuilder` wrapping `BgBrowserProcess` with a clone of `ready`.

##### Context: `BrowserProcessHandler` (CEF handler)

- **`on_context_initialized`** (private, L277)
  - Signature: `fn on_context_initialized(&self)`
  - Purpose: Marks CEF as ready to create browsers after context init.
  - Behavior: Sets `self.handler.ready` to `true`.

##### Context: `RenderHandler` (CEF handler)

- **`view_rect`** (private, L307)
  - Signature: `fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>)`
  - Purpose: Reports the logical pixel size CEF should render into.
  - Behavior: When `rect` is present and stored size is non-zero, sets origin (0,0) and width/height from shared `BgRender.size`. Leaves rect unchanged if size is zero.
- **`screen_info`** (private, L320)
  - Signature: `fn screen_info(&self, _browser: Option<&mut Browser>, screen_info: Option<&mut ScreenInfo>) -> c_int`
  - Purpose: Reports device scale factor for HiDPI OSR rendering.
  - Behavior: Sets `device_scale_factor` from `BgRender.scale` and returns 1 when `screen_info` is present; returns 0 otherwise.
- **`on_paint`** (private, L332)
  - Signature: `fn on_paint(&self, _browser: Option<&mut Browser>, type_: PaintElementType, _dirty_rects: Option<&[Rect]>, buffer: *const u8, width: c_int, height: c_int)`
  - Purpose: Captures main-view BGRA paint buffers from CEF OSR.
  - Behavior: Ignores non-default paint types, null buffers, and non-positive dimensions. Copies `width×height×4` bytes from the CEF buffer into the shared frame mutex via `store_paint_buffer`. Uses `unsafe` slice over CEF-owned memory for the duration of the callback only.

##### Context: `DisplayHandler` (CEF handler)

- **`on_console_message`** (private, L400)
  - Signature: `fn on_console_message(&self, _browser: Option<&mut Browser>, _level: LogSeverity, message: Option<&CefString>, _source: Option<&CefString>, _line: c_int) -> c_int`
  - Purpose: Intercepts IPC bridge messages logged by the injected shim.
  - Behavior: Strips `IPC_CONSOLE_PREFIX` and routes payload through `handle_ipc`. Returns 1 (handled) for bridge messages; returns 0 for ordinary console output.
- **`on_cursor_change`** (private, L419)
  - Signature: `fn on_cursor_change(&self, _browser: Option<&mut Browser>, _cursor: c_ulong, type_: CursorType, _custom_cursor_info: Option<&CursorInfo>) -> c_int`
  - Purpose: Forwards page-requested cursor changes to the GPUI host.
  - Behavior: Maps CEF cursor via `map_cef_cursor`, sends `HostEvent::CursorChanged` over `try_send`, returns 1 so CEF does not set the OS cursor.

##### Context: `ContextMenuHandler` (CEF handler)

- **`on_before_context_menu`** (private, L491)
  - Signature: `fn on_before_context_menu(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _params: Option<&mut ContextMenuParams>, model: Option<&mut MenuModel>)`
  - Purpose: Clears Chromium's native context menu before display.
  - Behavior: Calls `model.clear()` when present so only the HTML [`CONTENT_SCRIPT`] menu appears.
- **`run_context_menu`** (private, L505)
  - Signature: `fn run_context_menu(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _params: Option<&mut ContextMenuParams>, _model: Option<&mut MenuModel>, callback: Option<&mut RunContextMenuCallback>) -> c_int`
  - Purpose: Cancels platform context-menu display under windowless OSR.
  - Behavior: Calls `callback.cancel()` when present; returns 1 to indicate custom handling.

##### Context: `LoadHandler` (CEF handler)

- **`on_load_end`** (private, L536)
  - Signature: `fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, _http_status_code: c_int)`
  - Purpose: Wakes rendering after the main document finishes loading.
  - Behavior: Ignores subframe loads. On main frame: unhides browser, sets focus, invalidates paint, sends `HostEvent::OsrNeedsRedraw` to GPUI.

##### Context: `RequestHandler` (CEF handler)

- **`on_before_browse`** (private, L571)
  - Signature: `fn on_before_browse(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: c_int, _is_redirect: c_int) -> c_int`
  - Purpose: Blocks in-webview navigation to external URLs.
  - Behavior: When request URL is external per [`is_external_link`], opens it with `open::that_detached` and returns 1 (cancel navigation). Returns 0 for in-document `data:` loads and fragments.

##### Context: `Client` (CEF handler)

- **`render_handler`** (private, L605)
  - Signature: `fn render_handler(&self) -> Option<RenderHandler>`
  - Purpose: Exposes the OSR render handler to CEF.
  - Behavior: Clones and returns the stored `RenderHandler`.
- **`display_handler`** (private, L609)
  - Signature: `fn display_handler(&self) -> Option<DisplayHandler>`
  - Purpose: Exposes the display handler (console IPC bridge and cursor).
  - Behavior: Clones and returns the stored `DisplayHandler`.
- **`request_handler`** (private, L613)
  - Signature: `fn request_handler(&self) -> Option<RequestHandler>`
  - Purpose: Exposes external-link navigation handler.
  - Behavior: Clones and returns the stored `RequestHandler`.
- **`context_menu_handler`** (private, L617)
  - Signature: `fn context_menu_handler(&self) -> Option<ContextMenuHandler>`
  - Purpose: Exposes the native menu suppression handler.
  - Behavior: Clones and returns the stored `ContextMenuHandler`.
- **`load_handler`** (private, L621)
  - Signature: `fn load_handler(&self) -> Option<LoadHandler>`
  - Purpose: Exposes the main-frame load-end handler.
  - Behavior: Clones and returns the stored `LoadHandler`.

##### Context: `OsrBrowser`

- **`new`** (pub, L779)
  - Signature: `pub fn new(html: &str, to_host: Sender<HostEvent>, notify_body: String) -> Option<Self>`
  - Purpose: Creates a windowless CEF browser loading the given HTML via a `data:` URL.
  - Behavior: Returns `None` when [`is_ready`] is false or `browser_host_create_browser_sync` fails. Wires render, display, load, request, and context-menu handlers; increments `BROWSER_CREATE_COUNT`. Initial size 800×600, scale 1.0.
- **`set_html`** (pub, L848)
  - Signature: `pub fn set_html(&mut self, html: &str) -> (bool, Option<Arc<RenderImage>>)`
  - Purpose: Navigates to new HTML when the document changed (message or theme).
  - Behavior: No-op returning `(false, None)` when HTML unchanged. Otherwise `load_url` on main frame, clears pending paint buffer, takes previous GPUI texture, wakes/unfocuses host and invalidates. Returns `(true, previous_texture)`.
- **`pump_hard`** (pub, L874)
  - Signature: `pub fn pump_hard(&self)`
  - Purpose: Runs several message-loop ticks after navigation or activation.
  - Behavior: Calls [`pump`] eight times in a loop.
- **`set_notify_text`** (pub, L881)
  - Signature: `pub fn set_notify_text(&self, body: String)`
  - Purpose: Updates localized download-notification text without rebuilding the browser.
  - Behavior: Replaces the string in shared `notify_body` `RefCell`.
- **`set_size`** (pub, L888)
  - Signature: `pub fn set_size(&mut self, width: f32, height: f32, scale_factor: f32) -> bool`
  - Purpose: Updates logical view size and device scale for OSR rendering.
  - Behavior: Clamps dimensions to ≥1 logical pixel. Returns `false` when unchanged. On change notifies screen info (if scale changed), calls `was_resized`, and invalidates paint to force a new buffer.
- **`set_visible`** (pub, L911)
  - Signature: `pub fn set_visible(&mut self, visible: bool)`
  - Purpose: Shows or hides the OSR browser to pause background painting.
  - Behavior: Early return when visibility unchanged. Calls `host.was_hidden(0|1)` and updates `visible` flag.
- **`take_frame`** (pub, L925)
  - Signature: `pub fn take_frame(&mut self) -> Option<(Arc<RenderImage>, Option<Arc<RenderImage>>)>`
  - Purpose: Consumes the latest CEF paint as a GPUI texture.
  - Behavior: Takes pending `(w,h,bytes)` from frame mutex; builds BGRA `RenderImage`. Returns `(new, previous)` where `previous` is the last displayed texture for disposal. Returns `None` when no new frame pending or image build fails.
- **`has_pending_frame`** (pub, L943)
  - Signature: `pub fn has_pending_frame(&self) -> bool`
  - Purpose: Reports unconsumed paint buffers waiting in the shared mutex.
  - Behavior: Returns whether frame mutex holds `Some(_)`.
- **`current_frame`** (pub, L948)
  - Signature: `pub fn current_frame(&self) -> Option<Arc<RenderImage>>`
  - Purpose: Returns the last uploaded GPUI texture for repaint without consuming a new buffer.
  - Behavior: Clones `self.current`.
- **`current_frame_px`** (pub, L953)
  - Signature: `pub fn current_frame_px(&self) -> Option<(u32, u32)>`
  - Purpose: Returns physical pixel dimensions of the current displayed frame.
  - Behavior: Returns `self.current_px` copy.
- **`current_frame_fits_view`** (pub, L959)
  - Signature: `pub fn current_frame_fits_view(&self) -> bool`
  - Purpose: Checks whether the displayed buffer matches the current view geometry.
  - Behavior: Compares `current_px` to `expected_physical_size` of stored logical size and scale via `frame_matches_view`. Returns `false` when no current frame.
- **`flush_input`** (pub, L970)
  - Signature: `pub fn flush_input(&self)`
  - Purpose: Delivers coalesced wheel input and advances the message loop before sampling frames.
  - Behavior: Sends pending wheel via `send_wheel` if any, then calls [`pump`].
- **`mouse_move`** (pub, L978)
  - Signature: `pub fn mouse_move(&self, x: f32, y: f32, modifiers: u32)`
  - Purpose: Forwards mouse movement at logical view coordinates to CEF.
  - Behavior: Flushes pending wheel first. Sends `MouseEvent` with OR'd held-button flags via `send_mouse_move_event`.
- **`mouse_click`** (pub, L993)
  - Signature: `pub fn mouse_click(&self, x: f32, y: f32, button: MouseButton, pressed: bool, click_count: i32)`
  - Purpose: Forwards mouse press/release and tracks held-button flags for drag selection.
  - Behavior: Flushes pending wheel. Updates `buttons` cell on press/release. Sends click event with mouse-up flag inverted (0=down, 1=up).
- **`has_mouse_capture`** (pub, L1028)
  - Signature: `pub fn has_mouse_capture(&self) -> bool`
  - Purpose: Reports whether any mouse button is currently held in the OSR view.
  - Behavior: Returns `buttons != 0`.
- **`mouse_wheel`** (pub, L1034)
  - Signature: `pub fn mouse_wheel(&self, x: f32, y: f32, delta_x: f32, delta_y: f32)`
  - Purpose: Coalesces scroll-wheel deltas until the next flush or paint.
  - Behavior: Sums deltas into `pending_wheel`, keeping the latest pointer position. Does not send to CEF immediately.
- **`flush_pending_wheel`** (private, L1043)
  - Signature: `fn flush_pending_wheel(&self)`
  - Purpose: Sends accumulated wheel delta if one is queued.
  - Behavior: Takes `pending_wheel` and calls `send_wheel` when present.
- **`send_wheel`** (private, L1049)
  - Signature: `fn send_wheel(&self, x: f32, y: f32, delta_x: f32, delta_y: f32)`
  - Purpose: Forwards a scroll-wheel event to CEF.
  - Behavior: No-op when both deltas round to zero integers. Otherwise sends `send_mouse_wheel_event` with held-button modifiers.
- **`set_focus`** (pub, L1066)
  - Signature: `pub fn set_focus(&self, focused: bool)`
  - Purpose: Sets OSR browser keyboard focus for shortcuts and caret behavior.
  - Behavior: Calls `host.set_focus(1|0)` when host is available.
- **`key_event`** (pub, L1074)
  - Signature: `pub fn key_event(&self, pressed: bool, key: &str, key_char: Option<&str>, modifiers: u32)`
  - Purpose: Forwards keyboard press/release to CEF.
  - Behavior: No-op when `windows_virtual_key` returns `None` or host missing. On press sends RAWKEYDOWN then CHAR event; on release sends KEYUP. Character fields from `key_characters`.
- **`dismiss_context_menu`** (pub, L1113)
  - Signature: `pub fn dismiss_context_menu(&self)`
  - Purpose: Closes the HTML context menu from the host on outside clicks.
  - Behavior: Executes `window.__rmCloseMenu&&window.__rmCloseMenu()` on the main frame when available.

##### Context: `Drop for OsrBrowser`

- **`drop`** (private, L1122)
  - Signature: `fn drop(&mut self)`
  - Purpose: Force-closes the CEF browser when the GPUI wrapper is dropped.
  - Behavior: Calls `host.close_browser(1)` to prevent orphaned windowless browsers.

##### Context: `tests`

- **`data_url_percent_encodes_the_document`** (private, L1208)
  - Signature: `fn data_url_percent_encodes_the_document()`
  - Purpose: Asserts `data_url` percent-encodes spaces and ampersands in HTML.
  - Behavior: Checks prefix and absence of raw special characters in the URL string.
- **`compose_document_injects_shim_first_and_content_last`** (private, L1218)
  - Signature: `fn compose_document_injects_shim_first_and_content_last()`
  - Purpose: Verifies injection order of IPC shim and content script.
  - Behavior: Asserts shim prefix appears before content script markers and before `<title>`.
- **`compose_document_falls_back_without_head_or_body`** (private, L1231)
  - Signature: `fn compose_document_falls_back_without_head_or_body()`
  - Purpose: Verifies document composition works on minimal HTML fragments.
  - Behavior: Asserts both scripts are present alongside bare markup.
- **`modifier_flags_map_expected_bits`** (private, L1239)
  - Signature: `fn modifier_flags_map_expected_bits()`
  - Purpose: Verifies modifier bitmask mapping.
  - Behavior: Asserts individual and combined flag values match CEF constants.
- **`windows_virtual_key_maps_letters_and_named_keys`** (private, L1255)
  - Signature: `fn windows_virtual_key_maps_letters_and_named_keys()`
  - Purpose: Verifies GPUI key name to VK mapping.
  - Behavior: Asserts letter case insensitivity, named keys, and unknown key rejection.
- **`key_characters_use_control_codes_for_shortcuts`** (private, L1264)
  - Signature: `fn key_characters_use_control_codes_for_shortcuts()`
  - Purpose: Verifies Ctrl/Cmd produces ASCII control codes in character field.
  - Behavior: Asserts Ctrl+C → (3, 'c') and plain typing leaves character unchanged.
- **`map_cef_cursor_hand_and_ibeam`** (private, L1274)
  - Signature: `fn map_cef_cursor_hand_and_ibeam()`
  - Purpose: Verifies common CEF cursor mappings.
  - Behavior: Asserts HAND→Hand, IBEAM→IBeam, POINTER→Arrow.
- **`store_paint_buffer_reuses_allocation_for_same_size`** (private, L1281)
  - Signature: `fn store_paint_buffer_reuses_allocation_for_same_size()`
  - Purpose: Verifies in-place buffer reuse when dimensions match.
  - Behavior: Asserts pointer stability and updated bytes after second paint of same size.
- **`store_paint_buffer_reallocates_when_size_changes`** (private, L1293)
  - Signature: `fn store_paint_buffer_reallocates_when_size_changes()`
  - Purpose: Verifies reallocation when paint dimensions change.
  - Behavior: Asserts new width and byte length after size change.
- **`coalesce_wheel_deltas_sums_offsets`** (private, L1302)
  - Signature: `fn coalesce_wheel_deltas_sums_offsets()`
  - Purpose: Verifies wheel coalescing logic mirrors `mouse_wheel` accumulation.
  - Behavior: Uses local `coalesce` helper; asserts summed deltas and latest position.
- **`coalesce`** (private, nested in L1304)
  - Signature: `fn coalesce(pending: Option<(f32,f32,f32,f32)>, x: f32, y: f32, dx: f32, dy: f32) -> (f32, f32, f32, f32)`
  - Purpose: Test helper mirroring pending wheel accumulation.
  - Behavior: Sums deltas when pending exists; otherwise starts a new tuple with latest coordinates.
- **`expected_physical_size_scales_logical_view`** (private, L1322)
  - Signature: `fn expected_physical_size_scales_logical_view()`
  - Purpose: Verifies logical-to-physical size scaling.
  - Behavior: Asserts 1.0, 2.0, and 1.5 scale factors round correctly.
- **`frame_matches_view_allows_one_pixel_slack`** (private, L1329)
  - Signature: `fn frame_matches_view_allows_one_pixel_slack()`
  - Purpose: Verifies ±1px tolerance in frame/view matching.
  - Behavior: Asserts exact match and 1px off pass; 20px off fails.
- **`browser_create_is_single_call_site_and_set_html_navigates`** (private, L1336)
  - Signature: `fn browser_create_is_single_call_site_and_set_html_navigates()`
  - Purpose: Static source check that only one CEF browser is ever created per process.
  - Behavior: Parses production source (pre-`#[cfg(test)]`) and asserts one `browser_host_create_browser_sync`, presence of `load_url`, and `Drop for OsrBrowser`.
- **`browser_create_count_is_zero_without_cef_init`** (private, L1362)
  - Signature: `fn browser_create_count_is_zero_without_cef_init()`
  - Purpose: Verifies counter stays zero when CEF is never initialized in unit tests.
  - Behavior: Asserts `browser_create_count() == 0`.



### `src/command_palette.rs`

#### Types / constants

- **struct `CommandPaletteState`**: Mutable state for the in-window command palette overlay (open flag, query, selection index, search input entity).

#### Functions / methods

##### Context: `CommandPaletteState`

- **`filtered_entries`** (pub, L17)
  - Signature: `pub fn filtered_entries(&self, language: Language, ctx: &CommandContext) -> Vec<CommandEntry>`
  - Purpose: Returns palette commands filtered by the current query.
  - Behavior: Calls `palette_commands` then keeps entries whose label matches `self.query` via `command_matches_query`.

- **`clamp_selection`** (pub, L28)
  - Signature: `pub fn clamp_selection(&mut self, count: usize)`
  - Purpose: Keeps `selected_ix` within `[0, count-1]`.
  - Behavior: Resets to 0 when count is zero; clamps down when index exceeds last entry.

- **`on_query_change`** (pub, L36)
  - Signature: `pub fn on_query_change(&mut self, query: String)`
  - Purpose: Updates the filter query and resets selection.
  - Behavior: Stores query and sets `selected_ix` to 0 so filtering starts from the top.

- **`move_selection`** (pub, L41)
  - Signature: `pub fn move_selection(&mut self, delta: isize, entry_count: usize) -> bool`
  - Purpose: Moves highlight up/down by `delta`.
  - Behavior: Returns false when there are no entries or selection unchanged; otherwise clamps and returns true.

- **`selected_command`** (pub, L55)
  - Signature: `pub fn selected_command<'a>(&self, entries: &'a [CommandEntry]) -> Option<&'a CommandId>`
  - Purpose: Returns the command id at the current selection index.
  - Behavior: Indexes into `entries` at `selected_ix`; None when out of range.
##### Context: `module (tests)`

- **`selection_clamps_to_last_entry`** (private, L65)
  - Signature: `fn selection_clamps_to_last_entry()`
  - Purpose: Test: selection clamps when entry count shrinks.
  - Behavior: Sets index past end, clamps to 3 entries, expects index 2.

### `src/command_palette_overlay.rs`

#### Types / constants

- _(None at module top-level.)_

#### Functions / methods

- **`render_command_palette`** (pub(crate), L17)
  - Signature: `pub(crate) fn render_command_palette(cx: &mut Context<RootView>, entries: Vec<CommandEntry>, selected_ix: usize, input: Entity<TextInput>) -> gpui::AnyElement`
  - Purpose: Builds the dimmed in-window command palette overlay.
  - Behavior: Renders a semi-transparent backdrop (click/Escape dismisses), centered panel with search input and scrollable command list. List items highlight selection and execute on click. Key events delegate to `RootView::handle_command_palette_key`. Panel clicks stop propagation so backdrop dismiss does not fire.

- **`palette_module_exports_in_window_renderer`** (private, L99)
  - Signature: `fn palette_module_exports_in_window_renderer()`
  - Purpose: Test: overlay stays an in-window RootView helper.
  - Behavior: Source guard asserting module doc and `pub(crate) fn render_command_palette` exist.

### `src/commands.rs`

#### Types / constants

- **enum `CommandId`**: Identifies a command in menus and the palette (compose, settings, message actions, move-to-folder with path).
- **struct `CommandContext`**: Snapshot of UI state (selected message, detail, folders) used to decide command availability and labels.
- **struct `CommandEntry`**: One palette row: `id` plus localized `label`.

#### Functions / methods

##### Context: `CommandContext`

- **`message_in_trash`** (pub, L35)
  - Signature: `pub fn message_in_trash(&self) -> bool`
  - Purpose: Whether the selected message is in Trash.
  - Behavior: True when `message_detail.folders_csv` contains the system trash path.

- **`message_in_junk`** (pub, L41)
  - Signature: `pub fn message_in_junk(&self) -> bool`
  - Purpose: Whether the selected message is in Junk.
  - Behavior: True when folders CSV contains the system junk path.

- **`message_in_archive`** (pub, L47)
  - Signature: `pub fn message_in_archive(&self) -> bool`
  - Purpose: Whether the selected message is in Archive.
  - Behavior: True when folders CSV contains the system archive path.

- **`message_starred`** (pub, L53)
  - Signature: `pub fn message_starred(&self) -> bool`
  - Purpose: Whether the selected message is flagged/starred.
  - Behavior: Reads `starred` from `message_detail` when present.

- **`move_targets`** (pub, L59)
  - Signature: `pub fn move_targets(&self) -> Vec<(SharedString, SharedString)>`
  - Purpose: Eligible move destinations for the selected message.
  - Behavior: Returns `(display_name, path)` pairs for folders in the message's account, excluding forbidden manual-move destinations (sent, drafts, flagged, etc.).

##### Context: `module`

- **`message_has_folder`** (private, L85)
  - Signature: `fn message_has_folder(csv: &str, folder_path: &str) -> bool`
  - Purpose: Checks whether a comma-separated folder list contains a path.
  - Behavior: Splits on commas, ignores empty segments, compares paths exactly.
- **`folder_display_name`** (pub, L92)
  - Signature: `pub fn folder_display_name(folder: &Folder, language: Language) -> String`
  - Purpose: Localized display name for a storage folder.
  - Behavior: Uses `folder.display_name` when set; otherwise maps system paths to localized mailbox names via `MailboxKind`, falling back to the raw path.
- **`system_folder_label`** (private, L99)
  - Signature: `fn system_folder_label(path: &str, language: Language) -> String`
  - Purpose: Maps a system folder path to a localized label.
  - Behavior: Matches known system paths to `MailboxKind` display names; unknown paths pass through unchanged.
- **`label_for`** (private, L113)
  - Signature: `fn label_for(id: &CommandId, language: Language, ctx: &CommandContext) -> SharedString`
  - Purpose: Localized label for a command id.
  - Behavior: Uses locale keys for standard commands; flag toggles between Flag/Unflag based on starred state; move commands use folder display names.
- **`command_enabled`** (pub, L141)
  - Signature: `pub fn command_enabled(id: &CommandId, ctx: &CommandContext) -> bool`
  - Purpose: Whether a command can run in the current context.
  - Behavior: Always enables compose/settings/sidebar. Message commands require a selection and apply folder-state rules (e.g. delete disabled in trash, restore only in trash, archive disabled when already archived or in trash).
- **`palette_commands`** (pub, L163)
  - Signature: `pub fn palette_commands(language: Language, ctx: &CommandContext) -> Vec<CommandEntry>`
  - Purpose: Builds the filtered command palette list.
  - Behavior: Starts with global commands; adds message actions and move targets when a message is selected; filters through `command_enabled` and attaches localized labels.
- **`command_matches_query`** (pub, L194)
  - Signature: `pub fn command_matches_query(label: &str, query: &str) -> bool`
  - Purpose: Case-insensitive substring filter for the palette.
  - Behavior: Empty or whitespace-only queries match everything; otherwise compares ASCII-lowercased label against query.
##### Context: `module (tests)`

- **`ctx_with_message`** (private, L208)
  - Signature: `fn ctx_with_message()`
  - Purpose: Builds a test `CommandContext` with inbox or trash message.
  - Behavior: Constructs message detail and folder map for unit tests.

- **`delete_disabled_in_trash_restore_enabled`** (private, L257)
  - Signature: `fn delete_disabled_in_trash_restore_enabled()`
  - Purpose: Test: trash enables restore/permanent delete, disables normal delete.
  - Behavior: Uses trash context and asserts enabled/disabled flags.

- **`inbox_message_can_delete_not_restore`** (private, L265)
  - Signature: `fn inbox_message_can_delete_not_restore()`
  - Purpose: Test: inbox message allows delete, not restore.
  - Behavior: Asserts delete enabled and restore disabled for inbox message.

- **`palette_never_lists_command_palette_action`** (private, L272)
  - Signature: `fn palette_never_lists_command_palette_action()`
  - Purpose: Test: palette does not list itself.
  - Behavior: Ensures no palette entry mentions command palette in EN/PT.

- **`palette_without_message_omits_message_actions`** (private, L283)
  - Signature: `fn palette_without_message_omits_message_actions()`
  - Purpose: Test: no message commands without selection.
  - Behavior: Default context omits delete/archive/move entries but includes compose.

- **`palette_with_message_includes_move_targets`** (private, L296)
  - Signature: `fn palette_with_message_includes_move_targets()`
  - Purpose: Test: selection adds move-to-folder entries.
  - Behavior: Inbox message context includes at least one `MoveToFolder` command.

- **`flag_label_reflects_starred_state`** (private, L305)
  - Signature: `fn flag_label_reflects_starred_state()`
  - Purpose: Test: flag label becomes Unflag when starred.
  - Behavior: Starred message yields Unflag label for toggle-flag command.

- **`command_filter_is_case_insensitive`** (private, L312)
  - Signature: `fn command_filter_is_case_insensitive()`
  - Purpose: Test: query matching is case-insensitive.
  - Behavior: ARCH matches Archive Message; junk does not.

- **`move_targets_exclude_sent_drafts_and_flagged`** (private, L318)
  - Signature: `fn move_targets_exclude_sent_drafts_and_flagged()`
  - Purpose: Test: forbidden folders excluded from move list.
  - Behavior: Sent, drafts, and flagged paths absent; user folder present.

### `src/compose.rs`

#### Types / constants

- **`COMPOSE_DEFAULT_WIDTH`** (pub, L23)
  - Signature: `pub const COMPOSE_DEFAULT_WIDTH: f32 = 790.0`
  - Purpose: Default compose window width in logical pixels on first open.
  - Behavior: Used by `Config::default` and clamped upward by `clamp_compose_size`.

- **`COMPOSE_DEFAULT_HEIGHT`** (pub, L25)
  - Signature: `pub const COMPOSE_DEFAULT_HEIGHT: f32 = 720.0`
  - Purpose: Default compose window height in logical pixels on first open.
  - Behavior: Persisted in config defaults and respected unless below minimum height.

- **`COMPOSE_MIN_WIDTH`** (pub, L27)
  - Signature: `pub const COMPOSE_MIN_WIDTH: f32 = 480.0`
  - Purpose: Minimum allowed compose window width.
  - Behavior: Enforced by `clamp_compose_size` when restoring saved sizes.

- **`COMPOSE_MIN_HEIGHT`** (pub, L29)
  - Signature: `pub const COMPOSE_MIN_HEIGHT: f32 = 400.0`
  - Purpose: Minimum allowed compose window height.
  - Behavior: Enforced by `clamp_compose_size` when restoring saved sizes.

- **`COMPOSE_POSITION_UNSET`** (pub, L33)
  - Signature: `pub const COMPOSE_POSITION_UNSET: f32 = -1.0`
  - Purpose: Sentinel coordinate stored in config when the compose window has never been placed.
  - Behavior: Negative x/y values cause `open_bounds` to center the window instead of using saved origin.

- **`FIELD_LABEL_WIDTH`** (private, L65)
  - Signature: `const FIELD_LABEL_WIDTH: f32 = 72.0`
  - Purpose: Fixed width of the From/To/Cc/Subject label column in pixels.
  - Behavior: Applied in `field_row` and the To row so header rows align visually.

- **`ComposeView`** (pub, L68)
  - Signature: `pub struct ComposeView { accounts, from_account, show_cc_bcc, root, last_reported_bounds, white_background }`
  - Purpose: GPUI entity rendering the standalone new-message window (visual mock).
  - Behavior: Holds mock field state, a weak link to `RootView` for bounds persistence and close notification, cached white-background preference, and defers bounds sync to avoid re-entrancy during render.

#### Functions / methods

##### Context: `module`

- **`open_bounds`** (pub, L38)
  - Signature: `pub fn open_bounds(origin: Point<gpui::Pixels>, window_size: Size<gpui::Pixels>, cx: &App) -> Bounds<gpui::Pixels>`
  - Purpose: Computes initial window bounds from persisted compose geometry.
  - Behavior: Clamps size via `clamp_compose_size`. When `compose_position_is_unset(origin)`, returns centered bounds; otherwise uses the saved origin with clamped size.

- **`clamp_compose_size`** (pub, L52)
  - Signature: `pub fn clamp_compose_size(window_size: Size<gpui::Pixels>) -> Size<gpui::Pixels>`
  - Purpose: Ensures stored compose dimensions meet minimum width/height.
  - Behavior: Takes the max of each dimension with `COMPOSE_MIN_WIDTH` / `COMPOSE_MIN_HEIGHT`.

- **`compose_position_is_unset`** (pub, L60)
  - Signature: `pub fn compose_position_is_unset(origin: Point<gpui::Pixels>) -> bool`
  - Purpose: Detects whether persisted coordinates mean "center on first open".
  - Behavior: Returns true when either x or y converts to a negative `f32`.

- **`compose_body_colors`** (private, L363)
  - Signature: `fn compose_body_colors(white_background: bool) -> (Option<Hsla>, Color)`
  - Purpose: Chooses compose body background and placeholder colors, mirroring the reader.
  - Behavior: When `white_background` is true, returns white fill plus a custom mid-gray placeholder; otherwise transparent background with `Color::Disabled` placeholder.

- **`window_title`** (pub, L425)
  - Signature: `pub fn window_title(language: Language) -> &'static str`
  - Purpose: Localized window title for compose windows.
  - Behavior: Resolves `Key::ComposeWindowTitle` for the given language.

- **`compose_body_uses_white_background_when_reader_pref_is_on`** (private, L436)
  - Signature: `fn compose_body_uses_white_background_when_reader_pref_is_on()` (test)
  - Purpose: Verifies white-background color mapping for compose body.
  - Behavior: Asserts `compose_body_colors(true)` yields a background and `compose_body_colors(false)` yields none with disabled placeholder color.

- **`clamp_compose_size_enforces_minimums`** (private, L445)
  - Signature: `fn clamp_compose_size_enforces_minimums()` (test)
  - Purpose: Ensures undersized stored dimensions are raised to minimums.
  - Behavior: Clamps 100×200 to `COMPOSE_MIN_WIDTH` × `COMPOSE_MIN_HEIGHT`.

- **`clamp_compose_size_leaves_large_sizes_unchanged`** (private, L452)
  - Signature: `fn clamp_compose_size_leaves_large_sizes_unchanged()` (test)
  - Purpose: Ensures sizes above minimums pass through unchanged.
  - Behavior: Clamps 900×800 and expects identical output.

- **`compose_position_unset_detects_negative_coordinates`** (private, L459)
  - Signature: `fn compose_position_unset_detects_negative_coordinates()` (test)
  - Purpose: Covers sentinel and normal coordinate detection.
  - Behavior: Negative x or y is unset; (0, 0) is considered set.

- **`window_title_is_localized`** (private, L469)
  - Signature: `fn window_title_is_localized()` (test)
  - Purpose: Checks English and Portuguese compose titles.
  - Behavior: Expects "New Message" and "Nova mensagem".

- **`new_view_defaults_to_first_account_and_hidden_cc_bcc`** (private, L475)
  - Signature: `fn new_view_defaults_to_first_account_and_hidden_cc_bcc()` (test)
  - Purpose: Validates initial `ComposeView` state with sample accounts.
  - Behavior: Expects `from_account == 0`, hidden Cc/Bcc, and first account email as From address.

- **`selected_from_address_is_empty_without_accounts`** (private, L484)
  - Signature: `fn selected_from_address_is_empty_without_accounts()` (test)
  - Purpose: Ensures empty account list yields empty From address.
  - Behavior: Constructs view with no accounts and expects `""`.

- **`toggle_cc_bcc_flips_visibility`** (private, L490)
  - Signature: `fn toggle_cc_bcc_flips_visibility()` (test)
  - Purpose: Documents expected toggle semantics (logic-only test).
  - Behavior: Flips a boolean twice to assert show/hide behavior pattern.

- **`cycle_from_account_wraps`** (private, L499)
  - Signature: `fn cycle_from_account_wraps()` (test)
  - Purpose: Documents wrap-around account cycling (logic-only test).
  - Behavior: Increments from last index modulo account count expecting index 0.

##### Context: `ComposeView`

- **`new`** (pub, L88)
  - Signature: `pub fn new(accounts: Vec<Account>, root: WeakEntity<RootView>, white_background: bool) -> Self`
  - Purpose: Creates a compose view seeded with accounts and preferences.
  - Behavior: Initializes first account selected, Cc/Bcc hidden, no reported bounds, and stores the weak root handle plus white-background flag.

- **`set_white_background`** (pub(crate), L100)
  - Signature: `pub(crate) fn set_white_background(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Applies white-compose preference pushed from the main view without reading `RootView` during render.
  - Behavior: Updates cached flag and calls `cx.notify()` only when the value changes.

- **`toggle_cc_bcc`** (private, L108)
  - Signature: `fn toggle_cc_bcc(&mut self, cx: &mut Context<Self>)`
  - Purpose: Shows or hides Cc and Bcc header rows.
  - Behavior: Flips `show_cc_bcc` and notifies for re-render.

- **`cycle_from_account`** (private, L114)
  - Signature: `fn cycle_from_account(&mut self, cx: &mut Context<Self>)`
  - Purpose: Mock From-account selector cycling through available accounts.
  - Behavior: No-op when `accounts` is empty; otherwise increments index modulo length and notifies.

- **`send_message`** (pub(crate), L123)
  - Signature: `pub(crate) fn send_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Placeholder send action until the domain layer exists.
  - Behavior: Intentionally empty (ignores `cx`).

- **`attach_file`** (pub(crate), L128)
  - Signature: `pub(crate) fn attach_file(&mut self, cx: &mut Context<Self>)`
  - Purpose: Placeholder attach action until a file picker exists.
  - Behavior: Intentionally empty (ignores `cx`).

- **`discard_draft`** (pub(crate), L133)
  - Signature: `pub(crate) fn discard_draft(&mut self, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Mock discard that closes the compose window.
  - Behavior: Delegates to `close_window`.

- **`close_window`** (pub(crate), L138)
  - Signature: `pub(crate) fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Closes the compose window and notifies the main view.
  - Behavior: Defers work: upgrades `root` to call `on_compose_window_closed`, then removes the window. Avoids synchronous root updates during close handling.

- **`sync_menus_if_active`** (private, L150)
  - Signature: `fn sync_menus_if_active(&self, window: &Window, language: Language, cx: &mut Context<Self>)`
  - Purpose: Keeps the global menu bar on the compose surface when this window is active.
  - Behavior: Calls `app_menus::sync_compose_menus` when `window.is_window_active()`.

- **`selected_from_address`** (private, L156)
  - Signature: `fn selected_from_address(&self) -> &str`
  - Purpose: Returns the email of the currently selected From account.
  - Behavior: Looks up `accounts[from_account]` email or returns empty string.

- **`render_toolbar`** (private, L163)
  - Signature: `fn render_toolbar(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Builds the bottom toolbar (discard, attach, send).
  - Behavior: Renders bordered row with trash icon (discard), flexible spacer, attachment icon, and filled Send button wired to mock handlers.

- **`render_header`** (private, L206)
  - Signature: `fn render_header(&self, language: Language, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Builds the compose header fields (From, To, optional Cc/Bcc, Subject).
  - Behavior: From row cycles accounts on click; To row shows placeholder and Cc/Bcc toggle; optional Cc/Bcc rows and Subject row use `field_row`. Uses theme border and muted label colors.

- **`cc_bcc_toggle`** (private, L288)
  - Signature: `fn cc_bcc_toggle(&self, language: Language, text_color: Hsla, hover_bg: Hsla, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Compact Cc/Bcc toggle aligned with 12px field rows.
  - Behavior: Renders localized label with hover background, stops mouse-down propagation, and toggles Cc/Bcc visibility on click.

- **`field_row`** (private, L312)
  - Signature: `fn field_row(&self, label: &'static str, placeholder: impl Into<SharedString>, value_slot: Option<impl IntoElement>, border: Hsla) -> impl IntoElement`
  - Purpose: One labeled header row with optional interactive value slot.
  - Behavior: Fixed-width muted label, bottom border, and either a custom value element or disabled placeholder label.

- **`render_body`** (private, L352)
  - Signature: `fn render_body(&self, language: Language, white_background: bool) -> impl IntoElement`
  - Purpose: Builds the message body placeholder area.
  - Behavior: Flex-grow region with optional white background and localized body placeholder text.

##### Context: `Render for ComposeView`

- **`render`** (private, L380)
  - Signature: `fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Renders the full compose window and wires actions/menus/bounds sync.
  - Behavior: When windowed bounds change, defers `root.sync_compose_window_bounds`. Syncs compose menus if active. Builds vertical layout with header, body, toolbar; registers `ComposeSend`, `ComposeAttach`, `ComposeDiscard`, and `ComposeClose` actions.


### `src/config.rs`

#### Types / constants

- **`Config`** (pub, L22)
  - Signature: `pub struct Config { window_x, window_y, window_width, window_height, maximized, max_x, max_y, max_width, max_height, sidebar_width, list_width, load_remote_images, reader_white_background, compose_white_background, collapsed_accounts, compose_x, compose_y, compose_width, compose_height }`
  - Purpose: Serializable persisted layout and privacy settings (logical pixels).
  - Behavior: Loaded from `~/.config/BGMail/config.json`. `#[serde(default)]` fills missing fields from `Default`. Tracks main-window geometry (restored and maximized frames), column widths, remote-image opt-in, reader/compose white-background prefs, collapsed sidebar account indices, and compose-window bounds.

#### Functions / methods

##### Context: `Default for Config`

- **`default`** (private, L71)
  - Signature: `fn default() -> Self`
  - Purpose: Supplies built-in defaults matching the app's initial layout.
  - Behavior: Sets main window 1100×720 at origin, sidebar 200px, list 360px, privacy flags off, empty collapsed list, and compose bounds centered-unset with default compose size from `compose` constants.

##### Context: `module`

- **`home_dir`** (private, L100)
  - Signature: `fn home_dir() -> Option<PathBuf>`
  - Purpose: Resolves the user's home directory without extra dependencies.
  - Behavior: Reads `HOME` (Linux/macOS) or falls back to `USERPROFILE` (Windows).

- **`config_path`** (pub, L108)
  - Signature: `pub fn config_path() -> Option<PathBuf>`
  - Purpose: Returns the fixed cross-platform config file path.
  - Behavior: Joins home with `.config/BGMail/config.json`; returns `None` when home cannot be determined.

- **`load`** (pub, L113)
  - Signature: `pub fn load() -> Config`
  - Purpose: Loads settings from disk with safe fallback.
  - Behavior: Reads via `config_path` and `load_from`; returns `Config::default()` when path or parse fails.

- **`save`** (pub, L118)
  - Signature: `pub fn save(config: &Config)`
  - Purpose: Persists settings best-effort.
  - Behavior: Writes through `save_to` when `config_path` exists; silently ignores errors.

- **`load_from`** (private, L125)
  - Signature: `fn load_from(path: PathBuf) -> Config`
  - Purpose: Parses one config file path.
  - Behavior: Reads UTF-8 text, deserializes JSON to `Config`, or returns defaults on any failure.

- **`save_to`** (private, L133)
  - Signature: `fn save_to(path: &Path, config: &Config) -> std::io::Result<()>`
  - Purpose: Writes pretty JSON, creating parent directories.
  - Behavior: `create_dir_all` on parent, serializes with `serde_json::to_string_pretty`, then writes the file.

- **`config_path_uses_fixed_dot_config_location`** (private, L147)
  - Signature: `fn config_path_uses_fixed_dot_config_location()` (test)
  - Purpose: Asserts config lives under `.config/BGMail/config.json`.
  - Behavior: When `config_path()` is `Some`, checks suffix and `.config` segment.

- **`json_round_trips`** (private, L155)
  - Signature: `fn json_round_trips()` (test)
  - Purpose: Verifies full struct serde round-trip.
  - Behavior: Serializes a populated `Config` and deserializes back with equality.

- **`remote_images_are_blocked_by_default`** (private, L183)
  - Signature: `fn remote_images_are_blocked_by_default()` (test)
  - Purpose: Ensures privacy default for remote images.
  - Behavior: Default config and partial JSON without the field keep `load_remote_images == false`.

- **`collapsed_accounts_default_empty_and_round_trip`** (private, L192)
  - Signature: `fn collapsed_accounts_default_empty_and_round_trip()` (test)
  - Purpose: Validates collapsed-account persistence field.
  - Behavior: Default is empty; partial JSON preserves stored indices.

- **`compose_white_background_defaults_off`** (private, L199)
  - Signature: `fn compose_white_background_defaults_off()` (test)
  - Purpose: Ensures compose white background defaults off.
  - Behavior: Default and partial JSON keep the flag false.

- **`reader_white_background_defaults_off`** (private, L206)
  - Signature: `fn reader_white_background_defaults_off()` (test)
  - Purpose: Ensures reader white background defaults off.
  - Behavior: Default and partial JSON keep the flag false.

- **`missing_fields_fall_back_to_defaults`** (private, L213)
  - Signature: `fn missing_fields_fall_back_to_defaults()` (test)
  - Purpose: Confirms `#[serde(default)]` partial load behavior.
  - Behavior: JSON with only `sidebar_width` keeps that value and fills other fields from defaults.

- **`compose_bounds_default_to_centered_open_size`** (private, L221)
  - Signature: `fn compose_bounds_default_to_centered_open_size()` (test)
  - Purpose: Validates default compose geometry sentinels.
  - Behavior: Default has negative compose x/y and default width/height constants.

- **`missing_compose_fields_fall_back_to_defaults`** (private, L230)
  - Signature: `fn missing_compose_fields_fall_back_to_defaults()` (test)
  - Purpose: Ensures compose fields default when absent from JSON.
  - Behavior: Partial JSON yields default compose width and x.

- **`invalid_json_loads_defaults`** (private, L237)
  - Signature: `fn invalid_json_loads_defaults()` (test)
  - Purpose: Ensures corrupt files do not crash loading.
  - Behavior: Writes invalid text to a temp file and expects `Config::default()` from `load_from`.

- **`save_then_load_round_trips_on_disk`** (private, L245)
  - Signature: `fn save_then_load_round_trips_on_disk()` (test)
  - Purpose: End-to-end disk persistence test.
  - Behavior: Saves a non-default config to a temp path and reloads with equality.

- **`unique_temp_path`** (private, L273)
  - Signature: `fn unique_temp_path() -> PathBuf` (test helper)
  - Purpose: Generates a unique temp JSON path per test run.
  - Behavior: Combines system temp dir, process id, and nanosecond timestamp.


### `src/data.rs`

#### Types / constants

- **`EMBEDDED_IMAGE_BYTES`** (private, L17)
  - Signature: `const EMBEDDED_IMAGE_BYTES: &[u8]`
  - Purpose: Raw PNG bytes baked into the binary for the first sample message.
  - Behavior: Loaded via `include_bytes!` from `assets/tweezers.png`; encoded to a `data:` URI for inline webview rendering without file access.

- **`EMBEDDED_IMAGE_WIDTH`** (private, L22)
  - Signature: `const EMBEDDED_IMAGE_WIDTH: u32 = 700`
  - Purpose: Display width (px) for the embedded sample image.
  - Behavior: Matches asset intrinsic width; referenced in generated HTML `width` attributes and layout tests.

- **`EMBEDDED_IMAGE_HEIGHT`** (private, L23)
  - Signature: `const EMBEDDED_IMAGE_HEIGHT: u32 = 200`
  - Purpose: Display height (px) for the embedded sample image.
  - Behavior: Matches asset intrinsic height; referenced in generated HTML `height` attributes.

- **`MailboxKind`** (pub, L27)
  - Signature: `pub enum MailboxKind { Inbox, Drafts, Sent, Junk, Trash, Archive, Custom }`
  - Purpose: Semantic kind of a mailbox for icons and localized naming.
  - Behavior: Standard kinds map to locale keys; `Custom` carries its label on `Mailbox::label` instead.

- **`GlobalMailbox`** (pub, L66)
  - Signature: `pub enum GlobalMailbox { Inbox, Flagged, Drafts, Sent }`
  - Purpose: Unified sidebar mailbox aggregating the same logical folder across accounts (mock-only).
  - Behavior: Display order defined by `ALL`; each variant has a dedicated locale key.

- **`GlobalMailbox::ALL`** (pub, L75)
  - Signature: `pub const ALL: [GlobalMailbox; 4]`
  - Purpose: Ordered list of global mailboxes for sidebar rendering.
  - Behavior: `[Inbox, Flagged, Drafts, Sent]`.

- **`Mailbox`** (pub, L101)
  - Signature: `pub struct Mailbox { kind, unread, label }`
  - Purpose: One mailbox row within an account in mock data.
  - Behavior: Standard mailboxes use kind-derived names; custom folders set `label` and `kind: Custom`.

- **`Account`** (pub, L140)
  - Signature: `pub struct Account { name, email, mailboxes }`
  - Purpose: Mock connected e-mail account with nested mailboxes.
  - Behavior: Used by sidebar, settings, and compose mock selectors.

- **`MessageBody`** (pub, L149)
  - Signature: `pub enum MessageBody { Html(SharedString), Text(SharedString) }`
  - Purpose: Reader content variant for HTML or plain-text bodies.
  - Behavior: Carries full body string; HTML is rendered in the webview, text in a plain viewer.

- **`Message`** (pub, L156)
  - Signature: `pub struct Message { sender, sender_email, subject, preview, body, time, unread, starred, has_attachment }`
  - Purpose: Mock list/detail message model for the visual prototype.
  - Behavior: Populates message list and seeds storage via `db_seed`.

- **`BASE64_ALPHABET`** (private, L535)
  - Signature: `const BASE64_ALPHABET: &[u8; 64]`
  - Purpose: RFC 4648 base64 alphabet for the inline encoder.
  - Behavior: Used by `base64_encode` when building embedded image data URIs.

#### Functions / methods

##### Context: `MailboxKind`

- **`name_key`** (private, L42)
  - Signature: `fn name_key(self) -> Option<Key>`
  - Purpose: Locale key for standard mailbox names.
  - Behavior: Returns `Some(Key::…)` for built-in kinds and `None` for `Custom`.

- **`display_name`** (pub, L56)
  - Signature: `pub fn display_name(self, language: Language) -> &'static str`
  - Purpose: Localized name for a standard mailbox kind.
  - Behavior: Translates via `name_key`; returns empty string for `Custom` (see `Mailbox::display_name`).

##### Context: `GlobalMailbox`

- **`name_key`** (private, L83)
  - Signature: `fn name_key(self) -> Key`
  - Purpose: Locale key for a global mailbox label.
  - Behavior: Maps each variant to its corresponding `Key` (Inbox reuses mailbox inbox key).

- **`display_name`** (pub, L93)
  - Signature: `pub fn display_name(self, language: Language) -> &'static str`
  - Purpose: Localized label for a global mailbox.
  - Behavior: Resolves `name_key().tr(language)`.

##### Context: `Mailbox`

- **`new`** (private, L110)
  - Signature: `fn new(kind: MailboxKind, unread: usize) -> Self`
  - Purpose: Constructs a standard mailbox with no custom label.
  - Behavior: Sets `label: None` and stores kind plus unread count.

- **`custom`** (private, L119)
  - Signature: `fn custom(name: impl Into<SharedString>, unread: usize) -> Self`
  - Purpose: Constructs a user-created folder mailbox.
  - Behavior: Sets `kind: Custom` and stores explicit `label`.

- **`display_name`** (pub, L130)
  - Signature: `pub fn display_name(&self, language: Language) -> SharedString`
  - Purpose: Sidebar/settings display name for any mailbox.
  - Behavior: Returns custom label when present; otherwise localized kind name.

##### Context: `module`

- **`default_mailboxes`** (private, L169)
  - Signature: `fn default_mailboxes(inbox_unread: usize) -> Vec<Mailbox>`
  - Purpose: Standard six-mailbox set for mock accounts.
  - Behavior: Inbox (with given unread), Drafts, Sent, Junk (3 unread), Trash, Archive.

- **`sample_accounts`** (pub, L182)
  - Signature: `pub fn sample_accounts() -> Vec<Account>`
  - Purpose: Multi-account mock data exercising sidebar overflow and custom folders.
  - Behavior: Returns five accounts with varied unread counts; Work account adds six custom folders including a long-name truncation case.

- **`sample_messages`** (pub, L230)
  - Signature: `pub fn sample_messages() -> Vec<Message>`
  - Purpose: Mock inbox message list with diverse metadata and bodies.
  - Behavior: Builds 18 messages from static tuples; first message uses rich HTML with embedded `data:` image; others cycle through `sample_email_bodies()`.

- **`html_body`** (private, L450)
  - Signature: `fn html_body(subject: &str, preview: &str, sender: &str, image_src: &str) -> SharedString`
  - Purpose: Generates rich HTML exercising reader features.
  - Behavior: Includes headings, lists, blockquote, code block, inline/remote images, normal and ~2000-character links for status-bar overflow tests.

- **`sample_email_bodies`** (pub, L515)
  - Signature: `pub fn sample_email_bodies() -> Vec<MessageBody>`
  - Purpose: Catalog of reusable HTML/text fixtures from `assets/emails/`.
  - Behavior: Returns 14 `include_str!` bodies (mostly HTML, one plain text) for mocks and tests.

- **`base64_encode`** (private, L540)
  - Signature: `fn base64_encode(input: &[u8]) -> String`
  - Purpose: Minimal dependency-free base64 encoder with padding.
  - Behavior: Encodes 3-byte chunks using `BASE64_ALPHABET`; emits `=` padding as needed.

- **`embedded_image_data_uri`** (private, L564)
  - Signature: `fn embedded_image_data_uri() -> String`
  - Purpose: Self-contained PNG data URI for the first sample message.
  - Behavior: Prefixes `data:image/png;base64,` with base64 of `EMBEDDED_IMAGE_BYTES`.

- **`accounts_have_default_mailboxes`** (private, L576)
  - Signature: `fn accounts_have_default_mailboxes()` (test)
  - Purpose: Validates every sample account has six standard mailboxes in order.
  - Behavior: Asserts five accounts, each with at least six mailboxes matching expected kinds.

- **`work_account_has_custom_folders`** (private, L597)
  - Signature: `fn work_account_has_custom_folders()` (test)
  - Purpose: Ensures Work account custom folder seeding.
  - Behavior: Expects 12 mailboxes total and six `Custom` folders; first custom named "Clients".

- **`global_mailboxes_are_localized`** (private, L616)
  - Signature: `fn global_mailboxes_are_localized()` (test)
  - Purpose: Checks global mailbox translations and inbox label reuse.
  - Behavior: Compares English/Portuguese Flagged labels; Inbox matches `MailboxKind::Inbox` label.

- **`custom_folder_name_is_not_localized`** (private, L633)
  - Signature: `fn custom_folder_name_is_not_localized()` (test)
  - Purpose: Ensures custom folder labels are language-independent.
  - Behavior: Same display name in English and Portuguese for a custom folder.

- **`sample_messages_are_populated`** (private, L642)
  - Signature: `fn sample_messages_are_populated()` (test)
  - Purpose: Sanity-checks mock message list diversity.
  - Behavior: Non-empty list with at least one unread and one attachment flag.

- **`sample_email_bodies_has_at_least_ten_varied_contents`** (private, L650)
  - Signature: `fn sample_email_bodies_has_at_least_ten_varied_contents()` (test)
  - Purpose: Guards fixture catalog size.
  - Behavior: Asserts at least ten bodies in `sample_email_bodies()`.

- **`sample_email_bodies_are_all_non_empty`** (private, L660)
  - Signature: `fn sample_email_bodies_are_all_non_empty()` (test)
  - Purpose: Ensures no empty fixture files.
  - Behavior: Trims each HTML/text body and rejects empty strings.

- **`sample_email_bodies_mix_html_and_text`** (private, L670)
  - Signature: `fn sample_email_bodies_mix_html_and_text()` (test)
  - Purpose: Confirms fixture catalog includes both formats.
  - Behavior: Asserts at least one Html and one Text variant in the catalog.

- **`sample_messages_include_html_and_text_bodies`** (private, L677)
  - Signature: `fn sample_messages_include_html_and_text_bodies()` (test)
  - Purpose: Confirms assembled messages use both body kinds.
  - Behavior: Scans `sample_messages()` for Html and Text bodies.

- **`first_message_embeds_the_image_as_a_data_uri`** (private, L688)
  - Signature: `fn first_message_embeds_the_image_as_a_data_uri()` (test)
  - Purpose: Validates inline image embedding in the first HTML message.
  - Behavior: Expects `data:image/png;base64,` substring and explicit width/height attributes.

- **`base64_encodes_known_vectors`** (private, L705)
  - Signature: `fn base64_encodes_known_vectors()` (test)
  - Purpose: RFC 4648 vector coverage for the inline encoder.
  - Behavior: Checks empty input and classic padding cases (`f`, `fo`, `foo`, etc.).

- **`embedded_image_data_uri_is_well_formed`** (private, L716)
  - Signature: `fn embedded_image_data_uri_is_well_formed()` (test)
  - Purpose: Ensures data URI prefix and non-empty payload.
  - Behavior: Starts with PNG data-URI scheme and has content beyond the prefix.

- **`embedded_image_is_a_decodable_raster`** (private, L723)
  - Signature: `fn embedded_image_is_a_decodable_raster()` (test)
  - Purpose: Guards against non-raster bytes in the embedded asset.
  - Behavior: Asserts PNG or JPEG magic bytes at start of `EMBEDDED_IMAGE_BYTES`.

- **`embedded_image_is_wider_than_a_typical_reading_pane`** (private, L737)
  - Signature: `fn embedded_image_is_wider_than_a_typical_reading_pane()` (test)
  - Purpose: Ensures horizontal scrollbar exercise in the reader.
  - Behavior: Requires `EMBEDDED_IMAGE_WIDTH >= 640`.

- **`unread_count_matches_first_account`** (private, L744)
  - Signature: `fn unread_count_matches_first_account()` (test)
  - Purpose: Spot-checks Personal account inbox unread mock count.
  - Behavior: Expects 5000 unread on first account's inbox.

- **`mailbox_names_are_localized`** (private, L750)
  - Signature: `fn mailbox_names_are_localized()` (test)
  - Purpose: Verifies inbox localization in English and Portuguese.
  - Behavior: Expects "Inbox" and "Caixa de entrada" for `MailboxKind::Inbox`.


### `src/db_seed.rs`

#### Types / constants

_(None at module top level — conversion helpers only.)_

#### Functions / methods

##### Context: `module`

- **`seed_accounts`** (pub, L12)
  - Signature: `pub fn seed_accounts() -> Vec<SeedAccount>`
  - Purpose: Builds account and folder seed records for initial SQLite population.
  - Behavior: Maps `data::sample_accounts()` into `SeedAccount` values, converting each mailbox via `seed_mailbox`.

- **`seed_mailbox`** (private, L23)
  - Signature: `fn seed_mailbox(mailbox: &Mailbox) -> SeedMailbox`
  - Purpose: Converts one mock mailbox into a storage seed record.
  - Behavior: Standard kinds map to `system::*` paths via `seed_system`; custom folders set `custom_name` from `mailbox.label` and leave `system_path` unset.

- **`seed_system`** (private, L39)
  - Signature: `fn seed_system(path: &'static str, unread: usize) -> SeedMailbox`
  - Purpose: Builds a seed record for a known system folder path.
  - Behavior: Sets `system_path: Some(path)`, no custom name, and copies unread count.

- **`seed_messages`** (pub, L48)
  - Signature: `pub fn seed_messages() -> Vec<SeedMessage>`
  - Purpose: Builds message seed records from mock sample messages.
  - Behavior: Assigns all messages to the first sample account email (fallback `you@gmail.com`), preserving sample order as `sort_order`.

- **`seed_message`** (private, L61)
  - Signature: `fn seed_message(account_email: &str, sort_order: i64, message: &Message) -> SeedMessage`
  - Purpose: Converts one mock `Message` into a storage seed row.
  - Behavior: Copies HTML/text raw content and format; derives `plain_text` via `plain_text_from_raw`; uses mock preview or `preview_from_plain` when preview empty; copies metadata flags and leaves `extra_folders` empty.

- **`global_folder_path`** (pub, L92)
  - Signature: `pub fn global_folder_path(global: GlobalMailbox) -> &'static str`
  - Purpose: Maps unified sidebar mailboxes to storage system folder paths.
  - Behavior: Returns `system::INBOX`, `FLAGGED`, `DRAFTS`, or `SENT` for the corresponding `GlobalMailbox` variant.

- **`folder_kind_from_path`** (pub, L102)
  - Signature: `pub fn folder_kind_from_path(path: &str) -> MailboxKind`
  - Purpose: Maps stored folder paths back to UI mailbox kinds for icons and localization.
  - Behavior: Matches known system paths; any other path becomes `MailboxKind::Custom`.

- **`folder_display_name`** (pub, L115)
  - Signature: `pub fn folder_display_name(path: &str, display_name: &str, language: crate::locale::Language) -> gpui::SharedString`
  - Purpose: Resolves sidebar folder row labels from storage metadata.
  - Behavior: Returns non-empty `display_name` when provided; otherwise localized name from `folder_kind_from_path(path)`.

- **`message_body_from_detail`** (pub, L127)
  - Signature: `pub fn message_body_from_detail(detail: &storage::MessageDetail) -> MessageBody`
  - Purpose: Converts stored message detail into the UI reader body enum.
  - Behavior: Wraps `raw_content` as `MessageBody::Html` when `raw_format == "html"`, otherwise `MessageBody::Text`.

- **`seed_messages_match_sample_count`** (private, L140)
  - Signature: `fn seed_messages_match_sample_count()` (test)
  - Purpose: Ensures seed message count tracks mock catalog size.
  - Behavior: Asserts `seed_messages().len()` equals `sample_messages().len()`.

- **`seed_accounts_include_custom_folders`** (private, L145)
  - Signature: `fn seed_accounts_include_custom_folders()` (test)
  - Purpose: Verifies custom folders survive account seeding.
  - Behavior: Finds Work account and asserts a mailbox with custom name "Clients" exists.

- **`plain_text_is_derived_from_html_bodies`** (private, L155)
  - Signature: `fn plain_text_is_derived_from_html_bodies()` (test)
  - Purpose: Sanity-checks HTML-to-plain derivation used during seeding.
  - Behavior: Strips tags from sample HTML and retains visible text content.


### `src/locale.rs`

#### Types / constants

- **`Language`** (pub, L15)
  - Signature: `pub enum Language { English, Portuguese }`
  - Purpose: Supported UI languages (English default).
  - Behavior: Stored as a GPUI global; resolved at render time via `ActiveLanguage`.

- **`Language::ALL`** (pub, L25)
  - Signature: `pub const ALL: [Language; 2]`
  - Purpose: Ordered list for language selectors and exhaustive translation tests.
  - Behavior: `[English, Portuguese]`.

- **`Key`** (pub, L38)
  - Signature: `pub enum Key { … }` (64 UI string keys)
  - Purpose: Typed catalog of translatable UI chrome strings.
  - Behavior: Each variant maps to per-language text via `tr`; covers mailboxes, settings, toolbar, commands, compose, context menu, and status strings.

- **`GlobalLanguage`** (private, L332)
  - Signature: `struct GlobalLanguage(Language)`
  - Purpose: GPUI global holding the active UI language.
  - Behavior: Wrapped by `init` / `set_language`; read through `ActiveLanguage for App`.

- **`ActiveLanguage`** (pub, L350)
  - Signature: `pub trait ActiveLanguage { fn language(&self) -> Language; }`
  - Purpose: Trait for reading the active language from GPUI contexts.
  - Behavior: Implemented for `App` to return the inner `Language` from `GlobalLanguage`.

#### Functions / methods

##### Context: `Language`

- **`label`** (pub, L28)
  - Signature: `pub fn label(self) -> &'static str`
  - Purpose: Endonym shown in language picker UI.
  - Behavior: Returns "English" or "Português (Brasil)" for each variant.

##### Context: `Key`

- **`tr`** (pub, L126)
  - Signature: `pub fn tr(self, language: Language) -> &'static str`
  - Purpose: Resolves a UI string key to localized text.
  - Behavior: Large `match` on `(self, language)` returning static English or Brazilian Portuguese strings; includes format placeholders like `{}` for `BlockedElements`.

##### Context: `module`

- **`message_count`** (pub, L295)
  - Signature: `pub fn message_count(language: Language, count: usize) -> String`
  - Purpose: Localized message-list header count.
  - Behavior: English `"{count} messages"`; Portuguese `"{count} mensagens"`.

- **`status_counts`** (pub, L303)
  - Signature: `pub fn status_counts(language: Language, accounts: usize, messages: usize) -> String`
  - Purpose: Status bar left segment with account and message totals.
  - Behavior: English `"{accounts} accounts · {messages} messages"`; Portuguese uses "contas" and "mensagens".

- **`status_search_counts`** (pub, L311)
  - Signature: `pub fn status_search_counts(language: Language, accounts: usize, showing: usize, total: usize) -> String`
  - Purpose: Status bar left segment while search filtering is active.
  - Behavior: English `"{accounts} accounts · {showing} of {total} messages"`; Portuguese uses "de" phrasing.

- **`status_unread`** (pub, L324)
  - Signature: `pub fn status_unread(language: Language, unread: usize) -> String`
  - Purpose: Status bar right segment with unread count and sync hint.
  - Behavior: English `"{unread} unread · Updated just now"`; Portuguese uses "não lidas · Atualizado agora".

- **`init`** (pub, L337)
  - Signature: `pub fn init(language: Language, cx: &mut App)`
  - Purpose: Initializes localization at app startup.
  - Behavior: Sets `GlobalLanguage(language)` on the GPUI app.

- **`set_language`** (pub, L342)
  - Signature: `pub fn set_language(language: Language, cx: &mut App)`
  - Purpose: Switches active language live across all windows.
  - Behavior: Updates global and calls `cx.refresh_windows()` so every open window re-renders with new strings.

- **`every_key_has_text_in_every_language`** (private, L433)
  - Signature: `fn every_key_has_text_in_every_language()` (test)
  - Purpose: Ensures translation completeness for all 64 keys.
  - Behavior: Iterates `ALL_KEYS` × `Language::ALL` asserting non-empty `tr` results.

- **`translations_differ_between_languages`** (private, L445)
  - Signature: `fn translations_differ_between_languages()` (test)
  - Purpose: Spot-checks that locales actually differ where expected.
  - Behavior: Compares inbox and settings title strings across English and Portuguese.

- **`default_language_is_english`** (private, L458)
  - Signature: `fn default_language_is_english()` (test)
  - Purpose: Confirms `Language::default()` is English.
  - Behavior: Equality assertion on default enum value.

- **`formatted_strings_include_arguments`** (private, L463)
  - Signature: `fn formatted_strings_include_arguments()` (test)
  - Purpose: Verifies dynamic status strings embed numeric arguments.
  - Behavior: Checks English status counts contain digits and "accounts"; Portuguese unread string contains count and "não lidas".

##### Context: `ActiveLanguage for App`

- **`language`** (private, L356)
  - Signature: `fn language(&self) -> Language`
  - Purpose: Reads active language from the GPUI app global.
  - Behavior: Returns `self.global::<GlobalLanguage>().0`.


### `src/main.rs`

#### Types / constants

- **struct `MainWindow`**: GPUI global holding the `WindowHandle<RootView>` for routing app-wide shortcuts and deferred menu actions.
- **struct `CommandPaletteShortcut`**: GPUI global keeping the keystroke interceptor subscription alive for the app lifetime.

#### Functions / methods

##### Context: `module`

- **`is_command_palette_keystroke`** (private, L56)
  - Signature: `fn is_command_palette_keystroke(keystroke: &Keystroke) -> bool`
  - Purpose: Detects Ctrl+P / Cmd+P command-palette shortcut.
  - Behavior: Matches key P (case-insensitive) without Shift when Control or platform (Cmd) modifier is held.

- **`toggle_command_palette`** (private, L62)
  - Signature: `fn toggle_command_palette(cx: &mut App)`
  - Purpose: Defers palette toggle to the main RootView.
  - Behavior: Reads `MainWindow` global and schedules `RootView::toggle_command_palette` on the next frame.

- **`schedule_menu_command`** (private, L71)
  - Signature: `fn schedule_menu_command(id: CommandId, cx: &mut App)`
  - Purpose: Defers a menu command to RootView if enabled.
  - Behavior: Builds command context, checks `command_enabled`, then calls `execute_command` when allowed.

- **`schedule_compose_action`** (private, L83)
  - Signature: `fn schedule_compose_action(cx: &mut App, action: impl FnOnce(...) + 'static)`
  - Purpose: Defers an action on the compose window.
  - Behavior: Routes closure through `RootView::with_compose_window` when compose is open.

- **`schedule_compose_close`** (private, L95)
  - Signature: `fn schedule_compose_close(cx: &mut App)`
  - Purpose: Defers closing the focused auxiliary window.
  - Behavior: Calls `RootView::close_focused_auxiliary_window` (compose or settings).

- **`register_global_menu_actions`** (private, L105)
  - Signature: `fn register_global_menu_actions(cx: &mut App)`
  - Purpose: Registers app-wide action handlers for macOS menu bar.
  - Behavior: Maps each GPUI action to deferred menu/compose handlers so items stay enabled when webview holds focus.

- **`init_cef_osr`** (private, L140)
  - Signature: `fn init_cef_osr()`
  - Purpose: Initializes CEF OSR when feature enabled.
  - Behavior: With `cef-osr`: calls `cef_osr::initialize` and logs failure. Without feature: no-op.

- **`shutdown_cef_osr`** (private, L151)
  - Signature: `fn shutdown_cef_osr()`
  - Purpose: Shuts down CEF after app exit.
  - Behavior: With `cef-osr`: calls `cef_osr::shutdown_cef`. Without feature: no-op.

- **`app_menus`** (private, L161)
  - Signature: `fn app_menus() -> Vec<Menu>`
  - Purpose: Builds initial macOS application menus at startup.
  - Behavior: Uses default command context and language; full menus refresh from RootView as state changes.

- **`register_command_palette_shortcuts`** (private, L168)
  - Signature: `fn register_command_palette_shortcuts(cx: &mut App)`
  - Purpose: Wires command palette toggle action and keystroke interceptor.
  - Behavior: Registers `ToggleCommandPalette` handler and intercepts Ctrl/Cmd+P globally, stopping propagation.

- **`main`** (private, L181)
  - Signature: `fn main()`
  - Purpose: Application entry point.
  - Behavior: Handles CEF sub-process re-exec, startup milestones, theme/locale init, window open with persisted bounds, Windows cloaked maximize sequence, persistence hooks, globals, and CEF shutdown after `Application::run` returns.

- **`command_palette_shortcut_matches_ctrl_p_and_cmd_p`** (private, L391)
  - Signature: `fn command_palette_shortcut_matches_ctrl_p_and_cmd_p()`
  - Purpose: Test: palette shortcut detection.
  - Behavior: Asserts Ctrl+P, Cmd+P, and uppercase P match; plain P does not.

### `src/root.rs`

#### Types / constants

- **const `SIDEBAR_MIN_WIDTH`**: Minimum sidebar column width (150px).
- **const `LIST_MIN_WIDTH`**: Minimum message-list column width (350px).
- **const `READER_MIN_WIDTH`**: Minimum reading-pane width (550px).
- **const `WINDOW_MIN_WIDTH` / `WINDOW_MIN_HEIGHT`**: Minimum main window size (950×480px); exported for startup clamping.
- **const `NARROW_BREAKPOINT`**: Window width (1200px) below which sidebar auto-collapses and floats when reopened.
- **const `RESIZE_HANDLE_WIDTH`**: Hit width of column resize dividers (6px).
- **const `FOLD_ANIM_MS`**: Fixed sidebar account fold animation duration (90ms).
- **const `SIDEBAR_ROW_HEIGHT`**: Fixed sidebar row height for fold math (32px).
- **const `CHEVRON_BOX` / `CHEVRON_ICON`**: Chevron container and glyph sizes for account disclosure.
- **const `ITEM_INDENT` / `ITEM_PADDING`**: Sidebar tree indentation and inner row padding.
- **const `DRAG_THRESHOLD`**: Pixels before toolbar mouse-down becomes window drag (2px).
- **const `SEARCH_FIELD_WIDTH` / `SEARCH_ICON_WIDTH`**: Expanded and collapsed search control widths.
- **const `SEARCH_DEBOUNCE_MS`**: Delay before search query reloads the message list (150ms).
- **const `READER_LIGHT_TEXT`**: Near-black body text when reader uses forced white background.
- **const `READER_LINE_SCROLL`**: Pixels per wheel line forwarded to OSR browser (40px).
- **const `SEARCH_COLLAPSE_WIDTH` / `TOOLBAR_FIXED_OVERHEAD` / `ACTIONS_*_WIDTH`**: Toolbar layout breakpoints for search collapse and action-group visibility.
- **enum `ResizeHandle`**: Which column divider is being dragged (`Sidebar` or `List`).
- **struct `ResizeDrag`**: GPUI drag payload wrapping `ResizeHandle`.
- **enum `Selection`**: Sidebar selection — unified `Global` mailbox or account `Mailbox` with folder path.
- **struct `FoldAnim`**: In-flight account fold animation token and direction.
- **enum `SettingsSection`**: Settings window section (General, Accounts, Appearance, Notifications, Privacy).
- **struct `WebviewSignature`**: Memo key for reader HTML (message id, colors, language, remote-load and white-bg prefs).
- **struct `RootView`**: Main application view state (layout, selection, webview, search, palette, auxiliary windows, persistence).
- **struct `SettingsView`**: Preferences window content with section navigation.

#### Functions / methods

- **`reader_mouse_button`** (private, L108)
  - Signature: `fn reader_mouse_button(button: MouseButton) -> Option<WebviewMouseButton>`
  - Purpose: Maps GPUI mouse buttons to webview buttons.
  - Behavior: Maps left/right/middle; drops navigation buttons unused in email bodies.
- **`webview_cursor_style`** (private, L118)
  - Signature: `fn webview_cursor_style(cursor: WebviewCursor) -> CursorStyle`
  - Purpose: Maps page cursor requests to GPUI cursor styles.
  - Behavior: Translates WebviewCursor variants (IBeam, Hand, resize cursors, etc.) to GPUI equivalents.
- **`mail_list_query`** (private, L179)
  - Signature: `fn mail_list_query(selection: &Selection, search: &str) -> MailListQuery`
  - Purpose: Builds SQLite list query for current sidebar selection or active search.
  - Behavior: Non-empty search returns `Search`; otherwise maps global or account folder selection to the appropriate query variant.
- **`mailbox_icon`** (private, L248)
  - Signature: `fn mailbox_icon(kind: MailboxKind) -> IconName`
  - Purpose: Icon for a mailbox kind in the sidebar.
  - Behavior: Maps each MailboxKind to a semantic IconName.
- **`global_mailbox_icon`** (private, L260)
  - Signature: `fn global_mailbox_icon(kind: GlobalMailbox) -> IconName`
  - Purpose: Icon for unified global mailboxes.
  - Behavior: Maps inbox/flagged/drafts/sent global mailboxes to icons.
- **`chevron_svg`** (private, L272)
  - Signature: `fn chevron_svg(color: Hsla, angle: f32) -> Svg`
  - Purpose: Rotatable chevron SVG for account disclosure.
  - Behavior: Renders right-pointing chevron rotated by `angle` radians (0= collapsed, π/2= expanded).
- **`ui_accounts`** (private, L461)
  - Signature: `fn ui_accounts(accounts: &[storage::Account]) -> Vec<data::Account>`
  - Purpose: Adapts storage accounts for legacy compose/settings UI types.
  - Behavior: Maps name/email fields; leaves mailboxes empty.
- **`settings_row`** (private, L3684)
  - Signature: `fn settings_row(label: impl Into<SharedString>, value: impl Into<SharedString>) -> impl IntoElement`
  - Purpose: Settings UI row with muted label and value.
  - Behavior: Horizontal flex row with spaced labels.
- **`should_dismiss_webview_context_menu`** (pub(crate), L3903)
  - Signature: `pub(crate) fn should_dismiss_webview_context_menu(click: Point<Pixels>, webview_bounds: Option<Bounds<Pixels>>) -> bool`
  - Purpose: Whether a host click should dismiss the in-page context menu.
  - Behavior: Returns false when click is inside reader bounds (CEF handles it); true for chrome clicks or when bounds unknown.
- **`can_open_command_palette`** (pub(crate), L3911)
  - Signature: `pub(crate) fn can_open_command_palette(main_window_active: bool) -> bool`
  - Purpose: Gate for opening the command palette.
  - Behavior: Returns `main_window_active` — palette only opens when main window is key.
##### Context: `SettingsSection`

- **`title_key`** (private, L227)
  - Signature: `fn title_key(self) -> Key`
  - Purpose: Locale key for section title.
  - Behavior: Maps each section to its Key variant.

- **`icon`** (private, L237)
  - Signature: `fn icon(self) -> IconName`
  - Purpose: Icon for section nav.
  - Behavior: Maps each section to Settings/Account/Palette/Flag/Shield icons.
##### Context: `RootView`

- **`new`** (pub, L473)
  - Signature: `pub fn new(settings: Config) -> Self`
  - Purpose: Constructs RootView with default database path.
  - Behavior: Delegates to `new_with_database` using `storage::database_path()`.

- **`new_with_database`** (private, L477)
  - Signature: `fn new_with_database(settings: Config, db_path: impl AsRef<Path>) -> Self`
  - Purpose: Constructs RootView with explicit DB path (tests).
  - Behavior: Opens SQLite, seeds if empty, loads folders/messages, restores layout from settings, initializes selection to global inbox.

- **`reload_message_list`** (private, L586)
  - Signature: `fn reload_message_list(&mut self)`
  - Purpose: Reloads list column from database.
  - Behavior: Queries messages for current selection/search and updates `list_messages`.

- **`clear_message_selection`** (private, L605)
  - Signature: `fn clear_message_selection(&mut self)`
  - Purpose: Clears selected message and reader state.
  - Behavior: Resets selected id, detail, and webview signature.

- **`reload_selected_detail`** (private, L611)
  - Signature: `fn reload_selected_detail(&mut self)`
  - Purpose: Reloads full body for selected message.
  - Behavior: Fetches MessageDetail from DB when an id is selected.

- **`on_message_selection_changed`** (private, L617)
  - Signature: `fn on_message_selection_changed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Handles message selection side effects.
  - Behavior: Reloads detail, syncs menus, notifies GPUI.

- **`command_context`** (pub(crate), L624)
  - Signature: `pub(crate) fn command_context(&self) -> CommandContext`
  - Purpose: Snapshot for menus and palette.
  - Behavior: Packages selected message id, detail clone, and folders map.

- **`sync_app_menus`** (pub(crate), L633)
  - Signature: `pub(crate) fn sync_app_menus(&self, cx: &mut Context<Self>)`
  - Purpose: Refreshes global menu bar from current state.
  - Behavior: Skips when compose window is active; otherwise syncs main menus with command context.

- **`on_compose_window_closed`** (pub(crate), L643)
  - Signature: `pub(crate) fn on_compose_window_closed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Cleanup when compose window closes.
  - Behavior: Clears compose handle and restores main menu surface.

- **`on_settings_window_closed`** (pub(crate), L650)
  - Signature: `pub(crate) fn on_settings_window_closed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Cleanup when settings window closes.
  - Behavior: Clears settings handle and notifies.

- **`close_focused_auxiliary_window`** (pub(crate), L657)
  - Signature: `pub(crate) fn close_focused_auxiliary_window(&mut self, cx: &mut Context<Self>)`
  - Purpose: Closes focused compose or settings window.
  - Behavior: Removes and closes whichever auxiliary window has focus.

- **`close_settings_window`** (pub(crate), L672)
  - Signature: `pub(crate) fn close_settings_window(&mut self, cx: &mut Context<Self>)`
  - Purpose: Closes settings window if open.
  - Behavior: Used by global shortcut fallback.

- **`compose_window_open_and_active`** (private, L680)
  - Signature: `fn compose_window_open_and_active(&self, cx: &mut Context<Self>) -> bool`
  - Purpose: Whether compose window exists and is active.
  - Behavior: Checks compose handle active state.

- **`with_compose_window`** (pub(crate), L686)
  - Signature: `pub(crate) fn with_compose_window<R>(&self, cx, f) -> Option<R>`
  - Purpose: Runs closure on compose view if open.
  - Behavior: Updates compose entity and returns closure result or None.

- **`refresh_after_message_action`** (private, L698)
  - Signature: `fn refresh_after_message_action(&mut self, cx: &mut Context<Self>)`
  - Purpose: Post-action UI refresh.
  - Behavior: Reloads list, syncs menus, invalidates webview signature, notifies.

- **`ensure_command_palette`** (private, L705)
  - Signature: `fn ensure_command_palette(&mut self, cx: &mut Context<Self>)`
  - Purpose: Lazily creates command palette state.
  - Behavior: Creates TextInput, wires query observer to sync filter, stores CommandPaletteState.

- **`sync_command_palette_query`** (private, L729)
  - Signature: `fn sync_command_palette_query(&mut self, cx: &mut Context<Self>)`
  - Purpose: Syncs palette filter from input entity.
  - Behavior: Reads input content, calls on_query_change when changed, notifies.

- **`toggle_command_palette`** (pub(crate), L744)
  - Signature: `pub(crate) fn toggle_command_palette(&mut self, window: &Window, cx: &mut Context<Self>)`
  - Purpose: Public palette toggle entry.
  - Behavior: Delegates to internal toggle.

- **`toggle_command_palette_from_webview`** (private, L748)
  - Signature: `fn toggle_command_palette_from_webview(&mut self, cx: &mut Context<Self>)`
  - Purpose: Schedules palette toggle from webview IPC.
  - Behavior: Sets deferred flag handled next render frame.

- **`toggle_command_palette_internal`** (private, L753)
  - Signature: `fn toggle_command_palette_internal(&mut self, window: &Window, cx: &mut Context<Self>)`
  - Purpose: Opens or closes the palette.
  - Behavior: Closes if open; otherwise requires active main window, clears query/selection, marks focus pending.

- **`dismiss_command_palette`** (pub(crate), L775)
  - Signature: `pub(crate) fn dismiss_command_palette(&mut self, cx: &mut Context<Self>)`
  - Purpose: Closes palette overlay.
  - Behavior: Sets open=false, clears focus pending, notifies.

- **`close_command_palette`** (pub(crate), L783)
  - Signature: `pub(crate) fn close_command_palette(&mut self, cx: &mut Context<Self>)`
  - Purpose: Alias for dismiss_command_palette.
  - Behavior: Calls dismiss_command_palette.

- **`command_palette_render_args`** (private, L788)
  - Signature: `fn command_palette_render_args(&self, cx: &App) -> Option<(...)>`
  - Purpose: Snapshot for palette overlay render.
  - Behavior: When open, returns filtered entries, clamped selection index, and input entity.

- **`execute_command`** (pub(crate), L805)
  - Signature: `pub(crate) fn execute_command(&mut self, id: &CommandId, window, cx)`
  - Purpose: Runs a command by id.
  - Behavior: Dispatches to compose/settings/sidebar/message handlers, then closes palette.

- **`delete_message_to_trash`** (pub(crate), L830)
  - Signature: `pub(crate) fn delete_message_to_trash(&mut self, cx: &mut Context<Self>)`
  - Purpose: Moves selected message to trash.
  - Behavior: No-op without selection; DB move then refresh_after_message_action.

- **`delete_message_permanently`** (pub(crate), L838)
  - Signature: `pub(crate) fn delete_message_permanently(&mut self, cx: &mut Context<Self>)`
  - Purpose: Permanently deletes selected message.
  - Behavior: Clears selection on success, then refreshes.

- **`restore_message`** (pub(crate), L848)
  - Signature: `pub(crate) fn restore_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Restores message from trash.
  - Behavior: DB restore then refresh.

- **`archive_message`** (pub(crate), L856)
  - Signature: `pub(crate) fn archive_message(&mut self, cx: &mut Context<Self>)`
  - Purpose: Archives selected message.
  - Behavior: DB archive then refresh.

- **`mark_message_junk`** (pub(crate), L864)
  - Signature: `pub(crate) fn mark_message_junk(&mut self, cx: &mut Context<Self>)`
  - Purpose: Marks selected message as junk.
  - Behavior: DB junk move then refresh.

- **`toggle_message_flag`** (pub(crate), L872)
  - Signature: `pub(crate) fn toggle_message_flag(&mut self, cx: &mut Context<Self>)`
  - Purpose: Toggles starred/flagged state.
  - Behavior: DB toggle then refresh.

- **`move_message_to_folder`** (pub(crate), L880)
  - Signature: `pub(crate) fn move_message_to_folder(&mut self, folder_path: &str, cx: &mut Context<Self>)`
  - Purpose: Moves message to folder path.
  - Behavior: DB move then refresh.

- **`handle_command_palette_key`** (pub(crate), L888)
  - Signature: `pub(crate) fn handle_command_palette_key(&mut self, event, window, cx) -> bool`
  - Purpose: Keyboard handling while palette open.
  - Behavior: Handles Escape, Up/Down (move selection), Enter (execute); returns whether event was consumed.

- **`persist_now`** (pub(crate), L953)
  - Signature: `pub(crate) fn persist_now(&self)`
  - Purpose: Immediate synchronous settings save.
  - Behavior: No-op until persist_ready; writes current_config via config::save.

- **`enable_persistence`** (pub(crate), L960)
  - Signature: `pub(crate) fn enable_persistence(&mut self)`
  - Purpose: Arms debounced persistence after startup.
  - Behavior: Sets persist_ready true (~700ms after window open).

- **`mark_content_ready`** (pub(crate), L968)
  - Signature: `pub(crate) fn mark_content_ready(&mut self, cx: &mut Context<Self>)`
  - Purpose: Reveals UI after Windows layout settles.
  - Behavior: Sets content_ready and notifies (Windows cloaked open path).

- **`current_config`** (private, L976)
  - Signature: `fn current_config(&self) -> Config`
  - Purpose: Builds Config from live layout state.
  - Behavior: Snapshots window/column bounds, maximize state, compose window geometry, and preferences.

- **`sync_compose_window_bounds`** (pub(crate), L1006)
  - Signature: `pub(crate) fn sync_compose_window_bounds(&mut self, origin, window_size, cx)`
  - Purpose: Tracks compose window move/resize.
  - Behavior: Updates stored compose origin/size and schedules debounced save.

- **`request_save`** (private, L1026)
  - Signature: `fn request_save(&mut self, cx: &mut Context<Self>)`
  - Purpose: Debounced settings persistence.
  - Behavior: Increments save token, spawns timer; latest token wins, writes on background thread.

- **`ensure_scrollbar_states`** (private, L1047)
  - Signature: `fn ensure_scrollbar_states(&mut self, cx: &mut Context<Self>)`
  - Purpose: Lazily creates scrollbar state entities.
  - Behavior: Creates list and sidebar ScrollbarState entities on first render.

- **`ensure_search_input`** (private, L1054)
  - Signature: `fn ensure_search_input(&mut self, language, window, cx)`
  - Purpose: Lazily creates toolbar search TextInput.
  - Behavior: Creates input with placeholder, blur/change handlers for debounced search sync.

- **`sync_search_if_needed`** (private, L1090)
  - Signature: `fn sync_search_if_needed(&mut self, cx: &mut Context<Self>)`
  - Purpose: Applies search query changes from render.
  - Behavior: When query differs from last applied, reloads list, adjusts mailbox selection, syncs selection scroll.

- **`next_search_debounce_token`** (private, L1112)
  - Signature: `fn next_search_debounce_token(&mut self) -> u64`
  - Purpose: Allocates next search debounce token.
  - Behavior: Increments search_debounce_seq.

- **`search_debounce_token_current`** (private, L1117)
  - Signature: `fn search_debounce_token_current(&self, token: u64) -> bool`
  - Purpose: Checks debounce token still current.
  - Behavior: Compares token to latest search_debounce_seq.

- **`schedule_search_sync`** (private, L1121)
  - Signature: `fn schedule_search_sync(&mut self, cx: &mut Context<Self>)`
  - Purpose: Schedules debounced search list reload.
  - Behavior: Spawns timer; stale tokens ignored; notifies on fire.

- **`search_query`** (private, L1139)
  - Signature: `fn search_query(&self, cx: &App) -> String`
  - Purpose: Current search field text.
  - Behavior: Reads search input entity or empty string.

- **`search_is_active`** (private, L1147)
  - Signature: `fn search_is_active(&self, _cx: &App) -> bool`
  - Purpose: Whether search filter is applied.
  - Behavior: True when last applied query is non-empty.

- **`show_search_expanded`** (private, L1153)
  - Signature: `fn show_search_expanded(&self) -> bool`
  - Purpose: Whether toolbar shows expanded search.
  - Behavior: True when not compact or force-expanded flag set.

- **`prepare_search_focus`** (private, L1158)
  - Signature: `fn prepare_search_focus(&mut self)`
  - Purpose: Expands compact search before focus.
  - Behavior: Sets search_force_expanded when compact.

- **`show_search_clear_in_field`** (private, L1164)
  - Signature: `fn show_search_clear_in_field(&self, cx: &App) -> bool`
  - Purpose: Whether clear button shows in search field.
  - Behavior: Delegates to query-based helper with live query.

- **`show_search_clear_in_field_for_query`** (private, L1168)
  - Signature: `fn show_search_clear_in_field_for_query(&self, query: &str) -> bool`
  - Purpose: Clear button visibility rules.
  - Behavior: Always when force-expanded compact; otherwise only when query non-empty on wide toolbar.

- **`clear_search`** (private, L1177)
  - Signature: `fn clear_search(&mut self, cx: &mut Context<Self>)`
  - Purpose: Clears search and restores pre-search mailbox.
  - Behavior: Clears input, resets force-expanded, restores mailbox if searching, reloads list.

- **`sync_search_selection`** (private, L1205)
  - Signature: `fn sync_search_selection(&mut self)`
  - Purpose: Keeps selection valid after search/filter.
  - Behavior: Selects first message if current missing; scrolls list to top on query change.

- **`focus_search_input`** (private, L1222)
  - Signature: `fn focus_search_input(&mut self, window, cx)`
  - Purpose: Focuses toolbar search field.
  - Behavior: Ensures input exists, prepares expand, focuses handle.

- **`search_input_focused`** (private, L1235)
  - Signature: `fn search_input_focused(&self, window, cx) -> bool`
  - Purpose: Whether search input has focus.
  - Behavior: Checks focus handle on search entity.

- **`blur_search_if_focused`** (private, L1242)
  - Signature: `fn blur_search_if_focused(&mut self, window, cx)`
  - Purpose: Blurs search when focused.
  - Behavior: Clears focus on search input entity.

- **`should_collapse_search_after_blur`** (private, L1249)
  - Signature: `fn should_collapse_search_after_blur(&self, query: &str) -> bool`
  - Purpose: Whether compact search should collapse on blur.
  - Behavior: True when compact, force-expanded, and query empty.

- **`on_search_blur`** (private, L1254)
  - Signature: `fn on_search_blur(&mut self, cx: &mut Context<Self>)`
  - Purpose: Handles search field blur.
  - Behavior: Collapses force-expanded compact search when appropriate.

- **`on_main_window_activated`** (pub(crate), L1264)
  - Signature: `pub(crate) fn on_main_window_activated(&mut self, cx: &mut Context<Self>)`
  - Purpose: Wakes OSR reader after window activation.
  - Behavior: Pumps CEF hard and notifies webview when composited reader exists.

- **`ensure_cef_osr_tick`** (private, L1277)
  - Signature: `fn ensure_cef_osr_tick(&mut self, cx: &mut Context<Self>)`
  - Purpose: Starts background CEF pump loop.
  - Behavior: Spawns periodic tick while OSR webview exists so Chromium keeps painting when window inactive.

- **`sync_webview`** (private, L1316)
  - Signature: `fn sync_webview(&mut self, window, cx: &mut Context<Self>)`
  - Purpose: Creates/updates embedded email webview.
  - Behavior: Builds HTML when signature changes, reuses browser instance on message switch, hides when no selection, handles host events.

- **`handle_host_event`** (private, L1422)
  - Signature: `fn handle_host_event(&mut self, event: HostEvent, cx: &mut Context<Self>)`
  - Purpose: Handles webview IPC on foreground.
  - Behavior: Routes hover, clipboard, image shown, palette toggle, cursor, context menu dismiss, etc.

- **`reader_relative_position`** (private, L1457)
  - Signature: `fn reader_relative_position(&self, position) -> Option<(f32, f32)>`
  - Purpose: Window coords to reader-local coords.
  - Behavior: Subtracts webview_bounds origin; None before first layout.

- **`on_reader_mouse_move`** (private, L1467)
  - Signature: `fn on_reader_mouse_move(&mut self, event, _window, cx)`
  - Purpose: Forwards mouse move to OSR (Linux).
  - Behavior: Translates coords, forwards to webview with modifiers; updates cursor style.

- **`on_reader_mouse_down`** (private, L1497)
  - Signature: `fn on_reader_mouse_down(&mut self, event, window, cx)`
  - Purpose: Forwards mouse down to OSR.
  - Behavior: Ensures reader focus, forwards press, tracks click count for double-click.

- **`on_reader_mouse_up`** (private, L1523)
  - Signature: `fn on_reader_mouse_up(&mut self, event, _window, cx)`
  - Purpose: Forwards mouse up to OSR.
  - Behavior: Translates coords and releases button in webview.

- **`on_reader_scroll`** (private, L1544)
  - Signature: `fn on_reader_scroll(&mut self, event, _window, cx)`
  - Purpose: Forwards wheel scroll to OSR.
  - Behavior: Converts scroll delta (lines vs pixels) and sends wheel event.

- **`on_reader_key_down`** (private, L1568)
  - Signature: `fn on_reader_key_down(&mut self, event, _window, cx)`
  - Purpose: Forwards key down to OSR.
  - Behavior: Maps modifiers and sends key press to webview.

- **`on_reader_key_up`** (private, L1591)
  - Signature: `fn on_reader_key_up(&mut self, event, _window, cx)`
  - Purpose: Forwards key up to OSR.
  - Behavior: Sends key release to webview.

- **`ensure_reader_focus`** (private, L1614)
  - Signature: `fn ensure_reader_focus(&mut self, cx) -> Option<&FocusHandle>`
  - Purpose: Lazily creates reader focus handle.
  - Behavior: Creates FocusHandle on first use for keyboard routing.

- **`image_shown`** (private, L1629)
  - Signature: `fn image_shown(&mut self, url: String, cx: &mut Context<Self>)`
  - Purpose: Records user revealed a blocked remote image.
  - Behavior: Adds URL to per-message shown set, decrements blocked count, notifies.

- **`set_hovered_link`** (private, L1639)
  - Signature: `fn set_hovered_link(&mut self, url: String, cx: &mut Context<Self>)`
  - Purpose: Updates status bar hovered link.
  - Behavior: Stores URL and notifies only when changed.

- **`set_load_remote_images`** (private, L1648)
  - Signature: `fn set_load_remote_images(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Toggles global remote image loading pref.
  - Behavior: Persists, invalidates webview signature, notifies for re-sanitize.

- **`set_reader_white_background`** (private, L1658)
  - Signature: `fn set_reader_white_background(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Toggles reader white background pref.
  - Behavior: Persists and invalidates webview signature.

- **`set_compose_white_background`** (private, L1668)
  - Signature: `fn set_compose_white_background(&mut self, value: bool, cx: &mut Context<Self>)`
  - Purpose: Toggles compose white background pref.
  - Behavior: Persists and updates open compose view if any.

- **`open_settings`** (pub(crate), L1683)
  - Signature: `pub(crate) fn open_settings(&mut self, _window, cx: &mut Context<Self>)`
  - Purpose: Opens or focuses settings window.
  - Behavior: Creates SettingsView window with persisted bounds or activates existing.

- **`open_compose`** (pub(crate), L1729)
  - Signature: `pub(crate) fn open_compose(&mut self, _window, cx: &mut Context<Self>)`
  - Purpose: Opens or focuses compose window.
  - Behavior: Creates ComposeView window or activates existing; syncs compose menus.

- **`select_mailbox`** (private, L1780)
  - Signature: `fn select_mailbox(&mut self, selection: Selection, cx: &mut Context<Self>)`
  - Purpose: Selects sidebar mailbox with UI update.
  - Behavior: Applies selection, reloads list, clears message selection, syncs menus, notifies.

- **`apply_mailbox_selection`** (private, L1798)
  - Signature: `fn apply_mailbox_selection(&mut self, selection: Selection)`
  - Purpose: Core mailbox selection without notify.
  - Behavior: Sets selected_mailbox; dismisses floating sidebar in narrow layout.

- **`note_scroll`** (private, L1807)
  - Signature: `fn note_scroll(states, cx: &mut Context<Self>)`
  - Purpose: Marks scrollbars active after scroll.
  - Behavior: Calls note_scroll on states and schedules fade-out re-render.

- **`toggle_sidebar`** (pub(crate), L1829)
  - Signature: `pub(crate) fn toggle_sidebar(&mut self)`
  - Purpose: Shows/hides accounts sidebar.
  - Behavior: Flips show_sidebar flag.

- **`is_account_collapsed`** (private, L1834)
  - Signature: `fn is_account_collapsed(&self, account_idx: usize) -> bool`
  - Purpose: Whether account mailbox list is collapsed.
  - Behavior: Checks collapsed_accounts set.

- **`toggle_account`** (private, L1841)
  - Signature: `fn toggle_account(&mut self, account_idx: usize) -> FoldAnim`
  - Purpose: Toggles account expand/collapse with animation.
  - Behavior: Updates collapsed set, assigns fold token, returns FoldAnim for timer.

- **`clear_fold`** (private, L1860)
  - Signature: `fn clear_fold(&mut self, account_idx: usize, token: u64) -> bool`
  - Purpose: Finalizes fold animation.
  - Behavior: Removes fold_anim if token matches; returns whether state changed.

- **`account_list_visible`** (private, L1875)
  - Signature: `fn account_list_visible(&self, account_idx: usize) -> bool`
  - Purpose: Whether account rows should render.
  - Behavior: Visible when expanded or mid-collapse animation.

- **`sidebar_docked`** (private, L1886)
  - Signature: `fn sidebar_docked(&self) -> bool`
  - Purpose: Whether sidebar occupies a layout column.
  - Behavior: True when sidebar shown and not narrow floating mode.

- **`reader_segment_width`** (private, L1892)
  - Signature: `fn reader_segment_width(&self) -> Pixels`
  - Purpose: Toolbar width for reader segment.
  - Behavior: Window width minus sidebar and list columns (and handles).

- **`search_is_compact`** (private, L1913)
  - Signature: `fn search_is_compact(&self) -> bool`
  - Purpose: Whether search collapses to icon.
  - Behavior: True when reader segment narrower than SEARCH_COLLAPSE_WIDTH.

- **`visible_action_groups`** (private, L1920)
  - Signature: `fn visible_action_groups(&self) -> usize`
  - Purpose: Count of toolbar action groups that fit.
  - Behavior: 0–3 based on reader segment width thresholds.

- **`render_list_controls`** (private, L1941)
  - Signature: `fn render_list_controls(&self, show_count: bool, cx) -> impl IntoElement`
  - Purpose: Message list header controls.
  - Behavior: Mailbox title, optional count, filter/more buttons; used in toolbar or list header.

- **`sync_layout`** (private, L2007)
  - Signature: `fn sync_layout(&mut self, total: Pixels)`
  - Purpose: Reconciles column widths to window size.
  - Behavior: Tracks narrow breakpoint, auto sidebar collapse/restore, clamps columns to minimums preserving reader floor.

- **`resize`** (private, L2046)
  - Signature: `fn resize(&mut self, handle: ResizeHandle, x: Pixels, total: Pixels)`
  - Purpose: Applies column divider drag.
  - Behavior: Sets sidebar or list width from cursor x, clamped so reader keeps READER_MIN_WIDTH.

- **`render_toolbar`** (private, L2067)
  - Signature: `fn render_toolbar(&mut self, window, cx) -> impl IntoElement`
  - Purpose: Renders unified top toolbar.
  - Behavior: Window drag region, list controls when docked, compose/actions/search, theme and sidebar toggles.

- **`render_search_control`** (private, L2321)
  - Signature: `fn render_search_control(&self, compact_search, search_bg, language, cx) -> AnyElement`
  - Purpose: Renders compact or expanded search UI.
  - Behavior: Icon button or TextInput with clear affordance based on layout state.

- **`render_search_clear_button`** (private, L2377)
  - Signature: `fn render_search_clear_button(language, cx) -> impl IntoElement`
  - Purpose: Clear button inside search field.
  - Behavior: Small icon button calling clear_search.

- **`icon_button_with_tooltip`** (private, L2401)
  - Signature: `fn icon_button_with_tooltip(id, label, shortcut, button) -> impl IntoElement`
  - Purpose: IconButton wrapped with tooltip div.
  - Behavior: Adds localized label and optional shortcut display.

- **`render_toolbar_separator`** (private, L2419)
  - Signature: `fn render_toolbar_separator(color: Hsla) -> impl IntoElement`
  - Purpose: Vertical toolbar divider.
  - Behavior: 1px × 20px colored bar.

- **`render_sidebar`** (private, L2425)
  - Signature: `fn render_sidebar(&self, floating: bool, cx) -> impl IntoElement`
  - Purpose: Renders accounts/folders sidebar.
  - Behavior: Global section, account groups with fold animations, scrollbars; floating adds shadow/overlay styling.

- **`render_resize_handle`** (private, L2468)
  - Signature: `fn render_resize_handle(&self, kind: ResizeHandle, _cx) -> impl IntoElement`
  - Purpose: Draggable column resize strip.
  - Behavior: Hit target on column edge; drag handled at row level.

- **`global_unread`** (private, L2492)
  - Signature: `fn global_unread(&self, global: GlobalMailbox) -> usize`
  - Purpose: Aggregated unread for global mailbox.
  - Behavior: Sums inbox unreads or counts flagged/starred depending on mailbox.

- **`render_global_section`** (private, L2500)
  - Signature: `fn render_global_section(&self, cx) -> impl IntoElement`
  - Purpose: Unified mailboxes at sidebar top.
  - Behavior: Inbox/Flagged/Drafts/Sent rows with icons, selection, unread badges.

- **`render_account`** (private, L2541)
  - Signature: `fn render_account(&self, account_idx, account, cx) -> impl IntoElement`
  - Purpose: Renders one account group in sidebar.
  - Behavior: Header with chevron toggle, animated mailbox rows, folder selection.

- **`render_account_chevron`** (private, L2698)
  - Signature: `fn render_account_chevron(&self, account_idx, color) -> AnyElement`
  - Purpose: Animated disclosure chevron.
  - Behavior: Rotates between collapsed/expanded based on fold animation state.

- **`render_count_badge`** (private, L2731)
  - Signature: `fn render_count_badge(&self, count, selected, cx) -> impl IntoElement`
  - Purpose: Unread/count pill on sidebar rows.
  - Behavior: Shows count when non-zero with selected/unselected styling.

- **`render_message_list`** (private, L2766)
  - Signature: `fn render_message_list(&mut self, cx) -> impl IntoElement`
  - Purpose: Renders scrollable message list column.
  - Behavior: Optional list header when sidebar undocked, message rows, scrollbar.

- **`render_message_row`** (private, L2847)
  - Signature: `fn render_message_row(&self, message, cx) -> impl IntoElement`
  - Purpose: Single message list row.
  - Behavior: Sender, subject preview, date, unread/flag/attachment indicators; click selects message.

- **`shield_is_blocked`** (private, L2952)
  - Signature: `fn shield_is_blocked(&self) -> bool`
  - Purpose: Red privacy shield state.
  - Behavior: Remote content present, blocking on, message not fully unblocked.

- **`shield_is_loaded`** (private, L2959)
  - Signature: `fn shield_is_loaded(&self) -> bool`
  - Purpose: Green privacy shield state.
  - Behavior: Had remote images and blocked count is zero.

- **`render_privacy_shield`** (private, L2968)
  - Signature: `fn render_privacy_shield(&self, cx) -> impl IntoElement`
  - Purpose: Privacy shield on reader subject line.
  - Behavior: Red clickable shield when blocked, green when loaded, empty slot otherwise for layout stability.

- **`render_privacy_menu`** (private, L3038)
  - Signature: `fn render_privacy_menu(&self, cx) -> impl IntoElement`
  - Purpose: Dropdown from privacy shield.
  - Behavior: Deferred anchored menu to load remote images for this message; hides webview while open.

- **`render_reader`** (private, L3100)
  - Signature: `fn render_reader(&mut self, cx) -> impl IntoElement`
  - Purpose: Renders reading pane.
  - Behavior: Empty state, header with subject/sender/actions, composited webview or text fallback, privacy UI.

- **`render_text_fallback`** (private, L3274)
  - Signature: `fn render_text_fallback(&self, message, cx) -> impl IntoElement`
  - Purpose: Plain-text reader when webview unavailable.
  - Behavior: Scrollable preformatted plain_text body.

- **`render_status_bar`** (private, L3300)
  - Signature: `fn render_status_bar(&self, cx) -> impl IntoElement`
  - Purpose: Bottom status bar.
  - Behavior: Shows hovered link URL centered when present.
##### Context: `SettingsView`

- **`new`** (private, L3379)
  - Signature: `fn new(accounts: Vec<data::Account>, root: WeakEntity<RootView>) -> Self`
  - Purpose: Constructs settings view.
  - Behavior: Stores accounts, default General section, weak root link.

- **`close_window`** (private, L3387)
  - Signature: `fn close_window(&mut self, window, cx: &mut Context<Self>)`
  - Purpose: Closes settings window.
  - Behavior: Notifies root and removes window.

- **`render_nav`** (private, L3399)
  - Signature: `fn render_nav(&self, cx) -> impl IntoElement`
  - Purpose: Settings section sidebar nav.
  - Behavior: Lists sections with icons; click switches section.

- **`render_content`** (private, L3443)
  - Signature: `fn render_content(&self, cx) -> impl IntoElement`
  - Purpose: Active settings section body.
  - Behavior: Renders General/Accounts/Appearance/Notifications/Privacy content.

- **`render_language_picker`** (private, L3609)
  - Signature: `fn render_language_picker(&self, language, cx) -> impl IntoElement`
  - Purpose: Language selector control.
  - Behavior: Buttons for EN/PT; updates locale global and refreshes app.

- **`remote_images_button`** (private, L3636)
  - Signature: `fn remote_images_button(&self, id, label, selected, value, cx) -> impl IntoElement`
  - Purpose: Segmented remote-images toggle button.
  - Behavior: Styled button; click sets load_remote_images on root.
##### Context: `Render for SettingsView`

- **`render`** (private, L3660)
  - Signature: `fn render(&mut self, _window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Settings window layout.
  - Behavior: Split nav + content with titlebar close handling.
##### Context: `Render for RootView`

- **`render`** (private, L3697)
  - Signature: `fn render(&mut self, window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Main RootView render.
  - Behavior: Full three-column layout, toolbar, deferred palette/search/webview sync, content_ready gate, window frame wrap.
##### Context: `module (tests)`

- **`test_view`** (private, L3921)
  - Signature: `fn test_view()`
  - Purpose: Creates temp DB and RootView for unit tests.
  - Behavior: TempDir + new_with_database with default config.

- **`sidebar_starts_visible`** (private, L3929)
  - Signature: `fn sidebar_starts_visible()`
  - Purpose: Test: sidebar visible by default.
  - Behavior: Asserts show_sidebar true on new view.

- **`sync_webview_reuses_osr_browser_on_message_switch`** (private, L3935)
  - Signature: `fn sync_webview_reuses_osr_browser_on_message_switch()`
  - Purpose: Test: structural OSR reuse guard.
  - Behavior: Source contains navigate-only and retry-without-signature comments.

- **`command_palette_only_opens_when_main_window_is_focused`** (private, L3947)
  - Signature: `fn command_palette_only_opens_when_main_window_is_focused()`
  - Purpose: Test: can_open_command_palette gate.
  - Behavior: True when active, false when not.

- **`webview_context_menu_dismiss_skips_clicks_inside_reader`** (private, L3953)
  - Signature: `fn webview_context_menu_dismiss_skips_clicks_inside_reader()`
  - Purpose: Test: context menu dismiss bounds.
  - Behavior: Inside reader=false dismiss; outside=true; no bounds=true.

- **`command_palette_starts_closed`** (private, L3976)
  - Signature: `fn command_palette_starts_closed()`
  - Purpose: Test: palette closed initially.
  - Behavior: command_palette none or open=false.

- **`toggle_sidebar_flips_visibility`** (private, L3985)
  - Signature: `fn toggle_sidebar_flips_visibility()`
  - Purpose: Test: toggle_sidebar inverts flag.
  - Behavior: Toggle twice restores visibility.

- **`selecting_in_narrow_layout_dismisses_floating_sidebar`** (private, L3994)
  - Signature: `fn selecting_in_narrow_layout_dismisses_floating_sidebar()`
  - Purpose: Test: narrow select hides sidebar.
  - Behavior: Narrow layout + select closes floating sidebar.

- **`selecting_in_wide_layout_keeps_sidebar_open`** (private, L4010)
  - Signature: `fn selecting_in_wide_layout_keeps_sidebar_open()`
  - Purpose: Test: wide select keeps sidebar.
  - Behavior: Wide layout selection leaves sidebar visible.

- **`mailbox_change_clears_reader_selection`** (private, L4031)
  - Signature: `fn mailbox_change_clears_reader_selection()`
  - Purpose: Test: mailbox change clears message.
  - Behavior: Selecting different mailbox clears selected message.

- **`default_selection_is_global_inbox`** (private, L4042)
  - Signature: `fn default_selection_is_global_inbox()`
  - Purpose: Test: initial selection is global inbox.
  - Behavior: selected_mailbox is Global(Inbox).

- **`global_inbox_has_unread`** (private, L4051)
  - Signature: `fn global_inbox_has_unread()`
  - Purpose: Test: seeded inbox has unread.
  - Behavior: global_unread(Inbox) > 0.

- **`global_flagged_counts_flagged_folder`** (private, L4057)
  - Signature: `fn global_flagged_counts_flagged_folder()`
  - Purpose: Test: flagged global counts starred.
  - Behavior: Uses DB count for flagged folder.

- **`global_drafts_and_sent_have_no_unread`** (private, L4065)
  - Signature: `fn global_drafts_and_sent_have_no_unread()`
  - Purpose: Test: drafts/sent unread zero.
  - Behavior: Both global_unread return 0.

- **`accounts_start_expanded`** (private, L4072)
  - Signature: `fn accounts_start_expanded()`
  - Purpose: Test: all accounts start expanded.
  - Behavior: No account in collapsed set initially.

- **`toggle_account_collapses_and_expands`** (private, L4080)
  - Signature: `fn toggle_account_collapses_and_expands()`
  - Purpose: Test: account toggle changes collapsed state.
  - Behavior: Toggle adds/removes from collapsed_accounts.

- **`toggle_account_hands_out_unique_tokens`** (private, L4093)
  - Signature: `fn toggle_account_hands_out_unique_tokens()`
  - Purpose: Test: fold tokens unique per toggle.
  - Behavior: Two toggles yield different FoldAnim tokens.

- **`collapsing_account_stays_visible_until_finalized`** (private, L4101)
  - Signature: `fn collapsing_account_stays_visible_until_finalized()`
  - Purpose: Test: collapse animation keeps rows visible.
  - Behavior: account_list_visible true during collapse anim.

- **`clear_fold_ignores_stale_token`** (private, L4113)
  - Signature: `fn clear_fold_ignores_stale_token()`
  - Purpose: Test: stale fold timer ignored.
  - Behavior: clear_fold with old token returns false.

- **`expanded_account_is_visible_without_animation`** (private, L4124)
  - Signature: `fn expanded_account_is_visible_without_animation()`
  - Purpose: Test: expanded account list visible.
  - Behavior: account_list_visible(0) true initially.

- **`narrow_window_auto_collapses_sidebar`** (private, L4130)
  - Signature: `fn narrow_window_auto_collapses_sidebar()`
  - Purpose: Test: narrow sync_layout hides sidebar.
  - Behavior: 800px width sets narrow and hides sidebar.

- **`resizing_while_narrow_recollapses_floating_sidebar`** (private, L4138)
  - Signature: `fn resizing_while_narrow_recollapses_floating_sidebar()`
  - Purpose: Test: resize in narrow re-collapses sidebar.
  - Behavior: Opening sidebar then sync_layout collapses again.

- **`floating_sidebar_stays_open_without_resize`** (private, L4150)
  - Signature: `fn floating_sidebar_stays_open_without_resize()`
  - Purpose: Test: sidebar stays open if no layout sync.
  - Behavior: Manual toggle without sync keeps open.

- **`widening_window_restores_sidebar`** (private, L4160)
  - Signature: `fn widening_window_restores_sidebar()`
  - Purpose: Test: leaving narrow restores sidebar.
  - Behavior: Wide sync_layout sets show_sidebar true.

- **`resize_respects_sidebar_minimum`** (private, L4170)
  - Signature: `fn resize_respects_sidebar_minimum()`
  - Purpose: Test: sidebar drag respects SIDEBAR_MIN_WIDTH.
  - Behavior: Drag below min clamps to minimum.

- **`resize_respects_list_minimum`** (private, L4178)
  - Signature: `fn resize_respects_list_minimum()`
  - Purpose: Test: list drag respects LIST_MIN_WIDTH.
  - Behavior: List divider drag clamped to minimum.

- **`resize_keeps_reader_minimum`** (private, L4191)
  - Signature: `fn resize_keeps_reader_minimum()`
  - Purpose: Test: reader width floor enforced.
  - Behavior: Extreme drags leave reader at READER_MIN_WIDTH.

- **`resize_locks_panels_once_reader_minimum_is_reached`** (private, L4206)
  - Signature: `fn resize_locks_panels_once_reader_minimum_is_reached()`
  - Purpose: Test: divider locks at reader minimum.
  - Behavior: Further drag does not shrink reader below floor.

- **`search_collapses_when_reader_segment_is_narrow`** (private, L4226)
  - Signature: `fn search_collapses_when_reader_segment_is_narrow()`
  - Purpose: Test: narrow reader segment compacts search.
  - Behavior: search_is_compact true on narrow layout.

- **`search_expands_on_wide_reader_segment`** (private, L4237)
  - Signature: `fn search_expands_on_wide_reader_segment()`
  - Purpose: Test: wide layout expands search.
  - Behavior: 1600px layout search_is_compact false.

- **`all_action_groups_show_on_wide_window`** (private, L4245)
  - Signature: `fn all_action_groups_show_on_wide_window()`
  - Purpose: Test: wide window shows 3 action groups.
  - Behavior: visible_action_groups equals 3.

- **`action_groups_drop_as_reader_segment_shrinks`** (private, L4252)
  - Signature: `fn action_groups_drop_as_reader_segment_shrinks()`
  - Purpose: Test: action groups drop with width.
  - Behavior: Shrinking reader segment reduces visible groups.

- **`action_groups_vanish_but_search_survives_when_tiny`** (private, L4269)
  - Signature: `fn action_groups_vanish_but_search_survives_when_tiny()`
  - Purpose: Test: tiny segment hides actions, keeps search.
  - Behavior: Minimum layout still allows search affordance.

- **`sync_layout_shrinks_columns_to_fit`** (private, L4281)
  - Signature: `fn sync_layout_shrinks_columns_to_fit()`
  - Purpose: Test: sync_layout reduces column widths.
  - Behavior: Small window clamps sidebar/list widths.

- **`search_force_expanded_overrides_compact_layout`** (private, L4293)
  - Signature: `fn search_force_expanded_overrides_compact_layout()`
  - Purpose: Test: force-expanded overrides compact.
  - Behavior: show_search_expanded true when flag set.

- **`search_debounce_token_invalidates_previous_timer`** (private, L4303)
  - Signature: `fn search_debounce_token_invalidates_previous_timer()`
  - Purpose: Test: debounce token invalidates stale timers.
  - Behavior: New token makes old token not current.

- **`search_clear_shows_in_expanded_field_only_with_text_on_wide_toolbar`** (private, L4314)
  - Signature: `fn search_clear_shows_in_expanded_field_only_with_text_on_wide_toolbar()`
  - Purpose: Test: clear button rules on wide toolbar.
  - Behavior: Clear visible only with non-empty query unless force-expanded.

- **`search_clear_always_shows_in_force_expanded_compact_field`** (private, L4322)
  - Signature: `fn search_clear_always_shows_in_force_expanded_compact_field()`
  - Purpose: Test: force-expanded shows clear even empty.
  - Behavior: Empty query still shows clear when force-expanded.

- **`search_collapses_after_blur_only_when_compact_expanded_and_empty`** (private, L4330)
  - Signature: `fn search_collapses_after_blur_only_when_compact_expanded_and_empty()`
  - Purpose: Test: blur collapse conditions.
  - Behavior: should_collapse_search_after_blur matches compact+empty rules.

- **`move_to_trash_updates_folder_membership`** (private, L4343)
  - Signature: `fn move_to_trash_updates_folder_membership()`
  - Purpose: Test: delete moves message to trash in DB.
  - Behavior: After delete_message_to_trash, detail shows trash folder.

- **`command_context_reflects_trash_state`** (private, L4358)
  - Signature: `fn command_context_reflects_trash_state()`
  - Purpose: Test: command_context trash detection.
  - Behavior: After trash move, message_in_trash true in context.

### `src/shortcuts.rs`

#### Types / constants

- **const `COMPOSE_MAC` / `COMPOSE_OTHER`**: GPUI binding strings for New Message (Cmd+N / Ctrl+N).
- **const `SETTINGS_MAC` / `SETTINGS_OTHER`**: Binding strings for Settings (Cmd+, / Ctrl+,).
- **const `TOGGLE_SIDEBAR_MAC` / `TOGGLE_SIDEBAR_OTHER`**: Binding strings for sidebar toggle.
- **const `COMMAND_PALETTE_MAC` / `COMMAND_PALETTE_OTHER`**: Binding strings for command palette.
- **const `DELETE_MESSAGE_MAC` / `DELETE_MESSAGE_OTHER`**: Binding strings for move-to-trash.
- **const `ARCHIVE_MAC` / `ARCHIVE_OTHER`**: Binding strings for archive.
- **const `MARK_JUNK_MAC` / `MARK_JUNK_OTHER`**: Binding strings for mark junk.
- **const `TOGGLE_FLAG_MAC` / `TOGGLE_FLAG_OTHER`**: Binding strings for flag/unflag.
- **const `COMPOSE_CLOSE_MAC` / `COMPOSE_CLOSE_OTHER`**: Binding strings for closing compose window.

#### Functions / methods

- **`primary_binding`** (pub, L43)
  - Signature: `pub fn primary_binding<'a>(mac: &'a str, other: &'a str) -> &'a str`
  - Purpose: Returns the platform-appropriate binding string for tooltips.
  - Behavior: Returns `mac` on macOS and `other` elsewhere.
- **`format_binding`** (pub, L52)
  - Signature: `pub fn format_binding(source: &str) -> String`
  - Purpose: Formats a GPUI keystroke string for display.
  - Behavior: Parses `source` as a `Keystroke` and renders platform glyphs (e.g. `cmd-n` → `⌘N`). Falls back to the raw string on parse failure.
- **`bind_app_shortcuts`** (pub, L60)
  - Signature: `pub fn bind_app_shortcuts(cx: &mut App)`
  - Purpose: Registers global key bindings for menu-driven actions.
  - Behavior: Binds both macOS and Windows/Linux chords for each action. Command palette bindings also register under the `TextInput` context so they work while typing in search fields.

- **`format_binding_parses_compose_shortcut`** (private, L95)
  - Signature: `fn format_binding_parses_compose_shortcut()`
  - Purpose: Test: compose shortcut formats to a non-empty display string.
  - Behavior: Formats the platform compose binding and asserts it is non-empty (and contains N on macOS).

- **`settings_comma_binding_parses`** (private, L103)
  - Signature: `fn settings_comma_binding_parses()`
  - Purpose: Test: settings comma bindings parse as valid keystrokes.
  - Behavior: Asserts both macOS and non-macOS settings bindings parse successfully.

- **`primary_binding_follows_platform`** (private, L109)
  - Signature: `fn primary_binding_follows_platform()`
  - Purpose: Test: `primary_binding` selects the correct platform string.
  - Behavior: Asserts macOS returns the Mac variant and other platforms return the other variant.

### `src/startup.rs`

#### Types / constants

- _(None at module top-level.)_

#### Functions / methods

- **`mark_start`** (pub, L13)
  - Signature: `pub fn mark_start()`
  - Purpose: Records the process start instant for startup timing.
  - Behavior: Stores `Instant::now()` in a `OnceLock`. Subsequent calls are ignored.
- **`elapsed`** (pub, L18)
  - Signature: `pub fn elapsed() -> Duration`
  - Purpose: Returns elapsed time since `mark_start`.
  - Behavior: Returns zero if `mark_start` was never called.
- **`format_elapsed`** (pub, L23)
  - Signature: `pub fn format_elapsed(duration: Duration) -> String`
  - Purpose: Formats a duration for human-readable log output.
  - Behavior: Renders milliseconds with one decimal place (e.g. `"42.0ms"`).
- **`log_milestone`** (pub, L28)
  - Signature: `pub fn log_milestone(label: &str)`
  - Purpose: Logs a named startup milestone in debug builds.
  - Behavior: Prints `[BGMail startup] {label}: {elapsed}` to stderr when `debug_assertions` is enabled.

- **`format_elapsed_shows_milliseconds`** (private, L40)
  - Signature: `fn format_elapsed_shows_milliseconds()`
  - Purpose: Test: `format_elapsed` uses millisecond formatting.
  - Behavior: Asserts `format_elapsed(42ms)` equals `"42.0ms"`.

- **`elapsed_grows_after_mark_start`** (private, L45)
  - Signature: `fn elapsed_grows_after_mark_start()`
  - Purpose: Test: elapsed time increases after `mark_start`.
  - Behavior: Calls `mark_start`, sleeps briefly, and asserts `elapsed()` does not decrease.

### `src/web_view.rs`

#### Types / constants

- **enum `WebviewMouseButton`** (pub, L25): Mouse button (Left, Right, Middle) forwarded from the reader; platform-independent of GPUI's `MouseButton`.
- **enum `WebviewCursor`** (pub, L34): CSS-like cursor kind requested by the page; mapped to GPUI `CursorStyle` by the reader on OSR.
- **const `WEBVIEW_SUPPORTED`** (pub, L53): Compile-time flag: `true` when the `cef-osr` feature is enabled.
- **struct `ContextMenuLabels<'a>`** (pub, L69): Localized strings for custom image/link/selection context menus, embedded as `data-rm-*` attributes on `<body>`.
- **struct `RenderedEmail`** (pub, L93): Output of document rendering: full HTML plus remote-image privacy metadata for the reader shield.
- **struct `DocumentColors`** (pub, L110): Theme colors for page background/text/accent and separately for context-menu popup colors.
- **enum `IpcMessage<'a>`** (pub(crate), L365): Parsed IPC tag from the injected content script (hover, open, download, copy, show-image, body-mousedown, palette).
- **enum `HostEvent`** (pub, L406): Foreground actions the webview requests from GPUI (hover URL, clipboard, image shown, overlay dismiss, palette, cursor, redraw).
- **const `COMMAND_PALETTE_SHORTCUT_SCRIPT`** (private, L428/L431): Inline keydown script for Ctrl/Cmd+P; empty string when `cef-osr` is disabled.
- **const `CONTENT_SCRIPT`** (pub(crate), L450): Injected JS for hover reporting, custom context menus, copy/select-all shortcuts, and menu dismissal hook.
- **const `DISALLOWED_ELEMENTS`** (private, L667): Comma-separated selector list of HTML elements stripped entirely (and their content) during sanitization.
- **const `URL_ATTRIBUTES`** (private, L675): Attribute names vetted for dangerous URL schemes during sanitization.
- **const `RESOURCE_ATTRIBUTES`** (private, L696): Subset of URL attributes that auto-fetch on render; blocked for remoteness when loading is off.
- **const `COMPOSITES_IN_GPUI`** (pub, platform): Whether the backend paints into a GPUI texture (`true` for CEF OSR, `false` for stub).
- **struct `EmailWebView`** (pub, platform): GPUI-facing webview wrapper; real OSR backend or no-op stub depending on `cef-osr` feature.
- **const `INPUT_WARM_FRAMES`** (private, platform/cef-osr, L985): GPUI frames to keep redrawing after pointer input so late CEF paints land.
- **const `LOAD_WARM_FRAMES`** (private, platform/cef-osr, L989): GPUI frames to keep redrawing after document reload/navigation.

#### Functions / methods

##### Context: `module`

- **`pump_platform_events`** (pub, L57)
  - Signature: `pub fn pump_platform_events()`
  - Purpose: Advances CEF's external message pump from the app frame loop.
  - Behavior: Calls `cef_osr::pump` when `cef-osr` feature is enabled; otherwise no-op.
- **`email_document`** (pub, L118)
  - Signature: `pub fn email_document(colors: DocumentColors, body: &MessageBody, labels: ContextMenuLabels, load_remote: bool, shown: &HashSet<String>) -> RenderedEmail`
  - Purpose: Builds a complete themed HTML document for an e-mail body.
  - Behavior: Sanitizes HTML or escapes plain text into inner markup. Wraps in DOCTYPE/html with theme CSS, localized `data-rm-*` menu labels, command-palette script, and returns `RenderedEmail` with `has_remote` and `blocked_images` counts.
- **`document_css`** (private, L164)
  - Signature: `fn document_css(colors: DocumentColors) -> String`
  - Purpose: Generates the inline stylesheet for rendered message documents.
  - Behavior: Sets CSS variables from theme colors, picks light/dark `color-scheme` from background luminance, styles typography/links/code/blockquote/scrollbar, and blocked-image placeholders.
- **`css_color`** (private, L219)
  - Signature: `fn css_color(color: Hsla) -> String`
  - Purpose: Formats an opaque theme color as `rgb(r, g, b)`.
  - Behavior: Converts `Hsla` to `Rgba` and rounds channels to 0–255.
- **`css_color_alpha`** (private, L230)
  - Signature: `fn css_color_alpha(color: Hsla, alpha: f32) -> String`
  - Purpose: Formats a translucent theme color as `rgba(r, g, b, a)`.
  - Behavior: Clamps alpha to [0, 1] and formats RGB channels like `css_color`.
- **`channel`** (private, L241)
  - Signature: `fn channel(value: f32) -> u8`
  - Purpose: Converts a normalized 0–1 color channel to an 8-bit value.
  - Behavior: Clamps to [0, 1], multiplies by 255, rounds to `u8`.
- **`is_external_link`** (pub(crate), L249)
  - Signature: `pub(crate) fn is_external_link(url: &str) -> bool`
  - Purpose: Classifies URLs that should open in the system browser instead of the reader webview.
  - Behavior: Trims and lowercases; returns `true` for `http://`, `https://`, and `mailto:` prefixes.
- **`decode_data_uri`** (pub(crate), L261)
  - Signature: `pub(crate) fn decode_data_uri(url: &str) -> Option<(&'static str, Vec<u8>)>`
  - Purpose: Parses base64-encoded `data:` URIs into MIME-derived extension and raw bytes.
  - Behavior: Requires `data:` prefix (case-insensitive), comma-separated metadata with `base64`, non-empty decoded payload. Returns `None` for plain URLs, text `data:` URIs, or decode failures.
- **`extension_for_mime`** (private, L284)
  - Signature: `fn extension_for_mime(mime: &str) -> &'static str`
  - Purpose: Maps a MIME type to a file extension for materialized temp/download files.
  - Behavior: Matches common image and PDF types; returns `"bin"` for unknown MIME.
- **`downloads_dir`** (pub(crate), L302)
  - Signature: `pub(crate) fn downloads_dir() -> Option<std::path::PathBuf>`
  - Purpose: Resolves the user's Downloads folder from environment.
  - Behavior: Uses `$HOME/Downloads` or `%USERPROFILE%\Downloads`; returns `None` when home is missing or empty.
- **`unique_download_path`** (pub(crate), L314)
  - Signature: `pub(crate) fn unique_download_path(dir: &Path, stem: &str, extension: &str, exists: impl Fn(&Path) -> bool) -> std::path::PathBuf`
  - Purpose: Picks a non-colliding download path like browsers (`stem (n).ext`).
  - Behavior: Tries `stem.ext`, then increments `n` until `exists` returns false for the candidate.
- **`base64_decode`** (private, L332)
  - Signature: `fn base64_decode(input: &str) -> Option<Vec<u8>>`
  - Purpose: Decodes standard RFC 4648 base64 without external dependencies.
  - Behavior: Skips whitespace and `=` padding; returns `None` on invalid alphabet bytes. Delegates 6-bit values to nested `sextet`.
- **`sextet`** (private, nested in L333)
  - Signature: `fn sextet(byte: u8) -> Option<u32>`
  - Purpose: Maps one base64 character to its 6-bit value.
  - Behavior: Handles A–Z, a–z, 0–9, `+`, `/`; returns `None` for other bytes.
- **`parse_ipc_message`** (pub(crate), L388)
  - Signature: `pub(crate) fn parse_ipc_message(message: &str) -> Option<IpcMessage<'_>>`
  - Purpose: Parses `"tag\npayload"` IPC strings from the content script.
  - Behavior: Maps tags H/O/D/C/S/B/P to enum variants; unknown tags or missing newline return `None`. Body-mousedown and palette ignore payload.
- **`applescript_string`** (private, L628, macOS only)
  - Signature: `fn applescript_string(input: &str) -> String`
  - Purpose: Escapes a string for safe embedding in AppleScript literals.
  - Behavior: Wraps in double quotes; backslash-escapes `\` and `"` characters.
- **`sanitize_html`** (private, L717, `#[cfg(test)]`)
  - Signature: `fn sanitize_html(html: &str, load_remote: bool) -> String`
  - Purpose: Test helper returning only the sanitized HTML string.
  - Behavior: Calls `sanitize_html_inner` with empty `shown` set and discards metadata tuple.
- **`sanitize_html_inner`** (private, L726)
  - Signature: `fn sanitize_html_inner(html: &str, load_remote: bool, shown: &HashSet<String>) -> (String, bool, usize)`
  - Purpose: Sanitizes HTML and reports remote-image presence and blocked count.
  - Behavior: Uses `lol_html` to remove disallowed elements and neutralize attributes on survivors. When `load_remote` is false, also blanks CSS `url(...)` via `strip_css_urls`. On rewrite failure returns empty string. Updates `has_remote` and `blocked` counters from `neutralize_attributes`.
- **`strip_css_urls`** (private, L768)
  - Signature: `fn strip_css_urls(css: &str) -> String`
  - Purpose: Neutralizes remote fetches from inline CSS by emptying every `url(...)`.
  - Behavior: Scans case-insensitively for `url(`; replaces each with `url()`. Unterminated `url(` drops remainder of input.
- **`find_url_open`** (private, L793)
  - Signature: `fn find_url_open(s: &str) -> Option<usize>`
  - Purpose: Finds the byte index of the next case-insensitive `url(` substring.
  - Behavior: Scans ASCII windows of four bytes; returns start index or `None`.
- **`neutralize_attributes`** (private, L822)
  - Signature: `fn neutralize_attributes(el: &mut Element, load_remote: bool, shown: &HashSet<String>) -> Option<bool>`
  - Purpose: Strips dangerous or remote-loading attributes from a surviving element.
  - Behavior: Collects doomed attribute names first, then removes them. Drops `on*`, `contenteditable`, dangerous URLs, script-vector styles, and remote resource attrs when blocking. Blocked remote `<img src>` stashes URL in `data-rm-blocked-src`. Returns `Some(blocked)` for remote images, else `None`.
- **`is_dangerous_url`** (private, L879)
  - Signature: `fn is_dangerous_url(value: &str) -> bool`
  - Purpose: Detects URL schemes that execute or load active content.
  - Behavior: Normalizes scheme by stripping whitespace/control chars. `javascript:`/`vbscript:` are dangerous; `data:` is dangerous except raster images (not SVG). Relative URLs without scheme are safe.
- **`is_remote_url`** (private, L911)
  - Signature: `fn is_remote_url(value: &str) -> bool`
  - Purpose: Detects URLs that fetch from the network on render.
  - Behavior: True for `http`/`https` schemes and protocol-relative `//` URLs; false for `data:`, `cid:`, fragments, and relative paths.
- **`style_has_script_vector`** (private, L930)
  - Signature: `fn style_has_script_vector(style: &str) -> bool`
  - Purpose: Detects legacy CSS expressions that could carry script vectors.
  - Behavior: Case-insensitive substring search for `javascript:`, `expression(`, `-moz-binding`, `behavior:`.
- **`escape_html`** (private, L938)
  - Signature: `fn escape_html(input: &str) -> String`
  - Purpose: Escapes HTML-significant characters in plain text.
  - Behavior: Replaces `& < > " '` with entity references; passes other chars through.

##### Context: `platform` (`feature = "cef-osr"`)

- **`map_button`** (private, L1239)
  - Signature: `fn map_button(button: WebviewMouseButton) -> MouseButton`
  - Purpose: Maps portable webview mouse buttons to CEF OSR `MouseButton`.
  - Behavior: One-to-one enum match for Left, Right, Middle.

##### Context: `EmailWebView` (`feature = "cef-osr"`)

- **`new`** (pub, L994)
  - Signature: `pub fn new(_window: &Window, html: &str, to_host: Sender<HostEvent>, notify_body: String) -> Option<Self>`
  - Purpose: Creates the CEF OSR webview wrapper for the reader pane.
  - Behavior: Returns `None` when `OsrBrowser::new` fails. Initializes visibility, load warm-frame budget, and `awaiting_paint` for first navigation.
- **`set_html`** (pub, L1010)
  - Signature: `pub fn set_html(&mut self, html: &str) -> Option<Arc<gpui::RenderImage>>`
  - Purpose: Reloads the document when HTML changed and returns the previous GPUI texture to drop.
  - Behavior: Delegates to inner browser; on navigation sets `awaiting_paint`, resets warm frames to `LOAD_WARM_FRAMES`, and calls `pump_hard`. Returns previous image from inner `set_html`.
- **`set_notify_text`** (pub, L1021)
  - Signature: `pub fn set_notify_text(&self, body: String)`
  - Purpose: Updates localized download-notification text on the live browser.
  - Behavior: Forwards to `OsrBrowser::set_notify_text`.
- **`on_window_activated`** (pub, L1027)
  - Signature: `pub fn on_window_activated(&mut self)`
  - Purpose: Resumes OSR rendering when the GPUI window regains focus.
  - Behavior: Marks visible, sets inner visible/focused, pumps hard, extends warm frames, and re-enters `awaiting_paint` if no current frame.
- **`on_osr_tick`** (pub, L1041)
  - Signature: `pub fn on_osr_tick(&mut self) -> bool`
  - Purpose: Keeps CEF pumping between GPUI paints when the window may be inactive.
  - Behavior: Returns `false` when hidden. Otherwise pumps hard when awaiting paint, warm frames remain, or a pending frame exists; returns `true` to request GPUI `notify`.
- **`note_load_progress`** (pub, L1055)
  - Signature: `pub fn note_load_progress(&mut self)`
  - Purpose: Extends redraw loop after main-document load-end from CEF.
  - Behavior: Sets `awaiting_paint`, bumps warm frames to at least `LOAD_WARM_FRAMES`, calls `pump_hard`.
- **`position`** (pub, L1064)
  - Signature: `pub fn position(&mut self, _bounds: Bounds<Pixels>)`
  - Purpose: Marks the webview visible; actual sizing happens in `paint`.
  - Behavior: Sets visible flag and inner `set_visible(true)`.
- **`hide`** (pub, L1070)
  - Signature: `pub fn hide(&mut self)`
  - Purpose: Hides the webview when no message is shown or overlays cover it.
  - Behavior: Clears visible, warm frames, and awaiting-paint; hides inner browser.
- **`paint`** (pub, L1089)
  - Signature: `pub fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut gpui::App) -> bool`
  - Purpose: Syncs size, flushes input, uploads CEF frame, and composites into the reader.
  - Behavior: No-op when hidden. Updates inner size/scale; on resize extends warm frames. Pumps hard while awaiting paint else flushes input. Takes new frame and drops previous GPUI texture. Paints 1:1 at top-left when buffer size mismatches view (avoids stretch distortion). Returns `true` when another GPUI frame should be scheduled (warm frames, pending frame, or size mismatch).
- **`handle_mouse_move`** (pub, L1169)
  - Signature: `pub fn handle_mouse_move(&self, x: f32, y: f32, shift: bool, control: bool, alt: bool, meta: bool)`
  - Purpose: Forwards mouse move with modifier flags to CEF.
  - Behavior: Calls inner `mouse_move` with `modifier_flags`.
- **`handle_mouse_button`** (pub, L1183)
  - Signature: `pub fn handle_mouse_button(&mut self, x: f32, y: f32, button: WebviewMouseButton, pressed: bool, click_count: i32)`
  - Purpose: Forwards mouse press/release to CEF.
  - Behavior: Maps button, forwards click, sets `INPUT_WARM_FRAMES` warm budget.
- **`handle_scroll`** (pub, L1197)
  - Signature: `pub fn handle_scroll(&mut self, x: f32, y: f32, delta_x: f32, delta_y: f32)`
  - Purpose: Queues coalesced scroll-wheel input until next paint.
  - Behavior: Forwards to inner `mouse_wheel`; sets warm frames.
- **`handle_key`** (pub, L1204)
  - Signature: `pub fn handle_key(&mut self, pressed: bool, key: &str, key_char: Option<&str>, shift: bool, control: bool, alt: bool, meta: bool)`
  - Purpose: Forwards keyboard events to CEF with modifiers.
  - Behavior: Calls inner `key_event`; sets warm frames.
- **`set_focused`** (pub, L1224)
  - Signature: `pub fn set_focused(&self, focused: bool)`
  - Purpose: Sets OSR keyboard focus for shortcuts inside the document.
  - Behavior: Delegates to inner `set_focus`.
- **`dismiss_context_menu`** (pub, L1229)
  - Signature: `pub fn dismiss_context_menu(&self)`
  - Purpose: Closes the HTML context menu from GPUI outside-click handling.
  - Behavior: Delegates to inner `dismiss_context_menu`.
- **`has_mouse_capture`** (pub, L1234)
  - Signature: `pub fn has_mouse_capture(&self) -> bool`
  - Purpose: Reports held mouse buttons for drag-selection redraw scheduling.
  - Behavior: Delegates to inner `has_mouse_capture`.

##### Context: `EmailWebView` (`not(feature = "cef-osr")` stub)

- **`new`** (pub, L1261)
  - Signature: `pub fn new(...) -> Option<Self>`
  - Purpose: Stub constructor when no webview backend is available.
  - Behavior: Always returns `None` so the reader falls back to plain text.
- **`set_html`** (pub, L1269)
  - Signature: `pub fn set_html(&mut self, _html: &str) -> Option<Arc<gpui::RenderImage>>`
  - Purpose: Stub no-op document reload.
  - Behavior: Always returns `None`.
- **`on_window_activated`** (pub, L1272)
  - Signature: `pub fn on_window_activated(&mut self)`
  - Purpose: Stub activation handler.
  - Behavior: Empty body.
- **`on_osr_tick`** (pub, L1273)
  - Signature: `pub fn on_osr_tick(&mut self) -> bool`
  - Purpose: Stub OSR tick.
  - Behavior: Always returns `false`.
- **`note_load_progress`** (pub, L1276)
  - Signature: `pub fn note_load_progress(&mut self)`
  - Purpose: Stub load-progress hook.
  - Behavior: Empty body.
- **`set_notify_text`** (pub, L1277)
  - Signature: `pub fn set_notify_text(&self, _body: String)`
  - Purpose: Stub notification text update.
  - Behavior: Empty body.
- **`position`** (pub, L1278)
  - Signature: `pub fn position(&mut self, _bounds: Bounds<Pixels>)`
  - Purpose: Stub visibility/position hook.
  - Behavior: Empty body.
- **`dismiss_context_menu`** (pub, L1279)
  - Signature: `pub fn dismiss_context_menu(&self)`
  - Purpose: Stub context-menu dismiss.
  - Behavior: Empty body.
- **`hide`** (pub, L1280)
  - Signature: `pub fn hide(&mut self)`
  - Purpose: Stub hide hook.
  - Behavior: Empty body.
- **`paint`** (pub, L1281)
  - Signature: `pub fn paint(...) -> bool`
  - Purpose: Stub paint/composite hook.
  - Behavior: Always returns `false`.
- **`handle_mouse_move`** (pub, L1289)
  - Signature: `pub fn handle_mouse_move(...)`
  - Purpose: Stub mouse-move forwarder.
  - Behavior: Empty body.
- **`handle_mouse_button`** (pub, L1299)
  - Signature: `pub fn handle_mouse_button(...)`
  - Purpose: Stub mouse-button forwarder.
  - Behavior: Empty body.
- **`handle_scroll`** (pub, L1308)
  - Signature: `pub fn handle_scroll(&self, ...)`
  - Purpose: Stub scroll forwarder.
  - Behavior: Empty body.
- **`handle_key`** (pub, L1309)
  - Signature: `pub fn handle_key(...)`
  - Purpose: Stub keyboard forwarder.
  - Behavior: Empty body.
- **`set_focused`** (pub, L1320)
  - Signature: `pub fn set_focused(&self, _focused: bool)`
  - Purpose: Stub focus hook.
  - Behavior: Empty body.
- **`has_mouse_capture`** (pub, L1321)
  - Signature: `pub fn has_mouse_capture(&self) -> bool`
  - Purpose: Stub mouse-capture query.
  - Behavior: Always returns `false`.

##### Context: `tests`

- **`body_html`** (private, L1332)
  - Signature: `fn body_html() -> MessageBody`
  - Purpose: Returns sample HTML message body for document tests.
  - Behavior: Static `"Hello world"` paragraph HTML.
- **`doc_colors`** (private, L1338)
  - Signature: `fn doc_colors(background: Hsla, text: Hsla, accent: Hsla) -> DocumentColors`
  - Purpose: Builds test `DocumentColors` with menu colors matching page colors.
  - Behavior: Sets `menu_bg`/`menu_text` from background/text.
- **`labels`** (private, L1348)
  - Signature: `fn labels() -> ContextMenuLabels<'static>`
  - Purpose: Returns default English context-menu labels for tests.
  - Behavior: Static string literals including ⌘C copy shortcut.
- **`escapes_html_special_characters`** (private, L1361)
  - Signature: `fn escapes_html_special_characters()`
  - Purpose: Verifies `escape_html` entity encoding.
  - Behavior: Asserts all significant characters are escaped.
- **`external_links_route_to_the_browser`** (private, L1369)
  - Signature: `fn external_links_route_to_the_browser()`
  - Purpose: Verifies http/https/mailto are external.
  - Behavior: Asserts true for common external URL forms including trimmed/case variants.
- **`in_document_navigations_stay_in_place`** (private, L1377)
  - Signature: `fn in_document_navigations_stay_in_place()`
  - Purpose: Verifies in-document URLs are not external.
  - Behavior: Asserts false for about/data/fragment/empty URLs.
- **`base64_decode_reverses_known_vectors`** (private, L1385)
  - Signature: `fn base64_decode_reverses_known_vectors()`
  - Purpose: Verifies RFC 4648 decode vectors and whitespace tolerance.
  - Behavior: Asserts known encodings decode to expected byte strings.
- **`base64_decode_rejects_out_of_alphabet_bytes`** (private, L1398)
  - Signature: `fn base64_decode_rejects_out_of_alphabet_bytes()`
  - Purpose: Verifies invalid base64 returns `None`.
  - Behavior: Asserts `not*valid` fails decode.
- **`decode_data_uri_extracts_image_bytes`** (private, L1403)
  - Signature: `fn decode_data_uri_extracts_image_bytes()`
  - Purpose: Verifies PNG data URI decoding.
  - Behavior: Asserts extension `png` and bytes `foo` for known URI.
- **`decode_data_uri_is_case_insensitive_on_the_scheme`** (private, L1412)
  - Signature: `fn decode_data_uri_is_case_insensitive_on_the_scheme()`
  - Purpose: Verifies `DATA:` scheme prefix works.
  - Behavior: Asserts JPEG extension on uppercase scheme URI.
- **`decode_data_uri_rejects_non_data_and_non_base64`** (private, L1419)
  - Signature: `fn decode_data_uri_rejects_non_data_and_non_base64()`
  - Purpose: Verifies rejection of HTTP URLs, text data URIs, and empty payloads.
  - Behavior: Asserts all return `None`.
- **`extension_for_mime_maps_known_image_types`** (private, L1428)
  - Signature: `fn extension_for_mime_maps_known_image_types()`
  - Purpose: Verifies MIME→extension mapping table.
  - Behavior: Asserts png/jpg/gif/svg and unknown→bin.
- **`parse_ipc_message_routes_known_tags`** (private, L1437)
  - Signature: `fn parse_ipc_message_routes_known_tags()`
  - Purpose: Verifies all IPC tags parse to expected variants.
  - Behavior: Asserts H/O/D/C/S/P including empty hover payload.
- **`parse_ipc_message_rejects_unknown_or_malformed`** (private, L1464)
  - Signature: `fn parse_ipc_message_rejects_unknown_or_malformed()`
  - Purpose: Verifies unknown tags and missing newline fail.
  - Behavior: Asserts `None` for bad tag and missing separator.
- **`unique_download_path_appends_counter_when_taken`** (private, L1471)
  - Signature: `fn unique_download_path_appends_counter_when_taken()`
  - Purpose: Verifies browser-style numbered suffix when paths exist.
  - Behavior: Uses injectable `exists` closure; asserts `(3)` suffix when lower numbers taken.
- **`applescript_string_quotes_and_escapes`** (private, L1486, macOS)
  - Signature: `fn applescript_string_quotes_and_escapes()`
  - Purpose: Verifies AppleScript literal escaping.
  - Behavior: Asserts quoting and backslash escapes for `"` and `\`.
- **`channels_clamp_and_round`** (private, L1493)
  - Signature: `fn channels_clamp_and_round()`
  - Purpose: Verifies color channel clamping and rounding.
  - Behavior: Asserts 0, 255, and clamp behavior for out-of-range inputs.
- **`css_color_is_opaque_rgb`** (private, L1501)
  - Signature: `fn css_color_is_opaque_rgb()`
  - Purpose: Verifies opaque RGB formatting from HSLA.
  - Behavior: Asserts black and white extremes.
- **`document_wraps_html_body_verbatim`** (private, L1507)
  - Signature: `fn document_wraps_html_body_verbatim()`
  - Purpose: Verifies HTML body passes through document wrapper with theme CSS.
  - Behavior: Asserts DOCTYPE, body content, dark scheme, and scrollbar CSS present.
- **`document_escapes_and_wraps_plain_text`** (private, L1531)
  - Signature: `fn document_escapes_and_wraps_plain_text()`
  - Purpose: Verifies plain-text bodies become escaped `<pre class="plain">`.
  - Behavior: Asserts escaping and light color scheme.
- **`sanitize_strips_disallowed_elements_and_their_content`** (private, L1551)
  - Signature: `fn sanitize_strips_disallowed_elements_and_their_content()`
  - Purpose: Verifies dangerous/embed elements and content are removed.
  - Behavior: Asserts safe paragraphs remain; script/iframe/form/etc. absent.
- **`sanitize_keeps_svg_but_removes_its_scriptable_parts`** (private, L1597)
  - Signature: `fn sanitize_keeps_svg_but_removes_its_scriptable_parts()`
  - Purpose: Verifies inline SVG art survives but script/SMIL/foreignObject do not.
  - Behavior: Asserts `<svg>`/`<rect>` remain; scriptable parts stripped.
- **`sanitize_strips_event_handlers_and_contenteditable`** (private, L1618)
  - Signature: `fn sanitize_strips_event_handlers_and_contenteditable()`
  - Purpose: Verifies inline handlers and contenteditable removed.
  - Behavior: Asserts div/text remain; handler attrs absent.
- **`sanitize_neutralizes_dangerous_url_schemes`** (private, L1632)
  - Signature: `fn sanitize_neutralizes_dangerous_url_schemes()`
  - Purpose: Verifies javascript/vbscript/SVG data URLs stripped from attrs.
  - Behavior: Asserts elements remain but dangerous `href`/`src` values removed.
- **`is_dangerous_url_classifies_schemes`** (private, L1649)
  - Signature: `fn is_dangerous_url_classifies_schemes()`
  - Purpose: Verifies scheme classifier edge cases.
  - Behavior: Asserts dangerous script/data cases and safe relative/raster data cases.
- **`sanitize_drops_style_with_legacy_script_vectors`** (private, L1665)
  - Signature: `fn sanitize_drops_style_with_legacy_script_vectors()`
  - Purpose: Verifies legacy CSS script vectors cause style attr removal.
  - Behavior: Asserts expression/javascript substrings absent after sanitize.
- **`sanitize_preserves_safe_markup_and_inline_styles`** (private, L1677)
  - Signature: `fn sanitize_preserves_safe_markup_and_inline_styles()`
  - Purpose: Verifies benign markup, links, and inline data images kept when remote allowed.
  - Behavior: Asserts style, https link, and data image survive.
- **`is_remote_url_detects_network_resources`** (private, L1687)
  - Signature: `fn is_remote_url_detects_network_resources()`
  - Purpose: Verifies remote URL detector for http(s) and protocol-relative URLs.
  - Behavior: Asserts network URLs true; data/cid/relative/fragment false.
- **`sanitize_blocks_remote_resources_when_disabled`** (private, L1701)
  - Signature: `fn sanitize_blocks_remote_resources_when_disabled()`
  - Purpose: Verifies remote images blocked with stash in `data-rm-blocked-src`.
  - Behavior: Asserts tracker URL appears once in blocked-src; srcset removed; data image kept.
- **`sanitize_keeps_remote_link_href_when_blocking`** (private, L1719)
  - Signature: `fn sanitize_keeps_remote_link_href_when_blocking()`
  - Purpose: Verifies link hrefs survive remote blocking (click-time navigation).
  - Behavior: Asserts href kept; image src blocked to data attribute only.
- **`sanitize_does_not_stash_blocked_src_when_remote_enabled`** (private, L1733)
  - Signature: `fn sanitize_does_not_stash_blocked_src_when_remote_enabled()`
  - Purpose: Verifies remote-enabled mode keeps live image src.
  - Behavior: Asserts src present; no blocked-src attribute.
- **`sanitize_keeps_remote_resources_when_enabled`** (private, L1741)
  - Signature: `fn sanitize_keeps_remote_resources_when_enabled()`
  - Purpose: Verifies remote pixel URL retained when loading enabled.
  - Behavior: Asserts https src in output.
- **`strip_css_urls_empties_every_url`** (private, L1748)
  - Signature: `fn strip_css_urls_empties_every_url()`
  - Purpose: Verifies CSS url blanking including case, multiples, and unterminated input.
  - Behavior: Asserts `url()` replacements; unterminated drops tail; plain CSS unchanged.
- **`sanitize_blocks_css_url_resources_when_disabled`** (private, L1765)
  - Signature: `fn sanitize_blocks_css_url_resources_when_disabled()`
  - Purpose: Verifies style-block and inline CSS urls blanked when remote off.
  - Behavior: Asserts tracker host absent; two `url()` placeholders remain.
- **`sanitize_keeps_css_url_resources_when_enabled`** (private, L1774)
  - Signature: `fn sanitize_keeps_css_url_resources_when_enabled()`
  - Purpose: Verifies CSS background URLs kept when remote loading on.
  - Behavior: Asserts CDN URL present in sanitized output.
- **`document_strips_disallowed_elements_from_html_body`** (private, L1781)
  - Signature: `fn document_strips_disallowed_elements_from_html_body()`
  - Purpose: Verifies full document pipeline strips disallowed tags from HTML bodies.
  - Behavior: Asserts safe paragraph kept; iframe/input/video absent in final HTML.
- **`document_reports_remote_image_state`** (private, L1804)
  - Signature: `fn document_reports_remote_image_state()`
  - Purpose: Verifies `RenderedEmail` remote/blocked counts across load and shown settings.
  - Behavior: Asserts blocked=1 when off, 0 when on or individually shown; local body has no remote.
- **`document_embeds_escaped_menu_labels`** (private, L1834)
  - Signature: `fn document_embeds_escaped_menu_labels()`
  - Purpose: Verifies menu labels appear HTML-escaped in data attributes.
  - Behavior: Asserts all `data-rm-*` keys present with escaped quotes.
- **`document_embeds_command_palette_shortcut_script`** (private, L1867, cef-osr)
  - Signature: `fn document_embeds_command_palette_shortcut_script()`
  - Purpose: Verifies command-palette IPC script is embedded in rendered documents.
  - Behavior: Asserts `window.ipc.postMessage('P\n')` substring in HTML.
- **`content_script_runs_menu_actions_on_mousedown`** (private, L1885, cef-osr)
  - Signature: `fn content_script_runs_menu_actions_on_mousedown()`
  - Purpose: Verifies content script uses mousedown (not click) and blocked-image menu ordering.
  - Behavior: Asserts mousedown handler, menu.contains guard, and blocked-src branch precedes getAttribute src branch.


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


### `src/window_frame.rs`

#### Types / constants

- **`MAC_TRAFFIC_LIGHT_CLEARANCE`** (private, L19)
  - Signature: `const MAC_TRAFFIC_LIGHT_CLEARANCE: f32 = 90.0`
  - Purpose: Left toolbar inset reserving space for macOS traffic lights.
  - Behavior: Used by `toolbar_left_padding` on macOS only.

- **`DEFAULT_LEFT_PADDING`** (private, L20)
  - Signature: `const DEFAULT_LEFT_PADDING: f32 = 12.0`
  - Purpose: Baseline left toolbar padding on non-macOS platforms.
  - Behavior: Minimum left padding when no GNOME left-side caption buttons are cached.

- **`WIN_CAPTION_BUTTON_WIDTH`** (private, L22)
  - Signature: `const WIN_CAPTION_BUTTON_WIDTH: f32 = 46.0`
  - Purpose: Hit width of each Windows Fluent caption button.
  - Behavior: Three buttons reserved in `right_controls_reserved_width`.

- **`GTK_CAPTION_SLOT`** (private, L24)
  - Signature: `const GTK_CAPTION_SLOT: f32 = 36.0`
  - Purpose: Adwaita/GTK circular caption button slot width.
  - Behavior: Used in Linux CSD button layout and width calculations.

- **`GTK_CAPTION_GAP`** (private, L25)
  - Signature: `const GTK_CAPTION_GAP: f32 = 4.0`
  - Purpose: Spacing between Linux CSD caption buttons.
  - Behavior: Applied in `caption_side_width` and GTK-style control row gap.

- **`CLIENT_SIDE_SHADOW`** (private, L26)
  - Signature: `const CLIENT_SIDE_SHADOW: f32 = 10.0`
  - Purpose: CSD outer shadow/inset and resize hit margin in pixels.
  - Behavior: Drives client inset, resize edge detection, and shadow padding in `wrap_client_decorations`.

- **`CLIENT_SIDE_ROUNDING`** (private, L27)
  - Signature: `const CLIENT_SIDE_ROUNDING: f32 = 10.0`
  - Purpose: Corner radius for client-side decorated windows.
  - Behavior: Applied to outer/backdrop and inner content unless edge is tiled flush.

- **`CLIENT_SIDE_BORDER`** (private, L28)
  - Signature: `const CLIENT_SIDE_BORDER: f32 = 1.0`
  - Purpose: Border thickness for CSD inner frame.
  - Behavior: Drawn per non-tiled edge inside `wrap_client_decorations`.

- **`CaptionLayout`** (private, L32)
  - Signature: `struct CaptionLayout { left: Vec<CaptionKind>, right: Vec<CaptionKind> }`
  - Purpose: Which caption buttons appear on each titlebar side.
  - Behavior: Parsed from GNOME `button-layout`; cached in `CAPTION_LAYOUT`.

- **`CaptionKind`** (private, L38)
  - Signature: `enum CaptionKind { Minimize, Maximize, Close }`
  - Purpose: Logical caption button kinds for layout and rendering.
  - Behavior: Mapped from gsettings tokens and to UI buttons/actions.

- **`CAPTION_LAYOUT`** (private, L45)
  - Signature: `static CAPTION_LAYOUT: RwLock<Option<CaptionLayout>>`
  - Purpose: Process-wide cache of desktop button layout.
  - Behavior: Populated lazily by `cached_caption_layout` and refreshed via `refresh_caption_layout`.

- **`CaptionWindowControls`** (private, L413)
  - Signature: `struct CaptionWindowControls { buttons, maximized, gtk_style }`
  - Purpose: Renders a row of caption buttons for Windows or Linux CSD.
  - Behavior: Built by `windows()` or `linux()` factories; rendered via `RenderOnce`.

- **`CaptionButton`** (private, L468)
  - Signature: `struct CaptionButton { kind, gtk_style, index }`
  - Purpose: Single minimize/maximize/restore/close caption control.
  - Behavior: Maps to icons, GPUI `WindowControlArea`, and window actions.

- **`CaptionButtonKind`** (private, L475)
  - Signature: `enum CaptionButtonKind { Minimize, Restore, Maximize, Close }`
  - Purpose: Concrete button variant including restore state when maximized.
  - Behavior: Selected in `CaptionWindowControls::render` based on kind and `maximized`.

- **`CaptionAction`** (private, L574)
  - Signature: `enum CaptionAction { Minimize, Zoom, Close }`
  - Purpose: Window action triggered by a caption button click.
  - Behavior: Minimize/zoom/close map to GPUI window APIs and `cx.quit()` for close.

#### Functions / methods

##### Context: `module`

- **`main_titlebar_options`** (pub, L47)
  - Signature: `pub fn main_titlebar_options() -> TitlebarOptions`
  - Purpose: GPUI titlebar configuration for the transparent main toolbar strip.
  - Behavior: Sets title "BGMail", transparent appearance, and macOS traffic-light position when applicable.

- **`main_window_decorations`** (pub, L57)
  - Signature: `pub fn main_window_decorations() -> Option<WindowDecorations>`
  - Purpose: Requests client-side decorations on Linux/FreeBSD.
  - Behavior: Returns `Some(WindowDecorations::Client)` on Linux/FreeBSD; `None` elsewhere (native SSD/macOS traffic lights).

- **`traffic_light_position`** (private, L66)
  - Signature: `fn traffic_light_position() -> Option<gpui::Point<Pixels>>` (macOS)
  - Purpose: Positions native traffic lights for transparent titlebar.
  - Behavior: Returns point (12, 16) in pixels.

- **`traffic_light_position`** (private, L71)
  - Signature: `fn traffic_light_position() -> Option<gpui::Point<Pixels>>` (non-macOS)
  - Purpose: Stub when native traffic lights are absent.
  - Behavior: Returns `None`.

- **`toolbar_left_padding`** (pub, L75)
  - Signature: `pub fn toolbar_left_padding() -> Pixels`
  - Purpose: Left inset for toolbar content clearing window controls.
  - Behavior: macOS uses traffic-light clearance; other platforms use default padding (Linux may add left caption width separately via `left_controls_reserved_width`).

- **`right_controls_reserved_width`** (pub, L84)
  - Signature: `pub fn right_controls_reserved_width() -> Pixels`
  - Purpose: Toolbar right inset reserving custom caption buttons.
  - Behavior: Windows reserves three rectangular buttons; Linux/FreeBSD uses cached right-side GTK layout width; zero elsewhere.

- **`left_controls_reserved_width`** (pub, L95)
  - Signature: `pub fn left_controls_reserved_width() -> Pixels`
  - Purpose: Toolbar left inset for GNOME-style left-side caption buttons.
  - Behavior: Non-zero on Linux/FreeBSD from cached left layout; zero on other platforms.

- **`caption_side_width`** (private, L103)
  - Signature: `fn caption_side_width(buttons: &[CaptionKind]) -> Pixels`
  - Purpose: Computes horizontal space for a caption button group.
  - Behavior: Zero when empty; otherwise `n * GTK_CAPTION_SLOT + (n-1) * GTK_CAPTION_GAP`.

- **`uses_custom_caption_buttons`** (private, L111)
  - Signature: `fn uses_custom_caption_buttons(window: &Window) -> bool`
  - Purpose: Whether this window draws custom caption controls.
  - Behavior: True on Windows always; on Linux/FreeBSD when decorations are client-side.

- **`refresh_caption_layout`** (pub, L123)
  - Signature: `pub fn refresh_caption_layout()`
  - Purpose: Re-reads GNOME button layout when the main window activates.
  - Behavior: No-op off Linux/FreeBSD; otherwise shells `gsettings` and updates `CAPTION_LAYOUT` cache.

- **`cached_caption_layout`** (private, L133)
  - Signature: `fn cached_caption_layout() -> CaptionLayout`
  - Purpose: Returns cached desktop caption layout, populating on first use.
  - Behavior: Reads RwLock cache or computes via `linux_caption_layout_from_desktop(None)` and stores result.

- **`linux_caption_layout_from_desktop`** (private, L148)
  - Signature: `fn linux_caption_layout_from_desktop(controls: Option<WindowControls>) -> CaptionLayout`
  - Purpose: Builds Linux CSD caption layout from gsettings and desktop heuristics.
  - Behavior: Parses `button-layout` when available; GNOME-like fallback close-only; KDE-style full trio otherwise; optional compositor capability filtering.

- **`filter_layout_by_controls`** (private, L174)
  - Signature: `fn filter_layout_by_controls(layout: &mut CaptionLayout, controls: WindowControls)`
  - Purpose: Removes caption buttons unsupported by the compositor.
  - Behavior: Retains minimize/maximize only when reported available; close always kept.

- **`parse_button_layout`** (private, L185)
  - Signature: `fn parse_button_layout(raw: &str) -> CaptionLayout`
  - Purpose: Parses GNOME `button-layout` string (`left:right`).
  - Behavior: Trims quotes; splits on `:`; parses comma-separated sides via `parse_button_side`.

- **`parse_button_side`** (private, L197)
  - Signature: `fn parse_button_side(side: &str) -> Vec<CaptionKind>`
  - Purpose: Parses one side of a button-layout string.
  - Behavior: Maps `minimize`, `maximize`, `close` tokens; ignores unknown tokens like `appmenu`.

- **`is_gnome_like_desktop`** (private, L208)
  - Signature: `fn is_gnome_like_desktop() -> bool`
  - Purpose: Detects GNOME/Unity/COSMIC desktops for layout defaults.
  - Behavior: Checks `XDG_CURRENT_DESKTOP` colon-separated parts case-insensitively.

- **`read_gnome_button_layout`** (private, L220)
  - Signature: `fn read_gnome_button_layout() -> Option<String>`
  - Purpose: Reads `org.gnome.desktop.wm.preferences button-layout` via gsettings.
  - Behavior: Returns trimmed stdout on success; `None` on failure or empty output.

- **`render_right_window_controls`** (pub, L237)
  - Signature: `pub fn render_right_window_controls(window: &mut Window) -> Option<AnyElement>`
  - Purpose: Builds right-side caption controls when needed.
  - Behavior: Returns `None` when custom captions unused; Windows full trio; Linux right-side layout from `linux_layout_for_window`.

- **`render_left_window_controls`** (pub, L252)
  - Signature: `pub fn render_left_window_controls(window: &mut Window) -> Option<AnyElement>`
  - Purpose: Builds left-side caption controls for GNOME layouts like `close:minimize,maximize`.
  - Behavior: Linux/FreeBSD client-decorated windows only; `None` when left layout empty.

- **`linux_layout_for_window`** (private, L266)
  - Signature: `fn linux_layout_for_window(window: &Window) -> CaptionLayout`
  - Purpose: Combines cached gsettings layout with live compositor restrictions.
  - Behavior: Starts from cache; when Wayland reports min/max unavailable, filters via `filter_layout_by_controls`.

- **`wrap_client_decorations`** (pub, L280)
  - Signature: `pub fn wrap_client_decorations(content: AnyElement, window: &mut Window, border_color: Hsla) -> AnyElement`
  - Purpose: Wraps main UI in Zed-style CSD chrome (shadow, border, resize handles).
  - Behavior: No-op for server-side decorations. For client decorations, sets client inset, draws resize hit canvas with cursor changes, handles edge resize on mouse down, and nests bordered/shadowed rounded content respecting tiling flags.

- **`resize_edge`** (private, L380)
  - Signature: `fn resize_edge(pos: Point<Pixels>, shadow_size: Pixels, size: Size<Pixels>) -> Option<ResizeEdge>`
  - Purpose: Hit-tests pointer position against CSD resize margins.
  - Behavior: Checks corners first within `shadow_size` band, then edges; returns `None` in interior.

- **`cursor_for_resize_edge`** (private, L403)
  - Signature: `fn cursor_for_resize_edge(edge: ResizeEdge) -> CursorStyle`
  - Purpose: Maps resize edge to GPUI cursor style.
  - Behavior: Vertical, horizontal, or diagonal resize cursors per edge enum.

- **`main_titlebar_keeps_macos_traffic_lights_only_on_macos`** (private, L638)
  - Signature: `fn main_titlebar_keeps_macos_traffic_lights_only_on_macos()` (test)
  - Purpose: Validates titlebar options per platform.
  - Behavior: Asserts transparent title and BGMail title; traffic-light position only on macOS.

- **`toolbar_padding_reserves_space_for_macos_traffic_lights_only`** (private, L652)
  - Signature: `fn toolbar_padding_reserves_space_for_macos_traffic_lights_only()` (test)
  - Purpose: Checks left padding platform behavior.
  - Behavior: macOS equals clearance constant; elsewhere at least default padding.

- **`main_window_requests_client_decorations_on_linux`** (private, L664)
  - Signature: `fn main_window_requests_client_decorations_on_linux()` (test)
  - Purpose: Ensures Linux/FreeBSD request client decorations.
  - Behavior: Compares `main_window_decorations()` to `Some(Client)` on those targets.

- **`right_controls_reserve_width_on_windows`** (private, L674)
  - Signature: `fn right_controls_reserve_width_on_windows()` (test)
  - Purpose: Validates reserved caption width on Windows/macOS.
  - Behavior: Windows equals three button widths; macOS zero.

- **`parse_button_layout_gnome_close_only`** (private, L685)
  - Signature: `fn parse_button_layout_gnome_close_only()` (test)
  - Purpose: Parses GNOME default close-only layout.
  - Behavior: `:close` yields empty left and single Close on right.

- **`parse_button_layout_full_right`** (private, L692)
  - Signature: `fn parse_button_layout_full_right()` (test)
  - Purpose: Parses standard right-side trio layout.
  - Behavior: Expects minimize, maximize, close on right with empty left.

- **`parse_button_layout_close_on_left`** (private, L706)
  - Signature: `fn parse_button_layout_close_on_left()` (test)
  - Purpose: Parses split left/right caption layout.
  - Behavior: Close on left; minimize and maximize on right.

- **`parse_button_layout_ignores_menu_and_appmenu`** (private, L716)
  - Signature: `fn parse_button_layout_ignores_menu_and_appmenu()` (test)
  - Purpose: Ensures unknown layout tokens are ignored.
  - Behavior: `appmenu:minimize,maximize,close` yields three right buttons and empty left.

- **`filter_layout_hides_unavailable_min_max`** (private, L723)
  - Signature: `fn filter_layout_hides_unavailable_min_max()` (test)
  - Purpose: Verifies compositor capability filtering.
  - Behavior: When min/max unavailable, only Close remains.

- **`caption_side_width_scales_with_button_count`** (private, L738)
  - Signature: `fn caption_side_width_scales_with_button_count()` (test)
  - Purpose: Ensures width grows with button count.
  - Behavior: Empty side zero; two buttons wider than one.

- **`caption_buttons_map_to_explicit_actions`** (private, L746)
  - Signature: `fn caption_buttons_map_to_explicit_actions()` (test)
  - Purpose: Validates caption button action mapping.
  - Behavior: Minimize→Minimize, maximize/restore→Zoom, close→Close.

- **`caption_buttons_map_to_native_control_areas`** (private, L754)
  - Signature: `fn caption_buttons_map_to_native_control_areas()` (test)
  - Purpose: Validates GPUI native control area mapping.
  - Behavior: Min→Min, max/restore→Max, close→Close.

- **`resize_edge_detects_corners_and_sides`** (private, L774)
  - Signature: `fn resize_edge_detects_corners_and_sides()` (test)
  - Purpose: Spot-checks resize hit testing.
  - Behavior: Corner maps to TopLeft; interior None; top edge detected mid-width.

##### Context: `CaptionWindowControls`

- **`windows`** (private, L421)
  - Signature: `fn windows(maximized: bool) -> Self`
  - Purpose: Factory for Windows Fluent caption strip.
  - Behavior: Fixed minimize/maximize/close order, rectangular style (`gtk_style: false`).

- **`linux`** (private, L433)
  - Signature: `fn linux(buttons: Vec<CaptionKind>, maximized: bool) -> Self`
  - Purpose: Factory for Linux Adwaita-style caption row.
  - Behavior: Uses provided button list and circular GTK styling.

##### Context: `RenderOnce for CaptionWindowControls`

- **`render`** (private, L443)
  - Signature: `fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement`
  - Purpose: Renders horizontal caption button row.
  - Behavior: Builds flex row; adds GTK gap/padding when `gtk_style`; chooses restore vs maximize icon when maximized; assigns stable ids per index.

##### Context: `CaptionButton`

- **`minimize`** (private, L483)
  - Signature: `fn minimize() -> Self`
  - Purpose: Builder for minimize caption button.
  - Behavior: Sets `CaptionButtonKind::Minimize`.

- **`restore`** (private, L491)
  - Signature: `fn restore() -> Self`
  - Purpose: Builder for restore (un-maximize) caption button.
  - Behavior: Sets `CaptionButtonKind::Restore`.

- **`maximize`** (private, L499)
  - Signature: `fn maximize() -> Self`
  - Purpose: Builder for maximize caption button.
  - Behavior: Sets `CaptionButtonKind::Maximize`.

- **`close`** (private, L507)
  - Signature: `fn close() -> Self`
  - Purpose: Builder for close caption button.
  - Behavior: Sets `CaptionButtonKind::Close`.

- **`with_style`** (private, L515)
  - Signature: `fn with_style(mut self, gtk_style: bool) -> Self`
  - Purpose: Selects GTK circular vs Windows rectangular styling.
  - Behavior: Sets `gtk_style` flag on builder.

- **`with_index`** (private, L520)
  - Signature: `fn with_index(mut self, index: usize) -> Self`
  - Purpose: Assigns stable index for element id disambiguation.
  - Behavior: Stores index used by `id()`.

- **`id`** (private, L525)
  - Signature: `fn id(self) -> SharedString`
  - Purpose: Stable GPUI element id for a caption button.
  - Behavior: Formats `"{kind}-{index}"` (minimize, restore, maximize, close).

- **`icon`** (private, L539)
  - Signature: `fn icon(self) -> IconName`
  - Purpose: Icon for the caption button kind.
  - Behavior: WindowMinimize/Restore/Maximize icons; Close uses Clear icon.

- **`action`** (private, L548)
  - Signature: `fn action(self) -> CaptionAction`
  - Purpose: Window action associated with the button.
  - Behavior: Minimize→Minimize; Restore/Maximize→Zoom; Close→Close.

- **`control_area`** (private, L556)
  - Signature: `fn control_area(self) -> WindowControlArea`
  - Purpose: Native window control hit region for the platform shell.
  - Behavior: Maps actions to Min/Max/Close `WindowControlArea` values.

- **`activate`** (private, L564)
  - Signature: `fn activate(self, window: &mut Window, cx: &mut App)`
  - Purpose: Executes caption button click behavior.
  - Behavior: Minimize calls `minimize_window`; Zoom calls `zoom_window`; Close calls `cx.quit()`.

##### Context: `RenderOnce for CaptionButton`

- **`render`** (private, L581)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Draws one caption button with hover and native control area.
  - Behavior: Close button hovers error color; others use element hover. GTK style uses circular compact target; Windows style uses full-height rectangular strip. Registers `window_control_area` and click handler calling `activate`.

