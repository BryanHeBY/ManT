//! Assemble native tbl rows; source recovery has a separate transaction boundary.
use crate::mandoc::{LoweringContext, layout::layout, source_span};
use libmandoc_rs::{
    Node, TableAlignment as MandocTableAlignment, TableCellKind, TableFont,
    TableRowKind as MandocTableRowKind, TableRuleCellKind as MandocTableRuleCellKind,
};
use mant_ir::{
    Block, Inline, LayoutHint, TableAlignment as AstTableAlignment, TableCell as AstTableCell,
    TableRow,
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
        // Only native table spans carry a row kind. Keep the defensive guard
        // for synthetic test nodes and malformed foreign ASTs.
        debug_assert!(node.table_cells.is_empty());
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
                    let children = style_table_cell(children, cell.font);
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
            rows: vec![row],
            layout: layout(indent_columns),
            source: source_span(node),
        });
    }
}

// CVS tbl_html.c::print_tbl applies layout->font around each data cell.
// Preserve this parsed fact without changing table payload or link identity.
fn style_table_cell(children: Vec<Inline>, font: Option<TableFont>) -> Vec<Inline> {
    if children.is_empty() {
        return children;
    }
    let children = if matches!(
        font,
        Some(TableFont::Code | TableFont::CodeBold | TableFont::CodeItalic)
    ) {
        children.into_iter().map(code_font_text).collect()
    } else {
        children
    };
    match font {
        Some(TableFont::Bold | TableFont::CodeBold) => vec![Inline::Strong { children }],
        Some(TableFont::Italic | TableFont::CodeItalic) => vec![Inline::Emphasis { children }],
        Some(TableFont::BoldItalic) => vec![Inline::Strong {
            children: vec![Inline::Emphasis { children }],
        }],
        Some(TableFont::Code | TableFont::Roman) | None => children,
    }
}

fn code_font_text(inline: Inline) -> Inline {
    match inline {
        Inline::Text { value } => Inline::Code { value },
        Inline::Strong { children } => Inline::Strong {
            children: children.into_iter().map(code_font_text).collect(),
        },
        Inline::Emphasis { children } => Inline::Emphasis {
            children: children.into_iter().map(code_font_text).collect(),
        },
        Inline::Link {
            target,
            title,
            children,
        } => Inline::Link {
            target,
            title,
            children: children.into_iter().map(code_font_text).collect(),
        },
        other => other,
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
