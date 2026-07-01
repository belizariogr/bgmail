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
            .text_size(px(13.0))
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
    on_click: Option<ClickHandler>,
}

impl IconButton {
    /// Creates a new icon button.
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            size: IconSize::default(),
            color: Color::Default,
            selected: false,
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

    /// Registers the click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let hover_bg = colors.element_hover;
        let selected_bg = colors.element_active;
        let color = if self.selected {
            Color::Accent
        } else {
            self.color
        };

        h_flex()
            .id(self.id)
            .justify_center()
            .size(px(28.0))
            .rounded_md()
            .when(self.selected, |el| el.bg(selected_bg))
            .hover(move |el| el.bg(hover_bg))
            .child(Icon::new(self.icon).size(self.size).color(color))
            .when_some(self.on_click, |el, handler| {
                // See `Button`: keep title-bar dragging from swallowing button clicks.
                el.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
