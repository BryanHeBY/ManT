//! Read the first visible tbl cell's physical gap at an occupied HEAD row.

use super::{Kind, Node, TableColumnMetrics, TableExecution, measure};
use crate::mandoc::blocks::InlineTablePrefix;
use libmandoc_rs::{TableAlignment, TableCell, TableCellKind};

const EN: usize = 24;

impl TableExecution<'_> {
    pub(in crate::mandoc::blocks::tables) fn separates_prefix(
        &self,
        node: &Node,
        cell: &TableCell,
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
            let Some(fill_cursor) = self.prior_columns(node, prior) else {
                return true;
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
                Kind::Number => {
                    number_padding_writes(node, metric, text, before - frame, fill_cursor)
                }
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

impl TableExecution<'_> {
    /// The prior fill cursor, or `None` if an emitted rule already separates
    /// the two reading words.
    fn prior_columns(&self, node: &Node, prior: &[TableColumnMetrics]) -> Option<usize> {
        let Some(layout) = node.table_layout.as_ref() else {
            return Some(0);
        };
        let next = self.rows.get(1).and_then(|row| row.table_layout.as_ref());
        let mut offset = 0_usize;
        let mut cursor = 0_usize;
        let mut column = 0;
        let mut actual = node.table_cells.iter().peekable();
        while let Some(metric) = prior.get(column) {
            let Some(slot) = layout.cells.get(column).or_else(|| layout.cells.last()) else {
                return Some(cursor);
            };
            let cell = actual.next_if(|cell| cell.column as usize == column);
            let span = cell.map_or(1, |cell| cell.column_span as usize).max(1);
            let end = column.saturating_add(span).min(prior.len());
            let slots = &prior[column..end];
            let trailing = layout.cells.get(end - 1).unwrap_or(slot);
            let below = next.and_then(|layout| layout.cells.get(column));
            // term_tbl uses the last slot of an hspan for the upper arm
            // and the immediate successor span for the lower arm.
            if metric.spacing_columns > 0
                && (trailing.right_border.is_some()
                    || below.is_some_and(|slot| slot.right_border.is_some()))
            {
                return None;
            }
            // tbl_data can buffer a stroke inside a prior cell, independent
            // of the spacing needed to draw an inter-column junction.
            let horizontal = matches!(slot.kind, Kind::HorizontalRule | Kind::DoubleHorizontalRule)
                || cell.is_some_and(|cell| {
                    !matches!(cell.kind, TableCellKind::Text | TableCellKind::Empty)
                });
            if horizontal
                && metric.width_basic_units > 0
                && cursor < offset.saturating_add(metric.width_basic_units)
            {
                return None;
            }
            if !horizontal && let Some(cell) = cell.filter(|cell| cell.kind == TableCellKind::Text)
            {
                cursor = match slot.kind {
                    Kind::Number => cursor
                        .saturating_add(number_padding_writes(
                            node,
                            metric,
                            cell.decoder_text().unwrap_or_default(),
                            offset,
                            cursor,
                        ))
                        .saturating_add(metric.width_basic_units),
                    Kind::Left | Kind::Right | Kind::Center | Kind::Long => {
                        cursor.saturating_add(column_extent(slots, false))
                    }
                    _ => cursor,
                };
            }
            offset = offset.saturating_add(column_extent(slots, true));
            cursor = cursor.saturating_add(prior[end - 1].spacing_columns.saturating_mul(EN));
            column = end;
        }
        Some(cursor)
    }
}

fn number_padding_writes(
    node: &Node,
    metric: &TableColumnMetrics,
    text: &str,
    offset: usize,
    cursor: usize,
) -> usize {
    // tbl_fill_string writes one complete ASCII_NBRSP per iteration, even
    // for half an en, until its existing cursor reaches the target.
    node.table_layout
        .as_ref()
        .and_then(|layout| {
            metric
                .number_padding(text, layout.decimal_point, measure)
                .ok()
        })
        .filter(|pad| *pad != 0)
        .map_or(0, |pad| offset.saturating_add(pad).saturating_sub(cursor))
        .div_ceil(EN)
        .saturating_mul(EN)
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
