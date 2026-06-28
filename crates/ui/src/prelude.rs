//! Prelude for the `ui` crate. Import it with `use ui::prelude::*;` when building screens.

pub use gpui::prelude::*;
pub use gpui::{
    div, px, relative, rems, App, Context, Div, Element, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window,
};

pub use theme::ActiveTheme;

pub use crate::{Button, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, ListItem};

/// Horizontal flex container with vertically centered items.
///
/// Equivalent to Zed's `h_flex()`.
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// Vertical flex container.
///
/// Equivalent to Zed's `v_flex()`.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}
