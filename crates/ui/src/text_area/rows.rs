//! GPUI-free model of visual (soft-wrapped) rows used by [`super::TextArea`]
//! for caret placement, vertical movement and selection painting. The GPUI
//! layer turns shaped lines into [`VisualRow`]s once per reshape; everything
//! else here is plain offset math.

use std::ops::Range;

use super::buffer::previous_grapheme_boundary;

/// One painted row: a whole logical line, or one soft-wrapped slice of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisualRow {
    /// Index of the logical (`\n`-delimited) line this row belongs to.
    pub line: usize,
    /// Absolute byte offset where that logical line starts.
    pub line_start: usize,
    /// Absolute byte range of the row's text (never includes `\n`).
    pub range: Range<usize>,
    /// The row ends at a soft wrap: the same logical line continues below.
    pub soft_wrapped: bool,
}

/// Builds rows from each logical line's byte length and its soft-wrap offsets
/// (relative to the line start, ascending). Lines are assumed to be separated
/// by a single `\n`, as `shape_text` splits them.
pub(crate) fn build_rows(lines: impl IntoIterator<Item = (usize, Vec<usize>)>) -> Vec<VisualRow> {
    let mut rows = Vec::new();
    let mut line_start = 0;
    for (line, (len, wraps)) in lines.into_iter().enumerate() {
        let mut row_start = 0;
        for wrap in wraps.into_iter().filter(|&wrap| wrap > 0 && wrap < len) {
            rows.push(VisualRow {
                line,
                line_start,
                range: line_start + row_start..line_start + wrap,
                soft_wrapped: true,
            });
            row_start = wrap;
        }
        rows.push(VisualRow {
            line,
            line_start,
            range: line_start + row_start..line_start + len,
            soft_wrapped: false,
        });
        line_start += len + 1;
    }
    rows
}

/// Row that displays the caret at `offset`. At a soft wrap the offset is both
/// the end of one row and the start of the next; the caret goes to the next
/// row (downstream affinity), matching where typed text would appear.
pub(crate) fn row_index_for_offset(rows: &[VisualRow], offset: usize) -> usize {
    rows.partition_point(|row| row.range.start <= offset)
        .saturating_sub(1)
}

/// Last caret position that still renders on `row`: the row end, or for a
/// soft-wrapped row the position before its last grapheme (usually the space
/// the wrap happened at), since the row end itself renders on the next row.
pub(crate) fn row_caret_end(text: &str, row: &VisualRow) -> usize {
    if row.soft_wrapped {
        previous_grapheme_boundary(text, row.range.end).max(row.range.start)
    } else {
        row.range.end
    }
}

/// Clamps `offset` to the caret positions that render on `row`.
pub(crate) fn clamp_offset_to_row(text: &str, row: &VisualRow, offset: usize) -> usize {
    offset.min(row_caret_end(text, row)).max(row.range.start)
}

/// Row reached by moving `delta` rows from `row`, or `None` when that would
/// leave the document (callers then go to the document start/end).
pub(crate) fn vertical_target_row(row_count: usize, row: usize, delta: isize) -> Option<usize> {
    row.checked_add_signed(delta)
        .filter(|&target| target < row_count)
}

/// Part of a selection painted on one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectionSpan {
    pub row: usize,
    /// Selected bytes of the row (may be empty when only the newline is).
    pub range: Range<usize>,
    /// The `\n` after the row is selected too: painters extend the highlight
    /// a little so selected empty lines stay visible.
    pub includes_newline: bool,
}

/// Splits `selection` into per-row spans, visiting only the rows it touches.
pub(crate) fn selection_spans(rows: &[VisualRow], selection: &Range<usize>) -> Vec<SelectionSpan> {
    if selection.is_empty() || rows.is_empty() {
        return Vec::new();
    }
    let first = row_index_for_offset(rows, selection.start);
    let last = row_index_for_offset(rows, selection.end);
    (first..=last)
        .filter_map(|ix| {
            let row = &rows[ix];
            let start = selection.start.max(row.range.start);
            let end = selection.end.min(row.range.end);
            let includes_newline = !row.soft_wrapped && selection.end > row.range.end;
            (start < end || includes_newline).then_some(SelectionSpan {
                row: ix,
                range: start..end.max(start),
                includes_newline,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "hello world\n\nabc" with the first line wrapped after "hello ".
    fn sample() -> (&'static str, Vec<VisualRow>) {
        let text = "hello world\n\nabc";
        let rows = build_rows([(11, vec![6]), (0, vec![]), (3, vec![])]);
        (text, rows)
    }

    #[test]
    fn build_rows_splits_wraps_and_lines() {
        let (_, rows) = sample();
        let ranges: Vec<_> = rows.iter().map(|row| row.range.clone()).collect();
        assert_eq!(ranges, vec![0..6, 6..11, 12..12, 13..16]);
        assert!(rows[0].soft_wrapped && !rows[1].soft_wrapped);
        assert_eq!((rows[3].line, rows[3].line_start), (2, 13));
        // Degenerate wrap offsets (0 or at the end) are ignored.
        assert_eq!(build_rows([(3, vec![0, 3])]).len(), 1);
        assert_eq!(build_rows(std::iter::empty()).len(), 0);
    }

    #[test]
    fn row_index_for_offset_prefers_downstream_at_soft_wraps() {
        let (_, rows) = sample();
        assert_eq!(row_index_for_offset(&rows, 0), 0);
        assert_eq!(row_index_for_offset(&rows, 5), 0);
        assert_eq!(row_index_for_offset(&rows, 6), 1, "soft wrap goes down");
        assert_eq!(row_index_for_offset(&rows, 11), 1, "hard line end stays");
        assert_eq!(row_index_for_offset(&rows, 12), 2);
        assert_eq!(row_index_for_offset(&rows, 16), 3);
        assert_eq!(row_index_for_offset(&[], 3), 0);
    }

    #[test]
    fn row_caret_end_backs_off_soft_wrap() {
        let (text, rows) = sample();
        assert_eq!(row_caret_end(text, &rows[0]), 5);
        assert_eq!(row_caret_end(text, &rows[1]), 11);
        assert_eq!(row_caret_end(text, &rows[2]), 12);
    }

    #[test]
    fn clamp_offset_to_row_keeps_caret_on_row() {
        let (text, rows) = sample();
        assert_eq!(clamp_offset_to_row(text, &rows[0], 9), 5);
        assert_eq!(clamp_offset_to_row(text, &rows[1], 2), 6);
        assert_eq!(clamp_offset_to_row(text, &rows[1], 8), 8);
    }

    #[test]
    fn vertical_target_row_stops_at_document_edges() {
        assert_eq!(vertical_target_row(4, 0, -1), None);
        assert_eq!(vertical_target_row(4, 3, 1), None);
        assert_eq!(vertical_target_row(4, 1, 1), Some(2));
        assert_eq!(vertical_target_row(4, 2, -2), Some(0));
    }

    #[test]
    fn selection_spans_cover_wraps_and_empty_lines() {
        let (_, rows) = sample();
        let spans = selection_spans(&rows, &(3..14));
        assert_eq!(
            spans,
            vec![
                SelectionSpan {
                    row: 0,
                    range: 3..6,
                    includes_newline: false
                },
                SelectionSpan {
                    row: 1,
                    range: 6..11,
                    includes_newline: true
                },
                SelectionSpan {
                    row: 2,
                    range: 12..12,
                    includes_newline: true
                },
                SelectionSpan {
                    row: 3,
                    range: 13..14,
                    includes_newline: false
                },
            ]
        );
        assert!(selection_spans(&rows, &(4..4)).is_empty());
        // Ending exactly at a soft wrap paints nothing on the next row.
        assert_eq!(selection_spans(&rows, &(0..6)).len(), 1);
    }
}
