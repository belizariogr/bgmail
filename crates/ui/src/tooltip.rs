use gpui::{AnyView, App, Render, Window};

use crate::prelude::*;
use crate::{Label, LabelSize};

/// A minimal text tooltip, themed to match the surrounding UI.
///
/// Mirrors the single use we need from Zed's richer `Tooltip`: a short hint
/// string shown on hover. Build it with [`Tooltip::text`] and hand the closure
/// to `div().tooltip(...)`.
pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    /// Returns a builder suitable for `Element::tooltip`, rendering `text` in a
    /// themed container.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let text = text.into();
        move |_, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        div()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .px_2()
            .py_1()
            .child(Label::new(self.text.clone()).size(LabelSize::Small))
    }
}
