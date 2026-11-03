//! Pointer and text interaction with a document, independent of any window:
//! the input types [`HtmlDocument`](crate::HtmlDocument) accepts, what lies
//! under the pointer, word boundaries and text search.

use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use std::ops::Range;
use std::sync::Mutex;

/// Mouse button of a [`PointerInput`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// What happened to the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    /// A press; `click_count` is 2 for a double click, 3 for a triple click.
    Down {
        click_count: usize,
    },
    Move,
    Up,
}

/// Keyboard modifiers held during a pointer event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// Cmd on macOS, the Windows/Super key elsewhere.
    pub platform: bool,
}

/// A pointer event in document coordinates (CSS px from the document's
/// top-left corner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerInput {
    pub x: f32,
    pub y: f32,
    pub phase: PointerPhase,
    pub button: PointerButton,
    /// Whether the primary button is held (drag selection while moving).
    pub primary_pressed: bool,
    pub modifiers: PointerModifiers,
}

/// What is under a point of the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HitInfo {
    /// Absolute URL of the enclosing link, if any.
    pub link: Option<String>,
    /// Source of the image under the point (its blocked remote URL for
    /// images withheld by the sanitizer).
    pub image: Option<String>,
    /// Whether the point is over text (selectable).
    pub text: bool,
}

/// Byte range of the word around `offset` in `text` (an offset inside a
/// character counts as that character): a run of alphanumeric characters
/// (and `_`), or the single other character at `offset`. Empty at the end of
/// the text.
pub fn word_bounds(text: &str, offset: usize) -> Range<usize> {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let Some(at) = text[offset..].chars().next() else {
        return offset..offset;
    };
    if !is_word(at) {
        return offset..offset + at.len_utf8();
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word(c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = text[offset..]
        .char_indices()
        .find(|&(_, c)| !is_word(c))
        .map_or(text.len(), |(i, _)| offset + i);
    start..end
}

/// Byte ranges of every non-overlapping, case-insensitive occurrence of
/// `needle` in `haystack`, in order. Case folding is per character and only
/// where it keeps the byte length (so ranges index `haystack` directly).
pub fn find_all(haystack: &str, needle: &str) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    let fold = |c: char| {
        let mut lower = c.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(l), None) if l.len_utf8() == c.len_utf8() => l,
            _ => c,
        }
    };
    let needle: Vec<char> = needle.chars().map(fold).collect();
    let chars: Vec<(usize, char)> = haystack.char_indices().map(|(i, c)| (i, fold(c))).collect();
    let mut matches = Vec::new();
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()]
            .iter()
            .zip(&needle)
            .all(|((_, a), b)| a == b)
        {
            let start = chars[i].0;
            let end = chars
                .get(i + needle.len())
                .map_or(haystack.len(), |&(j, _)| j);
            matches.push(start..end);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    matches
}

/// Records link activations instead of navigating: mail never navigates the
/// reader; the host decides what to do with the URL (open it in a browser).
#[derive(Default)]
pub(crate) struct LinkCapture(Mutex<Option<String>>);

impl LinkCapture {
    /// The last activated link, cleared.
    pub(crate) fn take(&self) -> Option<String> {
        self.0.lock().ok()?.take()
    }
}

impl NavigationProvider for LinkCapture {
    fn navigate_to(&self, options: NavigationOptions) {
        if let Ok(mut link) = self.0.lock() {
            let web = crate::resources::protocol_relative_web_url(&options.url);
            *link = Some(web.unwrap_or(options.url).to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_bounds_expand_over_word_characters() {
        let text = "Hello, wide_world 42!";
        assert_eq!(&text[word_bounds(text, 2)], "Hello");
        assert_eq!(&text[word_bounds(text, 9)], "wide_world");
        assert_eq!(&text[word_bounds(text, 18)], "42");
        // Punctuation and spaces select just themselves.
        assert_eq!(&text[word_bounds(text, 5)], ",");
        assert_eq!(word_bounds(text, text.len()), text.len()..text.len());
    }

    #[test]
    fn word_bounds_handle_multibyte_text() {
        let text = "olá ação";
        let start = text.find("ação").unwrap();
        assert_eq!(&text[word_bounds(text, start + 2)], "ação");
        assert_eq!(&text[word_bounds(text, 1)], "olá");
    }

    #[test]
    fn find_all_is_case_insensitive_and_non_overlapping() {
        let text = "Order ORDER order; aaa";
        let found: Vec<&str> = find_all(text, "order")
            .iter()
            .map(|r| &text[r.clone()])
            .collect();
        assert_eq!(found, ["Order", "ORDER", "order"]);
        assert_eq!(find_all("aaa", "aa"), vec![0..2usize]);
        assert!(find_all(text, "").is_empty());
        assert!(find_all(text, "missing").is_empty());
    }

    #[test]
    fn find_all_ranges_index_the_original_text() {
        let text = "Ação e AÇÃO";
        let ranges = find_all(text, "ação");
        assert_eq!(ranges.len(), 2);
        assert_eq!(&text[ranges[1].clone()], "AÇÃO");
    }

    #[test]
    fn link_capture_keeps_the_last_activation_once() {
        let capture = LinkCapture::default();
        let url = url::Url::parse("https://example.com/a").unwrap();
        capture.navigate_to(NavigationOptions::new(url, None, 0));
        assert_eq!(capture.take().as_deref(), Some("https://example.com/a"));
        assert_eq!(capture.take(), None);
    }

    #[test]
    fn a_protocol_relative_link_opens_on_the_web() {
        let capture = LinkCapture::default();
        let url = url::Url::parse("bgmail://example.com/offer").unwrap();
        capture.navigate_to(NavigationOptions::new(url, None, 0));
        assert_eq!(capture.take().as_deref(), Some("https://example.com/offer"));
    }
}
