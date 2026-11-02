//! The zoom of the whole app (Ctrl/Cmd + `+`, `-`, `0`): which levels exist. Applying
//! one is `gpui::Window::set_content_scale`, on every window.

/// A subset of the browsers' levels: fine near 100%, where the steps are noticed.
const LEVELS: [f32; 9] = [0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// The level `steps` away from `current`, stopping at both ends.
pub(crate) fn step(current: f32, steps: i32) -> f32 {
    let at = nearest(current) as i32 + steps;
    LEVELS[at.clamp(0, LEVELS.len() as i32 - 1) as usize]
}

/// A level for a value read from the config, which anyone may have edited.
pub(crate) fn sanitize(saved: f32) -> f32 {
    if saved.is_finite() {
        LEVELS[nearest(saved)]
    } else {
        1.0
    }
}

fn nearest(value: f32) -> usize {
    let distance = |level: f32| (level - value).abs();
    (0..LEVELS.len())
        .min_by(|a, b| distance(LEVELS[*a]).total_cmp(&distance(LEVELS[*b])))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size, Modifiers, MouseButton, Render, Window};

    #[test]
    fn steps_walk_the_levels_and_stop_at_both_ends() {
        assert_eq!(step(1.0, 1), 1.1);
        assert_eq!(step(1.1, -1), 1.0);
        assert_eq!(step(2.0, 1), 2.0);
        assert_eq!(step(0.75, -1), 0.75);
    }

    #[test]
    fn saved_values_snap_to_a_level() {
        assert_eq!(sanitize(1.0), 1.0);
        assert_eq!(sanitize(1.2), 1.25);
        assert_eq!(sanitize(50.0), 2.0);
        assert_eq!(sanitize(-3.0), 0.75);
        assert_eq!(sanitize(f32::NAN), 1.0);
    }

    struct ClickTarget {
        clicked_at: Option<gpui::Point<gpui::Pixels>>,
    }

    impl Render for ClickTarget {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{div, InteractiveElement, ParentElement, Styled};
            div().size_full().child(
                div()
                    .debug_selector(|| "target".to_string())
                    .size_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, _| {
                            this.clicked_at = Some(event.position);
                        }),
                    ),
            )
        }
    }

    /// The patch in `vendor/gpui`: elements see a smaller window, drawn larger, and
    /// the pointer lands on what is under it on screen.
    #[gpui::test]
    fn the_content_scale_shrinks_the_logical_window_and_scales_input(
        cx: &mut gpui::TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|_, _| ClickTarget { clicked_at: None });
        cx.run_until_parked();
        let (full, base_scale) =
            cx.update(|window, _| (window.viewport_size(), window.scale_factor()));

        cx.update(|window, cx| window.set_content_scale(2.0, cx));
        cx.run_until_parked();
        cx.update(|window, _| {
            assert_eq!(
                window.viewport_size(),
                size(full.width / 2.0, full.height / 2.0)
            );
            assert_eq!(window.platform_viewport_size(), full);
            assert_eq!(window.scale_factor(), base_scale * 2.0);
        });

        // Test events go through `dispatch_event`, as platform ones do.
        cx.simulate_mouse_down(
            point(px(100.0), px(60.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        view.read_with(cx, |view, _| {
            assert_eq!(view.clicked_at, Some(point(px(50.0), px(30.0))));
        });

        cx.update(|window, cx| window.set_content_scale(1.0, cx));
        cx.update(|window, _| assert_eq!(window.viewport_size(), full));
    }
}
