//! Declared preferences place wrapped cells with their source coordinates.
use super::super::{
    Line, LogicalTableRow, RowCopyMap, Span, WrappedLine, WrappedLink, WrappedSearchCell,
};
use super::cells::wrap_table_cell;

/// The same measured origins and sequential cell rows as plain terminal
/// output, while links/search/anchors travel with their original glyphs.
pub(super) fn render_declared_columns(
    indent: usize,
    table: &LogicalTableRow,
    available: usize,
) -> Option<Vec<WrappedLine>> {
    let columns = mant_ir::geometry::DeclaredColumns::new(&table.layout.column_preferences)?;
    let preferred_start = |index: usize| {
        let start = columns.start(index);
        table
            .layout
            .column_preferences
            .max_alignment_padding_columns
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
        return None;
    }
    let mut group = 0;
    let mut cells = table
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            wrap_table_cell(
                cell,
                available.saturating_sub(preferred_start(index)).max(1),
                0,
                &mut group,
                true,
            )
        })
        .collect::<Vec<_>>();
    let mut widths = cells
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
    let placements = columns.place_at(&widths, mant_ir::geometry::coordinate(indent))?;
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
        return None;
    }
    if !position_declared_cells(
        &placements,
        &mut cells,
        &mut widths,
        &DeclaredArea {
            columns: &columns,
            preferences: &table.layout.column_preferences,
            cells: &table.cells,
            origin: indent,
            available,
        },
    ) {
        return None;
    }
    if placements.iter().flatten().any(|piece| {
        piece
            .column
            .saturating_add(widths[piece.cell][piece.line].output)
            > available
    }) {
        return None;
    }
    Some(
        placements
            .into_iter()
            .map(|pieces| placed_column_row(&pieces, &cells, &widths, indent))
            .collect(),
    )
}

struct DeclaredArea<'a> {
    columns: &'a mant_ir::geometry::DeclaredColumns,
    preferences: &'a mant_ir::ColumnPreferences,
    cells: &'a [super::super::LogicalTableCell],
    origin: usize,
    available: usize,
}

fn position_declared_cells(
    placements: &[Vec<mant_ir::geometry::ColumnPiece>],
    cells: &mut [Vec<WrappedLine>],
    widths: &mut [Vec<mant_ir::geometry::ColumnFieldWidth>],
    area: &DeclaredArea<'_>,
) -> bool {
    // Cell alignment stays in its column. Only owner-local row alignment in
    // the final cell of automatic flow can use the remaining parent width.
    for pieces in placements {
        for (index, piece) in pieces.iter().enumerate() {
            let slot_end = area
                .columns
                .field_end(piece.cell)
                .unwrap_or(area.available)
                .saturating_sub(if piece.cell < area.preferences.widths.len() {
                    usize::from(area.preferences.gap_columns)
                } else {
                    0
                })
                .min(area.available);
            let next_end = pieces.get(index + 1).map_or(area.available, |next| {
                next.column
                    .saturating_sub(usize::from(area.preferences.gap_columns))
            });
            let owner_end = if piece.cell + 1 == cells.len() {
                area.available
            } else {
                slot_end
            }
            .min(next_end);
            let row = &mut cells[piece.cell][piece.line];
            let previous_width = super::super::super::inline::spans_width(&row.line.spans);
            let owner_area = owner_end.saturating_sub(piece.column);
            if row
                .alignment_padding(owner_area)
                .is_some_and(|padding| padding > mant_ir::geometry::MAX_COLUMN_PADDING)
            {
                return false;
            }
            if !row.position_alignment(owner_area) {
                let spare = slot_end
                    .min(next_end)
                    .saturating_sub(piece.column)
                    .saturating_sub(previous_width);
                let offset = match area.cells[piece.cell].alignment {
                    mant_ir::TableAlignment::Left => 0,
                    mant_ir::TableAlignment::Center => spare / 2,
                    mant_ir::TableAlignment::Right => spare,
                };
                if offset > mant_ir::geometry::MAX_COLUMN_PADDING {
                    return false;
                }
                super::translate(row, offset);
            }
            let output = super::super::super::inline::spans_width(&row.line.spans);
            let actual = area
                .origin
                .saturating_add(piece.column)
                .saturating_add(output.saturating_sub(previous_width));
            if !area.cells[piece.cell]
                .origin_bounds
                .fits_at(mant_ir::geometry::coordinate(actual))
            {
                return false;
            }
            widths[piece.cell][piece.line].output = output;
        }
    }
    true
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
        // Empty content rows have no alignment padding; their table/parent origins
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
