//! Paragraph and literal flow own pending text, provenance and flush boundaries.
use super::{InlineBuilder, layout, targets};
use mant_ir::{Block, Inline};

mod assembly;
mod columns;
pub(super) use assembly::LiteralAssembly;
mod input;
mod literal;
mod output;
mod paragraph;
mod row;
#[cfg(test)]
mod tests;
use crate::mandoc::inline::OutputRowEnd;
use literal::LiteralFlow;
use paragraph::ParagraphFlow;

#[derive(Clone, Copy)]
struct ClosedOutputTail {
    output_end: usize,
    owner: Option<usize>,
    row_end: OutputRowEnd,
}

/// A declaration keeps rich inline semantics under native no-fill execution.
/// This chooses only the output container, never formatter registers or rows.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::mandoc::blocks) enum InlineOutputRole {
    SourceMode,
    Declaration,
}

pub(super) struct BlockState {
    pub(super) output: Vec<Block>,
    pub(super) formatter: crate::mandoc::formatter::FormatterState,
    pub(super) literal_assembly: LiteralAssembly,
    // Filled and literal buffers are independent, not mutually exclusive modes.
    paragraph: ParagraphFlow,
    literal: LiteralFlow,
    inline_output_role: InlineOutputRole,
    // A column display transfers the active projection vector to LiteralFlow.
    // Its source and native posts keep borrowing that destination, including
    // an empty vector or a device row which has just ended. Neither fact is
    // permission to run the same native buffer against a different Vec.
    column_display_owner: bool,
    // This field's pre closed the device row of the preceding column.
    // Return that boundary to its owner instead of fabricating a gap.
    closes_previous_column: bool,
    /// D1/Dl present one literal owner without changing source fill mode.
    pub(in crate::mandoc::blocks) single_line_display_owner: bool,
    // Only the latest accepted output tail can hand a closed graph boundary
    // to its cell. Transparent structures relocate this owner position;
    // later accepted graph reopens the tail.
    closed_output_tail: Option<ClosedOutputTail>,
    pending_targets: targets::PendingTargets,
    // A paragraph pre request can finish its BODY with a live text row. Its
    // leading distance belongs to the next emitted block, not a Rust return.
    pending_spacing: Option<(u16, Option<mant_ir::SourceSpan>)>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    // Source scopes can move offset while continuing this same paragraph.
    // Its IR owner keeps the original base; nested structural handlers use
    // the current source origin instead (mdoc_term.c::termp_nm_pre(BODY)).
    source_origin: crate::mandoc::layout::SourceIndent,
    hanging_origin: Option<crate::mandoc::layout::SourceIndent>,
}

#[derive(Clone, Copy)]
pub(super) struct LinkOutputCursor {
    output: usize,
    owner: crate::mandoc::inline::ownership::annotations::AnnotationKey,
}

impl BlockState {
    pub(super) fn link_output_cursor(
        &mut self,
        owner: crate::mandoc::inline::ownership::annotations::AnnotationKey,
    ) -> LinkOutputCursor {
        let marker = super::man_links::link_cursor_marker(owner);
        self.formatter
            .execution
            .pending_output_scope_prefixes
            .push(format!("{marker}:content"));
        if self.inline_output_is_literal(self.formatter.no_fill) {
            self.literal.insert_link_cursor(marker);
        } else {
            self.paragraph.insert_link_cursor(marker);
        }
        LinkOutputCursor {
            output: self.output.len(),
            owner,
        }
    }

    pub(super) fn close_annotation_root(&mut self, no_fill: bool) {
        let markers = self
            .formatter
            .execution
            .zero_advance
            .close_annotation_root();
        for marker in markers {
            if self.inline_output_is_literal(no_fill) {
                self.literal.insert_link_cursor(marker);
            } else {
                self.paragraph.insert_link_cursor(marker);
            }
        }
    }

    pub(super) fn wrap_first_link_since(
        &mut self,
        cursor: LinkOutputCursor,
        target: &mant_ir::LinkTarget,
        membership: crate::mandoc::inline::ownership::OutputMembership,
    ) -> bool {
        let marker = super::man_links::link_cursor_marker(cursor.owner);
        let mut range = super::man_links::LabelRange::default();
        let mut wrapped = false;
        for block in &mut self.output[cursor.output..] {
            wrapped |= super::man_links::wrap_first_visible_block(
                block, target, &marker, &mut range, membership,
            );
        }
        let literal = self.inline_output_is_literal(self.formatter.no_fill);
        wrapped |= self.paragraph.wrap_first_link(
            target,
            &marker,
            &mut range,
            membership,
            (!literal).then_some(&mut self.formatter.execution),
        );
        wrapped |= self.literal.wrap_first_link(
            target,
            &marker,
            &mut range,
            membership,
            literal.then_some(&mut self.formatter.execution),
        );
        wrapped
    }

    pub(super) fn finish_output_membership(
        &mut self,
        cursor: LinkOutputCursor,
        membership: crate::mandoc::inline::ownership::OutputMembership,
    ) {
        let marker = super::man_links::link_cursor_marker(cursor.owner);
        let mut started = false;
        for block in &mut self.output[cursor.output..] {
            super::man_links::finish_output_membership(block, &marker, &mut started, membership);
        }
        let literal = self.inline_output_is_literal(self.formatter.no_fill);
        self.paragraph.finish_output_membership(
            &marker,
            &mut started,
            membership,
            (!literal).then_some(&mut self.formatter.execution),
        );
        self.literal.finish_output_membership(
            &marker,
            &mut started,
            membership,
            literal.then_some(&mut self.formatter.execution),
        );
    }

    pub(super) fn discard_link_cursor(&mut self, cursor: LinkOutputCursor, entry: Option<&str>) {
        let marker = super::man_links::link_cursor_marker(cursor.owner);
        let mut scoped_output = self.output.split_off(cursor.output);
        let mut index = cursor.output;
        scoped_output.retain_mut(|block| {
            if super::man_links::remove_link_cursor_block(block, &marker, entry) {
                self.literal_assembly.remove(index);
                false
            } else {
                index += 1;
                true
            }
        });
        self.output.extend(scoped_output);
        let literal = self.inline_output_is_literal(self.formatter.no_fill);
        self.paragraph.discard_link_cursor(
            &marker,
            entry,
            (!literal).then_some(&mut self.formatter.execution),
        );
        self.literal.discard_link_cursor(
            &marker,
            entry,
            literal.then_some(&mut self.formatter.execution),
        );
    }

    pub(super) const fn source_indent(&self) -> crate::mandoc::layout::SourceIndent {
        self.source_origin
    }

    pub(super) fn set_inline_source_origin(&mut self, origin: crate::mandoc::layout::SourceIndent) {
        self.source_origin = origin;
    }

    pub(super) fn set_source_indent(&mut self, indent: crate::mandoc::layout::SourceIndent) {
        self.flush_preformatted();
        self.flush_paragraph();
        self.indent_columns = indent;
        self.source_origin = indent;
        self.formatter.row_presentation.parent =
            i32::try_from(indent.physical_basic_units()).unwrap_or(i32::MAX);
        self.formatter.row_presentation.offset_shift = 0;
        self.hanging_origin = None;
    }

    pub(super) fn start_hanging(&mut self, origin: crate::mandoc::layout::SourceIndent) {
        self.hanging_origin = Some(origin);
    }

    pub(super) fn consume_hanging_first_line(&mut self) {
        if let Some(origin) = self.hanging_origin.take() {
            self.indent_columns = origin;
            self.source_origin = origin;
            let units = origin.physical_basic_units();
            if !self
                .formatter
                .execution
                .set_man_hanging_reading_origin(units, self.paragraph.retains_hanging_first_line())
            {
                self.formatter.row_presentation.parent = i32::try_from(units).unwrap_or(i32::MAX);
                self.formatter.row_presentation.offset_shift = 0;
            }
        }
    }

    pub(super) fn with_output(
        indent_columns: crate::mandoc::layout::SourceIndent,
        spacing_enabled: bool,
        output: Vec<Block>,
        mut formatter: crate::mandoc::formatter::FormatterState,
    ) -> Self {
        formatter.set_spacing_enabled(spacing_enabled);
        formatter
            .execution
            .set_presentation_parent(indent_columns.physical_basic_units());
        formatter
            .execution
            .inherit_output_owner_origin(indent_columns.physical_basic_units());
        Self {
            output,
            formatter,
            literal_assembly: LiteralAssembly::default(),
            paragraph: ParagraphFlow::new(),
            literal: LiteralFlow::new(),
            inline_output_role: InlineOutputRole::SourceMode,
            column_display_owner: false,
            closes_previous_column: false,
            single_line_display_owner: false,
            closed_output_tail: None,
            pending_targets: targets::PendingTargets::new(),
            pending_spacing: None,
            indent_columns,
            source_origin: indent_columns,
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

    pub(super) fn inherit_run_in_execution(&mut self, run_in: super::RunInBody<'_>) {
        self.paragraph
            .inherit_preserved_execution(&mut self.formatter, run_in.execution);
        let mut began_source_line = false;
        if let Some(entry) = run_in.entry {
            // print_mdoc_node() enters the actual It BODY before its pre
            // emits the run-in separator. Explicitly closed extended heads
            // give this BODY NODE_LINE; ordinary Bq does not. Observe these
            // facts now, never infer them from a later child or topology.
            if entry.flags.line_start {
                self.formatter.note_definition_source_line();
            }
            began_source_line =
                self.paragraph
                    .with_inline_builder(&mut self.formatter, |builder| {
                        builder.observe_no_fill_source_lines(true);
                        let began_source_line =
                            builder.begin_executed_node_observing_source_row(entry);
                        builder.observe_no_fill_source_lines(false);
                        began_source_line
                    });
            if began_source_line {
                // NODE_LINE ran before BODY's generated word. Keep its
                // HEAD close distinct from that word's later row. A term
                // with no projected glyph needs the completed-row receipt
                // delivered now, before the generated row can reuse the
                // same paragraph owner's trailing delimiter.
                let represented = self.formatter.definition_head_row_pending();
                let completed_invisible_head = !represented
                    && !crate::mandoc::inline::trailing_completed_row_origins(
                        self.paragraph.output_nodes_mut(),
                    )
                    .is_empty();
                self.formatter
                    .note_definition_head_source_close_with_unrepresented_row(
                        completed_invisible_head,
                    );
                if represented
                    && self
                        .paragraph
                        .consume_invisible_head_row(&mut self.formatter)
                {
                    self.formatter.consume_definition_head_row();
                }
                if completed_invisible_head {
                    self.publish_closed_run_in_head();
                }
                self.formatter.settle_definition_head_rows();
            }
        }
        // Inset/diag BODY pre writes its separator only after assigning the
        // source offset; restoring the HEAD's live input is not that pre.
        self.paragraph
            .with_inline_builder(&mut self.formatter, |builder| {
                builder
                    .execution
                    .enter_definition_body_origin(self.indent_columns.physical_basic_units());
            });
        // Native BODY pre writes its generated word even when the
        // reading projection omits trailing padding of a shared HEAD.
        // A real source-line entry instead gives that word its own row.
        let projected_cells = if run_in.suppress_padding_on_shared_head && !began_source_line {
            0
        } else {
            run_in.generated_cells
        };
        let boundary = self.paragraph.append_run_in_cells(
            &mut self.formatter,
            projected_cells,
            run_in.native_generated_cells,
            run_in.generated_word,
        );
        self.formatter.note_definition_run_in_executed(boundary);
    }
}

pub(super) fn has_run_in_visible_ink(nodes: &[Inline], run_in_id: Option<u64>) -> bool {
    fn scan(nodes: &[Inline], run_in_id: Option<u64>, owned_by_run_in: &mut bool) -> bool {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. } => {
                    if let Some(rest) = id.as_str().strip_prefix("\0mant:field-word:") {
                        *owned_by_run_in = rest.contains(":relocated:")
                            || run_in_id.is_some() && rest.parse::<u64>().ok() == run_in_id;
                    }
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    if *owned_by_run_in && value.chars().any(|ch| !ch.is_whitespace()) {
                        return true;
                    }
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if scan(children, run_in_id, owned_by_run_in) {
                        return true;
                    }
                }
                Inline::LineBreak { .. } => {}
            }
        }
        false
    }
    scan(nodes, run_in_id, &mut false)
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
