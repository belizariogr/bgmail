//! Prints a compact summary of the display list (layers, fills, strokes).
//! Diagnostic. Usage: cargo run -p html_view --example dump_cmds -- <file.html>
use html_view::display_list::{Brush, Command, Geometry};
use html_view::{DocumentOptions, HtmlDocument, Resources};

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let html = std::fs::read_to_string(&path).unwrap();
    let base_url = std::fs::canonicalize(&path)
        .ok()
        .and_then(|p| url::Url::from_file_path(p).ok())
        .map(|u| u.to_string());
    let mut doc = HtmlDocument::parse(
        &html,
        DocumentOptions {
            base_url,
            resources: Resources::new().allow_file_urls(true),
            ..Default::default()
        },
    );
    doc.layout(800.0);
    let mut depth = 0usize;
    for c in &doc.display_list().commands {
        let pad = "  ".repeat(depth);
        match c {
            Command::PushLayer {
                clip, radii, alpha, ..
            } => {
                println!(
                    "{pad}layer {:.0},{:.0} {:.0}x{:.0} radii={:?} a={alpha}",
                    clip.x0,
                    clip.y0,
                    clip.width(),
                    clip.height(),
                    radii.map(|r| (r.top_left, r.top_right, r.bottom_right, r.bottom_left))
                );
                depth += 1;
            }
            Command::Pop => depth = depth.saturating_sub(1),
            Command::Fill { geometry, brush } => {
                let g = match geometry {
                    Geometry::Rect(r) => format!(
                        "rect {:.0},{:.0} {:.0}x{:.0}",
                        r.x0,
                        r.y0,
                        r.width(),
                        r.height()
                    ),
                    Geometry::RoundedRect(r, k) => format!(
                        "rrect {:.0},{:.0} {:.0}x{:.0} r=({:.1},{:.1},{:.1},{:.1})",
                        r.x0,
                        r.y0,
                        r.width(),
                        r.height(),
                        k.top_left,
                        k.top_right,
                        k.bottom_right,
                        k.bottom_left
                    ),
                    Geometry::Path(p) => {
                        let b = kurbo::Shape::bounding_box(p);
                        format!(
                            "path {:.0},{:.0} {:.0}x{:.0} ({} els)",
                            b.x0,
                            b.y0,
                            b.width(),
                            b.height(),
                            p.elements().len()
                        )
                    }
                };
                if let (Geometry::Path(p), Ok(_)) = (geometry, std::env::var("DUMP_PATHS")) {
                    println!("{pad}  {:?}", p.elements());
                }
                let b = match brush {
                    Brush::Solid(c) => &format!("solid{:?}", c.to_rgba8().to_u8_array()),
                    Brush::Gradient { .. } => "GRADIENT",
                    Brush::Image { .. } => "IMAGE",
                };
                println!("{pad}fill {b} {g}");
            }
            _ => {}
        }
    }
}
