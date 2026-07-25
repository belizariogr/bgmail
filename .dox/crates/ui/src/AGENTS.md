# crates/ui/src/ — Source API

## Purpose

Source for the `ui` crate: reusable GPUI components, semantic colors, layout
helpers, embedded SVG assets, and platform text-input integration. Every public
item here is re-exported from `ui` via `ui.rs`.

## Ownership

- Owns: All modules under `crates/ui/src/`.
- Does not own: crate-level dependency/feature policy (parent
  `.dox/crates/ui/AGENTS.md`).

## Local Contracts

- Every function/method and top-level type in this tree is listed under **API
  Catalog** with purpose and behavior. Update the matching entry when changing
  signatures or semantics.
- Prefer rustdoc on public items; DOX purpose/behavior should stay aligned with
  rustdoc.
- Visibility in entries (`pub` / `private`) reflects the source item.
- `#[cfg(test)]` items are included when they encode behavioral contracts.

## Work Guidance

- After adding/removing/renaming a function or type, update this catalog in the
  same change.
- Do not weaken parent DOX contracts from root/`crates/`/`ui/`.
- New icons: add SVG under `assets/icons/`, one line in `icon_assets!`, and a
  matching `IconName` variant with a test in `icon.rs`.

## Verification

- `cargo test -p ui`
- Workspace `fmt` / `clippy -D warnings` as required by root `AGENTS.md`.

## Child DOX Index

_(none — modules are files; their APIs are cataloged below.)_

## API Catalog

_138 functions/methods documented._

### `src/ui.rs`

#### Types / constants

- **module `ui`**: Crate root. Declares private submodules, re-exports their
  public API, re-exports `prelude::{h_flex, v_flex}`, and re-exports
  `theme::{ActiveTheme, Appearance, Theme, ThemeColors}` for consumer convenience.

#### Functions / methods

_(No functions — re-exports only.)_

### `src/prelude.rs`

#### Types / constants

- **module `prelude`**: Convenience re-exports of GPUI prelude/types,
  `theme::ActiveTheme`, and core `ui` components (`Button`, `Color`, `Icon`,
  `IconButton`, `IconName`, `IconSize`, `Label`, `LabelSize`, `ListItem`).

#### Functions / methods

##### Context: `module`

- **`h_flex`** (pub, L16)
  - Signature: `pub fn h_flex() -> Div`
  - Purpose: Returns a horizontal flex row with vertically centered children.
  - Behavior: Builds `div().flex().flex_row().items_center()`. Mirrors Zed's
    `h_flex()` helper.

- **`v_flex`** (pub, L23)
  - Signature: `pub fn v_flex() -> Div`
  - Purpose: Returns a vertical flex column container.
  - Behavior: Builds `div().flex().flex_col()`. Mirrors Zed's `v_flex()` helper.

### `src/assets.rs`

#### Types / constants

- **macro `icon_assets!`**: Compile-time table generator. For each `(NAME, path)`
  pair it emits a public `NAME` path constant and embeds bytes via
  `include_bytes!` into the private `ICON_ASSETS` slice used by [`Assets`].
- **pub const icon paths** (L34–L66): Asset path strings for every embedded SVG
  — `CHEVRON_RIGHT`, `CHEVRON_DOWN`, `INBOX`, `SEND`, `FILE_PEN`, `TRASH`,
  `TRIANGLE_EXCLAMATION`, `ARCHIVE`, `STAR`, `STAR_FILLED`, `PALETTE`, `FLAG`,
  `REPLY`, `REPLY_ALL`, `FORWARD`, `PEN_TO_SQUARE`, `SEARCH`, `SETTINGS`,
  `REFRESH`, `CIRCLE_USER`, `PAPERCLIP`, `ENVELOPE`, `SIDEBAR`, `FILTER`,
  `ELLIPSIS`, `FOLDER`, `SHIELD_HALVED`, `SHIELD`, `CHECK`, `XMARK`,
  `WINDOW_MINIMIZE`, `WINDOW_MAXIMIZE`, `WINDOW_RESTORE`. Each value is the
  GPUI asset key (e.g. `"icons/inbox.svg"`).
- **struct `Assets`**: Zero-sized [`AssetSource`] implementation. Register once
  with `Application::new().with_assets(ui::Assets)` so `gpui::svg()` can load
  embedded icon bytes by path.

#### Functions / methods

##### Context: `AssetSource for Assets`

- **`load`** (private, L74)
  - Signature: `fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>>`
  - Purpose: Resolves an asset path to embedded SVG bytes.
  - Behavior: Linear search in `ICON_ASSETS` for a matching path. Returns
    `Ok(Some(Cow::Borrowed(bytes)))` on hit, `Ok(None)` when unknown. Never
    errors for a missing path.

- **`list`** (private, L81)
  - Signature: `fn list(&self, _path: &str) -> Result<Vec<SharedString>>`
  - Purpose: Lists every embedded icon asset path.
  - Behavior: Ignores the `_path` argument. Returns all keys from `ICON_ASSETS`
    as `SharedString` values.

##### Context: `module` (tests)

- **`loads_every_embedded_icon`** (private, L94)
  - Signature: `fn loads_every_embedded_icon()`
  - Purpose: Regression test that every embedded asset loads as non-empty SVG.
  - Behavior: Iterates `ICON_ASSETS`, calls `Assets::load` for each path,
    asserts bytes exist, start with `<svg`, and are non-empty. Panics on failure.

- **`unknown_path_is_none`** (private, L106)
  - Signature: `fn unknown_path_is_none()`
  - Purpose: Regression test for missing asset lookup.
  - Behavior: Asserts `Assets.load("icons/does-not-exist.svg")` returns
    `Ok(None)`.

- **`list_reports_every_icon`** (private, L111)
  - Signature: `fn list_reports_every_icon()`
  - Purpose: Regression test that `list` covers the full embed table.
  - Behavior: Asserts `Assets.list("")` length equals `ICON_ASSETS.len()`.

### `src/color.rs`

#### Types / constants

- **enum `Color`**: Semantic color roles resolved at render time against the
  active theme. Variants: `Default`, `Muted`, `Disabled`, `Accent`, `OnAccent`,
  `Success`, `Warning`, `Error`, `Custom(Hsla)`. `Default` is the default
  variant.

#### Functions / methods

##### Context: `Color`

- **`hsla`** (pub, L35)
  - Signature: `pub fn hsla(self, cx: &App) -> Hsla`
  - Purpose: Resolves a semantic color to concrete theme pixels.
  - Behavior: Reads `cx.theme().colors()` and maps each variant to the matching
    theme field (`text`, `text_muted`, `text_disabled`, `text_accent`,
    `text_on_accent`, `success`, `warning`, `error`). `Custom` passes through
    unchanged.

### `src/icon.rs`

#### Types / constants

- **enum `IconSize`**: Square footprint presets — `XXSmall` (10px), `XSmall`
  (12px), `Small` (14px, default), `Medium` (16px).
- **enum `IconName`**: Stable icon identifiers for BGMail UI. Variants map to
  embedded SVG paths via `path()` — mailbox/actions (`Inbox`, `Sent`, `Drafts`,
  `Trash`, `Junk`, `Archive`, `Star`, `StarFilled`, …), compose/navigation
  (`Compose`, `Send`, `Search`, `ChevronRight`, `ChevronDown`, …), chrome
  (`Sidebar`, `Settings`, `WindowMinimize`, `WindowMaximize`, `WindowRestore`,
  …). `Sent` and `Send` share the paper-plane artwork but are distinct enum
  values.
- **struct `Icon`**: Themed SVG icon element (`IntoElement` + `RenderOnce`).
  Fields: `name`, `size`, `color`.

#### Functions / methods

##### Context: `IconSize`

- **`px`** (private, L20)
  - Signature: `fn px(self) -> Pixels`
  - Purpose: Converts an icon size preset to its square pixel dimension.
  - Behavior: Maps each variant to `px(10|12|14|16)`.

##### Context: `IconName`

- **`path`** (pub, L83)
  - Signature: `pub fn path(self) -> &'static str`
  - Purpose: Returns the GPUI asset path for this icon's embedded SVG.
  - Behavior: `match`es each variant to the corresponding `crate::…` path
    constant from `assets.rs`. Used by `Icon::render` and asset tests.

##### Context: `Icon`

- **`new`** (pub, L133)
  - Signature: `pub fn new(name: IconName) -> Self`
  - Purpose: Creates an icon with default size (`Small`) and color (`Default`).
  - Behavior: Stores `name`; initializes `size` and `color` to defaults.

- **`size`** (pub, L142)
  - Signature: `pub fn size(mut self, size: IconSize) -> Self`
  - Purpose: Sets the rendered square footprint.
  - Behavior: Builder method; updates `self.size` and returns `self`.

- **`color`** (pub, L148)
  - Signature: `pub fn color(mut self, color: Color) -> Self`
  - Purpose: Sets the semantic tint applied to the SVG.
  - Behavior: Builder method; updates `self.color` and returns `self`.

##### Context: `RenderOnce for Icon`

- **`render`** (private, L155)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Paints the icon as a tinted GPUI SVG element.
  - Behavior: Resolves `self.color` via `hsla(cx)`, converts `self.size` to
    pixels, builds `svg()` with fixed width/height, `path(self.name.path())`,
    and `text_color`. SVG fill comes from alpha coverage; color is applied as
    tint.

##### Context: `module` (tests)

- **`every_icon_points_to_an_svg_asset`** (private, L212)
  - Signature: `fn every_icon_points_to_an_svg_asset()`
  - Purpose: Ensures every `IconName` maps to an `.svg` path string.
  - Behavior: Iterates the `ALL` constant array; asserts each `path()` ends with
    `.svg`.

- **`every_icon_resolves_to_embedded_bytes`** (private, L226)
  - Signature: `fn every_icon_resolves_to_embedded_bytes()`
  - Purpose: Ensures font-free icons never regress to missing/tofu rendering.
  - Behavior: For each `IconName`, loads bytes via `Assets::load(path())`,
    asserts load succeeds, bytes exist, and start with `<svg`.

- **`star_variants_use_distinct_assets`** (private, L240)
  - Signature: `fn star_variants_use_distinct_assets()`
  - Purpose: Guards against accidental asset sharing between star states.
  - Behavior: Asserts `Star` and `StarFilled` resolve to different paths.

- **`shield_variants_use_distinct_assets`** (private, L245)
  - Signature: `fn shield_variants_use_distinct_assets()`
  - Purpose: Guards against accidental asset sharing between shield states.
  - Behavior: Asserts `Shield` and `ShieldSolid` resolve to different paths.

- **`sizes_are_distinct_and_ascending`** (private, L250)
  - Signature: `fn sizes_are_distinct_and_ascending()`
  - Purpose: Validates icon size ordering for layout consistency.
  - Behavior: Asserts strict ascending order of `px()` across all four
    `IconSize` variants.

### `src/label.rs`

#### Types / constants

- **enum `LabelSize`**: Text size presets — `XSmall` (11px), `Small` (12px),
  `Default` (14px), `Large` (16px). Default variant is `Default`.
- **struct `Label`**: Themed single-line or multi-line text (`IntoElement`).
  Fields: `text`, `size`, `color`, `weight`, `single_line`.

#### Functions / methods

##### Context: `LabelSize`

- **`px`** (private, L20)
  - Signature: `fn px(self) -> Pixels`
  - Purpose: Converts a label size preset to pixel `text_size`.
  - Behavior: Maps each variant to `px(11|12|14|16)`.

##### Context: `Label`

- **`new`** (pub, L46)
  - Signature: `pub fn new(text: impl Into<SharedString>) -> Self`
  - Purpose: Creates a label with default size, color, and normal weight.
  - Behavior: Converts `text` to `SharedString`; sets `size` to default,
    `color` to `Default`, `weight` to `FontWeight::NORMAL`, `single_line`
    to `false`.

- **`size`** (pub, L57)
  - Signature: `pub fn size(mut self, size: LabelSize) -> Self`
  - Purpose: Sets the rendered text size.
  - Behavior: Builder; assigns `size` and returns `self`.

- **`color`** (pub, L63)
  - Signature: `pub fn color(mut self, color: Color) -> Self`
  - Purpose: Sets the semantic text color.
  - Behavior: Builder; assigns `color` and returns `self`.

- **`weight`** (pub, L69)
  - Signature: `pub fn weight(mut self, weight: FontWeight) -> Self`
  - Purpose: Sets the GPUI font weight.
  - Behavior: Builder; assigns `weight` and returns `self`.

- **`bold`** (pub, L75)
  - Signature: `pub fn bold(mut self) -> Self`
  - Purpose: Shortcut for bold weight.
  - Behavior: Sets `weight` to `FontWeight::BOLD` and returns `self`.

- **`semibold`** (pub, L81)
  - Signature: `pub fn semibold(mut self) -> Self`
  - Purpose: Shortcut for semibold weight.
  - Behavior: Sets `weight` to `FontWeight::SEMIBOLD` and returns `self`.

- **`single_line`** (pub, L87)
  - Signature: `pub fn single_line(mut self) -> Self`
  - Purpose: Enables single-line truncation with ellipsis.
  - Behavior: Sets `single_line` to `true`. At render time applies
    `overflow_hidden`, `whitespace_nowrap`, and `text_ellipsis`.

##### Context: `RenderOnce for Label`

- **`render`** (private, L94)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Renders themed label text inside a `div`.
  - Behavior: Resolves color via `hsla(cx)`, applies `text_size`, `text_color`,
    and `font_weight`. When `single_line`, adds overflow/ellipsis modifiers.
    Child is the label string.

### `src/button.rs`

#### Types / constants

- **enum `ButtonStyle`**: Visual variants — `Subtle` (default, secondary),
  `Filled` (primary accent), `Ghost` (transparent until hover).
- **type `ClickHandler`**: `Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>`.
- **struct `Button`**: Text button with optional leading icon. Fields: `id`,
  `label`, `icon`, `style`, `full_width`, `on_click`.
- **struct `IconButton`**: Square icon-only toolbar button. Fields: `id`, `icon`,
  `size`, `color`, `selected`, `on_click`.

#### Functions / methods

##### Context: `Button`

- **`new`** (pub, L32)
  - Signature: `pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self`
  - Purpose: Creates a subtle-style text button without icon or handler.
  - Behavior: Stores `id` and `label`; defaults `icon` to `None`, `style` to
    `Subtle`, `full_width` to `false`, `on_click` to `None`.

- **`icon`** (pub, L44)
  - Signature: `pub fn icon(mut self, icon: IconName) -> Self`
  - Purpose: Prepends an icon before the label (e.g. Send button).
  - Behavior: Sets `self.icon = Some(icon)`; returns `self`.

- **`style`** (pub, L50)
  - Signature: `pub fn style(mut self, style: ButtonStyle) -> Self`
  - Purpose: Chooses background/hover treatment.
  - Behavior: Assigns `style`; returns `self`.

- **`full_width`** (pub, L56)
  - Signature: `pub fn full_width(mut self) -> Self`
  - Purpose: Makes the button expand to container width.
  - Behavior: Sets `full_width = true`; at render adds `.w_full()`.

- **`on_click`** (pub, L62)
  - Signature: `pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self`
  - Purpose: Registers a left-click handler.
  - Behavior: Boxes handler into `on_click`. Render adds pointer cursor,
    swallows left mouse-down (prevents title-bar drag stealing clicks), and
    forwards `ClickEvent` to the handler.

##### Context: `RenderOnce for Button`

- **`render`** (private, L72)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Paints the button row with theme-aware colors and optional icon.
  - Behavior: Reads theme colors; maps `ButtonStyle` to `(bg, text_color,
    hover_bg)` — subtle uses element background, filled uses accent/on-accent,
    ghost is transparent with hover fill. Builds centered horizontal flex with
    padding, rounded corners, 13px text. Optional icon uses `OnAccent` tint
    when filled, else `Default`. Applies hover background swap. Wires click
    handling when `on_click` is set.

##### Context: `IconButton`

- **`new`** (pub, L126)
  - Signature: `pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self`
  - Purpose: Creates a 28px square icon button.
  - Behavior: Defaults `size` to `IconSize::default()`, `color` to `Default`,
    `selected` to `false`, `on_click` to `None`.

- **`size`** (pub, L138)
  - Signature: `pub fn size(mut self, size: IconSize) -> Self`
  - Purpose: Sets the inner icon size.
  - Behavior: Assigns `size`; returns `self`.

- **`color`** (pub, L144)
  - Signature: `pub fn color(mut self, color: Color) -> Self`
  - Purpose: Sets icon color when not selected.
  - Behavior: Assigns `color`; returns `self`. Render overrides with `Accent`
    when `selected`.

- **`selected`** (pub, L150)
  - Signature: `pub fn selected(mut self, selected: bool) -> Self`
  - Purpose: Marks active/toolbar-selected state.
  - Behavior: Sets `selected`; render applies `element_active` background and
    accent icon color.

- **`on_click`** (pub, L156)
  - Signature: `pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self`
  - Purpose: Registers click handler with drag-safe mouse-down swallow.
  - Behavior: Same propagation pattern as `Button::on_click`.

##### Context: `RenderOnce for IconButton`

- **`render`** (private, L166)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Paints square icon button with hover/selected chrome.
  - Behavior: 28px rounded flex container. Selected state gets active
    background; all states get hover background. Renders `Icon` at configured
    size/color (accent when selected). Optional click wiring mirrors `Button`.

### `src/list_item.rs`

#### Types / constants

- **type `ClickHandler`**: `Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>`.
- **struct `ListItem`**: Full-width clickable row for sidebars and message lists.
  Supports selection/hover chrome, content inset, start/end slots, and multiple
  main children. Fields: `id`, `selected`, `inset`, `start_slot`, `end_slot`,
  `children`, `on_click`.

#### Functions / methods

##### Context: `ListItem`

- **`new`** (pub, L24)
  - Signature: `pub fn new(id: impl Into<ElementId>) -> Self`
  - Purpose: Creates an unselected row with no slots, children, or handler.
  - Behavior: Defaults `selected` false, `inset` zero, slots/handler `None`,
    empty `children` vector.

- **`selected`** (pub, L37)
  - Signature: `pub fn selected(mut self, selected: bool) -> Self`
  - Purpose: Toggles selected background styling.
  - Behavior: Assigns `selected`; render uses `element_selected` when true and
    suppresses hover highlight.

- **`inset`** (pub, L45)
  - Signature: `pub fn inset(mut self, inset: Pixels) -> Self`
  - Purpose: Indents row content without shrinking the full-width highlight.
  - Behavior: Adds `inset` to left padding (`pl(8.0 + inset)`) while background
    still spans full row width.

- **`start_slot`** (pub, L51)
  - Signature: `pub fn start_slot(mut self, element: impl IntoElement) -> Self`
  - Purpose: Sets left slot (typically an icon).
  - Behavior: Stores `element.into_any_element()` in `start_slot`.

- **`end_slot`** (pub, L57)
  - Signature: `pub fn end_slot(mut self, element: impl IntoElement) -> Self`
  - Purpose: Sets right slot (badge, counter, etc.).
  - Behavior: Stores element in `end_slot`.

- **`child`** (pub, L63)
  - Signature: `pub fn child(mut self, element: impl IntoElement) -> Self`
  - Purpose: Appends to the flexible main content column.
  - Behavior: Pushes `into_any_element()` onto `children`; returns `self`.

- **`on_click`** (pub, L69)
  - Signature: `pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self`
  - Purpose: Makes the row clickable.
  - Behavior: Boxes handler; render adds pointer cursor and `on_click` forward.

##### Context: `RenderOnce for ListItem`

- **`render`** (private, L79)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Paints a full-width row with slots and selection/hover states.
  - Behavior: Horizontal flex with gap, inset padding, rounded corners.
    Selected rows get `element_selected` background; unselected rows get hover
    `element_hover`. Start slot, flex-1 main column (`min_w_0` for truncation),
    and end slot render in order. Click handler attached when present.

### `src/switch.rs`

#### Types / constants

- **type `ClickHandler`**: `Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>`.
- **struct `Switch`**: Pill-track toggle with sliding thumb (Zed-style). Optional
  right label; entire row clickable when enabled. Fields: `id`, `checked`,
  `disabled`, `label`, `on_click`.

#### Functions / methods

##### Context: `Switch`

- **`new`** (pub, L22)
  - Signature: `pub fn new(id: impl Into<ElementId>, checked: bool) -> Self`
  - Purpose: Creates a switch in the given on/off state.
  - Behavior: Stores `checked`; defaults `disabled` false, no label or handler.

- **`label`** (pub, L33)
  - Signature: `pub fn label(mut self, label: impl Into<SharedString>) -> Self`
  - Purpose: Adds descriptive text to the right of the track.
  - Behavior: Stores label; render shows small `Label` beside track.

- **`disabled`** (pub, L39)
  - Signature: `pub fn disabled(mut self, disabled: bool) -> Self`
  - Purpose: Greys out the control and suppresses interaction.
  - Behavior: When disabled, thumb opacity drops, label uses `Color::Disabled`,
    hover styling is omitted, and `on_click` is not wired.

- **`on_click`** (pub, L45)
  - Signature: `pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self`
  - Purpose: Registers toggle handler (caller flips `checked` state).
  - Behavior: Handler only attached when not disabled. Swallows mouse-down to
    avoid title-bar drag interference.

##### Context: `RenderOnce for Switch`

- **`render`** (private, L55)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Paints track, thumb, optional label, and click target.
  - Behavior: On-state track uses translucent accent fill/border; off-state uses
    element background and border. Thumb is theme text color with opacity based
    on disabled/checked. Track uses group hover brighten. Thumb aligns start/end
    within 32×20 pill. Label renders when set. Click row when handler present
    and enabled.

### `src/tooltip.rs`

#### Types / constants

- **struct `Tooltip`**: Minimal hover tooltip view. Fields: `text`, optional
  `shortcut`. Implements `Render` (not `RenderOnce`) because GPUI tooltip
  builders instantiate it as an entity view.

#### Functions / methods

##### Context: `Tooltip`

- **`text`** (pub, L19)
  - Signature: `pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView`
  - Purpose: Factory for `Element::tooltip` closures showing text only.
  - Behavior: Delegates to `build(text, None)`. Returned closure creates a new
    `Tooltip` entity via `cx.new` and converts to `AnyView`.

- **`with_shortcut`** (pub, L24)
  - Signature: `pub fn with_shortcut(text: impl Into<SharedString>, shortcut: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView`
  - Purpose: Factory for tooltips with a muted shortcut label on the right.
  - Behavior: Calls `build(text, Some(shortcut))`.

- **`build`** (private, L31)
  - Signature: `fn build(text: impl Into<SharedString>, shortcut: Option<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView`
  - Purpose: Shared closure builder capturing text and optional shortcut.
  - Behavior: Captures strings; closure ignores window, spawns `Tooltip { text,
    shortcut }` entity, returns `AnyView`.

##### Context: `Render for Tooltip`

- **`render`** (private, L47)
  - Signature: `fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Paints elevated tooltip panel with body text and optional shortcut.
  - Behavior: Uses elevated surface background and border. Without shortcut,
    single small `Label`. With shortcut, horizontal flex: primary label plus
    muted shortcut label. Padding and rounded corners applied on outer `div`.

### `src/scrollbar.rs`

#### Types / constants

- **const `WIDTH`** (private): Thumb and track thickness — 6px.
- **const `PADDING`** (private): Gap between thumb and container edge — 3px.
- **const `HOVER_WIDTH`** (private): Wide invisible hover strip — 14px.
- **const `MIN_THUMB`** (private): Minimum thumb length — 24px (keeps grab target).
- **const `AUTO_HIDE`** (pub, L34): Duration thumb stays visible after scroll —
  250ms.
- **struct `ScrollbarState`**: Persistent entity state for drag, hover, and
  auto-hide timing. Fields: `drag_grab`, `last_scroll`, `hovered`.
- **struct `Scrollbar`**: Custom `Element` overlay bound to a `ScrollHandle` and
  shared `ScrollbarState`. Fields: `state`, `handle`, `axis` (`Vertical` or
  `Horizontal`).
- **struct `ScrollbarLayout`**: Prepaint output consumed in `paint`. Fields:
  `track`, `thumb`, `hover_hitbox`, `max_offset`.

#### Functions / methods

##### Context: `ScrollbarState`

- **`new`** (pub, L49)
  - Signature: `pub fn new() -> Self`
  - Purpose: Creates idle scrollbar state.
  - Behavior: Returns `Default` — no drag, no recent scroll, not hovered.

- **`note_scroll`** (pub, L54)
  - Signature: `pub fn note_scroll(&mut self)`
  - Purpose: Marks scrolling activity for auto-hide visibility.
  - Behavior: Sets `last_scroll` to `Instant::now()`. Callers invoke after wheel
    or programmatic scroll so the thumb stays visible briefly.

- **`recently_scrolled`** (private, L59)
  - Signature: `fn recently_scrolled(&self) -> bool`
  - Purpose: Reports whether scroll happened within `AUTO_HIDE`.
  - Behavior: True when `last_scroll` exists and elapsed time is less than
    `AUTO_HIDE`.

##### Context: `Scrollbar`

- **`vertical`** (pub, L73)
  - Signature: `pub fn vertical(state: Entity<ScrollbarState>, handle: ScrollHandle) -> Self`
  - Purpose: Builds a right-edge vertical scrollbar overlay.
  - Behavior: Stores entities with `axis = Vertical`.

- **`horizontal`** (pub, L82)
  - Signature: `pub fn horizontal(state: Entity<ScrollbarState>, handle: ScrollHandle) -> Self`
  - Purpose: Builds a bottom-edge horizontal scrollbar overlay.
  - Behavior: Same as vertical but `axis = Horizontal`. Both axes can share one
    handle/state pair.

##### Context: `module`

- **`main_axis`** (private, L92)
  - Signature: `fn main_axis(horizontal: bool, p: Point<Pixels>) -> f32`
  - Purpose: Extracts x or y from a point along the scrollbar axis.
  - Behavior: Returns `p.x` when horizontal, else `p.y`, as `f32`.

- **`origin_main`** (private, L101)
  - Signature: `fn origin_main(horizontal: bool, bounds: &Bounds<Pixels>) -> f32`
  - Purpose: Extracts origin coordinate along the scrollbar axis.
  - Behavior: Returns `bounds.origin.x` or `.y` depending on axis.

- **`size_main`** (private, L110)
  - Signature: `fn size_main(horizontal: bool, bounds: &Bounds<Pixels>) -> f32`
  - Purpose: Extracts extent along the scrollbar axis.
  - Behavior: Returns `bounds.size.width` or `.height` depending on axis.

- **`apply_offset`** (private, L119)
  - Signature: `fn apply_offset(handle: &ScrollHandle, horizontal: bool, offset: f32)`
  - Purpose: Writes scroll offset on one axis while preserving the other.
  - Behavior: Reads current offset; sets x or y to `px(offset)` via
    `handle.set_offset`.

- **`thumb_geometry`** (private, L133)
  - Signature: `fn thumb_geometry(track: f32, viewport: f32, max_offset: f32, scroll: f32) -> Option<(f32, f32)>`
  - Purpose: Computes thumb position and length within the track.
  - Behavior: Returns `None` when `max_offset`, `viewport`, or `track` is
    non-positive, or when computed thumb would fill the entire track. Otherwise
    thumb height scales with viewport/content ratio, clamped to `MIN_THUMB`.
    Maps current scroll (clamped) to thumb top along remaining track slack.
    Returns `(top, thumb_length)`.

- **`offset_for_thumb_top`** (private, L148)
  - Signature: `fn offset_for_thumb_top(thumb_top: f32, track: f32, thumb: f32, max_offset: f32) -> f32`
  - Purpose: Inverse of thumb positioning — maps drag position to scroll offset.
  - Behavior: Computes fraction of draggable track (`thumb_top / (track - thumb)`,
    clamped 0–1) and returns negative scroll offset `-frac * max_offset`. Zero
    denominator yields fraction 0.

##### Context: `IntoElement for Scrollbar`

- **`into_element`** (private, L170)
  - Signature: `fn into_element(self) -> Self::Element`
  - Purpose: Identity conversion for GPUI element pipeline.
  - Behavior: Returns `self`.

##### Context: `Element for Scrollbar`

- **`id`** (private, L179)
  - Signature: `fn id(&self) -> Option<ElementId>`
  - Purpose: Element identity hook.
  - Behavior: Always `None` (anonymous overlay).

- **`source_location`** (private, L183)
  - Signature: `fn source_location(&self) -> Option<&'static Location<'static>>`
  - Purpose: Inspector source location hook.
  - Behavior: Always `None`.

- **`request_layout`** (private, L187)
  - Signature: `fn request_layout(&mut self, …, window: &mut Window, cx: &mut App) -> (LayoutId, ())`
  - Purpose: Requests absolute fill of parent container.
  - Behavior: Builds absolute-positioned style filling 100% width/height; returns
    layout id from `window.request_layout`.

- **`prepaint`** (private, L203)
  - Signature: `fn prepaint(&mut self, …, bounds: Bounds<Pixels>, …, window: &mut Window, …) -> Option<ScrollbarLayout>`
  - Purpose: Computes track/thumb geometry and hover hitbox for the current frame.
  - Behavior: Reads scroll handle bounds, max offset, and current offset along
    active axis. Calls `thumb_geometry`; returns `None` (no scrollbar) when
    content fits. Places track/thumb on bottom (horizontal) or right (vertical)
    edge with `WIDTH`/`PADDING`. Inserts wider `hover_hitbox` strip for reveal.
    Stores `max_offset` in layout.

- **`paint`** (private, L285)
  - Signature: `fn paint(&mut self, …, prepaint: &mut Option<ScrollbarLayout>, window: &mut Window, cx: &mut App)`
  - Purpose: Draws thumb, handles drag/hover/track-click, and manages visibility.
  - Behavior: No-op if prepaint was `None`. Sets arrow cursor over hover strip.
    Reads drag/hover/recent-scroll flags from `ScrollbarState`. Paints rounded
    thumb quad when hovered, dragging, or recently scrolled; uses hover thumb
    color while dragging. Registers mouse-down: thumb drag captures grab offset;
    track click jumps thumb center to click and starts drag. Mouse-move updates
    hover flag (repaints on boundary cross) and applies scroll offset while
    dragging. Mouse-up clears drag. All handlers stop propagation on active
    interaction and call `window.refresh()`.

##### Context: `module` (tests)

- **`no_thumb_when_content_fits`** (private, L436)
  - Signature: `fn no_thumb_when_content_fits()`
  - Purpose: Validates no thumb when there is nothing to scroll.
  - Behavior: Asserts `thumb_geometry` returns `None` for zero max offset and
    for zero-sized inputs.

- **`thumb_shrinks_with_more_content`** (private, L442)
  - Signature: `fn thumb_shrinks_with_more_content()`
  - Purpose: Validates proportional thumb sizing.
  - Behavior: For viewport 200 of 400 total content, expects thumb height 100 at
    top 0 on 200px track.

- **`thumb_respects_minimum_height`** (private, L450)
  - Signature: `fn thumb_respects_minimum_height()`
  - Purpose: Validates `MIN_THUMB` floor.
  - Behavior: With tiny viewport vs huge content, asserts thumb length ≥
    `MIN_THUMB`.

- **`thumb_moves_to_bottom_at_max_scroll`** (private, L457)
  - Signature: `fn thumb_moves_to_bottom_at_max_scroll()`
  - Purpose: Validates thumb reaches track end at max scroll.
  - Behavior: At max scroll, asserts thumb top equals `track - thumb_height`.

- **`scroll_marks_recently_scrolled`** (private, L464)
  - Signature: `fn scroll_marks_recently_scrolled()`
  - Purpose: Validates auto-hide timing flag.
  - Behavior: Fresh state not recent; after `note_scroll()` becomes recent.

- **`offset_round_trips_through_thumb_top`** (private, L472)
  - Signature: `fn offset_round_trips_through_thumb_top()`
  - Purpose: Validates inverse mapping for drag math.
  - Behavior: Thumb at bottom yields `-max_offset`; thumb at top yields `0`.

### `src/text_input.rs`

#### Types / constants

- **actions `text_input`**: GPUI actions — `Backspace`, `Delete`, `Left`, `Right`,
  `WordLeft`, `WordRight`, `SelectLeft`, `SelectRight`, `SelectWordLeft`,
  `SelectWordRight`, `SelectAll`, `Home`, `End`, `SelectHome`, `SelectEnd`,
  `Paste`, `Cut`, `Copy`. Bound via `bind_keys`.
- **struct `TextInput`**: Single-line editable field (`Render` + `Focusable` +
  `EntityInputHandler`). Tracks content, placeholder, UTF-8 selection/marked
  ranges, shaped line layout, bounds, focus, and mouse selection state.
- **struct `TextInputElement`** (private): GPUI `Element` wrapper painting the
  field and wiring platform input.
- **struct `PrepaintState`** (private): Frame prepaint output — shaped `line`,
  optional `cursor` quad, optional `selection` quad.

#### Functions / methods

##### Context: `module`

- **`set_native_text_focus`** (pub, L21 — Windows)
  - Signature: `pub fn set_native_text_focus(window: &Window)`
  - Purpose: Forces OS keyboard focus to the GPUI window on Windows.
  - Behavior: Resolves Win32 `HWND` from the window handle and calls
    `SetFocus`. No-op return paths when handle missing or non-Win32. Uses
    isolated `unsafe` with documented safety rationale.

- **`set_native_text_focus`** (pub, L44 — non-Windows)
  - Signature: `pub fn set_native_text_focus(_window: &Window)`
  - Purpose: Cross-platform stub for native focus helper.
  - Behavior: Empty body; Windows-specific focus transfer is unnecessary on
    other platforms.

- **`previous_word_start`** (private, L71)
  - Signature: `fn previous_word_start(content: &str, offset: usize) -> usize`
  - Purpose: Windows-style previous-word boundary for Ctrl+Left navigation.
  - Behavior: Returns `0` at start or when only whitespace precedes cursor.
    Trims trailing whitespace before cursor, finds preceding whitespace run,
    then first non-whitespace after it. Falls back to `0` if already at first
    word.

- **`next_word_start`** (private, L98)
  - Signature: `fn next_word_start(content: &str, offset: usize) -> usize`
  - Purpose: Windows-style next-word boundary for Ctrl+Right navigation.
  - Behavior: Returns `len` when at/ past end. Skips current word chars, then
    skips whitespace to next non-whitespace byte index (or `len`).

- **`bind_keys`** (pub, L119)
  - Signature: `pub fn bind_keys(cx: &mut App)`
  - Purpose: Registers all keyboard shortcuts for `TextInput` focus context.
  - Behavior: Binds editing/navigation keys under context `"TextInput"`. Includes
    platform-specific word and document navigation — Ctrl on Windows/Linux,
    Option/Command variants on macOS. Call once at app startup before views use
    `TextInput`.

##### Context: `TextInput`

- **`new`** (pub, L196)
  - Signature: `pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self`
  - Purpose: Creates an empty focused-capable text field.
  - Behavior: Allocates focus handle from context; empty content; placeholder
    stored; collapsed selection at 0; no marked text, layout, or selection drag.

- **`content`** (pub, L211)
  - Signature: `pub fn content(&self) -> SharedString`
  - Purpose: Reads current field text.
  - Behavior: Clones internal `content` string.

- **`clear`** (pub, L216)
  - Signature: `pub fn clear(&mut self, cx: &mut Context<Self>)`
  - Purpose: Empties content and resets selection/mark state.
  - Behavior: Sets content to default, selection to `0..0`, clears marked range,
    notifies subscribers.

- **`set_placeholder`** (pub, L224)
  - Signature: `pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>)`
  - Purpose: Updates empty-state hint text.
  - Behavior: Replaces `placeholder` without notifying (visual update on next
    frame).

- **`left`** (private, L228)
  - Signature: `fn left(&mut self, _: &Left, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret one grapheme left, or collapses selection to start.
  - Behavior: Empty selection: move to previous grapheme boundary. Non-empty:
    collapse to range start.

- **`right`** (private, L236)
  - Signature: `fn right(&mut self, _: &Right, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret one grapheme right, or collapses to end.
  - Behavior: Empty selection: advance to next grapheme boundary. Non-empty:
    collapse to range end.

- **`word_left`** (private, L244)
  - Signature: `fn word_left(&mut self, _: &WordLeft, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to previous word start (or collapses selection).
  - Behavior: Uses `previous_word_start` when selection empty; else collapse to
    start.

- **`word_right`** (private, L255)
  - Signature: `fn word_right(&mut self, _: &WordRight, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to next word start (or collapses selection).
  - Behavior: Uses `next_word_start` when selection empty; else collapse to end.

- **`select_left`** (private, L266)
  - Signature: `fn select_left(&mut self, _: &SelectLeft, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection one grapheme left.
  - Behavior: Calls `select_to(previous_boundary(cursor))`.

- **`select_right`** (private, L270)
  - Signature: `fn select_right(&mut self, _: &SelectRight, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection one grapheme right.
  - Behavior: Calls `select_to(next_boundary(cursor))`.

- **`select_word_left`** (private, L274)
  - Signature: `fn select_word_left(&mut self, _: &SelectWordLeft, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to previous word boundary.
  - Behavior: `select_to(previous_word_start(...))`.

- **`select_word_right`** (private, L281)
  - Signature: `fn select_word_right(&mut self, _: &SelectWordRight, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to next word boundary.
  - Behavior: `select_to(next_word_start(...))`.

- **`select_home`** (private, L288)
  - Signature: `fn select_home(&mut self, _: &SelectHome, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to start of field.
  - Behavior: `select_to(0)`.

- **`select_end`** (private, L292)
  - Signature: `fn select_end(&mut self, _: &SelectEnd, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to end of field.
  - Behavior: `select_to(content.len())`.

- **`select_all`** (private, L296)
  - Signature: `fn select_all(&mut self, _: &SelectAll, …, cx: &mut Context<Self>)`
  - Purpose: Selects entire content.
  - Behavior: Moves caret to 0 then selects through `content.len()`.

- **`home`** (private, L301)
  - Signature: `fn home(&mut self, _: &Home, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to start without extending selection.
  - Behavior: `move_to(0)`.

- **`end`** (private, L305)
  - Signature: `fn end(&mut self, _: &End, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to end without extending selection.
  - Behavior: `move_to(content.len())`.

- **`backspace`** (private, L309)
  - Signature: `fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Deletes selection or grapheme before caret.
  - Behavior: Collapsed selection: extend selection backward one grapheme, then
    replace selection with empty string via platform input path.

- **`delete`** (private, L316)
  - Signature: `fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Deletes selection or grapheme after caret.
  - Behavior: Collapsed selection: extend forward one grapheme, then delete via
    `replace_text_in_range`.

- **`on_mouse_down`** (private, L323)
  - Signature: `fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Focuses field and starts caret placement or selection drag.
  - Behavior: Calls `set_native_text_focus`, focuses entity handle, stops
    propagation. Sets `is_selecting`. Shift extends selection to click index;
    otherwise moves caret to click index.

- **`on_mouse_up`** (private, L340)
  - Signature: `fn on_mouse_up(&mut self, …, _: &mut Context<Self>)`
  - Purpose: Ends mouse-driven selection drag.
  - Behavior: Clears `is_selecting`.

- **`on_mouse_move`** (private, L344)
  - Signature: `fn on_mouse_move(&mut self, event: &MouseMoveEvent, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection while dragging with button held.
  - Behavior: When `is_selecting`, updates selection end to index under pointer.

- **`paste`** (private, L350)
  - Signature: `fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Inserts clipboard text at selection.
  - Behavior: Reads clipboard text if any, replaces newlines with spaces (keeps
    field single-line), inserts via `replace_text_in_range`.

- **`copy`** (private, L356)
  - Signature: `fn copy(&mut self, _: &Copy, …, cx: &mut Context<Self>)`
  - Purpose: Copies selected text to clipboard.
  - Behavior: No-op when selection empty; otherwise writes selected substring to
    clipboard.

- **`cut`** (private, L364)
  - Signature: `fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Copies selection then deletes it.
  - Behavior: When selection non-empty, copies to clipboard then replaces
    selection with empty string.

- **`move_to`** (private, L373)
  - Signature: `fn move_to(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Collapses caret to a UTF-8 byte offset.
  - Behavior: Sets `selected_range` to `offset..offset`, notifies.

- **`cursor_offset`** (private, L378)
  - Signature: `fn cursor_offset(&self) -> usize`
  - Purpose: Returns active caret byte index respecting selection direction.
  - Behavior: Returns range start when `selection_reversed`, else range end.

- **`index_for_mouse_position`** (private, L386)
  - Signature: `fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize`
  - Purpose: Maps pointer x/y to nearest text byte index.
  - Behavior: Returns `0` for empty content or missing layout/bounds. Above top →
    `0`; below bottom → `content.len()`. Otherwise uses shaped line
    `closest_index_for_x` relative to bounds left edge.

- **`select_to`** (private, L403)
  - Signature: `fn select_to(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Extends or shrinks selection toward `offset`.
  - Behavior: Updates start or end depending on `selection_reversed`. Normalizes
    inverted ranges by swapping endpoints and toggling reversed flag. Notifies.

- **`offset_from_utf16`** (private, L416)
  - Signature: `fn offset_from_utf16(&self, offset: usize) -> usize`
  - Purpose: Converts UTF-16 code unit index to UTF-8 byte offset.
  - Behavior: Iterates chars accumulating UTF-16 counts until reaching target.

- **`offset_to_utf16`** (private, L429)
  - Signature: `fn offset_to_utf16(&self, offset: usize) -> usize`
  - Purpose: Converts UTF-8 byte offset to UTF-16 code unit index.
  - Behavior: Iterates chars accumulating UTF-8 bytes until reaching target.

- **`range_to_utf16`** (private, L442)
  - Signature: `fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize>`
  - Purpose: Converts a UTF-8 byte range to UTF-16 for platform APIs.
  - Behavior: Maps start and end independently via `offset_to_utf16`.

- **`range_from_utf16`** (private, L446)
  - Signature: `fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize>`
  - Purpose: Converts platform UTF-16 range to internal UTF-8 bytes.
  - Behavior: Maps start and end via `offset_from_utf16`.

- **`previous_boundary`** (private, L450)
  - Signature: `fn previous_boundary(&self, offset: usize) -> usize`
  - Purpose: Previous grapheme cluster start before `offset`.
  - Behavior: Reverse scan of `grapheme_indices`; returns last index `< offset` or
    `0`.

- **`next_boundary`** (private, L458)
  - Signature: `fn next_boundary(&self, offset: usize) -> usize`
  - Purpose: Next grapheme cluster start after `offset`.
  - Behavior: Forward scan; first index `> offset` or `content.len()`.

##### Context: `EntityInputHandler for TextInput`

- **`text_for_range`** (private, L467)
  - Signature: `fn text_for_range(&mut self, range_utf16: Range<usize>, actual_range: &mut Option<Range<usize>>, …) -> Option<String>`
  - Purpose: Supplies substring for IME/platform queries.
  - Behavior: Converts UTF-16 range to UTF-8, stores actual UTF-16 range back,
    returns owned substring (always `Some`).

- **`selected_text_range`** (private, L479)
  - Signature: `fn selected_text_range(&mut self, …) -> Option<UTF16Selection>`
  - Purpose: Reports current selection to the platform.
  - Behavior: Always returns `Some` with UTF-16 range and reversed flag.

- **`marked_text_range`** (private, L491)
  - Signature: `fn marked_text_range(&self, …) -> Option<Range<usize>>`
  - Purpose: Reports IME marked (preedit) range in UTF-16.
  - Behavior: Maps internal marked UTF-8 range when present.

- **`unmark_text`** (private, L501)
  - Signature: `fn unmark_text(&mut self, …)`
  - Purpose: Clears IME marked text state.
  - Behavior: Sets `marked_range` to `None`.

- **`replace_text_in_range`** (private, L505)
  - Signature: `fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, …, cx: &mut Context<Self>)`
  - Purpose: Applies platform text replacement (typing, delete, paste).
  - Behavior: Target range is explicit UTF-16 range, else marked range, else
    current selection. Splices `new_text` into content, collapses caret after
    insertion, clears marked range, notifies.

- **`replace_and_mark_text_in_range`** (private, L526)
  - Signature: `fn replace_and_mark_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, new_selected_range_utf16: Option<Range<usize>>, …, cx: &mut Context<Self>)`
  - Purpose: IME composition insert with optional marked underline range.
  - Behavior: Same splice as replace; sets marked range to inserted span when
    non-empty. Adjusts selection from optional UTF-16 sub-range or collapses at
    end of insertion. Notifies.

- **`bounds_for_range`** (private, L556)
  - Signature: `fn bounds_for_range(&mut self, range_utf16: Range<usize>, bounds: Bounds<Pixels>, …) -> Option<Bounds<Pixels>>`
  - Purpose: Returns pixel rect for a UTF-16 text range (IME candidate window).
  - Behavior: Requires cached shaped line. Converts range to UTF-8, builds
    horizontal bounds from line x positions at field top/bottom.

- **`character_index_for_point`** (private, L577)
  - Signature: `fn character_index_for_point(&mut self, point: Point<Pixels>, …) -> Option<usize>`
  - Purpose: Maps screen point to UTF-16 character index for platform hit-testing.
  - Behavior: Localizes point against last bounds, uses line `index_for_x`, converts
    resulting UTF-8 index to UTF-16. Returns `None` without layout.

##### Context: `IntoElement for TextInputElement`

- **`into_element`** (private, L603)
  - Signature: `fn into_element(self) -> Self::Element`
  - Purpose: Identity element conversion.
  - Behavior: Returns `self`.

##### Context: `Element for TextInputElement`

- **`id`** (private, L612)
  - Signature: `fn id(&self) -> Option<ElementId>`
  - Purpose: Element identity hook.
  - Behavior: Always `None`.

- **`source_location`** (private, L616)
  - Signature: `fn source_location(&self) -> Option<&'static Location<'static>>`
  - Purpose: Inspector source hook.
  - Behavior: Always `None`.

- **`request_layout`** (private, L620)
  - Signature: `fn request_layout(&mut self, …, window: &mut Window, cx: &mut App) -> (LayoutId, ())`
  - Purpose: Sizes field to full width and one line height.
  - Behavior: Style width `relative(1.)`, height `window.line_height()`.

- **`prepaint`** (private, L633)
  - Signature: `fn prepaint(&mut self, …, bounds: Bounds<Pixels>, …, window: &mut Window, cx: &mut App) -> PrepaintState`
  - Purpose: Shapes display text and prepares selection/caret quads.
  - Behavior: Reads input entity. Empty content shows placeholder in muted color;
    else uses window text style. Builds `TextRun`(s) with optional wavy underline
    on marked range. Shapes line. Collapsed selection → accent caret quad inset
    vertically; non-empty → selected background quad spanning glyph bounds.

- **`paint`** (private, L734)
  - Signature: `fn paint(&mut self, …, bounds: Bounds<Pixels>, prepaint: &mut PrepaintState, window: &mut Window, cx: &mut App)`
  - Purpose: Wires platform input handler and paints selection, text, caret.
  - Behavior: Registers `ElementInputHandler` for focus handle. Paints selection
    quad if any. Paints shaped line at bounds origin. When focused, paints caret
    quad. Updates entity `last_layout` and `last_bounds` for hit-testing.

##### Context: `Render for TextInput`

- **`render`** (private, L771)
  - Signature: `fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Builds interactive wrapper around `TextInputElement`.
  - Behavior: Flex child with `key_context("TextInput")`, focus tracking, I-beam
    cursor, all action listeners (navigation, selection, clipboard, editing), and
    mouse down/up/move handlers. Embeds `TextInputElement` with entity reference.

##### Context: `Focusable for TextInput`

- **`focus_handle`** (private, L806)
  - Signature: `fn focus_handle(&self, _: &App) -> FocusHandle`
  - Purpose: Exposes GPUI focus handle for tab/focus management.
  - Behavior: Clones internal `focus_handle`.

##### Context: `module` (tests)

- **`previous_word_start_moves_to_prior_word`** (private, L816)
  - Signature: `fn previous_word_start_moves_to_prior_word()`
  - Purpose: Validates word-left helper on two-word string.
  - Behavior: From end → index 6 (`"world"` start); from 6 or mid-first-word → 0.

- **`next_word_start_moves_to_following_word`** (private, L824)
  - Signature: `fn next_word_start_moves_to_following_word()`
  - Purpose: Validates word-right helper on two-word string.
  - Behavior: From 0 or 3 → 6; from 6 → string length.
