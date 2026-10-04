//! Bounded plain table projection for text output.
use mant_ir::{TableCell, TableCellKind, TableRow, TableRowPlan, bounded_table_rows};

const MAX_DENSE_PHYSICAL_SLOTS: usize = 16_384;

pub(super) trait TableText: Clone {
    fn plain(value: &str) -> Self;
    fn is_empty(&self) -> bool;
    fn line_count(&self) -> usize;
    fn width(&self) -> usize;
    fn physical_lines(&self) -> Vec<Self>;
    fn ends_with_break(&self) -> bool;
    fn join(values: &[Self], separator: &str) -> Self;
    fn prefixed(self, prefix: &str) -> Self;
    fn append(&mut self, other: &Self);
}

struct CellText<T> {
    value: T,
    origins: mant_ir::geometry::CellOriginBounds,
    completed: bool,
    break_after: bool,
}

impl<T: TableText> CellText<T> {
    fn closes_open_tail(&self) -> bool {
        self.break_after && !self.completed && self.value.ends_with_break()
    }

    fn line_count(&self) -> usize {
        self.value.line_count() + usize::from(self.closes_open_tail())
    }

    fn physical_rows(&self) -> Vec<T> {
        let mut rows = self.value.physical_lines();
        // The callback retires one provisional final delimiter. An explicit
        // cell boundary closes that retained empty source row, rather than
        // silently retiring it along with an occupied row's close.
        if self.closes_open_tail() {
            rows.push(T::plain(""));
        }
        rows
    }
}

#[cfg(test)]
fn table_rows(rows: &[TableRow], mut render_cell: impl FnMut(&TableCell) -> String) -> Vec<String> {
    projected_table_rows(rows, 2, 0, |cell| (render_cell(cell), false)).0
}

/// Prepare every source cell once, then place only its physical rows. Actual
/// parent/field/child origins and generated blanks share the generic bounds;
/// a failed projection reuses prepared cells in source order.
pub(super) fn projected_table_rows<T: TableText>(
    rows: &[TableRow],
    gap_columns: u16,
    origin: i32,
    mut render_cell: impl FnMut(&TableCell) -> (T, bool),
) -> (Vec<T>, bool) {
    let gap = usize::from(gap_columns);
    let mut output = Vec::new();
    let mut completed = false;
    for (plan, row) in bounded_table_rows(rows).into_iter().zip(rows) {
        if mant_ir::table_row_is_navigation_only(row) {
            continue;
        }
        let (lines, closed) = match plan {
            TableRowPlan::Empty => (vec![T::plain("")], true),
            TableRowPlan::WholeRule { double } => {
                (vec![T::plain(if double { "===" } else { "---" })], false)
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
                    (parts, false)
                } else {
                    (vec![T::join(&parts, &separator(gap))], false)
                }
            }
            TableRowPlan::Dense { slots } => dense_physical_rows(
                slots
                    .into_iter()
                    .map(|cell| cell.map(|cell| render_table_cell(cell, &mut render_cell)))
                    .collect(),
                gap,
                origin,
            ),
            TableRowPlan::Sparse { cells } => sparse_physical_rows(
                cells
                    .into_iter()
                    .map(|cell| (cell.column, render_table_cell(cell.cell, &mut render_cell)))
                    .collect(),
                origin,
            ),
        };
        output.extend(lines);
        completed = closed;
    }
    (output, completed)
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
    CellText {
        value,
        completed,
        break_after: cell.break_after,
        origins: mant_ir::geometry::table_cell_origin_bounds(cell),
    }
}

fn stacked_physical_rows<T: TableText>(cells: Vec<CellText<T>>) -> (Vec<T>, bool) {
    let mut completed = false;
    let mut rows = Vec::new();
    for cell in cells {
        if cell.break_after && !cell.completed && cell.value.is_empty() {
            // An empty field can complete the current data row. With no
            // active row (or after an already completed row) it owns one
            // existing empty row, rather than disappearing from topology.
            if rows.is_empty() || completed {
                rows.push(cell.value);
            } else if let Some(last) = rows.last_mut() {
                last.append(&cell.value);
            }
            completed = true;
            continue;
        }
        let physical = cell.physical_rows();
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
) -> (Vec<T>, bool) {
    if cells
        .iter()
        .take(cells.len().saturating_sub(1))
        .flatten()
        .any(|cell| cell.break_after)
    {
        return stacked_physical_rows(cells.into_iter().flatten().collect());
    }
    let height = cells
        .iter()
        .filter_map(Option::as_ref)
        .map(CellText::line_count)
        .max()
        .unwrap_or(1);
    if gap.saturating_mul(cells.len().saturating_sub(1)) > mant_ir::geometry::MAX_COLUMN_PADDING {
        return stacked_physical_rows(cells.into_iter().flatten().collect());
    }
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
        );
    }
    let physical = cells
        .iter()
        .map(|cell| {
            cell.as_ref()
                .map_or_else(|| vec![T::plain("")], CellText::physical_rows)
        })
        .collect::<Vec<_>>();
    let measured = physical
        .iter()
        .map(|rows| rows.iter().map(TableText::width).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut maximum = vec![0_usize; cells.len()];
    for line in 0..height {
        let mut column = 0_usize;
        for (index, widths) in measured.iter().enumerate() {
            maximum[index] = maximum[index].max(column);
            column = column
                .saturating_add(widths.get(line).copied().unwrap_or(0))
                .saturating_add(gap + 1);
        }
    }
    if cells.iter().zip(maximum).any(|(cell, start)| {
        cell.as_ref().is_some_and(|cell| {
            !cell.origins.fits_at(mant_ir::geometry::compose_origin(
                origin,
                mant_ir::geometry::coordinate(start),
            ))
        })
    }) {
        return stacked_physical_rows(cells.into_iter().flatten().collect());
    }
    let completed = cells.iter().zip(&physical).any(|(cell, rows)| {
        cell.as_ref().is_some_and(|cell| {
            (cell.completed || cell.break_after || rows.last().is_some_and(TableText::is_empty))
                && rows.len() == height
        })
    });
    let separator = separator(gap);
    let output = (0..height)
        .map(|line| {
            let mut segments = physical
                .iter()
                .map(|cell| cell.get(line).cloned().unwrap_or_else(|| T::plain("")))
                .collect::<Vec<_>>();
            let mut retired = Vec::new();
            while line > 0 && segments.last().is_some_and(TableText::is_empty) {
                retired.push(segments.pop().expect("empty tail"));
            }
            let mut output = T::join(&segments, &separator);
            for tail in retired.into_iter().rev() {
                output.append(&tail);
            }
            output
        })
        .collect();
    (output, completed)
}

fn sparse_physical_rows<T: TableText>(
    cells: Vec<(usize, CellText<T>)>,
    origin: i32,
) -> (Vec<T>, bool) {
    if cells.iter().any(|(column, cell)| {
        let prefix = format!("column {}: ", column.saturating_add(1));
        !cell.origins.fits_at(mant_ir::geometry::compose_origin(
            origin,
            mant_ir::geometry::coordinate(prefix.len()),
        ))
    }) {
        return stacked_physical_rows(cells.into_iter().map(|(_, cell)| cell).collect());
    }
    let mut completed = false;
    let output = cells
        .into_iter()
        .flat_map(|(column, cell)| {
            let physical = cell.physical_rows();
            completed = cell.completed
                || cell.break_after
                || physical.last().is_some_and(TableText::is_empty);
            physical
                .into_iter()
                .enumerate()
                .map(|(line, value)| {
                    value.prefixed(&if line == 0 {
                        format!("column {}: ", column.saturating_add(1))
                    } else {
                        "  ".into()
                    })
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
    fn line_count(&self) -> usize {
        if self.is_empty() {
            1
        } else {
            self.split_terminator('\n').count()
        }
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
