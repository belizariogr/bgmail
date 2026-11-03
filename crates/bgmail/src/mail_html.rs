//! E-mail HTML for the reader: sanitizes message bodies and wraps them in a
//! themed document that `html_view` renders natively.
//!
//! Sanitizing stays even though `html_view` runs no scripts and fetches
//! nothing remote: it strips content a reader must not show (forms, frames,
//! media, event handlers, dangerous URL schemes) and records which remote
//! images a message has, so the reader can say what was blocked.

use std::collections::HashSet;

use gpui::{Hsla, Rgba};
use lol_html::html_content::Element;

use crate::data::MessageBody;

/// A rendered e-mail document plus the privacy metadata the reader needs to
/// badge the message (blocked remote content).
pub struct RenderedEmail {
    /// The full HTML document handed to `html_view`.
    pub html: String,
    /// How many remote `<img>` were blocked.
    pub blocked_images: usize,
    /// The sanitized body didn't read as safe (see [`reads_as_safe`]), so it became text.
    pub refused: bool,
}

/// Theme colors of the message page. The page can be forced white
/// independently of the app theme (`reader_white_background`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DocumentColors {
    pub background: Hsla,
    pub text: Hsla,
    pub accent: Hsla,
}

impl DocumentColors {
    /// Whether the page is dark (drives `prefers-color-scheme`).
    pub fn is_dark(&self) -> bool {
        self.background.l < 0.5
    }

    /// What `html_view` needs to keep text readable, on a dark page.
    pub fn dark_page(&self) -> Option<html_view::DarkPage> {
        self.is_dark().then(|| html_view::DarkPage {
            background: rgba8(self.background),
            text: rgba8(self.text),
        })
    }
}

fn rgba8(color: Hsla) -> [u8; 4] {
    let rgba: gpui::Rgba = color.into();
    [rgba.r, rgba.g, rgba.b, rgba.a].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Builds a self-contained HTML document for `body`, themed to match the
/// current app colors. Plain-text bodies are HTML-escaped and wrapped so they
/// keep their line breaks and wrap to the pane width. Remote resources are
/// kept only with `load_remote` (the user allowed them, for this message or
/// always); otherwise they are blocked and counted.
pub fn email_document(
    colors: DocumentColors,
    body: &MessageBody,
    load_remote: bool,
) -> RenderedEmail {
    let (mut inner, _, mut blocked_images) = match body {
        MessageBody::Html(html) => sanitize_html_inner(html, load_remote, &HashSet::new()),
        MessageBody::Text(plain) => (
            format!("<pre class=\"plain\">{}</pre>", escape_html(plain)),
            false,
            0,
        ),
    };
    let refused = matches!(body, MessageBody::Html(_)) && !reads_as_safe(&inner);
    if let (true, MessageBody::Html(html)) = (refused, body) {
        inner = format!(
            "<pre class=\"plain\">{}</pre>",
            escape_html(&plain_text(html))
        );
        blocked_images = 0;
    }
    // Keep the message's own doctype: a legacy one selects the (quirks)
    // rendering mode its layout was written for.
    let doctype = match body {
        // A doctype holding a `<` is no doctype but markup, which could displace the
        // tags this page relies on (its Content-Security-Policy, for one).
        MessageBody::Html(html) => {
            leading_doctype(html).filter(|doctype| !doctype[1..].contains('<'))
        }
        MessageBody::Text(_) => None,
    }
    .unwrap_or("<!DOCTYPE html>");
    let html = format!(
        "{doctype}<html><head><meta charset=\"utf-8\">\
         <style>{css}</style></head><body>{inner}</body></html>",
        css = document_css(colors),
    );
    RenderedEmail {
        html,
        blocked_images,
        refused,
    }
}

/// What a printed message shows above its body (labels already localized).
pub struct PrintHeader<'a> {
    pub subject: &'a str,
    pub from_label: &'a str,
    pub from: &'a str,
    pub date_label: &'a str,
    pub date: &'a str,
}

/// Light page colors for printing, whatever the app theme.
const PRINT_COLORS: DocumentColors = DocumentColors {
    background: Hsla {
        h: 0.0,
        s: 0.0,
        l: 1.0,
        a: 1.0,
    },
    text: Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.1,
        a: 1.0,
    },
    accent: Hsla {
        h: 0.6,
        s: 0.8,
        l: 0.4,
        a: 1.0,
    },
};

/// A standalone page for printing a message in the system browser: the
/// sanitized body (remote content still blocked) under a header, on a light
/// page, opening the print dialog once loaded. A Content-Security-Policy
/// lets nothing load but `data:` images and only the page's own script run
/// (`nonce`), so the browser stays as offline as the reader even if the
/// sanitizer missed something.
pub fn print_document(
    header: &PrintHeader,
    body: &MessageBody,
    nonce: &str,
    load_remote: bool,
) -> String {
    // As in the reader, what is too costly to sanitize prints as text.
    let as_text = match body {
        MessageBody::Html(html) if html_view::has_too_many_tags(html) => {
            Some(MessageBody::Text(plain_text(html).into()))
        }
        _ => None,
    };
    let rendered = email_document(PRINT_COLORS, as_text.as_ref().unwrap_or(body), load_remote);
    // Remote images the user allowed for this message print too.
    let images = if load_remote {
        "data: http: https:"
    } else {
        "data:"
    };
    let csp = format!(
        "default-src 'none'; img-src {images}; style-src 'unsafe-inline'; \
         script-src 'nonce-{nonce}'; form-action 'none'; base-uri 'none'"
    );
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\
         <title>{subject}</title>\
         <style>.bgmail-print-header{{font:13px system-ui,sans-serif;color:#222;\
         margin:0 0 16px;padding:0 0 12px;border-bottom:1px solid #ccc}}\
         .bgmail-print-header h1{{font-size:18px;margin:0 0 8px}}\
         .bgmail-print-header p{{margin:2px 0}}</style>\
         <script nonce=\"{nonce}\">addEventListener('load',function(){{print()}})</script>",
        subject = escape_html(header.subject),
    );
    let header = format!(
        "<header class=\"bgmail-print-header\"><h1>{}</h1><p><b>{}:</b> {}</p>\
         <p><b>{}:</b> {}</p></header>",
        escape_html(header.subject),
        escape_html(header.from_label),
        escape_html(header.from),
        escape_html(header.date_label),
        escape_html(header.date),
    );
    // The CSP must precede everything else in <head>; the header opens <body>.
    rendered
        .html
        .replacen("<head>", &format!("<head>{head}"), 1)
        .replacen("<body>", &format!("<body>{header}"), 1)
}

/// The doctype an HTML body starts with (after a BOM, whitespace, an XML
/// declaration and comments), if any.
/// The original message's HTML as it goes inside a reply's quote: sanitized,
/// only what is in its body, and without `cid:` images, which point at parts
/// of the original that a reply doesn't carry. Empty if it can't be processed.
pub fn quotable_html(html: &str) -> String {
    use lol_html::{element, rewrite_str, RewriteStrSettings};

    let (sanitized, _, _) = sanitize_html_inner(html, true, &HashSet::new());
    let settings = RewriteStrSettings::new()
        // By the tokenizer: cutting where a text search found one could open a comment.
        .append_document_content_handler(lol_html::doctype!(|doctype| {
            doctype.remove();
            Ok(())
        }))
        // With what follows it when unclosed, as it would take the whole body.
        .append_element_content_handler(element!("head", |el| {
            el.remove_and_keep_content();
            Ok(())
        }))
        .append_element_content_handler(element!("title, head style", |el| {
            el.remove();
            Ok(())
        }))
        .append_element_content_handler(element!("html, body", |el| {
            el.remove_and_keep_content();
            Ok(())
        }))
        .append_element_content_handler(element!("img", |el| {
            let inline = el
                .get_attribute("src")
                .is_some_and(|src| src.trim().to_ascii_lowercase().starts_with("cid:"));
            if inline {
                el.remove();
            }
            Ok(())
        }));
    let quoted = rewrite_str(&sanitized, settings).unwrap_or_default();
    if reads_as_safe(&quoted) {
        quoted
    } else {
        String::new()
    }
}

/// Whether `html`, followed by what closes the page around it, reads to the parser that will show
/// it as free of what the sanitizer removes. The last check: rewriting tags in place can make the
/// output read differently from what was checked (each removal joins what was around it), and
/// no list of such cases has held. Tokens, not a tree: the modes where a tree builder reads
/// raw text differently are only entered through elements this already refuses.
pub fn reads_as_safe(html: &str) -> bool {
    use html5ever::tendril::StrTendril;
    use html5ever::tokenizer::{
        states::RawKind, BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer,
        TokenizerOpts,
    };

    #[derive(Default)]
    struct Check(std::cell::Cell<bool>);
    impl TokenSink for Check {
        type Handle = ();
        fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
            let Token::TagToken(tag) = token else {
                return TokenSinkResult::Continue;
            };
            if tag.kind != TagKind::StartTag {
                return TokenSinkResult::Continue;
            }
            let name = &*tag.name;
            let unsafe_attribute = tag.attrs.iter().any(|attr| {
                let attribute = &*attr.name.local;
                attribute.starts_with("on")
                    || (URL_ATTRIBUTES.contains(&attribute) && is_dangerous_url(&attr.value))
            });
            let unsafe_element = name == "svg"
                || name == "math"
                || DISALLOWED_ELEMENTS
                    .split(',')
                    .chain(WRAPPING_CONTROLS.split(','))
                    .any(|element| element.trim() == name);
            if unsafe_attribute || unsafe_element {
                self.0.set(true);
            }
            // As the tree builder does, or the text of these would read as tags.
            match name {
                "title" => TokenSinkResult::RawData(RawKind::Rcdata),
                "style" | "xmp" | "noembed" | "noframes" => {
                    TokenSinkResult::RawData(RawKind::Rawtext)
                }
                "plaintext" => TokenSinkResult::Plaintext,
                _ => TokenSinkResult::Continue,
            }
        }
    }
    let tokenizer = Tokenizer::new(Check::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    input.push_back(StrTendril::from_slice("</body></html>"));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    !tokenizer.sink.0.get()
}

/// A plain-text original as it goes inside a reply's quote.
pub fn quotable_text(text: &str) -> String {
    text.lines()
        .map(escape_html)
        .collect::<Vec<_>>()
        .join("<br>")
}

fn leading_doctype(html: &str) -> Option<&str> {
    let mut rest = html.trim_start_matches('\u{feff}').trim_start();
    loop {
        if rest.starts_with("<?") {
            rest = rest[rest.find("?>")? + 2..].trim_start();
        } else if rest.starts_with("<!--") {
            rest = rest[rest.find("-->")? + 3..].trim_start();
        } else {
            break;
        }
    }
    let head = rest.get(..9)?;
    if !head.eq_ignore_ascii_case("<!doctype") {
        return None;
    }
    Some(&rest[..rest.find('>')? + 1])
}

/// Whether a link clicked in a message may be opened (in the system browser
/// or mail handler): web and `mailto:` links only. Message-relative URLs,
/// `javascript:` and every other scheme are ignored.
pub fn is_openable_link(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

/// Theme-aware stylesheet shared by every rendered message, so the page
/// matches the surrounding UI (incl. dark mode) instead of a default white page.
fn document_css(colors: DocumentColors) -> String {
    let DocumentColors {
        background,
        text,
        accent,
    } = colors;
    let scheme = if colors.is_dark() { "dark" } else { "light" };
    format!(
        ":root {{ color-scheme: {scheme}; }}\
         html {{ background: {bg}; }}\
         body {{ margin: 0; padding: 16px 24px; background: {bg}; color: {fg}; \
           font: 16px/1.5 -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; \
           overflow-wrap: anywhere; }}\
         a {{ color: {accent}; }}\
         img[data-rm-blocked-src] {{ background: {soft}; border: 1px dashed {line}; \
           box-sizing: border-box; min-width: 24px; min-height: 24px; }}\
         h1, h2, h3 {{ line-height: 1.25; }}\
         code, pre {{ font-family: 'SF Mono', ui-monospace, Menlo, Consolas, monospace; font-size: 13px; }}\
         pre {{ white-space: pre-wrap; background: {soft}; padding: 12px; border-radius: 6px; }}\
         pre.plain {{ background: transparent; padding: 0; font: inherit; }}\
         code {{ background: {soft}; padding: 1px 4px; border-radius: 4px; }}\
         blockquote {{ margin: 0; padding-left: 12px; border-left: 3px solid {accent}; opacity: 0.85; }}\
         hr {{ border: none; border-top: 1px solid {fg}; opacity: 0.15; margin: 16px 0; }}",
        bg = css_color(background),
        fg = css_color(text),
        accent = css_color(accent),
        // A subtle fill for code blocks/inline code, derived from the text color.
        soft = css_color_alpha(text, 0.08),
        // A slightly stronger line for the blocked-image placeholder border.
        line = css_color_alpha(text, 0.3),
    )
}

/// Formats a theme color as an opaque CSS `rgb(...)` string.
fn css_color(color: Hsla) -> String {
    let rgba: Rgba = color.into();
    format!(
        "rgb({}, {}, {})",
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
    )
}

/// Formats a theme color as a translucent CSS `rgba(...)` string.
fn css_color_alpha(color: Hsla, alpha: f32) -> String {
    let rgba: Rgba = color.into();
    format!(
        "rgba({}, {}, {}, {:.3})",
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        alpha.clamp(0.0, 1.0),
    )
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// CSS selector matching every element we refuse to render in an e-mail body.
///
/// These are stripped (element *and* their content) before the HTML ever reaches
/// the renderer, so the elements never exist in the DOM:
/// - **Active/embedding content** (`script`, `object`, `embed`, `applet`):
///   e-mail must never execute code or load plugins.
/// - **Frames** (`iframe`, `frame`, `frameset`): sub-documents the reader
///   doesn't load; they would only leave empty boxes.
/// - **Media** (`video`, `audio`, `source`, `track`): a reader has no use for
///   embedded players.
/// - **Editable/interactive controls** (`input`, `textarea`, `select`, `button`,
///   `form`): a reader is not a form host; stripping them avoids dead,
///   confusing affordances.
/// - **Document/redirect/external-resource heads** (`base`, `meta`, `link`):
///   `base` rewrites every relative link, `meta[http-equiv=refresh]` redirects the
///   view, and `link` pulls remote stylesheets/prefetch (tracking). `head`/`title`
///   are deliberately *kept* so e-mails that ship a full document don't lose their
///   `<style>`.
/// - **Misc scripting surfaces** (`canvas`, `dialog`, `portal`).
///
/// `svg` and `math` go in passes of their own (see [`without_foreign_content`], user
/// decision). The SVG parts stay listed for a stray tag outside one: `foreignobject`
/// (hosts arbitrary HTML) and the SMIL animation family (`animate`, `animatetransform`,
/// `animatemotion`, `set`), which can assign event-handler attributes at runtime.
/// Form controls that only wrap what a message says: they become plain boxes, so what they hold
/// stays, and stays as hidden or styled as they made it. A whole ASP.NET message comes in a
/// `<form>`, and Gmail and Thunderbird show its content.
const WRAPPING_CONTROLS: &str = "form, button";

const DISALLOWED_ELEMENTS: &str = "script, object, embed, applet, \
     iframe, frame, frameset, \
     video, audio, source, track, \
     input, textarea, select, \
     base, meta, link, canvas, dialog, portal, \
     foreignobject, animate, animatetransform, animatemotion, set, \
     template, noscript";

/// URL-bearing attributes whose value we vet against dangerous schemes.
const URL_ATTRIBUTES: &[&str] = &[
    "href",
    "src",
    "action",
    "formaction",
    "xlink:href",
    "poster",
    "background",
    "cite",
    "data",
    "ping",
    "longdesc",
];

/// URL-bearing attributes that *auto-fetch a remote resource when the document
/// renders* (so they leak an "opened" signal / the reader's IP). These are the
/// only ones blocked for remoteness. Navigation and metadata URLs — `href`
/// (links), `cite`, `ping`, `longdesc`, `action` — act on click or fetch
/// nothing on render, so they are kept (a blocked `href` would break links).
/// `srcset` is handled separately. Dangerous-scheme vetting still applies to
/// every [`URL_ATTRIBUTES`] entry regardless.
const RESOURCE_ATTRIBUTES: &[&str] = &["src", "poster", "background", "data", "xlink:href"];

/// `html` with no `svg` or `math`, in passes of their own, before anything else reads
/// it. Once it has seen one of these tags the parser stays in "foreign" mode until its
/// own end tag, where a browser leaves it at any HTML end tag (`<div><svg></div>`), so
/// from there on the two read different tags: whatever it takes for text, like a
/// `<style>`'s content, could be real elements nothing has checked. Passes until none
/// is left, as a removal can join the pieces of a tag. Anything that doesn't settle is
/// dropped.
fn without_foreign_content(html: &str) -> std::borrow::Cow<'_, str> {
    use lol_html::{element, rewrite_str, RewriteStrSettings};
    const MAX_PASSES: usize = 8;

    let has_tag = |text: &str| {
        let lower = text.to_ascii_lowercase();
        lower.contains("<svg") || lower.contains("<math")
    };
    if !has_tag(html) {
        return html.into();
    }
    let mut current = html.to_string();
    for _ in 0..MAX_PASSES {
        let settings =
            RewriteStrSettings::new().append_element_content_handler(element!("svg, math", |el| {
                el.remove();
                Ok(())
            }));
        let next = rewrite_str(&current, settings).unwrap_or_default();
        // Still there but unchanged: inside text or a `<style>`, where it isn't a tag.
        if next == current || !has_tag(&next) {
            return next.into();
        }
        current = next;
    }
    "".into()
}

/// Sanitizes an e-mail HTML fragment so it can never execute code or load
/// untrusted active content in the renderer.
///
/// A real streaming parser ([`lol_html`]) is used rather than regex so malformed
/// markup can't smuggle anything through. Two passes run on every element:
/// 1. Disallowed elements (see [`DISALLOWED_ELEMENTS`]) are dropped together with
///    their content.
/// 2. Surviving elements have their XSS-bearing attributes neutralized
///    (see [`neutralize_attributes`]).
///
/// `load_remote` is a user preference: when false, attributes pointing at remote
/// (`http(s)`/protocol-relative) resources are stripped so e-mails can't load
/// tracking pixels; inline `data:` images are kept regardless. XSS neutralization
/// happens either way.
///
/// Everything else — including inline `style`, tables and links — is preserved
/// verbatim. If rewriting fails for any reason we drop the body entirely rather
/// than risk rendering unsanitized content.
#[cfg(test)]
fn sanitize_html(html: &str, load_remote: bool) -> String {
    sanitize_html_inner(html, load_remote, &HashSet::new()).0
}

/// Like [`sanitize_html`], but also reports the message's remote-image state:
/// whether it has any remote `<img>` and how many are still blocked. A remote
/// image is shown (not blocked) when `load_remote` is true or its URL is in
/// `shown` (individually revealed by the user); blocked images stash their URL
/// in `data-rm-blocked-src` (a placeholder box, and where a Stage 3 "show
/// remote image" action will read it).
fn sanitize_html_inner(
    html: &str,
    load_remote: bool,
    shown: &HashSet<String>,
) -> (String, bool, usize) {
    use lol_html::html_content::ContentType;
    use lol_html::{element, rewrite_str, text, RewriteStrSettings};
    use std::cell::{Cell, RefCell};

    // Closes a tag left open at the end, which the tokenizer writes back unchecked and what
    // follows this body in a page would close. The handler below takes it out again.
    let closed = format!("{html}<!---->");
    let without_foreign = without_foreign_content(&closed);
    let has_remote = Cell::new(false);
    let blocked = Cell::new(0usize);
    // `<style>` text arrives in chunks (a `url(` may straddle two): buffer it
    // and emit it once, stripped, at the end of the text node.
    let style_text = RefCell::new(String::new());
    let settings = RewriteStrSettings::new()
        // A `<` the tokenizer took for text is written back as it came, and what is removed
        // after it can leave it next to a name: `<<script></script>script>` came out as
        // `<script>`. As an entity, it stays text for whoever reads the output.
        .append_document_content_handler(lol_html::doc_comments!(|comment| {
            if comment.text().is_empty() {
                comment.remove();
            }
            Ok(())
        }))
        .append_document_content_handler(lol_html::doc_text!(|chunk| {
            if chunk.text_type() == lol_html::html_content::TextType::Data
                && chunk.as_str().contains('<')
            {
                let escaped = chunk.as_str().replace('<', "&lt;");
                chunk.replace(&escaped, ContentType::Html);
            }
            Ok(())
        }))
        .append_element_content_handler(element!(DISALLOWED_ELEMENTS, |el| {
            el.remove();
            Ok(())
        }))
        .append_element_content_handler(element!("*", |el| {
            if let Some(is_blocked) = neutralize_attributes(el, load_remote, shown) {
                has_remote.set(true);
                if is_blocked {
                    blocked.set(blocked.get() + 1);
                }
            }
            Ok(())
        }))
        // CSS `url(...)` (background images, web fonts, `@import`) also fetches
        // remote resources: blank it where CSS lives (inline `style` and
        // `<style>` blocks), never in the message's text.
        .append_element_content_handler(element!("[style]", |el| {
            if !load_remote {
                if let Some(style) = el.get_attribute("style") {
                    el.set_attribute("style", &strip_css_urls(&style))?;
                }
            }
            Ok(())
        }))
        // After the handlers above, which clean the attributes this copies. Written out, as
        // renaming the element in place also renames whichever end tag comes next: in
        // `<table><form>x</table>` the table's own, leaving it open with all that follows inside.
        // An element left open stays so: it ends where its parent does, as the `<form>` would.
        .append_element_content_handler(element!(WRAPPING_CONTROLS, |el| {
            if el.removed() {
                return Ok(());
            }
            let control = el.tag_name();
            let plain = if control == "button" {
                "span"
            } else {
                "section"
            };
            let attributes: String = el
                .attributes()
                .iter()
                .filter(|attr| {
                    attr.name()
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                })
                .map(|attr| {
                    let value = escape_html(&decoded_attribute(&attr.value()));
                    format!(" {}=\"{value}\"", attr.name())
                })
                .collect();
            el.before(&format!("<{plain}{attributes}>"), ContentType::Html);
            el.remove_and_keep_content();
            el.on_end_tag(lol_html::end_tag!(move |end| {
                let closing = if end.name() == control {
                    plain.to_string()
                } else {
                    end.name()
                };
                end.before(&format!("</{closing}>"), ContentType::Html);
                Ok(())
            }))?;
            Ok(())
        }))
        .append_element_content_handler(text!("style", |chunk| {
            if load_remote {
                return Ok(());
            }
            style_text.borrow_mut().push_str(chunk.as_str());
            if chunk.last_in_text_node() {
                let css = strip_css_urls(&style_text.take());
                chunk.replace(&css, ContentType::Html);
            } else {
                chunk.remove();
            }
            Ok(())
        }));

    let mut html = rewrite_str(&without_foreign, settings).unwrap_or_default();
    // Inside an unclosed `<xmp>` or `<title>` it was text, not a comment.
    if html.ends_with("<!---->") {
        html.truncate(html.len() - "<!---->".len());
    }
    (html, has_remote.get(), blocked.get())
}

/// Empties the contents of every CSS `url(...)` in `css` (an inline `style`
/// value or a `<style>` block) — `url(http://x.png)` becomes `url()` — so
/// stylesheets can't fetch remote resources. This is a deliberately blunt
/// textual pass (equivalent to the regex `url\([^)]*\)` →
/// `url()`); it does not parse CSS and makes no attempt to tell remote from
/// inline `data:` URLs, since when remote loading is off neither should fetch.
fn strip_css_urls(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(idx) = find_url_open(rest) {
        out.push_str(&rest[..idx]);
        out.push_str("url(");
        let after = &rest[idx + 4..];
        match after.find(')') {
            Some(close) => {
                out.push(')');
                rest = &after[close + 1..];
            }
            // Unterminated `url(`: drop everything that follows.
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Byte index of the next case-insensitive `url(` in `s`, if any. `url(` is ASCII,
/// so the index is always a valid char boundary.
fn find_url_open(s: &str) -> Option<usize> {
    s.as_bytes().windows(4).position(|w| {
        w[0].eq_ignore_ascii_case(&b'u')
            && w[1].eq_ignore_ascii_case(&b'r')
            && w[2].eq_ignore_ascii_case(&b'l')
            && w[3] == b'('
    })
}

/// Strips the unwanted attributes from a *surviving* element while keeping the
/// element itself. Always removed:
/// - every inline event handler (`on*`),
/// - `contenteditable` (a reader is not an editor),
/// - link/source attributes whose value uses a dangerous scheme
///   (see [`is_dangerous_url`]),
/// - `style` declarations carrying legacy CSS script vectors
///   (see [`style_has_script_vector`]).
///
/// When `load_remote` is false, also removed: link/source attributes (and
/// `srcset`) pointing at remote resources, so the message can't phone home. A
/// blocked `<img src>` is special-cased: its remote URL is preserved in
/// `data-rm-blocked-src` (the `src` is still dropped, so nothing loads) for a
/// placeholder and a later "show remote image" action.
/// Remote images whose URL is in `shown` are let through (the user revealed
/// them individually), even while `load_remote` is false.
///
/// Returns the element's remote-image state: `None` if it is not a remote
/// `<img>`, or `Some(blocked)` where `blocked` reports whether that image's
/// remote `src` was withheld this render.
fn neutralize_attributes(
    el: &mut Element,
    load_remote: bool,
    shown: &HashSet<String>,
) -> Option<bool> {
    // Removing them by name is quadratic in lol_html: 300000 took 89 s, on the UI thread when
    // replying. No such tag is meant to be read. With its content, which keeping would turn
    // the text of a `<xmp>` into markup after it was checked.
    if el.attributes().len() > html_view::MAX_TAG_ATTRIBUTES {
        el.remove();
        return None;
    }
    let is_img = el.tag_name() == "img";
    let is_svg_image = matches!(el.tag_name().as_str(), "image" | "feimage");
    // `attributes()` borrows the element, so collect everything we need first and
    // only mutate (`remove_attribute`/`set_attribute`) once the borrow is released.
    let mut blocked_img_src = None;
    let mut img_state: Option<bool> = None;
    let doomed: Vec<String> = el
        .attributes()
        .iter()
        .filter_map(|attr| {
            let name = attr.name(); // lower-cased by the parser
            let raw_value = attr.value();
            // As a browser will read it: `&#106;avascript:` is `javascript:`.
            let value = decoded_attribute(&raw_value);
            let is_url_attr = URL_ATTRIBUTES.contains(&name.as_str());
            // A remote `<img src>` is what the reader surfaces and lets the user
            // unblock one at a time, so track it (and honor the `shown` allowlist).
            let img_src_remote = is_img && name == "src" && is_url_attr && is_remote_url(&value);
            let shown_here =
                img_src_remote && (shown.contains(&raw_value) || shown.contains(value.as_str()));
            if img_src_remote {
                let blocked = !load_remote && !shown_here;
                img_state = Some(blocked);
                if blocked {
                    blocked_img_src = Some(raw_value.clone());
                }
            }
            // Only resource-loading attributes are blocked for remoteness;
            // navigation/metadata URLs (e.g. a link's `href`) are left intact.
            let is_remote = (RESOURCE_ATTRIBUTES.contains(&name.as_str()) && is_remote_url(&value))
                || (name == "srcset" && !value.trim().is_empty());
            let remote_blocked = is_remote && !load_remote && !shown_here;
            // A table's or the body's image, which the reader offers to show like any other.
            if name == "background" && is_remote {
                img_state.get_or_insert(!load_remote);
            }
            // Nothing in an SVG reads outside the message: its images come inline.
            let svg_file = is_svg_image
                && matches!(name.as_str(), "href" | "xlink:href")
                && !is_inline_raster(&value);
            let drop = name.starts_with("on")
                || name == "contenteditable"
                || svg_file
                || (is_url_attr && is_dangerous_url(&value))
                || (name == "style" && style_has_script_vector(&value))
                || remote_blocked;
            drop.then_some(name)
        })
        .collect();

    // A name can start with `=` only as the first; past it, rewriting the tag without what
    // stood between makes it the value of the attribute before, and the rest of the tag reads as
    // something else. Untouched, a tag comes out as it came. Without the tag, then, keeping what
    // it holds, unless that is raw text, which keeping would turn into markup.
    let rewritten = !doomed.is_empty() || blocked_img_src.is_some() || el.has_attribute("style");
    let joins = rewritten
        && el
            .attributes()
            .iter()
            .skip(1)
            .any(|attr| attr.name().starts_with('='));
    if joins {
        if matches!(
            el.tag_name().as_str(),
            "title" | "style" | "xmp" | "noembed" | "noframes" | "plaintext"
        ) {
            el.remove();
        } else {
            el.remove_and_keep_content();
        }
        return img_state;
    }
    for name in doomed {
        el.remove_attribute(&name);
    }
    if let Some(src) = blocked_img_src {
        let _ = el.set_attribute("data-rm-blocked-src", &src);
    }
    img_state
}

/// An attribute value with its character references decoded, which the parser leaves
/// as written. Numeric ones count without their `;`, as in browsers.
fn decoded_attribute(value: &str) -> String {
    if !value.contains('&') {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find("&#") {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 2..];
        let (radix, digits) = match after.strip_prefix(['x', 'X']) {
            Some(hex) => (16, hex),
            None => (10, after),
        };
        let end = digits
            .find(|c: char| !c.is_digit(radix))
            .unwrap_or(digits.len());
        match u32::from_str_radix(&digits[..end], radix)
            .ok()
            .and_then(char::from_u32)
        {
            Some(decoded) => {
                out.push(decoded);
                let tail = &digits[end..];
                rest = tail.strip_prefix(';').unwrap_or(tail);
            }
            None => {
                out.push_str("&#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    html_escape::decode_html_entities(&out).into_owned()
}

/// Whether `value` is a `data:` raster image.
fn is_inline_raster(value: &str) -> bool {
    value
        .trim_start()
        .get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
        && !is_dangerous_url(value)
}

/// Whether a URL value uses a scheme that can execute or load active content.
///
/// `javascript:`/`vbscript:` always execute; `data:` is blocked unless it is a
/// raster image (inline `<img>` payloads are common and inert), with SVG excluded
/// since an SVG document can script. Embedded whitespace/control characters in the
/// scheme (e.g. `java\tscript:`) are stripped first, mirroring how engines parse it.
fn is_dangerous_url(value: &str) -> bool {
    let trimmed = value.trim_start();
    let Some(colon) = trimmed.find(':') else {
        // No scheme: relative path, fragment or query — always safe.
        return false;
    };

    let scheme: String = trimmed[..colon]
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();

    match scheme.as_str() {
        "javascript" | "vbscript" => true,
        "data" => {
            // As a browser reads it: `image/svg+\nxml` is `image/svg+xml`.
            let media = trimmed[colon + 1..]
                .split([';', ','])
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_whitespace() && !c.is_control())
                .collect::<String>()
                .to_ascii_lowercase();
            !(media.starts_with("image/") && media != "image/svg+xml")
        }
        _ => false,
    }
}

/// Whether a URL value loads a resource from the network (so it leaks an "opened"
/// signal / IP address when rendered). Covers explicit `http`/`https` schemes and
/// protocol-relative URLs (`//host/...`). Inline `data:`, `cid:` (embedded
/// attachments), fragments and relative paths are treated as local/safe.
fn is_remote_url(value: &str) -> bool {
    let trimmed = value.trim_start();
    if trimmed.starts_with("//") {
        return true;
    }
    let Some(colon) = trimmed.find(':') else {
        return false;
    };
    let scheme: String = trimmed[..colon]
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https")
}

/// Whether an inline `style` value carries a legacy CSS script vector. These are
/// inert on modern WebKit/Chromium, but stripping them is cheap defense-in-depth
/// for older engines and an explicit signal of intent.
fn style_has_script_vector(style: &str) -> bool {
    let lower = style.to_ascii_lowercase();
    ["javascript:", "expression(", "-moz-binding", "behavior:"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Readable text of an HTML body, for the plain-text fallback (huge,
/// too-deep or crashing mail). A single linear scan, no tree: tags and
/// comments are dropped, `script`/`style`/`head`/`title` content skipped,
/// block elements start new lines, entities decoded, whitespace collapsed
/// (kept inside `<pre>`).
pub fn plain_text(html: &str) -> String {
    // Blocks separated by a blank line, and those that only start a line.
    const PARAGRAPHS: &[&str] = &[
        "p",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "table",
        "blockquote",
        "pre",
        "hr",
        "ul",
        "ol",
    ];
    const LINES: &[&str] = &[
        "div", "br", "tr", "li", "section", "article", "header", "footer", "dt", "dd",
    ];
    const SKIPPED: &[&str] = &["script", "style", "head", "title"];
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    let mut pre = 0usize;
    // Ends the current line; `blank` also leaves an empty line after it.
    let push_break = |out: &mut String, blank: bool| {
        while out.ends_with([' ', '\t']) {
            out.pop();
        }
        if out.is_empty() {
            return;
        }
        let wanted = if blank { "\n\n" } else { "\n" };
        while !out.ends_with(wanted) {
            if out.ends_with("\n\n") {
                break;
            }
            out.push('\n');
        }
    };
    while !rest.is_empty() {
        if let Some(comment) = rest.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        if rest.starts_with('<') {
            // An unclosed tag runs to the end (its last character may be
            // multi-byte, so never cut one byte short of the end).
            let close = rest.find('>');
            let tag = &rest[1..close.unwrap_or(rest.len())];
            rest = &rest[close.map_or(rest.len(), |i| i + 1)..];
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if !closing && SKIPPED.contains(&name.as_str()) {
                // Case-insensitive search without lower-casing the rest of
                // the body per skipped tag (quadratic on huge mail).
                let close = format!("</{name}");
                rest = find_ascii_ci(rest, &close).map_or("", |at| {
                    let after = &rest[at..];
                    after.find('>').map_or("", |gt| &after[gt + 1..])
                });
                continue;
            }
            if name == "pre" {
                pre = if closing {
                    pre.saturating_sub(1)
                } else {
                    pre + 1
                };
            }
            if PARAGRAPHS.contains(&name.as_str()) {
                push_break(&mut out, true);
            } else if LINES.contains(&name.as_str()) {
                push_break(&mut out, false);
            } else if (name == "td" || name == "th")
                && !closing
                && !out.is_empty()
                && !out.ends_with(['\n', ' ', '\t'])
            {
                out.push('\t');
            }
            continue;
        }
        let text_end = rest.find('<').unwrap_or(rest.len());
        let text = decode_entities(&rest[..text_end]);
        rest = &rest[text_end..];
        if pre > 0 {
            out.push_str(&text);
        } else {
            for word in text.split_whitespace() {
                if !out.is_empty() && !out.ends_with(['\n', ' ', '\t']) {
                    out.push(' ');
                }
                out.push_str(word);
            }
            if text.ends_with(char::is_whitespace)
                && !out.ends_with(['\n', ' ', '\t'])
                && !out.is_empty()
            {
                out.push(' ');
            }
        }
    }
    out.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Byte index of the first ASCII-case-insensitive occurrence of `needle` (an
/// ASCII string) in `haystack`; always a char boundary.
fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Decodes the HTML entities mail text commonly uses: the named basics,
/// `&nbsp;` and numeric references. Unknown entities are kept as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // Searched as bytes: a 12-byte cut may fall inside a multi-byte
        // character, and `;` is ASCII, so its index is a char boundary.
        let window = &rest.as_bytes()[..rest.len().min(12)];
        let Some(semi) = window.iter().position(|&b| b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .filter(|dec| dec.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|dec| dec.parse().ok())
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escapes the characters that are significant in HTML text content.
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_doctype_holding_markup_cannot_move_the_print_policy() {
        let body = MessageBody::Html(
            "<!DOCTYPE <body><math><mtext><mglyph><style><img src=x onerror=alert(1)>".into(),
        );
        let header = PrintHeader {
            subject: "s",
            from_label: "f",
            from: "a",
            date_label: "d",
            date: "t",
        };
        let page = print_document(&header, &body, "nonce", false);
        assert!(!page.to_ascii_lowercase().starts_with("<!doctype <"));
        let policy = page.find("Content-Security-Policy").expect("policy");
        assert!(policy < page.find("</head>").expect("head"));
        assert!(page.find("</head>") < page.find("<body>"));
        assert!(!page.contains("onerror"));
    }

    #[test]
    fn text_with_cdata_markers_is_left_alone() {
        let clean = sanitize_html("<p>if (a[b[0]]>1)</p>", false);
        assert!(clean.contains("a[b[0]]>1"), "{clean}");
    }

    #[test]
    fn vector_and_math_content_goes_but_the_rest_stays() {
        let clean = sanitize_html("<p>a</p><svg><path d=\"M0 0\"/></svg><p>b</p>", false);
        assert_eq!(clean, "<p>a</p><p>b</p>");
        let clean = sanitize_html("<p>a</p><math><mi>x</mi></math><p>b</p>", false);
        assert_eq!(clean, "<p>a</p><p>b</p>");
        let clean = sanitize_html("<style>a{content:\"<svg\"}</style><p>b</p>", false);
        assert!(clean.contains("<p>b</p>"), "{clean}");
    }

    #[test]
    fn markup_that_parses_differently_in_a_browser_is_dropped() {
        for payload in [
            "<math><mtext><mglyph><style><img src=x onerror=alert(1)>",
            "<math><mi><malignmark><style><img src=x onerror=alert(1)>",
            "<template><svg></template><![CDATA[><img src=x onerror=alert(1)>]]>",
            "<div><svg></div><style><a id=\"</style><img src=http://t/x onerror=a()>\">",
            "<div><math></div><style><a id=\"</style><img src=http://t/x onerror=a()>\">",
            "<template><svg></template><![CDA<![CDATA[TA[><img src=x onerror=alert(1)>]]]]>>",
            "<sv<svg></svg>g></div><style><a id=\"</style><img src=x onerror=a()>\">",
        ] {
            let clean = sanitize_html(payload, false);
            assert!(!clean.contains("onerror"), "{payload}: {clean}");
        }
    }

    #[test]
    fn nothing_hides_in_noscript() {
        let clean = sanitize_html(
            "<noscript><form action=https://evil.test><button>Go</button></form></noscript>x",
            false,
        );
        assert_eq!(clean, "x");
    }

    #[test]
    fn attribute_values_are_judged_with_their_references_decoded() {
        for href in [
            "&#106;avascript:alert(1)",
            "&#106avascript:alert(1)",
            "&#x6A;avascript:alert(1)",
            "javascript&colon;alert(1)",
            "java&Tab;script:alert(1)",
        ] {
            let clean = sanitize_html(&format!("<a href=\"{href}\">x</a>"), false);
            assert!(!clean.contains("href"), "{href}: {clean}");
        }
        let clean = sanitize_html("<img src=\"&#104;ttps://tracker.test/p.png\">", false);
        assert!(!clean.contains(" src="), "{clean}");
        assert!(clean.contains("data-rm-blocked-src"), "{clean}");

        assert_eq!(
            decoded_attribute("https://a.test/?x=1"),
            "https://a.test/?x=1"
        );
        assert_eq!(decoded_attribute("&#bad"), "&#bad");
        let kept = sanitize_html("<a href=\"https://a.test/?a=1&amp;b=2\">x</a>", false);
        assert!(
            kept.contains("href=\"https://a.test/?a=1&amp;b=2\""),
            "{kept}"
        );
    }

    #[test]
    fn an_svg_image_only_comes_inline() {
        for href in [
            "/dev/zero",
            "https://tracker.test/p.png",
            "\\\\host\\share\\x.png",
        ] {
            let clean = sanitize_html(&format!("<image href=\"{href}\"/>"), true);
            assert!(!clean.contains("href"), "{href}: {clean}");
        }
        let inline = "data:image/png;base64,AAAA";
        let clean = sanitize_html(&format!("<image href=\"{inline}\"/>"), false);
        assert!(clean.contains(inline), "{clean}");
    }

    #[test]
    fn only_dark_pages_adapt_text() {
        let light = DocumentColors {
            background: gpui::white(),
            text: gpui::black(),
            accent: gpui::black(),
        };
        assert_eq!(light.dark_page(), None);
        let dark = DocumentColors {
            background: gpui::black(),
            text: gpui::white(),
            accent: gpui::white(),
        };
        assert_eq!(
            dark.dark_page(),
            Some(html_view::DarkPage {
                background: [0, 0, 0, 255],
                text: [255, 255, 255, 255],
            })
        );
    }

    #[test]
    fn quotable_text_is_escaped_with_line_breaks() {
        assert_eq!(quotable_text("a <b>\nc"), "a &lt;b&gt;<br>c");
    }

    #[test]
    fn quotable_html_keeps_the_body_without_active_content() {
        let html = "<!DOCTYPE html><html><head><title>T</title><style>p{}</style></head>\
            <body onload=\"x()\"><p style=\"color:red\">Hi <b>there</b></p>\
            <img src=\"cid:logo\"><img src=\"https://a.test/x.png\">\
            <script>bad()</script></body></html>";
        let quoted = quotable_html(html);
        assert!(
            quoted.contains("<p style=\"color:red\">Hi <b>there</b></p>"),
            "{quoted}"
        );
        assert!(quoted.contains("https://a.test/x.png"), "{quoted}");
        for gone in [
            "DOCTYPE", "<html", "<head", "<title", "<body", "onload", "cid:", "script",
        ] {
            assert!(!quoted.contains(gone), "{gone} in {quoted}");
        }
    }

    #[test]
    fn remote_images_load_only_when_allowed() {
        let colors = DocumentColors {
            background: hsla(0.0, 0.0, 1.0, 1.0),
            text: hsla(0.0, 0.0, 0.1, 1.0),
            accent: hsla(0.6, 0.8, 0.4, 1.0),
        };
        let body = MessageBody::Html("<img src=\"https://cdn.test/a.png\">".into());
        let blocked = email_document(colors, &body, false);
        assert_eq!(blocked.blocked_images, 1);
        assert!(!blocked.html.contains(" src=\"https://cdn.test"));
        let allowed = email_document(colors, &body, true);
        assert_eq!(allowed.blocked_images, 0);
        assert!(allowed.html.contains(" src=\"https://cdn.test/a.png\""));
        // The print page lets allowed images through its CSP.
        let header = PrintHeader {
            subject: "s",
            from_label: "From",
            from: "f",
            date_label: "Date",
            date: "d",
        };
        assert!(print_document(&header, &body, "n", true).contains("img-src data: http: https:"));
        assert!(print_document(&header, &body, "n", false).contains("img-src data:;"));
    }

    #[test]
    fn print_pages_are_light_locked_down_and_headed() {
        let header = PrintHeader {
            subject: "Hello <you>",
            from_label: "From",
            from: "Ann <ann@x.test>",
            date_label: "Date",
            date: "2002-07-09",
        };
        let body = MessageBody::Html(
            "<p>Body</p><img src=\"https://tracker.test/p.gif\"><script>alert(1)</script>".into(),
        );
        let html = print_document(&header, &body, "n0nce", false);
        // The CSP comes first in <head>; only the nonce'd script may run.
        let head = &html[html.find("<head>").unwrap()..];
        assert!(head.starts_with("<head><meta http-equiv=\"Content-Security-Policy\""));
        assert!(html.contains("default-src 'none'; img-src data:"));
        assert!(html.contains("script-src 'nonce-n0nce'"));
        assert!(html.contains("<script nonce=\"n0nce\">"));
        assert_eq!(html.matches("<script").count(), 1);
        // Header escaped, body sanitized (remote image blocked).
        assert!(html.contains("<h1>Hello &lt;you&gt;</h1>"));
        assert!(html.contains("Ann &lt;ann@x.test&gt;"));
        assert!(html.contains("<p>Body</p>"));
        assert!(!html.contains(" src=\"https://tracker.test"));
        // Light page whatever the app theme.
        assert!(html.contains("color-scheme: light") || !html.contains("color-scheme: dark"));
    }
    use gpui::hsla;

    fn body_html() -> MessageBody {
        MessageBody::Html("<p>Hello <strong>world</strong></p>".into())
    }

    fn doc_colors(background: Hsla, text: Hsla, accent: Hsla) -> DocumentColors {
        DocumentColors {
            background,
            text,
            accent,
        }
    }

    #[test]
    fn escapes_html_special_characters() {
        assert_eq!(
            escape_html("a < b & c > d \"e\" 'f'"),
            "a &lt; b &amp; c &gt; d &quot;e&quot; &#39;f&#39;"
        );
    }

    #[test]
    fn channels_clamp_and_round() {
        assert_eq!(channel(0.0), 0);
        assert_eq!(channel(1.0), 255);
        assert_eq!(channel(2.0), 255);
        assert_eq!(channel(-1.0), 0);
    }

    #[test]
    fn css_color_is_opaque_rgb() {
        assert_eq!(css_color(hsla(0.0, 0.0, 0.0, 1.0)), "rgb(0, 0, 0)");
        assert_eq!(css_color(hsla(0.0, 0.0, 1.0, 1.0)), "rgb(255, 255, 255)");
    }

    #[test]
    fn document_wraps_html_body_verbatim() {
        let doc = email_document(
            doc_colors(
                hsla(0.0, 0.0, 0.1, 1.0),
                hsla(0.0, 0.0, 0.9, 1.0),
                hsla(0.6, 0.7, 0.5, 1.0),
            ),
            &body_html(),
            false,
        )
        .html;
        assert!(doc.starts_with("<!DOCTYPE html>"));
        assert!(doc.contains("<p>Hello <strong>world</strong></p>"));
        // A dark background selects the dark color scheme.
        assert!(doc.contains("color-scheme: dark"));
    }

    #[test]
    fn document_escapes_and_wraps_plain_text() {
        let body = MessageBody::Text("1 < 2 & 3".into());
        let doc = email_document(
            doc_colors(
                hsla(0.0, 0.0, 0.95, 1.0),
                hsla(0.0, 0.0, 0.1, 1.0),
                hsla(0.6, 0.7, 0.5, 1.0),
            ),
            &body,
            false,
        )
        .html;
        assert!(doc.contains("<pre class=\"plain\">1 &lt; 2 &amp; 3</pre>"));
        // A light background selects the light color scheme.
        assert!(doc.contains("color-scheme: light"));
    }

    #[test]
    fn sanitize_strips_disallowed_elements_and_their_content() {
        let dirty = "<p>keep</p>\
             <script>alert(1)</script>\
             <iframe src=\"https://evil.test\"></iframe>\
             <video controls><source src=\"x.mp4\"></video>\
             <audio src=\"x.mp3\"></audio>\
             <object data=\"x.swf\"></object>\
             <embed src=\"x.pdf\">\
             <form><input name=\"a\"><textarea>t</textarea>\
             <select><option>o</option></select><button>b</button></form>\
             <base href=\"https://evil.test/\"><link rel=\"stylesheet\" href=\"x.css\">\
             <meta http-equiv=\"refresh\" content=\"0;url=https://evil.test\">\
             <canvas></canvas><dialog>d</dialog>\
             <p>tail</p>";
        let clean = sanitize_html(dirty, true);

        assert!(clean.contains("<p>keep</p>"));
        assert!(clean.contains("<p>tail</p>"));
        for needle in [
            "<script",
            "alert(1)",
            "<iframe",
            "<video",
            "<source",
            "<audio",
            "<object",
            "<embed",
            "<form",
            "<input",
            "<textarea",
            "<select",
            "<button",
            "<base",
            "<link",
            "<meta",
            "<canvas",
            "<dialog",
        ] {
            assert!(
                !clean.contains(needle),
                "expected `{needle}` to be stripped, got: {clean}"
            );
        }
    }

    #[test]
    fn sanitize_removes_svg_with_its_scriptable_parts() {
        let dirty = "<svg viewBox=\"0 0 10 10\">\
             <script>x()</script>\
             <foreignObject><p>y</p></foreignObject>\
             <animate attributeName=\"onclick\"/>\
             <set attributeName=\"onload\"/>\
             </svg><p>after</p>";
        assert_eq!(sanitize_html(dirty, false), "<p>after</p>");
    }

    #[test]
    fn sanitize_strips_event_handlers_and_contenteditable() {
        let clean = sanitize_html(
            "<div contenteditable=\"true\" onclick=\"steal()\" ONERROR=\"x\">text</div>",
            true,
        );
        let lower = clean.to_ascii_lowercase();
        assert!(clean.contains("<div"));
        assert!(clean.contains("text"));
        assert!(!lower.contains("onclick"));
        assert!(!lower.contains("onerror"));
        assert!(!clean.contains("contenteditable"));
    }

    #[test]
    fn sanitize_neutralizes_dangerous_url_schemes() {
        let clean = sanitize_html(
            "<a href=\"javascript:alert(1)\">a</a>\
             <a href=\"vbscript:msgbox(1)\">b</a>\
             <img src=\"data:image/svg+xml,<svg/>\">",
            true,
        );
        let lower = clean.to_ascii_lowercase();
        assert!(!lower.contains("javascript:"));
        assert!(!lower.contains("vbscript:"));
        assert!(!lower.contains("data:image/svg+xml"));
        // The anchors and image survive — only the unsafe attribute is dropped.
        assert!(clean.contains("<a"));
        assert!(clean.contains("<img"));
    }

    #[test]
    fn is_dangerous_url_classifies_schemes() {
        assert!(is_dangerous_url("javascript:alert(1)"));
        assert!(is_dangerous_url("  JaVaScRiPt:alert(1)"));
        assert!(is_dangerous_url("java\tscript:alert(1)"));
        assert!(is_dangerous_url("vbscript:x"));
        assert!(is_dangerous_url("data:text/html,<b>"));
        assert!(is_dangerous_url("data:image/svg+xml,<svg/>"));
        // Safe: relative/fragment/known-good schemes and inline raster images.
        assert!(!is_dangerous_url("https://example.test/path"));
        assert!(!is_dangerous_url("mailto:a@b.test"));
        assert!(!is_dangerous_url("#section"));
        assert!(!is_dangerous_url("/relative/path"));
        assert!(!is_dangerous_url("data:image/png;base64,AAAA"));
    }

    #[test]
    fn sanitize_drops_style_with_legacy_script_vectors() {
        let clean = sanitize_html(
            "<p style=\"width:expression(alert(1))\">a</p>\
             <p style=\"background:url(javascript:alert(1))\">b</p>",
            true,
        );
        let lower = clean.to_ascii_lowercase();
        assert!(!lower.contains("expression("));
        assert!(!lower.contains("javascript:"));
    }

    #[test]
    fn sanitize_preserves_safe_markup_and_inline_styles() {
        let html = "<p style=\"color:red\">Hi <a href=\"https://ex.test\">link</a> \
             <img src=\"data:image/png;base64,AAAA\"></p>";
        let clean = sanitize_html(html, true);
        assert!(clean.contains("style=\"color:red\""));
        assert!(clean.contains("<a href=\"https://ex.test\">link</a>"));
        assert!(clean.contains("<img src=\"data:image/png;base64,AAAA\">"));
    }

    #[test]
    fn is_remote_url_detects_network_resources() {
        assert!(is_remote_url("http://tracker.test/p.gif"));
        assert!(is_remote_url("https://tracker.test/p.gif"));
        assert!(is_remote_url("  HTTPS://Tracker.test"));
        assert!(is_remote_url("//tracker.test/p.gif")); // protocol-relative
                                                        // Local / inline resources are not remote.
        assert!(!is_remote_url("data:image/png;base64,AAAA"));
        assert!(!is_remote_url("cid:part1@mail"));
        assert!(!is_remote_url("/relative.png"));
        assert!(!is_remote_url("#frag"));
        assert!(!is_remote_url("images/logo.png"));
    }

    #[test]
    fn sanitize_blocks_remote_resources_when_disabled() {
        let html = "<img src=\"https://tracker.test/pixel.gif\" \
             srcset=\"https://tracker.test/2x.gif 2x\">\
             <img src=\"data:image/png;base64,AAAA\">";
        let clean = sanitize_html(html, false);
        // The remote URL is preserved out-of-band (so the user can reveal it),
        // and *only* there: it appears exactly once, inside data-rm-blocked-src,
        // never as a live src/srcset. The srcset variant is dropped entirely.
        assert!(clean.contains("data-rm-blocked-src=\"https://tracker.test/pixel.gif\""));
        assert!(!clean.contains("srcset"));
        assert!(!clean.contains("2x.gif"));
        assert_eq!(clean.matches("tracker.test").count(), 1);
        // Both <img> elements survive; the inline data: image is untouched.
        assert!(clean.contains("data:image/png;base64,AAAA"));
        assert_eq!(clean.matches("<img").count(), 2);
    }

    #[test]
    fn sanitize_keeps_remote_link_href_when_blocking() {
        // A link's href is navigation (acts on click), not a render-time fetch,
        // so it must survive even with remote content blocked.
        let html = "<a href=\"https://example.test/page\">click</a>\
             <img src=\"https://tracker.test/pixel.gif\">";
        let clean = sanitize_html(html, false);
        assert!(clean.contains("href=\"https://example.test/page\""));
        // The image, by contrast, is still blocked: its URL only survives
        // out-of-band in data-rm-blocked-src, never as a live src.
        assert!(clean.contains("data-rm-blocked-src=\"https://tracker.test/pixel.gif\""));
        assert_eq!(clean.matches("tracker.test").count(), 1);
    }

    #[test]
    fn sanitize_does_not_stash_blocked_src_when_remote_enabled() {
        let html = "<img src=\"https://cdn.test/pixel.gif\">";
        let clean = sanitize_html(html, true);
        assert!(clean.contains("src=\"https://cdn.test/pixel.gif\""));
        assert!(!clean.contains("data-rm-blocked-src"));
    }

    #[test]
    fn sanitize_keeps_remote_resources_when_enabled() {
        let html = "<img src=\"https://cdn.test/pixel.gif\">";
        let clean = sanitize_html(html, true);
        assert!(clean.contains("https://cdn.test/pixel.gif"));
    }

    #[test]
    fn strip_css_urls_empties_every_url() {
        assert_eq!(
            strip_css_urls("a{background:url(http://x/y.png) no-repeat}"),
            "a{background:url() no-repeat}"
        );
        // Case-insensitive, multiple occurrences, and quotes/whitespace inside.
        assert_eq!(
            strip_css_urls("URL( 'http://a' ) and url(\"data:image/png;base64,AAA\")"),
            "url() and url()"
        );
        // Unterminated url( drops the remainder.
        assert_eq!(strip_css_urls("x url(http://a"), "x url(");
        // No url(): untouched.
        assert_eq!(strip_css_urls("color: red"), "color: red");
    }

    #[test]
    fn sanitize_blocks_css_url_resources_when_disabled() {
        let html = "<style>.hero{background-image:url(https://tracker.test/bg.png)}</style>\
             <p style=\"background:url('https://tracker.test/inline.png')\">hi</p>";
        let clean = sanitize_html(html, false);
        assert!(!clean.contains("tracker.test"));
        assert_eq!(clean.matches("url()").count(), 2);
    }

    #[test]
    fn sanitize_leaves_url_text_outside_css_alone() {
        // Prose mentioning `url(` (no closing paren) used to lose the rest
        // of the message.
        let html = "<p>Call curl(1) or use url( to link</p><p>still here</p>\
             <style>a{background:url(https://t.test/x.png)}</style>";
        let clean = sanitize_html(html, false);
        assert!(clean.contains("Call curl(1) or use url( to link"));
        assert!(clean.contains("<p>still here</p>"));
        assert!(!clean.contains("t.test"));
    }

    #[test]
    fn sanitize_keeps_css_url_resources_when_enabled() {
        let html = "<p style=\"background:url('https://cdn.test/bg.png')\">hi</p>";
        let clean = sanitize_html(html, true);
        assert!(clean.contains("https://cdn.test/bg.png"));
    }

    #[test]
    fn document_strips_disallowed_elements_from_html_body() {
        let body = MessageBody::Html(
            "<p>safe</p><iframe src=\"x\"></iframe><input><video></video>".into(),
        );
        let doc = email_document(
            doc_colors(
                hsla(0.0, 0.0, 0.1, 1.0),
                hsla(0.0, 0.0, 0.9, 1.0),
                hsla(0.6, 0.7, 0.5, 1.0),
            ),
            &body,
            false,
        )
        .html;
        assert!(doc.contains("<p>safe</p>"));
        assert!(!doc.contains("<iframe"));
        assert!(!doc.contains("<input"));
        assert!(!doc.contains("<video"));
    }

    #[test]
    fn remote_image_state_follows_the_policy() {
        let url = "https://tracker.test/p.gif";
        let html = format!("<img src=\"{url}\">");
        let none = HashSet::new();
        // Loading off → present and blocked; on → present, nothing blocked.
        assert!(sanitize_html_inner(&html, false, &none).1);
        assert_eq!(sanitize_html_inner(&html, false, &none).2, 1);
        assert_eq!(sanitize_html_inner(&html, true, &none).2, 0);
        // Individually shown images are let through while loading is off.
        let shown: HashSet<String> = [url.to_string()].into_iter().collect();
        assert_eq!(sanitize_html_inner(&html, false, &shown).2, 0);
        // A message without remote resources reports none.
        let local = sanitize_html_inner("<p>x</p>", false, &none);
        assert!(!local.1 && local.2 == 0);
    }

    #[test]
    fn plain_text_keeps_readable_structure() {
        let html = "<html><head><title>T</title><style>p{color:red}</style></head>\
            <body><h1>Hello&nbsp;there</h1><p>One  two\n three &amp; four</p>\
            <script>alert(1)</script><!-- note --><ul><li>a</li><li>b</li></ul>\
            <table><tr><td>x</td><td>y</td></tr></table><pre>  keep\n  this</pre></body></html>";
        assert_eq!(
            plain_text(html),
            "Hello there\n\nOne two three & four\n\na\nb\n\nx\ty\n\n  keep\n  this"
        );
    }

    #[test]
    fn plain_text_decodes_entities_and_survives_broken_markup() {
        assert_eq!(
            decode_entities("&lt;a&gt; &#233;&#xE9; &bogus; & x"),
            "<a> éé &bogus; & x"
        );
        assert_eq!(plain_text("text <b unclosed"), "text");
        assert_eq!(plain_text("<script>never closed"), "");
        assert_eq!(plain_text("a <!-- open comment"), "a");
    }

    #[test]
    fn plain_text_never_cuts_multibyte_characters() {
        // `&` followed by multi-byte text with no `;` in the first 12 bytes.
        assert_eq!(decode_entities("R&aaaaaaaaaaé"), "R&aaaaaaaaaaé");
        assert_eq!(plain_text("P&D çççççççç ok"), "P&D çççççççç ok");
        // An unclosed tag ending in a multi-byte character.
        assert_eq!(plain_text("1 < 2 é"), "1");
        assert_eq!(plain_text("x <é"), "x");
    }

    #[test]
    fn plain_text_skips_blocks_case_insensitively() {
        assert_eq!(plain_text("a <STYLE>p{}</Style>b"), "a b");
        assert_eq!(plain_text("<title>T</TITLE><p>é</p>"), "é");
        assert_eq!(find_ascii_ci("xx</HeAd>", "</head"), Some(2));
        assert_eq!(find_ascii_ci("é</p", "</p"), Some(2));
        assert_eq!(find_ascii_ci("abc", "</p"), None);
    }

    #[test]
    fn documents_keep_the_message_doctype() {
        let legacy = "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0 Transitional//EN\">";
        assert_eq!(
            leading_doctype(&format!("\u{feff} <!-- x -->{legacy}<html>")),
            Some(legacy)
        );
        assert_eq!(leading_doctype("<p>no doctype</p>"), None);
        let colors = doc_colors(
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.0, 0.0, 0.9, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
        );
        let doc = email_document(
            colors,
            &MessageBody::Html(format!("{legacy}<p>x</p>").into()),
            false,
        );
        assert!(doc.html.starts_with(legacy));
        let doc = email_document(colors, &MessageBody::Text("x".into()), false);
        assert!(doc.html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(is_openable_link("https://example.com/a"));
        assert!(is_openable_link(" HTTP://example.com"));
        assert!(is_openable_link("mailto:someone@example.com"));
        assert!(!is_openable_link("bgmail://message/page.html"));
        assert!(!is_openable_link("javascript:alert(1)"));
        assert!(!is_openable_link("file:///etc/passwd"));
    }

    #[test]
    fn documents_always_block_remote_images() {
        let body = MessageBody::Html("<img src=\"https://tracker.test/p.gif\">".into());
        let colors = doc_colors(
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.0, 0.0, 0.9, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
        );
        let doc = email_document(colors, &body, false);
        assert_eq!(doc.blocked_images, 1);
        assert!(!doc.html.contains(" src=\"https://"));
        assert!(colors.is_dark());
    }

    /// What comes out of the sanitizer is read by another parser: what it leaves or removes
    /// can't join with what is around it into something it was there to remove.
    #[test]
    fn nothing_removed_by_the_sanitizer_can_join_into_active_content() {
        let many: String = (0..1100).map(|i| format!(" a{i}")).collect();
        let prefixes = [
            "",
            "<",
            "<?",
            "</ ",
            "<!-- --!>",
            "<title>",
            "<a \">",
            "<!x>",
        ];
        let removed = [
            "<script></script>".to_string(),
            "<svg></svg>".to_string(),
            "<iframe></iframe>".to_string(),
            "<body>".to_string(),
            "<html>".to_string(),
            "<head></head>".to_string(),
            "<img src=cid:x>".to_string(),
            "<!doctype b>".to_string(),
            format!("<p{many}>"),
        ];
        for prefix in prefixes {
            for gap in &removed {
                for name in ["script", "iframe", "input", "form", "meta", "object"] {
                    for html in [
                        format!("{prefix}<{gap}{name} onclick=x>b<{gap}/{name}>"),
                        format!("{prefix}<{gap}{gap}{name} onclick=x>"),
                        format!("{prefix}<xmp{many}><{name} onclick=x></xmp>"),
                        format!("{prefix}<title{many}><{name} onclick=x></title>"),
                        format!("{prefix}<noembed{many}><{name} onclick=x></noembed>"),
                        format!("<?a> <{gap}{name} onclick=x> ?><!doctype b>"),
                        format!("<a b onclick=x = c=\"><{name} onclick=x>\">"),
                        format!("<a b/=c=\"><{name} onclick=x>\" style=x>"),
                        format!("<d =/=x=\"><{name} onclick=x>\" onclick=y>"),
                        format!("<!-- -<!doctype b>-> <{name} onclick=x> --><!doctype b>"),
                        "<?a><wbr<!doctype b> onclick=x>?><!doctype b>".to_string(),
                        format!("<p>x</p><{name} onclick=x "),
                    ] {
                        let sanitized = sanitize_html_inner(&html, false, &HashSet::new()).0;
                        assert!(reads_as_safe(&sanitized), "{html}\n{sanitized}");
                        let quoted = quotable_html(&html);
                        assert!(reads_as_safe(&quoted), "{html}\n{quoted}");
                        let again = sanitize_html_inner(&sanitized, false, &HashSet::new()).0;
                        assert_eq!(again, sanitized, "{html}");
                    }
                }
            }
        }
    }

    #[test]
    fn what_reads_as_active_content_is_refused_whatever_let_it_through() {
        for html in [
            "<script>1</script>",
            "<p onclick=x>",
            "<a href=javascript:x>",
            "<meta http-equiv=refresh content=0 ",
            "<input ",
            "<a b = c=\"><form>\">",
            "<xmp></xmp><iframe>",
        ] {
            assert!(!reads_as_safe(html), "{html}");
        }
        for html in [
            "<p>x</p>",
            "<style>a{}<script></style>",
            "<title><script></title>",
            "<xmp><script></xmp>",
            "<!-- <script> -->",
            "<a title=\"<script>\" href=http://x.test>",
            "a < b",
        ] {
            assert!(reads_as_safe(html), "{html}");
        }
    }

    #[test]
    fn the_samples_in_the_repository_all_read_as_safe() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../spikes/html_render/corpus"
        );
        let Ok(files) = std::fs::read_dir(dir) else {
            return;
        };
        for file in files.flatten() {
            let html = String::from_utf8_lossy(&std::fs::read(file.path()).unwrap()).into_owned();
            let sanitized = sanitize_html_inner(&html, true, &HashSet::new()).0;
            assert!(reads_as_safe(&sanitized), "{:?}", file.path());
            assert!(reads_as_safe(&quotable_html(&html)), "{:?}", file.path());
        }
    }

    #[test]
    fn a_quote_keeps_a_body_after_an_unclosed_head() {
        let quoted = quotable_html("<html><head><title>x</title><body>hello</body></html>");
        assert!(
            quoted.contains("hello") && !quoted.contains('x'),
            "{quoted}"
        );
    }

    #[test]
    fn a_stray_equals_sign_loses_nothing_that_was_safe() {
        for (html, kept) in [
            (
                "<html lang=en =\"\"><body><p>Everything</p></body></html>",
                "Everything",
            ),
            (
                "<table width=600 =\"\"><tr><td>Whole newsletter</td></tr></table>",
                "Whole newsletter",
            ),
            ("<p>Read <a href=x =y>this link</a> now</p>", "this link"),
            (
                "<table background=http://x.test/a =\"\"><tr><td>Kept</td></tr></table>",
                "Kept",
            ),
        ] {
            let sanitized = sanitize_html_inner(html, false, &HashSet::new()).0;
            assert!(sanitized.contains(kept), "{html}\n{sanitized}");
            assert!(reads_as_safe(&sanitized), "{html}\n{sanitized}");
        }
    }

    #[test]
    fn what_a_form_or_button_holds_stays() {
        let html = "<form id=form1><table><tr><td>Whole message</td></tr></table>\
            <input value=gone></form><a href=http://x.test><button>Buy now</button></a>";
        let sanitized = sanitize_html_inner(html, true, &HashSet::new()).0;
        for kept in ["Whole message", "Buy now", "href"] {
            assert!(sanitized.contains(kept), "{sanitized}");
        }
        for gone in ["<form", "<button", "<input", "gone"] {
            assert!(!sanitized.contains(gone), "{sanitized}");
        }
        assert!(reads_as_safe(&sanitized));
    }

    #[test]
    fn a_media_type_is_read_as_a_browser_reads_it() {
        assert!(is_dangerous_url("data:image/svg+\nxml;base64,AA"));
        assert!(is_dangerous_url("data:image/svg+\txml;base64,AA"));
        assert!(!is_dangerous_url("data:image/png;base64,AA"));
    }

    #[test]
    fn a_hidden_form_or_button_stays_hidden() {
        for (html, expected) in [
            (
                "<form style=\"display:none\">Hidden</form>",
                "<section style=\"display:none\">Hidden</section>",
            ),
            (
                "<form hidden>Hidden</form>",
                "<section hidden=\"\">Hidden</section>",
            ),
            (
                "<button class=x>Hidden</button>",
                "<span class=\"x\">Hidden</span>",
            ),
        ] {
            assert_eq!(sanitize_html_inner(html, true, &HashSet::new()).0, expected);
        }
    }

    #[test]
    fn a_remote_image_with_a_stray_equals_sign_is_still_counted() {
        let (_, has_remote, blocked) = sanitize_html_inner(
            "<img src=http://x.test/a.png onclick=x width=1 =\"\">",
            false,
            &HashSet::new(),
        );
        assert!(has_remote);
        assert_eq!(blocked, 1);
    }

    #[test]
    fn a_remote_background_attribute_is_counted_and_offered() {
        let html = "<table background=http://x.test/a.png><tr><td>x</td></tr></table>";
        let (clean, has_remote, blocked) = sanitize_html_inner(html, false, &HashSet::new());
        assert!(has_remote && !clean.contains("background"));
        assert_eq!(blocked, 1);
        let (clean, has_remote, blocked) = sanitize_html_inner(html, true, &HashSet::new());
        assert!(has_remote && clean.contains("background"));
        assert_eq!(blocked, 0);
    }

    #[test]
    fn a_form_left_open_doesnt_swallow_what_follows() {
        let html = "<div style=\"display:none\">x<form>y</div><p>Seen</p>";
        let sanitized = sanitize_html_inner(html, true, &HashSet::new()).0;
        assert_eq!(
            sanitized,
            "<div style=\"display:none\">x<section>y</div><p>Seen</p>"
        );
    }
}
