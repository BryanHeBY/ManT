//! Read the first visible tbl cell's physical gap at an occupied HEAD row.

use super::{Kind, Node, TableColumnMetrics, TableExecution, measure};
use crate::mandoc::blocks::InlineTablePrefix;
use libmandoc_rs::TableAlignment;

const EN: usize = 24;

impl TableExecution<'_> {
    pub(in crate::mandoc::blocks::tables) fn separates_prefix(
        &self,
        node: &Node,
        cell: &libmandoc_rs::TableCell,
        prefix: InlineTablePrefix,
        execution: &crate::mandoc::inline::InlineExecutionState,
    ) -> bool {
        let column = cell.column as usize;
        let kind = node
            .table_layout
            .as_ref()
            .and_then(|layout| layout.cells.get(column))
            .map_or(Kind::Left, |slot| slot.kind);
        let centered = node.table_alignment == Some(TableAlignment::Center);
        let boxed = node.table_frame.is_some();
        // If skipvsp suppressed the opening vertical space, an occupied
        // prefix can still reach the left rule. tbl_term.c draws a left
        // rule from this row or its immediate successor, then advances an
        // en before the first graph. The shared width of a later rule
        // alone does not prove this separation.
        if node.flags.table_start && self.rows.iter().copied().take(2).any(row_has_left_border) {
            return true;
        }
        let left_edge = self.rows.iter().copied().any(row_has_left_border);
        let frame = usize::from(boxed || left_edge) * 2 * EN;
        // tbl_term.c::term_tbl sets offsets including left-edge allowance
        // and all earlier columns, including cells with no visible graph.
        // Its LONG branch adjusts tp->tcol *before* advancing to the
        // current column. This adds no padding to its first print pass.
        if !centered {
            if prefix.left_pad(frame) > EN / 2 {
                return true;
            }
            if column == 0 && !matches!(kind, Kind::Center | Kind::Right | Kind::Number) {
                return false;
            }
        }
        self.with_metrics(|metrics| {
            let prefix = if centered {
                self.centered_prefix(node, prefix, metrics, boxed, left_edge)
            } else {
                prefix
            };
            let end = column.saturating_add(cell.column_span as usize);
            let (Some(prior), Some(span)) = (metrics.get(..column), metrics.get(column..end))
            else {
                return false;
            };
            let Some(metric) = span.first() else {
                return false;
            };
            let before = frame.saturating_add(column_extent(prior, true));
            let padding = prefix.left_pad(before);
            if padding > EN / 2 {
                return true;
            }
            let width = column_extent(span, false);
            let text = cell.decoder_text().unwrap_or_default();
            let target = prefix
                .offset
                .saturating_add(before)
                .saturating_add(width)
                .saturating_sub(prefix.printed.saturating_add(padding));
            // term.c::term_flushln uses the available part of the cell
            // after the live prefix, not the entire declared cell width.
            let aligned = match kind {
                Kind::Center | Kind::Right => {
                    let printed = execution.table_first_pass_width(text, target);
                    let slack = target.saturating_sub(printed);
                    if kind == Kind::Center {
                        slack / 2
                    } else {
                        slack
                    }
                }
                Kind::Number => node
                    .table_layout
                    .as_ref()
                    .and_then(|layout| {
                        metric
                            .number_padding(text, layout.decimal_point, measure)
                            .ok()
                    })
                    .unwrap_or_default(),
                _ => 0,
            };
            // term_ascii.c::ascii_advance rounds positive space to the
            // nearest en; a half en itself emits no space.
            padding.saturating_add(aligned) > EN / 2
        })
        .unwrap_or(prefix.min_padding > EN / 2)
    }

    fn centered_prefix(
        &self,
        node: &Node,
        mut prefix: InlineTablePrefix,
        metrics: &[TableColumnMetrics],
        boxed: bool,
        left_edge: bool,
    ) -> InlineTablePrefix {
        let Some((offset, margin)) = self.bounds.get() else {
            return prefix;
        };
        // tbl_term.c::term_tbl's center calculation reserves three en for
        // the left edge and two for the right, whereas the columns start
        // two en past the left edge. Use the final native shared edge count.
        let edges = node
            .table_layout
            .as_ref()
            .map_or(0, |layout| layout.width_edge_columns);
        let border = if boxed {
            5
        } else {
            2 * usize::from(edges) + usize::from(left_edge)
        };
        let mut width = column_extent(metrics, false).saturating_add(border * EN);
        if offset.saturating_add(width) > margin {
            width = width.saturating_sub(EN);
        }
        prefix.offset = offset.saturating_add(margin).saturating_sub(width) / EN / 2 * EN;
        prefix
    }
}

fn row_has_left_border(row: &Node) -> bool {
    row.table_layout
        .as_ref()
        .is_some_and(|layout| layout.left_border.is_some())
}

fn column_extent(metrics: &[TableColumnMetrics], include_final_spacing: bool) -> usize {
    metrics
        .iter()
        .enumerate()
        .fold(0, |width, (index, column)| {
            let spacing = if include_final_spacing || index + 1 < metrics.len() {
                column.spacing_columns.saturating_mul(EN)
            } else {
                0
            };
            width
                .saturating_add(column.width_basic_units)
                .saturating_add(spacing)
        })
}
