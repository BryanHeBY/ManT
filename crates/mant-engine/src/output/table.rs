//! Bounded plain table projection used by text and portable Markdown.
use mant_ir::{TableCell, TableGrid, TableRow};

pub(super) fn table_rows(
    rows: &[TableRow],
    render_cell: impl Fn(&TableCell) -> String,
) -> Vec<String> {
    table_slots(rows)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(label, cell)| {
                    let text = cell.map_or_else(String::new, &render_cell);
                    match label {
                        Some(column) => format!("column {column}: {text}"),
                        None => text,
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect()
}

/// One bounded row plan for both plain and source-mapped text composition.
/// A label is used only when dense slots would exceed the shared 256-column cap.
pub(super) fn table_slots(rows: &[TableRow]) -> Vec<Vec<(Option<usize>, Option<&TableCell>)>> {
    let grid = TableGrid::new(rows);
    (0..grid.rows.len())
        .map(|row| {
            if let Some(slots) = grid.slots(row, 256) {
                slots
                    .into_iter()
                    .map(|cell| (None, cell))
                    .collect::<Vec<_>>()
            } else {
                // Huge spans must not amplify a small IR into megabytes of
                // separators. Keep every payload with an explicit column label.
                grid.rows[row]
                    .iter()
                    .map(|positioned| {
                        (
                            Some(positioned.column.saturating_add(1)),
                            Some(positioned.cell),
                        )
                    })
                    .collect::<Vec<_>>()
            }
        })
        .collect()
}
