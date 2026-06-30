//! Platform-specific titlebar helpers for the custom main window frame.
//!
//! The main window uses a transparent titlebar so the toolbar can read as one
//! native strip. macOS still gets AppKit's traffic lights; Windows needs visible
//! caption buttons inside our toolbar, with GPUI hit-test areas mapped to the OS
//! close/minimize/maximize behavior.

#[cfg(target_os = "windows")]
use gpui::WindowControlArea;
use gpui::{px, AnyElement, TitlebarOptions, Window};
use ui::prelude::*;

const MAC_TRAFFIC_LIGHT_CLEARANCE: f32 = 90.0;
const DEFAULT_LEFT_PADDING: f32 = 12.0;
const WINDOWS_BUTTON_WIDTH: f32 = 46.0;
const WINDOWS_CONTROLS_WIDTH: f32 = WINDOWS_BUTTON_WIDTH * 3.0;

pub fn main_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("rMail".into()),
        appears_transparent: true,
        traffic_light_position: traffic_light_position(),
    }
}

#[cfg(target_os = "macos")]
fn traffic_light_position() -> Option<gpui::Point<Pixels>> {
    Some(gpui::point(px(12.0), px(16.0)))
}

#[cfg(not(target_os = "macos"))]
fn traffic_light_position() -> Option<gpui::Point<Pixels>> {
    None
}

pub fn toolbar_left_padding() -> Pixels {
    if cfg!(target_os = "macos") {
        px(MAC_TRAFFIC_LIGHT_CLEARANCE)
    } else {
        px(DEFAULT_LEFT_PADDING)
    }
}

pub fn right_controls_reserved_width() -> Pixels {
    if cfg!(target_os = "windows") {
        px(WINDOWS_CONTROLS_WIDTH)
    } else {
        px(0.0)
    }
}

#[cfg(target_os = "windows")]
pub fn render_right_window_controls(window: &mut Window) -> Option<AnyElement> {
    Some(WindowsWindowControls::new(window.is_maximized()).into_any_element())
}

#[cfg(not(target_os = "windows"))]
pub fn render_right_window_controls(_window: &mut Window) -> Option<AnyElement> {
    None
}

#[cfg(target_os = "windows")]
#[derive(IntoElement)]
struct WindowsWindowControls {
    maximized: bool,
}

#[cfg(target_os = "windows")]
impl WindowsWindowControls {
    fn new(maximized: bool) -> Self {
        Self { maximized }
    }
}

#[cfg(target_os = "windows")]
impl RenderOnce for WindowsWindowControls {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        h_flex()
            .id("windows-window-controls")
            .h_full()
            .flex_shrink_0()
            .font_family("Segoe Fluent Icons")
            .child(WindowsCaptionButton::Minimize)
            .child(if self.maximized {
                WindowsCaptionButton::Restore
            } else {
                WindowsCaptionButton::Maximize
            })
            .child(WindowsCaptionButton::Close)
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, IntoElement)]
enum WindowsCaptionButton {
    Minimize,
    Restore,
    Maximize,
    Close,
}

#[cfg(target_os = "windows")]
impl WindowsCaptionButton {
    fn id(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Restore => "restore",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Minimize => "\u{e921}",
            Self::Restore => "\u{e923}",
            Self::Maximize => "\u{e922}",
            Self::Close => "\u{e8bb}",
        }
    }

    fn action(self) -> WindowsCaptionAction {
        match self {
            Self::Minimize => WindowsCaptionAction::Minimize,
            Self::Restore | Self::Maximize => WindowsCaptionAction::Zoom,
            Self::Close => WindowsCaptionAction::Close,
        }
    }

    fn control_area(self) -> WindowControlArea {
        match self.action() {
            WindowsCaptionAction::Minimize => WindowControlArea::Min,
            WindowsCaptionAction::Zoom => WindowControlArea::Max,
            WindowsCaptionAction::Close => WindowControlArea::Close,
        }
    }

    fn activate(self, window: &mut Window, cx: &mut App) {
        match self.action() {
            WindowsCaptionAction::Minimize => window.minimize_window(),
            WindowsCaptionAction::Zoom => window.zoom_window(),
            WindowsCaptionAction::Close => cx.quit(),
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowsCaptionAction {
    Minimize,
    Zoom,
    Close,
}

#[cfg(target_os = "windows")]
impl RenderOnce for WindowsCaptionButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (hover_bg, hover_fg) = if matches!(self, Self::Close) {
            (colors.error, colors.text_on_accent)
        } else {
            (colors.element_hover, colors.text)
        };

        div()
            .id(self.id())
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .w(px(WINDOWS_BUTTON_WIDTH))
            .h_full()
            .text_size(px(10.0))
            .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
            .window_control_area(self.control_area())
            .on_click(move |_, window, cx| self.activate(window, cx))
            .child(self.icon())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_titlebar_keeps_macos_traffic_lights_only_on_macos() {
        let options = main_titlebar_options();
        assert!(options.appears_transparent);
        let title = options.title.as_ref().map(|title| title.to_string());
        assert_eq!(title.as_deref(), Some("rMail"));

        if cfg!(target_os = "macos") {
            assert!(options.traffic_light_position.is_some());
        } else {
            assert!(options.traffic_light_position.is_none());
        }
    }

    #[test]
    fn toolbar_padding_reserves_space_for_macos_traffic_lights_only() {
        let padding = toolbar_left_padding();
        if cfg!(target_os = "macos") {
            assert_eq!(padding, px(MAC_TRAFFIC_LIGHT_CLEARANCE));
        } else {
            assert_eq!(padding, px(DEFAULT_LEFT_PADDING));
        }
    }

    #[test]
    fn right_controls_only_reserve_width_on_windows() {
        let reserved = right_controls_reserved_width();
        if cfg!(target_os = "windows") {
            assert_eq!(reserved, px(WINDOWS_CONTROLS_WIDTH));
        } else {
            assert_eq!(reserved, px(0.0));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn caption_buttons_map_to_explicit_actions() {
        assert_eq!(
            WindowsCaptionButton::Minimize.action(),
            WindowsCaptionAction::Minimize
        );
        assert_eq!(
            WindowsCaptionButton::Maximize.action(),
            WindowsCaptionAction::Zoom
        );
        assert_eq!(
            WindowsCaptionButton::Restore.action(),
            WindowsCaptionAction::Zoom
        );
        assert_eq!(
            WindowsCaptionButton::Close.action(),
            WindowsCaptionAction::Close
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn caption_buttons_map_to_native_control_areas() {
        assert_eq!(
            WindowsCaptionButton::Minimize.control_area(),
            WindowControlArea::Min
        );
        assert_eq!(
            WindowsCaptionButton::Maximize.control_area(),
            WindowControlArea::Max
        );
        assert_eq!(
            WindowsCaptionButton::Restore.control_area(),
            WindowControlArea::Max
        );
        assert_eq!(
            WindowsCaptionButton::Close.control_area(),
            WindowControlArea::Close
        );
    }
}
