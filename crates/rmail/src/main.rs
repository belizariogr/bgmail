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

use root::RootView;

actions!(rmail, [Quit]);

/// Builds the macOS application menu. The first menu's name becomes the bold
/// "app menu" in the global menu bar, so it carries the application name.
fn app_menus() -> Vec<Menu> {
    vec![Menu {
        name: "rMail".into(),
        items: vec![MenuItem::action("Quit rMail", Quit)],
    }]
}

fn main() {
    Application::new()
        .with_assets(ui::Assets)
        .run(|cx: &mut App| {
            // Initialize the theme system starting in dark mode.
            theme::init(theme::Appearance::Dark, cx);
            // Initialize localization (English by default).
            locale::init(locale::Language::default(), cx);
            // Register the icon fonts (FontAwesome) used by the components.
            ui::init(cx);

            // Wire up the global menu bar and the standard Quit (Cmd+Q) shortcut, so
            // the app shows its name in the menu bar and can be quit like a native app.
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.set_menus(app_menus());

            // Restore the persisted layout (window/column sizes), clamped to the
            // window's minimum so a stale or tiny config can't open an unusable window.
            let settings = config::load();
            let win_width = settings.window_width.max(800.0);
            let win_height = settings.window_height.max(480.0);
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
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    window_min_size: Some(size(px(800.0), px(480.0))),
                    titlebar: Some(window_frame::main_titlebar_options()),
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
                    view
                },
            )
            .expect("failed to open the main window");

            cx.activate(true);
        });
}
