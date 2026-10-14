//! Bounded plain table projection for text output.
use mant_ir::{TableCell, TableCellKind, TableRow, TableRowPlan, bounded_table_rows};

const MAX_DENSE_PHYSICAL_SLOTS: usize = 16_384;

pub(super) trait TableText: Clone {
    fn has_alignment(&self) -> bool {
        false
    }
    fn align_within(self, _width: Option<usize>) -> Self {
        self
    }
    fn plain(value: &str) -> Self;
    fn is_empty(&self) -> bool;
    fn width(&self) -> usize;
    fn physical_lines(&self) -> Vec<Self>;
    fn ends_with_break(&self) -> bool;
    fn join(values: &[Self], separator: &str) -> Self;
    fn prefixed(self, prefix: &str) -> Self;
    fn append(&mut self, other: &Self);
}

struct CellText<T> {
    rows: Vec<T>,
    widths: Vec<usize>,
    empty: bool,
    origins: mant_ir::geometry::CellOriginBounds,
    completed: bool,
    break_after: bool,
}

impl<T: TableText> CellText<T> {
    fn line_count(&self) -> usize {
        self.rows.len()
    }
}

enum PreparedRow<T> {
    Complete(Vec<T>, bool),
    Dense(Vec<Option<CellText<T>>>),
    Sparse(Vec<(usize, CellText<T>)>),
}

#[cfg(test)]
fn table_rows(rows: &[TableRow], mut render_cell: impl FnMut(&TableCell) -> String) -> Vec<String> {
    projected_table_rows(rows, 2, 0, None, |cell| (render_cell(cell), false)).0
}

/// Prepare every source cell once, then place only its physical rows. Actual
/// parent/field/child origins and generated blanks share the generic bounds;
/// a failed projection reuses prepared cells in source order.
pub(super) fn projected_table_rows<T: TableText>(
    rows: &[TableRow],
    gap_columns: u16,
    origin: i32,
    viewport: Option<usize>,
    mut render_cell: impl FnMut(&TableCell) -> (T, bool),
) -> (Vec<T>, bool) {
    let gap = usize::from(gap_columns);
    let mut prepared = Vec::new();
    for (plan, row) in bounded_table_rows(rows).into_iter().zip(rows) {
        if mant_ir::table_row_is_navigation_only(row) {
            continue;
        }
        let prepared_row = match plan {
            TableRowPlan::Empty => PreparedRow::Complete(vec![T::plain("")], true),
            TableRowPlan::WholeRule { double } => {
                PreparedRow::Complete(vec![T::plain(if double { "===" } else { "---" })], false)
            }
            TableRowPlan::LayoutRule { cells } => {
                let parts = cells
                    .iter()
                    .map(|kind| {
                        T::plain(match kind {
                            mant_ir::TableRuleCellKind::Horizontal => "---",
                            mant_ir::TableRuleCellKind::DoubleHorizontal => "===",
                        })
                    })
                    .collect::<Vec<_>>();
                if gap.saturating_mul(parts.len().saturating_sub(1))
                    > mant_ir::geometry::MAX_COLUMN_PADDING
                {
                    PreparedRow::Complete(parts, false)
                } else {
                    PreparedRow::Complete(vec![T::join(&parts, &separator(gap))], false)
                }
            }
            TableRowPlan::Dense { slots } => PreparedRow::Dense(
                slots
                    .into_iter()
                    .map(|cell| cell.map(|cell| render_table_cell(cell, &mut render_cell)))
                    .collect(),
            ),
            TableRowPlan::Sparse { cells } => PreparedRow::Sparse(
                cells
                    .into_iter()
                    .map(|cell| (cell.column, render_table_cell(cell.cell, &mut render_cell)))
                    .collect(),
            ),
        };
        prepared.push(prepared_row);
    }
    let viewport = viewport.map(|width| width.saturating_sub(mant_ir::geometry::padding(origin)));
    let slots = measured_alignment_slots(&prepared, viewport);
    let mut output = Vec::new();
    let mut completed = false;
    for row in prepared {
        let (lines, closed) = match row {
            PreparedRow::Complete(lines, closed) => (lines, closed),
            PreparedRow::Dense(cells) => {
                dense_physical_rows(cells, gap, origin, viewport, slots.as_deref())
            }
            PreparedRow::Sparse(cells) => sparse_physical_rows(cells, origin, viewport),
        };
        output.extend(lines);
        completed = closed;
    }
    (output, completed)
}

/// Column measurements are intrinsic and table-wide, just as in the viewport
/// consumer. Generated owner alignment is applied only after these slots exist.
fn measured_alignment_slots<T: TableText>(
    prepared: &[PreparedRow<T>],
    viewport: Option<usize>,
) -> Option<Vec<usize>> {
    viewport?;
    let mut alignment = false;
    let mut widths = Vec::new();
    for row in prepared {
        if let PreparedRow::Dense(cells) = row {
            widths.resize(widths.len().max(cells.len()), 0);
            for (column, cell) in cells.iter().enumerate() {
                if let Some(cell) = cell {
                    alignment |= cell.rows.iter().any(TableText::has_alignment);
                    widths[column] =
                        widths[column].max(cell.widths.iter().copied().max().unwrap_or(0));
                }
            }
        }
    }
    alignment.then_some(widths)
}

fn separator(gap: usize) -> String {
    // Larger gaps only reach a single segment, which uses no separator.
    let gap = gap.min(mant_ir::geometry::MAX_COLUMN_PADDING);
    format!("{}|{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2))
}

fn render_table_cell<T: TableText>(
    cell: &TableCell,
    render_cell: &mut impl FnMut(&TableCell) -> (T, bool),
) -> CellText<T> {
    let (value, completed) = match cell.kind {
        TableCellKind::Text => render_cell(cell),
        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => {
            (T::plain("---"), false)
        }
        TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
            (T::plain("==="), false)
        }
    };
    let empty = value.is_empty();
    let closes_open_tail = cell.break_after && !completed && value.ends_with_break();
    let mut rows = value.physical_lines();
    // Retiring an occupied row's close must not retire an explicitly completed
    // empty source row. Keep this fact in the prepared cell for every fallback.
    if closes_open_tail {
        rows.push(T::plain(""));
    }
    let widths = rows.iter().map(TableText::width).collect();
    CellText {
        rows,
        widths,
        empty,
        completed,
        break_after: cell.break_after,
        origins: mant_ir::geometry::table_cell_origin_bounds(cell),
    }
}

fn stacked_physical_rows<T: TableText>(
    cells: Vec<CellText<T>>,
    viewport: Option<usize>,
) -> (Vec<T>, bool) {
    let mut completed = false;
    let mut rows = Vec::new();
    for cell in cells {
        if cell.break_after && !cell.completed && cell.empty {
            // An empty field can complete the current data row. With no
            // active row (or after an already completed row) it owns one
            // existing empty row, rather than disappearing from topology.
            if rows.is_empty() || completed {
                rows.extend(cell.rows);
            } else if let Some(last) = rows.last_mut() {
                for row in cell.rows {
                    last.append(&row);
                }
            }
            completed = true;
            continue;
        }
        let physical = cell
            .rows
            .into_iter()
            .map(|row| row.align_within(viewport))
            .collect::<Vec<_>>();
        completed =
            cell.completed || cell.break_after || physical.last().is_some_and(TableText::is_empty);
        rows.extend(physical);
    }
    (rows, completed)
}

fn dense_physical_rows<T: TableText>(
    cells: Vec<Option<CellText<T>>>,
    gap: usize,
    origin: i32,
    viewport: Option<usize>,
    slots: Option<&[usize]>,
) -> (Vec<T>, bool) {
    if cells
        .iter()
        .take(cells.len().saturating_sub(1))
        .flatten()
        .any(|cell| cell.break_after)
        || gap.saturating_mul(cells.len().saturating_sub(1)) > mant_ir::geometry::MAX_COLUMN_PADDING
    {
        return stacked_physical_rows(cells.into_iter().flatten().collect(), viewport);
    }
    let height = cells
        .iter()
        .filter_map(Option::as_ref)
        .map(CellText::line_count)
        .max()
        .unwrap_or(1);
    if height
        .checked_mul(cells.len())
        .is_none_or(|work| work > MAX_DENSE_PHYSICAL_SLOTS)
    {
        return sparse_physical_rows(
            cells
                .into_iter()
                .enumerate()
                .filter_map(|(column, cell)| cell.map(|cell| (column, cell)))
                .collect(),
            origin,
            viewport,
        );
    }
    if slots.is_some_and(|slots| {
        slots
            .iter()
            .sum::<usize>()
            .saturating_add((gap + 1).saturating_mul(slots.len().saturating_sub(1)))
            > viewport.unwrap_or(0)
    }) || !dense_origins_fit(&cells, height, gap, origin, slots)
    {
        return stacked_physical_rows(cells.into_iter().flatten().collect(), viewport);
    }
    let completed = cells.iter().flatten().any(|cell| {
        (cell.completed || cell.break_after || cell.rows.last().is_some_and(TableText::is_empty))
            && cell.rows.len() == height
    });
    (
        assemble_dense_rows(&cells, height, gap, viewport, slots),
        completed,
    )
}

fn dense_origins_fit<T: TableText>(
    cells: &[Option<CellText<T>>],
    height: usize,
    gap: usize,
    origin: i32,
    slots: Option<&[usize]>,
) -> bool {
    let mut maximum = vec![0_usize; cells.len()];
    for line in 0..height {
        let mut column = 0_usize;
        for (index, cell) in cells.iter().enumerate() {
            maximum[index] = maximum[index].max(column);
            column = column
                .saturating_add(slots.map_or_else(
                    || {
                        cell.as_ref()
                            .and_then(|cell| cell.widths.get(line))
                            .copied()
                            .unwrap_or(0)
                    },
                    |slots| slots[index],
                ))
                .saturating_add(gap + 1);
        }
    }
    cells.iter().zip(maximum).all(|(cell, start)| {
        cell.as_ref().is_none_or(|cell| {
            cell.origins.fits_at(mant_ir::geometry::compose_origin(
                origin,
                mant_ir::geometry::coordinate(start),
            ))
        })
    })
}

fn assemble_dense_rows<T: TableText>(
    cells: &[Option<CellText<T>>],
    height: usize,
    gap: usize,
    viewport: Option<usize>,
    slots: Option<&[usize]>,
) -> Vec<T> {
    let separator = separator(gap);
    (0..height)
        .map(|line| {
            let mut segments = cells
                .iter()
                .map(|cell| {
                    cell.as_ref()
                        .and_then(|cell| cell.rows.get(line))
                        .cloned()
                        .unwrap_or_else(|| T::plain(""))
                })
                .collect::<Vec<_>>();
            let mut retired = Vec::new();
            while line > 0 && segments.last().is_some_and(TableText::is_empty) {
                retired.push(segments.pop().expect("empty tail"));
            }
            let mut slot_start = 0usize;
            for (column, segment) in segments.iter_mut().enumerate() {
                let area = slots.and_then(|slots| {
                    if column + 1 == cells.len() {
                        viewport.map(|width| width.saturating_sub(slot_start))
                    } else {
                        slots.get(column).copied()
                    }
                });
                *segment = segment.clone().align_within(area);
                if let Some(area) = area
                    && column + 1 < cells.len()
                    && segment.width() < area
                {
                    segment.append(&T::plain(&" ".repeat(area - segment.width())));
                }
                slot_start = slot_start
                    .saturating_add(slots.map_or(0, |slots| slots[column]))
                    .saturating_add(gap + 1);
            }
            let mut output = T::join(&segments, &separator);
            for tail in retired.into_iter().rev() {
                output.append(&tail);
            }
            output
        })
        .collect()
}

fn sparse_physical_rows<T: TableText>(
    cells: Vec<(usize, CellText<T>)>,
    origin: i32,
    viewport: Option<usize>,
) -> (Vec<T>, bool) {
    if cells.iter().any(|(column, cell)| {
        let prefix = format!("column {}: ", column.saturating_add(1));
        !cell.origins.fits_at(mant_ir::geometry::compose_origin(
            origin,
            mant_ir::geometry::coordinate(prefix.len()),
        ))
    }) {
        return stacked_physical_rows(cells.into_iter().map(|(_, cell)| cell).collect(), viewport);
    }
    let mut completed = false;
    let output = cells
        .into_iter()
        .flat_map(|(column, cell)| {
            let physical = cell.rows;
            completed = cell.completed
                || cell.break_after
                || physical.last().is_some_and(TableText::is_empty);
            physical
                .into_iter()
                .enumerate()
                .map(|(line, value)| {
                    value
                        .prefixed(&if line == 0 {
                            format!("column {}: ", column.saturating_add(1))
                        } else {
                            "  ".into()
                        })
                        .align_within(viewport)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    (output, completed)
}

#[cfg(test)]
impl TableText for String {
    fn plain(value: &str) -> Self {
        value.to_owned()
    }
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn ends_with_break(&self) -> bool {
        self.ends_with('\n')
    }
    fn width(&self) -> usize {
        mant_ir::geometry::text_width(self)
    }
    fn physical_lines(&self) -> Vec<Self> {
        if self.is_empty() {
            vec![Self::new()]
        } else {
            self.split_terminator('\n').map(str::to_owned).collect()
        }
    }
    fn join(values: &[Self], separator: &str) -> Self {
        values.join(separator)
    }
    fn prefixed(self, prefix: &str) -> Self {
        format!("{prefix}{self}")
    }
    fn append(&mut self, other: &Self) {
        self.push_str(other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> TableCell {
        TableCell {
            break_after: false,
            kind: TableCellKind::Text,
            blocks: Vec::new(),
            column_span: 1,
            row_span: 1,
            alignment: None,
        }
    }

    #[test]
    fn dense_rows_preserve_cell_line_boundaries() {
        let rows = [TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![cell(), cell()],
        }];
        let mut values = ["A\nB C", "RIGHT"].into_iter();
        assert_eq!(
            table_rows(&rows, |_| values.next().unwrap().to_owned()),
            ["A | RIGHT", "B C"]
        );
    }

    #[test]
    fn dense_rows_preserve_a_leading_empty_cell_line() {
        let rows = [TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![cell()],
        }];
        assert_eq!(table_rows(&rows, |_| "\nBODY".to_owned()), ["", "BODY"]);
        assert_eq!(table_rows(&rows, |_| "BODY\n".to_owned()), ["BODY"]);
        assert_eq!(table_rows(&rows, |_| "\n".to_owned()), [""]);
    }

    #[test]
    fn structural_empty_and_rule_rows_keep_one_physical_line_each() {
        let rows = [
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: Vec::new(),
            },
            TableRow {
                kind: mant_ir::TableRowKind::HorizontalRule,
                cells: Vec::new(),
            },
            TableRow {
                kind: mant_ir::TableRowKind::DoubleHorizontalRule,
                cells: Vec::new(),
            },
        ];
        assert_eq!(table_rows(&rows, |_| unreachable!()), ["", "---", "==="]);
    }

    #[test]
    fn data_rows_render_cell_rule_roles_without_calling_the_text_renderer() {
        let rows = [TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    kind: TableCellKind::HorizontalRule,
                    ..cell()
                },
                TableCell {
                    kind: TableCellKind::DoubleHorizontalRule,
                    ..cell()
                },
            ],
        }];
        assert_eq!(table_rows(&rows, |_| unreachable!()), ["--- | ==="]);
    }

    #[test]
    fn sparse_rows_align_multiline_column_labels() {
        let rows = [TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![TableCell {
                column_span: u16::MAX,
                ..cell()
            }],
        }];
        assert_eq!(
            table_rows(&rows, |_| "FIRST\nSECOND".to_owned()),
            ["column 1: FIRST", "  SECOND"]
        );
    }

    #[test]
    fn tall_dense_rows_fall_back_before_slot_amplification() {
        let rows = [TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: vec![
                TableCell {
                    column_span: 255,
                    ..cell()
                },
                cell(),
            ],
        }];
        let tall = std::iter::repeat_n("X", 128).collect::<Vec<_>>().join("\n");
        let mut values = [String::new(), tall].into_iter();
        let rendered = table_rows(&rows, |_| values.next().unwrap());
        assert_eq!(rendered.len(), 129);
        assert_eq!(rendered[0], "column 1: ");
        assert_eq!(rendered[1], "column 256: X");
        assert!(rendered[2..].iter().all(|line| line == "  X"));
        assert!(rendered.iter().all(|line| line.len() < 32));
    }
}
