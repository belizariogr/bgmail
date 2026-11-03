//! GPUI-free model of formatted text: character styles and paragraph styles over
//! the plain text of a [`super::buffer::TextBuffer`], and its export to e-mail.
//!
//! Line breaks live in the text; the model only knows its byte lengths and which
//! line is which. Sizes belong to paragraphs because GPUI shapes a line at one
//! font size.

use std::ops::Range;

/// Font of a piece of text: the set Gmail offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// List a paragraph is an item of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListKind {
    Bullet,
    Numbered,
}

/// Highest indent level.
pub const MAX_INDENT: u8 = 8;

/// Style of a run of characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CharStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub family: FontFamily,
    /// `0xRRGGBB`; `None` is the page's text color.
    pub color: Option<u32>,
}

/// Style of a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
/// or mergeable neighbors; `paras` has one entry per line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichText {
    runs: Vec<(usize, CharStyle)>,
    paras: Vec<ParaStyle>,
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

    /// Style of what is typed at `offset`: that of the character before it.
    pub fn style_at(&self, offset: usize) -> CharStyle {
        let mut end = 0;
        for (len, style) in &self.runs {
            end += len;
            if offset <= end {
                return *style;
            }
        }
        self.runs
            .last()
            .map(|(_, style)| *style)
            .unwrap_or_default()
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

    /// `text` for the plain-text version of an e-mail: lists and indents as text.
    pub fn to_plain(&self, text: &str) -> String {
        text.split('\n')
            .enumerate()
            .map(|(line, content)| {
                let indent = "    ".repeat(usize::from(self.para(line).indent));
                match self.list_marker(line) {
                    Some(marker) => format!("{indent}{marker} {content}"),
                    None => format!("{indent}{content}"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `text` as e-mail HTML.
    pub fn to_html(&self, text: &str) -> String {
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
            if content.is_empty() {
                out.push_str("<br>");
            }
            for (run, style) in self.runs_in(range) {
                push_styled(&mut out, &text[run], &style);
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
}
