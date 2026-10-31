//! Prints each file's content width and height when laid out at `width`.
//! Diagnostic for comparing overflow with Chromium's `scrollWidth`.
//! Usage: cargo run -p html_view --example content_size -- <width> <file.html>...

use html_view::{DocumentOptions, HtmlDocument, Resources};

fn main() {
    let mut args = std::env::args().skip(1);
    let width: f32 = args.next().and_then(|w| w.parse().ok()).expect("width");
    for path in args {
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
        let height = doc.layout(width);
        println!("{path}\t{}\t{height}", doc.content_width());
    }
}
