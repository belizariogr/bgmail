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
    point, px, size, App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds,
    WindowOptions,
};

use root::RootView;

fn main() {
    Application::new().run(|cx: &mut App| {
        // Initialize the theme system starting in dark mode.
        theme::init(theme::Appearance::Dark, cx);
        // Initialize localization (English by default).
        locale::init(locale::Language::default(), cx);
        // Register the icon fonts (FontAwesome) used by the components.
        ui::init(cx);

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
