//! Cell-local columns and origin-preserving table fallback.
use super::super::{Arc, LogicalLine, LogicalTableCell, LogicalTableLayout};
use super::DocumentBuilder;
use mant_ir::geometry::padding;
use mant_ir::{TableRow, TableRowKind};

struct PreparedCell {
    cell: LogicalTableCell,
    gap_only: bool,
}

fn needs_cell_layout(row: &TableRow) -> bool {
    matches!(row.kind, TableRowKind::Data)
        && !row.cells.is_empty()
        && !mant_ir::table_row_is_navigation_only(row)
}

impl DocumentBuilder<'_> {
    pub(super) fn table(
        &mut self,
        rows: &[TableRow],
        preferences: &mant_ir::ColumnPreferences,
        indent: i32,
    ) {
        if rows.is_empty() {
            return;
        }
        let origin_stack = mant_ir::geometry::table_requires_origin_preserving_stack(rows, indent);
        let grid = mant_ir::TableGrid::new(rows);
        let simple = rows.iter().all(|row| {
            matches!(row.kind, TableRowKind::Data)
                && row.cells.iter().all(|cell| {
                    cell.kind == mant_ir::TableCellKind::Text
                        && cell.column_span == 1
                        && cell.row_span == 1
                })
        });
        let force_stack = origin_stack
            || grid.column_count > 256
            || (!preferences.widths.is_empty()
                && (!simple || mant_ir::geometry::DeclaredColumns::new(preferences).is_none()));
        let cell_origin = if origin_stack { indent } else { 0 };
        let outer_origin = if origin_stack { 0 } else { indent };
        let rendered_rows =
            self.prepare_table_rows(rows, &grid, cell_origin, origin_stack, force_stack);
        let layout_rows = rendered_rows
            .iter()
            .filter_map(Option::as_ref)
            .map(|cells: &Vec<PreparedCell>| cells.iter().map(|cell| cell.cell.clone()).collect())
            .collect::<Vec<_>>();
        let mut table_layout = LogicalTableLayout::for_rows(&layout_rows);
        table_layout.force_stack = force_stack;
        table_layout.retain_open_tail = origin_stack || !preferences.widths.is_empty();
        table_layout.column_preferences = preferences.clone();
        let table_layout = Arc::new(table_layout);
        for (row, rendered) in rows.iter().zip(rendered_rows) {
            if self.defer_navigation_row(row, indent) {
                continue;
            }
            match &row.kind {
                TableRowKind::Data if row.cells.is_empty() => self.push(LogicalLine::table(
                    padding(outer_origin),
                    Vec::new(),
                    Arc::clone(&table_layout),
                )),
                TableRowKind::Data => {
                    let cells = rendered.expect("data row was rendered");
                    if origin_stack {
                        self.origin_preserving_row(cells, &table_layout);
                    } else {
                        self.push(LogicalLine::table(
                            padding(outer_origin),
                            cells.into_iter().map(|cell| cell.cell).collect(),
                            Arc::clone(&table_layout),
                        ));
                    }
                }
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

    fn prepare_table_rows(
        &self,
        rows: &[TableRow],
        grid: &mant_ir::TableGrid<'_>,
        origin: i32,
        inherit_parent_boundary: bool,
        force_stack: bool,
    ) -> Vec<Option<Vec<PreparedCell>>> {
        // The parent has already emitted these pending rows. Cells append
        // only each request's newly accepted rows, so the active boundary
        // stays bounded across leading and trailing gaps and nested tables.
        let mut shared_gap = mant_ir::geometry::GapPlan::default();
        if inherit_parent_boundary {
            shared_gap.append_resolved(self.pending_gap.rows(0));
        }
        (0..grid.rows.len())
            .map(|row| {
                if !needs_cell_layout(&rows[row]) {
                    if !mant_ir::table_row_is_navigation_only(&rows[row]) {
                        shared_gap = mant_ir::geometry::GapPlan::default();
                    }
                    return None;
                }
                let positions = (if force_stack {
                    None
                } else {
                    grid.slots(row, 256)
                })
                .unwrap_or_else(|| {
                    grid.rows[row]
                        .iter()
                        .map(|positioned| Some(positioned.cell))
                        .collect()
                });
                let cells = positions
                    .into_iter()
                    .map(|cell| {
                        self.prepare_table_cell(
                            cell,
                            origin,
                            inherit_parent_boundary,
                            &mut shared_gap,
                        )
                    })
                    .collect();
                // A whole data-row boundary consumes any completed gap rows
                // once. It does not append a second empty physical row.
                shared_gap = mant_ir::geometry::GapPlan::default();
                Some(cells)
            })
            .collect()
    }

    fn prepare_table_cell(
        &self,
        cell: Option<&mant_ir::TableCell>,
        origin: i32,
        inherit_parent_boundary: bool,
        shared_gap: &mut mant_ir::geometry::GapPlan,
    ) -> PreparedCell {
        let mut builder = Self::new(String::new(), self.address.clone());
        builder.entry_styles = Arc::clone(&self.entry_styles);
        builder.reference_origins = Arc::clone(&self.reference_origins);
        if inherit_parent_boundary {
            builder.pending_gap = std::mem::take(shared_gap);
            builder.pending_gap_inherited = builder.pending_gap.rows(0) > 0;
        }
        if let Some(cell) = cell {
            match cell.kind {
                mant_ir::TableCellKind::Text => {
                    builder.table_cell_blocks(&cell.blocks, origin);
                }
                mant_ir::TableCellKind::HorizontalRule
                | mant_ir::TableCellKind::IsolatedHorizontalRule => {
                    builder.push(LogicalLine::rule(padding(origin)));
                }
                mant_ir::TableCellKind::DoubleHorizontalRule
                | mant_ir::TableCellKind::IsolatedDoubleHorizontalRule => {
                    builder.push(LogicalLine::double_rule(padding(origin)));
                }
            }
        }
        let break_after = cell.is_some_and(|cell| cell.break_after);
        let owned_gap = builder.pending_gap.rows(0) > 0 && !builder.pending_gap_inherited;
        let gap_only = (owned_gap || builder.completed_external_gap) && !builder.has_content_row;
        let completed_tail = owned_gap
            || builder.completed_external_gap
            || builder
                .lines
                .last()
                .and_then(|line| line.table_row.as_ref())
                .is_some_and(|row| row.completed_tail);
        let empty_boundary = break_after
            && !completed_tail
            && builder.lines.len() <= 1
            && builder.lines.iter().all(|line| {
                line.table_row.is_none() && line.spans.iter().all(|span| span.content.is_empty())
            });
        if inherit_parent_boundary && !break_after {
            *shared_gap = std::mem::take(&mut builder.pending_gap);
        }
        let content = builder.finish().content;
        let mut rendered =
            LogicalTableCell::new(content.lines, cell.and_then(|cell| cell.alignment));
        rendered.anchors = content.anchors;
        rendered.completed_tail = completed_tail || break_after;
        rendered.break_after = break_after;
        rendered.empty_boundary = empty_boundary;
        rendered.origin_bounds = cell.map_or_else(
            Default::default,
            mant_ir::geometry::table_cell_origin_bounds,
        );
        PreparedCell {
            cell: rendered,
            gap_only,
        }
    }

    /// Each fragment emitted only the new rows of the same pending cursor.
    /// Gap-only content may consume that cursor without adding a data-row
    /// placeholder when its complete budget was already printed by a parent.
    fn origin_preserving_row(
        &mut self,
        cells: Vec<PreparedCell>,
        layout: &Arc<LogicalTableLayout>,
    ) {
        if cells.iter().any(|cell| cell.gap_only)
            && cells.iter().all(|cell| cell.cell.lines.is_empty())
        {
            self.defer_anchors(
                cells
                    .into_iter()
                    .flat_map(|cell| cell.cell.anchors.into_keys()),
            );
            self.pending_gap = mant_ir::geometry::GapPlan::default();
            self.pending_gap_inherited = false;
            self.completed_external_gap = true;
            return;
        }
        let accepted = cells
            .into_iter()
            .filter_map(|prepared| {
                if prepared.gap_only && prepared.cell.lines.is_empty() {
                    self.defer_anchors(prepared.cell.anchors.into_keys());
                    None
                } else {
                    Some(prepared.cell)
                }
            })
            .collect();
        self.push(LogicalLine::table(0, accepted, Arc::clone(layout)));
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
