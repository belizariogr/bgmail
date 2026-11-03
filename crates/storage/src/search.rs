//! Accent-insensitive, case-insensitive search helpers.

use unicode_normalization::UnicodeNormalization;

/// Folds accents and lowercases text for partial SQL `LIKE` search.
pub fn fold_for_search(text: &str) -> String {
    text.nfd()
        .filter(|ch| !unicode_normalization::char::is_combining_mark(*ch))
        .collect::<String>()
        .to_lowercase()
}

/// Builds a `LIKE` pattern for a folded query string, for use with `ESCAPE '\\'`:
/// what the user typed is matched literally, even `%` and `_`.
pub fn search_like_pattern(query: &str) -> Option<String> {
    let folded = fold_for_search(query.trim());
    if folded.is_empty() {
        None
    } else {
        let escaped = folded
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        Some(format!("%{escaped}%"))
    }
}

/// Concatenates searchable fields into one folded blob stored on each message.
pub fn build_search_text(
    sender: &str,
    sender_email: &str,
    subject: &str,
    plain_text: &str,
) -> String {
    collapse_spaces(&fold_for_search(&format!(
        "{sender} {sender_email} {subject} {plain_text}"
    )))
}

/// Any run of spaces and line breaks as one space, in what is stored and what is searched:
/// a phrase can cross where a plain text message breaks its lines.
pub(crate) fn collapse_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_strips_accents_and_lowercases() {
        assert_eq!(fold_for_search("São Paulo"), "sao paulo");
        assert_eq!(fold_for_search("CAFÉ"), "cafe");
    }

    #[test]
    fn search_pattern_is_partial() {
        assert_eq!(search_like_pattern("  Git  "), Some("%git%".to_string()));
        assert!(search_like_pattern("   ").is_none());
        assert_eq!(
            search_like_pattern("a_%\\").as_deref(),
            Some("%a\\_\\%\\\\%")
        );
    }
}
