//! Reply / reply-all / forward drafts derived from a received message.
//!
//! Localized strings (attribution line, field labels) come in pre-formatted
//! via [`QuoteLabels`] so this crate stays free of i18n and date formatting.

use crate::address::{same_email, Address};
use crate::compose::Draft;
use crate::parse::ParsedMessage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReplyKind {
    Reply,
    ReplyAll,
    Forward,
}

/// Localized text the UI formats before calling [`prepare_reply`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuoteLabels {
    /// Line above the quote, e.g. "On Mon, 3 Jun, Ann wrote:".
    pub attribution: String,
    /// Separator above a forwarded block, e.g. "---------- Forwarded message ----------".
    pub forward_header: String,
    /// Field labels used in the forwarded block ("From", "Date", …).
    pub from: String,
    pub date: String,
    pub subject: String,
    pub to: String,
    /// The original message's date, already formatted for the user.
    pub formatted_date: String,
}

/// Prefixes (lowercase, with colon) that already mark a reply: English,
/// Portuguese, German and Scandinavian clients.
pub(crate) const REPLY_PREFIXES: &[&str] = &["re:", "res:", "aw:", "sv:"];
/// Prefixes that already mark a forward (English, Portuguese, German).
pub(crate) const FORWARD_PREFIXES: &[&str] = &["fwd:", "fw:", "enc:", "wg:"];

/// "Re: " + subject, unless the subject already carries a reply prefix.
pub fn reply_subject(subject: &str) -> String {
    prefixed_subject(subject, "Re: ", REPLY_PREFIXES)
}

/// "Fwd: " + subject, unless the subject already carries a forward prefix.
pub fn forward_subject(subject: &str) -> String {
    prefixed_subject(subject, "Fwd: ", FORWARD_PREFIXES)
}

fn prefixed_subject(subject: &str, prefix: &str, known: &[&str]) -> String {
    let subject = subject.trim();
    let lower = subject.to_lowercase();
    if known.iter().any(|p| lower.starts_with(p)) {
        subject.to_string()
    } else {
        format!("{prefix}{subject}")
    }
}

/// Classic `> ` quoting; blank lines become a bare `>` (no trailing space).
pub fn quote_text(text: &str) -> String {
    text.trim_end()
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds the draft for a reply, reply-all or forward of `original`.
///
/// `me` lists every address of the current account (aliases included) so
/// reply-all never copies the user; `from` defaults to `me[0]`. Forwarded
/// attachments are left to the caller (they need the raw message).
pub fn prepare_reply(
    original: &ParsedMessage,
    kind: ReplyKind,
    me: &[Address],
    labels: &QuoteLabels,
) -> Draft {
    let body_source = original
        .text
        .clone()
        .or_else(|| original.html.as_deref().map(html_to_text))
        .unwrap_or_default();
    let mut draft = Draft {
        from: me.first().cloned().unwrap_or_default(),
        ..Default::default()
    };
    let is_me = |a: &Address| me.iter().any(|m| same_email(&m.email, &a.email));

    match kind {
        ReplyKind::Forward => {
            draft.subject = forward_subject(&original.subject);
            let mut block = vec![labels.forward_header.clone()];
            if let Some(from) = &original.from {
                block.push(format!("{}: {}", labels.from, from.display()));
            }
            if !labels.formatted_date.is_empty() {
                block.push(format!("{}: {}", labels.date, labels.formatted_date));
            }
            block.push(format!("{}: {}", labels.subject, original.subject));
            if !original.to.is_empty() {
                block.push(format!(
                    "{}: {}",
                    labels.to,
                    crate::address::format_address_list(&original.to)
                ));
            }
            draft.text = format!("\n\n{}\n\n{}", block.join("\n"), body_source.trim_end());
        }
        ReplyKind::Reply | ReplyKind::ReplyAll => {
            draft.subject = reply_subject(&original.subject);
            let mut primary: Vec<Address> = if original.reply_to.is_empty() {
                original.from.iter().cloned().collect()
            } else {
                original.reply_to.clone()
            };
            // Replying to my own sent message continues the conversation with
            // its recipients instead of mailing myself.
            if !primary.is_empty() && primary.iter().all(is_me) {
                primary = original.to.clone();
            }
            draft.to = unique(primary.into_iter().filter(|a| !is_me(a)), &[]);
            if kind == ReplyKind::ReplyAll {
                let rest = original.to.iter().chain(&original.cc).cloned();
                draft.cc = unique(rest.filter(|a| !is_me(a)), &draft.to);
            }
            draft.in_reply_to = original.message_id.clone();
            draft.references = original.references.clone();
            if let Some(id) = &original.message_id {
                if !draft.references.contains(id) {
                    draft.references.push(id.clone());
                }
            }
            draft.text = format!("\n\n{}\n{}", labels.attribution, quote_text(&body_source));
        }
    }
    draft
}

/// Keeps the first occurrence of each email, skipping those in `exclude`.
fn unique(addresses: impl Iterator<Item = Address>, exclude: &[Address]) -> Vec<Address> {
    let mut out: Vec<Address> = Vec::new();
    for address in addresses {
        let seen = out
            .iter()
            .chain(exclude)
            .any(|a| same_email(&a.email, &address.email));
        if !seen {
            out.push(address);
        }
    }
    out
}

/// Crude HTML → plain text for quoting HTML-only mail: drops tags, scripts and
/// styles, decodes common entities, collapses whitespace and keeps line breaks
/// for block elements. Not a renderer; good enough for a quote.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(after) = rest.strip_prefix("<!--") {
                rest = after.find("-->").map_or("", |end| &after[end + 3..]);
                continue;
            }
            let Some(end) = rest.find('>') else {
                push_text(&mut out, rest);
                break;
            };
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if !closing && matches!(name.as_str(), "script" | "style" | "head" | "title") {
                let close = format!("</{name}");
                let lower = rest.to_ascii_lowercase();
                rest = match lower.find(&close) {
                    Some(pos) => rest[pos..].find('>').map_or("", |e| &rest[pos + e + 1..]),
                    None => "",
                };
                continue;
            }
            if closing && matches!(name.as_str(), "li" | "tr") {
                // Items break on open; closing too would double-space lists.
            } else if matches!(
                name.as_str(),
                "br" | "p"
                    | "div"
                    | "tr"
                    | "li"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "blockquote"
                    | "table"
                    | "ul"
                    | "ol"
                    | "hr"
                    | "pre"
            ) {
                out.push('\n');
            } else if matches!(name.as_str(), "td" | "th") && !closing {
                out.push(' ');
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            push_text(&mut out, &rest[..end]);
            rest = &rest[end..];
        }
    }
    tidy_lines(&out)
}

/// Appends decoded text, collapsing source whitespace into single spaces.
fn push_text(out: &mut String, raw: &str) {
    for c in decode_entities(raw).chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            if !out.ends_with([' ', '\n']) && !out.is_empty() {
                out.push(' ');
            }
        } else if c == '\u{a0}' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
}

fn decode_entities(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after.find(';').filter(|&end| end <= 10).and_then(|end| {
            let entity = &after[..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => entity.strip_prefix('#').and_then(|num| {
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => num.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }),
            }?;
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Trims each line and keeps at most one blank line between paragraphs.
fn tidy_lines(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(email: &str) -> Address {
        Address::new(None::<String>, email)
    }

    fn labels() -> QuoteLabels {
        QuoteLabels {
            attribution: "On Mon, Ann wrote:".into(),
            forward_header: "---------- Forwarded message ----------".into(),
            from: "From".into(),
            date: "Date".into(),
            subject: "Subject".into(),
            to: "To".into(),
            formatted_date: "Mon, 3 Jun 2024".into(),
        }
    }

    fn original() -> ParsedMessage {
        ParsedMessage {
            message_id: Some("orig@x".into()),
            references: vec!["root@x".into()],
            subject: "Plans".into(),
            from: Some(Address::new(Some("Ann"), "ann@x.com")),
            to: vec![a("me@x.com"), a("bob@x.com")],
            cc: vec![a("carol@x.com"), a("BOB@x.com"), a("alias@x.com")],
            text: Some("Hi\n\nSee you".into()),
            ..Default::default()
        }
    }

    fn me() -> Vec<Address> {
        vec![Address::new(Some("Me"), "me@x.com"), a("alias@x.com")]
    }

    #[test]
    fn subjects_avoid_double_prefixes() {
        assert_eq!(reply_subject("Plans"), "Re: Plans");
        assert_eq!(reply_subject(" RE: Plans "), "RE: Plans");
        assert_eq!(reply_subject("Res: Plano"), "Res: Plano");
        assert_eq!(reply_subject("AW: x"), "AW: x");
        assert_eq!(reply_subject("sv: x"), "sv: x");
        assert_eq!(reply_subject("Fwd: x"), "Re: Fwd: x");
        assert_eq!(reply_subject(""), "Re: ");
        assert_eq!(forward_subject("Plans"), "Fwd: Plans");
        for s in ["FWD: a", "Fw: a", "Enc: a", "WG: a"] {
            assert_eq!(forward_subject(s), s);
        }
        assert_eq!(forward_subject("Re: a"), "Fwd: Re: a");
    }

    #[test]
    fn quote_text_marks_lines() {
        assert_eq!(quote_text("a\n\nb\r\nc\n\n"), "> a\n>\n> b\n> c");
        assert_eq!(quote_text("> old"), "> > old");
        assert_eq!(quote_text(""), "");
    }

    #[test]
    fn reply_targets_reply_to_or_from() {
        let d = prepare_reply(&original(), ReplyKind::Reply, &me(), &labels());
        assert_eq!(d.from, me()[0]);
        assert_eq!(d.to, vec![Address::new(Some("Ann"), "ann@x.com")]);
        assert!(d.cc.is_empty());
        assert_eq!(d.subject, "Re: Plans");
        assert_eq!(d.in_reply_to.as_deref(), Some("orig@x"));
        assert_eq!(d.references, vec!["root@x", "orig@x"]);
        assert_eq!(d.text, "\n\nOn Mon, Ann wrote:\n> Hi\n>\n> See you");

        let mut with_reply_to = original();
        with_reply_to.reply_to = vec![a("list@x.com")];
        let d = prepare_reply(&with_reply_to, ReplyKind::Reply, &me(), &labels());
        assert_eq!(d.to, vec![a("list@x.com")]);
    }

    #[test]
    fn reply_all_excludes_me_and_duplicates() {
        let d = prepare_reply(&original(), ReplyKind::ReplyAll, &me(), &labels());
        assert_eq!(d.to, vec![Address::new(Some("Ann"), "ann@x.com")]);
        assert_eq!(d.cc, vec![a("bob@x.com"), a("carol@x.com")]);
    }

    #[test]
    fn replying_to_my_own_message_targets_its_recipients() {
        let mut sent = original();
        sent.from = Some(a("ME@x.com"));
        sent.to = vec![a("bob@x.com")];
        sent.cc = vec![];
        let d = prepare_reply(&sent, ReplyKind::Reply, &me(), &labels());
        assert_eq!(d.to, vec![a("bob@x.com")]);
    }

    #[test]
    fn forward_has_header_block_and_no_threading() {
        let d = prepare_reply(&original(), ReplyKind::Forward, &me(), &labels());
        assert!(d.to.is_empty() && d.cc.is_empty() && d.bcc.is_empty());
        assert_eq!(d.subject, "Fwd: Plans");
        assert_eq!(d.in_reply_to, None);
        assert!(d.references.is_empty());
        assert_eq!(
            d.text,
            "\n\n---------- Forwarded message ----------\nFrom: Ann <ann@x.com>\n\
Date: Mon, 3 Jun 2024\nSubject: Plans\nTo: me@x.com, bob@x.com\n\nHi\n\nSee you"
        );
    }

    #[test]
    fn html_only_original_is_quoted_as_text() {
        let mut m = original();
        m.text = None;
        m.html = Some("<p>Hello&nbsp;<b>world</b></p><p>Bye</p>".into());
        m.message_id = None;
        let d = prepare_reply(&m, ReplyKind::Reply, &[], &labels());
        assert_eq!(d.text, "\n\nOn Mon, Ann wrote:\n> Hello world\n>\n> Bye");
        assert_eq!(d.from, Address::default());
        assert_eq!(d.in_reply_to, None);
        assert_eq!(d.references, vec!["root@x"]);
    }

    #[test]
    fn html_to_text_basics() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body>\
<!-- hidden --><p>One   two\n three</p><div>A &amp; B &lt;c&gt; &#233;&#x00E9; &quot;q&quot; &bogus; &</div>\
line<br>break<ul><li>x</li><li>y</li></ul><table><tr><td>a</td><td>b</td></tr></table>\
<script>alert(1)</script>end</body></html>";
        assert_eq!(
            html_to_text(html),
            "One two three\n\nA & B <c> éé \"q\" &bogus; &\nline\nbreak\n\nx\ny\n\na b\nend"
        );
        assert_eq!(html_to_text("plain"), "plain");
        assert_eq!(html_to_text("a < b"), "a < b");
        assert_eq!(html_to_text(""), "");
    }
}
