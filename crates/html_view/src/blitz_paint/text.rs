use anyrender::PaintScene;
use blitz_dom::CopyStyleGetters;
use blitz_dom::{node::TextBrush, util::ToColorColor, BaseDocument, NodeId};
use kurbo::{Affine, BezPath, Cap, Circle, Rect, Stroke};
use parley::{Affinity, Cursor, Layout, Line, PositionedLayoutItem, Selection};
use peniko::Fill;
use std::collections::HashMap;
use style::properties::generated::longhands::text_decoration_style::computed_value::T as TextDecorationStyle;
use style::values::computed::{
    Length, LengthPercentage, TextDecorationLength, TextDecorationLine, TextUnderlinePosition,
};
use style::values::generics::text::{GenericTextDecorationInset, GenericTextDecorationLength};

use crate::blitz_paint::color::{Color, ToColorColor as _};
use crate::blitz_paint::FONT_EMBOLDEN_ENABLED;

/// Draw the backgrounds of inline elements (e.g. `<span style="background: ...">`).
///
/// Each glyph run carries the node id of the innermost inline element it belongs to
/// (via its brush). We look up that node's `background-color` and, if non-transparent,
/// fill a rectangle covering the run's advance and its font's ascent/descent so that the
/// background sits behind the text.
///
/// The inline root's own background is painted separately (as a normal block box), so
/// runs belonging to the root are skipped to avoid drawing it twice.
pub(crate) fn draw_inline_backgrounds<'a>(
    scene: &mut crate::display_list::DisplayList,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    inline_root_id: NodeId,
) {
    // BGMail: inline boxes paint their background over the padding box and draw
    // their borders and radius, like browsers. Vertical padding/borders apply to
    // every fragment; horizontal ones only to the element's first and last
    // fragment (box-decoration-break: slice). Consecutive runs of the same
    // element on a line form one fragment.
    struct Fragment {
        node_id: NodeId,
        line: usize,
        x0: f64,
        x1: f64,
        y0: f64,
        y1: f64,
    }
    let mut fragments: Vec<Fragment> = Vec::new();
    for (line_index, line) in lines.enumerate() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let node_id = glyph_run.style().brush.id;
            if node_id == inline_root_id {
                continue;
            }
            let metrics = glyph_run.run().font_metrics();
            let x = glyph_run.offset() as f64;
            let w = glyph_run.advance() as f64;
            let baseline = glyph_run.baseline() as f64;
            let (y0, y1) = (
                baseline - metrics.ascent as f64,
                baseline + metrics.descent as f64,
            );
            match fragments.last_mut() {
                Some(last)
                    if last.node_id == node_id
                        && last.line == line_index
                        && (last.x1 - x).abs() < 0.5 =>
                {
                    last.x1 = x + w;
                    last.y0 = last.y0.min(y0);
                    last.y1 = last.y1.max(y1);
                }
                _ => fragments.push(Fragment {
                    node_id,
                    line: line_index,
                    x0: x,
                    x1: x + w,
                    y0,
                    y1,
                }),
            }
        }
    }

    for (index, fragment) in fragments.iter().enumerate() {
        let Some(styles) = doc
            .get_node(fragment.node_id)
            .and_then(|node| node.primary_styles())
        else {
            continue;
        };
        let current_color = styles.clone_color();
        let bg_color = styles
            .get_background()
            .background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();
        let border = styles.get_border();
        let side = |width: style::values::computed::Au,
                    style: style::values::computed::BorderStyle| {
            if style.none_or_hidden() {
                0.0
            } else {
                width.to_f64_px()
            }
        };
        let bw = [
            side(border.border_top_width.0, border.border_top_style),
            side(border.border_right_width.0, border.border_right_style),
            side(border.border_bottom_width.0, border.border_bottom_style),
            side(border.border_left_width.0, border.border_left_style),
        ];
        if bg_color == Color::TRANSPARENT && bw.iter().all(|w| *w == 0.0) {
            continue;
        }
        let padding = styles.get_padding();
        let pad = |lp: &style::values::computed::LengthPercentage| {
            lp.resolve(style::values::computed::Length::new(0.0)).px() as f64
        };
        let first = !fragments[..index]
            .iter()
            .any(|f| f.node_id == fragment.node_id);
        let last = !fragments[index + 1..]
            .iter()
            .any(|f| f.node_id == fragment.node_id);
        let left = if first {
            pad(&padding.padding_left.0) + bw[3]
        } else {
            0.0
        };
        let right = if last {
            pad(&padding.padding_right.0) + bw[1]
        } else {
            0.0
        };
        let outer = Rect::new(
            fragment.x0 - left,
            fragment.y0 - pad(&padding.padding_top.0) - bw[0],
            fragment.x1 + right,
            fragment.y1 + pad(&padding.padding_bottom.0) + bw[2],
        );
        let radius = |r: &style::values::computed::BorderCornerRadius| {
            r.0.width
                .0
                .resolve(style::values::computed::Length::new(outer.width() as f32))
                .px() as f64
        };
        let radii = kurbo::RoundedRectRadii::new(
            if first {
                radius(&border.border_top_left_radius)
            } else {
                0.0
            },
            if last {
                radius(&border.border_top_right_radius)
            } else {
                0.0
            },
            if last {
                radius(&border.border_bottom_right_radius)
            } else {
                0.0
            },
            if first {
                radius(&border.border_bottom_left_radius)
            } else {
                0.0
            },
        );
        let inner = Rect::new(
            outer.x0 + if first { bw[3] } else { 0.0 },
            outer.y0 + bw[0],
            outer.x1 - if last { bw[1] } else { 0.0 },
            outer.y1 - bw[2],
        );
        let inner_radii = kurbo::RoundedRectRadii::new(
            (radii.top_left - bw[0].max(bw[3])).max(0.0),
            (radii.top_right - bw[0].max(bw[1])).max(0.0),
            (radii.bottom_right - bw[2].max(bw[1])).max(0.0),
            (radii.bottom_left - bw[2].max(bw[3])).max(0.0),
        );
        if bg_color != Color::TRANSPARENT {
            scene.fill(
                Fill::NonZero,
                transform,
                bg_color,
                None,
                &kurbo::RoundedRect::from_rect(inner, inner_radii),
            );
        }
        if bw.iter().any(|w| *w > 0.0) {
            // Border color: the top side's (mail buttons use one colour).
            let border_color = border
                .border_top_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            // The border is the ring between the outer and inner boxes, so
            // the background (or the page) shows inside it.
            let ring = inline_border_ring(outer, radii, inner, inner_radii);
            scene.fill(Fill::NonZero, transform, border_color, None, &ring);
        }
    }
}

/// BGMail: the area between an inline box's outer (border) and inner
/// (padding) boxes: the outer outline plus the reversed inner one, so a
/// non-zero fill leaves the inside empty.
pub(crate) fn inline_border_ring(
    outer: Rect,
    radii: kurbo::RoundedRectRadii,
    inner: Rect,
    inner_radii: kurbo::RoundedRectRadii,
) -> kurbo::BezPath {
    use kurbo::Shape as _;
    let mut ring = kurbo::RoundedRect::from_rect(outer, radii).to_path(0.1);
    if inner.width() > 0.0 && inner.height() > 0.0 {
        let hole = kurbo::RoundedRect::from_rect(inner, inner_radii)
            .to_path(0.1)
            .reverse_subpaths();
        ring.extend(hole);
    }
    ring
}

#[cfg(test)]
mod inline_border_tests {
    use super::*;

    #[test]
    fn border_rings_leave_the_inside_empty() {
        let outer = Rect::new(0.0, 0.0, 100.0, 20.0);
        let inner = Rect::new(0.0, 0.0, 100.0, 19.0);
        let ring = inline_border_ring(outer, Default::default(), inner, Default::default());
        use kurbo::Shape as _;
        // Inside the padding box: not covered; on the bottom border: covered.
        assert_eq!(ring.winding(kurbo::Point::new(50.0, 10.0)), 0);
        assert_ne!(ring.winding(kurbo::Point::new(50.0, 19.5)), 0);
    }
}

/// Per-font-face cache of the OS/2 `usWinAscent / unitsPerEm` ratio, keyed by the font
/// blob's unique id and face index. `None` is cached for fonts without a usable OS/2 or
/// head table so they aren't re-parsed either.
type WinAscentCache = HashMap<(u64, u32), Option<f32>>;

/// The font's OS/2 `usWinAscent`, in the same (device) pixels as `font_size`.
///
/// This is the ascent browsers use as the top of the "em box" when positioning overlines. It
/// is typically taller than the hhea ascent Parley exposes via [`parley::layout::run::RunMetrics`].
/// Returns `None` when the font has no OS/2 table.
fn win_ascent(cache: &mut WinAscentCache, font: &parley::FontData, font_size: f32) -> Option<f32> {
    let ratio = *cache
        .entry((font.data.id(), font.index))
        .or_insert_with(|| win_ascent_ratio(font));
    Some(ratio? * font_size)
}

/// The unitless `usWinAscent / unitsPerEm` ratio for a font face, or `None` when the font
/// has no usable head/OS/2 table.
fn win_ascent_ratio(font: &parley::FontData) -> Option<f32> {
    use skrifa::raw::{FontRef, TableProvider as _};
    let font_ref = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let units_per_em = font_ref.head().ok()?.units_per_em();
    let win_ascent = font_ref.os2().ok()?.us_win_ascent();
    Some(win_ascent as f32 / units_per_em as f32)
}

/// Mirrors Blink's `SelectBestDashGap`: choose the gap length (as close as possible to
/// `gap_length`) that fits a whole number of `dash_length` dashes across `stroke_length`,
/// so dashes are evenly distributed and a dash lands at each end of the line.
fn select_best_dash_gap(stroke_length: f64, dash_length: f64, gap_length: f64) -> f64 {
    let available_length = stroke_length + gap_length;
    let min_num_dashes = (available_length / (dash_length + gap_length)).floor();
    let max_num_dashes = min_num_dashes + 1.0;
    let min_num_gaps = min_num_dashes - 1.0;
    let max_num_gaps = max_num_dashes - 1.0;
    if min_num_gaps < 1.0 {
        return gap_length;
    }
    let min_gap = (stroke_length - min_num_dashes * dash_length) / min_num_gaps;
    let max_gap = (stroke_length - max_num_dashes * dash_length) / max_num_gaps;
    if max_gap <= 0.0 || (min_gap - gap_length).abs() < (max_gap - gap_length).abs() {
        min_gap
    } else {
        max_gap
    }
}

/// A text decoration resolved from a single "decorating box" (an element that
/// sets `text-decoration-line`), ready to be drawn over a glyph run. Because the
/// `text-decoration-*` properties do not inherit, decorations are propagated from
/// ancestors rather than inherited, so a run may be covered by several of these.
///
/// Only the run-independent (style-derived) values are stored here so an instance
/// can be cached on the ancestor stack and reused across every run within the same
/// decorating box. The `text-decoration-inset` and `text-underline-offset` values
/// depend on the run's advance/font-size and are resolved lazily when drawing.
#[derive(Clone)]
struct ResolvedDecoration {
    line: TextDecorationLine,
    style: TextDecorationStyle,
    color: Color,
    thickness: TextDecorationLength,
    /// `text-underline-offset`; `None` for `auto`. Resolved per run.
    underline_offset: Option<LengthPercentage>,
    /// Whether `text-underline-position: under` is in effect.
    underline_under: bool,
    /// Whether `text-underline-position: from-font` is in effect.
    underline_from_font: bool,
    /// `text-decoration-inset`. Resolved per run.
    inset: GenericTextDecorationInset<LengthPercentage>,
}

/// An element on the current ancestor path (inline root -> run node), caching the
/// values resolved from its computed style so descending into the same node across
/// consecutive runs doesn't re-resolve them.
struct DecorationStackEntry {
    node_id: NodeId,
    /// This node's (inherited) text fill colour, used for the glyphs of runs whose
    /// innermost node is this one.
    fill_color: Color,
    /// The decoration this node introduces as a decorating box, if any.
    decoration: Option<ResolvedDecoration>,
}

/// Resolve the cached style values for a single node into a [`DecorationStackEntry`].
fn resolve_decoration_entry(doc: &BaseDocument, node_id: NodeId) -> DecorationStackEntry {
    let Some(styles) = doc.get_node(node_id).and_then(|node| node.primary_styles()) else {
        return DecorationStackEntry {
            node_id,
            fill_color: Color::BLACK,
            decoration: None,
        };
    };

    let itext = styles.get_inherited_text();
    let text = styles.get_text();
    let text_color = itext.color.as_color_color();
    let fill_color = itext
        ._webkit_text_fill_color
        .resolve_to_absolute(&itext.color)
        .as_srgb_color();

    let drawn_lines = TextDecorationLine::UNDERLINE
        | TextDecorationLine::OVERLINE
        | TextDecorationLine::LINE_THROUGH;
    let line = text.text_decoration_line;
    // Decorations propagate through the box tree, and a `display: contents` element
    // generates no box, so its decorations have no effect on descendants.
    let is_contents = styles.clone_display().is_contents();
    let decoration = (!is_contents && line.intersects(drawn_lines)).then(|| {
        // `text-decoration-color: currentColor` (the initial value) resolves against
        // the decorating box's own colour, not the descendant run's.
        let color = text
            .text_decoration_color
            .as_absolute()
            .map(ToColorColor::as_color_color)
            .unwrap_or(text_color);

        ResolvedDecoration {
            line,
            style: text.text_decoration_style,
            color,
            thickness: text.text_decoration_thickness.clone(),
            underline_offset: itext.text_underline_offset.non_auto(),
            underline_under: itext
                .text_underline_position
                .contains(TextUnderlinePosition::UNDER),
            underline_from_font: itext
                .text_underline_position
                .contains(TextUnderlinePosition::FROM_FONT),
            inset: text.text_decoration_inset.clone(),
        }
    });

    DecorationStackEntry {
        node_id,
        fill_color,
        decoration,
    }
}

/// The run-dependent geometry used to position and size a decoration, captured from a
/// single glyph run. Firefox draws a decoration once for the whole decorating box using
/// the box's own font, so we prefer the values from a run whose innermost node *is* the
/// decorating box (its own text), falling back to the first run it covers.
#[derive(Clone)]
struct DecorationRunGeometry {
    /// The baseline the decoration is positioned from (the run's own, `vertical-align`
    /// shifted, baseline).
    baseline: f32,
    ascent: f32,
    descent: f32,
    underline_offset: f32,
    underline_size: f32,
    strikethrough_size: f32,
    font: parley::FontData,
    font_size: f32,
    css_font_size: f64,
}

/// A decoration accumulated across the runs of a single line for one decorating box.
///
/// The `text-decoration-*` properties describe the whole box, so we gather the horizontal
/// extent it covers on the line (`min_x`/`max_x`) and the font geometry to draw with, then
/// paint a single line spanning the box rather than one segment per run.
struct LineDecoration {
    node_id: NodeId,
    deco: ResolvedDecoration,
    min_x: f64,
    max_x: f64,
    /// Geometry from a run whose innermost node is the decorating box itself.
    own: Option<DecorationRunGeometry>,
    /// Geometry from the first run the box covers (fallback when it has no own text on
    /// this line, e.g. it only wraps differently-sized descendants).
    first: Option<DecorationRunGeometry>,
}

/// Reusable scratch storage for drawing text across all inline formatting contexts in a
/// document. The vectors are cleared between uses without releasing their allocations.
#[derive(Default)]
pub(crate) struct DrawTextContext {
    /// Face metadata (family/weight/style) per font file, for GPUI text runs.
    pub(crate) faces: crate::text::FaceCache,
    stack: Vec<DecorationStackEntry>,
    path_scratch: Vec<NodeId>,
    deco_boxes: Vec<LineDecoration>,
    win_ascent_ratios: WinAscentCache,
}

/// Resolve the CSS `text-decoration-thickness` to a device-pixel size.
///
/// - Percentages are resolved against the font size,
/// - `from-font` uses the metrics from Parley,
/// - `auto` uses a thickness of `font-size / 10` (minimum: 1px).
///
/// The result is floored to a whole number of device pixels (minimum 1), matching
/// browsers, which snap decoration thickness to whole pixels.
fn decoration_size(
    thickness: &TextDecorationLength,
    metric_size: f32,
    css_font_size: f64,
    scale: f64,
) -> f32 {
    (match thickness {
        GenericTextDecorationLength::LengthPercentage(lp) => {
            lp.resolve(Length::new(css_font_size as f32)).px() as f64 * scale
        }
        GenericTextDecorationLength::FromFont => metric_size as f64,
        GenericTextDecorationLength::Auto => (css_font_size / 10.0).max(1.0) * scale,
    })
    .floor()
    .max(1.0) as f32
}

/// BGMail: the most marks (dots, dashes, half waves) of one decoration line.
const MAX_DECORATION_MARKS: f64 = 16384.0;

/// Draws a single decoration line of the given `text-decoration-style` spanning
/// `[x0, x0 + width]`. `baseline` is the line's baseline and `offset` positions the line
/// relative to it (`y = baseline - offset + size / 2`, growing downward). Geometry is built
/// explicitly (rather than relying on backend dash support) so it renders consistently
/// across renderers.
///
/// `double_dir` points away from the text (`+1` = down, `-1` = up) and is used to place the
/// second line of a `double` decoration.
#[allow(clippy::too_many_arguments)]
fn draw_decoration_line(
    scene: &mut crate::display_list::DisplayList,
    transform: Affine,
    scale: f64,
    x0: f64,
    width: f64,
    baseline: f32,
    offset: f32,
    size: f32,
    brush: &anyrender::Paint,
    double_dir: f64,
    deco_style: TextDecorationStyle,
    inset_start: f64,
    inset_end: f64,
) {
    // Inset the line from the box's start/end edges (assumes LTR: start is the left edge).
    // Negative insets extend the line past the text.
    let x = x0 + inset_start;
    let w = width - inset_start - inset_end;
    if w <= 0.0 {
        return;
    }
    let y = (baseline - offset + size / 2.0) as f64;
    let size = size as f64;
    let butt_stroke = Stroke::new(size).with_caps(Cap::Butt);
    // BGMail: the marks of a line are geometry in proportion to its width, which a message
    // sets at will; a line no one can see as marks is drawn solid. (`size` is at least 1, so
    // no style has more marks than this.)
    let deco_style = match deco_style {
        TextDecorationStyle::Dotted | TextDecorationStyle::Dashed | TextDecorationStyle::Wavy
            if w.is_nan() || w / size > MAX_DECORATION_MARKS =>
        {
            TextDecorationStyle::Solid
        }
        style => style,
    };

    match deco_style {
        TextDecorationStyle::None => {
            // no line. Equivalent to `text-decoration-line: none`
        }
        // `solid`
        TextDecorationStyle::Solid => {
            let line = kurbo::Line::new((x, y), (x + w, y));
            scene.stroke(&butt_stroke, transform, brush, None, &line);
        }
        // Two lines, each `size` thick, separated by a 1px (CSS) gap. This
        // matches Chrome, where the second line is offset by `thickness + 1px`
        // and sits further from the text (below for underline, above for
        // overline).
        TextDecorationStyle::Double => {
            let one_css_px = scale;
            for cy in [y, y + double_dir * (size + one_css_px)] {
                let line = kurbo::Line::new((x, cy), (x + w, cy));
                scene.stroke(&butt_stroke, transform, brush, None, &line);
            }
        }
        // Round dots (diameter = line thickness) spaced one dot apart.
        TextDecorationStyle::Dotted => {
            let radius = size / 2.0;
            let step = 2.0 * size;
            let dots = ((w - 2.0 * radius) / step).floor() + 1.0;
            for i in 0..dots.max(0.0) as usize {
                let dot = Circle::new((x + radius + i as f64 * step, y), radius);
                scene.fill(Fill::NonZero, transform, brush, None, &dot);
            }
        }
        // Dashes as filled rectangles. Dash/gap lengths are relative to the
        // thickness (matching Blink): thinner lines use proportionally longer
        // dashes and gaps so they don't read as dotted or solid.
        TextDecorationStyle::Dashed => {
            let (dash_ratio, gap_ratio) = if size >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) };
            let dash = size * dash_ratio;
            let nominal_gap = size * gap_ratio;
            // Nudge the gap so a whole number of dashes fits the run evenly.
            let gap = if w > 2.0 * dash {
                select_best_dash_gap(w, dash, nominal_gap)
            } else {
                nominal_gap
            };
            for i in 0..(w / (dash + gap)).ceil() as usize {
                let dx = x + i as f64 * (dash + gap);
                let end = (dx + dash).min(x + w);
                let rect = Rect::new(dx, y - size / 2.0, end, y + size / 2.0);
                scene.fill(Fill::NonZero, transform, brush, None, &rect);
            }
        }
        // A squiggle built from alternating quadratic Béziers.
        TextDecorationStyle::Wavy => {
            let amplitude = size;
            let half_wave = (2.0 * size).max(1.0);
            let mut path = BezPath::new();
            path.move_to((x, y));
            let mut up = true;
            for i in 0..(w / half_wave).ceil() as usize {
                let px = x + i as f64 * half_wave;
                let nx = (px + half_wave).min(x + w);
                // A quad Bézier reaches half the control-point offset at its
                // midpoint, so use `2 * amplitude` to hit the target peak.
                let ctrl_y = if up {
                    y - 2.0 * amplitude
                } else {
                    y + 2.0 * amplitude
                };
                path.quad_to(((px + nx) / 2.0, ctrl_y), (nx, y));
                up = !up;
            }
            let stroke = Stroke::new(size).with_caps(Cap::Round);
            scene.stroke(&stroke, transform, brush, None, &path);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TextPaintOptions {
    pub scale: f64,
    pub brush_override: Option<Color>,
}

/// Paint the decorations accumulated for one line, one line per decorating box.
fn flush_line_decorations(
    scene: &mut crate::display_list::DisplayList,
    transform: Affine,
    options: TextPaintOptions,
    deco_boxes: &[LineDecoration],
    win_ascent_ratios: &mut WinAscentCache,
    inline_root_id: NodeId,
    line_baseline: f32,
) {
    let TextPaintOptions {
        scale,
        brush_override,
    } = options;
    // Draw innermost boxes first so ancestors' decorations paint on top, matching the
    // per-run drawing order this replaced (`stack.iter().rev()`).
    for acc in deco_boxes.iter().rev() {
        let deco = &acc.deco;
        // Prefer the decorating box's own font; fall back to the first run it covers.
        let geom = match (&acc.own, &acc.first) {
            (Some(own), _) => own.clone(),
            (None, Some(first)) => {
                let mut geom = first.clone();
                // A descendant run's baseline may be shifted by `vertical-align`, but the
                // decoration is positioned from the decorating box's own baseline. For the
                // inline root that is the line's baseline.
                if acc.node_id == inline_root_id {
                    geom.baseline = line_baseline;
                }
                geom
            }
            (None, None) => continue,
        };
        let geom = &geom;
        let width = acc.max_x - acc.min_x;
        if width <= 0.0 {
            continue;
        }
        let brush = anyrender::Paint::from(brush_override.unwrap_or(deco.color));

        // `text-decoration-inset` shortens (or, when negative, extends) the line from the
        // inline-start/end edges. Percentages resolve against the decoration line length
        // (the box's total advance on this line); `auto` is no inset.
        let (inset_start, inset_end) = match &deco.inset {
            GenericTextDecorationInset::LengthPercentage { start, end } => {
                // The extent is in device pixels; convert to CSS pixels so the resolved
                // result can be scaled back.
                let line_length = Length::new((width / scale) as f32);
                (
                    start.resolve(line_length).px() as f64 * scale,
                    end.resolve(line_length).px() as f64 * scale,
                )
            }
            GenericTextDecorationInset::Auto => (0.0, 0.0),
        };

        if deco.line.contains(TextDecorationLine::UNDERLINE) {
            let size = decoration_size(
                &deco.thickness,
                geom.underline_size,
                geom.css_font_size,
                scale,
            );

            // `text-underline-offset` moves the underline away from the text. `auto` keeps
            // the font's suggested position; otherwise it resolves against the font size
            // (percentages relative to 1em).
            let extra_underline_offset = deco
                .underline_offset
                .as_ref()
                .map(|lp| lp.resolve(Length::new(geom.css_font_size as f32)).px() as f64 * scale)
                .unwrap_or(0.0);

            // `text-underline-position: under` places the underline below the glyph box
            // (below descenders) rather than at the font's suggested position near the
            // alphabetic baseline. We anchor the top of the line at the descent so it clears
            // descending glyphs like "gqy".
            // An explicit offset under `auto` is taken from the alphabetic
            // baseline, as CSS and Chrome have it, not from the auto gap.
            let base_offset = if deco.underline_under {
                -geom.descent
            } else if deco.underline_offset.is_some() && !deco.underline_from_font {
                0.0
            } else {
                geom.underline_offset
            };
            let offset = base_offset - extra_underline_offset as f32;

            // A `double` underline extends downward, away from the text.
            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if deco.line.contains(TextDecorationLine::OVERLINE) {
            // Fonts don't provide a dedicated overline metric, so reuse the underline
            // thickness. The line sits at the top of the "em box": its lower edge rests on
            // the ascent so it clears the glyphs, and it extends upward from there.
            //
            // Browsers use the OS/2 `usWinAscent` for this edge, which is taller than the
            // hhea-based ascent Parley reports; using the smaller value would draw the
            // overline too low. Fall back to Parley's ascent for fonts without a usable
            // OS/2 table.
            let size = decoration_size(
                &deco.thickness,
                geom.underline_size,
                geom.css_font_size,
                scale,
            );
            let ascent =
                win_ascent(win_ascent_ratios, &geom.font, geom.font_size).unwrap_or(geom.ascent);
            let offset = ascent + size;

            // A `double` overline extends upward, away from the text.
            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                -1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if deco.line.contains(TextDecorationLine::LINE_THROUGH) {
            let size = decoration_size(
                &deco.thickness,
                geom.strikethrough_size,
                geom.css_font_size,
                scale,
            );

            // Centre the line-through a third of the ascent above the baseline, matching
            // Chrome (which places it `2/3 * ascent` below the text-top). Parley's
            // `strikethrough_offset` (the font's `yStrikeoutPosition`) sits lower and would
            // draw the line too close to the baseline.
            let ascent =
                win_ascent(win_ascent_ratios, &geom.font, geom.font_size).unwrap_or(geom.ascent);
            let offset = ascent / 3.0 + size / 2.0;

            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
    }
}

pub(crate) fn stroke_text<'a>(
    scene: &mut crate::display_list::DisplayList,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    options: TextPaintOptions,
    inline_root_id: NodeId,
    context: &mut DrawTextContext,
    source_text: Option<&str>,
) {
    let TextPaintOptions {
        scale,
        brush_override,
    } = options;
    let DrawTextContext {
        stack,
        path_scratch,
        deco_boxes,
        win_ascent_ratios,
        faces,
    } = context;
    stack.clear();
    path_scratch.clear();
    deco_boxes.clear();

    // Persistent stack mirroring the ancestor path (inline root -> current run's
    // node) as we walk the runs. The `text-decoration-*` properties are *not*
    // inherited; instead a decoration set on an ancestor is propagated to the
    // descendant text it wraps. Rather than re-resolving styles for the whole
    // ancestor chain on every run, we cache each node's resolved values here and,
    // for each run, only resolve styles for the nodes newly descended into (popping
    // as we ascend). `path_scratch` is a reusable buffer for the run's node path.
    for line in lines {
        // Decorations accumulated for this line, keyed by decorating box, so each box is
        // painted once (spanning all its runs) using its own font — matching Firefox, which
        // draws one decoration per box rather than one stepped segment per differently-sized
        // run. Clearing preserves the allocation for the next line and inline context.
        deco_boxes.clear();
        // BGMail: start x of the shaped run the current glyph run belongs to
        // (glyph runs of one shaped run are consecutive on a line).
        let mut shaped_run: Option<(std::ops::Range<usize>, u64, u32)> = None;
        let mut run_clusters = Vec::new();
        let mut next_cluster = 0;

        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                let key = (
                    glyph_run.run().text_range(),
                    glyph_run.run().font().data.id(),
                    glyph_run.run().font().index,
                );
                if shaped_run.as_ref() != Some(&key) {
                    shaped_run = Some(key);
                    run_clusters = source_text.map_or_else(Vec::new, |source| {
                        crate::text::shaped_clusters(&glyph_run.run(), source, glyph_run.offset())
                    });
                    next_cluster = 0;
                }
                let run = glyph_run.run();
                let font = run.font();
                let font_size = run.font_size();
                let metrics = run.font_metrics();
                let style = glyph_run.style();
                let synthesis = run.synthesis();
                let glyph_xform = synthesis
                    .skew()
                    .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

                let css_font_size = font_size as f64 / scale;

                // Reconcile the stack with this run's ancestor path. Build the path
                // (inline root -> run node), keep the shared prefix already on the stack,
                // pop the rest, and resolve styles only for the newly-descended nodes.
                path_scratch.clear();
                let mut walk_id = Some(style.brush.id);
                while let Some(node_id) = walk_id {
                    path_scratch.push(node_id);
                    if node_id == inline_root_id {
                        break;
                    }
                    walk_id = doc.get_node(node_id).and_then(|node| node.parent);
                }
                path_scratch.reverse();

                let shared = stack
                    .iter()
                    .zip(path_scratch.iter())
                    .take_while(|(entry, node_id)| entry.node_id == **node_id)
                    .count();
                stack.truncate(shared);
                for &node_id in &path_scratch[shared..] {
                    stack.push(resolve_decoration_entry(doc, node_id));
                }

                let fill_color = brush_override
                    .unwrap_or_else(|| stack.last().map(|e| e.fill_color).unwrap_or(Color::BLACK));

                let embolden = if FONT_EMBOLDEN_ENABLED {
                    let fs = font_size as f64 / scale;
                    kurbo::Vec2::new((0.015125 * fs).min(0.3), (0.0121 * fs).min(0.3))
                } else {
                    kurbo::Vec2::default()
                };

                // BGMail: hand GPUI the words (source text + Blitz position) so it
                // shapes and draws them with its own text system. Glyph ids are only
                // forwarded when the source text is unknown.
                // Colour-bitmap fonts (emoji) keep Blitz's glyph ids: GPUI 0.2.2
                // can't draw CBDT colour glyphs itself (see paint.rs).
                if faces.is_color_bitmap_font(run.font()) {
                    scene.push_color_glyphs(crate::text::color_glyph_run(&glyph_run, transform));
                    continue;
                }
                let text_run = source_text.and_then(|source| {
                    crate::text::text_run_from_glyph_run(
                        &glyph_run,
                        &run_clusters,
                        &mut next_cluster,
                        source,
                        transform,
                        fill_color,
                        faces,
                    )
                });
                if let Some(text_run) = text_run {
                    scene.push_text(text_run);
                } else {
                    let normalized_coords: &[i16] = bytemuck::cast_slice(run.normalized_coords());
                    scene.draw_glyphs(
                        font,
                        font_size,
                        !FONT_EMBOLDEN_ENABLED, // hint
                        normalized_coords,
                        embolden,
                        Fill::NonZero,
                        &anyrender::Paint::from(fill_color),
                        1.0, // alpha
                        transform,
                        glyph_xform,
                        glyph_run.positioned_glyphs().map(|glyph| anyrender::Glyph {
                            id: glyph.id as _,
                            x: glyph.x,
                            y: glyph.y,
                        }),
                    );
                }

                // Accumulate this run's contribution to each decorating box on its ancestor
                // path. The decoration is drawn once per box after the whole line has been
                // walked (see `flush_line_decorations`), so mixed font sizes within a box
                // produce a single straight line rather than one stepped segment per run.
                let geometry = DecorationRunGeometry {
                    baseline: glyph_run.baseline(),
                    ascent: metrics.ascent,
                    descent: metrics.descent,
                    underline_offset: metrics.underline_offset,
                    underline_size: metrics.underline_size,
                    strikethrough_size: metrics.strikethrough_size,
                    font: font.clone(),
                    font_size,
                    css_font_size,
                };
                let run_node_id = style.brush.id;
                let run_x0 = glyph_run.offset() as f64;
                let run_x1 = run_x0 + glyph_run.advance() as f64;

                for entry in stack.iter() {
                    if entry.decoration.is_none() {
                        continue;
                    }
                    let idx = match deco_boxes.iter().position(|d| d.node_id == entry.node_id) {
                        Some(idx) => idx,
                        None => {
                            deco_boxes.push(LineDecoration {
                                node_id: entry.node_id,
                                deco: entry.decoration.clone().unwrap(),
                                min_x: f64::INFINITY,
                                max_x: f64::NEG_INFINITY,
                                own: None,
                                first: None,
                            });
                            deco_boxes.len() - 1
                        }
                    };
                    let acc = &mut deco_boxes[idx];
                    acc.min_x = acc.min_x.min(run_x0);
                    acc.max_x = acc.max_x.max(run_x1);
                    if acc.first.is_none() {
                        acc.first = Some(geometry.clone());
                    }
                    // A run whose innermost node is the box itself is the box's own text, so
                    // its font is the one Firefox positions and sizes the decoration with.
                    if entry.node_id == run_node_id {
                        acc.own = Some(geometry.clone());
                    }
                }
            }
        }

        flush_line_decorations(
            scene,
            transform,
            options,
            deco_boxes,
            win_ascent_ratios,
            inline_root_id,
            line.metrics().baseline,
        );
    }
}

/// Draw selection highlight rectangles for the given byte range in a layout.
/// Uses Parley's Selection type for accurate geometry calculation.
pub(crate) fn draw_text_selection(
    scene: &mut crate::display_list::DisplayList,
    layout: &Layout<TextBrush>,
    transform: Affine,
    selection_start: usize,
    selection_end: usize,
    // BGMail: the host's selection color.
    color: Color,
) {
    let anchor = Cursor::from_byte_index(layout, selection_start, Affinity::Downstream);
    let focus = Cursor::from_byte_index(layout, selection_end, Affinity::Downstream);
    let selection = Selection::new(anchor, focus);

    selection.geometry_with(layout, |rect, _line_idx| {
        let rect = kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1);
        scene.fill(Fill::NonZero, transform, color, None, &rect);
    });
}
