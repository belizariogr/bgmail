//! Linux e-mail body rendering via CEF (Chromium Embedded Framework) using
//! **windowless off-screen rendering (OSR)**.
//!
//! # Why OSR on Linux (and not a child webview)
//!
//! On macOS and Windows the reader embeds a native child webview (`wry`:
//! WKWebView / WebView2) layered over the GPUI reader pane. On Linux `wry` uses
//! WebKitGTK, which can only be embedded as a *child window on X11*. Under a
//! Wayland session there is no X11 child-window model to reparent into, so the
//! child-webview approach only works via XWayland — a compatibility shim we do
//! not want to depend on.
//!
//! Instead, on Linux we run Chromium in **windowless mode**: CEF renders the page
//! into an off-screen BGRA buffer (`on_paint`) and we upload that buffer as a
//! [`gpui::RenderImage`] which GPUI composites into the reader pane like any other
//! texture. This composites natively on Wayland (and X11) with no reparenting.
//!
//! We use **soft OSR** (`shared_texture_enabled: false`): CEF hands us a CPU BGRA
//! buffer that we copy. Zero-copy GPU import (DMA-BUF) exists behind CEF's
//! `accelerated_osr` feature but pulls in `wgpu`/`ash`; a CPU copy of a single
//! reader-sized surface is inexpensive and keeps the dependency surface small.
//! Soft OSR makes Chromium's *smooth scrolling* expensive (many full-buffer
//! paints), so we disable it and rely on discrete wheel steps plus a short
//! GPUI redraw loop (`Context::notify` after paint/input) to show each
//! `on_paint` promptly.
//!
//! CEF is driven by an **external message pump**: the GPUI view calls [`pump`]
//! each paint (and after input), which forwards to `cef::do_message_loop_work()`.
//! Everything here runs on the main thread; painted frames are stashed in an
//! `Arc<Mutex<..>>` and converted to a [`gpui::RenderImage`] on the caller's
//! thread in [`OsrBrowser::take_frame`].
//!
//! The IPC used by the injected content script (see [`crate::web_view`]) is
//! bridged over the browser's console: a tiny shim maps `window.ipc.postMessage`
//! to `console.log('__BGMAIL_IPC__' + msg)`, and [`DisplayHandler::on_console_message`]
//! strips the prefix and routes the message just like the `wry` `with_ipc_handler`.

#![allow(clippy::type_complexity)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use async_channel::Sender;
use cef::{args::Args, *};
use gpui::RenderImage;

use crate::web_view::{
    decode_data_uri, downloads_dir, is_external_link, parse_ipc_message, unique_download_path,
    HostEvent, IpcMessage, CONTENT_SCRIPT,
};

// CEF `cef_event_flags_t` bits we care about for mouse input. Declared locally to
// avoid depending on the exact enum path across binding targets; the numeric
// values are part of CEF's stable ABI.
const EVENTFLAG_SHIFT_DOWN: u32 = 1 << 1;
const EVENTFLAG_CONTROL_DOWN: u32 = 1 << 2;
const EVENTFLAG_ALT_DOWN: u32 = 1 << 3;
const EVENTFLAG_LEFT_MOUSE_BUTTON: u32 = 1 << 4;
const EVENTFLAG_MIDDLE_MOUSE_BUTTON: u32 = 1 << 5;
const EVENTFLAG_RIGHT_MOUSE_BUTTON: u32 = 1 << 6;

/// A logical mouse button, mirroring the subset GPUI reports that we forward to
/// CEF. Kept independent of GPUI's own `MouseButton` so the reader glue does the
/// mapping and this module stays free of GPUI input types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

// --- Process-wide CEF lifecycle ------------------------------------------------

thread_local! {
    /// The initialized CEF runtime for this (browser) process. `None` until
    /// [`initialize`] succeeds; cleared by [`shutdown_cef`]. CEF and the objects
    /// it references must stay on the main thread, hence a thread-local.
    static CEF: RefCell<Option<CefRuntime>> = const { RefCell::new(None) };
}

/// Owns everything that must outlive `cef::initialize`: the `App` (whose handlers
/// CEF keeps calling) and the process `Args` (whose `argv` storage the main-args
/// pointer borrows). `ready` flips true once CEF's context is initialized.
struct CefRuntime {
    _app: App,
    _args: Args,
    ready: Rc<Cell<bool>>,
}

/// Builds the CEF `App` carrying our command-line tweaks and the browser-process
/// handler that reports context readiness through `ready`.
fn build_app(ready: Rc<Cell<bool>>) -> App {
    AppBuilder::new(BgApp { ready })
}

/// Returns true if this process is a CEF sub-process (renderer, GPU, utility,
/// …) that has already run to completion, in which case `main` must return
/// immediately without starting the UI. Returns false for the main/browser
/// process, which then proceeds to open the window and call [`initialize`].
///
/// Mirrors the OSR example's launch flow: a single binary is re-exec'd by CEF
/// for each sub-process with a `--type=` switch; `execute_process` runs the
/// sub-process to completion (returns >= 0) or returns -1 in the browser process.
pub fn run_if_subprocess() -> bool {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let Some(cmd) = args.as_cmd_line() else {
        return false;
    };
    // Sub-processes are launched with `--type=<renderer|gpu-process|...>`.
    let switch = CefString::from("type");
    let is_browser_process = cmd.has_switch(Some(&switch)) != 1;
    let ready = Rc::new(Cell::new(false));
    let mut app = build_app(ready);
    let ret = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if is_browser_process {
        // Browser process: `execute_process` returns -1 and we continue booting.
        false
    } else {
        // Sub-process: CEF already ran its own loop; the caller should exit.
        let _ = ret;
        true
    }
}

/// Initializes CEF for windowless OSR with an external message pump. Idempotent:
/// a second call while already initialized returns true without re-initializing.
/// Returns false if `cef::initialize` fails (e.g. the CEF runtime is missing).
pub fn initialize() -> bool {
    if CEF.with_borrow(|slot| slot.is_some()) {
        return true;
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let ready = Rc::new(Cell::new(false));
    let mut app = build_app(ready.clone());
    let settings = Settings {
        windowless_rendering_enabled: 1,
        // We drive the loop ourselves from GPUI's frame loop (see `pump`).
        external_message_pump: 1,
        // Dev-friendly: the zygote/setuid sandbox needs extra setup we skip for
        // a local build. Production packaging can flip this off.
        no_sandbox: 1,
        ..Default::default()
    };
    let ok = cef::initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) == 1;
    if ok {
        CEF.with_borrow_mut(|slot| {
            *slot = Some(CefRuntime {
                _app: app,
                _args: args,
                ready,
            });
        });
    }
    ok
}

/// Advances CEF's message loop. Safe no-op when CEF was never initialized (e.g.
/// the runtime failed to load, so the reader falls back to plain text).
pub fn pump() {
    if CEF.with_borrow(|slot| slot.is_some()) {
        do_message_loop_work();
    }
}

/// Shuts CEF down. Call once on app quit. No-op if not initialized.
pub fn shutdown_cef() {
    let was_initialized = CEF.with_borrow_mut(|slot| slot.take().is_some());
    if was_initialized {
        cef::shutdown();
    }
}

/// Whether CEF's context has finished initializing and can host browsers. New
/// [`OsrBrowser`]s should only be created once this is true.
pub fn is_ready() -> bool {
    CEF.with_borrow(|slot| slot.as_ref().is_some_and(|rt| rt.ready.get()))
}

// --- App / browser-process handler --------------------------------------------

#[derive(Clone)]
struct BgApp {
    ready: Rc<Cell<bool>>,
}

wrap_app! {
    struct AppBuilder {
        app: BgApp,
    }

    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(command_line) = command_line else {
                return;
            };
            // A reader, not a browser: no extensions, and no background phone-home.
            command_line.append_switch(Some(&"disable-extensions".into()));
            command_line.append_switch(Some(&"disable-background-networking".into()));
            // Soft OSR copies the full view each paint; Chromium smooth-scroll
            // would emit dozens of those per gesture and feel like molasses.
            command_line.append_switch(Some(&"disable-smooth-scrolling".into()));
            // Chromium's on-device ML stack probes WebGPU and logs
            // `Unable to get gpu adapter` when the adapter is unavailable (common
            // under Wayland OSR). We never use those models in the reader.
            command_line.append_switch_with_value(
                Some(&"disable-features".into()),
                Some(&"OnDeviceModel,OnDeviceModelService,OptimizationGuideOnDeviceModel".into()),
            );
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BrowserProcessHandlerBuilder::new(BgBrowserProcess {
                ready: self.app.ready.clone(),
            }))
        }
    }
}

#[derive(Clone)]
struct BgBrowserProcess {
    ready: Rc<Cell<bool>>,
}

wrap_browser_process_handler! {
    struct BrowserProcessHandlerBuilder {
        handler: BgBrowserProcess,
    }

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            self.handler.ready.set(true);
        }
    }
}

// --- Render handler (OSR paint sink) ------------------------------------------

/// A freshly painted BGRA frame: width, height (physical pixels) and the tightly
/// packed `width * height * 4` byte buffer as delivered by CEF's `on_paint`.
type FrameBuffer = (u32, u32, Vec<u8>);

#[derive(Clone)]
struct BgRender {
    /// Latest painted frame, replaced on every `on_paint` and taken by
    /// [`OsrBrowser::take_frame`]. `Some` means a new frame is pending.
    frame: Arc<Mutex<Option<FrameBuffer>>>,
    /// The view size in logical pixels CEF should render at (see `view_rect`).
    size: Rc<RefCell<(i32, i32)>>,
    /// Device scale factor reported to CEF (`screen_info`), so HiDPI pages render
    /// at native resolution.
    scale: Rc<Cell<f32>>,
}

wrap_render_handler! {
    struct RenderHandlerBuilder {
        handler: BgRender,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let (w, h) = *self.handler.size.borrow();
                // CEF requires a non-zero size; keep the last good one otherwise.
                if w > 0 && h > 0 {
                    rect.x = 0;
                    rect.y = 0;
                    rect.width = w;
                    rect.height = h;
                }
            }
        }

        fn screen_info(
            &self,
            _browser: Option<&mut Browser>,
            screen_info: Option<&mut ScreenInfo>,
        ) -> ::std::os::raw::c_int {
            if let Some(screen_info) = screen_info {
                screen_info.device_scale_factor = self.handler.scale.get();
                return 1;
            }
            0
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            // Only the main view; popups (e.g. <select>) are not surfaced here.
            if type_ != PaintElementType::default() || buffer.is_null() || width <= 0 || height <= 0
            {
                return;
            }
            let len = (width as usize) * (height as usize) * 4;
            // SAFETY: CEF guarantees `buffer` points at `width * height * 4` bytes
            // of BGRA for the duration of this callback; we copy them out
            // immediately and never retain the pointer.
            let bytes = unsafe { std::slice::from_raw_parts(buffer, len) };
            if let Ok(mut guard) = self.handler.frame.lock() {
                store_paint_buffer(&mut guard, width as u32, height as u32, bytes);
            }
        }
    }
}

/// Physical pixel size CEF should produce for a logical view of `width`×`height`
/// at `scale` (device scale factor). Used to detect stale frames after resize.
fn expected_physical_size(width: i32, height: i32, scale: f32) -> (u32, u32) {
    let scale = scale.max(0.01);
    (
        ((width.max(1) as f32) * scale).round() as u32,
        ((height.max(1) as f32) * scale).round() as u32,
    )
}

/// Whether a painted buffer's dimensions match the current view closely enough
/// to fill the reader without stretching (1px slack for rounding).
fn frame_matches_view(frame_w: u32, frame_h: u32, expected_w: u32, expected_h: u32) -> bool {
    frame_w.abs_diff(expected_w) <= 1 && frame_h.abs_diff(expected_h) <= 1
}

/// Writes a CEF `on_paint` buffer into `slot`, reusing the previous allocation
/// when the dimensions match so rapid paints (e.g. scroll) avoid re-allocating
/// a full-view BGRA buffer on every callback.
fn store_paint_buffer(slot: &mut Option<FrameBuffer>, width: u32, height: u32, src: &[u8]) {
    match slot {
        Some((w, h, buf)) if *w == width && *h == height && buf.len() == src.len() => {
            buf.copy_from_slice(src);
        }
        _ => *slot = Some((width, height, src.to_vec())),
    }
}

// --- Display handler (console → IPC bridge) -----------------------------------

#[derive(Clone)]
struct BgDisplay {
    to_host: Sender<HostEvent>,
    notify_body: Rc<RefCell<String>>,
}

wrap_display_handler! {
    struct DisplayHandlerBuilder {
        handler: BgDisplay,
    }

    impl DisplayHandler {
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            if let Some(message) = message {
                let text = message.to_string();
                if let Some(payload) = text.strip_prefix(IPC_CONSOLE_PREFIX) {
                    handle_ipc(payload, &self.handler.to_host, &self.handler.notify_body.borrow());
                    // Swallow our own bridge messages so they never reach the log.
                    return 1;
                }
            }
            0
        }
    }
}

// --- Request handler (external navigation) ------------------------------------

#[derive(Clone)]
struct BgRequest;

wrap_request_handler! {
    struct RequestHandlerBuilder {
        handler: BgRequest,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            if let Some(request) = request {
                let url = CefString::from(&request.url()).to_string();
                // The body itself loads from a `data:` URL (not external), so it
                // proceeds; real links go to the system browser and are cancelled.
                if is_external_link(&url) {
                    let _ = open::that_detached(&url);
                    return 1;
                }
            }
            0
        }
    }
}

// --- Client (ties the handlers together) --------------------------------------

wrap_client! {
    struct ClientBuilder {
        render_handler: RenderHandler,
        display_handler: DisplayHandler,
        request_handler: RequestHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display_handler.clone())
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request_handler.clone())
        }
    }
}

// --- IPC routing (shared with the wry backend's semantics) --------------------

/// Prefix the injected shim prepends to every `window.ipc.postMessage` payload
/// so [`DisplayHandler::on_console_message`] can tell our messages apart from
/// ordinary page logging.
const IPC_CONSOLE_PREFIX: &str = "__BGMAIL_IPC__";

/// Routes a bridged IPC message exactly like the `wry` backend: foreground
/// actions (hover, clipboard, image-shown, palette) go over `to_host`; open and
/// download run here on the main thread.
fn handle_ipc(message: &str, to_host: &Sender<HostEvent>, notify_body: &str) {
    match parse_ipc_message(message) {
        Some(IpcMessage::Hover(url)) => {
            let _ = to_host.try_send(HostEvent::HoverLink(url.to_string()));
        }
        Some(IpcMessage::OpenExternal(url)) => open_in_new_window(url),
        Some(IpcMessage::DownloadImage(url)) => download_image(url, notify_body),
        Some(IpcMessage::CopyToClipboard(text)) => {
            let _ = to_host.try_send(HostEvent::CopyToClipboard(text.to_string()));
        }
        Some(IpcMessage::ShowImage(url)) => {
            let _ = to_host.try_send(HostEvent::ImageShown(url.to_string()));
        }
        Some(IpcMessage::BodyMouseDown) => {
            let _ = to_host.try_send(HostEvent::BodyMouseDown);
        }
        Some(IpcMessage::CommandPalette) => {
            let _ = to_host.try_send(HostEvent::CommandPalette);
        }
        None => {}
    }
}

/// Opens a link/image target outside the reader: remote URLs go to the system
/// browser; an inline `data:` image is materialized to a temp file and opened by
/// the OS default viewer (it has no URL to navigate to).
fn open_in_new_window(url: &str) {
    if is_external_link(url) {
        let _ = open::that_detached(url);
    } else if let Some((extension, bytes)) = decode_data_uri(url) {
        if let Some(path) = persist_temp_file(extension, &bytes) {
            let _ = open::that_detached(path);
        }
    }
}

/// Saves an inline `data:` image straight to the user's Downloads folder (no
/// dialog). Remote images have no local bytes yet, so we fall back to opening
/// them in the browser where the user can save them. `_notify_body` mirrors the
/// wry backend's API; desktop notifications on Linux land with the notification
/// backend later.
fn download_image(url: &str, _notify_body: &str) {
    let Some((extension, bytes)) = decode_data_uri(url) else {
        if is_external_link(url) {
            let _ = open::that_detached(url);
        }
        return;
    };
    let Some(dir) = downloads_dir() else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = unique_download_path(&dir, "image", extension, |candidate| candidate.exists());
    let _ = std::fs::write(&path, bytes);
}

/// Writes `bytes` to a uniquely named temp file and returns its path, so an
/// inline image with no URL can still be handed to the OS default viewer.
fn persist_temp_file(extension: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut path = std::env::temp_dir();
    path.push(format!("rmail-image-{nanos}.{extension}"));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

// --- Document composition (data: URL + injected scripts) ----------------------

/// The IPC shim, injected as the first script in `<head>` so `window.ipc` exists
/// before any page or reader script runs. It funnels messages through the console
/// where [`DisplayHandler::on_console_message`] picks them up.
fn ipc_shim_script() -> String {
    format!(
        "<script>window.ipc={{postMessage:function(m){{console.log('{prefix}'+String(m));}}}};</script>",
        prefix = IPC_CONSOLE_PREFIX,
    )
}

/// Wraps a rendered e-mail document with the pieces the `wry` backend injects out
/// of band: the IPC shim (first, in `<head>`) and the reader's content script
/// (last, before `</body>`), which drives hover reporting and the custom menus.
fn compose_document(html: &str) -> String {
    let shim = ipc_shim_script();
    let content = format!("<script>{CONTENT_SCRIPT}</script>");
    let with_shim = if html.contains("<head>") {
        html.replacen("<head>", &format!("<head>{shim}"), 1)
    } else {
        format!("{shim}{html}")
    };
    if with_shim.contains("</body>") {
        with_shim.replacen("</body>", &format!("{content}</body>"), 1)
    } else {
        format!("{with_shim}{content}")
    }
}

/// Encodes a full HTML document as a `data:` URL CEF can load into the main
/// frame. Percent-encoding keeps the markup intact through the URL parser.
fn data_url(html: &str) -> String {
    let doc = compose_document(html);
    format!("data:text/html;charset=utf-8,{}", urlencoding::encode(&doc))
}

// --- OsrBrowser ---------------------------------------------------------------

/// A single windowless CEF browser rendering one e-mail body off-screen. Owns the
/// paint buffer shared with the render handler and converts painted frames into
/// [`gpui::RenderImage`]s for GPUI to composite.
pub struct OsrBrowser {
    browser: Browser,
    frame: Arc<Mutex<Option<FrameBuffer>>>,
    size: Rc<RefCell<(i32, i32)>>,
    scale: Rc<Cell<f32>>,
    /// The most recently produced image, kept so the reader can repaint it every
    /// frame and so [`take_frame`] can hand back the outgoing one for disposal.
    current: Option<Arc<RenderImage>>,
    /// Physical pixel size of [`Self::current`], so the painter can map it 1:1
    /// (unstretched) when the view has resized and CEF has not caught up yet.
    current_px: Option<(u32, u32)>,
    /// Currently-held mouse-button event flags, OR'd into move/wheel events so
    /// Chromium performs drag-selection while a button is down.
    buttons: Cell<u32>,
    last_html: String,
    visible: bool,
    /// Localized "image downloaded" text, shared with the display handler so a
    /// language switch is reflected without rebuilding the browser.
    notify_body: Rc<RefCell<String>>,
    /// Coalesced wheel deltas waiting to be flushed into CEF. Trackpads emit
    /// many small pixel events per gesture; sending each one forces a full soft
    /// OSR paint, so we accumulate until the next [`Self::flush_input`].
    pending_wheel: Cell<Option<(f32, f32, f32, f32)>>,
}

impl OsrBrowser {
    /// Creates a windowless browser loading `html` (as a `data:` URL). Returns
    /// `None` if CEF is not yet ready or the browser can't be created; the caller
    /// retries on a later frame.
    pub fn new(html: &str, to_host: Sender<HostEvent>, notify_body: String) -> Option<Self> {
        if !is_ready() {
            return None;
        }
        let frame: Arc<Mutex<Option<FrameBuffer>>> = Arc::new(Mutex::new(None));
        // Start non-zero so `view_rect` is valid before the first `set_size`.
        let size = Rc::new(RefCell::new((800, 600)));
        let scale = Rc::new(Cell::new(1.0));
        let notify_body = Rc::new(RefCell::new(notify_body));

        let render = RenderHandlerBuilder::new(BgRender {
            frame: frame.clone(),
            size: size.clone(),
            scale: scale.clone(),
        });
        let display = DisplayHandlerBuilder::new(BgDisplay {
            to_host,
            notify_body: notify_body.clone(),
        });
        let request = RequestHandlerBuilder::new(BgRequest);
        let mut client = ClientBuilder::new(render, display, request);

        // Windowless OSR requires the Alloy runtime style (the Chrome runtime does
        // not support off-screen rendering). Soft OSR: no shared texture / begin
        // frame, so CEF paints into the CPU buffer during the message loop.
        let window_info = WindowInfo {
            windowless_rendering_enabled: 1,
            shared_texture_enabled: 0,
            external_begin_frame_enabled: 0,
            runtime_style: RuntimeStyle::ALLOY,
            ..Default::default()
        };
        let browser_settings = BrowserSettings {
            windowless_frame_rate: 60,
            ..Default::default()
        };

        let url = CefString::from(data_url(html).as_str());
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client),
            Some(&url),
            Some(&browser_settings),
            None,
            None,
        )?;

        Some(Self {
            browser,
            frame,
            size,
            scale,
            current: None,
            current_px: None,
            buttons: Cell::new(0),
            last_html: html.to_string(),
            visible: true,
            notify_body,
            pending_wheel: Cell::new(None),
        })
    }

    /// Reloads the document if it changed (new message selected, theme toggled).
    pub fn set_html(&mut self, html: &str) {
        if self.last_html == html {
            return;
        }
        if let Some(frame) = self.browser.main_frame() {
            let url = CefString::from(data_url(html).as_str());
            frame.load_url(Some(&url));
            self.last_html = html.to_string();
        }
    }

    /// Updates the localized download-confirmation text live.
    pub fn set_notify_text(&self, body: String) {
        *self.notify_body.borrow_mut() = body;
    }

    /// Sets the render size (logical pixels) and device scale factor, telling CEF
    /// to re-render at the new geometry. Returns `true` when the geometry changed
    /// so the caller can keep GPUI frames flowing until a matching paint arrives.
    pub fn set_size(&mut self, width: f32, height: f32, scale_factor: f32) -> bool {
        let next = (width.max(1.0) as i32, height.max(1.0) as i32);
        let size_changed = *self.size.borrow() != next;
        *self.size.borrow_mut() = next;
        let scale_changed = (self.scale.get() - scale_factor).abs() > f32::EPSILON;
        self.scale.set(scale_factor);
        if !(size_changed || scale_changed) {
            return false;
        }
        if let Some(host) = self.browser.host() {
            if scale_changed {
                host.notify_screen_info_changed();
            }
            host.was_resized();
            // Force a paint at the new view_rect; without this, soft OSR can keep
            // serving the previous buffer while GPUI already lays out a new size.
            host.invalidate(PaintElementType::default());
        }
        true
    }

    /// Shows or hides the browser. Hiding pauses rendering (`was_hidden`), so a
    /// background reader doesn't keep painting.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if let Some(host) = self.browser.host() {
            host.was_hidden(if visible { 0 } else { 1 });
        }
    }

    /// Takes the latest painted frame as a [`gpui::RenderImage`] if one arrived
    /// since the last call, returning it together with the previous frame so the
    /// caller can release it from GPUI's texture cache. Returns `None` when no
    /// new frame is pending.
    pub fn take_frame(&mut self) -> Option<(Arc<RenderImage>, Option<Arc<RenderImage>>)> {
        let taken = self.frame.lock().ok().and_then(|mut guard| guard.take());
        let (width, height, bytes) = taken?;
        // CEF delivers BGRA; GPUI's `RenderImage` frame buffer is also BGRA, so we
        // hand the bytes over as-is (the `RgbaImage` is just the byte container).
        let image = image::RgbaImage::from_raw(width, height, bytes)?;
        let render = Arc::new(RenderImage::new(smallvec::SmallVec::from_elem(
            image::Frame::new(image),
            1,
        )));
        let previous = self.current.take();
        self.current = Some(render.clone());
        self.current_px = Some((width, height));
        Some((render, previous))
    }

    /// Whether CEF has painted a frame that has not yet been consumed by
    /// [`Self::take_frame`]. Used to keep GPUI's animation loop alive.
    pub fn has_pending_frame(&self) -> bool {
        self.frame.lock().ok().is_some_and(|guard| guard.is_some())
    }

    /// The current frame image, if any, for repainting between updates.
    pub fn current_frame(&self) -> Option<Arc<RenderImage>> {
        self.current.clone()
    }

    /// Physical pixel size of [`Self::current_frame`], if any.
    pub fn current_frame_px(&self) -> Option<(u32, u32)> {
        self.current_px
    }

    /// Whether [`Self::current_frame`] matches the view size CEF should be
    /// rendering now (safe to stretch-fill the reader bounds).
    pub fn current_frame_fits_view(&self) -> bool {
        let Some((fw, fh)) = self.current_px else {
            return false;
        };
        let (w, h) = *self.size.borrow();
        let (ew, eh) = expected_physical_size(w, h, self.scale.get());
        frame_matches_view(fw, fh, ew, eh)
    }

    /// Flushes coalesced wheel input into CEF, then advances the message loop so
    /// any resulting `on_paint` is available before the caller samples frames.
    pub fn flush_input(&self) {
        if let Some((x, y, dx, dy)) = self.pending_wheel.take() {
            self.send_wheel(x, y, dx, dy);
        }
        pump();
    }

    /// Forwards a mouse-move at view-relative logical coordinates.
    pub fn mouse_move(&self, x: f32, y: f32, modifiers: u32) {
        // Don't let a pending wheel sit forever if the pointer moves first.
        self.flush_pending_wheel();
        let event = MouseEvent {
            x: x as i32,
            y: y as i32,
            modifiers: modifiers | self.buttons.get(),
        };
        if let Some(host) = self.browser.host() {
            host.send_mouse_move_event(Some(&event), 0);
        }
    }

    /// Forwards a mouse press/release, tracking the held-button flags used by
    /// [`mouse_move`] for drag-selection.
    pub fn mouse_click(
        &self,
        x: f32,
        y: f32,
        button: MouseButton,
        pressed: bool,
        click_count: i32,
    ) {
        self.flush_pending_wheel();
        let (button_type, flag) = match button {
            MouseButton::Left => (MouseButtonType::LEFT, EVENTFLAG_LEFT_MOUSE_BUTTON),
            MouseButton::Right => (MouseButtonType::RIGHT, EVENTFLAG_RIGHT_MOUSE_BUTTON),
            MouseButton::Middle => (MouseButtonType::MIDDLE, EVENTFLAG_MIDDLE_MOUSE_BUTTON),
        };
        if pressed {
            self.buttons.set(self.buttons.get() | flag);
        } else {
            self.buttons.set(self.buttons.get() & !flag);
        }
        let event = MouseEvent {
            x: x as i32,
            y: y as i32,
            modifiers: self.buttons.get(),
        };
        if let Some(host) = self.browser.host() {
            host.send_mouse_click_event(
                Some(&event),
                button_type,
                if pressed { 0 } else { 1 },
                click_count,
            );
        }
    }

    /// Whether a mouse button is currently held inside the OSR view (drag-select).
    pub fn has_mouse_capture(&self) -> bool {
        self.buttons.get() != 0
    }

    /// Queues a scroll-wheel delta (view-relative logical coordinates). Multiple
    /// events between paints are summed so CEF paints once per GPUI frame.
    pub fn mouse_wheel(&self, x: f32, y: f32, delta_x: f32, delta_y: f32) {
        let (dx, dy) = match self.pending_wheel.get() {
            Some((_, _, pdx, pdy)) => (pdx + delta_x, pdy + delta_y),
            None => (delta_x, delta_y),
        };
        // Keep the latest pointer position; deltas accumulate.
        self.pending_wheel.set(Some((x, y, dx, dy)));
    }

    fn flush_pending_wheel(&self) {
        if let Some((x, y, dx, dy)) = self.pending_wheel.take() {
            self.send_wheel(x, y, dx, dy);
        }
    }

    fn send_wheel(&self, x: f32, y: f32, delta_x: f32, delta_y: f32) {
        // CEF ignores (0, 0) deltas; skip the round-trip when coalescing cancelled out.
        if delta_x as i32 == 0 && delta_y as i32 == 0 {
            return;
        }
        let event = MouseEvent {
            x: x as i32,
            y: y as i32,
            modifiers: self.buttons.get(),
        };
        if let Some(host) = self.browser.host() {
            host.send_mouse_wheel_event(Some(&event), delta_x as i32, delta_y as i32);
        }
    }

    /// Closes any custom context menu open inside the document (the host calls
    /// this on outside clicks that never reach the document).
    pub fn dismiss_context_menu(&self) {
        if let Some(frame) = self.browser.main_frame() {
            let code = CefString::from("window.__rmCloseMenu&&window.__rmCloseMenu()");
            frame.execute_java_script(Some(&code), None, 0);
        }
    }
}

/// Translates GPUI keyboard modifier state into CEF `event_flags`, so
/// Chromium sees Shift/Ctrl/Alt while selecting or clicking.
pub fn modifier_flags(shift: bool, control: bool, alt: bool) -> u32 {
    let mut flags = 0;
    if shift {
        flags |= EVENTFLAG_SHIFT_DOWN;
    }
    if control {
        flags |= EVENTFLAG_CONTROL_DOWN;
    }
    if alt {
        flags |= EVENTFLAG_ALT_DOWN;
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_percent_encodes_the_document() {
        let url = data_url("<html><head></head><body>a b&c</body></html>");
        assert!(url.starts_with("data:text/html;charset=utf-8,"));
        // Spaces and ampersands must be escaped so the URL parser keeps the body.
        assert!(url.contains("%20"));
        assert!(url.contains("%26"));
        assert!(!url.contains("a b&c"));
    }

    #[test]
    fn compose_document_injects_shim_first_and_content_last() {
        let doc = compose_document("<html><head><title>x</title></head><body>hi</body></html>");
        let shim = doc.find(IPC_CONSOLE_PREFIX).expect("ipc shim present");
        let content = doc
            .find("window.__rmCloseMenu")
            .expect("content script present");
        // The shim (in <head>) must precede the reader content script (before </body>).
        assert!(shim < content);
        // The shim sits inside <head>, ahead of the original title.
        assert!(shim < doc.find("<title>").unwrap());
    }

    #[test]
    fn compose_document_falls_back_without_head_or_body() {
        let doc = compose_document("<p>bare</p>");
        assert!(doc.contains(IPC_CONSOLE_PREFIX));
        assert!(doc.contains("window.__rmCloseMenu"));
        assert!(doc.contains("<p>bare</p>"));
    }

    #[test]
    fn modifier_flags_map_expected_bits() {
        assert_eq!(modifier_flags(false, false, false), 0);
        assert_eq!(modifier_flags(true, false, false), EVENTFLAG_SHIFT_DOWN);
        assert_eq!(
            modifier_flags(true, true, true),
            EVENTFLAG_SHIFT_DOWN | EVENTFLAG_CONTROL_DOWN | EVENTFLAG_ALT_DOWN
        );
    }

    #[test]
    fn store_paint_buffer_reuses_allocation_for_same_size() {
        let mut slot = None;
        let first = [1u8, 2, 3, 4, 5, 6, 7, 8];
        store_paint_buffer(&mut slot, 2, 1, &first);
        let ptr = slot.as_ref().unwrap().2.as_ptr();
        let second = [9u8, 8, 7, 6, 5, 4, 3, 2];
        store_paint_buffer(&mut slot, 2, 1, &second);
        assert_eq!(slot.as_ref().unwrap().2, second);
        assert_eq!(slot.as_ref().unwrap().2.as_ptr(), ptr);
    }

    #[test]
    fn store_paint_buffer_reallocates_when_size_changes() {
        let mut slot = None;
        store_paint_buffer(&mut slot, 1, 1, &[1, 2, 3, 4]);
        store_paint_buffer(&mut slot, 2, 1, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(slot.as_ref().unwrap().0, 2);
        assert_eq!(slot.as_ref().unwrap().2.len(), 8);
    }

    #[test]
    fn coalesce_wheel_deltas_sums_offsets() {
        // Pure helper mirroring EmailWebView / OsrBrowser wheel coalescing.
        fn coalesce(
            pending: Option<(f32, f32, f32, f32)>,
            x: f32,
            y: f32,
            dx: f32,
            dy: f32,
        ) -> (f32, f32, f32, f32) {
            match pending {
                Some((_, _, pdx, pdy)) => (x, y, pdx + dx, pdy + dy),
                None => (x, y, dx, dy),
            }
        }
        let a = coalesce(None, 10.0, 20.0, 0.0, -12.0);
        let b = coalesce(Some(a), 11.0, 21.0, 0.0, -8.0);
        assert_eq!(b, (11.0, 21.0, 0.0, -20.0));
    }

    #[test]
    fn expected_physical_size_scales_logical_view() {
        assert_eq!(expected_physical_size(800, 600, 1.0), (800, 600));
        assert_eq!(expected_physical_size(800, 600, 2.0), (1600, 1200));
        assert_eq!(expected_physical_size(100, 50, 1.5), (150, 75));
    }

    #[test]
    fn frame_matches_view_allows_one_pixel_slack() {
        assert!(frame_matches_view(800, 600, 800, 600));
        assert!(frame_matches_view(801, 600, 800, 600));
        assert!(!frame_matches_view(820, 600, 800, 600));
    }
}
