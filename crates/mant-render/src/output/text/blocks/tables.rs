//! Table cells compose one reusable layout into declared or fallback rows.

use super::{BlockRenderer, Flow, LayoutText, indent_lines, padding};

impl BlockRenderer<'_> {
    pub(super) fn table_flow(
        &self,
        rows: &[mant_ir::TableRow],
        preferences: &mant_ir::ColumnPreferences,
        origin: i32,
    ) -> Flow {
        if mant_ir::geometry::table_requires_origin_preserving_stack(rows, origin) {
            return self.stacked_table_flow(rows, preferences.gap_columns, origin, true);
        }
        if !preferences.widths.is_empty() {
            return self.declared_column_flow(rows, preferences, origin);
        }
        let (physical_rows, completed) = super::super::super::table::projected_table_rows(
            rows,
            preferences.gap_columns,
            origin,
            |cell| self.cell_block_flow(&cell.blocks, 0).finish_cell(),
        );
        let value = LayoutText::join(physical_rows.iter().cloned(), "\n");
        if physical_rows.is_empty() {
            Flow::default()
        } else if value.is_empty() {
            // The table plan owns this physical data row. Empty cells can
            // share it, but its whole-row boundary consumes the incoming gap.
            Flow::completed_text(value)
        } else if completed {
            Flow::completed_text(value.indented(padding(origin)))
        } else {
            Flow::text(value.indented(padding(origin)))
        }
    }

    /// Shared declared-field placement. Complex topology uses the existing
    /// source-order fallback; declarations never bypass origin preservation.
    fn declared_column_flow(
        &self,
        rows: &[mant_ir::TableRow],
        preferences: &mant_ir::ColumnPreferences,
        origin: i32,
    ) -> Flow {
        let Some(columns) = mant_ir::geometry::DeclaredColumns::new(preferences) else {
            return self.stacked_table_flow(rows, preferences.gap_columns, origin, false);
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
            return self.stacked_table_flow(rows, preferences.gap_columns, origin, false);
        }
        let mut output = Flow::default();
        for row in rows {
            if mant_ir::table_row_is_navigation_only(row) {
                continue;
            }
            if row.cells.is_empty() {
                output.extend(Flow::completed_text(LayoutText::default()));
                continue;
            }
            // A hard inline break leaves an open final row which the next
            // cell may use. A completed vertical row owns its final delimiter
            // and cannot be reused; both consumers pass that fact to the plan.
            let mut cells = Vec::with_capacity(row.cells.len());
            let mut widths = Vec::with_capacity(row.cells.len());
            let mut empty_closures = Vec::with_capacity(row.cells.len());
            for cell in &row.cells {
                let (text, completed) = self.cell_block_flow(&cell.blocks, 0).finish_cell();
                empty_closures.push(cell.break_after && !completed && text.is_empty());
                let lines = text.split(completed);
                let mut measured = lines
                    .iter()
                    .map(|line| mant_ir::geometry::ColumnFieldWidth::from_text(&line.visible))
                    .collect::<Vec<_>>();
                if let Some(last) = measured.last_mut() {
                    last.completed = completed || cell.break_after;
                }
                cells.push(lines);
                widths.push(measured);
            }
            let bounds = row
                .cells
                .iter()
                .map(mant_ir::geometry::table_cell_origin_bounds)
                .collect::<Vec<_>>();
            let placements = columns.place_at(&widths, origin).filter(|placements| {
                mant_ir::geometry::table_column_origins_fit(&bounds, placements, origin)
            });
            output.extend(Self::placed_column_row(
                placements,
                cells,
                &widths,
                &empty_closures,
                origin,
            ));
        }
        output
    }

    pub(super) fn placed_column_row(
        placements: Option<Vec<Vec<mant_ir::geometry::ColumnPiece>>>,
        cells: Vec<Vec<LayoutText>>,
        widths: &[Vec<mant_ir::geometry::ColumnFieldWidth>],
        empty_closures: &[bool],
        origin: i32,
    ) -> Flow {
        let mut output = Flow::default();
        let Some(placements) = placements else {
            // Reuse each prepared cell receipt exactly once, preserving
            // its open/completed tail and all decoration on empty rows.
            let mut rows = Vec::new();
            let mut open = false;
            let mut completed = false;
            for ((cell, measured), empty_closure) in
                cells.into_iter().zip(widths).zip(empty_closures)
            {
                if *empty_closure {
                    let decoration = LayoutText::join(cell, "");
                    Self::close_empty_cell(&mut rows, decoration, completed, &mut open);
                    completed = true;
                } else {
                    completed = measured.last().is_some_and(|row| row.completed);
                    Self::append_stacked_cell(&mut rows, cell, completed, &mut open);
                }
            }
            let text = LayoutText::join(rows, "\n").indented(padding(origin));
            output.extend(if completed {
                Flow::completed_text(text)
            } else {
                Flow::literal(text)
            });
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
        let text = LayoutText::join(lines, "\n").indented(padding(origin));
        output.extend(
            if widths
                .last()
                .and_then(|rows| rows.last())
                .is_some_and(|row| row.completed)
            {
                Flow::completed_text(text)
            } else {
                Flow::literal(text)
            },
        );
        output
    }

    fn close_empty_cell(
        rows: &mut Vec<LayoutText>,
        decoration: LayoutText,
        completed: bool,
        open: &mut bool,
    ) {
        if rows.is_empty() || completed {
            rows.push(decoration);
        } else if let Some(last) = rows.last_mut() {
            last.append(&decoration);
        }
        *open = false;
    }

    fn append_stacked_cell(
        rows: &mut Vec<LayoutText>,
        cell: Vec<LayoutText>,
        completed: bool,
        open: &mut bool,
    ) {
        let mut decoration = LayoutText::default();
        if *open && let Some(tail) = rows.pop() {
            decoration = tail;
        }
        let cell_open =
            !completed && cell.len() > 1 && cell.last().is_some_and(LayoutText::is_empty);
        let mut cell = cell.into_iter();
        if let Some(first) = cell.next() {
            decoration.append(&first);
            rows.push(decoration);
            rows.extend(cell);
        }
        *open = cell_open;
    }

    fn stacked_table_flow(
        &self,
        rows: &[mant_ir::TableRow],
        gap_columns: u16,
        origin: i32,
        share_gap: bool,
    ) -> Flow {
        let mut output = Flow::default();
        for row in rows {
            if mant_ir::table_row_is_navigation_only(row) {
                continue;
            }
            match &row.kind {
                mant_ir::TableRowKind::Data if row.cells.is_empty() => {
                    output.extend(Flow::completed_text(LayoutText::default()));
                }
                mant_ir::TableRowKind::Data => {
                    output.extend(if share_gap {
                        self.stacked_data_row(&row.cells, origin)
                    } else {
                        self.independent_stacked_row(&row.cells, origin)
                    });
                }
                mant_ir::TableRowKind::HorizontalRule => {
                    output.push_text(indent_lines("---", padding(origin)).into());
                }
                mant_ir::TableRowKind::DoubleHorizontalRule => {
                    output.push_text(indent_lines("===", padding(origin)).into());
                }
                mant_ir::TableRowKind::LayoutRule { cells } => {
                    let parts = cells
                        .iter()
                        .map(|kind| match kind {
                            mant_ir::TableRuleCellKind::Horizontal => "---",
                            mant_ir::TableRuleCellKind::DoubleHorizontal => "===",
                        })
                        .collect::<Vec<_>>();
                    let gap = usize::from(gap_columns);
                    if gap.saturating_mul(parts.len().saturating_sub(1))
                        > mant_ir::geometry::MAX_COLUMN_PADDING
                    {
                        for part in parts {
                            output.push_text(indent_lines(part, padding(origin)).into());
                        }
                    } else {
                        let separator =
                            format!("{}|{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2));
                        output.push_text(
                            indent_lines(&parts.join(&separator), padding(origin)).into(),
                        );
                    }
                }
            }
        }
        output
    }

    fn stacked_data_row(&self, cells: &[mant_ir::TableCell], origin: i32) -> Flow {
        let mut output = Flow::default();
        for cell in cells {
            let flow = self.stacked_cell_flow(cell, origin);
            if flow.has_physical_rows() || cell.break_after {
                output.append_stack_cell(flow, cell.break_after);
            }
        }
        if !output.has_physical_rows() {
            // False empty cells can share this already owned data row. They
            // do not erase its whole-row physical boundary.
            output.extend(Flow::completed_text(LayoutText::default()));
        }
        output.complete_table_gap();
        output
    }

    /// A fallback selected only by column topology still has independent
    /// cell budgets. Reuse its original physical receipts and tail policy.
    fn independent_stacked_row(&self, cells: &[mant_ir::TableCell], origin: i32) -> Flow {
        let mut lines = Vec::new();
        let mut open = false;
        let mut completed = false;
        for cell in cells {
            let flow = self.stacked_cell_flow(cell, origin);
            if !flow.has_physical_rows() && !cell.break_after {
                continue;
            }
            let (text, closed) = flow.finish_cell();
            if cell.break_after && !closed && text.is_empty() {
                Self::close_empty_cell(&mut lines, text, completed, &mut open);
                completed = true;
                continue;
            }
            let physical = text.split(closed);
            completed = closed || cell.break_after;
            Self::append_stacked_cell(&mut lines, physical, completed, &mut open);
        }
        if lines.is_empty() {
            return Flow::completed_text(LayoutText::default());
        }
        let text = LayoutText::join(lines, "\n");
        if completed {
            Flow::completed_text(text)
        } else {
            Flow::literal(text)
        }
    }

    fn stacked_cell_flow(&self, cell: &mant_ir::TableCell, origin: i32) -> Flow {
        match cell.kind {
            mant_ir::TableCellKind::Text => self.cell_block_flow(&cell.blocks, origin),
            mant_ir::TableCellKind::HorizontalRule
            | mant_ir::TableCellKind::IsolatedHorizontalRule => {
                Flow::text(indent_lines("---", padding(origin)).into())
            }
            mant_ir::TableCellKind::DoubleHorizontalRule
            | mant_ir::TableCellKind::IsolatedDoubleHorizontalRule => {
                Flow::text(indent_lines("===", padding(origin)).into())
            }
        }
    }
}
