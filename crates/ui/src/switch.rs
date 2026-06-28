use gpui::{ClickEvent, MouseButton, SharedString};

use crate::prelude::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A sliding on/off toggle, matching the switches in Zed's settings (e.g. the
/// "Vim Mode" toggle): a pill-shaped track with a circular thumb that slides to
/// the right when on. An optional label sits to its right and the whole row is
/// clickable.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Switch {
    /// Creates a switch in the given on/off state.
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            label: None,
            on_click: None,
        }
    }

    /// Adds a text label shown to the right of the track.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Registers the click/toggle handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let is_on = self.checked;

        let thumb_color = colors.text;
        let thumb_opacity = if is_on { 1.0 } else { 0.5 };

        // On: a soft accent-tinted track; off: a neutral filled track. Mirrors
        // Zed's translucent fill so the thumb (the theme's text color) reads on
        // both states.
        let (bg_color, border_color) = if is_on {
            (colors.accent.opacity(0.4), colors.accent.opacity(0.6))
        } else {
            (colors.element_background, colors.border)
        };
        let hover_bg = if is_on {
            bg_color.blend(colors.text.opacity(0.12))
        } else {
            bg_color.blend(colors.text.opacity(0.06))
        };

        let group_id = SharedString::from(format!("switch-group-{:?}", self.id));

        let track = h_flex()
            .group(group_id.clone())
            .w(px(32.0))
            .h(px(20.0))
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .when(is_on, |el| el.justify_end())
                    .when(!is_on, |el| el.justify_start())
                    .rounded_full()
                    .px(px(2.0))
                    .bg(bg_color)
                    .border_1()
                    .border_color(border_color)
                    .group_hover(group_id.clone(), move |el| el.bg(hover_bg))
                    .child(
                        div()
                            .size(px(12.0))
                            .rounded_full()
                            .bg(thumb_color)
                            .opacity(thumb_opacity),
                    ),
            );

        h_flex()
            .id(self.id)
            .gap_2()
            .items_center()
            .child(track)
            .when_some(self.label, |el, label| {
                el.child(Label::new(label).size(LabelSize::Small))
            })
            .when_some(self.on_click, |el, handler| {
                // Swallow the mouse-down so an enclosing draggable container can't
                // treat the toggle as the start of a drag (mirrors `Button`).
                el.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
