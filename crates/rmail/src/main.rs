//! rMail — a fast and elegant e-mail client built with GPUI.
//!
//! For now this binary is a **visual prototype (mock)**: it validates the layout
//! (inspired by macOS Mail) and the startup speed, without any real e-mail logic.
//! The domain layer will be added in later stages (see `docs/PLANEJAMENTO.md` and
//! `TODO.md`).

mod config;
mod data;
mod locale;
mod root;
mod web_view;
mod window_drag;
mod window_frame;

use std::time::Duration;

use gpui::{
    actions, point, px, size, App, AppContext, Application, Bounds, KeyBinding, Menu, MenuItem,
    WindowOptions,
};

use root::{RootView, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH};

actions!(rmail, [Quit]);

/// GPUI's Windows renderer uses DirectComposition by default (`WS_EX_NOREDIRECTIONBITMAP`).
/// That path does not compose reliably with the child HWND used by WebView2: the
/// page can be interactive while its pixels, especially text/selection, never
/// become visible. This env var is read by GPUI during platform initialization.
#[cfg(target_os = "windows")]
const GPUI_DISABLE_DIRECT_COMPOSITION: (&str, &str) = ("GPUI_DISABLE_DIRECT_COMPOSITION", "1");

#[cfg(target_os = "windows")]
fn configure_windows_webview_hosting() {
    std::env::set_var(
        GPUI_DISABLE_DIRECT_COMPOSITION.0,
        GPUI_DISABLE_DIRECT_COMPOSITION.1,
    );
}

#[cfg(not(target_os = "windows"))]
fn configure_windows_webview_hosting() {}

/// Builds the macOS application menu. The first menu's name becomes the bold
/// "app menu" in the global menu bar, so it carries the application name.
fn app_menus() -> Vec<Menu> {
    vec![Menu {
        name: "rMail".into(),
        items: vec![MenuItem::action("Quit rMail", Quit)],
    }]
}

fn main() {
    configure_windows_webview_hosting();

    Application::new()
        .with_assets(ui::Assets)
        .run(|cx: &mut App| {
            // Initialize the theme system starting in dark mode.
            theme::init(theme::Appearance::Dark, cx);
            // Initialize localization (English by default).
            locale::init(locale::Language::default(), cx);

            // Wire up the global menu bar and the standard Quit (Cmd+Q) shortcut, so
            // the app shows its name in the menu bar and can be quit like a native app.
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.set_menus(app_menus());

            // Restore the persisted layout (window/column sizes), clamped to the
            // window's minimum so a stale or tiny config can't open an unusable window.
            let settings = config::load();
            let win_width = settings.window_width.max(WINDOW_MIN_WIDTH);
            let win_height = settings.window_height.max(WINDOW_MIN_HEIGHT);
            let bounds = Bounds::new(
                point(px(settings.window_x), px(settings.window_y)),
                size(px(win_width), px(win_height)),
            );
            // The saved maximized frame, used on macOS to open directly at the
            // maximized size (no flicker). Falls back to the restored bounds if it
            // was never recorded.
            let max_bounds = if settings.max_width > 0.0 && settings.max_height > 0.0 {
                Bounds::new(
                    point(px(settings.max_x), px(settings.max_y)),
                    size(px(settings.max_width), px(settings.max_height)),
                )
            } else {
                bounds
            };
            // Per-platform initial bounds: on macOS opens windowed directly at the
            // maximized frame; elsewhere uses the OS maximized state. Either way the
            // restored `bounds` is the size/position to restore to on move.
            let window_bounds =
                window_drag::initial_window_bounds(bounds, settings.maximized, max_bounds);
            // Captured before `settings` is moved into the view: on Windows we wait
            // for the (asynchronous) maximize to land before revealing the UI.
            #[cfg(target_os = "windows")]
            let open_maximized = settings.maximized;
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
                    titlebar: Some(window_frame::main_titlebar_options()),
                    // Windows opens hidden so we can cloak it and run the
                    // (asynchronous) maximize + first layout off-screen, then reveal
                    // the finished window — see the spawn below. Elsewhere GPUI shows
                    // the window immediately.
                    show: !cfg!(target_os = "windows"),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| {
                        // Flush the latest layout synchronously when the app quits
                        // (e.g. Cmd+Q), so a move/resize made right before quitting
                        // — within the `request_save` debounce window — isn't lost.
                        cx.on_app_quit(|view: &mut RootView, _cx| {
                            view.persist_now();
                            async {}
                        })
                        .detach();
                        // Arm persistence only after the initial window state has
                        // settled, so the opening sequence can't overwrite the
                        // saved position/size.
                        cx.spawn(async move |view, cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(700))
                                .await;
                            let _ = view.update(cx, |view, _| view.enable_persistence());
                        })
                        .detach();
                        RootView::new(settings)
                    });
                    // Closing the window (e.g. the macOS traffic-light button)
                    // doesn't necessarily quit the app, so flush the layout here
                    // too — not just on app quit.
                    let weak = view.downgrade();
                    window.on_window_should_close(cx, move |_window, cx| {
                        if let Some(view) = weak.upgrade() {
                            view.update(cx, |view, _| view.persist_now());
                        }
                        true
                    });
                    // Windows: keep the window cloaked (invisible to the compositor)
                    // while it maximizes and lays out, then reveal it once finished —
                    // no restore→maximize or paint-in flicker. The maximize is applied
                    // asynchronously and the `WM_SIZE` that grows GPUI's cached viewport
                    // can be dropped during the busy open sequence, leaving the UI at
                    // the small base size; so we also poll, re-posting `WM_SIZE`
                    // whenever the viewport looks stale, until the window settles.
                    #[cfg(target_os = "windows")]
                    {
                        window_drag::set_window_cloaked(window, true);
                        // Created hidden (show:false); show + maximize it now while it
                        // stays cloaked (off-screen).
                        window.activate_window();

                        let ready = view.downgrade();
                        window
                            .spawn(cx, async move |cx| {
                                // ~1.5s cap (90 × 16ms) so a missed resize can't keep
                                // the window cloaked forever; then reveal it regardless.
                                for _ in 0..90 {
                                    cx.background_executor()
                                        .timer(Duration::from_millis(16))
                                        .await;
                                    let settled = cx
                                        .update(|window, _| {
                                            let actual = window.bounds().size;
                                            if window.viewport_size() != actual {
                                                window_drag::nudge_window_resize(window);
                                            }
                                            window_drag::window_layout_settled(
                                                open_maximized,
                                                window.is_maximized(),
                                                window.viewport_size(),
                                                actual,
                                            )
                                        })
                                        .unwrap_or(true);
                                    if settled {
                                        break;
                                    }
                                }
                                let _ = ready.update(cx, |view, cx| view.mark_content_ready(cx));
                                // Give the now-ready content time to fully paint before
                                // revealing the window, for a clean (flicker-free) appearance.
                                cx.background_executor()
                                    .timer(Duration::from_millis(250))
                                    .await;
                                let _ = cx.update(|window, _| {
                                    window_drag::set_window_cloaked(window, false);
                                });
                            })
                            .detach();
                    }
                    // macOS opens windowed directly at the maximized frame (no async
                    // maximize), so there's no layout race — only a small paint-in
                    // flash. Cloak via the window's alpha, then reveal after a short
                    // settle so the first full frame is painted before it appears.
                    #[cfg(target_os = "macos")]
                    {
                        window_drag::set_window_cloaked(window, true);
                        window
                            .spawn(cx, async move |cx| {
                                cx.background_executor()
                                    .timer(Duration::from_millis(250))
                                    .await;
                                let _ = cx.update(|window, _| {
                                    window_drag::set_window_cloaked(window, false);
                                });
                            })
                            .detach();
                    }
                    view
                },
            )
            .expect("failed to open the main window");

            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_webview_hosting_disables_gpui_direct_composition() {
        assert_eq!(
            super::GPUI_DISABLE_DIRECT_COMPOSITION,
            ("GPUI_DISABLE_DIRECT_COMPOSITION", "1")
        );
    }
}
