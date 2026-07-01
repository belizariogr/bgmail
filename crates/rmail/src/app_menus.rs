//! macOS / global menu bar definitions.
//!
//! GPUI 0.2 does not expose a `disabled` flag on [`MenuItem`]; unavailable message
//! actions are omitted from the Message menu rather than shown greyed out. Handlers
//! still guard with [`commands::command_enabled`] when invoked from the palette or
//! toolbar.

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
