//! Cell-local columns and origin-preserving table fallback.
use super::super::{Arc, LogicalLine, LogicalTableCell, LogicalTableLayout};
use super::DocumentBuilder;
use mant_ir::geometry::padding;
use mant_ir::{TableRow, TableRowKind};

fn needs_cell_layout(row: &TableRow) -> bool {
    matches!(row.kind, TableRowKind::Data)
        && !row.cells.is_empty()
        && !mant_ir::table_row_is_navigation_only(row)
}

impl DocumentBuilder<'_> {
    pub(super) fn table(&mut self, rows: &[TableRow], column_widths: &[u16], indent: i32) {
        if rows.is_empty() {
            return;
        }
        if mant_ir::geometry::table_requires_origin_preserving_stack(rows, indent) {
            self.stacked_table(rows, indent);
            return;
        }
        let grid = mant_ir::TableGrid::new(rows);
        let rendered_rows = (0..grid.rows.len())
            .map(|row| {
                if !needs_cell_layout(&rows[row]) {
                    return None;
                }
                grid.slots(row, 256)
                    .unwrap_or_else(|| {
                        grid.rows[row]
                            .iter()
                            .map(|positioned| Some(positioned.cell))
                            .collect()
                    })
                    .into_iter()
                    .map(|cell| self.prepare_table_cell(cell))
                    .collect::<Vec<_>>()
                    .into()
            })
            .collect::<Vec<_>>();
        let layout_rows = rendered_rows
            .iter()
            .filter_map(Option::as_ref)
            .cloned()
            .collect::<Vec<_>>();
        let mut table_layout = LogicalTableLayout::for_rows(&layout_rows);
        table_layout.force_stack = grid.column_count > 256;
        if !column_widths.is_empty() {
            // Both terminal consumers interpret the same declaration facts.
            // Complex topology retains the established source-order fallback.
            let simple = rows.iter().all(|row| {
                matches!(row.kind, TableRowKind::Data)
                    && row.cells.iter().all(|cell| {
                        cell.kind == mant_ir::TableCellKind::Text
                            && cell.column_span == 1
                            && cell.row_span == 1
                    })
            });
            table_layout.force_stack |=
                !simple || mant_ir::geometry::DeclaredColumns::new(column_widths).is_none();
            if simple && !table_layout.force_stack {
                table_layout.declared_widths = column_widths.to_vec();
            }
        }
        let table_layout = Arc::new(table_layout);
        for (row, rendered) in rows.iter().zip(rendered_rows) {
            if self.defer_navigation_row(row, indent) {
                continue;
            }
            match &row.kind {
                TableRowKind::Data if row.cells.is_empty() => self.push(LogicalLine::empty()),
                TableRowKind::Data => self.push(LogicalLine::table(
                    padding(indent),
                    rendered.expect("data row was rendered"),
                    Arc::clone(&table_layout),
                )),
                TableRowKind::HorizontalRule => {
                    self.push(LogicalLine::rule(padding(indent)));
                }
                TableRowKind::DoubleHorizontalRule => {
                    self.push(LogicalLine::double_rule(padding(indent)));
                }
                TableRowKind::LayoutRule { cells } => self.push(LogicalLine::table_rule(
                    padding(indent),
                    cells.clone(),
                    Arc::clone(&table_layout),
                )),
            }
        }
    }

    fn prepare_table_cell(&self, cell: Option<&mant_ir::TableCell>) -> LogicalTableCell {
        let mut builder = Self::new(String::new(), self.address.clone());
        builder.entry_styles = Arc::clone(&self.entry_styles);
        builder.reference_origins = Arc::clone(&self.reference_origins);
        if let Some(cell) = cell {
            match cell.kind {
                mant_ir::TableCellKind::Text => {
                    builder.table_cell_blocks(&cell.blocks, 0);
                }
                mant_ir::TableCellKind::HorizontalRule
                | mant_ir::TableCellKind::IsolatedHorizontalRule => {
                    builder.push(LogicalLine::rule(0));
                }
                mant_ir::TableCellKind::DoubleHorizontalRule
                | mant_ir::TableCellKind::IsolatedDoubleHorizontalRule => {
                    builder.push(LogicalLine::double_rule(0));
                }
            }
        }
        let completed_tail = builder.pending_gap.rows(0) > 0;
        let content = builder.finish().content;
        let mut rendered =
            LogicalTableCell::new(content.lines, cell.and_then(|cell| cell.alignment));
        rendered.anchors = content.anchors;
        rendered.completed_tail = completed_tail;
        rendered
    }

    fn stacked_table(&mut self, rows: &[TableRow], indent: i32) {
        for row in rows {
            if self.defer_navigation_row(row, indent) {
                continue;
            }
            match &row.kind {
                TableRowKind::Data if row.cells.is_empty() => self.push(LogicalLine::empty()),
                TableRowKind::Data => {
                    let start = self.lines.len();
                    for cell in &row.cells {
                        match cell.kind {
                            mant_ir::TableCellKind::Text => {
                                self.table_cell_blocks(&cell.blocks, indent);
                            }
                            mant_ir::TableCellKind::HorizontalRule
                            | mant_ir::TableCellKind::IsolatedHorizontalRule => {
                                self.push(LogicalLine::rule(padding(indent)));
                            }
                            mant_ir::TableCellKind::DoubleHorizontalRule
                            | mant_ir::TableCellKind::IsolatedDoubleHorizontalRule => {
                                self.push(LogicalLine::double_rule(padding(indent)));
                            }
                        }
                    }
                    if self.lines.len() == start {
                        // Real data-row topology survives even when its
                        // cells have no glyphs or literal row payload.
                        self.push(LogicalLine::empty());
                    }
                }
                TableRowKind::HorizontalRule => self.push(LogicalLine::rule(padding(indent))),
                TableRowKind::DoubleHorizontalRule => {
                    self.push(LogicalLine::double_rule(padding(indent)));
                }
                TableRowKind::LayoutRule { cells } => {
                    let layout = Arc::new(LogicalTableLayout {
                        preferred_widths: vec![1; cells.len()],
                        force_stack: true,
                        declared_widths: Vec::new(),
                    });
                    self.push(LogicalLine::table_rule(
                        padding(indent),
                        cells.clone(),
                        layout,
                    ));
                }
            }
        }
    }

    fn defer_navigation_row(&mut self, row: &TableRow, indent: i32) -> bool {
        if !mant_ir::table_row_is_navigation_only(row) {
            return false;
        }
        for cell in &row.cells {
            // Reuse normal zero-width inline ownership: targets wait for
            // the next physical output row without creating one.
            self.blocks(&cell.blocks, indent);
        }
        true
    }
}
