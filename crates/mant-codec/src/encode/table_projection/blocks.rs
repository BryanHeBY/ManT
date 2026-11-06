//! Flatten nested content while preserving accepted word and row relations.
use super::{ParagraphTail, Projection, Tail, join, project_rows};
use crate::encode::accepted_rows::{AcceptedRows, ContentRole};
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
            let value = plain_block(
                block,
                if index + 1 == blocks.len() {
                    tail
                } else {
                    ParagraphTail::BlockBoundary
                },
                track,
                shared_first == Some(index),
            );
            // Block composition cannot reuse an authored literal row as a
            // semicolon-separated word. A different table cell still uses
            // the independent open-tail receipt in Projection::append.
            let seam =
                if output.physical && value.physical && (output.literal_tail || value.literal_tail)
                {
                    "\n"
                } else {
                    separator
                };
            output.append(value, seam);
        }
    }
    output
}

fn inline_projection(
    content: InlineContentRef<'_>,
    paragraph: Option<ParagraphTail>,
    shared_first: bool,
) -> Projection {
    let mut accepted = AcceptedRows::from_inlines(
        content.content,
        if paragraph.is_some() {
            ContentRole::Reading
        } else {
            ContentRole::Code
        },
    );
    let ends_in_break = accepted.ends_in_break();
    let physical = if paragraph.is_some() {
        !accepted.is_empty()
    } else {
        accepted.authored()
    };
    let tail = if !ends_in_break {
        Tail::Shared
    } else if matches!(paragraph, Some(ParagraphTail::BlockBoundary)) {
        // Nonfinal paragraphs retire one provisional empty row; their frame
        // closes the retained row. Literal content never retires that tail.
        accepted.retire_open_tail();
        Tail::EndRow
    } else {
        Tail::Open
    };
    let open_row_indent = if ends_in_break && matches!(tail, Tail::Open) {
        accepted.open_row_origin(content.layout)
    } else {
        0
    };
    let mapped = accepted.into_layout_text(content.layout, shared_first);
    Projection {
        literal_tail: paragraph.is_none() && physical,
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
            let value = if matches!(block, Block::Unsupported { .. }) {
                value.trim_matches('\n')
            } else {
                value.trim()
            };
            let text = AcceptedRows::from_text(
                value,
                if matches!(block, Block::Unsupported { .. }) {
                    ContentRole::Reading
                } else {
                    ContentRole::Code
                },
            )
            .into_layout_text(&mant_ir::InlineLayout::default(), false);
            let physical = !text.is_empty();
            Projection::text(text.into(), Tail::Shared, physical)
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => Projection::default(),
    }
}
