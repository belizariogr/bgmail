//! GPUI-free model of formatted text: character styles and paragraph styles over
//! the plain text of a [`super::buffer::TextBuffer`], and its export to e-mail.
//!
//! Line breaks live in the text; the model only knows its byte lengths and which
//! line is which. Sizes belong to paragraphs because GPUI shapes a line at one
//! font size. An image is an [`IMAGE_CHAR`] alone on its line, styled with the
//! image it shows: GPUI can't shape an object inside a line of text.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// The character an image stands for in the text.
pub const IMAGE_CHAR: char = '\u{FFFC}';
const IMAGE_LEN: usize = IMAGE_CHAR.len_utf8();

/// An image of the text: its file and the size it shows at.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InlineImage {
    /// Tells decoded copies apart; new on every load.
    #[serde(skip, default = "next_image_id")]
    id: u64,
    pub name: String,
    pub mime_type: String,
    #[serde(with = "base64_bytes")]
    pub data: Arc<[u8]>,
    /// Size shown, in CSS pixels.
    pub width: u32,
    pub height: u32,
}

impl PartialEq for InlineImage {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.mime_type == other.mime_type
            && self.width == other.width
            && self.height == other.height
            && self.data == other.data
    }
}

impl Eq for InlineImage {}

fn next_image_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl InlineImage {
    pub fn new(
        name: impl Into<String>,
        mime_type: impl Into<String>,
        data: impl Into<Arc<[u8]>>,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            id: next_image_id(),
            name: name.into(),
            mime_type: mime_type.into(),
            data: data.into(),
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// Same for every copy of this image while the app runs.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The image as a `data:` URI, for pages that show it without a mail around it.
    pub fn data_uri(&self) -> String {
        use base64::Engine as _;
        format!(
            "data:{};base64,{}",
            self.mime_type,
            base64::engine::general_purpose::STANDARD.encode(&self.data)
        )
    }
}

/// Image bytes as base64 text in a serialized document.
mod base64_bytes {
    use std::sync::Arc;

    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(data: &Arc<[u8]>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<[u8]>, D::Error> {
        let text = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map(Into::into)
            .map_err(serde::de::Error::custom)
    }
}

/// Font of a piece of text: the set Gmail offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FontFamily {
    #[default]
    Sans,
    Serif,
    Mono,
    Wide,
    Narrow,
    ComicSans,
    Garamond,
    Georgia,
    Tahoma,
    Trebuchet,
    Verdana,
}

impl FontFamily {
    /// Every family, in menu order.
    pub const ALL: [FontFamily; 11] = [
        FontFamily::Sans,
        FontFamily::Serif,
        FontFamily::Mono,
        FontFamily::Wide,
        FontFamily::Narrow,
        FontFamily::ComicSans,
        FontFamily::Garamond,
        FontFamily::Georgia,
        FontFamily::Tahoma,
        FontFamily::Trebuchet,
        FontFamily::Verdana,
    ];

    /// Installed fonts that show it in the editor, by preference: the fonts
    /// themselves come with Windows and macOS, the rest are their usual stand-ins.
    pub(crate) fn candidates(self) -> &'static [&'static str] {
        match self {
            FontFamily::Sans => &[
                "Helvetica",
                "Arial",
                "Liberation Sans",
                "Noto Sans",
                "DejaVu Sans",
                "Ubuntu",
            ],
            FontFamily::Serif => &[
                "Times New Roman",
                "Liberation Serif",
                "Noto Serif",
                "DejaVu Serif",
            ],
            FontFamily::Mono => &[
                "Courier New",
                "Menlo",
                "Consolas",
                "Liberation Mono",
                "Noto Sans Mono",
                "DejaVu Sans Mono",
            ],
            FontFamily::Wide => &["Arial Black", "Noto Sans Black", "DejaVu Sans"],
            FontFamily::Narrow => &[
                "Arial Narrow",
                "Liberation Sans Narrow",
                "Nimbus Sans Narrow",
            ],
            FontFamily::ComicSans => &["Comic Sans MS", "Comic Neue", "Comic Relief"],
            FontFamily::Garamond => &["Garamond", "EB Garamond", "Liberation Serif"],
            FontFamily::Georgia => &["Georgia", "Gelasio", "Liberation Serif", "DejaVu Serif"],
            FontFamily::Tahoma => &["Tahoma", "Noto Sans", "DejaVu Sans"],
            FontFamily::Trebuchet => &["Trebuchet MS", "Fira Sans", "Noto Sans"],
            FontFamily::Verdana => &["Verdana", "DejaVu Sans", "Noto Sans"],
        }
    }

    /// CSS `font-family` in the sent mail; `None` leaves the reader's default.
    fn css(self) -> Option<&'static str> {
        match self {
            FontFamily::Sans => None,
            FontFamily::Serif => Some("'Times New Roman',serif"),
            FontFamily::Mono => Some("'Courier New',monospace"),
            FontFamily::Wide => Some("'Arial Black',sans-serif"),
            FontFamily::Narrow => Some("'Arial Narrow',sans-serif"),
            FontFamily::ComicSans => Some("'Comic Sans MS',sans-serif"),
            FontFamily::Garamond => Some("Garamond,'Times New Roman',serif"),
            FontFamily::Georgia => Some("Georgia,serif"),
            FontFamily::Tahoma => Some("Tahoma,sans-serif"),
            FontFamily::Trebuchet => Some("'Trebuchet MS',sans-serif"),
            FontFamily::Verdana => Some("Verdana,sans-serif"),
        }
    }
}

/// Size of a paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextSize {
    Small,
    #[default]
    Normal,
    Large,
    Huge,
}

impl TextSize {
    /// Size relative to the normal one.
    pub fn scale(self) -> f32 {
        match self {
            TextSize::Small => 0.8,
            TextSize::Normal => 1.0,
            TextSize::Large => 1.3,
            TextSize::Huge => 1.8,
        }
    }
}

/// Horizontal alignment of a paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// List a paragraph is an item of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// Highest indent level.
pub const MAX_INDENT: u8 = 8;

/// Style of a run of characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct CharStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub family: FontFamily,
    /// `0xRRGGBB`; `None` is the page's text color.
    pub color: Option<u32>,
    /// On an [`IMAGE_CHAR`]: the index of the image it shows in its [`RichText`].
    pub image: Option<u32>,
}

/// Style of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ParaStyle {
    pub align: Align,
    pub list: Option<ListKind>,
    pub indent: u8,
    pub size: TextSize,
}

/// Formatting at the caret or selection, for a toolbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Format {
    pub chars: CharStyle,
    pub para: ParaStyle,
}

/// Styles over a text. Invariants: `runs` covers the text exactly, without empty
/// or mergeable neighbors; `paras` has one entry per line; an image index points
/// into `images`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichText {
    runs: Vec<(usize, CharStyle)>,
    paras: Vec<ParaStyle>,
    /// Every image ever inserted (undo can bring a deleted one back).
    #[serde(default)]
    images: Vec<InlineImage>,
    /// A range that follows the edits, for a part of the text its owner replaces
    /// later (a signature). Not saved.
    #[serde(skip)]
    tracked: Option<Range<usize>>,
}

/// Formatted text, whole: what a signature saves and what is inserted with it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichDocument {
    pub text: String,
    pub rich: RichText,
}

impl RichDocument {
    /// `text` without formatting (line endings normalized).
    pub fn plain(text: &str) -> Self {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        Self {
            rich: RichText::new(&text),
            text,
        }
    }

    /// The document if its styles describe its text, else its text alone: a saved
    /// one may be damaged or come from another version. Images it doesn't show go.
    pub fn validated(self) -> Self {
        let images = self.rich.images.len();
        let mut end = 0;
        let valid = !self.text.contains('\r')
            && self.rich.matches(&self.text)
            && self.rich.runs.iter().all(|(len, style)| {
                end += len;
                *len > 0
                    && self.text.is_char_boundary(end)
                    && style
                        .image
                        .is_none_or(|ix| usize::try_from(ix).is_ok_and(|ix| ix < images))
            });
        if valid {
            Self {
                rich: self.rich.compacted(),
                text: self.text,
            }
        } else {
            Self::plain(&self.text)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// See [`RichText::to_plain`].
    pub fn to_plain(&self) -> String {
        self.rich.to_plain(&self.text)
    }

    /// See [`RichText::to_html_with`].
    pub fn to_html_with(&self, image_src: impl Fn(&InlineImage) -> String) -> String {
        self.rich.to_html_with(&self.text, image_src)
    }

    /// The images it shows, each once, in order.
    pub fn images(&self) -> Vec<InlineImage> {
        self.rich.images_used().into_iter().cloned().collect()
    }
}

impl Default for RichText {
    fn default() -> Self {
        Self::new("")
    }
}

impl RichText {
    /// `text` without any formatting.
    pub fn new(text: &str) -> Self {
        Self {
            runs: if text.is_empty() {
                Vec::new()
            } else {
                vec![(text.len(), CharStyle::default())]
            },
            paras: vec![ParaStyle::default(); text.matches('\n').count() + 1],
            images: Vec::new(),
            tracked: None,
        }
    }

    /// Whether this still describes `text`.
    pub fn matches(&self, text: &str) -> bool {
        self.runs.iter().map(|(len, _)| len).sum::<usize>() == text.len()
            && self.paras.len() == text.matches('\n').count() + 1
    }

    /// Whether anything is formatted.
    pub fn is_plain(&self) -> bool {
        self.runs
            .iter()
            .all(|(_, style)| *style == CharStyle::default())
            && self.paras.iter().all(|para| *para == ParaStyle::default())
    }

    /// Follows the edit that turned `old` into `new`, found by comparing them:
    /// inserted text takes `typed`, new lines the style of the line they split.
    pub fn apply_edit(&mut self, old: &str, new: &str, typed: CharStyle) {
        let start = common_prefix(old, new);
        let suffix = common_suffix(&old[start..], &new[start..]);
        let removed = start..old.len() - suffix;
        let inserted = &new[start..new.len() - suffix];
        self.apply_edit_at(old, removed, inserted, typed);
    }

    /// Follows the edit that replaced `removed` of `old` with `inserted`. With repeated
    /// text only the real range tells which of the copies changed.
    pub fn apply_edit_at(
        &mut self,
        old: &str,
        removed: std::ops::Range<usize>,
        inserted: &str,
        typed: CharStyle,
    ) {
        if let Some(tracked) = &self.tracked {
            self.tracked = Some(follow_edit(tracked, &removed, inserted.len()));
        }
        // A line break on both sides isn't an edit of the line after it.
        let (removed, inserted) = match inserted.strip_suffix('\n') {
            Some(shorter) if old[removed.clone()].ends_with('\n') => {
                (removed.start..removed.end - 1, shorter)
            }
            _ => (removed, inserted),
        };
        let start = removed.start;
        let line = old[..start].matches('\n').count();
        let removed_lines = old[removed.clone()].matches('\n').count();
        let at_line_start = start == 0 || old.as_bytes()[start - 1] == b'\n';
        if at_line_start && inserted.is_empty() {
            // Whole lines went: what remains is the last of them, with its own style.
            self.paras.drain(line..line + removed_lines);
        } else {
            let style = self.paras[line];
            self.paras.splice(
                line + 1..=line + removed_lines,
                std::iter::repeat_n(style, inserted.matches('\n').count()),
            );
        }

        self.split_at(removed.start);
        self.split_at(removed.end);
        let (first, last) = (self.run_at(removed.start), self.run_at(removed.end));
        let new_run = (!inserted.is_empty()).then_some((inserted.len(), typed));
        self.runs.splice(first..last, new_run);
        self.merge_runs();
    }

    /// Style of what is typed at `offset`: that of the character before it, as
    /// text (typing never makes an image).
    pub fn style_at(&self, offset: usize) -> CharStyle {
        let mut end = 0;
        let mut found = None;
        for (len, style) in &self.runs {
            end += len;
            if offset <= end {
                found = Some(*style);
                break;
            }
        }
        let mut style = found
            .or_else(|| self.runs.last().map(|(_, style)| *style))
            .unwrap_or_default();
        style.image = None;
        style
    }

    /// Adds `image` to the ones the text can show; its index goes in [`CharStyle::image`].
    pub fn add_image(&mut self, image: InlineImage) -> u32 {
        self.images.push(image);
        u32::try_from(self.images.len() - 1).unwrap_or(u32::MAX)
    }

    /// The image the character at `offset` shows, if it is an image.
    pub fn image_at(&self, text: &str, offset: usize) -> Option<&InlineImage> {
        if !text.get(offset..)?.starts_with(IMAGE_CHAR) {
            return None;
        }
        let (_, style) = self
            .runs_in(offset..offset + IMAGE_LEN)
            .into_iter()
            .next()?;
        self.images.get(usize::try_from(style.image?).ok()?)
    }

    /// The image `line` (a range of `text` without its line break) shows, when it is
    /// an image line.
    pub fn line_image(&self, text: &str, line: Range<usize>) -> Option<&InlineImage> {
        (line.len() == IMAGE_LEN)
            .then(|| self.image_at(text, line.start))
            .flatten()
    }

    /// The same formatting keeping only the images the text shows: the others are
    /// there for undo, and a saved document would carry them for good.
    pub fn compacted(&self) -> RichText {
        let mut used: Vec<u32> = Vec::new();
        let runs = self
            .runs
            .iter()
            .map(|(len, style)| {
                let mut style = *style;
                style.image = style.image.map(|ix| {
                    let position = used.iter().position(|seen| *seen == ix).unwrap_or_else(|| {
                        used.push(ix);
                        used.len() - 1
                    });
                    u32::try_from(position).unwrap_or(u32::MAX)
                });
                (*len, style)
            })
            .collect();
        RichText {
            runs,
            paras: self.paras.clone(),
            images: used
                .iter()
                .filter_map(|ix| self.images.get(usize::try_from(*ix).ok()?).cloned())
                .collect(),
            tracked: self.tracked.clone(),
        }
    }

    /// The images the text shows, each once, in order.
    pub fn images_used(&self) -> Vec<&InlineImage> {
        let mut seen = Vec::new();
        for (_, style) in &self.runs {
            if let Some(ix) = style.image {
                if !seen.contains(&ix) {
                    seen.push(ix);
                }
            }
        }
        seen.into_iter()
            .filter_map(|ix| self.images.get(usize::try_from(ix).ok()?))
            .collect()
    }

    /// The line breaks to put around `inserted` when it replaces `range` of `text`, so
    /// an image never shares its line: (before, after).
    pub fn image_separators(
        &self,
        text: &str,
        range: Range<usize>,
        inserted: &str,
    ) -> (&'static str, &'static str) {
        let image_before =
            range.start >= IMAGE_LEN && self.image_at(text, range.start - IMAGE_LEN).is_some();
        let image_after = self.image_at(text, range.end).is_some();
        let text_after = text.get(range.end..).and_then(|rest| rest.chars().next());
        let text_before = text.get(..range.start).and_then(|rest| rest.chars().last());
        let joins_before = image_before
            && match inserted.chars().next() {
                Some(first) => first != '\n',
                None => text_after.is_some_and(|next| next != '\n'),
            };
        let joins_after = image_after
            && match inserted.chars().last() {
                Some(last) => last != '\n',
                None => text_before.is_some_and(|previous| previous != '\n'),
            };
        match (joins_before, joins_after) {
            // Deleting what was between: one break keeps both apart.
            (true, true) if inserted.is_empty() => ("\n", ""),
            (before, after) => (
                if before { "\n" } else { "" },
                if after { "\n" } else { "" },
            ),
        }
    }

    /// [`Self::image_separators`] for a whole document put in place of `range`: also
    /// the breaks that keep the document's own first or last image off the text
    /// around it.
    pub fn document_separators(
        &self,
        text: &str,
        range: Range<usize>,
        document: &RichDocument,
    ) -> (&'static str, &'static str) {
        let (mut before, mut after) = self.image_separators(text, range.clone(), &document.text);
        let previous = text[..range.start].chars().last();
        let next = text[range.end..].chars().next();
        let inserted = &document.text;
        if document.rich.image_at(inserted, 0).is_some() && previous.is_some_and(|c| c != '\n') {
            before = "\n";
        }
        let last_image = inserted.len() >= IMAGE_LEN
            && document
                .rich
                .image_at(inserted, inserted.len() - IMAGE_LEN)
                .is_some();
        if last_image && next.is_some_and(|c| c != '\n') {
            after = "\n";
        }
        (before, after)
    }

    /// What a Backspace or Delete of `range` takes: the image next to it too when
    /// deleting `range` alone would join that image's line with text.
    pub fn deletion_range(&self, text: &str, range: Range<usize>) -> Range<usize> {
        if self.image_separators(text, range.clone(), "") == ("", "") {
            return range;
        }
        if range.start >= IMAGE_LEN && self.image_at(text, range.start - IMAGE_LEN).is_some() {
            range.start - IMAGE_LEN..range.end
        } else if self.image_at(text, range.end).is_some() {
            range.start..range.end + IMAGE_LEN
        } else {
            range
        }
    }

    /// The range set with [`Self::set_tracked`], as edits left it.
    pub fn tracked(&self) -> Option<Range<usize>> {
        self.tracked.clone()
    }

    /// Follows `range` through later edits: inside it they grow or shrink it, and
    /// text inserted at its edges stays out.
    pub fn set_tracked(&mut self, range: Option<Range<usize>>) {
        self.tracked = range;
    }

    /// Replaces `range` of `text` with `doc`, its styles and images included, and
    /// returns the new text. Lines that are wholly the document's take its line
    /// styles; a line it joins keeps its own.
    pub fn splice(&mut self, text: &str, range: Range<usize>, doc: &RichDocument) -> String {
        let new_text = format!("{}{}{}", &text[..range.start], doc.text, &text[range.end..]);
        self.apply_edit_at(text, range.clone(), &doc.text, CharStyle::default());
        let start = range.start;
        let mut mapped: Vec<(u32, u32)> = Vec::new();
        for (run, mut style) in doc.rich.runs_in(0..doc.text.len()) {
            if let Some(ix) = style.image {
                let image = usize::try_from(ix)
                    .ok()
                    .and_then(|ix| doc.rich.images.get(ix));
                style.image = match (mapped.iter().find(|(from, _)| *from == ix), image) {
                    (Some((_, to)), _) => Some(*to),
                    (None, Some(image)) => {
                        let to = self.add_image(image.clone());
                        mapped.push((ix, to));
                        Some(to)
                    }
                    (None, None) => None,
                };
            }
            self.update_chars(start + run.start..start + run.end, move |current| {
                *current = style;
            });
        }
        if !doc.text.is_empty() {
            let first_line = text[..start].matches('\n').count();
            let last = doc.text.matches('\n').count();
            let end = start + doc.text.len();
            let starts_line = start == 0 || text.as_bytes()[start - 1] == b'\n';
            let ends_line = end == new_text.len() || new_text.as_bytes()[end] == b'\n';
            for line in 0..=last {
                if (line > 0 || starts_line) && (line < last || ends_line) {
                    let para = doc.rich.para(line);
                    self.update_paras(first_line + line..first_line + line + 1, move |current| {
                        *current = para;
                    });
                }
            }
        }
        new_text
    }

    /// Style of line `line`.
    pub fn para(&self, line: usize) -> ParaStyle {
        self.paras.get(line).copied().unwrap_or_default()
    }

    /// Clipped runs inside `range`: (range, style).
    pub fn runs_in(&self, range: Range<usize>) -> Vec<(Range<usize>, CharStyle)> {
        let mut out = Vec::new();
        let mut start = 0;
        for (len, style) in &self.runs {
            let end = start + len;
            let clipped = start.max(range.start)..end.min(range.end);
            if clipped.start < clipped.end {
                out.push((clipped, *style));
            }
            start = end;
        }
        out
    }

    /// Whether every character of `range` satisfies `test`.
    pub fn all_chars(&self, range: Range<usize>, test: impl Fn(&CharStyle) -> bool) -> bool {
        self.runs_in(range).iter().all(|(_, style)| test(style))
    }

    /// Changes the style of every character of `range`.
    pub fn update_chars(&mut self, range: Range<usize>, update: impl Fn(&mut CharStyle)) {
        if range.is_empty() {
            return;
        }
        self.split_at(range.start);
        self.split_at(range.end);
        let (first, last) = (self.run_at(range.start), self.run_at(range.end));
        for (_, style) in &mut self.runs[first..last] {
            update(style);
        }
        self.merge_runs();
    }

    /// Changes the style of `lines`.
    pub fn update_paras(&mut self, lines: Range<usize>, update: impl Fn(&mut ParaStyle)) {
        let end = lines.end.min(self.paras.len());
        for para in &mut self.paras[lines.start.min(end)..end] {
            update(para);
        }
    }

    /// Number of an item of a numbered list: its position among the neighbors of
    /// its kind and indent.
    pub fn list_number(&self, line: usize) -> usize {
        let para = self.para(line);
        self.paras[..line]
            .iter()
            .rev()
            .take_while(|other| other.list == para.list && other.indent == para.indent)
            .count()
            + 1
    }

    /// Marker drawn before an item of a list.
    pub fn list_marker(&self, line: usize) -> Option<String> {
        match self.para(line).list? {
            ListKind::Bullet => Some("•".to_string()),
            ListKind::Numbered => Some(format!("{}.", self.list_number(line))),
        }
    }

    /// `text` for the plain-text version of an e-mail: lists and indents as text,
    /// images left out with their lines, as Thunderbird does.
    pub fn to_plain(&self, text: &str) -> String {
        let mut start = 0;
        text.split('\n')
            .enumerate()
            .filter_map(|(line, content)| {
                let range = start..start + content.len();
                start = range.end + 1;
                if self.line_image(text, range).is_some() {
                    return None;
                }
                let indent = "    ".repeat(usize::from(self.para(line).indent));
                Some(match self.list_marker(line) {
                    Some(marker) => format!("{indent}{marker} {content}"),
                    None => format!("{indent}{content}"),
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `text` as e-mail HTML, its images as `data:` URIs.
    pub fn to_html(&self, text: &str) -> String {
        self.to_html_with(text, InlineImage::data_uri)
    }

    /// `text` as e-mail HTML, each image's `src` from `image_src` (a `cid:` in a mail).
    pub fn to_html_with(&self, text: &str, image_src: impl Fn(&InlineImage) -> String) -> String {
        let mut out = String::from("<div dir=\"ltr\">");
        let mut open_list: Option<(ListKind, u8)> = None;
        let mut start = 0;
        for (line, content) in text.split('\n').enumerate() {
            let para = self.para(line);
            let range = start..start + content.len();
            start = range.end + 1;
            let list = para.list.map(|kind| (kind, para.indent));
            if open_list != list {
                if let Some((kind, _)) = open_list {
                    out.push_str(list_close(kind));
                }
                if let Some((kind, indent)) = list {
                    out.push_str(&list_open(kind, indent));
                }
                open_list = list;
            }
            let tag = if para.list.is_some() { "li" } else { "div" };
            out.push('<');
            out.push_str(tag);
            out.push_str(&para_style_attribute(&para));
            out.push('>');
            if let Some(image) = self.line_image(text, range.clone()) {
                push_image(&mut out, image, &image_src(image));
            } else {
                if content.is_empty() {
                    out.push_str("<br>");
                }
                let links = link_ranges(content);
                // Runs are cut at the links' edges, so each link wraps whole pieces.
                let mut cuts: Vec<usize> = links
                    .iter()
                    .flat_map(|(link, _)| [range.start + link.start, range.start + link.end])
                    .collect();
                cuts.sort_unstable();
                for (run, style) in self.runs_in(range.clone()) {
                    let mut start = run.start;
                    for end in cuts
                        .iter()
                        .copied()
                        .filter(|cut| *cut > run.start && *cut < run.end)
                        .chain([run.end])
                    {
                        let local = start - range.start;
                        if let Some((_, href)) = links.iter().find(|(link, _)| link.start == local)
                        {
                            out.push_str("<a href=\"");
                            push_escaped(&mut out, href);
                            out.push_str("\">");
                        }
                        push_styled(&mut out, &text[start..end], &style);
                        if links.iter().any(|(link, _)| link.end == end - range.start) {
                            out.push_str("</a>");
                        }
                        start = end;
                    }
                }
            }
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        if let Some((kind, _)) = open_list {
            out.push_str(list_close(kind));
        }
        out.push_str("</div>");
        out
    }

    /// Makes `offset` a boundary between runs.
    fn split_at(&mut self, offset: usize) {
        let mut start = 0;
        for ix in 0..self.runs.len() {
            let (len, style) = self.runs[ix];
            if offset > start && offset < start + len {
                self.runs[ix].0 = offset - start;
                self.runs.insert(ix + 1, (start + len - offset, style));
                return;
            }
            start += len;
        }
    }

    /// Index of the run starting at `offset`, a boundary (the end: their count).
    fn run_at(&self, offset: usize) -> usize {
        let mut start = 0;
        for (ix, (len, _)) in self.runs.iter().enumerate() {
            if start >= offset {
                return ix;
            }
            start += len;
        }
        self.runs.len()
    }

    fn merge_runs(&mut self) {
        self.runs.retain(|(len, _)| *len > 0);
        let mut merged: Vec<(usize, CharStyle)> = Vec::with_capacity(self.runs.len());
        for (len, style) in self.runs.drain(..) {
            match merged.last_mut() {
                Some((last_len, last)) if *last == style => *last_len += len,
                _ => merged.push((len, style)),
            }
        }
        self.runs = merged;
    }
}

/// Web and e-mail addresses in `line`, linked as Gmail's composer links them while
/// typing: (byte range, href). Punctuation around an address stays out of it.
pub fn link_ranges(line: &str) -> Vec<(Range<usize>, String)> {
    const LEADING: &[char] = &['(', '<', '[', '"', '\''];
    const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', '>', ']', '"', '\''];
    let mut links = Vec::new();
    let mut offset = 0;
    for token in line.split(' ') {
        let start = offset + (token.len() - token.trim_start_matches(LEADING).len());
        let word = token.trim_start_matches(LEADING).trim_end_matches(TRAILING);
        offset += token.len() + 1;
        let lower = word.to_ascii_lowercase();
        let href = if ["http://", "https://", "mailto:"]
            .iter()
            .any(|scheme| lower.starts_with(scheme) && word.len() > scheme.len())
        {
            Some(word.to_string())
        } else if lower.starts_with("www.") && word[4..].contains('.') {
            Some(format!("http://{word}"))
        } else if is_email(word) {
            Some(format!("mailto:{word}"))
        } else {
            None
        };
        if let Some(href) = href.filter(|_| !word.chars().any(char::is_whitespace)) {
            links.push((start..start + word.len(), href));
        }
    }
    links
}

/// Whether `word` reads as an e-mail address: `name@domain.tld`.
fn is_email(word: &str) -> bool {
    let Some((local, domain)) = word.split_once('@') else {
        return false;
    };
    let labels: Vec<&str> = domain.split('.').collect();
    !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_alphanumeric() || "._%+-".contains(c))
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty() && label.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
}

/// Where `range` goes when `removed` is replaced by `inserted_len` bytes: text put at
/// its edges stays out of it, and what replaces part of it too.
fn follow_edit(range: &Range<usize>, removed: &Range<usize>, inserted_len: usize) -> Range<usize> {
    let moved = |offset: usize| offset - removed.len() + inserted_len;
    let start = if range.start < removed.start {
        range.start
    } else if range.start >= removed.end {
        moved(range.start)
    } else {
        removed.start + inserted_len
    };
    let end = if range.end <= removed.start {
        range.end
    } else if range.end >= removed.end {
        moved(range.end)
    } else {
        removed.start
    };
    start..end.max(start)
}

/// Bytes `a` and `b` share at the start, on a character boundary.
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or(a.len().min(b.len()), |((ix, _), _)| ix)
}

/// Bytes `a` and `b` share at the end, on a character boundary.
fn common_suffix(a: &str, b: &str) -> usize {
    a.chars()
        .rev()
        .zip(b.chars().rev())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.len_utf8())
        .sum()
}

fn list_open(kind: ListKind, indent: u8) -> String {
    let tag = match kind {
        ListKind::Bullet => "ul",
        ListKind::Numbered => "ol",
    };
    format!(
        "<{tag} style=\"margin:0 0 0 {}px;padding-left:24px\">",
        u32::from(indent) * 40
    )
}

fn list_close(kind: ListKind) -> &'static str {
    match kind {
        ListKind::Bullet => "</ul>",
        ListKind::Numbered => "</ol>",
    }
}

fn para_style_attribute(para: &ParaStyle) -> String {
    let mut style = String::new();
    match para.align {
        Align::Left => {}
        Align::Center => style.push_str("text-align:center;"),
        Align::Right => style.push_str("text-align:right;"),
    }
    if para.indent > 0 && para.list.is_none() {
        style.push_str(&format!("margin-left:{}px;", u32::from(para.indent) * 40));
    }
    if para.size != TextSize::Normal {
        style.push_str(&format!("font-size:{}em;", para.size.scale()));
    }
    if style.is_empty() {
        style
    } else {
        format!(" style=\"{style}\"")
    }
}

fn push_styled(out: &mut String, text: &str, style: &CharStyle) {
    let mut css = String::new();
    if let Some(family) = style.family.css() {
        css.push_str(&format!("font-family:{family};"));
    }
    if let Some(color) = style.color {
        css.push_str(&format!("color:#{color:06x};"));
    }
    if !css.is_empty() {
        out.push_str(&format!("<span style=\"{css}\">"));
    }
    let tags = [
        (style.bold, "b"),
        (style.italic, "i"),
        (style.underline, "u"),
    ];
    for (_, tag) in tags.iter().filter(|(on, _)| *on) {
        out.push_str(&format!("<{tag}>"));
    }
    push_escaped(out, text);
    for (_, tag) in tags.iter().rev().filter(|(on, _)| *on) {
        out.push_str(&format!("</{tag}>"));
    }
    if !css.is_empty() {
        out.push_str("</span>");
    }
}

fn push_image(out: &mut String, image: &InlineImage, src: &str) {
    out.push_str("<img src=\"");
    push_escaped(out, src);
    out.push_str(&format!(
        "\" width=\"{}\" height=\"{}\" alt=\"",
        image.width, image.height
    ));
    push_escaped(out, &image.name);
    out.push_str("\">");
}

/// Escapes for HTML, keeping runs of spaces, which it would collapse.
fn push_escaped(out: &mut String, text: &str) {
    let mut after_space = false;
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            ' ' if after_space => out.push_str("&nbsp;"),
            c => out.push(c),
        }
        after_space = c == ' ';
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOLD: CharStyle = CharStyle {
        bold: true,
        italic: false,
        underline: false,
        family: FontFamily::Sans,
        color: None,
        image: None,
    };

    fn styles(rich: &RichText, text: &str) -> Vec<(String, bool)> {
        rich.runs_in(0..text.len())
            .into_iter()
            .map(|(range, style)| (text[range].to_string(), style.bold))
            .collect()
    }

    #[test]
    fn replacing_a_whole_line_leaves_the_next_one_alone() {
        let text = "AAA\nBBB\nCCC";
        for inserted in ["\n", "X\n"] {
            let mut rich = RichText::new(text);
            rich.update_paras(1..2, |para| para.align = Align::Center);
            rich.update_paras(2..3, |para| para.align = Align::Right);
            rich.apply_edit_at(text, 4..8, inserted, CharStyle::default());
            assert_eq!(rich.para(2).align, Align::Right, "{inserted:?}");
        }
    }

    #[test]
    fn with_repeated_text_the_real_range_says_which_copy_changed() {
        let text = "foo foo";
        let mut rich = RichText::new(text);
        rich.update_chars(4..7, |style| style.bold = true);
        rich.apply_edit_at(text, 0..4, "", CharStyle::default());
        assert!(
            rich.all_chars(0..3, |style| style.bold),
            "the bold one stays"
        );

        let text = "Item\nItem";
        let mut rich = RichText::new(text);
        rich.update_paras(1..2, |para| para.list = Some(ListKind::Bullet));
        rich.apply_edit_at(text, 0..5, "", CharStyle::default());
        assert_eq!(rich.para(0).list, Some(ListKind::Bullet));
    }

    #[test]
    fn new_text_is_plain() {
        let rich = RichText::new("a\nb");
        assert!(rich.is_plain() && rich.matches("a\nb"));
        assert!(!rich.matches("a"));
        assert!(RichText::new("").matches(""));
    }

    #[test]
    fn formatting_splits_and_merges_runs() {
        let text = "hello world";
        let mut rich = RichText::new(text);
        rich.update_chars(0..5, |style| style.bold = true);
        assert_eq!(
            styles(&rich, text),
            [("hello".to_string(), true), (" world".to_string(), false)]
        );
        assert!(rich.all_chars(0..5, |style| style.bold));
        assert!(!rich.all_chars(0..6, |style| style.bold));
        rich.update_chars(5..11, |style| style.bold = true);
        assert_eq!(styles(&rich, text), [(text.to_string(), true)]);
    }

    #[test]
    fn typing_inherits_the_style_before_the_caret() {
        let mut rich = RichText::new("ab");
        rich.update_chars(0..1, |style| style.bold = true);
        assert!(rich.style_at(1).bold);
        assert!(!rich.style_at(2).bold);
        assert!(rich.style_at(0).bold);
        rich.apply_edit("ab", "aXb", rich.style_at(1));
        assert_eq!(
            styles(&rich, "aXb"),
            [("aX".to_string(), true), ("b".to_string(), false)]
        );
        assert!(rich.matches("aXb"));
    }

    #[test]
    fn edits_keep_styles_around_them() {
        let mut rich = RichText::new("abcd");
        rich.update_chars(1..3, |style| style.bold = true);
        rich.apply_edit("abcd", "ad", CharStyle::default());
        assert!(rich.is_plain() && rich.matches("ad"));

        let mut rich = RichText::new("abcd");
        rich.update_chars(0..2, |style| style.bold = true);
        rich.apply_edit("abcd", "axyzd", CharStyle::default());
        assert_eq!(
            styles(&rich, "axyzd"),
            [("a".to_string(), true), ("xyzd".to_string(), false)]
        );

        let mut rich = RichText::new("");
        rich.apply_edit("", "é", BOLD);
        assert_eq!(styles(&rich, "é"), [("é".to_string(), true)]);
        rich.apply_edit("é", "", CharStyle::default());
        assert!(rich.matches(""));
    }

    #[test]
    fn repeated_characters_and_multibyte_edits_stay_consistent() {
        let mut rich = RichText::new("aaa");
        rich.apply_edit("aaa", "aaaa", CharStyle::default());
        assert!(rich.matches("aaaa"));
        let mut rich = RichText::new("ação");
        rich.apply_edit("ação", "acão", CharStyle::default());
        assert!(rich.matches("acão"));
    }

    #[test]
    fn lines_follow_edits() {
        let center = |para: &mut ParaStyle| para.align = Align::Center;
        let mut rich = RichText::new("one\ntwo");
        rich.update_paras(1..2, center);
        // Splitting a line: both halves keep its style.
        rich.apply_edit("one\ntwo", "one\nt\nwo", CharStyle::default());
        assert_eq!(rich.para(1).align, Align::Center);
        assert_eq!(rich.para(2).align, Align::Center);
        assert_eq!(rich.para(0).align, Align::Left);
        // Joining lines: the first one's style stays.
        rich.apply_edit("one\nt\nwo", "onet\nwo", CharStyle::default());
        assert_eq!(rich.para(0).align, Align::Left);
        assert_eq!(rich.para(1).align, Align::Center);
        assert!(rich.matches("onet\nwo"));
        // Removing several at once.
        let mut rich = RichText::new("a\nb\nc\nd");
        rich.apply_edit("a\nb\nc\nd", "a\nd", CharStyle::default());
        assert!(rich.matches("a\nd"));
    }

    #[test]
    fn numbered_items_count_their_neighbors() {
        let mut rich = RichText::new("a\nb\nc\nd");
        rich.update_paras(0..2, |para| para.list = Some(ListKind::Numbered));
        rich.update_paras(3..4, |para| para.list = Some(ListKind::Numbered));
        assert_eq!(rich.list_marker(0).as_deref(), Some("1."));
        assert_eq!(rich.list_marker(1).as_deref(), Some("2."));
        assert_eq!(rich.list_marker(2), None);
        assert_eq!(rich.list_marker(3).as_deref(), Some("1."));
    }

    #[test]
    fn plain_export_writes_lists_and_indents() {
        let mut rich = RichText::new("a\nb\nc");
        rich.update_paras(0..1, |para| para.list = Some(ListKind::Bullet));
        rich.update_paras(1..2, |para| para.list = Some(ListKind::Numbered));
        rich.update_paras(2..3, |para| para.indent = 1);
        assert_eq!(rich.to_plain("a\nb\nc"), "• a\n1. b\n    c");
    }

    #[test]
    fn html_export_covers_every_style() {
        let text = "bold plain\n<c>\n\nitem";
        let mut rich = RichText::new(text);
        rich.update_chars(0..4, |style| {
            style.bold = true;
            style.italic = true;
            style.underline = true;
        });
        rich.update_chars(5..10, |style| {
            style.family = FontFamily::Mono;
            style.color = Some(0xff0000);
        });
        rich.update_paras(1..2, |para| {
            para.align = Align::Right;
            para.size = TextSize::Large;
            para.indent = 2;
        });
        rich.update_paras(3..4, |para| para.list = Some(ListKind::Bullet));
        assert_eq!(
            rich.to_html(text),
            "<div dir=\"ltr\"><div><b><i><u>bold</u></i></b> \
             <span style=\"font-family:'Courier New',monospace;color:#ff0000;\">plain</span></div>\
             <div style=\"text-align:right;margin-left:80px;font-size:1.3em;\">&lt;c&gt;</div>\
             <div><br></div>\
             <ul style=\"margin:0 0 0 0px;padding-left:24px\"><li>item</li></ul></div>"
        );
    }

    #[test]
    fn every_family_but_the_default_is_named_in_the_mail() {
        for family in FontFamily::ALL {
            assert!(!family.candidates().is_empty(), "{family:?}");
            assert_eq!(family.css().is_none(), family == FontFamily::Sans);
        }
        let mut rich = RichText::new("x");
        rich.update_chars(0..1, |style| style.family = FontFamily::ComicSans);
        assert!(rich
            .to_html("x")
            .contains("font-family:'Comic Sans MS',sans-serif;"));
    }

    #[test]
    fn html_lists_group_neighbors_and_keep_spaces() {
        let text = "a\nb  c\nd";
        let mut rich = RichText::new(text);
        rich.update_paras(0..2, |para| para.list = Some(ListKind::Numbered));
        let html = rich.to_html(text);
        assert_eq!(html.matches("<ol").count(), 1);
        assert!(
            html.contains("<li>a</li><li>b &nbsp;c</li></ol><div>d</div>"),
            "{html}"
        );
    }

    fn image_doc(before: &str, after: &str) -> RichDocument {
        let text = format!("{before}{IMAGE_CHAR}{after}");
        let mut rich = RichText::new(&text);
        let ix = rich.add_image(InlineImage::new(
            "logo.png",
            "image/png",
            vec![1, 2, 3],
            10,
            5,
        ));
        rich.update_chars(before.len()..before.len() + IMAGE_LEN, |style| {
            style.image = Some(ix);
        });
        RichDocument { text, rich }
    }

    #[test]
    fn an_image_line_is_an_img_in_html_and_nothing_in_plain_text() {
        let doc = image_doc("Bob\n", "\nCEO");
        let html = doc.to_html_with(|image| format!("cid:{}", image.name));
        assert!(
            html.contains(
                "<div><img src=\"cid:logo.png\" width=\"10\" height=\"5\" alt=\"logo.png\"></div>"
            ),
            "{html}"
        );
        assert!(html.contains("<div>Bob</div>") && html.contains("<div>CEO</div>"));
        assert_eq!(doc.to_plain(), "Bob\nCEO");
        assert!(!doc.rich.is_plain());
        assert_eq!(doc.images().len(), 1);
        assert!(doc
            .rich
            .to_html(&doc.text)
            .contains("src=\"data:image/png;base64,AQID\""));
    }

    #[test]
    fn an_image_char_without_its_image_is_text() {
        let text = format!("a\n{IMAGE_CHAR}");
        let rich = RichText::new(&text);
        assert!(rich.line_image(&text, 2..5).is_none());
        assert_eq!(rich.to_plain(&text), text);
    }

    #[test]
    fn typing_next_to_an_image_never_joins_its_line() {
        let doc = image_doc("", "");
        let (text, rich) = (&doc.text, &doc.rich);
        // After it, before it, and over the break between it and text.
        assert_eq!(rich.image_separators(text, 3..3, "x"), ("\n", ""));
        assert_eq!(rich.image_separators(text, 0..0, "x"), ("", "\n"));
        assert_eq!(rich.image_separators(text, 0..0, "x\n"), ("", ""));
        let doc = image_doc("ab\n", "\ncd");
        let (text, rich) = (&doc.text, &doc.rich);
        assert_eq!(rich.image_separators(text, 6..7, ""), ("\n", ""));
        assert_eq!(rich.image_separators(text, 2..3, ""), ("", "\n"));
        assert_eq!(rich.image_separators(text, 0..1, ""), ("", ""));
        // Two images whose lines would meet stay one line apart.
        let text = format!("{IMAGE_CHAR}\n{IMAGE_CHAR}");
        let mut rich = RichText::new(&text);
        let ix = rich.add_image(InlineImage::new("a", "image/png", vec![0], 1, 1));
        rich.update_chars(0..text.len(), |style| style.image = Some(ix));
        rich.update_chars(3..4, |style| style.image = None);
        assert_eq!(rich.image_separators(&text, 3..4, ""), ("\n", ""));
    }

    #[test]
    fn backspace_and_delete_take_the_image_whose_line_they_would_join() {
        let doc = image_doc("ab\n", "\ncd");
        let (text, rich) = (&doc.text, &doc.rich);
        // Backspace at the start of "cd"; Delete at the end of "ab".
        assert_eq!(rich.deletion_range(text, 6..7), 3..7);
        assert_eq!(rich.deletion_range(text, 2..3), 2..6);
        assert_eq!(rich.deletion_range(text, 0..1), 0..1);
        // An empty line below it goes alone.
        let doc = image_doc("", "\n\nx");
        assert_eq!(doc.rich.deletion_range(&doc.text, 3..4), 3..4);
    }

    #[test]
    fn typing_after_an_image_is_text() {
        let doc = image_doc("", "");
        assert_eq!(doc.rich.style_at(3).image, None);
    }

    #[test]
    fn the_tracked_range_follows_edits_and_leaves_its_edges_out() {
        let text = "hi\nsig";
        let mut rich = RichText::new(text);
        rich.set_tracked(Some(3..6));
        rich.apply_edit("hi\nsig", "hello\nsig", CharStyle::default());
        assert_eq!(rich.tracked(), Some(6..9));
        // Inside it grows; at its end stays out.
        rich.apply_edit_at("hello\nsig", 8..8, "n", CharStyle::default());
        assert_eq!(rich.tracked(), Some(6..10));
        rich.apply_edit_at("hello\nsing", 10..10, "!", CharStyle::default());
        assert_eq!(rich.tracked(), Some(6..10));
        // Replacing all of it and more leaves it empty where it was.
        rich.apply_edit_at("hello\nsing!", 4..11, "x", CharStyle::default());
        assert_eq!(rich.tracked(), Some(5..5));
        assert_eq!(follow_edit(&(2..5), &(2..4), 1), 3..4);
        assert_eq!(follow_edit(&(2..5), &(3..7), 0), 2..3);
        assert_eq!(follow_edit(&(4..4), &(4..4), 2), 6..6);
    }

    #[test]
    fn a_spliced_document_keeps_its_styles_images_and_whole_lines() {
        let mut doc = image_doc("Bob\n", "");
        doc.rich.update_chars(0..3, |style| style.bold = true);
        doc.rich
            .update_paras(0..1, |para| para.align = Align::Center);
        let text = "Hi\n\n";
        let mut rich = RichText::new(text);
        rich.add_image(InlineImage::new("old", "image/png", vec![9], 1, 1));
        let new_text = rich.splice(text, 4..4, &doc);
        assert_eq!(new_text, format!("Hi\n\nBob\n{IMAGE_CHAR}"));
        assert!(rich.matches(&new_text));
        assert!(rich.all_chars(4..7, |style| style.bold));
        assert!(!rich.all_chars(0..2, |style| style.bold));
        assert_eq!(rich.para(2).align, Align::Center);
        assert_eq!(rich.para(0).align, Align::Left);
        assert_eq!(
            rich.line_image(&new_text, 8..11)
                .map(|image| image.name.as_str()),
            Some("logo.png")
        );

        // Inserted in the middle of a line, that line keeps its style.
        let text = "abcd";
        let mut rich = RichText::new(text);
        rich.update_paras(0..1, |para| para.align = Align::Right);
        let mut doc = RichDocument::plain("x\ny");
        doc.rich
            .update_paras(0..2, |para| para.align = Align::Center);
        let new_text = rich.splice(text, 2..2, &doc);
        assert_eq!(new_text, "abx\nycd");
        assert_eq!(rich.para(0).align, Align::Right);
        assert_eq!(rich.para(1).align, Align::Right);
    }

    #[test]
    fn a_saved_document_round_trips_and_a_damaged_one_is_its_text() {
        let doc = image_doc("Bob\n", "\nCEO");
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("\"data\":\"AQID\""), "{json}");
        let back: RichDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.clone().validated(), doc);
        assert_ne!(back.rich.images[0].id(), doc.rich.images[0].id());

        let mut damaged = doc.clone();
        damaged.text.push('!');
        assert_eq!(
            damaged.validated(),
            RichDocument::plain(&format!("Bob\n{IMAGE_CHAR}\nCEO!"))
        );
        let mut dangling = doc;
        dangling.rich.images.clear();
        assert!(dangling.validated().rich.is_plain());
        assert_eq!(RichDocument::plain("a\r\nb").text, "a\nb");
    }

    #[test]
    fn web_and_mail_addresses_become_links_without_their_punctuation() {
        let line = "Site: www.acme.com, mail (ann@acme.com). See https://acme.com/a?b=1&c!";
        let links = link_ranges(line);
        let found: Vec<(&str, &str)> = links
            .iter()
            .map(|(range, href)| (&line[range.clone()], href.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("www.acme.com", "http://www.acme.com"),
                ("ann@acme.com", "mailto:ann@acme.com"),
                ("https://acme.com/a?b=1&c", "https://acme.com/a?b=1&c"),
            ]
        );
        for plain in ["@", "a@b", "a@.com", "www.", "http://", "x.y", "e-mail"] {
            assert!(link_ranges(plain).is_empty(), "{plain}");
        }
    }

    #[test]
    fn a_link_wraps_its_pieces_in_the_html() {
        let text = "Mail ann@acme.com now";
        let mut rich = RichText::new(text);
        rich.update_chars(5..8, |style| style.bold = true);
        assert!(rich
            .to_html(text)
            .contains("Mail <a href=\"mailto:ann@acme.com\"><b>ann</b>@acme.com</a> now"));
        assert_eq!(rich.to_plain(text), text);
    }

    #[test]
    fn a_run_ending_inside_a_character_makes_a_saved_document_plain() {
        let text = "é".to_string();
        let mut rich = RichText::new(&text);
        rich.runs = vec![(1, BOLD), (1, CharStyle::default())];
        let damaged = RichDocument { text, rich };
        assert_eq!(damaged.validated(), RichDocument::plain("é"));
    }

    #[test]
    fn images_no_longer_shown_are_left_out_of_a_saved_document() {
        let mut doc = image_doc("a\n", "\nb");
        // Two images tried before the one kept, which is now the third.
        doc.rich
            .images
            .insert(0, InlineImage::new("x", "image/png", vec![9], 1, 1));
        doc.rich
            .images
            .insert(0, InlineImage::new("y", "image/png", vec![8], 1, 1));
        doc.rich.update_chars(2..5, |style| style.image = Some(2));
        let compact = doc.clone().validated();
        assert_eq!(compact.rich.images.len(), 1);
        assert_eq!(compact.images()[0].name, "logo.png");
        assert_eq!(
            compact.to_html_with(|image| image.name.clone()),
            doc.to_html_with(|image| image.name.clone())
        );
    }

    #[test]
    fn a_document_starting_or_ending_with_an_image_stays_off_the_text_around() {
        let logo = image_doc("", "");
        let ends_with_logo = image_doc("Thanks\n", "");
        let rich = RichText::new("HiAnn");
        assert_eq!(rich.document_separators("HiAnn", 2..5, &logo), ("\n", ""));
        assert_eq!(
            rich.document_separators("Hi\nAnn", 3..3, &ends_with_logo),
            ("", "\n")
        );
        assert_eq!(
            rich.document_separators("Hi\n\n", 4..4, &ends_with_logo),
            ("", "")
        );
        let plain = RichDocument::plain("Bob");
        assert_eq!(rich.document_separators("HiAnn", 2..5, &plain), ("", ""));
    }
}
