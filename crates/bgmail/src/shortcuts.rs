//! Application keyboard shortcuts and human-readable labels for tooltips/menus.
//!
//! Bindings follow macOS Mail where practical; each action registers both
//! platform-typical chords so hardware keyboards behave as expected everywhere.

use gpui::{App, KeyBinding, KeybindingKeystroke, Keystroke};

use crate::actions::{
    AppZoomIn, AppZoomOut, AppZoomReset, ComposeClose, ComposeNew, ExtendSelectionDown,
    ExtendSelectionUp, FindInMessage, GetNewMail, MessageArchive, MessageDelete, MessageForward,
    MessageMarkJunk, MessagePrint, MessageReply, MessageReplyAll, MessageToggleFlag,
    MessageToggleRead, OpenSettings, Quit, SelectAllMessages, SelectNextMessage,
    SelectPreviousMessage, ToggleCommandPalette, ToggleSidebar,
};

/// Key context of the focused message list: plain-key navigation and
/// deletion only apply there, never while typing in a text field.
pub const MESSAGE_LIST_CONTEXT: &str = "MessageList";

/// GPUI keystroke string shown in tooltips on macOS.
pub const COMPOSE_MAC: &str = "cmd-n";
/// GPUI keystroke string shown in tooltips on Windows/Linux.
pub const COMPOSE_OTHER: &str = "ctrl-n";

pub const SETTINGS_MAC: &str = "cmd-,";
pub const SETTINGS_OTHER: &str = "ctrl-,";

pub const TOGGLE_SIDEBAR_MAC: &str = "cmd-ctrl-s";
pub const TOGGLE_SIDEBAR_OTHER: &str = "ctrl-alt-s";

pub const COMMAND_PALETTE_MAC: &str = "cmd-k";
pub const COMMAND_PALETTE_OTHER: &str = "ctrl-k";

pub const DELETE_MESSAGE_MAC: &str = "cmd-backspace";
pub const DELETE_MESSAGE_OTHER: &str = "ctrl-backspace";

pub const ARCHIVE_MAC: &str = "cmd-ctrl-a";
pub const ARCHIVE_OTHER: &str = "ctrl-shift-a";

pub const MARK_JUNK_MAC: &str = "cmd-shift-j";
pub const MARK_JUNK_OTHER: &str = "ctrl-shift-j";

pub const TOGGLE_FLAG_MAC: &str = "cmd-shift-l";
pub const TOGGLE_FLAG_OTHER: &str = "ctrl-shift-l";
/// Prints the selected message (through the system browser).
pub const PRINT_MAC: &str = "cmd-p";
pub const PRINT_OTHER: &str = "ctrl-p";

pub const COMPOSE_CLOSE_MAC: &str = "cmd-w";
pub const COMPOSE_CLOSE_OTHER: &str = "ctrl-w";
/// Opens the find bar of the reader.
pub const FIND_MAC: &str = "cmd-f";
pub const FIND_OTHER: &str = "ctrl-f";

/// Reply / reply all / forward, as in macOS Mail.
pub const REPLY_MAC: &str = "cmd-r";
pub const REPLY_OTHER: &str = "ctrl-r";
pub const REPLY_ALL_MAC: &str = "cmd-shift-r";
pub const REPLY_ALL_OTHER: &str = "ctrl-shift-r";
pub const FORWARD_MAC: &str = "cmd-shift-f";
pub const FORWARD_OTHER: &str = "ctrl-shift-f";
/// Mark as read / unread. Elsewhere Ctrl+Shift+U is taken by the input
/// method (IBus/GTK Unicode entry), so it uses Thunderbird's "M" instead.
pub const TOGGLE_READ_MAC: &str = "cmd-shift-u";
pub const TOGGLE_READ_OTHER: &str = "ctrl-shift-m";
/// Get new mail (sync every account now).
pub const GET_MAIL_MAC: &str = "cmd-shift-n";
pub const GET_MAIL_OTHER: &str = "ctrl-shift-n";

/// Message-list keys (in [`MESSAGE_LIST_CONTEXT`] only): next / previous
/// message and move to Trash.
pub const LIST_NEXT_KEYS: [&str; 2] = ["down", "j"];
pub const LIST_PREVIOUS_KEYS: [&str; 2] = ["up", "k"];
pub const LIST_DELETE_KEYS: [&str; 2] = ["delete", "backspace"];
/// Message-list keys growing the selection, and selecting every row.
pub const LIST_EXTEND_DOWN: &str = "shift-down";
pub const LIST_EXTEND_UP: &str = "shift-up";
pub const LIST_SELECT_ALL_MAC: &str = "cmd-a";
pub const LIST_SELECT_ALL_OTHER: &str = "ctrl-a";

/// Primary binding label for tooltips on the current platform.
pub fn primary_binding<'a>(mac: &'a str, other: &'a str) -> &'a str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

/// Formats a GPUI keystroke string for display (e.g. `cmd-n` → `⌘N` on macOS).
pub fn format_binding(source: &str) -> String {
    Keystroke::parse(source)
        .map(|keystroke| KeybindingKeystroke::from_keystroke(keystroke).to_string())
        .unwrap_or_else(|_| source.to_string())
}

/// Registers global key bindings for menu actions. Menus pick up the same chords
/// automatically when rebuilt via [`crate::app_menus::sync_menus`].
pub fn bind_app_shortcuts(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new(COMPOSE_MAC, ComposeNew, None),
        KeyBinding::new(COMPOSE_OTHER, ComposeNew, None),
        KeyBinding::new(SETTINGS_MAC, OpenSettings, None),
        KeyBinding::new(SETTINGS_OTHER, OpenSettings, None),
        KeyBinding::new("secondary-=", AppZoomIn, None),
        KeyBinding::new("secondary-+", AppZoomIn, None),
        KeyBinding::new("secondary--", AppZoomOut, None),
        KeyBinding::new("secondary-0", AppZoomReset, None),
        KeyBinding::new(TOGGLE_SIDEBAR_MAC, ToggleSidebar, None),
        KeyBinding::new(TOGGLE_SIDEBAR_OTHER, ToggleSidebar, None),
        KeyBinding::new(COMMAND_PALETTE_MAC, ToggleCommandPalette, None),
        KeyBinding::new(COMMAND_PALETTE_OTHER, ToggleCommandPalette, None),
        KeyBinding::new(COMMAND_PALETTE_MAC, ToggleCommandPalette, Some("TextInput")),
        KeyBinding::new(
            COMMAND_PALETTE_OTHER,
            ToggleCommandPalette,
            Some("TextInput"),
        ),
        KeyBinding::new(DELETE_MESSAGE_MAC, MessageDelete, None),
        KeyBinding::new(DELETE_MESSAGE_OTHER, MessageDelete, None),
        KeyBinding::new(ARCHIVE_MAC, MessageArchive, None),
        KeyBinding::new(ARCHIVE_OTHER, MessageArchive, None),
        KeyBinding::new(MARK_JUNK_MAC, MessageMarkJunk, None),
        KeyBinding::new(MARK_JUNK_OTHER, MessageMarkJunk, None),
        KeyBinding::new(PRINT_MAC, MessagePrint, None),
        KeyBinding::new(PRINT_OTHER, MessagePrint, None),
        KeyBinding::new(TOGGLE_FLAG_MAC, MessageToggleFlag, None),
        KeyBinding::new(TOGGLE_FLAG_OTHER, MessageToggleFlag, None),
        KeyBinding::new(COMPOSE_CLOSE_MAC, ComposeClose, None),
        KeyBinding::new(COMPOSE_CLOSE_OTHER, ComposeClose, None),
        KeyBinding::new(FIND_MAC, FindInMessage, None),
        KeyBinding::new(FIND_OTHER, FindInMessage, None),
        KeyBinding::new(REPLY_MAC, MessageReply, None),
        KeyBinding::new(REPLY_OTHER, MessageReply, None),
        KeyBinding::new(REPLY_ALL_MAC, MessageReplyAll, None),
        KeyBinding::new(REPLY_ALL_OTHER, MessageReplyAll, None),
        KeyBinding::new(FORWARD_MAC, MessageForward, None),
        KeyBinding::new(FORWARD_OTHER, MessageForward, None),
        KeyBinding::new(TOGGLE_READ_MAC, MessageToggleRead, None),
        KeyBinding::new(TOGGLE_READ_OTHER, MessageToggleRead, None),
        KeyBinding::new(GET_MAIL_MAC, GetNewMail, None),
        KeyBinding::new(GET_MAIL_OTHER, GetNewMail, None),
    ]);
    let list = Some(MESSAGE_LIST_CONTEXT);
    cx.bind_keys(
        LIST_NEXT_KEYS
            .into_iter()
            .map(|key| KeyBinding::new(key, SelectNextMessage, list))
            .chain(
                LIST_PREVIOUS_KEYS
                    .into_iter()
                    .map(|key| KeyBinding::new(key, SelectPreviousMessage, list)),
            )
            .chain(
                LIST_DELETE_KEYS
                    .into_iter()
                    .map(|key| KeyBinding::new(key, MessageDelete, list)),
            )
            .chain([
                KeyBinding::new(LIST_EXTEND_DOWN, ExtendSelectionDown, list),
                KeyBinding::new(LIST_EXTEND_UP, ExtendSelectionUp, list),
                KeyBinding::new(LIST_SELECT_ALL_MAC, SelectAllMessages, list),
                KeyBinding::new(LIST_SELECT_ALL_OTHER, SelectAllMessages, list),
            ]),
    );
}

/// Every global chord pair registered by [`bind_app_shortcuts`], for
/// conflict checks.
#[cfg(test)]
const GLOBAL_CHORDS: [(&str, &str); 17] = [
    (COMPOSE_MAC, COMPOSE_OTHER),
    (SETTINGS_MAC, SETTINGS_OTHER),
    (TOGGLE_SIDEBAR_MAC, TOGGLE_SIDEBAR_OTHER),
    (COMMAND_PALETTE_MAC, COMMAND_PALETTE_OTHER),
    (DELETE_MESSAGE_MAC, DELETE_MESSAGE_OTHER),
    (ARCHIVE_MAC, ARCHIVE_OTHER),
    (MARK_JUNK_MAC, MARK_JUNK_OTHER),
    (TOGGLE_FLAG_MAC, TOGGLE_FLAG_OTHER),
    (PRINT_MAC, PRINT_OTHER),
    (COMPOSE_CLOSE_MAC, COMPOSE_CLOSE_OTHER),
    (FIND_MAC, FIND_OTHER),
    (REPLY_MAC, REPLY_OTHER),
    (REPLY_ALL_MAC, REPLY_ALL_OTHER),
    (FORWARD_MAC, FORWARD_OTHER),
    (TOGGLE_READ_MAC, TOGGLE_READ_OTHER),
    (GET_MAIL_MAC, GET_MAIL_OTHER),
    ("cmd-q", "cmd-q"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn plus_minus_and_zero_zoom_the_app_from_anywhere(cx: &mut gpui::TestAppContext) {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen = Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            bind_app_shortcuts(cx);
            for (id, step) in [(0, 1), (1, -1), (2, 0)] {
                let seen = seen.clone();
                let record = move || seen.borrow_mut().push(step);
                match id {
                    0 => cx.on_action(move |_: &AppZoomIn, _| record()),
                    1 => cx.on_action(move |_: &AppZoomOut, _| record()),
                    _ => cx.on_action(move |_: &AppZoomReset, _| record()),
                };
            }
        });
        let (_, cx) = cx.add_window_view(|_, _| gpui::Empty);
        cx.simulate_keystrokes("secondary-= secondary-+ secondary-- secondary-0");
        assert_eq!(*seen.borrow(), [1, 1, -1, 0]);
    }

    #[test]
    fn format_binding_parses_compose_shortcut() {
        let formatted = format_binding(primary_binding(COMPOSE_MAC, COMPOSE_OTHER));
        assert!(!formatted.is_empty());
        #[cfg(target_os = "macos")]
        assert!(formatted.contains('N') || formatted.contains('n'));
    }

    #[test]
    fn settings_comma_binding_parses() {
        assert!(Keystroke::parse(SETTINGS_MAC).is_ok());
        assert!(Keystroke::parse(SETTINGS_OTHER).is_ok());
    }

    #[test]
    fn global_chords_parse_and_never_collide() {
        let mut seen = std::collections::HashSet::new();
        for (mac, other) in GLOBAL_CHORDS {
            // The same chord on both platforms is one binding (cmd-q).
            let chords: &[&str] = if mac == other { &[mac] } else { &[mac, other] };
            for chord in chords {
                let keystroke = Keystroke::parse(chord).expect(chord);
                assert!(seen.insert(keystroke.unparse()), "{chord} is bound twice");
            }
        }
    }

    #[test]
    fn mail_shortcuts_mirror_macos_mail() {
        assert_eq!(REPLY_MAC, "cmd-r");
        assert_eq!(REPLY_ALL_MAC, "cmd-shift-r");
        assert_eq!(FORWARD_MAC, "cmd-shift-f");
        assert_eq!(TOGGLE_READ_MAC, "cmd-shift-u");
        assert_eq!(GET_MAIL_MAC, "cmd-shift-n");
        for key in LIST_NEXT_KEYS
            .iter()
            .chain(&LIST_PREVIOUS_KEYS)
            .chain(&LIST_DELETE_KEYS)
        {
            let keystroke = Keystroke::parse(key).expect(key);
            assert!(!keystroke.modifiers.modified(), "{key} must be a plain key");
        }
    }

    #[test]
    fn primary_binding_follows_platform() {
        #[cfg(target_os = "macos")]
        assert_eq!(primary_binding(COMPOSE_MAC, COMPOSE_OTHER), COMPOSE_MAC);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(primary_binding(COMPOSE_MAC, COMPOSE_OTHER), COMPOSE_OTHER);
    }
}
