use gpui::{ClickEvent, MouseButton};

use crate::prelude::*;

/// Visual style of a [`Button`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ButtonStyle {
    /// Subtle background that stands out from the surface (secondary action).
    #[default]
    Subtle,
    /// Solid accent background (primary action).
    Filled,
    /// No background until hovered (tertiary / toolbar action).
    Ghost,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A button with a text label.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    style: ButtonStyle,
    full_width: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    /// Creates a new button.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            style: ButtonStyle::default(),
            full_width: false,
            on_click: None,
        }
    }

    /// Prepends an icon before the label (e.g. the compose Send button).
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets the button style.
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// Makes the button span the full available width.
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    /// Registers the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (bg, text_color, hover_bg) = match self.style {
            ButtonStyle::Subtle => (colors.element_background, colors.text, colors.element_hover),
            ButtonStyle::Filled => (colors.accent, colors.text_on_accent, colors.accent),
            ButtonStyle::Ghost => (gpui::transparent_black(), colors.text, colors.element_hover),
        };

        h_flex()
            .id(self.id)
            .justify_center()
            .gap_1()
            .px_3()
            .py_1p5()
            .rounded_md()
            .bg(bg)
            .text_size(crate::ui_text_size(-1.0))
            .text_color(text_color)
            .when(self.full_width, |el| el.w_full())
            .hover(move |el| el.bg(hover_bg))
            .when_some(self.icon, |el, icon| {
                let icon_color = if self.style == ButtonStyle::Filled {
                    Color::OnAccent
                } else {
                    Color::Default
                };
                el.child(Icon::new(icon).size(IconSize::Small).color(icon_color))
            })
            .child(self.label)
            .when_some(self.on_click, |el, handler| {
                // Swallow the mouse-down so an enclosing draggable container (e.g.
                // the title bar) doesn't treat clicking the button as the start of
                // a window drag. The click itself still fires (its pending-down is
                // recorded before this runs).
                el.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

/// A button made of only an icon (used in toolbars).
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    size: IconSize,
    color: Color,
    selected: bool,
    disabled: bool,
    hover_color: Option<Color>,
    compact: bool,
    on_click: Option<ClickHandler>,
}

/// Group of an icon button whose icon reacts to the hover.
const ICON_BUTTON_GROUP: &str = "icon-button";

impl IconButton {
    /// Creates a new icon button.
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            size: IconSize::default(),
            color: Color::Default,
            selected: false,
            disabled: false,
            hover_color: None,
            compact: false,
            on_click: None,
        }
    }

    /// Sets the icon size.
    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    /// Sets the icon color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Marks the button as selected/active.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Disables the button (as Zed's `Disableable`): dimmed icon, no hover
    /// and the click handler is dropped.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Hover feedback through the icon color instead of a background, for
    /// buttons over surfaces whose color varies (e.g. a selected list row).
    pub fn hover_color(mut self, color: Color) -> Self {
        self.hover_color = Some(color);
        self
    }

    /// Makes the button exactly as big as its icon, for tight spots.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    /// Side of the button's square.
    fn side(&self) -> Pixels {
        if self.compact {
            self.size.px()
        } else {
            px(28.0)
        }
    }

    /// Registers the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Icon color after the selected/disabled states are applied.
    fn effective_color(&self) -> Color {
        if self.disabled {
            Color::Disabled
        } else if self.selected {
            Color::Accent
        } else {
            self.color
        }
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let hover_bg = colors.element_hover;
        let selected_bg = colors.element_active;
        let color = self.effective_color();
        let side = self.side();
        let disabled = self.disabled;
        let hover_color = self.hover_color.filter(|_| !disabled);
        let on_click = self.on_click.filter(|_| !disabled);
        let mut icon = Icon::new(self.icon).size(self.size).color(color);
        if let Some(hover_color) = hover_color {
            icon = icon.group_hover_color(ICON_BUTTON_GROUP, hover_color);
        }

        h_flex()
            .id(self.id)
            .group(ICON_BUTTON_GROUP)
            .justify_center()
            .size(side)
            .rounded_md()
            .when(self.selected && !disabled, |el| el.bg(selected_bg))
            .when(!disabled && hover_color.is_none(), |el| {
                el.hover(move |el| el.bg(hover_bg))
            })
            // A dead button still must not start a title-bar window drag.
            .when(disabled, |el| {
                el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
            .child(icon)
            .when_some(on_click, |el, handler| {
                // See `Button`: keep title-bar dragging from swallowing button clicks.
                el.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_icon_button_is_the_size_of_its_icon() {
        let button = IconButton::new("b", IconName::Reply).size(IconSize::Small);
        assert_eq!(button.side(), px(28.0));
        assert_eq!(button.compact().side(), IconSize::Small.px());
    }

    #[test]
    fn icon_button_color_follows_disabled_then_selected() {
        let button = IconButton::new("b", IconName::Reply).color(Color::Muted);
        assert_eq!(button.effective_color(), Color::Muted);
        let button = button.selected(true);
        assert_eq!(button.effective_color(), Color::Accent);
        let button = button.disabled(true);
        assert_eq!(button.effective_color(), Color::Disabled);
    }
}
