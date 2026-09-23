//! Portable table cells flatten presentation while retaining exact byte
//! placements for the logical roots they emit. Semantic owner scopes remain
//! independently attached to the nested item ranges.
use super::{inline::flatten_inline, mapped::MappedText, source_map::RenderedRootRange};
use mant_ir::{
    Block, ContentContext, ContentRootKey, EntryOwner, Inline, InlineView, TableCell, TableRow,
    TableRowPlan, bounded_table_rows,
};

pub(super) fn rows(content: ContentContext<'_>, rows: &[TableRow], track: bool) -> Vec<MappedText> {
    bounded_table_rows(rows)
        .into_iter()
        .map(|row| match row {
            TableRowPlan::Empty
            | TableRowPlan::WholeRule { .. }
            | TableRowPlan::LayoutRule { .. } => MappedText::default(),
            TableRowPlan::Dense { slots } => MappedText::join(
                slots.into_iter().map(|cell| {
                    cell.map_or_else(MappedText::default, |cell| plain_cell(content, cell, track))
                }),
                " | ",
            ),
            TableRowPlan::Sparse { cells } => MappedText::join(
                cells.into_iter().map(|positioned| {
                    let mut value = plain_cell(content, positioned.cell, track);
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

fn plain_cell(content: ContentContext<'_>, cell: &TableCell, track: bool) -> MappedText {
    MappedText::join(
        cell.blocks
            .iter()
            .filter_map(|block| plain_block(content, block, track)),
        "; ",
    )
}

fn plain_block(content: ContentContext<'_>, block: &Block, track: bool) -> Option<MappedText> {
    match block {
        Block::Paragraph { children, .. }
        | Block::Preformatted { children, .. }
        | Block::FixedDisplay { children, .. } => {
            plain_inline(content, children, track).trim().nonempty()
        }
        Block::List { items, .. } => MappedText::join(
            items.iter().filter_map(|item| {
                MappedText::join(
                    item.blocks
                        .iter()
                        .filter_map(|block| plain_block(content, block, track)),
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
                let terms = MappedText::join(
                    item.terms
                        .iter()
                        .map(|term| plain_inline(content, term, track)),
                    ", ",
                );
                let description = MappedText::join(
                    item.description
                        .iter()
                        .filter_map(|block| plain_block(content, block, track)),
                    "; ",
                );
                MappedText::join([terms, description], ": ")
                    .with_owner(EntryOwner::Definition(item), track)
            }),
            "; ",
        )
        .nonempty(),
        Block::Table { rows: table, .. } => {
            MappedText::join(rows(content, table, track), "; ").nonempty()
        }
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            MappedText::from(value.trim().to_owned()).nonempty()
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => None,
    }
}

fn plain_inline(content: ContentContext<'_>, nodes: &[Inline], track: bool) -> MappedText {
    // A cell may contain several roots (and one root may occur in fragments).
    // Mapping each leaf before table delimiters are inserted avoids claiming
    // the entire cell for every root when assigning search hit ownership.
    fn push(output: &mut MappedText, root: ContentRootKey, value: &str) {
        if value.is_empty() {
            return;
        }
        let start = output.text.len();
        output.text.push_str(value);
        if let Some(previous) = output.roots.last_mut()
            && previous.root == root
            && previous.markdown.end == start
        {
            previous.markdown.end = output.text.len();
        } else {
            output.roots.push(RenderedRootRange {
                root,
                markdown: start..output.text.len(),
            });
        }
    }

    fn walk(content: ContentContext<'_>, nodes: &[Inline], output: &mut MappedText) {
        for node in nodes {
            match content
                .inline(node)
                .expect("validated document content resolves through its content store")
            {
                InlineView::Text(value) | InlineView::Code(value) => {
                    let (Inline::Text { content: range } | Inline::Code { content: range }) = node
                    else {
                        unreachable!("resolved text keeps its structural leaf")
                    };
                    let root = content.atom(range.atom).expect("resolved atom").root;
                    push(output, root, value);
                }
                InlineView::Strong(children) | InlineView::Emphasis(children) => {
                    walk(content, children, output);
                }
                InlineView::Link(link) => walk(content, link.children(), output),
                InlineView::Anchor(_) => {}
                InlineView::LineBreak => {
                    let Inline::LineBreak { atom } = node else {
                        unreachable!("resolved break keeps its structural leaf")
                    };
                    let root = content.atom(*atom).expect("resolved atom").root;
                    push(output, root, "\n");
                }
                _ => unreachable!("all inline views are handled"),
            }
        }
    }

    if !track {
        return flatten_inline(content, nodes).into();
    }

    let mut output = MappedText::default();
    walk(content, nodes, &mut output);
    output
}
