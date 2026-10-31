# crates/html_view/src/ — Source API

## Purpose

Source of the `html_view` crate: document model, display list, text-run
extraction, GPUI replay, gradient sampling, resource policy and the GPUI
element/view. `html_view.rs` is the crate root and re-exports the public types.

## Ownership

- Owns: every module under `crates/html_view/src/` except the vendored painter
  (`blitz_paint/`, see child doc).
- Does not own: crate dependency/build policy (parent doc).

## Local Contracts

- Every function/method and top-level type here is listed in the **API
  Catalog** with purpose and behavior; update it in the same change as the code.
- Modules other than `paint.rs`/`element.rs` stay free of GPUI types so they can
  be unit-tested without a window.
- `#[cfg(test)]` tests listed when they encode contracts.

## Work Guidance

- New drawing features: record them in `display_list.rs` (document coordinates,
  no GPUI types), replay them in `paint.rs`.

## Verification

- `cargo test -p html_view`

## Child DOX Index

| Scope | Doc | Owns |
|---|---|---|
| `crates/html_view/src/blitz_paint/` | [`.dox/crates/html_view/src/blitz_paint/AGENTS.md`](blitz_paint/AGENTS.md) | Vendored `blitz-paint` and its BGMail changes |

## API Catalog

### `html_view.rs` (crate root)
- Re-exports `DisplayList`, `HtmlDocument`, `DocumentOptions`, `DocumentMode`,
  `Appearance`, `HtmlElement`, `HtmlView`, `Resources`; declares modules
  (`fonts` is private).

### `document.rs`
- `const VIEWPORT_HEIGHT` (private): layout viewport height (800) used only for
  `vh`/`height:100%` roots.
- `const MESSAGE_BASE_URL = "bgmail://message/"`: default base URL; must be
  hierarchical (Blitz panics resolving relative URLs against `about:blank`).
- `enum DocumentMode { NoQuirks, LimitedQuirks, Quirks }`.
- `fn document_mode(html) -> DocumentMode`: mode from the doctype (skips BOM,
  `<?xml`, comments): **no doctype → standards** (mail renders inside
  standards-mode webmail pages); non-html doctype → quirks; XHTML 1.0
  Transitional/Frameset → limited; HTML 4.01 Transitional/Frameset → limited
  with system id, else quirks; legacy public ids → quirks; otherwise standards.
- `enum Appearance { Light, Dark }`: forwarded to `prefers-color-scheme`.
- `struct DocumentOptions { appearance, user_agent_css, base_url, resources }`:
  parse options; extra UA CSS goes after Blitz's default sheet.
- `fn overflow_right(doc, node_id, left)` (private): rightmost edge of the
  in-flow boxes under a node, skipping the inside of boxes whose `overflow-x`
  isn't visible (Blitz's `scrollWidth` misses nested-table overflow).
- `struct HtmlDocument`: Blitz document + resources + cached layout width,
  content width/height and `(height, display list)`.
- `HtmlDocument::parse(html, options)`: parses via `force_html_parse`, wires the
  `ResourceProvider`, base URL, UA sheets and the shared browser-like font
  context (`fonts::font_context`), and enables Blitz's line height quirk unless
  `document_mode(html)` is `NoQuirks`.
- `HtmlDocument::layout(width) -> f32`: sets the viewport when the width changes
  (dropping the cached display list), resolves styles/layout (re-resolving while
  critical resources are pending, max 4 passes), records content width =
  max(width, `overflow_right`) and returns content height =
  max(root height, root scrollHeight).
- `HtmlDocument::content_width()`: width from the last layout (wider than the
  layout width when the mail overflows).
- `HtmlDocument::content_height()`: height from the last layout.
- `HtmlDocument::display_list()`: `display_list_for_height(content_height)`.
- `HtmlDocument::display_list_for_height(min_height) -> Arc<DisplayList>`:
  paints (scale 1) over the content width and max(content, `min_height`) so
  the root background covers the viewport; cached per height until
  invalidated.
- `HtmlDocument::invalidate_paint()`: drops the cached display list.
- `HtmlDocument::blocked_remote_urls()`: remote URLs requested and blocked.
- `HtmlDocument::blitz()` / `blitz_mut()`: underlying `BaseDocument`; mutable
  access invalidates the display list.
- `fn viewport(width, appearance)` (private): Blitz viewport at scale 1.
- `fn force_html_parse(html) -> String`: strips BOM, `<?xml` and doctype prologs
  (any case) and prefixes `<!DOCTYPE html>\n`, so Blitz never picks its XML
  parser.
- `fn box_of(html, id)` (test helper): border box of `#id` at 400px.
- Tests: document modes, line height quirk (quirks vs standards cell height),
  RTL box-only lines reversed, keyword max widths (`min-content`,
  `max-content`), anonymous-cell margin collapsing, consecutive block cells
  stacking in one anonymous cell (hidden cells between), missing families
  falling back to `serif`, clamped images keeping a definite height (ratio
  with `height:auto`), content width of overflowing mail, nested-table rows
  fitting inline-block buttons, inline-block tables sitting on their bottom
  edge, collapsed borders counted once per row, markers aligned with a block
  item's first line, symbol list markers drawn as 5px discs (not "•" glyphs),
  prolog replacement, fragments, lowercase doctype, layout height + caching,
  root background filling a requested height, relative URLs neither panicking
  nor counted as remote, XHTML mail keeps block `<p>`, blocked remote image
  recording, text runs carry ordered words + face.

### `fonts.rs`
- `const SINGLE_FONT_GENERICS` (private): generics resolved to one font (all
  but `Emoji`, which Parley appends for emoji clusters).
- `const EXTRA_FALLBACK_SCRIPTS` (private): `Zyyy`/`Zinh`/`Zzzz`, keyed
  besides fontique's sampled scripts.
- `fn font_context() -> FontContext` (crate): process-wide shared collection
  (system fonts scanned once, Blitz's bullet font registered); on first use
  installs the generic names in Blitz and the fallback order.
- `fn generic_family_names(collection, preferred) -> Vec<(GenericFamily,
  String)>` (crate): per generic, the platform's preferred family when the
  collection has it, else the first of fontique's expanded list.
- `fn set_fallback_order(collection, fallback)` (crate): makes the (deduped,
  installed) family order the default fallback of every script; an empty order
  keeps fontique's per-script fallback.
- `mod platform` (Linux/FreeBSD): `generic_families()` = fontconfig matches of
  Chromium's defaults (Times New Roman, Arial, monospace, Comic Sans MS,
  Impact, system-ui); `fallback_order()` = family-less `FcFontSort` (trimmed).
  Other platforms: both empty (fontique's native behaviour).
- Tests: generic names prefer the platform choice then the first family (no
  emoji, no uninstalled names), fallback order on every script (deduped),
  empty order keeps the platform fallback.

### `display_list.rs`
- `const PATH_TOLERANCE` (private): 0.1 px flattening tolerance.
- `enum Geometry { Rect, RoundedRect(Rect, RoundedRectRadii), Path(BezPath) }`;
  `Geometry::bounds()` returns the bounding box.
- `enum Brush { Solid(Color), Gradient { gradient, transform }, Image { image,
  transform } }`: brush transforms map brush space to document space.
- `struct Word { text, x, width, clusters: Vec<(start, end, x)> }`: a word with
  Blitz's width and per-cluster byte ranges/positions.
- `struct FaceSpec { family, weight, italic, font }`,
  `struct TextRun { face, font_size, color, baseline, words }`,
  `struct GlyphRun { font, font_size, color, transform, glyphs }`,
  `struct ColorGlyphRun { font, font_size, glyphs: Vec<(id, x, baseline)> }`.
- `enum Command { PushLayer { clip, radii, alpha, grayscale }, Pop, Fill, Stroke,
  BoxShadow, Text, Glyphs, ColorGlyphs }`: document-coordinate commands.
- `struct DisplayList { commands }`; `new()`; `push_text(run)` and
  `push_color_glyphs(run)` record non-empty runs.
- `fn is_axis_aligned(transform)`: no rotation/skew.
- `fn transform_geometry(shape, transform) -> Geometry`: keeps rects and rounded
  rects (radii scaled by the smaller axis factor) under axis-aligned transforms;
  other shapes become transformed paths, re-recognised as boxes when possible.
- `fn rounded_box_from_path(path) -> Option<Geometry>`: single-subpath paths
  whose points all lie on the bbox edges and whose (closed) area matches a
  rounded box become `Rect`/`RoundedRect` (radii from segment end points).
- `fn filter_effects(filter) -> (grayscale, alpha)`: grayscale (≥50%
  desaturation) and `opacity()` from a CSS filter's colour matrices.
- `fn layer_radii(geometry)` (private): radii of a rounded clip.
- `fn convert_paint(paint, transform, brush_transform)` (private): owned
  `Brush`, folding transforms; resource/custom paints become transparent.
- `fn is_finite_geometry(geometry)`, `fn is_finite_path(path)`: no NaN/∞
  (GPUI's tessellator rejects them).
- `fn is_polygonal(path)`: only straight segments.
- `fn translation(transform)`: translation component.
- `impl PaintScene for DisplayList`: `reset`; `push_layer` (clip bounds + radii,
  alpha × filter opacity, grayscale; blend ignored); `push_clip_layer`;
  `pop_layer`; `stroke` (dash patterns split with `kurbo::dash`, width scaled
  by the transform, non-finite dropped); `fill` (transparent and non-finite
  dropped); `draw_glyphs` (fallback `Glyphs`); `draw_box_shadow`.
- Tests: geometry conversions (rects, rounded rects, rotated, circles as fully
  rounded boxes, curved non-boxes), rounded-box recognition (incl. Blitz's
  unclosed path), non-box paths, dashes, rounded clip radii, filter effects,
  non-finite and transparent fills dropped, layer bounds/alpha, stroke width,
  shadow transform, empty text runs, translation.

### `text.rs`
- `struct FaceInfo { family, weight, italic, weight_axis }`.
- `struct FaceCache`; `face(font)`: memoised `read_face_info`;
  `is_color_bitmap_font(font)`: memoised `has_color_bitmaps`.
- `fn has_color_bitmaps(font)`: CBDT or sbix strikes (colour emoji fonts).
- `fn read_face_info(font)`: typographic (else legacy) family, OS/2 weight,
  italic flag and `wght` axis range via `skrifa`.
- `fn denormalize_weight(coord, (min, default, max))`: F2Dot14 `wght` → CSS
  weight (no `avar`).
- `fn effective_face(info, font, wght_coord, synthetic_bold, synthetic_italic)`:
  `FaceSpec` with variable weight, synthetic bold (≥700) and italic.
- `struct ClusterSpan { text_range, x, advance, is_space }`.
- `fn is_blank(source, range)`: whitespace/control text (or out of range).
- `fn clusters_in_span(clusters, offset, advance)`: clusters of one glyph run
  (Parley splits shaped runs by style without exposing cluster ranges).
- `fn group_words(source, clusters) -> Vec<Word>`: words (x of first visual
  cluster, width, per-cluster ranges); whitespace splits; line breaks → spaces.
- `fn color_glyph_run(glyph_run, transform)`: glyph ids + document positions.
- `fn text_run_from_glyph_run(glyph_run, run_start_x, source, transform, color,
  faces) -> Option<TextRun>`: GPUI-facing run (face, CSS-px size, document
  baseline, words in document x with widths/clusters).
- Tests: word grouping (+widths/clusters), RTL logical words, blank detection,
  no line breaks in words, whitespace-only runs, out-of-range clusters,
  glyph-run cluster isolation, weight denormalisation, synthetic bold/italic,
  variable weight, invalid font.

### `resources.rs`
- `enum Resolution { Served(Vec<u8>), Blocked, Missing }`.
- `struct Resources`; `new()`; `with_attachment(content_id, bytes)` (angle
  brackets stripped); `allow_file_urls(bool)`; `blocked_urls()`;
  `resolve(url)`: `data:` decoded, `cid:` (percent-decoded) from attachments,
  `file:` only when allowed, `bgmail:` (message-relative) missing, everything
  else blocked and recorded.
- `fn percent_decode(input)` (private): `%XX` decoding, invalid sequences kept.
- `struct ResourceProvider(Arc<Resources>)` (crate): `NetProvider` that answers
  Blitz synchronously (`bytes` or `error`).
- Tests: data URLs, cid with/without brackets, percent-encoded cid, remote
  blocking + recording, message-relative URLs, file opt-in, percent decoding.

### `gradient.rs`
- `fn gradient_t(kind, point)`: position along linear (projection), radial
  (concentric approximation) or sweep (angle) gradients.
- `fn extend_t(t, extend)`: pad/repeat/reflect.
- `fn sample_stops(stops, t)`: premultiplied-sRGB interpolation, straight RGBA.
- `fn rasterize(gradient, transform, area, width, height)`: BGRA samples.
- `fn is_native(gradient)`: two-stop linear (drawn by GPUI directly).
- Tests: linear/radial/sweep `t`, extend modes, premultiplied interpolation,
  BGRA output, native detection.

### `paint.rs`
- `struct PaintCache`: per-view caches (images, sampled gradients, colour
  glyph bitmaps, known font families, fonts registered with GPUI).
- `struct ColorGlyphImage` (private): decoded glyph bitmap + strike metrics.
- `fn color_glyph_rect(x, baseline, font_size, ppem, bearing_x, bearing_y,
  width, height, bottom_origin)`: document rect of a colour glyph bitmap.
- `fn decode_color_glyph(font, glyph, pixel_size)` (private): PNG/BGRA strike
  bitmap → straight BGRA `RenderImage`.
- `fn paint_color_glyphs(run, origin, layer, cache, window)` (private): culls,
  decodes (cached) and draws emoji bitmaps with `paint_image`.
- `fn to_hsla(color, alpha)`: peniko color × layer alpha → GPUI `Hsla`.
- `struct LayerState { alpha, rounded_clip, grayscale }` (+ `Default`): state
  inherited from enclosing layers during replay.
- `fn corners(radii)` (private): kurbo radii → GPUI corners.
- `fn clip_corners(rect, clip, radii)`: a rounded clip's radius on each corner
  of `rect` that sits on the clip's matching corner (GPUI masks are
  rectangular).
- `fn same_box(a, b)`: equal within half a pixel.
- `fn needs_cluster_placement(shaped_width, blitz_width)`: > max(0.75px, 2%).
- `fn to_bounds(rect, origin)` (private): document rect → window bounds.
- `fn css_angle(start, end)`: CSS gradient angle (0° up, clockwise).
- `fn to_bgra_straight(brush)`: RGBA/BGRA, premultiplied or straight → straight
  BGRA (GPUI's image format).
- `fn downscaled_size(source, target) -> Option<(u32, u32)>`: upload size when
  an image is drawn at less than half its size on either side (never above
  the source size).
- `fn resize_straight(buffer, width, height)`: area (triangle) resize in
  premultiplied alpha, returned straight.
- `fn render_image(cache, brush, target)` (private): cached `RenderImage` per
  image and upload size (pre-downscaled when `downscaled_size` says so).
- `fn gpui_font(face, cache, window)` (private): GPUI `Font` for a `FaceSpec`,
  registering the font bytes when the family is not installed.
- `fn path_builder(path, origin, builder)` (private): kurbo path → GPUI path.
- `fn paint_text(run, origin, alpha, cache, window, cx)` (private): culls runs
  outside the mask; shapes each word with `shape_line` on Blitz's baseline, or
  each cluster at its own x when widths disagree.
- `fn paint_fill(geometry, brush, origin, layer, cache, window)` (private):
  quads for (rounded) rects, paths otherwise; corners from the geometry or
  from the rounded clip corners the fill touches; native/sampled gradients;
  image fills.
- `fn gradient_background(gradient, transform, alpha)` (private): two-stop
  linear GPUI background.
- `const MAX_GRADIENT_TEXTURE` (private), `fn gradient_texture_size(area,
  scale_factor)`: device-size texture capped at 512 px per side.
- `fn paint_sampled_gradient(...)` (private): cached sampled gradient image.
- `fn paint_image_fill(area, corner_radii, brush, transform, origin, layer,
  cache, window)` (private): repeat-aware tiles clipped to the fill area,
  uploaded at device size when much smaller than the image, rounded when
  covering the box or on the rounded clip's corners, grayscale per layer.
- `fn tile_starts(start, len, from, to, repeat)`: tile origins covering a span.
- `fn paint_commands(...)` (private): recursive replay; layers become content
  masks with multiplied alpha, inherited rounded clip and grayscale.
- `fn paint_display_list(list, origin, cache, window, cx)`: replays a whole list
  (tolerates stray `Pop`).
- Tests: colour glyph placement, cluster placement threshold, `same_box`,
  `clip_corners`, CSS angles, color alpha, gradient texture cap, tiles,
  RGBA→BGRA, unpremultiply, downscale sizes, premultiplied resizing.

### `element.rs`
- `const FALLBACK_WIDTH` (private), `fn layout_width(known, available)`: known
  width, else definite available width, else 800.
- `fn canvas_height(element_height, element_top, visible_bottom)`: paint height
  reaching the bottom of the visible area.
- `fn content_width_changed(rendered, current)`: ≥ 0.5px apart.
- `struct HtmlElement { document, cache, rendered_content_width }`;
  `HtmlElement::new(document, cache)` records the document's content width at
  render time; `impl Element`: `request_layout` uses a measured layout (pane
  width, content height), `prepaint` refreshes the window when the content
  width changed since render (stale overflow spacer), `paint` replays the
  display list for `canvas_height` over max(element, content) width.
- `struct HtmlView`; `new(document)`; `set_document(document, cx)` (resets
  scroll); `document()`; `scroll_handle()`; `impl Render`: `div` scrolling on
  both axes with the element and a zero-height spacer as wide as the content.
- Tests: width selection, canvas height, content width change threshold.

### `examples/`
- `viewer.rs`: window (app id `html_view_viewer`) showing HTML files in an
  800 px (or `--width <px>`, first argument) column (`file:` allowed, base URL
  = file); Left/Right switch files;
  `--capture <dir>` scrolls each file, grabs every viewport with `grim`
  (window found via `hyprctl clients`) and stitches full-page PNGs.
- `layout_dump.rs`: element border boxes in document order (TSV).
- `dump_text.rs`: text runs (face, size, baseline, words), optional filter.
- `dump_cmds.rs`: display-list summary (`DUMP_PATHS=1` prints path elements).
- `measure.rs`: heights of `#t`/`#p` and faces used (font metric checks).
- `content_size.rs`: `<width> <files>`: content width and height per file
  (overflow vs Chromium's full-page width).
