//! # UI
//!
//! BGMail's visual component library, inspired by Zed's `ui` crate.
//!
//! Provides a small, reusable set of components (`Label`, `Icon`, `Button`,
//! `IconButton`, `ListItem`, `ContextMenu`, `TextInput`, `TextArea`) and layout helpers (`h_flex`, `v_flex`), all
//! integrated with the theme system (`theme`).
//!
//! The goal is to keep the API close to Zed's to ease porting patterns, while
//! avoiding the "bloat" of features an e-mail client does not use.

pub mod prelude;

mod assets;
mod button;
mod color;
mod context_menu;
mod icon;
mod label;
mod list_item;
pub mod prompt;
mod scrollbar;
mod switch;
pub mod text_area;
mod text_input;
mod text_size;
mod tooltip;

pub use assets::*;
pub use button::*;
pub use color::*;
pub use context_menu::*;
pub use icon::*;
pub use label::*;
pub use list_item::*;
pub use scrollbar::*;
pub use switch::*;
pub use text_area::rich::{
    Align, CharStyle, FontFamily, Format, ListKind, ParaStyle, TextSize, MAX_INDENT,
};
pub use text_area::{TextArea, TextAreaEvent};
pub use text_input::*;
pub use text_size::*;
pub use tooltip::*;

pub use prelude::{h_flex, v_flex};

/// Registers the key bindings of the text components ([`TextInput`] and
/// [`TextArea`]). Call once at app startup.
pub fn bind_keys(cx: &mut gpui::App) {
    text_input::bind_keys(cx);
    text_area::bind_keys(cx);
}

// Re-export the most used theme types for consumer convenience.
pub use theme::{ActiveTheme, Appearance, Theme, ThemeColors};
