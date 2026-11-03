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
  - `vendor/gpui/` (gpui 0.2.2 from crates.io, Apache-2.0, without `examples/`,
    `tests/` and their manifest targets), wired by `[patch.crates-io]` in the root
    `Cargo.toml`; its own workspace, listed in `[workspace] exclude`.
- Does not own: code vendored inside a crate (e.g.
  `crates/html_view/src/blitz_paint/`, documented there).

## Local Contracts

- `vendor/blitz` and `vendor/taffy` are their own Cargo workspaces and are
  listed in the root `[workspace] exclude`; never make them members.
- Every change is marked with a `BGMail` comment so a re-vendor can re-apply it.
- `serde_fmt`: upstream `impl From<serde_fmt::Error> for core::fmt::Error`
  removed. GPUI 0.2.2 enables `log/kv_unstable_serde`, which pulls
  `serde_fmt`; that foreign impl makes `?` ambiguous in Stylo's `ToCss` derive.
- `gpui`: per-window content scale, for the zoom of the whole app (no public API
  can scale a window's rendering and input together). `Window::set_content_scale`
  multiplies the scale factor and divides the viewport and event positions
  (`scale_event_positions`); what goes back to the platform (IME position, window
  menu position, client inset) is multiplied again. `Window::content_scale`,
  `Window::platform_viewport_size` expose it. `window_bounds`/`bounds` stay in
  platform coordinates.
  `platform/test/window.rs`: `TestWindow`'s window handle is an error instead of a
  panic, which took down any test reaching native code on Windows
  (`set_native_text_focus`).
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
  - `html.rs` + `font_metrics.rs`: `HtmlParserProvider: Send + Sync` and a
    compile-time assertion that `BlitzFontMetricsProvider` is `Send`, so documents
    can be built off the UI thread (`html_view::DetachedDocument`).
  - `assets/default.css`: `td, th { padding: 1px }`, cell min/max-height
    neutralised (`!important`).
  - `net.rs`/`mutator.rs`: nothing in a message's CSS or HTML can abort: an `@import`
    or `<img src>` that doesn't resolve is refused (`resolve_url` removed), and
    `MAX_IMPORTS` limits the total imports of a stylesheet.
  - `net.rs` (`keep_only_paint_in_state_rules`, also called by `make_stylesheet`): author rules
    matching on `:hover`, `:active`, `:focus*`, `:checked`, `:target` keep only paint properties,
    so moving the pointer never creates or frees a box. Reconstruction still happens when a
    width crosses `@media`, so stale-id tolerance stays.
  - `blitz-html/src/html_sink.rs`: fed in 4 KiB pieces, stops at `MAX_DOM_NODES` (500k), since a
    tag can make hundreds of nodes; hosts refuse the partial document by its node count.
  - `net.rs` + `document.rs`: WOFF1 tables sum at most 128 MiB (`woff1_table_bytes`), a
    document's fonts 512 MiB (`font_bytes`).
  - `layout/table.rs`: `TableContext` and `TableCell` keep the `ComputedValues` their `calc()`
    pointers point into (`computed_style`).
  - `layout/inline/build.rs`: at most `MAX_INLINE_SPANS` (60k) style spans per inline context,
    as Parley asserts on more than `u16::MAX`; past it text takes the open style and `<br>`
    doesn't break.
  - `layout/inline/layout.rs`: at most `MAX_FORCED_INLINE_LAYOUTS` (100k) full layouts per pass
    for baselines (`forced_inline_layouts`), as nested inline tables were exponential.
  - `node/node.rs`: `containing_block` and the walks up skip freed boxes.
  - `net.rs`: GIF animations decode under `Limits` (2 × `MAX_ANIMATION_BYTES`), still images
    under `MAX_IMAGE_BYTES` (256 MiB), and a document's images share
    `MAX_DOCUMENT_IMAGE_BYTES` (2 GiB, `BaseDocument::image_budget`): past it they stay
    placeholders.
  - `layout/table.rs`: `colspan` and `<col span>` count at most `MAX_COLSPAN` (1000, as in
    HTML), a table has at most `MAX_TABLE_COLUMNS` (8192; cells past it overlap the last
    column) and `MAX_TABLE_ROW` grid rows (`i16`).
  - `stylo_taffy/src/convert.rs` (`saturate_px`, `MAX_LENGTH_PX`) and `stylo_to_parley.rs`:
    lengths, `calc()` and letter/word spacing saturate at `nscoord_MAX`, without NaN.
  - `layout/inline/layout.rs`: a line's max advance never passes the layout's, which Parley
    asserts in release too.
  - `layout/construct.rs` + `layout/mod.rs`: `display:contents` pseudos of tables and text
    nodes without a wrap context aren't layout children; replaced elements never become
    tables.
  - `document.rs` + `layout/paint_tree.rs` + `layout/damage.rs`: freeing a node
    (`paint_tree_stale`) rebuilds the whole paint tree next frame; stale ids are skipped.
    `get_selected_text` tolerates offsets older than the text.
  - `resolve.rs` (`assert_layout_parents_consistent`, debug only): walks the box tree, as a
    box that became inline keeps its old, unread list.
  - `util.rs` (`parse_svg_image`): an SVG can't read local files; only `data:` images
    resolve, since a device or a UNC path in a message's `<image href>` would block
    the UI or reach the network.
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
    `serif`; `set_family_aliases` (embedder maps a family name to another
    family at a minimum weight, applied when it's the first family listed).
  - Real-mail round (2002-era quirks mail + modern templates, measured
    against Chromium):
    - `layout/table.rs`:
      - `ColumnCursor` keeps rowspan columns occupied through every spanned
        row. It used to free them one row early, which misplaced declared
        widths.
      - A cell's `height` is a minimum (`NodeFlags::IS_TABLE_CELL` →
        `StyleFlags::TABLE_CELL`). So is a table's own `height`.
      - Automatic layout gives the table `min-width` = its columns' min-content.
      - Spanning cells widen columns through `distribute_span_excess`:
        - minimums: fixed columns first, then auto columns, else all columns
          by width;
        - maximums: auto columns first.
      - Short of space, fixed columns grow in proportion to their distance
        from their minimum, so no space is lost.
      - Fixed layout caps `percent + padding` columns at 100%.
      - Only single-column cells set column widths in automatic layout. A
        spanning cell's width is its min/max contribution to the spanned
        columns (`TableCell::span_width`).
      - `TableCell::span` = (colspan, rowspan), so paint can draw collapsed
        borders per cell.
      - Collapsed-border tables have no padding.
      - Cells are placed at the column the HTML table model gives them
        (`ColumnCursor`), not by Taffy's auto-placement. A colspan
        overlapping a rowspan cell stays put instead of jumping past the
        table.
      - In automatic layout, tables with rowspan cells get their row heights
        from `rowspan_row_heights`:
        - cells are measured at their column widths;
        - rowspan cells, narrowest spans first, spread what they lack with
          `distribute_rowspan_height`, measured in Chromium:
          1. rows where another rowspan cell starts, equally;
          2. else unconstrained non-empty rows, by height;
          3. else non-empty rows, by height;
          4. else all rows.
        - The results become the row tracks' minimums. Taffy alone spread
          the excess over every spanned track.
    - `layout/inline/build.rs` (`LineHeightQuirk`): under the line height
      quirk:
      - an inline box, or the root strut, with no direct text and no vertical
        padding or border gets `line-height: 0`;
      - a `<br>` alone on its line gets its own span with the parent's line
        height;
      - only ASCII whitespace counts as no text (`&nbsp;` is text).
    - `layout/inline/build.rs`:
      - The root's `vertical-align` is reset. A cell's `middle` used to shift
        `<br>` spans.
      - Anonymous blocks inside inlines take the enclosing block as their strut
        and push the inline ancestors as spans.
    - `layout/inline/layout.rs`: a block holding only empty inlines keeps its
      given width and lets margins collapse through.
    - `stylo_taffy/src/wrapper.rs` (`ignores_size`): non-replaced
      `display: inline` boxes ignore width, height and min/max sizes.
    - `layout/construct.rs` (`select_options`, `create_select_display`) +
      `layout/mod.rs`:
      - `<select>` is a read-only one-line text control showing its selected
        option, as wide as its widest option (`TextInputData::select_width`).
      - Text inputs are `size` characters wide (`text_control_width`, default
        20).
    - `stylo.rs`:
      - `valign` on table parts;
      - image `align`, `hspace`, `vspace` (`legacy_image_align_css`, exported);
      - `<body link>`;
      - `align=justify` → `text-align: justify`;
    - `net.rs` (`decode_gif_animation`) + `node/element.rs`
      (`AnimationFrames`, `RasterImageData::animation`) + `document.rs`:
      - animated GIFs keep every composited frame with its delay (under 20ms
        plays at 100ms);
      - capped at 400 frames / 64 MiB;
      - `Resource::Image` carries them;
      - `AnimationFrames::frame_at` picks the frame for a time (looping).
      - `<hr color/noshade/size/width/align>`.
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
