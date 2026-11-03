//! A parsed, styled and laid-out HTML document, independent of any window.

use crate::contrast::{adapt_text_contrast, DarkPage};
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
use kurbo::Rect;
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

/// Most tags a document can have, by a scan that needs no parsing (see [`has_too_many_tags`]);
/// the sanitizer has a stack entry per open tag, about 30 KB per 1000. And most attributes of a
/// tag: removing them and rejecting duplicates are quadratic in a tag's.
const MAX_OPENING_TAGS: usize = 500_000;
pub const MAX_TAG_ATTRIBUTES: usize = 1024;

/// Longest a first recording of a document can take in [`HtmlDocument::prepare_within`].
const MAX_RECORDING_TIME: std::time::Duration = std::time::Duration::from_secs(2);

/// Most table cells that `colspan`, `rowspan` and `<col span>` can add to a document. A few
/// bytes declare them, and each costs memory in the layout's grid.
pub const MAX_TABLE_SPAN_AREA: u64 = 2_000_000;

/// Why a document can't be rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// Elements nest deeper than [`MAX_NESTING_DEPTH`].
    TooDeep { depth: usize },
    /// The document costs too much memory to lay out, whatever its size in bytes: past
    /// a budget of the parser (see [`blitz_dom::MAX_DOM_NODES`]) or [`MAX_TABLE_SPAN_AREA`].
    TooComplex,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooDeep { depth } => write!(
                f,
                "elements nest {depth} levels deep (limit {MAX_NESTING_DEPTH})"
            ),
            Self::TooComplex => write!(f, "the document is too complex to lay out"),
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

/// An inline `style` without the declarations that size or lay out a box
/// (`width`, `height`, their min/max, `display`, `aspect-ratio`).
fn without_box_size(style: &str) -> String {
    const BOX: [&str; 8] = [
        "width",
        "height",
        "min-width",
        "max-width",
        "min-height",
        "max-height",
        "display",
        "aspect-ratio",
    ];
    style
        .split(';')
        .filter(|declaration| {
            let property = declaration.split(':').next().unwrap_or("").trim();
            !BOX.iter().any(|name| property.eq_ignore_ascii_case(name))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Mail has no business asking for input: form controls never exist in a document, whatever
/// reaches the parser. Hosts sanitize them away too, but this holds if that ever fails, and
/// Blitz's code for them (where clicking a label once overflowed the stack) stays unreachable.
fn remove_form_controls(doc: &mut BaseDocument) {
    const CONTROLS: &[&str] = &["input", "textarea", "select", "button", "form"];
    let mut found = Vec::new();
    let mut stack = vec![doc.root_node().id];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        if node
            .element_data()
            .is_some_and(|element| CONTROLS.contains(&&*element.name.local))
        {
            found.push(id);
        } else {
            stack.extend(node.children.iter().copied());
        }
    }
    let mut mutator = doc.mutate();
    for id in found {
        mutator.remove_and_drop_node(id);
    }
}

/// Shows unavailable images (blocked remote, missing `cid:`) the way
/// Chromium does, so mail laid out around images keeps its shape:
/// - with alt text: the alt text after a 16px icon slot, inline (declared
///   sizes ignored), or as a block keeping the width and the attributes'
///   aspect ratio when the image is `display:block`;
/// - without alt text (or `alt=""`): a box of the declared size, framed only
///   from 18px each way (smaller ones, like spacer GIFs, show nothing), or
///   the unframed 16px icon slot when a dimension is missing (not for
///   `alt=""`).
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
            /// Whether the broken-image icon precedes the text.
            icon: bool,
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
                // Chromium frames a quirks-mode box and shows its icon only
                // when the icon fits (18px each way); smaller boxes show just
                // the clipped alt text.
                let (w, h) = (width.unwrap_or(16.0), height.unwrap_or(16.0));
                let icon = !quirks_box || (w >= 18.0 && h >= 18.0);
                let style = if quirks_box {
                    let display = if compact.contains("display:block") {
                        "block"
                    } else {
                        "inline-block"
                    };
                    let frame = if icon { IMAGE_FRAME } else { "" };
                    format!(
                        "{own};display:{display};overflow:hidden;width:{w}px;height:{h}px;\
                         vertical-align:bottom;{frame}{align}"
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
                    // Inline alt text ignores the image's size but keeps the
                    // rest of its style (`font-size:16px` under a `font-size:0`
                    // parent).
                    format!("{};{align}", without_box_size(&own))
                };
                Fallback::AltText {
                    alt: alt.to_string(),
                    style,
                    attrs,
                    icon,
                }
            }
            _ => match (width, height) {
                // Chromium frames the box only when its icon fits (18px each
                // way): spacer GIFs show nothing.
                (Some(w), Some(h)) if w >= 18.0 && h >= 18.0 => {
                    Fallback::Restyle(format!("{own};width:{w}px;height:{h}px;{IMAGE_FRAME}"))
                }
                (Some(w), Some(h)) => Fallback::Restyle(format!(
                    "{own};width:{w}px;height:{h}px;box-sizing:border-box"
                )),
                // A block keeps its width, 16px tall (full-width color bars).
                (Some(w), None) if alt.is_none() && compact.contains("display:block") => {
                    Fallback::Restyle(format!(
                        "{own};width:{w}px;height:16px;box-sizing:border-box"
                    ))
                }
                // The bare 16px icon slot (Chromium draws the icon, unframed).
                _ if alt.is_none() => Fallback::Restyle(format!(
                    "{own};width:16px;height:16px;box-sizing:border-box"
                )),
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
                icon,
            } => {
                if !style.is_empty() {
                    attrs.push(blitz_dom::Attribute {
                        name: blitz_dom::qual_name!("style"),
                        value: style,
                    });
                }
                let span = mutator.create_element(blitz_dom::qual_name!("span", html), attrs);
                let text = mutator.create_text_node(&alt);
                if icon {
                    let icon = mutator.create_element(
                        blitz_dom::qual_name!("span", html),
                        vec![blitz_dom::Attribute {
                            name: blitz_dom::qual_name!("style"),
                            value: IMAGE_ICON.to_string(),
                        }],
                    );
                    mutator.append_children(span, &[icon, text]);
                } else {
                    mutator.append_children(span, &[text]);
                }
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

/// Whether `html` has more tags, or an end tag more attributes, than hosts should sanitize and
/// parse: a linear scan to run before both. It counts everything that could be a tag, wherever it
/// is, so no state another parser is in can hide one; it only overstates. Start tags with too many
/// attributes are the sanitizer's to drop; it can't see those of end tags, which the parser's
/// tokenizer still compares with each other.
pub fn has_too_many_tags(html: &str) -> bool {
    let bytes = html.as_bytes();
    // Each end tag is read on its own, so one can't hide the next in its quotes; this keeps
    // that linear, for `</a b="</a b="...`.
    let mut budget = bytes.len().saturating_mul(16).saturating_add(1 << 20);
    let mut tags = 0;
    let mut at = 0;
    while let Some(found) = bytes[at..].iter().position(|&byte| byte == b'<') {
        at += found + 1;
        let closing = bytes.get(at) == Some(&b'/');
        let name_at = at + usize::from(closing);
        if !bytes.get(name_at).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        tags += 1;
        if tags > MAX_OPENING_TAGS {
            return true;
        }
        if closing {
            let (attributes, read) = end_tag_attributes(&bytes[name_at..]);
            budget = budget.saturating_sub(read);
            if attributes > MAX_TAG_ATTRIBUTES || budget == 0 {
                return true;
            }
        }
    }
    false
}

/// Attributes of the end tag whose name starts `bytes`, as the tokenizer reads them, and how much
/// it read to tell: it stops past [`MAX_TAG_ATTRIBUTES`].
fn end_tag_attributes(bytes: &[u8]) -> (usize, usize) {
    #[derive(Clone, Copy)]
    enum State {
        TagName,
        BeforeName,
        Name,
        AfterName,
        BeforeValue,
        Quoted(u8),
        Unquoted,
    }
    let mut state = State::TagName;
    let mut attributes = 0;
    for (at, &byte) in bytes.iter().enumerate() {
        let space = byte.is_ascii_whitespace();
        state = match state {
            State::Quoted(quote) if byte == quote => State::BeforeName,
            State::Quoted(quote) => State::Quoted(quote),
            _ if byte == b'>' => return (attributes, at),
            State::TagName if space || byte == b'/' => State::BeforeName,
            State::TagName => State::TagName,
            State::BeforeName if space || byte == b'/' => State::BeforeName,
            State::AfterName if space => State::AfterName,
            State::Name | State::AfterName if byte == b'/' => State::BeforeName,
            State::Name | State::AfterName if byte == b'=' => State::BeforeValue,
            State::Name if space => State::AfterName,
            State::Name => State::Name,
            State::BeforeName | State::AfterName => {
                attributes += 1;
                State::Name
            }
            State::BeforeValue if space => State::BeforeValue,
            State::BeforeValue if byte == b'"' || byte == b'\'' => State::Quoted(byte),
            State::BeforeValue => State::Unquoted,
            State::Unquoted if space => State::BeforeName,
            State::Unquoted => State::Unquoted,
        };
        if attributes > MAX_TAG_ATTRIBUTES {
            return (attributes, at);
        }
    }
    (attributes, bytes.len())
}

/// Runs `build` on a thread with the stack [`MAX_NESTING_DEPTH`] assumes. Style and layout
/// recurse per level, and pool threads have less: 512 KB on macOS, too little for 160 levels.
pub fn with_render_stack<T: Send>(build: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("bgmail-render".into())
            .stack_size(8 << 20)
            .spawn_scoped(scope, build)
            .expect("the system can't start a thread")
            .join()
            .expect("the render thread panicked")
    })
}

/// The cells the spans of `doc`'s tables add (see [`MAX_TABLE_SPAN_AREA`]).
fn table_span_area(doc: &BaseDocument) -> u64 {
    let mut area = 0u64;
    let mut stack = vec![doc.root_node().id];
    while let Some(id) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        stack.extend(node.children.iter().copied());
        let Some(element) = node.element_data() else {
            continue;
        };
        // Whatever the element: any can be a cell or a column by its style, unknown here.
        let span = |name: &str| {
            element
                .attrs()
                .iter()
                .find(|attr| &*attr.name.local == name)
                .and_then(|attr| attr.value.trim().parse::<u64>().ok())
                .unwrap_or(1)
                .clamp(1, u16::MAX as u64)
        };
        let cells = span("colspan") * span("rowspan") * span("span");
        if cells > 1 {
            area += cells;
        }
    }
    area
}

/// No animations: nothing here runs their clock, so they never end, and each style pass redid
/// every element with one (8000 of them took 2.3 s per pass). A message must read without them, as
/// many clients have none. Only where they can be, as this costs 15-20% of a layout (see
/// [`may_animate`]).
const NO_ANIMATION_UA_CSS: &str = "*, *::before, *::after { animation: none !important; \
     transition: none !important; }";

/// Whether `html` can declare animations or transitions, including through what hides the words.
fn may_animate(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    ["keyframes", "transition", "@import", "\\"]
        .iter()
        .any(|word| lower.contains(word))
}

/// Chromium UA rules Blitz's default sheet lacks:
/// - a plain `<hr>` is painted as a light #9a9a9a/#eeeeee bevel whatever its
///   color (`noshade` and `color` rules come from presentational hints);
/// - form control metrics (its UA sheet and theme), which the controls of
///   old newsletters' search boxes size table columns by: a 13.33px UI font,
///   text inputs with 3px of border + padding around their text, closed
///   drop-downs with room for the arrow at their end, 13px check boxes and
///   radio buttons.
const CHROMIUM_UA_CSS: &str = "input, select, textarea, button { \
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
     input[type=radio i] { margin: 3px 3px 0 5px; } \
     hr:not([noshade]):not([color]) { border-color: #9a9a9a #eeeeee #eeeeee #9a9a9a; \
     border-style: solid; }";

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
    /// The page is dark: text unreadable on what is behind it is adapted.
    pub dark_page: Option<DarkPage>,
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

/// Box closest to `point`, in reading terms: first the one on its line, then the
/// nearest on that line, so columns and table cells pick the right neighbor.
fn nearest_box<T: Copy>(boxes: &[(T, Rect)], point: kurbo::Point) -> Option<(T, Rect)> {
    let gap = |low: f64, high: f64, value: f64| (low - value).max(value - high).max(0.0);
    boxes
        .iter()
        .min_by(|(_, a), (_, b)| {
            let distance = |rect: &Rect| {
                (
                    gap(rect.y0, rect.y1, point.y),
                    gap(rect.x0, rect.x1, point.x),
                )
            };
            distance(a)
                .partial_cmp(&distance(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
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
    dark_page: Option<DarkPage>,
    /// Base URL links resolve against.
    base_url: url::Url,
    /// Links activated by clicks, for the host to open.
    links: Arc<LinkCapture>,
    /// Matches of the last [`HtmlDocument::find`]: (inline root, byte range).
    find_matches: Vec<(NodeId, Range<usize>)>,
    /// Element nesting depth of the parsed tree.
    depth: usize,
    /// Page zoom: CSS px are drawn this many pixels wide (see `set_zoom`).
    zoom: f32,
    /// A selection is being dragged, over these boxes of text.
    selection_dragging: bool,
    selection_boxes: Vec<(NodeId, Rect)>,
}

/// A document on its way from a background thread to the thread that shows it,
/// so large messages don't stall the window while they are parsed.
pub struct DetachedDocument(HtmlDocument);

// SAFETY: asserting `HtmlDocument: Send` fails only on Stylo's
// `Box<dyn FontMetricsProvider>`, a trait object without a `Send` bound. Its only
// implementor is Blitz's `BlitzFontMetricsProvider`, which `vendor/blitz` asserts
// to be `Send` at compile time. The document is moved whole and used by one thread
// at a time, never shared.
unsafe impl Send for DetachedDocument {}

impl DetachedDocument {
    pub fn new(document: HtmlDocument) -> Self {
        Self(document)
    }

    pub fn into_inner(self) -> HtmlDocument {
        self.0
    }
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
            UserAgentStylesheet::parse(CHROMIUM_UA_CSS),
        ];
        if mode == DocumentMode::Quirks {
            ua_stylesheets.push(UserAgentStylesheet::parse(QUIRKS_UA_CSS));
        }
        if may_animate(html) {
            ua_stylesheets.push(UserAgentStylesheet::parse(NO_ANIMATION_UA_CSS));
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
            viewport: Some(viewport(800, options.appearance, 1.0)),
            ua_stylesheets,
            font_ctx: Some(crate::fonts::font_context()),
            ..Default::default()
        };
        let mut inner = blitz_html::HtmlDocument::from_html(
            &force_html_parse(&repair_c1_controls(html), mode),
            config,
        );
        let base = url::Url::parse(&base_url)
            .unwrap_or_else(|_| url::Url::parse(MESSAGE_BASE_URL).expect("valid base URL"));
        remove_form_controls(&mut inner);
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
            dark_page: options.dark_page,
            base_url: base,
            links,
            find_matches: Vec::new(),
            depth: 0,
            zoom: 1.0,
            selection_dragging: false,
            selection_boxes: Vec::new(),
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
        if document.inner.parse_refused {
            return Err(RenderError::TooComplex);
        }
        if table_span_area(&document.inner) > MAX_TABLE_SPAN_AREA {
            return Err(RenderError::TooComplex);
        }
        Ok(document)
    }

    /// Does the costly first layout and recording for `width` CSS px up front,
    /// so a document built on a background thread is cheap to show.
    pub fn prepare(&mut self, width: f32) {
        self.layout(width);
        self.display_list();
    }

    /// [`Self::prepare`] that gives up on a layout passing `limit`, returning `false`: the document
    /// is then incomplete, and hosts show what they can instead. The last resort for what no other
    /// limit caught: a hang can't be stopped from outside, and a partial message would hide its end.
    pub fn prepare_within(&mut self, width: f32, limit: std::time::Duration) -> bool {
        self.inner.layout_deadline = Some(std::time::Instant::now() + limit);
        self.layout(width);
        self.inner.layout_deadline = None;
        if self.inner.layout_timed_out {
            return false;
        }
        // Recording too, at most what a repaint can take, as hovering repeats it on the UI thread.
        let recording = std::time::Instant::now();
        self.display_list();
        recording.elapsed() <= limit.min(MAX_RECORDING_TIME)
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
            self.inner
                .set_viewport(viewport(width, self.appearance, self.zoom));
            self.layout_width = Some(width);
            self.display_list = None;
        }
        // Resolve until resources delivered during resolution are applied.
        for _ in 0..4 {
            self.inner.resolve(0.0);
            // Styles ask for background images when resolved, and those of the message itself
            // are already here: recorded without them, a long message never showed them.
            if self.inner.handle_messages() {
                self.display_list = None;
            } else if !self.inner.has_pending_critical_resources() {
                break;
            }
        }
        let root = self.inner.root_element();
        // Layout is in CSS px; the host's coordinates are zoomed pixels.
        let zoom = self.zoom;
        self.content_height = root.final_layout().size.height.max(root.scroll_height()) * zoom;
        // Mail wider than the pane (fixed-width tables) overflows to the right
        // and scrolls horizontally, as in a browser; layout keeps the pane width.
        self.content_width = (width as f32).max(overflow_right(&self.inner, root.id, 0.0) * zoom);
        self.content_height
    }

    /// Page zoom, like a browser's: the document lays out `1 / zoom` as many
    /// CSS px across the pane and draws everything `zoom` times larger, so
    /// text reflows instead of overflowing. Every coordinate this type takes
    /// or returns (layout width, content size, pointer input, match lines)
    /// stays in pixels. Clamped to 25%–500%; takes effect at the next
    /// [`Self::layout`].
    pub fn set_zoom(&mut self, zoom: f32) {
        let zoom = if zoom.is_finite() {
            zoom.clamp(0.25, 5.0)
        } else {
            1.0
        };
        if zoom != self.zoom {
            self.zoom = zoom;
            self.layout_width = None;
            self.display_list = None;
        }
    }

    /// The page zoom (1 = 100%).
    pub fn zoom(&self) -> f32 {
        self.zoom
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
            self.zoom as f64,
            width,
            height as u32,
            0,
            0,
            self.selection_color,
        );
        if let Some(page) = self.dark_page {
            adapt_text_contrast(&mut list, page);
        }
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
        let input = PointerInput {
            x: input.x / self.zoom,
            y: input.y / self.zoom,
            ..input
        };
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
        self.follow_selection_drag(&input, pressed);
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

    /// Selection by dragging, as in a browser: it starts and follows the pointer
    /// wherever it is, using the nearest text when not over any, where Blitz would
    /// neither start nor extend it.
    fn follow_selection_drag(&mut self, input: &PointerInput, pressed: bool) {
        match input.phase {
            PointerPhase::Down { click_count } if pressed && click_count < 2 => {
                self.selection_boxes = self.text_boxes();
                match self.nearest_text_position(input.x, input.y) {
                    Some((root, offset)) => {
                        self.inner.set_text_selection(root, offset, root, offset)
                    }
                    None => self.inner.clear_text_selection(),
                }
                self.selection_dragging = true;
            }
            PointerPhase::Move if self.selection_dragging && input.primary_pressed => {
                if let Some((root, offset)) = self.nearest_text_position(input.x, input.y) {
                    self.inner.update_selection_focus(root, offset);
                }
            }
            PointerPhase::Up | PointerPhase::Move => {
                self.selection_dragging = false;
                self.selection_boxes.clear();
            }
            PointerPhase::Down { .. } => {}
        }
    }

    /// Where each block of text is in the document: (inline root, box).
    fn text_boxes(&self) -> Vec<(NodeId, Rect)> {
        let mut roots = Vec::new();
        inline_roots(&self.inner, self.inner.root_element().id, &mut roots);
        roots
            .into_iter()
            .filter(|root| {
                self.root_text(*root)
                    .is_some_and(|text| !text.trim().is_empty())
            })
            .filter_map(|root| {
                let node = self.inner.get_node(root)?;
                let origin = node.absolute_position(0.0, 0.0);
                let size = node.final_layout().size;
                (size.width > 0.0 && size.height > 0.0).then(|| {
                    (
                        root,
                        Rect::new(
                            f64::from(origin.x),
                            f64::from(origin.y),
                            f64::from(origin.x + size.width),
                            f64::from(origin.y + size.height),
                        ),
                    )
                })
            })
            .collect()
    }

    /// The text position under a point, or the closest one to it.
    fn nearest_text_position(&self, x: f32, y: f32) -> Option<(NodeId, usize)> {
        if let Some(position) = self.inner.find_text_position(x, y) {
            return Some(position);
        }
        let point = kurbo::Point::new(f64::from(x), f64::from(y));
        let (root, rect) = nearest_box(&self.selection_boxes, point)?;
        let node = self.inner.get_node(root)?;
        let offset = node.text_offset_at_point(
            (point.x.clamp(rect.x0, rect.x1) - rect.x0) as f32,
            (point.y.clamp(rect.y0, rect.y1) - rect.y0) as f32,
        )?;
        Some((root, offset))
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
        let Some(hit) = self.inner.hit(x / self.zoom, y / self.zoom) else {
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
                        .map(|url| {
                            crate::resources::protocol_relative_web_url(&url)
                                .unwrap_or(url)
                                .to_string()
                        });
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
        let zoom = self.zoom;
        Some((
            (top + caret.y0 as f32 / scale) * zoom,
            (top + caret.y1 as f32 / scale) * zoom,
        ))
    }

    /// The document's remote loads (only with
    /// [`Resources::allow_remote`]): hosts re-lay out and repaint while they
    /// arrive.
    pub fn remote_loads(&self) -> Arc<crate::remote::RemoteLoads> {
        self.resources.remote_loads().clone()
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

/// A Blitz viewport `width` pixels wide at `zoom` (Blitz's scale factor:
/// layout gets `width / zoom` CSS px).
fn viewport(width: u32, appearance: Appearance, zoom: f32) -> Viewport {
    let scheme = match appearance {
        Appearance::Light => ColorScheme::Light,
        Appearance::Dark => ColorScheme::Dark,
    };
    let height = (VIEWPORT_HEIGHT as f32 * zoom).round() as u32;
    Viewport::new(width, height, zoom, scheme)
}

/// Windows-1252 characters for the C1 controls U+0080–U+009F (`None` where
/// windows-1252 leaves the byte undefined).
const WINDOWS_1252_C1: [Option<char>; 32] = [
    Some('€'),
    None,
    Some('‚'),
    Some('ƒ'),
    Some('„'),
    Some('…'),
    Some('†'),
    Some('‡'),
    Some('ˆ'),
    Some('‰'),
    Some('Š'),
    Some('‹'),
    Some('Œ'),
    None,
    Some('Ž'),
    None,
    None,
    Some('‘'),
    Some('’'),
    Some('“'),
    Some('”'),
    Some('•'),
    Some('–'),
    Some('—'),
    Some('˜'),
    Some('™'),
    Some('š'),
    Some('›'),
    Some('œ'),
    None,
    Some('ž'),
    Some('Ÿ'),
];

/// Text never holds C1 controls: they come from Windows-1252 mail decoded as
/// strict ISO-8859-1 (browsers decode that label as windows-1252), so they
/// become the characters the sender typed (curly quotes, dashes, `€`).
fn repair_c1_controls(html: &str) -> std::borrow::Cow<'_, str> {
    if !html.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)) {
        return std::borrow::Cow::Borrowed(html);
    }
    html.chars()
        .map(|c| match c {
            '\u{80}'..='\u{9f}' => WINDOWS_1252_C1[c as usize - 0x80].unwrap_or(c),
            _ => c,
        })
        .collect::<String>()
        .into()
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

    fn drag(document: &mut HtmlDocument, x: f32, y: f32, phase: PointerPhase) {
        document.pointer(PointerInput {
            x,
            y,
            phase,
            button: PointerButton::Primary,
            primary_pressed: phase != PointerPhase::Up,
            modifiers: Default::default(),
        });
    }

    const SPACED: &str = "<body style=\"margin:0\"><p style=\"margin:0 0 200px 100px\">First</p>\
        <p style=\"margin:0 0 0 100px\">Second</p></body>";

    #[test]
    fn a_drag_selects_from_blank_space_to_blank_space() {
        let mut document = HtmlDocument::parse(SPACED, DocumentOptions::default());
        document.prepare(600.0);
        // From the empty left margin of the first line...
        drag(
            &mut document,
            10.0,
            8.0,
            PointerPhase::Down { click_count: 1 },
        );
        // ...through the gap between the paragraphs...
        drag(&mut document, 300.0, 60.0, PointerPhase::Move);
        assert_eq!(document.selected_text().unwrap().trim(), "First");
        // ...to past the end of the document.
        drag(&mut document, 590.0, 5000.0, PointerPhase::Move);
        drag(&mut document, 590.0, 5000.0, PointerPhase::Up);
        let text = document.selected_text().unwrap();
        assert!(
            text.contains("First") && text.contains("Second"),
            "{text:?}"
        );
    }

    #[test]
    fn a_drag_upward_selects_too() {
        let mut document = HtmlDocument::parse(SPACED, DocumentOptions::default());
        document.prepare(600.0);
        drag(
            &mut document,
            590.0,
            5000.0,
            PointerPhase::Down { click_count: 1 },
        );
        drag(&mut document, 0.0, 0.0, PointerPhase::Move);
        let text = document.selected_text().unwrap();
        assert!(
            text.contains("First") && text.contains("Second"),
            "{text:?}"
        );
    }

    #[test]
    fn moving_without_a_press_selects_nothing() {
        let mut document = HtmlDocument::parse(SPACED, DocumentOptions::default());
        document.prepare(600.0);
        drag(
            &mut document,
            10.0,
            8.0,
            PointerPhase::Down { click_count: 1 },
        );
        drag(&mut document, 10.0, 8.0, PointerPhase::Up);
        document.pointer(PointerInput {
            x: 590.0,
            y: 5000.0,
            phase: PointerPhase::Move,
            button: PointerButton::Primary,
            primary_pressed: false,
            modifiers: Default::default(),
        });
        assert!(!document.has_selection());
    }

    #[test]
    fn nearest_box_prefers_the_same_line() {
        let a = 1;
        let b = 2;
        let boxes = [
            (a, Rect::new(0.0, 0.0, 100.0, 20.0)),
            (b, Rect::new(300.0, 0.0, 400.0, 20.0)),
        ];
        let near = |x, y| nearest_box(&boxes, kurbo::Point::new(x, y)).unwrap().0;
        assert_eq!(near(150.0, 10.0), a);
        assert_eq!(near(280.0, 10.0), b);
        assert!(nearest_box::<u8>(&[], kurbo::Point::ZERO).is_none());
    }

    #[test]
    fn a_document_built_on_another_thread_works_here() {
        let detached = std::thread::spawn(|| {
            let mut document =
                HtmlDocument::parse("<p>Hello there</p>", DocumentOptions::default());
            document.prepare(400.0);
            DetachedDocument::new(document)
        })
        .join()
        .expect("worker");
        let mut document = detached.into_inner();
        assert!(document.content_height() > 0.0);
        assert!(document.layout(200.0) > 0.0);
        assert_eq!(document.find("hello"), 1);
    }
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
    pub(super) fn box_of(html: &str, id: &str) -> (f32, f32, f32, f32) {
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
    fn imports_and_images_that_do_not_resolve_are_ignored() {
        for html in [
            "<!DOCTYPE html><style>@import url();</style><p>x</p>",
            "<!DOCTYPE html><style>@import \"\";</style><p>x</p>",
            "<!DOCTYPE html><style>@import url(http://[);</style><p>x</p>",
            "<!DOCTYPE html><style>@import \"data:text/css,@import 'x.css';\";</style><p>x</p>",
            "<!DOCTYPE html><img src=\"ftp://\"><p>x</p>",
            "<!DOCTYPE html><img src=\"https://x.test:99999/a.png\"><p>x</p>",
        ] {
            laid_out(html);
        }
    }

    #[test]
    fn a_stylesheet_importing_itself_runs_out_of_imports() {
        let start = std::time::Instant::now();
        laid_out(
            "<!DOCTYPE html><style>@import \"data:text/css,@import '%23';@import '%23';@import '%23';\";</style><p>x</p>",
        );
        // Without a limit on the total, this doesn't finish in minutes.
        assert!(start.elapsed() < std::time::Duration::from_secs(20));
    }

    #[test]
    fn hit_info_reports_a_protocol_relative_link_as_a_web_link() {
        const HTML: &str = "<!DOCTYPE html><body style=\"margin:0\"><p id=\"l\" style=\"margin:0\"><a href=\"//example.com/x\">link</a></p>";
        let doc = laid_out(HTML);
        let (x, y, _, h) = box_of(HTML, "l");
        assert_eq!(
            doc.hit_info(x + 5.0, y + h / 2.0).link.as_deref(),
            Some("https://example.com/x")
        );
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
            // The parser stops well before the end of it.
            Err(RenderError::TooDeep { depth }) if depth > MAX_NESTING_DEPTH
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
    fn form_controls_never_exist() {
        let html = "<!DOCTYPE html><body><p>kept</p><form><input value=a><textarea>b</textarea>\
            <select><option>c</option></select><button>d</button></form><input value=e>\
            <select><option>f</option></select><button>g</button><textarea>h</textarea>";
        let mut doc = laid_out(html);
        let list = format!("{:?}", doc.display_list().commands);
        assert!(list.contains("kept"));
        for gone in [
            "\"a\"", "\"b\"", "\"c\"", "\"d\"", "\"e\"", "\"f\"", "\"g\"", "\"h\"",
        ] {
            assert!(!list.contains(gone), "{gone}");
        }
    }

    #[test]
    fn width_only_block_images_keep_their_width() {
        let html = "<!DOCTYPE html><body style=\"margin:0\">\
            <img id=\"b\" src=\"https://t.test/b.png\" width=\"300\" style=\"display:block;background-color:#02b5df\">\
            <img id=\"i\" src=\"https://t.test/i.png\" width=\"300\">";
        assert_eq!(box_of(html, "b").2..=box_of(html, "b").3, 300.0..=16.0);
        assert_eq!(box_of(html, "i").2..=box_of(html, "i").3, 16.0..=16.0);
    }

    #[test]
    fn unavailable_images_are_framed_only_when_the_icon_fits() {
        // Paint commands (fills, strokes) of a page holding one such image.
        let fills = |size: &str| {
            let mut doc = laid_out(&format!(
                "<!DOCTYPE html><body><img src=\"https://t.test/s.gif\" {size}></body>"
            ));
            doc.display_list()
                .commands
                .iter()
                .filter(|c| {
                    matches!(
                        c,
                        crate::display_list::Command::Fill { .. }
                            | crate::display_list::Command::Stroke { .. }
                    )
                })
                .count()
        };
        let bare = fills("width=\"0\"");
        // Spacer GIFs and the size-less icon slot paint nothing...
        assert_eq!(fills("width=\"300\" height=\"5\""), bare);
        assert_eq!(fills("width=\"17\" height=\"60\""), bare);
        // ...from 18px each way the silver frame shows.
        assert!(fills("width=\"18\" height=\"18\"") > bare);
    }

    #[test]
    fn c1_controls_become_windows_1252_characters() {
        assert_eq!(
            repair_c1_controls("We\u{92}ve \u{93}new\u{94} \u{80}5 \u{96} \u{81}"),
            "We’ve “new” €5 – \u{81}"
        );
        assert!(matches!(
            repair_c1_controls("plain ‘ok’"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn collapsed_borders_follow_each_cell() {
        // An MJML card: cells bordered only left and right (no top border).
        let mut doc = laid_out("<!DOCTYPE html><style>table{border-collapse:collapse}</style><body style=\"margin:0\"><div style=\"width:300px\"><table style=\"width:100%\"><tr><td style=\"border-left:solid 1px #000;border-right:solid 1px #000;padding:0\"><div style=\"background:#eee;height:40px\"></div></td></tr></table></div>");
        let rects: Vec<kurbo::Rect> = doc
            .display_list()
            .commands
            .iter()
            .filter_map(|c| match c {
                crate::display_list::Command::Fill {
                    geometry: crate::display_list::Geometry::Rect(rect),
                    ..
                } => Some(*rect),
                _ => None,
            })
            .collect();
        let edge = |x0: f64| {
            rects
                .iter()
                .any(|r| r.x0 == x0 && r.width() == 1.0 && r.height() == 40.0)
        };
        // Both side borders sit on the grid lines, outside the cell's content.
        assert!(edge(0.0) && edge(299.0), "{rects:?}");
        // No top or bottom border.
        assert!(!rects.iter().any(|r| r.height() == 1.0));
    }

    #[test]
    fn inline_alt_text_keeps_the_image_font() {
        assert_eq!(
            without_box_size("font-size:16px; WIDTH: 5px;max-width:100%;color:red;display:block"),
            "font-size:16px;color:red"
        );
        // A `font-size:0` parent doesn't hide alt text whose image restores it.
        let html = "<!DOCTYPE html><body><div style=\"font-size:0\"><img id=\"i\" alt=\"Latest\" src=\"https://t.test/x.png\" style=\"font-size:16px;max-width:100%\" width=\"598\"></div>";
        let mut doc = laid_out(html);
        doc.select_all();
        assert_eq!(
            doc.selected_text().as_deref().map(str::trim),
            Some("Latest")
        );
        let sizes: Vec<f32> = doc
            .display_list()
            .commands
            .iter()
            .filter_map(|c| match c {
                crate::display_list::Command::Text(run) => Some(run.font_size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes, [16.0]);
    }

    #[test]
    fn collapsed_tables_have_no_padding() {
        let html = |collapse: &str| {
            format!("<!DOCTYPE html><body style=\"margin:0\"><table id=\"t\" cellpadding=0 cellspacing=0 style=\"border-collapse:{collapse};padding:24px 0\"><tr><td><div style=\"height:10px;width:10px\"></div></td></tr></table>")
        };
        assert_eq!(box_of(&html("collapse"), "t").3, 10.0);
        assert_eq!(box_of(&html("separate"), "t").3, 58.0);
    }

    #[test]
    fn alt_text_in_links_keeps_the_underline() {
        let decorations = |content: &str| {
            let mut doc = laid_out(&format!(
                "<!DOCTYPE html><body><a href=\"https://x.test/\">{content}</a>"
            ));
            doc.display_list()
                .commands
                .iter()
                .filter(|c| {
                    matches!(
                        c,
                        crate::display_list::Command::Stroke { .. }
                            | crate::display_list::Command::Fill { .. }
                    )
                })
                .count()
        };
        let text = decorations("Shop now");
        assert!(text > 0);
        // The image's `text-decoration:none` can't remove the link's underline.
        let alt = decorations("<img alt=\"Shop now\" src=\"https://h.test/x.png\" style=\"text-decoration:none;border:0\">");
        assert!(alt > text, "{alt} vs {text}"); // underline + icon frame
    }

    #[test]
    fn plain_rules_paint_chromium_bevel() {
        let colors = |hr: &str| {
            let mut doc = laid_out(&format!("<!DOCTYPE html><body>{hr}</body>"));
            doc.display_list()
                .commands
                .iter()
                .filter_map(|c| match c {
                    crate::display_list::Command::Fill {
                        brush: crate::display_list::Brush::Solid(color),
                        ..
                    } => Some(color.to_rgba8().to_u8_array()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let plain = colors("<hr>");
        assert!(plain.contains(&[154, 154, 154, 255]) && plain.contains(&[238, 238, 238, 255]));
        assert_eq!(colors("<hr style=\"color:red\">"), plain);
        assert!(colors("<hr noshade>")
            .iter()
            .all(|c| *c == [128, 128, 128, 255]));
    }

    #[test]
    fn align_justify_justifies_text() {
        let first_line_end = |attr: &str| {
            let mut doc = laid_out(&format!("<!DOCTYPE html><body style=\"margin:0\"><div style=\"width:200px\"><p {attr}>aaa bbb ccc ddd eee fff ggg hhh iii jjj kkk lll mmm nnn</p></div>"));
            let list = doc.display_list();
            let run = list
                .commands
                .iter()
                .find_map(|c| match c {
                    crate::display_list::Command::Text(run) => Some(run.clone()),
                    _ => None,
                })
                .unwrap();
            let last = run.words.last().unwrap();
            last.x + last.width
        };
        assert!(first_line_end("") < 195.0);
        assert!((first_line_end("align=\"justify\"") - 200.0).abs() < 1.0);
    }

    #[test]
    fn aliased_faces_render_heavy() {
        // Holds both where the face is filed under "Arial" (alias) and where
        // "Arial Black" is a family of its own.
        let mut collection = crate::fonts::font_context().collection;
        if collection.family_id("Arial Black").is_none() {
            return; // Not installed.
        }
        let mut doc =
            laid_out("<!DOCTYPE html><body><p style=\"font-family:'arial black',arial\">Heavy</p>");
        let weights: Vec<f32> = doc
            .display_list()
            .commands
            .iter()
            .filter_map(|c| match c {
                crate::display_list::Command::Text(run) => Some(run.face.weight),
                _ => None,
            })
            .collect();
        assert_eq!(weights, [900.0]);
    }

    #[test]
    fn rowspan_heights_and_overlaps_follow_the_html_table_model() {
        // The extra height of a tall rowspan cell goes to the row where
        // another rowspan cell starts (Chromium), not to every row.
        // A fixed line height: the text row must not depend on the platform's
        // default font.
        let html = "<!DOCTYPE html><body style=\"margin:0;font:10px/10px serif\">\
            <table cellpadding=0 cellspacing=0>\
            <tr><td rowspan=4 height=200>x</td><td id=a height=19>a</td></tr>\
            <tr><td rowspan=2 height=59>b</td><td id=b height=24>b</td></tr>\
            <tr><td id=c height=35>c</td></tr><tr><td id=d>d</td></tr></table>";
        assert_eq!(box_of(html, "a").3, 19.0);
        assert_eq!(box_of(html, "b").3, 136.0);
        assert_eq!(box_of(html, "c").3, 35.0);
        // A colspan overlapping a rowspan cell stays at its HTML column.
        let html = "<!DOCTYPE html><body style=\"margin:0\"><table cellpadding=0 cellspacing=0>\
            <tr><td width=10>a</td><td width=10>b</td><td rowspan=2 width=10>c</td></tr>\
            <tr><td id=s colspan=3>s</td></tr></table>";
        assert_eq!(box_of(html, "s").0, 0.0);
    }

    #[test]
    fn small_quirks_alt_boxes_show_only_text() {
        let html = format!("{QUIRKS}<body style=\"margin:0\"><img id=\"s\" alt=\"Books\" src=\"https://t.test/b.png\" width=\"35\" height=\"15\"><img id=\"l\" alt=\"Books\" src=\"https://t.test/b.png\" width=\"60\" height=\"40\">");
        let doc = laid_out(&html);
        let blitz = doc.blitz();
        let children = |id: &str| {
            blitz
                .get_node(blitz.get_element_by_id(id).unwrap())
                .unwrap()
                .children
                .len()
        };
        // Too small for the icon: just the (clipped) text; else icon + text.
        assert_eq!(children("s"), 1);
        assert_eq!(children("l"), 2);
    }

    #[test]
    fn zoom_scales_layout_paint_and_input() {
        let html = "<!DOCTYPE html><body style=\"margin:0\"><div style=\"width:100px;height:50px;background:red\"></div><a href=\"https://x.test/\" style=\"font-size:16px\">link</a>";
        let mut doc = HtmlDocument::parse(html, DocumentOptions::default());
        let normal = doc.layout(400.0);
        doc.set_zoom(2.0);
        assert_eq!(doc.zoom(), 2.0);
        // 200 CSS px across a 400px pane, everything drawn twice as large.
        let zoomed = doc.layout(400.0);
        // (Line heights are rounded at the zoomed size, as in browsers.)
        assert!((zoomed - normal * 2.0).abs() <= 4.0, "{zoomed} vs {normal}");
        let list = doc.display_list();
        let red = list.commands.iter().find_map(|c| match c {
            crate::display_list::Command::Fill {
                geometry: crate::display_list::Geometry::Rect(rect),
                brush: crate::display_list::Brush::Solid(color),
            } if color.to_rgba8().to_u8_array() == [255, 0, 0, 255] => Some(*rect),
            _ => None,
        });
        assert_eq!(red.map(|r| (r.width(), r.height())), Some((200.0, 100.0)));
        let size = list.commands.iter().find_map(|c| match c {
            crate::display_list::Command::Text(run) => Some(run.font_size),
            _ => None,
        });
        assert_eq!(size, Some(32.0));
        // Pointer coordinates are zoomed pixels: the link sits below y=100.
        assert!(doc.hit_info(10.0, 110.0).link.is_some());
        assert!(doc.hit_info(10.0, 60.0).link.is_none());
        // Out-of-range zooms are clamped.
        doc.set_zoom(100.0);
        assert_eq!(doc.zoom(), 5.0);
    }

    /// `data:` URL of a 20×20 GIF animating through `colors`, 100ms each.
    fn animated_gif_url(colors: &[[u8; 4]]) -> String {
        use image::codecs::gif::{GifEncoder, Repeat};
        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            for color in colors {
                let buffer = image::RgbaImage::from_pixel(20, 20, image::Rgba(*color));
                let delay = image::Delay::from_numer_denom_ms(100, 1);
                encoder
                    .encode_frame(image::Frame::from_parts(buffer, 0, 0, delay))
                    .unwrap();
            }
        }
        let encoded: String = bytes.iter().map(|b| format!("%{b:02X}")).collect();
        format!("data:image/gif,{encoded}")
    }

    #[test]
    fn animated_gifs_are_recorded_for_replay() {
        let gif = animated_gif_url(&[[255, 0, 0, 255], [0, 0, 255, 255]]);
        let still = animated_gif_url(&[[0, 255, 0, 255]]);
        let mut doc = laid_out(&format!(
            "<!DOCTYPE html><body><img src=\"{gif}\"><div style=\"width:20px;height:20px;background:url('{gif}')\"></div><img src=\"{still}\"></body>"
        ));
        let list = doc.display_list();
        // One animation (the same GIF twice), the still one isn't.
        assert_eq!(list.animations.len(), 1);
        let frames = list.animations.values().next().unwrap();
        assert_eq!(frames.frames.len(), 2);
        assert_eq!(frames.duration_ms(), 200);
    }
}

/// Markup a message can send to attack the renderer: none of it may panic, hang or take
/// gigabytes, since a panic or an allocation failure takes the whole app down.
#[cfg(test)]
mod hostile_tests {
    use super::tests::box_of;
    use super::*;
    use crate::display_list::{Command, Geometry};

    fn parse(html: &str) -> HtmlDocument {
        let mut document = HtmlDocument::parse(html, DocumentOptions::default());
        document.prepare(640.0);
        document
    }

    fn hover(document: &mut HtmlDocument, x: f32, y: f32) {
        document.pointer(PointerInput {
            x,
            y,
            phase: PointerPhase::Move,
            button: PointerButton::Primary,
            primary_pressed: false,
            modifiers: Default::default(),
        });
        document.layout(640.0);
        document.display_list();
    }

    fn path_elements(document: &mut HtmlDocument) -> usize {
        document
            .display_list()
            .commands
            .iter()
            .map(|command| match command {
                Command::Fill {
                    geometry: Geometry::Path(path),
                    ..
                } => path.elements().len(),
                Command::Fill { .. } => 1,
                Command::Stroke { path, .. } => path.elements().len(),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn colspans_past_the_limits_dont_wrap_around() {
        // `u16` sums of the column cursor used to wrap and panic slicing.
        parse("<table><tr><td>x<td colspan=65535>y</table>");
        parse(&format!(
            "<table><tr>{}</table>",
            "<td colspan=1000>x".repeat(66)
        ));
        parse("<table><col span=65535 width=1><tr><td>x</table>");
        parse("<table><tr><td rowspan=65534 colspan=65535>x</table>");
    }

    #[test]
    fn a_table_past_the_row_limit_still_lays_out() {
        parse(&format!(
            "<table>{}<tr><td rowspan=65534>y</table>",
            "<tr><td>x".repeat(32_800)
        ));
    }

    #[test]
    fn huge_lengths_dont_trip_line_breaking() {
        // Line and layout widths rounded apart, tripping an assert also active in release.
        parse("<p style=\"padding:100000000px\">hello</p>");
        parse("<p style=\"padding:calc(infinity*1px)\">hello</p>");
        parse("<p style=\"float:left;padding:1e30px\">a</p><p style=\"padding:1e30px\">hello</p>");
    }

    #[test]
    fn a_contents_pseudo_of_a_table_isnt_a_layout_child() {
        parse("<style>table::before{display:contents;content:'y'}</style><table><tr><td>a</table>");
        parse("<style>table::after{display:contents;content:'y'}</style><table><tr><td>a</table>");
    }

    #[test]
    fn a_replaced_element_keeps_its_image_when_styled_as_a_table() {
        parse("<style>img::after{content:'x'}</style><img style=\"display:table\">");
        parse(
            "<style>img::before{content:'x'}</style><img width=1 style=\"display:inline-table\">",
        );
    }

    #[test]
    fn boxes_freed_by_hover_arent_painted() {
        // Box reconstruction left the freed ids in lists whose owners saw no damage.
        for html in [
            "<style>::after{display:flex;content:'y'}p:hover{display:flex}</style></p><img>",
            "<style>::before{content:'x';display:block}*:hover{display:inline}</style>hello",
        ] {
            let mut document = parse(html);
            for y in (0..60).step_by(4) {
                for x in (0..120).step_by(8) {
                    hover(&mut document, x as f32, y as f32);
                }
                document.set_zoom(1.5);
                document.layout(640.0);
                document.display_list();
                hover(&mut document, 500.0, 500.0);
                document.set_zoom(1.0);
                document.layout(640.0);
                document.display_list();
            }
        }
    }

    #[test]
    fn a_selection_older_than_its_text_is_left_out() {
        let mut document =
            parse("<style>*:hover::before{content:'x'}</style>&nbsp;&shy;&#8203;&#x202e;");
        document.select_all();
        for (x, y) in [(8.0, 8.0), (4.0, 12.0), (500.0, 500.0), (8.0, 8.0)] {
            hover(&mut document, x, y);
            let _ = document.selected_text();
        }
    }

    #[test]
    fn long_marked_borders_and_lines_are_bounded() {
        for html in [
            "<div style=\"width:100000000px;border-top:1px dotted red\">x</div>",
            "<div style=\"width:100000000px;border-top:1px dashed red\">x</div>",
            "<legend style=\"min-width:1e30px;border:5px dashed red\">x</legend>",
            "<mark style=\"padding:1e30px;border:5px dashed red;border-radius:50%\">x</mark>",
            "<mark style=\"padding:1e30px;border:5px dotted red;border-radius:50%\">x</mark>",
            "<u style=\"text-decoration-style:wavy;letter-spacing:1e8px\">xx</u>",
            "<u style=\"text-decoration-style:dotted;letter-spacing:1e30px\">xx</u>",
            "<u style=\"text-decoration-style:dashed;letter-spacing:1e30px\">xx</u>",
        ] {
            let mut document = parse(html);
            let elements = path_elements(&mut document);
            assert!(elements < 1 << 20, "{html}: {elements}");
        }
    }

    #[test]
    fn marks_of_ordinary_size_are_kept() {
        let mut document = parse("<div style=\"width:400px;border-top:2px dashed red\">x</div>");
        assert!(path_elements(&mut document) > 40);
        let mut document = parse("<u style=\"text-decoration-style:wavy\">wavy text</u>");
        assert!(path_elements(&mut document) > 4);
    }

    #[test]
    fn documents_too_costly_to_lay_out_are_refused() {
        let refused = |html: &str| {
            matches!(
                HtmlDocument::try_parse(html, DocumentOptions::default()),
                Err(RenderError::TooComplex)
            )
        };
        assert!(refused(
            "<table><tr><td colspan=1000 rowspan=32767>x</table>"
        ));
        assert!(refused(
            &"<table><tr><td colspan=1000>x</table>".repeat(2001)
        ));
        // A tag can make hundreds of nodes, by the formatting elements it reopens.
        assert!(refused(&"<p>x".repeat(300_000)));
        let formatting: String = (0..240).map(|i| format!("<b a{i}>")).collect();
        assert!(refused(&format!("<p>{formatting}{}", "<p>x".repeat(3000))));
        assert!(!refused(&"<table><tr><td colspan=2>x</table>".repeat(100)));
    }

    #[test]
    fn tags_and_end_tag_attributes_are_counted_without_parsing() {
        let limit = MAX_OPENING_TAGS + 1;
        assert!(has_too_many_tags(&"<p>".repeat(limit)));
        assert!(has_too_many_tags(&"</i>".repeat(limit)));
        assert!(!has_too_many_tags(&"<p>".repeat(MAX_OPENING_TAGS)));
        assert!(!has_too_many_tags("a < b, é <é <"));
        let spaced: String = (0..2000).map(|i| format!(" a{i}")).collect();
        let slashed: String = (0..2000).map(|i| format!("/a{i}")).collect();
        let quoted: String = (0..2000).map(|i| format!("a{i}=\"\"")).collect();
        // Nothing can hide a tag or an end tag from it.
        for prefix in [
            "",
            "<a \">",
            "<? <a b=\">",
            "</ <a b=\">",
            "<title><a b=\"</title>",
            "<!-- --!>",
        ] {
            assert!(
                has_too_many_tags(&format!("{prefix}{}", "<i>".repeat(limit))),
                "{prefix}"
            );
            for attributes in [&spaced, &slashed] {
                assert!(
                    has_too_many_tags(&format!("{prefix}<p>x</p{attributes}>")),
                    "{prefix}"
                );
            }
            assert!(
                has_too_many_tags(&format!("{prefix}<p>x</p {quoted}>")),
                "{prefix}"
            );
            assert!(
                has_too_many_tags(&format!("{prefix}<p>x</p a=\">\" {spaced}>")),
                "{prefix}"
            );
        }
        // A long quoted value is one attribute, and a page of words in a style isn't one.
        assert!(!has_too_many_tags(&format!("</p a=\"{spaced}\">")));
        // Linear, also on tags that never end.
        let start = std::time::Instant::now();
        assert!(has_too_many_tags(&"</a ".repeat(1_000_000)));
        assert!(has_too_many_tags(&"<a ".repeat(1_000_000)));
        let _ = has_too_many_tags(&format!(
            "{}{}",
            "</a b=".repeat(100_000),
            "x".repeat(1 << 20)
        ));
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn the_parser_stops_at_its_budgets() {
        let refused = |html: &str| {
            let start = std::time::Instant::now();
            let result = HtmlDocument::try_parse(html, DocumentOptions::default());
            assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
            result.map(|_| ()).err()
        };
        // Comments, text, and attributes copied by each reopened formatting element.
        assert_eq!(
            refused(&format!("<p>x{}", "<!>".repeat(600_000))),
            Some(RenderError::TooComplex)
        );
        assert_eq!(
            refused(&format!("<p>{}", "x".repeat(5 << 20))),
            Some(RenderError::TooComplex)
        );
        let attribute = "a".repeat(125_000);
        let html = format!("<p><b a0=\"{attribute}\">{}", "<p>x".repeat(1000));
        assert_eq!(refused(&html), Some(RenderError::TooComplex));
        // Nesting closed in a way the parser ignores stops early, and is still too deep.
        let html = "<span><div></span></div>".repeat(100_000);
        assert!(matches!(refused(&html), Some(RenderError::TooDeep { .. })));
        assert_eq!(refused("<p>hello</p><!-- ok -->"), None);
    }

    #[test]
    fn hoisted_boxes_freed_by_a_media_query_arent_rounded() {
        let html = "<style>@media(min-width:401px){::before{position:fixed;content:'x'}}</style>\
            <p style=\"filter:contrast(1)\"><mark>";
        let mut document = parse(html);
        for width in [300.0, 640.0, 300.0] {
            document.layout(width);
            document.display_list();
        }
        document.set_zoom(2.5);
        document.layout(800.0);
        document.display_list();
    }

    #[test]
    fn a_custom_property_cant_carry_layout_through_a_hover() {
        let html = "<style>body:hover{--x-color:none} p{display:var(--x-color,block)}</style>\
            <body style=margin:0><p>hello</p><p style=\"height:300px\">b</p>";
        let mut document = parse(html);
        let height = document.layout(640.0);
        hover(&mut document, 10.0, 20.0);
        assert_eq!(document.layout(640.0), height);
    }

    #[test]
    fn nowrap_spans_past_the_limit_dont_trip_parley() {
        parse(&format!(
            "<style>b{{white-space:nowrap}}</style><p>{}",
            "<b>x </b> y".repeat(70_000)
        ));
    }

    #[test]
    fn a_large_table_in_nested_inline_tables_lays_out_in_bounded_time() {
        let open = "<table style=display:inline-table><tr><td>".repeat(8);
        let start = std::time::Instant::now();
        parse(&format!(
            "{open}<table><tr>{}</table>",
            "<td>x".repeat(1000)
        ));
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    }

    #[test]
    fn hover_rules_keep_only_what_paints() {
        let html = "<style>p:hover{height:300px;display:none;color:red}\
            @media all{p:is(:hover){--x:1;float:left}}</style><body style=margin:0><p>hello</p>";
        let mut document = parse(html);
        let height = document.layout(640.0);
        let colors =
            |document: &mut HtmlDocument| format!("{:?}", document.display_list().commands);
        let before = colors(&mut document);
        hover(&mut document, 10.0, 20.0);
        assert_eq!(document.layout(640.0), height);
        // The color of the rule still applies.
        assert_ne!(colors(&mut document), before);
    }

    #[test]
    fn styles_replaced_under_a_table_context_stay_alive() {
        // The context keeps `calc()` pointers into styles a restyle replaces: this read freed
        // memory, and crashed where it landed.
        for html in [
            "<style>*:hover{color:red}::after{content:'x'}</style>\
                <table style=\"width:calc(100% - 1px)\"><tr><td style=\"width:calc(50% - 1px)\">a</table>",
            "<style>@media(max-width:400px){*{color:red}}::after{content:'x'}</style>\
                <table style=\"width:calc(100% - 1px)\"><tr><td style=\"width:calc(50% - 1px)\">a</table>",
        ] {
            let mut document = parse(html);
            for _ in 0..20 {
                hover(&mut document, 8.0, 8.0);
                document.layout(300.0);
                document.display_list();
                hover(&mut document, 500.0, 500.0);
                document.layout(640.0);
                document.display_list();
            }
        }
    }

    #[test]
    fn boxes_freed_by_a_media_query_arent_reached_by_walks_up() {
        let html = "<style>@media(max-width:400px){a{display:contents}*{display:inline}}\
            ::after{content:'x'}</style><a href=#>link</a><p>y</p>";
        let mut document = parse(html);
        for _ in 0..5 {
            hover(&mut document, 10.0, 10.0);
            document.layout(300.0);
            hover(&mut document, 10.0, 10.0);
            let _ = document.find("x");
            document.select_match(0);
            document.layout(640.0);
            document.display_list();
        }
    }

    #[test]
    fn a_long_log_with_a_break_per_line_lays_out() {
        // Parley asserts on more styles than a `u16` holds.
        parse(&"<br>".repeat(70_000));
        parse(&"<a>x".repeat(70_000));
        parse(&format!("<pre>{}", "<b>x</b>".repeat(70_000)));
    }

    #[test]
    fn nested_inline_tables_lay_out_in_bounded_time() {
        let open = "<table style=display:inline-table><tr><td>";
        let start = std::time::Instant::now();
        parse(&format!("{}x", open.repeat(60)));
        assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
    }

    #[test]
    fn an_exhausted_budget_stops_layers_shadows_and_stops() {
        let stops = vec!["red"; 1000].join(",");
        let layers = vec![format!("linear-gradient({stops})"); 100].join(",");
        let html = format!(
            "<style>div{{height:1px;background:{layers}}}</style>{}",
            "<div></div>".repeat(300)
        );
        let mut document = parse(&html);
        let commands = document.display_list().commands.len();
        assert!(commands < 10_000, "{commands}");
        let shadows = vec!["0 0 1px red"; 1000].join(",");
        let html = format!(
            "<style>div{{height:1px;box-shadow:{shadows}}}</style>{}",
            "<div></div>".repeat(2000)
        );
        let mut document = parse(&html);
        let commands = document.display_list().commands.len();
        assert!(commands < 600_000, "{commands}");
    }

    #[test]
    fn spaced_tiles_past_the_limit_arent_drawn_one_by_one() {
        const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M/wHwAEBgIApD5fRAAAAABJRU5ErkJggg==";
        let html = format!(
            "<div style=\"background:url({PNG}) space;width:17000000px;height:17000000px\"></div>"
        );
        let mut document = HtmlDocument::parse(
            &html,
            DocumentOptions {
                resources: Resources::new().allow_remote(true),
                ..Default::default()
            },
        );
        document.prepare(640.0);
        document.display_list();
    }

    #[test]
    fn a_long_word_of_many_styles_is_recorded_in_linear_time() {
        // Each style's run walked every cluster of the word: 10000 took 3 s, on the UI thread.
        let start = std::time::Instant::now();
        parse(&format!("<pre>{}", "<b>x</b>".repeat(50_000)));
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }

    #[test]
    fn a_hidden_box_doesnt_run_the_layout_it_had_when_shown() {
        // Its `img` has lost its styles by then, and this used to abort.
        for html in [
            "<style>@media(min-width:601px){summary{display:inline}}</style>\
                <details open><summary>s</summary><div><img></div></details>",
            "<style>@media(max-width:600px){summary{display:inline-block}}</style>\
                <details open><summary>s</summary><div><span style=display:inline-block>x</span></div></details>",
        ] {
            let mut document = parse(html);
            for (x, y) in [(5.0, 5.0), (20.0, 12.0), (8.0, 8.0)] {
                drag(&mut document, x, y);
                document.layout(300.0);
                document.display_list();
                drag(&mut document, x, y);
                document.layout(640.0);
                document.display_list();
            }
        }
    }

    fn drag(document: &mut HtmlDocument, x: f32, y: f32) {
        for (phase, pressed) in [
            (PointerPhase::Down { click_count: 1 }, true),
            (PointerPhase::Up, false),
        ] {
            document.pointer(PointerInput {
                x,
                y,
                phase,
                button: PointerButton::Primary,
                primary_pressed: pressed,
                modifiers: Default::default(),
            });
        }
    }

    #[test]
    fn huge_radii_dont_make_huge_paths() {
        let start = std::time::Instant::now();
        let text = "a ".repeat(1000);
        parse(&format!(
            "<p style=\"text-decoration:line-through dotted;font-size:calc(1px*infinity)\">{text}"
        ));
        parse(
            &"<div style=\"clip-path:circle(1e30px);width:9px;height:9px;background:red\"></div>"
                .repeat(3000),
        );
        parse(
            &"<div style=\"clip-path:ellipse(1e30px 1e30px);width:9px;height:9px\"></div>"
                .repeat(3000),
        );
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }

    #[test]
    fn merged_root_attributes_are_bounded() {
        let html: String = (0..20_000).map(|i| format!("<html a{i}>")).collect();
        let start = std::time::Instant::now();
        let document = HtmlDocument::try_parse(&html, DocumentOptions::default());
        assert!(document.is_ok());
        assert!(start.elapsed().as_secs() < 2, "{:?}", start.elapsed());
    }

    #[test]
    fn a_hoisted_list_is_cleared_when_its_box_becomes_a_leaf() {
        let html = "<style>@media(min-width:401px){.a{position:absolute;display:flex}\
            .a::after{content:'p'}}</style><p class=a><span class=a></span></p>";
        let mut document = parse(html);
        for width in [300.0, 640.0, 300.0] {
            document.layout(width);
            document.display_list();
        }
    }

    #[test]
    fn a_huge_line_height_doesnt_make_huge_paths() {
        let start = std::time::Instant::now();
        let mut document = parse(&format!(
            "<style>div{{border-radius:50%;background:red;line-height:calc(1px*infinity)}}</style>{}x",
            "<div>".repeat(20)
        ));
        document.layout(300.0);
        document.display_list();
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn a_box_no_longer_out_of_flow_is_painted_by_its_parent() {
        // It kept the containing block it had when absolute, whose list no longer holds it.
        let html = "<style>@media(max-width:400px){table,tbody,tr,td{position:absolute}}</style>\
            <table><tr><td>ShopNow</td></tr></table>";
        let mut document = parse(html);
        let shown = |document: &mut HtmlDocument| {
            format!("{:?}", document.display_list().commands).contains("ShopNow")
        };
        assert!(shown(&mut document));
        document.layout(300.0);
        document.display_list();
        document.layout(640.0);
        assert!(shown(&mut document));
    }

    #[test]
    fn nested_inline_tables_with_much_text_lay_out_in_bounded_time() {
        let open =
            "<table style=\"display:inline-table;width:50%\"><td>a b c <img width=9 height=9>";
        let html = format!("{}{}", open.repeat(8), "word ".repeat(2000));
        let start = std::time::Instant::now();
        let mut document = parse(&html);
        document.layout(300.0);
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    }

    #[test]
    fn a_row_group_with_display_contents_keeps_the_table() {
        let html = "<table><tbody style=display:contents><tr><td>Kept</td></tr></tbody></table>";
        let mut document = parse(html);
        let list = format!("{:?}", document.display_list().commands);
        assert!(list.contains("Kept"));
    }

    #[test]
    fn a_label_cant_click_a_control_that_doesnt_exist() {
        // This used to overflow the stack and abort, the click going around label and control.
        for html in [
            "<label>x<input type=file>y</label>",
            "<label for=b id=a><label for=a id=b><input id=c type=file></label></label>",
        ] {
            let mut document = parse(html);
            for (x, y) in [(5.0, 10.0), (20.0, 12.0), (40.0, 12.0)] {
                drag(&mut document, x, y);
            }
        }
    }

    #[test]
    fn what_isnt_a_row_or_cell_in_a_table_keeps_its_content() {
        for html in [
            "<table><tr style=display:block><td>Kept</td></tr></table>",
            "<table><tr style=display:flex><td>Kept</td></tr></table>",
            "<table><tr style=display:inline><td>Kept</td></tr></table>",
            "<table><tbody style=display:block><tr><td>Kept</td></tr></tbody></table>",
            "<table><thead style=display:block><tr><td>Kept</td></tr></thead></table>",
            "<div style=display:table><p>Kept</p></div>",
            "<table><caption>Kept</caption><tr><td>x</td></tr></table>",
        ] {
            let mut document = parse(html);
            let list = format!("{:?}", document.display_list().commands);
            assert!(list.contains("Kept"), "{html}");
        }
    }

    #[test]
    fn a_table_part_changing_display_at_a_width_rebuilds_the_table() {
        let html = "<style>@media(max-width:480px){tbody{display:block!important}}</style>\
            <table><tr><td>Kept</td><td>Also</td></tr></table>";
        let mut document = parse(html);
        for width in [300.0, 640.0, 300.0, 640.0] {
            document.layout(width);
            let list = format!("{:?}", document.display_list().commands);
            assert!(list.contains("Kept") && list.contains("Also"), "{width}");
        }
    }

    #[test]
    fn generated_content_of_nested_inlines_is_kept() {
        let html =
            "<style>.n::before{content:'Made'}</style><p><span><a class=n>link</a></span></p>";
        let mut document = parse(html);
        let list = format!("{:?}", document.display_list().commands);
        assert!(list.contains("Made") && list.contains("link"));
    }

    #[test]
    fn vertical_align_changing_at_a_width_lays_out_again() {
        let html = "<style>.d{display:inline-block;height:20px;width:9px}\
            @media(max-width:400px){.d{vertical-align:40px}}</style><p style=margin:0><span class=d></span>x</p>";
        let fresh = parse(html).layout(640.0);
        let mut document = parse(html);
        document.layout(300.0);
        assert_eq!(document.layout(640.0), fresh);
    }

    #[test]
    fn a_cell_that_stops_being_a_block_rebuilds_its_layout() {
        // It kept an inline layout with a float in it, reaching an `unreachable!()`.
        for row in [
            "style=display:contents",
            "style=\"display:table-row-group\"",
        ] {
            let html = format!(
                "<style>@media(max-width:400px){{*{{float:right}}}}</style>\
                 <table><tr {row}><td><ul></ul>x"
            );
            let mut document = parse(&html);
            for width in [300.0, 640.0, 300.0, 640.0] {
                document.layout(width);
                document.display_list();
            }
        }
    }

    #[test]
    fn a_contents_row_keeps_its_text_and_place() {
        let html =
            "<table><tr style=display:contents><td>First</td></tr><tr><td>Second</td></tr></table>";
        let mut document = parse(html);
        let list = format!("{:?}", document.display_list().commands);
        let (first, second) = (list.find("First"), list.find("Second"));
        assert!(
            first.is_some() && second.is_some() && first < second,
            "{first:?} {second:?}"
        );
    }

    #[test]
    fn a_cells_color_stays_in_the_cell_without_a_row() {
        let html = "<div style=\"display:table;width:400px\"><div style=\"display:table-cell;\
            background:#ff0000;width:100px\">Menu</div><div style=\"display:table-cell\">Body</div></div>";
        let mut document = parse(html);
        let reds = format!("{:?}", document.display_list().commands)
            .matches("components: [1.0, 0.0, 0.0, 1.0]")
            .count();
        assert_eq!(reds, 1);
    }

    #[test]
    fn a_contents_row_stays_a_row_of_its_own() {
        let html = "<table><tr><td>A</td></tr><tr style=display:contents><td>B</td></tr><tr><td>C</td></tr></table>";
        let mut document = parse(html);
        let baselines: Vec<u32> = document
            .display_list()
            .commands
            .iter()
            .filter_map(|command| match command {
                Command::Text(run) => Some(run.baseline as u32),
                _ => None,
            })
            .collect();
        assert_eq!(baselines.len(), 3);
        assert!(
            baselines[0] < baselines[1] && baselines[1] < baselines[2],
            "{baselines:?}"
        );
    }

    #[test]
    fn huge_curves_arent_recorded() {
        for html in [
            "<div style=\"transform:scale(1e20);border:1px dashed red;border-radius:9px\">x",
            "<div style=\"transform:matrix(1e16,0,0,1e16,0,0);border:1px solid red;border-radius:9px\">x",
        ] {
            let mut document = parse(html);
            for command in &document.display_list().commands {
                let path = match command {
                    Command::Fill { geometry: Geometry::Path(path), .. } | Command::Stroke { path, .. } => path,
                    _ => continue,
                };
                assert!(path.elements().iter().all(|el| !matches!(
                    el,
                    kurbo::PathEl::CurveTo(a, ..) if a.x.abs() > 1e12
                )), "{html}");
            }
        }
    }

    #[test]
    fn a_layout_past_its_time_gives_up() {
        let mut document = HtmlDocument::parse("<p>hello</p>", DocumentOptions::default());
        assert!(!document.prepare_within(640.0, std::time::Duration::ZERO));
        let mut document = HtmlDocument::parse("<p>hello</p>", DocumentOptions::default());
        assert!(document.prepare_within(640.0, std::time::Duration::from_secs(60)));
        assert!(format!("{:?}", document.display_list().commands).contains("hello"));
    }

    #[test]
    fn generated_text_has_a_budget_per_pass() {
        let big = "a".repeat(32 << 10);
        let start = std::time::Instant::now();
        parse(&format!(
            "<style>b::before{{content:'{big}'}}</style>{}",
            "<b></b>".repeat(4000)
        ));
        parse(&format!(
            "<style>li{{list-style-type:'{big}'}}</style>{}",
            "<li>".repeat(4000)
        ));
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
        // What fits is kept, across widths.
        let mut document = parse(
            "<style>b::before{content:'Made'}@media(max-width:400px){b{color:red}}</style><b>x</b>",
        );
        for width in [300.0, 640.0, 300.0] {
            document.layout(width);
            assert!(format!("{:?}", document.display_list().commands).contains("Made"));
        }
    }

    #[test]
    fn rounding_doesnt_keep_grid_tracks_growing() {
        let html = "<ul style=\"grid-template-areas:'a b' 'c d';display:grid\">t<div style=\"grid-area:1/1/99999/99999\">fi fl ffi iso</div><p style=\"margin:auto;grid-column:-32768/-1\"><div style=\"margin-left:auto\">x</div><ul style=\"grid-column:-32768/-1\">y".repeat(8);
        let start = std::time::Instant::now();
        parse(&html);
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn huge_rounded_borders_share_the_budget_by_their_cost() {
        let html = format!(
            "<style>b{{position:absolute;left:0;top:0;width:16000px;height:16000px;border:1px solid red;\
             border-radius:50%;display:block}}</style>{}",
            "<b></b>".repeat(36_000)
        );
        let mut document = parse(&html);
        let curves: usize = document
            .display_list()
            .commands
            .iter()
            .map(|command| match command {
                Command::Fill {
                    geometry: Geometry::Path(path),
                    ..
                }
                | Command::Stroke { path, .. } => path.elements().len(),
                _ => 0,
            })
            .sum();
        assert!(curves < 100_000, "{curves}");
    }

    #[test]
    fn rows_of_contents_in_separate_groups_stay_apart() {
        let html = "<table><tbody><tr style=display:contents><td>A</td></tr></tbody>\
            <tbody><tr style=display:contents><td>B</td></tr></tbody></table>";
        let mut document = parse(html);
        let baselines: Vec<u32> = document
            .display_list()
            .commands
            .iter()
            .filter_map(|command| match command {
                Command::Text(run) => Some(run.baseline as u32),
                _ => None,
            })
            .collect();
        assert!(
            baselines.len() == 2 && baselines[0] < baselines[1],
            "{baselines:?}"
        );
    }

    #[test]
    fn a_contents_wrapper_in_a_row_keeps_its_cells() {
        let html = "<div style=display:table><div style=display:table-row><div style=display:contents>\
            <div style=display:table-cell>Kept</div></div><div style=display:table-cell>Also</div></div></div>";
        let mut document = parse(html);
        let list = format!("{:?}", document.display_list().commands);
        assert!(list.contains("Kept") && list.contains("Also"));
    }

    #[test]
    fn a_color_attribute_outside_ascii_is_no_color() {
        for html in [
            "<p bgcolor=\"#日\">x",
            "<p bgcolor=\"#éa\">x",
            "<p bgcolor=\"#日本\">x",
            "<font color=\"#日\">x",
        ] {
            parse(html);
        }
    }

    #[test]
    fn sibling_selectors_stay_bounded() {
        let many = "<p>".repeat(8000);
        let start = std::time::Instant::now();
        parse(&format!("<style>x~p{{color:red}}</style>{many}"));
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        // And what they select is still right.
        let html =
            "<style>i~b{color:#ff0000}b+b{color:#00ff00}</style><b>a</b><i>x</i><b>c</b><b>d</b>";
        let mut document = parse(html);
        let list = format!("{:?}", document.display_list().commands);
        assert!(
            list.contains("components: [1.0, 0.0, 0.0, 1.0]")
                || list.contains("components: [0.0, 1.0, 0.0, 1.0]")
        );
    }

    #[test]
    fn a_style_pass_out_of_steps_gives_up_a_first_layout() {
        let rules = "x~p{color:red}".repeat(400);
        let html = format!("<style>{rules}</style>{}", "<p>".repeat(8000));
        let mut document = HtmlDocument::parse(&html, DocumentOptions::default());
        let start = std::time::Instant::now();
        let _ = document.prepare_within(640.0, std::time::Duration::from_secs(30));
        assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
    }

    #[test]
    fn inline_padding_saturates_before_making_corners() {
        let html =
            "<span style=\"border:1px dotted red;border-radius:50%;padding:1e39px\">x</span>";
        let start = std::time::Instant::now();
        let mut document = parse(html);
        document.layout(300.0);
        document.display_list();
        assert!(start.elapsed().as_millis() < 500, "{:?}", start.elapsed());
    }

    #[test]
    fn fonts_from_a_message_arent_loaded() {
        // One changed byte aborted the app; with no fetch the bytes never reach the text system.
        let font: String = blitz_dom::BULLET_FONT
            .iter()
            .map(|b| format!("%{b:02X}"))
            .collect();
        let html = format!(
            "<style>@font-face{{font-family:Z;src:url(data:font/otf,{font})}}p{{font-family:Z}}</style><p>x"
        );
        let mut document = parse(&html);
        assert_eq!(document.inner.web_font_bytes(), 0);
        assert!(format!("{:?}", document.display_list().commands).contains('x'));
    }

    #[test]
    fn a_float_in_a_cell_keeps_its_height_when_measured_twice() {
        // The cell's height, against Chromium: an image in a paragraph, floated, was lost from it.
        let html = "<table width=\"100%\"><tr><td><div style=\"float:left;width:50%\"><table><tr><td>\
            <p style=margin:0><img id=i src=\"data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7\" \
            align=left width=120 height=120>Lorem ipsum</p></td></tr></table></div></td></tr>\
            <tr><td id=n>Next row</td></tr></table>";
        let (_, image_y, _, image_height) = box_of(html, "i");
        let (_, next_y, _, _) = box_of(html, "n");
        assert!(
            image_y + image_height <= next_y,
            "{image_y} {image_height} {next_y}"
        );
    }

    #[test]
    fn long_tables_keep_a_row_each() {
        let rows: String = (0..1500)
            .map(|i| format!("<tr><td>Row {i}</td></tr>"))
            .collect();
        let html =
            format!("<!DOCTYPE html><body style=margin:0><table id=t cellspacing=0>{rows}</table>");
        let (.., height) = box_of(&html, "t");
        assert!(height > 1500.0 * 15.0, "{height}");
    }

    #[test]
    fn a_caption_spans_the_table_on_top() {
        let html =
            "<!DOCTYPE html><body style=margin:0><table width=500 cellspacing=0 cellpadding=0>\
            <caption id=c>A rather long caption that must not widen a column</caption>\
            <tr><td id=a>one</td><td id=b>two</td><td>three</td></tr></table>";
        let (cx, cy, cw, _) = box_of(html, "c");
        let (ax, ay, aw, _) = box_of(html, "a");
        let (bx, _, bw, _) = box_of(html, "b");
        assert_eq!((cx, cw), (0.0, 500.0));
        assert!(cy < ay, "{cy} {ay}");
        assert_eq!(ax, 0.0);
        assert!((aw - bw).abs() < 40.0 && bx > 0.0, "{aw} {bw}");
    }

    #[test]
    fn dotted_borders_are_quads_that_cost_little() {
        let mut document =
            parse("<div style=\"width:20000px;height:10px;border:1px dotted #123\">x</div>");
        assert!(document
            .display_list()
            .commands
            .iter()
            .any(|command| matches!(command, Command::Dots { .. })));
        let rows = "<div style=\"width:600px;border-bottom:1px dotted #123\">x</div>".repeat(1500);
        let mut document = parse(&rows);
        let dots = document
            .display_list()
            .commands
            .iter()
            .filter(|command| matches!(command, Command::Dots { .. }))
            .count();
        assert_eq!(dots, 1500);
    }

    /// The solid colors that something is filled or stroked with, in `html`.
    fn solid_colors(html: &str) -> Vec<[u8; 3]> {
        parse(html)
            .display_list()
            .commands
            .iter()
            .filter_map(|command| match command {
                Command::Fill {
                    brush: crate::display_list::Brush::Solid(color),
                    ..
                }
                | Command::Stroke {
                    brush: crate::display_list::Brush::Solid(color),
                    ..
                } => {
                    let color = color.to_rgba8();
                    Some([color.r, color.g, color.b])
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn legacy_background_colors_are_a_browsers() {
        let table = "<table width=300 bgcolor=VALUE cellpadding=8><tr><td>x</td></tr></table>";
        for (value, expected) in [
            ("#abc", [0xaa, 0xbb, 0xcc]),
            ("abc", [0x0a, 0x0b, 0x0c]),
            ("#aabbcc", [0xaa, 0xbb, 0xcc]),
            ("Red", [255, 0, 0]),
        ] {
            let colors = solid_colors(&table.replace("VALUE", value));
            assert!(colors.contains(&expected), "{value}: {colors:?}");
        }
        for tag in ["div", "p", "span"] {
            let colors = solid_colors(&format!("<{tag} bgcolor=#abcdef>x</{tag}>"));
            assert!(!colors.contains(&[0xab, 0xcd, 0xef]), "{tag}");
        }
    }

    #[test]
    fn a_legacy_attribute_adds_no_declarations() {
        for html in [
            "<font color=\"#000;background-color:red\">x</font>",
            "<font face=\"a\n;background-color:red\">x</font>",
            "<font face=\"a\\\">x</font><font style=background-color:inherit>y</font>",
        ] {
            assert!(!solid_colors(html).contains(&[255, 0, 0]), "{html}");
        }
    }

    #[test]
    fn a_table_border_takes_its_color() {
        let colors = solid_colors(
            "<table border=1 bordercolor=#ff0000 width=300><tr><td bordercolor=#0000ff>b</td></tr></table>",
        );
        assert!(colors.contains(&[0, 0, 255]), "{colors:?}");
        assert!(colors.contains(&[255, 0, 0]), "{colors:?}");
    }

    #[test]
    fn a_background_attribute_paints_where_a_browser_does() {
        const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACgAAAAoCAIAAAADnC86AAAAMElEQVR4nO3NQQkAAAgEsItjRCMayxKCn8H+S/W8iFgsFovFYrFYLBaLxWKxWHxnAZEyynm2D49kAAAAAElFTkSuQmCC";
        let has_image = |html: String| {
            let mut document = parse(&html);
            let list = document.display_list();
            let drawn = list.commands.iter().any(|command| {
                matches!(
                    command,
                    Command::Fill {
                        brush: crate::display_list::Brush::Image { .. },
                        ..
                    }
                )
            });
            drawn
        };
        assert!(has_image(format!(
            "<table><tr><td background=\"{PIXEL}\" width=50 height=50>x</td></tr></table>"
        )));
        assert!(has_image(format!(
            "<table background=\"{PIXEL}\" width=50 height=50><tr><td>x</td></tr></table>"
        )));
        assert!(!has_image(format!(
            "<div background=\"{PIXEL}\" style=width:50px;height:50px>x</div>"
        )));
        // A quote doesn't leave the URL, and nothing is fetched for a relative one.
        for value in ["x.png", "a&quot;);color:red;x:url(&quot;b", ""] {
            assert!(!has_image(format!(
                "<table><tr><td background=\"{value}\" width=50 height=50>x</td></tr></table>"
            )));
        }
    }

    #[test]
    fn a_long_message_shows_its_background_images_at_once() {
        const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M/wHwAEBgIApD5fRAAAAABJRU5ErkJggg==";
        let mut document = parse(&format!(
            "<div style=\"background:url({PNG});height:9000px\">x</div>"
        ));
        // Painted at its own height, the first recording is the one kept.
        let list = document.display_list_for_height(100.0);
        assert!(list.commands.iter().any(|command| matches!(
            command,
            Command::Fill {
                brush: crate::display_list::Brush::Image { .. },
                ..
            }
        )));
    }

    #[test]
    fn text_directly_in_a_css_table_stays() {
        // Chromium: 133 x 44, centered.
        let html = "<!DOCTYPE html><body style=margin:0><a id=a href=x \
            style=\"display:table;padding:10px 20px;margin:0 auto\">Call to action</a>";
        let (_, _, width, height) = box_of(html, "a");
        assert!(width > 100.0 && height > 30.0, "{width} {height}");
        let mut document = parse(html);
        document.select_all();
        assert_eq!(document.selected_text().unwrap().trim(), "Call to action");
        // With a box of its own beside it, in one cell, and as text of a row.
        let mut document = parse(
            "<div style=display:table><div style=float:left>f</div>z <b>b</b></div>\
             <div style=display:table><div style=display:table-row>row</div></div>",
        );
        document.select_all();
        let text = document.selected_text().unwrap();
        for word in ["f", "z", "b", "row"] {
            assert!(text.contains(word), "{word}: {text}");
        }
    }

    #[test]
    fn percentages_widen_a_table_without_a_width() {
        // Chromium: 152 x 34, where each column's text stays in a line.
        let html = "<!DOCTYPE html><body style=margin:0><table id=t border=1><tr>\
            <td id=a width=\"30%\">Label</td><td id=b width=\"70%\">Value</td></tr></table>";
        let (_, _, width, height) = box_of(html, "t");
        assert!((140.0..=170.0).contains(&width), "{width}");
        assert!(height < 40.0, "{height}");
        // A column of 100% leaves nothing for the other, which takes all there is.
        let html = "<!DOCTYPE html><body style=margin:0><table id=t><tr>\
            <td width=\"100%\">a</td><td>b</td></tr></table>";
        assert!(box_of(html, "t").2 > 390.0);
    }

    #[test]
    fn a_cells_limits_bound_its_columns_ask() {
        let table = |attributes: &str, cells: &str| {
            format!(
                "<!DOCTYPE html><body style=margin:0><table id=t {attributes} border=1><tr>{cells}</tr></table>"
            )
        };
        // Chromium: 300, and the column takes the table's width past `max-width`.
        let long = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod.";
        let html = table(
            "width=300",
            &format!(
                "<td style=\"max-width:100px;overflow:hidden;white-space:nowrap\">{long}</td>"
            ),
        );
        assert_eq!(box_of(&html, "t").2, 300.0);
        let html = table(
            "width=300",
            &format!("<td id=c style=max-width:100px>{long}</td><td>b</td>"),
        );
        assert!(box_of(&html, "c").2 > 200.0);
        // Chromium: 316 and 510.
        let html = table(
            "",
            "<td style=min-width:200px>a</td><td style=min-width:100px>b</td>",
        );
        assert!((box_of(&html, "t").2 - 316.0).abs() <= 2.0);
        let html = table("width=300", "<td style=min-width:500px>a</td>");
        assert!((box_of(&html, "t").2 - 510.0).abs() <= 2.0);
    }

    #[test]
    fn every_line_of_a_long_log_keeps_its_break() {
        let height = |lines: usize| {
            let html = format!(
                "<!DOCTYPE html><body style=margin:0>{}",
                "l<br>".repeat(lines)
            );
            parse(&html).layout(640.0)
        };
        let (short, long) = (height(1000), height(31000));
        assert!((long / short - 31.0).abs() < 0.01, "{short} {long}");
        // Past the limit, and with spans costing more each, without aborting.
        parse(&"<span style=white-space:nowrap> x </span><br>".repeat(40000));
    }

    #[test]
    fn a_caption_wraps_to_its_table_and_comes_first() {
        // Chromium: 111 x 328.
        let html =
            "<!DOCTYPE html><body style=margin:0><table id=t><caption>A rather long caption \
            that must not make the table wider than a word of it</caption>\
            <tr><td id=a>one</td></tr></table>";
        assert!(box_of(html, "t").2 < 120.0);
        let mut document = parse(html);
        document.select_all();
        let text = document.selected_text().unwrap();
        assert!(
            text.find("rather").unwrap() < text.find("one").unwrap(),
            "{text}"
        );
    }

    #[test]
    fn a_stretched_dotted_border_keeps_its_shape() {
        let dots = |transform: &str| {
            parse(&format!(
                "<div style=\"width:100px;height:10px;border:2px dotted #123;transform:{transform}\">x</div>"
            ))
            .display_list()
            .commands
            .iter()
            .any(|command| matches!(command, Command::Dots { .. }))
        };
        assert!(dots("scale(2)"));
        assert!(!dots("scale(2,1)"));
    }

    #[test]
    fn text_past_a_cells_height_can_be_scrolled_to() {
        let lines = "<br>line".repeat(40);
        let html = format!(
            "<!DOCTYPE html><body style=margin:0><table><tr><td height=50><div style=height:50px>{lines}</div></td></tr></table>"
        );
        let mut document = HtmlDocument::parse(&html, DocumentOptions::default());
        assert!(document.layout(640.0) > 500.0);
    }

    #[test]
    fn only_what_can_animate_pays_for_stopping_it() {
        assert!(!may_animate("<p style=color:red>hello</p>"));
        for html in [
            "@KEYFRAMES a{}",
            "<p style=transition:all>",
            "@import 'x'",
            "@keyfr\\61mes a{}",
        ] {
            assert!(may_animate(html), "{html}");
        }
    }

    #[test]
    fn budgeted_painters_stop_early() {
        let html = format!(
            "<style>i{{position:absolute;left:0;top:0;width:600px;height:30000px;border:3px dotted #123;\
             border-radius:50%}}</style>{}",
            "<i></i>".repeat(8000)
        );
        let mut document = HtmlDocument::parse(&html, DocumentOptions::default());
        document.layout(640.0);
        let start = std::time::Instant::now();
        document.display_list();
        assert!(start.elapsed().as_secs() < 3, "{:?}", start.elapsed());
    }

    #[test]
    fn a_long_list_of_families_is_cut() {
        let families = vec!["a"; 60_000].join(",");
        let html = format!(
            "<style>p{{font-family:{families}}}</style>{}",
            "<p>x y".repeat(4000)
        );
        let start = std::time::Instant::now();
        parse(&html);
        assert!(start.elapsed().as_secs() < 3, "{:?}", start.elapsed());
    }

    #[test]
    fn animations_dont_start_invisible() {
        let html = "<style>@keyframes a{from{opacity:0}}p{animation:a 1s}</style><p>Seen</p>";
        let mut document = parse(html);
        assert!(format!("{:?}", document.display_list().commands).contains("Seen"));
    }
}
