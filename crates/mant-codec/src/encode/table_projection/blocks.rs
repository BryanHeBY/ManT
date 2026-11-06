//! Flatten nested content while preserving accepted word and row relations.
use super::{ParagraphTail, Projection, Tail, join, project_rows};
use mant_ir::{Block, EntryOwner, Inline, InlineContentRef};

#[derive(Clone, Copy)]
enum ContentRole {
    Reading,
    Code,
}

/// Finish role-specific content before a parent records its mapped byte
/// extent. A table fence is syntax, not permission to reinterpret Text CR.
fn append_content(
    output: &mut String,
    value: &str,
    role: ContentRole,
    previous_code_cr: &mut bool,
) {
    match role {
        ContentRole::Reading => {
            if !value.is_empty() {
                *previous_code_cr = false;
            }
            for character in value.chars() {
                if character == '\r' {
                    output.push('\u{fffd}');
                } else if crate::text_safety::is_safe_source_character(character) {
                    output.push(character);
                } else {
                    // Keep the existing checked hand-IR control projection;
                    // accepted producer content was already admitted once.
                    output.extend(std::iter::repeat_n(' ', character.len_utf8()));
                }
            }
        }
        ContentRole::Code => {
            let normalized = crate::text_safety::markdown_code_text(value, false);
            // Styling and Text-node fragmentation do not turn a CRLF in
            // one literal flow into two lines. An explicit LineBreak below
            // is instead an independently authored hard-boundary event.
            output.push_str(if *previous_code_cr && value.starts_with('\n') {
                &normalized[1..]
            } else {
                &normalized
            });
            if !value.is_empty() {
                *previous_code_cr = value.ends_with('\r');
            }
        }
    }
}

fn inline_content(
    output: &mut String,
    nodes: &[Inline],
    role: ContentRole,
    previous_code_cr: &mut bool,
) -> bool {
    let mut authored = false;
    for node in nodes {
        match node {
            Inline::Text { value } => {
                authored = true;
                append_content(output, value, role, previous_code_cr);
            }
            Inline::Code { value } | Inline::Equation { value, .. } => {
                authored = true;
                append_content(output, value, ContentRole::Code, previous_code_cr);
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                authored |= inline_content(output, children, role, previous_code_cr);
            }
            Inline::LineBreak {} => {
                authored = true;
                *previous_code_cr = false;
                output.push('\n');
            }
            Inline::Anchor { .. } => {}
        }
    }
    authored
}

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
    let mut text = String::new();
    let authored_row = inline_content(
        &mut text,
        content.content,
        if paragraph.is_some() {
            ContentRole::Reading
        } else {
            ContentRole::Code
        },
        &mut false,
    );
    let ends_in_break = text.ends_with('\n');
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
        // Code-role CR canonicalization can introduce an accepted LF that
        // was not an original logical row. Never reuse the previous row's
        // hint for the newly opened tail just because raw IR counted fewer.
        content
            .layout
            .row_layout(text.bytes().filter(|byte| *byte == b'\n').count())
            .resolve(0, 0)
            .first_visual_origin
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
                    &" ".repeat(mant_ir::geometry::padding(
                        content
                            .layout
                            .row_layout(row)
                            .resolve(0, 0)
                            .first_visual_origin,
                    )),
                );
            }
            mapped.push_str(text);
        }
    }
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
            let mut text = String::new();
            append_content(
                &mut text,
                value,
                if matches!(block, Block::Unsupported { .. }) {
                    ContentRole::Reading
                } else {
                    ContentRole::Code
                },
                &mut false,
            );
            let physical = !text.is_empty();
            Projection::text(text.into(), Tail::Shared, physical)
        }
        Block::VerticalSpace { .. } | Block::ThematicBreak { .. } => Projection::default(),
    }
}
