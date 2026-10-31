//! A parsed, styled and laid-out HTML document, independent of any window.

use crate::display_list::DisplayList;
use crate::interaction::{
    find_all, word_bounds, HitInfo, LinkCapture, PointerButton, PointerInput, PointerPhase,
};
use crate::resources::{ResourceProvider, Resources};
use blitz_dom::UserAgentStylesheet;
use blitz_dom::{local_name, BaseDocument, DocumentConfig, EventDriver, NoopEventHandler};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords,
    PointerDetails, UiEvent,
};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::ops::Range;
use std::sync::Arc;

/// Height of the layout viewport. Mail is laid out in a scrolling column, so
/// this only feeds `vh` units and `height: 100%` roots; content height is
/// measured separately.
const VIEWPORT_HEIGHT: u32 = 800;

/// Default base URL. It must be hierarchical: Blitz panics resolving relative
/// references against `about:blank`. Its scheme is never fetched (see
/// [`Resources::resolve`]), so relative `src`s simply don't load.
pub const MESSAGE_BASE_URL: &str = "bgmail://message/";

/// Rendering mode browsers pick from the doctype (HTML "quirks mode").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentMode {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// Rendering mode for `html`, from its doctype (simplified HTML §13.2.6.4.1),
/// except that a missing doctype means standards mode (see below). An XHTML 1.0
/// Transitional doctype (limited quirks) is common in mail and changes line
/// heights around images.
pub fn document_mode(html: &str) -> DocumentMode {
    let mut rest = html.trim_start_matches('\u{feff}').trim_start();
    // Skip an XML prolog and leading comments.
    loop {
        if rest.starts_with("<?") {
            rest = rest.find("?>").map_or("", |i| rest[i + 2..].trim_start());
        } else if rest.starts_with("<!--") {
            rest = rest.find("-->").map_or("", |i| rest[i + 3..].trim_start());
        } else {
            break;
        }
    }
    let lower = rest
        .get(..9)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if lower != "<!doctype" {
        // No doctype: a standalone browser would use quirks mode, but mail is
        // shown inside webmail pages that are in standards mode (Gmail, Outlook
        // on the web), and fragments (`<div dir="ltr">…`) were written in such
        // editors. Treat doctype-less mail — full documents or fragments — as
        // standards; only an explicit legacy doctype opts into quirks.
        return DocumentMode::NoQuirks;
    }
    let doctype = rest[9..rest.find('>').unwrap_or(rest.len())].trim();
    let upper = doctype.to_ascii_uppercase();
    let mut parts = upper.split_whitespace();
    if parts.next() != Some("HTML") {
        return DocumentMode::Quirks;
    }
    let quoted: Vec<&str> = upper.split('"').skip(1).step_by(2).collect();
    let public_id = upper
        .contains("PUBLIC")
        .then(|| quoted.first().copied())
        .flatten();
    let has_system_id = if public_id.is_some() {
        quoted.len() > 1
    } else {
        upper.contains("SYSTEM")
    };
    let Some(public_id) = public_id else {
        return DocumentMode::NoQuirks;
    };
    const QUIRKS_PREFIXES: [&str; 6] = [
        "-//W3C//DTD HTML 4.0 TRANSITIONAL//",
        "-//W3C//DTD HTML 4.0 FRAMESET//",
        "-//W3C//DTD HTML 3",
        "-//W3C//DTD HTML 2",
        "-//IETF//DTD HTML",
        "-//W3O//DTD W3 HTML",
    ];
    if QUIRKS_PREFIXES.iter().any(|p| public_id.starts_with(p)) {
        return DocumentMode::Quirks;
    }
    if public_id.starts_with("-//W3C//DTD XHTML 1.0 TRANSITIONAL//")
        || public_id.starts_with("-//W3C//DTD XHTML 1.0 FRAMESET//")
    {
        return DocumentMode::LimitedQuirks;
    }
    if public_id.starts_with("-//W3C//DTD HTML 4.01 TRANSITIONAL//")
        || public_id.starts_with("-//W3C//DTD HTML 4.01 FRAMESET//")
    {
        return if has_system_id {
            DocumentMode::LimitedQuirks
        } else {
            DocumentMode::Quirks
        };
    }
    DocumentMode::NoQuirks
}

/// Light or dark rendering, forwarded to `prefers-color-scheme`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
}

/// Deepest element nesting [`HtmlDocument::try_parse`] accepts. Style and
/// layout recurse per level; around 2,000 levels exhaust an 8 MB stack, so
/// hostile mail could crash the app. Real mail stays well under 100.
pub const MAX_NESTING_DEPTH: usize = 256;

/// Why a document can't be rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// Elements nest deeper than [`MAX_NESTING_DEPTH`].
    TooDeep { depth: usize },
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooDeep { depth } => write!(
                f,
                "elements nest {depth} levels deep (limit {MAX_NESTING_DEPTH})"
            ),
        }
    }
}

impl std::error::Error for RenderError {}

/// Thin frame and icon slot browsers draw for unavailable images.
const IMAGE_FRAME: &str = "border:1px solid silver;box-sizing:border-box;";
const IMAGE_ICON: &str = "display:inline-block;width:16px;height:16px;\
     vertical-align:text-bottom;border:1px solid silver;box-sizing:border-box;";

/// Declarations of an inline `style`, lower-cased with spaces removed, for
/// the few checks below (`display:block`, explicit pixel sizes).
fn compact_style(style: &str) -> String {
    style
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// A `px` length declared for `property` in a compacted inline style.
fn style_px(style: &str, property: &str) -> Option<f32> {
    style.split(';').find_map(|declaration| {
        let value = declaration.strip_prefix(property)?.strip_prefix(':')?;
        value
            .strip_suffix("px")?
            .parse()
            .ok()
            .filter(|px: &f32| *px > 0.0)
    })
}

/// Shows unavailable images (blocked remote, missing `cid:`) the way
/// Chromium does, so mail laid out around images keeps its shape:
/// - with alt text: the alt text after a 16px icon slot, inline (declared
///   sizes ignored), or as a block keeping the width and the attributes'
///   aspect ratio when the image is `display:block`;
/// - without alt text (or `alt=""`): a framed box of the declared size, or
///   just the 16px icon slot when a dimension is missing (not for `alt=""`).
///
/// In quirks mode (`quirks`) any declared size wins, alt text or not: a
/// framed box of that size (square when only one dimension is declared)
/// with the alt text clipped inside.
fn replace_unavailable_images(
    doc: &mut BaseDocument,
    resources: &Resources,
    base: &url::Url,
    quirks: bool,
) {
    enum Fallback {
        /// Replace the image with a span holding the icon slot and alt text.
        AltText {
            alt: String,
            style: String,
            attrs: Vec<blitz_dom::Attribute>,
        },
        /// Keep the image, restyled.
        Restyle(String),
    }
    let mut fallbacks = Vec::new();
    let mut stack = vec![doc.root_node().id];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        stack.extend(node.children.iter().copied());
        let Some(element) = node.element_data() else {
            continue;
        };
        if element.name.local != local_name!("img") {
            continue;
        }
        let available = element
            .attr(local_name!("src"))
            .and_then(|src| base.join(src.trim()).ok())
            .is_some_and(|url| resources.is_available(&url));
        if available {
            continue;
        }
        let attr_px = |name| {
            element
                .attr(name)
                .and_then(|value| value.trim().trim_end_matches("px").parse::<f32>().ok())
                .filter(|px| *px > 0.0)
        };
        let own = element.attr(local_name!("style")).unwrap_or("");
        let compact = compact_style(own);
        let width = style_px(&compact, "width").or(attr_px(local_name!("width")));
        let height = style_px(&compact, "height").or(attr_px(local_name!("height")));
        let alt = element.attr(local_name!("alt")).map(str::trim);
        // Quirks mode keeps the declared size (square from one dimension).
        let (width, height) = match (quirks, width, height) {
            (true, Some(w), None) => (Some(w), Some(w)),
            (true, None, Some(h)) => (Some(h), Some(h)),
            _ => (width, height),
        };
        let quirks_box = quirks && width.is_some();
        let fallback = match alt {
            Some(alt) if !alt.is_empty() => {
                let attrs = ["class", "id", "title", "align"]
                    .into_iter()
                    .filter_map(|name| {
                        element
                            .attrs()
                            .iter()
                            .find(|a| &*a.name.local == name)
                            .cloned()
                    })
                    .collect();
                // The image's `align` floats (or aligns) the alt text too,
                // and its `hspace`/`vspace` (or quirks-mode float spacing)
                // still space it; they go before the author style it loses to.
                let align_attr = element
                    .attr(local_name!("align"))
                    .map(|a| a.trim().to_ascii_lowercase());
                let align = align_attr
                    .as_deref()
                    .and_then(blitz_dom::legacy_image_align_css)
                    .unwrap_or("");
                let mut spacing = match (quirks, align_attr.as_deref()) {
                    (true, Some("left")) => "margin-right:3px;".to_string(),
                    (true, Some("right")) => "margin-left:3px;".to_string(),
                    _ => String::new(),
                };
                if let Some(px) = attr_px(local_name!("hspace")) {
                    spacing.push_str(&format!("margin-left:{px}px;margin-right:{px}px;"));
                }
                if let Some(px) = attr_px(local_name!("vspace")) {
                    spacing.push_str(&format!("margin-top:{px}px;margin-bottom:{px}px;"));
                }
                let own = format!("{spacing}{own}");
                // A floated image is block-level: it keeps its box like
                // `display:block` does.
                let block = compact.contains("display:block")
                    || compact.contains("float:left")
                    || compact.contains("float:right")
                    || matches!(align_attr.as_deref(), Some("left" | "right"));
                let style = if quirks_box {
                    let display = if compact.contains("display:block") {
                        "block"
                    } else {
                        "inline-block"
                    };
                    format!(
                        "{own};display:{display};overflow:hidden;width:{}px;height:{}px;\
                         vertical-align:bottom;{IMAGE_FRAME}{align}",
                        width.unwrap_or(16.0),
                        height.unwrap_or(16.0)
                    )
                } else if block {
                    // Keep the block's width; its height follows the
                    // attributes' ratio when only the width is fixed.
                    let ratio = match (
                        attr_px(local_name!("width")),
                        attr_px(local_name!("height")),
                    ) {
                        (Some(w), Some(h)) if style_px(&compact, "height").is_none() => {
                            format!("aspect-ratio:{w}/{h};")
                        }
                        _ => String::new(),
                    };
                    let width = width.map_or(String::new(), |w| format!("width:{w}px;"));
                    format!("{width}{ratio}{own};display:block;overflow:hidden;{align}")
                } else {
                    format!("{spacing}{align}")
                };
                Fallback::AltText {
                    alt: alt.to_string(),
                    style,
                    attrs,
                }
            }
            _ => match (width, height) {
                // Too small for the frame's borders: just keep the size.
                (Some(w), Some(h)) if w < 2.0 || h < 2.0 => Fallback::Restyle(format!(
                    "{own};width:{w}px;height:{h}px;box-sizing:border-box"
                )),
                (Some(w), Some(h)) => {
                    Fallback::Restyle(format!("{own};width:{w}px;height:{h}px;{IMAGE_FRAME}"))
                }
                _ if alt.is_none() => {
                    Fallback::Restyle(format!("{own};width:16px;height:16px;{IMAGE_FRAME}"))
                }
                _ => continue,
            },
        };
        fallbacks.push((id, fallback));
    }
    let mut mutator = doc.mutate();
    for (img, fallback) in fallbacks {
        match fallback {
            Fallback::Restyle(style) => {
                mutator.set_attribute(img, blitz_dom::qual_name!("style"), &style);
            }
            Fallback::AltText {
                alt,
                style,
                mut attrs,
            } => {
                if !style.is_empty() {
                    attrs.push(blitz_dom::Attribute {
                        name: blitz_dom::qual_name!("style"),
                        value: style,
                    });
                }
                let span = mutator.create_element(blitz_dom::qual_name!("span", html), attrs);
                let icon = mutator.create_element(
                    blitz_dom::qual_name!("span", html),
                    vec![blitz_dom::Attribute {
                        name: blitz_dom::qual_name!("style"),
                        value: IMAGE_ICON.to_string(),
                    }],
                );
                let text = mutator.create_text_node(&alt);
                mutator.append_children(span, &[icon, text]);
                mutator.replace_node_with(img, &[span]);
            }
        }
    }
}

/// Deepest node nesting under `root` (iterative: the tree may be too deep
/// for recursion, which is what this guards against).
fn nesting_depth(doc: &BaseDocument, root: NodeId) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(root, 1)];
    while let Some((id, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        if let Some(node) = doc.get_node(id) {
            stack.extend(node.children.iter().map(|&child| (child, depth + 1)));
        }
    }
    deepest
}

/// Chromium's form control metrics (its UA sheet and theme), which the
/// controls of old newsletters' search boxes size table columns by: a
/// 13.33px UI font, text inputs with 3px of border + padding around their
/// text, closed drop-downs with room for the arrow at their end, 13px check
/// boxes and radio buttons.
const FORM_CONTROLS_UA_CSS: &str = "input, select, textarea, button { \
     font: 13.333px sans-serif; letter-spacing: normal; word-spacing: normal; \
     text-transform: none; text-indent: 0; text-shadow: none; } \
     input:not([type]), input[type=text i], input[type=password i], input[type=email i], \
     input[type=search i], input[type=tel i], input[type=url i] { \
     border: 1px solid #767676; border-radius: 2px; padding: 2px 3px; \
     background-color: white; color: black; } \
     select { display: inline-block; border: 1px solid #767676; border-radius: 2px; \
     padding: 1px 16px 1px 4px; box-sizing: border-box; \
     background-color: white; color: black; white-space: pre; } \
     input[type=checkbox i], input[type=radio i] { box-sizing: border-box; \
     width: 13px; height: 13px; padding: 0; margin: 3px 3px 3px 4px; } \
     input[type=radio i] { margin: 3px 3px 0 5px; }";

/// Chromium's quirks-mode UA rules that mail visibly depends on:
/// - tables don't inherit font and alignment (an old page's small body font
///   doesn't shrink its table headlines);
/// - floated images get 3px of space toward the text, forms a bottom margin,
///   text inputs border-box sizing (quirks.css);
/// - the default ("quirky") margins at the start of the body or a table
///   cell and at the end of a cell are dropped (see
///   [`mark_quirky_margins`]); author margins win over it.
const QUIRKS_UA_CSS: &str = "table { white-space: normal; line-height: normal; \
     font-weight: normal; font-size: medium; font-variant: normal; font-style: normal; \
     text-align: start; } \
     img[align=left i] { margin-right: 3px; } img[align=right i] { margin-left: 3px; } \
     input:not([type=image i]), textarea { box-sizing: border-box; } \
     form { margin-block-end: 1em; } \
     [data-bgmail-quirky-start] { margin-top: 0; } \
     [data-bgmail-quirky-end] { margin-bottom: 0; }";

/// Marks the blocks whose default margin Chromium ignores in quirks mode
/// (Blink's "quirky" `__qem` margins), for [`QUIRKS_UA_CSS`]:
/// - `data-bgmail-quirky-start`: the paragraph, list, heading, ... that is
///   the first content of the body or a table cell;
/// - `data-bgmail-quirky-end`: the paragraph or `pre` that is the last
///   content of a table cell (other blocks' bottom margins aren't quirky).
///
/// Both look through wrappers (`<td><font><p>`). Text beside the block keeps
/// its margin, which is why this is a DOM pass and not a `:first-child`
/// selector.
fn mark_quirky_margins(doc: &mut BaseDocument) {
    const QUIRKY_START: [&str; 16] = [
        "p",
        "ul",
        "ol",
        "dl",
        "menu",
        "dir",
        "blockquote",
        "pre",
        "listing",
        "xmp",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
    ];
    const QUIRKY_END: [&str; 4] = ["p", "pre", "listing", "xmp"];
    const WRAPPERS: [&str; 5] = ["div", "center", "font", "form", "span"];
    let tag = |doc: &BaseDocument, id: NodeId| {
        doc.get_node(id)
            .and_then(|node| node.element_data())
            .map(|element| element.name.local.to_string())
    };
    // The quirky block at one edge of `container`, through wrappers.
    let edge_block = |doc: &BaseDocument, mut container: NodeId, end: bool, quirky: &[&str]| loop {
        let node = doc.get_node(container)?;
        // Comments, collapsible whitespace and floats (`<img align=left>`)
        // aren't in-flow content.
        let is_content = |child: &NodeId| {
            doc.get_node(*child).is_some_and(|child| match &child.data {
                blitz_dom::NodeData::Text(text) => {
                    !text.content.chars().all(|c| c.is_ascii_whitespace())
                }
                blitz_dom::NodeData::Comment { .. } => false,
                blitz_dom::NodeData::Element(element) => {
                    !matches!(
                        element
                            .attr(local_name!("align"))
                            .map(str::to_ascii_lowercase)
                            .as_deref(),
                        Some("left" | "right")
                    ) || !matches!(element.name.local.as_ref(), "img" | "table")
                }
                _ => true,
            })
        };
        let child = if end {
            node.children.iter().rev().copied().find(is_content)
        } else {
            node.children.iter().copied().find(is_content)
        }?;
        match tag(doc, child).as_deref() {
            Some(name) if quirky.contains(&name) => return Some(child),
            Some(name) if WRAPPERS.contains(&name) => container = child,
            _ => return None,
        }
    };
    let mut marked = Vec::new();
    let mut stack = vec![doc.root_node().id];
    while let Some(id) = stack.pop() {
        if let Some(node) = doc.get_node(id) {
            stack.extend(node.children.iter().copied());
        }
        let cell = match tag(doc, id).as_deref() {
            Some("td" | "th") => true,
            Some("body") => false,
            _ => continue,
        };
        if let Some(block) = edge_block(doc, id, false, &QUIRKY_START) {
            marked.push((block, "data-bgmail-quirky-start"));
        }
        if let Some(block) = cell
            .then(|| edge_block(doc, id, true, &QUIRKY_END))
            .flatten()
        {
            marked.push((block, "data-bgmail-quirky-end"));
        }
    }
    let mut mutator = doc.mutate();
    for (id, attr) in marked {
        mutator.set_attribute(
            id,
            blitz_dom::QualName::new(None, blitz_dom::ns!(), attr.into()),
            "",
        );
    }
}

/// Text selection highlight used when [`DocumentOptions::selection_color`]
/// is not set (RGBA).
pub const DEFAULT_SELECTION_COLOR: [u8; 4] = [180, 213, 255, 255];

/// Options for [`HtmlDocument::parse`].
#[derive(Default)]
pub struct DocumentOptions {
    pub appearance: Appearance,
    /// Extra user-agent CSS appended after Blitz's default sheet (e.g. the
    /// reader's base typography). Author styles in the mail still win.
    pub user_agent_css: Option<String>,
    /// URL relative references resolve against ([`MESSAGE_BASE_URL`] when unset).
    pub base_url: Option<String>,
    pub resources: Resources,
    /// Text selection highlight (RGBA); the host passes its theme's color.
    pub selection_color: Option<[u8; 4]>,
}

/// Inline formatting roots (the boxes that own text) under `node_id`, in
/// layout order.
fn inline_roots(doc: &BaseDocument, node_id: NodeId, out: &mut Vec<NodeId>) {
    let Some(node) = doc.get_node(node_id) else {
        return;
    };
    if node.flags.is_inline_root() {
        out.push(node_id);
    }
    if let Some(children) = node.layout_children.borrow().as_ref() {
        for &child in children.iter() {
            inline_roots(doc, child, out);
        }
    }
}

/// Rightmost edge (px from the left of `node_id`'s parent at `left`) of the
/// in-flow boxes under `node_id`, not descending into boxes that clip their
/// horizontal overflow. Blitz's `scrollWidth` misses overflow from nested
/// tables, which fixed-width mail has plenty of.
fn overflow_right(
    doc: &blitz_dom::BaseDocument,
    node_id: blitz_traits::node_id::NodeId,
    left: f32,
) -> f32 {
    let Some(node) = doc.get_node(node_id) else {
        return left;
    };
    let layout = node.final_layout();
    let x = left + layout.location.x;
    let mut right = x + layout.size.width;
    let clips = node.primary_styles().is_some_and(|style| {
        !matches!(
            style.get_box().overflow_x,
            style::values::computed::Overflow::Visible
        )
    });
    if !clips {
        if let Some(children) = node.layout_children.borrow().as_ref() {
            for &child in children.iter() {
                right = right.max(overflow_right(doc, child, x));
            }
        }
    }
    right
}

/// A mail body ready to be measured and painted.
pub struct HtmlDocument {
    inner: blitz_html::HtmlDocument,
    resources: Arc<Resources>,
    appearance: Appearance,
    layout_width: Option<u32>,
    content_width: f32,
    content_height: f32,
    /// Cached display list and the height it was painted for.
    display_list: Option<(f32, Arc<DisplayList>)>,
    selection_color: peniko::Color,
    /// Base URL links resolve against.
    base_url: url::Url,
    /// Links activated by clicks, for the host to open.
    links: Arc<LinkCapture>,
    /// Matches of the last [`HtmlDocument::find`]: (inline root, byte range).
    find_matches: Vec<(NodeId, Range<usize>)>,
    /// Element nesting depth of the parsed tree.
    depth: usize,
}

impl HtmlDocument {
    /// Parses `html` as an HTML document (never as XML, whatever its doctype).
    pub fn parse(html: &str, options: DocumentOptions) -> Self {
        let resources = Arc::new(options.resources);
        let base_url = options
            .base_url
            .clone()
            .unwrap_or_else(|| MESSAGE_BASE_URL.to_string());
        let links = Arc::new(LinkCapture::default());
        let mode = document_mode(html);
        let mut ua_stylesheets = vec![
            UserAgentStylesheet::default_sheet(),
            UserAgentStylesheet::parse(FORM_CONTROLS_UA_CSS),
        ];
        if mode == DocumentMode::Quirks {
            ua_stylesheets.push(UserAgentStylesheet::parse(QUIRKS_UA_CSS));
        }
        if let Some(css) = &options.user_agent_css {
            ua_stylesheets.push(UserAgentStylesheet::parse(css));
        }
        let config = DocumentConfig {
            // A base URL makes relative and `cid:` references resolvable.
            base_url: Some(base_url.clone()),
            net_provider: Some(Arc::new(ResourceProvider(resources.clone()))),
            // Clicked links are reported to the host, never followed.
            navigation_provider: Some(links.clone()),
            viewport: Some(viewport(800, options.appearance)),
            ua_stylesheets,
            font_ctx: Some(crate::fonts::font_context()),
            ..Default::default()
        };
        let mut inner = blitz_html::HtmlDocument::from_html(&force_html_parse(html, mode), config);
        let base = url::Url::parse(&base_url)
            .unwrap_or_else(|_| url::Url::parse(MESSAGE_BASE_URL).expect("valid base URL"));
        replace_unavailable_images(&mut inner, &resources, &base, mode == DocumentMode::Quirks);
        if mode == DocumentMode::Quirks {
            mark_quirky_margins(&mut inner);
        }
        // Blitz styles in no-quirks mode; keep the quirk mail layouts visibly
        // depend on.
        inner.set_line_height_quirk(mode != DocumentMode::NoQuirks);
        Self {
            inner,
            resources,
            appearance: options.appearance,
            layout_width: None,
            content_width: 0.0,
            content_height: 0.0,
            display_list: None,
            selection_color: {
                let [r, g, b, a] = options.selection_color.unwrap_or(DEFAULT_SELECTION_COLOR);
                peniko::Color::from_rgba8(r, g, b, a)
            },
            base_url: base,
            links,
            find_matches: Vec::new(),
            depth: 0,
        }
        .with_depth()
    }

    /// Records the tree's nesting depth (see [`MAX_NESTING_DEPTH`]).
    fn with_depth(mut self) -> Self {
        self.depth = nesting_depth(&self.inner, self.inner.root_node().id);
        self
    }

    /// Like [`Self::parse`], but refuses documents that can't be rendered
    /// safely (see [`RenderError`]).
    pub fn try_parse(html: &str, options: DocumentOptions) -> Result<Self, RenderError> {
        let document = Self::parse(html, options);
        if document.depth > MAX_NESTING_DEPTH {
            return Err(RenderError::TooDeep {
                depth: document.depth,
            });
        }
        Ok(document)
    }

    /// Element nesting depth of the document.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Lays the document out for `width` CSS px and returns its content height.
    /// Cheap when the width did not change.
    pub fn layout(&mut self, width: f32) -> f32 {
        // Never style or lay out a tree deep enough to overflow the stack;
        // it stays empty (hosts use `try_parse` and show a fallback).
        if self.depth > MAX_NESTING_DEPTH {
            return 0.0;
        }
        let width = width.max(1.0).round() as u32;
        if self.layout_width != Some(width) {
            self.inner.set_viewport(viewport(width, self.appearance));
            self.layout_width = Some(width);
            self.display_list = None;
        }
        // Resolve until resources delivered during resolution are applied.
        for _ in 0..4 {
            self.inner.resolve(0.0);
            if !self.inner.has_pending_critical_resources() {
                break;
            }
        }
        let root = self.inner.root_element();
        self.content_height = root.final_layout().size.height.max(root.scroll_height());
        // Mail wider than the pane (fixed-width tables) overflows to the right
        // and scrolls horizontally, as in a browser; layout keeps the pane width.
        self.content_width = (width as f32).max(overflow_right(&self.inner, root.id, 0.0));
        self.content_height
    }

    /// Content width from the last [`HtmlDocument::layout`]: the layout width,
    /// or more when the mail overflows it.
    pub fn content_width(&self) -> f32 {
        self.content_width
    }

    /// Content height from the last [`HtmlDocument::layout`].
    pub fn content_height(&self) -> f32 {
        self.content_height
    }

    /// Drawing commands for the current layout, painted at content height.
    pub fn display_list(&mut self) -> Arc<DisplayList> {
        self.display_list_for_height(self.content_height)
    }

    /// Drawing commands for the current layout painted over at least
    /// `min_height` px: like a browser canvas, the root background extends to
    /// the bottom of the viewport when the content is shorter. Recorded once
    /// per layout/height and cached until the document state changes.
    pub fn display_list_for_height(&mut self, min_height: f32) -> Arc<DisplayList> {
        if self.depth > MAX_NESTING_DEPTH {
            return Arc::new(DisplayList::new());
        }
        let height = self.content_height.max(min_height).ceil().max(1.0);
        if let Some((cached_height, list)) = &self.display_list {
            if *cached_height == height {
                return list.clone();
            }
        }
        let width = (self.content_width.ceil() as u32).max(self.layout_width.unwrap_or(800));
        let mut list = DisplayList::new();
        crate::blitz_paint::paint_scene(
            &mut list,
            &mut self.inner,
            1.0,
            width,
            height as u32,
            0,
            0,
            self.selection_color,
        );
        let list = Arc::new(list);
        self.display_list = Some((height, list.clone()));
        list
    }

    /// Discards the cached display list (after hover/selection changes).
    pub fn invalidate_paint(&mut self) {
        self.display_list = None;
    }

    /// Handles a pointer event: hover, text selection (drag; double click
    /// selects a word, triple click a paragraph) and link clicks (see
    /// [`Self::take_activated_link`]). Returns whether the document must be
    /// repainted. Only the primary button presses reach the document; other
    /// buttons are the host's (context menus).
    pub fn pointer(&mut self, input: PointerInput) -> bool {
        let before = (
            self.inner.get_hover_node_id(),
            self.inner.get_text_selection_ranges(),
        );
        let pressed = input.button == PointerButton::Primary;
        match input.phase {
            PointerPhase::Down { click_count } if pressed && click_count >= 2 => {
                self.select_at(input.x, input.y, click_count >= 3);
            }
            PointerPhase::Down { .. } | PointerPhase::Up if !pressed => {}
            phase => {
                let event = pointer_event(&input);
                let event = match phase {
                    PointerPhase::Down { .. } => UiEvent::PointerDown(event),
                    PointerPhase::Move => UiEvent::PointerMove(event),
                    PointerPhase::Up => UiEvent::PointerUp(event),
                };
                EventDriver::new(&mut self.inner, NoopEventHandler).handle_ui_event(event);
                // Fragment links scroll Blitz's own viewport; the host owns
                // scrolling, so the document always paints from its origin.
                let scroll = self.inner.viewport_scroll();
                if scroll.x != 0.0 || scroll.y != 0.0 {
                    self.inner
                        .set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 0.0 });
                }
            }
        }
        let after = (
            self.inner.get_hover_node_id(),
            self.inner.get_text_selection_ranges(),
        );
        let changed = before != after;
        if changed {
            self.display_list = None;
        }
        changed
    }

    /// Selects the word (or, with `paragraph`, the whole inline root) at a
    /// point; clears the selection when the point is not over text.
    fn select_at(&mut self, x: f32, y: f32, paragraph: bool) {
        let Some((root, offset)) = self.inner.find_text_position(x, y) else {
            self.inner.clear_text_selection();
            return;
        };
        let Some(text) = self.root_text(root) else {
            return;
        };
        let range = if paragraph {
            0..text.len()
        } else {
            word_bounds(text, offset)
        };
        self.inner
            .set_text_selection(root, range.start, root, range.end);
    }

    /// Text of an inline root.
    fn root_text(&self, root: NodeId) -> Option<&str> {
        let layout = self
            .inner
            .get_node(root)?
            .element_data()?
            .inline_layout_data
            .as_ref()?;
        Some(&layout.text)
    }

    /// The link activated by the last click (absolute URL), cleared.
    pub fn take_activated_link(&mut self) -> Option<String> {
        self.links.take()
    }

    /// What lies under a document point: link URL, image source, text.
    pub fn hit_info(&self, x: f32, y: f32) -> HitInfo {
        let Some(hit) = self.inner.hit(x, y) else {
            return HitInfo::default();
        };
        let mut info = HitInfo {
            text: hit.is_text,
            ..Default::default()
        };
        let mut node = self.inner.get_node(hit.node_id);
        while let Some(current) = node {
            if let Some(element) = current.element_data() {
                if info.image.is_none() && element.name.local == local_name!("img") {
                    info.image = element
                        .attr(local_name!("src"))
                        .filter(|src| !src.is_empty())
                        .or_else(|| {
                            element.attrs().iter().find_map(|attr| {
                                (&*attr.name.local == "data-rm-blocked-src")
                                    .then_some(attr.value.as_str())
                            })
                        })
                        .map(str::to_string);
                }
                if element.name.local == local_name!("a") {
                    info.link = element
                        .attr(local_name!("href"))
                        .and_then(|href| self.base_url.join(href.trim()).ok())
                        .map(|url| url.to_string());
                    break;
                }
            }
            node = current.parent.and_then(|id| self.inner.get_node(id));
        }
        info
    }

    /// Whether some text is selected.
    pub fn has_selection(&self) -> bool {
        self.inner.has_text_selection()
    }

    /// The selected text, if any.
    pub fn selected_text(&self) -> Option<String> {
        self.inner.get_selected_text()
    }

    /// Selects all the text of the document.
    pub fn select_all(&mut self) {
        let mut roots = Vec::new();
        inline_roots(&self.inner, self.inner.root_element().id, &mut roots);
        let (Some(&first), Some(&last)) = (roots.first(), roots.last()) else {
            return;
        };
        let end = self.root_text(last).map_or(0, str::len);
        self.inner.set_text_selection(first, 0, last, end);
        self.display_list = None;
    }

    /// Clears the text selection.
    pub fn clear_selection(&mut self) {
        if self.inner.has_text_selection() {
            self.inner.clear_text_selection();
            self.display_list = None;
        }
    }

    /// Finds every case-insensitive occurrence of `query` in the document's
    /// text and returns how many there are (see [`Self::select_match`]).
    pub fn find(&mut self, query: &str) -> usize {
        let mut roots = Vec::new();
        inline_roots(&self.inner, self.inner.root_element().id, &mut roots);
        self.find_matches = roots
            .into_iter()
            .flat_map(|root| {
                let ranges = self.root_text(root).map(|text| find_all(text, query));
                ranges
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |range| (root, range))
            })
            .collect();
        self.find_matches.len()
    }

    /// Selects match `index` of the last [`Self::find`] and returns the top
    /// and bottom of its line (document y), to scroll it into view.
    pub fn select_match(&mut self, index: usize) -> Option<(f32, f32)> {
        let (root, range) = self.find_matches.get(index)?.clone();
        self.inner
            .set_text_selection(root, range.start, root, range.end);
        self.display_list = None;
        let node = self.inner.get_node(root)?;
        let text = node.element_data()?.inline_layout_data.as_ref()?;
        let scale = text.layout.scale();
        let caret = parley::Cursor::from_byte_index(
            &text.layout,
            range.start,
            parley::Affinity::Downstream,
        )
        .geometry(&text.layout, 1.0);
        let layout = node.final_layout();
        let top = node.absolute_position(0.0, 0.0).y
            + layout.padding.top
            + layout.border.top
            + text.block_offset;
        Some((top + caret.y0 as f32 / scale, top + caret.y1 as f32 / scale))
    }

    /// Remote URLs the document asked for and that were blocked.
    pub fn blocked_remote_urls(&self) -> Vec<String> {
        self.resources.blocked_urls()
    }

    /// The underlying Blitz document (hit testing, selection, DOM access).
    pub fn blitz(&self) -> &blitz_dom::BaseDocument {
        &self.inner
    }

    /// Mutable access to the underlying Blitz document.
    pub fn blitz_mut(&mut self) -> &mut blitz_dom::BaseDocument {
        self.display_list = None;
        &mut self.inner
    }
}

/// A Blitz mouse event for `input` (document, client and screen coordinates
/// coincide: the document is painted from its origin).
fn pointer_event(input: &PointerInput) -> BlitzPointerEvent {
    let button = match input.button {
        PointerButton::Primary => MouseEventButton::Main,
        PointerButton::Secondary => MouseEventButton::Secondary,
        PointerButton::Middle => MouseEventButton::Auxiliary,
    };
    let buttons = if input.primary_pressed {
        MouseEventButtons::Primary
    } else {
        MouseEventButtons::None
    };
    let mut mods = keyboard_types::Modifiers::empty();
    mods.set(keyboard_types::Modifiers::SHIFT, input.modifiers.shift);
    mods.set(keyboard_types::Modifiers::CONTROL, input.modifiers.control);
    mods.set(keyboard_types::Modifiers::ALT, input.modifiers.alt);
    mods.set(keyboard_types::Modifiers::META, input.modifiers.platform);
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: input.x,
            page_y: input.y,
            screen_x: input.x,
            screen_y: input.y,
            client_x: input.x,
            client_y: input.y,
        },
        button,
        buttons,
        mods,
        details: PointerDetails::default(),
        element: Default::default(),
        active_pointers: Default::default(),
    }
}

fn viewport(width: u32, appearance: Appearance) -> Viewport {
    let scheme = match appearance {
        Appearance::Light => ColorScheme::Light,
        Appearance::Dark => ColorScheme::Dark,
    };
    Viewport::new(width, VIEWPORT_HEIGHT, 1.0, scheme)
}

/// Blitz sniffs XHTML doctypes, `<?xml` prologs and `<html xmlns=…xhtml>` roots
/// and switches to its XML parser, which drops the HTML user-agent stylesheet
/// (`<p>` turns inline, hidden preheaders show). Mail is always `text/html`, so
/// browsers parse it as HTML; replacing the prolog with an HTML doctype on its
/// own line makes Blitz do the same. A quirks-mode document gets a legacy
/// HTML 4 doctype, so the parser keeps its quirks (`<table>` doesn't close an
/// open `<p>`).
pub fn force_html_parse(html: &str, mode: DocumentMode) -> String {
    let mut rest = html.trim_start_matches('\u{feff}').trim_start();
    loop {
        let lower = rest
            .get(..9)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if lower.starts_with("<?xml") || lower.starts_with("<!doctype") {
            match rest.find('>') {
                Some(end) => rest = rest[end + 1..].trim_start(),
                None => break,
            }
        } else {
            break;
        }
    }
    let doctype = match mode {
        DocumentMode::Quirks => "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0 Transitional//EN\">",
        DocumentMode::LimitedQuirks | DocumentMode::NoQuirks => "<!DOCTYPE html>",
    };
    format!("{doctype}\n{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::{HitInfo, PointerButton, PointerInput, PointerPhase};

    #[test]
    fn document_modes_follow_the_doctype() {
        // Doctype-less mail (documents or fragments) renders in standards mode.
        assert_eq!(document_mode("<p>no doctype"), DocumentMode::NoQuirks);
        assert_eq!(
            document_mode("<html><body>x</body></html>"),
            DocumentMode::NoQuirks
        );
        assert_eq!(document_mode("<!DOCTYPE html><p>"), DocumentMode::NoQuirks);
        assert_eq!(
            document_mode("\u{feff}<!doctype HTML>"),
            DocumentMode::NoQuirks
        );
        assert_eq!(
            document_mode("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">"),
            DocumentMode::LimitedQuirks
        );
        assert_eq!(
            document_mode("<?xml version=\"1.0\"?><!-- c --><!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"x\">"),
            DocumentMode::NoQuirks
        );
        assert_eq!(
            document_mode("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">"),
            DocumentMode::Quirks
        );
        assert_eq!(
            document_mode("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\" \"http://www.w3.org/TR/html4/loose.dtd\">"),
            DocumentMode::LimitedQuirks
        );
    }

    #[test]
    fn line_height_quirk_removes_gap_under_lone_images() {
        let cell = "<table cellpadding=0 cellspacing=0><tr><td id=c><a style=\"display:inline-block\"><img width=20 height=20 style=\"display:block\"></a></td></tr></table>";
        let height = |html: String| {
            let mut doc = HtmlDocument::parse(&html, DocumentOptions::default());
            doc.layout(400.0);
            let id = doc.blitz().get_element_by_id("c").unwrap();
            doc.blitz().get_node(id).unwrap().final_layout().size.height
        };
        let quirks = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.01 Transitional//EN\">";
        assert_eq!(height(format!("{quirks}{cell}")), 20.0); // quirks: just the image
        assert!(height(format!("<!DOCTYPE html>{cell}")) > 20.0); // standards: strut gap
    }

    #[test]
    fn force_html_parse_replaces_xhtml_prolog() {
        let html = "\u{feff}<?xml version=\"1.0\"?>\n<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"x\"><html xmlns=\"http://www.w3.org/1999/xhtml\"><p>a</p></html>";
        let out = force_html_parse(html, DocumentMode::LimitedQuirks);
        assert!(out.starts_with("<!DOCTYPE html>\n<html xmlns="));
        assert!(!out.contains("XHTML"));
    }

    #[test]
    fn force_html_parse_adds_doctype_to_fragments() {
        assert_eq!(
            force_html_parse("<div>x</div>", DocumentMode::NoQuirks),
            "<!DOCTYPE html>\n<div>x</div>"
        );
    }

    #[test]
    fn force_html_parse_handles_lowercase_doctype() {
        assert_eq!(
            force_html_parse("<!doctype html><p>x", DocumentMode::NoQuirks),
            "<!DOCTYPE html>\n<p>x"
        );
    }

    #[test]
    fn layout_measures_content_and_caches_paint() {
        let mut doc = HtmlDocument::parse(
            "<div style=\"height: 1234px\">tall</div>",
            DocumentOptions::default(),
        );
        let height = doc.layout(400.0);
        assert!(height >= 1234.0, "height = {height}");
        let first = doc.display_list();
        let second = doc.display_list();
        assert!(Arc::ptr_eq(&first, &second));
        doc.layout(300.0);
        assert!(!Arc::ptr_eq(&first, &doc.display_list()));
    }

    #[test]
    fn root_background_fills_the_requested_height() {
        let mut doc = HtmlDocument::parse(
            "<body style=\"background:#123456\"><p>short</p></body>",
            DocumentOptions::default(),
        );
        let content = doc.layout(400.0);
        let list = doc.display_list_for_height(content + 500.0);
        let tallest_fill = list
            .commands
            .iter()
            .filter_map(|c| match c {
                crate::display_list::Command::Fill { geometry, .. } => {
                    Some(geometry.bounds().height())
                }
                _ => None,
            })
            .fold(0.0, f64::max);
        assert!(
            tallest_fill >= (content + 500.0) as f64 - 1.0,
            "{tallest_fill}"
        );
    }

    #[test]
    fn xhtml_mail_keeps_block_paragraphs() {
        // Parsed as XML, `<p>` would be inline and both lines share a baseline.
        let html = "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \"x\"><html><body><p>one</p><p>two</p></body></html>";
        let mut doc = HtmlDocument::parse(html, DocumentOptions::default());
        doc.layout(600.0);
        let list = doc.display_list();
        let baselines: Vec<f32> = list
            .commands
            .iter()
            .filter_map(|c| match c {
                crate::display_list::Command::Text(run) => Some(run.baseline),
                _ => None,
            })
            .collect();
        assert_eq!(baselines.len(), 2, "{baselines:?}");
        assert!(baselines[1] > baselines[0] + 10.0, "{baselines:?}");
    }

    #[test]
    fn relative_urls_do_not_panic_and_are_not_remote() {
        let mut doc = HtmlDocument::parse(
            "<img src=\"assets/logo.png\"><div style=\"background:url(x.png)\">x</div>",
            DocumentOptions::default(),
        );
        doc.layout(300.0);
        assert!(doc.blocked_remote_urls().is_empty());
    }

    #[test]
    fn records_blocked_remote_images() {
        let mut doc = HtmlDocument::parse(
            "<img src=\"https://tracker.example/p.gif\">",
            DocumentOptions::default(),
        );
        doc.layout(300.0);
        assert_eq!(
            doc.blocked_remote_urls(),
            vec!["https://tracker.example/p.gif".to_string()]
        );
    }

    #[test]
    fn text_runs_carry_words_and_face() {
        let mut doc = HtmlDocument::parse("<p>Hello brave world</p>", DocumentOptions::default());
        doc.layout(600.0);
        let list = doc.display_list();
        let run = list
            .commands
            .iter()
            .find_map(|c| match c {
                crate::display_list::Command::Text(run) => Some(run.clone()),
                _ => None,
            })
            .expect("a text run");
        let words: Vec<&str> = run.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(words, ["Hello", "brave", "world"]);
        assert!(run.words[0].x < run.words[1].x && run.words[1].x < run.words[2].x);
        assert!(!run.face.family.is_empty());
        assert!(run.font_size > 10.0);
    }

    /// Border box (x, y, w, h) of `#id` after laying `html` out at 400px.
    fn box_of(html: &str, id: &str) -> (f32, f32, f32, f32) {
        let mut doc = HtmlDocument::parse(html, DocumentOptions::default());
        doc.layout(400.0);
        let blitz = doc.blitz();
        let node = blitz
            .get_node(blitz.get_element_by_id(id).unwrap())
            .unwrap();
        let p = node.absolute_position(0.0, 0.0);
        let size = node.final_layout().size;
        (p.x, p.y, size.width, size.height)
    }

    #[test]
    fn rtl_containers_reverse_box_only_lines() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div dir=\"rtl\" style=\"width:400px;font-size:0\"><div id=\"a\" style=\"display:inline-block;width:100px;height:10px\"></div><div id=\"b\" style=\"display:inline-block;width:300px;height:10px\"></div></div>";
        assert_eq!(box_of(html, "a").0, 300.0);
        assert_eq!(box_of(html, "b").0, 0.0);
    }

    #[test]
    fn keyword_max_widths_size_blocks() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><p id=\"m\" style=\"max-width:min-content;margin:0\">aa bb</p><p id=\"f\" style=\"max-width:max-content;margin:0\">aa</p>";
        let (_, _, min_w, _) = box_of(html, "m");
        let (_, _, fit_w, _) = box_of(html, "f");
        assert!(min_w > 0.0 && min_w < 40.0, "{min_w}");
        assert!(fit_w > 0.0 && fit_w < 40.0, "{fit_w}");
    }

    #[test]
    fn anonymous_cell_block_margins_collapse_with_children() {
        // `<td style="display:block">` sits in an anonymous cell where its 15px
        // margin collapses with its child's 16px ones: cell = 16 + 24 + 16.
        let html = "<!DOCTYPE html><body style=\"margin:0\"><table id=\"t\" cellpadding=0 cellspacing=0 width=\"100%\"><tr><td id=\"c\" style=\"display:block;margin-top:15px\"><p style=\"margin:16px 0;height:24px\"></p></td></tr></table>";
        assert_eq!(box_of(html, "t").3, 56.0);
        let (_, y, _, h) = box_of(html, "c");
        assert_eq!((y, h), (16.0, 24.0));
    }

    #[test]
    fn consecutive_block_cells_stack_in_one_anonymous_cell() {
        // Responsive mail: `display:block` cells of one row stack full width,
        // even with hidden cells between them.
        let html = "<!DOCTYPE html><body style=\"margin:0\"><table cellpadding=0 cellspacing=0 width=\"100%\"><tr><td id=\"a\" style=\"display:block\"><div style=\"height:30px\"></div></td> <td style=\"display:none\">hidden</td> <td id=\"b\" style=\"display:block\"><div style=\"height:20px\"></div></td></tr></table>";
        assert_eq!(box_of(html, "a"), (0.0, 0.0, 400.0, 30.0));
        assert_eq!(box_of(html, "b"), (0.0, 30.0, 400.0, 20.0));
    }

    #[test]
    fn clamped_images_keep_a_definite_height() {
        // A 540:48-ratio image (as a `cid:` attachment) in a 400px body.
        let mut png = Vec::new();
        image::RgbaImage::new(45, 4)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let size_of = |style: &str| {
            let html = format!("<!DOCTYPE html><body style=\"margin:0\"><img id=\"i\" src=\"cid:p\" width=\"540\" height=\"48\" style=\"display:block;max-width:100%;{style}\">");
            let options = DocumentOptions {
                resources: Resources::new().with_attachment("p", png.clone()),
                ..Default::default()
            };
            let mut doc = HtmlDocument::parse(&html, options);
            doc.layout(400.0);
            let blitz = doc.blitz();
            let node = blitz
                .get_node(blitz.get_element_by_id("i").unwrap())
                .unwrap();
            let size = node.final_layout().size;
            (size.width, size.height)
        };
        // `height` (from the attribute) stays; with `height:auto` the ratio wins.
        assert_eq!(size_of(""), (400.0, 48.0));
        let (w, h) = size_of("height:auto");
        assert_eq!(w, 400.0);
        assert!((h - 400.0 * 48.0 / 540.0).abs() < 0.5, "{h}");
    }

    #[test]
    fn content_width_covers_overflowing_mail() {
        let mut doc = HtmlDocument::parse(
            "<!DOCTYPE html><body style=\"margin:0\"><table width=\"700\"><tr><td>x</td></tr></table>",
            DocumentOptions::default(),
        );
        doc.layout(400.0);
        assert_eq!(doc.content_width(), 700.0);
        doc.layout(900.0);
        assert_eq!(doc.content_width(), 900.0);
    }

    #[test]
    fn nested_table_rows_fit_inline_block_buttons() {
        // The inner row is measured before its final layout; the button must
        // sit on its text baseline then too, or the row grows by the strut.
        let button = "<a id=\"a\" style=\"display:inline-block;padding:12px 24px;border:2px solid blue\">Go</a>";
        let html = format!("<!DOCTYPE html><body style=\"margin:0;font-size:16px;line-height:1.3\"><table cellpadding=0 cellspacing=0><tr><td><table cellpadding=0 cellspacing=0><tr><td id=\"c\">{button}</td></tr></table></td></tr></table>");
        assert_eq!(box_of(&html, "c").3, box_of(&html, "a").3);
    }

    #[test]
    fn inline_block_tables_sit_on_their_bottom_edge() {
        // Responsive buttons: `<table style="display:inline-block">` lends no
        // baseline to the line (Chromium), unlike a real `inline-table`.
        let line = |display: &str| {
            let html = format!("<!DOCTYPE html><body style=\"margin:0\"><div id=\"o\" style=\"font-size:15px;line-height:20px\"><table cellpadding=0 cellspacing=0 style=\"display:{display}\"><tr><td style=\"padding:13px;line-height:15px\">Btn</td></tr></table></div>");
            box_of(&html, "o").3
        };
        assert!(line("inline-block") > line("inline-table") + 3.0);
    }

    fn laid_out(html: &str) -> HtmlDocument {
        let mut doc = HtmlDocument::parse(html, DocumentOptions::default());
        doc.layout(400.0);
        doc
    }

    fn press(doc: &mut HtmlDocument, x: f32, y: f32, phase: PointerPhase, held: bool) -> bool {
        doc.pointer(PointerInput {
            x,
            y,
            phase,
            button: PointerButton::Primary,
            primary_pressed: held,
            modifiers: Default::default(),
        })
    }

    /// Two paragraphs, two links (one per line, so their paragraphs locate
    /// them: inline boxes have no box of their own), loose text in an
    /// anonymous block and a blocked image.
    const TEXT: &str = "<!DOCTYPE html><body style=\"margin:0;font-size:16px\"><p id=\"a\" style=\"margin:0\">hello brave world</p><p id=\"b\" style=\"margin:0\">second line</p><p id=\"l\" style=\"margin:0\"><a href=\"https://example.com/x\">link</a></p><p id=\"r\" style=\"margin:0\"><a href=\"page.html\">rel</a></p>loose text<img id=\"i\" data-rm-blocked-src=\"https://t.test/p.gif\" style=\"display:block;width:20px;height:20px\">";

    #[test]
    fn clicking_a_link_reports_it_without_navigating() {
        let mut doc = laid_out(TEXT);
        let (x, y, _, h) = box_of(TEXT, "l");
        let (x, y) = (x + 2.0, y + h / 2.0);
        press(&mut doc, x, y, PointerPhase::Down { click_count: 1 }, true);
        press(&mut doc, x, y, PointerPhase::Up, false);
        assert_eq!(
            doc.take_activated_link().as_deref(),
            Some("https://example.com/x")
        );
        assert_eq!(doc.take_activated_link(), None);
    }

    #[test]
    fn hit_info_reports_links_images_and_text() {
        let doc = laid_out(TEXT);
        // A point on the first glyphs of a line (links sit at the start).
        let start = |id: &str| {
            let (x, y, _, h) = box_of(TEXT, id);
            (x + 5.0, y + h / 2.0)
        };
        let (x, y) = start("l");
        let link = doc.hit_info(x, y);
        assert_eq!(link.link.as_deref(), Some("https://example.com/x"));
        assert!(link.text);
        let (x, y) = start("r");
        assert_eq!(
            doc.hit_info(x, y).link.as_deref(),
            Some("bgmail://message/page.html")
        );
        let (x, y) = start("i");
        assert_eq!(
            doc.hit_info(x, y).image.as_deref(),
            Some("https://t.test/p.gif")
        );
        let (x, y) = start("a");
        assert_eq!(
            doc.hit_info(x, y),
            HitInfo {
                text: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn double_and_triple_clicks_select_word_and_paragraph() {
        let mut doc = laid_out(TEXT);
        let (x, y, _, h) = box_of(TEXT, "a");
        // "hello" starts the line; a point inside it.
        assert!(press(
            &mut doc,
            x + 10.0,
            y + h / 2.0,
            PointerPhase::Down { click_count: 2 },
            true
        ));
        assert_eq!(doc.selected_text().as_deref(), Some("hello"));
        press(
            &mut doc,
            x + 10.0,
            y + h / 2.0,
            PointerPhase::Down { click_count: 3 },
            true,
        );
        assert_eq!(doc.selected_text().as_deref(), Some("hello brave world"));
        doc.clear_selection();
        assert!(!doc.has_selection());
    }

    #[test]
    fn dragging_selects_text_and_select_all_spans_the_document() {
        let mut doc = laid_out(TEXT);
        let (x, y, w, h) = box_of(TEXT, "a");
        let mid = y + h / 2.0;
        press(
            &mut doc,
            x + 1.0,
            mid,
            PointerPhase::Down { click_count: 1 },
            true,
        );
        assert!(press(
            &mut doc,
            x + w.min(60.0),
            mid,
            PointerPhase::Move,
            true
        ));
        press(&mut doc, x + w.min(60.0), mid, PointerPhase::Up, false);
        let dragged = doc.selected_text().unwrap_or_default();
        assert!(dragged.starts_with("hel"), "{dragged}");
        doc.select_all();
        let all = doc.selected_text().unwrap_or_default();
        // Across paragraphs and into the anonymous block of loose text.
        for part in ["hello brave world", "second line", "loose text"] {
            assert!(all.contains(part), "{all}");
        }
    }

    #[test]
    fn find_counts_matches_and_selects_them_in_place() {
        let mut doc = laid_out(TEXT);
        assert_eq!(doc.find("LINE"), 1);
        assert_eq!(doc.find("o"), 5); // hello, world, second, loose (2)
        assert_eq!(doc.find("second"), 1);
        let (top, bottom) = doc.select_match(0).unwrap();
        let (_, y, _, h) = box_of(TEXT, "b");
        assert!(
            top >= y - 0.5 && bottom <= y + h + 0.5,
            "{top}..{bottom} vs {y}+{h}"
        );
        assert_eq!(doc.selected_text().as_deref(), Some("second"));
        assert_eq!(doc.select_match(5), None);
    }

    #[test]
    fn selection_is_painted_with_the_given_color() {
        let mut doc = HtmlDocument::parse(
            TEXT,
            DocumentOptions {
                selection_color: Some([10, 20, 30, 255]),
                ..Default::default()
            },
        );
        doc.layout(400.0);
        doc.select_all();
        let color = peniko::Color::from_rgba8(10, 20, 30, 255);
        let painted = doc.display_list().commands.iter().any(|c| {
            matches!(c, crate::display_list::Command::Fill {
                brush: crate::display_list::Brush::Solid(fill), ..
            } if *fill == color)
        });
        assert!(painted);
    }

    #[test]
    fn hostile_nesting_is_refused_and_never_laid_out() {
        let deep = |levels: usize| {
            format!(
                "<!DOCTYPE html><body>{}x{}",
                "<div>".repeat(levels),
                "</div>".repeat(levels)
            )
        };
        // Shallow enough: parses and lays out.
        let ok = HtmlDocument::try_parse(&deep(100), DocumentOptions::default()).unwrap();
        assert!(ok.depth() > 100 && ok.depth() <= MAX_NESTING_DEPTH);
        // Deeper than the limit (and than the stack could take): refused, and
        // a plain `parse` of it renders nothing instead of overflowing.
        let html = deep(8_000);
        assert!(matches!(
            HtmlDocument::try_parse(&html, DocumentOptions::default()),
            Err(RenderError::TooDeep { depth }) if depth > 8_000
        ));
        let mut doc = HtmlDocument::parse(&html, DocumentOptions::default());
        assert_eq!(doc.layout(400.0), 0.0);
        assert!(doc.display_list().commands.is_empty());
    }

    #[test]
    fn valign_attributes_align_cells() {
        // A tall cell and a short one whose content `valign` (on the cell or
        // its row) places within the row.
        let row = |attrs: &str, cell: &str| {
            format!("<!DOCTYPE html><body style=\"margin:0\"><table cellpadding=0 cellspacing=0><tr {attrs}><td><div style=\"height:100px;width:10px\"></div></td><td {cell}><div id=\"c\" style=\"height:20px;width:10px\"></div></td></tr></table>")
        };
        assert_eq!(box_of(&row("", "VALIGN=\"top\""), "c").1, 0.0);
        assert_eq!(box_of(&row("valign=bottom", ""), "c").1, 80.0);
        // No attribute: browsers center cells vertically.
        assert_eq!(box_of(&row("", ""), "c").1, 40.0);
    }

    #[test]
    fn unavailable_images_follow_chromium_fallbacks() {
        // Mirrors Chromium's measured boxes for broken images (see the
        // `replace_unavailable_images` docs).
        let html = "<!DOCTYPE html><body style=\"margin:0;font:16px Arial\">\
            <div id=\"a\"><img src=\"https://t.test/a.png\" alt=\"ACME Logo\" width=\"120\" height=\"40\"></div>\
            <div><img id=\"e\" src=\"https://t.test/e.png\" alt=\"Ratio\" width=\"180\" height=\"126\" style=\"display:block;width:180px;height:auto\"></div>\
            <div><img id=\"d\" src=\"https://t.test/d.png\" width=\"120\" height=\"40\"></div>\
            <div><img id=\"b\" src=\"cid:missing\"></div>\
            <div><img id=\"j\" src=\"https://t.test/j.png\" alt=\"\" style=\"width:50px;height:50px\"></div>\
            <div><img id=\"z\" src=\"https://t.test/z.png\" alt=\"\"></div></body>";
        let mut doc = laid_out(html);
        doc.select_all();
        let text = doc.selected_text().unwrap_or_default();
        assert!(
            text.contains("ACME Logo") && text.contains("Ratio"),
            "{text}"
        );
        let size = |id: &str| box_of(html, id).2..=box_of(html, id).3;
        // Inline with alt: one line of text, declared size ignored.
        let (_, _, _, line) = box_of(html, "a");
        assert!(line < 30.0, "{line}");
        // Block with alt: width kept, height from the attributes' ratio.
        let (_, _, w, h) = box_of(html, "e");
        assert_eq!((w, h.round()), (180.0, 126.0));
        // No alt: framed declared size, or the 16px icon slot.
        assert_eq!(size("d"), 120.0..=40.0);
        assert_eq!(size("b"), 16.0..=16.0);
        assert_eq!(size("j"), 50.0..=50.0);
        // Decorative without size: nothing.
        assert_eq!(size("z"), 0.0..=0.0);
    }

    #[test]
    fn quirks_mode_unavailable_images_keep_declared_sizes() {
        let html = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0 Transitional//EN\"><body style=\"margin:0\">\
            <div><img id=\"c\" src=\"https://t.test/c.png\" alt=\"Sized alt\" width=\"120\" height=\"40\"></div>\
            <div><img id=\"f\" src=\"https://t.test/f.png\" width=\"177\" style=\"height:auto\"></div></body>";
        let size = |id: &str| {
            let (_, _, w, h) = box_of(html, id);
            (w, h)
        };
        assert_eq!(size("c"), (120.0, 40.0));
        // One dimension: a square.
        assert_eq!(size("f"), (177.0, 177.0));
    }

    #[test]
    fn quirks_mode_tables_reset_inherited_fonts() {
        let body = "<body style=\"margin:0;font-size:8pt\"><table><tr><td><span id=\"s\">X</span></td></tr></table>";
        let font_size = |doctype: &str| {
            let mut doc = laid_out(&format!("{doctype}{body}"));
            let list = doc.display_list();
            list.commands
                .iter()
                .find_map(|c| match c {
                    crate::display_list::Command::Text(run) => Some(run.font_size),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(
            font_size("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0 Transitional//EN\">"),
            16.0
        );
        assert!((font_size("<!DOCTYPE html>") - 8.0 * 4.0 / 3.0).abs() < 0.1);
    }

    #[test]
    fn image_align_floats_and_hspace_adds_margins() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div><img id=\"r\" src=\"cid:x\" width=\"50\" height=\"50\" align=\"right\"><img id=\"l\" src=\"cid:y\" width=\"50\" height=\"50\" ALIGN=\"left\" hspace=\"10\">text beside the images</div></body>";
        let (x, y, _, _) = box_of(html, "r");
        assert_eq!((x, y), (350.0, 0.0));
        let (x, y, _, _) = box_of(html, "l");
        assert_eq!((x, y), (10.0, 0.0));
    }

    #[test]
    fn cell_heights_are_minimums() {
        let html = |content: u32| {
            format!("<!DOCTYPE html><body style=\"margin:0\"><table id=\"t\" cellpadding=0 cellspacing=0><tr><td height=\"81\"><div style=\"height:{content}px;width:10px\"></div></td></tr></table>")
        };
        // Taller content grows the row; shorter content keeps the height.
        assert_eq!(box_of(&html(300), "t").3, 300.0);
        assert_eq!(box_of(&html(20), "t").3, 81.0);
    }

    #[test]
    fn legacy_link_and_hr_attributes_apply() {
        let html = "<!DOCTYPE html><body link=\"#ff0000\" style=\"margin:0\"><a href=\"https://x.test\">link</a><hr id=\"h\" align=\"left\" width=\"100\" size=\"6\" color=\"#00ff00\"><hr id=\"z\" size=\"1\"></body>";
        let mut doc = laid_out(html);
        let commands = &doc.display_list().commands;
        let red = peniko::Color::from_rgba8(255, 0, 0, 255);
        let green = peniko::Color::from_rgba8(0, 255, 0, 255);
        let uses = |color| {
            commands.iter().any(|c| match c {
                crate::display_list::Command::Text(run) => run.color == color,
                crate::display_list::Command::Fill {
                    brush: crate::display_list::Brush::Solid(fill),
                    ..
                } => *fill == color,
                _ => false,
            })
        };
        assert!(uses(red), "link colour from <body link>");
        assert!(uses(green), "hr colour");
        // Chromium: 100px content + 1px borders, `size` includes the borders.
        let (x, _, w, h) = box_of(html, "h");
        assert_eq!((x, w, h), (0.0, 102.0, 6.0));
        assert_eq!(box_of(html, "z").3, 1.0);
    }

    #[test]
    fn collapsed_borders_are_counted_once_per_row() {
        // 1px collapsed borders: rows are 20px of content + one shared border.
        let cell =
            "<td style=\"border:1px solid green;padding:0\"><div style=\"height:20px\"></div></td>";
        let html = format!("<!DOCTYPE html><body style=\"margin:0\"><table id=\"t\" style=\"border-collapse:collapse\" cellspacing=0><tr>{cell}</tr><tr>{cell}</tr></table>");
        assert_eq!(box_of(&html, "t").3, 43.0);
    }

    #[test]
    fn missing_families_fall_back_to_the_default_serif_font() {
        let width = |family: &str| {
            let html = format!("<!DOCTYPE html><body style=\"margin:0\"><span id=\"s\" style=\"font-family:{family};font-size:20px\">Mailbox</span>");
            box_of(&html, "s").2
        };
        assert_eq!(width("NoSuchFamilyXyz"), width("serif"));
    }

    #[test]
    fn markers_align_with_the_first_line_of_a_block_item() {
        let mut doc = HtmlDocument::parse(
            "<!DOCTYPE html><body style=\"margin:0\"><ul style=\"margin:0;font-size:16px\"><li><p style=\"margin:0;line-height:40px\">item</p></li></ul>",
            DocumentOptions::default(),
        );
        doc.layout(400.0);
        let disc_bottom = doc
            .display_list()
            .commands
            .iter()
            .find_map(|c| match c {
                crate::display_list::Command::Fill {
                    geometry: crate::display_list::Geometry::RoundedRect(r, _),
                    ..
                } if (r.width() - 5.0).abs() < 0.01 => Some(r.y1),
                _ => None,
            })
            .expect("disc marker");
        // The paragraph's baseline is ~25px down; the disc sits 2px above it.
        assert!((21.0..25.0).contains(&disc_bottom), "{disc_bottom}");
    }

    #[test]
    fn symbol_list_markers_are_drawn_as_shapes() {
        let mut doc = HtmlDocument::parse(
            "<!DOCTYPE html><ul style=\"font-size:16px\"><li>item</li></ul>",
            DocumentOptions::default(),
        );
        doc.layout(400.0);
        let list = doc.display_list();
        let disc = list.commands.iter().any(|c| matches!(c,
            crate::display_list::Command::Fill { geometry: crate::display_list::Geometry::RoundedRect(r, _), .. }
                if (r.width() - 5.0).abs() < 0.01 && (r.height() - 5.0).abs() < 0.01));
        assert!(disc, "expected a 5px disc marker");
        let glyph_bullet = list.commands.iter().any(|c| {
            matches!(c,
            crate::display_list::Command::Text(run) if run.words.iter().any(|w| w.text == "•"))
        });
        assert!(!glyph_bullet);
    }

    const QUIRKS: &str = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0 Transitional//EN\">";

    #[test]
    fn line_height_quirk_drops_the_strut_of_text_free_boxes() {
        let height = |doctype: &str, content: &str| {
            let html = format!(
                "{doctype}<body style=\"margin:0\"><p id=\"p\" style=\"margin:0\">{content}</p>"
            );
            box_of(&html, "p").3
        };
        let small = "<font size=\"1\">x</font>";
        // Quirks: the line is as tall as the small text, not the paragraph's strut.
        assert!(height(QUIRKS, small) < height("<!DOCTYPE html>", small));
        // Text-free ancestors of the text don't count either.
        let nested = "<em><span><font size=\"1\">x</font></span></em>";
        assert_eq!(height(QUIRKS, nested), height(QUIRKS, small));
        // `&nbsp;` is text: it keeps the paragraph's line height.
        assert_eq!(height(QUIRKS, "&nbsp;"), height(QUIRKS, "x"));
    }

    #[test]
    fn breaks_alone_on_their_line_keep_the_parent_line_height() {
        let height = |content: &str| {
            let html = format!(
                "{QUIRKS}<body style=\"margin:0\"><p id=\"p\" style=\"margin:0\">{content}</p>"
            );
            box_of(&html, "p").3
        };
        let small = height("<font size=\"1\">x</font>");
        // A break after text adds nothing; one alone on its line adds a full line.
        assert_eq!(
            height("<font size=\"1\">x</font><br><font size=\"1\">y</font>"),
            2.0 * small
        );
        assert!(height("<br><font size=\"1\">y</font>") > 2.0 * small);
    }

    #[test]
    fn quirky_margins_drop_at_the_edges_of_cells() {
        let cell = |content: &str| {
            let html = format!("{QUIRKS}<body style=\"margin:0\"><table cellpadding=0 cellspacing=0><tr><td id=\"c\">{content}</td></tr></table>");
            box_of(&html, "c").3
        };
        let line = cell("a");
        // A cell's leading and closing paragraphs lose their default margins,
        // through wrappers and floats too.
        assert_eq!(cell("<p>a</p>"), line);
        assert_eq!(cell("<font><!-- c --><p>a</p></font>"), line);
        assert_eq!(
            cell("<img src=\"cid:x\" width=\"5\" height=\"5\" align=\"left\"><p>a</p>"),
            line
        );
        // Text before the paragraph keeps its top margin; a list keeps its bottom one.
        assert!(cell("x<p>a</p>") > 2.0 * line);
        assert!(cell("<ul><li>a</li></ul>") > line);
        // Author margins stay.
        assert!(cell("<p style=\"margin-top:10px\">a</p>") > line);
        // Standards mode keeps every margin.
        let html = "<!DOCTYPE html><body style=\"margin:0\"><table cellpadding=0 cellspacing=0><tr><td id=\"c\"><p>a</p></td></tr></table>";
        assert!(box_of(html, "c").3 > line);
    }

    #[test]
    fn quirks_mode_parsing_keeps_tables_inside_paragraphs() {
        let p = |doctype: &str| {
            let html = format!("{doctype}<body style=\"margin:0\"><p id=\"p\">a<table><tr><td><div style=\"height:50px\"></div></td></tr></table>b</p>");
            box_of(&html, "p").3
        };
        assert!(p(QUIRKS) > 50.0);
        assert!(p("<!DOCTYPE html>") < 50.0);
    }

    #[test]
    fn tiny_and_floated_unavailable_images_match_chromium() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div style=\"width:300px\">\
            <img id=\"t\" src=\"https://t.test/t.gif\" width=\"1\" height=\"1\">\
            <img id=\"f\" src=\"https://t.test/f.gif\" width=\"66\" height=\"86\" align=\"left\" hspace=\"15\" vspace=\"5\" alt=\"Wayne\">text</div>";
        let (_, _, w, h) = box_of(html, "t");
        assert_eq!((w, h), (1.0, 1.0));
        let doc = laid_out(html);
        let blitz = doc.blitz();
        // The alt-text replacement of the floated image keeps its box and spacing.
        let span = blitz
            .get_node(blitz.get_element_by_id("f").unwrap())
            .unwrap();
        let p = span.absolute_position(0.0, 0.0);
        let size = span.final_layout().size;
        assert_eq!((p.x, p.y, size.width, size.height), (15.0, 5.0, 66.0, 86.0));
    }

    #[test]
    fn table_sizes_are_minimums() {
        let html = "<!DOCTYPE html><body style=\"margin:0\">\
            <table id=\"h\" height=\"40\" cellpadding=0 cellspacing=0><tr><td height=\"50\">a</td></tr><tr><td height=\"20\">b</td></tr></table>\
            <table id=\"w\" width=\"100\" cellpadding=0 cellspacing=0><tr><td><div style=\"width:200px;height:1px\"></div></td></tr></table>";
        assert_eq!(box_of(html, "h").3, 70.0);
        assert_eq!(box_of(html, "w").2, 200.0);
    }

    #[test]
    fn fixed_tables_cap_padded_percentage_columns() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><table style=\"width:300px;table-layout:fixed\" cellpadding=0 cellspacing=0><tr>\
            <td id=\"c\" style=\"width:100%;padding:0 60px\">a</td><td>b</td></tr></table>";
        assert_eq!(box_of(html, "c").2, 300.0);
    }

    #[test]
    fn spanning_cells_widen_fixed_columns_first() {
        let html = |first: &str, second: &str| {
            format!("<!DOCTYPE html><body style=\"margin:0\"><table cellpadding=0 cellspacing=0><tr>\
                <td id=\"a\"{first}><div style=\"width:5px;height:1px\"></div></td><td id=\"b\"{second}>x</td></tr>\
                <tr><td colspan=\"2\"><div style=\"width:300px;height:1px\"></div></td></tr></table>")
        };
        // An auto column takes the excess; a fixed one keeps its width.
        let auto_fixed = html("", " width=\"50\"");
        assert_eq!(
            (box_of(&auto_fixed, "a").2, box_of(&auto_fixed, "b").2),
            (250.0, 50.0)
        );
        // Only fixed columns: in proportion to their widths.
        let fixed = html(" width=\"100\"", " width=\"50\"");
        assert_eq!(
            (box_of(&fixed, "a").2, box_of(&fixed, "b").2),
            (200.0, 100.0)
        );
    }

    #[test]
    fn short_fixed_columns_share_the_whole_table() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><table id=\"t\" width=\"300\" cellpadding=0 cellspacing=0><tr>\
            <td id=\"a\" width=\"300\">a</td><td id=\"b\" width=\"5\"><div style=\"width:5px\"></div></td><td id=\"c\" width=\"100\">c</td></tr></table>";
        let total = box_of(html, "a").2 + box_of(html, "b").2 + box_of(html, "c").2;
        assert_eq!(total, box_of(html, "t").2);
        // A column already at its width loses nothing.
        assert_eq!(box_of(html, "b").2, 5.0);
    }

    #[test]
    fn inline_elements_ignore_width_around_blocks() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div style=\"width:120px\"><a id=\"a\" style=\"width:200px\"><div>x</div></a></div>";
        assert_eq!(box_of(html, "a").2, 120.0);
    }

    #[test]
    fn cell_vertical_align_does_not_grow_lines() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div id=\"d\">a<br>b</div><table cellpadding=0 cellspacing=0><tr><td id=\"c\">a<br>b</td></tr></table>";
        assert_eq!(box_of(html, "c").3, box_of(html, "d").3);
    }

    #[test]
    fn empty_inline_blocks_collapse_their_margins() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div id=\"a\"><p>a</p><p><font></font></p></div><div id=\"b\"><p>a</p><p></p></div>";
        assert_eq!(box_of(html, "a").3, box_of(html, "b").3);
        assert_eq!(box_of(html, "a").2, 400.0);
    }

    #[test]
    fn form_controls_take_chromium_sizes() {
        let html = "<!DOCTYPE html><body style=\"margin:0\">\
            <div><input id=\"i10\" size=\"10\"></div><div><input id=\"i\"></div>\
            <div><select id=\"s\"><option>a</option></select></div>\
            <div><select id=\"l\"><option>a</option><option selected>A much longer option</option></select></div>\
            <div><input id=\"r\" type=\"radio\"></div>";
        // `size` characters of the 13.33px control font, plus border and padding.
        assert!((box_of(html, "i10").2 - 98.0).abs() < 1.0);
        assert!((box_of(html, "i").2 - 158.0).abs() < 1.0);
        // A drop-down is as wide as its widest option plus the arrow.
        let (_, _, short, height) = box_of(html, "s");
        assert!(height > 10.0 && short > 22.0);
        assert!(box_of(html, "l").2 > short + 50.0);
        assert_eq!(box_of(html, "r").2, 13.0);
    }
}
