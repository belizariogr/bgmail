//! E-mail addresses as the composer and the sync layer see them.
//!
//! Parsing here is intentionally forgiving (what users type into a "To" field),
//! not a full RFC 5322 grammar: the goal is to split, validate and round-trip
//! the common shapes and report the tokens that make no sense.

use serde::{Deserialize, Serialize};

/// A single mailbox: optional display name plus the address itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

/// Characters that force a display name to be quoted (RFC 5322 `specials`).
const SPECIALS: &[char] = &[
    '(', ')', '<', '>', '[', ']', ':', ';', '@', '\\', ',', '.', '"',
];

impl Address {
    /// Builds an address; blank names collapse to `None` so callers never have
    /// to distinguish `Some("")` from "no name".
    pub fn new(name: Option<impl Into<String>>, email: impl Into<String>) -> Self {
        let name = name
            .map(Into::into)
            .map(|n: String| n.trim().to_string())
            .filter(|n| !n.is_empty());
        Self {
            name,
            email: email.into().trim().to_string(),
        }
    }

    /// Header-style rendering: `Name <email>`, or the bare email when there is
    /// no name. Names containing specials are quoted so the output re-parses.
    pub fn display(&self) -> String {
        match self.name.as_deref().filter(|n| !n.is_empty()) {
            None => self.email.clone(),
            Some(name) if name.contains(SPECIALS) => {
                let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\" <{}>", self.email)
            }
            Some(name) => format!("{name} <{}>", self.email),
        }
    }

    /// Short human label for lists and chips: the name when present.
    pub fn label(&self) -> &str {
        self.name
            .as_deref()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.email)
    }
}

/// Splits a user-typed recipient list on `,` / `;` (outside quotes and angle
/// brackets). Returns every valid address, or `Err` with each invalid token so
/// the UI can point at exactly what is wrong.
pub fn parse_address_list(input: &str) -> Result<Vec<Address>, Vec<String>> {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for token in split_tokens(input) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match parse_single(token) {
            Some(address) => valid.push(address),
            None => invalid.push(token.to_string()),
        }
    }
    if invalid.is_empty() {
        Ok(valid)
    } else {
        Err(invalid)
    }
}

/// Inverse of [`parse_address_list`]: `display()` of each, comma separated.
pub fn format_address_list(addresses: &[Address]) -> String {
    addresses
        .iter()
        .map(Address::display)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Mail systems treat addresses case-insensitively in practice (the local part
/// is technically case-sensitive, but no real provider relies on it).
pub fn same_email(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn split_tokens(input: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut in_quotes = false;
    let mut in_angle = false;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            '<' if !in_quotes => in_angle = true,
            '>' if !in_quotes => in_angle = false,
            ',' | ';' if !in_quotes && !in_angle => {
                tokens.push(&input[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    tokens.push(&input[start..]);
    tokens
}

fn parse_single(token: &str) -> Option<Address> {
    let Some(open) = find_unquoted(token, '<') else {
        return is_valid_email(token).then(|| Address::new(None::<String>, token));
    };
    let close = token[open..].find('>')? + open;
    if !token[close + 1..].trim().is_empty() {
        return None;
    }
    let email = token[open + 1..close].trim();
    if !is_valid_email(email) {
        return None;
    }
    let name = unquote(token[..open].trim());
    Some(Address::new(Some(name), email))
}

fn find_unquoted(token: &str, needle: char) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (i, c) in token.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' && in_quotes {
            escaped = true;
        } else if c == '"' {
            in_quotes = !in_quotes;
        } else if c == needle && !in_quotes {
            return Some(i);
        }
    }
    None
}

fn unquote(name: &str) -> String {
    let inner = name
        .strip_prefix('"')
        .and_then(|n| n.strip_suffix('"'))
        .unwrap_or(name);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_valid_email(email: &str) -> bool {
    let mut parts = email.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !email
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';' | '"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Address {
        Address::new(name, email)
    }

    #[test]
    fn new_trims_and_drops_blank_names() {
        assert_eq!(addr(Some("  "), " a@b.c ").name, None);
        assert_eq!(addr(Some(" Ann "), "a@b.c").name.as_deref(), Some("Ann"));
        assert_eq!(addr(None, " a@b.c ").email, "a@b.c");
    }

    #[test]
    fn display_plain_named_and_quoted() {
        assert_eq!(addr(None, "a@b.c").display(), "a@b.c");
        assert_eq!(addr(Some("Ann"), "a@b.c").display(), "Ann <a@b.c>");
        assert_eq!(
            addr(Some("Doe, John"), "j@x.com").display(),
            "\"Doe, John\" <j@x.com>"
        );
        assert_eq!(
            addr(Some("A \"B\" C."), "j@x.com").display(),
            "\"A \\\"B\\\" C.\" <j@x.com>"
        );
    }

    #[test]
    fn label_prefers_name() {
        assert_eq!(addr(Some("Ann"), "a@b.c").label(), "Ann");
        assert_eq!(addr(None, "a@b.c").label(), "a@b.c");
    }

    #[test]
    fn parses_common_shapes() {
        let list =
            parse_address_list(" \"Doe, John\" <j@x.com>; ann@y.org , Bob <bob@z.io>,, ;").unwrap();
        assert_eq!(
            list,
            vec![
                addr(Some("Doe, John"), "j@x.com"),
                addr(None, "ann@y.org"),
                addr(Some("Bob"), "bob@z.io"),
            ]
        );
        assert_eq!(parse_address_list("   ").unwrap(), vec![]);
        assert_eq!(
            parse_address_list("<solo@x.com>").unwrap(),
            vec![addr(None, "solo@x.com")]
        );
    }

    #[test]
    fn reports_invalid_tokens() {
        let err = parse_address_list("ok@x.com, nope, a@@b, two words@x.com, @x.com, a@, X <bad>")
            .unwrap_err();
        assert_eq!(
            err,
            vec!["nope", "a@@b", "two words@x.com", "@x.com", "a@", "X <bad>"]
        );
        assert!(parse_address_list("A <a@x.com> trailing").is_err());
    }

    #[test]
    fn format_round_trips() {
        let list = vec![addr(Some("Doe, John"), "j@x.com"), addr(None, "a@b.c")];
        let text = format_address_list(&list);
        assert_eq!(text, "\"Doe, John\" <j@x.com>, a@b.c");
        assert_eq!(parse_address_list(&text).unwrap(), list);
        assert_eq!(format_address_list(&[]), "");
    }

    #[test]
    fn same_email_ignores_case_and_whitespace() {
        assert!(same_email("Ann@Example.COM", " ann@example.com"));
        assert!(!same_email("ann@example.com", "anna@example.com"));
    }

    #[test]
    fn serde_round_trip() {
        let a = addr(Some("Ann"), "a@b.c");
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Address>(&json).unwrap(), a);
    }
}
