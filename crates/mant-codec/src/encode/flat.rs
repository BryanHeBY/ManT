//! Portable table cells flatten presentation, never semantic ownership.
use super::{inline::flatten_inline, mapped::MappedText};
use mant_ir::{
    Block, EntryOwner, TableCell, TableCellKind, TableRow, TableRowPlan, TableRuleCellKind,
    bounded_table_rows,
};

pub(super) fn rows(rows: &[TableRow], track: bool) -> Vec<MappedText> {
    bounded_table_rows(rows)
        .into_iter()
        .zip(rows)
        .filter_map(|(plan, row)| (!mant_ir::table_row_is_navigation_only(row)).then_some(plan))
        .map(|row| match row {
            TableRowPlan::Empty => MappedText::default(),
            TableRowPlan::WholeRule { double } => {
                MappedText::from(if double { "===" } else { "---" }.to_owned())
            }
            TableRowPlan::LayoutRule { cells } => MappedText::join(
                cells.iter().map(|cell| {
                    MappedText::from(
                        match cell {
                            TableRuleCellKind::Horizontal => "---",
                            TableRuleCellKind::DoubleHorizontal => "===",
                        }
                        .to_owned(),
                    )
                }),
                " | ",
            ),
            TableRowPlan::Dense { slots } => MappedText::join(
                slots.into_iter().map(|cell| {
                    cell.map_or_else(MappedText::default, |cell| plain_cell(cell, track))
                }),
                " | ",
            ),
            TableRowPlan::Sparse { cells } => MappedText::join(
                cells.into_iter().map(|positioned| {
                    let mut value = plain_cell(positioned.cell, track);
                    value.insert(
                        0,
                        &format!("column {}: ", positioned.column.saturating_add(1)),
                    );
                    value
                }),
                " | ",
            ),
        })
        .collect()
}

fn plain_cell(cell: &TableCell, track: bool) -> MappedText {
    // tbl_term.c::term_tbl/tbl_hrule distinguish data, whole-row and
    // layout rules. The portable spelling keeps their order and strength;
    // a ruled cell's suppressed payload never becomes table text.
    match cell.kind {
        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => {
            return "---".to_owned().into();
        }
        TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
            return "===".to_owned().into();
        }
        TableCellKind::Text => {}
    }
    MappedText::join(
        cell.blocks
            .iter()
            .filter_map(|block| plain_block(block, track)),
        "; ",
    )
}

fn plain_block(block: &Block, track: bool) -> Option<MappedText> {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            // A cell can begin/end with executed hard rows (term.c::
            // ESCAPE_BREAK/term_fill; mdoc_term.c::termp_it_post). Flattening
            // its portable geometry must not trim those authored boundaries.
            MappedText::from(
                flatten_inline(children)
                    .trim_matches([' ', '\t'])
                    .to_owned(),
            )
            .nonempty()
        }
        Block::List { items, .. } => MappedText::join(
            items.iter().filter_map(|item| {
                MappedText::join(
                    item.blocks
                        .iter()
                        .filter_map(|block| plain_block(block, track)),
                    ", ",
                )
                .with_owner(EntryOwner::List(item), track)
                .nonempty()
            }),
            ", ",
        )
        .nonempty(),
        Block::DefinitionList { items, .. } => MappedText::join(
            items.iter().map(|item| {
                let terms = item
                    .terms
                    .iter()
                    .map(|term| flatten_inline(term))
                    .collect::<Vec<_>>()
                    .join(", ");
                let description = MappedText::join(
                    item.description
                        .iter()
                        .filter_map(|block| plain_block(block, track)),
                    "; ",
                );
                MappedText::join([terms.into(), description], ": ")
                    .with_owner(EntryOwner::Definition(item), track)
            }),
            "; ",
        )
        .nonempty(),
        Block::Table { rows: table, .. } => MappedText::join(rows(table, track), "; ").nonempty(),
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            MappedText::from(value.trim().to_owned()).nonempty()
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}
