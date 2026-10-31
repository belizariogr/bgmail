//! GPUI-free editing core of [`super::TextArea`]: text storage, selection,
//! IME marked range and the undo/redo history, plus the string helpers they
//! rely on. Everything here works on UTF-8 byte offsets and is unit-tested
//! without a window.

use std::borrow::Cow;
use std::ops::Range;

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// Upper bound on undo entries; the oldest are dropped first. Edits are stored
/// as diffs, so this bounds memory even for long quoted replies.
const MAX_UNDO_ENTRIES: usize = 1000;

/// Converts `\r\n` and lone `\r` to `\n` so offsets and line math only ever
/// see one line terminator. Borrows when there is nothing to change.
pub(crate) fn normalize_newlines(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Largest char boundary `<= offset` (and `<= text.len()`), so offsets coming
/// from layout or the platform can never split a code point.
pub(crate) fn clamp_to_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Start of the grapheme cluster before `offset` (0 at the start).
pub(crate) fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    GraphemeCursor::new(offset, text.len(), true)
        .prev_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(0)
}

/// End of the grapheme cluster at `offset` (`text.len()` at the end).
pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    GraphemeCursor::new(offset, text.len(), true)
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}

/// Byte range of the logical (`\n`-delimited) line containing `offset`,
/// excluding the terminating newline.
pub(crate) fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = clamp_to_char_boundary(text, offset);
    let start = text[..offset].rfind('\n').map_or(0, |ix| ix + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |ix| offset + ix);
    start..end
}

/// Byte range of the Unicode word segment under `offset` (double-click
/// selection). Segmentation is limited to the current line so it stays cheap
/// on long documents. At a line end, the segment before the caret is used.
pub(crate) fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    let offset = clamp_to_char_boundary(text, offset);
    let line = line_range_at(text, offset);
    if line.is_empty() {
        return offset..offset;
    }
    let local = offset - line.start;
    let line_text = &text[line.clone()];
    let mut fallback = None;
    for (start, segment) in line_text.split_word_bound_indices() {
        let end = start + segment.len();
        if local >= start && local < end {
            return line.start + start..line.start + end;
        }
        fallback = Some(line.start + start..line.start + end);
    }
    fallback.unwrap_or(offset..offset)
}

/// UTF-8 byte offset to UTF-16 code-unit offset (platform IME coordinates).
pub(crate) fn offset_to_utf16(text: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    text[..offset].chars().map(char::len_utf16).sum()
}

/// UTF-16 code-unit offset to UTF-8 byte offset, clamped to the text end.
pub(crate) fn offset_from_utf16(text: &str, offset_utf16: usize) -> usize {
    let mut utf16 = 0;
    for (ix, ch) in text.char_indices() {
        if utf16 >= offset_utf16 {
            return ix;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

/// Whether an edit may merge into the previous undo entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditKind {
    /// Typing, IME composition and single-grapheme deletes: consecutive edits
    /// coalesce into one undo step.
    Typing,
    /// Paste, cut and selection deletes: always a separate undo step.
    Other,
}

/// One reversible change: `range` (in the text *before* the edit) was replaced
/// by `new_text`; `old_text` is what it held.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edit {
    range: Range<usize>,
    old_text: String,
    new_text: String,
    selection_before: Range<usize>,
    reversed_before: bool,
}

impl Edit {
    /// Range the edit occupies in the text *after* it was applied.
    fn inserted_range(&self) -> Range<usize> {
        self.range.start..self.range.start + self.new_text.len()
    }

    /// Folds a follow-up typing edit into `self` when it touches only text
    /// `self` produced (typing, IME re-composition, backspacing what was just
    /// typed) or extends a pure deletion run. Returns `false` when unrelated.
    fn try_merge(&mut self, next: &Edit) -> bool {
        let inserted = self.inserted_range();
        if next.range.start >= inserted.start && next.range.end <= inserted.end {
            let local = next.range.start - inserted.start..next.range.end - inserted.start;
            self.new_text.replace_range(local, &next.new_text);
            return true;
        }
        if self.new_text.is_empty() && next.new_text.is_empty() {
            // Backspace run: the next deletion ends where this one started.
            if next.range.end == self.range.start {
                self.old_text.insert_str(0, &next.old_text);
                self.range.start = next.range.start;
                return true;
            }
            // Forward-delete run: the next deletion starts at the same spot.
            if next.range.start == self.range.start {
                self.old_text.push_str(&next.old_text);
                self.range.end += next.old_text.len();
                return true;
            }
        }
        false
    }
}

/// Undo/redo stacks of [`Edit`] diffs with typing coalescing.
#[derive(Debug, Default)]
struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Whether the next typing edit may merge into the top undo entry. Cleared
    /// by caret moves and non-typing edits so each "burst" is one step.
    coalesce: bool,
}

impl History {
    fn record(&mut self, edit: Edit, kind: EditKind) {
        self.redo.clear();
        let breaks_run = edit.new_text.contains('\n');
        let merged = kind == EditKind::Typing
            && self.coalesce
            && self
                .undo
                .last_mut()
                .is_some_and(|last| last.try_merge(&edit));
        if !merged {
            self.undo.push(edit);
            if self.undo.len() > MAX_UNDO_ENTRIES {
                self.undo.remove(0);
            }
        }
        self.coalesce = kind == EditKind::Typing && !breaks_run;
    }
}

/// Text + selection + IME mark + history of a multi-line editor.
///
/// Invariants: `selected_range` and `marked_range` are ordered, in bounds and
/// on char boundaries; `text` only contains `\n` line terminators.
#[derive(Debug, Default)]
pub(crate) struct TextBuffer {
    text: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    revision: u64,
    history: History,
}

impl TextBuffer {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Bumped on every text change; used as the layout cache key.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn selected_range(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    pub(crate) fn selection_reversed(&self) -> bool {
        self.selection_reversed
    }

    pub(crate) fn marked_range(&self) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    pub(crate) fn selected_text(&self) -> &str {
        &self.text[self.selected_range.clone()]
    }

    /// The moving end of the selection (where the caret is drawn).
    pub(crate) fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    /// The fixed end of the selection.
    pub(crate) fn anchor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        }
    }

    /// Replaces the whole text (normalized), resets history and puts the caret
    /// at the start. Not undoable: it represents loading a new document.
    pub(crate) fn set_text(&mut self, text: &str) {
        self.text = normalize_newlines(text).into_owned();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.history = History::default();
        self.revision += 1;
    }

    /// Collapses the selection to `offset`.
    pub(crate) fn move_to(&mut self, offset: usize) {
        let offset = clamp_to_char_boundary(&self.text, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.history.coalesce = false;
    }

    /// Moves the caret end of the selection to `offset`, keeping the anchor.
    pub(crate) fn select_to(&mut self, offset: usize) {
        let offset = clamp_to_char_boundary(&self.text, offset);
        let anchor = self.anchor();
        self.select_range(anchor, offset);
    }

    /// Selects from `anchor` to `cursor` (either order).
    pub(crate) fn select_range(&mut self, anchor: usize, cursor: usize) {
        let anchor = clamp_to_char_boundary(&self.text, anchor);
        let cursor = clamp_to_char_boundary(&self.text, cursor);
        self.selection_reversed = cursor < anchor;
        self.selected_range = anchor.min(cursor)..anchor.max(cursor);
        self.history.coalesce = false;
    }

    pub(crate) fn select_all(&mut self) {
        self.select_range(0, self.text.len());
    }

    /// Replaces `range` (or the marked range, or the selection) with `new_text`
    /// (normalized), places the caret after it and records an undo entry.
    pub(crate) fn replace(&mut self, range: Option<Range<usize>>, new_text: &str, kind: EditKind) {
        let range = self.resolve_range(range);
        let new_text = normalize_newlines(new_text);
        self.apply(range.clone(), &new_text, kind);
        let caret = range.start + new_text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    /// IME composition: replaces like [`Self::replace`], marks the inserted
    /// text and sets the selection to `selected_within` (relative to the
    /// inserted text) when given.
    pub(crate) fn replace_and_mark(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        selected_within: Option<Range<usize>>,
    ) {
        let range = self.resolve_range(range);
        let new_text = normalize_newlines(new_text);
        self.apply(range.clone(), &new_text, EditKind::Typing);
        let inserted = range.start..range.start + new_text.len();
        self.marked_range = (!inserted.is_empty()).then_some(inserted.clone());
        let selected = selected_within
            .map(|sel| {
                let start = clamp_to_char_boundary(&self.text, inserted.start + sel.start);
                let end = clamp_to_char_boundary(&self.text, inserted.start + sel.end);
                start.min(end)..start.max(end)
            })
            .unwrap_or(inserted.end..inserted.end);
        self.selected_range = selected;
        self.selection_reversed = false;
    }

    pub(crate) fn unmark(&mut self) {
        self.marked_range = None;
    }

    /// Deletes the selection, or the grapheme before the caret. Returns whether
    /// the text changed.
    pub(crate) fn backspace(&mut self) -> bool {
        if self.selected_range.is_empty() {
            let cursor = self.cursor();
            if cursor == 0 {
                return false;
            }
            let start = previous_grapheme_boundary(&self.text, cursor);
            self.replace(Some(start..cursor), "", EditKind::Typing);
        } else {
            self.replace(None, "", EditKind::Other);
        }
        true
    }

    /// Deletes the selection, or the grapheme after the caret. Returns whether
    /// the text changed.
    pub(crate) fn delete(&mut self) -> bool {
        if self.selected_range.is_empty() {
            let cursor = self.cursor();
            if cursor == self.text.len() {
                return false;
            }
            let end = next_grapheme_boundary(&self.text, cursor);
            self.replace(Some(cursor..end), "", EditKind::Typing);
        } else {
            self.replace(None, "", EditKind::Other);
        }
        true
    }

    /// Reverts the last edit and restores the selection it started from.
    /// Returns whether anything was undone.
    pub(crate) fn undo(&mut self) -> bool {
        let Some(edit) = self.history.undo.pop() else {
            return false;
        };
        self.text
            .replace_range(edit.inserted_range(), &edit.old_text);
        self.selected_range = edit.selection_before.clone();
        self.selection_reversed = edit.reversed_before;
        self.after_history_step();
        self.history.redo.push(edit);
        true
    }

    /// Re-applies the last undone edit, caret after the inserted text. Returns
    /// whether anything was redone.
    pub(crate) fn redo(&mut self) -> bool {
        let Some(edit) = self.history.redo.pop() else {
            return false;
        };
        self.text.replace_range(edit.range.clone(), &edit.new_text);
        let caret = edit.inserted_range().end;
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.after_history_step();
        self.history.undo.push(edit);
        true
    }

    fn after_history_step(&mut self) {
        self.marked_range = None;
        self.history.coalesce = false;
        self.revision += 1;
    }

    fn resolve_range(&self, range: Option<Range<usize>>) -> Range<usize> {
        let range = range
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let start = clamp_to_char_boundary(&self.text, range.start);
        let end = clamp_to_char_boundary(&self.text, range.end);
        start.min(end)..start.max(end)
    }

    fn apply(&mut self, range: Range<usize>, new_text: &str, kind: EditKind) {
        if range.is_empty() && new_text.is_empty() {
            return;
        }
        let edit = Edit {
            old_text: self.text[range.clone()].to_owned(),
            new_text: new_text.to_owned(),
            range: range.clone(),
            selection_before: self.selected_range.clone(),
            reversed_before: self.selection_reversed,
        };
        self.text.replace_range(range, new_text);
        self.history.record(edit, kind);
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(text: &str) -> TextBuffer {
        let mut buffer = TextBuffer::default();
        buffer.set_text(text);
        buffer
    }

    fn type_text(buffer: &mut TextBuffer, text: &str) {
        for ch in text.chars() {
            buffer.replace(None, ch.encode_utf8(&mut [0; 4]), EditKind::Typing);
        }
    }

    #[test]
    fn normalize_newlines_converts_crlf_and_cr() {
        assert_eq!(normalize_newlines("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert!(matches!(normalize_newlines("plain\n"), Cow::Borrowed(_)));
    }

    #[test]
    fn clamp_to_char_boundary_never_splits_code_points() {
        let text = "aé"; // é is 2 bytes at 1..3
        assert_eq!(clamp_to_char_boundary(text, 2), 1);
        assert_eq!(clamp_to_char_boundary(text, 3), 3);
        assert_eq!(clamp_to_char_boundary(text, 99), 3);
    }

    #[test]
    fn grapheme_boundaries_skip_whole_clusters() {
        let text = "a👍🏽e\u{301}b"; // emoji + skin tone, e + combining acute
        let after_emoji = 1 + "👍🏽".len();
        assert_eq!(next_grapheme_boundary(text, 1), after_emoji);
        assert_eq!(previous_grapheme_boundary(text, after_emoji), 1);
        let after_e = after_emoji + "e\u{301}".len();
        assert_eq!(next_grapheme_boundary(text, after_emoji), after_e);
        assert_eq!(previous_grapheme_boundary(text, after_e), after_emoji);
        assert_eq!(previous_grapheme_boundary(text, 0), 0);
        assert_eq!(next_grapheme_boundary(text, text.len()), text.len());
    }

    #[test]
    fn line_range_at_excludes_newline() {
        let text = "one\ntwo\n\nfour";
        assert_eq!(line_range_at(text, 0), 0..3);
        assert_eq!(line_range_at(text, 3), 0..3);
        assert_eq!(line_range_at(text, 4), 4..7);
        assert_eq!(line_range_at(text, 8), 8..8);
        assert_eq!(line_range_at(text, text.len()), 9..13);
    }

    #[test]
    fn word_range_at_selects_word_or_separator() {
        let text = "hello, world\nnext";
        assert_eq!(word_range_at(text, 2), 0..5);
        assert_eq!(word_range_at(text, 5), 5..6);
        assert_eq!(word_range_at(text, 12), 7..12, "line end picks last word");
        assert_eq!(word_range_at(text, 14), 13..17);
        assert_eq!(word_range_at("a\n\nb", 2), 2..2, "empty line");
    }

    #[test]
    fn utf16_round_trip() {
        let text = "a😀b";
        assert_eq!(offset_to_utf16(text, 0), 0);
        assert_eq!(offset_to_utf16(text, 1), 1);
        assert_eq!(offset_to_utf16(text, 5), 3);
        assert_eq!(offset_from_utf16(text, 3), 5);
        assert_eq!(offset_from_utf16(text, 1), 1);
        assert_eq!(offset_from_utf16(text, 99), text.len());
    }

    #[test]
    fn edit_try_merge_rules() {
        let edit = |range: Range<usize>, old: &str, new: &str| Edit {
            range,
            old_text: old.into(),
            new_text: new.into(),
            selection_before: 0..0,
            reversed_before: false,
        };
        let mut typed = edit(2..2, "", "ab");
        assert!(typed.try_merge(&edit(4..4, "", "c")));
        assert_eq!(typed.new_text, "abc");
        assert!(typed.try_merge(&edit(4..5, "c", "")));
        assert_eq!(typed.new_text, "ab");
        assert!(!typed.try_merge(&edit(0..1, "x", "")));

        let mut backspaces = edit(5..6, "f", "");
        assert!(backspaces.try_merge(&edit(4..5, "e", "")));
        assert_eq!(
            (backspaces.range.clone(), backspaces.old_text.as_str()),
            (4..6, "ef")
        );
        let mut deletes = edit(5..6, "f", "");
        assert!(deletes.try_merge(&edit(5..6, "g", "")));
        assert_eq!((deletes.range, deletes.old_text.as_str()), (5..7, "fg"));
    }

    #[test]
    fn history_record_coalesces_typing_until_broken() {
        let mut history = History::default();
        let edit = |at: usize, new: &str| Edit {
            range: at..at,
            old_text: String::new(),
            new_text: new.into(),
            selection_before: at..at,
            reversed_before: false,
        };
        history.record(edit(0, "a"), EditKind::Typing);
        history.record(edit(1, "b"), EditKind::Typing);
        assert_eq!(history.undo.len(), 1);
        history.record(edit(2, "\n"), EditKind::Typing);
        history.record(edit(3, "c"), EditKind::Typing);
        assert_eq!(history.undo.len(), 2, "newline closes the run");
        history.record(edit(4, "zz"), EditKind::Other);
        assert_eq!(history.undo.len(), 3);
        assert!(history.redo.is_empty());
    }

    #[test]
    fn set_text_normalizes_and_resets() {
        let mut buffer = buffer("x");
        type_text(&mut buffer, "abc");
        let revision = buffer.revision();
        buffer.set_text("a\r\nb");
        assert_eq!(buffer.text(), "a\nb");
        assert_eq!(buffer.selected_range(), 0..0);
        assert!(buffer.revision() > revision);
        assert!(!buffer.undo(), "history is reset");
    }

    #[test]
    fn cursor_anchor_and_selection_direction() {
        let mut buffer = buffer("hello");
        buffer.move_to(3);
        buffer.select_to(1);
        assert_eq!(buffer.selected_range(), 1..3);
        assert!(buffer.selection_reversed());
        assert_eq!((buffer.cursor(), buffer.anchor()), (1, 3));
        buffer.select_to(5);
        assert_eq!(buffer.selected_range(), 3..5);
        assert!(!buffer.selection_reversed());
        assert_eq!(buffer.selected_text(), "lo");
        buffer.select_range(4, 2);
        assert_eq!((buffer.cursor(), buffer.anchor()), (2, 4));
        buffer.select_all();
        assert_eq!(buffer.selected_range(), 0..5);
        buffer.move_to(99);
        assert_eq!(buffer.selected_range(), 5..5);
    }

    #[test]
    fn replace_normalizes_and_moves_caret() {
        let mut buffer = buffer("ab");
        buffer.move_to(1);
        buffer.replace(None, "x\r\ny", EditKind::Other);
        assert_eq!(buffer.text(), "ax\nyb");
        assert_eq!(buffer.selected_range(), 4..4);
        buffer.replace(Some(0..1), "", EditKind::Other);
        assert_eq!(buffer.text(), "x\nyb");
    }

    #[test]
    fn replace_and_mark_tracks_composition() {
        let mut buffer = buffer("ab");
        buffer.move_to(1);
        buffer.replace_and_mark(None, "ka", Some(2..2));
        assert_eq!(buffer.text(), "akab");
        assert_eq!(buffer.marked_range(), Some(1..3));
        assert_eq!(buffer.selected_range(), 3..3);
        // The next composition step replaces the marked text.
        buffer.replace_and_mark(None, "か", None);
        assert_eq!(buffer.text(), "aかb");
        assert_eq!(buffer.marked_range(), Some(1..4));
        buffer.replace(None, "火", EditKind::Typing);
        assert_eq!(buffer.text(), "a火b");
        assert_eq!(buffer.marked_range(), None);
        buffer.replace_and_mark(None, "x", None);
        buffer.unmark();
        assert_eq!(buffer.marked_range(), None);
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "ab", "whole composition is one undo step");
    }

    #[test]
    fn backspace_and_delete_handle_graphemes_and_edges() {
        let mut buffer = buffer("a👍🏽b");
        buffer.move_to(1 + "👍🏽".len());
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "ab");
        buffer.move_to(0);
        assert!(!buffer.backspace());
        assert!(buffer.delete());
        assert_eq!(buffer.text(), "b");
        buffer.move_to(1);
        assert!(!buffer.delete());
        buffer.select_all();
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "");
        buffer.set_text("xy");
        buffer.select_all();
        assert!(buffer.delete());
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn undo_redo_restore_text_and_selection() {
        let mut buffer = buffer("hello world");
        buffer.select_range(0, 5);
        buffer.replace(None, "bye", EditKind::Other);
        assert_eq!(buffer.text(), "bye world");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "hello world");
        assert_eq!(buffer.selected_range(), 0..5);
        assert!(buffer.redo());
        assert_eq!(buffer.text(), "bye world");
        assert_eq!(buffer.selected_range(), 3..3);
        assert!(!buffer.redo());
        assert!(buffer.undo());
        assert!(!buffer.undo());
    }

    #[test]
    fn typing_coalesces_until_caret_moves() {
        let mut buffer = buffer("");
        type_text(&mut buffer, "abc");
        buffer.backspace();
        type_text(&mut buffer, "d");
        buffer.move_to(0);
        type_text(&mut buffer, "X");
        assert_eq!(buffer.text(), "Xabd");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "abd");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut buffer = buffer("");
        type_text(&mut buffer, "a");
        buffer.undo();
        type_text(&mut buffer, "b");
        assert!(!buffer.redo());
        assert_eq!(buffer.text(), "b");
    }

    #[test]
    fn undo_stack_is_bounded() {
        let mut buffer = buffer("");
        for _ in 0..MAX_UNDO_ENTRIES + 5 {
            buffer.replace(None, "x", EditKind::Other);
        }
        let mut steps = 0;
        while buffer.undo() {
            steps += 1;
        }
        assert_eq!(steps, MAX_UNDO_ENTRIES);
        assert_eq!(buffer.text().len(), 5);
    }

    #[test]
    fn no_op_edit_is_not_recorded() {
        let mut buffer = buffer("a");
        let revision = buffer.revision();
        buffer.replace(None, "", EditKind::Other);
        assert_eq!(buffer.revision(), revision);
        assert!(!buffer.undo());
    }
}
