//! Application keyboard shortcuts and human-readable labels for tooltips/menus.
//!
//! Bindings follow macOS Mail where practical; each action registers both
//! platform-typical chords so hardware keyboards behave as expected everywhere.

use gpui::{App, KeyBinding, KeybindingKeystroke, Keystroke};

use crate::actions::{
    ComposeClose, ComposeNew, MessageArchive, MessageDelete, MessageMarkJunk, MessageToggleFlag,
    OpenSettings, Quit, ToggleCommandPalette, ToggleSidebar,
};

/// GPUI keystroke string shown in tooltips on macOS.
pub const COMPOSE_MAC: &str = "cmd-n";
/// GPUI keystroke string shown in tooltips on Windows/Linux.
pub const COMPOSE_OTHER: &str = "ctrl-n";

pub const SETTINGS_MAC: &str = "cmd-,";
pub const SETTINGS_OTHER: &str = "ctrl-,";

pub const TOGGLE_SIDEBAR_MAC: &str = "cmd-ctrl-s";
pub const TOGGLE_SIDEBAR_OTHER: &str = "ctrl-alt-s";

pub const COMMAND_PALETTE_MAC: &str = "cmd-p";
pub const COMMAND_PALETTE_OTHER: &str = "ctrl-p";

pub const DELETE_MESSAGE_MAC: &str = "cmd-backspace";
pub const DELETE_MESSAGE_OTHER: &str = "ctrl-backspace";

pub const ARCHIVE_MAC: &str = "cmd-ctrl-a";
pub const ARCHIVE_OTHER: &str = "ctrl-shift-a";

pub const MARK_JUNK_MAC: &str = "cmd-shift-j";
pub const MARK_JUNK_OTHER: &str = "ctrl-shift-j";

pub const TOGGLE_FLAG_MAC: &str = "cmd-shift-l";
pub const TOGGLE_FLAG_OTHER: &str = "ctrl-shift-l";

pub const COMPOSE_CLOSE_MAC: &str = "cmd-w";
pub const COMPOSE_CLOSE_OTHER: &str = "ctrl-w";

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
        KeyBinding::new(TOGGLE_FLAG_MAC, MessageToggleFlag, None),
        KeyBinding::new(TOGGLE_FLAG_OTHER, MessageToggleFlag, None),
        KeyBinding::new(COMPOSE_CLOSE_MAC, ComposeClose, None),
        KeyBinding::new(COMPOSE_CLOSE_OTHER, ComposeClose, None),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn primary_binding_follows_platform() {
        #[cfg(target_os = "macos")]
        assert_eq!(primary_binding(COMPOSE_MAC, COMPOSE_OTHER), COMPOSE_MAC);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(primary_binding(COMPOSE_MAC, COMPOSE_OTHER), COMPOSE_OTHER);
    }
}
