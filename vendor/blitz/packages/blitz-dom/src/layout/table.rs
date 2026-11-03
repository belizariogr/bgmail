use blitz_traits::node_id::NodeId;
use std::{ops::Range, sync::Arc};
use style::logical_geometry::WritingMode;
use style::properties::ComputedValues;
use stylo_taffy::CopyStyleGetters;

use atomic_refcell::AtomicRefCell;
use markup5ever::local_name;
use style::properties::style_structs::Border;
use style::servo_arc::Arc as ServoArc;
use style::typed_om::NumericBaseType;
use style::values::computed::calc::CalcPercentageLeaf;
use style::values::computed::length_percentage::{
    CalcLengthPercentage, CalcNode, ComputedLeaf, Unpacked as UnpackedLengthPercentage,
};
use style::values::computed::{Length, LengthPercentage};
use style::values::generics::Optional;
use style::values::generics::calc::MinMaxOp;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{
    Atom, computed_values::border_collapse::T as BorderCollapse,
    computed_values::table_layout::T as TableLayout, values::computed::BorderStyle,
};
use style_traits::values::specified::AllowedNumericType;
use taffy::{
    DetailedGridInfo, LayoutPartialTree as _, ResolveOrZero, TrackSizingFunction, style_helpers,
};

use crate::BaseDocument;
use crate::layout::LayoutPassState;

use super::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC};
use super::resolve_calc_value;

pub struct TableTreeWrapper<'a, 'doc> {
    pub(crate) doc: &'a mut LayoutPassState<'doc>,
    pub(crate) ctx: Arc<TableContext>,
    /// BGMail: container style with browser-computed column widths for this
    /// layout call (see [`TableTreeWrapper::auto_layout_column_style`]).
    pub(crate) style_override: Option<taffy::Style<Atom>>,
    /// BGMail: collapsed (top, bottom) margins of anonymous cells, by cell index.
    pub(crate) anonymous_margins: std::collections::HashMap<usize, (f32, f32)>,
}

// Deliberately not `Clone`: `style` may hold raw pointers into `calc_values`.
#[derive(Debug)]
pub struct TableContext {
    pub style: taffy::Style<Atom>,
    pub cells: Vec<TableCell>,
    /// Absolutely positioned descendants of the table's row groups and rows (and of the
    /// table itself). They take no part in the table grid.
    pub oof_children: Vec<(NodeId, taffy::Position)>,
    pub rows: Vec<TableRow>,
    pub columns: Vec<TableColumn>,
    pub computed_grid_info: AtomicRefCell<Option<DetailedGridInfo<Atom>>>,
    pub border_style: Option<ServoArc<Border>>,
    pub border_collapse: BorderCollapse,
    /// `table-layout: fixed` with a definite width (columns come from the first row).
    pub is_fixed: bool,
    /// Backing storage for `calc()` track sizes synthesised by the table layout code.
    /// Taffy stores calc values as raw pointers, so these must outlive the `style`.
    #[allow(dead_code)]
    calc_values: Vec<LengthPercentage>,
    /// BGMail: `style` holds raw pointers to the `calc()` values of these styles, and a
    /// restyle that compares equal keeps this context while replacing the styles. Without
    /// them a hover or a window resize made layout read freed memory.
    #[allow(dead_code)]
    computed_style: ServoArc<ComputedValues>,
}

// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum TableItemKind {
//     Row,
//     Cell,
// }

#[derive(Debug, Clone)]
pub struct TableCell {
    // kind: TableItemKind,
    pub node_id: NodeId,
    /// BGMail: keeps the `calc()` values of `style` alive; see `TableContext::computed_style`.
    #[allow(dead_code)]
    computed_style: ServoArc<ComputedValues>,
    /// Index (into `TableContext::rows`) of the row the cell originates in
    pub row: u16,
    /// Index of the column the cell originates in
    pub column: u16,
    /// BGMail: a non-cell row child (e.g. `<td style="display:block">`) laid out
    /// as if wrapped in an anonymous table cell.
    pub anonymous: bool,
    /// The anonymously wrapped block's own vertical margins (px, lengths only).
    pub own_margins: (f32, f32),
    /// BGMail: (colspan, rowspan), for painting collapsed borders.
    pub span: (u16, u16),
    /// BGMail: border-box `width` of a spanning cell (px, lengths only): its
    /// min/max contribution to the columns it spans in automatic layout.
    pub span_width: Option<f32>,
    style: taffy::Style<Atom>,
}

/// Tracks the current column position while walking the table's cells, so that
/// cells are assigned the same columns Taffy's dense auto-placement will give them
/// (skipping columns still occupied by rowspan cells from earlier rows).
#[derive(Debug, Default)]
struct ColumnCursor {
    /// The column the next cell in the current row will be placed in
    col: u16,
    /// The total number of columns seen so far
    num_columns: u16,
    /// For each column, the number of rows (counting the current one) it stays
    /// occupied by a rowspan cell; `start_row` counts the new row off
    rowspans: Vec<u16>,
}

/// BGMail: the most a `colspan` or `<col span>` counts, as in the HTML standard.
const MAX_COLSPAN: u16 = 1000;
/// BGMail: the most columns of a table. A column costs about 55 bytes in every layout of
/// the table, and grid lines are `i16`.
const MAX_TABLE_COLUMNS: u16 = 8192;
/// BGMail: the last row that gets its own grid line (`i16`); later ones overlap it.
const MAX_TABLE_ROW: u16 = i16::MAX as u16;

impl ColumnCursor {
    fn start_row(&mut self) {
        self.col = 0;
        for remaining in self.rowspans.iter_mut() {
            *remaining = remaining.saturating_sub(1);
        }
    }

    /// Advance past occupied columns, returning the column of the next cell
    fn next_free(&mut self) -> u16 {
        while self.rowspans.get(self.col as usize).is_some_and(|r| *r > 0) {
            self.col += 1;
        }
        self.col
    }

    /// BGMail: the column and span a cell at the next free column gets within
    /// [`MAX_TABLE_COLUMNS`]. Cells that don't fit overlap the last column, as browsers let
    /// cells overlap.
    fn fit(&mut self, colspan: u16) -> (u16, u16) {
        let col = self.next_free().min(MAX_TABLE_COLUMNS - 1);
        self.col = col;
        (col, colspan.clamp(1, MAX_TABLE_COLUMNS - col))
    }

    fn place(&mut self, colspan: u16, rowspan: u16) {
        // BGMail: `u16` wrapped around past 65535 columns, and the slice below panicked.
        let end = (self.col as usize + colspan as usize).min(MAX_TABLE_COLUMNS as usize) as u16;
        if self.rowspans.len() < end as usize {
            self.rowspans.resize(end as usize, 0);
        }
        // BGMail: the cell's own row is counted off by the next `start_row`;
        // storing `rowspan - 1` freed the columns one row early, so declared
        // widths of the next row's cells landed on the wrong columns.
        for remaining in &mut self.rowspans[self.col as usize..end as usize] {
            *remaining = rowspan;
        }
        self.col = end;
        self.num_columns = self.num_columns.max(end);
    }
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub node_id: NodeId,
    /// The column group (if any) that contains the column
    pub group: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    // kind: TableItemKind,
    /// BGMail: `None` for an anonymous row, which has no style of its own to paint.
    pub node_id: Option<NodeId>,
    /// The row group (if any) that contains the row
    pub group: Option<NodeId>,
    /// The row's specified `height`, if it is a length
    pub height: Option<f32>,
    /// Whether the row or any cell originating in it (and spanning only this row) has a
    /// specified length `height`. Such rows do not receive excess table height when there
    /// are unconstrained rows to receive it instead.
    pub is_constrained: bool,
}

/// The used width of one border side: border widths are not adjusted for
/// border-style in computed styles, so a border with `none`/`hidden` style
/// must be treated as zero-width.
/// The writing mode the table's grid runs in (the table's own)
#[cfg_attr(not(feature = "writing-mode"), allow(unused_variables))]
fn table_wm(doc: &BaseDocument, table_root_node_id: NodeId) -> WritingMode {
    #[cfg(feature = "writing-mode")]
    {
        if doc.try_root_element().map(|root| root.id) == Some(table_root_node_id) {
            doc.root_layout_wm(table_root_node_id)
        } else {
            doc.nodes[table_root_node_id].writing_mode()
        }
    }
    #[cfg(not(feature = "writing-mode"))]
    {
        WritingMode::empty()
    }
}

/// Convert a table part's style into the table's writing mode
fn table_taffy_style(style: &ComputedValues, table_wm: WritingMode) -> taffy::Style<Atom> {
    stylo_taffy::to_taffy_style_in(style, table_wm)
}

fn side_width(width: app_units::Au, style: BorderStyle) -> f32 {
    if style.none_or_hidden() {
        0.0
    } else {
        width.to_f32_px()
    }
}

/// Build a `calc(<percent> + <length>)` track sizing function. The calc value is
/// boxed and stored in `calc_values` so that the raw pointer Taffy holds stays valid.
fn percent_plus_length(
    calc_values: &mut Vec<LengthPercentage>,
    percent: f32,
    length: f32,
) -> TrackSizingFunction {
    let percentage = |percent| {
        CalcNode::Leaf(ComputedLeaf::Percentage(CalcPercentageLeaf::new(
            percent,
            Optional::Some(NumericBaseType::Length),
        )))
    };
    let sum = CalcNode::Sum(
        vec![
            percentage(percent),
            CalcNode::Leaf(ComputedLeaf::Length(Length::new(length))),
        ]
        .into(),
    );
    // BGMail: a column is never wider than the table (Chromium: a `100%`
    // cell with padding fills the table instead of overflowing it).
    let node = CalcNode::MinMax(vec![sum, percentage(1.0)].into(), MinMaxOp::Min);
    let value = LengthPercentage::new_calc(node, AllowedNumericType::NonNegative);
    let dim = match value.unpack() {
        UnpackedLengthPercentage::Calc(calc) => {
            let ptr = calc as *const CalcLengthPercentage as *const ();
            // SAFETY: the pointer targets the heap allocation owned by `value`, which
            // is kept alive by `calc_values` for as long as the TableContext exists.
            unsafe { taffy::Dimension::from_raw(taffy::CompactLength::calc(ptr)) }
        }
        UnpackedLengthPercentage::Length(len) => style_helpers::length(len.px()),
        UnpackedLengthPercentage::Percentage(p) => style_helpers::percent(p.0),
    };
    calc_values.push(value);
    dim.into()
}

pub(crate) fn build_table_context(
    doc: &mut BaseDocument,
    table_root_node_id: NodeId,
) -> (TableContext, Vec<NodeId>, Vec<NodeId>) {
    let mut cells: Vec<TableCell> = Vec::new();
    // BGMail: anonymous cell wrappers created for this table (owned by it).
    let mut anonymous_blocks: Vec<NodeId> = Vec::new();
    let mut oof_children: Vec<(NodeId, taffy::Position)> = Vec::new();
    let mut rows: Vec<TableRow> = Vec::new();
    let mut row = 0u16;
    let mut cursor = ColumnCursor::default();

    let wm = table_wm(doc, table_root_node_id);
    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let computed_style = ServoArc::clone(&stylo_styles);
    let mut style = table_taffy_style(&stylo_styles, wm);
    style.item_is_table = true;
    // BGMail: a table's `height` is a minimum: the table grows to fit its
    // rows (`<table height=40>` around two taller rows).
    if style.size.height.tag() == taffy::CompactLength::LENGTH_TAG {
        let height = style.size.height.value();
        let min = style.min_size.height.into_raw();
        if min.tag() != taffy::CompactLength::LENGTH_TAG || min.value() < height {
            style.min_size.height = style_helpers::length(height);
        }
        style.size.height = style_helpers::auto();
    }
    // Use `dense` row-flow so that each cell scans the row from its
    // leftmost column for the first free track. Without `dense`,
    // `place_definite_secondary_axis_item` keeps a per-item secondary
    // cursor across rows, which means cells in later rows do not
    // backfill columns freed up by rowspan cells from earlier rows.
    style.grid_auto_flow = taffy::GridAutoFlow::RowDense;
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    // The fixed table layout algorithm only applies when the table has a non-auto width
    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => !style.size.width.is_auto(),
        TableLayout::Auto => false,
    };

    let border_collapse = stylo_styles.clone_border_collapse();
    let border_spacing = stylo_styles.clone_border_spacing().0;

    drop(stylo_styles);

    let mut columns: Vec<TableColumn> = Vec::new();
    let mut column_sizes: Vec<taffy::TrackSizingFunction> = Vec::new();
    for child_id in children.iter().copied() {
        collect_columns(doc, child_id, wm, &mut columns, &mut column_sizes);
    }
    // BGMail: percentage `<col>` widths are kept for the automatic layout too;
    // `auto_layout_column_style` resolves them like browsers do.
    let mut first_cell_border: Option<ServoArc<Border>> = None;
    // Percentage widths set on first-row cells: (column index, percentage, padding + border)
    let mut percent_columns: Vec<(u16, f32, f32)> = Vec::new();
    let mut calc_values: Vec<LengthPercentage> = Vec::new();
    // Header row groups are laid out before other rows, and footer row groups after
    let row_group_order = |doc: &BaseDocument, child_id: NodeId| -> u8 {
        let display = doc.nodes[child_id]
            .primary_styles()
            .map(|s| s.clone_display());
        // BGMail: captions last, so that the first real row sets what depends on it (fixed
        // widths, collapsed borders). They are moved into place below.
        if display.is_some_and(|d| d.outside() == DisplayOutside::TableCaption) {
            return 3;
        }
        match display.map(|d| d.inside()) {
            Some(DisplayInside::TableHeaderGroup) => 0,
            Some(DisplayInside::TableFooterGroup) => 2,
            _ => 1,
        }
    };
    for order in 0..4 {
        for child_id in children.iter().copied() {
            if row_group_order(doc, child_id) != order {
                continue;
            }
            collect_table_cells(
                doc,
                child_id,
                wm,
                is_fixed,
                border_collapse,
                &mut row,
                &mut cursor,
                &mut cells,
                &mut oof_children,
                &mut rows,
                &mut column_sizes,
                &mut first_cell_border,
                &mut percent_columns,
                false,
                &mut anonymous_blocks,
            );
        }
    }
    let remaining_column = if is_fixed {
        style_helpers::minmax(style_helpers::length(0.0), style_helpers::fr(1.0))
    } else {
        style_helpers::auto()
    };
    // Trailing `<col>` columns which contain no cells and have no definite width
    // (i.e. would be zero-width) are dropped, so they don't add border-spacing.
    while column_sizes.len() > cursor.num_columns as usize
        && column_sizes
            .last()
            .is_some_and(|c| c.min.into_raw().tag() == taffy::CompactLength::AUTO_TAG)
    {
        column_sizes.pop();
        columns.pop();
    }
    let num_columns = cursor.num_columns.max(column_sizes.len() as u16);
    column_sizes.resize(num_columns as usize, remaining_column);
    // BGMail: a caption spans the table, and comes before the rows. `caption-side` isn't in this
    // build of Stylo, so `bottom` is read as the top.
    let is_caption = |doc: &BaseDocument, cell: &TableCell| {
        doc.nodes[cell.node_id]
            .display_style()
            .is_some_and(|d| d.outside() == DisplayOutside::TableCaption)
    };
    let mut top_captions = Vec::new();
    for cell in cells.iter_mut().filter(|cell| is_caption(doc, cell)) {
        cell.span.0 = num_columns.max(1);
        cell.style.grid_column.end = style_helpers::span(num_columns.max(1));
        top_captions.push(cell.row);
    }
    if !top_captions.is_empty() {
        let mut order: Vec<u16> = top_captions.clone();
        order.extend((0..rows.len() as u16).filter(|row| !top_captions.contains(row)));
        let mut new_row = vec![0u16; rows.len()];
        for (new, old) in order.iter().enumerate() {
            new_row[*old as usize] = new as u16;
        }
        let mut taken: Vec<Option<TableRow>> =
            std::mem::take(&mut rows).into_iter().map(Some).collect();
        rows = order
            .iter()
            .filter_map(|old| taken[*old as usize].take())
            .collect();
        for cell in &mut cells {
            if let Some(&row) = new_row.get(cell.row as usize) {
                cell.row = row;
                cell.style.grid_row.start =
                    style_helpers::line((row + 1).min(MAX_TABLE_ROW) as i16);
            }
        }
    }
    if is_fixed {
        // In the fixed table layout algorithm, percentage column widths resolve against
        // the table's content width minus the horizontal border-spacing between columns,
        // whereas Taffy resolves percentage tracks against the container's inner width
        // (from which only the outer border-spacing has been removed via padding). Fold
        // the inner border-spacing into the percentage using `calc(N% - N * spacing)`.
        let spacing_x = match border_collapse {
            BorderCollapse::Separate => border_spacing.width.px(),
            BorderCollapse::Collapse => 0.0,
        };
        let inner_spacing = spacing_x * num_columns.saturating_sub(1) as f32;
        let mut percent_track = |percent: f32, extra: f32| -> TrackSizingFunction {
            let extra = extra - percent * inner_spacing;
            if extra == 0.0 {
                style_helpers::percent(percent)
            } else {
                percent_plus_length(&mut calc_values, percent, extra)
            }
        };

        for column in column_sizes.iter_mut() {
            if column.max.is_auto() {
                *column = remaining_column;
            } else if column.max.into_raw().tag() == taffy::CompactLength::PERCENT_TAG {
                *column = percent_track(column.max.into_raw().value(), 0.0);
            }
        }

        // Percentage widths on cells apply to the cell's content box, so the
        // cell's horizontal padding and border must be added to the column width.
        for (col_idx, percent, extra) in percent_columns {
            if let Some(column) = column_sizes.get_mut(col_idx as usize) {
                *column = percent_track(percent, extra);
            }
        }
    }

    style.grid_template_columns = column_sizes.into_iter().map(|dim| dim.into()).collect();
    // A row's `height` is a minimum: the row grows to fit the content of its cells.
    // Excess table height is distributed to unconstrained rows (via the stretching of
    // `auto` tracks) if there are any, and to all rows otherwise. So constrained rows
    // use a `max-content` (rather than `auto`) maximum when there are unconstrained rows.
    let has_unconstrained_rows = rows.iter().any(|row| !row.is_constrained);
    style.grid_template_rows = rows
        .iter()
        .map(|row| {
            let min = match row.height {
                Some(height) => style_helpers::length(height),
                None => style_helpers::auto(),
            };
            let max = if row.is_constrained && has_unconstrained_rows {
                style_helpers::max_content()
            } else {
                style_helpers::auto()
            };
            style_helpers::minmax(min, max)
        })
        .collect();

    style.gap = match border_collapse {
        BorderCollapse::Separate => {
            // In the separated borders model, `border-spacing` also applies between
            // the table border and the outermost cells, in addition to between cells.
            let spacing_x = border_spacing.width.px();
            let spacing_y = border_spacing.height.px();
            let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
            style.padding = taffy::Rect {
                left: style_helpers::length(padding.left + spacing_x),
                right: style_helpers::length(padding.right + spacing_x),
                top: style_helpers::length(padding.top + spacing_y),
                bottom: style_helpers::length(padding.bottom + spacing_y),
            };
            taffy::Size {
                width: style_helpers::length(spacing_x),
                height: style_helpers::length(spacing_y),
            }
        }
        BorderCollapse::Collapse => first_cell_border
            .as_ref()
            .map(|border| {
                let x = side_width(border.border_left_width.0, border.border_left_style).max(
                    side_width(border.border_right_width.0, border.border_right_style),
                );
                let y = side_width(border.border_top_width.0, border.border_top_style).max(
                    side_width(border.border_bottom_width.0, border.border_bottom_style),
                );
                taffy::Size {
                    width: style_helpers::length(x),
                    height: style_helpers::length(y),
                }
            })
            .unwrap_or(taffy::Size::ZERO.map(style_helpers::length)),
    };

    if border_collapse == BorderCollapse::Collapse {
        // BGMail: in the collapsing border model the table has no padding
        // (CSS 2.1 §17.6.2; Chromium ignores `padding` there).
        style.padding = taffy::Rect::ZERO.map(style_helpers::length);
        style.border = taffy::Rect {
            left: style.gap.width,
            right: style.gap.width,
            top: style.gap.height,
            bottom: style.gap.height,
        };
    }

    let layout_children = cells
        .iter()
        .map(|cell| cell.node_id)
        .chain(oof_children.iter().map(|(node_id, _)| *node_id))
        .collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;

    (
        TableContext {
            style,
            cells,
            oof_children,
            rows,
            columns,
            computed_grid_info: AtomicRefCell::new(None),
            border_collapse,
            border_style: first_cell_border,
            is_fixed,
            calc_values,
            computed_style,
        },
        layout_children,
        anonymous_blocks,
    )
}

/// Collect `<col>` elements (and `display: table-column` elements) along with their
/// widths, which take precedence over cell widths in the fixed table layout algorithm.
/// Columns without a specified width are recorded as `auto`.
fn collect_columns(
    doc: &mut BaseDocument,
    node_id: NodeId,
    wm: WritingMode,
    columns: &mut Vec<TableColumn>,
    column_sizes: &mut Vec<TrackSizingFunction>,
) {
    let node = &doc.nodes[node_id];
    if !node.is_element() {
        return;
    }
    let Some(display) = node.display_style() else {
        return;
    };

    match display.inside() {
        DisplayInside::TableColumnGroup => {
            let first_column = columns.len();
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_columns(doc, child_id, wm, columns, column_sizes);
            }
            doc.nodes[node_id].children = children;
            for column in &mut columns[first_column..] {
                column.group.get_or_insert(node_id);
            }
        }
        DisplayInside::TableColumn => {
            let style = table_taffy_style(&node.primary_styles().unwrap(), wm);
            let span: u16 = node
                .attr(local_name!("span"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|v| v.max(1))
                .unwrap_or(1);
            let column: TrackSizingFunction = match style.size.width.tag() {
                taffy::CompactLength::LENGTH_TAG => {
                    // A definite `max-width` clamps the column width; a zero width is treated as auto
                    let mut width = style.size.width.value();
                    if style.max_size.width.into_raw().tag() == taffy::CompactLength::LENGTH_TAG {
                        width = width.min(style.max_size.width.into_raw().value());
                    }
                    if width > 0.0 {
                        style_helpers::length(width)
                    } else {
                        style_helpers::auto()
                    }
                }
                taffy::CompactLength::PERCENT_TAG => {
                    style_helpers::percent(style.size.width.value())
                }
                // Browsers treat calc() widths on columns as auto
                _ => style_helpers::auto(),
            };
            // A definite `min-width` on a column acts as a floor on its width
            let column = match style.min_size.width.into_raw().tag() {
                taffy::CompactLength::LENGTH_TAG if column.max.is_auto() => style_helpers::minmax(
                    style_helpers::length(style.min_size.width.into_raw().value()),
                    style_helpers::auto(),
                ),
                _ => column,
            };
            // BGMail: limited as `colspan`, or a short `<col>` costs 65535 columns.
            let room = (MAX_TABLE_COLUMNS as usize).saturating_sub(columns.len());
            for _ in 0..(span as usize).min(MAX_COLSPAN as usize).min(room) {
                columns.push(TableColumn {
                    node_id,
                    group: None,
                });
                column_sizes.push(column);
            }
        }
        _ => {}
    }
}

/// BGMail: whether a table-row child would be wrapped in an anonymous cell
/// (an in-flow, non-table-internal element box).
fn needs_anonymous_cell(doc: &BaseDocument, node_id: NodeId) -> bool {
    let node = &doc.nodes[node_id];
    if !node.is_element() {
        return false;
    }
    let Some(display) = node.display_style() else {
        return false;
    };
    if display.outside() == DisplayOutside::None {
        return false;
    }
    let absolutely_positioned = node
        .primary_styles()
        .is_some_and(|style| style.clone_position().is_absolutely_positioned());
    !absolutely_positioned
        && matches!(
            display.inside(),
            DisplayInside::Flow
                | DisplayInside::FlowRoot
                | DisplayInside::Flex
                | DisplayInside::Grid
        )
}

/// BGMail: whether an element generates no box (`display: none`).
fn generates_no_box(doc: &BaseDocument, node_id: NodeId) -> bool {
    doc.nodes[node_id].display_style().is_some_and(|display| {
        // `contents` has no box, but what is inside it does.
        display.outside() == DisplayOutside::None && display.inside() != DisplayInside::Contents
    })
}

/// BGMail: `children` of row `row_id` with each run of consecutive children
/// that need an anonymous cell replaced by one anonymous block holding the run
/// (recorded in `anonymous_blocks`). Whitespace between them does not break a run.
fn group_anonymous_cell_runs(
    doc: &mut BaseDocument,
    row_id: NodeId,
    children: &[NodeId],
    anonymous_blocks: &mut Vec<NodeId>,
) -> Vec<NodeId> {
    let mut items = Vec::with_capacity(children.len());
    let mut run: Vec<NodeId> = Vec::new();
    let mut flush = |doc: &mut BaseDocument, run: &mut Vec<NodeId>, items: &mut Vec<NodeId>| {
        if run.is_empty() {
            return;
        }
        let wrapper = super::construct::create_anonymous_block_node(doc, row_id);
        doc.nodes[wrapper].children = std::mem::take(run).into();
        anonymous_blocks.push(wrapper);
        items.push(wrapper);
    };
    for &child_id in children {
        if needs_anonymous_cell(doc, child_id) {
            run.push(child_id);
        } else if !doc.nodes[child_id].is_element() || generates_no_box(doc, child_id) {
            // Neither text (rows lay out none; whitespace between cells is
            // insignificant) nor `display:none` boxes break a run. Responsive
            // mail hides cells between the ones it stacks.
            continue;
        } else {
            flush(doc, &mut run, &mut items);
            items.push(child_id);
        }
    }
    flush(doc, &mut run, &mut items);
    items
}

#[allow(clippy::too_many_arguments)]
fn collect_table_cells(
    doc: &mut BaseDocument,
    node_id: NodeId,
    wm: WritingMode,
    is_fixed: bool,
    border_collapse: BorderCollapse,
    row: &mut u16,
    cursor: &mut ColumnCursor,
    cells: &mut Vec<TableCell>,
    oof_children: &mut Vec<(NodeId, taffy::Position)>,
    rows: &mut Vec<TableRow>,
    columns: &mut Vec<TrackSizingFunction>,
    first_cell_border: &mut Option<ServoArc<Border>>,
    percent_columns: &mut Vec<(u16, f32, f32)>,
    in_row: bool,
    anonymous_blocks: &mut Vec<NodeId>,
) {
    let node = &mut doc.nodes[node_id];

    // BGMail: anonymous cell wrappers (see `group_anonymous_cell_runs`) too.
    if !node.is_element() && !node.is_anonymous() {
        return;
    }

    let Some(display) = node.display_style() else {
        #[cfg(feature = "tracing")]
        tracing::info!("Ignoring table descendent because it has no styles");
        return;
    };

    // BGMail: `display:contents` has no outer display either, but its children are the table's
    // (the arm for it below was unreachable, and the row or table's text vanished).
    if display.outside() == DisplayOutside::None && display.inside() != DisplayInside::Contents {
        node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
        return;
    }

    // Absolutely positioned boxes are out-of-flow: they are laid out by their containing
    // block rather than as part of the table grid.
    let position = node.primary_styles().unwrap().clone_position();
    if position.is_absolutely_positioned() {
        oof_children.push((node_id, stylo_taffy::convert::position(position)));
        return;
    }

    // CSS 2.2 §17.2.1: a non-table-internal child of a table row is wrapped in an
    // anonymous table-cell. HTML e-mail relies on this (`<td style="display:block">`
    // in responsive templates); ignoring such children made their content vanish.
    // The box itself is placed as the cell, keeping its own margins/size so it
    // behaves like a block inside a borderless, padding-less anonymous cell.
    let treat_as_anonymous_cell = in_row
        && matches!(
            display.inside(),
            DisplayInside::Flow
                | DisplayInside::FlowRoot
                | DisplayInside::Flex
                | DisplayInside::Grid
        );
    let inside = if treat_as_anonymous_cell {
        DisplayInside::TableCell
    } else {
        display.inside()
    };

    match inside {
        DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => {
            // BGMail: its own, not its children's: a cell needs its own to rebuild its boxes, as
            // a width can make it a block with an inline layout, and keeping that one after it
            // is a cell again aborted the app. Rows clear theirs.
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            let first_row = rows.len();
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(
                    doc,
                    child_id,
                    wm,
                    is_fixed,
                    border_collapse,
                    row,
                    cursor,
                    cells,
                    oof_children,
                    rows,
                    columns,
                    first_cell_border,
                    percent_columns,
                    // BGMail: `contents` in a row is still in that row.
                    in_row,
                    anonymous_blocks,
                );
            }
            doc.nodes[node_id].children = children;
            if display.inside() != DisplayInside::Contents {
                for row in &mut rows[first_row..] {
                    row.group.get_or_insert(node_id);
                }
            }
        }
        DisplayInside::TableRow => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            *row = row.saturating_add(1);
            cursor.start_row();

            let height = node.primary_styles().and_then(|style| {
                let height = stylo_taffy::convert::dimension(style.get_height());
                (height.tag() == taffy::CompactLength::LENGTH_TAG).then(|| height.value())
            });
            rows.push(TableRow {
                node_id: Some(node_id),
                group: None,
                height,
                is_constrained: height.is_some(),
            });

            let children = std::mem::take(&mut doc.nodes[node_id].children);
            // BGMail: consecutive non-cell children share one anonymous cell
            // (CSS 2.2 §17.2.1), so `<td style="display:block">` columns in
            // responsive mail stack instead of sitting side by side.
            let items = group_anonymous_cell_runs(doc, node_id, &children, anonymous_blocks);
            for child_id in items {
                collect_table_cells(
                    doc,
                    child_id,
                    wm,
                    is_fixed,
                    border_collapse,
                    row,
                    cursor,
                    cells,
                    oof_children,
                    rows,
                    columns,
                    first_cell_border,
                    percent_columns,
                    true,
                    anonymous_blocks,
                );
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableCell => {
            // BGMail: a cell outside a row (its `<tr>` is `display:contents`) opens an anonymous
            // one, unless the one before is anonymous too, as siblings in `display:table` share
            // it. On grid line 0, which doesn't exist, it would land after the rows following it.
            if !in_row
                && rows
                    .last()
                    .is_none_or(|last| last.node_id.is_some() || last.group.is_some())
            {
                *row = row.saturating_add(1);
                cursor.start_row();
                rows.push(TableRow {
                    node_id: None,
                    group: None,
                    height: None,
                    is_constrained: false,
                });
            }
            let anonymous = treat_as_anonymous_cell;
            let collapsed = border_collapse == BorderCollapse::Collapse && !anonymous;
            // BGMail: the cell's own layout must not count its borders either
            // (they are the table's gaps), or every row grows by its borders.
            node.flags
                .set(crate::node::NodeFlags::IS_COLLAPSED_BORDER_CELL, collapsed);
            node.flags
                .set(crate::node::NodeFlags::IS_TABLE_CELL, !anonymous);
            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            let stylo_style = &node.primary_styles().unwrap();
            let colspan: u16 = (!anonymous)
                .then(|| node.attr(local_name!("colspan")))
                .flatten()
                .and_then(|val| val.parse::<u16>().ok())
                .map_or(1, |v| v.clamp(1, MAX_COLSPAN));
            let rowspan: u16 = (!anonymous)
                .then(|| node.attr(local_name!("rowspan")))
                .flatten()
                .and_then(|val| val.parse::<u16>().ok())
                // BGMail: 65534 as in the standard wraps the `i16` grid lines around.
                .map(|v| v.clamp(1, MAX_TABLE_ROW.saturating_sub(*row).max(1)))
                .unwrap_or(1);
            let mut style = table_taffy_style(stylo_style, wm);
            let (col, colspan) = cursor.fit(colspan);

            // In the collapsed borders model the borders are laid out as gutters between
            // the cells (see the table's `gap`) rather than as part of the cells
            if collapsed {
                style.border = taffy::Rect::ZERO.map(style_helpers::length);
            }

            if first_cell_border.is_none() {
                *first_cell_border = Some(stylo_style.clone_border());
            }

            // In the fixed table layout algorithm the widths of columns are not
            // affected by cell contents, so cells must not impose a min-content
            // floor on their tracks.
            if is_fixed {
                style.min_size.width = style_helpers::length(0.0);
            }

            // Column widths from `<col>` elements take precedence over cell widths
            let col_needs_width =
                (col as usize) >= columns.len() || columns[col as usize].max.is_auto();
            // Fixed layout takes widths from the first row only; the automatic
            // algorithm (BGMail) considers cells of every row, like browsers,
            // but only single-column ones: a spanning cell's width isn't its
            // first column's (it widens its columns through
            // `distribute_span_excess` instead).
            let sets_column = if is_fixed { *row == 1 } else { colspan == 1 };
            let span_width = (colspan > 1
                && !anonymous
                && style.size.width.tag() == taffy::CompactLength::LENGTH_TAG)
                .then(|| {
                    let len = style.size.width.value();
                    match style.box_sizing {
                        taffy::BoxSizing::ContentBox => {
                            let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                            let border = style.border.resolve_or_zero(None, resolve_calc_value);
                            len + padding.left + padding.right + border.left + border.right
                        }
                        taffy::BoxSizing::BorderBox => len,
                    }
                });
            if sets_column && col_needs_width && !anonymous {
                let column: TrackSizingFunction = match style.size.width.tag() {
                    taffy::CompactLength::LENGTH_TAG => {
                        let len = style.size.width.value();
                        let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                        let border = style.border.resolve_or_zero(None, resolve_calc_value);
                        match style.box_sizing {
                            taffy::BoxSizing::ContentBox => style_helpers::length(
                                len + padding.left + padding.right + border.left + border.right,
                            ),
                            taffy::BoxSizing::BorderBox => style_helpers::length(len),
                        }
                    }
                    taffy::CompactLength::PERCENT_TAG => {
                        if !is_fixed {
                            // BGMail: kept for the browser auto-layout algorithm.
                            style_helpers::percent(style.size.width.value())
                        } else if is_fixed {
                            let extra = match style.box_sizing {
                                taffy::BoxSizing::ContentBox => {
                                    let padding =
                                        style.padding.resolve_or_zero(None, resolve_calc_value);
                                    let border =
                                        style.border.resolve_or_zero(None, resolve_calc_value);
                                    padding.left + padding.right + border.left + border.right
                                }
                                taffy::BoxSizing::BorderBox => 0.0,
                            };
                            // Resolved in `build_table_context` once the column count is known
                            percent_columns.push((col, style.size.width.value(), extra));
                            style_helpers::percent(style.size.width.value())
                        } else {
                            style_helpers::auto()
                        }
                    }
                    taffy::CompactLength::AUTO_TAG => style_helpers::auto(),
                    // Dimension values are always length, percentage, auto or calc(),
                    // so any other tag is a calc() value. Pass it through so that
                    // Taffy resolves it against the table's inner width.
                    _ => style.size.width.into(),
                };
                if (col as usize) < columns.len() {
                    if !column.max.is_auto() {
                        columns[col as usize] = column;
                    }
                } else {
                    columns.resize(col as usize, style_helpers::auto());
                    columns.push(column);
                }
            }

            // The margin properties do not apply to table-internal elements (an
            // anonymously wrapped block keeps them: they apply inside its cell)
            if !anonymous {
                style.margin = taffy::Rect::ZERO.map(style_helpers::length);
            }
            let own_margins = {
                let px = |m: taffy::LengthPercentageAuto| {
                    let raw = m.into_raw();
                    if raw.tag() == taffy::CompactLength::LENGTH_TAG {
                        raw.value()
                    } else {
                        0.0
                    }
                };
                (px(style.margin.top), px(style.margin.bottom))
            };
            if anonymous {
                // Vertical margins of an anonymously wrapped block collapse with
                // its children's inside the anonymous cell (see
                // `TableTreeWrapper::compute_child_layout`), not as grid margins.
                style.margin.top = style_helpers::length(0.0);
                style.margin.bottom = style_helpers::length(0.0);
            }

            // BGMail: place the cell at the column the HTML table model gives
            // it (`cursor`, which skips columns held by rowspan cells). Taffy's
            // auto-placement would instead move a colspan that overlaps a
            // rowspan cell to the next free run of columns, past the table
            // (browsers let the cells overlap).
            style.grid_column = taffy::Line {
                start: style_helpers::line(col as i16 + 1),
                end: style_helpers::span(colspan),
            };
            style.grid_row = taffy::Line {
                start: style_helpers::line((*row).min(MAX_TABLE_ROW) as i16),
                end: style_helpers::span(rowspan),
            };
            if rowspan == 1 && style.size.height.tag() == taffy::CompactLength::LENGTH_TAG {
                if let Some(row) = rows.last_mut() {
                    row.is_constrained = true;
                }
            }

            if !anonymous {
                style.size.width = style_helpers::auto();
                // BGMail: min/max heights on table cells are undefined in CSS 2.1 and
                // ignored by Blink (e-mail templates set `min-height` on cells).
                // A cell's `height` is a minimum: the row grows to fit its
                // content (`<td height=81>` around taller content).
                style.min_size.height =
                    if style.size.height.tag() == taffy::CompactLength::LENGTH_TAG {
                        style_helpers::length(style.size.height.value())
                    } else {
                        style_helpers::auto()
                    };
                style.size.height = style_helpers::auto();
                style.max_size.height = style_helpers::auto();
            } else if style.size.width.is_auto() {
                // A block with `width:auto` fills its (anonymous) cell like any block
                // box; as a grid item with auto margins it would shrink-to-fit
                // instead. Use 100% of the cell as a border-box, so max-width and
                // auto margins still clamp and center it.
                if style.box_sizing == taffy::BoxSizing::ContentBox {
                    let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                    let border = style.border.resolve_or_zero(None, resolve_calc_value);
                    let extra = padding.left + padding.right + border.left + border.right;
                    let max_width = style.max_size.width.into_raw();
                    if max_width.tag() == taffy::CompactLength::LENGTH_TAG {
                        style.max_size.width = style_helpers::length(max_width.value() + extra);
                    }
                    style.box_sizing = taffy::BoxSizing::BorderBox;
                }
                style.size.width = style_helpers::percent(1.0);
            }
            cells.push(TableCell {
                computed_style: ServoArc::clone(stylo_style),
                span: (colspan, rowspan),
                span_width,
                node_id,
                row: row.saturating_sub(1),
                column: col,
                anonymous,
                own_margins,
                style,
            });

            cursor.place(colspan, rowspan);
        }
        // BGMail: what isn't a row or cell directly in a table or row group gets an anonymous
        // row and cell (CSS 2.2 §17.2.1). `tbody{display:block}` in a media query for narrow
        // screens left the whole message blank. Captions come here too: display outside, not
        // inside, makes them one.
        DisplayInside::Flow
        | DisplayInside::FlowRoot
        | DisplayInside::Flex
        | DisplayInside::Grid
        | DisplayInside::GridLanes => {
            // Its row group took this off, expecting a row. Everything inside too: as parts of a
            // table, its descendants may never have had boxes of their own.
            let mut pending = vec![node_id];
            while let Some(id) = pending.pop() {
                let descendant = &mut doc.nodes[id];
                descendant.insert_damage(CONSTRUCT_BOX);
                descendant.invalidate_layout_cache();
                descendant
                    .flags
                    .remove(crate::node::NodeFlags::IS_TABLE_CELL);
                descendant
                    .flags
                    .remove(crate::node::NodeFlags::IS_COLLAPSED_BORDER_CELL);
                pending.extend(descendant.children.iter().copied());
            }
            *row = row.saturating_add(1);
            cursor.start_row();
            rows.push(TableRow {
                node_id: None,
                group: None,
                height: None,
                is_constrained: false,
            });
            collect_table_cells(
                doc,
                node_id,
                wm,
                is_fixed,
                border_collapse,
                row,
                cursor,
                cells,
                oof_children,
                rows,
                columns,
                first_cell_border,
                percent_columns,
                true,
                anonymous_blocks,
            );
        }
        DisplayInside::TableColumnGroup | DisplayInside::TableColumn | DisplayInside::Table => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            //Ignore
        }
        DisplayInside::None => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Ignore
        }
    }
}

pub struct RangeIter(Range<usize>);

impl Iterator for RangeIter {
    type Item = taffy::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(taffy::NodeId::from)
    }
}

impl taffy::TraversePartialTree for TableTreeWrapper<'_, '_> {
    type ChildIter<'a>
        = RangeIter
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, _node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        RangeIter(0..self.ctx.cells.len())
    }

    #[inline(always)]
    fn child_count(&self, _node_id: taffy::NodeId) -> usize {
        self.ctx.cells.len()
    }

    #[inline(always)]
    fn get_child_id(&self, _node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        index.into()
    }
}
impl taffy::TraverseTree for TableTreeWrapper<'_, '_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_, '_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style<Atom> {
        self.style_override.as_ref().unwrap_or(&self.ctx.style)
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &taffy::Layout) {
        let index = usize::from(node_id);
        let node_id = crate::taffy_node_id(self.ctx.cells[index].node_id);
        // The grid area of an anonymous cell includes the block's collapsed
        // margins; the block itself sits inside them.
        if let Some(&(top, bottom)) = self.anonymous_margins.get(&index) {
            let mut layout = *layout;
            layout.location.y += top;
            layout.size.height = (layout.size.height - top - bottom).max(0.0);
            return self.doc.set_unrounded_layout(node_id, &layout);
        }
        self.doc.set_unrounded_layout(node_id, layout)
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let index = usize::from(node_id);
        let cell = &self.ctx.cells[index];
        let dom_id = crate::taffy_node_id(cell.node_id);
        if !cell.anonymous {
            return self.doc.compute_child_layout(dom_id, inputs);
        }
        // BGMail: inside an anonymous cell (a BFC root) the block's vertical
        // margins collapse with its first/last children's. Lay it out with
        // collapsible margins and make the cell as tall as box + margins.
        let mut inputs = inputs;
        inputs.vertical_margins_are_collapsible = taffy::Line::TRUE;
        if let (Some(height), Some(&(top, bottom))) = (
            inputs.known_dimensions.height,
            self.anonymous_margins.get(&index),
        ) {
            inputs.known_dimensions.height = Some((height - top - bottom).max(0.0));
        }
        let (own_top, own_bottom) = cell.own_margins;
        let mut output = self.doc.compute_child_layout(dom_id, inputs);
        // The block's own margins join the sets collapsed through its edges.
        let top = output.top_margin.collapse_with_margin(own_top).resolve();
        let bottom = output
            .bottom_margin
            .collapse_with_margin(own_bottom)
            .resolve();
        self.anonymous_margins.insert(index, (top, bottom));
        output.size.height += top + bottom;
        output
    }
}

impl taffy::LayoutGridContainer for TableTreeWrapper<'_, '_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        &self.ctx.cells[usize::from(child_node_id)].style
    }

    fn set_detailed_grid_info(
        &mut self,
        _node_id: taffy::NodeId,
        detailed_grid_info: DetailedGridInfo<Atom>,
    ) {
        *self.ctx.computed_grid_info.borrow_mut() = Some(detailed_grid_info);
    }
}

/// Column widths of the automatic table layout algorithm as browsers implement
/// it (BGMail, after Blink's `AutoTableLayout`). `columns` holds each column's
/// (min, max) content width, a fixed width if a cell specified one, and a
/// percentage if one was specified. Returns the used width of every column for
/// an inner table width `inner`.
///
/// Every column first gets its minimum; the remaining space then goes, in
/// order, to percentage columns (up to their share), fixed columns (up to their
/// width) and auto columns (up to their max, in proportion to max - min). Any
/// excess goes to auto columns in proportion to their max (else to the others).
pub(crate) fn distribute_auto_columns(
    columns: &[(f32, f32, Option<f32>, Option<f32>)],
    inner: f32,
) -> Vec<f32> {
    let n = columns.len();
    let mut widths: Vec<f32> = columns.iter().map(|c| c.0).collect();
    let mut remaining = inner - widths.iter().sum::<f32>();
    if remaining <= 0.0 || n == 0 {
        return widths;
    }
    // Grow `targets` columns toward their target widths, in proportion to how
    // much each still needs, using at most `remaining`.
    let grow = |widths: &mut Vec<f32>, remaining: &mut f32, targets: &[(usize, f32, f32)]| {
        // (index, target, weight)
        let need: f32 = targets
            .iter()
            .map(|&(i, t, _)| (t - widths[i]).max(0.0))
            .sum();
        if need <= 0.0 || *remaining <= 0.0 {
            return;
        }
        // Half a pixel of slack: percentages summing to 100% (e.g. three
        // `33.333333%` columns) overshoot by float error and must still all
        // reach their targets.
        if need <= *remaining + 0.5 {
            for &(i, t, _) in targets {
                widths[i] = widths[i].max(t);
            }
            *remaining = (*remaining - need).max(0.0);
        } else {
            let weight: f32 = targets
                .iter()
                .map(|&(i, t, w)| if t > widths[i] { w } else { 0.0 })
                .sum();
            let budget = *remaining;
            for &(i, t, w) in targets {
                if t > widths[i] && weight > 0.0 {
                    widths[i] = (widths[i] + budget * w / weight).min(t);
                }
            }
            *remaining = 0.0;
        }
    };
    let percent: Vec<(usize, f32, f32)> = (0..n)
        .filter_map(|i| columns[i].3.map(|p| (i, (p * inner).max(columns[i].0), p)))
        .collect();
    grow(&mut widths, &mut remaining, &percent);
    // Short of space, fixed columns get it in proportion to how far each is
    // from its width (Chromium interpolates between minimum and width: no
    // column overshoots, so none of the space is lost to capping).
    let fixed: Vec<(usize, f32, f32)> = (0..n)
        .filter(|&i| columns[i].3.is_none())
        .filter_map(|i| {
            columns[i].2.map(|f| {
                let target = f.max(columns[i].0);
                (i, target, target - columns[i].0)
            })
        })
        .collect();
    grow(&mut widths, &mut remaining, &fixed);
    let auto: Vec<usize> = (0..n)
        .filter(|&i| columns[i].2.is_none() && columns[i].3.is_none())
        .collect();
    let auto_targets: Vec<(usize, f32, f32)> = auto
        .iter()
        .map(|&i| {
            let max = columns[i].1.max(columns[i].0);
            (i, max, (max - columns[i].0).max(0.0))
        })
        .collect();
    grow(&mut widths, &mut remaining, &auto_targets);
    if remaining > 0.0 {
        // Excess: auto columns by their max; else fixed by width; else percent.
        let receivers: Vec<usize> = if !auto.is_empty() {
            auto
        } else if !fixed.is_empty() {
            fixed.iter().map(|f| f.0).collect()
        } else {
            (0..n).collect()
        };
        let weight: f32 = receivers.iter().map(|&i| widths[i]).sum();
        for &i in &receivers {
            widths[i] += if weight > 0.0 {
                remaining * widths[i] / weight
            } else {
                remaining / receivers.len() as f32
            };
        }
    }
    widths
}

/// BGMail: widens the columns a cell spans by `excess`, the cell's
/// min-content (`min`) or max-content width beyond theirs, like Chromium:
/// fixed-width columns grow toward their width first (minimums only: a
/// fixed column's maximum already is its width), then auto columns take the
/// rest in proportion to their max-content width; without auto columns,
/// every column does, in proportion to its width. Columns are (min, max,
/// fixed, percent).
fn distribute_span_excess(
    columns: &mut [(f32, f32, Option<f32>, Option<f32>)],
    mut excess: f32,
    min: bool,
) {
    if min {
        let need: f32 = columns
            .iter()
            .filter_map(|c| c.2.map(|fixed| (fixed - c.0).max(0.0)))
            .sum();
        if need > 0.0 {
            let used = need.min(excess);
            for c in columns.iter_mut() {
                if let Some(fixed) = c.2 {
                    c.0 += (fixed - c.0).max(0.0) * used / need;
                }
            }
            excess -= used;
        }
        if excess <= 0.0 {
            return;
        }
    }
    let is_auto = |c: &(f32, f32, Option<f32>, Option<f32>)| c.2.is_none() && c.3.is_none();
    let any_auto = columns.iter().any(is_auto);
    let takes = |c: &(f32, f32, Option<f32>, Option<f32>)| !any_auto || is_auto(c);
    let weight = |c: &(f32, f32, Option<f32>, Option<f32>)| {
        if takes(c) { c.2.unwrap_or(c.1) } else { 0.0 }
    };
    let total: f32 = columns.iter().map(weight).sum();
    let count = columns.iter().filter(|c| takes(c)).count().max(1) as f32;
    for c in columns.iter_mut() {
        let share = match (takes(c), total > 0.0) {
            (false, _) => 0.0,
            (true, true) => weight(c) / total,
            (true, false) => 1.0 / count,
        };
        if min {
            c.0 += excess * share;
        } else {
            c.1 += excess * share;
        }
    }
}

impl TableTreeWrapper<'_, '_> {
    /// BGMail: the table's container style with explicit column widths from the
    /// browsers' automatic table layout. Taffy's grid would otherwise grow auto
    /// tracks *equally*; tables grow them in proportion to (max - min) content.
    pub(crate) fn auto_layout_column_style(
        &mut self,
        inputs: &taffy::LayoutInput,
    ) -> Option<taffy::Style<Atom>> {
        use taffy::{
            AvailableSpace, GridPlacement, GridTemplateComponent, LayoutInput, Line,
            MaybeResolve as _, RequestedAxis, RunMode, Size, SizingMode,
        };
        // Own handle so cell measurement (`&mut self`) can run while reading it.
        let ctx = Arc::clone(&self.ctx);
        if ctx.is_fixed || ctx.cells.is_empty() {
            return None;
        }
        let style = &ctx.style;
        let n = style.grid_template_columns.len();
        if n == 0 {
            return None;
        }
        let parent_width = inputs.parent_size.width;
        let padding = style
            .padding
            .resolve_or_zero(parent_width, resolve_calc_value);
        let border = style
            .border
            .resolve_or_zero(parent_width, resolve_calc_value);
        let gap = style
            .gap
            .width
            .resolve_or_zero(parent_width, resolve_calc_value);
        let chrome =
            padding.left + padding.right + border.left + border.right + gap * (n as f32 - 1.0);

        // Per-column (min, max, fixed, percent).
        let mut columns: Vec<(f32, f32, Option<f32>, Option<f32>)> = style
            .grid_template_columns
            .iter()
            .map(|track| match track {
                GridTemplateComponent::Single(tsf) => {
                    let raw = tsf.max.into_raw();
                    match raw.tag() {
                        taffy::CompactLength::LENGTH_TAG => (0.0, 0.0, Some(raw.value()), None),
                        taffy::CompactLength::PERCENT_TAG => (0.0, 0.0, None, Some(raw.value())),
                        _ => (0.0, 0.0, None, None),
                    }
                }
                _ => (0.0, 0.0, None, None),
            })
            .collect();

        let mut spanning = Vec::new();
        for idx in 0..ctx.cells.len() {
            let cell = &ctx.cells[idx];
            let col = cell.column as usize;
            let span = match cell.style.grid_column.end {
                GridPlacement::Span(span) => span.max(1) as usize,
                _ => 1,
            };
            let mut measure = |space: AvailableSpace| {
                self.compute_child_layout(
                    idx.into(),
                    LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        // Content contribution only: a cell's own `width` is already a
                        // column constraint (fixed track) and must not cap its minimum.
                        sizing_mode: SizingMode::ContentSize,
                        axis: RequestedAxis::Horizontal,
                        known_dimensions: Size::NONE,
                        known_dimensions_are_definite: Size {
                            width: true,
                            height: true,
                        },
                        parent_size: Size::NONE,
                        available_space: Size {
                            width: space,
                            height: AvailableSpace::MaxContent,
                        },
                        vertical_margins_are_collapsible: Line::FALSE,
                    },
                )
                .size
                .width
            };
            let span_width = ctx.cells[idx].span_width.unwrap_or(0.0);
            let min = measure(AvailableSpace::MinContent).max(span_width);
            let max = measure(AvailableSpace::MaxContent).max(min);
            if span == 1 {
                if let Some(c) = columns.get_mut(col) {
                    c.0 = c.0.max(min);
                    c.1 = c.1.max(max);
                }
            } else {
                spanning.push((col, span, min, max));
            }
        }
        // Spanning cells widen the columns they cover when needed.
        for (col, span, min, max) in spanning {
            let end = (col + span).min(n);
            if col >= end {
                continue;
            }
            let inner_gaps = gap * (end - col - 1) as f32;
            let covered_min: f32 = columns[col..end].iter().map(|c| c.0).sum::<f32>() + inner_gaps;
            // A fixed column's max-content width is its width.
            let covered_max: f32 = columns[col..end]
                .iter()
                .map(|c| c.2.map_or(c.1, |fixed| fixed.max(c.0)))
                .sum::<f32>()
                + inner_gaps;
            if min > covered_min {
                distribute_span_excess(&mut columns[col..end], min - covered_min, true);
            }
            if max > covered_max {
                distribute_span_excess(&mut columns[col..end], max - covered_max, false);
            }
            for c in &mut columns[col..end] {
                c.1 = c.1.max(c.0);
            }
        }

        let col_max = |c: &(f32, f32, Option<f32>, Option<f32>)| c.2.map_or(c.1, |f| f.max(c.0));
        let sum_min: f32 = columns.iter().map(|c| c.0).sum();
        let sum_max: f32 = columns.iter().map(col_max).sum();
        // The table's own min/max-width bound whatever width it would take.
        let min_width = style
            .min_size
            .width
            .maybe_resolve(parent_width, resolve_calc_value);
        let max_width = style
            .max_size
            .width
            .maybe_resolve(parent_width, resolve_calc_value);
        let clamp = |w: f32| {
            let w = max_width.map_or(w, |m| w.min(m));
            min_width.map_or(w, |m| w.max(m))
        };
        let outer = match (inputs.known_dimensions.width, inputs.available_space.width) {
            (Some(width), _) => width.max(sum_min + chrome),
            (None, AvailableSpace::Definite(available)) => match style
                .size
                .width
                .maybe_resolve(parent_width, resolve_calc_value)
            {
                Some(width) => clamp(width).max(sum_min + chrome),
                None => clamp(available.min(sum_max + chrome)).max(sum_min + chrome),
            },
            (None, AvailableSpace::MinContent) => sum_min + chrome,
            (None, AvailableSpace::MaxContent) => clamp(sum_max + chrome).max(sum_min + chrome),
        };
        let inner = (outer - chrome).max(sum_min);
        let widths = distribute_auto_columns(&columns, inner);

        let mut style = style.clone();
        // A table is never narrower than its columns' content (`<table
        // width=100>` around 159px of unbreakable text grows to 159px).
        let min_outer = sum_min + chrome;
        if min_width.is_none_or(|m| m < min_outer) {
            style.min_size.width = style_helpers::length(min_outer);
        }
        // BGMail: rowspan cells spread their extra height like Chromium (see
        // `distribute_rowspan_height`); Taffy's grid would spread it over
        // every spanned track instead.
        if ctx.cells.iter().any(|cell| cell.span.1 > 1) {
            let gap_y = style
                .gap
                .height
                .resolve_or_zero(parent_width, resolve_calc_value);
            let rows = self.rowspan_row_heights(&ctx, &widths, gap, gap_y);
            for (track, height) in style.grid_template_rows.iter_mut().zip(rows) {
                if let GridTemplateComponent::Single(track) = track {
                    if height > 0.0 {
                        track.min = taffy::MinTrackSizingFunction::length(height);
                    }
                }
            }
        }
        style.grid_template_columns = widths
            .into_iter()
            .map(|w| GridTemplateComponent::Single(style_helpers::length(w)))
            .collect();
        Some(style)
    }

    /// BGMail: row heights of a table with rowspan cells, from every cell's
    /// height at its columns' `widths`: single-row cells set their row's
    /// height, then rowspan cells (narrowest spans first) add what they lack
    /// with `distribute_rowspan_height`.
    fn rowspan_row_heights(
        &mut self,
        ctx: &TableContext,
        widths: &[f32],
        gap_x: f32,
        gap_y: f32,
    ) -> Vec<f32> {
        use taffy::{AvailableSpace, LayoutInput, Line, RequestedAxis, RunMode, Size, SizingMode};
        let n = ctx.rows.len();
        let mut heights: Vec<f32> = ctx
            .rows
            .iter()
            .map(|row| row.height.unwrap_or(0.0))
            .collect();
        let mut origins = vec![false; n];
        let mut spanning = Vec::new();
        for idx in 0..ctx.cells.len() {
            let cell = &ctx.cells[idx];
            let (colspan, rowspan) = cell.span;
            let (row, col) = (cell.row as usize, cell.column as usize);
            if row >= n {
                continue;
            }
            let cols = col.min(widths.len())..(col + colspan as usize).min(widths.len());
            let width = widths[cols.clone()].iter().sum::<f32>()
                + gap_x * cols.len().saturating_sub(1) as f32;
            // The cell's height (its declared `height` being a minimum).
            let height = self
                .compute_child_layout(
                    idx.into(),
                    LayoutInput {
                        run_mode: RunMode::ComputeSize,
                        sizing_mode: SizingMode::InherentSize,
                        axis: RequestedAxis::Vertical,
                        known_dimensions: Size {
                            width: Some(width),
                            height: None,
                        },
                        known_dimensions_are_definite: Size {
                            width: true,
                            height: false,
                        },
                        parent_size: Size {
                            width: Some(width),
                            height: None,
                        },
                        available_space: Size {
                            width: AvailableSpace::Definite(width),
                            height: AvailableSpace::MaxContent,
                        },
                        vertical_margins_are_collapsible: Line::FALSE,
                    },
                )
                .size
                .height;
            if rowspan > 1 {
                origins[row] = true;
                let end = (row + rowspan as usize).min(n);
                spanning.push((end - row, row, end, height));
            } else {
                heights[row] = heights[row].max(height);
            }
        }
        spanning.sort_by_key(|&(span, row, _, _)| (span, row));
        let constrained: Vec<bool> = ctx.rows.iter().map(|row| row.is_constrained).collect();
        for (span, start, end, need) in spanning {
            let have = heights[start..end].iter().sum::<f32>() + gap_y * (span - 1) as f32;
            if need > have {
                distribute_rowspan_height(
                    &mut heights[start..end],
                    &origins[start..end],
                    &constrained[start..end],
                    need - have,
                );
            }
        }
        heights
    }
}

/// BGMail: spreads `extra` height a rowspan cell needs over its rows
/// (`rows[0]` is its own row), like Chromium:
/// 1. rows (other than its own) where another rowspan cell starts, equally;
/// 2. else non-empty rows without a declared height, by their height;
/// 3. else non-empty rows, by their height;
/// 4. else every row, equally.
fn distribute_rowspan_height(rows: &mut [f32], origins: &[bool], constrained: &[bool], extra: f32) {
    let starts: Vec<usize> = (1..rows.len()).filter(|&i| origins[i]).collect();
    if !starts.is_empty() {
        for &i in &starts {
            rows[i] += extra / starts.len() as f32;
        }
        return;
    }
    let by_height = |rows: &mut [f32], targets: &[usize]| {
        let total: f32 = targets.iter().map(|&i| rows[i]).sum();
        for &i in targets {
            rows[i] += extra * rows[i] / total;
        }
    };
    let free: Vec<usize> = (0..rows.len())
        .filter(|&i| rows[i] > 0.0 && !constrained[i])
        .collect();
    if !free.is_empty() {
        return by_height(rows, &free);
    }
    let filled: Vec<usize> = (0..rows.len()).filter(|&i| rows[i] > 0.0).collect();
    if !filled.is_empty() {
        return by_height(rows, &filled);
    }
    let share = extra / rows.len() as f32;
    for row in rows.iter_mut() {
        *row += share;
    }
}

#[cfg(test)]
mod auto_table_tests {
    use super::{
        ColumnCursor, distribute_auto_columns, distribute_rowspan_height, distribute_span_excess,
    };

    #[test]
    fn rowspan_height_prefers_rows_starting_rowspans() {
        // Measured in Chromium: rows where another rowspan cell starts take
        // the extra equally, even over auto rows.
        let mut rows = [19.0, 24.0, 35.0, 18.0];
        distribute_rowspan_height(
            &mut rows,
            &[true, true, false, false],
            &[true, true, true, false],
            104.0,
        );
        assert!(close(&rows, &[19.0, 128.0, 35.0, 18.0]));
        // Else unconstrained non-empty rows, by their height...
        let mut rows = [19.0, 24.0, 18.0];
        distribute_rowspan_height(
            &mut rows,
            &[true, false, false],
            &[true, true, false],
            139.0,
        );
        assert!(close(&rows, &[19.0, 24.0, 157.0]));
        // ...else every non-empty row by its height (an empty one stays empty).
        let mut rows = [19.0, 24.0, 35.0, 0.0];
        distribute_rowspan_height(&mut rows, &[true, false, false, false], &[true; 4], 122.0);
        assert!(close(&rows, &[48.72, 61.54, 89.74, 0.0]));
        // ...else all rows equally.
        let mut rows = [0.0, 0.0];
        distribute_rowspan_height(&mut rows, &[true, false], &[false; 2], 10.0);
        assert!(close(&rows, &[5.0, 5.0]));
    }

    #[test]
    fn span_minimums_fill_fixed_columns_first() {
        // (min, max, fixed, percent)
        let mut columns = [(310.0, 310.0, None, None), (39.0, 92.0, Some(110.0), None)];
        distribute_span_excess(&mut columns, 71.0, true);
        assert!(close(&[columns[0].0, columns[1].0], &[310.0, 110.0]));
        // Beyond the fixed widths, auto columns take the rest...
        let mut columns = [(28.0, 59.0, None, None), (8.0, 8.0, Some(50.0), None)];
        distribute_span_excess(&mut columns, 264.0, true);
        assert!(close(&[columns[0].0, columns[1].0], &[250.0, 50.0]));
        // ...or, without any, every column by its width.
        let mut columns = [(8.0, 8.0, Some(100.0), None), (8.0, 8.0, Some(50.0), None)];
        distribute_span_excess(&mut columns, 284.0, true);
        assert!(close(&[columns[0].0, columns[1].0], &[200.0, 100.0]));
    }

    #[test]
    fn span_maximums_skip_fixed_columns_when_auto_ones_exist() {
        let mut columns = [(28.0, 59.0, None, None), (8.0, 8.0, Some(50.0), None)];
        distribute_span_excess(&mut columns, 191.0, false);
        assert!(close(&[columns[0].1, columns[1].1], &[250.0, 8.0]));
    }

    #[test]
    fn short_fixed_columns_lose_no_space() {
        // Fixed widths summing past the table share all of it, each in
        // proportion to its distance from its minimum.
        let w = distribute_auto_columns(
            &[
                (478.0, 478.0, Some(483.0), None),
                (5.0, 5.0, Some(5.0), None),
                (151.0, 151.0, Some(161.0), None),
            ],
            648.0,
        );
        assert!((w.iter().sum::<f32>() - 648.0).abs() < 0.01);
        assert!(close(&w, &[482.6667, 5.0, 160.3333]));
    }

    #[test]
    fn rowspan_cells_occupy_their_columns_in_every_spanned_row() {
        let mut cursor = ColumnCursor::default();
        let mut row = |cursor: &mut ColumnCursor, cells: &[(u16, u16)]| {
            cursor.start_row();
            cells
                .iter()
                .map(|&(colspan, rowspan)| {
                    let col = cursor.next_free();
                    cursor.place(colspan, rowspan);
                    col
                })
                .collect::<Vec<u16>>()
        };
        // <td rowspan=2><td colspan=2> / <td><td> / <td><td><td>
        assert_eq!(row(&mut cursor, &[(1, 2), (2, 1)]), [0, 1]);
        assert_eq!(row(&mut cursor, &[(1, 1), (1, 1)]), [1, 2]);
        assert_eq!(row(&mut cursor, &[(1, 1), (1, 1), (1, 1)]), [0, 1, 2]);
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
    }

    #[test]
    fn grows_in_proportion_to_max_minus_min() {
        // A long text column (min 37, max 641) and a short one (39/39) in 680px:
        // the short one stays at its max, the long one takes the rest.
        let w = distribute_auto_columns(
            &[(37.0, 700.0, None, None), (39.0, 39.0, None, None)],
            680.0,
        );
        assert!(close(&w, &[641.0, 39.0]), "{w:?}");
    }

    #[test]
    fn percentages_summing_to_100_fill_the_table() {
        // BGMail: three `33.333333%` columns overshoot 100% by float error.
        let p = Some(0.333_333_34);
        let w = distribute_auto_columns(
            &[
                (72.0, 72.0, None, p),
                (16.0, 16.0, None, p),
                (8.0, 20.0, None, p),
            ],
            520.0,
        );
        let third = 520.0 / 3.0;
        assert!(close(&w, &[third, third, third]), "{w:?}");
    }

    #[test]
    fn extra_space_goes_in_proportion_to_max() {
        let w = distribute_auto_columns(
            &[(10.0, 100.0, None, None), (10.0, 300.0, None, None)],
            800.0,
        );
        assert!(close(&w, &[200.0, 600.0]), "{w:?}");
    }

    #[test]
    fn fixed_columns_keep_their_width_when_auto_ones_absorb_extra() {
        let w = distribute_auto_columns(
            &[(10.0, 50.0, Some(100.0), None), (10.0, 50.0, None, None)],
            400.0,
        );
        assert!(close(&w, &[100.0, 300.0]), "{w:?}");
    }

    #[test]
    fn percentages_resolve_first() {
        let w = distribute_auto_columns(
            &[(0.0, 10.0, None, Some(0.25)), (0.0, 10.0, None, None)],
            400.0,
        );
        assert!(close(&w, &[100.0, 300.0]), "{w:?}");
    }

    #[test]
    fn fixed_columns_win_over_auto_when_space_is_short() {
        // 260px fixed, 20px fixed, long-text auto column in 560px (Arcane mail).
        let w = distribute_auto_columns(
            &[
                (100.0, 100.0, Some(260.0), None),
                (0.0, 0.0, Some(20.0), None),
                (80.0, 500.0, None, None),
            ],
            560.0,
        );
        assert!(close(&w, &[260.0, 20.0, 280.0]), "{w:?}");
    }

    #[test]
    fn never_below_minimum() {
        let w = distribute_auto_columns(
            &[(300.0, 400.0, None, None), (200.0, 300.0, None, None)],
            100.0,
        );
        assert!(close(&w, &[300.0, 200.0]), "{w:?}");
    }
}
