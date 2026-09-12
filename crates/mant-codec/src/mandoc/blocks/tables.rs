//! Assemble native tbl rows; source recovery has a separate transaction boundary.
use crate::mandoc::{LoweringContext, layout::layout, source_span};
use libmandoc_rs::{Node, TableAlignment as MandocTableAlignment, TableCellKind};
use mant_ir::{
    Block, LayoutHint, TableAlignment as AstTableAlignment, TableCell as AstTableCell, TableRow,
};
mod recovery;
use recovery::lower_table_cell;
pub(super) use recovery::{TableEmbedding, TableEmbeddingPlan};

pub(super) fn append_table_row(
    output: &mut Vec<Block>,
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    embedding: Option<&TableEmbedding>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    if node.table_cells.is_empty() {
        return;
    }
    let mut text_block_index = 0;
    let row = TableRow {
        // `tbl_data.c` determines field boundaries using the table's executed
        // delimiter and escape state. The owned native row is consequently
        // the only authority for how many cells exist. Source text can enrich
        // one proven cell below, but must never manufacture a new column from
        // a raw TAB (or from an alternate `tab()` delimiter).
        cells: (0..node.table_cells.len())
            .map(|index| {
                let cell = &node.table_cells[index];
                let vertical_continuation = cell.vertical_continuation;
                let text_block = if cell.text_block {
                    let block =
                        embedding.and_then(|embedding| embedding.blocks.get(text_block_index));
                    text_block_index += 1;
                    block
                } else {
                    None
                };
                let blocks = if vertical_continuation || cell.kind != TableCellKind::Text {
                    // `\^` is tbl's vertical-span control marker. The
                    // preceding cell owns the actual content and its copied
                    // `row_span`; rendering the marker as text would invent
                    // a visible token that groff and mandoc both suppress.
                    // Rules likewise own layout, not their data-column
                    // payload: source recovery must never resurrect it.
                    Vec::new()
                } else {
                    let children = lower_table_cell(
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
                    // A successfully decoded control-only cell is empty, not
                    // missing source that needs synthetic recovery.
                    .unwrap_or_default();
                    vec![Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: source_span(node),
                    }]
                };
                AstTableCell {
                    blocks,
                    column_span: cell.column_span,
                    row_span: cell.row_span,
                    alignment: Some(match cell.alignment {
                        MandocTableAlignment::Left => AstTableAlignment::Left,
                        MandocTableAlignment::Center => AstTableAlignment::Center,
                        MandocTableAlignment::Right => AstTableAlignment::Right,
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
