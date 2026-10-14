//! Table cells compose one reusable layout into declared or fallback rows.

use super::{BlockRenderer, Flow, LayoutText, indent_lines, padding};

/// One completed cell visit, including whether it produced any physical row.
/// Empty text alone cannot distinguish an absent paragraph from literal rows.
pub(super) struct PreparedCell {
    pub(super) lines: Vec<LayoutText>,
    has_physical_rows: bool,
    completed: bool,
    break_after: bool,
}

impl PreparedCell {
    pub(super) fn new(flow: Flow, break_after: bool) -> Self {
        let has_physical_rows = flow.has_physical_rows();
        let (text, completed) = flow.finish_cell();
        Self {
            lines: text.split(completed),
            has_physical_rows,
            completed,
            break_after,
        }
    }

    fn widths(&self) -> Vec<mant_ir::geometry::ColumnFieldWidth> {
        let mut widths = self
            .lines
            .iter()
            .map(|line| mant_ir::geometry::ColumnFieldWidth::from_text(&line.visible))
            .collect::<Vec<_>>();
        if let Some(last) = widths.last_mut() {
            last.completed = self.completed || self.break_after;
        }
        widths
    }
}

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
        // Intrinsic cell preparation precedes placement. A page viewport is
        // not a cell viewport: alignment padding must not enlarge measured
        // content or change the table's hard-row placement decisions.
        let cell_renderer = BlockRenderer {
            width: None,
            ..*self
        };
        let (physical_rows, completed) = super::super::super::table::projected_table_rows(
            rows,
            preferences.gap_columns,
            origin,
            self.width,
            |cell| cell_renderer.cell_block_flow(&cell.blocks, 0).finish_cell(),
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
        let cell_renderer = BlockRenderer {
            width: None,
            ..*self
        };
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
            for cell in &row.cells {
                let prepared = PreparedCell::new(
                    cell_renderer.cell_block_flow(&cell.blocks, 0),
                    cell.break_after,
                );
                widths.push(prepared.widths());
                cells.push(prepared);
            }
            let bounds = row
                .cells
                .iter()
                .map(mant_ir::geometry::table_cell_origin_bounds)
                .collect::<Vec<_>>();
            let placements = columns.place_at(&widths, origin).filter(|placements| {
                mant_ir::geometry::table_column_origins_fit(&bounds, placements, origin)
            });
            let ends = (0..cells.len())
                .map(|column| {
                    (column + 1 < cells.len())
                        .then(|| columns.field_end(column))
                        .flatten()
                        .map(|end| {
                            if column < preferences.widths.len() {
                                end.saturating_sub(usize::from(preferences.gap_columns))
                            } else {
                                end
                            }
                        })
                })
                .collect::<Vec<_>>();
            output.extend(Self::placed_column_row(
                placements,
                cells,
                &widths,
                origin,
                &ends,
                usize::from(preferences.gap_columns),
                self.width
                    .map(|width| width.saturating_sub(padding(origin))),
            ));
        }
        output
    }

    pub(super) fn placed_column_row(
        placements: Option<Vec<Vec<mant_ir::geometry::ColumnPiece>>>,
        cells: Vec<PreparedCell>,
        widths: &[Vec<mant_ir::geometry::ColumnFieldWidth>],
        origin: i32,
        ends: &[Option<usize>],
        gap: usize,
        viewport: Option<usize>,
    ) -> Flow {
        let mut output = Flow::default();
        let Some(placements) = placements else {
            // Reuse each prepared cell receipt exactly once, preserving
            // its open/completed tail and all decoration on empty rows.
            return Self::stacked_column_row(cells, origin, viewport);
        };
        let mut lines = Vec::new();
        for pieces in placements {
            let mut line = LayoutText::default();
            let mut visible = 0_usize;
            for (index, piece) in pieces.iter().enumerate() {
                line.push_plain(&" ".repeat(piece.column.saturating_sub(visible)));
                let area = viewport.map(|width| {
                    let end = ends
                        .get(piece.cell)
                        .copied()
                        .flatten()
                        .unwrap_or(width)
                        .min(width);
                    let end = pieces
                        .get(index + 1)
                        .map_or(end, |next| end.min(next.column.saturating_sub(gap)));
                    end.saturating_sub(piece.column)
                });
                let content = cells[piece.cell].lines[piece.line]
                    .clone()
                    .position_alignments(area);
                visible = piece
                    .column
                    .saturating_add(mant_ir::geometry::text_width(&content.visible));
                line.append(&content);
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
        // Topology has already selected stacking, so this area's width is
        // known before each cell's single visit. Late placement fallback
        // instead reuses its prepared, unaligned intrinsic cell receipts.
        let cell_renderer = BlockRenderer {
            width: None,
            ..*self
        };
        Self::stacked_column_row(
            cells.iter().map(|cell| {
                PreparedCell::new(cell_renderer.stacked_cell_flow(cell, 0), cell.break_after)
            }),
            origin,
            self.width
                .map(|width| width.saturating_sub(padding(origin))),
        )
    }

    /// Both early topology fallback and late placement fallback consume the
    /// same receipt. No subsequent decision revisits the cell's source tree.
    fn stacked_column_row(
        cells: impl IntoIterator<Item = PreparedCell>,
        origin: i32,
        viewport: Option<usize>,
    ) -> Flow {
        let mut lines = Vec::new();
        let mut open = false;
        let mut completed = false;
        for mut cell in cells {
            cell.lines = cell
                .lines
                .into_iter()
                .map(|row| row.position_alignments(viewport))
                .collect();
            if !cell.has_physical_rows && !cell.break_after {
                continue;
            }
            if cell.break_after
                && !cell.completed
                && cell.lines.len() == 1
                && cell.lines[0].is_empty()
            {
                Self::close_empty_cell(
                    &mut lines,
                    LayoutText::join(cell.lines, ""),
                    completed,
                    &mut open,
                );
                completed = true;
                continue;
            }
            completed = cell.completed || cell.break_after;
            Self::append_stacked_cell(&mut lines, cell.lines, completed, &mut open);
        }
        if lines.is_empty() {
            return Flow::completed_text(LayoutText::default());
        }
        let text = LayoutText::join(lines, "\n").indented(padding(origin));
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
