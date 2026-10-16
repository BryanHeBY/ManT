//! Table cells compose one reusable layout into declared or fallback rows.

use super::{BlockRenderer, Flow, LayoutText, indent_lines, padding};
mod declared;

/// One completed cell visit, including whether it produced any physical row.
/// Empty text alone cannot distinguish an absent paragraph from literal rows.
pub(super) struct PreparedCell {
    pub(super) lines: Vec<LayoutText>,
    has_physical_rows: bool,
    completed: bool,
    break_after: bool,
    alignment: Option<mant_ir::TableAlignment>,
    origins: mant_ir::geometry::CellOriginBounds,
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
            alignment: None,
            origins: mant_ir::geometry::CellOriginBounds::default(),
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
        alignment: Option<mant_ir::TableAlignment>,
        column_count: Option<u32>,
        borders: &mant_ir::TableBorders,
        origin: i32,
    ) -> Flow {
        if mant_ir::geometry::table_requires_origin_preserving_stack(rows, origin) {
            return self
                .stacked_table_flow(rows, preferences.gap_columns, origin, true)
                .frame_resolved_table(borders.outer, None, self.width, origin);
        }
        if !preferences.widths.is_empty() {
            if borders.outer.is_none() {
                return self.declared_column_flow(
                    rows,
                    preferences,
                    alignment,
                    column_count,
                    origin,
                );
            }
            return self
                .declared_column_flow(
                    rows,
                    preferences,
                    Some(mant_ir::TableAlignment::Left),
                    column_count,
                    origin,
                )
                .frame_resolved_table(borders.outer, alignment, self.width, origin);
        }
        // Intrinsic cell preparation precedes placement. A page viewport is
        // not a cell viewport: alignment padding must not enlarge measured
        // content or change the table's hard-row placement decisions.
        let cell_renderer = BlockRenderer {
            width: None,
            ..*self
        };
        let physical_rows = super::super::super::table::projected_table_rows(
            rows,
            super::super::super::table::TableProjection {
                gap: preferences.gap_columns,
                origin,
                viewport: self.width,
                bounded_frame: alignment.is_some(),
                column_count,
            },
            borders,
            |cell| {
                let flow = cell_renderer.cell_block_flow(&cell.blocks, 0);
                let physical = flow.has_physical_rows();
                let (text, completed) = flow.finish_cell();
                (text, completed, physical)
            },
        );
        let logical = mant_ir::TableGrid::with_column_count(rows, column_count).column_count;
        let mut output = Flow::default();
        let mut frame_width = 0;
        let mut stacked = logical > mant_ir::MAX_DENSE_TABLE_COLUMNS;
        for row in physical_rows {
            match row {
                super::super::super::table::ProjectedTableRow::FrameWidth(width) => {
                    frame_width = frame_width.max(width);
                }
                super::super::super::table::ProjectedTableRow::Content(
                    rows,
                    completed,
                    fallback,
                ) => {
                    stacked |= fallback;
                    if rows.is_empty() {
                        continue;
                    }
                    let value = LayoutText::join(rows, "\n").indented(padding(origin));
                    output.extend(if value.is_empty() || completed {
                        Flow::completed_text(value)
                    } else {
                        Flow::text(value)
                    });
                }
                super::super::super::table::ProjectedTableRow::Rule(cells, widths) => {
                    output.push_table_rule(cells, preferences.gap_columns, widths, true);
                }
            }
        }
        let minimum = (!stacked
            && borders.is_empty()
            && logical <= mant_ir::MAX_DENSE_TABLE_COLUMNS)
            .then(|| {
                logical.saturating_add(
                    (usize::from(preferences.gap_columns) + 1)
                        .saturating_mul(logical.saturating_sub(1)),
                )
            });
        let minimum = minimum.map_or(Some(frame_width), |minimum| Some(minimum.max(frame_width)));
        output.position_table(
            if stacked { None } else { alignment },
            self.width,
            origin,
            minimum,
        )
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
                    output.push_table_rule(
                        vec![mant_ir::TableRuleCellKind::Horizontal],
                        gap_columns,
                        None,
                        true,
                    );
                }
                mant_ir::TableRowKind::DoubleHorizontalRule => {
                    output.push_table_rule(
                        vec![mant_ir::TableRuleCellKind::DoubleHorizontal],
                        gap_columns,
                        None,
                        true,
                    );
                }
                mant_ir::TableRowKind::LayoutRule { cells } => {
                    output.push_table_rule(cells.clone(), gap_columns, None, true);
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
