//! BGMail — a fast and elegant e-mail client built with GPUI.
//!
//! For now this binary is a **visual prototype (mock)**: it validates the layout
//! (inspired by macOS Mail) and the startup speed, without any real e-mail logic.
//! The domain layer will be added in later stages (see `docs/PLANEJAMENTO.md` and
//! `TODO.md`).

mod actions;
mod app_menus;
mod command_palette;
mod command_palette_overlay;
mod commands;
mod compose;
mod config;
mod data;
mod db_seed;
mod locale;
mod root;
mod shortcuts;
mod startup;
mod web_view;
mod window_drag;
mod window_frame;

use std::time::Duration;

use gpui::{
    point, px, size, App, AppContext, Application, Bounds, Global, Keystroke, Menu, Subscription,
    WindowHandle, WindowOptions,
};

use crate::commands::CommandId;
use actions::{
    ComposeAttach, ComposeClose, ComposeDiscard, ComposeNew, ComposeSend, MessageArchive,
    MessageDelete, MessageDeletePermanent, MessageMarkJunk, MessageRestore, MessageToggleFlag,
    MoveMessageToFolder, OpenSettings, Quit, ToggleCommandPalette, ToggleSidebar,
};
use compose::ComposeView;
use root::{RootView, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH};

/// Handle to the main mail window, used to route global shortcuts.
pub(crate) struct MainWindow(WindowHandle<RootView>);

impl Global for MainWindow {}

/// Keeps the command-palette key interceptor subscribed for the app lifetime.
struct CommandPaletteShortcut {
    _subscription: Subscription,
}

impl Global for CommandPaletteShortcut {}

/// Whether `keystroke` is the command-palette shortcut (Ctrl+P / Cmd+P).
fn is_command_palette_keystroke(keystroke: &Keystroke) -> bool {
    keystroke.key.eq_ignore_ascii_case("p")
        && !keystroke.modifiers.shift
        && (keystroke.modifiers.control || keystroke.modifiers.platform)
}

fn toggle_command_palette(cx: &mut App) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |view, window, cx| {
            view.toggle_command_palette(window, cx);
        });
    });
}

fn schedule_menu_command(id: CommandId, cx: &mut App) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |view, window, cx| {
            let ctx = view.command_context();
            if commands::command_enabled(&id, &ctx) {
                view.execute_command(&id, window, cx);
            }
        });
    });
}

fn schedule_compose_action(
    cx: &mut App,
    action: impl FnOnce(&mut ComposeView, &mut gpui::Window, &mut gpui::Context<ComposeView>) + 'static,
) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |root, _, cx| {
            root.with_compose_window(cx, action);
        });
    });
}

fn schedule_compose_close(cx: &mut App) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |root, _, cx| {
            root.close_focused_auxiliary_window(cx);
        });
    });
}

/// Global handlers keep macOS menu items enabled when a native webview holds focus.
fn register_global_menu_actions(cx: &mut App) {
    cx.on_action(|_: &ComposeNew, cx| schedule_menu_command(CommandId::ComposeNew, cx));
    cx.on_action(|_: &OpenSettings, cx| schedule_menu_command(CommandId::OpenSettings, cx));
    cx.on_action(|_: &ToggleSidebar, cx| schedule_menu_command(CommandId::ToggleSidebar, cx));
    cx.on_action(|_: &MessageDelete, cx| schedule_menu_command(CommandId::MessageDelete, cx));
    cx.on_action(|_: &MessageDeletePermanent, cx| {
        schedule_menu_command(CommandId::MessageDeletePermanent, cx);
    });
    cx.on_action(|_: &MessageRestore, cx| schedule_menu_command(CommandId::MessageRestore, cx));
    cx.on_action(|_: &MessageArchive, cx| schedule_menu_command(CommandId::MessageArchive, cx));
    cx.on_action(|_: &MessageMarkJunk, cx| schedule_menu_command(CommandId::MessageMarkJunk, cx));
    cx.on_action(|_: &MessageToggleFlag, cx| {
        schedule_menu_command(CommandId::MessageToggleFlag, cx);
    });
    cx.on_action(|action: &MoveMessageToFolder, cx| {
        schedule_menu_command(CommandId::MoveToFolder(action.path.clone()), cx);
    });
    cx.on_action(|_: &ComposeSend, cx| {
        schedule_compose_action(cx, |view, _, cx| view.send_message(cx));
    });
    cx.on_action(|_: &ComposeAttach, cx| {
        schedule_compose_action(cx, |view, _, cx| view.attach_file(cx));
    });
    cx.on_action(|_: &ComposeDiscard, cx| {
        schedule_compose_action(cx, |view, window, cx| view.discard_draft(window, cx));
    });
    cx.on_action(|_: &ComposeClose, cx| {
        schedule_compose_close(cx);
    });
}

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
/// Full menus are refreshed from [`RootView`] as selection changes.
fn app_menus() -> Vec<Menu> {
    app_menus::build_menus(
        &commands::CommandContext::default(),
        locale::Language::default(),
    )
}

fn register_command_palette_shortcuts(cx: &mut App) {
    cx.on_action(|_: &ToggleCommandPalette, cx| toggle_command_palette(cx));
    let subscription = cx.intercept_keystrokes(|event, _, cx| {
        if is_command_palette_keystroke(&event.keystroke) {
            toggle_command_palette(cx);
            cx.stop_propagation();
        }
    });
    cx.set_global(CommandPaletteShortcut {
        _subscription: subscription,
    });
}

fn main() {
    startup::mark_start();
    startup::log_milestone("main entered");

    configure_windows_webview_hosting();

    Application::new()
        .with_assets(ui::Assets)
        .run(|cx: &mut App| {
            startup::log_milestone("application run callback");
            // Initialize the theme system starting in dark mode.
            theme::init(theme::Appearance::Dark, cx);
            // Initialize localization (English by default).
            locale::init(locale::Language::default(), cx);
            ui::bind_keys(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            shortcuts::bind_app_shortcuts(cx);
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
            let main_window = cx
                .open_window(
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
                        view.update(cx, |view, cx| {
                            cx.observe_window_activation(window, |this, window, cx| {
                                if !window.is_window_active() {
                                    this.schedule_close_command_palette_if_not_focused(cx);
                                }
                            })
                            .detach();
                            view.sync_app_menus(cx);
                        });
                        // Closing the window (e.g. the macOS traffic-light button)
                        // doesn't necessarily quit the app, so flush the layout here
                        // too — not just on app quit.
                        let weak = view.downgrade();
                        window.on_window_should_close(cx, move |_window, cx| {
                            if let Some(view) = weak.upgrade() {
                                view.update(cx, |view, _| view.persist_now());
                            }
                            cx.quit();
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
                                    let _ =
                                        ready.update(cx, |view, cx| view.mark_content_ready(cx));
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
            cx.set_global(MainWindow(main_window));
            register_command_palette_shortcuts(cx);
            register_global_menu_actions(cx);

            startup::log_milestone("main window opened");

            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use gpui::Keystroke;

    use super::*;

    #[test]
    fn command_palette_shortcut_matches_ctrl_p_and_cmd_p() {
        let ctrl_p = Keystroke {
            modifiers: gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            key: "p".into(),
            key_char: None,
        };
        let cmd_p = Keystroke {
            modifiers: gpui::Modifiers {
                platform: true,
                ..Default::default()
            },
            key: "p".into(),
            key_char: None,
        };
        let uppercase_ctrl_p = Keystroke {
            modifiers: gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            key: "P".into(),
            key_char: None,
        };
        let plain_p = Keystroke::default();
        assert!(is_command_palette_keystroke(&ctrl_p));
        assert!(is_command_palette_keystroke(&cmd_p));
        assert!(is_command_palette_keystroke(&uppercase_ctrl_p));
        assert!(!is_command_palette_keystroke(&plain_p));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_webview_hosting_disables_gpui_direct_composition() {
        assert_eq!(
            super::GPUI_DISABLE_DIRECT_COMPOSITION,
            ("GPUI_DISABLE_DIRECT_COMPOSITION", "1")
        );
    }
}
