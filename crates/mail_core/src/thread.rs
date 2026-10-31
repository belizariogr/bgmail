//! Conversation grouping helpers.

use crate::compose::strip_brackets;
use crate::reply::{FORWARD_PREFIXES, REPLY_PREFIXES};

/// Subject reduced for thread matching: reply/forward prefixes stripped
/// repeatedly (also `Re[2]:` counters), lowercased, trimmed. Bracketed list
/// tags like `[dev]` are kept: they often distinguish real conversations.
pub fn normalize_subject(subject: &str) -> String {
    let mut current = subject.trim().to_lowercase();
    loop {
        let stripped = REPLY_PREFIXES
            .iter()
            .chain(FORWARD_PREFIXES)
            .find_map(|prefix| strip_prefix_with_counter(&current, prefix));
        match stripped {
            Some(rest) => current = rest.trim_start().to_string(),
            None => return current.trim().to_string(),
        }
    }
}

/// Strips `re:` as well as `re[2]:` / `re (2):` style counters.
fn strip_prefix_with_counter<'a>(subject: &'a str, prefix: &str) -> Option<&'a str> {
    let word = prefix.trim_end_matches(':');
    if let Some(rest) = subject.strip_prefix(prefix) {
        return Some(rest);
    }
    let rest = subject.strip_prefix(word)?.trim_start();
    let (open, close) = match rest.chars().next()? {
        '[' => ('[', ']'),
        '(' => ('(', ')'),
        _ => return None,
    };
    let inner_end = rest.find(close)?;
    let counter = &rest[open.len_utf8()..inner_end];
    if counter.is_empty() || !counter.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    rest[inner_end + 1..].trim_start().strip_prefix(':')
}

/// Stable thread identifier: the root of the References chain, else the
/// parent, else the message itself. `None` when no id is known at all (the
/// caller then falls back to subject grouping).
pub fn thread_key(
    message_id: Option<&str>,
    in_reply_to: Option<&str>,
    references: &[String],
) -> Option<String> {
    references
        .iter()
        .map(String::as_str)
        .chain(in_reply_to)
        .chain(message_id)
        .map(strip_brackets)
        .find(|id| !id.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_prefix_chains() {
        assert_eq!(normalize_subject("Re: RE: Fwd: Plans "), "plans");
        assert_eq!(normalize_subject("RES: ENC: Proposta"), "proposta");
        assert_eq!(normalize_subject("AW: WG: SV: Fw: x"), "x");
        assert_eq!(normalize_subject("Re[2]: Re (3): Hello"), "hello");
        assert_eq!(normalize_subject("[dev] Re: build"), "[dev] re: build");
        assert_eq!(normalize_subject("Regarding taxes"), "regarding taxes");
        assert_eq!(normalize_subject("Re[x]: keep"), "re[x]: keep");
        assert_eq!(normalize_subject("Re:"), "");
        assert_eq!(normalize_subject(""), "");
    }

    #[test]
    fn thread_key_prefers_root_reference() {
        let refs = vec!["<root@x>".to_string(), "mid@x".to_string()];
        assert_eq!(
            thread_key(Some("me@x"), Some("mid@x"), &refs).as_deref(),
            Some("root@x")
        );
        assert_eq!(
            thread_key(Some("me@x"), Some("<parent@x>"), &[]).as_deref(),
            Some("parent@x")
        );
        assert_eq!(thread_key(Some("me@x"), None, &[]).as_deref(), Some("me@x"));
        assert_eq!(thread_key(Some(" "), Some(""), &[String::new()]), None);
        assert_eq!(thread_key(None, None, &[]), None);
    }
}
