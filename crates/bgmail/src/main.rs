//! BGMail — a fast and elegant e-mail client built with GPUI.
//!
//! For now this binary is a **visual prototype (mock)**: it validates the layout
//! (inspired by macOS Mail) and the startup speed, without any real e-mail logic.
//! The domain layer will be added in later stages (see `docs/PLANEJAMENTO.md` and
//! `TODO.md`).

mod account_setup;
mod actions;
mod app_menus;
mod app_zoom;
mod attachments;
mod command_palette;
mod command_palette_overlay;
mod commands;
mod compose;
mod config;
mod crash_guard;
mod data;
mod dates;
mod db_seed;
mod default_mail_app;
mod instance;
mod locale;
mod mail_html;
mod mail_service;
mod notifications;
mod onboarding;
mod print;
mod recipients;
mod root;
mod settings;
mod shortcuts;
mod startup;
mod window_drag;
mod window_frame;

use std::time::Duration;

use gpui::{
    point, px, size, App, AppContext, Application, Bounds, Global, Keystroke, Menu, Subscription,
    WindowHandle, WindowOptions,
};

use crate::commands::CommandId;
use actions::{
    AppZoomIn, AppZoomOut, AppZoomReset, ComposeAttach, ComposeClose, ComposeDiscard, ComposeNew,
    ComposeSend, FindInMessage, GetNewMail, MessageArchive, MessageDelete, MessageDeletePermanent,
    MessageForward, MessageMarkJunk, MessagePrint, MessageReply, MessageReplyAll, MessageRestore,
    MessageToggleFlag, MessageToggleRead, MoveMessageToFolder, OpenSettings, Quit,
    ToggleCommandPalette, ToggleSidebar,
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

/// Whether `keystroke` is the command-palette shortcut (Ctrl+K / Cmd+K).
fn is_command_palette_keystroke(keystroke: &Keystroke) -> bool {
    keystroke.key.eq_ignore_ascii_case("k")
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

/// [`schedule_menu_command`] for what acts on the selected messages: from another window,
/// they aren't what is in front of the user.
fn schedule_message_command(id: CommandId, cx: &mut App) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |view, window, cx| {
            let ctx = view.command_context();
            if window.is_window_active() && commands::command_enabled(&id, &ctx) {
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

/// Opens the reader's find bar in the main window. Global so the shortcut also
/// works when nothing in the main window has focus yet.
fn schedule_find_in_message(cx: &mut App) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |root, window, cx| root.open_find_bar(window, cx));
    });
}

/// Steps the zoom of the whole app; `0` goes back to normal. Deferred because the
/// window the key went to is busy dispatching it, and all of them have to change.
fn schedule_app_zoom(steps: i32, cx: &mut App) {
    let main = cx.global::<MainWindow>().0;
    cx.defer(move |cx| {
        let _ = main.update(cx, |root, window, cx| root.zoom_app(steps, window, cx));
    });
}

/// Quits unless a message is being written. Deferred because the window the action
/// came from is busy dispatching it, and the compose window may be the one to ask.
pub(crate) fn schedule_quit(cx: &mut App) {
    // The first-run window comes before the main one.
    let Some(main) = cx.try_global::<MainWindow>().map(|main| main.0) else {
        cx.quit();
        return;
    };
    cx.defer(move |cx| {
        let may_quit = main.update(cx, |root, _, cx| root.may_quit(cx));
        if may_quit.unwrap_or(true) {
            cx.quit();
        }
    });
}

/// Global handlers keep macOS menu items enabled when focus is outside the main
/// view tree (compose/settings windows).
fn register_global_menu_actions(cx: &mut App) {
    cx.on_action(|_: &FindInMessage, cx| schedule_find_in_message(cx));
    cx.on_action(|_: &ComposeNew, cx| schedule_menu_command(CommandId::ComposeNew, cx));
    cx.on_action(|_: &OpenSettings, cx| schedule_menu_command(CommandId::OpenSettings, cx));
    cx.on_action(|_: &ToggleSidebar, cx| schedule_menu_command(CommandId::ToggleSidebar, cx));
    cx.on_action(|_: &AppZoomIn, cx| schedule_app_zoom(1, cx));
    cx.on_action(|_: &AppZoomOut, cx| schedule_app_zoom(-1, cx));
    cx.on_action(|_: &AppZoomReset, cx| schedule_app_zoom(0, cx));
    cx.on_action(|_: &MessageDelete, cx| {
        let main = cx.global::<MainWindow>().0;
        cx.defer(move |cx| {
            // From another window, it wouldn't be the message in front of the user.
            let _ = main.update(cx, |view, window, cx| {
                if window.is_window_active() {
                    view.request_delete(window, cx);
                }
            });
        });
    });
    cx.on_action(|_: &MessageDeletePermanent, cx| {
        schedule_message_command(CommandId::MessageDeletePermanent, cx);
    });
    cx.on_action(|_: &MessageRestore, cx| schedule_message_command(CommandId::MessageRestore, cx));
    cx.on_action(|_: &MessageArchive, cx| schedule_message_command(CommandId::MessageArchive, cx));
    cx.on_action(|_: &MessageMarkJunk, cx| {
        schedule_message_command(CommandId::MessageMarkJunk, cx)
    });
    cx.on_action(|_: &MessagePrint, cx| schedule_message_command(CommandId::MessagePrint, cx));
    cx.on_action(|_: &MessageToggleFlag, cx| {
        schedule_message_command(CommandId::MessageToggleFlag, cx);
    });
    cx.on_action(|_: &MessageReply, cx| schedule_message_command(CommandId::MessageReply, cx));
    cx.on_action(|_: &MessageReplyAll, cx| {
        schedule_message_command(CommandId::MessageReplyAll, cx);
    });
    cx.on_action(|_: &MessageForward, cx| schedule_message_command(CommandId::MessageForward, cx));
    cx.on_action(|_: &MessageToggleRead, cx| {
        schedule_message_command(CommandId::MessageToggleRead, cx);
    });
    cx.on_action(|_: &GetNewMail, cx| schedule_menu_command(CommandId::SyncNow, cx));
    cx.on_action(|action: &MoveMessageToFolder, cx| {
        schedule_message_command(CommandId::MoveToFolder(action.path.clone()), cx);
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

    // A later launch only hands over what it came for; this binding keeps the profile
    // for as long as the process runs.
    let launch_link = instance::mailto_argument(std::env::args().skip(1));
    let _profile = match instance::lock_profile() {
        instance::ProfileLock::Busy => {
            let message = launch_link.as_deref().unwrap_or(instance::ACTIVATE);
            if !instance::hand_over(message) {
                eprintln!("BGMail is already running but did not answer");
            }
            return;
        }
        lock => lock,
    };
    let (links, incoming_links) = async_channel::unbounded::<String>();
    if let Some(link) = launch_link {
        links.send_blocking(link).ok();
    }
    let later_links = links.clone();
    instance::listen(move |link| {
        later_links.send_blocking(link).ok();
    });

    let application = Application::new().with_assets(ui::Assets);
    // Where macOS delivers links, to a bundle.
    application.on_open_urls(move |urls| {
        for url in urls {
            links.send_blocking(url).ok();
        }
    });
    application.run(move |cx: &mut App| {
        startup::log_milestone("application run callback");
        let settings = config::load();
        let ui_scale = app_zoom::sanitize(settings.ui_scale);
        theme::init(settings.appearance(), cx);
        locale::init(settings.language(), cx);
        ui::bind_keys(cx);
        ui::prompt::init(cx);
        html_view::init(cx);
        cx.on_action(|_: &Quit, cx| schedule_quit(cx));
        shortcuts::bind_app_shortcuts(cx);
        cx.set_menus(app_menus());

        // Restore the persisted layout (window/column sizes), clamped to the
        // window's minimum so a stale or tiny config can't open an unusable window.
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
                    window_decorations: window_frame::main_window_decorations(),
                    app_id: Some(root::MAIN_WINDOW_APP_ID.into()),
                    // Windows opens hidden so we can cloak it and run the
                    // (asynchronous) maximize + first layout off-screen, then reveal
                    // the finished window — see the spawn below. Elsewhere GPUI shows
                    // the window immediately.
                    show: !cfg!(target_os = "windows"),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_content_scale(ui_scale, cx);
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
                        // Sync starts after the window exists; it never
                        // blocks the first frame (threads + async keychain).
                        view.start_mail(cx);
                        cx.observe_window_activation(window, |this, window, cx| {
                            if window.is_window_active() {
                                this.on_main_window_activated(cx);
                            }
                        })
                        .detach();
                        // Pick up GNOME button-layout before the first toolbar paint.
                        crate::window_frame::refresh_caption_layout();
                        view.sync_app_menus(cx);
                    });
                    // Closing the window (e.g. the macOS traffic-light button)
                    // doesn't necessarily quit the app, so flush the layout here
                    // too — not just on app quit.
                    let weak = view.downgrade();
                    window.on_window_should_close(cx, move |_window, cx| {
                        let may_quit = weak.upgrade().is_none_or(|view| {
                            view.update(cx, |view, cx| {
                                view.persist_now();
                                view.may_quit(cx)
                            })
                        });
                        if may_quit {
                            cx.quit();
                        }
                        may_quit
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
                                            if window.platform_viewport_size() != actual {
                                                window_drag::nudge_window_resize(window);
                                            }
                                            window_drag::window_layout_settled(
                                                open_maximized,
                                                window.is_maximized(),
                                                window.platform_viewport_size(),
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
        cx.set_global(MainWindow(main_window));
        register_command_palette_shortcuts(cx);
        register_global_menu_actions(cx);

        startup::log_milestone("main window opened");

        main_window
            .update(cx, |view, _, cx| view.watch_unsent(cx))
            .ok();
        cx.spawn(async move |cx| {
            while let Ok(link) = incoming_links.recv().await {
                main_window
                    .update(cx, |view, window, cx| {
                        if link == instance::ACTIVATE {
                            window.activate_window();
                            cx.activate(true);
                        } else {
                            view.open_mailto(&link, window, cx);
                        }
                    })
                    .ok();
            }
        })
        .detach();

        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use gpui::Keystroke;

    use super::*;

    #[test]
    fn command_palette_shortcut_is_ctrl_k_or_cmd_k() {
        let chord = |key: &str, control, platform, shift| Keystroke {
            modifiers: gpui::Modifiers {
                control,
                platform,
                shift,
                ..Default::default()
            },
            key: key.into(),
            key_char: None,
        };
        assert!(is_command_palette_keystroke(&chord(
            "k", true, false, false
        )));
        assert!(is_command_palette_keystroke(&chord(
            "k", false, true, false
        )));
        assert!(is_command_palette_keystroke(&chord(
            "K", true, false, false
        )));
        assert!(!is_command_palette_keystroke(&chord(
            "k", false, false, false
        )));
        assert!(!is_command_palette_keystroke(&chord(
            "k", true, false, true
        )));
        assert!(!is_command_palette_keystroke(&chord(
            "p", true, false, true
        )));
    }
}
