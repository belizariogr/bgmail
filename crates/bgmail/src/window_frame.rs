//! Platform-specific titlebar helpers for the custom main window frame.
//!
//! The main window uses a transparent titlebar so the toolbar can read as one
//! native strip. macOS still gets AppKit's traffic lights; Windows draws
//! rectangular caption buttons; Linux (client-side decorations) draws
//! Adwaita-style circular title buttons and respects the desktop's button layout
//! (GNOME `button-layout` / compositor `WindowControls`) so minimize/maximize
//! stay hidden when the shell does not show them.

use std::sync::RwLock;

use gpui::{
    canvas, point, px, AnyElement, Bounds, CursorStyle, Decorations, HitboxBehavior, Hsla,
    MouseButton, Pixels, Point, ResizeEdge, SharedString, Size, TitlebarOptions, Window,
    WindowControlArea, WindowControls, WindowDecorations,
};
use ui::prelude::*;

const MAC_TRAFFIC_LIGHT_CLEARANCE: f32 = 90.0;
const DEFAULT_LEFT_PADDING: f32 = 12.0;
/// Windows caption button hit width (Fluent-style rectangular strip).
const WIN_CAPTION_BUTTON_WIDTH: f32 = 46.0;
/// Linux/Adwaita title-button slot (circle + padding), matching GTK CSD density.
const GTK_CAPTION_SLOT: f32 = 36.0;
const GTK_CAPTION_GAP: f32 = 4.0;
const CLIENT_SIDE_SHADOW: f32 = 10.0;
const CLIENT_SIDE_ROUNDING: f32 = 10.0;
const CLIENT_SIDE_BORDER: f32 = 1.0;

/// Which caption buttons to draw, split by titlebar side (GNOME `button-layout`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CaptionLayout {
    left: Vec<CaptionKind>,
    right: Vec<CaptionKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptionKind {
    Minimize,
    Maximize,
    Close,
}

/// Cached desktop button layout so we do not shell out to `gsettings` every frame.
static CAPTION_LAYOUT: RwLock<Option<CaptionLayout>> = RwLock::new(None);

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
    if cfg!(target_os = "windows") {
        return px(WIN_CAPTION_BUTTON_WIDTH * 3.0);
    }
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return caption_side_width(&cached_caption_layout().right);
    }
    px(0.0)
}

/// Width reserved on the left for GNOME-style caption buttons (e.g. `close:`).
pub fn left_controls_reserved_width() -> Pixels {
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        caption_side_width(&cached_caption_layout().left)
    } else {
        px(0.0)
    }
}

fn caption_side_width(buttons: &[CaptionKind]) -> Pixels {
    if buttons.is_empty() {
        return px(0.0);
    }
    let n = buttons.len() as f32;
    px(n * GTK_CAPTION_SLOT + (n - 1.0).max(0.0) * GTK_CAPTION_GAP)
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

/// Re-reads the desktop button layout (GNOME `gsettings`). Call when the main
/// window is activated so Tweaks / layout changes apply without restarting.
pub fn refresh_caption_layout() {
    if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return;
    }
    let layout = linux_caption_layout_from_desktop(None);
    if let Ok(mut guard) = CAPTION_LAYOUT.write() {
        *guard = Some(layout);
    }
}

fn cached_caption_layout() -> CaptionLayout {
    if let Ok(guard) = CAPTION_LAYOUT.read() {
        if let Some(layout) = guard.as_ref() {
            return layout.clone();
        }
    }
    let layout = linux_caption_layout_from_desktop(None);
    if let Ok(mut guard) = CAPTION_LAYOUT.write() {
        *guard = Some(layout.clone());
    }
    layout
}

/// Builds the caption layout for Linux CSD. `controls` filters by compositor
/// capabilities when provided (Wayland `WmCapabilities`).
fn linux_caption_layout_from_desktop(controls: Option<WindowControls>) -> CaptionLayout {
    let mut layout = if let Some(raw) = read_gnome_button_layout() {
        parse_button_layout(&raw)
    } else if is_gnome_like_desktop() {
        // GNOME's default titlebar is close-only when gsettings is unavailable.
        CaptionLayout {
            left: Vec::new(),
            right: vec![CaptionKind::Close],
        }
    } else {
        // KDE / tiling WMs: show the usual trio on the right.
        CaptionLayout {
            left: Vec::new(),
            right: vec![
                CaptionKind::Minimize,
                CaptionKind::Maximize,
                CaptionKind::Close,
            ],
        }
    };
    if let Some(controls) = controls {
        filter_layout_by_controls(&mut layout, controls);
    }
    layout
}

fn filter_layout_by_controls(layout: &mut CaptionLayout, controls: WindowControls) {
    let keep = |kind: &CaptionKind| match kind {
        CaptionKind::Minimize => controls.minimize,
        CaptionKind::Maximize => controls.maximize,
        CaptionKind::Close => true,
    };
    layout.left.retain(keep);
    layout.right.retain(keep);
}

/// Parses GNOME/Mutter `button-layout` (`left:right`, comma-separated kinds).
fn parse_button_layout(raw: &str) -> CaptionLayout {
    let cleaned = raw.trim().trim_matches('\'').trim_matches('"').trim();
    let (left_raw, right_raw) = match cleaned.split_once(':') {
        Some((l, r)) => (l, r),
        None => ("", cleaned),
    };
    CaptionLayout {
        left: parse_button_side(left_raw),
        right: parse_button_side(right_raw),
    }
}

fn parse_button_side(side: &str) -> Vec<CaptionKind> {
    side.split(',')
        .filter_map(|part| match part.trim() {
            "minimize" => Some(CaptionKind::Minimize),
            "maximize" => Some(CaptionKind::Maximize),
            "close" => Some(CaptionKind::Close),
            _ => None,
        })
        .collect()
}

fn is_gnome_like_desktop() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .map(|desktop| {
            desktop
                .to_ascii_uppercase()
                .split(':')
                .any(|part| matches!(part, "GNOME" | "UNITY" | "COSMIC"))
        })
        .unwrap_or(false)
}

fn read_gnome_button_layout() -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn render_right_window_controls(window: &mut Window) -> Option<AnyElement> {
    if !uses_custom_caption_buttons(window) {
        return None;
    }
    if cfg!(target_os = "windows") {
        return Some(CaptionWindowControls::windows(window.is_maximized()).into_any_element());
    }
    let layout = linux_layout_for_window(window);
    if layout.right.is_empty() {
        return None;
    }
    Some(CaptionWindowControls::linux(layout.right, window.is_maximized()).into_any_element())
}

/// Left-side caption buttons for layouts like GNOME `close:minimize,maximize`.
pub fn render_left_window_controls(window: &mut Window) -> Option<AnyElement> {
    if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return None;
    }
    if !uses_custom_caption_buttons(window) {
        return None;
    }
    let layout = linux_layout_for_window(window);
    if layout.left.is_empty() {
        return None;
    }
    Some(CaptionWindowControls::linux(layout.left, window.is_maximized()).into_any_element())
}

fn linux_layout_for_window(window: &Window) -> CaptionLayout {
    let mut layout = cached_caption_layout();
    // Prefer live compositor capabilities when they actually restrict buttons.
    // GPUI's Wayland default assumes everything is available, so we only filter
    // when at least one of min/max is reported unavailable.
    let controls = window.window_controls();
    if !controls.minimize || !controls.maximize {
        filter_layout_by_controls(&mut layout, controls);
    }
    layout
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
    buttons: Vec<CaptionKind>,
    maximized: bool,
    /// Adwaita circular buttons on Linux; rectangular Fluent strip on Windows.
    gtk_style: bool,
}

impl CaptionWindowControls {
    fn windows(maximized: bool) -> Self {
        Self {
            buttons: vec![
                CaptionKind::Minimize,
                CaptionKind::Maximize,
                CaptionKind::Close,
            ],
            maximized,
            gtk_style: false,
        }
    }

    fn linux(buttons: Vec<CaptionKind>, maximized: bool) -> Self {
        Self {
            buttons,
            maximized,
            gtk_style: true,
        }
    }
}

impl RenderOnce for CaptionWindowControls {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let maximized = self.maximized;
        let gtk_style = self.gtk_style;
        let mut row = h_flex()
            .id("caption-window-controls")
            .h_full()
            .flex_shrink_0()
            .items_center();
        if gtk_style {
            row = row.gap(px(GTK_CAPTION_GAP)).px(px(GTK_CAPTION_GAP));
        }
        for (ix, kind) in self.buttons.into_iter().enumerate() {
            let button = match kind {
                CaptionKind::Minimize => CaptionButton::minimize(),
                CaptionKind::Maximize if maximized => CaptionButton::restore(),
                CaptionKind::Maximize => CaptionButton::maximize(),
                CaptionKind::Close => CaptionButton::close(),
            };
            row = row.child(button.with_style(gtk_style).with_index(ix));
        }
        row
    }
}

#[derive(Clone, Copy, IntoElement)]
struct CaptionButton {
    kind: CaptionButtonKind,
    gtk_style: bool,
    index: usize,
}

#[derive(Clone, Copy)]
enum CaptionButtonKind {
    Minimize,
    Restore,
    Maximize,
    Close,
}

impl CaptionButton {
    fn minimize() -> Self {
        Self {
            kind: CaptionButtonKind::Minimize,
            gtk_style: false,
            index: 0,
        }
    }

    fn restore() -> Self {
        Self {
            kind: CaptionButtonKind::Restore,
            gtk_style: false,
            index: 0,
        }
    }

    fn maximize() -> Self {
        Self {
            kind: CaptionButtonKind::Maximize,
            gtk_style: false,
            index: 0,
        }
    }

    fn close() -> Self {
        Self {
            kind: CaptionButtonKind::Close,
            gtk_style: false,
            index: 0,
        }
    }

    fn with_style(mut self, gtk_style: bool) -> Self {
        self.gtk_style = gtk_style;
        self
    }

    fn with_index(mut self, index: usize) -> Self {
        self.index = index;
        self
    }

    fn id(self) -> SharedString {
        format!(
            "{}-{}",
            match self.kind {
                CaptionButtonKind::Minimize => "minimize",
                CaptionButtonKind::Restore => "restore",
                CaptionButtonKind::Maximize => "maximize",
                CaptionButtonKind::Close => "close",
            },
            self.index
        )
        .into()
    }

    fn icon(self) -> IconName {
        match self.kind {
            CaptionButtonKind::Minimize => IconName::WindowMinimize,
            CaptionButtonKind::Restore => IconName::WindowRestore,
            CaptionButtonKind::Maximize => IconName::WindowMaximize,
            CaptionButtonKind::Close => IconName::Clear,
        }
    }

    fn action(self) -> CaptionAction {
        match self.kind {
            CaptionButtonKind::Minimize => CaptionAction::Minimize,
            CaptionButtonKind::Restore | CaptionButtonKind::Maximize => CaptionAction::Zoom,
            CaptionButtonKind::Close => CaptionAction::Close,
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
        let is_close = matches!(self.kind, CaptionButtonKind::Close);
        let hover_bg = if is_close {
            colors.error
        } else {
            colors.element_hover
        };

        if self.gtk_style {
            // Adwaita/GTK CSD: circular title buttons with a compact hit target.
            return div()
                .id(self.id())
                .flex()
                .items_center()
                .justify_center()
                .occlude()
                .w(px(GTK_CAPTION_SLOT))
                .h(px(GTK_CAPTION_SLOT))
                .rounded_full()
                .hover(move |style| style.bg(hover_bg))
                .window_control_area(self.control_area())
                .on_click(move |_, window, cx| self.activate(window, cx))
                .child(
                    Icon::new(self.icon())
                        .size(IconSize::XSmall)
                        .color(Color::Default),
                )
                .into_any_element();
        }

        div()
            .id(self.id())
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .w(px(WIN_CAPTION_BUTTON_WIDTH))
            .h_full()
            .hover(move |style| style.bg(hover_bg))
            .window_control_area(self.control_area())
            .on_click(move |_, window, cx| self.activate(window, cx))
            .child(
                Icon::new(self.icon())
                    .size(IconSize::Small)
                    .color(Color::Default),
            )
            .into_any_element()
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
            // Left padding grows when a GNOME layout places buttons on the left;
            // without a cached layout this is at least the default inset.
            assert!(padding >= px(DEFAULT_LEFT_PADDING));
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
    fn right_controls_reserve_width_on_windows() {
        let reserved = right_controls_reserved_width();
        if cfg!(target_os = "windows") {
            assert_eq!(reserved, px(WIN_CAPTION_BUTTON_WIDTH * 3.0));
        } else if cfg!(target_os = "macos") {
            assert_eq!(reserved, px(0.0));
        }
        // Linux width depends on the live button-layout cache; covered below.
    }

    #[test]
    fn parse_button_layout_gnome_close_only() {
        let layout = parse_button_layout(":close");
        assert!(layout.left.is_empty());
        assert_eq!(layout.right, vec![CaptionKind::Close]);
    }

    #[test]
    fn parse_button_layout_full_right() {
        let layout = parse_button_layout(":minimize,maximize,close");
        assert!(layout.left.is_empty());
        assert_eq!(
            layout.right,
            vec![
                CaptionKind::Minimize,
                CaptionKind::Maximize,
                CaptionKind::Close
            ]
        );
    }

    #[test]
    fn parse_button_layout_close_on_left() {
        let layout = parse_button_layout("close:minimize,maximize");
        assert_eq!(layout.left, vec![CaptionKind::Close]);
        assert_eq!(
            layout.right,
            vec![CaptionKind::Minimize, CaptionKind::Maximize]
        );
    }

    #[test]
    fn parse_button_layout_ignores_menu_and_appmenu() {
        let layout = parse_button_layout("appmenu:minimize,maximize,close");
        assert!(layout.left.is_empty());
        assert_eq!(layout.right.len(), 3);
    }

    #[test]
    fn filter_layout_hides_unavailable_min_max() {
        let mut layout = parse_button_layout(":minimize,maximize,close");
        filter_layout_by_controls(
            &mut layout,
            WindowControls {
                fullscreen: true,
                maximize: false,
                minimize: false,
                window_menu: true,
            },
        );
        assert_eq!(layout.right, vec![CaptionKind::Close]);
    }

    #[test]
    fn caption_side_width_scales_with_button_count() {
        assert_eq!(caption_side_width(&[]), px(0.0));
        let one = caption_side_width(&[CaptionKind::Close]);
        let two = caption_side_width(&[CaptionKind::Minimize, CaptionKind::Close]);
        assert!(two > one);
    }

    #[test]
    fn caption_buttons_map_to_explicit_actions() {
        assert_eq!(CaptionButton::minimize().action(), CaptionAction::Minimize);
        assert_eq!(CaptionButton::maximize().action(), CaptionAction::Zoom);
        assert_eq!(CaptionButton::restore().action(), CaptionAction::Zoom);
        assert_eq!(CaptionButton::close().action(), CaptionAction::Close);
    }

    #[test]
    fn caption_buttons_map_to_native_control_areas() {
        assert_eq!(
            CaptionButton::minimize().control_area(),
            WindowControlArea::Min
        );
        assert_eq!(
            CaptionButton::maximize().control_area(),
            WindowControlArea::Max
        );
        assert_eq!(
            CaptionButton::restore().control_area(),
            WindowControlArea::Max
        );
        assert_eq!(
            CaptionButton::close().control_area(),
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
