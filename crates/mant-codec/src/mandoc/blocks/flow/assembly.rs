//! Source-proven literal owners, assembled only after execution has finished.
//!
//! Formatter drains are not document boundaries. Keep their indices stable
//! while links and row receipts still refer to them, then collect each actual
//! no-fill/display owner into one renderer-neutral content block.

use std::ops::Range;

use mant_ir::{Block, Inline, InlineLayout, LayoutHint, RowLayoutHint};

#[derive(Default)]
pub(in crate::mandoc::blocks) struct LiteralAssembly {
    ranges: Vec<Range<usize>>,
    active: Option<usize>,
    pending_close: bool,
}

impl LiteralAssembly {
    pub(in crate::mandoc::blocks) fn begin(&mut self, output: usize) {
        self.active.get_or_insert(output);
    }

    pub(in crate::mandoc::blocks) fn mark_close(&mut self) {
        self.pending_close = self.active.is_some();
    }

    pub(in crate::mandoc::blocks) const fn close_pending(&self) -> bool {
        self.pending_close
    }

    pub(in crate::mandoc::blocks) fn after_drain(&mut self, output: usize) {
        if self.pending_close {
            self.end(output);
        }
    }

    pub(in crate::mandoc::blocks) fn end(&mut self, output: usize) {
        if let Some(start) = self.active.take()
            && start < output
        {
            self.ranges.push(start..output);
        }
        self.pending_close = false;
    }

    pub(in crate::mandoc::blocks) fn remove(&mut self, index: usize) {
        for range in &mut self.ranges {
            range.start -= usize::from(range.start > index);
            range.end -= usize::from(range.end > index);
        }
        if let Some(start) = &mut self.active {
            *start -= usize::from(*start > index);
        }
    }

    pub(in crate::mandoc::blocks) fn insert(&mut self, index: usize, count: usize) {
        for range in &mut self.ranges {
            if range.start >= index {
                range.start += count;
            }
            if range.end > index {
                range.end += count;
            }
        }
        if let Some(start) = &mut self.active
            && *start > index
        {
            *start += count;
        }
    }

    pub(in crate::mandoc::blocks) fn finish(mut self, output: Vec<Block>) -> Vec<Block> {
        self.end(output.len());
        let mut input = output.into_iter().enumerate().peekable();
        let mut result = Vec::new();
        for range in self.ranges {
            while input.peek().is_some_and(|(index, _)| *index < range.start) {
                result.push(input.next().expect("checked output prefix").1);
            }
            let mut group = Vec::new();
            while input.peek().is_some_and(|(index, _)| *index < range.end) {
                group.push(input.next().expect("checked literal owner").1);
            }
            assemble(group, &mut result);
        }
        result.extend(input.map(|(_, block)| block));
        result
    }
}

fn assemble(blocks: Vec<Block>, output: &mut Vec<Block>) {
    let mut owner: Option<AssembledLiteral> = None;
    let mut gaps = Vec::new();
    for block in blocks {
        match block {
            Block::VerticalSpace { .. } => gaps.push(block),
            Block::Preformatted {
                ref children,
                ref inline_layout,
                ref layout,
                ..
            } if !mant_ir::geometry::has_literal_rows(children)
                && (!inline_layout.is_empty() || layout.spacing_before_lines != 0) =>
            {
                if let Some(previous) = owner.take() {
                    output.push(previous.block);
                }
                output.append(&mut gaps);
                output.push(block);
            }
            Block::Preformatted { .. } => {
                if let Some(previous) = &mut owner {
                    append_literal(previous, block, &mut gaps);
                } else {
                    // A leading gap can precede any accepted literal content;
                    // retain it outside the block instead of inventing a row.
                    output.append(&mut gaps);
                    owner = Some(AssembledLiteral::new(block));
                }
            }
            // A transparent target-only fallback has no content row. Preserve
            // its identity inside the actual literal owner, not as a second
            // styled surface. Other structures remain real boundaries.
            Block::Paragraph {
                children,
                inline_layout,
                layout,
                ..
            } if !mant_ir::geometry::has_literal_rows(&children)
                && inline_layout.is_empty()
                && layout.spacing_before_lines == 0
                && owner.is_some() =>
            {
                if let Some(AssembledLiteral {
                    block:
                        Block::Preformatted {
                            children: previous, ..
                        },
                    ..
                }) = &mut owner
                {
                    previous.extend(children);
                }
            }
            other => {
                if let Some(previous) = owner.take() {
                    output.push(previous.block);
                }
                output.append(&mut gaps);
                output.push(other);
            }
        }
    }
    if let Some(previous) = owner {
        output.push(previous.block);
    }
    // A trailing gap is not followed by a literal word proving its ownership.
    // Keep the existing resolved boundary rather than speculating about it.
    output.append(&mut gaps);
}

struct AssembledLiteral {
    block: Block,
    rows: usize,
    has_rows: bool,
}

impl AssembledLiteral {
    fn new(block: Block) -> Self {
        let Block::Preformatted { children, .. } = &block else {
            unreachable!("only literal fragments start literal assembly")
        };
        Self {
            rows: mant_ir::logical_row_count(children),
            has_rows: mant_ir::geometry::has_literal_rows(children),
            block,
        }
    }
}

fn append_literal(previous: &mut AssembledLiteral, incoming: Block, gaps: &mut Vec<Block>) {
    let Block::Preformatted {
        children,
        inline_layout,
        layout,
        language,
        ..
    } = &mut previous.block
    else {
        unreachable!("literal assembly only appends to literal owners")
    };
    let Block::Preformatted {
        children: incoming,
        inline_layout: incoming_hints,
        layout: incoming_layout,
        language: incoming_language,
        ..
    } = incoming
    else {
        unreachable!("literal assembly only accepts literal fragments")
    };
    debug_assert_eq!(*language, incoming_language);
    let has_after = mant_ir::geometry::has_literal_rows(&incoming);
    if !has_after {
        // Targets do not occupy a content row or consume pending spacing.
        // Guarded above, this fragment owns no independent layout metadata.
        children.extend(incoming);
        return;
    }
    let mut gap = mant_ir::geometry::GapPlan::default();
    for block in gaps.drain(..) {
        gap.append_resolved(mant_ir::geometry::block_gap(&block));
    }
    gap.append_resolved(incoming_layout.spacing_before_lines);
    let incoming_rows = mant_ir::logical_row_count(&incoming);
    let gap_rows = usize::from(gap.rows(0));
    let separator = if previous.has_rows && has_after {
        children.extend(std::iter::repeat_n(Inline::line_break(), gap_rows + 1));
        gap_rows + 1
    } else if gap_rows > 0 {
        children.extend(std::iter::repeat_n(Inline::line_break(), gap_rows));
        gap_rows
    } else {
        0
    };
    let row_offset = previous.rows.saturating_add(separator).saturating_sub(1);
    rebase_hints(
        inline_layout,
        incoming_hints,
        &incoming,
        row_offset,
        *layout,
        incoming_layout,
    );
    children.extend(incoming);
    previous.rows = previous
        .rows
        .saturating_add(separator)
        .saturating_add(incoming_rows.saturating_sub(1));
    previous.has_rows |= has_after || separator > 0;
}

fn rebase_hints(
    destination: &mut InlineLayout,
    incoming: InlineLayout,
    content: &[Inline],
    offset: usize,
    destination_layout: LayoutHint,
    source_layout: LayoutHint,
) {
    let continuation = source_layout
        .indent_columns
        .saturating_add(source_layout.continuation_indent_columns)
        .saturating_sub(destination_layout.indent_columns)
        .saturating_sub(destination_layout.continuation_indent_columns);
    let first = destination_layout
        .continuation_indent_columns
        .saturating_sub(source_layout.continuation_indent_columns);
    let mut hints = incoming.row_hints.into_iter().peekable();
    for row in 0..mant_ir::logical_row_count(content) {
        let row = u32::try_from(row).unwrap_or(u32::MAX);
        let mut hint = if hints.peek().is_some_and(|hint| hint.row == row) {
            hints.next().expect("checked row hint")
        } else {
            RowLayoutHint {
                row,
                ..RowLayoutHint::default()
            }
        };
        hint.row = hint
            .row
            .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
        hint.indent_columns = hint.indent_columns.saturating_add(continuation);
        hint.first_line_indent_columns = hint.first_line_indent_columns.saturating_add(first);
        if !hint.is_empty() {
            destination.row_hints.push(hint);
        }
    }
}
