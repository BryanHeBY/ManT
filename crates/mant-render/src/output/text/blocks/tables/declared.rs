//! Declared fields prepare once, then choose placement and one table frame.
use super::{BlockRenderer, Flow, LayoutText, PreparedCell, padding};

fn align_declared_cell(
    text: LayoutText,
    area: Option<usize>,
    alignment: Option<mant_ir::TableAlignment>,
) -> LayoutText {
    if text.is_empty() {
        return text;
    }
    let spare = area
        .unwrap_or(0)
        .saturating_sub(mant_ir::geometry::text_width(&text.visible));
    let extra = match alignment {
        Some(mant_ir::TableAlignment::Center) => spare / 2,
        Some(mant_ir::TableAlignment::Right) => spare,
        _ => 0,
    };
    if extra > mant_ir::geometry::MAX_COLUMN_PADDING {
        return text;
    }
    text.indented(extra)
}

fn declared_field_ends(
    columns: &mant_ir::geometry::DeclaredColumns,
    count: usize,
    preferences: &mant_ir::ColumnPreferences,
) -> Vec<Option<usize>> {
    (0..count)
        .map(|column| {
            columns.field_end(column).map(|end| {
                if column < preferences.widths.len() {
                    end.saturating_sub(usize::from(preferences.gap_columns))
                } else {
                    end
                }
            })
        })
        .collect()
}

fn declared_rule(
    row: &mant_ir::TableRow,
    preferences: &mant_ir::ColumnPreferences,
    logical: usize,
) -> Option<(Vec<mant_ir::TableRuleCellKind>, Option<Vec<usize>>)> {
    match &row.kind {
        mant_ir::TableRowKind::HorizontalRule => {
            Some((vec![mant_ir::TableRuleCellKind::Horizontal], None))
        }
        mant_ir::TableRowKind::DoubleHorizontalRule => {
            Some((vec![mant_ir::TableRuleCellKind::DoubleHorizontal], None))
        }
        mant_ir::TableRowKind::LayoutRule { cells } => Some((
            cells.clone(),
            Some(
                (0..logical.max(preferences.widths.len()))
                    .map(|column| usize::from(preferences.widths.get(column).copied().unwrap_or(1)))
                    .collect(),
            ),
        )),
        mant_ir::TableRowKind::Data => None,
    }
}

fn supports_declared_layout(rows: &[mant_ir::TableRow], logical: usize) -> bool {
    logical <= mant_ir::MAX_DENSE_TABLE_COLUMNS
        && rows.iter().all(|row| {
            row.cells.len() <= mant_ir::geometry::MAX_DECLARED_COLUMNS
                && row.cells.iter().all(|cell| {
                    cell.column_span == 1
                        && cell.row_span == 1
                        && cell.kind == mant_ir::TableCellKind::Text
                })
        })
}

fn declared_columns(
    rows: &[mant_ir::TableRow],
    logical: usize,
    preferences: &mant_ir::ColumnPreferences,
) -> Option<mant_ir::geometry::DeclaredColumns> {
    supports_declared_layout(rows, logical)
        .then(|| mant_ir::geometry::DeclaredColumns::new(preferences))
        .flatten()
}

fn declared_minimum_width(logical: usize, preferences: &mant_ir::ColumnPreferences) -> usize {
    let count = logical.max(preferences.widths.len());
    (0..count)
        .map(|column| usize::from(preferences.widths.get(column).copied().unwrap_or(1)))
        .sum::<usize>()
        .saturating_add(
            usize::from(preferences.gap_columns).saturating_mul(count.saturating_sub(1)),
        )
}

struct PreparedDeclaredRow {
    cells: Vec<PreparedCell>,
    widths: Vec<Vec<mant_ir::geometry::ColumnFieldWidth>>,
    placements: Option<Vec<Vec<mant_ir::geometry::ColumnPiece>>>,
}

impl PreparedDeclaredRow {
    fn new(
        renderer: &BlockRenderer<'_>,
        row: &mant_ir::TableRow,
        columns: &mant_ir::geometry::DeclaredColumns,
        origin: i32,
    ) -> Self {
        let mut cells = Vec::with_capacity(row.cells.len());
        let mut widths = Vec::with_capacity(row.cells.len());
        for cell in &row.cells {
            let mut receipt =
                PreparedCell::new(renderer.cell_block_flow(&cell.blocks, 0), cell.break_after);
            receipt.alignment = cell.alignment;
            receipt.origins = mant_ir::geometry::table_cell_origin_bounds(cell);
            widths.push(receipt.widths());
            cells.push(receipt);
        }
        let bounds = cells.iter().map(|cell| cell.origins).collect::<Vec<_>>();
        let placements = columns.place_at(&widths, origin).filter(|placements| {
            mant_ir::geometry::table_column_origins_fit(&bounds, placements, origin)
        });
        Self {
            cells,
            widths,
            placements,
        }
    }

    fn alignment_fits(
        &self,
        columns: &mant_ir::geometry::DeclaredColumns,
        preferences: &mant_ir::ColumnPreferences,
        origin: i32,
        viewport: Option<usize>,
    ) -> bool {
        let ends = declared_field_ends(columns, self.cells.len(), preferences);
        self.placements.as_ref().is_none_or(|rows| {
            rows.iter().all(|pieces| {
                pieces.iter().enumerate().all(|(index, piece)| {
                    let cell = &self.cells[piece.cell];
                    let text = &cell.lines[piece.line];
                    let area = viewport.map(|width| {
                        let end = if piece.cell + 1 == self.cells.len() {
                            width
                        } else {
                            ends.get(piece.cell)
                                .copied()
                                .flatten()
                                .unwrap_or(width)
                                .min(width)
                        };
                        let end = pieces.get(index + 1).map_or(end, |next| {
                            end.min(
                                next.column
                                    .saturating_sub(usize::from(preferences.gap_columns)),
                            )
                        });
                        end.saturating_sub(piece.column)
                    });
                    let owner = text.alignment_padding(area);
                    let cell_area = ends.get(piece.cell).copied().flatten().map(|end| {
                        viewport
                            .map_or(end, |viewport| end.min(viewport))
                            .saturating_sub(piece.column)
                    });
                    let spare = cell_area.unwrap_or(0).saturating_sub(
                        mant_ir::geometry::text_width(&text.visible).saturating_add(owner),
                    );
                    let extra = owner
                        + match if text.has_row_alignment() {
                            None
                        } else {
                            cell.alignment
                        } {
                            Some(mant_ir::TableAlignment::Center) => spare / 2,
                            Some(mant_ir::TableAlignment::Right) => spare,
                            _ => 0,
                        };
                    cell.origins.fits_at(mant_ir::geometry::compose_origin(
                        origin,
                        mant_ir::geometry::coordinate(piece.column.saturating_add(extra)),
                    ))
                })
            })
        })
    }

    fn extent(&self) -> usize {
        self.placements
            .as_ref()
            .into_iter()
            .flatten()
            .flatten()
            .map(|piece| {
                piece
                    .column
                    .saturating_add(self.widths[piece.cell][piece.line].output)
            })
            .max()
            .unwrap_or(0)
    }
}

impl BlockRenderer<'_> {
    /// Prepare data cells once. Rule rows do not choose another topology,
    /// and a late placement failure reuses the same physical receipts.
    pub(super) fn declared_column_flow(
        &self,
        rows: &[mant_ir::TableRow],
        preferences: &mant_ir::ColumnPreferences,
        alignment: Option<mant_ir::TableAlignment>,
        column_count: Option<u32>,
        origin: i32,
    ) -> Flow {
        let logical = mant_ir::TableGrid::with_column_count(rows, column_count).column_count;
        let Some(columns) = declared_columns(rows, logical, preferences) else {
            return self
                .stacked_table_flow(rows, preferences.gap_columns, origin, false)
                .position_table(None, self.width, origin, None);
        };
        let cell_renderer = BlockRenderer {
            width: None,
            ..*self
        };
        let mut prepared = Vec::with_capacity(rows.len());
        let gap = usize::from(preferences.gap_columns);
        let mut frame = declared_minimum_width(logical, preferences);
        let mut stacked = false;
        for row in rows {
            if mant_ir::table_row_is_navigation_only(row) {
                prepared.push(None);
                continue;
            }
            if !matches!(row.kind, mant_ir::TableRowKind::Data) {
                prepared.push(None);
                continue;
            }
            let row = PreparedDeclaredRow::new(&cell_renderer, row, &columns, origin);
            frame = frame.max(row.extent());
            stacked |= row.placements.is_none();
            prepared.push(Some(row));
        }
        if alignment.is_some()
            && (frame > mant_ir::geometry::MAX_COLUMN_PADDING
                || self
                    .width
                    .is_some_and(|width| frame.saturating_add(padding(origin)) > width))
        {
            stacked = true;
        }
        let viewport = if alignment.is_some() && !stacked {
            Some(frame)
        } else {
            self.width
                .map(|width| width.saturating_sub(padding(origin)))
        };
        stacked |= prepared
            .iter()
            .flatten()
            .any(|row| !row.alignment_fits(&columns, preferences, origin, viewport));
        let viewport = if stacked {
            self.width.map(|width| {
                width
                    .saturating_sub(padding(origin))
                    .min(mant_ir::geometry::MAX_COLUMN_PADDING.saturating_sub(padding(origin)))
            })
        } else {
            viewport
        };
        let mut output = Flow::default();
        for (row, prepared) in rows.iter().zip(prepared) {
            if mant_ir::table_row_is_navigation_only(row) {
                continue;
            }
            if let Some(PreparedDeclaredRow {
                cells,
                widths,
                placements,
            }) = prepared
            {
                if row.cells.is_empty() {
                    output.extend(Flow::completed_text(LayoutText::default()));
                    continue;
                }
                let ends = declared_field_ends(&columns, cells.len(), preferences);
                output.extend(Self::placed_column_row(
                    if stacked { None } else { placements },
                    cells,
                    &widths,
                    origin,
                    &ends,
                    gap,
                    viewport,
                ));
            } else {
                let Some((rules, rule_widths)) = declared_rule(row, preferences, logical) else {
                    continue;
                };
                output.push_table_rule(
                    rules,
                    preferences.gap_columns,
                    if stacked { None } else { rule_widths },
                    stacked,
                );
            }
        }
        output.position_table(
            if stacked { None } else { alignment },
            self.width,
            origin,
            (!stacked && alignment.is_some()).then_some(frame),
        )
    }

    pub(in crate::output::text::blocks) fn placed_column_row(
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
                    let end = if piece.cell + 1 == cells.len() {
                        width
                    } else {
                        ends.get(piece.cell)
                            .copied()
                            .flatten()
                            .unwrap_or(width)
                            .min(width)
                    };
                    let end = pieces
                        .get(index + 1)
                        .map_or(end, |next| end.min(next.column.saturating_sub(gap)));
                    end.saturating_sub(piece.column)
                });
                let has_owner_alignment = cells[piece.cell].lines[piece.line].has_row_alignment();
                let content = cells[piece.cell].lines[piece.line]
                    .clone()
                    .position_alignments(area);
                let cell_area = ends.get(piece.cell).copied().flatten().map(|end| {
                    let end = viewport.map_or(end, |viewport| end.min(viewport));
                    end.saturating_sub(piece.column)
                });
                let content = align_declared_cell(
                    content,
                    cell_area,
                    if has_owner_alignment {
                        None
                    } else {
                        cells[piece.cell].alignment
                    },
                );
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
}
