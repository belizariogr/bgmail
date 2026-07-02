//! macOS / global menu bar definitions.
//!
//! GPUI 0.2 does not expose a `disabled` flag on [`MenuItem`]; unavailable message
//! actions are omitted from the Message menu rather than shown greyed out. Menu items
//! are validated with [`App::is_action_available`], which requires a global
//! [`App::on_action`] handler when focus is outside the main view tree (e.g. the
//! native e-mail webview). Keyboard shortcuts are registered in [`crate::shortcuts`]
//! and appear in the menu bar automatically via GPUI's keymap integration.
//!
//! When a compose window is key, [`MenuSurface::Compose`] menus replace the mail
//! reader menus until the main window is focused again.

use gpui::{App, Global, Menu, MenuItem, SystemMenuType};

use crate::actions::{
    ComposeAttach, ComposeClose, ComposeDiscard, ComposeNew, ComposeSend, MessageArchive,
    MessageDelete, MessageDeletePermanent, MessageMarkJunk, MessageRestore, MessageToggleFlag,
    MoveMessageToFolder, OpenSettings, Quit, ToggleCommandPalette, ToggleSidebar,
};
use crate::commands::{self, CommandContext, CommandId};
use crate::locale::{Key, Language};

/// Which window's commands are reflected in the global menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuSurface {
    Main,
    Compose,
}

/// Tracks the menu bar layout last applied via [`sync_main_menus`] /
/// [`sync_compose_menus`].
pub struct ActiveMenuSurface(pub MenuSurface);

impl Global for ActiveMenuSurface {}

fn app_menu() -> Menu {
    Menu {
        name: "rMail".into(),
        items: vec![
            MenuItem::action("About rMail", gpui::NoAction),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit rMail", Quit),
        ],
    }
}

fn edit_menu() -> Menu {
    Menu {
        name: "Edit".into(),
        items: vec![
            MenuItem::os_action("Undo", gpui::NoAction, gpui::OsAction::Undo),
            MenuItem::os_action("Redo", gpui::NoAction, gpui::OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", gpui::NoAction, gpui::OsAction::Cut),
            MenuItem::os_action("Copy", gpui::NoAction, gpui::OsAction::Copy),
            MenuItem::os_action("Paste", gpui::NoAction, gpui::OsAction::Paste),
            MenuItem::os_action("Select All", gpui::NoAction, gpui::OsAction::SelectAll),
        ],
    }
}

/// Builds the menu tree for the main mail window.
pub fn build_menus(ctx: &CommandContext, language: Language) -> Vec<Menu> {
    let move_items: Vec<MenuItem> = ctx
        .move_targets()
        .into_iter()
        .filter(|(_, path)| commands::command_enabled(&CommandId::MoveToFolder(path.clone()), ctx))
        .map(|(label, path)| MenuItem::action(label, MoveMessageToFolder { path }))
        .collect();

    let mut message_items = Vec::new();
    push_if_enabled(
        &mut message_items,
        Key::CommandDelete.tr(language),
        MessageDelete,
        CommandId::MessageDelete,
        ctx,
    );
    push_if_enabled(
        &mut message_items,
        Key::CommandDeletePermanent.tr(language),
        MessageDeletePermanent,
        CommandId::MessageDeletePermanent,
        ctx,
    );
    push_if_enabled(
        &mut message_items,
        Key::CommandRestore.tr(language),
        MessageRestore,
        CommandId::MessageRestore,
        ctx,
    );
    if message_items.len() > 1 {
        message_items.push(MenuItem::separator());
    }
    push_if_enabled(
        &mut message_items,
        Key::CommandArchive.tr(language),
        MessageArchive,
        CommandId::MessageArchive,
        ctx,
    );
    push_if_enabled(
        &mut message_items,
        Key::CommandMarkJunk.tr(language),
        MessageMarkJunk,
        CommandId::MessageMarkJunk,
        ctx,
    );
    push_if_enabled(
        &mut message_items,
        if ctx.message_starred() {
            Key::CommandUnflag.tr(language)
        } else {
            Key::CommandFlag.tr(language)
        },
        MessageToggleFlag,
        CommandId::MessageToggleFlag,
        ctx,
    );
    if !move_items.is_empty() {
        message_items.push(MenuItem::separator());
        message_items.push(MenuItem::submenu(Menu {
            name: Key::CommandMoveTo.tr(language).into(),
            items: move_items,
        }));
    }

    vec![
        app_menu(),
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action(Key::ComposeWindowTitle.tr(language), ComposeNew),
                MenuItem::separator(),
                MenuItem::action(Key::SettingsTitle.tr(language), OpenSettings),
            ],
        },
        edit_menu(),
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action(Key::ToolbarToggleSidebar.tr(language), ToggleSidebar),
                MenuItem::separator(),
                MenuItem::action(Key::CommandPalette.tr(language), ToggleCommandPalette),
            ],
        },
        Menu {
            name: "Message".into(),
            items: message_items,
        },
    ]
}

/// Builds the menu tree while a compose window is key.
pub fn build_compose_menus(language: Language) -> Vec<Menu> {
    vec![
        app_menu(),
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action(Key::ComposeSend.tr(language), ComposeSend),
                MenuItem::action(Key::ComposeAttach.tr(language), ComposeAttach),
                MenuItem::separator(),
                MenuItem::action(Key::ComposeClose.tr(language), ComposeClose),
                MenuItem::separator(),
                MenuItem::action(Key::ComposeDiscard.tr(language), ComposeDiscard),
            ],
        },
        edit_menu(),
    ]
}

/// Returns the current menu surface, defaulting to [`MenuSurface::Main`].
pub fn active_menu_surface(cx: &App) -> MenuSurface {
    cx.try_global::<ActiveMenuSurface>()
        .map(|surface| surface.0)
        .unwrap_or(MenuSurface::Main)
}

fn push_if_enabled<A: gpui::Action>(
    items: &mut Vec<MenuItem>,
    label: &'static str,
    action: A,
    id: CommandId,
    ctx: &CommandContext,
) {
    if commands::command_enabled(&id, ctx) {
        items.push(MenuItem::action(label, action));
    }
}

/// Refreshes the global menu bar for the main mail window.
pub fn sync_main_menus(cx: &mut App, ctx: &CommandContext, language: Language) {
    cx.set_global(ActiveMenuSurface(MenuSurface::Main));
    cx.set_menus(build_menus(ctx, language));
}

/// Refreshes the global menu bar for the compose window.
pub fn sync_compose_menus(cx: &mut App, language: Language) {
    cx.set_global(ActiveMenuSurface(MenuSurface::Compose));
    cx.set_menus(build_compose_menus(language));
}

/// Refreshes the global menu bar from `ctx`.
pub fn sync_menus(cx: &mut App, ctx: &CommandContext, language: Language) {
    sync_main_menus(cx, ctx, language);
}

/// Returns whether `menu` contains an action item whose label matches `label`.
#[cfg(test)]
fn menu_has_action_label(menu: &Menu, label: &str) -> bool {
    menu.items.iter().any(|item| match item {
        MenuItem::Action { name, .. } => name.as_ref() == label,
        MenuItem::Submenu(submenu) => menu_has_action_label(submenu, label),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandContext;
    use std::collections::HashMap;
    use storage::{system, Folder, MessageDetail};

    fn ctx_with_inbox_message() -> CommandContext {
        let account_id = 1;
        CommandContext {
            selected_message_id: Some(7),
            message_detail: Some(MessageDetail {
                id: 7,
                account_id,
                sender: "Alice".into(),
                sender_email: "alice@example.com".into(),
                subject: "Test".into(),
                plain_text: "Hello".into(),
                raw_content: "Hello".into(),
                raw_format: "text".into(),
                received_at: "2024-01-01".into(),
                unread: true,
                starred: false,
                has_attachment: false,
                folders_csv: format!(",{},", system::INBOX),
            }),
            folders_by_account: HashMap::from([(
                account_id,
                vec![Folder {
                    id: 1,
                    account_id,
                    path: system::INBOX.into(),
                    display_name: String::new(),
                }],
            )]),
        }
    }

    #[test]
    fn message_menu_includes_delete_when_message_selected() {
        let menus = build_menus(&ctx_with_inbox_message(), Language::English);
        let message_menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "Message")
            .expect("Message menu");
        assert!(menu_has_action_label(
            message_menu,
            Key::CommandDelete.tr(Language::English),
        ));
    }

    #[test]
    fn message_menu_omits_delete_without_selection() {
        let menus = build_menus(&CommandContext::default(), Language::English);
        let message_menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "Message")
            .expect("Message menu");
        assert!(!menu_has_action_label(
            message_menu,
            Key::CommandDelete.tr(Language::English),
        ));
    }

    #[test]
    fn file_menu_always_includes_compose() {
        let menus = build_menus(&CommandContext::default(), Language::English);
        let file_menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "File")
            .expect("File menu");
        assert!(menu_has_action_label(
            file_menu,
            Key::ComposeWindowTitle.tr(Language::English),
        ));
    }

    #[test]
    fn compose_menus_include_send_attach_close_and_discard() {
        let menus = build_compose_menus(Language::English);
        let file_menu = menus
            .iter()
            .find(|menu| menu.name.as_ref() == "File")
            .expect("File menu");
        assert!(menu_has_action_label(
            file_menu,
            Key::ComposeSend.tr(Language::English),
        ));
        assert!(menu_has_action_label(
            file_menu,
            Key::ComposeAttach.tr(Language::English),
        ));
        assert!(menu_has_action_label(
            file_menu,
            Key::ComposeClose.tr(Language::English),
        ));
        assert!(menu_has_action_label(
            file_menu,
            Key::ComposeDiscard.tr(Language::English),
        ));
        assert!(
            !menus.iter().any(|menu| menu.name.as_ref() == "Message"),
            "compose surface should not show the Message menu"
        );
        assert!(
            !menus.iter().any(|menu| menu.name.as_ref() == "View"),
            "compose surface should not show the View menu"
        );
    }
}
