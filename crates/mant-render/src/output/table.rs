//! Bounded plain table projection for text output.
use mant_ir::{TableCell, TableCellKind, TableRow, TableRowPlan, bounded_table_rows};

/// Dense projection remains useful for ordinary tables, but multiplying an
/// exceptionally tall cell by every logical slot would amplify bounded input
/// into unbounded separator output. Above this work budget, retain every
/// source cell in an explicit sparse form instead.
const MAX_DENSE_PHYSICAL_SLOTS: usize = 16_384;

pub(super) fn table_rows(
    rows: &[TableRow],
    mut render_cell: impl FnMut(&TableCell) -> String,
) -> Vec<String> {
    bounded_table_rows(rows)
        .into_iter()
        .flat_map(|row| match row {
            TableRowPlan::Empty => vec![String::new()],
            TableRowPlan::WholeRule { double } => {
                vec![if double { "===" } else { "---" }.to_owned()]
            }
            TableRowPlan::LayoutRule { cells } => vec![
                cells
                    .iter()
                    .map(|kind| match kind {
                        mant_ir::TableRuleCellKind::Horizontal => "---",
                        mant_ir::TableRuleCellKind::DoubleHorizontal => "===",
                    })
                    .collect::<Vec<_>>()
                    .join(" | "),
            ],
            TableRowPlan::Dense { slots } => dense_physical_rows(
                slots
                    .into_iter()
                    .map(|cell| cell.map(|cell| render_table_cell(cell, &mut render_cell)))
                    .collect(),
            ),
            TableRowPlan::Sparse { cells } => sparse_physical_rows(
                cells
                    .into_iter()
                    .map(|positioned| {
                        (
                            positioned.column,
                            render_table_cell(positioned.cell, &mut render_cell),
                        )
                    })
                    .collect(),
            ),
        })
        .collect()
}

fn render_table_cell(
    cell: &TableCell,
    render_cell: &mut impl FnMut(&TableCell) -> String,
) -> String {
    match cell.kind {
        TableCellKind::Text => render_cell(cell),
        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => "---".to_owned(),
        TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
            "===".to_owned()
        }
    }
}

/// Expand one logical table row into the physical rows retained by its
/// cells.  CVS `tbl_term.c` flushes one line from every cell per pass and
/// repeats the row while any cell has content left; flattening a cell before
/// this point would silently discard formatter-requested line boundaries.
fn dense_physical_rows(cells: Vec<Option<String>>) -> Vec<String> {
    let height = cells
        .iter()
        .filter_map(Option::as_deref)
        .map(line_count)
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
        );
    }
    let cells = cells
        .iter()
        .map(|cell| physical_lines(cell.as_deref().unwrap_or_default()))
        .collect::<Vec<_>>();
    (0..height)
        .map(|line| {
            let mut segments = cells
                .iter()
                .map(|cell| cell.get(line).copied().unwrap_or_default())
                .collect::<Vec<_>>();
            while line > 0 && segments.last().is_some_and(|value| value.is_empty()) {
                segments.pop();
            }
            segments.join(" | ")
        })
        .collect()
}

fn sparse_physical_rows(cells: Vec<(usize, String)>) -> Vec<String> {
    cells
        .into_iter()
        .flat_map(|(column, cell)| {
            physical_lines(&cell)
                .into_iter()
                .enumerate()
                .map(|(line, value)| {
                    if line == 0 {
                        format!("column {}: {value}", column.saturating_add(1))
                    } else {
                        format!("  {value}")
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn line_count(value: &str) -> usize {
    if value.is_empty() {
        1
    } else {
        value.split_terminator('\n').count()
    }
}

fn physical_lines(value: &str) -> Vec<&str> {
    if value.is_empty() {
        vec![""]
    } else {
        value.split_terminator('\n').collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> TableCell {
        TableCell {
            kind: TableCellKind::Text,
            blocks: Vec::new(),
            column_span: 1,
            row_span: 1,
            alignment: None,
            source: None,
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
