# assets/icons/ — SVG icon set

## Purpose

Square SVG icons embedded by `crates/ui` and rendered via `gpui::svg()` with
theme tinting.

## Ownership

- Owns: SVG files under `assets/icons/`.
- Does not own: `IconName` enum / embed table (`crates/ui`), app window icon
  (`assets/appIcon/`).

## Local Contracts

- One SVG per icon; normalized square `viewBox` for a consistent footprint.
- Artwork baseline: FontAwesome 6 Free Solid (Star outline uses Regular);
  `chevron-right.svg` is a hand-drawn stroke chevron (must rotate for sidebar
  disclosure).
- Adding an icon requires: new SVG here, `IconName` variant, and embed entry in
  `crates/ui/src/assets.rs`.
- Narrow FA glyphs are centered in a square box by widening the `viewBox`
  (448-wide → `-32 0 512 512`; 384-wide file icons → `-64 0 512 512`).
- Attachment chips use the FA 6.7.2 file family: `file`, `file-image`,
  `file-pdf`, `file-zipper`, `file-audio`, `file-video`, `file-lines`.
- Do not switch back to icon fonts for cross-platform consistency.

## Work Guidance

- Prefer official FA paths when matching existing style; keep strokes/fills
  compatible with GPUI tinting.
- Name files in kebab-case matching the semantic use (`shield-halved.svg`, …).

## Verification

- Build/run UI; icon appears tinted. Add/adjust `ui` tests only when mapping
  logic changes.

## Child DOX Index

_(none)_
