//! Assemble native tbl rows; source recovery has a separate transaction boundary.
use crate::mandoc::{LoweringContext, layout::layout, source_span};
use libmandoc_rs::{Node, TableAlignment as MandocTableAlignment, TableCellKind};
use mant_ir::{
    Block, LayoutHint, TableAlignment as AstTableAlignment, TableCell as AstTableCell, TableRow,
};
mod recovery;
pub(super) use recovery::{TableEmbedding, TableEmbeddingPlan};
use recovery::{lower_missing_table_cell, lower_table_cell};

pub(super) fn append_table_row(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    embedding: Option<&TableEmbedding<'_>>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    if node.table_cells.is_empty() {
        return;
    }
    let mut text_block_index = 0;
    let source_cells = context.tab_separated_table_cells(node.line);
    let cell_count = node
        .table_cells
        .len()
        .max(source_cells.as_ref().map_or(0, Vec::len));
    let row = TableRow {
        cells: (0..cell_count)
            .map(|index| {
                let cell = node.table_cells.get(index);
                let vertical_continuation = cell.is_some_and(|cell| cell.vertical_continuation);
                let text_block = if cell.is_some_and(|cell| cell.text_block) {
                    let block =
                        embedding.and_then(|embedding| embedding.blocks.get(text_block_index));
                    text_block_index += 1;
                    block
                } else {
                    None
                };
                let raw_source = source_cells
                    .as_ref()
                    .and_then(|cells| cells.get(index))
                    .copied();
                let blocks = if vertical_continuation
                    || cell.is_some_and(|cell| cell.kind != TableCellKind::Text)
                {
                    // `\^` is tbl's vertical-span control marker. The
                    // preceding cell owns the actual content and its copied
                    // `row_span`; rendering the marker as text would invent
                    // a visible token that groff and mandoc both suppress.
                    // Rules likewise own layout, not their data-column
                    // payload: source recovery must never resurrect it.
                    Vec::new()
                } else {
                    let children = if let Some(cell) = cell {
                        let lowered = lower_table_cell(
                            cell,
                            recovery::CellPosition {
                                index,
                                row: &node.table_cells,
                            },
                            node,
                            context,
                            text_block,
                            embedding.map_or(&[], |embedding| embedding.nodes.as_slice()),
                            formatter,
                        );
                        match lowered {
                            // A successfully decoded control-only cell is
                            // empty, not missing source that needs recovery.
                            Some(inlines) => inlines,
                            None if text_block.is_none() => {
                                lower_missing_table_cell(raw_source, node, context, formatter)
                            }
                            None => Vec::new(),
                        }
                    } else {
                        lower_missing_table_cell(raw_source, node, context, formatter)
                    };
                    vec![Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: source_span(node),
                    }]
                };
                AstTableCell {
                    blocks,
                    column_span: cell.map_or(1, |cell| cell.column_span),
                    row_span: cell.map_or(1, |cell| cell.row_span),
                    alignment: Some(match cell.map(|cell| cell.alignment) {
                        None | Some(MandocTableAlignment::Left) => AstTableAlignment::Left,
                        Some(MandocTableAlignment::Center) => AstTableAlignment::Center,
                        Some(MandocTableAlignment::Right) => AstTableAlignment::Right,
                    }),
                }
            })
            .collect(),
    };
    if !node.flags.table_start
        && let Some(Block::Table { rows, .. }) = output.last_mut()
    {
        rows.push(row);
    } else {
        output.push(Block::Table {
            rows: vec![row],
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
}
