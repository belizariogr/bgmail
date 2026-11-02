//! The GPUI element and view that display an [`HtmlDocument`], with pointer
//! interaction (hover, links, text selection, context menus), keyboard
//! commands (copy, select all, scrolling) and page zoom.

use crate::document::HtmlDocument;
use crate::interaction::{HitInfo, PointerButton, PointerInput, PointerModifiers, PointerPhase};
use crate::paint::{paint_display_list, PaintCache};
use gpui::{
    actions, div, point, px, relative, size, App, AvailableSpace, Bounds, ClipboardItem, Context,
    CursorStyle, DispatchPhase, Element, ElementId, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement, IntoElement,
    KeyBinding, LayoutId, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Point, Render, ScrollHandle, ScrollWheelEvent,
    StatefulInteractiveElement, Style, Styled, Task, WeakEntity, Window,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

actions!(
    html_view,
    [
        /// Copies the selected text.
        Copy,
        /// Selects all the text of the document.
        SelectAll,
        ScrollUp,
        ScrollDown,
        PageUp,
        PageDown,
        ScrollToTop,
        ScrollToBottom,
        /// Zooms the page in one step.
        ZoomIn,
        /// Zooms the page out one step.
        ZoomOut,
        /// Back to 100%.
        ResetZoom,
    ]
);

/// Browser zoom levels (Chromium's), from 25% to 500%.
pub const ZOOM_LEVELS: [f32; 17] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// The zoom level one step in (`steps > 0`) or out from `zoom`.
pub fn zoom_step(zoom: f32, steps: i32) -> f32 {
    let index = ZOOM_LEVELS
        .iter()
        .position(|level| (level - zoom).abs() < 0.005)
        // Between levels: the nearest one in the step's direction.
        .map(|i| i as i32 + steps)
        .unwrap_or_else(|| {
            let above = ZOOM_LEVELS.iter().position(|level| *level > zoom);
            match above {
                Some(i) if steps > 0 => i as i32 + steps - 1,
                Some(i) => i as i32 + steps,
                None => ZOOM_LEVELS.len() as i32 + steps,
            }
        });
    ZOOM_LEVELS[index.clamp(0, ZOOM_LEVELS.len() as i32 - 1) as usize]
}

/// Key context of a focused [`HtmlView`].
pub const KEY_CONTEXT: &str = "HtmlView";

/// How often a view with remote loads in flight looks for arrivals.
const REMOTE_POLL: Duration = Duration::from_millis(100);

/// Pixels scrolled by the arrow keys.
const LINE_SCROLL: f32 = 40.0;

/// Binds the [`HtmlView`] keyboard commands (call once at startup).
pub fn init(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-c", Copy, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("up", ScrollUp, context),
        KeyBinding::new("down", ScrollDown, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new("space", PageDown, context),
        KeyBinding::new("shift-space", PageUp, context),
        KeyBinding::new("home", ScrollToTop, context),
        KeyBinding::new("end", ScrollToBottom, context),
        KeyBinding::new("secondary-=", ZoomIn, context),
        KeyBinding::new("secondary-+", ZoomIn, context),
        KeyBinding::new("secondary--", ZoomOut, context),
        KeyBinding::new("secondary-0", ResetZoom, context),
    ]);
}

/// What an [`HtmlView`] reports to its host.
#[derive(Clone, Debug, PartialEq)]
pub enum HtmlViewEvent {
    /// The page zoom changed (1 = 100%).
    ZoomChanged(f32),
    /// The pointer entered (`Some(url)`) or left (`None`) a link.
    LinkHovered(Option<String>),
    /// A link was clicked; the host decides how to open it.
    LinkActivated(String),
    /// A secondary click: show a context menu for what is under the pointer.
    ContextMenu {
        position: Point<Pixels>,
        hit: HitInfo,
    },
}

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

/// Pointer cursor for what is under it: a hand over links, a text cursor over
/// text, an arrow elsewhere.
pub fn cursor_for(hit: &HitInfo) -> CursorStyle {
    if hit.link.is_some() {
        CursorStyle::PointingHand
    } else if hit.text {
        CursorStyle::IBeam
    } else {
        CursorStyle::Arrow
    }
}

/// Scroll position (distance scrolled down, ≥ 0) that reveals the document
/// span `top..bottom` in a viewport of `height`: unchanged when already
/// visible, else the span sits a third of the way down.
pub fn reveal_scroll(scrolled: f32, height: f32, top: f32, bottom: f32) -> f32 {
    if top >= scrolled && bottom <= scrolled + height {
        scrolled
    } else {
        (top - height / 3.0).max(0.0)
    }
}

fn modifiers(modifiers: &Modifiers) -> PointerModifiers {
    PointerModifiers {
        shift: modifiers.shift,
        control: modifiers.control,
        alt: modifiers.alt,
        platform: modifiers.platform,
    }
}

/// Element that lays out a document at the pane width and paints it at its
/// full content size (wider when the mail overflows). Put it inside a
/// scrolling container (see [`HtmlView`], which also makes the overflow
/// scrollable and handles input).
pub struct HtmlElement {
    document: Rc<RefCell<HtmlDocument>>,
    cache: Rc<RefCell<PaintCache>>,
    /// Content width when this element was built (by the view's render).
    rendered_content_width: f32,
    /// The view receiving pointer input, if interactive.
    view: Option<WeakEntity<HtmlView>>,
}

impl HtmlElement {
    pub fn new(document: Rc<RefCell<HtmlDocument>>, cache: Rc<RefCell<PaintCache>>) -> Self {
        let rendered_content_width = document.borrow().content_width();
        Self {
            document,
            cache,
            rendered_content_width,
            view: None,
        }
    }

    /// Routes pointer input over the element to `view`.
    pub fn interactive(mut self, view: WeakEntity<HtmlView>) -> Self {
        self.view = Some(view);
        self
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
    type PrepaintState = Hitbox;

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
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        // Layout just ran: re-render if the overflow spacer is out of date.
        let current = self.document.borrow().content_width();
        if content_width_changed(self.rendered_content_width, current) {
            window.refresh();
        }
        let mut area = bounds;
        area.size.width = area.size.width.max(px(current));
        window.insert_hitbox(area, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
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
        // Animated images (GIFs): repaint when the next frame is due; remote
        // images still loading: look again shortly.
        let loading = self.document.borrow().remote_loads().pending();
        let next_frame = self.cache.borrow().next_animation_frame();
        let next_frame = match (next_frame, loading) {
            (Some(delay), true) => Some(delay.min(REMOTE_POLL)),
            (None, true) => Some(REMOTE_POLL),
            (delay, false) => delay,
        };
        if let Some(delay) = next_frame {
            match self.view.as_ref().and_then(|view| view.upgrade()) {
                Some(view) => view.update(cx, |view, cx| view.schedule_frame(delay, cx)),
                None => window.request_animation_frame(),
            }
        }
        if let Some(view) = self.view.clone() {
            paint_input_listeners(view, bounds.origin, hitbox.clone(), window, cx);
        }
    }
}

/// Registers the window-level pointer listeners of an interactive element:
/// presses over it, and moves/releases anywhere (a selection drag may leave
/// the element). Positions become document coordinates relative to `origin`.
fn paint_input_listeners(
    view: WeakEntity<HtmlView>,
    origin: Point<Pixels>,
    hitbox: Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(entity) = view.upgrade() {
        window.set_cursor_style(entity.read(cx).cursor, &hitbox);
    }
    let to_document = move |position: Point<Pixels>| {
        (
            f32::from(position.x - origin.x),
            f32::from(position.y - origin.y),
        )
    };
    let down_view = view.clone();
    let down_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble && down_hitbox.is_hovered(window) {
            let (x, y) = to_document(event.position);
            let _ = down_view.update(cx, |view, cx| view.mouse_down(event, x, y, window, cx));
        }
    });
    let move_view = view.clone();
    let move_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase == DispatchPhase::Bubble {
            let (x, y) = to_document(event.position);
            let over = move_hitbox.is_hovered(window);
            let _ = move_view.update(cx, |view, cx| view.mouse_move(event, x, y, over, cx));
        }
    });
    // Ctrl/Cmd + wheel zooms instead of scrolling (captured before the
    // scroll container sees it).
    let wheel_view = view.clone();
    let wheel_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
        if phase == DispatchPhase::Capture
            && event.modifiers.secondary()
            && wheel_hitbox.is_hovered(window)
        {
            let dy = f32::from(event.delta.pixel_delta(px(20.)).y);
            if dy != 0.0 {
                let steps = if dy > 0.0 { 1 } else { -1 };
                let _ = wheel_view.update(cx, |view, cx| view.zoom_by(steps, cx));
            }
            cx.stop_propagation();
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
        if phase == DispatchPhase::Bubble {
            let (x, y) = to_document(event.position);
            let _ = view.update(cx, |view, cx| view.mouse_up(event, x, y, cx));
        }
    });
}

/// A scrollable, interactive view showing one HTML document.
pub struct HtmlView {
    document: Rc<RefCell<HtmlDocument>>,
    cache: Rc<RefCell<PaintCache>>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    /// Pointer cursor for what is under the pointer.
    cursor: CursorStyle,
    /// Link under the pointer (reported through [`HtmlViewEvent::LinkHovered`]).
    hovered_link: Option<String>,
    /// Whether a primary-button press started in the document (selection drag).
    dragging: bool,
    /// Page zoom, kept across documents like a browser tab's.
    zoom: f32,
    /// Pending repaint for animated images, and when it fires.
    frame_timer: Option<(Instant, Task<()>)>,
}

impl EventEmitter<HtmlViewEvent> for HtmlView {}

impl Focusable for HtmlView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl HtmlView {
    pub fn new(document: HtmlDocument, cx: &mut Context<Self>) -> Self {
        Self {
            document: Rc::new(RefCell::new(document)),
            cache: Rc::default(),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            cursor: CursorStyle::Arrow,
            hovered_link: None,
            dragging: false,
            zoom: 1.0,
            frame_timer: None,
        }
    }

    /// Height of the laid out document, for hosts that show it whole.
    pub fn content_height(&self) -> f32 {
        self.document.borrow().content_height()
    }

    /// Replaces the displayed document and scrolls back to the top.
    pub fn set_document(&mut self, mut document: HtmlDocument, cx: &mut Context<Self>) {
        document.set_zoom(self.zoom);
        self.document = Rc::new(RefCell::new(document));
        // A new parse has new image ids: the old textures can never be hit
        // again and would otherwise accumulate for the whole session.
        self.cache.borrow_mut().forget_document();
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.dragging = false;
        self.cursor = CursorStyle::Arrow;
        self.set_hovered_link(None, cx);
        cx.notify();
    }

    /// Repaints after `delay` (an animated image's next frame, or a look
    /// for arrived remote resources), unless an earlier repaint is already
    /// pending.
    fn schedule_frame(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let due = Instant::now() + delay;
        if self
            .frame_timer
            .as_ref()
            .is_some_and(|(pending, _)| *pending <= due)
        {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                this.frame_timer = None;
                // Remote resources that arrived are applied by the next
                // layout; the cached paint is stale.
                let arrived = this.document.borrow().remote_loads().take_arrived();
                if arrived {
                    this.document.borrow_mut().invalidate_paint();
                }
                cx.notify();
            });
        });
        self.frame_timer = Some((due, task));
    }

    /// The page zoom (1 = 100%).
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Sets the page zoom (clamped to [`ZOOM_LEVELS`]' range), keeping the
    /// same part of the document at the top of the view.
    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let zoom = zoom.clamp(ZOOM_LEVELS[0], ZOOM_LEVELS[ZOOM_LEVELS.len() - 1]);
        if zoom == self.zoom {
            return;
        }
        let ratio = zoom / self.zoom;
        self.zoom = zoom;
        self.document.borrow_mut().set_zoom(zoom);
        let offset = self.scroll.offset();
        self.scroll
            .set_offset(point(offset.x * ratio, offset.y * ratio));
        cx.emit(HtmlViewEvent::ZoomChanged(zoom));
        cx.notify();
    }

    /// Zooms `steps` levels in (> 0) or out (< 0) of [`ZOOM_LEVELS`].
    pub fn zoom_by(&mut self, steps: i32, cx: &mut Context<Self>) {
        self.set_zoom(zoom_step(self.zoom, steps), cx);
    }

    pub fn document(&self) -> Rc<RefCell<HtmlDocument>> {
        self.document.clone()
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// Whether some text is selected.
    pub fn has_selection(&self) -> bool {
        self.document.borrow().has_selection()
    }

    /// Copies the selected text to the clipboard (no-op without a selection).
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.document.borrow().selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Selects all the text of the document.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.document.borrow_mut().select_all();
        cx.notify();
    }

    /// Finds `query` in the document (case-insensitive) and returns the
    /// number of matches; see [`Self::select_match`].
    pub fn find(&mut self, query: &str, cx: &mut Context<Self>) -> usize {
        let count = self.document.borrow_mut().find(query);
        if count == 0 {
            self.document.borrow_mut().clear_selection();
        }
        cx.notify();
        count
    }

    /// Selects match `index` of the last [`Self::find`] and scrolls it into view.
    pub fn select_match(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((top, bottom)) = self.document.borrow_mut().select_match(index) else {
            return;
        };
        let offset = self.scroll.offset();
        let height = f32::from(self.scroll.bounds().size.height);
        let scrolled = reveal_scroll(-f32::from(offset.y), height, top, bottom);
        self.scroll.set_offset(point(offset.x, px(-scrolled)));
        cx.notify();
    }

    fn set_hovered_link(&mut self, link: Option<String>, cx: &mut Context<Self>) {
        if self.hovered_link != link {
            self.hovered_link = link.clone();
            cx.emit(HtmlViewEvent::LinkHovered(link));
        }
    }

    fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        x: f32,
        y: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        let button = match event.button {
            MouseButton::Left => PointerButton::Primary,
            MouseButton::Right => PointerButton::Secondary,
            MouseButton::Middle => PointerButton::Middle,
            _ => return,
        };
        if button == PointerButton::Secondary {
            let hit = self.document.borrow().hit_info(x, y);
            cx.emit(HtmlViewEvent::ContextMenu {
                position: event.position,
                hit,
            });
            return;
        }
        self.dragging = button == PointerButton::Primary;
        let changed = self.document.borrow_mut().pointer(PointerInput {
            x,
            y,
            phase: PointerPhase::Down {
                click_count: event.click_count,
            },
            button,
            primary_pressed: self.dragging,
            modifiers: modifiers(&event.modifiers),
        });
        if changed {
            cx.notify();
        }
    }

    fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        x: f32,
        y: f32,
        over: bool,
        cx: &mut Context<Self>,
    ) {
        if !over && !self.dragging {
            if self.hovered_link.is_some() || self.cursor != CursorStyle::Arrow {
                self.cursor = CursorStyle::Arrow;
                self.set_hovered_link(None, cx);
                cx.notify();
            }
            return;
        }
        let pressed = self.dragging && event.pressed_button == Some(MouseButton::Left);
        let mut document = self.document.borrow_mut();
        let mut changed = document.pointer(PointerInput {
            x,
            y,
            phase: PointerPhase::Move,
            button: PointerButton::Primary,
            primary_pressed: pressed,
            modifiers: modifiers(&event.modifiers),
        });
        let hit = document.hit_info(x, y);
        drop(document);
        let cursor = if pressed {
            CursorStyle::IBeam
        } else {
            cursor_for(&hit)
        };
        changed |= cursor != self.cursor;
        self.cursor = cursor;
        self.set_hovered_link(hit.link, cx);
        if changed {
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, x: f32, y: f32, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left || !self.dragging {
            return;
        }
        self.dragging = false;
        let mut document = self.document.borrow_mut();
        let changed = document.pointer(PointerInput {
            x,
            y,
            phase: PointerPhase::Up,
            button: PointerButton::Primary,
            primary_pressed: false,
            modifiers: modifiers(&event.modifiers),
        });
        let link = document.take_activated_link();
        drop(document);
        if let Some(link) = link {
            cx.emit(HtmlViewEvent::LinkActivated(link));
        }
        if changed {
            cx.notify();
        }
    }

    /// Scrolls vertically by `delta` px (positive = down), clamped.
    fn scroll_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let offset = self.scroll.offset();
        let max = f32::from(self.scroll.max_offset().height);
        let scrolled = (-f32::from(offset.y) + delta).clamp(0.0, max.max(0.0));
        self.scroll.set_offset(point(offset.x, px(-scrolled)));
        cx.notify();
    }

    fn page(&self) -> f32 {
        (f32::from(self.scroll.bounds().size.height) - LINE_SCROLL).max(LINE_SCROLL)
    }
}

impl Render for HtmlView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Mail wider than the pane (fixed-width tables) scrolls horizontally:
        // a zero-height spacer as wide as the content extends the scroll area,
        // while the document itself keeps laying out at the pane width.
        let content_width = self.document.borrow().content_width();
        div()
            .id("html-view")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.scroll)
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy_selection(cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
            .on_action(cx.listener(|this, _: &ScrollUp, _, cx| this.scroll_by(-LINE_SCROLL, cx)))
            .on_action(cx.listener(|this, _: &ScrollDown, _, cx| this.scroll_by(LINE_SCROLL, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| {
                let page = this.page();
                this.scroll_by(-page, cx)
            }))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| {
                let page = this.page();
                this.scroll_by(page, cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollToTop, _, cx| this.scroll_by(f32::MIN, cx)))
            .on_action(cx.listener(|this, _: &ScrollToBottom, _, cx| this.scroll_by(f32::MAX, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(-1, cx)))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.set_zoom(1.0, cx)))
            .child(
                HtmlElement::new(self.document.clone(), self.cache.clone())
                    .interactive(cx.weak_entity()),
            )
            .child(div().w(px(content_width)).h(px(0.)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_follow_browser_levels() {
        assert_eq!(zoom_step(1.0, 1), 1.1);
        assert_eq!(zoom_step(1.0, -1), 0.9);
        assert_eq!(zoom_step(1.0, 3), 1.5);
        // Clamped at both ends.
        assert_eq!(zoom_step(5.0, 1), 5.0);
        assert_eq!(zoom_step(0.25, -1), 0.25);
        // From between two levels, the next one in the step's direction.
        assert_eq!(zoom_step(1.2, 1), 1.25);
        assert_eq!(zoom_step(1.2, -1), 1.1);
        assert_eq!(zoom_step(9.0, -1), 5.0);
    }

    #[test]
    fn content_width_changes_ignore_subpixel_noise() {
        assert!(!content_width_changed(600.0, 600.2));
        assert!(content_width_changed(0.0, 375.0));
        assert!(content_width_changed(668.0, 375.0));
    }

    #[test]
    fn cursor_follows_what_is_under_the_pointer() {
        let link = HitInfo {
            link: Some("https://x".into()),
            text: true,
            ..Default::default()
        };
        assert_eq!(cursor_for(&link), CursorStyle::PointingHand);
        let text = HitInfo {
            text: true,
            ..Default::default()
        };
        assert_eq!(cursor_for(&text), CursorStyle::IBeam);
        assert_eq!(cursor_for(&HitInfo::default()), CursorStyle::Arrow);
    }

    #[test]
    fn reveal_scroll_keeps_visible_spans_and_centers_hidden_ones() {
        // Visible: unchanged.
        assert_eq!(reveal_scroll(100.0, 300.0, 150.0, 170.0), 100.0);
        // Below or above the viewport: a third of the way down.
        assert_eq!(reveal_scroll(0.0, 300.0, 700.0, 720.0), 600.0);
        assert_eq!(reveal_scroll(500.0, 300.0, 40.0, 60.0), 0.0);
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
