//! Cell-local columns and origin-preserving table fallback.
use super::super::model::TableCellOccupancy;
use super::super::{Arc, LogicalLine, LogicalTableCell, LogicalTableLayout};
use super::DocumentBuilder;
use mant_ir::geometry::padding;
use mant_ir::{TableRow, TableRowKind};

struct PreparedCell {
    cell: LogicalTableCell,
    gap_only: bool,
}

impl PreparedCell {
    fn for_measurement(&self, declared: bool) -> LogicalTableCell {
        let mut cell = self.cell.clone();
        if declared {
            cell.preferred_width = None;
        }
        cell
    }
}

fn fill_horizontal_owners(cells: &mut [PreparedCell]) {
    for column in 0..cells.len() {
        if cells[column].cell.occupancy != TableCellOccupancy::Content {
            continue;
        }
        let end = column
            .saturating_add(cells[column].cell.column_span)
            .min(cells.len());
        for cell in cells.iter_mut().take(end).skip(column + 1) {
            if cell.cell.occupancy == TableCellOccupancy::Vacant {
                cell.cell.occupancy = TableCellOccupancy::Covered {
                    owner_column: column,
                };
            }
        }
    }
}

fn table_rails(
    rows: &[TableRow],
    borders: &mant_ir::TableBorders,
    count: usize,
    declared: bool,
) -> super::super::model::TableRails {
    let mut rails = super::super::model::TableRails {
        columns: borders.inner_columns.is_some() && count > 1 && !declared,
        outer: borders.outer.is_some(),
    };
    for edge in rows.iter().flat_map(|row| &row.column_borders) {
        let column = usize::try_from(edge.column).unwrap_or(usize::MAX);
        rails.outer |= column == 0 || column == count;
        rails.columns |= column > 0 && column < count && !declared;
    }
    rails
}

fn needs_cell_layout(row: &TableRow) -> bool {
    matches!(row.kind, TableRowKind::Data)
        && !row.cells.is_empty()
        && !mant_ir::table_row_is_navigation_only(row)
}

fn has_only_single_text_cells(rows: &[TableRow]) -> bool {
    rows.iter()
        .filter(|row| matches!(row.kind, TableRowKind::Data))
        .all(|row| {
            row.cells.iter().all(|cell| {
                cell.kind == mant_ir::TableCellKind::Text
                    && cell.column_span == 1
                    && cell.row_span == 1
            })
        })
}

impl DocumentBuilder<'_> {
    pub(super) fn table(
        &mut self,
        rows: &[TableRow],
        borders: &mant_ir::TableBorders,
        preferences: &mant_ir::ColumnPreferences,
        column_count: Option<u32>,
        alignment: Option<mant_ir::TableAlignment>,
        indent: i32,
    ) {
        if rows.is_empty() {
            return;
        }
        let origin_stack = mant_ir::geometry::table_requires_origin_preserving_stack(rows, indent);
        let grid = mant_ir::TableGrid::with_column_count(rows, column_count);
        let simple = has_only_single_text_cells(rows);
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
            .map(|cells| {
                cells
                    .iter()
                    .map(|cell| cell.for_measurement(!preferences.widths.is_empty()))
                    .collect()
            })
            .collect::<Vec<_>>();
        let rails = table_rails(
            rows,
            borders,
            grid.column_count,
            !preferences.widths.is_empty(),
        );
        let mut table_layout = LogicalTableLayout::for_rows_with_gap(
            &layout_rows,
            usize::from(preferences.gap_columns) + usize::from(rails.columns),
        );
        table_layout.borders = borders.clone();
        table_layout.rails = rails;
        table_layout.logical_columns = grid.column_count;
        table_layout.force_stack = force_stack;
        table_layout.alignment = alignment;
        if !force_stack {
            table_layout.preferred_widths.resize(grid.column_count, 1);
        }
        table_layout.retain_open_tail = origin_stack || !preferences.widths.is_empty();
        table_layout.column_preferences = preferences.clone();
        let table_layout = Arc::new(table_layout);
        for (row, rendered) in rows.iter().zip(rendered_rows) {
            if self.defer_navigation_row(row, indent) {
                continue;
            }
            match &row.kind {
                TableRowKind::Data if row.cells.is_empty() => self.push_table_line(
                    LogicalLine::table(
                        padding(outer_origin),
                        Vec::new(),
                        Arc::clone(&table_layout),
                    ),
                    row,
                ),
                TableRowKind::Data => {
                    let cells = rendered.expect("data row was rendered");
                    if origin_stack {
                        self.origin_preserving_row(cells, &table_layout, &row.column_borders);
                    } else {
                        self.push_table_line(
                            LogicalLine::table(
                                padding(outer_origin),
                                cells.into_iter().map(|cell| cell.cell).collect(),
                                Arc::clone(&table_layout),
                            ),
                            row,
                        );
                    }
                }
                TableRowKind::HorizontalRule => {
                    self.push_table_line(
                        Self::whole_table_rule(padding(outer_origin), false, &table_layout),
                        row,
                    );
                }
                TableRowKind::DoubleHorizontalRule => {
                    self.push_table_line(
                        Self::whole_table_rule(padding(outer_origin), true, &table_layout),
                        row,
                    );
                }
                TableRowKind::LayoutRule { cells } => self.push_table_line(
                    LogicalLine::table_rule(
                        padding(outer_origin),
                        cells.clone(),
                        Arc::clone(&table_layout),
                    ),
                    row,
                ),
            }
        }
    }

    fn push_table_line(&mut self, mut line: LogicalLine, row: &TableRow) {
        line.table_row
            .as_mut()
            .expect("table row")
            .column_borders
            .clone_from(&row.column_borders);
        self.push(line);
    }

    fn whole_table_rule(
        indent: usize,
        double: bool,
        layout: &Arc<LogicalTableLayout>,
    ) -> LogicalLine {
        let mut line = LogicalLine::table_rule(indent, Vec::new(), Arc::clone(layout));
        line.table_row.as_mut().expect("table row").whole_rule = Some(double);
        line
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
        let mut active_spans =
            vec![(0_u32, 0_usize); if force_stack { 0 } else { grid.column_count }];
        (0..grid.rows.len())
            .map(|row| {
                if !needs_cell_layout(&rows[row]) {
                    if matches!(rows[row].kind, TableRowKind::Data)
                        && !mant_ir::table_row_is_navigation_only(&rows[row])
                    {
                        for (remaining, _) in &mut active_spans {
                            *remaining = remaining.saturating_sub(1);
                        }
                    }
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
                let covered = active_spans
                    .iter_mut()
                    .map(|(remaining, owner)| {
                        let covered = (*remaining > 0).then_some(*owner);
                        *remaining = remaining.saturating_sub(1);
                        covered
                    })
                    .collect::<Vec<_>>();
                let mut cells: Vec<PreparedCell> = positions
                    .into_iter()
                    .enumerate()
                    .map(|(column, cell)| {
                        let mut prepared = self.prepare_table_cell(
                            cell,
                            origin,
                            inherit_parent_boundary,
                            &mut shared_gap,
                        );
                        if prepared.cell.lines.is_empty()
                            && let Some(owner_column) = covered.get(column).copied().flatten()
                        {
                            prepared.cell.occupancy =
                                TableCellOccupancy::Continuation { owner_column };
                        }
                        if !prepared.cell.occupancy.covered() && prepared.cell.row_span > 1 {
                            for (remaining, owner) in active_spans
                                .iter_mut()
                                .skip(column)
                                .take(prepared.cell.column_span)
                            {
                                *remaining = (*remaining).max(prepared.cell.row_span - 1);
                                *owner = column;
                            }
                        }
                        prepared
                    })
                    .collect();
                if !force_stack {
                    fill_horizontal_owners(&mut cells);
                }
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
        rendered.occupancy = if cell.is_some() {
            TableCellOccupancy::Content
        } else {
            TableCellOccupancy::Vacant
        };
        rendered.column_span = cell.map_or(1, |cell| {
            usize::try_from(cell.column_span).unwrap_or(usize::MAX)
        });
        rendered.row_span = cell.map_or(1, |cell| cell.row_span);
        rendered.preferred_width = cell.and_then(|cell| {
            cell.preferred_width_columns.map(|width| {
                let width = usize::from(width);
                if cell.blocks.iter().all(|block| {
                    matches!(
                        block,
                        mant_ir::Block::Paragraph { .. } | mant_ir::Block::VerticalSpace { .. }
                    )
                }) {
                    width
                } else {
                    width.max(rendered.preferred_width())
                }
            })
        });
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
        column_borders: &[mant_ir::TableColumnBorder],
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
        let mut line = LogicalLine::table(0, accepted, Arc::clone(layout));
        line.table_row.as_mut().expect("table row").column_borders = column_borders.to_vec();
        self.push(line);
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
