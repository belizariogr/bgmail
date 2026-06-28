//! # UI
//!
//! rMail's visual component library, inspired by Zed's `ui` crate.
//!
//! Provides a small, reusable set of components (`Label`, `Icon`, `Button`,
//! `IconButton`, `ListItem`) and layout helpers (`h_flex`, `v_flex`), all
//! integrated with the theme system (`theme`).
//!
//! The goal is to keep the API close to Zed's to ease porting patterns, while
//! avoiding the "bloat" of features an e-mail client does not use.

pub mod prelude;

mod assets;
mod button;
mod color;
mod icon;
mod label;
mod list_item;
mod scrollbar;

pub use assets::*;
pub use button::*;
pub use color::*;
pub use icon::*;
pub use label::*;
pub use list_item::*;
pub use scrollbar::*;

pub use prelude::{h_flex, v_flex};

// Re-export the most used theme types for consumer convenience.
pub use theme::{ActiveTheme, Appearance, Theme, ThemeColors};

/// Initializes the UI crate by registering the icon fonts (FontAwesome) in
/// GPUI's text system. Must be called once, before opening windows.
pub fn init(cx: &mut gpui::App) {
    icon::init_fonts(cx);
}
