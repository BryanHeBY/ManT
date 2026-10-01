//! Table cells compose one reusable layout into declared or fallback rows.

use super::{BlockRenderer, Flow, LayoutText, TableCell, indent_lines, padding};

impl BlockRenderer<'_> {
    fn cell_layout(&self, cell: &TableCell) -> LayoutText {
        self.block_flow(&cell.blocks, 0).finish_cell().0
    }

    pub(super) fn table_flow(
        &self,
        rows: &[mant_ir::TableRow],
        column_widths: &[u16],
        origin: i32,
    ) -> Flow {
        if mant_ir::geometry::table_requires_origin_preserving_stack(rows, origin) {
            return self.stacked_table_flow(rows, origin);
        }
        if !column_widths.is_empty() {
            return self.declared_column_flow(rows, column_widths, origin);
        }
        let physical_rows =
            super::super::super::table::projected_table_rows(rows, |cell| self.cell_layout(cell));
        let value = LayoutText::join(physical_rows.iter().cloned(), "\n");
        if physical_rows.is_empty() {
            Flow::default()
        } else if value.is_empty() {
            // One real, empty tbl row is layout, not an absent table. Keep it
            // in the shared gap flow so surrounding blocks retain exactly one
            // blank physical row without inventing whitespace cell content.
            let mut flow = Flow::default();
            flow.gap(1);
            flow
        } else {
            Flow::text(value.indented(padding(origin)))
        }
    }

    /// Shared declared-field placement. Complex topology uses the existing
    /// source-order fallback; declarations never bypass origin preservation.
    fn declared_column_flow(
        &self,
        rows: &[mant_ir::TableRow],
        column_widths: &[u16],
        origin: i32,
    ) -> Flow {
        let Some(columns) = mant_ir::geometry::DeclaredColumns::new(column_widths) else {
            return self.stacked_table_flow(rows, origin);
        };
        if rows.iter().any(|row| {
            !matches!(row.kind, mant_ir::TableRowKind::Data)
                || row.cells.len() > mant_ir::geometry::MAX_DECLARED_COLUMNS
                || row.cells.iter().any(|cell| {
                    cell.column_span != 1
                        || cell.row_span != 1
                        || cell.kind != mant_ir::TableCellKind::Text
                })
        }) {
            return self.stacked_table_flow(rows, origin);
        }
        let mut output = Flow::default();
        for row in rows {
            if mant_ir::table_row_is_navigation_only(row) {
                continue;
            }
            if row.cells.is_empty() {
                output.gap(1);
                continue;
            }
            // A hard inline break leaves an open final row which the next
            // cell may use. A completed vertical row owns its final delimiter
            // and cannot be reused; both consumers pass that fact to the plan.
            let mut cells = Vec::with_capacity(row.cells.len());
            let mut widths = Vec::with_capacity(row.cells.len());
            for cell in &row.cells {
                let (text, completed) = self.block_flow(&cell.blocks, 0).finish_cell();
                let lines = text.split(completed);
                let mut measured = lines
                    .iter()
                    .map(|line| mant_ir::geometry::ColumnFieldWidth::from_text(&line.visible))
                    .collect::<Vec<_>>();
                if let Some(last) = measured.last_mut() {
                    last.completed = completed;
                }
                cells.push(lines);
                widths.push(measured);
            }
            output.extend(Self::placed_column_row(&columns, cells, &widths, origin));
        }
        output
    }

    pub(super) fn placed_column_row(
        columns: &mant_ir::geometry::DeclaredColumns,
        cells: Vec<Vec<LayoutText>>,
        widths: &[Vec<mant_ir::geometry::ColumnFieldWidth>],
        origin: i32,
    ) -> Flow {
        let mut output = Flow::default();
        let Some(placements) = columns.place(widths) else {
            // Defensive fallback reuses the completed layouts. In normal
            // input it is excluded by the preflight cell budget and the
            // content <= output invariant of ColumnFieldWidth::from_text.
            for cell in cells {
                output.extend(Flow::literal(
                    LayoutText::join(cell, "\n").indented(padding(origin)),
                ));
            }
            return output;
        };
        let mut lines = Vec::new();
        for pieces in placements {
            let mut line = LayoutText::default();
            let mut visible = 0_usize;
            for piece in pieces {
                line.push_plain(&" ".repeat(piece.column.saturating_sub(visible)));
                line.append(&cells[piece.cell][piece.line]);
                visible = piece
                    .column
                    .saturating_add(widths[piece.cell][piece.line].output);
            }
            lines.push(line);
        }
        output.extend(Flow::literal(
            LayoutText::join(lines, "\n").indented(padding(origin)),
        ));
        output
    }

    fn stacked_table_flow(&self, rows: &[mant_ir::TableRow], origin: i32) -> Flow {
        let mut output = Flow::default();
        for row in rows {
            if mant_ir::table_row_is_navigation_only(row) {
                continue;
            }
            match &row.kind {
                mant_ir::TableRowKind::Data if row.cells.is_empty() => output.gap(1),
                mant_ir::TableRowKind::Data => {
                    let mut row_flow = Flow::default();
                    for cell in &row.cells {
                        match cell.kind {
                            mant_ir::TableCellKind::Text => {
                                row_flow.extend(self.block_flow(&cell.blocks, origin));
                            }
                            mant_ir::TableCellKind::HorizontalRule
                            | mant_ir::TableCellKind::IsolatedHorizontalRule => {
                                row_flow.push_text(indent_lines("---", padding(origin)).into());
                            }
                            mant_ir::TableCellKind::DoubleHorizontalRule
                            | mant_ir::TableCellKind::IsolatedDoubleHorizontalRule => {
                                row_flow.push_text(indent_lines("===", padding(origin)).into());
                            }
                        }
                    }
                    if row_flow.has_physical_rows() {
                        output.extend(row_flow);
                    } else {
                        output.gap(1);
                    }
                }
                mant_ir::TableRowKind::HorizontalRule => {
                    output.push_text(indent_lines("---", padding(origin)).into());
                }
                mant_ir::TableRowKind::DoubleHorizontalRule => {
                    output.push_text(indent_lines("===", padding(origin)).into());
                }
                mant_ir::TableRowKind::LayoutRule { cells } => {
                    let row = cells
                        .iter()
                        .map(|kind| match kind {
                            mant_ir::TableRuleCellKind::Horizontal => "---",
                            mant_ir::TableRuleCellKind::DoubleHorizontal => "===",
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    output.push_text(indent_lines(&row, padding(origin)).into());
                }
            }
        }
        output
    }
}
