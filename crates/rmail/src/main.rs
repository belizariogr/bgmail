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

use gpui::{
    actions, point, px, size, App, AppContext, Application, Bounds, KeyBinding, Menu, MenuItem,
    TitlebarOptions, WindowBounds, WindowOptions,
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
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(800.0), px(480.0))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("rMail".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(12.0), px(16.0))),
                    }),
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
