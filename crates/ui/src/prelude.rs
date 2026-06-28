//! Prelúdio do crate `ui`. Importe com `use ui::prelude::*;` ao construir telas.

pub use gpui::prelude::*;
pub use gpui::{
    div, px, relative, rems, App, Context, Div, Element, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window,
};

pub use theme::ActiveTheme;

pub use crate::{Button, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, ListItem};

/// Container flexível horizontal com itens centralizados verticalmente.
///
/// Equivalente ao `h_flex()` do Zed.
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// Container flexível vertical.
///
/// Equivalente ao `v_flex()` do Zed.
pub fn v_flex() -> Div {
    div().flex().flex_col()
}
