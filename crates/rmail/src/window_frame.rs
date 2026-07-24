//! Platform-specific titlebar helpers for the custom main window frame.
//!
//! The main window uses a transparent titlebar so the toolbar can read as one
//! native strip. macOS still gets AppKit's traffic lights; Windows and Linux
//! (client-side decorations) draw caption buttons inside our toolbar. On Linux
//! we request [`WindowDecorations::Client`] so the compositor drops its SSD —
//! matching Zed — and wrap the content with a light CSD chrome (shadow/resize).

use gpui::{
    canvas, point, px, AnyElement, Bounds, CursorStyle, Decorations, HitboxBehavior, Hsla,
    MouseButton, Pixels, Point, ResizeEdge, Size, TitlebarOptions, Window, WindowControlArea,
    WindowDecorations,
};
use ui::prelude::*;

const MAC_TRAFFIC_LIGHT_CLEARANCE: f32 = 90.0;
const DEFAULT_LEFT_PADDING: f32 = 12.0;
const CAPTION_BUTTON_WIDTH: f32 = 46.0;
const CAPTION_CONTROLS_WIDTH: f32 = CAPTION_BUTTON_WIDTH * 3.0;
const CLIENT_SIDE_SHADOW: f32 = 10.0;
const CLIENT_SIDE_ROUNDING: f32 = 10.0;
const CLIENT_SIDE_BORDER: f32 = 1.0;

pub fn main_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("BGMail".into()),
        appears_transparent: true,
        traffic_light_position: traffic_light_position(),
    }
}

/// Linux (and FreeBSD) main windows ask for client-side decorations so the
/// custom toolbar replaces the compositor title bar, like Zed.
pub fn main_window_decorations() -> Option<WindowDecorations> {
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        Some(WindowDecorations::Client)
    } else {
        None
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

/// Width reserved on the right of the toolbar for custom caption buttons.
pub fn right_controls_reserved_width() -> Pixels {
    if cfg!(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd"
    )) {
        px(CAPTION_CONTROLS_WIDTH)
    } else {
        px(0.0)
    }
}

fn uses_custom_caption_buttons(window: &Window) -> bool {
    if cfg!(target_os = "windows") {
        return true;
    }
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return matches!(window.window_decorations(), Decorations::Client { .. });
    }
    false
}

pub fn render_right_window_controls(window: &mut Window) -> Option<AnyElement> {
    if !uses_custom_caption_buttons(window) {
        return None;
    }
    Some(CaptionWindowControls::new(window.is_maximized()).into_any_element())
}

/// Wraps the main UI in Zed-style client-side decoration chrome when the
/// compositor is using CSD. No-op (returns `content` unchanged) for SSD.
pub fn wrap_client_decorations(
    content: AnyElement,
    window: &mut Window,
    border_color: Hsla,
) -> AnyElement {
    let decorations = window.window_decorations();
    let Decorations::Client { tiling } = decorations else {
        return content;
    };

    let shadow_size = px(CLIENT_SIDE_SHADOW);
    let rounding = px(CLIENT_SIDE_ROUNDING);
    let border_size = px(CLIENT_SIDE_BORDER);
    window.set_client_inset(shadow_size);

    div()
        .id("window-backdrop")
        .bg(gpui::transparent_black())
        .size_full()
        .child(
            canvas(
                |_bounds, window, _cx| {
                    window.insert_hitbox(
                        Bounds::new(
                            point(px(0.0), px(0.0)),
                            window.window_bounds().get_bounds().size,
                        ),
                        HitboxBehavior::Normal,
                    )
                },
                move |_bounds, hitbox, window, _cx| {
                    let mouse = window.mouse_position();
                    let size = window.window_bounds().get_bounds().size;
                    let Some(edge) = resize_edge(mouse, shadow_size, size) else {
                        return;
                    };
                    window.set_cursor_style(cursor_for_resize_edge(edge), &hitbox);
                },
            )
            .size_full()
            .absolute(),
        )
        .when(!(tiling.top || tiling.right), |div| {
            div.rounded_tr(rounding)
        })
        .when(!(tiling.top || tiling.left), |div| div.rounded_tl(rounding))
        .when(!(tiling.bottom || tiling.right), |div| {
            div.rounded_br(rounding)
        })
        .when(!(tiling.bottom || tiling.left), |div| {
            div.rounded_bl(rounding)
        })
        .when(!tiling.top, |div| div.pt(shadow_size))
        .when(!tiling.bottom, |div| div.pb(shadow_size))
        .when(!tiling.left, |div| div.pl(shadow_size))
        .when(!tiling.right, |div| div.pr(shadow_size))
        .on_mouse_move(|_, window, _| window.refresh())
        .on_mouse_down(MouseButton::Left, move |e, window, _| {
            let size = window.window_bounds().get_bounds().size;
            if let Some(edge) = resize_edge(e.position, shadow_size, size) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .overflow_hidden()
                .border_color(border_color)
                .when(!(tiling.top || tiling.right), |div| {
                    div.rounded_tr(rounding)
                })
                .when(!(tiling.top || tiling.left), |div| div.rounded_tl(rounding))
                .when(!(tiling.bottom || tiling.right), |div| {
                    div.rounded_br(rounding)
                })
                .when(!(tiling.bottom || tiling.left), |div| {
                    div.rounded_bl(rounding)
                })
                .when(!tiling.top, |div| div.border_t(border_size))
                .when(!tiling.bottom, |div| div.border_b(border_size))
                .when(!tiling.left, |div| div.border_l(border_size))
                .when(!tiling.right, |div| div.border_r(border_size))
                .when(!tiling.is_tiled(), |div| {
                    div.shadow(vec![gpui::BoxShadow {
                        color: Hsla {
                            h: 0.,
                            s: 0.,
                            l: 0.,
                            a: 0.4,
                        },
                        blur_radius: shadow_size / 2.,
                        spread_radius: px(0.),
                        offset: point(px(0.0), px(0.0)),
                    }])
                })
                .child(content),
        )
        .into_any_element()
}

fn resize_edge(pos: Point<Pixels>, shadow_size: Pixels, size: Size<Pixels>) -> Option<ResizeEdge> {
    let edge = if pos.y < shadow_size && pos.x < shadow_size {
        ResizeEdge::TopLeft
    } else if pos.y < shadow_size && pos.x > size.width - shadow_size {
        ResizeEdge::TopRight
    } else if pos.y < shadow_size {
        ResizeEdge::Top
    } else if pos.y > size.height - shadow_size && pos.x < shadow_size {
        ResizeEdge::BottomLeft
    } else if pos.y > size.height - shadow_size && pos.x > size.width - shadow_size {
        ResizeEdge::BottomRight
    } else if pos.y > size.height - shadow_size {
        ResizeEdge::Bottom
    } else if pos.x < shadow_size {
        ResizeEdge::Left
    } else if pos.x > size.width - shadow_size {
        ResizeEdge::Right
    } else {
        return None;
    };
    Some(edge)
}

fn cursor_for_resize_edge(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

#[derive(IntoElement)]
struct CaptionWindowControls {
    maximized: bool,
}

impl CaptionWindowControls {
    fn new(maximized: bool) -> Self {
        Self { maximized }
    }
}

impl RenderOnce for CaptionWindowControls {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        h_flex()
            .id("caption-window-controls")
            .h_full()
            .flex_shrink_0()
            .child(CaptionButton::Minimize)
            .child(if self.maximized {
                CaptionButton::Restore
            } else {
                CaptionButton::Maximize
            })
            .child(CaptionButton::Close)
    }
}

#[derive(Clone, Copy, IntoElement)]
enum CaptionButton {
    Minimize,
    Restore,
    Maximize,
    Close,
}

impl CaptionButton {
    fn id(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Restore => "restore",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Minimize => IconName::WindowMinimize,
            Self::Restore => IconName::WindowRestore,
            Self::Maximize => IconName::WindowMaximize,
            Self::Close => IconName::Clear,
        }
    }

    fn action(self) -> CaptionAction {
        match self {
            Self::Minimize => CaptionAction::Minimize,
            Self::Restore | Self::Maximize => CaptionAction::Zoom,
            Self::Close => CaptionAction::Close,
        }
    }

    fn control_area(self) -> WindowControlArea {
        match self.action() {
            CaptionAction::Minimize => WindowControlArea::Min,
            CaptionAction::Zoom => WindowControlArea::Max,
            CaptionAction::Close => WindowControlArea::Close,
        }
    }

    fn activate(self, window: &mut Window, cx: &mut App) {
        match self.action() {
            CaptionAction::Minimize => window.minimize_window(),
            CaptionAction::Zoom => window.zoom_window(),
            CaptionAction::Close => cx.quit(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptionAction {
    Minimize,
    Zoom,
    Close,
}

impl RenderOnce for CaptionButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let hover_bg = if matches!(self, Self::Close) {
            colors.error
        } else {
            colors.element_hover
        };

        div()
            .id(self.id())
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .w(px(CAPTION_BUTTON_WIDTH))
            .h_full()
            .hover(move |style| style.bg(hover_bg))
            .window_control_area(self.control_area())
            .on_click(move |_, window, cx| self.activate(window, cx))
            .child(
                Icon::new(self.icon())
                    .size(IconSize::Small)
                    .color(Color::Default),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn main_titlebar_keeps_macos_traffic_lights_only_on_macos() {
        let options = main_titlebar_options();
        assert!(options.appears_transparent);
        let title = options.title.as_ref().map(|title| title.to_string());
        assert_eq!(title.as_deref(), Some("BGMail"));

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
    fn main_window_requests_client_decorations_on_linux() {
        let decorations = main_window_decorations();
        if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            assert_eq!(decorations, Some(WindowDecorations::Client));
        } else {
            assert_eq!(decorations, None);
        }
    }

    #[test]
    fn right_controls_reserve_width_on_windows_and_linux() {
        let reserved = right_controls_reserved_width();
        if cfg!(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )) {
            assert_eq!(reserved, px(CAPTION_CONTROLS_WIDTH));
        } else {
            assert_eq!(reserved, px(0.0));
        }
    }

    #[test]
    fn caption_buttons_map_to_explicit_actions() {
        assert_eq!(CaptionButton::Minimize.action(), CaptionAction::Minimize);
        assert_eq!(CaptionButton::Maximize.action(), CaptionAction::Zoom);
        assert_eq!(CaptionButton::Restore.action(), CaptionAction::Zoom);
        assert_eq!(CaptionButton::Close.action(), CaptionAction::Close);
    }

    #[test]
    fn caption_buttons_map_to_native_control_areas() {
        assert_eq!(
            CaptionButton::Minimize.control_area(),
            WindowControlArea::Min
        );
        assert_eq!(
            CaptionButton::Maximize.control_area(),
            WindowControlArea::Max
        );
        assert_eq!(
            CaptionButton::Restore.control_area(),
            WindowControlArea::Max
        );
        assert_eq!(
            CaptionButton::Close.control_area(),
            WindowControlArea::Close
        );
    }

    #[test]
    fn resize_edge_detects_corners_and_sides() {
        let shadow = px(CLIENT_SIDE_SHADOW);
        let size = size(px(200.0), px(100.0));
        assert_eq!(
            resize_edge(point(px(2.0), px(2.0)), shadow, size),
            Some(ResizeEdge::TopLeft)
        );
        assert_eq!(resize_edge(point(px(100.0), px(50.0)), shadow, size), None);
        assert_eq!(
            resize_edge(point(px(100.0), px(2.0)), shadow, size),
            Some(ResizeEdge::Top)
        );
    }
}
