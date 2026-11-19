//! Decode real cell owners while recording the row's native input effects.
use super::recovery::lower_table_cell;
use super::{
    InlineTablePrefix, TableEmbedding, execution, recovery, table_alignment, table_cell_font,
    table_cell_kind, table_cell_width,
};
use crate::mandoc::{LoweringContext, inline::TableCellExecution, source_span};
use libmandoc_rs::{Node, TableCellKind, TableFont};
use mant_ir::{Block, LayoutHint, TableCell as AstTableCell};

pub(super) struct ExecutedCell {
    pub(super) cell: AstTableCell,
    pub(super) empty_tail: usize,
    pub(super) entry_font: Option<crate::mandoc::inline::FontState>,
}

impl ExecutedCell {
    pub(super) fn into_block_cell(mut self) -> AstTableCell {
        if self.empty_tail != 0
            && let [
                Block::Paragraph {
                    children, source, ..
                },
            ] = self.cell.blocks.as_mut_slice()
        {
            // The raw scan proved completed empty passes, distinct from
            // an open inline tail that the enclosing row may reuse.
            let source = *source;
            for _ in 0..self.empty_tail {
                crate::mandoc::inline::consume_one_row_ending(children);
            }
            // A device-only accepted pass has no reading glyphs, but it
            // owns the original data row in addition to any following
            // completed empty passes. An empty paragraph supplies no row.
            let mut rows = self.empty_tail + usize::from(children.is_empty());
            while rows != 0 {
                let lines = u16::try_from(rows).unwrap_or(u16::MAX);
                self.cell
                    .blocks
                    .push(Block::VerticalSpace { lines, source });
                rows -= usize::from(lines);
            }
        }
        self.cell
    }
}

pub(super) fn lower_cells(
    node: &Node,
    kind: &mant_ir::TableRowKind,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    embedding: Option<&TableEmbedding<'_>>,
    prefix: Option<InlineTablePrefix>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<ExecutedCell> {
    let execution = embedding.map(|embedding| embedding.execution.as_ref());
    if let Some(execution) = execution {
        execution.enter(formatter, indent_columns.physical_basic_units());
    }
    let active = matches!(
        kind,
        mant_ir::TableRowKind::Data | mant_ir::TableRowKind::LayoutRule { .. }
    );
    let planned = if active {
        execution.map_or_else(
            || Some(execution::columns(node)),
            |execution| execution.columns(node),
        )
    } else {
        Some(Vec::new())
    };
    let proved_columns = planned.is_some();
    let columns = planned.unwrap_or_else(|| execution::data_columns(node));
    if !proved_columns && let Some(execution) = execution {
        execution.report_failure(
            node,
            context,
            "declared-column execution work budget exceeded",
        );
    }
    let source_blocks =
        embedding.map_or_else(Vec::new, |_| recovery::recover_text_blocks(node, context));
    let text_blocks = text_blocks(node, &source_blocks);
    let mut cells = cell_owners(node);
    let slot_inputs = slot_input_plan(
        node,
        &columns,
        execution.filter(|_| proved_columns),
        context,
        prefix,
        formatter,
    );
    let mut buffered = false;
    if active {
        for (column_index, column) in columns.iter().enumerate() {
            // tbl_term.c::term_tbl: every non-hspan EXECUTION column enters,
            // including missing trailing data. All fills precede all flushes.
            formatter.clear_zero_advance();
            formatter.execution.table_cell = Some(TableCellExecution::default());
            if let Some((index, cell)) = raw_word_for_column(node, column) {
                let text = cell.decoder_text();
                let needs_slots = text.is_some_and(|text| needs_slot_input(column, text));
                if prefix.is_some() && needs_slots {
                    // term_tbl feeds each word before any tcol prints.
                    // Later column font changes cannot remeasure this word.
                    cells[index].entry_font = Some(formatter.font.clone());
                }
                if needs_slots && let Some(input) = slot_inputs.get(column_index) {
                    formatter.execution.table_cell = Some(TableCellExecution {
                        line_width_units: Some(input.line_width),
                        field_width_units: input.fill_target,
                        leading_blanks: input.leading_blanks,
                        ..TableCellExecution::default()
                    });
                }
                let blocks =
                    project_text_cell(cell, index, node, context, text_blocks[index], formatter);
                if !cell.vertical_continuation {
                    cells[index].cell.blocks = blocks;
                    cells[index].empty_tail = formatter
                        .execution
                        .table_cell
                        .map_or(0, |cell| cell.trailing_rows);
                }
            }
            let executed = formatter.execution.table_cell.take().unwrap_or_default();
            buffered |= executed.buffered;
        }
        if !buffered
            && proved_columns
            && let Some(execution) = execution
        {
            match execution.buffered_padding(node, &columns) {
                Ok(written) => buffered = written,
                Err(message) => {
                    execution.report_failure(node, context, &message);
                    // An unproved device effect cannot silently consume safe
                    // following text; no successful flush is claimed here.
                    formatter.clear_zero_advance();
                }
            }
        }
        // Every nonempty tcol eventually completes or rejects at
        // term.c::term_flushln's reset (233-237), even after partial passes.
        // Whole-row/outer borders bypass it; a truly empty row preserves z.
        if buffered {
            formatter.execute_word();
        }
        if !proved_columns {
            formatter.clear_zero_advance();
        }
    }
    cells
}

fn raw_word_for_column<'a>(
    node: &'a Node,
    column: &execution::Column,
) -> Option<(usize, &'a libmandoc_rs::TableCell)> {
    use libmandoc_rs::TableLayoutKind;
    if matches!(
        column.kind,
        TableLayoutKind::Down
            | TableLayoutKind::Span
            | TableLayoutKind::HorizontalRule
            | TableLayoutKind::DoubleHorizontalRule
    ) {
        return None;
    }
    let index = column.cell?;
    let cell = node.table_cells.get(index)?;
    (cell.kind == TableCellKind::Text).then_some((index, cell))
}

fn slot_input_plan(
    node: &Node,
    columns: &[execution::Column],
    execution: Option<&execution::TableExecution<'_>>,
    context: &LoweringContext<'_>,
    prefix: Option<InlineTablePrefix>,
    formatter: &crate::mandoc::formatter::FormatterState,
) -> Vec<execution::ColumnInput> {
    let Some(execution) = execution.filter(|_| {
        columns.iter().any(|column| {
            raw_word_for_column(node, column)
                .and_then(|(_, cell)| cell.decoder_text())
                .is_some_and(|text| needs_slot_input(column, text))
        })
    }) else {
        return Vec::new();
    };
    match execution.column_inputs(
        node,
        columns,
        formatter.execution.table_no_break_margin(),
        prefix,
    ) {
        Ok(inputs) => inputs,
        Err(message) => {
            execution.report_failure(node, context, &message);
            Vec::new()
        }
    }
}

fn needs_slot_input(column: &execution::Column, text: &str) -> bool {
    crate::mandoc::inline::table_word_needs_native_slots(text)
        || column.kind == libmandoc_rs::TableLayoutKind::Number && text.contains("\\p")
}

fn text_blocks<'a>(
    node: &Node,
    source_blocks: &'a [Option<crate::mandoc::TableTextBlock>],
) -> Vec<Option<&'a crate::mandoc::TableTextBlock>> {
    let mut text_block_index = 0;
    node.table_cells
        .iter()
        .map(|cell| {
            if !cell.text_block {
                return None;
            }
            let block = source_blocks.get(text_block_index).and_then(Option::as_ref);
            text_block_index += 1;
            block
        })
        .collect::<Vec<_>>()
}
fn cell_owners(node: &Node) -> Vec<ExecutedCell> {
    node.table_cells
        .iter()
        .map(|cell| ExecutedCell {
            cell: AstTableCell {
                preferred_width_columns: table_cell_width(node, cell),
                break_after: false,
                kind: table_cell_kind(cell.kind),
                blocks: Vec::new(),
                column_span: cell.column_span,
                row_span: cell.row_span,
                alignment: Some(table_alignment(cell.alignment)),
            },
            empty_tail: 0,
            entry_font: None,
        })
        .collect::<Vec<_>>()
}

fn project_text_cell(
    cell: &libmandoc_rs::TableCell,
    index: usize,
    node: &Node,
    context: &LoweringContext<'_>,
    text_block: Option<&crate::mandoc::TableTextBlock>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<Block> {
    // tbl_word() scopes the layout font, including a decoded control-only
    // word. A literal \^ data word still executes; only a DOWN layout skips
    // it. Its semantic owner is suppressed by the caller after execution.
    let saved_font = table_cell_font(cell.font).map(|font| formatter.font.push_scope(font));
    if cell.font == Some(TableFont::Code) {
        formatter.font.begin_code_table_cell();
    }
    let mut children = if cell.vertical_continuation {
        context.lower_text(cell.decoder_text().unwrap_or_default(), formatter)
    } else {
        lower_table_cell(
            cell,
            recovery::CellPosition {
                index,
                row: &node.table_cells,
            },
            node,
            context,
            text_block,
            formatter,
        )
        .unwrap_or_default()
    };
    if let Some(saved_font) = saved_font {
        formatter.font.pop_scope(saved_font);
    }
    if cell.font == Some(TableFont::Code) {
        formatter.font.end_code_table_cell();
    }
    vec![Block::Paragraph {
        inline_layout: crate::mandoc::inline::take_inline_layout(&mut children),
        children,
        layout: LayoutHint::default(),
        source: source_span(node),
    }]
}
