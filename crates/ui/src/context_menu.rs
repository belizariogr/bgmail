use std::cell::Cell;
use std::rc::Rc;

use gpui::{canvas, px, AnyElement, Bounds, ClickEvent, Corner, Entity, Point, ScrollHandle, Size};

use crate::prelude::*;
use crate::{Icon, IconName, IconSize, Label, LabelSize, Scrollbar, ScrollbarState};

type EntryHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type HoverHandler = Box<dyn Fn(bool, &mut Window, &mut App) + 'static>;

/// Height of a menu entry, so a menu can be told how many fit.
pub const MENU_ENTRY_HEIGHT: Pixels = px(29.0);
/// Room the scrollbar of a scrolling section takes from its entries.
const SCROLLBAR_ROOM: Pixels = px(10.0);

/// See [`ContextMenu::scroll_first_section`].
struct ScrollSection {
    visible: usize,
    handle: ScrollHandle,
    scrollbar: Entity<ScrollbarState>,
}

/// Room a submenu is expected to need beside its menu, to pick the side it opens on.
const SUBMENU_ROOM: Pixels = px(240.0);
/// From an entry to the outer edge of its menu: the menu's padding and border.
const MENU_INSET: Pixels = px(5.0);
/// Space kept between a submenu and the edge of the window.
const WINDOW_MARGIN: Pixels = px(8.0);

/// What makes an entry lead to a submenu. The host draws the submenu and decides when
/// it shows, as it does with the menu itself (see [`submenu_anchor`]).
pub struct SubmenuTrigger {
    open: bool,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    on_hover: HoverHandler,
}

/// One row of a [`ContextMenu`].
pub enum ContextMenuItem {
    Entry {
        label: SharedString,
        /// Icon shown before the label.
        icon: Option<IconName>,
        /// The icon carries the accent dot.
        icon_dot: bool,
        /// Set when the entry leads to a submenu.
        submenu: Option<SubmenuTrigger>,
        /// Keyboard shortcut shown at the end of the row (e.g. "Ctrl+C").
        key_hint: Option<SharedString>,
        /// Font the label shows in, to preview it.
        font_family: Option<SharedString>,
        handler: EntryHandler,
    },
    Separator,
}

/// A floating menu of actions (named after Zed's `ContextMenu`). The host
/// positions it (e.g. `anchored()` at the click inside `deferred()`) and
/// closes it from the entry handlers or on an outside click.
#[derive(IntoElement, Default)]
pub struct ContextMenu {
    items: Vec<ContextMenuItem>,
    on_hover: Option<HoverHandler>,
    scroll: Option<ScrollSection>,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry running `handler` when clicked.
    pub fn entry(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.items.push(ContextMenuItem::Entry {
            label: label.into(),
            icon: None,
            icon_dot: false,
            submenu: None,
            key_hint: None,
            font_family: None,
            handler: Box::new(handler),
        });
        self
    }

    /// Adds an entry with a keyboard shortcut hint at its end.
    pub fn entry_with_key_hint(
        mut self,
        label: impl Into<SharedString>,
        key_hint: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.items.push(ContextMenuItem::Entry {
            label: label.into(),
            icon: None,
            icon_dot: false,
            submenu: None,
            key_hint: Some(key_hint.into()),
            font_family: None,
            handler: Box::new(handler),
        });
        self
    }

    /// Shows `name` before the last entry's label.
    pub fn icon(mut self, name: IconName) -> Self {
        if let Some(ContextMenuItem::Entry { icon, .. }) = self.items.last_mut() {
            *icon = Some(name);
        }
        self
    }

    /// Puts the accent dot on the last entry's icon when `dot`.
    pub fn icon_dot(mut self, dot: bool) -> Self {
        if let Some(ContextMenuItem::Entry { icon_dot, .. }) = self.items.last_mut() {
            *icon_dot = dot;
        }
        self
    }

    /// Makes the last entry lead to a submenu: it gets a triangle, stays highlighted while
    /// `open`, writes where it is to `bounds` on every paint and calls `on_hover` as the
    /// mouse enters and leaves it.
    pub fn submenu(
        mut self,
        open: bool,
        bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
        on_hover: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        if let Some(ContextMenuItem::Entry { submenu, .. }) = self.items.last_mut() {
            *submenu = Some(SubmenuTrigger {
                open,
                bounds,
                on_hover: Box::new(on_hover),
            });
        }
        self
    }

    /// Calls `handler` as the mouse enters and leaves the menu.
    pub fn on_hover(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }

    /// Scrolls the entries before the first separator, with a scrollbar past `visible`
    /// of them; the ones after it stay in view.
    pub fn scroll_first_section(
        mut self,
        visible: usize,
        handle: ScrollHandle,
        scrollbar: Entity<ScrollbarState>,
    ) -> Self {
        self.scroll = Some(ScrollSection {
            visible,
            handle,
            scrollbar,
        });
        self
    }

    /// Shows the last entry's label in `family`.
    pub fn in_font(mut self, family: impl Into<SharedString>) -> Self {
        if let Some(ContextMenuItem::Entry { font_family, .. }) = self.items.last_mut() {
            *font_family = Some(family.into());
        }
        self
    }

    /// Adds a separator line, unless the menu is empty or already ends with one.
    pub fn separator(mut self) -> Self {
        if matches!(self.items.last(), Some(ContextMenuItem::Entry { .. })) {
            self.items.push(ContextMenuItem::Separator);
        }
        self
    }

    /// The menu's rows.
    pub fn items(&self) -> &[ContextMenuItem] {
        &self.items
    }

    /// Whether the menu has no entries.
    pub fn is_empty(&self) -> bool {
        !self
            .items
            .iter()
            .any(|item| matches!(item, ContextMenuItem::Entry { .. }))
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let mut items = self.items;
        let on_hover = self.on_hover;
        let scroll = self.scroll;
        // A trailing separator separates nothing.
        if matches!(items.last(), Some(ContextMenuItem::Separator)) {
            items.pop();
        }
        let section_len = scroll.as_ref().map_or(0, |_| {
            items
                .iter()
                .position(|item| matches!(item, ContextMenuItem::Separator))
                .unwrap_or(items.len())
        });
        let mut section: Vec<AnyElement> = items
            .into_iter()
            .enumerate()
            .map(|(ix, item)| {
                match item {
                    ContextMenuItem::Separator => {
                        div().my_1().h(px(1.0)).bg(colors.border).into_any_element()
                    }
                    ContextMenuItem::Entry {
                        label,
                        icon,
                        icon_dot,
                        submenu,
                        key_hint,
                        font_family,
                        handler,
                    } => h_flex()
                        .id(("context-menu-entry", ix))
                        .debug_selector(|| format!("context-menu-entry-{ix}"))
                        // Stretched by the column, not `w_full`: a percentage has
                        // nothing to resolve against in a menu sized by its content.
                        .gap_4()
                        .justify_between()
                        .h(MENU_ENTRY_HEIGHT)
                        .flex_shrink_0()
                        .px_2()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|el| el.bg(colors.element_hover))
                        .child(
                            h_flex()
                                .gap_2()
                                .when_some(icon, |el, name| {
                                    let icon = Icon::new(name)
                                        .size(IconSize::Small)
                                        .color(crate::Color::Muted);
                                    if icon_dot {
                                        el.child(
                                            crate::button::icon_with_dot(icon, colors.accent)
                                                .into_any_element(),
                                        )
                                    } else {
                                        el.child(icon.into_any_element())
                                    }
                                })
                                .child(
                                    div()
                                        .when_some(font_family, |el, family| el.font_family(family))
                                        .child(Label::new(label).size(LabelSize::Small)),
                                ),
                        )
                        .when_some(key_hint, |el, hint| {
                            el.child(
                                Label::new(hint)
                                    .size(LabelSize::Small)
                                    .color(crate::Color::Muted),
                            )
                        })
                        .when_some(submenu, |el, submenu| {
                            let SubmenuTrigger {
                                open,
                                bounds,
                                on_hover,
                            } = submenu;
                            el.relative()
                                .when(open, |el| el.bg(colors.element_hover))
                                .on_hover(move |hovered, window, cx| on_hover(*hovered, window, cx))
                                .child(
                                    Icon::new(IconName::CaretRight)
                                        .size(IconSize::XXSmall)
                                        .color(crate::Color::Muted),
                                )
                                .child(
                                    canvas(
                                        move |entry, _, _| bounds.set(Some(entry)),
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full(),
                                )
                        })
                        .on_click(move |event, window, cx| handler(event, window, cx))
                        .into_any_element(),
                }
            })
            .collect();
        let rest = section.split_off(section_len);
        v_flex()
            .id("context-menu")
            .occlude()
            .when_some(on_hover, |el, on_hover| {
                el.on_hover(move |hovered, window, cx| on_hover(*hovered, window, cx))
            })
            .min_w(px(200.0))
            .p_1()
            .bg(colors.elevated_surface_background)
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .shadow_lg()
            .when_some(scroll, |el, scroll| {
                el.child(scroll.render(section, section_len))
            })
            .children(rest)
    }
}

impl ScrollSection {
    fn render(self, entries: Vec<AnyElement>, count: usize) -> impl IntoElement {
        let overflows = count > self.visible;
        v_flex()
            .relative()
            .child(
                v_flex()
                    .id("context-menu-section")
                    .debug_selector(|| "context-menu-section".to_string())
                    .track_scroll(&self.handle)
                    .overflow_y_scroll()
                    .max_h(MENU_ENTRY_HEIGHT * self.visible as f32)
                    .when(overflows, |el| el.pr(SCROLLBAR_ROOM))
                    .children(entries),
            )
            .child(Scrollbar::vertical(self.scrollbar, self.handle).always_visible())
    }
}

/// Where a submenu goes, and the corner of it that goes there: beside its menu, level
/// with the entry `trigger`, on the left if the right has no room.
pub fn submenu_anchor(trigger: Bounds<Pixels>, viewport: Size<Pixels>) -> (Point<Pixels>, Corner) {
    let top = trigger.top() - MENU_INSET;
    let right = trigger.right() + MENU_INSET;
    if right + SUBMENU_ROOM + WINDOW_MARGIN <= viewport.width {
        (gpui::point(right, top), Corner::TopLeft)
    } else {
        (
            gpui::point(trigger.left() - MENU_INSET, top),
            Corner::TopRight,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // As the hosts use it: anchored, in a box that scrolls long menus.
            div().child(gpui::deferred(
                gpui::anchored().child(
                    div().id("menu").max_h(px(400.0)).overflow_y_scroll().child(
                        ContextMenu::new()
                            .entry("A", |_, _, _| {})
                            .entry_with_key_hint("Much longer entry", "Ctrl+A", |_, _, _| {}),
                    ),
                ),
            ))
        }
    }

    #[gpui::test]
    fn entries_fill_the_width_of_the_menu(cx: &mut TestAppContext) {
        cx.update(|cx| theme::init(theme::Appearance::Dark, cx));
        let (_, cx) = cx.add_window_view(|_, _| Host);
        cx.run_until_parked();
        let short = cx
            .debug_bounds("context-menu-entry-0")
            .expect("short entry");
        let long = cx.debug_bounds("context-menu-entry-1").expect("long entry");
        assert_eq!(short.size.width, long.size.width);
        // At least the menu's minimum, less its padding (4px) and border (1px).
        assert!(short.size.width >= px(190.0), "{:?}", short.size.width);
    }

    #[test]
    fn in_font_applies_to_the_last_entry() {
        let menu = ContextMenu::new()
            .entry("A", |_, _, _| {})
            .entry("B", |_, _, _| {})
            .in_font("Serif");
        let fonts: Vec<_> = menu
            .items()
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { font_family, .. } => font_family.clone(),
                ContextMenuItem::Separator => None,
            })
            .collect();
        assert_eq!(fonts, [None, Some("Serif".into())]);
    }

    struct LongMenu {
        handle: ScrollHandle,
        scrollbar: Entity<ScrollbarState>,
    }

    impl Render for LongMenu {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut menu = ContextMenu::new().scroll_first_section(
                12,
                self.handle.clone(),
                self.scrollbar.clone(),
            );
            for ix in 0..20 {
                menu = menu.entry(format!("Folder {ix}"), |_, _, _| {});
            }
            menu.separator().entry("Trash", |_, _, _| {})
        }
    }

    #[gpui::test]
    fn only_the_first_section_scrolls_and_shows_the_entries_it_was_told(cx: &mut TestAppContext) {
        cx.update(|cx| theme::init(theme::Appearance::Dark, cx));
        let (view, cx) = cx.add_window_view(|_, cx| LongMenu {
            handle: ScrollHandle::new(),
            scrollbar: cx.new(|_| ScrollbarState::new()),
        });
        cx.run_until_parked();
        let section = cx
            .debug_bounds("context-menu-section")
            .expect("the section");
        assert_eq!(section.size.height, MENU_ENTRY_HEIGHT * 12.0);
        let hidden = view.read_with(cx, |view, _| view.handle.max_offset().height);
        assert_eq!(hidden, MENU_ENTRY_HEIGHT * 8.0);
        let first = cx
            .debug_bounds("context-menu-entry-0")
            .expect("first entry");
        assert_eq!(first.size.height, MENU_ENTRY_HEIGHT);
        // 20 folders and the separator come before it: it sits below the section.
        let trash = cx
            .debug_bounds("context-menu-entry-21")
            .expect("the entry below");
        assert!(trash.top() >= section.bottom());
        assert!(first.right() + px(5.0) < trash.right(), "room for the bar");
    }

    #[test]
    fn a_submenu_opens_on_the_side_with_room() {
        let trigger = Bounds::new(
            gpui::point(px(100.0), px(50.0)),
            gpui::size(px(200.0), px(24.0)),
        );
        let wide = gpui::size(px(1000.0), px(800.0));
        assert_eq!(
            submenu_anchor(trigger, wide),
            (gpui::point(px(305.0), px(45.0)), Corner::TopLeft)
        );
        let narrow = gpui::size(px(500.0), px(800.0));
        assert_eq!(
            submenu_anchor(trigger, narrow),
            (gpui::point(px(95.0), px(45.0)), Corner::TopRight)
        );
    }

    #[test]
    fn submenu_applies_to_the_last_entry() {
        let bounds = Rc::new(Cell::new(None));
        let menu = ContextMenu::new()
            .submenu(false, bounds.clone(), |_, _, _| {})
            .entry("A", |_, _, _| {})
            .entry("B", |_, _, _| {})
            .submenu(true, bounds, |_, _, _| {});
        let opens: Vec<_> = menu
            .items()
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { submenu, .. } => submenu.as_ref().map(|s| s.open),
                ContextMenuItem::Separator => None,
            })
            .collect();
        assert_eq!(opens, [None, Some(true)]);
    }

    #[test]
    fn the_icon_dot_applies_to_the_last_entry() {
        let menu = ContextMenu::new()
            .entry("A", |_, _, _| {})
            .entry("B", |_, _, _| {})
            .icon_dot(true);
        let dots: Vec<_> = menu
            .items()
            .iter()
            .map(|item| matches!(item, ContextMenuItem::Entry { icon_dot: true, .. }))
            .collect();
        assert_eq!(dots, [false, true]);
    }

    #[test]
    fn icon_applies_to_the_last_entry() {
        let menu = ContextMenu::new()
            .icon(IconName::Trash)
            .entry("A", |_, _, _| {})
            .entry_with_key_hint("B", "Ctrl+B", |_, _, _| {})
            .icon(IconName::Trash)
            .separator()
            .icon(IconName::Reply);
        let icons: Vec<_> = menu
            .items()
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { icon, .. } => *icon,
                ContextMenuItem::Separator => None,
            })
            .collect();
        assert_eq!(icons, [None, Some(IconName::Trash), None]);
    }

    #[test]
    fn separators_only_follow_entries() {
        let menu = ContextMenu::new()
            .separator()
            .entry("Copy", |_, _, _| {})
            .separator()
            .separator()
            .entry_with_key_hint("Select all", "Ctrl+A", |_, _, _| {});
        let kinds: Vec<bool> = menu
            .items()
            .iter()
            .map(|item| matches!(item, ContextMenuItem::Entry { .. }))
            .collect();
        assert_eq!(kinds, [true, false, true]);
    }

    #[test]
    fn empty_menus_have_no_entries() {
        assert!(ContextMenu::new().is_empty());
        assert!(ContextMenu::new().separator().is_empty());
        assert!(!ContextMenu::new().entry("Copy", |_, _, _| {}).is_empty());
    }
}
