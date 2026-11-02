//! Downloaded raw message → what `storage::Database::store_body` keeps.

use mail_core::{inline_images, parse_message, readable_text};
use storage::{Database, NewAttachment};

/// Upper bound of image bytes inlined as `data:` URIs into one stored HTML
/// body; images beyond it keep their `cid:` reference.
pub const MAX_INLINE_BYTES: usize = 5 * 1024 * 1024;

/// Characters kept in the list preview.
pub const PREVIEW_CHARS: usize = 200;

/// Storage-ready body of one message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BodyContent {
    /// Reader content: HTML (with `cid:` images inlined) or plain text.
    pub content: String,
    /// `"html"` or `"text"`.
    pub format: &'static str,
    /// Searchable / quotable text (`\n` line endings).
    pub plain_text: String,
    /// First [`PREVIEW_CHARS`] characters of `plain_text`, whitespace collapsed.
    pub preview: String,
    pub attachments: Vec<NewAttachment>,
}

/// Splits a raw RFC 822 message into reader content, plain text, preview and
/// attachment metadata. HTML is preferred; plain text comes from the text
/// part or, for HTML-only mail, from `html_to_text`. Unparseable input yields
/// an empty text body.
pub fn body_content(raw: &[u8]) -> BodyContent {
    let parsed = parse_message(raw).unwrap_or_default();
    let (content, format, plain_text) = match (&parsed.html, &parsed.text) {
        (Some(html), text) => {
            let content = if html.to_ascii_lowercase().contains("cid:") {
                inline_cid_images(html, &inline_images(raw), MAX_INLINE_BYTES)
            } else {
                html.clone()
            };
            let plain = readable_text(text.as_deref(), Some(html));
            (content, "html", plain)
        }
        (None, Some(text)) => (text.replace("\r\n", "\n"), "text", text.clone()),
        (None, None) => (String::new(), "text", String::new()),
    };
    let plain_text = plain_text.replace("\r\n", "\n");
    BodyContent {
        preview: preview_text(&plain_text, PREVIEW_CHARS),
        attachments: parsed
            .attachments
            .iter()
            .map(|a| NewAttachment {
                part_index: u32::try_from(a.index).unwrap_or(u32::MAX),
                filename: a.filename.clone(),
                mime_type: a.mime_type.clone(),
                size: a.size as u64,
                content_id: a.content_id.clone(),
                inline: a.inline,
            })
            .collect(),
        content,
        format,
        plain_text,
    }
}

/// Re-derives the plain text and preview of stored bodies written before
/// HTML in text parts and named entities were handled, so their list rows
/// stop showing markup. Failures only leave a row as it was.
pub fn repair_stale_previews(db: &Database, account_id: i64) -> rusqlite::Result<()> {
    for id in db.messages_with_markup_preview(account_id)? {
        let Some(raw) = db.message_raw(id)? else {
            continue;
        };
        let body = body_content(&raw);
        db.update_body_text(id, &body.plain_text, &body.preview)?;
    }
    Ok(())
}

/// Whitespace collapsed to single spaces, cut at `max_chars` characters.
pub fn preview_text(plain: &str, max_chars: usize) -> String {
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect()
}

/// Rewrites `cid:` URLs used as attribute values (`src="cid:x"`,
/// `background=cid:x`) or CSS `url(cid:x)` into `data:` URIs from `images`
/// (`(content_id, mime, bytes)` as returned by `mail_core::inline_images`),
/// so the reader (which never fetches by itself) shows embedded images.
/// Stops inlining once `max_bytes` image bytes were embedded; unknown ids
/// and other text are left untouched.
pub fn inline_cid_images(
    html: &str,
    images: &[(String, String, Vec<u8>)],
    max_bytes: usize,
) -> String {
    if images.is_empty() {
        return html.to_string();
    }
    // ASCII lowercasing keeps byte offsets identical to `html`.
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut budget = max_bytes;
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("cid:") {
        let start = pos + found;
        let value_start = start + "cid:".len();
        let value_end = html[value_start..]
            .find(|c: char| matches!(c, '"' | '\'' | ')' | '>') || c.is_whitespace())
            .map_or(html.len(), |end| value_start + end);
        let prefix = &lower[..start];
        let prefix = prefix
            .strip_suffix(['"', '\''])
            .unwrap_or(prefix)
            .trim_end();
        let is_url = prefix.ends_with('=') || prefix.ends_with("url(");
        let cid = percent_decode(&html[value_start..value_end]);
        let image = images
            .iter()
            .find(|(id, _, _)| id.eq_ignore_ascii_case(&cid))
            .filter(|(_, _, bytes)| is_url && bytes.len() <= budget);
        match image {
            Some((_, mime, bytes)) => {
                budget -= bytes.len();
                out.push_str(&html[pos..start]);
                out.push_str("data:");
                out.push_str(mime);
                out.push_str(";base64,");
                out.push_str(&base64_encode(bytes));
            }
            None => out.push_str(&html[pos..value_end]),
        }
        pos = value_end;
    }
    out.push_str(&html[pos..]);
    out
}

/// `%XX` decoding for `cid:` URLs (RFC 2392 encodes reserved characters).
/// Invalid escapes are kept verbatim.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Standard base64 with padding (RFC 4648); small enough not to warrant a
/// dependency.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> shift & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALTERNATIVE: &[u8] = b"From: a@x\r\n\
Subject: Hi\r\n\
Content-Type: multipart/alternative; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
Plain\r\nbody\r\n\
--b\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>HTML body</p>\r\n\
--b--\r\n";

    const RELATED: &[u8] = b"From: a@x\r\n\
Subject: Logo\r\n\
Content-Type: multipart/related; boundary=\"r\"\r\n\
\r\n\
--r\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>Hello</p><img src=\"cid:logo@x\"><img src=\"cid:missing@x\">\r\n\
--r\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Disposition: inline\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0K\r\n\
--r--\r\n";

    const MIXED: &[u8] = b"From: a@x\r\n\
Subject: Report\r\n\
Content-Type: multipart/mixed; boundary=\"m\"\r\n\
\r\n\
--m\r\n\
Content-Type: text/plain\r\n\
\r\n\
See   attached.\r\n\
--m\r\n\
Content-Type: application/pdf; name=\"r.pdf\"\r\n\
Content-Disposition: attachment; filename=\"r.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQgZmFrZQ==\r\n\
--m--\r\n";

    #[test]
    fn html_preferred_with_text_part_as_plain() {
        let body = body_content(ALTERNATIVE);
        assert_eq!(body.format, "html");
        assert!(body.content.contains("<p>HTML body</p>"));
        assert_eq!(body.plain_text.trim(), "Plain\nbody");
        assert_eq!(body.preview, "Plain body");
        assert!(body.attachments.is_empty());
    }

    #[test]
    fn html_only_inlines_cid_images_and_derives_plain_text() {
        let body = body_content(RELATED);
        assert_eq!(body.format, "html");
        assert!(body
            .content
            .contains("src=\"data:image/png;base64,iVBORw0K\""));
        assert!(body.content.contains("src=\"cid:missing@x\""));
        assert_eq!(body.plain_text.trim(), "Hello");
        assert_eq!(body.attachments.len(), 1);
        assert!(body.attachments[0].inline);
        assert_eq!(body.attachments[0].content_id.as_deref(), Some("logo@x"));
    }

    #[test]
    fn text_only_and_attachments() {
        let body = body_content(MIXED);
        assert_eq!(body.format, "text");
        assert_eq!(body.content.trim(), "See   attached.");
        assert_eq!(body.preview, "See attached.");
        assert_eq!(body.attachments.len(), 1);
        let pdf = &body.attachments[0];
        assert_eq!(pdf.filename, "r.pdf");
        assert_eq!(pdf.mime_type, "application/pdf");
        assert_eq!(pdf.size, 13);
        assert!(!pdf.inline);
    }

    #[test]
    fn unparseable_body_is_empty_text() {
        let body = body_content(b"");
        assert_eq!(body.format, "text");
        assert!(body.content.is_empty() && body.plain_text.is_empty());
        let headers_only = body_content(b"Subject: x\r\n\r\n");
        assert_eq!(headers_only.format, "text");
        assert!(headers_only.content.is_empty());
    }

    #[test]
    fn html_in_the_text_part_gives_a_readable_preview() {
        let raw = b"From: a@b\r\nSubject: s\r\nMIME-Version: 1.0\r\n\
            Content-Type: multipart/alternative; boundary=x\r\n\r\n\
            --x\r\nContent-Type: text/plain\r\n\r\n\
            <!DOCTYPE html><html><head><style>p{}</style></head><body>Sua fatura</body></html>\r\n\
            --x\r\nContent-Type: text/html\r\n\r\n\
            <html><body><p>Sua fatura&zwnj; chegou</p></body></html>\r\n--x--\r\n";
        let body = body_content(raw);
        assert_eq!(body.format, "html");
        assert_eq!(body.preview, "Sua fatura chegou");
    }

    #[test]
    fn preview_collapses_and_truncates() {
        assert_eq!(preview_text("  a\n\n b\tc  ", 10), "a b c");
        assert_eq!(preview_text("áéíóú", 3), "áéí");
        assert_eq!(preview_text("", 3), "");
    }

    #[test]
    fn inline_cid_images_variants_and_budget() {
        let images = vec![
            ("a@x".to_string(), "image/png".to_string(), vec![1, 2, 3]),
            ("b c@x".to_string(), "image/gif".to_string(), vec![4; 4]),
        ];
        let html = "<img src='CID:A@X'><td background=cid:b%20c@x>\
<div style=\"background:url(cid:a@x)\">cid:a@x as text</div>";
        let out = inline_cid_images(html, &images, 100);
        assert_eq!(
            out,
            "<img src='data:image/png;base64,AQID'><td background=data:image/gif;base64,BAQEBA==>\
<div style=\"background:url(data:image/png;base64,AQID)\">cid:a@x as text</div>"
        );
        // Budget: the first image fits (3 bytes), the second (4) does not.
        let capped = inline_cid_images("<img src=\"cid:a@x\"><img src=\"cid:b c@x\">", &images, 5);
        assert_eq!(
            capped,
            "<img src=\"data:image/png;base64,AQID\"><img src=\"cid:b c@x\">"
        );
        assert_eq!(
            inline_cid_images("<img src=cid:a@x>", &[], 100),
            "<img src=cid:a@x>"
        );
        assert_eq!(
            inline_cid_images("é src=cid:a@x", &images, 100),
            "é src=data:image/png;base64,AQID"
        );
    }

    #[test]
    fn percent_decode_handles_escapes() {
        assert_eq!(percent_decode("a%20b%40c"), "a b@c");
        assert_eq!(percent_decode("bad%zzend%2"), "bad%zzend%2");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        let vectors = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in vectors {
            assert_eq!(base64_encode(input.as_bytes()), expected);
        }
    }
}
