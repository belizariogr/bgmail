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
pub mod rich;
mod rows;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    actions, fill, point, px, relative, size, App, AvailableSpace, Bounds, ClipboardItem, Context,
    CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels,
    Point, Render, ScrollHandle, SharedString, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine,
};
use rich::{Align, CharStyle, FontFamily, Format, ListKind, ParaStyle, RichText, TextSize};
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
        ToggleBold,
        ToggleItalic,
        ToggleUnderline,
    ]
);

/// Width of one indent level, in pixels.
const INDENT_STEP: f32 = 28.0;
/// Room for the marker of a list item, in pixels.
const LIST_GUTTER: f32 = 24.0;
/// Narrowest a deeply indented line may wrap at, in pixels.
const MIN_WRAP_WIDTH: f32 = 40.0;
/// Steps of formatting history kept, as the text's.
const MAX_RICH_UNDO: usize = 1000;

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
        KeyBinding::new("cmd-b", ToggleBold, CONTEXT),
        KeyBinding::new("ctrl-b", ToggleBold, CONTEXT),
        KeyBinding::new("cmd-i", ToggleItalic, CONTEXT),
        KeyBinding::new("ctrl-i", ToggleItalic, CONTEXT),
        KeyBinding::new("cmd-u", ToggleUnderline, CONTEXT),
        KeyBinding::new("ctrl-u", ToggleUnderline, CONTEXT),
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
    /// Changes with the formatting.
    format_revision: u64,
    /// `Some` while the (empty) area shows its placeholder.
    placeholder: Option<SharedString>,
    marked: Option<Range<usize>>,
    wrap_width: Option<Pixels>,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    color: Hsla,
}

/// Where a logical line goes and how it is drawn.
#[derive(Clone, Debug)]
struct LineGeometry {
    /// Start of its text, after indent and list marker.
    left: Pixels,
    /// Width it wraps and aligns in.
    width: Option<Pixels>,
    align: TextAlign,
    font_size: Pixels,
    line_height: Pixels,
    /// Top of its first row.
    top: Pixels,
    /// List marker, drawn left of its text.
    marker: Option<SharedString>,
}

/// Shaped lines + visual rows of the last layout, shared between the
/// [`TextArea`] (hit testing, vertical movement) and its element (painting).
#[derive(Default)]
struct TextLayout {
    key: Option<LayoutKey>,
    lines: Vec<WrappedLine>,
    geometry: Vec<LineGeometry>,
    rows: Vec<VisualRow>,
    /// Top of each row of `rows`.
    row_tops: Vec<Pixels>,
    height: Pixels,
    max_width: Pixels,
    /// The last measure pass had no definite width, so the height it reported
    /// may be stale; prepaint schedules one more frame.
    measured_without_width: bool,
    /// Resolved once: fonts installed don't change while the app runs.
    families: Option<Families>,
}

/// Runs of `range` of the text: its styles over the key's font and color, with
/// the IME marked range underlined.
fn line_runs(
    key: &LayoutKey,
    rich: &RichText,
    range: Range<usize>,
    families: &Families,
) -> Vec<TextRun> {
    let mut runs = Vec::new();
    for (run, style) in rich.runs_in(range.clone()) {
        let marked = key.marked.as_ref().and_then(|marked| {
            let clipped = marked.start.max(run.start)..marked.end.min(run.end);
            (clipped.start < clipped.end)
                .then(|| clipped.start - run.start..clipped.end - run.start)
        });
        runs.extend(marked_text_runs(
            run.len(),
            marked.as_ref(),
            styled_run(key, &style, families),
        ));
    }
    runs
}

/// Fonts used for text, by [`FontFamily`].
#[derive(Clone, Debug)]
pub(crate) struct Families(HashMap<FontFamily, SharedString>);

impl Families {
    /// A family can resolve to a fallback font without weights and styles, even
    /// while being listed as installed, which would lose bold and italic. So the
    /// default family needs a bold that is a different face, and the others just
    /// not to be that fallback, as some only come in one weight (Arial Black).
    fn resolve(inherited: &SharedString, window: &Window) -> Self {
        let system = window.text_system();
        let face = |family: &SharedString, weight: gpui::FontWeight| {
            let mut font = gpui::font(family.clone());
            font.weight = weight;
            system.resolve_font(&font)
        };
        let has_bold = |family: &SharedString| {
            face(family, gpui::FontWeight::NORMAL) != face(family, gpui::FontWeight::BOLD)
        };
        let fallback = face(
            &SharedString::from("BGMail No Such Font"),
            gpui::FontWeight::NORMAL,
        );
        let exists = |family: &SharedString| {
            has_bold(family) || face(family, gpui::FontWeight::NORMAL) != fallback
        };
        let pick = |family: FontFamily| {
            family
                .candidates()
                .iter()
                .map(|name| SharedString::from(*name))
                .find(|name| exists(name))
        };
        let sans = if has_bold(inherited) {
            inherited.clone()
        } else {
            FontFamily::Sans
                .candidates()
                .iter()
                .map(|name| SharedString::from(*name))
                .find(|name| has_bold(name))
                .unwrap_or_else(|| inherited.clone())
        };
        let mut families: HashMap<_, _> = FontFamily::ALL
            .into_iter()
            .map(|family| (family, pick(family).unwrap_or_else(|| sans.clone())))
            .collect();
        families.insert(FontFamily::Sans, sans);
        Self(families)
    }

    fn get(&self, family: FontFamily) -> SharedString {
        self.0.get(&family).cloned().unwrap_or_default()
    }
}

/// The installed font that shows `family`, for previews outside the editor.
pub fn font_for_family(
    family: FontFamily,
    inherited: &SharedString,
    window: &Window,
) -> SharedString {
    Families::resolve(inherited, window).get(family)
}

fn styled_run(key: &LayoutKey, style: &CharStyle, families: &Families) -> TextRun {
    let mut font = key.font.clone();
    font.family = families.get(style.family);
    if style.bold {
        font.weight = gpui::FontWeight::BOLD;
    }
    if style.italic {
        font.style = gpui::FontStyle::Italic;
    }
    let color = style.color.map_or(key.color, |rgb| gpui::rgb(rgb).into());
    TextRun {
        len: 0,
        font,
        color,
        background_color: None,
        underline: style.underline.then_some(UnderlineStyle {
            color: Some(color),
            thickness: px(1.0),
            wavy: false,
        }),
        strikethrough: None,
    }
}

/// Space a line's text starts at: its indent and the room for its list marker.
fn line_left(para: &ParaStyle) -> f32 {
    f32::from(para.indent) * INDENT_STEP
        + if para.list.is_some() {
            LIST_GUTTER
        } else {
            0.0
        }
}

impl TextLayout {
    /// Reshapes when `key` differs from the cached one. Lines are shaped one by
    /// one, each at its own size and width.
    fn ensure(
        &mut self,
        key: LayoutKey,
        source: impl FnOnce() -> (SharedString, RichText),
        window: &Window,
    ) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let (text, rich) = source();
        let families = self
            .families
            .get_or_insert_with(|| Families::resolve(&key.font.family, window))
            .clone();
        let mut lines = Vec::new();
        let mut geometry = Vec::new();
        let mut row_tops = Vec::new();
        let mut rows_input = Vec::new();
        let mut top = Pixels::ZERO;
        let mut start = 0;
        for (ix, content) in text.split('\n').enumerate() {
            let para = rich.para(ix);
            let scale = para.size.scale();
            let font_size = key.font_size * scale;
            let line_height = key.line_height * scale;
            let left = px(line_left(&para));
            let width = key
                .wrap_width
                .map(|width| (width - left).max(px(MIN_WRAP_WIDTH)));
            let runs = line_runs(&key, &rich, start..start + content.len(), &families);
            let line = window
                .text_system()
                .shape_text(
                    SharedString::from(content.to_string()),
                    font_size,
                    &runs,
                    width,
                    None,
                )
                .ok()
                .and_then(|shaped| shaped.into_iter().next());
            start += content.len() + 1;
            let Some(line) = line else {
                continue;
            };
            let wraps: Vec<usize> = line
                .wrap_boundaries()
                .iter()
                .map(|boundary| {
                    line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index
                })
                .collect();
            let row_count = wraps.iter().filter(|&&w| w > 0 && w < line.len()).count() + 1;
            for row in 0..row_count {
                row_tops.push(top + line_height * row as f32);
            }
            geometry.push(LineGeometry {
                left,
                width,
                align: match para.align {
                    Align::Left => TextAlign::Left,
                    Align::Center => TextAlign::Center,
                    Align::Right => TextAlign::Right,
                },
                font_size,
                line_height,
                top,
                marker: rich.list_marker(ix).map(SharedString::from),
            });
            rows_input.push((line.len(), wraps));
            top += line_height * row_count as f32;
            lines.push(line);
        }
        self.rows = build_rows(rows_input);
        self.row_tops = row_tops;
        self.height = top;
        self.max_width = lines
            .iter()
            .zip(&geometry)
            .map(|(line, geometry)| line.width() + geometry.left)
            .fold(Pixels::ZERO, Pixels::max);
        self.lines = lines;
        self.geometry = geometry;
        self.key = Some(key);
    }

    fn line_height(&self) -> Pixels {
        self.key.as_ref().map_or(px(16.), |key| key.line_height)
    }

    /// Line height of row `row_ix`.
    fn row_height(&self, row_ix: usize) -> Pixels {
        self.rows
            .get(row_ix)
            .and_then(|row| self.geometry.get(row.line))
            .map_or_else(|| self.line_height(), |geometry| geometry.line_height)
    }

    fn row_top(&self, row_ix: usize) -> Pixels {
        self.row_tops.get(row_ix).copied().unwrap_or(Pixels::ZERO)
    }

    fn showing_placeholder(&self) -> bool {
        self.key
            .as_ref()
            .is_none_or(|key| key.placeholder.is_some())
    }

    /// Height of the laid-out content, at least `min_lines` rows.
    fn content_height(&self, min_lines: usize) -> Pixels {
        self.height
            .max(self.line_height() * min_lines.max(1) as f32)
    }

    /// Where the text of row `row_ix` starts: its line's indent and alignment.
    fn row_left(&self, row_ix: usize) -> Pixels {
        let Some(row) = self.rows.get(row_ix) else {
            return Pixels::ZERO;
        };
        let (Some(line), Some(geometry)) = (self.lines.get(row.line), self.geometry.get(row.line))
        else {
            return Pixels::ZERO;
        };
        let layout = &line.unwrapped_layout;
        let row_width = layout.x_for_index(row.range.end - row.line_start)
            - layout.x_for_index(row.range.start - row.line_start);
        let free = geometry
            .width
            .map_or(Pixels::ZERO, |width| (width - row_width).max(Pixels::ZERO));
        geometry.left
            + match geometry.align {
                TextAlign::Left => Pixels::ZERO,
                TextAlign::Center => free / 2.,
                TextAlign::Right => free,
            }
    }

    /// X of `offset` relative to the element origin, on row `row_ix`.
    fn x_for_offset(&self, row_ix: usize, offset: usize) -> Pixels {
        let (Some(row), true) = (self.rows.get(row_ix), !self.showing_placeholder()) else {
            return Pixels::ZERO;
        };
        let Some(line) = self.lines.get(row.line) else {
            return Pixels::ZERO;
        };
        let layout = &line.unwrapped_layout;
        self.row_left(row_ix) + layout.x_for_index(offset.saturating_sub(row.line_start))
            - layout.x_for_index(row.range.start - row.line_start)
    }

    /// Caret position of `offset`, relative to the element origin.
    fn position_for_offset(&self, offset: usize) -> Point<Pixels> {
        let row_ix = row_index_for_offset(&self.rows, offset);
        point(self.x_for_offset(row_ix, offset), self.row_top(row_ix))
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
        let x = (x - self.row_left(row_ix)).max(Pixels::ZERO);
        let index = row.line_start + layout.closest_index_for_x(row_x + x);
        clamp_offset_to_row(text, row, index)
    }

    /// Caret offset under `position` (relative to the element origin).
    fn offset_for_point(&self, text: &str, position: Point<Pixels>) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let row = self
            .row_tops
            .partition_point(|top| *top <= position.y)
            .saturating_sub(1);
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
    /// Formatting of the text.
    rich: RichText,
    format_revision: u64,
    /// Style the next typed text takes, chosen with nothing selected.
    pending_style: Option<CharStyle>,
    /// `rich` before each undo step of the buffer, and after each undone one.
    rich_undo: Vec<RichText>,
    rich_redo: Vec<RichText>,
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
            rich: RichText::default(),
            format_revision: 0,
            pending_style: None,
            rich_undo: Vec::new(),
            rich_redo: Vec::new(),
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
        self.rich = RichText::new(self.buffer.text());
        self.format_revision += 1;
        self.pending_style = None;
        self.rich_undo.clear();
        self.rich_redo.clear();
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

    /// The placeholder shown while the area is empty.
    pub fn placeholder(&self) -> &SharedString {
        &self.placeholder
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

    fn display_text(&self) -> (SharedString, RichText) {
        if self.buffer.text().is_empty() {
            (self.placeholder.clone(), RichText::new(&self.placeholder))
        } else {
            (
                SharedString::from(self.buffer.text().to_owned()),
                self.rich.clone(),
            )
        }
    }

    /// Formatting at the caret, or at the start of the selection.
    pub fn format(&self) -> Format {
        let selected = self.buffer.selected_range();
        let chars = match self.pending_style {
            Some(style) => style,
            None if selected.is_empty() => self.rich.style_at(selected.start),
            // The style of the first selected character, not the one before it.
            None => self
                .rich
                .runs_in(selected.clone())
                .first()
                .map(|(_, style)| *style)
                .unwrap_or_default(),
        };
        Format {
            chars,
            para: self.rich.para(self.first_selected_line()),
        }
    }

    /// The text as e-mail HTML, when anything is formatted.
    pub fn formatted_html(&self) -> Option<String> {
        (!self.rich.is_plain()).then(|| self.rich.to_html(self.buffer.text()))
    }

    /// The text for the plain-text version of an e-mail: lists and indents
    /// written out.
    pub fn plain_export(&self) -> String {
        self.rich.to_plain(self.buffer.text())
    }

    pub fn can_undo(&self) -> bool {
        self.buffer.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.buffer.can_redo()
    }

    pub fn toggle_bold(&mut self, cx: &mut Context<Self>) {
        self.toggle_chars(|style| style.bold, |style, on| style.bold = on, cx);
    }

    pub fn toggle_italic(&mut self, cx: &mut Context<Self>) {
        self.toggle_chars(|style| style.italic, |style, on| style.italic = on, cx);
    }

    pub fn toggle_underline(&mut self, cx: &mut Context<Self>) {
        self.toggle_chars(
            |style| style.underline,
            |style, on| style.underline = on,
            cx,
        );
    }

    pub fn set_font_family(&mut self, family: FontFamily, cx: &mut Context<Self>) {
        self.update_chars(move |style| style.family = family, cx);
    }

    /// Sets the text color (`0xRRGGBB`; `None`: the page's).
    pub fn set_text_color(&mut self, color: Option<u32>, cx: &mut Context<Self>) {
        self.update_chars(move |style| style.color = color, cx);
    }

    pub fn set_text_size(&mut self, size: TextSize, cx: &mut Context<Self>) {
        self.update_paras(move |para| para.size = size, cx);
    }

    pub fn set_align(&mut self, align: Align, cx: &mut Context<Self>) {
        self.update_paras(move |para| para.align = align, cx);
    }

    /// Makes the selected lines items of `kind`, or plain lines when they are.
    pub fn toggle_list(&mut self, kind: ListKind, cx: &mut Context<Self>) {
        let list = (self.format().para.list != Some(kind)).then_some(kind);
        self.update_paras(move |para| para.list = list, cx);
    }

    /// Indents the selected lines one more (`1`) or one less (`-1`) level.
    pub fn indent(&mut self, delta: i8, cx: &mut Context<Self>) {
        self.update_paras(
            move |para| {
                para.indent = para
                    .indent
                    .saturating_add_signed(delta)
                    .min(rich::MAX_INDENT);
            },
            cx,
        );
    }

    /// Reverts the last step, of the text or its formatting.
    pub fn undo_step(&mut self, cx: &mut Context<Self>) {
        if self.buffer.undo() {
            let previous = self.rich_undo.pop();
            self.restore_rich(previous, true);
            self.after_format(cx);
        }
    }

    /// Reapplies the last undone step.
    pub fn redo_step(&mut self, cx: &mut Context<Self>) {
        if self.buffer.redo() {
            let next = self.rich_redo.pop();
            self.restore_rich(next, false);
            self.after_format(cx);
        }
    }

    /// Makes `state` the formatting, keeping the current one for the other
    /// direction. A missing or stale state falls back to no formatting rather
    /// than styling the wrong bytes.
    fn restore_rich(&mut self, state: Option<RichText>, undoing: bool) {
        let state = state
            .filter(|state| state.matches(self.buffer.text()))
            .unwrap_or_else(|| RichText::new(self.buffer.text()));
        let current = std::mem::replace(&mut self.rich, state);
        if undoing {
            self.rich_redo.push(current);
        } else {
            self.rich_undo.push(current);
        }
        self.pending_style = None;
    }

    fn first_selected_line(&self) -> usize {
        let start = self.buffer.selected_range().start;
        self.buffer.text()[..start].matches('\n').count()
    }

    /// Lines the selection touches (the caret's, without a selection).
    fn selected_lines(&self) -> Range<usize> {
        let selected = self.buffer.selected_range();
        let text = self.buffer.text();
        let first = text[..selected.start].matches('\n').count();
        first..first + text[selected].matches('\n').count() + 1
    }

    fn toggle_chars(
        &mut self,
        get: fn(&CharStyle) -> bool,
        set: fn(&mut CharStyle, bool),
        cx: &mut Context<Self>,
    ) {
        let selected = self.buffer.selected_range();
        let on = if selected.is_empty() {
            !get(&self.format().chars)
        } else {
            !self.rich.all_chars(selected, get)
        };
        self.update_chars(move |style| set(style, on), cx);
    }

    /// With a selection, changes it; with only a caret, what is typed next.
    fn update_chars(&mut self, update: impl Fn(&mut CharStyle), cx: &mut Context<Self>) {
        let selected = self.buffer.selected_range();
        if selected.is_empty() {
            let mut style = self.format().chars;
            update(&mut style);
            self.pending_style = Some(style);
            cx.notify();
            return;
        }
        self.record_format_step();
        self.rich.update_chars(selected, update);
        self.after_format(cx);
    }

    fn update_paras(&mut self, update: impl Fn(&mut ParaStyle), cx: &mut Context<Self>) {
        self.record_format_step();
        self.rich.update_paras(self.selected_lines(), update);
        self.after_format(cx);
    }

    fn record_format_step(&mut self) {
        self.buffer.add_undo_step();
        self.push_rich_undo(self.rich.clone());
    }

    fn push_rich_undo(&mut self, state: RichText) {
        self.rich_undo.push(state);
        if self.rich_undo.len() > MAX_RICH_UNDO {
            self.rich_undo.remove(0);
        }
        self.rich_redo.clear();
    }

    fn after_format(&mut self, cx: &mut Context<Self>) {
        self.format_revision += 1;
        self.autoscroll_requested = true;
        cx.notify();
        cx.emit(TextAreaEvent::Changed);
    }

    /// An empty list item: Enter there ends the list instead of adding one.
    fn ends_list_on_newline(&self) -> bool {
        let selected = self.buffer.selected_range();
        let line = self.first_selected_line();
        selected.is_empty()
            && self.rich.para(line).list.is_some()
            && line_range_at(self.buffer.text(), selected.start).is_empty()
    }

    /// Re-shapes with the last frame's style and width when the text changed
    /// since, so keyboard navigation never works on stale rows.
    fn sync_layout(&self, window: &Window) {
        let mut layout = self.layout.borrow_mut();
        let Some(mut key) = layout.key.clone() else {
            return;
        };
        key.revision = self.buffer.revision();
        key.format_revision = self.format_revision;
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
        self.pending_style = None;
        self.goal_x = None;
        self.autoscroll_requested = true;
        cx.notify();
    }

    /// Runs a buffer mutation and, when the text changed, emits
    /// [`TextAreaEvent::Changed`].
    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut TextBuffer)) {
        let revision = self.buffer.revision();
        let steps = self.buffer.undo_steps();
        let before_text = self.buffer.text().to_owned();
        let before_rich = self.rich.clone();
        let typed = self
            .pending_style
            .unwrap_or_else(|| self.rich.style_at(self.buffer.selected_range().start));
        f(&mut self.buffer);
        if self.buffer.revision() != revision {
            self.rich
                .apply_edit(&before_text, self.buffer.text(), typed);
            if self.buffer.undo_steps() != steps {
                self.push_rich_undo(before_rich);
            }
            self.format_revision += 1;
        }
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
        if self.ends_list_on_newline() {
            self.update_paras(|para| para.list = None, cx);
            return;
        }
        self.edit(cx, |buffer| buffer.replace(None, "\n", EditKind::Typing));
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.undo_step(cx);
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.redo_step(cx);
    }

    fn bold_action(&mut self, _: &ToggleBold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_bold(cx);
    }

    fn italic_action(&mut self, _: &ToggleItalic, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_italic(cx);
    }

    fn underline_action(&mut self, _: &ToggleUnderline, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_underline(cx);
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

    /// A press outside the text but on its page (e.g. below it): the caret goes
    /// to the nearest position and a selection can start from there.
    pub fn press_outside(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_mouse_down(event, window, cx);
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
            // Past the visible part, the container follows the selection.
            self.autoscroll_requested = true;
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
            element_bounds.origin
                + point(
                    end_x.max(start.x),
                    start.y + layout.row_height(row_index_for_offset(&layout.rows, range.start)),
                ),
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
            format_revision: area.format_revision,
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

        if std::mem::take(&mut layout.measured_without_width) {
            // The height was guessed without a width; lay out again now that
            // the real wrap width is known.
            let entity = self.area.entity_id();
            window.on_next_frame(move |_, cx| cx.notify(entity));
        }

        let colors = cx.theme().colors();
        let selected = area.buffer.selected_range();
        let row_origin = |row: usize| bounds.origin + point(Pixels::ZERO, layout.row_top(row));
        // A selected newline shows as a sliver past the line end.
        let newline_width = key_font_size(&layout) * 0.4;
        // Off-screen rows of a large selection (select-all on a long reply)
        // are skipped; the scroll container clips them anyway.
        let visible = window.content_mask().bounds;
        let selections = selection_spans(&layout.rows, &selected)
            .into_iter()
            .filter(|span| {
                let top = row_origin(span.row).y;
                top + layout.row_height(span.row) >= visible.top() && top <= visible.bottom()
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
                        origin + point(end_x, layout.row_height(span.row)),
                    ),
                    area.selection_color.unwrap_or(colors.element_selected),
                )
            })
            .collect();

        let caret_row = row_index_for_offset(&layout.rows, area.buffer.cursor());
        let line_height = layout.row_height(caret_row);
        let caret_size = layout
            .rows
            .get(caret_row)
            .and_then(|row| layout.geometry.get(row.line))
            .map_or_else(|| key_font_size(&layout), |geometry| geometry.font_size);
        let caret_height = (caret_size * 1.3).min(line_height);
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
        // Anywhere in the window: a selection drag may leave the text.
        let dragged = self.area.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                dragged.update(cx, |area, cx| area.on_mouse_move(event, window, cx));
            }
        });
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
            let visible = window.content_mask().bounds;
            let marker_color = layout
                .key
                .as_ref()
                .map_or_else(|| window.text_style().color, |key| key.color);
            for (line, geometry) in layout.lines.iter().zip(&layout.geometry) {
                let y = bounds.top() + geometry.top;
                let height = geometry.line_height * (line.wrap_boundaries().len() + 1) as f32;
                if y > visible.bottom() {
                    break;
                }
                if y + height < visible.top() {
                    continue;
                }
                let origin = point(bounds.left() + geometry.left, y);
                let align_bounds = geometry
                    .width
                    .map(|width| Bounds::new(origin, size(width, height)));
                line.paint(
                    origin,
                    geometry.line_height,
                    geometry.align,
                    align_bounds,
                    window,
                    cx,
                )
                .ok();
                if let Some(marker) = &geometry.marker {
                    let run = TextRun {
                        len: marker.len(),
                        font: window.text_style().font(),
                        color: marker_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window.text_system().shape_line(
                        marker.clone(),
                        geometry.font_size,
                        &[run],
                        None,
                    );
                    shaped
                        .paint(
                            point(origin.x - shaped.width - px(6.0), y),
                            geometry.line_height,
                            window,
                            cx,
                        )
                        .ok();
                }
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
            .on_action(cx.listener(Self::bold_action))
            .on_action(cx.listener(Self::italic_action))
            .on_action(cx.listener(Self::underline_action))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
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

    fn area(cx: &mut gpui::TestAppContext, text: &str) -> Entity<TextArea> {
        let area = cx.new(|cx| TextArea::new("", cx));
        area.update(cx, |area, cx| area.set_content(text, cx));
        area
    }

    struct DragHost(Entity<TextArea>);

    impl Render for DragHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone())
        }
    }

    #[gpui::test]
    fn a_drag_keeps_selecting_outside_the_text(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| theme::init(theme::Appearance::Dark, cx));
        let area = area(cx, "one\ntwo");
        let (_, cx) = cx.add_window_view(|_, _| DragHost(area.clone()));
        cx.run_until_parked();
        cx.simulate_mouse_down(
            point(px(1.0), px(1.0)),
            MouseButton::Left,
            Default::default(),
        );
        cx.simulate_mouse_move(
            point(px(5000.0), px(5000.0)),
            Some(MouseButton::Left),
            Default::default(),
        );
        cx.run_until_parked();
        area.read_with(cx, |area, _| assert_eq!(area.selected_range(), 0..7));
        cx.simulate_mouse_up(
            point(px(5000.0), px(5000.0)),
            MouseButton::Left,
            Default::default(),
        );
        cx.simulate_mouse_move(point(px(1.0), px(1.0)), None, Default::default());
        cx.run_until_parked();
        area.read_with(cx, |area, _| assert_eq!(area.selected_range(), 0..7));
    }

    #[gpui::test]
    fn formatting_a_selection_toggles_and_undoes(cx: &mut gpui::TestAppContext) {
        let area = area(cx, "hello world");
        area.update(cx, |area, cx| {
            area.buffer.select_range(0, 5);
            area.toggle_bold(cx);
            assert!(area.format().chars.bold);
            assert!(area.formatted_html().unwrap().contains("<b>hello</b>"));

            area.toggle_bold(cx);
            assert!(!area.format().chars.bold);
            assert_eq!(area.formatted_html(), None);

            area.undo_step(cx);
            assert!(area.format().chars.bold);
            area.undo_step(cx);
            assert_eq!(area.formatted_html(), None);
            assert!(!area.can_undo());
            area.redo_step(cx);
            assert!(area.formatted_html().is_some());
            assert_eq!(area.buffer.text(), "hello world");
        });
    }

    #[gpui::test]
    fn with_only_a_caret_the_style_applies_to_what_is_typed(cx: &mut gpui::TestAppContext) {
        let area = area(cx, "a");
        area.update(cx, |area, cx| {
            area.buffer.move_to(1);
            area.toggle_italic(cx);
            assert!(area.format().chars.italic);
            assert_eq!(area.formatted_html(), None);
            area.edit(cx, |buffer| buffer.replace(None, "b", EditKind::Typing));
            assert!(area.formatted_html().unwrap().contains("<i>b</i>"));
            // Moving the caret forgets a style nothing was typed with.
            area.toggle_underline(cx);
            area.move_to(0, cx);
            assert!(!area.format().chars.underline);
        });
    }

    #[gpui::test]
    fn undo_restores_the_formatting_of_deleted_text(cx: &mut gpui::TestAppContext) {
        let area = area(cx, "hello");
        area.update(cx, |area, cx| {
            area.buffer.select_range(0, 5);
            area.set_text_color(Some(0xff0000), cx);
            area.buffer.select_range(0, 5);
            area.edit(cx, |buffer| buffer.replace(None, "", EditKind::Other));
            assert_eq!(area.buffer.text(), "");
            assert!(area.rich.matches(""));
            area.undo_step(cx);
            assert_eq!(area.buffer.text(), "hello");
            assert!(area.formatted_html().unwrap().contains("#ff0000"));
        });
    }

    #[gpui::test]
    fn paragraph_styles_cover_the_selected_lines(cx: &mut gpui::TestAppContext) {
        let area = area(cx, "one\ntwo\nthree");
        area.update(cx, |area, cx| {
            area.buffer.select_range(1, 5);
            area.set_align(Align::Center, cx);
            assert_eq!(area.rich.para(0).align, Align::Center);
            assert_eq!(area.rich.para(1).align, Align::Center);
            assert_eq!(area.rich.para(2).align, Align::Left);
            area.indent(1, cx);
            area.indent(-1, cx);
            area.indent(-1, cx);
            assert_eq!(area.rich.para(0).indent, 0);
            area.set_text_size(TextSize::Huge, cx);
            assert_eq!(area.format().para.size, TextSize::Huge);
        });
    }

    #[gpui::test]
    fn lists_toggle_and_enter_on_an_empty_item_ends_them(cx: &mut gpui::TestAppContext) {
        let area = area(cx, "a");
        area.update(cx, |area, cx| {
            area.toggle_list(ListKind::Bullet, cx);
            assert_eq!(area.format().para.list, Some(ListKind::Bullet));
            area.toggle_list(ListKind::Numbered, cx);
            assert_eq!(area.format().para.list, Some(ListKind::Numbered));
            area.buffer.move_to(1);
            assert_eq!(area.plain_export(), "1. a");
            area.edit(cx, |buffer| buffer.replace(None, "\n", EditKind::Typing));
            assert_eq!(area.format().para.list, Some(ListKind::Numbered));
            assert!(area.ends_list_on_newline());
            assert_eq!(area.plain_export(), "1. a\n2. ");
        });
    }

    #[gpui::test]
    fn set_content_forgets_the_formatting(cx: &mut gpui::TestAppContext) {
        let area = area(cx, "a");
        area.update(cx, |area, cx| {
            area.buffer.select_range(0, 1);
            area.toggle_bold(cx);
            area.set_content("b", cx);
            assert_eq!(area.formatted_html(), None);
            assert!(!area.can_undo());
        });
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
