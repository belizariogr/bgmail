//! Native GPUI rendering of HTML documents (e-mail bodies).
//!
//! Blitz parses, styles and lays out the HTML; a painter derived from
//! `blitz-paint` records a [`DisplayList`] that is replayed with GPUI
//! primitives, and text is shaped and drawn by GPUI's own text system.

#[allow(dead_code, clippy::all, unused_imports)]
mod blitz_paint;
pub mod display_list;
pub mod document;
pub mod element;
mod fonts;
pub mod gradient;
pub mod paint;
pub mod resources;
pub mod text;

pub use display_list::DisplayList;
pub use document::{Appearance, DocumentMode, DocumentOptions, HtmlDocument};
pub use element::{HtmlElement, HtmlView};
pub use resources::Resources;
