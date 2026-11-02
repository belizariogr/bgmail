//! Turns Blitz/Parley glyph runs into word-positioned [`TextRun`]s.
//!
//! Blitz (Parley) decides *where* text goes: line breaks, bidi and the x of
//! every cluster. GPUI must decide *how* it looks, so instead of forwarding
//! glyph ids we hand GPUI the source text of each word plus its position and
//! the face Parley picked. Positioning per word (as litehtml does) keeps any
//! sub-pixel advance difference between the two shapers from accumulating
//! along a line.

use crate::display_list::{ColorGlyphRun, FaceSpec, TextRun, Word};
use blitz_dom::node::TextBrush;
use kurbo::{Affine, Point};
use parley::layout::GlyphRun;
use peniko::{Color, FontData};
use skrifa::{string::StringId, MetadataProvider};
use std::collections::HashMap;

/// Upright face metadata read from a font file.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceInfo {
    pub family: String,
    pub weight: f32,
    pub italic: bool,
    /// Range of the variable `wght` axis as (min, default, max), if any.
    pub weight_axis: Option<(f32, f32, f32)>,
}

/// Caches [`FaceInfo`] per font file so name tables are parsed once.
#[derive(Default)]
pub struct FaceCache {
    faces: HashMap<(u64, u32), Option<FaceInfo>>,
    color_bitmaps: HashMap<(u64, u32), bool>,
}

/// Whether a font carries colour bitmap strikes (CBDT/sbix: Noto Color Emoji,
/// Apple Color Emoji), which are drawn from their bitmaps.
pub fn has_color_bitmaps(font: &FontData) -> bool {
    skrifa::FontRef::from_index(font.data.as_ref(), font.index)
        .map(|font_ref| {
            let strikes = skrifa::bitmap::BitmapStrikes::new(&font_ref);
            matches!(
                strikes.format(),
                Some(skrifa::bitmap::BitmapFormat::Cbdt | skrifa::bitmap::BitmapFormat::Sbix)
            )
        })
        .unwrap_or(false)
}

impl FaceCache {
    /// Memoised [`has_color_bitmaps`].
    pub fn is_color_bitmap_font(&mut self, font: &FontData) -> bool {
        *self
            .color_bitmaps
            .entry((font.data.id(), font.index))
            .or_insert_with(|| has_color_bitmaps(font))
    }

    pub fn face(&mut self, font: &FontData) -> Option<&FaceInfo> {
        self.faces
            .entry((font.data.id(), font.index))
            .or_insert_with(|| read_face_info(font))
            .as_ref()
    }
}

/// Reads family, weight, style and the `wght` axis of a font file.
pub fn read_face_info(font: &FontData) -> Option<FaceInfo> {
    let font_ref = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    // Prefer the typographic family ("Inter") over the legacy one, which
    // for static fonts often embeds the style ("Inter SemiBold").
    let family = font_ref
        .localized_strings(StringId::TYPOGRAPHIC_FAMILY_NAME)
        .english_or_first()
        .or_else(|| {
            font_ref
                .localized_strings(StringId::FAMILY_NAME)
                .english_or_first()
        })?
        .to_string();
    let attributes = font_ref.attributes();
    let weight_axis = font_ref
        .axes()
        .iter()
        .find(|axis| axis.tag() == skrifa::Tag::new(b"wght"))
        .map(|axis| (axis.min_value(), axis.default_value(), axis.max_value()));
    Some(FaceInfo {
        family,
        weight: attributes.weight.value(),
        italic: attributes.style != skrifa::attribute::Style::Normal,
        weight_axis,
    })
}

/// Maps a normalized `wght` coordinate (F2Dot14) back to a CSS weight.
/// Ignores `avar` remapping, which only shifts intermediate weights slightly.
pub fn denormalize_weight(coord: i16, (min, default, max): (f32, f32, f32)) -> f32 {
    let n = coord as f32 / 16384.0;
    if n >= 0.0 {
        default + n * (max - default)
    } else {
        default + n * (default - min)
    }
}

/// Effective face of a run: the file's own weight/style, adjusted by variable
/// axes and by Parley's synthetic bold/italic.
pub fn effective_face(
    info: &FaceInfo,
    font: &FontData,
    wght_coord: Option<i16>,
    synthetic_bold: bool,
    synthetic_italic: bool,
) -> FaceSpec {
    let mut weight = match (info.weight_axis, wght_coord) {
        (Some(axis), Some(coord)) => denormalize_weight(coord, axis),
        _ => info.weight,
    };
    if synthetic_bold {
        weight = weight.max(700.0);
    }
    FaceSpec {
        family: info.family.clone(),
        weight,
        italic: info.italic || synthetic_italic,
        font: font.clone(),
    }
}

/// A cluster reduced to what word grouping needs.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterSpan {
    pub text_range: std::ops::Range<usize>,
    /// Left edge of the cluster, inline-layout coordinates.
    pub x: f32,
    pub advance: f32,
    pub is_space: bool,
}

/// Whether a cluster's source text is whitespace or control characters (line
/// breaks from `<br>`, tabs), which separate words and are never drawn.
pub fn is_blank(source: &str, range: std::ops::Range<usize>) -> bool {
    source
        .get(range)
        .is_none_or(|text| text.chars().all(|c| c.is_whitespace() || c.is_control()))
}

/// Clusters belonging to one glyph run.
///
/// Parley splits a shaped run into glyph runs wherever the style changes (a
/// link's colour, say) without exposing which clusters each one covers. The
/// glyph runs of a run partition its clusters contiguously in visual order, so
/// the clusters inside `[offset, offset + advance)` are exactly this run's.
pub fn clusters_in_span(clusters: &[ClusterSpan], offset: f32, advance: f32) -> Vec<ClusterSpan> {
    const EPSILON: f32 = 0.01;
    clusters
        .iter()
        .filter(|c| c.x >= offset - EPSILON && c.x < offset + advance - EPSILON)
        .cloned()
        .collect()
}

/// Groups clusters (in visual order) into words, in inline-layout coordinates:
/// x of the first visual cluster, total advance, and each cluster's byte range
/// within the word with its x. Whitespace separates words and is not drawn.
pub fn group_words(source: &str, clusters: &[ClusterSpan]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current: Vec<&ClusterSpan> = Vec::new();
    let flush = |current: &mut Vec<&ClusterSpan>, words: &mut Vec<Word>| {
        if current.is_empty() {
            return;
        }
        let start = current
            .iter()
            .map(|c| c.text_range.start)
            .min()
            .unwrap_or(0);
        let end = current.iter().map(|c| c.text_range.end).max().unwrap_or(0);
        if let Some(text) = source.get(start..end).filter(|t| !t.trim().is_empty()) {
            let x = current[0].x;
            let last = current[current.len() - 1];
            words.push(Word {
                text: text.replace(['\n', '\r'], " "),
                x,
                width: last.x + last.advance - x,
                clusters: current
                    .iter()
                    .map(|c| (c.text_range.start - start, c.text_range.end - start, c.x))
                    .collect(),
            });
        }
        current.clear();
    };
    for cluster in clusters {
        if cluster.is_space {
            flush(&mut current, &mut words);
        } else {
            current.push(cluster);
        }
    }
    flush(&mut current, &mut words);
    words
}

/// Builds a [`ColorGlyphRun`] (document coordinates) for a glyph run whose
/// font draws from colour bitmaps.
pub fn color_glyph_run(glyph_run: &GlyphRun<'_, TextBrush>, transform: Affine) -> ColorGlyphRun {
    let run = glyph_run.run();
    ColorGlyphRun {
        font: run.font().clone(),
        font_size: run.font_size(),
        glyphs: glyph_run
            .positioned_glyphs()
            .map(|g| {
                let p = transform * Point::new(g.x as f64, g.y as f64);
                (g.id, p.x as f32, p.y as f32)
            })
            .collect(),
    }
}

/// Builds a [`TextRun`] for one Parley glyph run.
///
/// `run_start_x` is the x where the glyph run's underlying shaped run starts
/// on this line (the offset of its first glyph run); `transform` maps the
/// inline layout's coordinates to document coordinates (it is the transform
/// Blitz would pass to `draw_glyphs`).
pub fn text_run_from_glyph_run(
    glyph_run: &GlyphRun<'_, TextBrush>,
    run_start_x: f32,
    source: &str,
    transform: Affine,
    color: Color,
    faces: &mut FaceCache,
) -> Option<TextRun> {
    let run = glyph_run.run();
    let font = run.font();
    let info = faces.face(font)?.clone();
    let wght_coord = info.weight_axis.and_then(|_| {
        let font_ref = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
        let index = font_ref
            .axes()
            .iter()
            .position(|axis| axis.tag() == skrifa::Tag::new(b"wght"))?;
        bytemuck::cast_slice::<_, i16>(run.normalized_coords())
            .get(index)
            .copied()
    });
    let synthesis = run.synthesis();
    let face = effective_face(
        &info,
        font,
        wght_coord,
        synthesis.embolden(),
        synthesis.skew().is_some(),
    );

    let mut x = run_start_x;
    let clusters: Vec<ClusterSpan> = run
        .visual_clusters()
        .map(|cluster| {
            let span = ClusterSpan {
                text_range: cluster.text_range(),
                x,
                advance: cluster.advance(),
                is_space: cluster.is_space_or_nbsp() || is_blank(source, cluster.text_range()),
            };
            x += span.advance;
            span
        })
        .collect();
    let clusters = clusters_in_span(&clusters, glyph_run.offset(), glyph_run.advance());
    let baseline = transform * Point::new(glyph_run.offset() as f64, glyph_run.baseline() as f64);
    let [scale_x, ..] = transform.as_coeffs();
    let to_doc_x = |x: f32| (transform * Point::new(x as f64, 0.0)).x as f32;
    let words = group_words(source, &clusters)
        .into_iter()
        .map(|word| Word {
            x: to_doc_x(word.x),
            width: word.width * scale_x as f32,
            clusters: word
                .clusters
                .iter()
                .map(|&(start, end, x)| (start, end, to_doc_x(x)))
                .collect(),
            text: word.text,
        })
        .collect();
    Some(TextRun {
        face,
        // Documents are laid out at scale 1 (GPUI applies the display scale),
        // so Parley's font size is already in CSS px.
        font_size: run.font_size(),
        color,
        baseline: baseline.y as f32,
        words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lays clusters out left to right from `start_x`.
    fn spans(start_x: f32, items: &[(std::ops::Range<usize>, f32, bool)]) -> Vec<ClusterSpan> {
        let mut x = start_x;
        items
            .iter()
            .map(|(range, advance, is_space)| {
                let s = ClusterSpan {
                    text_range: range.clone(),
                    x,
                    advance: *advance,
                    is_space: *is_space,
                };
                x += advance;
                s
            })
            .collect()
    }

    #[test]
    fn groups_words_with_their_start_x() {
        let source = "ab cd";
        let clusters = spans(
            10.0,
            &[
                (0..1, 5.0, false),
                (1..2, 5.0, false),
                (2..3, 3.0, true),
                (3..4, 5.0, false),
                (4..5, 5.0, false),
            ],
        );
        let words = group_words(source, &clusters);
        let simple: Vec<(&str, f32, f32)> = words
            .iter()
            .map(|w| (w.text.as_str(), w.x, w.width))
            .collect();
        assert_eq!(simple, vec![("ab", 10.0, 10.0), ("cd", 23.0, 10.0)]);
        assert_eq!(words[1].clusters, vec![(0, 1, 23.0), (1, 2, 28.0)]);
    }

    #[test]
    fn rtl_clusters_rebuild_logical_word() {
        // Visual order is reversed for RTL; the word text stays logical.
        let source = "שלום";
        let clusters = spans(
            0.0,
            &[
                (6..8, 4.0, false),
                (4..6, 4.0, false),
                (2..4, 4.0, false),
                (0..2, 4.0, false),
            ],
        );
        let words = group_words(source, &clusters);
        assert_eq!(words.len(), 1);
        assert_eq!(
            (words[0].text.as_str(), words[0].x, words[0].width),
            ("שלום", 0.0, 16.0)
        );
        // Visual order kept; byte ranges are relative to the word.
        assert_eq!(words[0].clusters[0], (6, 8, 0.0));
    }

    #[test]
    fn line_breaks_and_controls_are_blank() {
        assert!(is_blank("a\nb", 1..2));
        assert!(is_blank("a\tb", 1..2));
        assert!(!is_blank("a\nb", 0..1));
        assert!(is_blank("ab", 5..6));
    }

    #[test]
    fn words_never_contain_line_breaks() {
        let clusters = spans(0.0, &[(0..3, 5.0, false)]);
        assert_eq!(group_words("a\nb", &clusters)[0].text, "a b");
    }

    #[test]
    fn whitespace_only_runs_produce_no_words() {
        let clusters = spans(0.0, &[(0..1, 3.0, true), (1..2, 3.0, true)]);
        assert!(group_words("  ", &clusters).is_empty());
    }

    #[test]
    fn out_of_range_clusters_are_ignored() {
        let clusters = spans(0.0, &[(0..10, 3.0, false)]);
        assert!(group_words("abc", &clusters).is_empty());
    }

    #[test]
    fn glyph_run_takes_only_its_clusters() {
        // "see link now": the link (style change) is its own glyph run.
        let source = "see link now";
        let all = spans(
            0.0,
            &[
                (0..3, 30.0, false),
                (3..4, 5.0, true),
                (4..8, 40.0, false),
                (8..9, 5.0, true),
                (9..12, 30.0, false),
            ],
        );
        let link = clusters_in_span(&all, 35.0, 40.0);
        let link_words = group_words(source, &link);
        assert_eq!(
            (link_words[0].text.as_str(), link_words[0].x),
            ("link", 35.0)
        );
        let tail = clusters_in_span(&all, 75.0, 35.0);
        let tail_words = group_words(source, &tail);
        assert_eq!(
            (tail_words[0].text.as_str(), tail_words[0].x),
            ("now", 80.0)
        );
    }

    #[test]
    fn denormalizes_weight_axis() {
        let axis = (100.0, 400.0, 900.0);
        assert_eq!(denormalize_weight(0, axis), 400.0);
        assert_eq!(denormalize_weight(16384, axis), 900.0);
        assert_eq!(denormalize_weight(-16384, axis), 100.0);
        assert_eq!(denormalize_weight(8192, axis), 650.0);
    }

    #[test]
    fn synthetic_bold_and_italic_adjust_face() {
        let info = FaceInfo {
            family: "Test".into(),
            weight: 400.0,
            italic: false,
            weight_axis: None,
        };
        let font = FontData::new(peniko::Blob::new(std::sync::Arc::new(Vec::<u8>::new())), 0);
        let face = effective_face(&info, &font, None, true, true);
        assert_eq!(face.weight, 700.0);
        assert!(face.italic);
    }

    #[test]
    fn variable_weight_comes_from_coordinates() {
        let info = FaceInfo {
            family: "Var".into(),
            weight: 400.0,
            italic: false,
            weight_axis: Some((100.0, 400.0, 900.0)),
        };
        let font = FontData::new(peniko::Blob::new(std::sync::Arc::new(Vec::<u8>::new())), 0);
        assert_eq!(
            effective_face(&info, &font, Some(16384), false, false).weight,
            900.0
        );
    }

    #[test]
    fn invalid_font_has_no_face_info() {
        let font = FontData::new(peniko::Blob::new(std::sync::Arc::new(vec![0u8; 4])), 0);
        assert!(read_face_info(&font).is_none());
        let mut cache = FaceCache::default();
        assert!(cache.face(&font).is_none());
    }
}
