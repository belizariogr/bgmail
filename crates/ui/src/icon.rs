use crate::prelude::*;
use gpui::svg;

/// Sizes available for [`Icon`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum IconSize {
    /// 10px.
    XXSmall,
    /// 12px.
    XSmall,
    /// 14px.
    #[default]
    Small,
    /// 16px.
    Medium,
}

impl IconSize {
    /// Square footprint of the rendered icon.
    fn px(self) -> Pixels {
        match self {
            IconSize::XXSmall => px(10.0),
            IconSize::XSmall => px(12.0),
            IconSize::Small => px(14.0),
            IconSize::Medium => px(16.0),
        }
    }
}

/// Set of icons used by rMail.
///
/// Each icon is an SVG embedded in the binary (see [`crate::Assets`]) and
/// rendered with [`gpui::svg`], which tints it with the icon's color. SVGs are
/// used instead of a glyph font so rendering never depends on a platform font
/// being matched correctly. The enum abstracts the concrete asset path, so the
/// call sites stay stable if an icon's artwork is swapped in the future.
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
    ShieldSolid,
    Check,
}

impl IconName {
    /// Asset path of the icon's SVG, resolvable by [`crate::Assets`].
    pub fn path(self) -> &'static str {
        match self {
            IconName::Inbox => crate::INBOX,
            IconName::Sent => crate::SEND,
            IconName::Drafts => crate::FILE_PEN,
            IconName::Trash => crate::TRASH,
            IconName::Junk => crate::TRIANGLE_EXCLAMATION,
            IconName::Archive => crate::ARCHIVE,
            IconName::Star => crate::STAR,
            IconName::StarFilled => crate::STAR_FILLED,
            IconName::Flag => crate::FLAG,
            IconName::Reply => crate::REPLY,
            IconName::ReplyAll => crate::REPLY_ALL,
            IconName::Forward => crate::FORWARD,
            IconName::Compose => crate::PEN_TO_SQUARE,
            IconName::Search => crate::SEARCH,
            IconName::Settings => crate::SETTINGS,
            IconName::Refresh => crate::REFRESH,
            IconName::ChevronRight => crate::CHEVRON_RIGHT,
            IconName::ChevronDown => crate::CHEVRON_DOWN,
            IconName::Account => crate::CIRCLE_USER,
            IconName::Attachment => crate::PAPERCLIP,
            IconName::Mail => crate::ENVELOPE,
            IconName::Sidebar => crate::SIDEBAR,
            IconName::Filter => crate::FILTER,
            IconName::More => crate::ELLIPSIS,
            IconName::Folder => crate::FOLDER,
            IconName::Shield => crate::SHIELD_HALVED,
            IconName::ShieldSolid => crate::SHIELD,
            IconName::Check => crate::CHECK,
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
        let size = self.size.px();
        svg()
            .flex_none()
            .w(size)
            .h(size)
            .path(self.name.path())
            .text_color(color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Assets;
    use gpui::AssetSource;

    /// Every icon variant, so coverage tests can iterate over the full set.
    const ALL: [IconName; 28] = [
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
        IconName::ShieldSolid,
        IconName::Check,
    ];

    #[test]
    fn every_icon_points_to_an_svg_asset() {
        for name in ALL {
            let path = name.path();
            assert!(
                path.ends_with(".svg"),
                "{name:?} path is not an SVG: {path}"
            );
        }
    }

    /// The asset source must be able to resolve every icon's SVG. This is what
    /// keeps the font-free icons from regressing into the "tofu"/blank state the
    /// glyph font used to cause when it failed to match on a platform.
    #[test]
    fn every_icon_resolves_to_embedded_bytes() {
        for name in ALL {
            let bytes = Assets
                .load(name.path())
                .expect("load must not error")
                .unwrap_or_else(|| panic!("{name:?} has no embedded SVG at {}", name.path()));
            assert!(
                bytes.starts_with(b"<svg"),
                "{name:?} asset is not an SVG document"
            );
        }
    }

    #[test]
    fn star_variants_use_distinct_assets() {
        assert_ne!(IconName::Star.path(), IconName::StarFilled.path());
    }

    #[test]
    fn shield_variants_use_distinct_assets() {
        assert_ne!(IconName::Shield.path(), IconName::ShieldSolid.path());
    }

    #[test]
    fn sizes_are_distinct_and_ascending() {
        assert!(IconSize::XXSmall.px() < IconSize::XSmall.px());
        assert!(IconSize::XSmall.px() < IconSize::Small.px());
        assert!(IconSize::Small.px() < IconSize::Medium.px());
    }
}
