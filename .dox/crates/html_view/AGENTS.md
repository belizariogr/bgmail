# crates/html_view/ — Native GPUI HTML rendering (mail bodies)

## Purpose

Render HTML e-mail bodies **natively inside GPUI**: no webview, no child
window, no off-screen browser, no CPU bitmap of the page. Blitz parses, styles
(Stylo) and lays out (Taffy + Parley) the document; a painter derived from
`blitz-paint` records a display list that is replayed with GPUI primitives, and
text is shaped and drawn by GPUI's own text system, word by word, at the
positions Blitz computed.

## Ownership

- Owns: `HtmlDocument` (parse/layout/paint cache, pointer/text interaction),
  `HtmlView`/`HtmlElement` (GPUI view/element, input, keyboard commands), the display list, the GPUI replayer, gradient sampling,
  the offline resource policy (`Resources`), the browser-like font policy
  (`src/fonts.rs`), and the vendored painter `src/blitz_paint/`.
- Does not own: HTML sanitisation policy and reader UI (`bgmail`), theme colors
  (`theme`), networking (Stage 3, none here).

## Local Contracts

- **Dependency direction:** depends on `gpui` + Blitz crates only; never on
  `bgmail`, `ui`, `theme` or `storage`.
- **Blitz/Taffy:** `blitz-*` are path dependencies on `vendor/blitz` (rev
  `e42ee1f` + BGMail layout fixes); Taffy is patched to `vendor/taffy`; Stylo
  rev matches that Blitz. Layout fixes go there (see `.dox/vendor/AGENTS.md`);
  re-run the `spikes/html_render` loop after any change.
- **Build couplings with GPUI 0.2.2** (keep both or the build breaks):
  - `fontique` with `fontconfig-dlopen`, and the `fontconfig` wrapper with
    `dlopen`, on Linux/FreeBSD (GPUI builds `yeslogic-fontconfig-sys` in
    dlopen mode).
  - Root `[patch.crates-io] serde_fmt = vendor/serde_fmt` (GPUI's
    `log/kv_unstable_serde` pulls a foreign `From<_> for fmt::Error` impl that
    breaks Stylo's derives). See `.dox/vendor/AGENTS.md`.
- **Parse as HTML always:** `force_html_parse` replaces XHTML/XML prologs with
  an HTML doctype (Blitz would otherwise parse mail as XML); quirks documents
  get a legacy HTML 4.0 one so the parser keeps its quirks. The original
  doctype selects the document mode (`document_mode`): a missing doctype
  means **standards** (mail is shown inside standards-mode webmail pages);
  only explicit legacy doctypes (XHTML 1.0 / HTML 4.01 Transitional, HTML 4.0…)
  give (limited-)quirks, which enables Blitz's line height quirk; full quirks
  also adds Chromium's quirks.css rules and quirky-margin dropping.
- **Unavailable images** (blocked remote, missing `cid:`) are rewritten after
  parsing into Chromium's broken-image rendering (alt text after an icon, or
  framed boxes; see `replace_unavailable_images`), so layouts match a browser
  that couldn't load them.
- **Form controls** are inert pictures with Chromium's metrics: `<select>`
  shows its selected option with a ▾ and is as wide as its widest option;
  text inputs are `size` characters wide.
- **Base URL:** defaults to `bgmail://message/` (Blitz panics resolving
  relative URLs against `about:blank`); that scheme resolves to "missing",
  except protocol-relative references (`//host/…` → `bgmail://host/…`),
  which are treated as `https://host/…` remote content.
- **Resources:** `data:` and `cid:` served from memory; `file:` only
  with `Resources::allow_file_urls(true)` (tools/tests); `http:`/`https:` only
  with `Resources::allow_remote(true)` (the user allowed remote content),
  fetched off the UI thread by `remote` (4 lazily started threads, 15s
  timeout, 15 MB cap, no cookies or referrer, 64 MB body cache); `HtmlView`
  re-lays out as they arrive. Everything else is blocked and recorded
  (`blocked_remote_urls`).
- **Coordinates:** documents lay out and paint at scale 1 in CSS px = GPUI
  logical `Pixels`; GPUI applies the display scale factor.
- **Fonts (browser policy, `fonts.rs`):** one process-wide shared font
  collection (system scan once). Each CSS generic family resolves to **one**
  font, given to Blitz as a family name (`blitz_dom::set_generic_family_names`;
  Linux: Chromium's defaults — Times New Roman, Arial, monospace, … — through
  fontconfig). Characters missing from the author's fonts fall back per
  character in the platform order (Linux: family-less `FcFontSort`, as
  Chromium); emoji clusters still try the colour emoji generic first. Font
  lists without a generic end in `serif` (the browser default font).
  "<Family> Black" faces filed under their base family (Windows fonts on
  Linux) are drawn as that family at weight 900 (`weight_aliases`), the
  heaviest face available; decided: no per-file registration of secondary
  family names (startup cost).
- **Text:** never forward glyph ids when the source text is known. Each word is
  shaped with `TextSystem::shape_line` using the family/weight/style of the
  face Parley resolved (`FaceSpec`) and placed at Blitz's x and baseline.
- **Text placement:** when GPUI's shaped width differs from Blitz's word
  width (letter-spacing, justification), clusters are placed individually.
- **Colour emoji:** GPUI 0.2.2 draws CBDT emoji as monochrome masks on Linux,
  so colour-bitmap fonts (CBDT/sbix) record `ColorGlyphs` and are drawn from
  the font's own bitmaps via `paint_image`.
- **Shapes:** rounded boxes arriving as paths are recognised
  (`rounded_box_from_path`) and drawn as GPUI quads; rounded clip layers keep
  their radii so images/gradients filling them get rounded corners (GPUI
  masks are rectangular). Non-finite geometry is dropped. Dashes are split on
  the CPU (`kurbo::dash`). `filter: grayscale()/opacity()` apply to images.
- **Gradients:** two-stop linear → native GPUI `linear_gradient`; radial,
  sweep and multi-stop → sampled texture (≤512 px side) drawn with
  `paint_image`, cached per gradient/size.
- **Canvas:** the root background is painted down to the bottom of the
  visible area (`canvas_height`), like a browser canvas.
- **Horizontal overflow:** layout always uses the pane width; mail wider than
  that (fixed-width tables) reports `content_width()` (rightmost in-flow box,
  not descending into overflow-clipping boxes) and `HtmlView` scrolls both
  axes, extending the scroll area with a zero-height spacer of that width.
- **Interaction:** `HtmlDocument` takes pointer input in document coordinates
  (`PointerInput`) and drives Blitz's event driver (hover, drag selection,
  link clicks); double/triple clicks select a word/paragraph; links are
  captured (`take_activated_link`), never navigated. `hit_info` reports link,
  image and text under a point. Selection is painted with the host's color
  (`DocumentOptions::selection_color`). `find`/`select_match` search the
  text. `HtmlView` routes window-level mouse events (drags may leave it),
  sets the cursor (hand/I-beam), emits `HtmlViewEvent` (`LinkHovered`,
  `LinkActivated`, `ContextMenu`) and handles `Copy`, `SelectAll` and
  scrolling keys in the `HtmlView` key context (`html_view::init` binds them;
  call it once at startup).
- **Animated GIFs:** Blitz keeps every frame (≤400 frames, ≤64 MiB; past
  that the GIF stays still) as `RasterImageData::animation`; the painter
  registers animated images in the display list, replay draws the frame
  due by each animation's clock (started when first painted), and the view
  repaints only when the next frame of a painted animation is due (one
  pending timer; nothing runs for still mail or hidden views).
- **Zoom:** `HtmlDocument::set_zoom` is Blitz's scale factor (layout at
  `width / zoom` CSS px, text shaped at the zoomed size, so text reflows and
  stays sharp); the API stays in pixels. `HtmlView` keeps the zoom across
  documents and zooms with Ctrl/Cmd `+`/`-`/`0` and Ctrl/Cmd + wheel
  (Chromium's levels), reporting `ZoomChanged`.
- **Hostile input:** style/layout recurse per nesting level (~2,000 levels
  overflow an 8 MB stack). `try_parse` refuses trees deeper than
  `MAX_NESTING_DEPTH` (256) with `RenderError::TooDeep`, and `layout`/paint
  never process such a tree (defense in depth). The Blitz document is not
  `Send`, so it can't move off the UI thread. Hostile markup is bounded by
  `MAX_MARKUP_TAGS` and `MAX_TABLE_SPAN_AREA` (`RenderError::TooComplex`), by the display
  list's `MAX_PATH_ELEMENTS` and `MAX_DASHES`, and by `MAX_DECORATION_MARKS`,
  `MAX_BORDER_DOTS` in the painters; what passes them is drawn solid or not at all.
- **Images:** drawn at less than half their size, they are pre-downscaled on
  the CPU (area filter, premultiplied alpha) and cached per upload size —
  GPUI samples without mipmaps.
- **Vendored painter** (`src/blitz_paint/`, MIT/Apache-2.0, Blitz `e42ee1f`):
  keep diffs minimal and marked `BGMail`; see its child doc.

- Web fonts reach GPUI under a family unique to each file
  (`fonts::with_family_name` rewrites the `name` table): GPUI caches a family's
  faces on first use, so later faces of the same family would never be found
  (Google Fonts' italic-first CSS made whole messages italic).

- `DetachedDocument` moves a document built on a background thread to the one
  showing it. It holds this crate's only `unsafe` (`impl Send`): the compiler
  refuses `HtmlDocument` only for Stylo's `Box<dyn FontMetricsProvider>`, whose one
  implementor `vendor/blitz` asserts to be `Send`. A document is moved whole and
  used by one thread at a time. `HtmlDocument::prepare` does the first layout and
  recording up front.

- Selection by dragging starts and follows the pointer anywhere, using the nearest
  text when it isn't over any (`HtmlDocument::follow_selection_drag`), as in a
  browser; Blitz only reacts over text. `HtmlView` scrolls while the pointer is
  past its edges.

- `DocumentOptions::dark_page`: on a dark page, text that can't be read on what is
  behind it is adapted when recording the display list (`contrast`), as colors fixed
  inline for a white page beat any stylesheet. Neutral text takes the page's text
  color, colored text keeps its hue; text on images and gradients stays.

## Work Guidance

- Prefer fixing layout/style issues upstream in Blitz; keep local changes to the
  painting side.
- Visual check: `cargo run -p html_view --example viewer -- [--width <px>] <file.html>...`
  (Left/Right switch files). The spike corpus lives in
  `spikes/html_render/corpus/`.
- Known gaps: a selection can't start in blank space (Blitz anchors only on
  text); find highlights only the current match; blend modes and filters other than grayscale/opacity ignored;
  listbox `<select size>` drawn as a
  drop-down; a bordered inline around a block draws a box border (Chromium:
  empty line fragments); weight matching follows CSS (a face list holding "Arial
  Black" under "Arial" gives it for weight 800, Chromium gives Bold); small `<br>`/line-height drifts (≤5px).
- Diagnostics: `viewer --capture` (stitched full-page grabs with `grim`),
  `layout_dump` (element boxes, diff with `spikes/html_render/tools/ldiff.sh`),
  `dump_text`, `dump_cmds`, `measure`, `content_size` (content width/height
  at a width, vs Chromium's overflow).

## Verification

- `cargo test -p html_view`
- `cargo clippy -p html_view --all-targets -- -D warnings`
- Visual: `viewer` example against `spikes/html_render/corpus`.

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/html_view/src/` | [`.dox/crates/html_view/src/AGENTS.md`](src/AGENTS.md) | Full API catalog (every fn/method) |
