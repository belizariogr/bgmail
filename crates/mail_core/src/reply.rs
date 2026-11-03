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
    let body_source = readable_text(original.text.as_deref(), original.html.as_deref());
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
            // RFC 5322 3.6.4: with no References, the parent is In-Reply-To.
            if draft.references.is_empty() {
                draft.references.extend(original.in_reply_to.clone());
            }
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

/// The text a person reads for a message's bodies: its plain-text part, or its
/// HTML converted when that part is missing, blank or HTML itself (some senders
/// put the same markup in both parts).
pub fn readable_text(text: Option<&str>, html: Option<&str>) -> String {
    match (text, html) {
        (Some(text), Some(html)) if text.trim().is_empty() || looks_like_html(text) => {
            html_to_text(html)
        }
        (Some(text), _) => text.to_string(),
        (None, Some(html)) => html_to_text(html),
        (None, None) => String::new(),
    }
}

/// Whether `text` starts like an HTML document.
fn looks_like_html(text: &str) -> bool {
    let start: String = text
        .trim_start()
        .chars()
        .take(9)
        .collect::<String>()
        .to_ascii_lowercase();
    ["<!doctype", "<html", "<head", "<body"]
        .iter()
        .any(|tag| start.starts_with(tag))
}

/// Style of Gmail's quote block, which it also uses to recognize and collapse
/// quoted text.
const GMAIL_QUOTE_STYLE: &str =
    "margin:0px 0px 0px 0.8ex;border-left:1px solid rgb(204,204,204);padding-left:1ex";

/// HTML alternative of a reply's plain text, with the markup Gmail writes:
/// `> ` lines become nested `blockquote.gmail_quote` inside a `div.gmail_quote`,
/// headed by their attribution line (the last text line before them, when it
/// ends in a colon). Built from the text so it always matches what was edited.
pub fn reply_html(text: &str) -> String {
    let lines: Vec<(usize, &str)> = text.lines().map(quote_depth).collect();
    let attribution = lines
        .iter()
        .position(|(depth, _)| *depth > 0)
        .and_then(|first| {
            lines[..first]
                .iter()
                .rposition(|(_, line)| !line.trim().is_empty())
                .filter(|&ix| lines[ix].1.trim_end().ends_with(':'))
        });
    const OPEN_QUOTE: &str = "</div><br><div class=\"gmail_quote\">";

    let mut out = String::from("<div dir=\"ltr\">");
    let mut in_quote = false;
    let mut depth = 0;
    for (ix, &(line_depth, line)) in lines.iter().enumerate() {
        if Some(ix) == attribution {
            out.push_str(OPEN_QUOTE);
            out.push_str("<div dir=\"ltr\" class=\"gmail_attr\">");
            out.push_str(&html_escape::encode_text(line.trim_end()));
            out.push_str("<br></div>");
            in_quote = true;
            continue;
        }
        if line_depth == 0 && in_quote {
            if line.trim().is_empty() {
                continue;
            }
            // Text written below the quote must not collapse with it.
            out.push_str(&"</blockquote>".repeat(depth));
            out.push_str("</div><div dir=\"ltr\">");
            depth = 0;
            in_quote = false;
        }
        if line_depth > 0 && !in_quote {
            out.push_str(OPEN_QUOTE);
            in_quote = true;
        }
        if depth < line_depth {
            let open = format!("<blockquote class=\"gmail_quote\" style=\"{GMAIL_QUOTE_STYLE}\">");
            out.push_str(&open.repeat(line_depth - depth));
        } else {
            out.push_str(&"</blockquote>".repeat(depth - line_depth));
        }
        depth = line_depth;
        push_keeping_spaces(&mut out, line);
        out.push_str("<br>");
    }
    out.push_str(&"</blockquote>".repeat(depth));
    out.push_str("</div>");
    out
}

/// Escapes a line for HTML, keeping the runs of spaces it would collapse, and those it
/// starts with: indentation or a table in columns would otherwise lose their shape.
fn push_keeping_spaces(out: &mut String, line: &str) {
    for piece in line.split_inclusive(' ') {
        match piece.strip_suffix(' ') {
            Some("") => out.push_str("&nbsp;"),
            Some(text) => {
                out.push_str(&html_escape::encode_text(text));
                out.push(' ');
            }
            None => out.push_str(&html_escape::encode_text(piece)),
        }
    }
}

/// What the user wrote, already as HTML, followed by a Gmail quote of the
/// original's HTML, which must already be safe to send on.
pub fn reply_html_with_original(
    written_html: &str,
    attribution: &str,
    original_html: &str,
) -> String {
    let mut out = written_html.to_string();
    out.push_str("<br><div class=\"gmail_quote\"><div dir=\"ltr\" class=\"gmail_attr\">");
    out.push_str(&html_escape::encode_text(attribution));
    out.push_str("<br></div><blockquote class=\"gmail_quote\" style=\"");
    out.push_str(GMAIL_QUOTE_STYLE);
    out.push_str("\">");
    out.push_str(original_html);
    out.push_str("</blockquote></div>");
    out
}

/// Quote depth of a line and its text without the `> ` markers.
fn quote_depth(line: &str) -> (usize, &str) {
    let mut depth = 0;
    let mut rest = line;
    while let Some(after) = rest.strip_prefix('>') {
        depth += 1;
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    (depth, rest)
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
            // Like a browser, `a < b` is text: a tag starts with a letter, `/`, `!` or `?`.
            let next = rest[1..].chars().next();
            if !next.is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?')) {
                push_text(&mut out, "<");
                rest = &rest[1..];
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
            if !closing && matches!(name.as_str(), "script" | "style" | "title") {
                let close = format!("</{name}");
                // Without lowercasing the rest of the document at every tag: a sender
                // repeating `<style></style>` would make that quadratic.
                let found = rest
                    .as_bytes()
                    .windows(close.len())
                    .position(|window| window.eq_ignore_ascii_case(close.as_bytes()));
                rest = match found {
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

/// Invisible characters marketing mail pads hidden preview text with. The zero
/// width joiner stays: it builds emoji sequences.
fn is_invisible_filler(c: char) -> bool {
    matches!(
        c,
        '\u{ad}' | '\u{200b}' | '\u{200c}' | '\u{2060}' | '\u{feff}'
    )
}

/// Appends decoded text, collapsing source whitespace into single spaces.
fn push_text(out: &mut String, raw: &str) {
    for c in html_escape::decode_html_entities(raw).chars() {
        if c.is_whitespace() {
            if !out.ends_with([' ', '\n']) && !out.is_empty() {
                out.push(' ');
            }
        } else if !is_invisible_filler(c) {
            out.push(c);
        }
    }
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
mod reply_format_tests {
    use super::*;

    #[test]
    fn named_entities_and_fillers_leave_no_trace() {
        let text = html_to_text("<p>Ol&aacute;&nbsp;&zwnj;&nbsp;&zwnj;&nbsp;mundo&#8203;!</p>");
        assert_eq!(text, "Olá mundo!");
    }

    #[test]
    fn readable_text_prefers_the_text_part_unless_it_is_html() {
        let html = "<p>Real</p>";
        assert_eq!(readable_text(Some("Plain"), Some(html)), "Plain");
        assert_eq!(readable_text(None, Some(html)), "Real");
        assert_eq!(readable_text(Some("  \n"), Some(html)), "Real");
        assert_eq!(
            readable_text(Some("<!DOCTYPE html><html><p>Copy</p></html>"), Some(html)),
            "Real"
        );
        // Without an HTML part the sender's text stays as it was written.
        assert_eq!(
            readable_text(Some("<html> is a tag"), None),
            "<html> is a tag"
        );
        assert_eq!(readable_text(None, None), "");
    }

    #[test]
    fn reply_html_follows_gmail_structure() {
        let html = reply_html("Hi <you>\n\nOn Mon, Ann <a@x> wrote:\n> first\n>\n>> deep\n");
        assert_eq!(
            html,
            format!(
                "<div dir=\"ltr\">Hi &lt;you&gt;<br><br></div><br><div class=\"gmail_quote\">\
                 <div dir=\"ltr\" class=\"gmail_attr\">On Mon, Ann &lt;a@x&gt; wrote:<br></div>\
                 <blockquote class=\"gmail_quote\" style=\"{GMAIL_QUOTE_STYLE}\">first<br><br>\
                 <blockquote class=\"gmail_quote\" style=\"{GMAIL_QUOTE_STYLE}\">deep<br>\
                 </blockquote></blockquote></div>"
            )
        );
    }

    #[test]
    fn reply_html_keeps_text_below_the_quote_out_of_it() {
        let html = reply_html("On Ann wrote:\n> quoted\n\nMy answer");
        let quote_end = html.find("</blockquote>").unwrap();
        assert!(html.find("My answer").unwrap() > quote_end);
        assert!(html.ends_with("<div dir=\"ltr\">My answer<br></div>"));
    }

    #[test]
    fn reply_quotes_the_original_html_below_what_was_written() {
        let html =
            reply_html_with_original(&reply_html("My <answer>"), "On Ann wrote:", "<b>Hello</b>");
        assert!(html.starts_with("<div dir=\"ltr\">My &lt;answer&gt;<br></div>"));
        assert!(html.contains("class=\"gmail_attr\">On Ann wrote:<br></div>"));
        assert!(html.ends_with("<b>Hello</b></blockquote></div>"));
    }

    #[test]
    fn reply_html_without_quote_is_a_single_block() {
        assert_eq!(reply_html("Hi"), "<div dir=\"ltr\">Hi<br></div>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_without_its_closing_tags_keeps_its_text() {
        for html in [
            "<html><head><meta charset=utf-8><body><p>Hello world</p></body></html>",
            "<head/><p>Hello world</p>",
            "<head><meta x><p>Hello world</p>",
        ] {
            assert_eq!(html_to_text(html), "Hello world", "{html}");
        }
        assert_eq!(
            html_to_text("<html><head><title>T</title><style>a{}</style></head><p>x</p>"),
            "x"
        );
    }

    #[test]
    fn many_skipped_blocks_take_linear_time() {
        let html = "<style></style>".repeat(200_000);
        let start = std::time::Instant::now();
        assert_eq!(html_to_text(&html), "");
        // Quadratic, this size takes well over a minute.
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn a_less_than_sign_in_text_is_not_a_tag() {
        assert_eq!(
            html_to_text("<p>if x < 3 and y > 2 then</p><p>I <3 you</p>"),
            "if x < 3 and y > 2 then\n\nI <3 you"
        );
        assert_eq!(html_to_text("a <"), "a <");
    }

    #[test]
    fn a_reply_keeps_spaces_and_the_conversation() {
        let html = reply_html("Total:   10\n    indented");
        assert!(html.contains("Total: &nbsp;&nbsp;10"), "{html}");
        assert!(
            html.contains("<br>&nbsp;&nbsp;&nbsp;&nbsp;indented"),
            "{html}"
        );
        assert!(reply_html("a b").contains("a b"));

        let original = ParsedMessage {
            message_id: Some("child@x".into()),
            in_reply_to: Some("root@x".into()),
            from: Some(Address::new(None::<String>, "a@x.test")),
            ..Default::default()
        };
        let draft = prepare_reply(&original, ReplyKind::Reply, &[], &QuoteLabels::default());
        assert_eq!(draft.references, ["root@x", "child@x"]);
    }

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
