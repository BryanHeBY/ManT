//! Declared preferences place wrapped cells with their source coordinates.
use super::super::{
    Line, LogicalTableRow, RowCopyMap, Span, WrappedLine, WrappedLink, WrappedSearchCell,
};
use super::cells::{stack_table_cells, wrap_table_cell};

/// The same measured origins and sequential cell rows as plain terminal
/// output, while links/search/anchors travel with their original glyphs.
pub(super) fn render_declared_columns(
    indent: usize,
    table: &LogicalTableRow,
    width: usize,
    available: usize,
) -> Vec<WrappedLine> {
    let Some(columns) = mant_ir::geometry::DeclaredColumns::new(&table.layout.column_preferences)
    else {
        return stack_table_cells(indent, table, width);
    };
    let preferred_start = |index: usize| {
        let start = columns.start(index);
        table
            .layout
            .column_preferences
            .advance_limit_columns
            .map_or(start, |limit| {
                start.min(usize::from(limit).saturating_mul(index))
            })
    };
    if table.cells.len() > mant_ir::geometry::MAX_DECLARED_COLUMNS
        || table
            .cells
            .iter()
            .enumerate()
            .any(|(i, _)| preferred_start(i) >= available)
    {
        return stack_table_cells(indent, table, width);
    }
    let mut group = 0;
    let cells = table
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            wrap_table_cell(
                cell,
                available.saturating_sub(preferred_start(index)).max(1),
                0,
                &mut group,
            )
        })
        .collect::<Vec<_>>();
    let widths = cells
        .iter()
        .zip(&table.cells)
        .map(|(rows, cell)| {
            rows.iter()
                .enumerate()
                .map(|(index, row)| mant_ir::geometry::ColumnFieldWidth {
                    content: mant_ir::geometry::declared_field_width(&row.line.to_string()),
                    output: super::super::super::inline::spans_width(&row.line.spans),
                    completed: cell.completed_tail && index + 1 == rows.len(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let Some(placements) = columns.place_at(&widths, mant_ir::geometry::coordinate(indent)) else {
        return stack_table_cells(indent, table, width);
    };
    let bounds = table
        .cells
        .iter()
        .map(|cell| cell.origin_bounds)
        .collect::<Vec<_>>();
    if !mant_ir::geometry::table_column_origins_fit(
        &bounds,
        &placements,
        mant_ir::geometry::coordinate(indent),
    ) {
        return stack_table_cells(indent, table, width);
    }
    if placements.iter().flatten().any(|piece| {
        piece
            .column
            .saturating_add(widths[piece.cell][piece.line].output)
            > available
    }) {
        return stack_table_cells(indent, table, width);
    }
    placements
        .into_iter()
        .map(|pieces| placed_column_row(&pieces, &cells, &widths, indent))
        .collect()
}

fn placed_column_row(
    pieces: &[mant_ir::geometry::ColumnPiece],
    cells: &[Vec<WrappedLine>],
    widths: &[Vec<mant_ir::geometry::ColumnFieldWidth>],
    indent: usize,
) -> WrappedLine {
    let mut spans = vec![Span::raw(" ".repeat(indent))];
    let mut links = Vec::new();
    let mut search_cells = Vec::new();
    let mut anchors = Vec::new();
    let mut copy_map = RowCopyMap::default();
    let mut visible = 0_usize;
    for piece in pieces {
        let row = &cells[piece.cell][piece.line];
        spans.push(Span::raw(" ".repeat(piece.column.saturating_sub(visible))));
        spans.extend(row.line.spans.clone());
        let offset = indent.saturating_add(piece.column);
        copy_map.append_shifted(
            &row.copy_map,
            offset,
            super::super::super::inline::spans_width(&row.line.spans),
        );
        links.extend(row.links.iter().map(|link| WrappedLink {
            target: link.target.clone(),
            start_column: offset.saturating_add(link.start_column),
            end_column: offset.saturating_add(link.end_column),
        }));
        search_cells.extend(row.search_cells.iter().map(|cell| WrappedSearchCell {
            group: cell.group,
            join_before: cell.join_before,
            character: cell.character,
            start_column: offset.saturating_add(cell.start_column),
            end_column: offset.saturating_add(cell.end_column),
        }));
        anchors.extend(row.anchors.iter().cloned());
        visible = piece
            .column
            .saturating_add(widths[piece.cell][piece.line].output);
    }
    if pieces.iter().all(|piece| {
        let row = &cells[piece.cell][piece.line];
        row.source_end.is_none() && row.line.spans.iter().all(|span| span.content.is_empty())
    }) {
        // Empty device rows have no advance; their table/parent origins
        // cannot become visible blank cells. Anchors still own this row.
        spans.clear();
    }
    WrappedLine {
        source_end: None,
        anchors,
        line: Line::from(spans),
        links,
        search_cells,
        copy_map,
    }
}
