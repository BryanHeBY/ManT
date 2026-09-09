//! Bounded plain table projection used by text and portable Markdown.
use mant_ir::{TableCell, TableRow, TableRowPlan, bounded_table_rows};

pub(super) fn table_rows(
    rows: &[TableRow],
    render_cell: impl Fn(&TableCell) -> String,
) -> Vec<String> {
    bounded_table_rows(rows)
        .into_iter()
        .map(|row| match row {
            TableRowPlan::Dense { slots } => slots
                .into_iter()
                .map(|cell| cell.map_or_else(String::new, &render_cell))
                .collect::<Vec<_>>()
                .join(" | "),
            TableRowPlan::Sparse { cells } => cells
                .into_iter()
                .map(|positioned| {
                    format!(
                        "column {}: {}",
                        positioned.column.saturating_add(1),
                        render_cell(positioned.cell)
                    )
                })
                .collect::<Vec<_>>()
                .join(" | "),
        })
        .collect()
}
