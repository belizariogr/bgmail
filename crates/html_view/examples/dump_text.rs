//! Prints every text run (face, size, words) of an HTML file. Diagnostic.
//! Usage: cargo run -p html_view --example dump_text -- <file.html> [filter]
use html_view::display_list::Command;
use html_view::{DocumentOptions, HtmlDocument, Resources};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("file");
    let filter = args.next();
    let html = std::fs::read_to_string(&path).unwrap();
    let mut doc = HtmlDocument::parse(
        &html,
        DocumentOptions {
            resources: Resources::new().allow_file_urls(true),
            ..Default::default()
        },
    );
    doc.layout(800.0);
    for command in &doc.display_list().commands {
        if let Command::Text(run) = command {
            let words: Vec<String> = run
                .words
                .iter()
                .map(|w| format!("{}@{:.1}/{:.1}", w.text, w.x, w.width))
                .collect();
            let line = format!(
                "{} {} it={} {:.1}px base={:.1}: {}",
                run.face.family,
                run.face.weight,
                run.face.italic,
                run.font_size,
                run.baseline,
                words.join(" ")
            );
            if filter.as_ref().is_none_or(|f| line.contains(f.as_str())) {
                println!("{line}");
            }
        }
    }
}
