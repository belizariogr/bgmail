//! Turning raw RFC 5322 bytes into the fields BGMail stores and shows.
//!
//! `mail-parser` does the heavy lifting (MIME, charsets, RFC 2047); this
//! module pins down *which* of its views we trust so storage and UI agree.

use std::collections::HashSet;

use mail_parser::{Message, MessageParser, MessagePart, MimeHeaders, PartType};
use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::compose::extension_for_mime;

/// Largest body a message can have, attachments apart (user decision). Larger ones are
/// rejected, received or sent.
pub const MAX_BODY_BYTES: usize = 256 * 1024 * 1024;

/// The parts of a message the app needs, decoupled from `mail-parser`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedMessage {
    /// Bytes of the body parts, each counted once: what [`MAX_BODY_BYTES`] limits.
    pub body_size: usize,
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

/// A parsed message that can be dropped at any nesting. Each `message/rfc822` part owns
/// the next by value, and the generated `Drop` recursed once per level: a few MB of
/// nested messages overflowed the stack of the sync thread on every start, before the
/// user opened anything.
struct Parsed<'x>(Message<'x>);

impl<'x> std::ops::Deref for Parsed<'x> {
    type Target = Message<'x>;

    fn deref(&self) -> &Message<'x> {
        &self.0
    }
}

impl Drop for Parsed<'_> {
    fn drop(&mut self) {
        let mut parts = std::mem::take(&mut self.0.parts);
        while let Some(mut part) = parts.pop() {
            if let PartType::Message(nested) = &mut part.body {
                parts.append(&mut nested.parts);
            }
        }
    }
}

fn parse_raw(raw: &[u8]) -> Option<Parsed<'_>> {
    MessageParser::default().parse(raw).map(Parsed)
}

/// Parses a full message or just its header block (bodies are then `None`).
/// Returns `None` only when no headers can be found at all.
pub fn parse_message(raw: &[u8]) -> Option<ParsedMessage> {
    let message = parse_raw(raw)?;
    if message.headers().is_empty() {
        return None;
    }
    let html = html_body(&message);
    let cids = referenced_cids(html.as_deref().unwrap_or_default());
    Some(ParsedMessage {
        body_size: body_size(&message),
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
            .map(|(index, part)| describe(raw, index, part, &cids))
            .collect(),
        html,
        text: text_body(&message),
    })
}

/// [`parse_message`] for a fetched header block, cut at [`MAX_HEADER_BLOCK_BYTES`]. It arrives on
/// every sync whatever the message's size, and the parser is quadratic in some headers: 31 MB of
/// `name*0=x; name*1=x` took 45 s.
pub fn parse_header_block(raw: &[u8]) -> Option<ParsedMessage> {
    if raw.len() <= MAX_HEADER_BLOCK_BYTES {
        return parse_message(raw);
    }
    let head = &raw[..MAX_HEADER_BLOCK_BYTES];
    let end = head
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(head.len(), |at| at + 1);
    parse_message(&head[..end])
}

/// Far past the 998 bytes per line and the few dozen lines real headers have.
const MAX_HEADER_BLOCK_BYTES: usize = 1 << 20;

/// Decoded bytes of the part listed as `AttachmentInfo::index`.
pub fn attachment_content(raw: &[u8], index: usize) -> Option<Vec<u8>> {
    let message = parse_raw(raw)?;
    let part = message.parts.get(index)?;
    part_bytes(raw, part)
}

/// Decoded bytes of the parts at `indexes`, parsing the message once: a forward can carry
/// thousands of them.
pub fn attachment_contents(raw: &[u8], indexes: &[usize]) -> Vec<Option<Vec<u8>>> {
    let Some(message) = parse_raw(raw) else {
        return vec![None; indexes.len()];
    };
    indexes
        .iter()
        .map(|&index| part_bytes(raw, message.parts.get(index)?))
        .collect()
}

/// What only renders a body again. Listed so that anything unknown stays reachable as
/// an attachment rather than vanishing.
const OTHER_RENDERINGS: &[&str] = &[
    "text/x-amp-html",
    "text/watch-html",
    "text/enriched",
    "text/richtext",
];

/// Parts of a `multipart/alternative` that are only another rendering of a body it
/// also has: the same message, not attachments. Anything with a name is meant to be
/// seen as one.
fn other_body_versions(message: &Message<'_>) -> Vec<u32> {
    let bodies = body_ids(message);
    let is_body = |id: &u32| bodies.contains(id);
    let mut versions = Vec::new();
    for part in &message.parts {
        let PartType::Multipart(children) = &part.body else {
            continue;
        };
        let alternative = part.content_type().is_some_and(|ct| {
            ct.subtype()
                .is_some_and(|sub| sub.eq_ignore_ascii_case("alternative"))
        });
        if !alternative || !children.iter().any(is_body) {
            continue;
        }
        versions.extend(children.iter().copied().filter(|id| {
            !is_body(id)
                && message.parts.get(*id as usize).is_some_and(|child| {
                    OTHER_RENDERINGS.contains(&mime_type(child).as_str())
                        && child.attachment_name().is_none()
                        && !child
                            .content_disposition()
                            .is_some_and(|d| d.ctype().eq_ignore_ascii_case("attachment"))
                })
        }));
    }
    versions
}

/// `(content_id, mime_type, bytes)` for every image with a Content-ID, i.e.
/// everything an HTML body can reference via `cid:`.
pub fn inline_images(raw: &[u8]) -> Vec<(String, String, Vec<u8>)> {
    let Some(message) = parse_raw(raw) else {
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
    let other_versions: HashSet<u32> = other_body_versions(message).into_iter().collect();
    let attachments: HashSet<u32> = message.attachments.iter().copied().collect();
    let bodies = body_ids(message);
    message
        .parts
        .iter()
        .enumerate()
        .filter(move |(index, part)| {
            let id = *index as u32;
            if other_versions.contains(&id) {
                return false;
            }
            match part.body {
                PartType::Multipart(_) => false,
                PartType::Binary(_) | PartType::InlineBinary(_) | PartType::Message(_) => {
                    attachments.contains(&id) || part.content_id().is_some()
                }
                PartType::Text(_) | PartType::Html(_) => {
                    attachments.contains(&id) && !bodies.contains(&id)
                }
            }
        })
}

fn describe(
    raw: &[u8],
    index: usize,
    part: &MessagePart<'_>,
    cids: &HashSet<String>,
) -> AttachmentInfo {
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
            .is_some_and(|cid| cids.contains(&cid.to_ascii_lowercase()));
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
    referenced_cids(html).contains(&cid.to_ascii_lowercase())
}

/// Every Content-ID the HTML refers to, lowercase. Checking each part against the set
/// instead of scanning the body per part keeps a message with many parts linear.
pub fn referenced_cids(html: &str) -> HashSet<String> {
    // ASCII lowercasing keeps byte offsets identical to `html`.
    let lower = html.to_ascii_lowercase();
    let mut cids = HashSet::new();
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("cid:") {
        let start = pos + found + "cid:".len();
        match cid_value_len(&html[start..], |c| matches!(c, '<' | '&' | ';')) {
            Some(len) => {
                cids.insert(percent_decoded(&html[start..start + len]).to_ascii_lowercase());
                pos = start + len;
            }
            None => pos = start + cid_skip_len(&html[start..]),
        }
    }
    cids
}

/// How much of `rest` to skip when [`cid_value_len`] finds no value: all it looked at, or
/// a body of nothing but `cid:` would cost that much per marker.
pub fn cid_skip_len(rest: &str) -> usize {
    let mut len = MAX_CID_BYTES.min(rest.len());
    while !rest.is_char_boundary(len) {
        len -= 1;
    }
    len
}

/// Longest `cid:` value looked for: what a header line can hold.
const MAX_CID_BYTES: usize = 998;

/// Length of the `cid:` value at the start of `rest`, ending at whitespace, a quote, `)`, `>`
/// or what `also_ends` takes. `None` if it doesn't end within what a header can hold: copying
/// up to the end of the body for each `cid:` took gigabytes for 400 KB of them.
pub fn cid_value_len(rest: &str, also_ends: impl Fn(char) -> bool) -> Option<usize> {
    rest.char_indices()
        .take_while(|(at, _)| *at <= MAX_CID_BYTES)
        .find(|(_, c)| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '>') || also_ends(*c))
        .map(|(at, _)| at)
        .or((rest.len() <= MAX_CID_BYTES).then_some(rest.len()))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| text.get(i + 1..i + 3))
            .flatten()
            .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
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

/// Letters, digits and the punctuation of RFC 6838: the type goes back into a header
/// when forwarding, where anything else could add to it.
fn is_mime_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b))
}

fn mime_type(part: &MessagePart<'_>) -> String {
    let declared = part.content_type().and_then(|ct| {
        let subtype = ct.subtype()?;
        (is_mime_token(ct.ctype()) && is_mime_token(subtype))
            .then(|| format!("{}/{}", ct.ctype(), subtype).to_ascii_lowercase())
    });
    match declared {
        Some(mime) => mime,
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

/// Bytes of the text and HTML body parts; an attachment doesn't count.
/// The parts that make up the body, for lookups in a message of many parts.
fn body_ids(message: &Message<'_>) -> HashSet<u32> {
    (message.text_body.iter().chain(&message.html_body))
        .copied()
        .collect()
}

fn body_size(message: &Message<'_>) -> usize {
    let bodies = body_ids(message);
    message
        .parts
        .iter()
        .enumerate()
        .filter(|(index, _)| bodies.contains(&(*index as u32)))
        .map(|(_, part)| match &part.body {
            PartType::Text(text) | PartType::Html(text) => text.len(),
            _ => 0,
        })
        .sum()
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
    crate::compose::header_id(id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_body_size_leaves_attachments_out_and_counts_each_part_once() {
        let raw = b"From: a@b.c\r\nContent-Type: multipart/mixed; boundary=X\r\n\r\n--X\r\nContent-Type: text/plain\r\n\r\n12345\r\n--X\r\nContent-Type: application/pdf; name=r.pdf\r\nContent-Disposition: attachment; filename=r.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAAAAAAAAAAAAAA\r\n--X--\r\n";
        let parsed = super::parse_message(raw).unwrap();
        assert!((5..=7).contains(&parsed.body_size), "{}", parsed.body_size);
    }

    use super::*;

    fn attached(part: &str) -> Vec<u8> {
        multipart("mixed", &[&format!("{TEXT}See attached"), part])
    }

    #[test]
    fn a_charset_that_could_add_headers_is_dropped() {
        for charset in [
            "charset*=utf-8''x%0d%0aContent-Disposition:%20inline%0d%0a%0d%0aPAY",
            "charset==?utf-8?Q?x=0D=0AX-Evil:_1?=",
            "charset=\"x; foo=bar\"",
            "charset=\"x y\"",
        ] {
            let raw = attached(&format!(
                "Content-Type: text/plain; {charset}; name=a.csv\r\nContent-Disposition: attachment; filename=a.csv\r\n\r\nreal"
            ));
            let parsed = parse_message(&raw).unwrap();
            assert_eq!(parsed.attachments[0].charset, None, "{charset}");
        }
        let raw = attached(
            "Content-Type: text/plain; charset=ISO-8859-1; name=a.csv\r\nContent-Disposition: attachment; filename=a.csv\r\n\r\nreal",
        );
        assert_eq!(
            parse_message(&raw).unwrap().attachments[0]
                .charset
                .as_deref(),
            Some("ISO-8859-1")
        );
    }

    #[test]
    fn an_attached_message_is_that_message_not_the_one_around_it() {
        let inner = "From: inner@b.c\r\nSubject: Secret\r\n\r\nInner body";
        let raw = attached(&format!(
            "Content-Type: message/rfc822; name=note.eml\r\nContent-Disposition: attachment; filename=note.eml\r\n\r\n{inner}"
        ));
        let info = &parse_message(&raw).unwrap().attachments[0];
        let content = attachment_content(&raw, info.index).unwrap();
        assert_eq!(std::str::from_utf8(&content).unwrap().trim_end(), inner);
        assert!(info.size < raw.len());

        let digest = multipart(
            "digest",
            &[
                "Content-Type: message/rfc822\r\n\r\nFrom: a@b.c\r\n\r\nBody one",
                "Content-Type: message/rfc822\r\n\r\nFrom: a@b.c\r\n\r\nBody two",
            ],
        );
        let parsed = parse_message(&digest).unwrap();
        let bodies: Vec<_> = parsed
            .attachments
            .iter()
            .map(|info| attachment_content(&digest, info.index).unwrap())
            .collect();
        assert!(bodies[0].ends_with(b"Body one") || bodies[0].ends_with(b"Body one\r\n"));
        assert!(bodies[1].ends_with(b"Body two") || bodies[1].ends_with(b"Body two\r\n"));
    }

    #[test]
    fn other_versions_of_the_body_are_not_attachments() {
        let raw = multipart(
            "alternative",
            &[
                &format!("{TEXT}plain"),
                "Content-Type: text/x-amp-html\r\n\r\n<html amp></html>",
                "Content-Type: text/watch-html\r\n\r\n<b>w</b>",
                "Content-Type: text/calendar; name=invite.ics\r\n\r\nBEGIN:VCALENDAR",
                &format!("{HTML}<p>h</p>"),
            ],
        );
        let parsed = parse_message(&raw).unwrap();
        let listed: Vec<_> = parsed
            .attachments
            .iter()
            .map(|a| a.mime_type.as_str())
            .collect();
        assert_eq!(listed, ["text/calendar"]);

        // With no other version it is all there is to read.
        let raw = multipart(
            "alternative",
            &["Content-Type: text/x-amp-html\r\n\r\n<html amp></html>"],
        );
        assert_eq!(parse_message(&raw).unwrap().attachments.len(), 1);
    }

    #[test]
    fn ids_lose_what_a_header_cannot_carry() {
        let raw = b"From: a@b.c\r\nMessage-ID: <a@b\rX-Evil: 1>\r\n\r\nx";
        assert_eq!(
            parse_message(raw).unwrap().message_id.as_deref(),
            Some("a@bX-Evil: 1")
        );
    }

    #[test]
    fn ids_keep_their_spaces() {
        let raw = b"From: a@b.c\r\nMessage-ID: <abcdef\r\n gh@example.com>\r\n\r\nx";
        assert_eq!(
            parse_message(raw).unwrap().message_id.as_deref(),
            Some("abcdef gh@example.com")
        );
        let raw = multipart(
            "related",
            &[
                &format!("{HTML}<img src=\"cid:my%20logo.png\">"),
                "Content-Type: image/png\r\nContent-ID: <my logo.png>\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID",
            ],
        );
        let info = &parse_message(&raw).unwrap().attachments[0];
        assert_eq!(info.content_id.as_deref(), Some("my logo.png"));
        assert!(info.inline);
    }

    #[test]
    fn only_known_renderings_leave_the_attachments() {
        for mime in ["text/vcard", "text/csv", "text/x-vcalendar"] {
            let raw = multipart(
                "alternative",
                &[
                    &format!("{TEXT}plain"),
                    &format!("Content-Type: {mime}\r\n\r\nx"),
                    &format!("{HTML}<p>h</p>"),
                ],
            );
            assert_eq!(parse_message(&raw).unwrap().attachments.len(), 1, "{mime}");
        }
    }

    #[test]
    fn a_mime_type_is_only_kept_as_a_plain_token() {
        let raw = attached(
            "Content-Type: application/pdf\rX-Evil: 1\r\nContent-Disposition: attachment; filename=a.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAA",
        );
        let info = &parse_message(&raw).unwrap().attachments[0];
        assert!(!info.mime_type.contains('\r'), "{:?}", info.mime_type);
        assert_eq!(info.mime_type, "application/octet-stream");

        let raw = attached(
            "Content-Type: application/vnd.ms-excel\r\nContent-Disposition: attachment; filename=a.xls\r\nContent-Transfer-Encoding: base64\r\n\r\nAAAA",
        );
        assert_eq!(
            parse_message(&raw).unwrap().attachments[0].mime_type,
            "application/vnd.ms-excel"
        );
    }

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

    #[test]
    fn messages_nested_past_the_stack_are_dropped_without_recursing() {
        let mut raw = String::new();
        for _ in 0..100_000 {
            raw.push_str("Content-Type: message/rfc822\r\n\r\n");
        }
        raw.push_str("Subject: x\r\n\r\nbody");
        // Test threads have 2 MiB, as the sync threads do.
        assert!(parse_message(raw.as_bytes()).is_some());
        let _ = inline_images(raw.as_bytes());
        let _ = attachment_content(raw.as_bytes(), 0);
    }

    #[test]
    fn a_message_of_many_parts_and_references_stays_linear() {
        let mut raw = String::from(
            "Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\
            Content-Type: text/html\r\n\r\n",
        );
        raw.push_str(&"<img src=cid:x>".repeat(100_000));
        for i in 0..20_000 {
            raw.push_str(&format!(
                "\r\n--B\r\nContent-Type: image/png\r\nContent-ID: <c{i}>\r\n\r\nA"
            ));
        }
        raw.push_str("\r\n--B--\r\n");
        let start = std::time::Instant::now();
        assert_eq!(
            parse_message(raw.as_bytes()).unwrap().attachments.len(),
            20_000
        );
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn parameter_continuations_stay_linear_however_they_are_wrapped() {
        let plain = |parameter: &str| {
            let mut parameters = String::new();
            for i in 0..100_000 {
                parameters.push_str(&parameter.replace("{i}", &i.to_string()));
            }
            parameters
        };
        for parameter in ["a{i}*1=x; ", "a*{i}=x; ", "a{i}*=u'en'x; "] {
            let parameters = plain(parameter);
            let encoded = |text: &str| {
                let mut out = String::new();
                for chunk in text.as_bytes().chunks(57) {
                    for bytes in chunk.chunks(3) {
                        const ALPHABET: &[u8] =
                            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
                        let word = bytes.iter().fold(0u32, |w, b| w << 8 | *b as u32)
                            << (8 * (3 - bytes.len()));
                        for at in 0..4 {
                            out.push(if at <= bytes.len() {
                                ALPHABET[(word >> (18 - 6 * at) & 63) as usize] as char
                            } else {
                                '='
                            });
                        }
                    }
                    out.push_str("\r\n");
                }
                out
            };
            let header = format!("Content-Type: text/plain; {parameters}\r\n\r\nbody");
            for raw in [
                format!("Subject: kept\r\n{header}"),
                format!(" {header}"),
                format!(
                    "Subject: kept\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\n{header}\r\n--b--\r\n"
                ),
                format!(
                    "Subject: kept\r\nContent-Type: message/rfc822\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
                    encoded(&header)
                ),
            ] {
                let start = std::time::Instant::now();
                let _ = parse_message(raw.as_bytes());
                assert!(start.elapsed().as_secs() < 5, "{parameter}: {:?}", start.elapsed());
            }
        }
    }

    #[test]
    fn parts_declaring_boundaries_they_never_use_stay_linear() {
        let mut raw =
            String::from("Subject: kept\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n");
        for _ in 0..80_000 {
            raw.push_str("--B\r\nContent-Type:multipart/x;boundary=z\r\n\r\n");
        }
        raw.push_str("--B--\r\n");
        let start = std::time::Instant::now();
        assert_eq!(parse_message(raw.as_bytes()).unwrap().subject, "kept");
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn a_boundary_past_its_limit_makes_a_part_of_text() {
        let boundary = "-".repeat(1 << 20) + "x";
        let raw = format!(
            "Subject: kept\r\nContent-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n{}",
            "-".repeat(1 << 20)
        );
        let start = std::time::Instant::now();
        let parsed = parse_message(raw.as_bytes()).unwrap();
        assert_eq!(parsed.subject, "kept");
        assert!(parsed.text.is_some() && parsed.attachments.is_empty());
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn a_multipart_without_its_boundary_reads_as_its_text() {
        let raw = "Content-Type: multipart/mixed; boundary=none\r\n\r\nWhat it says\r\n";
        let parsed = parse_message(raw.as_bytes()).unwrap();
        assert!(parsed.text.unwrap().contains("What it says"));
        assert!(parsed.attachments.is_empty());
    }

    #[test]
    fn a_broken_multipart_doesnt_stop_the_next_ones() {
        let raw = "Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\
            Content-Type: multipart/x; boundary=none\r\n\r\nA\r\n--B\r\n\
            Content-Type: text/plain\r\n\r\nSeen\r\n--B--\r\n";
        assert!(parse_message(raw.as_bytes())
            .unwrap()
            .text
            .unwrap()
            .contains("Seen"));
    }

    #[test]
    fn continuations_are_still_joined_in_order() {
        let raw = b"Content-Type: text/plain; name*1=b.txt; name*0=a\r\n\r\nbody";
        let message = parse_raw(raw).unwrap();
        let content_type = message.parts[0].content_type().unwrap();
        assert_eq!(content_type.attribute("name"), Some("ab.txt"));
    }

    #[test]
    fn a_body_full_of_multiplications_is_left_alone() {
        let mut raw = String::from("Content-Type: text/plain\r\n\r\n");
        for i in 0..6000 {
            raw.push_str(&format!("{i}*2={}\r\n", i * 2));
        }
        assert!(parse_message(raw.as_bytes())
            .unwrap()
            .text
            .unwrap()
            .contains("7*2=14"));
    }

    #[test]
    fn a_huge_header_block_is_cut_at_a_line() {
        let mut raw = String::from("Subject: kept\r\nContent-Type: text/plain; ");
        for i in 0..2_000_000 {
            raw.push_str(&format!("name*{i}=x;\r\n "));
        }
        let start = std::time::Instant::now();
        assert_eq!(parse_header_block(raw.as_bytes()).unwrap().subject, "kept");
        assert!(start.elapsed().as_secs() < 10, "{:?}", start.elapsed());
    }

    #[test]
    fn contents_of_many_parts_match_each_on_its_own() {
        let raw = b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\
            Content-Type: text/plain\r\n\r\nhello\r\n--B\r\n\
            Content-Type: application/x-a\r\n\r\nAA\r\n--B--\r\n";
        let contents = attachment_contents(raw, &[2, 99]);
        assert_eq!(contents[0], attachment_content(raw, 2));
        assert!(contents[0].is_some());
        assert_eq!(contents[1], None);
    }

    #[test]
    fn referenced_cids_are_lowercase_and_decoded() {
        let cids = referenced_cids("<img src='CID:A%40b'> <img src=cid:c> <img src=cid:%41bc>");
        assert!(cids.contains("a@b") && cids.contains("c") && cids.contains("abc"));
        assert_eq!(cids.len(), 3);
    }

    #[test]
    fn repeated_cid_markers_dont_copy_the_rest_of_the_body() {
        let html = "cid:".repeat(100_000);
        let start = std::time::Instant::now();
        // Only the tail fits a value.
        assert!(referenced_cids(&html).len() <= 1);
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        assert_eq!(cid_value_len("abc", |_| false), Some(3));
        assert_eq!(cid_value_len("abc>d", |_| false), Some(3));
        assert_eq!(cid_value_len(&"é".repeat(1000), |_| false), None);
    }
}
