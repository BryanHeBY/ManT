//! Assemble native tbl rows; source recovery has a separate transaction boundary.
use crate::mandoc::roff_escape::RoffFont;
use crate::mandoc::{LoweringContext, layout::layout, source_span};
use libmandoc_rs::{
    Node, TableAlignment as MandocTableAlignment, TableCellKind, TableFont,
    TableRowKind as MandocTableRowKind, TableRuleCellKind as MandocTableRuleCellKind,
};
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
    let Some(kind) = table_row_kind(node) else {
        // Only native table spans carry a row kind; foreign or synthetic
        // nodes without one (even if they carry cells) are tolerated and
        // dropped here rather than treated as an invariant violation.
        return;
    };
    if !matches!(&kind, mant_ir::TableRowKind::Data) && !node.table_cells.is_empty() {
        // CVS whole-row rules do not own data cells. Refuse to reinterpret a
        // structurally inconsistent foreign snapshot as printable contents.
        return;
    }
    let mut text_block_index = 0;
    let row = TableRow {
        kind,
        // `tbl_data.c` determines field boundaries using the table's executed
        // delimiter and escape state. The owned native row is consequently
        // the only authority for how many cells exist. Source text can enrich
        // one proven cell below, but must never manufacture a new column from
        // a raw TAB (or from an alternate `tab()` delimiter).
        cells: (0..node.table_cells.len())
            .map(|index| {
                // tbl_term.c clears both backtracking flags before each data
                // cell. A bare `\z` produced by the final cell can survive
                // the table, but input state and preceding cells cannot enter
                // this one.
                formatter.clear_zero_advance();
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
                    // CVS tbl_html.c::print_tbl and tbl_term.c::tbl_word
                    // select the layout font before printing the cell word.
                    // In-word \f escapes can therefore override it, while
                    // the selection itself ends at the cell boundary.
                    let saved_font =
                        table_cell_font(cell.font).map(|font| formatter.font.push_scope(font));
                    if cell.font == Some(TableFont::Code) {
                        formatter.font.begin_code_table_cell();
                    }
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
                    if let Some(saved_font) = saved_font {
                        formatter.font.pop_scope(saved_font);
                    }
                    if cell.font == Some(TableFont::Code) {
                        formatter.font.end_code_table_cell();
                    }
                    // `tbl_term.c` clears BACKAFTER/BACKBEFORE before every
                    // cell and the row flush clears them again whenever this
                    // cell actually populated the native buffer.  Only a
                    // completely bare `\z` cell can carry its armed state
                    // beyond the final column.
                    if table_cell_occupies_formatter(cell, &children) {
                        formatter.clear_zero_advance();
                    }
                    vec![Block::Paragraph {
                        children,
                        layout: LayoutHint::default(),
                        source: source_span(node),
                    }]
                };
                AstTableCell {
                    kind: table_cell_kind(cell.kind),
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
            column_widths: Vec::new(),
            rows: vec![row],
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
}

fn table_cell_font(font: Option<TableFont>) -> Option<RoffFont> {
    match font {
        Some(TableFont::Bold) => Some(RoffFont::Strong),
        Some(TableFont::Italic) => Some(RoffFont::Emphasis),
        Some(TableFont::BoldItalic) => Some(RoffFont::StrongEmphasis),
        Some(TableFont::CodeBold) => Some(RoffFont::CodeStrong),
        Some(TableFont::CodeItalic) => Some(RoffFont::CodeEmphasis),
        // CVS tbl_term.c::tbl_word does not push CR or Roman: an enclosing
        // .Bf/.ft selection stays on the terminal stack. CR presentation is
        // applied separately from that execution state.
        Some(TableFont::Code | TableFont::Roman) | None => None,
    }
}

fn table_row_kind(node: &Node) -> Option<mant_ir::TableRowKind> {
    match &node.table_row_kind {
        Some(MandocTableRowKind::Data) => Some(mant_ir::TableRowKind::Data),
        Some(MandocTableRowKind::HorizontalRule) => Some(mant_ir::TableRowKind::HorizontalRule),
        Some(MandocTableRowKind::DoubleHorizontalRule) => {
            Some(mant_ir::TableRowKind::DoubleHorizontalRule)
        }
        Some(MandocTableRowKind::LayoutRule { cells }) => Some(mant_ir::TableRowKind::LayoutRule {
            cells: cells
                .iter()
                .map(|cell| match cell {
                    MandocTableRuleCellKind::Horizontal => mant_ir::TableRuleCellKind::Horizontal,
                    MandocTableRuleCellKind::DoubleHorizontal => {
                        mant_ir::TableRuleCellKind::DoubleHorizontal
                    }
                })
                .collect(),
        }),
        None => None,
    }
}

const fn table_cell_kind(kind: TableCellKind) -> mant_ir::TableCellKind {
    match kind {
        TableCellKind::HorizontalRule => mant_ir::TableCellKind::HorizontalRule,
        TableCellKind::DoubleHorizontalRule => mant_ir::TableCellKind::DoubleHorizontalRule,
        TableCellKind::IsolatedHorizontalRule => mant_ir::TableCellKind::IsolatedHorizontalRule,
        TableCellKind::IsolatedDoubleHorizontalRule => {
            mant_ir::TableCellKind::IsolatedDoubleHorizontalRule
        }
        TableCellKind::Text | TableCellKind::Empty => mant_ir::TableCellKind::Text,
    }
}

fn table_cell_occupies_formatter(
    cell: &libmandoc_rs::TableCell,
    children: &[mant_ir::Inline],
) -> bool {
    if !children.is_empty() {
        return true;
    }
    cell.text.as_deref().is_some_and(|text| {
        crate::mandoc::roff_escape::decode(text)
            .iter()
            .any(|event| {
                !matches!(
                    crate::mandoc::roff_escape::inline_event_effect(event),
                    crate::mandoc::roff_escape::InlineEventEffect::StateOnly
                )
            })
    })
}
