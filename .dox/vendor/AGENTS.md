# vendor/ — Patched third-party crates

## Purpose

Local copies of third-party crates BGMail must patch, so fixes ship with the
repository instead of living in someone's checkout.

## Ownership

- Owns:
  - `vendor/serde_fmt/` (serde_fmt 1.1.0, MIT OR Apache-2.0), wired by
    `[patch.crates-io]` in the root `Cargo.toml`.
  - `vendor/blitz/` (DioxusLabs/blitz rev `e42ee1f`, MIT OR Apache-2.0): the
    subset `crates/html_view` uses (`blitz-dom`, `blitz-html`, `blitz-traits`,
    `stylo_taffy`, `debug_timer`), path-depended on by `html_view`.
  - `vendor/taffy/` (DioxusLabs/taffy rev `c256056`, MIT), wired by
    `[patch."https://github.com/DioxusLabs/taffy"]` in the root `Cargo.toml`
    and in `vendor/blitz/Cargo.toml`.
- Does not own: code vendored inside a crate (e.g.
  `crates/html_view/src/blitz_paint/`, documented there).

## Local Contracts

- `vendor/blitz` and `vendor/taffy` are their own Cargo workspaces and are
  listed in the root `[workspace] exclude`; never make them members.
- Every change is marked with a `BGMail` comment so a re-vendor can re-apply it.
- `serde_fmt`: upstream `impl From<serde_fmt::Error> for core::fmt::Error`
  removed. GPUI 0.2.2 enables `log/kv_unstable_serde`, which pulls
  `serde_fmt`; that foreign impl makes `?` ambiguous in Stylo's `ToCss` derive.
- `blitz` (HTML e-mail fidelity, measured in `spikes/html_render`):
  - `layout/table.rs`: non-cell children of a row become anonymous cells
    (`<td style="display:block">`); browser automatic table layout
    (`distribute_auto_columns`, `TableTreeWrapper::auto_layout_column_style`:
    min/max content per column, colspans, percent → fixed → auto priority,
    table min/max-width); cell widths from every row; min/max-height ignored on
    cells.
  - `stylo.rs`: `<table align>` → auto margins/float; case-insensitive `align`;
    legacy attributes as CSS (`legacy_presentational_css`): `cellspacing`,
    `cellpadding`, table/cell `border` (incl. `border="0"`), `<font
    color/size/face>`, `<body text>`, named/bare-hex `bgcolor`.
  - `assets/default.css`: `td, th { padding: 1px }`, cell min/max-height
    neutralised (`!important`).
  - `stylo_to_parley.rs`: `-apple-system`/`BlinkMacSystemFont` map to
    `system-ui` on every platform (as Chromium does).
  - `document.rs` + `layout/inline/layout.rs`: `set_line_height_quirk` (lines
    with only atomic inline boxes get no strut, as in (limited-)quirks
    browsers); line-fit tolerance of 1/32 px.
  - `layout/inline/build.rs`: zero-height spacer inline boxes (`SPACER_BIT`
    ids) reserve inline elements' horizontal margin+border+padding; every
    inline-box id lookup skips spacers.
  - `layout/inline/layout.rs`: in RTL containers, lines made only of inline
    boxes are mirrored (Parley leaves boxes in source order).
  - `layout/table.rs`: anonymous cells lay their block out with collapsible
    margins (own + children's) and size the cell to box + collapsed margins
    (`TableCell::anonymous`/`own_margins`, `TableTreeWrapper::anonymous_margins`).
  - `stylo_taffy/src/convert.rs` (`size_with_keyword_max`, used by the style
    wrapper's `size()`): `auto` size + `max-*: min-content` → `min-content`;
    `max-content`/`fit-content` → `fit-content` (Taffy has no keyword max sizes).
  - `layout/table.rs` + `layout/construct.rs`: consecutive non-cell children
    of a row (`<td style="display:block">`, `display:none` ones skipped) are
    wrapped in **one** anonymous block (`group_anonymous_cell_runs`,
    `create_anonymous_block_node`) that becomes the anonymous cell, so
    responsive columns stack; the wrappers are owned by the table's
    `anonymous_blocks`. Collapsed-border cells get
    `NodeFlags::IS_COLLAPSED_BORDER_CELL` → `StyleFlags::NO_BORDER` (no
    double-counted borders). Column distribution has 0.5px slack (percentages
    summing to 100%).
  - `layout/mod.rs`: tables export no baseline except `inline-table`;
    `layout/inline/layout.rs`: inline boxes that export a baseline are laid
    out with `PerformLayout` even when the line is only measured, and a
    `<table>` made `inline-block` exports none (Chromium).
  - `layout/replaced.rs`: a definite preferred size in one axis wins over the
    ratio when the other axis is clamped/known (CSS2 §10.4's table only for
    `auto`×`auto`).
  - `traversal.rs` (`collect_inline_roots_in_range`): walks the layout tree
    (which has the anonymous blocks) so multi-root text selections are in
    order and complete; the DOM-order helpers it used are removed.
  - `stylo_to_parley.rs`: `set_generic_family_names` (embedder resolves CSS
    generics to single family names); font lists without a generic end in
    `serif`.
- `taffy`:
  - `compute/block.rs`: compressible replaced items with percentage
    `width`/`max-width` contribute 0 to min-content width; block-level replaced
    items avoid floats; BFC roots keep bottom padding/border below floats.
- Remove a vendored copy as soon as upstream makes the patch unnecessary;
  prefer upstreaming each Blitz/Taffy fix.

## Work Guidance

- Re-vendoring: copy the upstream revision, re-apply the `BGMail` changes,
  then run `cargo +1.95.0 test -p blitz-dom` inside `vendor/blitz` and the
  `spikes/html_render` capture loop (`tools/capture_iter.sh`).

## Verification

- `cargo build --workspace`
- `cd vendor/blitz && cargo test -p blitz-dom`

## Child DOX Index

_(none)_
