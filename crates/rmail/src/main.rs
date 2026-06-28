//! rMail — um cliente de e-mail rápido e elegante construído com GPUI.
//!
//! Este binário é, por enquanto, um **protótipo visual (mock)**: ele valida o
//! layout (inspirado no Mail do macOS) e a velocidade de inicialização, sem
//! lógica real de e-mail. A camada de domínio será adicionada em etapas
//! posteriores (ver `docs/PLANEJAMENTO.md` e `TODO.md`).

mod data;
mod root;

use gpui::{
    point, px, size, App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds,
    WindowOptions,
};

use root::RootView;

fn main() {
    Application::new().run(|cx: &mut App| {
        // Inicializa o sistema de temas começando no tema escuro.
        theme::init(theme::Appearance::Dark, cx);

        let bounds = Bounds::centered(None, size(px(1100.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("rMail".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.0), px(16.0))),
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| RootView::new()),
        )
        .expect("falha ao abrir a janela principal");

        cx.activate(true);
    });
}
