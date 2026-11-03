//! `mailto:` links (RFC 6068): what another app asks BGMail to compose.

use crate::address::{valid_addresses, Address};
use crate::compose::Draft;

/// The draft a `mailto:` URI describes, or `None` for any other URI. Only the
/// recipients, subject and body are honored: links come from anywhere, so
/// nothing they say may attach files or set other headers.
pub fn draft_from_mailto(uri: &str) -> Option<Draft> {
    let rest = uri
        .trim()
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        .map(|_| &uri.trim()[7..])?;
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut draft = Draft {
        to: addresses(&percent_decode(to)),
        ..Default::default()
    };
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value);
        match key.to_ascii_lowercase().as_str() {
            "to" => draft.to.extend(addresses(&value)),
            "cc" => draft.cc.extend(addresses(&value)),
            "bcc" => draft.bcc.extend(addresses(&value)),
            "subject" => draft.subject = single_line(&value),
            "body" => draft.text = value.replace("\r\n", "\n"),
            _ => {}
        }
    }
    Some(draft)
}

/// Valid addresses of a comma-separated list; the invalid ones are dropped.
fn addresses(list: &str) -> Vec<Address> {
    valid_addresses(list)
}

/// A line break in a subject would be a header injection once sent.
fn single_line(text: &str) -> String {
    text.split(['\r', '\n'])
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// `%XX` decoded as UTF-8 (lossy); a bad escape stays as written. `+` is a plus
/// sign in `mailto:`, not a space.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        let escape = (bytes[ix] == b'%')
            .then(|| text.get(ix + 1..ix + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escape {
            Some(byte) => {
                out.push(byte);
                ix += 3;
            }
            None => {
                out.push(bytes[ix]);
                ix += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_name_survives_an_invalid_neighbor() {
        let draft = draft_from_mailto("mailto:%22Doe,%20John%22%20%3Cj@x.test%3E,nope").unwrap();
        assert_eq!(draft.to.len(), 1);
        assert_eq!(draft.to[0].email, "j@x.test");
    }

    fn emails(list: &[Address]) -> Vec<&str> {
        list.iter().map(|a| a.email.as_str()).collect()
    }

    #[test]
    fn plain_address() {
        let draft = draft_from_mailto("mailto:ann@example.com").unwrap();
        assert_eq!(emails(&draft.to), ["ann@example.com"]);
        assert!(draft.subject.is_empty() && draft.text.is_empty());
    }

    #[test]
    fn every_supported_field_is_read() {
        let draft = draft_from_mailto(
            "MAILTO:a@x.test,b@x.test?cc=c@x.test&BCC=d@x.test&to=e@x.test\
             &subject=Ol%C3%A1%20mundo&body=linha%201%0D%0Alinha%202",
        )
        .unwrap();
        assert_eq!(emails(&draft.to), ["a@x.test", "b@x.test", "e@x.test"]);
        assert_eq!(emails(&draft.cc), ["c@x.test"]);
        assert_eq!(emails(&draft.bcc), ["d@x.test"]);
        assert_eq!(draft.subject, "Olá mundo");
        assert_eq!(draft.text, "linha 1\nlinha 2");
    }

    #[test]
    fn no_recipient_is_fine() {
        let draft = draft_from_mailto("mailto:?subject=Hi").unwrap();
        assert!(draft.to.is_empty());
        assert_eq!(draft.subject, "Hi");
    }

    #[test]
    fn links_cannot_inject_headers_or_attach_files() {
        let draft = draft_from_mailto(
            "mailto:a@x.test?subject=Hi%0D%0ABcc:%20evil@x.test\
             &attach=/etc/passwd&in-reply-to=x@y&from=evil@x.test",
        )
        .unwrap();
        assert_eq!(draft.subject, "Hi Bcc: evil@x.test");
        assert!(draft.attachments.is_empty() && draft.bcc.is_empty());
        assert_eq!(draft.in_reply_to, None);
        assert!(draft.from.email.is_empty());
    }

    #[test]
    fn invalid_addresses_are_dropped_and_bad_escapes_kept() {
        let draft = draft_from_mailto("mailto:a@x.test,nope?subject=100%25%20%ZZ").unwrap();
        assert_eq!(emails(&draft.to), ["a@x.test"]);
        assert_eq!(draft.subject, "100% %ZZ");
    }

    #[test]
    fn plus_stays_a_plus() {
        let draft = draft_from_mailto("mailto:a+tag@x.test?subject=1+1").unwrap();
        assert_eq!(emails(&draft.to), ["a+tag@x.test"]);
        assert_eq!(draft.subject, "1+1");
    }

    #[test]
    fn other_uris_are_refused() {
        for uri in ["https://x.test", "mail", "", "mailt:a@b.c", "ção:a@b.c"] {
            assert_eq!(draft_from_mailto(uri), None, "{uri}");
        }
    }
}
