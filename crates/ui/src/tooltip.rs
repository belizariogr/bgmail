use gpui::{AnyView, App, Render, Window};

use crate::prelude::*;
use crate::{Color, Label, LabelSize};

/// A minimal text tooltip, themed to match the surrounding UI.
///
/// Mirrors the single use we need from Zed's richer `Tooltip`: a short hint
/// string shown on hover. Build it with [`Tooltip::text`] and hand the closure
/// to `div().tooltip(...)`.
pub struct Tooltip {
    text: SharedString,
    shortcut: Option<SharedString>,
}

impl Tooltip {
    /// Returns a builder suitable for `Element::tooltip`, rendering `text` in a
    /// themed container.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        Self::build(text, None)
    }

    /// Like [`Self::text`], with a keyboard shortcut shown on the right.
    pub fn with_shortcut(
        text: impl Into<SharedString>,
        shortcut: impl Into<SharedString>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView {
        Self::build(text, Some(shortcut.into()))
    }

    fn build(
        text: impl Into<SharedString>,
        shortcut: Option<SharedString>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let text = text.into();
        move |_, cx| {
            cx.new(|_| Tooltip {
                text: text.clone(),
                shortcut: shortcut.clone(),
            })
            .into()
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let body = match &self.shortcut {
            Some(shortcut) => h_flex()
                .gap_4()
                .items_center()
                .child(Label::new(self.text.clone()).size(LabelSize::Small))
                .child(
                    Label::new(shortcut.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
            None => Label::new(self.text.clone())
                .size(LabelSize::Small)
                .into_any_element(),
        };

        div()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .px_2()
            .py_1()
            .child(body)
    }
}
