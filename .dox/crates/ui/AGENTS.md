# crates/ui/ — Reusable visual components

## Purpose

Small GPUI component library inspired by Zed's `ui` crate: labels, icons,
buttons, list rows, inputs, scrollbar, tooltip, and flex helpers — enough for
an e-mail client, without Zed-scale bloat.

## Ownership

- Owns: visual primitives (`Label`, `Icon`, `Button`, `IconButton`, `ListItem`,
  `Scrollbar`, `Switch`, `TextInput`, `Tooltip`), `Color`, layout helpers
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

## Work Guidance

- Before adding a component, check Zed (`~/dev/zed`) for the equivalent and
  mirror names/API shape where practical.
- Keep the public surface small; extract only when `bgmail` repeats the same UI.
- User-facing copy does not live here — callers pass strings (localized in
  `bgmail::locale`).

## Verification

- Component/unit tests in each module where logic exists (e.g. scrollbar
  geometry).
- `cargo test -p ui`
- Workspace clippy/fmt as in parent.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/ui/src/` | [`.dox/crates/ui/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
