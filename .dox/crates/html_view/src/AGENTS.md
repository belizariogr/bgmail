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
  `Appearance`, `RenderError`, `MAX_NESTING_DEPTH`, `DEFAULT_SELECTION_COLOR`, `init`, `HtmlElement`, `HtmlView`,
  `HtmlViewEvent`, `HitInfo`, `PointerButton`, `PointerInput`,
  `PointerModifiers`, `PointerPhase`, `Resources`; declares modules (`fonts`
  is private).

### `interaction.rs`
- `enum PointerButton { Primary, Secondary, Middle }`,
  `enum PointerPhase { Down { click_count }, Move, Up }`,
  `struct PointerModifiers { shift, control, alt, platform }`,
  `struct PointerInput { x, y, phase, button, primary_pressed, modifiers }`
  (document coordinates).
- `struct HitInfo { link, image, text }`: absolute link URL, image source (or
  its blocked remote URL), whether over text.
- `fn word_bounds(text, offset) -> Range`: word (alphanumeric/`_` run) or
  single other char around `offset`; offsets inside a char snap back.
- `fn find_all(haystack, needle) -> Vec<Range>`: non-overlapping,
  case-insensitive (per-char folding that keeps byte length) matches.
- `struct LinkCapture` (crate): `NavigationProvider` recording the last
  activated URL; `take()`.
- Tests: word bounds (ASCII, multibyte), find (case, overlap, empty),
  multibyte ranges, link capture.

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
- `struct DocumentOptions { appearance, user_agent_css, base_url, resources,
  selection_color }`: parse options; extra UA CSS goes after Blitz's default
  sheet; `selection_color` is RGBA.
- `const MAX_NESTING_DEPTH = 256`: deepest nesting `try_parse` accepts.
- `enum RenderError { TooDeep { depth } }` (+ `Display`, `Error`).
- `fn nesting_depth(doc, root)` (private): deepest node nesting, iterative.
- `const DEFAULT_SELECTION_COLOR`: RGBA used when no selection color is given.
- `fn inline_roots(doc, node_id, out)` (private): inline roots in layout order
  (the layout tree includes anonymous blocks).
- `fn pointer_event(input)` (private): Blitz mouse event for a `PointerInput`
  (page = client = screen coordinates, primary button state, modifiers).
- `fn overflow_right(doc, node_id, left)` (private): rightmost edge of the
  in-flow boxes under a node, skipping the inside of boxes whose `overflow-x`
  isn't visible (Blitz's `scrollWidth` misses nested-table overflow).
- `struct HtmlDocument`: Blitz document + resources + cached layout width,
  content width/height and `(height, display list)`, selection color, base
  URL, link capture and last find matches.
- `const IMAGE_FRAME` / `IMAGE_ICON` (private): CSS of the silver frame
  Chromium draws around broken images and of the 16px broken-image icon slot.
- `fn compact_style(style)` (private): an inline `style`, lower-cased without
  spaces, for the `display:block` / `float:` / pixel-size checks.
- `fn style_px(compact, property)` (private): pixel value of `width`/`height`
  in a compacted style.
- `fn without_box_size(style)` (private): an inline style without
  `width`/`height` (and min/max), `display`, `aspect-ratio`.
- `fn replace_unavailable_images(doc, resources, base, quirks)` (private):
  images whose `src` isn't available (`Resources::is_available`) take
  Chromium's broken-image rendering:
  - With alt text:
    - Inline images become a span holding the icon slot and the alt text.
      They keep `class`/`id`/`title`/`align`, the image's style without its
      box sizes (`without_box_size`: fonts, colors), the `align` float and
      `hspace`/`vspace` margins (quirks: 3px toward the text).
    - Block-level ones (`display:block`, floated, `align=left/right`) keep
      their width, with the height from the attributes' ratio.
    - In quirks mode, any declared size wins: a framed box (square from one
      dimension) clipping the alt text.
  - Without alt (or with `alt=""`):
    - Both dimensions declared: a box of that size, framed only from 18px
      each way (smaller spacer GIFs show nothing).
    - Only a width on a `display:block` image (no `alt`): that width, 16px
      tall.
    - Otherwise, without `alt`: an unframed 16×16 box.
- `const CHROMIUM_UA_CSS` (private), always applied: Chromium UA rules Blitz
  lacks:
  - plain `<hr>` (no `noshade`/`color`) painted as a #9a9a9a/#eeeeee bevel;
  - 13.33px `sans-serif` control font;
  - text inputs with 1px border + 2px/3px padding;
  - `<select>` `inline-block`, border-box, padded for the ▾;
  - 13px check boxes and radios with Chromium's margins.
- `const QUIRKS_UA_CSS` (private), quirks mode only: Chromium's quirks.css:
  - tables don't inherit font, alignment or white-space;
  - 3px beside `img[align]`;
  - `form` bottom margin;
  - border-box text inputs;
  - `[data-bgmail-quirky-start|end]` margins dropped.
- `fn mark_quirky_margins(doc)` (private), quirks mode only: marks the blocks
  whose default ("quirky") margin Chromium ignores, so the UA sheet can drop
  it:
  - the first p/list/heading/blockquote/pre/dl of the body or a cell
    (`data-bgmail-quirky-start`);
  - the last p/pre of a cell (`data-bgmail-quirky-end`).
  The walk looks through div/center/font/form/span wrappers and skips
  comments, collapsible whitespace and `align=left/right` img/table floats.
  Text beside the block keeps its margin.
- `HtmlDocument::parse(html, options)`:
  - Parses `repair_c1_controls(html)` via `force_html_parse(…, mode)`.
  - Wires the `ResourceProvider`, base URL, the shared browser-like font
    context (`fonts::font_context`) and the `LinkCapture` navigation provider.
  - UA sheets: Blitz default + `CHROMIUM_UA_CSS` (+ `QUIRKS_UA_CSS` in
    quirks) + `user_agent_css`.
  - Then runs `replace_unavailable_images` (and `mark_quirky_margins` in
    quirks), and enables Blitz's line height quirk unless
    `document_mode(html)` is `NoQuirks`.
- `with_depth()` (private): records the tree's nesting depth after parsing.
- `HtmlDocument::try_parse(html, options) -> Result<Self, RenderError>`:
  `parse`, then `TooDeep` above `MAX_NESTING_DEPTH`.
- `HtmlDocument::depth()`: element nesting depth.
- `HtmlDocument::layout(width) -> f32`: returns 0 without resolving a tree
  deeper than the limit; otherwise sets the viewport when the width changes
  (dropping the cached display list), resolves styles/layout (re-resolving while
  critical resources are pending, max 4 passes), records content width =
  max(width, `overflow_right`) and returns content height =
  max(root height, root scrollHeight).
- `HtmlDocument::content_width()`: width from the last layout (wider than the
  layout width when the mail overflows).
- `HtmlDocument::content_height()`: height from the last layout.
- `HtmlDocument::display_list()`: `display_list_for_height(content_height)`.
- `HtmlDocument::display_list_for_height(min_height) -> Arc<DisplayList>`:
  paints nothing for a too-deep tree; otherwise paints (scale 1) over the content width and max(content, `min_height`) so
  the root background covers the viewport; cached per height until
  invalidated.
- `HtmlDocument::invalidate_paint()`: drops the cached display list.
- `HtmlDocument::pointer(input) -> bool`: primary double/triple clicks select
  word/paragraph (`select_at`); other primary events go through Blitz's
  `EventDriver` (hover, drag selection, clicks); secondary/middle presses are
  ignored (host menus). Resets Blitz's viewport scroll (fragment links).
  Returns (and invalidates the paint) when hover or selection changed.
- `select_at(x, y, paragraph)` / `root_text(root)` (private): word or
  paragraph selection at a point; text of an inline root.
- `HtmlDocument::take_activated_link()`: last clicked link, cleared.
- `HtmlDocument::hit_info(x, y) -> HitInfo`: text flag from the hit; walks
  ancestors for `<img>` (src, else `data-rm-blocked-src`) and `<a href>`
  (resolved against the base URL).
- `has_selection()`, `selected_text()`, `select_all()` (first to last inline
  root), `clear_selection()`.
- `HtmlDocument::find(query) -> usize`: matches over every inline root's text.
- `HtmlDocument::select_match(index) -> Option<(top, bottom)>`: selects the
  match and returns its line span in document y (parley cursor geometry).
- `HtmlDocument::blocked_remote_urls()`: remote URLs requested and blocked.
- `HtmlDocument::blitz()` / `blitz_mut()`: underlying `BaseDocument`; mutable
  access invalidates the display list.
- `fn viewport(width, appearance)` (private): Blitz viewport at scale 1.
- `const WINDOWS_1252_C1` (private): windows-1252 characters for U+0080–U+009F.
- `fn repair_c1_controls(html) -> Cow<str>` (private): maps C1 controls (from
  Windows-1252 mail decoded as strict ISO-8859-1) to their windows-1252
  characters before parsing, as browsers decode that label; borrowed when
  there are none.
- `fn force_html_parse(html, mode) -> String`: strips BOM, `<?xml` and doctype
  prologs (any case) and prefixes a doctype Blitz never takes for XML:
  `<!DOCTYPE html>`, or for `Quirks` a legacy HTML 4.0 one so html5ever keeps
  the quirks tree building (`<table>` doesn't close an open `<p>`).
- `fn box_of(html, id)` (test helper): border box of `#id` at 400px.
- Tests: document modes, line height quirk (quirks vs standards cell height),
  RTL box-only lines reversed, keyword max widths (`min-content`,
  `max-content`), anonymous-cell margin collapsing, consecutive block cells
  stacking in one anonymous cell (hidden cells between), missing families
  falling back to `serif`, clamped images keeping a definite height (ratio
  with `height:auto`), content width of overflowing mail, nested-table rows
  fitting inline-block buttons, inline-block tables sitting on their bottom
  edge, collapsed borders counted once per row, markers aligned with a block
  item's first line, link clicks reported (not followed), hit info (links,
  relative links, blocked images, text), double/triple click selection, drag
  selection and select-all across paragraphs into anonymous blocks, find and
  match selection, selection painted with the given color, hostile nesting
  refused and never laid out,
  symbol list markers drawn as 5px discs (not "•" glyphs),
  prolog replacement, fragments, lowercase doctype, layout height + caching,
  unavailable images (standards, quirks, tiny, floated with hspace/vspace),
  quirks tables resetting fonts, image `align`/`hspace`, `valign`, cell and
  table sizes as minimums, legacy link/`hr` attributes, line height quirk
  (text-free boxes, nested spans, `&nbsp;`, breaks alone on a line), quirky
  margins at cell edges, quirks parsing of `<p><table>`, capped padded
  percentage columns, spanning cells widening fixed columns first, short
  fixed columns sharing the whole table, inline width ignored around blocks,
  cell `vertical-align` not growing lines, empty-inline blocks collapsing
  margins, form control sizes, selects painting their option and ▾,
  unavailable images framed only from 18px, width-only block images,
  inline alt text keeping the image's font, collapsed borders per cell, C1
  controls repaired, collapsed tables without padding, alt text in links
  underlined, plain `<hr>` bevel,
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
- `Resources::is_available(url)`: whether `resolve` would serve the URL
  (decodable `data:`, known `cid:`, allowed `file:`), without recording it.
- `fn percent_decode(input)` (private): `%XX` decoding, invalid sequences kept.
- `struct ResourceProvider(Arc<Resources>)` (crate): `NetProvider` that answers
  Blitz synchronously (`bytes` or `error`).
- Tests: data URLs, cid with/without brackets, percent-encoded cid, remote
  blocking + recording, message-relative URLs, file opt-in, percent decoding,
  availability without recording.

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
- `actions!(html_view, [Copy, SelectAll, ScrollUp, ScrollDown, PageUp,
  PageDown, ScrollToTop, ScrollToBottom])`; `const KEY_CONTEXT = "HtmlView"`;
  `const LINE_SCROLL` (40px, private).
- `fn init(cx)`: binds `secondary-c`/`secondary-a`, arrows, page keys,
  space/shift-space, home/end in the `HtmlView` context.
- `enum HtmlViewEvent { LinkHovered(Option<String>), LinkActivated(String),
  ContextMenu { position, hit } }`.
- `const FALLBACK_WIDTH` (private), `fn layout_width(known, available)`: known
  width, else definite available width, else 800.
- `fn canvas_height(element_height, element_top, visible_bottom)`.
- `fn content_width_changed(rendered, current)`: ≥ 0.5px apart.
- `fn cursor_for(hit)`: hand over links, I-beam over text, else arrow.
- `fn reveal_scroll(scrolled, height, top, bottom)`: keeps a visible span,
  else puts it a third of the way down (≥ 0).
- `fn modifiers(&Modifiers)` (private): GPUI → `PointerModifiers`.
- `struct HtmlElement { document, cache, rendered_content_width, view }`;
  `new(document, cache)`; `interactive(view)` routes input to an `HtmlView`.
  `impl Element`: measured layout (pane width, content height); `prepaint`
  refreshes on content width change and inserts the hitbox (content width);
  `paint` replays the display list (`canvas_height`, content width) and, when
  interactive, `paint_input_listeners`.
- `fn paint_input_listeners(view, origin, hitbox, window, cx)` (private): sets
  the view's cursor on the hitbox; window-level mouse down (over the hitbox),
  move and up listeners converting to document coordinates.
- `struct HtmlView { document, cache, scroll, focus, cursor, hovered_link,
  dragging }` (`EventEmitter<HtmlViewEvent>`, `Focusable`); `new(document,
  cx)`; `set_document` (resets scroll, drag, cursor, hovered link);
  `document()`; `scroll_handle()`; `has_selection()`; `copy_selection(cx)`;
  `select_all(cx)`; `find(query, cx)` (clears selection without matches);
  `select_match(index, cx)` (scrolls with `reveal_scroll`).
  Private: `set_hovered_link` (emits on change), `mouse_down` (focus;
  secondary → `ContextMenu`; primary starts a drag), `mouse_move` (hover-only
  updates when outside and not dragging; cursor and link hover),
  `mouse_up` (ends the drag, emits `LinkActivated`), `scroll_by` (clamped),
  `page` (viewport − one line).
  `impl Render`: focusable `div` in the `HtmlView` key context scrolling on
  both axes, action handlers, the interactive element and the overflow spacer.
- Tests: width selection, canvas height, content width change threshold,
  cursor choice, reveal scroll.

### `examples/`
- `viewer.rs`: calls `html_view::init`; window (app id `html_view_viewer`) showing HTML files in an
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
