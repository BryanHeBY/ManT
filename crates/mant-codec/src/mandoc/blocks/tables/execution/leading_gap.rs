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
    ) -> bool {
        let column = cell.column as usize;
        let kind = node
            .table_layout
            .as_ref()
            .and_then(|layout| layout.cells.get(column))
            .map_or(Kind::Left, |slot| slot.kind);
        let centered = node.table_alignment == Some(TableAlignment::Center);
        let boxed = node.table_frame.is_some();
        let left_edge = self.rows.iter().any(|row| {
            row.table_layout
                .as_ref()
                .is_some_and(|layout| layout.left_border.is_some())
        });
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
            let slack = target.saturating_sub(measure(text));
            let aligned = match kind {
                Kind::Center => slack / 2,
                Kind::Right => slack,
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
