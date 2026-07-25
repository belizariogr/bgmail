# assets/ — Shared binary assets

## Purpose

Repo-level visual assets consumed by the UI (and packaging): icons, font
metadata leftovers, and the application icon.

## Ownership

- Owns: shared files under `assets/` (icons, fonts, appIcon).
- Does not own: bgmail sample e-mail fixtures (`crates/bgmail/assets/`), runtime
  embedding glue (`crates/ui/src/assets.rs`).

## Local Contracts

- **Icons** live in `assets/icons/` and are the source of truth for `IconName`
  artwork (see child doc).
- **App icon** lives in `assets/appIcon/` for packaging / branding.
- **Fonts** under `assets/fonts/` hold legacy FontAwesome codepoint maps; icons
  render as SVGs, not glyph fonts — do not reintroduce icon fonts for UI.
- Prefer adding assets here over embedding large blobs in Rust source.

## Work Guidance

- New toolbar/UI glyphs → `assets/icons/` + `ui` wiring (see icons child).
- Keep license/attribution notes for third-party artwork (FontAwesome CC BY 4.0)
  accurate when swapping files.

## Verification

- Visual/manual: icons tint and share a consistent square footprint.
- `ui`/`bgmail` tests that touch icon or fixture loading continue to pass.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `assets/icons/` | [`.dox/assets/icons/AGENTS.md`](icons/AGENTS.md) | SVG icon set for `ui::IconName` |
