//! Prints the laid-out height of `#t`/`#p` and the faces used, per HTML file.
//! Diagnostic for comparing Blitz metrics with Chromium.
//! Usage: cargo run -p html_view --example measure -- <file.html>...

use html_view::display_list::Command;
use html_view::{DocumentOptions, HtmlDocument};

fn main() {
    for path in std::env::args().skip(1) {
        let html = std::fs::read_to_string(&path).unwrap();
        let mut doc = HtmlDocument::parse(&html, DocumentOptions::default());
        doc.layout(800.0);
        let height = |id: &str| {
            doc.blitz()
                .get_element_by_id(id)
                .and_then(|n| doc.blitz().get_node(n))
                .map(|n| n.final_layout().size.height)
                .unwrap_or(-1.0)
        };
        let (t, p) = (height("t"), height("p"));
        let mut faces: Vec<String> = doc
            .display_list()
            .commands
            .iter()
            .filter_map(|c| match c {
                Command::Text(run) => Some(format!("{}@{}", run.face.family, run.face.weight)),
                _ => None,
            })
            .collect();
        faces.dedup();
        println!("{path}\t{t:.1}\t{p:.1}\t{}", faces.join("|"));
    }
}
