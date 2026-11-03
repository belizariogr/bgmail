//! A recorded, GPUI-agnostic list of drawing commands for one painted document.
//!
//! The vendored Blitz painter (`crate::blitz_paint`) pushes `anyrender`
//! commands into [`DisplayList`]. Geometry is normalised at record time into
//! document coordinates (CSS px, scale 1): axis-aligned rectangles stay
//! rectangles so they can be drawn as GPUI quads; everything else becomes a
//! path. Text arrives as words with their Blitz/Parley positions so GPUI can
//! shape and draw each word with its own text system (see [`TextRun`]).
//!
//! Keeping the list free of GPUI types makes it cacheable across frames and
//! unit-testable without a window.

use anyrender::{Glyph, NormalizedCoord, Paint, PaintRef, PaintScene, RenderContext};
use kurbo::{Affine, BezPath, PathEl, Point, Rect, RoundedRectRadii, Shape};
use peniko::{BlendMode, Color, Fill, FontData, Gradient, ImageBrush, StyleRef};
use std::collections::HashMap;
use std::sync::Arc;

/// Flattening tolerance (CSS px) used when a shape has to become a path.
const PATH_TOLERANCE: f64 = 0.1;

/// Most dashes of one stroked path; past it the path is drawn solid. A message sets the
/// length of its borders at will, and the dashes are split on the CPU.
const MAX_DASHES: f64 = 16384.0;

/// Most path elements, gradient stops, shadows and layers recorded for a document; what comes
/// after them isn't drawn. The limit of one line or border alone is multiplied by however many
/// a message has, and a real message is far below this. A command counts as at least
/// [`MIN_SHAPE_COST`].
const MAX_PATH_ELEMENTS: usize = 2 << 20;
const MIN_SHAPE_COST: usize = 4;

/// Geometry of a filled shape, in document coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Rect(Rect),
    RoundedRect(Rect, RoundedRectRadii),
    Path(BezPath),
}

impl Geometry {
    /// Bounding box of the geometry.
    pub fn bounds(&self) -> Rect {
        match self {
            Geometry::Rect(rect) | Geometry::RoundedRect(rect, _) => *rect,
            Geometry::Path(path) => path.bounding_box(),
        }
    }
}

/// How a shape is coloured.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    /// Gradient whose geometry is mapped to document space by `transform`.
    Gradient {
        gradient: Gradient,
        transform: Affine,
    },
    /// Image whose pixel space is mapped to document space by `transform`.
    Image {
        image: Arc<ImageBrush>,
        transform: Affine,
    },
}

/// One word (or other unbreakable chunk) positioned by Blitz.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    /// Left edge of the word, document coordinates.
    pub x: f32,
    /// Width Blitz gave the word (includes letter-spacing, justification).
    pub width: f32,
    /// Each cluster as (byte start, byte end) within `text` and its document x.
    /// Used to place clusters individually when GPUI's shaping disagrees with
    /// Blitz's width (letter-spacing, justification).
    pub clusters: Vec<(usize, usize, f32)>,
}

/// The resolved face Blitz used for a run, described so GPUI can resolve the
/// same face through its own font system.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceSpec {
    pub family: String,
    pub weight: f32,
    pub italic: bool,
    /// Raw font file, used to register the face with GPUI when the family is
    /// not installed system-wide (e.g. a WOFF web font embedded in the mail).
    pub font: FontData,
}

/// A run of text sharing one face, size and colour, split into words.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub face: FaceSpec,
    pub font_size: f32,
    pub color: Color,
    /// Baseline, document coordinates.
    pub baseline: f32,
    pub words: Vec<Word>,
}

/// Glyphs Blitz positioned without source text (e.g. form-control editors).
/// Kept so nothing silently disappears; drawn with GPUI's glyph painter.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    pub font: FontData,
    pub font_size: f32,
    pub color: Color,
    pub transform: Affine,
    pub glyphs: Vec<Glyph>,
}

/// Colour-bitmap glyphs (emoji) positioned by Blitz, document coordinates.
/// GPUI 0.2.2 draws CBDT emoji as monochrome masks on Linux, so these are
/// drawn from the font's own colour bitmaps instead (see `paint.rs`).
#[derive(Clone, Debug, PartialEq)]
pub struct ColorGlyphRun {
    pub font: FontData,
    pub font_size: f32,
    /// (glyph id, x, baseline y) per glyph.
    pub glyphs: Vec<(u32, f32, f32)>,
}

/// A drawing command, already in document coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Clip (and optionally fade) everything until the matching [`Command::Pop`].
    /// GPUI only clips to rectangles, so non-rectangular clips use their bounds.
    PushLayer {
        clip: Rect,
        /// Corner radii when the clip is a rounded box (GPUI masks are
        /// rectangular; images/fills covering the clip reuse these radii).
        radii: Option<RoundedRectRadii>,
        alpha: f32,
        /// `filter: grayscale(…)` on the layer (applied to images).
        grayscale: bool,
    },
    Pop,
    Fill {
        geometry: Geometry,
        brush: Brush,
    },
    Stroke {
        path: BezPath,
        width: f64,
        brush: Brush,
    },
    BoxShadow {
        rect: Rect,
        color: Color,
        radius: f64,
        std_dev: f64,
    },
    Text(TextRun),
    Glyphs(GlyphRun),
    ColorGlyphs(ColorGlyphRun),
}

/// Ordered drawing commands for a painted document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    pub commands: Vec<Command>,
    /// Animated images, by the id of their first frame's data (what the
    /// commands draw): replay shows the current frame instead.
    pub animations: HashMap<u64, Arc<blitz_dom::node::AnimationFrames>>,
    /// Path elements recorded so far (see [`MAX_PATH_ELEMENTS`]).
    path_elements: usize,
    /// Layers refused by the budget still open, whose `Pop`s must not reach the list.
    refused_layers: usize,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes a command of `elements` from the budget, if it still fits. Once one doesn't
    /// fit nothing does, so a document isn't drawn with holes further on.
    fn charge(&mut self, elements: usize) -> bool {
        let total = self
            .path_elements
            .saturating_add(elements.max(MIN_SHAPE_COST));
        let fits = total <= MAX_PATH_ELEMENTS;
        self.path_elements = if fits { total } else { MAX_PATH_ELEMENTS };
        fits
    }

    /// Notes that images drawing `first_frame` animate through `frames`.
    pub fn register_animation(
        &mut self,
        first_frame: u64,
        frames: Arc<blitz_dom::node::AnimationFrames>,
    ) {
        self.animations.insert(first_frame, frames);
    }

    /// Records colour-bitmap glyphs (emoji).
    pub fn push_color_glyphs(&mut self, run: ColorGlyphRun) {
        if !run.glyphs.is_empty() {
            self.commands.push(Command::ColorGlyphs(run));
        }
    }

    /// Records a text run (called by the vendored text painter in place of
    /// `draw_glyphs`, whenever the source text is known).
    pub fn push_text(&mut self, run: TextRun) {
        if !run.words.is_empty() && self.refused_layers == 0 {
            self.commands.push(Command::Text(run));
        }
    }
}

/// Whether `transform` keeps rectangles axis-aligned (no rotation or skew).
pub fn is_axis_aligned(transform: &Affine) -> bool {
    let [_, b, c, _, _, _] = transform.as_coeffs();
    b == 0.0 && c == 0.0
}

/// Maps `shape` through `transform` into the most specific [`Geometry`].
pub fn transform_geometry(shape: &impl Shape, transform: Affine) -> Geometry {
    if is_axis_aligned(&transform) {
        if let Some(rect) = shape.as_rect() {
            return Geometry::Rect(transform.transform_rect_bbox(rect));
        }
        if let Some(rounded) = shape.as_rounded_rect() {
            let [sx, _, _, sy, _, _] = transform.as_coeffs();
            // Radii scale with the (possibly non-uniform) transform; use the
            // smaller factor so corners never overshoot the box.
            let factor = sx.abs().min(sy.abs());
            let r = rounded.radii();
            let radii = RoundedRectRadii::new(
                r.top_left * factor,
                r.top_right * factor,
                r.bottom_right * factor,
                r.bottom_left * factor,
            );
            return Geometry::RoundedRect(transform.transform_rect_bbox(rounded.rect()), radii);
        }
    }
    let mut path = shape.into_path(PATH_TOLERANCE);
    path.apply_affine(transform);
    rounded_box_from_path(&path).unwrap_or(Geometry::Path(path))
}

/// Recognises a path that is an axis-aligned (rounded) rectangle, as Blitz emits
/// for box backgrounds with `border-radius`, so it can be drawn as a GPUI quad
/// with exact corners instead of a tessellated path.
///
/// A rounded box has every point (ends and control points of its arcs) on an
/// edge of its bounding box; the area check rejects other shapes with that
/// property (e.g. a triangle touching three edges).
pub fn rounded_box_from_path(path: &BezPath) -> Option<Geometry> {
    const EPS: f64 = 0.01;
    let bb = path.bounding_box();
    if bb.width() <= EPS || bb.height() <= EPS {
        return None;
    }
    let mut moves = 0;
    // All points (incl. control points) for the on-edge test; segment end
    // points only for measuring radii (arc control points sit inside them).
    let mut points = Vec::new();
    let mut ends = Vec::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(a) => {
                moves += 1;
                points.push(a);
                ends.push(a);
            }
            PathEl::LineTo(a) => {
                points.push(a);
                ends.push(a);
            }
            PathEl::QuadTo(a, b) => {
                points.extend([a, b]);
                ends.push(b);
            }
            PathEl::CurveTo(a, b, c) => {
                points.extend([a, b, c]);
                ends.push(c);
            }
            PathEl::ClosePath => {}
        }
    }
    if moves != 1 {
        return None;
    }
    let on_edge = |p: &Point| {
        (p.x - bb.x0).abs() < EPS
            || (p.x - bb.x1).abs() < EPS
            || (p.y - bb.y0).abs() < EPS
            || (p.y - bb.y1).abs() < EPS
    };
    if !points.iter().all(on_edge) {
        return None;
    }
    // Radius of each corner: distance from the corner to the first point along
    // the horizontal and vertical edges, averaged (GPUI corners are circular).
    let along = |edge_y: f64, edge_x: f64| {
        let rx = ends
            .iter()
            .filter(|p| (p.y - edge_y).abs() < EPS)
            .map(|p| (p.x - edge_x).abs())
            .fold(f64::INFINITY, f64::min);
        let ry = ends
            .iter()
            .filter(|p| (p.x - edge_x).abs() < EPS)
            .map(|p| (p.y - edge_y).abs())
            .fold(f64::INFINITY, f64::min);
        let r = (rx.min(bb.width()) + ry.min(bb.height())) / 2.0;
        if r.is_finite() {
            r
        } else {
            0.0
        }
    };
    let radii = RoundedRectRadii::new(
        along(bb.y0, bb.x0),
        along(bb.y0, bb.x1),
        along(bb.y1, bb.x1),
        along(bb.y1, bb.x0),
    );
    let corner_loss = (4.0 - std::f64::consts::PI)
        * (radii.top_left.powi(2)
            + radii.top_right.powi(2)
            + radii.bottom_right.powi(2)
            + radii.bottom_left.powi(2))
        / 4.0;
    let expected = bb.area() - corner_loss;
    // Blitz emits box paths without a final ClosePath; kurbo's area needs it.
    let mut closed = path.clone();
    if !matches!(closed.elements().last(), Some(PathEl::ClosePath)) {
        closed.close_path();
    }
    if (closed.area().abs() - expected).abs() > expected * 0.02 + 0.5 {
        return None;
    }
    if [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ]
    .iter()
    .all(|r| *r < EPS)
    {
        Some(Geometry::Rect(bb))
    } else {
        Some(Geometry::RoundedRect(bb, radii))
    }
}

/// Reads what GPUI can apply from a CSS filter: grayscale (images) and an
/// opacity factor. Other effects (blur, hue-rotate…) are ignored.
pub fn filter_effects(filter: &anyrender::Filter) -> (bool, f32) {
    let mut grayscale = false;
    let mut alpha = 1.0;
    for node in filter.nodes() {
        if let anyrender::filters::FilterEffect::ColorMatrix(m) = &node.effect {
            let m = m.0;
            let rgb_identity = (0..3).all(|row| {
                (0..5).all(|col| {
                    let expected = if row == col { 1.0 } else { 0.0 };
                    (m[row * 5 + col] - expected).abs() < 1e-3
                })
            });
            if rgb_identity {
                // opacity(): only the alpha row scales.
                alpha *= m[18];
            } else {
                // Rows of a full desaturation are equal; treat ≥50% grayscale
                // as grayscale (GPUI only has an on/off switch).
                let row = |r: usize| [m[r * 5], m[r * 5 + 1], m[r * 5 + 2]];
                let (r, g) = (row(0), row(1));
                let spread: f32 = r.iter().zip(g.iter()).map(|(a, b)| (a - b).abs()).sum();
                if spread < 0.5 {
                    grayscale = true;
                }
            }
        }
    }
    (grayscale, alpha)
}

/// Corner radii of a rounded clip geometry, if any.
fn layer_radii(geometry: &Geometry) -> Option<RoundedRectRadii> {
    match geometry {
        Geometry::RoundedRect(_, radii) => Some(*radii),
        _ => None,
    }
}

/// Converts an `anyrender` paint into an owned [`Brush`], folding the shape
/// and brush transforms into the brush's own transform.
fn convert_paint(paint: PaintRef<'_>, transform: Affine, brush_transform: Option<Affine>) -> Brush {
    let brush_transform = transform * brush_transform.unwrap_or(Affine::IDENTITY);
    match paint {
        Paint::Solid(color) => Brush::Solid(color),
        Paint::Gradient(gradient) => Brush::Gradient {
            gradient: gradient.clone(),
            transform: brush_transform,
        },
        Paint::Image(image) => Brush::Image {
            image: Arc::new(image.to_owned()),
            transform: brush_transform,
        },
        // Custom/resource paints are renderer-specific; Blitz only uses them for
        // custom widgets, which BGMail does not enable.
        Paint::Resource(_) | Paint::Custom(_) => Brush::Solid(Color::TRANSPARENT),
    }
}

/// Whether every coordinate of the geometry is finite. Degenerate CSS (zero
/// sizes divided, huge radii) can yield NaN, which GPUI's tessellator rejects.
pub fn is_finite_geometry(geometry: &Geometry) -> bool {
    let rect_ok = |r: &Rect| [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite());
    match geometry {
        Geometry::Rect(rect) => rect_ok(rect),
        Geometry::RoundedRect(rect, radii) => {
            rect_ok(rect)
                && [
                    radii.top_left,
                    radii.top_right,
                    radii.bottom_right,
                    radii.bottom_left,
                ]
                .iter()
                .all(|v| v.is_finite())
        }
        Geometry::Path(path) => is_finite_path(path),
    }
}

/// Whether every point of `path` is finite.
/// What a brush costs on top of its shape: each command keeps its own copy of the stops.
fn gradient_stops(brush: &Brush) -> usize {
    match brush {
        Brush::Gradient { gradient, .. } => gradient.stops.len(),
        Brush::Solid(_) | Brush::Image { .. } => 0,
    }
}

/// Whether `style`'s dashes on `path` are few enough to split. Written so NaN and infinite
/// lengths don't dash either: `kurbo::dash` wouldn't return from them.
fn should_dash(path: &BezPath, style: &kurbo::Stroke) -> bool {
    if style.dash_pattern.is_empty() {
        return false;
    }
    let period: f64 = style.dash_pattern.iter().sum();
    let dashes = path.perimeter(PATH_TOLERANCE) / period;
    period > 0.0 && dashes <= MAX_DASHES
}

pub fn is_finite_path(path: &BezPath) -> bool {
    let ok = |p: &Point| p.x.is_finite() && p.y.is_finite();
    path.elements().iter().all(|el| match el {
        PathEl::MoveTo(a) | PathEl::LineTo(a) => ok(a),
        PathEl::QuadTo(a, b) => ok(a) && ok(b),
        PathEl::CurveTo(a, b, c) => ok(a) && ok(b) && ok(c),
        PathEl::ClosePath => true,
    })
}

/// Whether a path is made only of straight segments (cheap check used by tests
/// and by the replayer to prefer polygons).
pub fn is_polygonal(path: &BezPath) -> bool {
    path.elements().iter().all(|el| {
        matches!(
            el,
            PathEl::MoveTo(_) | PathEl::LineTo(_) | PathEl::ClosePath
        )
    })
}

impl RenderContext for DisplayList {}

impl PaintScene for DisplayList {
    fn reset(&mut self) {
        self.commands.clear();
    }

    fn push_layer(
        &mut self,
        _fill: Fill,
        _blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<anyrender::Filter>>,
        _backdrop_filter: Option<Arc<anyrender::Filter>>,
    ) {
        // Blend modes have no GPUI equivalent and are rare in mail; the layer
        // still clips, fades and (for images) desaturates its content.
        let (grayscale, filter_alpha) = filter
            .as_deref()
            .map(filter_effects)
            .unwrap_or((false, 1.0));
        let geometry = transform_geometry(clip, transform);
        if self.refused_layers > 0 || !self.charge(0) {
            self.refused_layers += 1;
            return;
        }
        self.commands.push(Command::PushLayer {
            clip: geometry.bounds(),
            radii: layer_radii(&geometry),
            alpha: alpha * filter_alpha,
            grayscale,
        });
    }

    fn push_clip_layer(&mut self, _fill: Fill, transform: Affine, clip: &impl Shape) {
        let geometry = transform_geometry(clip, transform);
        if self.refused_layers > 0 || !self.charge(0) {
            self.refused_layers += 1;
            return;
        }
        self.commands.push(Command::PushLayer {
            clip: geometry.bounds(),
            radii: layer_radii(&geometry),
            alpha: 1.0,
            grayscale: false,
        });
    }

    fn pop_layer(&mut self) {
        if self.refused_layers > 0 {
            self.refused_layers -= 1;
            return;
        }
        self.commands.push(Command::Pop);
    }

    fn stroke<'a>(
        &mut self,
        style: &kurbo::Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let mut path = shape.into_path(PATH_TOLERANCE);
        if should_dash(&path, style) {
            // GPUI strokes solid paths; split the dashes on the CPU instead.
            path = BezPath::from_vec(
                kurbo::dash(path.iter(), style.dash_offset, &style.dash_pattern).collect(),
            );
        }
        path.apply_affine(transform);
        let brush = convert_paint(brush.into(), transform, brush_transform);
        if !is_finite_path(&path) || !self.charge(path.elements().len() + gradient_stops(&brush)) {
            return;
        }
        let [a, b, c, d, _, _] = transform.as_coeffs();
        // Stroke width scales with the transform's average linear factor.
        let scale = ((a * a + b * b).sqrt() + (c * c + d * d).sqrt()) / 2.0;
        self.commands.push(Command::Stroke {
            path,
            width: style.width * scale,
            brush,
        });
    }

    fn fill<'a>(
        &mut self,
        _style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let brush = convert_paint(brush.into(), transform, brush_transform);
        if matches!(brush, Brush::Solid(color) if color.components[3] == 0.0) {
            return;
        }
        let geometry = transform_geometry(shape, transform);
        let elements = match &geometry {
            Geometry::Path(path) => path.elements().len(),
            Geometry::Rect(_) | Geometry::RoundedRect(..) => 0,
        } + gradient_stops(&brush);
        if is_finite_geometry(&geometry) && self.charge(elements) {
            self.commands.push(Command::Fill { geometry, brush });
        }
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        _hint: bool,
        _normalized_coords: &'a [NormalizedCoord],
        _embolden: kurbo::Vec2,
        _style: impl Into<StyleRef<'a>>,
        brush: impl Into<PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        _glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        // Drawn without the clip of a refused layer, it could land anywhere.
        if self.refused_layers > 0 {
            return;
        }
        let color = match brush.into() {
            Paint::Solid(color) => color.multiply_alpha(brush_alpha),
            _ => Color::BLACK,
        };
        self.commands.push(Command::Glyphs(GlyphRun {
            font: font.clone(),
            font_size,
            color,
            transform,
            glyphs: glyphs.collect(),
        }));
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        if self.refused_layers > 0 || !self.charge(0) {
            return;
        }
        let rect = transform.transform_rect_bbox(rect);
        self.commands.push(Command::BoxShadow {
            rect,
            color: brush,
            radius,
            std_dev,
        });
    }
}

/// Translation part of an affine transform.
pub fn translation(transform: &Affine) -> Point {
    let [_, _, _, _, e, f] = transform.as_coeffs();
    Point::new(e, f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Circle, RoundedRect};

    #[test]
    fn rect_stays_rect_under_translate_and_scale() {
        let g = transform_geometry(
            &Rect::new(0.0, 0.0, 10.0, 5.0),
            Affine::translate((3.0, 4.0)) * Affine::scale(2.0),
        );
        assert_eq!(g, Geometry::Rect(Rect::new(3.0, 4.0, 23.0, 14.0)));
    }

    #[test]
    fn rounded_rect_keeps_scaled_radii() {
        let rr = RoundedRect::new(0.0, 0.0, 10.0, 10.0, 2.0);
        match transform_geometry(&rr, Affine::scale(2.0)) {
            Geometry::RoundedRect(rect, radii) => {
                assert_eq!(rect, Rect::new(0.0, 0.0, 20.0, 20.0));
                assert_eq!(radii.top_left, 4.0);
            }
            other => panic!("expected rounded rect, got {other:?}"),
        }
    }

    #[test]
    fn rotated_rect_becomes_path() {
        let g = transform_geometry(&Rect::new(0.0, 0.0, 10.0, 10.0), Affine::rotate(0.5));
        assert!(matches!(g, Geometry::Path(_)));
    }

    #[test]
    fn rounded_rect_paths_are_recognised() {
        let rr = RoundedRect::new(0.0, 0.0, 100.0, 50.0, 8.0);
        let path = rr.to_path(0.1);
        match rounded_box_from_path(&path) {
            Some(Geometry::RoundedRect(rect, radii)) => {
                let expected = Rect::new(0.0, 0.0, 100.0, 50.0);
                assert!(
                    (rect.x0 - expected.x0).abs() < 1e-6 && (rect.y0 - expected.y0).abs() < 1e-6
                );
                assert!(
                    (rect.x1 - expected.x1).abs() < 1e-6 && (rect.y1 - expected.y1).abs() < 1e-6
                );
                assert!((radii.top_left - 8.0).abs() < 0.5, "{radii:?}");
            }
            other => panic!("expected rounded rect, got {other:?}"),
        }
        assert_eq!(
            rounded_box_from_path(&Rect::new(1.0, 2.0, 3.0, 4.0).to_path(0.1)),
            Some(Geometry::Rect(Rect::new(1.0, 2.0, 3.0, 4.0)))
        );
    }

    #[test]
    fn blitz_rounded_box_path_is_recognised() {
        // Exact path Blitz emits for a 600x521 box with 16px radii (no ClosePath).
        let mut p = BezPath::new();
        p.move_to((100.0, 437.0));
        p.curve_to(
            (100.0, 428.1634440027073),
            (107.16344400270731, 421.0),
            (116.0, 421.0),
        );
        p.line_to((684.0, 421.0));
        p.curve_to(
            (692.8365559972926, 421.0),
            (700.0, 428.1634440027073),
            (700.0, 437.0),
        );
        p.line_to((700.0, 926.0));
        p.curve_to(
            (700.0, 934.8365559972926),
            (692.8365559972927, 942.0),
            (684.0, 942.0),
        );
        p.line_to((116.0, 942.0));
        p.curve_to(
            (107.16344400270731, 942.0),
            (100.0, 934.8365559972927),
            (100.0, 926.0),
        );
        assert!(
            matches!(rounded_box_from_path(&p), Some(Geometry::RoundedRect(_, r)) if (r.top_left - 16.0).abs() < 0.01)
        );
    }

    #[test]
    fn non_box_paths_stay_paths() {
        let mut triangle = BezPath::new();
        triangle.move_to((0.0, 0.0));
        triangle.line_to((10.0, 0.0));
        triangle.line_to((5.0, 10.0));
        triangle.close_path();
        assert_eq!(rounded_box_from_path(&triangle), None);
        let two = {
            let mut p = Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.1);
            p.extend(Rect::new(2.0, 0.0, 3.0, 1.0).to_path(0.1));
            p
        };
        assert_eq!(rounded_box_from_path(&two), None);
    }

    #[test]
    fn dashed_strokes_are_split() {
        let mut list = DisplayList::new();
        list.stroke(
            &kurbo::Stroke::new(1.0).with_dashes(0.0, [2.0, 2.0]),
            Affine::IDENTITY,
            Paint::Solid(Color::BLACK),
            None,
            &kurbo::Line::new((0.0, 0.0), (10.0, 0.0)),
        );
        match &list.commands[0] {
            Command::Stroke { path, .. } => {
                let moves = path
                    .elements()
                    .iter()
                    .filter(|e| matches!(e, PathEl::MoveTo(_)))
                    .count();
                assert!(moves >= 2, "{path:?}");
            }
            other => panic!("expected stroke, got {other:?}"),
        }
    }

    #[test]
    fn rounded_clip_layers_keep_radii() {
        let mut list = DisplayList::new();
        list.push_clip_layer(
            Fill::NonZero,
            Affine::IDENTITY,
            &RoundedRect::new(0.0, 0.0, 40.0, 40.0, 20.0),
        );
        assert!(matches!(
            list.commands[0],
            Command::PushLayer { radii: Some(r), .. } if (r.top_left - 20.0).abs() < 0.5
        ));
    }

    #[test]
    fn filter_effects_detect_grayscale_and_opacity() {
        use anyrender::{filters::FilterEffect, Filter};
        let gray = Filter::single(FilterEffect::grayscale(1.0));
        assert_eq!(filter_effects(&gray), (true, 1.0));
        let faded = Filter::single(FilterEffect::opacity(0.5));
        let (g, a) = filter_effects(&faded);
        assert!(!g && (a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn circles_become_fully_rounded_boxes() {
        // A circle is a box whose radius is half its side: GPUI draws it as a
        // quad, which is exact and cheaper than a tessellated path.
        match transform_geometry(&Circle::new((5.0, 5.0), 5.0), Affine::IDENTITY) {
            Geometry::RoundedRect(_, radii) => assert!((radii.top_left - 5.0).abs() < 0.01),
            other => panic!("expected rounded rect, got {other:?}"),
        }
    }

    #[test]
    fn curved_non_boxes_stay_curved_paths() {
        let mut blob = BezPath::new();
        blob.move_to((0.0, 0.0));
        blob.quad_to((10.0, 10.0), (20.0, 0.0));
        blob.line_to((20.0, 20.0));
        blob.close_path();
        match transform_geometry(&blob, Affine::IDENTITY) {
            Geometry::Path(path) => assert!(!is_polygonal(&path)),
            other => panic!("expected path, got {other:?}"),
        }
    }

    #[test]
    fn non_finite_geometry_is_dropped() {
        let mut list = DisplayList::new();
        list.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Paint::Solid(Color::BLACK),
            None,
            &kurbo::Line::new((0.0, 0.0), (f64::NAN, 1.0)),
        );
        list.stroke(
            &kurbo::Stroke::new(1.0),
            Affine::IDENTITY,
            Paint::Solid(Color::BLACK),
            None,
            &kurbo::Line::new((0.0, f64::INFINITY), (1.0, 1.0)),
        );
        assert!(list.commands.is_empty());
        assert!(!is_finite_geometry(&Geometry::Rect(Rect::new(
            0.0,
            0.0,
            f64::NAN,
            1.0
        ))));
        assert!(is_finite_geometry(&Geometry::Rect(Rect::new(
            0.0, 0.0, 1.0, 1.0
        ))));
    }

    #[test]
    fn transparent_fills_are_dropped() {
        let mut list = DisplayList::new();
        list.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Paint::Solid(Color::TRANSPARENT),
            None,
            &Rect::new(0.0, 0.0, 1.0, 1.0),
        );
        assert!(list.commands.is_empty());
    }

    #[test]
    fn layers_record_clip_bounds_and_alpha() {
        let mut list = DisplayList::new();
        list.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            0.5,
            Affine::translate((10.0, 0.0)),
            &Rect::new(0.0, 0.0, 5.0, 5.0),
            None,
            None,
        );
        list.pop_layer();
        assert_eq!(
            list.commands,
            vec![
                Command::PushLayer {
                    clip: Rect::new(10.0, 0.0, 15.0, 5.0),
                    radii: None,
                    alpha: 0.5,
                    grayscale: false,
                },
                Command::Pop
            ]
        );
    }

    #[test]
    fn stroke_width_follows_transform_scale() {
        let mut list = DisplayList::new();
        list.stroke(
            &kurbo::Stroke::new(2.0),
            Affine::scale(3.0),
            Paint::Solid(Color::BLACK),
            None,
            &kurbo::Line::new((0.0, 0.0), (1.0, 0.0)),
        );
        match &list.commands[0] {
            Command::Stroke { width, .. } => assert_eq!(*width, 6.0),
            other => panic!("expected stroke, got {other:?}"),
        }
    }

    #[test]
    fn box_shadow_rect_is_transformed() {
        let mut list = DisplayList::new();
        list.draw_box_shadow(
            Affine::translate((1.0, 2.0)),
            Rect::new(0.0, 0.0, 4.0, 4.0),
            Color::BLACK,
            2.0,
            3.0,
        );
        assert!(matches!(
            list.commands[0],
            Command::BoxShadow { rect, .. } if rect == Rect::new(1.0, 2.0, 5.0, 6.0)
        ));
    }

    #[test]
    fn empty_text_runs_are_skipped() {
        let mut list = DisplayList::new();
        list.push_text(TextRun {
            face: FaceSpec {
                family: "X".into(),
                weight: 400.0,
                italic: false,
                font: FontData::new(peniko::Blob::new(Arc::new(Vec::<u8>::new())), 0),
            },
            font_size: 12.0,
            color: Color::BLACK,
            baseline: 10.0,
            words: vec![],
        });
        assert!(list.commands.is_empty());
    }

    #[test]
    fn translation_reads_offsets() {
        assert_eq!(
            translation(&Affine::translate((7.0, 9.0))),
            Point::new(7.0, 9.0)
        );
    }
}
