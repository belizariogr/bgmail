//! The GPUI element and view that display an [`HtmlDocument`].

use crate::document::HtmlDocument;
use crate::paint::{paint_display_list, PaintCache};
use gpui::{
    div, px, relative, size, App, AvailableSpace, Bounds, Context, Element, ElementId,
    GlobalElementId, InspectorElementId, InteractiveElement, IntoElement, LayoutId, ParentElement,
    Pixels, Render, ScrollHandle, StatefulInteractiveElement, Style, Styled, Window,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Width used when the parent gives no definite width (should not happen in a
/// laid-out reader pane, but keeps measurement total).
const FALLBACK_WIDTH: f32 = 800.0;

/// Picks the width to lay the document out at from GPUI's constraints.
pub fn layout_width(known: Option<Pixels>, available: AvailableSpace) -> f32 {
    known
        .or(match available {
            AvailableSpace::Definite(width) => Some(width),
            _ => None,
        })
        .map(f32::from)
        .filter(|w| *w > 0.0)
        .unwrap_or(FALLBACK_WIDTH)
}

/// Height to paint the document canvas at: the element's own height, or down
/// to the bottom of the visible area if that is lower.
pub fn canvas_height(element_height: Pixels, element_top: Pixels, visible_bottom: Pixels) -> f32 {
    f32::from(element_height).max(f32::from(visible_bottom - element_top))
}

/// Whether the document's content width moved away from the one the view was
/// rendered with (its overflow spacer is then stale and needs a new frame).
pub fn content_width_changed(rendered: f32, current: f32) -> bool {
    (rendered - current).abs() >= 0.5
}

/// Element that lays out a document at the pane width and paints it at its
/// full content size (wider when the mail overflows). Put it inside a
/// scrolling container (see [`HtmlView`], which also makes the overflow
/// scrollable).
pub struct HtmlElement {
    document: Rc<RefCell<HtmlDocument>>,
    cache: Rc<RefCell<PaintCache>>,
    /// Content width when this element was built (by the view's render).
    rendered_content_width: f32,
}

impl HtmlElement {
    pub fn new(document: Rc<RefCell<HtmlDocument>>, cache: Rc<RefCell<PaintCache>>) -> Self {
        let rendered_content_width = document.borrow().content_width();
        Self {
            document,
            cache,
            rendered_content_width,
        }
    }
}

impl IntoElement for HtmlElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for HtmlElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let document = self.document.clone();
        let layout_id = window.request_measured_layout(style, move |known, available, _, _| {
            let width = layout_width(known.width, available.width);
            let height = document.borrow_mut().layout(width);
            size(px(width), px(height))
        });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        // Layout just ran: re-render if the overflow spacer is out of date.
        let current = self.document.borrow().content_width();
        if content_width_changed(self.rendered_content_width, current) {
            window.refresh();
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Like a browser canvas, the root background extends to the bottom of
        // the visible area when the mail is shorter than the viewport: paint
        // down to the enclosing (scroll container) mask, not just our bounds.
        let visible = window.content_mask().bounds;
        let height = canvas_height(bounds.size.height, bounds.origin.y, visible.bottom());
        let list = self.document.borrow_mut().display_list_for_height(height);
        let mut canvas = bounds;
        canvas.size.width = canvas
            .size
            .width
            .max(px(self.document.borrow().content_width()));
        canvas.size.height = px(height);
        window.with_content_mask(Some(gpui::ContentMask { bounds: canvas }), |window| {
            paint_display_list(
                &list,
                bounds.origin,
                &mut self.cache.borrow_mut(),
                window,
                cx,
            );
        });
    }
}

/// A scrollable view showing one HTML document.
pub struct HtmlView {
    document: Rc<RefCell<HtmlDocument>>,
    cache: Rc<RefCell<PaintCache>>,
    scroll: ScrollHandle,
}

impl HtmlView {
    pub fn new(document: HtmlDocument) -> Self {
        Self {
            document: Rc::new(RefCell::new(document)),
            cache: Rc::default(),
            scroll: ScrollHandle::new(),
        }
    }

    /// Replaces the displayed document and scrolls back to the top.
    pub fn set_document(&mut self, document: HtmlDocument, cx: &mut Context<Self>) {
        self.document = Rc::new(RefCell::new(document));
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    pub fn document(&self) -> Rc<RefCell<HtmlDocument>> {
        self.document.clone()
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }
}

impl Render for HtmlView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Mail wider than the pane (fixed-width tables) scrolls horizontally:
        // a zero-height spacer as wide as the content extends the scroll area,
        // while the document itself keeps laying out at the pane width.
        let content_width = self.document.borrow().content_width();
        div()
            .id("html-view")
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.scroll)
            .child(HtmlElement::new(self.document.clone(), self.cache.clone()))
            .child(div().w(px(content_width)).h(px(0.)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_width_changes_ignore_subpixel_noise() {
        assert!(!content_width_changed(600.0, 600.2));
        assert!(content_width_changed(0.0, 375.0));
        assert!(content_width_changed(668.0, 375.0));
    }

    #[test]
    fn canvas_extends_to_the_visible_bottom() {
        assert_eq!(canvas_height(px(300.), px(10.), px(800.)), 790.0);
        assert_eq!(canvas_height(px(3000.), px(-200.), px(800.)), 3000.0);
    }

    #[test]
    fn layout_width_prefers_known_then_definite_then_fallback() {
        assert_eq!(
            layout_width(Some(px(300.)), AvailableSpace::Definite(px(500.))),
            300.0
        );
        assert_eq!(
            layout_width(None, AvailableSpace::Definite(px(500.))),
            500.0
        );
        assert_eq!(
            layout_width(None, AvailableSpace::MinContent),
            FALLBACK_WIDTH
        );
        assert_eq!(
            layout_width(Some(px(0.)), AvailableSpace::MaxContent),
            FALLBACK_WIDTH
        );
    }
}
