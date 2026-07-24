//! # Theme
//!
//! BGMail's theme system, modeled after Zed's `theme` crate, but lean and focused
//! on the elements an e-mail client needs.
//!
//! A [`Theme`] is a collection of colors ([`ThemeColors`]) used to build a
//! consistent appearance across all UI components. There are two built-in
//! themes: dark (based on *VSCode Dark Modern*) and light (based on *VSCode
//! Light Modern*).
//!
//! The active theme is stored as a [`gpui::Global`] and accessed through the
//! [`ActiveTheme`] trait, implemented for [`App`]. This lets you write
//! `cx.theme().colors().background` in any component.

use std::sync::Arc;

use gpui::{rgb, App, Global, Hsla};

/// Converts a hexadecimal color (`0xRRGGBB`) into [`Hsla`].
///
/// Centralizes the conversion so palettes can be declared readably using
/// hexadecimal values, as in the VSCode theme files.
#[inline]
fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// Like [`hex`], but overrides the alpha channel (`0.0..=1.0`). Used for
/// translucent surfaces such as scrollbar thumbs.
#[inline]
fn hexa(value: u32, alpha: f32) -> Hsla {
    Hsla {
        a: alpha,
        ..hex(value)
    }
}

/// Theme appearance: light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Light theme.
    Light,
    /// Dark theme.
    Dark,
}

impl Appearance {
    /// Returns `true` if the appearance is light.
    pub fn is_light(self) -> bool {
        matches!(self, Appearance::Light)
    }

    /// Returns the opposite appearance (used by the theme toggle button).
    pub fn toggled(self) -> Self {
        match self {
            Appearance::Light => Appearance::Dark,
            Appearance::Dark => Appearance::Light,
        }
    }
}

/// Set of colors that defines the UI appearance.
///
/// The names follow Zed's convention (`background`, `surface_background`,
/// `element_hover`, etc.) to ease porting components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeColors {
    /// Main background of the application and the reading pane.
    pub background: Hsla,
    /// Background of "fixed" surfaces such as the sidebar and lists.
    pub surface_background: Hsla,
    /// Background of elevated surfaces (context menus, popovers, dialogs).
    pub elevated_surface_background: Hsla,

    /// Default border color (dividers between panels).
    pub border: Hsla,
    /// Lower-contrast border, for subtle divisions.
    pub border_variant: Hsla,
    /// Border of a focused element (keyboard focus).
    pub border_focused: Hsla,

    /// Background of an interactive element (button, input).
    pub element_background: Hsla,
    /// Background of an element under mouse hover.
    pub element_hover: Hsla,
    /// Background of a pressed/active element.
    pub element_active: Hsla,
    /// Background of a selected element (e.g. active list item).
    pub element_selected: Hsla,

    /// Default text color.
    pub text: Hsla,
    /// Muted/secondary text (message preview, timestamp).
    pub text_muted: Hsla,
    /// Disabled text.
    pub text_disabled: Hsla,
    /// Accent/highlight text (links, counters).
    pub text_accent: Hsla,
    /// Text over selected/accent surfaces.
    pub text_on_accent: Hsla,

    /// Default icon fill color.
    pub icon: Hsla,
    /// Color of muted icons.
    pub icon_muted: Hsla,
    /// Color of accent icons (active state).
    pub icon_accent: Hsla,

    /// Primary accent color (selection/highlight blue).
    pub accent: Hsla,

    /// Background of the title bar / top toolbar.
    pub title_bar_background: Hsla,
    /// Background of the bottom status bar.
    pub status_bar_background: Hsla,
    /// Background of the side panel (accounts/mailboxes list).
    pub panel_background: Hsla,

    /// Scrollbar thumb (translucent, drawn over the scrollable content).
    pub scrollbar_thumb: Hsla,
    /// Scrollbar thumb when hovered or being dragged.
    pub scrollbar_thumb_hover: Hsla,

    /// Success color (e.g. connection established).
    pub success: Hsla,
    /// Warning color.
    pub warning: Hsla,
    /// Error color.
    pub error: Hsla,
}

/// A complete theme: identity + appearance + colors.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Human-readable theme name.
    pub name: &'static str,
    /// Whether it is light or dark.
    pub appearance: Appearance,
    /// The theme colors.
    pub colors: ThemeColors,
}

impl Theme {
    /// Shortcut to access the theme colors.
    #[inline]
    pub fn colors(&self) -> &ThemeColors {
        &self.colors
    }

    /// Shortcut for the theme appearance.
    #[inline]
    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// Built-in dark theme, based on *VSCode Dark Modern*.
    pub fn dark() -> Self {
        Theme {
            name: "BGMail Dark",
            appearance: Appearance::Dark,
            colors: ThemeColors {
                background: hex(0x1f1f1f),
                surface_background: hex(0x181818),
                elevated_surface_background: hex(0x252526),

                border: hex(0x2b2b2b),
                border_variant: hex(0x313131),
                border_focused: hex(0x0078d4),

                element_background: hex(0x313131),
                element_hover: hex(0x2a2d2e),
                element_active: hex(0x37373d),
                element_selected: hex(0x04395e),

                text: hex(0xcccccc),
                text_muted: hex(0x9d9d9d),
                text_disabled: hex(0x5a5a5a),
                text_accent: hex(0x4daafc),
                text_on_accent: hex(0xffffff),

                icon: hex(0xcccccc),
                icon_muted: hex(0x858585),
                icon_accent: hex(0x4daafc),

                accent: hex(0x0078d4),

                title_bar_background: hex(0x181818),
                status_bar_background: hex(0x181818),
                panel_background: hex(0x181818),

                scrollbar_thumb: hexa(0xa1a1a1, 0.4),
                scrollbar_thumb_hover: hexa(0xc0c0c0, 0.7),

                success: hex(0x89d185),
                warning: hex(0xcca700),
                error: hex(0xf14c4c),
            },
        }
    }

    /// Built-in light theme, based on *VSCode Light Modern*.
    pub fn light() -> Self {
        Theme {
            name: "BGMail Light",
            appearance: Appearance::Light,
            colors: ThemeColors {
                background: hex(0xffffff),
                surface_background: hex(0xf8f8f8),
                elevated_surface_background: hex(0xffffff),

                border: hex(0xe5e5e5),
                border_variant: hex(0xeaeaea),
                border_focused: hex(0x005fb8),

                element_background: hex(0xf3f3f3),
                element_hover: hex(0xf0f0f0),
                element_active: hex(0xe4e6f1),
                element_selected: hex(0xcfe3fa),

                text: hex(0x3b3b3b),
                text_muted: hex(0x767676),
                text_disabled: hex(0xa0a0a0),
                text_accent: hex(0x005fb8),
                text_on_accent: hex(0xffffff),

                icon: hex(0x3b3b3b),
                icon_muted: hex(0x616161),
                icon_accent: hex(0x005fb8),

                accent: hex(0x005fb8),

                title_bar_background: hex(0xf8f8f8),
                status_bar_background: hex(0xf8f8f8),
                panel_background: hex(0xf8f8f8),

                scrollbar_thumb: hexa(0x6b6b6b, 0.35),
                scrollbar_thumb_hover: hexa(0x4b4b4b, 0.55),

                success: hex(0x1a7f37),
                warning: hex(0xbf8803),
                error: hex(0xcc2936),
            },
        }
    }

    /// Returns the built-in theme matching the given [`Appearance`].
    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Light => Theme::light(),
            Appearance::Dark => Theme::dark(),
        }
    }
}

/// Global state holding the application's active theme.
pub struct GlobalTheme(pub Arc<Theme>);

impl Global for GlobalTheme {}

/// Initializes the theme system on [`App`] with the given appearance.
pub fn init(appearance: Appearance, cx: &mut App) {
    cx.set_global(GlobalTheme(Arc::new(Theme::for_appearance(appearance))));
}

/// Replaces the active theme.
pub fn set_theme(theme: Theme, cx: &mut App) {
    cx.set_global(GlobalTheme(Arc::new(theme)));
    // The theme is a global that views read at render time but don't observe, so
    // every open window must be redrawn for the change to show immediately (not
    // just the window that triggered it).
    cx.refresh_windows();
}

/// Toggles between the light and dark theme, returning the new appearance.
pub fn toggle_appearance(cx: &mut App) -> Appearance {
    let next = cx.global::<GlobalTheme>().0.appearance.toggled();
    set_theme(Theme::for_appearance(next), cx);
    next
}

/// Trait for accessing the active theme from a context.
pub trait ActiveTheme {
    /// Returns the active theme.
    fn theme(&self) -> &Arc<Theme>;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Arc<Theme> {
        &self.global::<GlobalTheme>().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_toggles() {
        assert_eq!(Appearance::Light.toggled(), Appearance::Dark);
        assert_eq!(Appearance::Dark.toggled(), Appearance::Light);
        assert!(Appearance::Light.is_light());
        assert!(!Appearance::Dark.is_light());
    }

    #[test]
    fn builtin_themes_have_matching_appearance() {
        assert_eq!(Theme::dark().appearance(), Appearance::Dark);
        assert_eq!(Theme::light().appearance(), Appearance::Light);
        assert_eq!(Theme::dark().name, "BGMail Dark");
        assert_eq!(Theme::light().name, "BGMail Light");
    }

    #[test]
    fn for_appearance_returns_correct_theme() {
        assert_eq!(Theme::for_appearance(Appearance::Dark), Theme::dark());
        assert_eq!(Theme::for_appearance(Appearance::Light), Theme::light());
    }

    #[test]
    fn dark_and_light_differ() {
        assert_ne!(
            Theme::dark().colors().background,
            Theme::light().colors().background
        );
    }

    #[test]
    fn hex_conversion_is_stable() {
        // Pure white and black must stay at the extremes of lightness.
        assert!(hex(0xffffff).l > 0.99);
        assert!(hex(0x000000).l < 0.01);
    }
}
