//! Renders every `*.html` in a corpus dir with litehtml (CPU, tiny-skia +
//! cosmic-text pixbuf container) to PNG.
//! Usage: litehtml_render <corpus_dir> <out_dir> [only_stem]
use litehtml::pixbuf::PixbufContainer;
use litehtml::{Document, DrawContext, Position};
use std::{fs, path::Path, time::Instant};

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
    for path in entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let result = std::panic::catch_unwind(|| render(corpus, &path, &out.join(format!("{name}.png"))));
        match result {
            Ok((layout_ms, paint_ms, h)) => println!("{name}\tok\t{layout_ms}\t{paint_ms}\t{h}"),
            Err(_) => println!("{name}\tPANIC\t0\t0\t0"),
        }
    }
}

/// Loads every image litehtml asked for from the corpus dir; returns whether any were new.
fn feed_images(container: &mut PixbufContainer, corpus: &Path) -> bool {
    let pending = container.take_pending_images();
    for (src, _) in &pending {
        if let Ok(bytes) = fs::read(corpus.join(src)) {
            container.load_image_data(src, &bytes);
        }
    }
    !pending.is_empty()
}

fn render(corpus: &Path, path: &Path, out: &Path) -> (u128, u128, u32) {
    let raw = fs::read(path).unwrap();
    let t0 = Instant::now();
    // The crate's own pipeline: decode charset, map legacy attrs, strip active content.
    let html = litehtml::html::prepare_html(&raw, None, None).html;
    let mut container = PixbufContainer::new(WIDTH, MIN_HEIGHT);
    // Pass 1 discovers images, pass 2 lays out with their intrinsic sizes.
    let mut height = MIN_HEIGHT;
    for _ in 0..2 {
        if let Ok(mut doc) = Document::from_html(&html, &mut container, None, None) {
            let content_w = doc.render(WIDTH as f32);
            if std::env::var_os("LH_WIDTH").is_some() {
                eprintln!("content_width\t{content_w}");
            }
            height = (doc.height().ceil() as u32).clamp(MIN_HEIGHT, MAX_HEIGHT);
        }
        if !feed_images(&mut container, corpus) {
            break;
        }
    }
    let layout_ms = t0.elapsed().as_millis();
    let t1 = Instant::now();
    container.resize(WIDTH, height);
    if let Ok(mut doc) = Document::from_html(&html, &mut container, None, None) {
        let _ = doc.render(WIDTH as f32);
        doc.draw(DrawContext::default(), 0.0, 0.0,
            Some(Position { x: 0.0, y: 0.0, width: WIDTH as f32, height: height as f32 }));
    }
    let paint_ms = t1.elapsed().as_millis();
    // Premultiplied RGBA over white -> opaque RGBA.
    let mut rgba = container.pixels().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        let inv = 255 - px[3] as u32;
        for c in &mut px[..3] {
            *c = (*c as u32 + inv).min(255) as u8;
        }
        px[3] = 255;
    }
    let file = fs::File::create(out).unwrap();
    let mut enc = png::Encoder::new(file, WIDTH, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    (layout_ms, paint_ms, height)
}
