//! Flatten nested content while preserving accepted word and row relations.
use super::{ParagraphTail, Projection, Tail, join, project_rows};
use mant_ir::{Block, EntryOwner, InlineContentRef};

pub(super) fn plain_blocks(
    blocks: &[Block],
    separator: &str,
    tail: ParagraphTail,
    track: bool,
) -> Projection {
    plain_blocks_in_row(blocks, separator, tail, track, None)
}

fn plain_blocks_in_row(
    blocks: &[Block],
    separator: &str,
    tail: ParagraphTail,
    track: bool,
    shared_first: Option<usize>,
) -> Projection {
    let mut output = Projection::default();
    for (index, block) in blocks.iter().enumerate() {
        output.gap(mant_ir::geometry::block_gap(block));
        if !matches!(block, Block::VerticalSpace { .. }) {
            output.append(
                plain_block(
                    block,
                    if index + 1 == blocks.len() {
                        tail
                    } else {
                        ParagraphTail::BlockBoundary
                    },
                    track,
                    shared_first == Some(index),
                ),
                separator,
            );
        }
    }
    output
}

fn inline_projection(
    content: InlineContentRef<'_>,
    paragraph: Option<ParagraphTail>,
    shared_first: bool,
) -> Projection {
    let mut text = String::new();
    let mut ends_in_break = false;
    let mut authored_row = false;
    mant_ir::visit_inline_plain_text(content.content, |chunk| {
        authored_row = true;
        text.push_str(chunk);
        if let Some(last) = chunk.as_bytes().last() {
            ends_in_break = *last == b'\n';
        }
    });
    let physical = if paragraph.is_some() {
        !text.is_empty()
    } else {
        authored_row
    };
    let tail = if !ends_in_break {
        Tail::Shared
    } else if matches!(paragraph, Some(ParagraphTail::BlockBoundary)) {
        // Nonfinal paragraphs retire one provisional empty row; their frame
        // closes the retained row. Literal content never retires that tail.
        text.pop();
        Tail::EndRow
    } else {
        Tail::Open
    };
    let open_row_indent = if ends_in_break && matches!(tail, Tail::Open) {
        content
            .layout
            .row_indent(mant_ir::logical_row_count(content.content) - 1)
    } else {
        0
    };
    let mut mapped = String::new();
    for (row, text) in text.split('\n').enumerate() {
        if row > 0 {
            mapped.push('\n');
        }
        if !text.is_empty() {
            if row > 0 || !shared_first {
                mapped.push_str(
                    &" ".repeat(mant_ir::geometry::padding(content.layout.row_indent(row))),
                );
            }
            mapped.push_str(text);
        }
    }
    Projection {
        open_row_indent,
        ..Projection::text(mapped.into(), tail, physical)
    }
}

fn plain_block(block: &Block, tail: ParagraphTail, track: bool, shared_first: bool) -> Projection {
    match block {
        Block::Paragraph {
            children,
            inline_layout,
            ..
        } => inline_projection(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            Some(tail),
            shared_first,
        ),
        Block::Preformatted {
            children,
            inline_layout,
            ..
        } => inline_projection(
            InlineContentRef {
                content: children,
                layout: inline_layout,
            },
            None,
            shared_first,
        ),
        Block::List { items, .. } => join(
            items.iter().map(|item| {
                plain_blocks(&item.blocks, ", ", ParagraphTail::BlockBoundary, track)
                    .with_owner(EntryOwner::List(item), track)
            }),
            ", ",
        ),
        Block::DefinitionList { items, .. } => join(
            items.iter().map(|item| {
                let mut terms = join(
                    item.terms.iter().map(|term| {
                        inline_projection(
                            term.inline_content(),
                            Some(ParagraphTail::OpenCellRow),
                            false,
                        )
                    }),
                    "\n",
                );
                let shared = terms
                    .physical
                    .then(|| item.shared_description().map(|body| body.block_index))
                    .flatten();
                let description = plain_blocks_in_row(
                    &item.description,
                    "\n",
                    ParagraphTail::BlockBoundary,
                    track,
                    shared,
                );
                // Table simplification does not authorize changing the
                // accepted HEAD/BODY row or word seam. Geometric BODY origins
                // cannot insert cells into a joined word (termp_it_pre/post).
                let separator = if shared.is_none() {
                    "\n"
                } else if item.head_body_relation.joins_without_separator() {
                    ""
                } else {
                    " "
                };
                terms.append(description, separator);
                terms.with_owner(EntryOwner::Definition(item), track)
            }),
            "; ",
        ),
        Block::Table { rows, .. } => join(project_rows(rows, track), "; "),
        Block::Equation { value, .. } | Block::Unsupported { text: value, .. } => {
            let text = value.trim().to_owned();
            let physical = !text.is_empty();
            Projection::text(text.into(), Tail::Shared, physical)
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => Projection::default(),
    }
}
