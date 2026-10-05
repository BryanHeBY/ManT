//! Paragraph and literal flow own pending text, provenance and flush boundaries.
use super::{FilledBoundary, InlineBuilder, layout, targets};
use mant_ir::{Block, Inline};

mod columns;
mod input;
mod literal;
mod output;
mod paragraph;
#[cfg(test)]
mod tests;
use crate::mandoc::inline::OutputRowEnd;
use literal::LiteralFlow;
use paragraph::ParagraphFlow;

#[derive(Clone, Copy)]
struct ClosedOutputTail {
    output_end: usize,
    owner: usize,
    row_end: OutputRowEnd,
}

pub(super) struct BlockState {
    pub(super) output: Vec<Block>,
    pub(super) formatter: crate::mandoc::formatter::FormatterState,
    // Filled and literal buffers are independent, not mutually exclusive modes.
    paragraph: ParagraphFlow,
    literal: LiteralFlow,
    // A column display transfers the active projection vector to LiteralFlow.
    // Its source and native posts keep borrowing that destination, including
    // an empty vector or a device row which has just ended. Neither fact is
    // permission to run the same native buffer against a different Vec.
    column_display_owner: bool,
    // Only the latest accepted output tail can hand a closed graph boundary
    // to its cell. Transparent structures relocate this owner position;
    // later accepted graph reopens the tail.
    closed_output_tail: Option<ClosedOutputTail>,
    pending_targets: targets::PendingTargets,
    // A paragraph pre request can finish its BODY with a live text row. Its
    // leading distance belongs to the next emitted block, not a Rust return.
    pending_spacing: Option<(u16, Option<mant_ir::SourceSpan>)>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    hanging_origin: Option<crate::mandoc::layout::SourceIndent>,
}

#[derive(Clone, Copy)]
pub(super) struct LinkOutputCursor {
    output: usize,
    node_id: u32,
}

impl BlockState {
    pub(super) fn link_output_cursor(&mut self, node_id: u32) -> LinkOutputCursor {
        let marker = super::man_links::link_cursor_marker(node_id);
        if self.formatter.no_fill || self.column_uses_literal_output() {
            self.literal.insert_link_cursor(marker);
        } else {
            self.paragraph.insert_link_cursor(marker);
        }
        LinkOutputCursor {
            output: self.output.len(),
            node_id,
        }
    }

    pub(super) fn wrap_first_link_since(
        &mut self,
        cursor: LinkOutputCursor,
        target: &mant_ir::LinkTarget,
        skip_prior_glyph: bool,
    ) -> bool {
        let mut skip_visible = usize::from(skip_prior_glyph);
        let marker = super::man_links::link_cursor_marker(cursor.node_id);
        let mut started = false;
        for index in cursor.output..self.output.len() {
            let block = &mut self.output[index];
            let wrapped = super::man_links::wrap_first_visible_block(
                block,
                target,
                &marker,
                &mut started,
                &mut skip_visible,
            );
            if started {
                // HTML closes the annotation when this output owner closes
                // (man_html.c::man_IP_pre and html_close_paragraph). A
                // rejected/empty label cannot migrate to a later list item.
                if matches!(block, Block::Paragraph { children, .. } | Block::Preformatted { children, .. } if children.is_empty())
                {
                    self.output.remove(index);
                }
                return wrapped;
            }
        }
        let wrapped =
            self.paragraph
                .wrap_first_link(target, &marker, &mut started, &mut skip_visible);
        if started {
            return wrapped;
        }
        self.literal
            .wrap_first_link(target, &marker, &mut started, &mut skip_visible)
    }

    pub(super) fn discard_link_cursor(&mut self, cursor: LinkOutputCursor) {
        let marker = super::man_links::link_cursor_marker(cursor.node_id);
        let mut scoped_output = self.output.split_off(cursor.output);
        scoped_output
            .retain_mut(|block| !super::man_links::remove_link_cursor_block(block, &marker));
        self.output.extend(scoped_output);
        self.paragraph.discard_link_cursor(&marker);
        self.literal.discard_link_cursor(&marker);
    }

    pub(super) const fn source_indent(&self) -> crate::mandoc::layout::SourceIndent {
        self.indent_columns
    }

    pub(super) fn set_source_indent(&mut self, indent: crate::mandoc::layout::SourceIndent) {
        self.flush_preformatted();
        self.flush_paragraph();
        self.indent_columns = indent;
        self.hanging_origin = None;
    }

    pub(super) fn start_hanging(&mut self, origin: crate::mandoc::layout::SourceIndent) {
        self.hanging_origin = Some(origin);
    }

    pub(super) fn consume_hanging_first_line(&mut self) {
        if let Some(origin) = self.hanging_origin.take() {
            self.indent_columns = origin;
        }
    }

    pub(super) fn with_output(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
        output: Vec<Block>,
        mut formatter: crate::mandoc::formatter::FormatterState,
    ) -> Self {
        formatter.set_spacing_enabled(spacing_enabled);
        Self {
            output,
            formatter,
            paragraph: ParagraphFlow::new(),
            literal: LiteralFlow::new(),
            column_display_owner: false,
            closed_output_tail: None,
            pending_targets: targets::PendingTargets::new(),
            pending_spacing: None,
            indent_columns,
            hanging_origin: None,
        }
    }

    pub(super) fn inherit_scope_posts(&mut self, posts: crate::mandoc::containers::ScopePostState) {
        self.formatter.execution.scope_posts = posts;
    }

    pub(super) fn spacing_enabled(&self) -> bool {
        self.formatter.spacing_enabled()
    }

    pub(super) fn inherit_author_execution(
        &mut self,
        flow: crate::mandoc::formatter::AuthorFlow,
        authors_section: bool,
    ) {
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder.inherit_author_execution(flow, authors_section);
            });
    }

    pub(super) fn inherit_run_in_execution(
        &mut self,
        state: crate::mandoc::inline::PreservedInlineState,
        generated_cells: usize,
        native_generated_cells: usize,
        generated_word: bool,
        entry: Option<&libmandoc_rs::Node>,
    ) {
        self.paragraph
            .inherit_preserved_execution(&mut self.formatter, state);
        if let Some(entry) = entry {
            // print_mdoc_node() enters the actual It BODY before its pre
            // emits the run-in separator. Explicitly closed extended heads
            // give this BODY NODE_LINE; ordinary Bq does not. Observe these
            // facts now, never infer them from a later child or topology.
            let closes_head = entry.flags.no_fill
                && entry.flags.line_start
                && !self.formatter.execution.source_row_continues();
            if entry.flags.line_start {
                self.formatter.note_definition_source_line();
            }
            self.paragraph
                .with_inline_builder(&mut self.formatter, |builder| {
                    builder.observe_no_fill_source_lines(true);
                    builder.begin_executed_node(entry);
                    builder.observe_no_fill_source_lines(false);
                });
            if closes_head {
                if self.formatter.consume_definition_head_row() {
                    self.paragraph.consume_invisible_head_row();
                }
                self.formatter.settle_definition_head_rows();
            }
        }
        self.paragraph.append_run_in_cells(
            &mut self.formatter,
            generated_cells,
            native_generated_cells,
            generated_word,
        );
        self.formatter.note_definition_run_in_executed();
    }
}

fn has_formatter_text_cell(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Text { .. } | Inline::Code { .. } | Inline::Equation { .. } => true,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => has_formatter_text_cell(children),
        Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
    })
}

/// Only actual buffered rows satisfy an unconditional formatter flush.
/// Zero-width target blocks remain available without masquerading as rows.
pub(super) fn has_flushed_row(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Preformatted { children, .. } => mant_ir::geometry::has_literal_rows(children),
        Block::Paragraph { children, .. } => mant_ir::has_printable_character(children),
        _ => false,
    })
}
