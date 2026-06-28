//! A compact, draggable vertical scrollbar.
//!
//! This mirrors the core idea of Zed's `Scrollbar` (a custom [`Element`] that
//! reads a [`ScrollHandle`] and paints a thumb), but trimmed to what the mock
//! needs: a single vertical axis, always visible while the content overflows,
//! with thumb dragging and track clicking. There is no auto-hide animation or
//! settings integration — adding those would be bloat for now.
//!
//! Usage: place it as a child of a `relative` container that also holds the
//! scrollable content tracking the same [`ScrollHandle`]. The element overlays
//! the container and draws the thumb on the right edge.

use std::panic::Location;
use std::time::{Duration, Instant};

use gpui::{
    point, px, quad, relative, size, App, BorderStyle, Bounds, Corners, CursorStyle, DispatchPhase,
    Edges, Element, ElementId, Entity, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId,
    IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Position, ScrollHandle, Style, Window,
};
use theme::ActiveTheme;

/// Visual width of the thumb (and the track strip it lives in).
const WIDTH: Pixels = px(6.0);
/// Right-edge padding between the thumb and the container border.
const PADDING: Pixels = px(3.0);
/// Width of the hover-sensitive strip that reveals the scrollbar.
const HOVER_WIDTH: Pixels = px(14.0);
/// Minimum thumb height so it stays grabbable on long lists.
const MIN_THUMB: f32 = 24.0;

/// How long the scrollbar stays visible after scrolling stops.
pub const AUTO_HIDE: Duration = Duration::from_millis(250);

/// Persistent scrollbar state, kept across frames in an [`Entity`].
#[derive(Default)]
pub struct ScrollbarState {
    /// While dragging, the pointer's vertical offset within the thumb.
    drag_grab: Option<f32>,
    /// When scrolling last happened (used for the auto-hide timeout).
    last_scroll: Option<Instant>,
    /// Whether the pointer is currently over the (wider) scrollbar strip.
    hovered: bool,
}

impl ScrollbarState {
    /// Creates a fresh state (not dragging).
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that scrolling just happened, keeping the bar briefly visible.
    pub fn note_scroll(&mut self) {
        self.last_scroll = Some(Instant::now());
    }

    /// Whether scrolling happened within the last [`AUTO_HIDE`] window.
    fn recently_scrolled(&self) -> bool {
        self.last_scroll.is_some_and(|t| t.elapsed() < AUTO_HIDE)
    }
}

/// A vertical scrollbar overlay bound to a [`ScrollHandle`].
pub struct Scrollbar {
    state: Entity<ScrollbarState>,
    handle: ScrollHandle,
}

impl Scrollbar {
    /// Creates a vertical scrollbar for the given scroll handle and state.
    pub fn vertical(state: Entity<ScrollbarState>, handle: ScrollHandle) -> Self {
        Self { state, handle }
    }
}

/// Computes the thumb's `(top, height)` within a track of `track` pixels.
///
/// Returns [`None`] when the content fits (nothing to scroll). `viewport` is
/// the visible height, `max_offset` the maximum scroll distance and `scroll`
/// the current scrolled distance (`0..=max_offset`).
fn thumb_geometry(track: f32, viewport: f32, max_offset: f32, scroll: f32) -> Option<(f32, f32)> {
    if max_offset <= 0.0 || viewport <= 0.0 || track <= 0.0 {
        return None;
    }
    let content = viewport + max_offset;
    let thumb = (track * (viewport / content)).max(MIN_THUMB);
    if thumb >= track {
        return None;
    }
    let scrolled = scroll.clamp(0.0, max_offset);
    let top = (scrolled / max_offset) * (track - thumb);
    Some((top, thumb))
}

/// Maps a thumb top position back into a (negative) scroll offset.
fn offset_for_thumb_top(thumb_top: f32, track: f32, thumb: f32, max_offset: f32) -> f32 {
    let denom = track - thumb;
    let frac = if denom > 0.0 {
        (thumb_top / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    -(frac * max_offset)
}

/// Layout computed during prepaint and consumed at paint time.
pub struct ScrollbarLayout {
    track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    /// Hover-sensitive strip; the scrollbar is only shown while it's hovered.
    hover_hitbox: Hitbox,
    max_offset: f32,
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = Option<ScrollbarLayout>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            position: Position::Absolute,
            inset: Edges::default(),
            size: size(relative(1.0), relative(1.0)).map(Into::into),
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
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
        let viewport = f32::from(self.handle.bounds().size.height);
        let max_offset = f32::from(self.handle.max_offset().height);
        let scroll = -f32::from(self.handle.offset().y);
        let track_height = f32::from(bounds.size.height);

        let (thumb_top, thumb_height) = thumb_geometry(track_height, viewport, max_offset, scroll)?;

        let track_x = bounds.origin.x + bounds.size.width - WIDTH - PADDING;
        let track = Bounds::new(
            point(track_x, bounds.origin.y),
            size(WIDTH, bounds.size.height),
        );
        let thumb = Bounds::new(
            point(track_x, bounds.origin.y + px(thumb_top)),
            size(WIDTH, px(thumb_height)),
        );
        let hover_region = Bounds::new(
            point(
                bounds.origin.x + bounds.size.width - HOVER_WIDTH,
                bounds.origin.y,
            ),
            size(HOVER_WIDTH, bounds.size.height),
        );
        let hover_hitbox = window.insert_hitbox(hover_region, HitboxBehavior::Normal);

        Some(ScrollbarLayout {
            track,
            thumb,
            hover_hitbox,
            max_offset,
        })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(layout) = prepaint.take() else {
            return;
        };

        // Keep the default arrow cursor over the scrollbar strip (instead of the
        // pointer the underlying clickable rows would otherwise show).
        window.set_cursor_style(CursorStyle::Arrow, &layout.hover_hitbox);

        let (dragging, recently_scrolled, hovered) = {
            let state = self.state.read(cx);
            (
                state.drag_grab.is_some(),
                state.recently_scrolled(),
                state.hovered,
            )
        };
        // Reveal the scrollbar while the strip is hovered, while a drag is in
        // progress, or briefly after scrolling (even via the mouse wheel). The
        // hovered flag is updated from the mouse-move handler so visibility is
        // never stale (a plain hover move otherwise wouldn't repaint the view).
        if hovered || dragging || recently_scrolled {
            let colors = cx.theme().colors();
            let thumb_color = if dragging {
                colors.scrollbar_thumb_hover
            } else {
                colors.scrollbar_thumb
            };

            window.paint_quad(quad(
                layout.thumb,
                Corners::all(WIDTH / 2.0),
                thumb_color,
                Edges::default(),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }

        let track = layout.track;
        let thumb = layout.thumb;
        let max_offset = layout.max_offset;
        let hover_bounds = layout.hover_hitbox.bounds;

        // Start dragging from the thumb, or jump-then-drag from the track.
        window.on_mouse_event({
            let state = self.state.clone();
            let handle = self.handle.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if thumb.contains(&event.position) {
                    let grab = f32::from(event.position.y - thumb.origin.y);
                    state.update(cx, |state, _| state.drag_grab = Some(grab));
                    window.refresh();
                    cx.stop_propagation();
                } else if track.contains(&event.position) {
                    let grab = f32::from(thumb.size.height) / 2.0;
                    let thumb_top = f32::from(event.position.y - track.origin.y) - grab;
                    let offset = offset_for_thumb_top(
                        thumb_top,
                        f32::from(track.size.height),
                        f32::from(thumb.size.height),
                        max_offset,
                    );
                    handle.set_offset(point(handle.offset().x, px(offset)));
                    state.update(cx, |state, _| state.drag_grab = Some(grab));
                    window.refresh();
                    cx.stop_propagation();
                }
            }
        });

        // Track hover (to reveal/hide the bar) and update the offset while
        // dragging. Hover is computed geometrically so it stays correct between
        // frames; crossing the strip boundary forces a repaint.
        window.on_mouse_event({
            let state = self.state.clone();
            let handle = self.handle.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }

                let inside = hover_bounds.contains(&event.position);
                let (drag_grab, hover_changed) = state.update(cx, |state, _| {
                    let hover_changed = state.hovered != inside;
                    state.hovered = inside;
                    (state.drag_grab, hover_changed)
                });

                if let Some(grab) = drag_grab {
                    if event.dragging() {
                        let thumb_top = f32::from(event.position.y - track.origin.y) - grab;
                        let offset = offset_for_thumb_top(
                            thumb_top,
                            f32::from(track.size.height),
                            f32::from(thumb.size.height),
                            max_offset,
                        );
                        handle.set_offset(point(handle.offset().x, px(offset)));
                        window.refresh();
                        cx.stop_propagation();
                        return;
                    }
                }

                if hover_changed {
                    window.refresh();
                }
            }
        });

        // Release the drag.
        window.on_mouse_event({
            let state = self.state.clone();
            move |_event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if state.read(cx).drag_grab.is_some() {
                    state.update(cx, |state, _| state.drag_grab = None);
                    window.refresh();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_thumb_when_content_fits() {
        assert_eq!(thumb_geometry(300.0, 300.0, 0.0, 0.0), None);
        assert_eq!(thumb_geometry(0.0, 0.0, 0.0, 0.0), None);
    }

    #[test]
    fn thumb_shrinks_with_more_content() {
        // Viewport 200 of 400 total => thumb is half the track (100 of 200).
        let (top, height) = thumb_geometry(200.0, 200.0, 200.0, 0.0).unwrap();
        assert!((height - 100.0).abs() < 0.01);
        assert!((top - 0.0).abs() < 0.01);
    }

    #[test]
    fn thumb_respects_minimum_height() {
        // Tiny viewport relative to content would give a sub-minimum thumb.
        let (_, height) = thumb_geometry(300.0, 30.0, 3000.0, 0.0).unwrap();
        assert!(height >= MIN_THUMB);
    }

    #[test]
    fn thumb_moves_to_bottom_at_max_scroll() {
        let track = 200.0;
        let (top, height) = thumb_geometry(track, 200.0, 200.0, 200.0).unwrap();
        assert!((top - (track - height)).abs() < 0.01);
    }

    #[test]
    fn scroll_marks_recently_scrolled() {
        let mut state = ScrollbarState::new();
        assert!(!state.recently_scrolled());
        state.note_scroll();
        assert!(state.recently_scrolled());
    }

    #[test]
    fn offset_round_trips_through_thumb_top() {
        // Dragging the thumb to the bottom yields -max_offset.
        let track = 200.0;
        let thumb = 100.0;
        let max = 200.0;
        assert!((offset_for_thumb_top(track - thumb, track, thumb, max) - (-max)).abs() < 0.01);
        // Top of the track maps to zero offset.
        assert_eq!(offset_for_thumb_top(0.0, track, thumb, max), 0.0);
    }
}
