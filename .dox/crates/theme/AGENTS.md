# crates/theme/ — Themes and colors

## Purpose

Lean theme system modeled on Zed's `theme` crate: light/dark palettes and an
app-global active theme for consistent coloring across UI.

## Ownership

- Owns: `Theme`, `ThemeColors`, `Appearance`, `ActiveTheme`, built-in light/dark
  palettes (VSCode Light/Dark Modern).
- Does not own: semantic component color roles (`ui::Color`), layout/components
  (`ui`), app chrome (`rmail`).

## Local Contracts

- Lib path: `src/theme.rs` (single-file crate).
- Colors are declared here only (hex helpers → `Hsla`). Components must not
  hardcode loose hex/`rgb` values; they resolve via theme + `ui::Color`.
- Active theme is a `gpui::Global`; access via `ActiveTheme` on `App`
  (`cx.theme().colors()…`).
- Two built-ins only for now: dark and light. Runtime toggle flips
  `Appearance`.
- Name roles after Zed (`background`, `surface_background`, `element_hover`,
  …) to ease porting.

## Work Guidance

- Add a new color role only when a UI need appears in more than one place or is
  clearly thematic (e.g. scrollbar thumb translucency).
- Keep the crate GPUI-only besides theme types; no app/domain imports.
- Consult Zed's theme crate before inventing new roles or APIs.

## Verification

- Unit tests in `theme.rs` for appearance toggle and theme globals.
- `cargo test -p theme`
- Workspace clippy/fmt as in parent.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/theme/src/` | [`.dox/crates/theme/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
