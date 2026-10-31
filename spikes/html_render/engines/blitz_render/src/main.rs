//! Renders every `*.html` in a corpus dir with Blitz (CPU, vello_cpu) to PNG.
//! Usage: blitz_render <corpus_dir> <out_dir>
use anyrender::{render_to_buffer, PaintScene as _};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{util::Color, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::{kurbo::Rect, Fill};
use std::{fs, path::Path, sync::Arc, time::Instant};

const WIDTH: u32 = 800;
const MIN_HEIGHT: u32 = 600;
const MAX_HEIGHT: u32 = 12_000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (corpus, out) = (Path::new(&args[1]), Path::new(&args[2]));
    fs::create_dir_all(out).unwrap();
    let only = args.get(3);
    let mut entries: Vec<_> = fs::read_dir(corpus)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "html"))
        .filter(|p| only.map_or(true, |o| p.file_stem().unwrap().to_string_lossy() == *o))
        .collect();
    entries.sort();
    // blitz-net spawns its fetches on Tokio; a worker pool lets them complete
    // while the main thread polls the document.
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let _guard = rt.enter();
    for path in entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let result = std::panic::catch_unwind(|| render(&path, &out.join(format!("{name}.png"))));
        match result {
            Ok((layout_ms, paint_ms, h)) => println!("{name}\tok\t{layout_ms}\t{paint_ms}\t{h}"),
            Err(_) => println!("{name}\tPANIC\t0\t0\t0"),
        }
    }
}

fn render(path: &Path, out: &Path) -> (u128, u128, u32) {
    let raw = String::from_utf8_lossy(&fs::read(path).unwrap()).to_string();
    let html = force_html_parse(&raw);
    let base = format!("file://{}", fs::canonicalize(path).unwrap().display());
    let t0 = Instant::now();
    let net = Arc::new(Provider::new(None));
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(base),
            net_provider: Some(Arc::clone(&net) as _),
            viewport: Some(Viewport::new(WIDTH, MIN_HEIGHT, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    // Local file:// assets only; bounded so a stuck fetch can't hang the run.
    for _ in 0..2000 {
        doc.resolve(0.0);
        if net.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    doc.as_mut().resolve(0.0);
    let layout_ms = t0.elapsed().as_millis();
    // Like a browser's full-page capture: the root's scrollHeight, so
    // `html, body { height: 100% }` layouts aren't cut at the viewport.
    let root = doc.as_ref().root_element();
    let content = root.final_layout().size.height.max(root.scroll_height());
    let height = (content.ceil() as u32).clamp(MIN_HEIGHT, MAX_HEIGHT);
    let t1 = Instant::now();
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(Fill::NonZero, Default::default(), Color::WHITE, Default::default(),
                &Rect::new(0.0, 0.0, WIDTH as f64, height as f64));
            paint_scene(scene, doc.as_mut(), 1.0, WIDTH, height, 0, 0);
        },
        WIDTH,
        height,
    );
    let paint_ms = t1.elapsed().as_millis();
    let file = fs::File::create(out).unwrap();
    let mut enc = png::Encoder::new(file, WIDTH, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    w.write_image_data(&buffer).unwrap();
    (layout_ms, paint_ms, height)
}

/// Blitz sniffs an XHTML doctype / `<?xml` prolog and switches to its XML
/// parser, which drops the HTML user-agent stylesheet (`<p>` turns inline,
/// `display:none` previews show). Mail is always `text/html`, so browsers parse
/// it as HTML regardless; strip the prolog so Blitz does the same.
fn force_html_parse(html: &str) -> String {
    let mut rest = html.trim_start_matches('\u{feff}').trim_start();
    for prefix in ["<?xml", "<!DOCTYPE", "<!doctype"] {
        if rest.starts_with(prefix) {
            if let Some(end) = rest.find('>') {
                rest = rest[end + 1..].trim_start();
            }
        }
    }
    // A bare `<html xmlns="…/xhtml">` root is sniffed as XML too; an HTML5
    // doctype on its own first line short-circuits both heuristics.
    format!("<!DOCTYPE html>\n{rest}")
}
