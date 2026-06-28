//! Embedded SVG assets bundled with the UI crate.
//!
//! GPUI's [`gpui::svg`] element loads its shape from the app's [`AssetSource`] by
//! path. Font-glyph icons (see [`crate::Icon`]) can't be rotated, so disclosure
//! controls that need a rotation animation use a real SVG served from here.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Right-pointing chevron used by collapsible/disclosure controls. Rotating it
/// 90° turns it into a down chevron, which the sidebar uses to animate folds.
const CHEVRON_RIGHT_SVG: &[u8] = include_bytes!("../../../assets/icons/chevron-right.svg");

/// Asset path of the right-pointing chevron (see [`Assets::load`]).
pub const CHEVRON_RIGHT: &str = "icons/chevron-right.svg";

/// Asset source that serves the UI crate's embedded SVGs. Register it once with
/// `gpui::Application::new().with_assets(ui::Assets)` before opening windows.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            CHEVRON_RIGHT => Some(Cow::Borrowed(CHEVRON_RIGHT_SVG)),
            _ => None,
        })
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![SharedString::from(CHEVRON_RIGHT)])
    }
}
