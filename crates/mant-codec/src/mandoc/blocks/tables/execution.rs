//! `tbl_term.c`'s two phases: enter/fill every execution column, then flush.
//!
//! A completed `term_tbl` call drains every nonempty column (including
//! rejected and partial passes). At least one such buffer therefore reaches
//! `term_flushln`'s final reset; no fill occurs after that reset. A row-local
//! occupancy receipt suffices for BACK* without reproducing device output.
use crate::mandoc::formatter::FormatterState;
use libmandoc_rs::{Node, TableCellKind, TableColumnMetrics, TableLayoutKind as Kind};
use std::cell::{Cell, RefCell};

pub(super) struct TableExecution<'a> {
    rows: Vec<&'a Node>,
    bounds: Cell<Option<(usize, usize)>>,
    metrics: RefCell<Option<Result<Vec<TableColumnMetrics>, String>>>,
    failure_reported: Cell<bool>,
    column_work_left: Cell<usize>,
}

impl<'a> TableExecution<'a> {
    pub(super) fn new(rows: Vec<&'a Node>) -> Self {
        Self {
            rows,
            bounds: Cell::new(None),
            metrics: RefCell::new(None),
            failure_reported: Cell::new(false),
            column_work_left: Cell::new(4_000_000),
        }
    }

    pub(super) fn enter(&self, formatter: &FormatterState, source_offset: usize) {
        if self.bounds.get().is_none() {
            self.bounds
                .set(Some(formatter.execution.table_source_bounds(source_offset)));
        }
    }

    pub(super) fn columns(&self, node: &Node) -> Option<Vec<Column>> {
        let remaining = self
            .column_work_left
            .get()
            .checked_sub(column_count(node))?;
        self.column_work_left.set(remaining);
        Some(columns(node))
    }

    pub(super) fn report_failure(
        &self,
        node: &Node,
        context: &crate::mandoc::LoweringContext<'_>,
        message: &str,
    ) {
        if !self.failure_reported.replace(true) {
            context.diagnostics.borrow_mut().push(mant_ir::Diagnostic {
                impact: mant_ir::DiagnosticImpact::ContentCoverage,
                level: mant_ir::DiagnosticLevel::Warning,
                code: Some("manual.table-execution-incomplete".into()),
                message: format!("table input execution could not be proved ({message}); safe cell contents and following text were retained"),
                source: crate::mandoc::source_span(node),
            });
        }
    }

    fn with_metrics<T>(
        &self,
        use_metrics: impl FnOnce(&[TableColumnMetrics]) -> T,
    ) -> Result<T, String> {
        let mut metrics = self.metrics.borrow_mut();
        let (offset, margin) = self.bounds.get().ok_or("missing table source bounds")?;
        let result = metrics.get_or_insert_with(|| {
            libmandoc_rs::measure_table_columns(&self.rows, offset, margin, measure)
        });
        result
            .as_ref()
            .map(|metrics| use_metrics(metrics))
            .map_err(Clone::clone)
    }
}

fn measure(text: &str) -> usize {
    crate::mandoc::roff_escape::width_sample(text).saturating_mul(24)
}

#[derive(Clone, Copy)]
pub(super) struct Column {
    pub(super) logical: usize,
    pub(super) cell: Option<usize>,
    pub(super) kind: Kind,
}

fn column_count(node: &Node) -> usize {
    node.table_column_count.map_or_else(
        || {
            node.table_cells
                .iter()
                .map(|cell| cell.column.saturating_add(cell.column_span) as usize)
                .max()
                .unwrap_or_default()
        },
        |count| count as usize,
    )
}

pub(super) fn columns(node: &Node) -> Vec<Column> {
    let count = column_count(node);
    let mut columns = Vec::new();
    let mut next_cell = 0;
    let mut spans = 0;
    let layout = node.table_layout.as_ref();
    let first_is_span = layout
        .and_then(|layout| layout.cells.first())
        .is_some_and(|slot| slot.kind == Kind::Span);
    for logical in 0..count {
        let kind = layout
            .and_then(|layout| layout.cells.get(logical).or_else(|| layout.cells.last()))
            .map_or(Kind::Left, |slot| slot.kind);
        if spans > 0 {
            spans -= 1;
            continue;
        }
        let cell = (next_cell < node.table_cells.len()).then_some(next_cell);
        columns.push(Column {
            logical,
            cell,
            kind,
        });
        if let Some(index) = cell.filter(|_| logical != 0 || !first_is_span) {
            spans = node.table_cells[index].column_span.saturating_sub(1);
            next_cell += 1;
        }
    }
    columns
}

/// Unproved columns must not suppress safe owned text. A bounded fallback
/// visits each actual cell once, reports incomplete execution and retires no
/// successful native flush. Missing slots have no synthetic semantic owner.
pub(super) fn data_columns(node: &Node) -> Vec<Column> {
    node.table_cells
        .iter()
        .enumerate()
        .map(|(index, cell)| Column {
            logical: cell.column as usize,
            cell: Some(index),
            kind: node
                .table_layout
                .as_ref()
                .and_then(|layout| layout.cells.get(cell.column as usize))
                .map_or(Kind::Left, |slot| slot.kind),
        })
        .collect()
}

/// Native rules and numeric padding can write without semantic children.
/// Only these paths need widths. Ordinary table words supply their ordered
/// write receipt directly, so no output text is rescanned here.
pub(super) fn buffered_padding(
    node: &Node,
    columns: &[Column],
    width_dependent_lines: &[usize],
    execution: &TableExecution<'_>,
) -> Result<bool, String> {
    if width_dependent_lines.is_empty()
        && !columns.iter().any(|column| {
            matches!(
                column.kind,
                Kind::Number | Kind::HorizontalRule | Kind::DoubleHorizontalRule
            ) || column.cell.is_some_and(|index| {
                !matches!(
                    node.table_cells[index].kind,
                    TableCellKind::Text | TableCellKind::Empty
                )
            })
        })
    {
        return Ok(false);
    }
    execution
        .with_metrics(|metrics| {
            let offsets = metrics
                .iter()
                .scan(0_usize, |offset, column| {
                    let result = *offset;
                    *offset = offset
                        .saturating_add(column.width_basic_units)
                        .saturating_add(column.spacing_columns.saturating_mul(24));
                    Some(result)
                })
                .collect::<Vec<_>>();
            let mut coloff = 0;
            for column in columns {
                let metric = metrics.get(column.logical)?;
                let offset = *offsets.get(column.logical)?;
                let cell = column.cell.map(|index| &node.table_cells[index]);
                if width_dependent_lines.binary_search(&column.logical).is_ok() {
                    let end = column
                        .logical
                        .checked_add(cell.map_or(1, |cell| cell.column_span as usize))?;
                    if (column.logical..end).any(|logical| {
                        metrics.get(logical).is_some_and(|metric| {
                            metric.width_basic_units > 0
                                || logical + 1 < end && metric.spacing_columns > 0
                        })
                    }) {
                        return Some(true);
                    }
                }

                let rule = matches!(
                    column.kind,
                    Kind::HorizontalRule | Kind::DoubleHorizontalRule
                ) || cell.is_some_and(|cell| {
                    !matches!(cell.kind, TableCellKind::Text | TableCellKind::Empty)
                });
                if rule {
                    // tbl_fill_string(): even a positive effective width writes
                    // nothing if an earlier span advanced coloff past its target.
                    if metric.width_basic_units != 0
                        && coloff < offset.saturating_add(metric.width_basic_units)
                    {
                        return Some(true);
                    }
                } else if let Some(cell) = cell.filter(|cell| cell.kind == TableCellKind::Text) {
                    match column.kind {
                        Kind::Number => {
                            let decimal = node.table_layout.as_ref()?.decimal_point;
                            let pad = metric
                                .number_padding(
                                    cell.decoder_text().unwrap_or_default(),
                                    decimal,
                                    measure,
                                )
                                .ok()?;
                            if pad != 0 && coloff < offset.saturating_add(pad) {
                                return Some(true);
                            }
                            coloff = coloff.saturating_add(metric.width_basic_units);
                        }
                        Kind::Left | Kind::Right | Kind::Center | Kind::Long => {
                            let end = column.logical.checked_add(cell.column_span as usize)?;
                            for logical in column.logical..end {
                                let spanned = metrics.get(logical)?;
                                coloff = coloff.saturating_add(spanned.width_basic_units);
                                if logical + 1 < end {
                                    coloff = coloff
                                        .saturating_add(spanned.spacing_columns.saturating_mul(24));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                coloff = coloff.saturating_add(metric.spacing_columns.saturating_mul(24));
            }
            Some(false)
        })
        .and_then(|result| result.ok_or_else(|| "invalid table execution geometry".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn first(node: &Node) -> Option<&Node> {
        if node.kind == libmandoc_rs::NodeKind::Table {
            Some(node)
        } else {
            node.children.iter().find_map(first)
        }
    }
    #[test]
    fn exhausted_column_proof_preserves_all_actual_cells_and_following_text() {
        // Exact complete source previously ran the pristine five profiles.
        // Force only the private proof limit, never mutate native syntax.
        let source = include_str!("recovery/budget_tests/fixtures/man-style.1");
        let report = libmandoc_rs::Parser::default()
            .parse_bytes("budget.1", source.as_bytes())
            .unwrap();
        let row = first(&report.document.root).unwrap();
        let execution = std::rc::Rc::new(TableExecution::new(vec![row]));
        execution.column_work_left.set(0);
        let embedding = super::super::recovery::TableEmbedding {
            blocks: Vec::new(),
            execution,
        };
        let context = crate::mandoc::LoweringContext::new(None, Some(source));
        let mut formatter = FormatterState::default();
        let mut blocks = Vec::new();
        super::super::append_table_row(
            &mut blocks,
            row,
            &context,
            crate::mandoc::layout::SourceIndent::default(),
            Some(&embedding),
            &mut formatter,
        );
        let mant_ir::Block::Table { rows, .. } = &blocks[0] else {
            panic!("original table owner")
        };
        let mant_ir::Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
            panic!("original cell owner")
        };
        assert_eq!(mant_ir::inline_plain_text(children), "SAFE");
        assert_eq!(
            mant_ir::inline_plain_text(&context.lower_text("AFTER", &mut formatter)),
            "AFTER"
        );
        let diagnostics = context.take_diagnostics();
        assert_eq!(
            diagnostics
                .iter()
                .filter(
                    |d| d.code.as_deref() == Some("manual.table-execution-incomplete")
                        && d.impact == mant_ir::DiagnosticImpact::ContentCoverage
                )
                .count(),
            1
        );
    }
}
