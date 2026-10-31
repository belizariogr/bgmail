# crates/html_view/src/blitz_paint/ — Vendored Blitz painter

## Purpose

Copy of `blitz-paint` (DioxusLabs/blitz `e42ee1f`, MIT OR Apache-2.0, licenses
in this folder) adapted to paint into `crate::display_list::DisplayList`
instead of a generic `anyrender::PaintScene`. It walks a resolved
`blitz_dom::BaseDocument` and emits backgrounds, borders, outlines, shadows,
images, SVG, list markers, selections, text decorations and text.

## Ownership

- Owns: the vendored files and the BGMail modifications listed below.
- Does not own: drawing semantics of upstream code (follow upstream); the
  display list and GPUI replay (parent `src/`).

## Local Contracts

- Third-party code: not cataloged function by function (exception to the
  full-catalog preference, which applies to BGMail-authored source). Upstream
  docs/rustdoc are the reference; only BGMail changes are listed here.
- Keep diffs minimal and grep-able (`BGMail` comments) to ease re-vendoring.
- Entry point: `paint_scene(scene: &mut DisplayList, doc, scale, width, height,
  x_offset, y_offset)` (`mod.rs`). BGMail calls it with scale 1.
- BGMail modifications:
  - `crate::` paths rewritten to `crate::blitz_paint::`; `mod.rs` is upstream
    `lib.rs`; `serde`/`schemars` derives removed from `non_uniform_radii.rs`.
  - Every `&mut impl PaintScene` / `S: PaintScene` is the concrete
    `DisplayList` (text needs `DisplayList::push_text`).
  - `text.rs`: `DrawTextContext` gains `faces: FaceCache`; `stroke_text` takes
    `source_text: Option<&str>`, tracks the start x of each shaped run per line
    and, when the source is known, records a word-positioned `TextRun`
    (`crate::text::text_run_from_glyph_run`) instead of `draw_glyphs`.
  - `text.rs` (`stroke_text`): glyph runs in colour-bitmap fonts (emoji) are
    recorded as `ColorGlyphs` (`crate::text::color_glyph_run`).
  - `text.rs` (`draw_inline_backgrounds`): inline boxes paint background over
    the padding box, then their border as a ring (`inline_border_ring`: outer
    path + reversed inner path, so the inside stays empty), with radius;
    horizontal padding/border only on the element's first/last fragment
    (consecutive runs merged per line).
  - `render/form_controls.rs` (`draw_select_arrow`): a `<select>` (laid out by
    Blitz as a read-only text control) gets a ▾ chevron centered in its end
    padding, in the text color.
  - `render.rs`: callers pass the source text (inline layout text, list marker
    text; `None` for text inputs).
  - `render.rs` (`draw_marker`): symbol markers (`•`, `◦`, `▪`, `■`) are drawn
    as shapes sized like Chromium's (≈ ascent/3, bottom 0.125em above the
    baseline, 6.5px + 0.3em before the content box); ordered markers get a
    0.25em trailing space. Items without their own text (`<li><p>…`) align
    the marker with the first line found down their in-flow block children
    (`first_line_baseline`).
  - `mod.rs`/`render.rs`/`text.rs`: the selection highlight color is a
    `paint_scene` parameter stored on the painter (`selection_color`,
    passed to sub-documents and `draw_text_selection`); the
    `SELECTION_COLOR` constant is gone.
  - `layers.rs` tests adapted to `DisplayList` commands.

## Work Guidance

- Re-vendoring: copy upstream `packages/blitz-paint/src`, re-apply the list
  above, run `cargo test -p html_view` and the `viewer` example.

## Verification

- `cargo test -p html_view` (includes adapted upstream tests in `layers.rs`,
  `gradient.rs`, `kurbo_css/css_box.rs`).

## Child DOX Index

_(none)_
