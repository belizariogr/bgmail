//! Embedded webview used to render e-mail HTML.
//!
//! On macOS and Windows the message body is rendered by a native OS webview
//! (`wry`: WKWebView / WebView2) layered over the reader pane. This replaces the
//! hand-rolled HTML element renderer: the OS engine handles layout, scrolling,
//! text selection and copy natively, which is both simpler and far more capable.
//!
//! Linux is intentionally left out for now (webkit2gtk integration is deferred —
//! see `AGENTS.md`). There [`EmailWebView::new`] returns `None` and the reader
//! falls back to a plain-text view, so the app still builds and runs everywhere.

use std::collections::HashSet;
use std::path::Path;

use gpui::{Hsla, Rgba};
use lol_html::html_content::Element;

use crate::data::MessageBody;

/// Whether the native embedded webview backend is available on this target.
pub const WEBVIEW_SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Localized labels for the custom context menus (images and links). They are
/// embedded in the rendered document (as `data-*` attributes) and read by the
/// injected menu script, so switching the UI language updates the menus on the
/// next reload.
#[derive(Debug, Clone, Copy)]
pub struct ContextMenuLabels<'a> {
    /// Image menu: "Open image in browser".
    pub image_open: &'a str,
    /// Image menu: "Download image".
    pub image_download: &'a str,
    /// Blocked-image menu: "Show remote image" (loads the one remote image).
    pub image_show: &'a str,
    /// Link menu: "Open in browser".
    pub link_open: &'a str,
    /// Link menu: "Copy link".
    pub link_copy: &'a str,
    /// Selection menu: "Copy".
    pub selection_copy: &'a str,
    /// Keyboard hint shown next to "Copy" (`⌘C` on macOS, `Ctrl+C` elsewhere).
    pub copy_shortcut: &'a str,
}

/// Builds a self-contained HTML document for `body`, themed to match the current
/// app colors. Plain-text bodies are HTML-escaped and wrapped so they keep their
/// line breaks and wrap to the pane width. `labels` localize the custom image
/// context menu. When `load_remote` is false, remote resources (e.g. tracking
/// pixels) are stripped from HTML bodies; inline `data:` images always render.
/// A rendered e-mail document plus the privacy metadata the reader needs to
/// badge the message (blocked vs. loaded remote content).
pub struct RenderedEmail {
    /// The full HTML document fed to the webview.
    pub html: String,
    /// Whether the message contains any remote `<img>` (blocked or shown).
    /// Drives whether the reader's privacy shield appears at all.
    pub has_remote: bool,
    /// How many remote `<img>` are still blocked this render (i.e. neither the
    /// global setting is on nor were they individually shown). The shield is red
    /// while this is non-zero and green once it reaches zero.
    pub blocked_images: usize,
}

pub fn email_document(
    background: Hsla,
    text: Hsla,
    accent: Hsla,
    body: &MessageBody,
    labels: ContextMenuLabels,
    load_remote: bool,
    shown: &HashSet<String>,
) -> RenderedEmail {
    let (inner, has_remote, blocked_images) = match body {
        MessageBody::Html(html) => sanitize_html_inner(html, load_remote, shown),
        MessageBody::Text(plain) => (
            format!("<pre class=\"plain\">{}</pre>", escape_html(plain)),
            false,
            0,
        ),
    };

    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <style>{css}</style></head>\
         <body data-rm-img-open=\"{img_open}\" data-rm-img-download=\"{img_download}\" \
         data-rm-img-show=\"{img_show}\" \
         data-rm-link-open=\"{link_open}\" data-rm-link-copy=\"{link_copy}\" \
         data-rm-sel-copy=\"{sel_copy}\" data-rm-copy-key=\"{copy_key}\">\
         {inner}</body></html>",
        css = document_css(background, text, accent),
        img_open = escape_html(labels.image_open),
        img_download = escape_html(labels.image_download),
        img_show = escape_html(labels.image_show),
        link_open = escape_html(labels.link_open),
        link_copy = escape_html(labels.link_copy),
        sel_copy = escape_html(labels.selection_copy),
        copy_key = escape_html(labels.copy_shortcut),
    );
    RenderedEmail {
        html,
        has_remote,
        blocked_images,
    }
}

/// Theme-aware stylesheet shared by every rendered message. Colors come straight
/// from the active theme so the webview matches the surrounding UI (incl. dark
/// mode), instead of the engine's default white page.
fn document_css(background: Hsla, text: Hsla, accent: Hsla) -> String {
    let scheme = if background.l < 0.5 { "dark" } else { "light" };
    format!(
        ":root {{ color-scheme: {scheme}; --rm-bg: {bg}; --rm-fg: {fg}; --rm-accent: {accent}; }}\
         html, body {{ margin: 0; padding: 16px 24px; background: {bg}; color: {fg}; \
           font: 14px/1.55 -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; \
           -webkit-font-smoothing: antialiased; overflow-wrap: anywhere; }}\
         a {{ color: {accent}; }}\
         img {{ display: block; }}\
         img[data-rm-blocked-src] {{ background: {soft}; border: 1px dashed {line}; \
           box-sizing: border-box; min-width: 24px; min-height: 24px; }}\
         h1, h2, h3 {{ line-height: 1.25; }}\
         code, pre {{ font-family: 'SF Mono', ui-monospace, Menlo, Consolas, monospace; font-size: 13px; }}\
         pre {{ white-space: pre-wrap; background: {soft}; padding: 12px; border-radius: 6px; }}\
         pre.plain {{ background: transparent; padding: 0; }}\
         code {{ background: {soft}; padding: 1px 4px; border-radius: 4px; }}\
         blockquote {{ margin: 0; padding-left: 12px; border-left: 3px solid {accent}; opacity: 0.85; }}\
         hr {{ border: none; border-top: 1px solid {fg}; opacity: 0.15; margin: 16px 0; }}\
         /* Styling the WebKit scrollbar opts out of macOS overlay scrollbars, \
            which auto-hide and visibly flash on every trackpad gesture (incl. \
            the one that opens the context menu). A themed, always-present bar \
            matches the steady look users get with a plugged-in mouse. */\
         ::-webkit-scrollbar {{ width: 12px; height: 12px; }}\
         ::-webkit-scrollbar-track {{ background: transparent; }}\
         ::-webkit-scrollbar-corner {{ background: transparent; }}\
         ::-webkit-scrollbar-thumb {{ background: {thumb}; border-radius: 8px; \
           border: 3px solid transparent; background-clip: padding-box; }}\
         ::-webkit-scrollbar-thumb:hover {{ background: {thumb_hover}; \
           border: 3px solid transparent; background-clip: padding-box; }}",
        bg = css_color(background),
        fg = css_color(text),
        accent = css_color(accent),
        // A subtle fill for code blocks/inline code, derived from the text color.
        soft = css_color_alpha(text, 0.08),
        // A slightly stronger line for the blocked-image placeholder border.
        line = css_color_alpha(text, 0.3),
        // Scrollbar thumb, derived from the text color so it reads on either theme.
        thumb = css_color_alpha(text, 0.25),
        thumb_hover = css_color_alpha(text, 0.4),
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

/// Whether a navigation target should be handed off to the system browser
/// instead of being followed inside the reader's webview. We treat real web and
/// mail destinations as external; in-document navigations (the `about:`/`data:`
/// document we load the body into, anchor fragments, etc.) stay in-place.
fn is_external_link(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")
}

/// Parses a base64 `data:` URI (the form inline e-mail images use), returning a
/// file extension derived from the MIME type and the decoded bytes. Used by the
/// "Open Image in New Window" context-menu action: a `data:` image has no remote
/// URL to hand to the browser, so we materialize it and let the OS open it.
///
/// Returns `None` for anything that isn't a non-empty, base64-encoded `data:`
/// URI (e.g. plain URLs or the percent-encoded text form).
fn decode_data_uri(url: &str) -> Option<(&'static str, Vec<u8>)> {
    let trimmed = url.trim();
    let rest = trimmed
        .get(..5)
        .filter(|prefix| prefix.eq_ignore_ascii_case("data:"))
        .map(|_| &trimmed[5..])?;
    let (meta, payload) = rest.split_once(',')?;
    let meta = meta.to_ascii_lowercase();
    // We only decode the base64 form; the percent-encoded text variant is not
    // something we ever produce for images.
    if !meta.split(';').any(|seg| seg == "base64") {
        return None;
    }
    let mime = meta.split(';').next().unwrap_or("");
    let bytes = base64_decode(payload)?;
    if bytes.is_empty() {
        return None;
    }
    Some((extension_for_mime(mime), bytes))
}

/// Maps a MIME type to a sensible file extension so the materialized temp file
/// opens in the right app. Unknown types fall back to `bin`.
fn extension_for_mime(mime: &str) -> &'static str {
    match mime.trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

/// Returns the user's Downloads directory (`$HOME/Downloads`, or
/// `%USERPROFILE%\Downloads` on Windows). Used as the fixed save location for
/// the image-download action, mirroring how `config.rs` resolves the home dir
/// without pulling in an extra dependency.
fn downloads_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    if home.is_empty() {
        return None;
    }
    Some(Path::new(&home).join("Downloads"))
}

/// Picks a non-clobbering path inside `dir` for `stem.ext`, appending ` (n)`
/// before the extension when earlier files exist (like browsers do). `exists`
/// reports whether a candidate is already taken, kept injectable so the naming
/// logic is testable without touching the filesystem.
fn unique_download_path(
    dir: &Path,
    stem: &str,
    extension: &str,
    exists: impl Fn(&Path) -> bool,
) -> std::path::PathBuf {
    let mut candidate = dir.join(format!("{stem}.{extension}"));
    let mut counter = 1;
    while exists(&candidate) {
        candidate = dir.join(format!("{stem} ({counter}).{extension}"));
        counter += 1;
    }
    candidate
}

/// Minimal RFC 4648 base64 decoder (standard alphabet). Whitespace and padding
/// are skipped. Returns `None` on any out-of-alphabet byte. Kept dependency-free
/// to mirror the encoder in `data.rs`.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn sextet(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some((byte - b'A') as u32),
            b'a'..=b'z' => Some((byte - b'a' + 26) as u32),
            b'0'..=b'9' => Some((byte - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for &byte in input.as_bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        buffer = (buffer << 6) | sextet(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// One IPC message kind sent from the document to the host. Messages are encoded
/// as `"<tag>\n<payload>"` so a single channel can carry several intents (see
/// [`CONTENT_SCRIPT`] and [`parse_ipc_message`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IpcMessage<'a> {
    /// The link under the cursor changed (payload is the URL, empty when none).
    /// Mirrored into the status bar like a browser.
    Hover(&'a str),
    /// "Open in browser" (image or link) — open the payload URL externally.
    OpenExternal(&'a str),
    /// "Download image" — save the image (payload URL) to Downloads, no dialog.
    DownloadImage(&'a str),
    /// "Copy link" — copy the payload URL to the system clipboard.
    CopyToClipboard(&'a str),
    /// "Show remote image" — the user revealed a blocked image (payload is its
    /// URL); the host records it so it stays shown and updates the blocked count.
    ShowImage(&'a str),
    /// A mouse press landed inside the webview (no payload). Lets the host close
    /// any open GPUI overlay, since those clicks never reach GPUI's catcher.
    BodyMouseDown,
}

/// Parses an IPC message produced by [`CONTENT_SCRIPT`]. Returns `None` for an
/// unknown tag or a message missing its `\n` separator.
fn parse_ipc_message(message: &str) -> Option<IpcMessage<'_>> {
    let (tag, payload) = message.split_once('\n')?;
    match tag {
        "H" => Some(IpcMessage::Hover(payload)),
        "O" => Some(IpcMessage::OpenExternal(payload)),
        "D" => Some(IpcMessage::DownloadImage(payload)),
        "C" => Some(IpcMessage::CopyToClipboard(payload)),
        "S" => Some(IpcMessage::ShowImage(payload)),
        "B" => Some(IpcMessage::BodyMouseDown),
        _ => None,
    }
}

/// An action the webview asks the GPUI host to perform on the foreground. Sent
/// over a channel because these touch app state (status bar) or need GPUI APIs
/// (clipboard) that the IPC callback can't reach directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// Update the hovered-link URL shown in the status bar (empty clears it).
    HoverLink(String),
    /// Copy text (a link URL) to the system clipboard.
    CopyToClipboard(String),
    /// The user revealed a blocked remote image (payload is its URL).
    ImageShown(String),
    /// A mouse press landed inside the webview; the host dismisses any open GPUI
    /// overlay (those clicks never reach GPUI's outside-click catcher).
    BodyMouseDown,
}

/// Injected into every rendered message. Two responsibilities:
///
/// 1. Report the URL under the cursor to the host (status-bar mirroring): link
///    `href`s, plus the original URL of images blocked for privacy (stashed in
///    `data-rm-blocked-src`).
/// 2. Replace the native context menu *for images and links* with our own. A
///    privacy-blocked image gets a "Show remote image" item that loads just that
///    one image in place (sets its `src` from `data-rm-blocked-src`).
///    WebKit's image menu is inert ("Download Image" never reaches the download
///    delegate and exposes no URL), and for links we want a consistent, themed
///    menu. Both route their actions over IPC. The native menu is suppressed
///    everywhere — it only offers "Reload", which makes no sense in an e-mail
///    body — and replaced by our own: a "Copy" menu when text is selected, and
///    nothing on empty background. Labels are read from `<body data-rm-*>` and
///    colors from the document's CSS variables, so all menus follow the active
///    language and theme without rebuilding the view.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const CONTENT_SCRIPT: &str = r#"(function () {
  function send(tag, value) { window.ipc.postMessage(tag + '\n' + (value || '')); }

  function closestTag(el, tag) {
    while (el && el.nodeType === 1) {
      if (el.tagName === tag) return el;
      el = el.parentElement;
    }
    return null;
  }

  // 1. Hovered-URL reporting: links, and remote images blocked for privacy
  //    (whose original URL we stashed in data-rm-blocked-src).
  var currentHref = null;
  function reportHover(href) {
    if (href !== currentHref) { currentHref = href; send('H', href); }
  }
  document.addEventListener('mouseover', function (e) {
    var a = closestTag(e.target, 'A');
    if (a && a.href) { reportHover(a.href); return; }
    var img = closestTag(e.target, 'IMG');
    reportHover(img && img.dataset.rmBlockedSrc ? img.dataset.rmBlockedSrc : '');
  }, true);
  document.addEventListener('mouseleave', function () { reportHover(''); }, true);

  // 2. Custom context menus (image + link).
  var STYLE_ID = 'rm-ctx-style';
  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    var s = document.createElement('style');
    s.id = STYLE_ID;
    s.textContent =
      '.rm-ctx{position:fixed;z-index:2147483647;min-width:200px;padding:4px;border-radius:8px;' +
      'border:1px solid color-mix(in srgb, var(--rm-fg) 16%, var(--rm-bg));' +
      'background:color-mix(in srgb, var(--rm-fg) 6%, var(--rm-bg));' +
      'box-shadow:0 8px 24px rgba(0,0,0,.28);overflow:hidden;' +
      'font:13px -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:var(--rm-fg);' +
      'user-select:none;-webkit-user-select:none;}' +
      '.rm-ctx button{display:flex;align-items:center;justify-content:space-between;gap:24px;' +
      'width:100%;text-align:left;border:0;background:transparent;color:inherit;padding:7px 12px;' +
      'border-radius:5px;cursor:default;font:inherit;white-space:nowrap;}' +
      '.rm-ctx button:hover{background:var(--rm-accent);color:#fff;}' +
      '.rm-ctx .rm-hint{margin-left:auto;opacity:.5;font-size:12px;}';
    document.head.appendChild(s);
  }

  var menu = null;
  function closeMenu() { if (menu) { menu.remove(); menu = null; } }
  function openMenu(x, y, items) {
    ensureStyle();
    closeMenu();
    menu = document.createElement('div');
    menu.className = 'rm-ctx';
    items.forEach(function (it) {
      var b = document.createElement('button');
      var label = document.createElement('span');
      label.textContent = it.label;
      b.appendChild(label);
      if (it.hint) {
        var hint = document.createElement('span');
        hint.className = 'rm-hint';
        hint.textContent = it.hint;
        b.appendChild(hint);
      }
      b.addEventListener('click', function () { it.run(); closeMenu(); });
      menu.appendChild(b);
    });
    document.body.appendChild(menu);
    var r = menu.getBoundingClientRect();
    if (x + r.width > window.innerWidth) x = window.innerWidth - r.width - 4;
    if (y + r.height > window.innerHeight) y = window.innerHeight - r.height - 4;
    menu.style.left = Math.max(4, x) + 'px';
    menu.style.top = Math.max(4, y) + 'px';
  }

  document.addEventListener('contextmenu', function (e) {
    var data = document.body.dataset;
    var img = closestTag(e.target, 'IMG');
    if (img && img.src) {
      e.preventDefault();
      openMenu(e.clientX, e.clientY, [
        { label: data.rmImgOpen || 'Open image in browser', run: function () { send('O', img.src); } },
        { label: data.rmImgDownload || 'Download image', run: function () { send('D', img.src); } }
      ]);
      return;
    }
    if (img && img.dataset.rmBlockedSrc) {
      // A remote image blocked for privacy: offer to load this one in place.
      e.preventDefault();
      openMenu(e.clientX, e.clientY, [
        { label: data.rmImgShow || 'Show remote image', run: function () {
            var url = img.dataset.rmBlockedSrc;
            img.src = url;
            img.removeAttribute('data-rm-blocked-src');
            reportHover('');
            // Tell the host so it keeps this image shown and updates the count.
            send('S', url);
        } }
      ]);
      return;
    }
    var a = closestTag(e.target, 'A');
    if (a && a.href) {
      e.preventDefault();
      openMenu(e.clientX, e.clientY, [
        { label: data.rmLinkOpen || 'Open in browser', run: function () { send('O', a.href); } },
        { label: data.rmLinkCopy || 'Copy link', run: function () { send('C', a.href); } }
      ]);
      return;
    }
    // Background: never the native menu (it only offers "Reload"). If text is
    // selected, show our own menu with Copy; otherwise show nothing.
    e.preventDefault();
    var selection = window.getSelection();
    var selectedText = selection ? selection.toString() : '';
    if (selectedText.length > 0) {
      openMenu(e.clientX, e.clientY, [
        { label: data.rmSelCopy || 'Copy', hint: data.rmCopyKey || '', run: function () { send('C', selectedText); } }
      ]);
    } else {
      closeMenu();
    }
  }, true);
  document.addEventListener('mousedown', function (e) {
    if (menu && !menu.contains(e.target)) closeMenu();
    // Report the click so the host can dismiss any GPUI overlay (e.g. the
    // privacy dropdown): clicks on the webview don't reach GPUI's catcher.
    send('B', '');
  }, true);
  document.addEventListener('scroll', closeMenu, true);
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape') closeMenu(); });
  window.addEventListener('blur', closeMenu);
  // Exposed so the host can dismiss the menu when the user clicks the GPUI UI
  // outside the webview: clicks on sibling native views don't blur the webview
  // nor deliver a 'mousedown' here, so the menu would otherwise linger.
  window.__rmCloseMenu = closeMenu;
})();"#;

/// Quotes a string as an AppleScript string literal (escaping `\` and `"`), so
/// notification text passed to `osascript -e` can't break out of the string.
#[cfg(target_os = "macos")]
fn applescript_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// CSS selector matching every element we refuse to render in an e-mail body.
///
/// These are stripped (element *and* their content) before the HTML ever reaches
/// the OS web engine, so the elements never exist in the DOM:
/// - **Active/embedding content** (`script`, `object`, `embed`, `applet`):
///   e-mail must never execute code or load plugins.
/// - **Frames** (`iframe`, `frame`, `frameset`): our context-menu script is only
///   injected in the main frame, so a sub-frame would resurface the native menu
///   (with "Reload") and bypass our handling. Removing them closes that gap.
/// - **Media** (`video`, `audio`, `source`, `track`): the native media menu
///   exposes inert "Download/Open" items (same problem as images) and players can
///   autoplay; a reader has no use for embedded players.
/// - **Editable/interactive controls** (`input`, `textarea`, `select`, `button`,
///   `form`): a reader is not a form host, and our selection "Copy" can't read a
///   field's selection anyway. Stripping them avoids dead/confusing affordances.
/// - **Document/redirect/external-resource heads** (`base`, `meta`, `link`):
///   `base` rewrites every relative link, `meta[http-equiv=refresh]` redirects the
///   view, and `link` pulls remote stylesheets/prefetch (tracking). `head`/`title`
///   are deliberately *kept* so e-mails that ship a full document don't lose their
///   `<style>`.
/// - **Misc scripting surfaces** (`canvas`, `dialog`, `portal`).
///
/// `svg` is intentionally **kept** (some e-mails use inline vector art), but its
/// scriptable parts are removed: `script` (covered above), `foreignobject` (hosts
/// arbitrary HTML) and the SMIL animation family (`animate`, `animatetransform`,
/// `animatemotion`, `set`) which can assign event-handler attributes at runtime.
const DISALLOWED_ELEMENTS: &str = "script, object, embed, applet, \
     iframe, frame, frameset, \
     video, audio, source, track, \
     input, textarea, select, button, form, \
     base, meta, link, canvas, dialog, portal, \
     foreignobject, animate, animatetransform, animatemotion, set";

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

/// Sanitizes an e-mail HTML fragment so it can never execute code or load
/// untrusted active content in the OS web engine.
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
/// in `data-rm-blocked-src` for the "Show remote image" affordance.
fn sanitize_html_inner(
    html: &str,
    load_remote: bool,
    shown: &HashSet<String>,
) -> (String, bool, usize) {
    use lol_html::{element, rewrite_str, RewriteStrSettings};
    use std::cell::Cell;

    let has_remote = Cell::new(false);
    let blocked = Cell::new(0usize);
    let settings = RewriteStrSettings::new()
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
        }));

    let sanitized = rewrite_str(html, settings).unwrap_or_default();
    let html = if load_remote {
        sanitized
    } else {
        // CSS `url(...)` (background images, web fonts, `@import`) also fetches
        // remote resources, in both inline `style` and `<style>` blocks. Blank
        // them too. By preference this is a blunt textual pass, not a CSS parser.
        strip_css_urls(&sanitized)
    };
    (html, has_remote.get(), blocked.get())
}

/// Empties the contents of every CSS `url(...)` in `css` — `url(http://x.png)`
/// becomes `url()` — so stylesheets can't fetch remote resources. This is a
/// deliberately blunt textual pass (equivalent to the regex `url\([^)]*\)` →
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
/// `data-rm-blocked-src` (the `src` is still dropped, so nothing loads) so the
/// content script can show the URL on hover and offer "Show remote image".
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
    let is_img = el.tag_name() == "img";
    // `attributes()` borrows the element, so collect everything we need first and
    // only mutate (`remove_attribute`/`set_attribute`) once the borrow is released.
    let mut blocked_img_src = None;
    let mut img_state: Option<bool> = None;
    let doomed: Vec<String> = el
        .attributes()
        .iter()
        .filter_map(|attr| {
            let name = attr.name(); // lower-cased by the parser
            let value = attr.value();
            let is_url_attr = URL_ATTRIBUTES.contains(&name.as_str());
            // A remote `<img src>` is what the reader surfaces and lets the user
            // unblock one at a time, so track it (and honor the `shown` allowlist).
            let img_src_remote = is_img && name == "src" && is_url_attr && is_remote_url(&value);
            let shown_here = img_src_remote && shown.contains(value.as_str());
            if img_src_remote {
                let blocked = !load_remote && !shown_here;
                img_state = Some(blocked);
                if blocked {
                    blocked_img_src = Some(value.clone());
                }
            }
            // Only resource-loading attributes are blocked for remoteness;
            // navigation/metadata URLs (e.g. a link's `href`) are left intact.
            let is_remote = (RESOURCE_ATTRIBUTES.contains(&name.as_str()) && is_remote_url(&value))
                || (name == "srcset" && !value.trim().is_empty());
            let remote_blocked = is_remote && !load_remote && !shown_here;
            let drop = name.starts_with("on")
                || name == "contenteditable"
                || (is_url_attr && is_dangerous_url(&value))
                || (name == "style" && style_has_script_vector(&value))
                || remote_blocked;
            drop.then_some(name)
        })
        .collect();

    for name in doomed {
        el.remove_attribute(&name);
    }
    if let Some(src) = blocked_img_src {
        let _ = el.set_attribute("data-rm-blocked-src", &src);
    }
    img_state
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
            let media = trimmed[colon + 1..]
                .split([';', ','])
                .next()
                .unwrap_or("")
                .trim()
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

pub use platform::EmailWebView;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod platform {
    use async_channel::Sender;
    use gpui::{Bounds, Pixels, Window};
    use wry::{
        dpi::{LogicalPosition, LogicalSize},
        NewWindowResponse, Rect, WebView, WebViewBuilder,
    };

    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        decode_data_uri, downloads_dir, is_external_link, parse_ipc_message, unique_download_path,
        HostEvent, IpcMessage, CONTENT_SCRIPT,
    };

    /// Opens `url` in the user's default browser, detached so it never blocks the
    /// UI thread. Errors are ignored: a failed launch shouldn't crash the reader.
    fn open_external(url: &str) {
        let _ = open::that_detached(url);
    }

    /// Routes a message sent from the document's content script. Actions that
    /// touch app state (hover) or need GPUI (clipboard) are forwarded to the
    /// foreground over `to_host`; open/download run here on the main thread.
    /// `notify_body` is the localized text shown after a successful download.
    fn handle_ipc(message: &str, to_host: &Sender<HostEvent>, notify_body: &str) {
        match parse_ipc_message(message) {
            Some(IpcMessage::Hover(url)) => {
                let _ = to_host.try_send(HostEvent::HoverLink(url.to_string()));
            }
            Some(IpcMessage::OpenExternal(url)) => open_in_new_window(url),
            Some(IpcMessage::DownloadImage(url)) => download_image(url, notify_body),
            Some(IpcMessage::CopyToClipboard(text)) => {
                let _ = to_host.try_send(HostEvent::CopyToClipboard(text.to_string()));
            }
            Some(IpcMessage::ShowImage(url)) => {
                let _ = to_host.try_send(HostEvent::ImageShown(url.to_string()));
            }
            Some(IpcMessage::BodyMouseDown) => {
                let _ = to_host.try_send(HostEvent::BodyMouseDown);
            }
            None => {}
        }
    }

    /// Handles a "open in new window" request (links or images): the embedded
    /// reader never spawns its own window. Remote targets go to the system
    /// browser; an inline `data:` image — which has no URL to navigate to — is
    /// written to a temp file and handed to the OS default viewer instead.
    fn open_in_new_window(url: &str) {
        if is_external_link(url) {
            open_external(url);
        } else if let Some((extension, bytes)) = decode_data_uri(url) {
            if let Some(path) = persist_temp_file(extension, &bytes) {
                let _ = open::that_detached(path);
            }
        }
    }

    /// Saves an image straight to the user's Downloads folder (no dialog) and
    /// fires a desktop notification. This backs the custom context menu's
    /// "Download image", which exists because WebKit's own "Download Image" item
    /// never reaches the download delegate.
    ///
    /// Inline `data:` images are decoded and written directly. Remote images
    /// would need a network fetch we don't have yet, so we fall back to opening
    /// them in the browser, where the user can save them.
    fn download_image(url: &str, notify_body: &str) {
        let Some((extension, bytes)) = decode_data_uri(url) else {
            if is_external_link(url) {
                open_external(url);
            }
            return;
        };
        let Some(dir) = downloads_dir() else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = unique_download_path(&dir, "image", extension, |candidate| candidate.exists());
        if std::fs::write(&path, bytes).is_ok() {
            notify(notify_body);
        }
    }

    /// Shows a desktop notification confirming the download. On macOS we shell
    /// out to `osascript`, which works for an unbundled binary (`cargo run`) —
    /// unlike `UNUserNotificationCenter`, which needs an app bundle. Other
    /// platforms are a no-op until their notification backend lands.
    #[allow(unused_variables)]
    fn notify(body: &str) {
        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "display notification {} with title {}",
                super::applescript_string(body),
                super::applescript_string("rMail"),
            );
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .spawn();
        }
    }

    /// Writes `bytes` to a uniquely named file in the OS temp directory and
    /// returns its path. The nanosecond timestamp keeps successive opens from
    /// clobbering each other. Returns `None` if the write fails.
    fn persist_temp_file(extension: &str, bytes: &[u8]) -> Option<std::path::PathBuf> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("rmail-image-{nanos}.{extension}"));
        std::fs::write(&path, bytes).ok()?;
        Some(path)
    }

    /// A native webview hosted as a child of the GPUI window. It floats over the
    /// reader pane; we only have to keep its bounds, HTML and visibility in sync.
    pub struct EmailWebView {
        webview: WebView,
        last_html: String,
        last_bounds: Option<(f32, f32, f32, f32)>,
        visible: bool,
        /// Localized text shown after a successful image download. Shared with
        /// the IPC closure so [`Self::set_notify_text`] can relocalize it live
        /// (the view isn't rebuilt on a language switch).
        notify_body: Rc<RefCell<String>>,
    }

    impl EmailWebView {
        /// Creates the child webview hosted by `window`, initially hidden so it
        /// doesn't flash at the default origin before it is first positioned.
        pub fn new(
            window: &Window,
            html: &str,
            to_host: Sender<HostEvent>,
            notify_body: String,
        ) -> Option<Self> {
            let notify_body = Rc::new(RefCell::new(notify_body));
            let notify_for_ipc = notify_body.clone();
            let webview = WebViewBuilder::new()
                .with_html(html)
                // Links must open in the system browser, not hijack the reader.
                // We cancel external navigations and hand the URL to the OS.
                .with_navigation_handler(|url| {
                    if is_external_link(&url) {
                        open_external(&url);
                        false
                    } else {
                        true
                    }
                })
                // "Open Link/Image in New Window", `target="_blank"` and
                // `window.open` never spawn an embedded window: links go to the
                // system browser and inline images are opened by the OS.
                .with_new_window_req_handler(|url, _features| {
                    open_in_new_window(&url);
                    NewWindowResponse::Deny
                })
                // Hovered-link reporting + the custom image context menu (the
                // native "Download Image" is inert in WebKit, so we provide our
                // own and route the action through IPC).
                .with_initialization_script(CONTENT_SCRIPT)
                .with_ipc_handler(move |req| {
                    handle_ipc(&req.into_body(), &to_host, &notify_for_ipc.borrow());
                })
                // Never expose the OS Web Inspector ("Inspect Element"). wry turns
                // devtools on by default in debug builds, which both pollutes the
                // context menu and, once opened, attaches an inspector that resizes
                // the child WKWebView so it overflows the reader pane. This is an
                // e-mail reader, not a browser: the body stays sandboxed.
                .with_devtools(false)
                .with_visible(false)
                .build_as_child(window)
                .ok()?;
            Some(Self {
                webview,
                last_html: html.to_string(),
                last_bounds: None,
                visible: false,
                notify_body,
            })
        }

        /// Reloads the document if it changed (e.g. another message was selected
        /// or the theme toggled).
        pub fn set_html(&mut self, html: &str) {
            if self.last_html == html {
                return;
            }
            if self.webview.load_html(html).is_ok() {
                self.last_html = html.to_string();
            }
        }

        /// Updates the localized notification text used after a download, so a
        /// language switch is reflected without rebuilding the webview.
        pub fn set_notify_text(&self, body: String) {
            *self.notify_body.borrow_mut() = body;
        }

        /// Positions the webview over `bounds` (window-relative, logical pixels)
        /// and makes it visible. Bounds are only pushed to the OS when they change.
        pub fn position(&mut self, bounds: Bounds<Pixels>) {
            let next = (
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            );
            if self.last_bounds != Some(next) {
                let rect = Rect {
                    position: LogicalPosition::new(next.0, next.1).into(),
                    size: LogicalSize::new(next.2.max(1.0), next.3.max(1.0)).into(),
                };
                let _ = self.webview.set_bounds(rect);
                self.last_bounds = Some(next);
            }
            self.set_visible(true);
        }

        /// Closes any custom context menu currently open inside the document.
        /// Clicking a sibling GPUI view neither blurs the webview nor delivers a
        /// DOM event, so the host calls this on any outside click to dismiss it.
        pub fn dismiss_context_menu(&self) {
            let _ = self
                .webview
                .evaluate_script("window.__rmCloseMenu&&window.__rmCloseMenu()");
        }

        fn set_visible(&mut self, visible: bool) {
            if self.visible != visible && self.webview.set_visible(visible).is_ok() {
                self.visible = visible;
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use gpui::{Bounds, Pixels, Window};

    use super::HostEvent;

    /// No-op stand-in on targets without a supported webview backend (Linux).
    pub struct EmailWebView;

    impl EmailWebView {
        pub fn new(
            _window: &Window,
            _html: &str,
            _to_host: async_channel::Sender<HostEvent>,
            _notify_body: String,
        ) -> Option<Self> {
            None
        }
        pub fn set_html(&mut self, _html: &str) {}
        pub fn set_notify_text(&self, _body: String) {}
        pub fn position(&mut self, _bounds: Bounds<Pixels>) {}
        pub fn dismiss_context_menu(&self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    fn body_html() -> MessageBody {
        MessageBody::Html("<p>Hello <strong>world</strong></p>".into())
    }

    fn labels() -> ContextMenuLabels<'static> {
        ContextMenuLabels {
            image_open: "Open image in browser",
            image_download: "Download image",
            image_show: "Show remote image",
            link_open: "Open in browser",
            link_copy: "Copy link",
            selection_copy: "Copy",
            copy_shortcut: "\u{2318}C",
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
    fn external_links_route_to_the_browser() {
        assert!(is_external_link("https://example.com/path?q=1"));
        assert!(is_external_link("http://example.com"));
        assert!(is_external_link("  HTTPS://Example.com  "));
        assert!(is_external_link("mailto:someone@example.com"));
    }

    #[test]
    fn in_document_navigations_stay_in_place() {
        assert!(!is_external_link("about:blank"));
        assert!(!is_external_link("data:text/html,<p>hi</p>"));
        assert!(!is_external_link("#section"));
        assert!(!is_external_link(""));
    }

    #[test]
    fn base64_decode_reverses_known_vectors() {
        // RFC 4648 vectors, mirroring the encoder's tests in `data.rs`.
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64_decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        // Embedded whitespace (line wrapping) is tolerated.
        assert_eq!(base64_decode("Zm9v\n  YmFy").unwrap(), b"foobar");
    }

    #[test]
    fn base64_decode_rejects_out_of_alphabet_bytes() {
        assert!(base64_decode("not*valid").is_none());
    }

    #[test]
    fn decode_data_uri_extracts_image_bytes() {
        // "data:image/png;base64," + base64("foo")
        let uri = "data:image/png;base64,Zm9v";
        let (extension, bytes) = decode_data_uri(uri).expect("a base64 image data URI");
        assert_eq!(extension, "png");
        assert_eq!(bytes, b"foo");
    }

    #[test]
    fn decode_data_uri_is_case_insensitive_on_the_scheme() {
        let (extension, _) =
            decode_data_uri("DATA:image/jpeg;base64,Zm9v").expect("scheme is case-insensitive");
        assert_eq!(extension, "jpg");
    }

    #[test]
    fn decode_data_uri_rejects_non_data_and_non_base64() {
        assert!(decode_data_uri("https://example.com/cat.png").is_none());
        // The percent-encoded text form is not something we materialize.
        assert!(decode_data_uri("data:text/plain,hello").is_none());
        // Empty payloads carry nothing to open.
        assert!(decode_data_uri("data:image/png;base64,").is_none());
    }

    #[test]
    fn extension_for_mime_maps_known_image_types() {
        assert_eq!(extension_for_mime("image/png"), "png");
        assert_eq!(extension_for_mime("image/jpeg"), "jpg");
        assert_eq!(extension_for_mime("image/gif"), "gif");
        assert_eq!(extension_for_mime("image/svg+xml"), "svg");
        assert_eq!(extension_for_mime("application/octet-stream"), "bin");
    }

    #[test]
    fn parse_ipc_message_routes_known_tags() {
        assert_eq!(
            parse_ipc_message("H\nhttps://example.com"),
            Some(IpcMessage::Hover("https://example.com"))
        );
        // An empty hover payload (cursor left the link) is preserved.
        assert_eq!(parse_ipc_message("H\n"), Some(IpcMessage::Hover("")));
        assert_eq!(
            parse_ipc_message("O\ndata:image/png;base64,Zm9v"),
            Some(IpcMessage::OpenExternal("data:image/png;base64,Zm9v"))
        );
        assert_eq!(
            parse_ipc_message("D\ndata:image/png;base64,Zm9v"),
            Some(IpcMessage::DownloadImage("data:image/png;base64,Zm9v"))
        );
        assert_eq!(
            parse_ipc_message("C\nhttps://example.com/page"),
            Some(IpcMessage::CopyToClipboard("https://example.com/page"))
        );
        assert_eq!(
            parse_ipc_message("S\nhttps://tracker.test/p.gif"),
            Some(IpcMessage::ShowImage("https://tracker.test/p.gif"))
        );
    }

    #[test]
    fn parse_ipc_message_rejects_unknown_or_malformed() {
        assert_eq!(parse_ipc_message("X\npayload"), None);
        // Missing the `\n` separator.
        assert_eq!(parse_ipc_message("Hhttps://example.com"), None);
    }

    #[test]
    fn unique_download_path_appends_counter_when_taken() {
        let dir = Path::new("/tmp/dl");
        // Nothing exists yet → the plain name.
        let first = unique_download_path(dir, "image", "png", |_| false);
        assert_eq!(first, dir.join("image.png"));
        // The plain name and the first two numbered variants are taken.
        let taken = ["image.png", "image (1).png", "image (2).png"];
        let path = unique_download_path(dir, "image", "png", |candidate| {
            taken.iter().any(|name| candidate == dir.join(name))
        });
        assert_eq!(path, dir.join("image (3).png"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_string_quotes_and_escapes() {
        assert_eq!(applescript_string("hi"), "\"hi\"");
        // Embedded quotes and backslashes are escaped so the literal is safe.
        assert_eq!(applescript_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
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
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.0, 0.0, 0.9, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
            &body_html(),
            labels(),
            true,
            &HashSet::new(),
        )
        .html;
        assert!(doc.starts_with("<!DOCTYPE html>"));
        assert!(doc.contains("<p>Hello <strong>world</strong></p>"));
        // A dark background selects the dark color scheme.
        assert!(doc.contains("color-scheme: dark"));
        // Custom scrollbar styling opts the body out of macOS overlay scrollbars
        // (which flash on trackpad gestures); it must always be emitted.
        assert!(doc.contains("::-webkit-scrollbar"));
        assert!(doc.contains("::-webkit-scrollbar-thumb"));
    }

    #[test]
    fn document_escapes_and_wraps_plain_text() {
        let body = MessageBody::Text("1 < 2 & 3".into());
        let doc = email_document(
            hsla(0.0, 0.0, 0.95, 1.0),
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
            &body,
            labels(),
            true,
            &HashSet::new(),
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
    fn sanitize_keeps_svg_but_removes_its_scriptable_parts() {
        let dirty = "<svg viewBox=\"0 0 10 10\">\
             <rect width=\"10\" height=\"10\"></rect>\
             <script>alert(1)</script>\
             <foreignObject><body>x</body></foreignObject>\
             <animate attributeName=\"x\"></animate>\
             <set attributeName=\"onload\" to=\"alert(1)\"></set>\
             </svg>";
        let clean = sanitize_html(dirty, true);
        let lower = clean.to_ascii_lowercase();
        assert!(clean.contains("<svg"));
        assert!(clean.contains("<rect"));
        for needle in ["<script", "alert(1)", "foreignobject", "<animate", "<set"] {
            assert!(
                !lower.contains(needle),
                "expected `{needle}` to be stripped from svg, got: {clean}"
            );
        }
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
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.0, 0.0, 0.9, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
            &body,
            labels(),
            true,
            &HashSet::new(),
        )
        .html;
        assert!(doc.contains("<p>safe</p>"));
        assert!(!doc.contains("<iframe"));
        assert!(!doc.contains("<input"));
        assert!(!doc.contains("<video"));
    }

    #[test]
    fn document_reports_remote_image_state() {
        let url = "https://tracker.test/p.gif";
        let body = MessageBody::Html(format!("<img src=\"{url}\">").into());
        let colors = (
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.0, 0.0, 0.9, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
        );
        let none = HashSet::new();
        // Loading off → the image is present and blocked.
        let blocked = email_document(colors.0, colors.1, colors.2, &body, labels(), false, &none);
        assert!(blocked.has_remote);
        assert_eq!(blocked.blocked_images, 1);
        // Loading on → present but nothing blocked.
        let allowed = email_document(colors.0, colors.1, colors.2, &body, labels(), true, &none);
        assert!(allowed.has_remote);
        assert_eq!(allowed.blocked_images, 0);
        // Loading off but this image individually shown → present, not blocked.
        let shown: HashSet<String> = [url.to_string()].into_iter().collect();
        let revealed = email_document(colors.0, colors.1, colors.2, &body, labels(), false, &shown);
        assert!(revealed.has_remote);
        assert_eq!(revealed.blocked_images, 0);
        // A message with no remote resources never reports remote content.
        let local = email_document(
            colors.0,
            colors.1,
            colors.2,
            &body_html(),
            labels(),
            false,
            &none,
        );
        assert!(!local.has_remote);
        assert_eq!(local.blocked_images, 0);
    }

    #[test]
    fn document_embeds_escaped_menu_labels() {
        let doc = email_document(
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.0, 0.0, 0.9, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
            &body_html(),
            ContextMenuLabels {
                image_open: "Open \"image\"",
                image_download: "Download",
                image_show: "Show",
                link_open: "Open",
                link_copy: "Copy link",
                selection_copy: "Copy",
                copy_shortcut: "\u{2318}C",
            },
            true,
            &HashSet::new(),
        )
        .html;
        // Labels ride on the body as data-* attributes, HTML-escaped.
        assert!(doc.contains("data-rm-img-open=\"Open &quot;image&quot;\""));
        assert!(doc.contains("data-rm-img-download=\"Download\""));
        assert!(doc.contains("data-rm-img-show=\"Show\""));
        assert!(doc.contains("data-rm-link-open=\"Open\""));
        assert!(doc.contains("data-rm-link-copy=\"Copy link\""));
        assert!(doc.contains("data-rm-sel-copy=\"Copy\""));
        assert!(doc.contains("data-rm-copy-key=\"\u{2318}C\""));
    }
}
