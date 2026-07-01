//! GPUI actions for rMail menus and key bindings.

use gpui::{actions, Action, SharedString};

actions!(
    rmail,
    [
        Quit,
        ToggleCommandPalette,
        ComposeNew,
        OpenSettings,
        ToggleSidebar,
        MessageDelete,
        MessageDeletePermanent,
        MessageRestore,
        MessageArchive,
        MessageMarkJunk,
        MessageToggleFlag,
    ]
);

/// Moves the selected message to a folder identified by its storage path.
#[derive(Clone, PartialEq, Eq, Debug, Action)]
#[action(namespace = rmail, no_json)]
pub struct MoveMessageToFolder {
    pub path: SharedString,
}
