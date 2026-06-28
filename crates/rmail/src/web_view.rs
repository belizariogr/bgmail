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

use gpui::{Hsla, Rgba};

use crate::data::MessageBody;

/// Whether the native embedded webview backend is available on this target.
pub const WEBVIEW_SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// Builds a self-contained HTML document for `body`, themed to match the current
/// app colors. Plain-text bodies are HTML-escaped and wrapped so they keep their
/// line breaks and wrap to the pane width.
pub fn email_document(background: Hsla, text: Hsla, accent: Hsla, body: &MessageBody) -> String {
    let inner = match body {
        MessageBody::Html(html) => html.to_string(),
        MessageBody::Text(plain) => format!("<pre class=\"plain\">{}</pre>", escape_html(plain)),
    };

    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <style>{css}</style></head><body>{inner}</body></html>",
        css = document_css(background, text, accent),
    )
}

/// Theme-aware stylesheet shared by every rendered message. Colors come straight
/// from the active theme so the webview matches the surrounding UI (incl. dark
/// mode), instead of the engine's default white page.
fn document_css(background: Hsla, text: Hsla, accent: Hsla) -> String {
    let scheme = if background.l < 0.5 { "dark" } else { "light" };
    format!(
        ":root {{ color-scheme: {scheme}; }}\
         html, body {{ margin: 0; padding: 16px 24px; background: {bg}; color: {fg}; \
           font: 14px/1.55 -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; \
           -webkit-font-smoothing: antialiased; overflow-wrap: anywhere; }}\
         a {{ color: {accent}; }}\
         img {{ display: block; }}\
         h1, h2, h3 {{ line-height: 1.25; }}\
         code, pre {{ font-family: 'SF Mono', ui-monospace, Menlo, Consolas, monospace; font-size: 13px; }}\
         pre {{ white-space: pre-wrap; background: {soft}; padding: 12px; border-radius: 6px; }}\
         pre.plain {{ background: transparent; padding: 0; }}\
         code {{ background: {soft}; padding: 1px 4px; border-radius: 4px; }}\
         blockquote {{ margin: 0; padding-left: 12px; border-left: 3px solid {accent}; opacity: 0.85; }}\
         hr {{ border: none; border-top: 1px solid {fg}; opacity: 0.15; margin: 16px 0; }}",
        bg = css_color(background),
        fg = css_color(text),
        accent = css_color(accent),
        // A subtle fill for code blocks/inline code, derived from the text color.
        soft = css_color_alpha(text, 0.08),
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

/// Injected into every rendered message: reports the destination of the link
/// under the cursor (or an empty string when none) to the host via the IPC
/// channel, so the UI can mirror it in the status bar like a browser does.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const HOVER_SCRIPT: &str = r#"(function () {
  var current = null;
  function anchorHref(el) {
    while (el && el.nodeType === 1) {
      if (el.tagName === 'A' && el.href) return el.href;
      el = el.parentElement;
    }
    return '';
  }
  function report(href) {
    if (href !== current) {
      current = href;
      window.ipc.postMessage(href);
    }
  }
  document.addEventListener('mouseover', function (e) { report(anchorHref(e.target)); }, true);
  document.addEventListener('mouseleave', function () { report(''); }, true);
})();"#;

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

    use super::{is_external_link, HOVER_SCRIPT};

    /// Opens `url` in the user's default browser, detached so it never blocks the
    /// UI thread. Errors are ignored: a failed launch shouldn't crash the reader.
    fn open_external(url: &str) {
        let _ = open::that_detached(url);
    }

    /// A native webview hosted as a child of the GPUI window. It floats over the
    /// reader pane; we only have to keep its bounds, HTML and visibility in sync.
    pub struct EmailWebView {
        webview: WebView,
        last_html: String,
        last_bounds: Option<(f32, f32, f32, f32)>,
        visible: bool,
    }

    impl EmailWebView {
        /// Creates the child webview hosted by `window`, initially hidden so it
        /// doesn't flash at the default origin before it is first positioned.
        pub fn new(window: &Window, html: &str, on_hover: Sender<String>) -> Option<Self> {
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
                // `target="_blank"` / `window.open` links never spawn an embedded
                // window; they go to the browser too.
                .with_new_window_req_handler(|url, _features| {
                    if is_external_link(&url) {
                        open_external(&url);
                    }
                    NewWindowResponse::Deny
                })
                // Mirror the hovered link's URL into the status bar.
                .with_initialization_script(HOVER_SCRIPT)
                .with_ipc_handler(move |req| {
                    let _ = on_hover.try_send(req.into_body());
                })
                .with_visible(false)
                .build_as_child(window)
                .ok()?;
            Some(Self {
                webview,
                last_html: html.to_string(),
                last_bounds: None,
                visible: false,
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

    /// No-op stand-in on targets without a supported webview backend (Linux).
    pub struct EmailWebView;

    impl EmailWebView {
        pub fn new(
            _window: &Window,
            _html: &str,
            _on_hover: async_channel::Sender<String>,
        ) -> Option<Self> {
            None
        }
        pub fn set_html(&mut self, _html: &str) {}
        pub fn position(&mut self, _bounds: Bounds<Pixels>) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    fn body_html() -> MessageBody {
        MessageBody::Html("<p>Hello <strong>world</strong></p>".into())
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
        );
        assert!(doc.starts_with("<!DOCTYPE html>"));
        assert!(doc.contains("<p>Hello <strong>world</strong></p>"));
        // A dark background selects the dark color scheme.
        assert!(doc.contains("color-scheme: dark"));
    }

    #[test]
    fn document_escapes_and_wraps_plain_text() {
        let body = MessageBody::Text("1 < 2 & 3".into());
        let doc = email_document(
            hsla(0.0, 0.0, 0.95, 1.0),
            hsla(0.0, 0.0, 0.1, 1.0),
            hsla(0.6, 0.7, 0.5, 1.0),
            &body,
        );
        assert!(doc.contains("<pre class=\"plain\">1 &lt; 2 &amp; 3</pre>"));
        // A light background selects the light color scheme.
        assert!(doc.contains("color-scheme: light"));
    }
}
