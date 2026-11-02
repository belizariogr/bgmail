//! Multi-line plain-text editor backed by GPUI's platform input handler.
//!
//! Sibling of [`crate::TextInput`] for long text (the compose body): soft
//! wraps to the element width, grows with its content (put it inside a
//! scrollable container) and keeps undo/redo history. The GPUI-free editing
//! core lives in [`buffer`] (text, selection, history) and [`rows`] (visual
//! row math) so it can be unit-tested without a window.
//!
//! ```ignore
//! use ui::{TextArea, TextAreaEvent};
//!
//! // At startup (once): `ui::bind_keys(cx)` registers TextInput + TextArea keys.
//! let scroll = ScrollHandle::new();
//! let body = cx.new(|cx| {
//!     let mut area = TextArea::new("Write your message…", cx).min_lines(8);
//!     area.track_scroll(&scroll); // keep the caret visible while typing
//!     area
//! });
//! cx.subscribe(&body, |this, body, _: &TextAreaEvent, cx| {
//!     this.draft_body = body.read(cx).content();
//! })
//! .detach();
//! // In render: div().id("body").overflow_y_scroll().track_scroll(&scroll).child(body.clone())
//! ```

mod buffer;
mod rows;

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    actions, fill, point, px, relative, size, App, AvailableSpace, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, Font, GlobalElementId, Hsla, KeyBinding, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, ScrollHandle, SharedString,
    Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
};
use theme::ActiveTheme;

use crate::prelude::*;
use crate::text_input::{
    focus_next_field, focus_previous_field, next_word_start, previous_word_start,
    set_native_text_focus, FocusNext, FocusPrevious,
};

pub(crate) use buffer::word_range_at;
use buffer::{
    line_range_at, next_grapheme_boundary, offset_from_utf16, offset_to_utf16,
    previous_grapheme_boundary, EditKind, TextBuffer,
};
use rows::{
    build_rows, clamp_offset_to_row, row_caret_end, row_index_for_offset, selection_spans,
    vertical_target_row, VisualRow,
};

const KEY_CONTEXT: &str = "TextArea";

actions!(
    text_area,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        Home,
        End,
        MoveToBeginning,
        MoveToEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectHome,
        SelectEnd,
        SelectToBeginning,
        SelectToEnd,
        SelectAll,
        Newline,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
    ]
);

/// Registers the key bindings consumed by [`TextArea`] (key context
/// `"TextArea"`). Called by [`crate::bind_keys`]; Tab moves focus between fields
/// and never inserts a tab character.
pub fn bind_keys(cx: &mut App) {
    const CONTEXT: Option<&str> = Some(KEY_CONTEXT);

    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, CONTEXT),
        KeyBinding::new("shift-tab", FocusPrevious, CONTEXT),
        KeyBinding::new("backspace", Backspace, CONTEXT),
        KeyBinding::new("shift-backspace", Backspace, CONTEXT),
        KeyBinding::new("delete", Delete, CONTEXT),
        KeyBinding::new("left", Left, CONTEXT),
        KeyBinding::new("right", Right, CONTEXT),
        KeyBinding::new("up", Up, CONTEXT),
        KeyBinding::new("down", Down, CONTEXT),
        KeyBinding::new("shift-left", SelectLeft, CONTEXT),
        KeyBinding::new("shift-right", SelectRight, CONTEXT),
        KeyBinding::new("shift-up", SelectUp, CONTEXT),
        KeyBinding::new("shift-down", SelectDown, CONTEXT),
        KeyBinding::new("home", Home, CONTEXT),
        KeyBinding::new("end", End, CONTEXT),
        KeyBinding::new("shift-home", SelectHome, CONTEXT),
        KeyBinding::new("shift-end", SelectEnd, CONTEXT),
        KeyBinding::new("enter", Newline, CONTEXT),
        KeyBinding::new("shift-enter", Newline, CONTEXT),
        KeyBinding::new("cmd-a", SelectAll, CONTEXT),
        KeyBinding::new("cmd-v", Paste, CONTEXT),
        KeyBinding::new("ctrl-v", Paste, CONTEXT),
        KeyBinding::new("cmd-c", Copy, CONTEXT),
        KeyBinding::new("ctrl-c", Copy, CONTEXT),
        KeyBinding::new("cmd-x", Cut, CONTEXT),
        KeyBinding::new("ctrl-x", Cut, CONTEXT),
        KeyBinding::new("cmd-z", Undo, CONTEXT),
        KeyBinding::new("cmd-shift-z", Redo, CONTEXT),
    ]);

    // Windows/Linux: Ctrl for words, document edges and history (Zed default-windows).
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-a", SelectAll, CONTEXT),
        KeyBinding::new("ctrl-z", Undo, CONTEXT),
        KeyBinding::new("ctrl-y", Redo, CONTEXT),
        KeyBinding::new("ctrl-shift-z", Redo, CONTEXT),
        KeyBinding::new("ctrl-left", WordLeft, CONTEXT),
        KeyBinding::new("ctrl-right", WordRight, CONTEXT),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, CONTEXT),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, CONTEXT),
        KeyBinding::new("ctrl-home", MoveToBeginning, CONTEXT),
        KeyBinding::new("ctrl-end", MoveToEnd, CONTEXT),
        KeyBinding::new("ctrl-shift-home", SelectToBeginning, CONTEXT),
        KeyBinding::new("ctrl-shift-end", SelectToEnd, CONTEXT),
    ]);

    // macOS: Option for words, Command for line/document (Zed default-macos).
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("alt-left", WordLeft, CONTEXT),
        KeyBinding::new("alt-right", WordRight, CONTEXT),
        KeyBinding::new("alt-shift-left", SelectWordLeft, CONTEXT),
        KeyBinding::new("alt-shift-right", SelectWordRight, CONTEXT),
        KeyBinding::new("cmd-left", Home, CONTEXT),
        KeyBinding::new("cmd-right", End, CONTEXT),
        KeyBinding::new("cmd-shift-left", SelectHome, CONTEXT),
        KeyBinding::new("cmd-shift-right", SelectEnd, CONTEXT),
        KeyBinding::new("ctrl-a", Home, CONTEXT),
        KeyBinding::new("ctrl-e", End, CONTEXT),
        KeyBinding::new("ctrl-shift-a", SelectHome, CONTEXT),
        KeyBinding::new("ctrl-shift-e", SelectEnd, CONTEXT),
        KeyBinding::new("cmd-up", MoveToBeginning, CONTEXT),
        KeyBinding::new("cmd-down", MoveToEnd, CONTEXT),
        KeyBinding::new("cmd-home", MoveToBeginning, CONTEXT),
        KeyBinding::new("cmd-end", MoveToEnd, CONTEXT),
        KeyBinding::new("cmd-shift-up", SelectToBeginning, CONTEXT),
        KeyBinding::new("cmd-shift-down", SelectToEnd, CONTEXT),
        KeyBinding::new("cmd-shift-home", SelectToBeginning, CONTEXT),
        KeyBinding::new("cmd-shift-end", SelectToEnd, CONTEXT),
    ]);
}

/// Splits one text run into up to three so the IME marked range is
/// underlined. Shared with [`crate::TextInput`].
pub(crate) fn marked_text_runs(
    len: usize,
    marked: Option<&Range<usize>>,
    run: TextRun,
) -> Vec<TextRun> {
    let Some(marked) = marked.filter(|marked| marked.end <= len) else {
        return vec![TextRun { len, ..run }];
    };
    let underline = UnderlineStyle {
        color: Some(run.color),
        thickness: px(1.0),
        wavy: false,
    };
    [
        TextRun {
            len: marked.start,
            ..run.clone()
        },
        TextRun {
            len: marked.end - marked.start,
            underline: Some(underline),
            ..run.clone()
        },
        TextRun {
            len: len - marked.end,
            ..run
        },
    ]
    .into_iter()
    .filter(|run| run.len > 0)
    .collect()
}

/// Events emitted by [`TextArea`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAreaEvent {
    /// The user changed the text (typing, IME, paste, cut, delete, undo/redo).
    /// Programmatic `set_content` / `clear` do not emit it.
    Changed,
}

/// Everything that affects shaping. When it is unchanged the cached wrapped
/// lines are reused, so idle frames and caret moves never reshape.
#[derive(Clone, Debug, PartialEq)]
struct LayoutKey {
    revision: u64,
    /// `Some` while the (empty) area shows its placeholder.
    placeholder: Option<SharedString>,
    marked: Option<Range<usize>>,
    wrap_width: Option<Pixels>,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    color: Hsla,
}

/// Shaped lines + visual rows of the last layout, shared between the
/// [`TextArea`] (hit testing, vertical movement) and its element (painting).
#[derive(Default)]
struct TextLayout {
    key: Option<LayoutKey>,
    lines: Vec<WrappedLine>,
    rows: Vec<VisualRow>,
    max_width: Pixels,
    /// The last measure pass had no definite width, so the height it reported
    /// may be stale; prepaint schedules one more frame.
    measured_without_width: bool,
}

impl TextLayout {
    /// Reshapes `text()` when `key` differs from the cached one.
    fn ensure(&mut self, key: LayoutKey, text: impl FnOnce() -> SharedString, window: &Window) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let text = text();
        let run = TextRun {
            len: text.len(),
            font: key.font.clone(),
            color: key.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = marked_text_runs(text.len(), key.marked.as_ref(), run);
        let lines: Vec<WrappedLine> = window
            .text_system()
            .shape_text(text, key.font_size, &runs, key.wrap_width, None)
            .map(|lines| lines.into_iter().collect())
            .unwrap_or_default();
        self.rows = build_rows(lines.iter().map(|line| {
            let wraps = line
                .wrap_boundaries()
                .iter()
                .map(|boundary| {
                    line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
                })
                .collect();
            (line.len(), wraps)
        }));
        self.max_width = lines
            .iter()
            .map(|line| line.width())
            .fold(Pixels::ZERO, Pixels::max);
        self.lines = lines;
        self.key = Some(key);
    }

    fn line_height(&self) -> Pixels {
        self.key.as_ref().map_or(px(16.), |key| key.line_height)
    }

    fn showing_placeholder(&self) -> bool {
        self.key
            .as_ref()
            .is_none_or(|key| key.placeholder.is_some())
    }

    /// Height of the laid-out content, at least `min_lines` rows.
    fn content_height(&self, min_lines: usize) -> Pixels {
        self.line_height() * self.rows.len().max(min_lines).max(1) as f32
    }

    /// X of `offset` relative to the start of row `row_ix`.
    fn x_for_offset(&self, row_ix: usize, offset: usize) -> Pixels {
        let (Some(row), true) = (self.rows.get(row_ix), !self.showing_placeholder()) else {
            return Pixels::ZERO;
        };
        let Some(line) = self.lines.get(row.line) else {
            return Pixels::ZERO;
        };
        let layout = &line.unwrapped_layout;
        layout.x_for_index(offset.saturating_sub(row.line_start))
            - layout.x_for_index(row.range.start - row.line_start)
    }

    /// Caret position of `offset`, relative to the element origin.
    fn position_for_offset(&self, offset: usize) -> Point<Pixels> {
        let row_ix = row_index_for_offset(&self.rows, offset);
        point(
            self.x_for_offset(row_ix, offset),
            self.line_height() * row_ix as f32,
        )
    }

    /// Closest caret offset to `x` on row `row_ix`.
    fn offset_for_x(&self, text: &str, row_ix: usize, x: Pixels) -> usize {
        if self.showing_placeholder() {
            return 0;
        }
        let Some(row) = self.rows.get(row_ix) else {
            return text.len();
        };
        let Some(line) = self.lines.get(row.line) else {
            return text.len();
        };
        let layout = &line.unwrapped_layout;
        let row_x = layout.x_for_index(row.range.start - row.line_start);
        let index = row.line_start + layout.closest_index_for_x(row_x + x.max(Pixels::ZERO));
        clamp_offset_to_row(text, row, index)
    }

    /// Caret offset under `position` (relative to the element origin).
    fn offset_for_point(&self, text: &str, position: Point<Pixels>) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let row = (position.y / self.line_height()).floor().max(0.) as usize;
        self.offset_for_x(text, row.min(self.rows.len() - 1), position.x)
    }
}

/// Multi-line editable text area (compose body). Grows with its content.
pub struct TextArea {
    focus_handle: FocusHandle,
    buffer: TextBuffer,
    placeholder: SharedString,
    min_lines: usize,
    layout: Rc<RefCell<TextLayout>>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    /// Preferred x for consecutive Up/Down presses, so the caret returns to
    /// its column after crossing shorter lines.
    goal_x: Option<Pixels>,
    scroll_handle: Option<ScrollHandle>,
    /// Selection highlight when it isn't the theme's: the text may sit on a page
    /// of another color.
    selection_color: Option<Hsla>,
    /// Set by keyboard moves/edits; consumed by the next paint to scroll the
    /// tracked container so the caret stays visible.
    autoscroll_requested: bool,
}

impl EventEmitter<TextAreaEvent> for TextArea {}

impl TextArea {
    /// Creates an empty area with the given placeholder and a one-line minimum height.
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            buffer: TextBuffer::default(),
            placeholder: placeholder.into(),
            min_lines: 1,
            layout: Rc::default(),
            last_bounds: None,
            is_selecting: false,
            goal_x: None,
            scroll_handle: None,
            selection_color: None,
            autoscroll_requested: false,
        }
    }

    /// Builder: minimum height in rows, so an empty body is still a comfortable
    /// click target.
    pub fn min_lines(mut self, min_lines: usize) -> Self {
        self.min_lines = min_lines.max(1);
        self
    }

    /// Changes the minimum height in rows.
    pub fn set_min_lines(&mut self, min_lines: usize, cx: &mut Context<Self>) {
        self.min_lines = min_lines.max(1);
        cx.notify();
    }

    /// Scrolls `handle`'s container (the scrollable parent this area lives in)
    /// to keep the caret visible after keyboard moves and edits.
    pub fn track_scroll(&mut self, handle: &ScrollHandle) {
        self.scroll_handle = Some(handle.clone());
    }

    /// Sets the selection highlight; `None` follows the theme.
    pub fn set_selection_color(&mut self, color: Option<Hsla>, cx: &mut Context<Self>) {
        self.selection_color = color;
        cx.notify();
    }

    /// Current text (always `\n` line endings).
    pub fn content(&self) -> SharedString {
        SharedString::from(self.buffer.text().to_owned())
    }

    /// Whether the text is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.text().is_empty()
    }

    /// Replaces the text (normalizing `\r\n`), moves the caret to the start and
    /// resets undo history. Does not emit [`TextAreaEvent::Changed`].
    pub fn set_content(&mut self, text: impl AsRef<str>, cx: &mut Context<Self>) {
        self.buffer.set_text(text.as_ref());
        self.goal_x = None;
        self.autoscroll_requested = true;
        cx.notify();
    }

    /// Clears the text and history. Does not emit [`TextAreaEvent::Changed`].
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_content("", cx);
    }

    /// Moves the caret to `offset` (UTF-8 bytes, clamped to the text) and
    /// collapses the selection — e.g. to start typing above a quoted reply.
    pub fn set_cursor_offset(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.move_to(offset, cx);
    }

    /// Updates the placeholder shown while the area is empty.
    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }

    /// Caret offset (UTF-8 bytes).
    pub fn cursor_offset(&self) -> usize {
        self.buffer.cursor()
    }

    /// Selected byte range (empty when there is only a caret).
    pub fn selected_range(&self) -> Range<usize> {
        self.buffer.selected_range()
    }

    fn display_text(&self) -> SharedString {
        if self.buffer.text().is_empty() {
            self.placeholder.clone()
        } else {
            SharedString::from(self.buffer.text().to_owned())
        }
    }

    /// Re-shapes with the last frame's style and width when the text changed
    /// since, so keyboard navigation never works on stale rows.
    fn sync_layout(&self, window: &Window) {
        let mut layout = self.layout.borrow_mut();
        let Some(mut key) = layout.key.clone() else {
            return;
        };
        key.revision = self.buffer.revision();
        key.marked = self.buffer.marked_range();
        if self.buffer.text().is_empty() {
            key.placeholder
                .get_or_insert_with(|| self.placeholder.clone());
        } else {
            key.placeholder = None;
        }
        layout.ensure(key, || self.display_text(), window);
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buffer.move_to(offset);
        self.after_move(cx);
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buffer.select_to(offset);
        self.after_move(cx);
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.autoscroll_requested = true;
        cx.notify();
    }

    /// Runs a buffer mutation and, when the text changed, emits
    /// [`TextAreaEvent::Changed`].
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut TextBuffer)) {
        let revision = self.buffer.revision();
        f(&mut self.buffer);
        self.after_move(cx);
        if self.buffer.revision() != revision {
            cx.emit(TextAreaEvent::Changed);
        }
    }

    /// Target of a horizontal move: with a selection, its edge; otherwise
    /// `step` applied to the caret.
    fn collapse_or(&self, to_start: bool, step: impl FnOnce(&str, usize) -> usize) -> usize {
        let selected = self.buffer.selected_range();
        if selected.is_empty() {
            step(self.buffer.text(), self.buffer.cursor())
        } else if to_start {
            selected.start
        } else {
            selected.end
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.collapse_or(true, previous_grapheme_boundary);
        self.move_to(offset, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.collapse_or(false, next_grapheme_boundary);
        self.move_to(offset, cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.collapse_or(true, previous_word_start);
        self.move_to(offset, cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.collapse_or(false, next_word_start);
        self.move_to(offset, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let offset = previous_grapheme_boundary(self.buffer.text(), self.buffer.cursor());
        self.select_to(offset, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let offset = next_grapheme_boundary(self.buffer.text(), self.buffer.cursor());
        self.select_to(offset, cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let offset = previous_word_start(self.buffer.text(), self.buffer.cursor());
        self.select_to(offset, cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let offset = next_word_start(self.buffer.text(), self.buffer.cursor());
        self.select_to(offset, cx);
    }

    /// Start (`end == false`) or caret end of the visual row holding the caret.
    fn row_edge(&self, end: bool, window: &Window) -> usize {
        self.sync_layout(window);
        let layout = self.layout.borrow();
        let text = self.buffer.text();
        let cursor = self.buffer.cursor();
        if layout.showing_placeholder() || layout.rows.is_empty() {
            // No layout yet: fall back to the logical line.
            let line = line_range_at(text, cursor);
            return if end { line.end } else { line.start };
        }
        let row = &layout.rows[row_index_for_offset(&layout.rows, cursor)];
        if end {
            row_caret_end(text, row)
        } else {
            row.range.start
        }
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.row_edge(false, window);
        self.move_to(offset, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.row_edge(true, window);
        self.move_to(offset, cx);
    }

    fn select_home(&mut self, _: &SelectHome, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.row_edge(false, window);
        self.select_to(offset, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.row_edge(true, window);
        self.select_to(offset, cx);
    }

    fn move_to_beginning(&mut self, _: &MoveToBeginning, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn move_to_end(&mut self, _: &MoveToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.buffer.text().len(), cx);
    }

    fn select_to_beginning(
        &mut self,
        _: &SelectToBeginning,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.buffer.text().len(), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        self.after_move(cx);
    }

    /// Moves the caret `delta` visual rows, keeping the preferred column.
    fn move_vertically(
        &mut self,
        delta: isize,
        select: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_layout(window);
        let selected = self.buffer.selected_range();
        let origin = match (select || selected.is_empty(), delta < 0) {
            (true, _) => self.buffer.cursor(),
            (false, true) => selected.start,
            (false, false) => selected.end,
        };
        let (target, goal_x) = {
            let layout = self.layout.borrow();
            let text = self.buffer.text();
            let row = row_index_for_offset(&layout.rows, origin);
            let goal_x = self
                .goal_x
                .unwrap_or_else(|| layout.x_for_offset(row, origin));
            let target = match vertical_target_row(layout.rows.len(), row, delta) {
                Some(target_row) => layout.offset_for_x(text, target_row, goal_x),
                None if delta < 0 => 0,
                None => text.len(),
            };
            (target, goal_x)
        };
        if select {
            self.select_to(target, cx);
        } else {
            self.move_to(target, cx);
        }
        self.goal_x = Some(goal_x);
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1, false, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1, false, window, cx);
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1, true, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1, true, window, cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |buffer| {
            buffer.backspace();
        });
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |buffer| {
            buffer.delete();
        });
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |buffer| buffer.replace(None, "\n", EditKind::Typing));
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |buffer| {
            buffer.undo();
        });
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.edit(cx, |buffer| {
            buffer.redo();
        });
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit(cx, |buffer| buffer.replace(None, &text, EditKind::Other));
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.selected_range().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.selected_text().to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.selected_range().is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.selected_text().to_owned(),
            ));
            self.edit(cx, |buffer| buffer.replace(None, "", EditKind::Other));
        }
    }

    fn offset_for_mouse(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        self.layout
            .borrow()
            .offset_for_point(self.buffer.text(), position - bounds.origin)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        set_native_text_focus(window);
        window.focus(&self.focus_handle);
        cx.stop_propagation();
        self.is_selecting = true;
        let offset = self.offset_for_mouse(event.position);
        let text = self.buffer.text();
        match event.click_count {
            2 => {
                let word = word_range_at(text, offset);
                self.buffer.select_range(word.start, word.end);
            }
            count if count >= 3 => {
                let line = line_range_at(text, offset);
                // Include the newline so deleting a triple-clicked line removes it.
                let end = (line.end + 1).min(text.len());
                self.buffer.select_range(line.start, end);
            }
            _ if event.modifiers.shift => self.buffer.select_to(offset),
            _ => self.buffer.move_to(offset),
        }
        self.goal_x = None;
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting && event.dragging() {
            let offset = self.offset_for_mouse(event.position);
            self.buffer.select_to(offset);
            self.goal_x = None;
            cx.notify();
        }
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.buffer.text();
        offset_to_utf16(text, range.start)..offset_to_utf16(text, range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let text = self.buffer.text();
        offset_from_utf16(text, range.start)..offset_from_utf16(text, range.end)
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.buffer.text()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.buffer.selected_range()),
            reversed: self.buffer.selection_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.buffer
            .marked_range()
            .map(|range| self.range_to_utf16(&range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.unmark();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Tab is bound to focus navigation; a platform may still deliver it as
        // text input (e.g. through an IME).
        if new_text == "\t" {
            return;
        }
        let range = range_utf16.map(|range| self.range_from_utf16(&range));
        self.edit(cx, |buffer| {
            buffer.replace(range, new_text, EditKind::Typing)
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| self.range_from_utf16(&range));
        // The selection is relative to `new_text`, so convert within it.
        let selected = new_selected_range_utf16.map(|sel| {
            offset_from_utf16(new_text, sel.start)..offset_from_utf16(new_text, sel.end)
        });
        self.edit(cx, |buffer| {
            buffer.replace_and_mark(range, new_text, selected)
        });
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.sync_layout(window);
        let range = self.range_from_utf16(&range_utf16);
        let layout = self.layout.borrow();
        let start = layout.position_for_offset(range.start);
        let end = layout.position_for_offset(range.end);
        let end_x = if end.y == start.y { end.x } else { start.x };
        Some(Bounds::from_corners(
            element_bounds.origin + start,
            element_bounds.origin + point(end_x.max(start.x), start.y + layout.line_height()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.sync_layout(window);
        let offset = self.offset_for_mouse(point);
        Some(offset_to_utf16(self.buffer.text(), offset))
    }
}

struct TextAreaElement {
    area: Entity<TextArea>,
}

struct PrepaintState {
    selections: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}

impl TextAreaElement {
    /// Layout key for the current state and inherited text style.
    fn layout_key(&self, wrap_width: Option<Pixels>, window: &Window, cx: &App) -> LayoutKey {
        let area = self.area.read(cx);
        let style = window.text_style();
        let empty = area.buffer.text().is_empty();
        LayoutKey {
            revision: area.buffer.revision(),
            placeholder: empty.then(|| area.placeholder.clone()),
            marked: area.buffer.marked_range(),
            wrap_width,
            font: style.font(),
            font_size: style.font_size.to_pixels(window.rem_size()),
            line_height: window.line_height(),
            color: if empty {
                cx.theme().colors().text_muted
            } else {
                style.color
            },
        }
    }
}

impl IntoElement for TextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let base_key = self.layout_key(None, window, cx);
        let area = self.area.clone();
        let (layout, min_lines) = {
            let area = area.read(cx);
            (area.layout.clone(), area.min_lines)
        };
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let layout_id =
            window.request_measured_layout(style, move |known, available, window, cx| {
                let wrap_width = known.width.or(match available.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let mut layout = layout.borrow_mut();
                // Only shape for a real width: min/max-content probes would
                // otherwise reshape the whole document with a different key.
                layout.measured_without_width = wrap_width.is_none();
                if let Some(width) = wrap_width {
                    let key = LayoutKey {
                        wrap_width: Some(width),
                        ..base_key.clone()
                    };
                    layout.ensure(key, || area.read(cx).display_text(), window);
                }
                let width = wrap_width.unwrap_or(match available.width {
                    AvailableSpace::MaxContent => layout.max_width,
                    _ => Pixels::ZERO,
                });
                size(width, layout.content_height(min_lines))
            });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let key = self.layout_key(Some(bounds.size.width), window, cx);
        let area = self.area.read(cx);
        let mut layout = area.layout.borrow_mut();
        layout.ensure(key, || area.display_text(), window);

        let line_height = layout.line_height();
        if std::mem::take(&mut layout.measured_without_width) {
            // The height was guessed without a width; lay out again now that
            // the real wrap width is known.
            let entity = self.area.entity_id();
            window.on_next_frame(move |_, cx| cx.notify(entity));
        }

        let colors = cx.theme().colors();
        let selected = area.buffer.selected_range();
        let row_origin = |row: usize| bounds.origin + point(Pixels::ZERO, line_height * row as f32);
        // A selected newline shows as a sliver past the line end.
        let newline_width = key_font_size(&layout) * 0.4;
        // Off-screen rows of a large selection (select-all on a long reply)
        // are skipped; the scroll container clips them anyway.
        let visible = window.content_mask().bounds;
        let selections = selection_spans(&layout.rows, &selected)
            .into_iter()
            .filter(|span| {
                let top = row_origin(span.row).y;
                top + line_height >= visible.top() && top <= visible.bottom()
            })
            .map(|span| {
                let origin = row_origin(span.row);
                let start_x = layout.x_for_offset(span.row, span.range.start);
                let mut end_x = layout.x_for_offset(span.row, span.range.end);
                if span.includes_newline {
                    end_x += newline_width;
                }
                fill(
                    Bounds::from_corners(
                        origin + point(start_x, Pixels::ZERO),
                        origin + point(end_x, line_height),
                    ),
                    area.selection_color.unwrap_or(colors.element_selected),
                )
            })
            .collect();

        let caret_height = (key_font_size(&layout) * 1.3).min(line_height);
        let caret_origin = bounds.origin
            + layout.position_for_offset(area.buffer.cursor())
            + point(Pixels::ZERO, (line_height - caret_height) / 2.);
        let caret_bounds = Bounds::new(caret_origin, size(px(2.), caret_height));
        let cursor = selected
            .is_empty()
            .then(|| fill(caret_bounds, colors.accent));

        if area.autoscroll_requested {
            if let Some(handle) = area.scroll_handle.as_ref() {
                scroll_into_view(handle, caret_bounds, line_height, window);
            }
        }

        PrepaintState { selections, cursor }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, layout) = {
            let area = self.area.read(cx);
            (area.focus_handle.clone(), area.layout.clone())
        };
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        // Only the focused field shows its selection, as there is one at a time.
        if focus_handle.is_focused(window) {
            for selection in prepaint.selections.drain(..) {
                window.paint_quad(selection);
            }
        }

        // Only paint lines inside the visible clip: long quoted replies can
        // have thousands of rows of which a screenful is on screen.
        {
            let layout = layout.borrow();
            let line_height = layout.line_height();
            let visible = window.content_mask().bounds;
            let mut y = bounds.top();
            for line in &layout.lines {
                let height = line_height * (line.wrap_boundaries().len() + 1) as f32;
                if y > visible.bottom() {
                    break;
                }
                if y + height >= visible.top() {
                    line.paint(
                        point(bounds.left(), y),
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
                }
                y += height;
            }
        }

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.area.update(cx, |area, _cx| {
            area.last_bounds = Some(bounds);
            area.autoscroll_requested = false;
        });
    }
}

fn key_font_size(layout: &TextLayout) -> Pixels {
    layout.key.as_ref().map_or(px(14.), |key| key.font_size)
}

/// Adjusts `handle`'s offset so `target` (window coordinates) is inside its
/// viewport, with one line of breathing room, and schedules a redraw.
fn scroll_into_view(
    handle: &ScrollHandle,
    target: Bounds<Pixels>,
    margin: Pixels,
    window: &Window,
) {
    let viewport = handle.bounds();
    if viewport.size.height <= Pixels::ZERO {
        return;
    }
    let delta = scroll_delta(
        (viewport.top(), viewport.bottom()),
        (target.top(), target.bottom()),
        margin,
    );
    if delta != Pixels::ZERO {
        let mut offset = handle.offset();
        let max = handle.max_offset().height;
        offset.y = (offset.y + delta).min(Pixels::ZERO).max(-max);
        handle.set_offset(offset);
        window.request_animation_frame();
    }
}

/// Signed offset change (GPUI scroll offsets are <= 0) that brings the
/// vertical span `target` inside `viewport`, keeping `margin` where possible.
fn scroll_delta(viewport: (Pixels, Pixels), target: (Pixels, Pixels), margin: Pixels) -> Pixels {
    let (top, bottom) = viewport;
    let margin = margin.min((bottom - top - (target.1 - target.0)).max(Pixels::ZERO) / 2.);
    if target.0 - margin < top {
        top - (target.0 - margin)
    } else if target.1 + margin > bottom {
        bottom - (target.1 + margin)
    } else {
        Pixels::ZERO
    }
}

impl Render for TextArea {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(focus_next_field)
            .on_action(focus_previous_field)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::move_to_beginning))
            .on_action(cx.listener(Self::move_to_end))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::select_to_beginning))
            .on_action(cx.listener(Self::select_to_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextAreaElement { area: cx.entity() })
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{font, hsla};

    fn base_run() -> TextRun {
        TextRun {
            len: 0,
            font: font("Test"),
            color: hsla(0., 0., 0., 1.),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn marked_text_runs_underlines_only_marked_range() {
        let runs = marked_text_runs(10, Some(&(2..5)), base_run());
        let lens: Vec<_> = runs.iter().map(|run| run.len).collect();
        assert_eq!(lens, vec![2, 3, 5]);
        assert!(runs[1].underline.is_some() && runs[0].underline.is_none());

        let runs = marked_text_runs(4, Some(&(0..4)), base_run());
        assert_eq!(runs.len(), 1);
        assert!(runs[0].underline.is_some());

        let runs = marked_text_runs(4, None, base_run());
        assert_eq!((runs.len(), runs[0].len), (1, 4));
        // A stale mark past the end is ignored instead of producing bad runs.
        assert_eq!(marked_text_runs(4, Some(&(2..9)), base_run()).len(), 1);
    }

    #[test]
    fn scroll_delta_keeps_target_in_viewport() {
        let viewport = (px(100.), px(300.));
        let margin = px(20.);
        assert_eq!(scroll_delta(viewport, (px(150.), px(170.)), margin), px(0.));
        // Below: scroll content up (negative delta).
        assert_eq!(
            scroll_delta(viewport, (px(290.), px(310.)), margin),
            px(-30.)
        );
        // Above: scroll content down (positive delta).
        assert_eq!(scroll_delta(viewport, (px(90.), px(110.)), margin), px(30.));
        // A viewport too small for the margin still shows the target.
        assert_eq!(
            scroll_delta((px(0.), px(20.)), (px(30.), px(50.)), margin),
            px(-30.)
        );
    }
}
