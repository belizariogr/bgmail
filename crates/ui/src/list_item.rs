use gpui::{px, AnyElement, ClickEvent, Pixels};

use crate::prelude::*;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A clickable list row, with hover and selection states.
///
/// Accepts a start *slot* (usually an [`Icon`]), main content and an end *slot*
/// (badge/counter). It is the basis for the sidebar lists and the message list.
#[derive(IntoElement)]
pub struct ListItem {
    id: ElementId,
    selected: bool,
    inset: Pixels,
    start_slot: Option<AnyElement>,
    end_slot: Option<AnyElement>,
    children: Vec<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl ListItem {
    /// Creates a new list row.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            selected: false,
            inset: px(0.0),
            start_slot: None,
            end_slot: None,
            children: Vec::new(),
            on_click: None,
        }
    }

    /// Marks the row as selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Extra left padding applied to the row's content (start slot, text, …)
    /// without moving the row's hover/selection background, which keeps filling
    /// the full width. Useful to inset content while the highlight stays put.
    pub fn inset(mut self, inset: Pixels) -> Self {
        self.inset = inset;
        self
    }

    /// Sets the start slot (icon on the left).
    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start_slot = Some(element.into_any_element());
        self
    }

    /// Sets the end slot (badge/counter on the right).
    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end_slot = Some(element.into_any_element());
        self
    }

    /// Adds an element to the main content.
    pub fn child(mut self, element: impl IntoElement) -> Self {
        self.children.push(element.into_any_element());
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

impl RenderOnce for ListItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let selected_bg = colors.element_selected;
        let hover_bg = colors.element_hover;

        h_flex()
            .id(self.id)
            .w_full()
            .gap_2()
            .pl(px(8.0) + self.inset)
            .pr_2()
            .py_1()
            .rounded_md()
            .when(self.selected, |el| el.bg(selected_bg))
            .when(!self.selected, |el| el.hover(move |el| el.bg(hover_bg)))
            .when_some(self.start_slot, |el, slot| el.child(slot))
            .child(
                // Main content takes the remaining space.
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .children(self.children),
            )
            .when_some(self.end_slot, |el, slot| el.child(slot))
            .when_some(self.on_click, |el, handler| {
                el.cursor_pointer()
                    .on_click(move |event, window, cx| handler(event, window, cx))
            })
    }
}
