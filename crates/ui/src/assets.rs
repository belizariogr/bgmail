//! Embedded SVG assets bundled with the UI crate.
//!
//! GPUI's [`gpui::svg`] element loads its shape from the app's [`AssetSource`] by
//! path, then tints it with the element's `text_color` (the colors declared
//! inside the SVG are ignored — only the alpha coverage matters). Every UI icon
//! ships as an SVG here instead of a font glyph, so rendering never depends on a
//! platform font being matched correctly (which broke on some platforms).

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Declares the embedded icon SVGs.
///
/// For each entry it emits a `pub const` holding the asset path (so call sites
/// and [`crate::IconName`] can reference icons by name) and embeds the file
/// bytes at compile time. The generated `ICON_ASSETS` table backs
/// [`Assets::load`]/[`Assets::list`], so adding an icon is a single line here
/// plus the matching file under `assets/icons/`.
macro_rules! icon_assets {
    ($(($name:ident, $path:literal)),* $(,)?) => {
        $(
            #[doc = concat!("Asset path of `", $path, "`.")]
            pub const $name: &str = $path;
        )*

        const ICON_ASSETS: &[(&str, &[u8])] = &[
            $(($path, include_bytes!(concat!("../../../assets/", $path))),)*
        ];
    };
}

icon_assets! {
    (CHEVRON_RIGHT, "icons/chevron-right.svg"),
    (CHEVRON_DOWN, "icons/chevron-down.svg"),
    (INBOX, "icons/inbox.svg"),
    (SEND, "icons/send.svg"),
    (FILE_PEN, "icons/file-pen.svg"),
    (TRASH, "icons/trash.svg"),
    (TRIANGLE_EXCLAMATION, "icons/triangle-exclamation.svg"),
    (ARCHIVE, "icons/archive.svg"),
    (STAR, "icons/star.svg"),
    (STAR_FILLED, "icons/star-filled.svg"),
    (PALETTE, "icons/palette.svg"),
    (FLAG, "icons/flag.svg"),
    (REPLY, "icons/reply.svg"),
    (REPLY_ALL, "icons/reply-all.svg"),
    (FORWARD, "icons/forward.svg"),
    (PEN_TO_SQUARE, "icons/pen-to-square.svg"),
    (SEARCH, "icons/search.svg"),
    (SETTINGS, "icons/settings.svg"),
    (REFRESH, "icons/refresh.svg"),
    (CIRCLE_USER, "icons/circle-user.svg"),
    (PAPERCLIP, "icons/paperclip.svg"),
    (ENVELOPE, "icons/envelope.svg"),
    (SIDEBAR, "icons/sidebar.svg"),
    (FILTER, "icons/filter.svg"),
    (ELLIPSIS, "icons/ellipsis.svg"),
    (FOLDER, "icons/folder.svg"),
    (SHIELD_HALVED, "icons/shield-halved.svg"),
    (SHIELD, "icons/shield.svg"),
    (CHECK, "icons/check.svg"),
    (XMARK, "icons/xmark.svg"),
    (WINDOW_MINIMIZE, "icons/window-minimize.svg"),
    (WINDOW_MAXIMIZE, "icons/window-maximize.svg"),
    (WINDOW_RESTORE, "icons/window-restore.svg"),
}

/// Asset source that serves the UI crate's embedded SVGs. Register it once with
/// `gpui::Application::new().with_assets(ui::Assets)` before opening windows.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICON_ASSETS
            .iter()
            .find(|(asset_path, _)| *asset_path == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(ICON_ASSETS
            .iter()
            .map(|(asset_path, _)| SharedString::from(*asset_path))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_every_embedded_icon() {
        for (path, _) in ICON_ASSETS {
            let bytes = Assets
                .load(path)
                .expect("load must not error")
                .unwrap_or_else(|| panic!("missing asset bytes for {path}"));
            assert!(!bytes.is_empty(), "{path} is empty");
            assert!(bytes.starts_with(b"<svg"), "{path} is not an SVG document");
        }
    }

    #[test]
    fn unknown_path_is_none() {
        assert!(Assets.load("icons/does-not-exist.svg").unwrap().is_none());
    }

    #[test]
    fn list_reports_every_icon() {
        let listed = Assets.list("").unwrap();
        assert_eq!(listed.len(), ICON_ASSETS.len());
    }
}
