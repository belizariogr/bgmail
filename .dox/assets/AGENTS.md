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
  `BGMail.svg` is the source; `BGMail-{16,24,32,48,64,128,256,512,1024}.png`,
  `BGMail.ico` (16–256, PNG frames) and `BGMail.icns` (16–1024 incl. @2x) are
  generated from it and referenced by the packager config in
  `crates/bgmail/Cargo.toml` (and the Windows exe icon in
  `crates/bgmail/build.rs`). Keep file names stable. The envelope (body, not
  its drop shadow) sits on the tile's centre: equal top and bottom margins.
- **Fonts** under `assets/fonts/` hold legacy FontAwesome codepoint maps; icons
  render as SVGs, not glyph fonts — do not reintroduce icon fonts for UI.
- Prefer adding assets here over embedding large blobs in Rust source.

## Work Guidance

- New toolbar/UI glyphs → `assets/icons/` + `ui` wiring (see icons child).
- After editing `appIcon/BGMail.svg`, run `script/generate-app-icons` (every
  PNG, the PNG-framed `.ico` and the `.icns` with icp4/icp5/icp6/ic07–ic14) and
  `script/generate-macos-dmg-assets` (the DMG volume icon carries the app icon).
- Keep license/attribution notes for third-party artwork (FontAwesome CC BY 4.0)
  accurate when swapping files.

## Verification

- Visual/manual: icons tint and share a consistent square footprint.
- `ui`/`bgmail` tests that touch icon or fixture loading continue to pass.
- App icon: `magick identify assets/appIcon/BGMail.ico` lists all frames;
  `iconutil -c iconset -o /tmp/x.iconset assets/appIcon/BGMail.icns` unpacks it.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `assets/icons/` | [`.dox/assets/icons/AGENTS.md`](icons/AGENTS.md) | SVG icon set for `ui::IconName` |
