//! Signatures, as in Gmail: the saved ones, which one a message gets, and where it
//! goes in the body.
//!
//! Each account has one for new mail and one for replies and forwards (Gmail's
//! "Signature defaults"). The signature goes in the editor, after what the user
//! writes and before the quoted text, without the `-- ` line: Gmail's "Insert this
//! signature before quoted text in replies and remove the '--' line that precedes it".
//! Pure functions here; `compose` and `signature_settings` drive them.

use std::ops::Range;

use ui::RichDocument;

use crate::compose::ComposeAccount;

/// A saved signature, ready to insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureEntry {
    pub id: i64,
    pub name: String,
    pub document: RichDocument,
}

impl SignatureEntry {
    /// `row` with its document read back; one that can't be read is empty, so a
    /// damaged signature never stops a message from being written.
    pub fn from_row(row: &storage::Signature) -> Self {
        Self {
            id: row.id,
            name: row.name.clone(),
            document: read_document(&row.document),
        }
    }
}

/// The stored form of a signature's document.
pub fn document_json(document: &RichDocument) -> String {
    serde_json::to_string(document).unwrap_or_default()
}

/// A stored document. One that can't be read as a whole (written by a newer
/// version, damaged) keeps its text when that can be read, without the formatting;
/// otherwise it is empty.
pub fn read_document(json: &str) -> RichDocument {
    serde_json::from_str::<RichDocument>(json)
        .map(RichDocument::validated)
        .unwrap_or_else(|_| {
            serde_json::from_str::<serde_json::Value>(json)
                .ok()
                .and_then(|value| value.get("text")?.as_str().map(RichDocument::plain))
                .unwrap_or_default()
        })
}

/// Which of an account's two signatures a message takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureSlot {
    NewMail,
    /// Replies and forwards.
    Reply,
}

/// The signature `account` puts in a message of `slot`.
pub fn account_signature(account: &ComposeAccount, slot: SignatureSlot) -> Option<i64> {
    match slot {
        SignatureSlot::NewMail => account.signature_new,
        SignatureSlot::Reply => account.signature_reply,
    }
}

/// Where a message's signature goes in `text`: after what is written, so at the end
/// of a new message or a reply whose quote stays out of the editor, and above the
/// original when the editor holds it (a forward, or a reply whose quote couldn't be
/// kept out).
pub fn signature_offset(text: &str, quote_in_editor: bool) -> usize {
    if quote_in_editor {
        original_start(text).unwrap_or_else(|| text.len() - text.trim_start_matches('\n').len())
    } else {
        text.len()
    }
}

/// Where the original starts in a body that holds it: the forwarded block's header
/// line (`---------- … ----------`, in any language) or the attribution line of a
/// `> ` quote (a line ending in `:` followed by one starting with `>`).
pub fn original_start(text: &str) -> Option<usize> {
    let mut offset = 0;
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        let forward_header = trimmed.len() > 20
            && trimmed.starts_with("----------")
            && trimmed.ends_with("----------");
        let attribution =
            trimmed.ends_with(':') && lines.peek().is_some_and(|next| next.starts_with('>'));
        if forward_header || attribution {
            return Some(offset);
        }
        offset += line.len() + 1;
    }
    None
}

/// What goes in at `at` of `text` to add `signature`, and where the signature is in it:
/// a blank line between it and what is around, and at the start a line to type on.
pub fn signature_insertion(
    text: &str,
    at: usize,
    signature: &RichDocument,
) -> (RichDocument, Range<usize>) {
    let breaks_before = text[..at].len() - text[..at].trim_end_matches('\n').len();
    let before = "\n".repeat(2usize.saturating_sub(breaks_before));
    let rest = &text[at..];
    let breaks_after = rest.len() - rest.trim_start_matches('\n').len();
    let after = if rest.is_empty() {
        String::new()
    } else {
        "\n".repeat(2usize.saturating_sub(breaks_after))
    };
    let mut insertion = RichDocument::plain(&before);
    for part in [signature, &RichDocument::plain(&after)] {
        let end = insertion.text.len();
        insertion.text = insertion.rich.splice(&insertion.text, end..end, part);
    }
    let start = before.len();
    (insertion, start..start + signature.text.len())
}

/// `text` with `signature` at its place (see [`signature_offset`]), and the range it
/// took. Without one, the empty range where one would go.
pub fn body_with_signature(
    text: &str,
    quote_in_editor: bool,
    signature: Option<&RichDocument>,
) -> (RichDocument, Range<usize>) {
    let mut body = RichDocument::plain(text);
    // On the text as the editor holds it (line endings normalized).
    let at = signature_offset(&body.text, quote_in_editor);
    match signature.filter(|signature| !signature.is_empty()) {
        Some(signature) => {
            let (insertion, range) = signature_insertion(&body.text, at, signature);
            body.text = body.rich.splice(&body.text, at..at, &insertion);
            (body, at + range.start..at + range.end)
        }
        None => (body, at..at),
    }
}

/// The signature a body already has (an edited unsent message): the saved one whose
/// plain text (as it went out: images left out) ends nearest above the original, or
/// the end. Its range and id.
pub fn find_signature(
    text: &str,
    quote_in_editor: bool,
    signatures: &[SignatureEntry],
) -> Option<(Range<usize>, i64)> {
    let limit = if quote_in_editor {
        original_start(text).unwrap_or(text.len())
    } else {
        text.len()
    };
    let searched = &text[..limit];
    signatures
        .iter()
        .filter_map(|entry| {
            let plain = entry.document.to_plain();
            let plain = plain.trim_end_matches('\n');
            let start = (!plain.trim().is_empty())
                .then(|| searched.rfind(plain))
                .flatten()?;
            Some((start..start + plain.len(), entry.id))
        })
        .max_by_key(|(range, _)| (range.end, range.len()))
}

/// What the body holds where its signature goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureInBody {
    /// Nothing: none was put there, or it was deleted.
    Nothing,
    /// A saved signature, as saved.
    Saved(i64),
    /// Text that isn't a saved signature as saved: one the user edited.
    Edited,
}

/// What `text` holds in `tracked`, the editor's range of the signature, put there
/// as signature `tag`. Read from the text itself (the tag travels with undo), so
/// undoing a change of signature can't leave it out of date.
pub fn signature_in_body(
    text: &str,
    tracked: Option<Range<usize>>,
    tag: Option<i64>,
    signatures: &[SignatureEntry],
) -> SignatureInBody {
    let Some(held) = tracked
        .filter(|range| !range.is_empty())
        .and_then(|range| text.get(range))
    else {
        return SignatureInBody::Nothing;
    };
    let as_saved =
        |entry: &&SignatureEntry| !entry.document.is_empty() && entry.document.text == held;
    // Signatures may share their text: the one put there is the one held.
    match tag.and_then(|tag| signatures.iter().find(|entry| entry.id == tag)) {
        Some(entry) if as_saved(&entry) => SignatureInBody::Saved(entry.id),
        Some(_) => SignatureInBody::Edited,
        None => signatures
            .iter()
            .find(as_saved)
            .map_or(SignatureInBody::Edited, |entry| {
                SignatureInBody::Saved(entry.id)
            }),
    }
}

/// Whether the body, holding `held`, must change to have signature `wanted`.
pub fn needs_signature_change(held: SignatureInBody, wanted: Option<i64>) -> bool {
    match (held, wanted) {
        (SignatureInBody::Nothing, None) => false,
        (SignatureInBody::Saved(id), Some(wanted)) => id != wanted,
        _ => true,
    }
}

/// Where the caret at `caret` goes when `replaced` becomes `inserted_len` bytes: it
/// stays put before, moves with the text after, and goes to the start from inside.
pub fn caret_after_replace(caret: usize, replaced: &Range<usize>, inserted_len: usize) -> usize {
    if caret <= replaced.start {
        caret
    } else if caret >= replaced.end {
        caret - replaced.len() + inserted_len
    } else {
        replaced.start
    }
}

/// The name of a new signature, numbered after those taken (`template` has `{n}`).
pub fn new_signature_name(template: &str, existing: &[SignatureEntry]) -> String {
    (existing.len() + 1..)
        .map(|n| template.replace("{n}", &n.to_string()))
        .find(|name| !existing.iter().any(|entry| entry.name == *name))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::{InlineImage, IMAGE_CHAR};

    fn signature(text: &str) -> RichDocument {
        let mut document = RichDocument::plain(text);
        document
            .rich
            .update_chars(0..text.len(), |style| style.bold = true);
        document
    }

    fn account(new: Option<i64>, reply: Option<i64>) -> ComposeAccount {
        ComposeAccount {
            id: 1,
            name: "Me".into(),
            email: "me@x.test".into(),
            can_send: true,
            signature_new: new,
            signature_reply: reply,
        }
    }

    #[test]
    fn new_mail_and_replies_take_their_own_signature() {
        let account = account(Some(1), Some(2));
        assert_eq!(account_signature(&account, SignatureSlot::NewMail), Some(1));
        assert_eq!(account_signature(&account, SignatureSlot::Reply), Some(2));
    }

    #[test]
    fn the_signature_follows_a_blank_line_with_a_line_above_to_type_on() {
        let (body, range) = body_with_signature("", false, Some(&signature("Bob")));
        assert_eq!(body.text, "\n\nBob");
        assert_eq!(range, 2..5);
        assert!(body.rich.all_chars(2..5, |style| style.bold));
        // After a body that came with the message (mailto:), a blank line apart.
        let (body, range) = body_with_signature("Hi", false, Some(&signature("Bob")));
        assert_eq!(body.text, "Hi\n\nBob");
        assert_eq!(&body.text[range], "Bob");
        let (body, _) = body_with_signature("Hi\n", false, Some(&signature("Bob")));
        assert_eq!(body.text, "Hi\n\nBob");
    }

    #[test]
    fn a_forward_is_signed_above_the_forwarded_message() {
        let forwarded = "\n\n---------- Forwarded message ----------\nFrom: Ann";
        let (body, range) = body_with_signature(forwarded, true, Some(&signature("Bob")));
        assert_eq!(
            body.text,
            "\n\nBob\n\n---------- Forwarded message ----------\nFrom: Ann"
        );
        assert_eq!(&body.text[range], "Bob");
        assert!(!body.rich.all_chars(5..10, |style| style.bold));
    }

    #[test]
    fn without_a_signature_the_place_for_one_is_kept() {
        let (body, range) = body_with_signature("Hi", false, None);
        assert_eq!((body.text.as_str(), range), ("Hi", 2..2));
        let (body, range) = body_with_signature("Hi", false, Some(&RichDocument::plain("")));
        assert_eq!((body.text.as_str(), range), ("Hi", 2..2));
        assert_eq!(signature_offset("\n\nFwd", true), 2);
    }

    #[test]
    fn a_signature_added_later_gets_the_same_room() {
        let (insertion, range) = signature_insertion("Hi\n\n", 4, &signature("Bob"));
        assert_eq!((insertion.text.as_str(), range), ("Bob", 0..3));
        let (insertion, range) = signature_insertion("\n\nFwd", 2, &signature("Bob"));
        assert_eq!((insertion.text.as_str(), range), ("Bob\n\n", 0..3));
    }

    #[test]
    fn a_signature_brings_its_images() {
        let text = format!("Bob\n{IMAGE_CHAR}");
        let mut logo = RichDocument::plain(&text);
        let index = logo
            .rich
            .add_image(InlineImage::new("logo.png", "image/png", vec![1], 4, 4));
        logo.rich
            .update_chars(4..text.len(), move |style| style.image = Some(index));
        let (body, _) = body_with_signature("", false, Some(&logo));
        assert_eq!(body.images().len(), 1);
        // And its stored form brings them back.
        let stored = read_document(&document_json(&logo));
        assert_eq!(stored, logo);
        assert_eq!(read_document("not json"), RichDocument::default());
    }

    #[test]
    fn the_caret_stays_out_of_a_replaced_signature() {
        assert_eq!(caret_after_replace(0, &(4..7), 10), 0);
        assert_eq!(caret_after_replace(9, &(4..7), 10), 16);
        assert_eq!(caret_after_replace(5, &(4..7), 10), 4);
    }

    #[test]
    fn a_new_signature_is_numbered_after_the_others() {
        let entry = |name: &str| SignatureEntry {
            id: 1,
            name: name.into(),
            document: RichDocument::default(),
        };
        assert_eq!(new_signature_name("Signature {n}", &[]), "Signature 1");
        assert_eq!(
            new_signature_name("Signature {n}", &[entry("Signature 2"), entry("Work")]),
            "Signature 3"
        );
        assert_eq!(
            new_signature_name("Signature {n}", &[entry("Signature 2")]),
            "Signature 3"
        );
    }

    #[test]
    fn the_signature_in_the_body_is_read_from_its_text() {
        let entry = |id: i64, text: &str| SignatureEntry {
            id,
            name: String::new(),
            document: RichDocument::plain(text),
        };
        let saved = [entry(1, ""), entry(2, "Ann"), entry(3, "Bob")];
        let text = "Hi\n\nAnn";
        let held = |range, tag| signature_in_body(text, range, tag, &saved);
        assert_eq!(held(Some(4..7), None), SignatureInBody::Saved(2));
        assert_eq!(held(Some(4..6), None), SignatureInBody::Edited);
        assert_eq!(held(Some(7..7), None), SignatureInBody::Nothing);
        assert_eq!(held(None, None), SignatureInBody::Nothing);
        assert_eq!(held(Some(4..99), None), SignatureInBody::Nothing);
        // Two saved with the same text: the one put there is the one held.
        let twins = [entry(2, "Ann"), entry(4, "Ann")];
        assert_eq!(
            signature_in_body(text, Some(4..7), Some(4), &twins),
            SignatureInBody::Saved(4)
        );
        assert_eq!(
            signature_in_body("Hi\n\nAnnie", Some(4..9), Some(4), &twins),
            SignatureInBody::Edited
        );

        use SignatureInBody::*;
        assert!(!needs_signature_change(Nothing, None));
        assert!(!needs_signature_change(Saved(2), Some(2)));
        assert!(needs_signature_change(Saved(2), Some(3)));
        assert!(needs_signature_change(Saved(2), None));
        assert!(needs_signature_change(Edited, None));
        assert!(needs_signature_change(Edited, Some(2)));
        assert!(needs_signature_change(Nothing, Some(2)));
    }

    #[test]
    fn a_reply_holding_its_quote_is_signed_above_it() {
        let reply = "\n\nOn Mon, Zoe wrote:\n> Hi";
        let (body, range) = body_with_signature(reply, true, Some(&signature("Bob")));
        assert_eq!(body.text, "\n\nBob\n\nOn Mon, Zoe wrote:\n> Hi");
        assert_eq!(&body.text[range], "Bob");
    }

    #[test]
    fn a_re_edited_reply_or_forward_is_signed_above_its_original() {
        let reply = "Hi\n\nOn Mon, Zoe wrote:\n> Hello\n>\n> Bye";
        assert_eq!(original_start(reply), Some(4));
        let (body, range) = body_with_signature(reply, true, Some(&signature("Bob")));
        assert_eq!(&body.text[range], "Bob");
        assert!(body.text.starts_with("Hi\n\nBob\n\nOn Mon, Zoe wrote:"));
        let forward = "Oi\n\n---------- Mensagem encaminhada ----------\nDe: Zoe";
        assert_eq!(original_start(forward), Some(4));
        assert_eq!(original_start("Hi: there\nno quote"), None);
        assert_eq!(original_start("Plain\n\nBob"), None);
    }

    #[test]
    fn a_body_with_windows_line_endings_is_signed_inside_it() {
        let (body, range) = body_with_signature("Hi\r\nBob", false, Some(&signature("Sig")));
        assert_eq!(body.text, "Hi\nBob\n\nSig");
        assert_eq!(&body.text[range], "Sig");
        let (body, range) = body_with_signature("Hi\r\n", false, None);
        assert_eq!((body.text.as_str(), range), ("Hi\n", 3..3));
    }

    #[test]
    fn a_signature_from_a_newer_version_keeps_its_text() {
        let json =
            r#"{"text":"Ann\nCEO","rich":{"runs":[[7,{"family":"Futura"}]],"paras":[{},{}]}}"#;
        assert_eq!(read_document(json), RichDocument::plain("Ann\nCEO"));
        assert_eq!(read_document("{}"), RichDocument::default());
    }

    #[test]
    fn the_signature_an_unsent_message_has_is_found_above_its_original() {
        let entry = |id: i64, text: &str| SignatureEntry {
            id,
            name: String::new(),
            document: RichDocument::plain(text),
        };
        let saved = [entry(1, "Ann"), entry(2, "Ann Smith\nCEO"), entry(3, "")];
        let reply = "Hi Ann\n\nAnn Smith\nCEO\n\nOn Mon, Zoe wrote:\n> Ann";
        let (range, id) = find_signature(reply, true, &saved).unwrap();
        assert_eq!((&reply[range], id), ("Ann Smith\nCEO", 2));
        assert_eq!(find_signature("Hi there", false, &saved), None);
        // With its image, it is found by the text that went out.
        let text = format!("Bob\n{}", ui::IMAGE_CHAR);
        let mut logo = RichDocument::plain(&text);
        let ix = logo
            .rich
            .add_image(ui::InlineImage::new("l.png", "image/png", vec![1], 1, 1));
        logo.rich
            .update_chars(4..text.len(), move |style| style.image = Some(ix));
        let with_logo = [SignatureEntry {
            id: 9,
            name: String::new(),
            document: logo,
        }];
        let (range, id) = find_signature("Hi\n\nBob", false, &with_logo).unwrap();
        assert_eq!((range, id), (4..7, 9));
    }
}
