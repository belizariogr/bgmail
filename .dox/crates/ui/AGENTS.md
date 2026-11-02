# crates/ui/ — Reusable visual components

## Purpose

Small GPUI component library inspired by Zed's `ui` crate: labels, icons,
buttons, list rows, single-line inputs, a multi-line text area, scrollbar,
tooltip, and flex helpers — enough for
an e-mail client, without Zed-scale bloat.

## Ownership

- Owns: visual primitives (`Label`, `Icon`, `Button`, `IconButton`, `ListItem`,
  `ContextMenu`, `Scrollbar`, `Switch`, `TextInput`, `TextArea`, `Tooltip`),
  `Color`, layout helpers
  (`h_flex` / `v_flex`), embedded SVG `Assets` / `IconName`.
- Does not own: theme palettes (`theme`), app views/windows (`bgmail`), mail
  domain (`storage` / future `mail_core`).

## Local Contracts

- Pattern: `#[derive(IntoElement)]` + `impl RenderOnce`, chainable builder
  methods (Zed style).
- Colors: use `ui::Color` semantic roles resolved against `ActiveTheme`. Never
  scatter literal theme hex in components.
- Icons: each `IconName` maps to an SVG under repo `assets/icons/`, embedded in
  `assets.rs`, rendered with `gpui::svg()` (tinted). No icon fonts.
- New icons: add SVG to `assets/icons/`, wire `IconName` + embed in `Assets`,
  keep square viewBox footprint.
- Prefer reusing existing components over one-off markup in `bgmail`.
- Windows-only deps for text input stay behind `cfg`; do not leak OS details
  into public component APIs.
- Text components: `ui::bind_keys(cx)` (once at startup) registers both
  `TextInput` (context `"TextInput"`) and `TextArea` (context `"TextArea"`)
  bindings, with macOS `cmd`/`alt` and other-platform `ctrl` variants. Both
  emit a `Changed` event (`TextInputEvent` / `TextAreaEvent`) on user edits
  only; programmatic `set_content` / `clear` stay silent. Both own IME via
  `EntityInputHandler` and call `set_native_text_focus` on click.
- Only the focused text field paints its selection. `TextArea::set_selection_color` overrides the highlight for text on a page of another color.
- Repeated clicks: double click selects the word (a masked `TextInput` selects
  everything), triple click the field (`TextInput`) or line (`TextArea`).
- `IconButton::hover_color`: hover feedback through the icon color and no
  background, for buttons over surfaces of varying color.
- `ContextMenu` entries fill the menu's width (minus its padding) wherever the
  host places it, including content-sized anchored boxes.
- `IconButton::compact`: the button is exactly its icon's size.
- `Scrollbar::always_visible` / `thumb_colors`: for content that doesn't follow
  the theme (the reader's page).
- `TextArea` (compose body): plain text with `\n` line endings only (`\r\n`
  normalized on load/paste/input), soft-wraps to its width and grows to its
  content height (`min_lines` floor) — the caller provides the scroll
  container and may pass its `ScrollHandle` via `track_scroll` for caret
  autoscroll. Tab and Shift+Tab move focus to the next/previous field. Its actions are
  namespaced under `ui::text_area::*`; only `TextArea` / `TextAreaEvent` are
  re-exported at the crate root.
- `TextArea` keeps GPUI-free editing logic (text/selection/undo in
  `text_area/buffer.rs`, soft-wrap row math in `text_area/rows.rs`) separate
  from the element so it is unit-tested without a window; reshaping happens
  only when the `LayoutKey` (text revision, width, font, size, color, IME mark,
  placeholder) changes, and painting skips rows outside the content mask.

## Work Guidance

- Before adding a component, check Zed (`~/dev/zed`) for the equivalent and
  mirror names/API shape where practical.
- Keep the public surface small; extract only when `bgmail` repeats the same UI.
- User-facing copy does not live here — callers pass strings (localized in
  `bgmail::locale`).

## Verification

- Component/unit tests in each module where logic exists (e.g. scrollbar
  geometry, `text_area` buffer/rows core). Focus navigation runs on
  GPUI's `test-support` (dev-dependency); other element code is covered through
  its pure helpers.
- `cargo test -p ui`
- Workspace clippy/fmt as in parent.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/ui/src/` | [`.dox/crates/ui/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
