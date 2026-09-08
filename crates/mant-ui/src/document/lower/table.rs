//! Cell-local columns and origin-preserving table fallback.
use super::super::{Arc, LogicalLine, LogicalTableCell, LogicalTableLayout};
use super::DocumentBuilder;
use mant_ir::TableRow;
use mant_protocol::geometry::padding;

impl DocumentBuilder<'_> {
    pub(super) fn table(&mut self, rows: &[TableRow], indent: i32) {
        if mant_protocol::geometry::table_requires_origin_preserving_stack(rows, indent) {
            for cell in rows.iter().flat_map(|row| &row.cells) {
                self.blocks(&cell.blocks, indent);
            }
            return;
        }
        let grid = mant_ir::TableGrid::new(rows);
        let rows = (0..grid.rows.len())
            .map(|row| {
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
                        if let Some(cell) = cell {
                            builder.blocks(&cell.blocks, 0);
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
            })
            .collect::<Vec<_>>();
        let mut table_layout = LogicalTableLayout::for_rows(&rows);
        table_layout.force_stack = grid.column_count > 256;
        let table_layout = Arc::new(table_layout);
        for cells in rows {
            self.push(LogicalLine::table(
                padding(indent),
                cells,
                Arc::clone(&table_layout),
            ));
        }
    }
}
