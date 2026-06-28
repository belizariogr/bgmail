//! rMail — a fast and elegant e-mail client built with GPUI.
//!
//! For now this binary is a **visual prototype (mock)**: it validates the layout
//! (inspired by macOS Mail) and the startup speed, without any real e-mail logic.
//! The domain layer will be added in later stages (see `docs/PLANEJAMENTO.md` and
//! `TODO.md`).

mod data;
mod locale;
mod root;
mod web_view;

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
    Application::new().run(|cx: &mut App| {
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

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1100.0), px(720.0)));
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
            |_window, cx| cx.new(|_cx| RootView::new()),
        )
        .expect("failed to open the main window");

        cx.activate(true);
    });
}
