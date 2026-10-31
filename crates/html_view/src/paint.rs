//! Replays a [`DisplayList`] with GPUI primitives.
//!
//! Every command becomes a native GPUI draw in the window's own scene: quads,
//! paths, shadows, images, and text shaped by GPUI's text system. Nothing is
//! rasterised on the CPU and no foreign surface is composited.

use crate::display_list::{
    Brush, ColorGlyphRun, Command, DisplayList, FaceSpec, Geometry, TextRun,
};
use gpui::{
    linear_gradient, point, px, size, App, BorderStyle, Bounds, BoxShadow, ContentMask, Corners,
    Edges, Font, FontFeatures, FontStyle, FontWeight, Hsla, LinearColorStop, PaintQuad,
    PathBuilder, Pixels, Point, RenderImage, Rgba, SharedString, TextRun as GpuiTextRun, Window,
};
use kurbo::{Affine, BezPath, PathEl, Rect, RoundedRectRadii};
use peniko::{Color, Extend, GradientKind, ImageAlphaType, ImageBrush, ImageFormat};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Per-view caches that survive across frames.
#[derive(Default)]
pub struct PaintCache {
    /// Keyed by image id, source size and the size it is uploaded at (the
    /// source size unless pre-downscaled, see [`downscaled_size`]).
    images: HashMap<(u64, u32, u32, u32, u32), Arc<RenderImage>>,
    gradients: HashMap<(String, u32, u32), Arc<RenderImage>>,
    /// Colour glyph bitmaps by (font blob, face index, glyph, strike ppem).
    color_glyphs: HashMap<(u64, u32, u32, u32), Option<ColorGlyphImage>>,
    known_families: Option<HashSet<String>>,
    registered_fonts: HashSet<u64>,
}

/// A decoded colour glyph bitmap and its placement metrics (strike units).
#[derive(Clone)]
struct ColorGlyphImage {
    image: Arc<RenderImage>,
    ppem: f32,
    bearing_x: f32,
    bearing_y: f32,
    width: f32,
    height: f32,
    bottom_origin: bool,
}

/// Placement of a colour glyph bitmap: (left, top, width, height) in document
/// px for a glyph at `(x, baseline)` drawn at `font_size`.
#[allow(clippy::too_many_arguments)]
pub fn color_glyph_rect(
    x: f32,
    baseline: f32,
    font_size: f32,
    ppem: f32,
    bearing_x: f32,
    bearing_y: f32,
    width: f32,
    height: f32,
    bottom_origin: bool,
) -> (f32, f32, f32, f32) {
    let scale = font_size / ppem.max(1.0);
    let top = if bottom_origin {
        baseline - (bearing_y + height) * scale
    } else {
        baseline - bearing_y * scale
    };
    (x + bearing_x * scale, top, width * scale, height * scale)
}

fn decode_color_glyph(
    font: &peniko::FontData,
    glyph: u32,
    pixel_size: f32,
) -> Option<ColorGlyphImage> {
    use skrifa::bitmap::{BitmapData, BitmapStrikes, Origin};
    let font_ref = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let strikes = BitmapStrikes::new(&font_ref);
    let bitmap = strikes.glyph_for_size(
        skrifa::instance::Size::new(pixel_size),
        skrifa::GlyphId::new(glyph),
    )?;
    let (w, h) = (bitmap.width, bitmap.height);
    let rgba = match bitmap.data {
        BitmapData::Png(bytes) => {
            image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                .ok()?
                .to_rgba8()
        }
        BitmapData::Bgra(bytes) => {
            // Premultiplied BGRA → straight RGBA (swapped again below).
            let mut data = bytes.to_vec();
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2);
                let a = px[3] as u32;
                if a != 0 && a != 255 {
                    for c in &mut px[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            image::RgbaImage::from_raw(w, h, data)?
        }
        BitmapData::Mask(_) => return None,
    };
    let (iw, ih) = rgba.dimensions();
    let mut bgra = rgba.into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(iw, ih, bgra)?;
    Some(ColorGlyphImage {
        image: Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
            buffer
        )])),
        ppem: bitmap.ppem_y,
        bearing_x: bitmap.bearing_x + bitmap.inner_bearing_x,
        bearing_y: bitmap.bearing_y + bitmap.inner_bearing_y,
        width: iw as f32,
        height: ih as f32,
        bottom_origin: bitmap.placement_origin == Origin::BottomLeft,
    })
}

/// Draws colour-bitmap glyphs (emoji) from the font's own strikes.
fn paint_color_glyphs(
    run: &ColorGlyphRun,
    origin: Point<Pixels>,
    layer: LayerState,
    cache: &mut PaintCache,
    window: &mut Window,
) {
    let pixel_size = run.font_size * window.scale_factor();
    let visible = window.content_mask().bounds;
    for &(glyph, x, baseline) in &run.glyphs {
        let y = origin.y + px(baseline);
        if y + px(run.font_size * 2.0) < visible.top()
            || y - px(run.font_size * 2.0) > visible.bottom()
        {
            continue;
        }
        let key = (
            run.font.data.id(),
            run.font.index,
            glyph,
            pixel_size.round() as u32,
        );
        let decoded = cache
            .color_glyphs
            .entry(key)
            .or_insert_with(|| decode_color_glyph(&run.font, glyph, pixel_size))
            .clone();
        let Some(g) = decoded else { continue };
        let (left, top, w, h) = color_glyph_rect(
            x,
            baseline,
            run.font_size,
            g.ppem,
            g.bearing_x,
            g.bearing_y,
            g.width,
            g.height,
            g.bottom_origin,
        );
        let bounds = to_bounds(
            Rect::new(left as f64, top as f64, (left + w) as f64, (top + h) as f64),
            origin,
        );
        window
            .paint_image(bounds, Corners::default(), g.image, 0, layer.grayscale)
            .ok();
    }
}

/// Converts a peniko color to GPUI, multiplying in a layer opacity.
pub fn to_hsla(color: Color, alpha: f32) -> Hsla {
    let [r, g, b, a] = color.components;
    Rgba {
        r,
        g,
        b,
        a: a * alpha,
    }
    .into()
}

/// State inherited from enclosing layers while replaying.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerState {
    pub alpha: f32,
    /// Innermost rounded clip (GPUI masks are rectangular, so rounded clips are
    /// re-applied as corner radii on images that fill them).
    pub rounded_clip: Option<(Rect, RoundedRectRadii)>,
    pub grayscale: bool,
}

impl Default for LayerState {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            rounded_clip: None,
            grayscale: false,
        }
    }
}

fn corners(r: &RoundedRectRadii) -> Corners<Pixels> {
    Corners {
        top_left: px(r.top_left as f32),
        top_right: px(r.top_right as f32),
        bottom_right: px(r.bottom_right as f32),
        bottom_left: px(r.bottom_left as f32),
    }
}

/// Corner radii of a rounded clip that apply to `rect` drawn inside it: GPUI
/// masks are rectangular, so a clip's rounded corners are re-applied to each
/// primitive whose corner sits on the clip's corner (an image filling the top
/// of a rounded card gets the card's top radii).
pub fn clip_corners(rect: Rect, clip: Rect, r: &RoundedRectRadii) -> Corners<Pixels> {
    let at = |x: f64, y: f64, cx: f64, cy: f64| (x - cx).abs() < 0.5 && (y - cy).abs() < 0.5;
    let pick = |hit: bool, radius: f64| px(if hit { radius as f32 } else { 0.0 });
    Corners {
        top_left: pick(at(rect.x0, rect.y0, clip.x0, clip.y0), r.top_left),
        top_right: pick(at(rect.x1, rect.y0, clip.x1, clip.y0), r.top_right),
        bottom_right: pick(at(rect.x1, rect.y1, clip.x1, clip.y1), r.bottom_right),
        bottom_left: pick(at(rect.x0, rect.y1, clip.x0, clip.y1), r.bottom_left),
    }
}

/// Whether `a` and `b` describe the same box within half a pixel.
pub fn same_box(a: Rect, b: Rect) -> bool {
    (a.x0 - b.x0).abs() < 0.5
        && (a.y0 - b.y0).abs() < 0.5
        && (a.x1 - b.x1).abs() < 0.5
        && (a.y1 - b.y1).abs() < 0.5
}

/// Whether GPUI's shaped width is far enough from Blitz's for the word to be
/// placed cluster by cluster (letter-spacing, justification, shaping drift).
pub fn needs_cluster_placement(shaped_width: f32, blitz_width: f32) -> bool {
    (shaped_width - blitz_width).abs() > (0.75f32).max(blitz_width * 0.02)
}

fn to_bounds(rect: Rect, origin: Point<Pixels>) -> Bounds<Pixels> {
    Bounds {
        origin: point(origin.x + px(rect.x0 as f32), origin.y + px(rect.y0 as f32)),
        size: size(px(rect.width() as f32), px(rect.height() as f32)),
    }
}

/// CSS-style angle (0° = up, clockwise) of the gradient line `start -> end`.
pub fn css_angle(start: kurbo::Point, end: kurbo::Point) -> f32 {
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let degrees = dx.atan2(-dy).to_degrees() as f32;
    (degrees + 360.0) % 360.0
}

/// Converts a (non-premultiplied or premultiplied) RGBA/BGRA image into the
/// straight-alpha BGRA frame GPUI uploads.
pub fn to_bgra_straight(brush: &ImageBrush) -> Vec<u8> {
    let image = &brush.image;
    let mut data = image.data.data().to_vec();
    for px in data.chunks_exact_mut(4) {
        if image.format == ImageFormat::Rgba8 {
            px.swap(0, 2);
        }
        if image.alpha_type == ImageAlphaType::AlphaPremultiplied && px[3] != 0 && px[3] != 255 {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    data
}

/// Size to pre-downscale an image to before upload, when it is drawn at less
/// than half its size (`target` in device pixels). GPUI samples textures
/// bilinearly without mipmaps, so larger reductions alias (moiré, jagged
/// icons); browsers filter them.
pub fn downscaled_size(source: (u32, u32), target: (f64, f64)) -> Option<(u32, u32)> {
    let (w, h) = (target.0.ceil().max(1.0), target.1.ceil().max(1.0));
    let shrinks = |src: u32, dst: f64| src as f64 > 2.0 * dst;
    (shrinks(source.0, w) || shrinks(source.1, h))
        .then(|| ((w as u32).min(source.0), (h as u32).min(source.1)))
}

/// Resizes straight-alpha 4-channel pixels with an area-averaging filter,
/// filtering in premultiplied alpha so transparent pixels don't bleed into
/// edges.
pub fn resize_straight(buffer: &image::RgbaImage, width: u32, height: u32) -> image::RgbaImage {
    let mut premultiplied = buffer.clone();
    for px in premultiplied.pixels_mut() {
        let a = px[3] as u32;
        for c in &mut px.0[..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    let mut out = image::imageops::resize(
        &premultiplied,
        width,
        height,
        image::imageops::FilterType::Triangle,
    );
    for px in out.pixels_mut() {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px.0[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

/// Uploadable image for `brush` drawn at `target` device pixels.
fn render_image(
    cache: &mut PaintCache,
    brush: &ImageBrush,
    target: (f64, f64),
) -> Option<Arc<RenderImage>> {
    let image = &brush.image;
    let upload =
        downscaled_size((image.width, image.height), target).unwrap_or((image.width, image.height));
    let key = (
        image.data.id(),
        image.width,
        image.height,
        upload.0,
        upload.1,
    );
    if let Some(found) = cache.images.get(&key) {
        return Some(found.clone());
    }
    let mut buffer =
        image::RgbaImage::from_raw(image.width, image.height, to_bgra_straight(brush))?;
    if upload != (image.width, image.height) {
        buffer = resize_straight(&buffer, upload.0, upload.1);
    }
    let frame = image::Frame::new(buffer);
    let rendered = Arc::new(RenderImage::new(smallvec::smallvec![frame]));
    cache.images.insert(key, rendered.clone());
    Some(rendered)
}

/// Maps a [`FaceSpec`] to a GPUI font, registering the font file with GPUI
/// when its family is not installed (embedded web fonts).
fn gpui_font(face: &FaceSpec, cache: &mut PaintCache, window: &mut Window) -> Font {
    let known = cache
        .known_families
        .get_or_insert_with(|| window.text_system().all_font_names().into_iter().collect());
    if !known.contains(&face.family) && cache.registered_fonts.insert(face.font.data.id()) {
        let bytes = face.font.data.data().to_vec();
        if window
            .text_system()
            .add_fonts(vec![Cow::Owned(bytes)])
            .is_ok()
        {
            known.insert(face.family.clone());
        }
    }
    Font {
        family: SharedString::from(face.family.clone()),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight(face.weight),
        style: if face.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

fn path_builder(path: &BezPath, origin: Point<Pixels>, mut builder: PathBuilder) -> PathBuilder {
    let p = |pt: kurbo::Point| point(origin.x + px(pt.x as f32), origin.y + px(pt.y as f32));
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(a) => builder.move_to(p(a)),
            PathEl::LineTo(a) => builder.line_to(p(a)),
            PathEl::QuadTo(c, a) => builder.curve_to(p(a), p(c)),
            PathEl::CurveTo(c1, c2, a) => builder.cubic_bezier_to(p(a), p(c1), p(c2)),
            PathEl::ClosePath => builder.close(),
        }
    }
    builder
}

fn paint_text(
    run: &TextRun,
    origin: Point<Pixels>,
    alpha: f32,
    cache: &mut PaintCache,
    window: &mut Window,
    cx: &mut App,
) {
    let font = gpui_font(&run.face, cache, window);
    let color = to_hsla(run.color, alpha);
    let font_size = px(run.font_size);
    let visible = window.content_mask().bounds;
    let baseline = origin.y + px(run.baseline);
    // Skip runs far outside the visible area: shaping is the costly part.
    if baseline + font_size * 2.0 < visible.top() || baseline - font_size * 2.0 > visible.bottom() {
        return;
    }
    let shape = |text: &str, window: &mut Window| {
        let text = SharedString::from(text.to_string());
        let runs = [GpuiTextRun {
            len: text.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
        window
            .text_system()
            .shape_line(text, font_size, &runs, None)
    };
    for word in &run.words {
        let shaped = shape(&word.text, window);
        // With line_height = ascent + descent GPUI puts the baseline at
        // origin.y + ascent, so this lands exactly on Blitz's baseline.
        let line_height = shaped.ascent + shaped.descent;
        let top = baseline - shaped.ascent;
        if word.clusters.len() > 1 && needs_cluster_placement(f32::from(shaped.width), word.width) {
            for &(start, end, x) in &word.clusters {
                if let Some(text) = word.text.get(start..end) {
                    let cluster = shape(text, window);
                    cluster
                        .paint(point(origin.x + px(x), top), line_height, window, cx)
                        .ok();
                }
            }
        } else {
            shaped
                .paint(point(origin.x + px(word.x), top), line_height, window, cx)
                .ok();
        }
    }
}

fn paint_fill(
    geometry: &Geometry,
    brush: &Brush,
    origin: Point<Pixels>,
    layer: LayerState,
    cache: &mut PaintCache,
    window: &mut Window,
) {
    let alpha = layer.alpha;
    let corner_radii = match geometry {
        Geometry::RoundedRect(_, r) => corners(r),
        // A fill touching a rounded clip's corners takes those corners.
        _ => match layer.rounded_clip {
            Some((clip, r)) => clip_corners(geometry.bounds(), clip, &r),
            None => Corners::default(),
        },
    };
    let background: gpui::Background = match brush {
        Brush::Solid(color) => to_hsla(*color, alpha).into(),
        Brush::Gradient {
            gradient,
            transform,
        } if crate::gradient::is_native(gradient) => {
            gradient_background(gradient, transform, alpha)
        }
        Brush::Gradient {
            gradient,
            transform,
        } => {
            paint_sampled_gradient(
                geometry.bounds(),
                corner_radii,
                gradient,
                transform,
                origin,
                cache,
                window,
            );
            return;
        }
        Brush::Image { image, transform } => {
            paint_image_fill(
                geometry.bounds(),
                corner_radii,
                image,
                transform,
                origin,
                layer,
                cache,
                window,
            );
            return;
        }
    };
    match geometry {
        Geometry::Rect(rect) | Geometry::RoundedRect(rect, _) => window.paint_quad(PaintQuad {
            bounds: to_bounds(*rect, origin),
            corner_radii,
            background,
            border_widths: Edges::default(),
            border_color: gpui::transparent_black(),
            border_style: BorderStyle::default(),
        }),
        Geometry::Path(path) => {
            if let Ok(path) = path_builder(path, origin, PathBuilder::fill()).build() {
                window.paint_path(path, background);
            }
        }
    }
}

/// Native GPUI background for a two-stop linear gradient.
fn gradient_background(
    gradient: &peniko::Gradient,
    transform: &Affine,
    alpha: f32,
) -> gpui::Background {
    let stops: Vec<(f32, Color)> = gradient
        .stops
        .iter()
        .map(|s| (s.offset, s.color.to_alpha_color()))
        .collect();
    let (Some(first), Some(last), GradientKind::Linear(line)) =
        (stops.first(), stops.last(), &gradient.kind)
    else {
        return gpui::transparent_black().into();
    };
    linear_gradient(
        css_angle(*transform * line.start, *transform * line.end),
        LinearColorStop {
            color: to_hsla(first.1, alpha),
            percentage: first.0,
        },
        LinearColorStop {
            color: to_hsla(last.1, alpha),
            percentage: last.0,
        },
    )
}

/// Largest side, in pixels, of a sampled gradient texture. Gradients are
/// smooth, so GPUI's bilinear sampling hides the upscale on large boxes.
const MAX_GRADIENT_TEXTURE: f64 = 512.0;

/// Pixel size of the texture sampled for a gradient covering `area`.
pub fn gradient_texture_size(area: Rect, scale_factor: f32) -> (u32, u32) {
    let scale = scale_factor as f64;
    let (w, h) = (area.width() * scale, area.height() * scale);
    let shrink = (MAX_GRADIENT_TEXTURE / w.max(h)).min(1.0);
    (
        ((w * shrink).ceil() as u32).max(1),
        ((h * shrink).ceil() as u32).max(1),
    )
}

/// Draws a gradient GPUI has no primitive for as a sampled texture.
fn paint_sampled_gradient(
    area: Rect,
    corner_radii: Corners<Pixels>,
    gradient: &peniko::Gradient,
    transform: &Affine,
    origin: Point<Pixels>,
    cache: &mut PaintCache,
    window: &mut Window,
) {
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return;
    }
    let (w, h) = gradient_texture_size(area, window.scale_factor());
    let key = (format!("{gradient:?}{transform:?}{area:?}"), w, h);
    let image = match cache.gradients.get(&key) {
        Some(image) => image.clone(),
        None => {
            let pixels = crate::gradient::rasterize(gradient, transform, area, w, h);
            let Some(buffer) = image::RgbaImage::from_raw(w, h, pixels) else {
                return;
            };
            let image = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                buffer
            )]));
            cache.gradients.insert(key, image.clone());
            image
        }
    };
    window
        .paint_image(to_bounds(area, origin), corner_radii, image, 0, false)
        .ok();
}

#[allow(clippy::too_many_arguments)]
fn paint_image_fill(
    area: Rect,
    corner_radii: Corners<Pixels>,
    brush: &ImageBrush,
    transform: &Affine,
    origin: Point<Pixels>,
    layer: LayerState,
    cache: &mut PaintCache,
    window: &mut Window,
) {
    let tile = transform.transform_rect_bbox(Rect::new(
        0.0,
        0.0,
        brush.image.width as f64,
        brush.image.height as f64,
    ));
    if tile.width() <= 0.0 || tile.height() <= 0.0 {
        return;
    }
    let scale = window.scale_factor() as f64;
    let Some(image) = render_image(cache, brush, (tile.width() * scale, tile.height() * scale))
    else {
        return;
    };
    // GPUI images have no opacity input in 0.2.2; layer alpha is not applied.
    let repeat_x = brush.sampler.x_extend == Extend::Repeat;
    let repeat_y = brush.sampler.y_extend == Extend::Repeat;
    let xs = tile_starts(tile.x0, tile.width(), area.x0, area.x1, repeat_x);
    let ys = tile_starts(tile.y0, tile.height(), area.y0, area.y1, repeat_y);
    window.with_content_mask(
        Some(ContentMask {
            bounds: to_bounds(area, origin),
        }),
        |window| {
            for &y in &ys {
                for &x in &xs {
                    let rect = Rect::new(x, y, x + tile.width(), y + tile.height());
                    // Rounded corners apply when the image covers the box (or the
                    // rounded clip it is drawn into, e.g. `<img>` with radius).
                    let radii = if same_box(rect, area) {
                        corner_radii
                    } else {
                        match layer.rounded_clip {
                            Some((clip, r)) => clip_corners(rect, clip, &r),
                            None => Corners::default(),
                        }
                    };
                    window
                        .paint_image(
                            to_bounds(rect, origin),
                            radii,
                            image.clone(),
                            0,
                            layer.grayscale,
                        )
                        .ok();
                }
            }
        },
    );
}

/// Start offsets of the tiles needed to cover `[from, to)`.
pub fn tile_starts(start: f64, len: f64, from: f64, to: f64, repeat: bool) -> Vec<f64> {
    if !repeat {
        return vec![start];
    }
    let mut x = start - ((start - from) / len).ceil().max(0.0) * len;
    let mut out = Vec::new();
    while x < to && out.len() < 512 {
        out.push(x);
        x += len;
    }
    out
}

fn paint_commands(
    commands: &[Command],
    index: &mut usize,
    origin: Point<Pixels>,
    layer: LayerState,
    cache: &mut PaintCache,
    window: &mut Window,
    cx: &mut App,
) {
    while *index < commands.len() {
        let command = &commands[*index];
        *index += 1;
        match command {
            Command::PushLayer {
                clip,
                radii,
                alpha,
                grayscale,
            } => {
                let inner = LayerState {
                    alpha: layer.alpha * alpha,
                    rounded_clip: radii.map(|r| (*clip, r)).or(layer.rounded_clip),
                    grayscale: layer.grayscale || *grayscale,
                };
                let mask = ContentMask {
                    bounds: to_bounds(*clip, origin),
                };
                window.with_content_mask(Some(mask), |window| {
                    paint_commands(commands, index, origin, inner, cache, window, cx)
                });
            }
            Command::Pop => return,
            Command::Fill { geometry, brush } => {
                paint_fill(geometry, brush, origin, layer, cache, window)
            }
            Command::Stroke { path, width, brush } => {
                let color = match brush {
                    Brush::Solid(color) => to_hsla(*color, layer.alpha),
                    _ => continue,
                };
                if let Ok(path) =
                    path_builder(path, origin, PathBuilder::stroke(px(*width as f32))).build()
                {
                    window.paint_path(path, color);
                }
            }
            Command::BoxShadow {
                rect,
                color,
                radius,
                std_dev,
            } => {
                let r = px(*radius as f32);
                window.paint_shadows(
                    to_bounds(*rect, origin),
                    Corners::all(r),
                    &[BoxShadow {
                        color: to_hsla(*color, layer.alpha),
                        offset: point(px(0.), px(0.)),
                        // GPUI's shadow shader uses sigma = blur_radius / 2.
                        blur_radius: px(*std_dev as f32 * 2.0),
                        spread_radius: px(0.),
                    }],
                );
            }
            Command::Text(run) => paint_text(run, origin, layer.alpha, cache, window, cx),
            Command::ColorGlyphs(run) => paint_color_glyphs(run, origin, layer, cache, window),
            // Glyph-only runs come from form-control editors, which mail never
            // keeps (sanitised away); nothing to draw them with yet.
            Command::Glyphs(_) => {}
        }
    }
}

/// Paints `list` with its document origin at `origin` (window coordinates).
pub fn paint_display_list(
    list: &DisplayList,
    origin: Point<Pixels>,
    cache: &mut PaintCache,
    window: &mut Window,
    cx: &mut App,
) {
    let mut index = 0;
    while index < list.commands.len() {
        // A stray Pop at top level is skipped instead of ending the replay.
        paint_commands(
            &list.commands,
            &mut index,
            origin,
            LayerState::default(),
            cache,
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn downscales_only_large_reductions() {
        assert_eq!(downscaled_size((1180, 2048), (16.0, 26.0)), Some((16, 26)));
        assert_eq!(downscaled_size((600, 300), (400.0, 200.0)), None);
        // Never upscales a side when only the other one shrinks a lot.
        assert_eq!(downscaled_size((1000, 10), (100.0, 20.0)), Some((100, 10)));
        assert_eq!(downscaled_size((100, 100), (0.2, 0.2)), Some((1, 1)));
    }

    #[test]
    fn resizing_averages_and_ignores_transparent_colour() {
        // Opaque white next to transparent black averages to half-transparent
        // white, not grey.
        let mut buffer = image::RgbaImage::new(2, 1);
        buffer.put_pixel(0, 0, image::Rgba([255, 255, 255, 255]));
        buffer.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        let out = resize_straight(&buffer, 1, 1);
        let px = out.get_pixel(0, 0);
        assert!(px[0] >= 250 && (120..=136).contains(&px[3]), "{px:?}");
    }

    use super::*;
    use peniko::{Blob, ImageData, ImageSampler};

    #[test]
    fn css_angles_follow_css_convention() {
        let o = kurbo::Point::new(0.0, 0.0);
        assert_eq!(css_angle(o, kurbo::Point::new(0.0, -1.0)), 0.0); // to top
        assert_eq!(css_angle(o, kurbo::Point::new(1.0, 0.0)), 90.0); // to right
        assert_eq!(css_angle(o, kurbo::Point::new(0.0, 1.0)), 180.0); // to bottom
        assert_eq!(css_angle(o, kurbo::Point::new(-1.0, 0.0)), 270.0); // to left
    }

    #[test]
    fn colors_convert_with_layer_alpha() {
        let c = to_hsla(Color::new([1.0, 0.0, 0.0, 0.5]), 0.5);
        assert!((c.a - 0.25).abs() < 1e-6);
        assert!(c.l > 0.4 && c.l < 0.6);
    }

    #[test]
    fn color_glyphs_scale_from_strike_to_font_size() {
        // 136px-tall bitmap in a 109ppem strike, top-left origin, 101 above baseline.
        let (left, top, w, h) = color_glyph_rect(
            10.0,
            100.0,
            109.0 / 2.0,
            109.0,
            0.0,
            101.0,
            136.0,
            128.0,
            false,
        );
        assert_eq!((left, top, w, h), (10.0, 49.5, 68.0, 64.0));
        // sbix-style bottom-left origin.
        let (_, top, _, _) = color_glyph_rect(0.0, 100.0, 10.0, 10.0, 0.0, -2.0, 10.0, 10.0, true);
        assert_eq!(top, 92.0);
    }

    #[test]
    fn cluster_placement_only_on_real_mismatch() {
        assert!(!needs_cluster_placement(100.4, 100.0));
        assert!(!needs_cluster_placement(101.5, 100.0)); // within 2%
        assert!(needs_cluster_placement(110.0, 100.0)); // letter-spacing
        assert!(needs_cluster_placement(12.0, 10.0)); // > 0.75px on short words
    }

    #[test]
    fn clip_corners_apply_only_where_corners_meet() {
        let clip = Rect::new(0.0, 0.0, 300.0, 400.0);
        let r = RoundedRectRadii::new(7.0, 7.0, 7.0, 7.0);
        // An image across the top of the card: rounded top corners only.
        let top = clip_corners(Rect::new(0.0, 0.0, 300.0, 150.0), clip, &r);
        assert_eq!(
            (
                top.top_left,
                top.top_right,
                top.bottom_right,
                top.bottom_left
            ),
            (px(7.), px(7.), px(0.), px(0.))
        );
        let inner = clip_corners(Rect::new(10.0, 10.0, 290.0, 390.0), clip, &r);
        assert_eq!(inner, Corners::default());
        let full = clip_corners(clip, clip, &r);
        assert_eq!(full, corners(&r));
    }

    #[test]
    fn same_box_tolerates_subpixel_offsets() {
        assert!(same_box(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(0.2, 0.0, 10.3, 10.0)
        ));
        assert!(!same_box(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(1.0, 0.0, 10.0, 10.0)
        ));
    }

    #[test]
    fn gradient_textures_are_capped() {
        assert_eq!(
            gradient_texture_size(Rect::new(0.0, 0.0, 100.0, 50.0), 2.0),
            (200, 100)
        );
        assert_eq!(
            gradient_texture_size(Rect::new(0.0, 0.0, 2000.0, 1000.0), 1.0),
            (512, 256)
        );
    }

    #[test]
    fn tiles_cover_area_only_when_repeating() {
        assert_eq!(tile_starts(10.0, 5.0, 0.0, 20.0, false), vec![10.0]);
        assert_eq!(
            tile_starts(10.0, 5.0, 0.0, 20.0, true),
            vec![0.0, 5.0, 10.0, 15.0]
        );
    }

    fn brush(format: ImageFormat, alpha_type: ImageAlphaType, px: [u8; 4]) -> ImageBrush {
        ImageBrush {
            image: ImageData {
                data: Blob::new(Arc::new(px.to_vec())),
                format,
                alpha_type,
                width: 1,
                height: 1,
            },
            sampler: ImageSampler::default(),
        }
    }

    #[test]
    fn rgba_images_become_bgra() {
        let b = brush(ImageFormat::Rgba8, ImageAlphaType::Alpha, [1, 2, 3, 255]);
        assert_eq!(to_bgra_straight(&b), vec![3, 2, 1, 255]);
    }

    #[test]
    fn premultiplied_images_are_unpremultiplied() {
        let b = brush(
            ImageFormat::Bgra8,
            ImageAlphaType::AlphaPremultiplied,
            [50, 50, 50, 128],
        );
        assert_eq!(to_bgra_straight(&b), vec![100, 100, 100, 128]);
    }
}
