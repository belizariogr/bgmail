//! Turning raw RFC 5322 bytes into the fields BGMail stores and shows.
//!
//! `mail-parser` does the heavy lifting (MIME, charsets, RFC 2047); this
//! module pins down *which* of its views we trust so storage and UI agree.

use mail_parser::{Message, MessageParser, MessagePart, MimeHeaders, PartType};
use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::compose::{extension_for_mime, strip_brackets};

/// The parts of a message the app needs, decoupled from `mail-parser`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedMessage {
    /// Without angle brackets.
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub from: Option<Address>,
    pub sender: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    /// Unix seconds, UTC.
    pub date: Option<i64>,
    /// The message's own HTML body; never synthesized from plain text.
    pub html: Option<String>,
    /// The message's own plain-text body; never synthesized from HTML.
    pub text: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
}

/// Metadata for a downloadable or `cid:`-referenced part. Serializable so it
/// can be persisted alongside the message row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentInfo {
    /// `mail-parser` part id; stable for the same raw bytes, so it can be fed
    /// back into [`attachment_content`].
    pub index: usize,
    pub filename: String,
    pub mime_type: String,
    /// Decoded size in bytes.
    pub size: usize,
    /// Without angle brackets.
    pub content_id: Option<String>,
    /// What the bytes of a text part are in, as declared.
    pub charset: Option<String>,
    /// Embedded in the HTML body (`cid:`) rather than a user-facing attachment.
    pub inline: bool,
}

/// Parses a full message or just its header block (bodies are then `None`).
/// Returns `None` only when no headers can be found at all.
pub fn parse_message(raw: &[u8]) -> Option<ParsedMessage> {
    let message = MessageParser::default().parse(raw)?;
    if message.headers().is_empty() {
        return None;
    }
    let html = html_body(&message);
    Some(ParsedMessage {
        message_id: message.message_id().map(clean_id).filter(|i| !i.is_empty()),
        in_reply_to: id_list(message.in_reply_to()).into_iter().next(),
        references: id_list(message.references()),
        subject: message.subject().unwrap_or_default().trim().to_string(),
        from: message.from().and_then(|a| addresses(a).into_iter().next()),
        sender: message
            .sender()
            .and_then(|a| addresses(a).into_iter().next()),
        to: message.to().map(addresses).unwrap_or_default(),
        cc: message.cc().map(addresses).unwrap_or_default(),
        bcc: message.bcc().map(addresses).unwrap_or_default(),
        reply_to: message.reply_to().map(addresses).unwrap_or_default(),
        date: message.date().map(|d| d.to_timestamp()),
        attachments: attachment_parts(&message)
            .map(|(index, part)| describe(raw, index, part, html.as_deref().unwrap_or_default()))
            .collect(),
        html,
        text: text_body(&message),
    })
}

/// Decoded bytes of the part listed as `AttachmentInfo::index`.
pub fn attachment_content(raw: &[u8], index: usize) -> Option<Vec<u8>> {
    let message = MessageParser::default().parse(raw)?;
    let part = message.parts.get(index)?;
    part_bytes(raw, part)
}

/// `(content_id, mime_type, bytes)` for every image with a Content-ID, i.e.
/// everything an HTML body can reference via `cid:`.
pub fn inline_images(raw: &[u8]) -> Vec<(String, String, Vec<u8>)> {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    attachment_parts(&message)
        .filter_map(|(_, part)| {
            let cid = part.content_id().map(clean_id).filter(|c| !c.is_empty())?;
            let mime = mime_type(part);
            if !mime.starts_with("image/") {
                return None;
            }
            Some((cid, mime, part_bytes(raw, part)?))
        })
        .collect()
}

/// True when the message has user-facing (non-inline) attachments.
pub fn has_attachments(message: &ParsedMessage) -> bool {
    message.attachments.iter().any(|a| !a.inline)
}

/// Parts worth listing: mail-parser's attachments, plus any non-body part
/// carrying a Content-ID (inline images inside multipart/related).
fn attachment_parts<'m>(
    message: &'m Message<'m>,
) -> impl Iterator<Item = (usize, &'m MessagePart<'m>)> + 'm {
    message
        .parts
        .iter()
        .enumerate()
        .filter(move |(index, part)| {
            let id = *index as u32;
            match part.body {
                PartType::Multipart(_) => false,
                PartType::Binary(_) | PartType::InlineBinary(_) | PartType::Message(_) => {
                    message.attachments.contains(&id) || part.content_id().is_some()
                }
                PartType::Text(_) | PartType::Html(_) => {
                    message.attachments.contains(&id)
                        && !message.text_body.contains(&id)
                        && !message.html_body.contains(&id)
                }
            }
        })
}

fn describe(raw: &[u8], index: usize, part: &MessagePart<'_>, html: &str) -> AttachmentInfo {
    let mime_type = mime_type(part);
    let content_id = part.content_id().map(clean_id).filter(|c| !c.is_empty());
    let is_attachment_disposition = part
        .content_disposition()
        .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"));
    // Only an image the body shows is out of the user's sight; a part that just has
    // a Content-ID (a PDF, a photo under a text body) would otherwise vanish.
    let shown_in_body = mime_type.starts_with("image/")
        && content_id
            .as_deref()
            .is_some_and(|cid| html_references_cid(html, cid));
    let filename = part
        .attachment_name()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match extension_for_mime(&mime_type) {
            Some(ext) => format!("attachment.{ext}"),
            None => "attachment".to_string(),
        });
    AttachmentInfo {
        index,
        filename,
        size: part_bytes(raw, part).map_or(0, |b| b.len()),
        charset: part
            .content_type()
            .and_then(|ct| ct.attribute("charset"))
            // It comes decoded and goes back into a header when forwarding: anything but
            // a plain token could add headers or take over the part.
            .filter(|charset| {
                !charset.is_empty()
                    && charset
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._:+-".contains(&b))
            })
            .map(str::to_string),
        inline: shown_in_body && !is_attachment_disposition,
        content_id,
        mime_type,
    }
}

/// Whether the HTML refers to exactly this Content-ID, in any case and percent-encoded
/// or not: `cid:part1` isn't `cid:part10`.
pub fn html_references_cid(html: &str, cid: &str) -> bool {
    let html = html.to_ascii_lowercase();
    let cid = cid.to_ascii_lowercase();
    html.match_indices("cid:").any(|(start, _)| {
        let rest = &html[start + "cid:".len()..];
        let end = rest
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | '&' | ';')
            })
            .unwrap_or(rest.len());
        percent_decoded(&rest[..end]) == cid
    })
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escaped {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime_type(part: &MessagePart<'_>) -> String {
    match part.content_type() {
        Some(ct) => match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub).to_ascii_lowercase(),
            None => ct.ctype().to_ascii_lowercase(),
        },
        None => match part.body {
            PartType::Text(_) => "text/plain".into(),
            PartType::Html(_) => "text/html".into(),
            PartType::Message(_) => "message/rfc822".into(),
            _ => "application/octet-stream".into(),
        },
    }
}

/// The part's bytes as sent. A text part comes from the raw message rather than
/// mail-parser's text, already decoded to UTF-8 with whatever it couldn't read lost:
/// a Latin-1 CSV has to save and forward unchanged.
fn part_bytes(raw: &[u8], part: &MessagePart<'_>) -> Option<Vec<u8>> {
    match &part.body {
        PartType::Binary(bytes) | PartType::InlineBinary(bytes) => Some(bytes.to_vec()),
        PartType::Text(text) | PartType::Html(text) => {
            original_bytes(raw, part).or_else(|| Some(text.as_bytes().to_vec()))
        }
        // Not `raw_message`: without an encoding, mail-parser leaves the whole outer
        // message there.
        PartType::Message(_) => original_bytes(raw, part),
        PartType::Multipart(_) => None,
    }
}

/// Reverts only the Content-Transfer-Encoding of the part's raw body.
fn original_bytes(raw: &[u8], part: &MessagePart<'_>) -> Option<Vec<u8>> {
    let body = raw.get(part.offset_body as usize..part.offset_end as usize)?;
    match part.encoding {
        mail_parser::Encoding::Base64 => mail_parser::decoders::base64::base64_decode(body),
        mail_parser::Encoding::QuotedPrintable => {
            mail_parser::decoders::quoted_printable::quoted_printable_decode(body)
        }
        mail_parser::Encoding::None => Some(body.to_vec()),
    }
}

/// The HTML of every body part in order, with text parts between them marked up: a
/// message can have several, as when text follows a PDF in Apple Mail. `None` without
/// an HTML part: mail-parser also lists text parts as the HTML body of text-only mail.
fn html_body(message: &Message<'_>) -> Option<String> {
    let parts = body_parts(message, &message.html_body);
    if !parts
        .iter()
        .any(|part| matches!(part.body, PartType::Html(_)))
    {
        return None;
    }
    let html: String = parts
        .iter()
        .map(|part| match &part.body {
            PartType::Html(html) => html.to_string(),
            PartType::Text(text) => format!(
                "<div style=\"white-space:pre-wrap\">{}</div>",
                html_escape::encode_text(text)
            ),
            _ => String::new(),
        })
        .collect();
    (!html.is_empty()).then_some(html)
}

/// The text of every body part in order, [`html_body`]'s counterpart.
fn text_body(message: &Message<'_>) -> Option<String> {
    let parts = body_parts(message, &message.text_body);
    if !parts
        .iter()
        .any(|part| matches!(part.body, PartType::Text(_)))
    {
        return None;
    }
    let text = parts
        .iter()
        .filter_map(|part| match &part.body {
            PartType::Text(text) => Some(text.to_string()),
            PartType::Html(html) => Some(crate::reply::html_to_text(html)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!text.is_empty()).then_some(text)
}

fn body_parts<'m>(message: &'m Message<'m>, ids: &[u32]) -> Vec<&'m MessagePart<'m>> {
    ids.iter()
        .filter_map(|id| message.parts.get(*id as usize))
        .collect()
}

fn addresses(address: &mail_parser::Address<'_>) -> Vec<Address> {
    address
        .iter()
        .filter_map(|addr| {
            let email = addr.address()?.trim();
            (!email.is_empty()).then(|| Address::new(addr.name(), email))
        })
        .collect()
}

fn id_list(value: &mail_parser::HeaderValue<'_>) -> Vec<String> {
    let ids: Vec<String> = match value.as_text_list() {
        Some(list) => list.iter().map(|id| clean_id(id)).collect(),
        None => value.as_text().map(clean_id).into_iter().collect(),
    };
    ids.into_iter().filter(|i| !i.is_empty()).collect()
}

fn clean_id(id: &str) -> String {
    strip_brackets(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn multipart(kind: &str, parts: &[&str]) -> Vec<u8> {
        let mut raw = format!("From: a@b.c\r\nContent-Type: multipart/{kind}; boundary=X\r\n\r\n");
        for part in parts {
            raw.push_str(&format!("--X\r\n{part}\r\n"));
        }
        raw.push_str("--X--\r\n");
        raw.into_bytes()
    }

    const TEXT: &str = "Content-Type: text/plain\r\n\r\n";
    const HTML: &str = "Content-Type: text/html\r\n\r\n";

    #[test]
    fn every_body_part_counts() {
        let raw = multipart(
            "mixed",
            &[&format!("{TEXT}INTRO"), &format!("{HTML}<p>RICH</p>")],
        );
        let parsed = parse_message(&raw).unwrap();
        let html = parsed.html.unwrap();
        assert!(
            html.contains("INTRO") && html.contains("<p>RICH</p>"),
            "{html}"
        );
        let text = parsed.text.unwrap();
        assert!(text.contains("INTRO") && text.contains("RICH"), "{text}");
        assert!(parsed.attachments.is_empty());

        let raw = multipart(
            "mixed",
            &[&format!("{TEXT}FIRST"), &format!("{TEXT}SECOND")],
        );
        let parsed = parse_message(&raw).unwrap();
        assert_eq!(parsed.text.as_deref(), Some("FIRST\n\nSECOND"));
        assert_eq!(parsed.html, None);

        let raw = multipart(
            "mixed",
            &[
                &format!("{HTML}<p>FIRST</p>"),
                "Content-Type: application/pdf; name=r.pdf\r\nContent-Disposition: attachment; filename=r.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAA",
                &format!("{HTML}<p>SECOND</p>"),
            ],
        );
        let parsed = parse_message(&raw).unwrap();
        let html = parsed.html.unwrap();
        assert!(html.contains("FIRST") && html.contains("SECOND"), "{html}");
        assert_eq!(parsed.attachments.len(), 1);
    }

    #[test]
    fn text_attachments_keep_their_bytes() {
        let csv = b"S\xe3o Paulo;1\r\n";
        for (headers, expected_charset) in [
            ("Content-Type: text/csv; name=a.csv", None),
            (
                "Content-Type: text/csv; charset=iso-8859-1; name=a.csv",
                Some("iso-8859-1"),
            ),
        ] {
            let mut raw = b"From: a@b.c\r\nContent-Type: multipart/mixed; boundary=X\r\n\r\n--X\r\nContent-Type: text/plain\r\n\r\nHi\r\n--X\r\n".to_vec();
            raw.extend_from_slice(headers.as_bytes());
            raw.extend_from_slice(b"\r\nContent-Disposition: attachment; filename=a.csv\r\nContent-Transfer-Encoding: 8bit\r\n\r\n");
            raw.extend_from_slice(csv);
            raw.extend_from_slice(b"\r\n--X--\r\n");
            let parsed = parse_message(&raw).unwrap();
            let info = &parsed.attachments[0];
            assert_eq!(info.charset.as_deref(), expected_charset);
            let content = attachment_content(&raw, info.index).unwrap();
            assert_eq!(
                content.strip_suffix(b"\r\n").unwrap_or(&content),
                &csv[..csv.len() - 2]
            );
        }
        // SP\xe3o in base64 and quoted-printable.
        for (encoding, body) in [("base64", "U+NvDQo="), ("quoted-printable", "S=E3o")] {
            let raw = multipart(
                "mixed",
                &[
                    &format!("{TEXT}Hi"),
                    &format!(
                        "Content-Type: text/csv; name=a.csv\r\nContent-Disposition: attachment; filename=a.csv\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n{body}"
                    ),
                ],
            );
            let parsed = parse_message(&raw).unwrap();
            let content = attachment_content(&raw, parsed.attachments[0].index).unwrap();
            assert!(content.starts_with(b"S\xe3o"), "{encoding}: {content:?}");
        }
    }

    fn with_part(body: &str, part_headers: &str) -> Vec<u8> {
        format!(
            "From: a@b.c\r\nContent-Type: multipart/mixed; boundary=X\r\n\r\n--X\r\n{body}\r\n--X\r\n{part_headers}\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAA\r\n--X--\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn a_content_id_is_matched_whole() {
        let html = "<img src=\"cid:part10\"><img src='CID:Pic%40x'><p>cid:spaced end";
        assert!(html_references_cid(html, "part10"));
        assert!(html_references_cid(html, "pic@x"));
        assert!(html_references_cid(html, "spaced"));
        assert!(!html_references_cid(html, "part1"));
        assert!(!html_references_cid(html, "pic"));
    }

    #[test]
    fn a_content_id_alone_does_not_hide_an_attachment() {
        let text = "Content-Type: text/plain\r\n\r\nHello";
        let html = "Content-Type: text/html\r\n\r\n<img src=\"CID:Pic%40x\">";
        let pdf = "Content-Type: application/pdf; name=r.pdf\r\nContent-Disposition: inline; filename=r.pdf\r\nContent-ID: <pic@x>";
        let image = "Content-Type: image/jpeg; name=f.jpg\r\nContent-Disposition: inline; filename=f.jpg\r\nContent-ID: <pic@x>";
        let inline = |raw: Vec<u8>| parse_message(&raw).unwrap().attachments[0].inline;
        assert!(!inline(with_part(text, pdf)));
        assert!(!inline(with_part(text, image)));
        assert!(!inline(with_part(html, pdf)));
        assert!(inline(with_part(html, image)));
    }

    const ALTERNATIVE: &str = "From: \"Ann Example\" <ann@example.com>\r\n\
To: bob@example.org, \"Doe, John\" <john@example.org>\r\n\
Cc: carol@example.net\r\n\
Reply-To: replies@example.com\r\n\
Subject: Lunch?\r\n\
Date: Tue, 14 Nov 2023 22:13:20 +0000\r\n\
Message-ID: <alt-1@example.com>\r\n\
In-Reply-To: <parent@example.com>\r\n\
References: <root@example.com>\r\n <parent@example.com>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Plain body\r\n\
--b1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>HTML body</p>\r\n\
--b1--\r\n";

    const MIXED_PDF: &str = "From: ann@example.com\r\n\
Subject: Report\r\n\
Date: Tue, 14 Nov 2023 19:13:20 -0300\r\n\
Content-Type: multipart/mixed; boundary=\"m\"\r\n\
\r\n\
--m\r\n\
Content-Type: text/plain\r\n\
\r\n\
See attached.\r\n\
--m\r\n\
Content-Type: application/pdf; name=\"report.pdf\"\r\n\
Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
JVBERi0xLjQgZmFrZQ==\r\n\
--m\r\n\
Content-Type: application/octet-stream\r\n\
Content-Disposition: attachment\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
AAEC\r\n\
--m--\r\n";

    const RELATED: &str = "From: ann@example.com\r\n\
Subject: Logo\r\n\
Content-Type: multipart/related; boundary=\"r\"\r\n\
\r\n\
--r\r\n\
Content-Type: text/html\r\n\
\r\n\
<img src=\"cid:logo@x\">\r\n\
--r\r\n\
Content-Type: image/png\r\n\
Content-ID: <logo@x>\r\n\
Content-Disposition: inline\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
iVBORw0K\r\n\
--r--\r\n";

    #[test]
    fn parses_alternative_headers_and_bodies() {
        let m = parse_message(ALTERNATIVE.as_bytes()).unwrap();
        assert_eq!(m.message_id.as_deref(), Some("alt-1@example.com"));
        assert_eq!(m.in_reply_to.as_deref(), Some("parent@example.com"));
        assert_eq!(m.references, vec!["root@example.com", "parent@example.com"]);
        assert_eq!(m.subject, "Lunch?");
        assert_eq!(
            m.from,
            Some(Address::new(Some("Ann Example"), "ann@example.com"))
        );
        assert_eq!(m.sender, None);
        assert_eq!(
            m.to,
            vec![
                Address::new(None::<String>, "bob@example.org"),
                Address::new(Some("Doe, John"), "john@example.org"),
            ]
        );
        assert_eq!(m.cc.len(), 1);
        assert_eq!(m.reply_to[0].email, "replies@example.com");
        assert_eq!(m.date, Some(1_700_000_000));
        assert_eq!(m.text.as_deref().map(str::trim), Some("Plain body"));
        assert_eq!(m.html.as_deref().map(str::trim), Some("<p>HTML body</p>"));
        assert!(m.attachments.is_empty());
        assert!(!has_attachments(&m));
    }

    #[test]
    fn date_honours_timezone() {
        let m = parse_message(MIXED_PDF.as_bytes()).unwrap();
        assert_eq!(m.date, Some(1_700_000_000));
    }

    #[test]
    fn lists_mixed_attachments_and_decodes_content() {
        let raw = MIXED_PDF.as_bytes();
        let m = parse_message(raw).unwrap();
        assert_eq!(m.text.as_deref().map(str::trim), Some("See attached."));
        assert_eq!(m.html, None);
        assert_eq!(m.attachments.len(), 2);
        let pdf = &m.attachments[0];
        assert_eq!(pdf.filename, "report.pdf");
        assert_eq!(pdf.mime_type, "application/pdf");
        assert_eq!(pdf.size, 13);
        assert!(!pdf.inline);
        assert_eq!(
            attachment_content(raw, pdf.index).unwrap(),
            b"%PDF-1.4 fake"
        );
        let unnamed = &m.attachments[1];
        assert_eq!(unnamed.filename, "attachment");
        assert_eq!(
            attachment_content(raw, unnamed.index).unwrap(),
            vec![0, 1, 2]
        );
        assert!(has_attachments(&m));
        assert_eq!(attachment_content(raw, 999), None);
        // The multipart container itself has no content.
        assert_eq!(attachment_content(raw, 0), None);
    }

    #[test]
    fn related_cid_image_is_inline() {
        let raw = RELATED.as_bytes();
        let m = parse_message(raw).unwrap();
        assert!(m.html.as_deref().unwrap().contains("cid:logo@x"));
        assert_eq!(m.text, None, "html-only mail must not synthesize text");
        assert_eq!(m.attachments.len(), 1);
        let logo = &m.attachments[0];
        assert!(logo.inline);
        assert_eq!(logo.content_id.as_deref(), Some("logo@x"));
        assert_eq!(logo.filename, "attachment.png");
        assert!(!has_attachments(&m));
        let images = inline_images(raw);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].0, "logo@x");
        assert_eq!(images[0].1, "image/png");
        assert_eq!(images[0].2, vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a]);
        assert!(inline_images(MIXED_PDF.as_bytes()).is_empty());
        assert!(inline_images(b"").is_empty());
    }

    #[test]
    fn quoted_printable_latin1_and_rfc2047() {
        let raw = b"From: =?ISO-8859-1?Q?Jos=E9_Silva?= <jose@example.com>\r\n\
To: =?UTF-8?B?Sm/Do28=?= <joao@example.com>\r\n\
Subject: =?UTF-8?Q?Reuni=C3=A3o_amanh=C3=A3?=\r\n\
Content-Type: text/plain; charset=iso-8859-1\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Ol=E1, tudo bem? Caf=E9 =\r\n\
amanh=E3.\r\n";
        let m = parse_message(raw).unwrap();
        assert_eq!(m.subject, "Reunião amanhã");
        assert_eq!(m.from.unwrap().name.as_deref(), Some("José Silva"));
        assert_eq!(m.to[0].name.as_deref(), Some("João"));
        assert_eq!(
            m.text.as_deref().map(str::trim),
            Some("Olá, tudo bem? Café amanhã.")
        );
        assert_eq!(m.html, None, "text-only mail must not synthesize html");
        assert_eq!(m.date, None, "missing Date stays None");
        assert_eq!(m.message_id, None);
        assert!(m.references.is_empty());
    }

    #[test]
    fn headers_only_input() {
        let raw = b"From: ann@example.com\r\nSubject: Just headers\r\nMessage-ID: <h@x>\r\n\r\n";
        let m = parse_message(raw).unwrap();
        assert_eq!(m.subject, "Just headers");
        assert_eq!(m.message_id.as_deref(), Some("h@x"));
        assert_eq!(m.text, None);
        assert_eq!(m.html, None);
        assert!(m.attachments.is_empty());
        let no_blank = parse_message(b"From: ann@example.com\r\nSubject: x\r\n").unwrap();
        assert_eq!(no_blank.subject, "x");
        assert_eq!(no_blank.text, None);
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(parse_message(b""), None);
    }
}
