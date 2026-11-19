//! The device slots written before a tbl word and its field's fill target.

use super::{Column, Kind, Node, TableColumnMetrics, TableExecution, measure};
use crate::mandoc::blocks::InlineTablePrefix;
use libmandoc_rs::{TableAlignment, TableCellKind};

const EN: usize = 24;

pub(in crate::mandoc::blocks::tables) struct ColumnInput {
    pub(in crate::mandoc::blocks::tables) line_width: usize,
    pub(in crate::mandoc::blocks::tables) fill_target: Option<usize>,
    pub(in crate::mandoc::blocks::tables) leading_blanks: usize,
}

impl TableExecution<'_> {
    pub(in crate::mandoc::blocks::tables) fn column_inputs(
        &self,
        node: &Node,
        columns: &[Column],
        no_break_margin: Option<usize>,
        prefix: Option<InlineTablePrefix>,
    ) -> Result<Vec<ColumnInput>, String> {
        self.with_metrics(|metrics| {
            let first_data_offset = self.first_data_offset(node, metrics);
            let mut inputs = Vec::with_capacity(columns.len());
            visit_row_input(node, columns, metrics, |entry| {
                let offset = first_data_offset.saturating_add(entry.offset);
                let start = prefix.map_or(offset, |prefix| {
                    prefix.printed.saturating_add(
                        offset
                            .saturating_sub(prefix.printed)
                            .max(prefix.min_padding),
                    )
                });
                let margin = no_break_margin.unwrap_or_else(|| offset.saturating_add(entry.width));
                // `print_mdoc_node` / `print_man_node` set BRNEVER from
                // this span's NODE_NOFILL, not the caller's display mode.
                inputs.push(ColumnInput {
                    line_width: entry.width,
                    fill_target: (!node.flags.no_fill).then(|| margin.saturating_sub(start)),
                    leading_blanks: entry.leading_blanks,
                });
                false
            })?;
            Ok(inputs)
        })?
    }

    /// Native rules and numeric padding write even when the authored words
    /// supply no buffered input. Measure only if a column can write a pad.
    pub(in crate::mandoc::blocks::tables) fn buffered_padding(
        &self,
        node: &Node,
        columns: &[Column],
    ) -> Result<bool, String> {
        if !columns
            .iter()
            .any(|column| column.kind == Kind::Number || is_rule(node, column))
        {
            return Ok(false);
        }
        self.with_metrics(|metrics| {
            visit_row_input(node, columns, metrics, |entry| {
                entry.rule_written || entry.leading_blanks != 0
            })
        })?
    }

    fn first_data_offset(&self, node: &Node, metrics: &[TableColumnMetrics]) -> usize {
        let Some((offset, _)) = self.bounds.get() else {
            return 0;
        };
        let left_edge = self.left_edge;
        let boxed = node.table_frame.is_some();
        let offset = if node.table_alignment == Some(TableAlignment::Center) {
            self.centered_prefix(
                node,
                InlineTablePrefix {
                    offset,
                    printed: 0,
                    min_padding: 0,
                },
                metrics,
                boxed,
                left_edge,
            )
            .offset
        } else {
            offset
        };
        offset.saturating_add(usize::from(boxed || left_edge) * 2 * EN)
    }
}

struct RowEntry {
    offset: usize,
    width: usize,
    leading_blanks: usize,
    rule_written: bool,
}

fn visit_row_input(
    node: &Node,
    columns: &[Column],
    metrics: &[TableColumnMetrics],
    mut visit: impl FnMut(RowEntry) -> bool,
) -> Result<bool, String> {
    let mut offset = 0_usize;
    let mut cursor = 0_usize;
    for column in columns {
        let cell = column.cell.map(|index| &node.table_cells[index]);
        let span = if column.logical == 0 && column.kind == Kind::Span {
            1
        } else {
            cell.map_or(1, |cell| cell.column_span as usize).max(1)
        };
        let end = column.logical.saturating_add(span);
        let Some(slots) = metrics.get(column.logical..end) else {
            return Err("invalid table cell width".into());
        };
        let metric = &slots[0];
        let width = super::leading_gap::column_extent(slots, false);
        let mut entry = RowEntry {
            offset,
            width,
            leading_blanks: 0,
            rule_written: false,
        };
        if is_rule(node, column) {
            let count = fill_character_count(offset, metric.width_basic_units, cursor);
            cursor = cursor.saturating_add(count.saturating_mul(EN));
            entry.rule_written = count != 0;
        } else if let Some(cell) = cell.filter(|cell| cell.kind == TableCellKind::Text) {
            let extent = match column.kind {
                Kind::Number => {
                    entry.leading_blanks = number_padding(
                        node,
                        metric,
                        cell.decoder_text().unwrap_or_default(),
                        offset,
                        cursor,
                    )?;
                    metric
                        .width_basic_units
                        .saturating_add(entry.leading_blanks.saturating_mul(EN))
                }
                Kind::Left | Kind::Right | Kind::Center | Kind::Long => width,
                _ => 0,
            };
            // tbl_data updates the parser's width cursor, whereas each cell
            // word writes into a separate native buffer starting at col 0.
            cursor = cursor.saturating_add(extent);
        }
        if visit(entry) {
            return Ok(true);
        }
        cursor = cursor.saturating_add(metric.spacing_columns.saturating_mul(EN));
        offset = offset.saturating_add(super::leading_gap::column_extent(slots, true));
    }
    Ok(false)
}

fn is_rule(node: &Node, column: &Column) -> bool {
    matches!(
        column.kind,
        Kind::HorizontalRule | Kind::DoubleHorizontalRule
    ) || column.cell.is_some_and(|index| {
        !matches!(
            node.table_cells[index].kind,
            TableCellKind::Text | TableCellKind::Empty
        )
    })
}

pub(super) fn number_padding(
    node: &Node,
    metric: &TableColumnMetrics,
    text: &str,
    offset: usize,
    cursor: usize,
) -> Result<usize, String> {
    let decimal = node
        .table_layout
        .as_ref()
        .ok_or("missing numeric table layout")?
        .decimal_point;
    let pad = metric.number_padding(text, decimal, measure)?;
    Ok(fill_character_count(offset, pad, cursor))
}

fn fill_character_count(offset: usize, width: usize, cursor: usize) -> usize {
    // tbl_fill_string() starts only for a positive width, then writes one
    // complete character at a time until coloff reaches offset + width.
    if width == 0 {
        0
    } else {
        offset
            .saturating_add(width)
            .saturating_sub(cursor)
            .div_ceil(EN)
    }
}
