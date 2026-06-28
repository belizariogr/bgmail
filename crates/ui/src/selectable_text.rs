//! Selectable static text.
//!
//! GPUI has no built-in selectable text element, so this wraps a [`StyledText`]
//! and adds click-and-drag selection: it maps pointer positions to character
//! indices (via the text layout), highlights the selected range and publishes
//! the selected substring as a [`ActiveTextSelection`] global so the host view
//! can copy it (e.g. on `Cmd+C`).
//!
//! Selection is per text block (one element). Selecting across several blocks at
//! once is out of scope for the mock — each block tracks its own range.

use std::cell::RefCell;
use std::ops::Range;
use std::panic::Location;
use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, Global, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, StyledText, TextLayout, TextRun,
    Window,
};

/// The text currently selected in the app, published by [`SelectableText`].
///
/// Stored as a global so a host view (the reader pane) can copy it without the
/// text element knowing about clipboards, focus or key bindings.
#[derive(Default)]
pub struct ActiveTextSelection {
    /// The selected text, or `None` when the selection is empty.
    pub text: Option<String>,
}

impl Global for ActiveTextSelection {}

/// Per-block selection, shared between the element and its event handlers.
#[derive(Clone, Default)]
struct SelectionHandle(Rc<RefCell<SelectionInner>>);

#[derive(Default)]
struct SelectionInner {
    anchor: usize,
    head: usize,
    dragging: bool,
}

impl SelectionInner {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

/// A run of styled text that can be selected with the mouse.
pub struct SelectableText {
    id: ElementId,
    text: SharedString,
    runs: Vec<TextRun>,
    selection: Hsla,
}

impl SelectableText {
    /// Creates a selectable text block. `runs` must cover the whole `text`.
    pub fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        runs: Vec<TextRun>,
        selection: Hsla,
    ) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            runs,
            selection,
        }
    }
}

/// Splits `runs` so the bytes in `range` carry the selection background, keeping
/// every other run attribute (color, weight, underline, ...) intact.
fn highlight(runs: &[TextRun], range: &Range<usize>, color: Hsla) -> Vec<TextRun> {
    if range.is_empty() {
        return runs.to_vec();
    }
    let mut out = Vec::with_capacity(runs.len());
    let mut pos = 0usize;
    for run in runs {
        let start = pos;
        let end = pos + run.len;
        pos = end;

        // Cut this run at the selection edges that fall inside it.
        let mut cuts = vec![start, end];
        if (start..end).contains(&range.start) {
            cuts.push(range.start);
        }
        if (start..end).contains(&range.end) {
            cuts.push(range.end);
        }
        cuts.sort_unstable();
        cuts.dedup();

        for segment in cuts.windows(2) {
            let (lo, hi) = (segment[0], segment[1]);
            if hi == lo {
                continue;
            }
            let mut piece = run.clone();
            piece.len = hi - lo;
            if lo >= range.start && hi <= range.end {
                piece.background_color = Some(color);
            }
            out.push(piece);
        }
    }
    out
}

/// Layout state carried from `request_layout` to `paint`.
pub struct SelectableLayout {
    styled: StyledText,
    layout: TextLayout,
}

impl IntoElement for SelectableText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableText {
    type RequestLayoutState = SelectableLayout;
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let range = window.with_element_state::<SelectionHandle, _>(
            id.expect("SelectableText has an id"),
            |state, _| {
                let state = state.unwrap_or_default();
                let range = state.0.borrow().range();
                (range, state)
            },
        );

        let runs = highlight(&self.runs, &range, self.selection);
        let mut styled = StyledText::new(self.text.clone()).with_runs(runs);
        let (layout_id, ()) = styled.request_layout(None, None, window, cx);
        let layout = styled.layout().clone();
        (layout_id, SelectableLayout { styled, layout })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        request_layout
            .styled
            .prepaint(None, None, bounds, &mut (), window, cx);
        Some(window.insert_hitbox(bounds, HitboxBehavior::Normal))
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        request_layout
            .styled
            .paint(None, None, bounds, &mut (), &mut (), window, cx);

        if let Some(hitbox) = prepaint {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        let handle = window.with_element_state::<SelectionHandle, _>(
            id.expect("SelectableText has an id"),
            |state, _| {
                let state = state.unwrap_or_default();
                (state.clone(), state)
            },
        );
        let layout = request_layout.layout.clone();

        // Begin a selection from the pressed position.
        window.on_mouse_event({
            let handle = handle.clone();
            let layout = layout.clone();
            let text = self.text.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if !bounds.contains(&event.position) {
                    return;
                }
                let index = index_at(&layout, event.position);
                {
                    let mut sel = handle.0.borrow_mut();
                    sel.anchor = index;
                    sel.head = index;
                    sel.dragging = true;
                }
                publish_selection(&handle, &text, cx);
                window.refresh();
            }
        });

        // Extend the selection while dragging.
        window.on_mouse_event({
            let handle = handle.clone();
            let layout = layout.clone();
            let text = self.text.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                if !handle.0.borrow().dragging || !event.dragging() {
                    return;
                }
                let index = index_at(&layout, event.position);
                handle.0.borrow_mut().head = index;
                publish_selection(&handle, &text, cx);
                window.refresh();
            }
        });

        // Finish the drag (the range stays highlighted).
        window.on_mouse_event({
            let handle = handle.clone();
            move |_event: &MouseUpEvent, phase, _window, _cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                handle.0.borrow_mut().dragging = false;
            }
        });
    }
}

/// Maps a window position to the nearest character index in the layout.
fn index_at(layout: &TextLayout, position: Point<Pixels>) -> usize {
    layout
        .index_for_position(position)
        .unwrap_or_else(|near| near)
}

/// Publishes the current selection's text as the [`ActiveTextSelection`] global.
fn publish_selection(handle: &SelectionHandle, text: &SharedString, cx: &mut App) {
    let range = handle.0.borrow().range();
    let selected = text.get(range).filter(|s| !s.is_empty()).map(str::to_owned);
    cx.set_global(ActiveTextSelection { text: selected });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Hsla, TextRun};

    fn run(len: usize) -> TextRun {
        TextRun {
            len,
            font: gpui::Font {
                family: "Helvetica".into(),
                features: Default::default(),
                fallbacks: None,
                weight: Default::default(),
                style: Default::default(),
            },
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    fn sel() -> Hsla {
        Hsla {
            h: 0.6,
            s: 0.5,
            l: 0.5,
            a: 0.3,
        }
    }

    #[test]
    fn empty_range_keeps_runs_unchanged() {
        let runs = vec![run(5)];
        let out = highlight(&runs, &(2..2), sel());
        assert_eq!(out.len(), 1);
        assert!(out[0].background_color.is_none());
    }

    #[test]
    fn selection_splits_a_single_run_into_three() {
        // "hello" with 1..3 selected -> "h" | "el" | "lo".
        let out = highlight(&[run(5)], &(1..3), sel());
        assert_eq!(out.iter().map(|r| r.len).collect::<Vec<_>>(), vec![1, 2, 2]);
        assert_eq!(out[0].background_color, None);
        assert_eq!(out[1].background_color, Some(sel()));
        assert_eq!(out[2].background_color, None);
    }

    #[test]
    fn selection_preserves_total_length() {
        let runs = vec![run(3), run(4), run(2)];
        let out = highlight(&runs, &(2..7), sel());
        let total: usize = out.iter().map(|r| r.len).sum();
        assert_eq!(total, 9);
        // The bytes in 2..7 are highlighted, the rest is not.
        let highlighted: usize = out
            .iter()
            .filter(|r| r.background_color.is_some())
            .map(|r| r.len)
            .sum();
        assert_eq!(highlighted, 5);
    }
}
