//! A compact HTML renderer for the e-mail reading pane.
//!
//! Real messages arrive as HTML or plain text, so the reader needs to display
//! both. This renders a curated subset of HTML — headings, paragraphs, lists,
//! links, emphasis, inline code, block quotes, preformatted blocks, horizontal
//! rules and image placeholders — into themed GPUI elements.
//!
//! It is deliberately *not* a browser engine: there is no CSS, layout tables or
//! scripting. The goal is clean, readable rendering of typical message bodies.
//! We mirror Zed's parser choice (`html5ever` + `markup5ever_rcdom`).

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    div, img, px, AnyElement, App, FontStyle, FontWeight, Hsla, IntoElement, ParentElement,
    RenderOnce, SharedString, StrikethroughStyle, Styled, StyledText, TextRun, TextStyle,
    UnderlineStyle, Window,
};

use crate::SelectableText;
use html5ever::driver::ParseOpts;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use theme::ActiveTheme;

use crate::prelude::v_flex;

/// An HTML e-mail body rendered as themed GPUI elements.
///
/// ```ignore
/// HtmlView::new("<p>Hello <strong>world</strong></p>")
/// ```
#[derive(IntoElement)]
pub struct HtmlView {
    html: SharedString,
}

impl HtmlView {
    /// Creates a view that renders the given HTML markup.
    pub fn new(html: impl Into<SharedString>) -> Self {
        Self { html: html.into() }
    }
}

impl RenderOnce for HtmlView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_2()
            .items_start()
            .children(html_blocks(&self.html, window, cx))
    }
}

/// Parses `html` and returns its block-level elements.
///
/// The blocks are meant to be placed **directly** inside a scroll container with
/// `items_start()`: text blocks are `w_full` (so they wrap to the available
/// width), while images and `<pre>` hug their content so they can be wider than
/// the viewport and drive a horizontal scrollbar.
pub fn html_blocks(html: &str, window: &Window, cx: &App) -> Vec<AnyElement> {
    let palette = Palette::from_theme(cx);
    let base = window.text_style();
    render_html(html, &base, &palette)
}

/// Theme colors used while rendering HTML.
struct Palette {
    text: Hsla,
    muted: Hsla,
    link: Hsla,
    border: Hsla,
    code_bg: Hsla,
    selection: Hsla,
}

impl Palette {
    fn from_theme(cx: &App) -> Self {
        let c = cx.theme().colors();
        // A translucent accent reads as a selection highlight over any run.
        let mut selection = c.text_accent;
        selection.a = 0.30;
        Self {
            text: c.text,
            muted: c.text_muted,
            link: c.text_accent,
            border: c.border,
            code_bg: c.element_background,
            selection,
        }
    }
}

/// Inline styling carried down the tree as we render text runs.
#[derive(Clone, Copy, Default)]
struct Inline {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    link: bool,
    code: bool,
    muted: bool,
}

/// Parses `html` and renders it into a list of block-level elements.
fn render_html(html: &str, base: &TextStyle, palette: &Palette) -> Vec<AnyElement> {
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: true,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bytes = html.as_bytes();
    let Ok(dom) = parse_document(RcDom::default(), opts)
        .from_utf8()
        .read_from(&mut bytes)
    else {
        return vec![plain_block(html, base, palette)];
    };

    let mut renderer = Renderer {
        base,
        palette,
        blocks: Vec::new(),
        para_text: String::new(),
        para_runs: Vec::new(),
        ids: Rc::new(Cell::new(0)),
    };
    renderer.walk_children(&dom.document, Inline::default());
    renderer.flush();
    renderer.blocks
}

struct Renderer<'a> {
    base: &'a TextStyle,
    palette: &'a Palette,
    blocks: Vec<AnyElement>,
    para_text: String,
    para_runs: Vec<TextRun>,
    /// Document-wide counter giving each selectable block a stable id (stable
    /// across frames so its selection persists; shared with sub-renderers so
    /// ids stay unique).
    ids: Rc<Cell<usize>>,
}

impl Renderer<'_> {
    /// Allocates the next stable element id for a selectable text block.
    fn next_text_id(&self) -> (&'static str, usize) {
        let n = self.ids.get();
        self.ids.set(n + 1);
        ("email-text", n)
    }
}

impl Renderer<'_> {
    /// Walks the block-level children of `node`, grouping inline content into
    /// paragraphs and emitting specialized blocks for structural tags.
    fn walk_children(&mut self, node: &Handle, style: Inline) {
        for child in node.children.borrow().iter() {
            match &child.data {
                NodeData::Text { contents } => {
                    let text = collapse_ws(&contents.borrow());
                    if !text.is_empty() {
                        self.append_text(&text, style);
                    }
                }
                NodeData::Element { name, .. } => {
                    let tag = name.local.as_ref();
                    match tag {
                        // Non-visual / metadata elements.
                        "script" | "style" | "head" | "title" | "noscript" | "meta" | "link" => {}

                        // Block containers: recurse, separating paragraphs.
                        "html" | "body" | "div" | "p" | "section" | "article" | "header"
                        | "footer" | "main" | "figure" | "figcaption" | "dl" | "dd" | "dt"
                        | "nav" | "aside" | "center" | "form" | "fieldset" | "table" | "tbody"
                        | "thead" | "tfoot" => {
                            self.flush();
                            self.walk_children(child, style);
                            self.flush();
                        }

                        // A table row: cells flow inline, then the row breaks.
                        "tr" => {
                            self.flush();
                            self.walk_children(child, style);
                            self.flush();
                        }
                        // Table cells: inline content followed by a separator.
                        "td" | "th" => {
                            self.append_inline(child, style);
                            self.append_text("   ", style);
                        }

                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.heading(child, tag, style),
                        "ul" | "ol" => self.list(child, tag == "ol", style),
                        "li" => {
                            // A stray <li> outside a list: render as a plain line.
                            self.flush();
                            let (text, runs) = self.inline_of(child, style);
                            self.push_paragraph(text, runs, |el| el);
                        }
                        "blockquote" => self.blockquote(child, style),
                        "pre" => self.preformatted(child),
                        "hr" => {
                            self.flush();
                            self.blocks.push(
                                div()
                                    .my_2()
                                    .h(px(1.0))
                                    .bg(self.palette.border)
                                    .into_any_element(),
                            );
                        }
                        "img" => {
                            self.flush();
                            self.blocks.push(self.image(child));
                        }

                        // Everything else (a, b, em, span, code, br, ...) is inline.
                        _ => self.append_inline(child, style),
                    }
                }
                _ => {}
            }
        }
    }

    /// Appends an inline subtree (applying the element's own styling) to the
    /// current paragraph buffer.
    fn append_inline(&mut self, node: &Handle, style: Inline) {
        let mut text = std::mem::take(&mut self.para_text);
        let mut runs = std::mem::take(&mut self.para_runs);
        self.collect(node, style, &mut text, &mut runs);
        self.para_text = text;
        self.para_runs = runs;
    }

    /// Appends a plain string to the current paragraph buffer.
    fn append_text(&mut self, text: &str, style: Inline) {
        let mut buf = std::mem::take(&mut self.para_text);
        let mut runs = std::mem::take(&mut self.para_runs);
        self.push_run(&mut buf, &mut runs, text, style);
        self.para_text = buf;
        self.para_runs = runs;
    }

    /// Collects the inline content of `node` into `text`/`runs`, flattening any
    /// nested block structure (used for headings, list items, etc.).
    fn collect(&self, node: &Handle, style: Inline, text: &mut String, runs: &mut Vec<TextRun>) {
        match &node.data {
            NodeData::Text { contents } => {
                let collapsed = collapse_ws(&contents.borrow());
                if !collapsed.is_empty() {
                    self.push_run(text, runs, &collapsed, style);
                }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref();
                let style = match tag {
                    "script" | "style" => return,
                    "br" => {
                        self.push_run(text, runs, "\n", style);
                        return;
                    }
                    "b" | "strong" => Inline {
                        bold: true,
                        ..style
                    },
                    "i" | "em" | "cite" | "var" => Inline {
                        italic: true,
                        ..style
                    },
                    "u" | "ins" => Inline {
                        underline: true,
                        ..style
                    },
                    "s" | "strike" | "del" => Inline {
                        strike: true,
                        ..style
                    },
                    "code" | "tt" | "kbd" | "samp" => Inline {
                        code: true,
                        ..style
                    },
                    "a" => Inline {
                        link: true,
                        underline: true,
                        ..style
                    },
                    _ => style,
                };
                for child in node.children.borrow().iter() {
                    self.collect(child, style, text, runs);
                }
            }
            _ => {}
        }
    }

    /// Builds a styled text run for `s` and appends it to `text`/`runs`.
    fn push_run(&self, text: &mut String, runs: &mut Vec<TextRun>, s: &str, style: Inline) {
        let mut run = self.base.to_run(s.len());
        run.color = if style.link {
            self.palette.link
        } else if style.muted {
            self.palette.muted
        } else {
            self.palette.text
        };
        if style.bold {
            run.font.weight = FontWeight::BOLD;
        }
        if style.italic {
            run.font.style = FontStyle::Italic;
        }
        if style.code {
            run.background_color = Some(self.palette.code_bg);
        }
        if style.link || style.underline {
            run.underline = Some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(run.color),
                wavy: false,
            });
        }
        if style.strike {
            run.strikethrough = Some(StrikethroughStyle {
                thickness: px(1.0),
                color: None,
            });
        }
        text.push_str(s);
        runs.push(run);
    }

    /// Collects the inline content of a node into a fresh buffer.
    fn inline_of(&self, node: &Handle, style: Inline) -> (String, Vec<TextRun>) {
        let mut text = String::new();
        let mut runs = Vec::new();
        for child in node.children.borrow().iter() {
            self.collect(child, style, &mut text, &mut runs);
        }
        (text, runs)
    }

    /// Emits the buffered paragraph (if it has visible content) and clears it.
    fn flush(&mut self) {
        if self.para_text.trim().is_empty() {
            self.para_text.clear();
            self.para_runs.clear();
            return;
        }
        let text = std::mem::take(&mut self.para_text);
        let runs = std::mem::take(&mut self.para_runs);
        self.push_paragraph(text, runs, |el| el);
    }

    /// Pushes a paragraph block built from `text`/`runs`, allowing the caller to
    /// customize the wrapping element (size, weight, indentation, ...).
    fn push_paragraph(
        &mut self,
        text: String,
        runs: Vec<TextRun>,
        decorate: impl FnOnce(gpui::Div) -> gpui::Div,
    ) {
        if text.trim().is_empty() {
            return;
        }
        let selectable = SelectableText::new(
            self.next_text_id(),
            SharedString::from(text),
            runs,
            self.palette.selection,
        );
        // Full width so the text wraps to the container (paragraphs are the
        // wrapping blocks; wide atoms like images/<pre> hug instead).
        self.blocks.push(
            decorate(div().w_full())
                .child(selectable)
                .into_any_element(),
        );
    }

    fn heading(&mut self, node: &Handle, tag: &str, style: Inline) {
        self.flush();
        let (text, runs) = self.inline_of(
            node,
            Inline {
                bold: true,
                ..style
            },
        );
        let size = match tag {
            "h1" => px(24.0),
            "h2" => px(20.0),
            "h3" => px(18.0),
            "h4" => px(16.0),
            "h5" => px(15.0),
            _ => px(14.0),
        };
        self.push_paragraph(text, runs, move |el| {
            el.mt_2().text_size(size).font_weight(FontWeight::BOLD)
        });
    }

    fn list(&mut self, node: &Handle, ordered: bool, style: Inline) {
        self.flush();
        let mut index = 1usize;
        for child in node.children.borrow().iter() {
            let NodeData::Element { name, .. } = &child.data else {
                continue;
            };
            if name.local.as_ref() != "li" {
                continue;
            }
            let marker = if ordered {
                format!("{index}.  ")
            } else {
                "•   ".to_string()
            };
            index += 1;

            let mut text = String::new();
            let mut runs = Vec::new();
            self.push_run(&mut text, &mut runs, &marker, style);
            let (item_text, item_runs) = self.inline_of(child, style);
            text.push_str(&item_text);
            runs.extend(item_runs);

            self.push_paragraph(text, runs, |el| el.pl_4());
        }
    }

    fn blockquote(&mut self, node: &Handle, style: Inline) {
        self.flush();
        let mut sub = Renderer {
            base: self.base,
            palette: self.palette,
            blocks: Vec::new(),
            para_text: String::new(),
            para_runs: Vec::new(),
            ids: self.ids.clone(),
        };
        sub.walk_children(
            node,
            Inline {
                muted: true,
                ..style
            },
        );
        sub.flush();
        let inner = sub.blocks;
        let border = self.palette.border;
        self.blocks.push(
            div()
                .w_full()
                .pl_3()
                .border_l_2()
                .border_color(border)
                .child(v_flex().gap_2().items_start().children(inner))
                .into_any_element(),
        );
    }

    fn preformatted(&mut self, node: &Handle) {
        self.flush();
        let mut raw = String::new();
        raw_text(node, &mut raw);
        let raw = raw.trim_matches('\n').to_string();
        if raw.is_empty() {
            return;
        }
        let mut run = self.base.to_run(raw.len());
        run.color = self.palette.text;
        let selectable =
            SelectableText::new(self.next_text_id(), raw, vec![run], self.palette.selection);
        // Preformatted text keeps its own line breaks and does not wrap; long
        // lines overflow horizontally (reachable via the horizontal scrollbar).
        self.blocks.push(
            div()
                .my_1()
                .p_2()
                .rounded_md()
                .bg(self.palette.code_bg)
                .whitespace_nowrap()
                .child(selectable)
                .into_any_element(),
        );
    }

    /// Renders an `<img>`: a real image when the source is a local file, or a
    /// labelled placeholder for remote sources (we don't fetch over the network
    /// in the mock).
    fn image(&self, node: &Handle) -> AnyElement {
        if let Some(path) = element_attr(node, "src")
            .as_deref()
            .and_then(local_image_path)
        {
            let image = img(path).rounded_md();
            // Honor an explicit `width` (common in real e-mails); otherwise use
            // the image's natural size (capped). Height stays auto so GPUI keeps
            // the aspect ratio. Images wider than the pane overflow horizontally
            // and are reachable via the horizontal scrollbar.
            let image = match image_width(node) {
                Some(width) => image.w(px(width)),
                None => image.max_w(px(900.0)),
            };
            return image.into_any_element();
        }

        let alt = element_attr(node, "alt").filter(|a| !a.is_empty());
        let label = match alt {
            Some(alt) => format!("[image: {alt}]"),
            None => "[image]".to_string(),
        };
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(self.palette.border)
            .text_color(self.palette.muted)
            .child(SharedString::from(label))
            .into_any_element()
    }
}

/// Reads an `<img>`'s explicit pixel width, if present (e.g. `width="700"` or
/// `width="700px"`). Percentages and other units are ignored.
fn image_width(node: &Handle) -> Option<f32> {
    let raw = element_attr(node, "width")?;
    let raw = raw.trim().strip_suffix("px").unwrap_or(raw.trim());
    raw.parse::<f32>().ok().filter(|w| *w > 0.0)
}

/// Resolves an `<img src>` to a local filesystem path, if it points to one.
/// Remote URLs return `None` so they fall back to a placeholder.
fn local_image_path(src: &str) -> Option<PathBuf> {
    if let Some(rest) = src.strip_prefix("file://") {
        return Some(PathBuf::from(rest));
    }
    if src.starts_with('/') {
        return Some(PathBuf::from(src));
    }
    None
}

/// Builds a single plain-text block (used as a fallback and for text bodies).
fn plain_block(text: &str, base: &TextStyle, palette: &Palette) -> AnyElement {
    let mut run = base.to_run(text.len());
    run.color = palette.text;
    let styled = StyledText::new(SharedString::from(text.to_string())).with_runs(vec![run]);
    div().w_full().child(styled).into_any_element()
}

/// Recursively concatenates the raw text of a node, preserving whitespace.
fn raw_text(node: &Handle, out: &mut String) {
    match &node.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow()),
        _ => {
            for child in node.children.borrow().iter() {
                raw_text(child, out);
            }
        }
    }
}

/// Reads an attribute value off an element node.
fn element_attr(node: &Handle, name: &str) -> Option<String> {
    let NodeData::Element { attrs, .. } = &node.data else {
        return None;
    };
    attrs
        .borrow()
        .iter()
        .find(|attr| attr.name.local.as_ref() == name)
        .map(|attr| attr.value.to_string())
}

/// Collapses runs of HTML whitespace into single spaces (preserving a single
/// leading/trailing space so inline elements stay separated).
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        let theme = theme::Theme::dark();
        let c = theme.colors();
        Palette {
            text: c.text,
            muted: c.text_muted,
            link: c.text_accent,
            border: c.border,
            code_bg: c.element_background,
            selection: c.text_accent,
        }
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(collapse_ws("  a\n\t b  "), " a b ");
        assert_eq!(collapse_ws("x"), "x");
    }

    #[test]
    fn renders_paragraphs_as_blocks() {
        let base = TextStyle::default();
        let blocks = render_html("<p>One</p><p>Two</p>", &base, &palette());
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn lists_produce_one_block_per_item() {
        let base = TextStyle::default();
        let blocks = render_html("<ul><li>a</li><li>b</li><li>c</li></ul>", &base, &palette());
        assert_eq!(blocks.len(), 3);
    }

    #[test]
    fn inline_runs_cover_the_whole_text() {
        // A paragraph mixing styles must produce runs whose byte lengths sum to
        // the text length, otherwise StyledText would panic at paint time.
        let base = TextStyle::default();
        let p = palette();
        let r = Renderer {
            base: &base,
            palette: &p,
            blocks: Vec::new(),
            para_text: String::new(),
            para_runs: Vec::new(),
            ids: Rc::new(Cell::new(0)),
        };
        let (text, runs) = {
            let opts = ParseOpts::default();
            let mut bytes = "Hello <strong>bold</strong> and <a href=\"#\">link</a>".as_bytes();
            let dom = parse_document(RcDom::default(), opts)
                .from_utf8()
                .read_from(&mut bytes)
                .unwrap();
            r.inline_of(&dom.document, Inline::default())
        };
        let runs_len: usize = runs.iter().map(|run| run.len).sum();
        assert_eq!(runs_len, text.len());
        assert!(text.contains("bold"));
        assert!(text.contains("link"));
    }

    #[test]
    fn local_image_paths_are_detected() {
        assert_eq!(
            local_image_path("/tmp/a.png"),
            Some(PathBuf::from("/tmp/a.png"))
        );
        assert_eq!(
            local_image_path("file:///tmp/b.png"),
            Some(PathBuf::from("/tmp/b.png"))
        );
        assert_eq!(local_image_path("https://example.com/c.png"), None);
    }

    #[test]
    fn malformed_html_falls_back_to_text() {
        let base = TextStyle::default();
        let blocks = render_html("just text, no tags", &base, &palette());
        assert_eq!(blocks.len(), 1);
    }

    /// Finds the first `<img>` element in a parsed document.
    fn find_img(node: &Handle) -> Option<Handle> {
        if let NodeData::Element { name, .. } = &node.data {
            if name.local.as_ref() == "img" {
                return Some(node.clone());
            }
        }
        node.children.borrow().iter().find_map(find_img)
    }

    fn parse(html: &str) -> RcDom {
        parse_document(RcDom::default(), ParseOpts::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap()
    }

    #[test]
    fn explicit_image_width_is_parsed() {
        let dom = parse("<img src=\"/a.png\" width=\"700\">");
        let img = find_img(&dom.document).expect("img element");
        assert_eq!(image_width(&img), Some(700.0));

        let dom = parse("<img src=\"/a.png\" width=\"640px\">");
        let img = find_img(&dom.document).expect("img element");
        assert_eq!(image_width(&img), Some(640.0));
    }

    #[test]
    fn image_without_width_has_none() {
        let dom = parse("<img src=\"/a.png\">");
        let img = find_img(&dom.document).expect("img element");
        assert_eq!(image_width(&img), None);

        // Percentage widths are not absolute pixels, so they're ignored.
        let dom = parse("<img src=\"/a.png\" width=\"100%\">");
        let img = find_img(&dom.document).expect("img element");
        assert_eq!(image_width(&img), None);
    }

    #[test]
    fn local_image_produces_one_block() {
        // A local image renders as a single (image) block, not a placeholder.
        let base = TextStyle::default();
        let blocks = render_html(
            "<p><img src=\"/tmp/x.png\" width=\"700\"></p>",
            &base,
            &palette(),
        );
        assert_eq!(blocks.len(), 1);
    }
}
