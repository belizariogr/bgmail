//! macOS / global menu bar definitions.
//!
//! GPUI 0.2 does not expose a `disabled` flag on [`MenuItem`]; unavailable message
//! actions are omitted from the Message menu rather than shown greyed out. Menu items
//! are validated with [`App::is_action_available`], which requires a global
//! [`App::on_action`] handler when focus is outside the main view tree (e.g. the
//! native e-mail webview). Handlers still guard with [`commands::command_enabled`]
//! when invoked from the palette or toolbar.

use gpui::{App, Menu, MenuItem, SystemMenuType};

use crate::actions::{
    ComposeNew, MessageArchive, MessageDelete, MessageDeletePermanent, MessageMarkJunk,
    MessageRestore, MessageToggleFlag, MoveMessageToFolder, OpenSettings, Quit,
    ToggleCommandPalette, ToggleSidebar,
};
use crate::commands::{self, CommandContext, CommandId};
use crate::locale::{Key, Language};

/// Builds the full application menu tree for the current command context.
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
        Menu {
            name: "rMail".into(),
            items: vec![
                MenuItem::action("About rMail", gpui::NoAction),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit rMail", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action(Key::ComposeWindowTitle.tr(language), ComposeNew),
                MenuItem::separator(),
                MenuItem::action(Key::SettingsTitle.tr(language), OpenSettings),
            ],
        },
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
        },
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

/// Refreshes the global menu bar from `ctx`.
pub fn sync_menus(cx: &mut App, ctx: &CommandContext, language: Language) {
    cx.set_menus(build_menus(ctx, language));
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
}
