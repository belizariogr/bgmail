use crate::prelude::*;
use gpui::FontWeight;
use std::borrow::Cow;

/// Single typographic family of FontAwesome 6 Free. The solid (weight
/// `Black`/900) and regular (weight `Normal`/400) variants share this name; the
/// style is selected by the font weight (see [`FaStyle`]).
const FA_FAMILY: &str = "Font Awesome 6 Free";

/// Glyph style within the FontAwesome 6 Free family.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FaStyle {
    /// Filled variant (weight 900).
    Solid,
    /// Outline variant (weight 400).
    Regular,
}

impl FaStyle {
    fn weight(self) -> FontWeight {
        match self {
            FaStyle::Solid => FontWeight::BLACK,
            FaStyle::Regular => FontWeight::NORMAL,
        }
    }
}

/// FontAwesome font bytes embedded in the binary (from `assets/fonts`).
const FA_SOLID_TTF: &[u8] = include_bytes!("../../../assets/fonts/fa-solid-900.ttf");
const FA_REGULAR_TTF: &[u8] = include_bytes!("../../../assets/fonts/fa-regular-400.ttf");

/// Registers the icon fonts in GPUI's text system.
///
/// Must be called once during startup, before opening windows. Without it the
/// glyphs render as "tofu" (empty rectangles).
pub(crate) fn init_fonts(cx: &mut App) {
    let fonts = vec![Cow::Borrowed(FA_SOLID_TTF), Cow::Borrowed(FA_REGULAR_TTF)];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("rMail: failed to register FontAwesome fonts: {err:?}");
    }
}

/// Sizes available for [`Icon`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum IconSize {
    /// 12px.
    XSmall,
    /// 14px.
    #[default]
    Small,
    /// 16px.
    Medium,
}

impl IconSize {
    /// Size of the glyph itself.
    fn glyph(self) -> Pixels {
        match self {
            IconSize::XSmall => px(12.0),
            IconSize::Small => px(14.0),
            IconSize::Medium => px(16.0),
        }
    }
}

/// Set of icons used by rMail.
///
/// Icons are rendered as glyphs from the FontAwesome 6 Free font (loaded by
/// [`init_fonts`]). The enum abstracts the concrete family and codepoint, so the
/// call sites stay stable if the font is swapped in the future.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconName {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    Star,
    StarFilled,
    Flag,
    Reply,
    ReplyAll,
    Forward,
    Compose,
    Search,
    Settings,
    Refresh,
    ChevronRight,
    ChevronDown,
    Account,
    Attachment,
    Mail,
    Sidebar,
    Filter,
    More,
    Folder,
    Shield,
}

impl IconName {
    /// Style (solid/regular) and codepoint (private use area) of the FontAwesome glyph.
    fn glyph(self) -> (FaStyle, char) {
        match self {
            IconName::Inbox => (FaStyle::Solid, '\u{f01c}'), // inbox
            IconName::Sent => (FaStyle::Solid, '\u{f1d8}'),  // paper-plane
            IconName::Drafts => (FaStyle::Solid, '\u{f31c}'), // file-pen
            IconName::Trash => (FaStyle::Solid, '\u{f1f8}'), // trash
            IconName::Junk => (FaStyle::Solid, '\u{f071}'),  // triangle-exclamation
            IconName::Archive => (FaStyle::Solid, '\u{f187}'), // box-archive
            IconName::Star => (FaStyle::Regular, '\u{f005}'), // star (outline)
            IconName::StarFilled => (FaStyle::Solid, '\u{f005}'), // star (filled)
            IconName::Flag => (FaStyle::Solid, '\u{f024}'),  // flag
            IconName::Reply => (FaStyle::Solid, '\u{f112}'), // reply
            IconName::ReplyAll => (FaStyle::Solid, '\u{f122}'), // reply-all
            IconName::Forward => (FaStyle::Solid, '\u{f064}'), // share
            IconName::Compose => (FaStyle::Solid, '\u{f044}'), // pen-to-square
            IconName::Search => (FaStyle::Solid, '\u{f002}'), // magnifying-glass
            IconName::Settings => (FaStyle::Solid, '\u{f013}'), // gear
            IconName::Refresh => (FaStyle::Solid, '\u{f021}'), // arrows-rotate
            IconName::ChevronRight => (FaStyle::Solid, '\u{f054}'), // chevron-right
            IconName::ChevronDown => (FaStyle::Solid, '\u{f078}'), // chevron-down
            IconName::Account => (FaStyle::Solid, '\u{f2bd}'), // circle-user
            IconName::Attachment => (FaStyle::Solid, '\u{f0c6}'), // paperclip
            IconName::Mail => (FaStyle::Solid, '\u{f0e0}'),  // envelope
            IconName::Sidebar => (FaStyle::Solid, '\u{f0db}'), // table-columns
            IconName::Filter => (FaStyle::Solid, '\u{f0b0}'), // filter
            IconName::More => (FaStyle::Solid, '\u{f141}'),  // ellipsis
            IconName::Folder => (FaStyle::Solid, '\u{f07b}'), // folder
            IconName::Shield => (FaStyle::Solid, '\u{f3ed}'), // shield-halved
        }
    }
}

/// A themed icon.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: IconSize,
    color: Color,
}

impl Icon {
    /// Creates a new icon.
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: IconSize::default(),
            color: Color::default(),
        }
    }

    /// Sets the icon size.
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// Sets the icon's semantic color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.hsla(cx);
        let (style, glyph) = self.name.glyph();
        div()
            .flex()
            .items_center()
            .justify_center()
            .font_family(FA_FAMILY)
            .font_weight(style.weight())
            .text_size(self.size.glyph())
            .text_color(color)
            .child(glyph.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon must map to a codepoint in the Private Use Area (where
    /// FontAwesome places its glyphs).
    const ALL: [IconName; 26] = [
        IconName::Inbox,
        IconName::Sent,
        IconName::Drafts,
        IconName::Trash,
        IconName::Junk,
        IconName::Archive,
        IconName::Star,
        IconName::StarFilled,
        IconName::Flag,
        IconName::Reply,
        IconName::ReplyAll,
        IconName::Forward,
        IconName::Compose,
        IconName::Search,
        IconName::Settings,
        IconName::Refresh,
        IconName::ChevronRight,
        IconName::ChevronDown,
        IconName::Account,
        IconName::Attachment,
        IconName::Mail,
        IconName::Sidebar,
        IconName::Filter,
        IconName::More,
        IconName::Folder,
        IconName::Shield,
    ];

    #[test]
    fn every_icon_maps_to_pua_codepoint() {
        for name in ALL {
            let (_style, glyph) = name.glyph();
            let cp = glyph as u32;
            assert!(
                (0xE000..=0xF8FF).contains(&cp),
                "{name:?} has a codepoint outside the PUA: U+{cp:04X}"
            );
        }
    }

    #[test]
    fn star_variants_share_codepoint_but_differ_in_style() {
        let (regular_style, regular_glyph) = IconName::Star.glyph();
        let (solid_style, solid_glyph) = IconName::StarFilled.glyph();
        assert_eq!(regular_glyph, solid_glyph);
        assert_eq!(regular_style, FaStyle::Regular);
        assert_eq!(solid_style, FaStyle::Solid);
    }

    #[test]
    fn styles_map_to_distinct_weights() {
        assert_eq!(FaStyle::Solid.weight(), FontWeight::BLACK);
        assert_eq!(FaStyle::Regular.weight(), FontWeight::NORMAL);
        assert_ne!(FaStyle::Solid.weight(), FaStyle::Regular.weight());
    }
}
