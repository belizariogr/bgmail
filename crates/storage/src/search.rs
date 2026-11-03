//! Accent-insensitive, case-insensitive search helpers.

use unicode_normalization::UnicodeNormalization;

/// Folds accents and lowercases text for partial SQL `LIKE` search.
pub fn fold_for_search(text: &str) -> String {
    // Without NUL too, which FTS takes for the end of the query.
    text.nfd()
        .filter(|ch| !unicode_normalization::char::is_combining_mark(*ch) && *ch != '\0')
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

const MAX_INDEXED_TEXT_BYTES: usize = 2 << 20;

/// Concatenates searchable fields into one folded blob stored on each message.
pub fn build_search_text(
    sender: &str,
    sender_email: &str,
    subject: &str,
    plain_text: &str,
) -> String {
    // As much as the reader shows. Trigrams take about 3 times the text, and all of it while
    // holding the write lock: 128 MB of text took 21 s and 2 GB during sync. Thunderbird also
    // indexes only the start of very long bodies.
    let mut end = plain_text.len().min(MAX_INDEXED_TEXT_BYTES);
    while !plain_text.is_char_boundary(end) {
        end -= 1;
    }
    let plain_text = &plain_text[..end];
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

    #[test]
    fn only_the_start_of_a_very_long_body_is_indexed() {
        let text = "é".repeat(2 << 20);
        let indexed = build_search_text("a", "b", "c", &text);
        assert!(indexed.len() < 3 << 20);
        assert!(build_search_text("a", "b", "c", "word").ends_with("word"));
    }
}
