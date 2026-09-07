//! Portable table cells flatten presentation, never semantic ownership.
use super::{inline::flatten_inline, mapped::MappedText};
use mant_ir::{Block, EntryOwner, TableCell, TableRow};

pub(super) fn rows(rows: &[TableRow], track: bool) -> Vec<MappedText> {
    super::super::table::table_slots(rows)
        .into_iter()
        .map(|row| {
            MappedText::join(
                row.into_iter().map(|(label, cell)| {
                    let mut value =
                        cell.map_or_else(MappedText::default, |cell| plain_cell(cell, track));
                    if let Some(column) = label {
                        value.insert(0, &format!("column {column}: "));
                    }
                    value
                }),
                " | ",
            )
        })
        .collect()
}

fn plain_cell(cell: &TableCell, track: bool) -> MappedText {
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
            MappedText::from(flatten_inline(children).trim().to_owned()).nonempty()
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
