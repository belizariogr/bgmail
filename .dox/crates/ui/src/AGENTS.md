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

_(none — modules are files, including the `text_area/` submodules; their APIs are cataloged below.)_

## API Catalog

_300 functions/methods documented._

### `src/ui.rs`

#### Types / constants

- **module `ui`**: Crate root. Declares private submodules (plus the public
  `text_area` module, whose actions stay namespaced as `ui::text_area::*` to
  avoid clashing with `TextInput`'s glob-exported actions), re-exports their
  public API (`text_area` only `TextArea` + `TextAreaEvent`), re-exports
  `prelude::{h_flex, v_flex}`, and re-exports
  `theme::{ActiveTheme, Appearance, Theme, ThemeColors}` for consumer convenience.

#### Functions / methods

##### Context: `module`

- **`bind_keys`** (pub, L46)
  - Signature: `pub fn bind_keys(cx: &mut gpui::App)`
  - Purpose: Single startup entry point for text-component key bindings.
  - Behavior: Calls `text_input::bind_keys` then `text_area::bind_keys`. Shadows
    the glob-exported `text_input::bind_keys`, so existing `ui::bind_keys(cx)`
    callers now also get `TextArea` bindings.

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
  — `CHEVRON_RIGHT`, `CHEVRON_DOWN`, `CHEVRON_UP`, `INBOX`, `SEND`, `FILE_PEN`, `TRASH`,
  `TRIANGLE_EXCLAMATION`, `ARCHIVE`, `STAR`, `STAR_FILLED`, `PALETTE`, `FLAG`,
  `REPLY`, `REPLY_ALL`, `FORWARD`, `PEN_TO_SQUARE`, `SEARCH`, `SETTINGS`,
  `REFRESH`, `CIRCLE_USER`, `PAPERCLIP`, `ENVELOPE`, `SIDEBAR`, `FILTER`,
  `ELLIPSIS`, `FOLDER`, `SHIELD_HALVED`, `SHIELD`, `CHECK`, `XMARK`,
  `WINDOW_MINIMIZE`, `WINDOW_MAXIMIZE`, `WINDOW_RESTORE`, and the attachment
  file icons `FILE`, `FILE_IMAGE`, `FILE_PDF`, `FILE_ZIPPER`, `FILE_AUDIO`,
  `FILE_VIDEO`, `FILE_LINES`. Each value is the
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
  (`Compose`, `Send`, `Search`, `ChevronRight`, `ChevronDown`, `ChevronUp`, …), chrome
  (`Sidebar`, `Settings`, `WindowMinimize`, `WindowMaximize`, `WindowRestore`,
  …), attachment families (`File`, `FileImage`, `FilePdf`, `FileArchive`,
  `FileAudio`, `FileVideo`, `FileText`). `Sent` and `Send` share the paper-plane artwork but are distinct enum
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

- **enum `LabelSize`**: Text size presets relative to `UI_FONT_SIZE` —
  `XSmall` (−3px), `Small` (−2px), `Default` (base), `Large` (+2px).
  Default variant is `Default`.
- **struct `Label`**: Themed single-line or multi-line text (`IntoElement`).
  Fields: `text`, `size`, `color`, `weight`, `single_line`.

#### Functions / methods

##### Context: `LabelSize`

- **`px`** (private, L20)
  - Signature: `fn px(self) -> Pixels`
  - Purpose: Converts a label size preset to pixel `text_size`.
  - Behavior: `ui_text_size(-3 | -2 | 0 | +2)`.

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

### `src/context_menu.rs`

#### Types / constants

- **type `EntryHandler`** (private): boxed click handler of an entry.
- **enum `ContextMenuItem`** (pub): `Entry { label, key_hint, handler }` or
  `Separator`.
- **struct `ContextMenu`** (pub, `IntoElement` + `Default`): floating menu of
  actions named after Zed's `ContextMenu`; the host positions it (e.g.
  `deferred(anchored().position(..))`) and closes it.

#### Functions / methods

- **`ContextMenu::new`**: empty menu.
- **`ContextMenu::entry(label, handler)`**: appends an entry run on click.
- **`ContextMenu::entry_with_key_hint(label, key_hint, handler)`**: entry with
  a muted shortcut hint at its end.
- **`ContextMenu::separator`**: appends a separator only after an entry (no
  leading/double separators).
- **`ContextMenu::items`** / **`ContextMenu::is_empty`**: rows; whether it has
  no entries.
- **`RenderOnce for ContextMenu`**: occluding elevated surface (min 200px,
  border, rounded, shadow); rows with hover background, small label and hint;
  a trailing separator is dropped.
- Tests: separators only follow entries; empty menus.

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
    padding, rounded corners, `ui_text_size(-1)` text. Optional icon uses `OnAccent` tint
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

- **`disabled`** (pub)
  - Signature: `pub fn disabled(mut self, disabled: bool) -> Self`
  - Purpose: Inert state (as Zed's `Disableable`), e.g. message actions with no selection.
  - Behavior: Render uses `Color::Disabled`, no hover/selected background, drops the click handler, but still swallows left mouse-down (no title-bar drag).

- **`prompt`**: `init` (pub); `themed_prompt`, `action_for_key`, `is_cancel`, `ThemedPrompt` (private).
- **`TextArea::press_outside`** (pub): a press on the page around the text, as one on it.
- **`font_for_family`** (pub, `text_area`) / **`ContextMenu::in_font`** (pub) / **`FontFamily::ALL`**: previewing the fonts in a menu.
- **`text_area::rich`** (GPUI-free): `RichText` (`new`, `matches`, `is_plain`, `apply_edit`, `style_at`, `para`, `runs_in`, `all_chars`, `update_chars`, `update_paras`, `list_number`, `list_marker`, `to_plain`, `to_html`), `CharStyle`, `ParaStyle`, `Format`, `FontFamily`, `TextSize` (`scale`), `Align`, `ListKind`, `MAX_INDENT`.
- **`TextArea` formatting** (pub): `format`, `formatted_html`, `plain_export`, `can_undo`, `can_redo`, `toggle_bold`, `toggle_italic`, `toggle_underline`, `set_font_family`, `set_text_color`, `set_text_size`, `set_align`, `toggle_list`, `indent`, `undo_step`, `redo_step`, `set_selection_color`. Actions `ToggleBold`, `ToggleItalic`, `ToggleUnderline` (Cmd/Ctrl+B, I, U).
- **`Families`** / **`styled_run`** / **`line_runs`** / **`line_left`** / **`LineGeometry`** (private, `text_area`): fonts with real bold faces, runs of a line, and where each line goes.
- **`TextBuffer::undo_steps`** / **`can_undo`** / **`can_redo`** / **`add_undo_step`** (pub(crate)): history counters and an undo step without a text change.
- **`IconButton::hover_color`** (pub): brightens the icon on hover instead of showing a background.
- **`IconButton::compact`** (pub) / **`side`** (private): makes the button its icon's size / side of the button's square.
- **`Icon::group_hover_color`** (pub): icon color while the named group is hovered.
- **`click_selection`** (private, `text_input`): range selected by a repeated click (word, or the whole field when masked or on a triple click).
- **`Scrollbar::always_visible`** / **`Scrollbar::thumb_colors`** (pub): keep the thumb on screen whenever content overflows / override its theme colors.
- **`effective_color`** (private)
  - Signature: `fn effective_color(&self) -> Color`
  - Purpose: Icon color after states.
  - Behavior: `Disabled` when disabled, else `Accent` when selected, else the configured color. Tested by `icon_button_color_follows_disabled_then_selected`.

##### Context: `RenderOnce for IconButton`

- **`render`** (private, L166)
  - Signature: `fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement`
  - Purpose: Paints square icon button with hover/selected chrome.
  - Behavior: 28px rounded flex container. Selected state gets active
    background; enabled buttons get hover background. Renders `Icon` at
    `effective_color`. Optional click wiring mirrors `Button` (skipped when
    disabled).

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

### `src/text_size.rs`

- **const `UI_FONT_SIZE`** (pub): base UI text size, 15px; every UI text size
  derives from it so the interface scales as a whole.
- **`ui_text_size(offset)`** (pub): `px(UI_FONT_SIZE + offset)`.
- Tests: sizes follow the base.

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

- **actions `ui`**: `FocusNext`, `FocusPrevious` — Tab navigation between text
  fields. `TextInput` and `TextArea` focus handles are tab stops; order follows
  paint order and wraps. Handled by `focus_next_field` / `focus_previous_field`
  (pub(crate)), which also call `set_native_text_focus`. A view can veto them with
  `capture_action` (the command palette does). Tests: `focus_tests` (walk + wrap,
  Tab leaves the text untouched).
- **actions `text_input`**: GPUI actions — `Backspace`, `Delete`, `Left`, `Right`,
  `WordLeft`, `WordRight`, `SelectLeft`, `SelectRight`, `SelectWordLeft`,
  `SelectWordRight`, `SelectAll`, `Home`, `End`, `SelectHome`, `SelectEnd`,
  `Paste`, `Cut`, `Copy`. Bound via `bind_keys`.
- **enum `TextInputEvent`**: `Changed` — emitted (via `EventEmitter`) when the
  user changes the text through the input handler (typing, IME, paste, cut,
  delete). Programmatic `clear` / `set_content` do not emit it.
- **struct `TextInput`**: Single-line editable field (`Render` + `Focusable` +
  `EntityInputHandler` + `EventEmitter<TextInputEvent>`). Tracks content, placeholder, UTF-8 selection/marked
  ranges, shaped line layout, bounds, focus, mouse selection state, and
  `masked` (password field).
- **const `MASK_CHAR`** (private): `•`, drawn once per character of a masked field.
- **struct `TextInputElement`** (private): GPUI `Element` wrapper painting the
  field and wiring platform input.
- **struct `PrepaintState`** (private): Frame prepaint output — shaped `line`,
  optional `cursor` quad, optional `selection` quad.

#### Functions / methods

##### Context: `module`

- **`set_native_text_focus`** (pub, L22 — Windows)
  - Signature: `pub fn set_native_text_focus(window: &Window)`
  - Purpose: Forces OS keyboard focus to the GPUI window on Windows.
  - Behavior: Resolves Win32 `HWND` from the window handle and calls
    `SetFocus`. No-op return paths when handle missing or non-Win32. Uses
    isolated `unsafe` with documented safety rationale.

- **`set_native_text_focus`** (pub, L45 — non-Windows)
  - Signature: `pub fn set_native_text_focus(_window: &Window)`
  - Purpose: Cross-platform stub for native focus helper.
  - Behavior: Empty body; Windows-specific focus transfer is unnecessary on
    other platforms.

- **`previous_word_start`** (pub(crate), L72)
  - Signature: `fn previous_word_start(content: &str, offset: usize) -> usize`
  - Purpose: Windows-style previous-word boundary for Ctrl+Left navigation.
  - Behavior: Returns `0` at start or when only whitespace precedes cursor.
    Trims trailing whitespace before cursor, finds preceding whitespace run,
    then first non-whitespace after it. Falls back to `0` if already at first
    word. Reused by `TextArea` word navigation (newlines count as whitespace).

- **`next_word_start`** (pub(crate), L99)
  - Signature: `fn next_word_start(content: &str, offset: usize) -> usize`
  - Purpose: Windows-style next-word boundary for Ctrl+Right navigation.
  - Behavior: Returns `len` when at/ past end. Skips current word chars, then
    skips whitespace to next non-whitespace byte index (or `len`).

- **`single_line`** (pub(crate), L120)
  - Signature: `fn single_line(text: &str) -> String`
  - Purpose: Folds `\r\n` / `\r` / `\n` into single spaces for the one-line field.
  - Behavior: Used by `TextInput::set_content`; covered by
    `single_line_folds_line_breaks_into_spaces`.

- **`masked_text`** / **`masked_offset`** / **`unmasked_offset`** (pub(crate))
  - Signatures: `fn masked_text(content: &str) -> String`, `fn masked_offset(content: &str, offset: usize) -> usize`, `fn unmasked_offset(content: &str, display_offset: usize) -> usize`
  - Purpose: Display text of a masked field and the mapping between content
    byte offsets and offsets in the drawn bullets.
  - Behavior: One `MASK_CHAR` per `char`; `masked_offset` = chars before the
    offset × 3 bytes; `unmasked_offset` rounds down to the char boundary
    (past the end → `content.len()`). Covered by
    `masking_draws_one_bullet_per_character_and_maps_offsets`.

- **`bind_keys`** (pub, L120)
  - Signature: `pub fn bind_keys(cx: &mut App)`
  - Purpose: Registers all keyboard shortcuts for `TextInput` focus context.
  - Behavior: Binds Tab/Shift+Tab (`FocusNext`/`FocusPrevious`) and editing/navigation keys under context `"TextInput"`. Includes
    platform-specific word and document navigation — Ctrl on Windows/Linux,
    Option/Command variants on macOS. Call once at app startup before views use
    `TextInput`.

##### Context: `TextInput`

- **`new`** (pub, L207)
  - Signature: `pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self`
  - Purpose: Creates an empty focused-capable text field.
  - Behavior: Allocates focus handle from context; empty content; placeholder
    stored; collapsed selection at 0; no marked text, layout, or selection drag.

- **`content`** (pub, L222)
  - Signature: `pub fn content(&self) -> SharedString`
  - Purpose: Reads current field text.
  - Behavior: Clones internal `content` string.

- **`clear`** (pub, L227)
  - Signature: `pub fn clear(&mut self, cx: &mut Context<Self>)`
  - Purpose: Empties content and resets selection/mark state.
  - Behavior: Sets content to default, selection to `0..0`, clears marked range,
    notifies subscribers.

- **`set_content`** (pub, L240)
  - Signature: `pub fn set_content(&mut self, text: impl AsRef<str>, cx: &mut Context<Self>)`
  - Purpose: Prefills the field (compose To/Cc/Bcc/Subject from a reply draft).
  - Behavior: Stores `single_line(text)`, puts the caret at the end, clears the
    marked range and notifies. Like `clear`, does not emit `Changed`.

- **`set_masked`** (pub)
  - Signature: `pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>)`
  - Purpose: Password mode (Settings ▸ Accounts password fields).
  - Behavior: Draws bullets instead of the text and makes copy/cut no-ops;
    editing, selection and IME work as usual through `display_offset` /
    `content_offset`. Notifies.

- **`display_offset`** / **`content_offset`** (private)
  - Purpose: Map content byte offsets to/from the drawn line (identity unless
    masked). Used by caret/selection painting, mouse hit-testing and the IME
    `bounds_for_range` / `character_index_for_point`.

- **`placeholder`** (pub) — `pub fn placeholder(&self) -> &SharedString`: the current placeholder.

- **`set_placeholder`** (pub, L235)
  - Signature: `pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>)`
  - Purpose: Updates empty-state hint text.
  - Behavior: Replaces `placeholder` without notifying (visual update on next
    frame).

- **`left`** (private, L239)
  - Signature: `fn left(&mut self, _: &Left, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret one grapheme left, or collapses selection to start.
  - Behavior: Empty selection: move to previous grapheme boundary. Non-empty:
    collapse to range start.

- **`right`** (private, L247)
  - Signature: `fn right(&mut self, _: &Right, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret one grapheme right, or collapses to end.
  - Behavior: Empty selection: advance to next grapheme boundary. Non-empty:
    collapse to range end.

- **`word_left`** (private, L255)
  - Signature: `fn word_left(&mut self, _: &WordLeft, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to previous word start (or collapses selection).
  - Behavior: Uses `previous_word_start` when selection empty; else collapse to
    start.

- **`word_right`** (private, L266)
  - Signature: `fn word_right(&mut self, _: &WordRight, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to next word start (or collapses selection).
  - Behavior: Uses `next_word_start` when selection empty; else collapse to end.

- **`select_left`** (private, L277)
  - Signature: `fn select_left(&mut self, _: &SelectLeft, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection one grapheme left.
  - Behavior: Calls `select_to(previous_boundary(cursor))`.

- **`select_right`** (private, L281)
  - Signature: `fn select_right(&mut self, _: &SelectRight, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection one grapheme right.
  - Behavior: Calls `select_to(next_boundary(cursor))`.

- **`select_word_left`** (private, L285)
  - Signature: `fn select_word_left(&mut self, _: &SelectWordLeft, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to previous word boundary.
  - Behavior: `select_to(previous_word_start(...))`.

- **`select_word_right`** (private, L292)
  - Signature: `fn select_word_right(&mut self, _: &SelectWordRight, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to next word boundary.
  - Behavior: `select_to(next_word_start(...))`.

- **`select_home`** (private, L299)
  - Signature: `fn select_home(&mut self, _: &SelectHome, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to start of field.
  - Behavior: `select_to(0)`.

- **`select_end`** (private, L303)
  - Signature: `fn select_end(&mut self, _: &SelectEnd, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection to end of field.
  - Behavior: `select_to(content.len())`.

- **`select_all`** (private, L307)
  - Signature: `fn select_all(&mut self, _: &SelectAll, …, cx: &mut Context<Self>)`
  - Purpose: Selects entire content.
  - Behavior: Moves caret to 0 then selects through `content.len()`.

- **`home`** (private, L312)
  - Signature: `fn home(&mut self, _: &Home, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to start without extending selection.
  - Behavior: `move_to(0)`.

- **`end`** (private, L316)
  - Signature: `fn end(&mut self, _: &End, …, cx: &mut Context<Self>)`
  - Purpose: Moves caret to end without extending selection.
  - Behavior: `move_to(content.len())`.

- **`backspace`** (private, L320)
  - Signature: `fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Deletes selection or grapheme before caret.
  - Behavior: Collapsed selection: extend selection backward one grapheme, then
    replace selection with empty string via platform input path.

- **`delete`** (private, L327)
  - Signature: `fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Deletes selection or grapheme after caret.
  - Behavior: Collapsed selection: extend forward one grapheme, then delete via
    `replace_text_in_range`.

- **`on_mouse_down`** (private, L334)
  - Signature: `fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Focuses field and starts caret placement or selection drag.
  - Behavior: Calls `set_native_text_focus`, focuses entity handle, stops
    propagation. Sets `is_selecting`. Shift extends selection to click index;
    otherwise moves caret to click index.

- **`on_mouse_up`** (private, L351)
  - Signature: `fn on_mouse_up(&mut self, …, _: &mut Context<Self>)`
  - Purpose: Ends mouse-driven selection drag.
  - Behavior: Clears `is_selecting`.

- **`on_mouse_move`** (private, L355)
  - Signature: `fn on_mouse_move(&mut self, event: &MouseMoveEvent, …, cx: &mut Context<Self>)`
  - Purpose: Extends selection while dragging with button held.
  - Behavior: When `is_selecting`, updates selection end to index under pointer.

- **`paste`** (private, L361)
  - Signature: `fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Inserts clipboard text at selection.
  - Behavior: Reads clipboard text if any, replaces newlines with spaces (keeps
    field single-line), inserts via `replace_text_in_range`.

- **`copy`** (private, L367)
  - Signature: `fn copy(&mut self, _: &Copy, …, cx: &mut Context<Self>)`
  - Purpose: Copies selected text to clipboard.
  - Behavior: No-op when selection empty or the field is masked; otherwise
    writes selected substring to clipboard.

- **`cut`** (private, L375)
  - Signature: `fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Copies selection then deletes it.
  - Behavior: When selection non-empty and the field isn't masked, copies to
    clipboard then replaces selection with empty string.

- **`move_to`** (private, L384)
  - Signature: `fn move_to(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Collapses caret to a UTF-8 byte offset.
  - Behavior: Sets `selected_range` to `offset..offset`, notifies.

- **`cursor_offset`** (private, L389)
  - Signature: `fn cursor_offset(&self) -> usize`
  - Purpose: Returns active caret byte index respecting selection direction.
  - Behavior: Returns range start when `selection_reversed`, else range end.

- **`index_for_mouse_position`** (private, L397)
  - Signature: `fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize`
  - Purpose: Maps pointer x/y to nearest text byte index.
  - Behavior: Returns `0` for empty content or missing layout/bounds. Above top →
    `0`; below bottom → `content.len()`. Otherwise uses shaped line
    `closest_index_for_x` relative to bounds left edge, mapped back through
    `content_offset` (masked fields).

- **`select_to`** (private, L414)
  - Signature: `fn select_to(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Extends or shrinks selection toward `offset`.
  - Behavior: Updates start or end depending on `selection_reversed`. Normalizes
    inverted ranges by swapping endpoints and toggling reversed flag. Notifies.

- **`offset_from_utf16`** (private, L427)
  - Signature: `fn offset_from_utf16(&self, offset: usize) -> usize`
  - Purpose: Converts UTF-16 code unit index to UTF-8 byte offset.
  - Behavior: Iterates chars accumulating UTF-16 counts until reaching target.

- **`offset_to_utf16`** (private, L440)
  - Signature: `fn offset_to_utf16(&self, offset: usize) -> usize`
  - Purpose: Converts UTF-8 byte offset to UTF-16 code unit index.
  - Behavior: Iterates chars accumulating UTF-8 bytes until reaching target.

- **`range_to_utf16`** (private, L453)
  - Signature: `fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize>`
  - Purpose: Converts a UTF-8 byte range to UTF-16 for platform APIs.
  - Behavior: Maps start and end independently via `offset_to_utf16`.

- **`range_from_utf16`** (private, L457)
  - Signature: `fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize>`
  - Purpose: Converts platform UTF-16 range to internal UTF-8 bytes.
  - Behavior: Maps start and end via `offset_from_utf16`.

- **`previous_boundary`** (private, L461)
  - Signature: `fn previous_boundary(&self, offset: usize) -> usize`
  - Purpose: Previous grapheme cluster start before `offset`.
  - Behavior: Reverse scan of `grapheme_indices`; returns last index `< offset` or
    `0`.

- **`next_boundary`** (private, L469)
  - Signature: `fn next_boundary(&self, offset: usize) -> usize`
  - Purpose: Next grapheme cluster start after `offset`.
  - Behavior: Forward scan; first index `> offset` or `content.len()`.

##### Context: `EntityInputHandler for TextInput`

- **`text_for_range`** (private, L478)
  - Signature: `fn text_for_range(&mut self, range_utf16: Range<usize>, actual_range: &mut Option<Range<usize>>, …) -> Option<String>`
  - Purpose: Supplies substring for IME/platform queries.
  - Behavior: Converts UTF-16 range to UTF-8, stores actual UTF-16 range back,
    returns owned substring (always `Some`).

- **`selected_text_range`** (private, L490)
  - Signature: `fn selected_text_range(&mut self, …) -> Option<UTF16Selection>`
  - Purpose: Reports current selection to the platform.
  - Behavior: Always returns `Some` with UTF-16 range and reversed flag.

- **`marked_text_range`** (private, L502)
  - Signature: `fn marked_text_range(&self, …) -> Option<Range<usize>>`
  - Purpose: Reports IME marked (preedit) range in UTF-16.
  - Behavior: Maps internal marked UTF-8 range when present.

- **`unmark_text`** (private, L512)
  - Signature: `fn unmark_text(&mut self, …)`
  - Purpose: Clears IME marked text state.
  - Behavior: Sets `marked_range` to `None`.

- **`replace_text_in_range`** (private, L516)
  - Signature: `fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, …, cx: &mut Context<Self>)`
  - Purpose: Applies platform text replacement (typing, delete, paste).
  - Behavior: Target range is explicit UTF-16 range, else marked range, else
    current selection. Splices `new_text` into content, collapses caret after
    insertion, clears marked range, emits `TextInputEvent::Changed` unless the
    edit was a no-op (empty range + empty text), notifies.

- **`replace_and_mark_text_in_range`** (private, L540)
  - Signature: `fn replace_and_mark_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, new_selected_range_utf16: Option<Range<usize>>, …, cx: &mut Context<Self>)`
  - Purpose: IME composition insert with optional marked underline range.
  - Behavior: Same splice as replace; sets marked range to inserted span when
    non-empty. Adjusts selection from optional UTF-16 sub-range or collapses at
    end of insertion. Emits `TextInputEvent::Changed` unless a no-op. Notifies.

- **`bounds_for_range`** (private, L573)
  - Signature: `fn bounds_for_range(&mut self, range_utf16: Range<usize>, bounds: Bounds<Pixels>, …) -> Option<Bounds<Pixels>>`
  - Purpose: Returns pixel rect for a UTF-16 text range (IME candidate window).
  - Behavior: Requires cached shaped line. Converts range to UTF-8, builds
    horizontal bounds from line x positions at field top/bottom.

- **`character_index_for_point`** (private, L594)
  - Signature: `fn character_index_for_point(&mut self, point: Point<Pixels>, …) -> Option<usize>`
  - Purpose: Maps screen point to UTF-16 character index for platform hit-testing.
  - Behavior: Localizes point against last bounds, uses line `index_for_x`, converts
    resulting UTF-8 index to UTF-16. Returns `None` without layout.

##### Context: `IntoElement for TextInputElement`

- **`into_element`** (private, L620)
  - Signature: `fn into_element(self) -> Self::Element`
  - Purpose: Identity element conversion.
  - Behavior: Returns `self`.

##### Context: `Element for TextInputElement`

- **`id`** (private, L629)
  - Signature: `fn id(&self) -> Option<ElementId>`
  - Purpose: Element identity hook.
  - Behavior: Always `None`.

- **`source_location`** (private, L633)
  - Signature: `fn source_location(&self) -> Option<&'static Location<'static>>`
  - Purpose: Inspector source hook.
  - Behavior: Always `None`.

- **`request_layout`** (private, L637)
  - Signature: `fn request_layout(&mut self, …, window: &mut Window, cx: &mut App) -> (LayoutId, ())`
  - Purpose: Sizes field to full width and one line height.
  - Behavior: Style width `relative(1.)`, height `window.line_height()`.

- **`prepaint`** (private, L650)
  - Signature: `fn prepaint(&mut self, …, bounds: Bounds<Pixels>, …, window: &mut Window, cx: &mut App) -> PrepaintState`
  - Purpose: Shapes display text and prepares selection/caret quads.
  - Behavior: Reads input entity. Empty content shows placeholder in muted color;
    else uses window text style (masked: `masked_text`, with caret, selection
    and marked range mapped through `display_offset`). Builds `TextRun`(s) via
    `text_area::marked_text_runs` (solid underline on the marked range). Shapes line. Collapsed selection → accent caret quad inset
    vertically; non-empty → selected background quad spanning glyph bounds.

- **`paint`** (private, L726)
  - Signature: `fn paint(&mut self, …, bounds: Bounds<Pixels>, prepaint: &mut PrepaintState, window: &mut Window, cx: &mut App)`
  - Purpose: Wires platform input handler and paints selection, text, caret.
  - Behavior: Registers `ElementInputHandler` for focus handle. Paints selection
    quad if any. Paints shaped line at bounds origin. When focused, paints caret
    quad. Updates entity `last_layout` and `last_bounds` for hit-testing.

##### Context: `Render for TextInput`

- **`render`** (private, L763)
  - Signature: `fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Builds interactive wrapper around `TextInputElement`.
  - Behavior: Flex child with `key_context("TextInput")`, focus tracking, I-beam
    cursor, all action listeners (navigation, selection, clipboard, editing), and
    mouse down/up/move handlers. Embeds `TextInputElement` with entity reference.

##### Context: `Focusable for TextInput`

- **`focus_handle`** (private, L798)
  - Signature: `fn focus_handle(&self, _: &App) -> FocusHandle`
  - Purpose: Exposes GPUI focus handle for tab/focus management.
  - Behavior: Clones internal `focus_handle`.

##### Context: `module` (tests)

- **`previous_word_start_moves_to_prior_word`** (private, L808)
  - Signature: `fn previous_word_start_moves_to_prior_word()`
  - Purpose: Validates word-left helper on two-word string.
  - Behavior: From end → index 6 (`"world"` start); from 6 or mid-first-word → 0.

- **`next_word_start_moves_to_following_word`** (private, L816)
  - Signature: `fn next_word_start_moves_to_following_word()`
  - Purpose: Validates word-right helper on two-word string.
  - Behavior: From 0 or 3 → 6; from 6 → string length.

### `src/text_area.rs`

Multi-line plain-text editor (compose body). GPUI layer only; the pure editing
core lives in `text_area/buffer.rs` and `text_area/rows.rs` (below).

#### Types / constants

- **const `KEY_CONTEXT`** (private): `"TextArea"` — key context of the wrapper div.
- **actions `text_area`** (pub, namespaced as `ui::text_area::*`): `Backspace`,
  `Delete`, `Left`, `Right`, `Up`, `Down`, `WordLeft`, `WordRight`, `Home`,
  `End`, `MoveToBeginning`, `MoveToEnd`, `SelectLeft`, `SelectRight`,
  `SelectUp`, `SelectDown`, `SelectWordLeft`, `SelectWordRight`, `SelectHome`,
  `SelectEnd`, `SelectToBeginning`, `SelectToEnd`, `SelectAll`, `Newline`,
  `Copy`, `Cut`, `Paste`, `Undo`, `Redo`. Bound via `bind_keys`.
- **enum `TextAreaEvent`** (pub): `Changed` — user edits only (typing, IME,
  newline, paste, cut, delete, undo/redo that changed text). Programmatic
  `set_content` / `clear` do not emit it.
- **struct `LayoutKey`** (private): Everything affecting shaping — buffer
  `revision`, shown `placeholder` (`Some` when empty), IME `marked` range,
  `wrap_width`, `font`, `font_size`, `line_height`, text `color`. Equal key ⇒
  cached layout reused (idle frames and caret moves never reshape).
- **struct `TextLayout`** (private): Shared (`Rc<RefCell<_>>`) between the
  entity and its element — `key`, shaped `lines: Vec<WrappedLine>` (one per
  logical line), `rows: Vec<VisualRow>`, `max_width`, and
  `measured_without_width` (measure pass had no definite width).
- **struct `TextArea`** (pub): Entity (`Render` + `Focusable` +
  `EntityInputHandler` + `EventEmitter<TextAreaEvent>`). Holds focus handle,
  `TextBuffer`, placeholder, `min_lines`, shared layout, last bounds, mouse
  drag flag, `goal_x` (preferred column for Up/Down), optional tracked
  `ScrollHandle`, and `autoscroll_requested`.
- **struct `TextAreaElement`** (private): Custom element: measured layout,
  selection/caret/text painting, platform input registration.
- **struct `PrepaintState`** (private): Visible selection quads + optional caret quad.

#### Functions / methods

##### Context: `module`

- **`bind_keys`** (pub, L94)
  - Signature: `pub fn bind_keys(cx: &mut App)`
  - Purpose: Registers `TextArea` bindings under context `"TextArea"`.
  - Behavior: Common: tab/shift-tab → `FocusNext`/`FocusPrevious` (never inserts a tab), backspace/shift-backspace, delete, arrows (+shift),
    home/end (+shift, visual row), enter/shift-enter → `Newline`, cmd-a, cmd/ctrl
    c/x/v, cmd-z / cmd-shift-z. Non-macOS: ctrl-a, ctrl-z, ctrl-y /
    ctrl-shift-z (redo), ctrl-left/right (+shift) words, ctrl-home/end (+shift)
    document. macOS: alt-left/right (+shift) words, cmd-left/right and
    ctrl-a/e (+shift) row edges, cmd-up/down and cmd-home/end (+shift)
    document. Tab is unbound on purpose (parent focus navigation). Called by
    `ui::bind_keys`.

- **`marked_text_runs`** (pub(crate), L171)
  - Signature: `pub(crate) fn marked_text_runs(len: usize, marked: Option<&Range<usize>>, run: TextRun) -> Vec<TextRun>`
  - Purpose: Builds text runs with the IME marked range underlined (shared with `TextInput`).
  - Behavior: No mark (or a mark past `len`) → single run of `len`. Otherwise
    before/marked/after runs (solid 1px underline in the run color on the
    marked one), dropping zero-length runs.

- **`key_font_size`** (private, L1116)
  - Signature: `fn key_font_size(layout: &TextLayout) -> Pixels`
  - Purpose: Font size of the last layout (caret height, newline sliver).
  - Behavior: `key.font_size`, or 14px before the first layout.

- **`scroll_into_view`** (private, L1122)
  - Signature: `fn scroll_into_view(handle: &ScrollHandle, target: Bounds<Pixels>, margin: Pixels, window: &Window)`
  - Purpose: Keeps the caret visible inside the tracked scroll container.
  - Behavior: Skips zero-height viewports. Applies `scroll_delta` to the
    handle's y offset, clamped to `[-max_offset, 0]`, and requests an animation
    frame when it changed.

- **`scroll_delta`** (private, L1148)
  - Signature: `fn scroll_delta(viewport: (Pixels, Pixels), target: (Pixels, Pixels), margin: Pixels) -> Pixels`
  - Purpose: Pure scroll math for `scroll_into_view`.
  - Behavior: Returns the signed offset change bringing `target` (top, bottom)
    into `viewport` with `margin` (shrunk to fit small viewports); positive when
    the target is above, negative when below, zero when visible.

##### Context: `TextLayout`

- **`ensure`** (private, L242)
  - Signature: `fn ensure(&mut self, key: LayoutKey, text: impl FnOnce() -> SharedString, window: &Window)`
  - Purpose: The only reshape point; caches by `LayoutKey`.
  - Behavior: Returns early on equal key. Otherwise builds runs
    (`marked_text_runs`), calls `shape_text` with the wrap width, derives
    `VisualRow`s from each line's wrap-boundary glyph indices (`build_rows`),
    recomputes `max_width`, stores the key. Shaping errors yield no lines.

- **`line_height`** (private, L279)
  - Signature: `fn line_height(&self) -> Pixels`
  - Purpose: Row height of the last layout (16px before any).

- **`showing_placeholder`** (private, L283)
  - Signature: `fn showing_placeholder(&self) -> bool`
  - Purpose: Whether cached rows describe the placeholder (or nothing yet).
  - Behavior: `true` without a key or when `key.placeholder` is `Some`; hit
    testing then maps everything to offset 0.

- **`content_height`** (private, L290)
  - Signature: `fn content_height(&self, min_lines: usize) -> Pixels`
  - Purpose: Auto-grow height.
  - Behavior: `line_height × max(rows, min_lines, 1)`.

- **`x_for_offset`** (private, L295)
  - Signature: `fn x_for_offset(&self, row_ix: usize, offset: usize) -> Pixels`
  - Purpose: X of an offset relative to its row's left edge.
  - Behavior: `x_for_index` on the unwrapped logical line minus the row start's
    x. Zero for placeholder / missing rows.

- **`position_for_offset`** (private, L308)
  - Signature: `fn position_for_offset(&self, offset: usize) -> Point<Pixels>`
  - Purpose: Caret position relative to the element origin.
  - Behavior: Row via `row_index_for_offset` (downstream at soft wraps), y =
    row × line height, x = `x_for_offset`.

- **`offset_for_x`** (private, L317)
  - Signature: `fn offset_for_x(&self, text: &str, row_ix: usize, x: Pixels) -> usize`
  - Purpose: Closest caret offset to `x` on a row (clicks, Up/Down).
  - Behavior: Placeholder → 0; missing row → `text.len()`. Uses
    `closest_index_for_x` on the logical line shifted by the row start x, then
    `clamp_offset_to_row` so the caret stays on that row.

- **`offset_for_point`** (private, L334)
  - Signature: `fn offset_for_point(&self, text: &str, position: Point<Pixels>) -> usize`
  - Purpose: Hit test a local point.
  - Behavior: Row = floor(y / line height) clamped to existing rows (above → first,
    below → last), then `offset_for_x`. No rows → 0.

##### Context: `TextArea`

- **`new`** (pub, L365)
  - Signature: `pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self`
  - Purpose: Creates an empty area.
  - Behavior: New focus handle, empty buffer, `min_lines = 1`, no layout,
    bounds, goal column or scroll handle.

- **`min_lines`** (pub, L382)
  - Signature: `pub fn min_lines(mut self, min_lines: usize) -> Self`
  - Purpose: Builder for the minimum height in rows (comfortable empty click target).
  - Behavior: Stores `max(min_lines, 1)`.

- **`set_min_lines`** (pub, L388)
  - Signature: `pub fn set_min_lines(&mut self, min_lines: usize, cx: &mut Context<Self>)`
  - Purpose: Runtime variant of `min_lines`.
  - Behavior: Stores `max(min_lines, 1)` and notifies.

- **`track_scroll`** (pub, L395)
  - Signature: `pub fn track_scroll(&mut self, handle: &ScrollHandle)`
  - Purpose: Opt-in caret autoscroll for the scrollable parent.
  - Behavior: Stores a clone; after keyboard moves/edits the next prepaint
    scrolls that container so the caret (+1 line margin) is visible.

- **`content`** (pub, L400)
  - Signature: `pub fn content(&self) -> SharedString`
  - Purpose: Current text (always `\n` line endings).
  - Behavior: Copies the buffer text into a `SharedString`.

- **`is_empty`** (pub, L405)
  - Signature: `pub fn is_empty(&self) -> bool`
  - Purpose: Whether the text is empty.

- **`set_content`** (pub, L411)
  - Signature: `pub fn set_content(&mut self, text: impl AsRef<str>, cx: &mut Context<Self>)`
  - Purpose: Loads a document (e.g. quoted reply).
  - Behavior: Normalizes newlines, caret to 0, resets history and goal column,
    requests autoscroll, notifies. No `Changed` event.

- **`clear`** (pub, L419)
  - Signature: `pub fn clear(&mut self, cx: &mut Context<Self>)`
  - Purpose: Empties the area.
  - Behavior: `set_content("")` (history reset, no event).

- **`set_cursor_offset`** (pub, L425)
  - Signature: `pub fn set_cursor_offset(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Places the caret (UTF-8 bytes) programmatically.
  - Behavior: Clamped to text/char boundary, collapses selection, autoscrolls, notifies.

- **`placeholder`** (pub) — `pub fn placeholder(&self) -> &SharedString`: the current placeholder.

- **`set_placeholder`** (pub, L430)
  - Signature: `pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>)`
  - Purpose: Updates the empty-state hint.
  - Behavior: No notify (visible next frame; part of the layout key).

- **`cursor_offset`** (pub, L435)
  - Signature: `pub fn cursor_offset(&self) -> usize`
  - Purpose: Caret offset (moving end of the selection).

- **`selected_range`** (pub, L440)
  - Signature: `pub fn selected_range(&self) -> Range<usize>`
  - Purpose: Selected byte range (empty for a bare caret).

- **`display_text`** (private, L444)
  - Signature: `fn display_text(&self) -> SharedString`
  - Purpose: Text to shape: placeholder when empty, else a copy of the buffer.

- **`sync_layout`** (private, L454)
  - Signature: `fn sync_layout(&self, window: &Window)`
  - Purpose: Avoids stale rows between an edit and the next frame.
  - Behavior: Reuses the last key's style/width with the current revision,
    mark and placeholder state and calls `ensure`. No-op before the first layout.

- **`move_to`** (private, L470)
  - Signature: `fn move_to(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Collapse selection to `offset` then `after_move`.

- **`select_to`** (private, L475)
  - Signature: `fn select_to(&mut self, offset: usize, cx: &mut Context<Self>)`
  - Purpose: Extend selection's caret end to `offset` then `after_move`.

- **`after_move`** (private, L480)
  - Signature: `fn after_move(&mut self, cx: &mut Context<Self>)`
  - Purpose: Common post-move bookkeeping.
  - Behavior: Clears `goal_x`, requests autoscroll, notifies.

- **`edit`** (private, L488)
  - Signature: `fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut TextBuffer))`
  - Purpose: Single path for user edits.
  - Behavior: Runs `f`, `after_move`, emits `TextAreaEvent::Changed` only if the
    buffer revision changed.

- **`collapse_or`** (private, L499)
  - Signature: `fn collapse_or(&self, to_start: bool, step: impl FnOnce(&str, usize) -> usize) -> usize`
  - Purpose: Horizontal move target.
  - Behavior: With a selection returns its start/end; otherwise `step(text, cursor)`.

- **`left` / `right`** (private, L510 / L515)
  - Signature: `fn left(&mut self, _: &Left, …)`, `fn right(&mut self, _: &Right, …)`
  - Purpose: Grapheme-wise caret move, or collapse selection to its start/end.

- **`word_left` / `word_right`** (private, L520 / L525)
  - Signature: `fn word_left(&mut self, _: &WordLeft, …)`, `fn word_right(&mut self, _: &WordRight, …)`
  - Purpose: Word move via `text_input::{previous,next}_word_start`, or collapse selection.

- **`select_left` / `select_right`** (private, L530 / L535)
  - Signature: `fn select_left(&mut self, _: &SelectLeft, …)`, `fn select_right(&mut self, _: &SelectRight, …)`
  - Purpose: Extend selection by one grapheme.

- **`select_word_left` / `select_word_right`** (private, L540 / L545)
  - Signature: `fn select_word_left(&mut self, _: &SelectWordLeft, …)`, `fn select_word_right(&mut self, _: &SelectWordRight, …)`
  - Purpose: Extend selection to the previous/next word start.

- **`row_edge`** (private, L551)
  - Signature: `fn row_edge(&self, end: bool, window: &Window) -> usize`
  - Purpose: Home/End target on the caret's visual row.
  - Behavior: Syncs layout; start → row start, end → `row_caret_end` (before the
    trailing wrap grapheme on soft-wrapped rows). Without a real layout falls
    back to the logical line bounds.

- **`home` / `end` / `select_home` / `select_end`** (private, L569 / L574 / L579 / L584)
  - Signature: `fn home(&mut self, _: &Home, window: &mut Window, …)` (and siblings)
  - Purpose: Move / extend to the visual row start or end via `row_edge`.

- **`move_to_beginning` / `move_to_end`** (private, L589 / L593)
  - Signature: `fn move_to_beginning(&mut self, _: &MoveToBeginning, …)`, `fn move_to_end(&mut self, _: &MoveToEnd, …)`
  - Purpose: Caret to document start (0) / end (`len`).

- **`select_to_beginning` / `select_to_end`** (private, L597 / L606)
  - Signature: `fn select_to_beginning(&mut self, _: &SelectToBeginning, …)`, `fn select_to_end(&mut self, _: &SelectToEnd, …)`
  - Purpose: Extend selection to document start / end.

- **`select_all`** (private, L610)
  - Signature: `fn select_all(&mut self, _: &SelectAll, …)`
  - Purpose: Select the whole text (caret at end).

- **`move_vertically`** (private, L616)
  - Signature: `fn move_vertically(&mut self, delta: isize, select: bool, window: &Window, cx: &mut Context<Self>)`
  - Purpose: Up/Down by visual row keeping the preferred column.
  - Behavior: Syncs layout. Origin = caret (or, when collapsing a selection,
    its start for up / end for down). Goal x = stored `goal_x` or origin x.
    Target row via `vertical_target_row`; off the top → 0, off the bottom →
    `len`; else `offset_for_x`. Moves or extends, then restores `goal_x`.

- **`up` / `down` / `select_up` / `select_down`** (private, L652 / L656 / L660 / L664)
  - Signature: `fn up(&mut self, _: &Up, window: &mut Window, …)` (and siblings)
  - Purpose: `move_vertically(±1, select)`.

- **`backspace` / `delete`** (private, L668 / L674)
  - Signature: `fn backspace(&mut self, _: &Backspace, …)`, `fn delete(&mut self, _: &Delete, …)`
  - Purpose: Delete selection or one grapheme before/after the caret via `edit`.

- **`newline`** (private, L680)
  - Signature: `fn newline(&mut self, _: &Newline, …)`
  - Purpose: Inserts `\n` (Typing kind; closes the coalescing run).

- **`undo` / `redo`** (private, L684 / L690)
  - Signature: `fn undo(&mut self, _: &Undo, …)`, `fn redo(&mut self, _: &Redo, …)`
  - Purpose: History steps via `edit` (event only when text changed).

- **`paste`** (private, L696)
  - Signature: `fn paste(&mut self, _: &Paste, …)`
  - Purpose: Inserts clipboard text, keeping newlines (`\r\n`/`\r` → `\n`), one undo step.

- **`copy`** (private, L702)
  - Signature: `fn copy(&mut self, _: &Copy, …)`
  - Purpose: Writes the non-empty selection to the clipboard.

- **`cut`** (private, L710)
  - Signature: `fn cut(&mut self, _: &Cut, …)`
  - Purpose: Copy then delete the non-empty selection (one undo step).

- **`offset_for_mouse`** (private, L719)
  - Signature: `fn offset_for_mouse(&self, position: Point<Pixels>) -> usize`
  - Purpose: Window-coordinate hit test using last painted bounds (0 before first paint).

- **`on_mouse_down`** (private, L728)
  - Signature: `fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>)`
  - Purpose: Focus + click selection.
  - Behavior: `set_native_text_focus`, focus, stop propagation, start drag.
    1 click: move (shift extends); 2: select word (`word_range_at`); 3+: select
    logical line including its newline. Clears goal column, notifies.

- **`on_mouse_up`** (private, L758)
  - Signature: `fn on_mouse_up(&mut self, _: &MouseUpEvent, …)`
  - Purpose: Ends drag selection.

- **`on_mouse_move`** (private, L762)
  - Signature: `fn on_mouse_move(&mut self, event: &MouseMoveEvent, …)`
  - Purpose: Drag selection: while dragging, extends selection to the hit offset.

- **`range_to_utf16` / `range_from_utf16`** (private, L771 / L776)
  - Signature: `fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize>` (and inverse)
  - Purpose: UTF-8 ↔ UTF-16 range conversion for the platform input handler.

##### Context: `EventEmitter<TextAreaEvent> for TextArea`

_(Marker impl — no methods.)_

##### Context: `EntityInputHandler for TextArea`

- **`text_for_range`** (private, L783)
  - Signature: `fn text_for_range(&mut self, range_utf16: Range<usize>, actual_range: &mut Option<Range<usize>>, …) -> Option<String>`
  - Purpose: Substring for IME queries; reports the actual UTF-16 range.

- **`selected_text_range`** (private, L795)
  - Signature: `fn selected_text_range(&mut self, …) -> Option<UTF16Selection>`
  - Purpose: Selection in UTF-16 plus reversed flag (always `Some`).

- **`marked_text_range`** (private, L807)
  - Signature: `fn marked_text_range(&self, …) -> Option<Range<usize>>`
  - Purpose: IME marked range in UTF-16, if any.

- **`unmark_text`** (private, L817)
  - Signature: `fn unmark_text(&mut self, …)`
  - Purpose: Clears the marked range and notifies (text unchanged, no event).

- **`replace_text_in_range`** (private, L822)
  - Signature: `fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, …)`
  - Purpose: Platform text insertion (typing, IME commit).
  - Behavior: Ignores a bare `"\t"` (Tab stays with the parent). Otherwise
    `TextBuffer::replace` (Typing kind) on explicit / marked / selected range
    via `edit` (emits `Changed`).

- **`replace_and_mark_text_in_range`** (private, L840)
  - Signature: `fn replace_and_mark_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, new_selected_range_utf16: Option<Range<usize>>, …)`
  - Purpose: IME composition.
  - Behavior: Converts the selection (UTF-16, relative to `new_text`) to UTF-8
    within `new_text`, then `TextBuffer::replace_and_mark` via `edit`.

- **`bounds_for_range`** (private, L858)
  - Signature: `fn bounds_for_range(&mut self, range_utf16: Range<usize>, element_bounds: Bounds<Pixels>, …) -> Option<Bounds<Pixels>>`
  - Purpose: IME candidate window anchor.
  - Behavior: Syncs layout; one-row-high rect from the range start to its end
    (or zero width when the range spans rows), in window coordinates.

- **`character_index_for_point`** (private, L877)
  - Signature: `fn character_index_for_point(&mut self, point: Point<Pixels>, …) -> Option<usize>`
  - Purpose: Platform hit test; `offset_for_mouse` converted to UTF-16.

##### Context: `TextAreaElement`

- **`layout_key`** (private, L900)
  - Signature: `fn layout_key(&self, wrap_width: Option<Pixels>, window: &Window, cx: &App) -> LayoutKey`
  - Purpose: Builds the cache key from entity state and inherited text style.
  - Behavior: Empty text → placeholder with `text_muted`; else the style color.
    Font, size (rem-resolved) and `window.line_height()` from the current style.

##### Context: `IntoElement for TextAreaElement`

- **`into_element`** (private, L924)
  - Signature: `fn into_element(self) -> Self::Element`
  - Purpose: Identity conversion.

##### Context: `Element for TextAreaElement`

- **`id` / `source_location`** (private, L933 / L937)
  - Signature: `fn id(&self) -> Option<ElementId>`, `fn source_location(&self) -> Option<&'static Location<'static>>`
  - Purpose: Always `None`.

- **`request_layout`** (private, L941)
  - Signature: `fn request_layout(&mut self, …, window: &mut Window, cx: &mut App) -> (LayoutId, ())`
  - Purpose: Auto-growing measured layout.
  - Behavior: Width 100%; measure closure shapes (through the cache) only for a
    definite width (known or `Definite` available) and returns
    `content_height(min_lines)`. Indefinite probes report 0 (min-content) or
    `max_width` (max-content) without reshaping and flag
    `measured_without_width`.

- **`prepaint`** (private, L982)
  - Signature: `fn prepaint(&mut self, …, bounds: Bounds<Pixels>, …, window: &mut Window, cx: &mut App) -> PrepaintState`
  - Purpose: Ensures layout at the real width; builds selection and caret quads.
  - Behavior: Schedules one extra frame if the height was measured without a
    width. Selection: `selection_spans`, skipping rows outside the content mask,
    `element_selected` color, a newline sliver (0.4 em) on spans that include
    `\n`. Caret: 2px `accent` quad, 1.3 em tall centered in the row, only when
    the selection is empty. Runs `scroll_into_view` when autoscroll was
    requested and a scroll handle is tracked.

- **`paint`** (private, L1053)
  - Signature: `fn paint(&mut self, …, bounds: Bounds<Pixels>, prepaint: &mut PrepaintState, window: &mut Window, cx: &mut App)`
  - Purpose: Registers the input handler and paints.
  - Behavior: `handle_input`, selection quads, then only the logical lines
    intersecting the content mask (stops after the visible bottom), caret when
    focused. Stores `last_bounds` and clears `autoscroll_requested`.

##### Context: `Render for TextArea`

- **`render`** (private, L1161)
  - Signature: `fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement`
  - Purpose: Interactive wrapper (`key_context("TextArea")`, focus tracking,
    I-beam cursor, every action listener, mouse down/up/up-out/move) around
    `TextAreaElement`; full width, `min_w_0`.

##### Context: `Focusable for TextArea`

- **`focus_handle`** (private, L1208)
  - Signature: `fn focus_handle(&self, _: &App) -> FocusHandle`
  - Purpose: Clones the focus handle.

##### Context: `module` (tests)

- **`base_run`** (helper): default black `TextRun` for run tests.
- **`marked_text_runs_underlines_only_marked_range`**: split lengths 2/3/5,
  only the middle underlined; full mark → one underlined run; no mark → one
  run; out-of-bounds mark ignored.
- **`scroll_delta_keeps_target_in_viewport`**: visible → 0; below → negative;
  above → positive; tiny viewport still reveals the target.

### `src/text_area/buffer.rs`

GPUI-free editing core: UTF-8 byte offsets throughout.

#### Types / constants

- **const `MAX_UNDO_ENTRIES`** (private): 1000; oldest undo entries dropped first.
- **enum `EditKind`** (pub(crate)): `Typing` (typing, IME, newline,
  single-grapheme deletes — coalesce) / `Other` (paste, cut, selection
  deletes — always separate undo steps).
- **struct `Edit`** (private): Diff — `range` (pre-edit), `old_text`,
  `new_text`, `selection_before`, `reversed_before`.
- **struct `History`** (private): `undo` / `redo` stacks of `Edit` and the
  `coalesce` flag (cleared by caret moves and non-typing edits).
- **struct `TextBuffer`** (pub(crate)): `text` (only `\n` terminators),
  ordered in-bounds `selected_range` + `selection_reversed`, optional
  `marked_range`, `revision` (bumped on every text change), `history`.

#### Functions / methods

##### Context: `module`

- **`normalize_newlines`** (pub(crate), L17)
  - Signature: `pub(crate) fn normalize_newlines(text: &str) -> Cow<'_, str>`
  - Purpose: `\r\n` and lone `\r` → `\n`; borrows when there is no `\r`.

- **`clamp_to_char_boundary`** (pub(crate), L27)
  - Signature: `pub(crate) fn clamp_to_char_boundary(text: &str, offset: usize) -> usize`
  - Purpose: Largest char boundary `<= min(offset, len)` (never splits a code point).

- **`previous_grapheme_boundary`** (pub(crate), L36)
  - Signature: `pub(crate) fn previous_grapheme_boundary(text: &str, offset: usize) -> usize`
  - Purpose: Start of the grapheme before `offset` (`GraphemeCursor`, O(cluster)); 0 at start.

- **`next_grapheme_boundary`** (pub(crate), L46)
  - Signature: `pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize`
  - Purpose: End of the grapheme at `offset`; `len` at end.

- **`line_range_at`** (pub(crate), L57)
  - Signature: `pub(crate) fn line_range_at(text: &str, offset: usize) -> Range<usize>`
  - Purpose: Logical line containing `offset`, excluding its `\n`.

- **`word_range_at`** (pub(crate), L69)
  - Signature: `pub(crate) fn word_range_at(text: &str, offset: usize) -> Range<usize>`
  - Purpose: Double-click selection.
  - Behavior: Unicode word-bound segment under `offset`, segmenting only the
    current line; at a line end uses the last segment; empty line → empty range.

- **`offset_to_utf16`** (pub(crate), L89)
  - Signature: `pub(crate) fn offset_to_utf16(text: &str, offset: usize) -> usize`
  - Purpose: UTF-8 byte offset → UTF-16 units (clamped to a char boundary).

- **`offset_from_utf16`** (pub(crate), L95)
  - Signature: `pub(crate) fn offset_from_utf16(text: &str, offset_utf16: usize) -> usize`
  - Purpose: UTF-16 units → UTF-8 byte offset, clamped to `len`.

##### Context: `Edit`

- **`inserted_range`** (private, L129)
  - Signature: `fn inserted_range(&self) -> Range<usize>`
  - Purpose: Range of `new_text` in the post-edit text.

- **`try_merge`** (private, L136)
  - Signature: `fn try_merge(&mut self, next: &Edit) -> bool`
  - Purpose: Typing coalescing.
  - Behavior: Merges when `next` only touches text this edit inserted (append,
    IME re-composition, backspacing just-typed text) by splicing `new_text`;
    or extends a pure-deletion run (backspace: `next` ends at our start;
    forward delete: same start). Otherwise `false`.

##### Context: `History`

- **`record`** (private, L172)
  - Signature: `fn record(&mut self, edit: Edit, kind: EditKind)`
  - Purpose: Pushes or merges an edit.
  - Behavior: Clears redo. Merges into the top entry only for `Typing` while
    `coalesce` is set; else pushes (dropping the oldest past
    `MAX_UNDO_ENTRIES`). Next `coalesce` = Typing and no `\n` inserted.

##### Context: `TextBuffer`

- **`text` / `revision` / `selected_range` / `selection_reversed` / `marked_range` / `selected_text`** (pub(crate), L206 …)
  - Signature: plain getters (`&str`, `u64`, `Range<usize>`, `bool`, `Option<Range<usize>>`, `&str`).
  - Purpose: Read state; `revision` is the layout cache key.

- **`cursor` / `anchor`** (pub(crate), L232 / L241)
  - Signature: `pub(crate) fn cursor(&self) -> usize`, `pub(crate) fn anchor(&self) -> usize`
  - Purpose: Moving / fixed end of the selection (per `selection_reversed`).

- **`set_text`** (pub(crate), L251)
  - Signature: `pub(crate) fn set_text(&mut self, text: &str)`
  - Purpose: Load a document: normalized text, caret 0, no mark, fresh
    history, revision bump. Not undoable.

- **`move_to`** (pub(crate), L261)
  - Signature: `pub(crate) fn move_to(&mut self, offset: usize)`
  - Purpose: Collapse to clamped `offset`; breaks typing coalescing.

- **`select_to`** (pub(crate), L269)
  - Signature: `pub(crate) fn select_to(&mut self, offset: usize)`
  - Purpose: Move the caret end, keep the anchor (`select_range(anchor, offset)`).

- **`select_range`** (pub(crate), L276)
  - Signature: `pub(crate) fn select_range(&mut self, anchor: usize, cursor: usize)`
  - Purpose: Select between clamped offsets in either order (sets reversed);
    breaks coalescing.

- **`select_all`** (pub(crate), L284)
  - Signature: `pub(crate) fn select_all(&mut self)`
  - Purpose: `select_range(0, len)`.

- **`replace`** (pub(crate), L290)
  - Signature: `pub(crate) fn replace(&mut self, range: Option<Range<usize>>, new_text: &str, kind: EditKind)`
  - Purpose: Core edit.
  - Behavior: Range = given / marked / selection (clamped, ordered). Inserts
    normalized text, records history, caret after insertion, clears mark.

- **`replace_and_mark`** (pub(crate), L303)
  - Signature: `pub(crate) fn replace_and_mark(&mut self, range: Option<Range<usize>>, new_text: &str, selected_within: Option<Range<usize>>)`
  - Purpose: IME composition step (Typing kind, so a whole composition is one undo step).
  - Behavior: As `replace`, then marks the inserted span (none if empty) and
    selects `selected_within` relative to it (or caret at its end).

- **`unmark`** (pub(crate), L325)
  - Signature: `pub(crate) fn unmark(&mut self)`
  - Purpose: Drops the marked range.

- **`backspace` / `delete`** (pub(crate), L331 / L347)
  - Signature: `pub(crate) fn backspace(&mut self) -> bool`, `pub(crate) fn delete(&mut self) -> bool`
  - Purpose: Delete selection (Other) or one grapheme before/after the caret
    (Typing). `false` at the document edge with no selection.

- **`undo`** (pub(crate), L363)
  - Signature: `pub(crate) fn undo(&mut self) -> bool`
  - Purpose: Reverts the top edit, restores its pre-edit selection, moves it to redo.

- **`redo`** (pub(crate), L378)
  - Signature: `pub(crate) fn redo(&mut self) -> bool`
  - Purpose: Re-applies the top redo edit, caret after it, moves it to undo.

- **`after_history_step`** (private, L391)
  - Signature: `fn after_history_step(&mut self)`
  - Purpose: After undo/redo: clear mark, break coalescing, bump revision.

- **`resolve_range`** (private, L397)
  - Signature: `fn resolve_range(&self, range: Option<Range<usize>>) -> Range<usize>`
  - Purpose: Given / marked / selected range, clamped to char boundaries and ordered.

- **`apply`** (private, L406)
  - Signature: `fn apply(&mut self, range: Range<usize>, new_text: &str, kind: EditKind)`
  - Purpose: Splice + history record + revision bump; no-op edits (empty range,
    empty text) are skipped entirely.

##### Context: `module` (tests)

- Helpers: **`buffer`** (buffer with text), **`type_text`** (per-char Typing replaces).
- **`normalize_newlines_converts_crlf_and_cr`**, **`clamp_to_char_boundary_never_splits_code_points`**,
  **`grapheme_boundaries_skip_whole_clusters`** (emoji + skin tone, combining
  accent), **`line_range_at_excludes_newline`**, **`word_range_at_selects_word_or_separator`**,
  **`utf16_round_trip`**, **`edit_try_merge_rules`**, **`history_record_coalesces_typing_until_broken`**,
  **`set_text_normalizes_and_resets`**, **`cursor_anchor_and_selection_direction`**,
  **`replace_normalizes_and_moves_caret`**, **`replace_and_mark_tracks_composition`**
  (composition = one undo step), **`backspace_and_delete_handle_graphemes_and_edges`**,
  **`undo_redo_restore_text_and_selection`**, **`typing_coalesces_until_caret_moves`**,
  **`new_edit_clears_redo`**, **`undo_stack_is_bounded`**, **`no_op_edit_is_not_recorded`**.

### `src/text_area/rows.rs`

GPUI-free visual-row model (soft wraps) for caret placement, vertical movement
and selection painting.

#### Types / constants

- **struct `VisualRow`** (pub(crate)): `line` (logical line index),
  `line_start` (absolute), `range` (absolute, never includes `\n`),
  `soft_wrapped` (same logical line continues on the next row).
- **struct `SelectionSpan`** (pub(crate)): `row`, selected `range` on that row
  (may be empty), `includes_newline`.

#### Functions / methods

##### Context: `module`

- **`build_rows`** (pub(crate), L26)
  - Signature: `pub(crate) fn build_rows(lines: impl IntoIterator<Item = (usize, Vec<usize>)>) -> Vec<VisualRow>`
  - Purpose: Rows from each logical line's length + relative wrap offsets.
  - Behavior: Lines separated by one `\n`; wrap offsets of 0 or `>= len` ignored.

- **`row_index_for_offset`** (pub(crate), L54)
  - Signature: `pub(crate) fn row_index_for_offset(rows: &[VisualRow], offset: usize) -> usize`
  - Purpose: Row displaying the caret (binary search, last row starting `<= offset`).
  - Behavior: Downstream affinity at soft wraps (offset = next row start →
    next row); hard line ends stay on their row; empty rows → 0.

- **`row_caret_end`** (pub(crate), L62)
  - Signature: `pub(crate) fn row_caret_end(text: &str, row: &VisualRow) -> usize`
  - Purpose: Last caret position rendered on the row: the row end, or one
    grapheme before it on soft-wrapped rows.

- **`clamp_offset_to_row`** (pub(crate), L71)
  - Signature: `pub(crate) fn clamp_offset_to_row(text: &str, row: &VisualRow, offset: usize) -> usize`
  - Purpose: Clamp into `[row.start, row_caret_end]`.

- **`vertical_target_row`** (pub(crate), L77)
  - Signature: `pub(crate) fn vertical_target_row(row_count: usize, row: usize, delta: isize) -> Option<usize>`
  - Purpose: Row after moving `delta`; `None` past either document edge.

- **`selection_spans`** (pub(crate), L94)
  - Signature: `pub(crate) fn selection_spans(rows: &[VisualRow], selection: &Range<usize>) -> Vec<SelectionSpan>`
  - Purpose: Per-row selection pieces, visiting only touched rows.
  - Behavior: Empty selection / no rows → none. `includes_newline` when a hard
    row's `\n` is inside the selection; rows with neither bytes nor newline
    (e.g. selection ending at a soft wrap) are dropped.

##### Context: `module` (tests)

- **`sample`** (helper): `"hello world\n\nabc"` wrapped after `"hello "`.
- **`build_rows_splits_wraps_and_lines`**, **`row_index_for_offset_prefers_downstream_at_soft_wraps`**,
  **`row_caret_end_backs_off_soft_wrap`**, **`clamp_offset_to_row_keeps_caret_on_row`**,
  **`vertical_target_row_stops_at_document_edges`**, **`selection_spans_cover_wraps_and_empty_lines`**.
