//! Bounded plain table projection used by text and portable Markdown.
use mant_ir::{TableCell, TableGrid, TableRow};

pub(super) fn table_rows(
    rows: &[TableRow],
    render_cell: impl Fn(&TableCell) -> String,
) -> Vec<String> {
    let grid = TableGrid::new(rows);
    (0..grid.rows.len())
        .map(|row| {
            if let Some(slots) = grid.slots(row, 256) {
                slots
                    .into_iter()
                    .map(|cell| cell.map_or_else(String::new, &render_cell))
                    .collect::<Vec<_>>()
                    .join(" | ")
            } else {
                // Huge spans must not amplify a small IR into megabytes of
                // separators. Keep every payload with an explicit column label.
                grid.rows[row]
                    .iter()
                    .map(|positioned| {
                        format!(
                            "column {}: {}",
                            positioned.column.saturating_add(1),
                            render_cell(positioned.cell)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ")
            }
        })
        .collect()
}
