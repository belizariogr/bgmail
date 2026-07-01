//! Accent-insensitive, case-insensitive search helpers.

use unicode_normalization::UnicodeNormalization;

/// Folds accents and lowercases text for partial SQL `LIKE` search.
pub fn fold_for_search(text: &str) -> String {
    text.nfd()
        .filter(|ch| !unicode_normalization::char::is_combining_mark(*ch))
        .collect::<String>()
        .to_lowercase()
}

/// Builds a `LIKE` pattern for a folded query string.
pub fn search_like_pattern(query: &str) -> Option<String> {
    let folded = fold_for_search(query.trim());
    if folded.is_empty() {
        None
    } else {
        Some(format!("%{folded}%"))
    }
}

/// Concatenates searchable fields into one folded blob stored on each message.
pub fn build_search_text(
    sender: &str,
    sender_email: &str,
    subject: &str,
    plain_text: &str,
) -> String {
    fold_for_search(&format!("{sender} {sender_email} {subject} {plain_text}"))
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
    }
}
