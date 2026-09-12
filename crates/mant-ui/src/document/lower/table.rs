//! Cell-local columns and origin-preserving table fallback.
use super::super::{Arc, LogicalLine, LogicalTableCell, LogicalTableLayout};
use super::DocumentBuilder;
use mant_ir::geometry::padding;
use mant_ir::{TableRow, TableRowKind};

impl DocumentBuilder<'_> {
    pub(super) fn table(&mut self, rows: &[TableRow], indent: i32) {
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
                if !matches!(rows[row].kind, TableRowKind::Data) || rows[row].cells.is_empty() {
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
                    .map(|cell| {
                        let mut builder = Self::new(String::new(), self.address.clone());
                        builder.entry_styles = Arc::clone(&self.entry_styles);
                        builder.reference_origins = Arc::clone(&self.reference_origins);
                        if let Some(cell) = cell {
                            match cell.kind {
                                mant_ir::TableCellKind::Text => builder.blocks(&cell.blocks, 0),
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
                        let content = builder.finish().content;
                        let mut rendered = LogicalTableCell::new(
                            content.lines,
                            cell.and_then(|cell| cell.alignment),
                        );
                        rendered.anchors = content.anchors;
                        rendered
                    })
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
        let table_layout = Arc::new(table_layout);
        for (row, rendered) in rows.iter().zip(rendered_rows) {
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

    fn stacked_table(&mut self, rows: &[TableRow], indent: i32) {
        for row in rows {
            match &row.kind {
                TableRowKind::Data if row.cells.is_empty() => self.push(LogicalLine::empty()),
                TableRowKind::Data => {
                    for cell in &row.cells {
                        match cell.kind {
                            mant_ir::TableCellKind::Text => self.blocks(&cell.blocks, indent),
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
                }
                TableRowKind::HorizontalRule => self.push(LogicalLine::rule(padding(indent))),
                TableRowKind::DoubleHorizontalRule => {
                    self.push(LogicalLine::double_rule(padding(indent)));
                }
                TableRowKind::LayoutRule { cells } => {
                    let layout = Arc::new(LogicalTableLayout {
                        preferred_widths: vec![1; cells.len()],
                        force_stack: true,
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
}
