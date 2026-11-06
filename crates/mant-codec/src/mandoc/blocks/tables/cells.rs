//! Decode real cell owners while recording the row's native input effects.
use super::recovery::lower_table_cell;
use super::{
    TableEmbedding, execution, recovery, table_alignment, table_cell_font, table_cell_kind,
    table_cell_width,
};
use crate::mandoc::{LoweringContext, source_span};
use libmandoc_rs::{Node, TableCellKind, TableFont};
use mant_ir::{Block, LayoutHint, TableCell as AstTableCell};

pub(super) fn lower_cells(
    node: &Node,
    kind: &mant_ir::TableRowKind,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    embedding: Option<&TableEmbedding<'_>>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Vec<AstTableCell> {
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
    let text_blocks = text_blocks(node, embedding);
    let mut cells = cell_owners(node);
    let mut buffered = false;
    let mut width_dependent_lines = Vec::new();
    if active {
        for column in &columns {
            // tbl_term.c::term_tbl: every non-hspan EXECUTION column enters,
            // including missing trailing data. All fills precede all flushes.
            formatter.clear_zero_advance();
            formatter.execution.table_buffer_written = Some(false);
            formatter.execution.table_line_needs_width = false;
            if let Some(index) = column.cell
                && !matches!(
                    column.kind,
                    libmandoc_rs::TableLayoutKind::Down
                        | libmandoc_rs::TableLayoutKind::Span
                        | libmandoc_rs::TableLayoutKind::HorizontalRule
                        | libmandoc_rs::TableLayoutKind::DoubleHorizontalRule
                )
                && node.table_cells[index].kind == TableCellKind::Text
            {
                let cell = &node.table_cells[index];
                let blocks =
                    project_text_cell(cell, index, node, context, text_blocks[index], formatter);
                if !cell.vertical_continuation {
                    cells[index].blocks = blocks;
                }
            }
            if std::mem::take(&mut formatter.execution.table_line_needs_width) {
                width_dependent_lines.push(column.logical);
            }
            buffered |= formatter
                .execution
                .table_buffer_written
                .take()
                .unwrap_or(false);
        }
        if !buffered
            && proved_columns
            && let Some(execution) = execution
        {
            match execution::buffered_padding(node, &columns, &width_dependent_lines, execution) {
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

fn text_blocks<'a>(
    node: &Node,
    embedding: Option<&'a TableEmbedding<'_>>,
) -> Vec<Option<&'a crate::mandoc::TableTextBlock>> {
    let mut text_block_index = 0;
    node.table_cells
        .iter()
        .map(|cell| {
            if !cell.text_block {
                return None;
            }
            let block = embedding
                .and_then(|embedding| embedding.blocks.get(text_block_index))
                .and_then(Option::as_ref);
            text_block_index += 1;
            block
        })
        .collect::<Vec<_>>()
}
fn cell_owners(node: &Node) -> Vec<AstTableCell> {
    node.table_cells
        .iter()
        .map(|cell| AstTableCell {
            preferred_width_columns: table_cell_width(node, cell),
            break_after: false,
            kind: table_cell_kind(cell.kind),
            blocks: Vec::new(),
            column_span: cell.column_span,
            row_span: cell.row_span,
            alignment: Some(table_alignment(cell.alignment)),
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
