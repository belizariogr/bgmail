//! Native GPUI rendering of HTML documents (e-mail bodies).
//!
//! Blitz parses, styles and lays out the HTML; a painter derived from
//! `blitz-paint` records a [`DisplayList`] that is replayed with GPUI
//! primitives, and text is shaped and drawn by GPUI's own text system.

#[allow(dead_code, clippy::all, unused_imports)]
mod blitz_paint;
pub mod contrast;
pub mod display_list;
pub mod document;
pub mod element;
mod fonts;
pub mod gradient;
pub mod interaction;
pub mod paint;
pub mod remote;
pub mod resources;
pub mod text;

pub use contrast::DarkPage;
pub use display_list::DisplayList;
pub use document::{
    has_too_many_tags, with_render_stack, Appearance, DetachedDocument, DocumentMode,
    DocumentOptions, HtmlDocument, RenderError, DEFAULT_SELECTION_COLOR, MAX_NESTING_DEPTH,
    MAX_TAG_ATTRIBUTES,
};
pub use element::{init, HtmlElement, HtmlView, HtmlViewEvent};
pub use interaction::{HitInfo, PointerButton, PointerInput, PointerModifiers, PointerPhase};
pub use resources::Resources;
