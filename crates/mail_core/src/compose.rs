//! Building outgoing RFC 5322 messages from a [`Draft`].

use std::time::{SystemTime, UNIX_EPOCH};

use mail_builder::headers::address::Address as BuilderAddress;
use mail_builder::headers::date::Date;
use mail_builder::headers::message_id::MessageId;
use mail_builder::MessageBuilder;
use serde::{Deserialize, Serialize};

use crate::address::Address;

/// A file the user attached in the composer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingAttachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// The copy of `raw` kept in the sender's Sent folder: with a `Bcc:` header for the
/// envelope recipients missing from To and Cc. Only there: the SMTP server gets `raw`,
/// or the other recipients could read it.
pub fn sent_copy(raw: &[u8], envelope: &[String]) -> Vec<u8> {
    let Some(parsed) = crate::parse_message(raw) else {
        return raw.to_vec();
    };
    let visible = || parsed.to.iter().chain(&parsed.cc);
    let hidden: Vec<&str> = envelope
        .iter()
        .filter(|recipient| {
            !visible().any(|address| crate::same_email(&address.email, recipient))
                && !recipient.chars().any(char::is_control)
        })
        .map(String::as_str)
        .collect();
    if hidden.is_empty() {
        return raw.to_vec();
    }
    let mut copy = format!("Bcc: {}\r\n", hidden.join(",\r\n ")).into_bytes();
    copy.extend_from_slice(raw);
    copy
}

/// Everything needed to produce (and later re-open) an outgoing message.
/// Serializable so a queued/outbox draft can be persisted as JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    /// Message-ID being answered, without angle brackets.
    pub in_reply_to: Option<String>,
    /// Thread ancestry, oldest first, without angle brackets.
    pub references: Vec<String>,
    pub attachments: Vec<OutgoingAttachment>,
}

/// Serializes `draft` to raw bytes. Message-ID and Date are explicit inputs so
/// the output is deterministic (testable) and the caller controls identity.
/// Bcc is deliberately left out of the headers: it only travels in the SMTP
/// envelope (see [`envelope_recipients`]).
pub fn build_message(draft: &Draft, message_id: &str, date_unix: i64) -> Vec<u8> {
    let mut builder = MessageBuilder::new()
        .message_id(strip_brackets(message_id).to_string())
        .date(Date::new(date_unix))
        .from(to_builder(&draft.from))
        .subject(draft.subject.as_str());
    if !draft.to.is_empty() {
        builder = builder.to(to_builder_list(&draft.to));
    }
    if !draft.cc.is_empty() {
        builder = builder.cc(to_builder_list(&draft.cc));
    }
    // The builder folds between ids only, and a server may refuse lines over 998 bytes: replying
    // to a message with a hostile id loses the link to it, not the reply.
    let usable = |id: &String| !id.is_empty() && id.len() <= 900;
    if let Some(id) = draft.in_reply_to.as_deref().map(header_id).filter(usable) {
        builder = builder.in_reply_to(id);
    }
    let references: Vec<String> = draft
        .references
        .iter()
        .map(|r| header_id(r))
        .filter(usable)
        .collect();
    if !references.is_empty() {
        builder = builder.references(MessageId::from(references));
    }
    builder = builder.text_body(draft.text.as_str());
    if let Some(html) = &draft.html {
        builder = builder.html_body(html.as_str());
    }
    for attachment in &draft.attachments {
        builder = builder.attachment(
            attachment.mime_type.as_str(),
            attachment.filename.as_str(),
            attachment.data.as_slice(),
        );
    }
    // Writing into a Vec cannot fail.
    builder.write_to_vec().unwrap_or_default()
}

/// Message-ID (without brackets) as `<unix>.<unique>.bgmail@<domain>`. The
/// caller supplies `unique` (random) so this crate needs no RNG; the domain is
/// taken from the sender address so the ID matches the sending domain.
pub fn new_message_id(domain_from_email: &str, unique: u64) -> String {
    let domain = domain_from_email
        .rsplit_once('@')
        .map_or(domain_from_email, |(_, d)| d)
        .trim();
    let domain = if domain.is_empty() {
        "localhost"
    } else {
        domain
    };
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{unix}.{unique:x}.bgmail@{domain}")
}

/// SMTP `RCPT TO` list: to + cc + bcc, de-duplicated case-insensitively
/// (first spelling wins) so nobody receives two copies.
pub fn envelope_recipients(draft: &Draft) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .filter(|address| {
            !address.email.is_empty() && seen.insert(address.email.trim().to_lowercase())
        })
        .map(|address| address.email.clone())
        .collect()
}

/// Extension ↔ MIME pairs. The first entry for a MIME type is its canonical
/// extension (used when naming nameless attachments).
const MIME_TABLE: &[(&str, &str)] = &[
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("bmp", "image/bmp"),
    ("heic", "image/heic"),
    ("ico", "image/x-icon"),
    ("txt", "text/plain"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("csv", "text/csv"),
    ("ics", "text/calendar"),
    ("vcf", "text/vcard"),
    ("md", "text/markdown"),
    ("xml", "application/xml"),
    ("json", "application/json"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("7z", "application/x-7z-compressed"),
    ("rar", "application/vnd.rar"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("rtf", "application/rtf"),
    ("eml", "message/rfc822"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("mp4", "video/mp4"),
    ("mov", "video/quicktime"),
];

/// MIME type from a file name's extension (case-insensitive), falling back to
/// `application/octet-stream`.
pub fn guess_mime_type(filename: &str) -> &'static str {
    filename
        .rsplit_once('.')
        .and_then(|(_, ext)| {
            MIME_TABLE
                .iter()
                .find(|(e, _)| e.eq_ignore_ascii_case(ext))
                .map(|(_, mime)| *mime)
        })
        .unwrap_or("application/octet-stream")
}

/// Canonical extension (no dot) for a MIME type, used to name nameless parts.
pub(crate) fn extension_for_mime(mime: &str) -> Option<&'static str> {
    MIME_TABLE
        .iter()
        .find(|(_, m)| m.eq_ignore_ascii_case(mime))
        .map(|(ext, _)| *ext)
}

/// A message or content id as it can go in a header. The line breaks of folding go, as
/// does any other control character: a CR left in the id of a message being answered
/// could start a header of its sender's choosing. Spaces stay: they are part of it for
/// whoever searches the server or follows a `cid:`.
pub(crate) fn header_id(id: &str) -> String {
    strip_brackets(id)
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

pub(crate) fn strip_brackets(id: &str) -> &str {
    let id = id.trim();
    id.strip_prefix('<')
        .and_then(|i| i.strip_suffix('>'))
        .unwrap_or(id)
        .trim()
}

fn to_builder(address: &Address) -> BuilderAddress<'_> {
    BuilderAddress::new_address(address.name.as_deref(), address.email.as_str())
}

fn to_builder_list(addresses: &[Address]) -> BuilderAddress<'_> {
    BuilderAddress::new_list(addresses.iter().map(to_builder).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_id_cannot_add_a_header_to_a_reply() {
        let draft = Draft {
            from: Address::new(None::<String>, "a@x.test"),
            to: vec![Address::new(None::<String>, "b@x.test")],
            in_reply_to: Some("a@b\rX-Evil: 1".into()),
            references: vec!["r@b\rX-Evil: 2".into()],
            ..Default::default()
        };
        let raw = build_message(&draft, "id@x.test", 0);
        assert!(
            !raw.windows(2).any(|w| w == b"\rX"),
            "{}",
            String::from_utf8_lossy(&raw)
        );
    }

    #[test]
    fn the_sent_copy_records_who_was_hidden() {
        let raw = b"From: a@x.test\r\nTo: B <b@x.test>\r\nCc: c@x.test\r\nSubject: s\r\n\r\nbody";
        let envelope = |list: &[&str]| list.iter().map(|r| r.to_string()).collect::<Vec<_>>();
        assert_eq!(sent_copy(raw, &envelope(&["b@x.test", "C@x.test"])), raw);

        let copy = sent_copy(raw, &envelope(&["b@x.test", "d@x.test", "e@x.test"]));
        let parsed = crate::parse_message(&copy).unwrap();
        let hidden: Vec<_> = parsed.bcc.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(hidden, ["d@x.test", "e@x.test"]);
        assert_eq!(parsed.subject, "s");
        assert_eq!(parsed.to[0].email, "b@x.test");

        // A value that could add headers isn't written.
        let copy = sent_copy(raw, &envelope(&["x@x.test\r\nSubject: y"]));
        assert_eq!(copy, raw);
    }
    use crate::parse::{attachment_content, parse_message};

    fn draft() -> Draft {
        Draft {
            from: Address::new(Some("Me Myself"), "me@example.com"),
            to: vec![Address::new(Some("Doe, John"), "john@example.org")],
            cc: vec![Address::new(None::<String>, "cc@example.org")],
            bcc: vec![Address::new(None::<String>, "secret@example.org")],
            subject: "Olá — çà va?".into(),
            text: "Hello there\nSecond line".into(),
            ..Default::default()
        }
    }

    #[test]
    fn builds_plain_text_message() {
        let raw = build_message(&draft(), "<abc@example.com>", 1_700_000_000);
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.to_ascii_lowercase().contains("bcc"), "{text}");
        assert!(!text.contains("secret@example.org"));
        assert!(!text.contains("multipart"));
        let parsed = parse_message(&raw).unwrap();
        assert_eq!(parsed.message_id.as_deref(), Some("abc@example.com"));
        assert_eq!(parsed.subject, "Olá — çà va?");
        assert_eq!(parsed.date, Some(1_700_000_000));
        assert_eq!(parsed.from, Some(draft().from));
        assert_eq!(parsed.to, draft().to);
        assert_eq!(parsed.cc, draft().cc);
        assert!(parsed.bcc.is_empty());
        assert_eq!(
            parsed
                .text
                .as_deref()
                .map(|t| t.replace("\r\n", "\n").trim_end().to_string()),
            Some("Hello there\nSecond line".to_string())
        );
        assert_eq!(parsed.html, None);
    }

    #[test]
    fn builds_alternative_and_mixed_with_threading() {
        let mut d = draft();
        d.html = Some("<p>Hello <b>there</b></p>".into());
        d.in_reply_to = Some("<parent@x>".into());
        d.references = vec!["root@x".into(), "<parent@x>".into()];
        d.attachments.push(OutgoingAttachment {
            filename: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            data: b"%PDF-1.4 fake".to_vec(),
        });
        let raw = build_message(&d, "id@x", 0);
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("multipart/mixed"));
        assert!(text.contains("multipart/alternative"));
        assert!(text.contains("In-Reply-To: <parent@x>"));
        assert!(text.contains("References: <root@x> <parent@x>"));
        let parsed = parse_message(&raw).unwrap();
        assert_eq!(parsed.in_reply_to.as_deref(), Some("parent@x"));
        assert_eq!(parsed.references, vec!["root@x", "parent@x"]);
        assert!(parsed.html.unwrap().contains("<b>there</b>"));
        assert!(parsed.text.unwrap().contains("Hello there"));
        assert_eq!(parsed.attachments.len(), 1);
        let att = &parsed.attachments[0];
        assert_eq!(att.filename, "report.pdf");
        assert_eq!(att.mime_type, "application/pdf");
        assert!(!att.inline);
        assert_eq!(
            attachment_content(&raw, att.index).unwrap(),
            b"%PDF-1.4 fake".to_vec()
        );
    }

    #[test]
    fn text_plus_attachment_is_mixed_without_alternative() {
        let mut d = draft();
        d.attachments.push(OutgoingAttachment {
            filename: "a.txt".into(),
            mime_type: "text/plain".into(),
            data: b"note".to_vec(),
        });
        let raw = build_message(&d, "id@x", 0);
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("multipart/mixed"));
        assert!(!text.contains("multipart/alternative"));
    }

    #[test]
    fn message_id_format() {
        let id = new_message_id("me@Example.com", 255);
        let (left, domain) = id.split_once('@').unwrap();
        assert_eq!(domain, "Example.com");
        let parts: Vec<&str> = left.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].parse::<u64>().unwrap() > 1_600_000_000);
        assert_eq!(parts[1], "ff");
        assert_eq!(parts[2], "bgmail");
        assert!(new_message_id("example.net", 1).ends_with("@example.net"));
        assert!(new_message_id("", 1).ends_with("@localhost"));
        assert_ne!(new_message_id("a@b", 1), new_message_id("a@b", 2));
    }

    #[test]
    fn envelope_dedupes_case_insensitively() {
        let mut d = draft();
        d.cc.push(Address::new(None::<String>, "JOHN@example.org"));
        d.bcc.push(Address::new(None::<String>, "cc@EXAMPLE.org"));
        assert_eq!(
            envelope_recipients(&d),
            vec!["john@example.org", "cc@example.org", "secret@example.org"]
        );
    }

    #[test]
    fn guesses_mime_types() {
        assert_eq!(guess_mime_type("Report.PDF"), "application/pdf");
        assert_eq!(guess_mime_type("photo.jpeg"), "image/jpeg");
        assert_eq!(
            guess_mime_type("x.docx"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(guess_mime_type("noext"), "application/octet-stream");
        assert_eq!(guess_mime_type("weird.zzz"), "application/octet-stream");
    }

    #[test]
    fn extension_for_mime_uses_canonical_entry() {
        assert_eq!(extension_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for_mime("APPLICATION/PDF"), Some("pdf"));
        assert_eq!(extension_for_mime("message/rfc822"), Some("eml"));
        assert_eq!(extension_for_mime("application/x-unknown"), None);
    }

    #[test]
    fn strip_brackets_trims() {
        assert_eq!(strip_brackets(" <a@b> "), "a@b");
        assert_eq!(strip_brackets("a@b"), "a@b");
        assert_eq!(strip_brackets("<>"), "");
    }
}
