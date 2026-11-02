//! Dumps every element's border box (document order) as TSV:
//! `tag  x  y  w  h`. Diagnostic for diffing Blitz layout against Chromium.
//! Usage: cargo run -p html_view --example layout_dump -- <file.html> [width]
use html_view::{DocumentOptions, HtmlDocument, Resources};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("file");
    let width: f32 = args.next().and_then(|w| w.parse().ok()).unwrap_or(800.0);
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
    doc.layout(width);
    let blitz = doc.blitz();
    let Some(body) = blitz.find_body_node().map(|n| n.id) else {
        return;
    };
    let mut stack = vec![body];
    while let Some(id) = stack.pop() {
        let node = blitz.get_node(id).unwrap();
        if let Some(el) = node.element_data() {
            let tag = el.name.local.to_string();
            if !matches!(
                tag.as_str(),
                "script" | "style" | "head" | "title" | "meta" | "link"
            ) {
                let p = node.absolute_position(0.0, 0.0);
                let size = node.final_layout().size;
                println!(
                    "{tag}\t{:.0}\t{:.0}\t{:.0}\t{:.0}",
                    p.x, p.y, size.width, size.height
                );
            }
        }
        for &child in node.children.iter().rev() {
            stack.push(child);
        }
    }
}
