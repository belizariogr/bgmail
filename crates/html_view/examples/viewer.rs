//! Opens HTML files in a GPUI window rendered by `html_view`.
//!
//! Usage:
//!   cargo run -p html_view --example viewer -- <file.html>...
//!       Left/Right switch between files.
//!   cargo run -p html_view --example viewer -- [--width <px>] --capture <out_dir> <file.html>...
//!       Renders every file in an 800px (or `--width`) column, scrolls through it, grabs each
//!       viewport with `grim` (Wayland/Hyprland only) and stitches a full-page
//!       PNG per file into `out_dir`. Used to compare against Chromium.
//!
//! `file:` URLs next to each file are allowed so the spike corpus (with its
//! local `assets/`) renders offline.

use gpui::{
    actions, div, point, prelude::*, px, rgb, size, App, Application, AsyncApp, Bounds, Context,
    Entity, FocusHandle, Focusable, KeyBinding, WeakEntity, Window, WindowBounds, WindowOptions,
};
use html_view::{DocumentOptions, HtmlDocument, HtmlView, Resources};
use std::path::{Path, PathBuf};
use std::time::Duration;

actions!(viewer, [Next, Previous]);

/// Default width of the document column (the Chromium reference viewport).
const DEFAULT_COLUMN_WIDTH: f32 = 800.0;
static COLUMN_WIDTH: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

/// Width of the document column (`--width`, else 800px).
fn column_width() -> f32 {
    *COLUMN_WIDTH.get().unwrap_or(&DEFAULT_COLUMN_WIDTH)
}
/// Time for GPUI to present a frame and the compositor to show it.
const SETTLE: Duration = Duration::from_millis(350);

struct Viewer {
    files: Vec<PathBuf>,
    index: usize,
    view: Entity<HtmlView>,
    focus: FocusHandle,
}

fn load(path: &Path) -> HtmlDocument {
    let html = std::fs::read_to_string(path).unwrap_or_else(|e| format!("<p>{e}</p>"));
    let base_url = path
        .canonicalize()
        .ok()
        .and_then(|p| url::Url::from_file_path(p).ok())
        .map(|u| u.to_string());
    let start = std::time::Instant::now();
    let mut document = HtmlDocument::parse(
        &html,
        DocumentOptions {
            base_url,
            resources: Resources::new().allow_file_urls(true),
            ..Default::default()
        },
    );
    document.layout(column_width());
    eprintln!(
        "{}: parsed + laid out in {:?}",
        path.display(),
        start.elapsed()
    );
    document
}

impl Viewer {
    fn show(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.files.len() as isize;
        self.index = (self.index as isize + delta).rem_euclid(len) as usize;
        let document = load(&self.files[self.index]);
        self.view
            .update(cx, |view, cx| view.set_document(document, cx));
        window.set_window_title(&format!(
            "{} ({}/{})",
            self.files[self.index]
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            self.index + 1,
            self.files.len()
        ));
        cx.notify();
    }
}

impl Focusable for Viewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Viewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Next, window, cx| this.show(1, window, cx)))
            .on_action(cx.listener(|this, _: &Previous, window, cx| this.show(-1, window, cx)))
            .size_full()
            .bg(rgb(0xffffff))
            .child(
                div()
                    .w(px(column_width()))
                    .h_full()
                    .bg(rgb(0xffffff))
                    .child(self.view.clone()),
            )
    }
}

/// Window geometry (x, y, w, h) in compositor pixels, looked up by our pid.
fn window_geometry() -> Option<(i64, i64, i64, i64)> {
    let out = std::process::Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let pid = std::process::id().to_string();
    // Minimal scan of hyprctl's JSON to avoid a serde dependency in the example.
    for client in text.split("\"address\"").skip(1) {
        if !client.contains(&format!("\"pid\": {pid},")) {
            continue;
        }
        let nums = |key: &str| -> Option<(i64, i64)> {
            let start = client.find(&format!("\"{key}\": ["))? + key.len() + 5;
            let end = start + client[start..].find(']')?;
            let mut parts = client[start..end]
                .split(',')
                .map(|v| v.trim().parse::<i64>());
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        };
        let (x, y) = nums("at")?;
        let (w, h) = nums("size")?;
        return Some((x, y, w, h));
    }
    None
}

fn grab(region: (i64, i64, i64, i64), out: &Path) -> bool {
    let (x, y, w, h) = region;
    std::process::Command::new("grim")
        .args(["-g", &format!("{x},{y} {w}x{h}")])
        .arg(out)
        .status()
        .is_ok_and(|s| s.success())
}

/// Stitches viewport grabs taken at `offsets` into one full-height image.
fn stitch(parts: &[(PathBuf, u32)], width: u32, height: u32, out: &Path) -> image::ImageResult<()> {
    let mut page =
        image::RgbaImage::from_pixel(width, height.max(1), image::Rgba([255, 255, 255, 255]));
    for (path, offset) in parts {
        let part = image::open(path)?.to_rgba8();
        let visible = part.height().min(height.saturating_sub(*offset));
        let part =
            image::imageops::crop_imm(&part, 0, 0, width.min(part.width()), visible).to_image();
        image::imageops::replace(&mut page, &part, 0, *offset as i64);
    }
    page.save(out)
}

async fn capture_all(
    viewer: WeakEntity<Viewer>,
    files: Vec<PathBuf>,
    out_dir: PathBuf,
    cx: &mut AsyncApp,
) -> Option<()> {
    std::fs::create_dir_all(&out_dir).ok()?;
    let parts_dir = out_dir.join("parts");
    std::fs::create_dir_all(&parts_dir).ok()?;
    cx.background_executor()
        .timer(Duration::from_millis(800))
        .await;
    let (wx, wy, _, wh) = window_geometry()?;
    let viewport = wh as u32;
    let view = viewer.read_with(cx, |v, _| v.view.clone()).ok()?;
    for (index, file) in files.iter().enumerate() {
        let name = file.file_stem()?.to_string_lossy().to_string();
        let start = std::time::Instant::now();
        let document = load(file);
        let layout_ms = start.elapsed().as_millis();
        let content = document.content_height().ceil() as u32;
        view.update(cx, |view, cx| view.set_document(document, cx))
            .ok()?;
        let mut parts = Vec::new();
        let mut offset = 0u32;
        loop {
            let y = offset.min(content.saturating_sub(viewport));
            view.update(cx, |view, cx| {
                view.scroll_handle()
                    .set_offset(point(px(0.), px(-(y as f32))));
                cx.notify();
            })
            .ok()?;
            cx.background_executor().timer(SETTLE).await;
            let part = parts_dir.join(format!("{name}__{}.png", parts.len()));
            if !grab((wx, wy, column_width() as i64, viewport as i64), &part) {
                eprintln!("grim failed for {name}");
                return None;
            }
            parts.push((part, y));
            if y + viewport >= content {
                break;
            }
            offset += viewport;
        }
        let out = out_dir.join(format!("{name}.png"));
        if let Err(e) = stitch(
            &parts,
            column_width() as u32,
            content.max(viewport.min(600)),
            &out,
        ) {
            eprintln!("stitch failed for {name}: {e}");
        }
        println!("{name}\tok\t{layout_ms}\t0\t{content}");
        eprintln!("[{}/{}] {name}", index + 1, files.len());
    }
    cx.update(|cx| cx.quit()).ok();
    Some(())
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--width") && args.len() > 1 {
        args.remove(0);
        let width = args.remove(0).parse().expect("--width <px>");
        COLUMN_WIDTH.set(width).ok();
    }
    let capture = if args.first().map(String::as_str) == Some("--capture") && args.len() > 2 {
        args.remove(0);
        Some(PathBuf::from(args.remove(0)))
    } else {
        None
    };
    let files: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    if files.is_empty() {
        eprintln!("usage: viewer [--capture <out_dir>] <file.html>...");
        std::process::exit(2);
    }
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("right", Next, None),
            KeyBinding::new("left", Previous, None),
        ]);
        let bounds = Bounds::centered(None, size(px(column_width() + 60.), px(900.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // Lets compositor rules target the viewer (e.g. disable
                    // opacity/blur while capturing).
                    app_id: Some("html_view_viewer".into()),
                    ..Default::default()
                },
                |window, cx| {
                    let document = load(&files[0]);
                    let view = cx.new(|_| HtmlView::new(document));
                    let viewer = cx.new(|cx| Viewer {
                        files: files.clone(),
                        index: 0,
                        view,
                        focus: cx.focus_handle(),
                    });
                    window.focus(&viewer.focus_handle(cx));
                    viewer
                },
            )
            .unwrap();
        cx.activate(true);
        if let Some(out_dir) = capture.clone() {
            let files = files.clone();
            window
                .update(cx, |viewer, _, cx| {
                    cx.spawn(async move |this, cx| {
                        capture_all(this, files, out_dir, cx).await;
                    })
                    .detach();
                    let _ = viewer;
                })
                .ok();
        }
    });
}
