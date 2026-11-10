//! Drain output owners and settle their current row, target and spacing receipts.
use super::paragraph::ParagraphDrain;
use super::row::RowOwner;
use super::{
    BlockState, ClosedOutputTail, InlineBuilder, OutputRowEnd, has_formatter_text_cell, layout,
};
use mant_ir::{Block, Inline};

impl BlockState {
    pub(in crate::mandoc::blocks) fn queue_targets(
        &mut self,
        targets: impl IntoIterator<Item = String>,
        owner_source: Option<mant_ir::SourceSpan>,
    ) {
        self.pending_targets.queue(targets, owner_source);
    }

    pub(in crate::mandoc::blocks) fn attach_pending_to_new_output(&mut self, output_start: usize) {
        if self.output.len() == output_start {
            return;
        }
        self.attach_pending_to_structural_output(output_start);
    }

    pub(in crate::mandoc::blocks) fn attach_pending_to_structural_output(
        &mut self,
        output_start: usize,
    ) {
        if let Some(block) = self.output.get_mut(output_start)
            && let Some((lines, _)) = self.pending_spacing.take()
        {
            crate::mandoc::layout::set_block_spacing(block, lines);
        }
        if self.pending_targets.is_empty() {
            return;
        }
        let previous_end = self.output.len();
        let mut lowered = self.output.split_off(output_start.min(self.output.len()));
        self.pending_targets
            .attach_leading(&mut lowered, layout(self.indent_columns));
        self.output.append(&mut lowered);
        self.literal_assembly
            .insert(output_start, self.output.len().saturating_sub(previous_end));
        if let Some(tail) = &mut self.closed_output_tail
            && tail.output_end == previous_end
        {
            // Target fallback may insert a zero-width paragraph. It writes
            // no native cell and cannot reopen the accepted physical tail.
            if tail.owner >= output_start {
                tail.owner += self.output.len().saturating_sub(previous_end);
            }
            tail.output_end = self.output.len();
        }
    }

    pub(in crate::mandoc::blocks) fn finish_structural_output(
        &mut self,
        previous_end: usize,
        accepted_before: (u64, bool),
    ) {
        if let Some(tail) = self.closed_output_tail
            && tail.output_end == previous_end
        {
            // Bl pre/post only call term_newln (mdoc_term.c:1128-1154).
            // An empty wrapper after a real endline changes IR topology,
            // but only another accepted graph can reopen that device row.
            // Use the execution receipt, never a block visibility guess.
            self.closed_output_tail = (!self
                .formatter
                .execution
                .has_visible_content_since(accepted_before))
            .then_some(ClosedOutputTail {
                output_end: self.output.len(),
                ..tail
            });
        }
    }

    pub(in crate::mandoc::blocks) fn request_leading_spacing(
        &mut self,
        lines: u16,
        source: Option<mant_ir::SourceSpan>,
    ) {
        if lines == 0 {
            return;
        }
        self.pending_spacing = Some(
            self.pending_spacing
                .take()
                .map_or((lines, source), |(previous, origin)| {
                    (previous.saturating_add(lines), origin.or(source))
                }),
        );
    }

    pub(in crate::mandoc::blocks) fn request_man_paragraph_spacing(
        &mut self,
        distance: u16,
        has_predecessor: bool,
        source: Option<mant_ir::SourceSpan>,
    ) {
        let lines = crate::mandoc::layout::execute_man_paragraph_spacing(
            &mut self.formatter,
            distance,
            has_predecessor,
        );
        self.request_leading_spacing(lines, source);
    }

    /// An empty paragraph BODY still executed native `print_bvspace()`.
    pub(in crate::mandoc::blocks) fn materialize_idle_spacing(&mut self) {
        if !self.paragraph.is_empty()
            || !self.literal.is_empty()
            || self.formatter.execution.has_formatter_cell()
            || self
                .formatter
                .no_fill_inline
                .has_pending_formatter_cell(&self.formatter.execution)
        {
            return;
        }
        if let Some((lines, source)) = self.pending_spacing.take() {
            self.output.push(Block::VerticalSpace { lines, source });
        }
    }

    pub(in crate::mandoc::blocks) fn flush_paragraph(&mut self) {
        self.flush_paragraph_with(ParagraphDrain::Ordinary);
    }

    pub(in crate::mandoc::blocks) fn flush_paragraph_for_line_request(&mut self) {
        self.flush_paragraph_with(ParagraphDrain::LineRequest);
    }

    fn flush_paragraph_with(&mut self, drain: ParagraphDrain) {
        let line_request = drain == ParagraphDrain::LineRequest;
        if self.uses_literal_output() && self.paragraph.is_empty() {
            // The live column buffer belongs to LiteralFlow. Draining an
            // empty paragraph must not retire its native cells or claim its
            // completed-row receipts at a different IR destination.
            return;
        }
        if self.inline_output_is_literal(self.formatter.no_fill)
            && self.paragraph.is_empty()
            && !line_request
        {
            // A block output boundary has no filled content to drain. The
            // current no-fill formatter row remains live until term_newln().
            return;
        }
        let output_start = self.output.len();
        let (block, empty_word_end_break, completed_rows, closed_tail) = self.paragraph.take(
            &mut self.formatter,
            self.indent_columns,
            self.source_origin.offset_from(self.indent_columns),
            self.hanging_origin,
            drain,
        );
        let mut suppressed_head_row = false;
        if let Some(block) = block {
            match block {
                Block::Paragraph {
                    children,
                    layout,
                    source,
                    ..
                } if !crate::mandoc::inline::has_rendered_formatter_glyph(&children)
                    && has_formatter_text_cell(&children) =>
                {
                    // An empty or whitespace-only formatter word owns one
                    // physical row even when term_fill() prints no glyph.
                    // This includes a styled space made by `.B "" ""` and an
                    // explicit zero-width cell such as `\&`.
                    // Retain zero-width targets at that row during the
                    // representation change.
                    let completed_rows = children
                        .iter()
                        .filter(|inline| matches!(inline, Inline::LineBreak { .. }))
                        .count();
                    let active_row = children
                        .iter()
                        .rev()
                        .take_while(|inline| !matches!(inline, Inline::LineBreak { .. }))
                        .any(|inline| !matches!(inline, Inline::Anchor { .. }));
                    let rows = u16::try_from(completed_rows + usize::from(active_row))
                        .unwrap_or(u16::MAX)
                        .max(1);
                    let mut identities = children;
                    crate::mandoc::inline::retain_inline_identities(&mut identities);
                    if !identities.is_empty() {
                        self.output.push(Block::Paragraph {
                            inline_layout: mant_ir::InlineLayout::default(),
                            children: identities,
                            layout,
                            source,
                        });
                    }
                    suppressed_head_row = self.formatter.consume_definition_head_row();
                    let body_rows = rows.saturating_sub(u16::from(suppressed_head_row));
                    if body_rows > 0 {
                        self.output.push(Block::VerticalSpace {
                            lines: body_rows,
                            source,
                        });
                    }
                }
                block => self.output.push(block),
            }
        }
        if !completed_rows.is_empty() {
            // A detached HEAD may already represent the device row that
            // this flush ended (termp_it_post followed by term_flushln).
            // Claim its row once regardless of whether the receipt came
            // from an invisible word or a rejected plain-buffer suffix.
            let head_row = !suppressed_head_row && self.formatter.consume_definition_head_row();
            self.append_completed_paragraph_rows(
                &completed_rows[usize::from(head_row)..],
                output_start,
            );
        }
        if empty_word_end_break && !suppressed_head_row {
            // A bare \p can occupy the native tag row without leaving an
            // inline paragraph. Consume that row at this actual flush, just
            // as for an explicit empty formatter cell above.
            if !self.formatter.consume_definition_head_row() {
                self.output.push(Block::VerticalSpace {
                    lines: 1,
                    source: None,
                });
            }
        }
        if line_request {
            // roff_term_pre_sp() calls term_vspace() (and thus term_newln())
            // before adding its own empty row. Settle the native tag row now;
            // subsequent control-only BODY words own independent rows.
            self.formatter.settle_definition_head_rows();
        }
        if self.output.len() > output_start {
            self.closed_output_tail = closed_tail.is_closed().then_some(ClosedOutputTail {
                output_end: self.output.len(),
                owner: output_start,
                row_end: closed_tail,
            });
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
    }

    fn append_completed_paragraph_rows(
        &mut self,
        rows: &[crate::mandoc::inline::CompletedRowOrigin],
        output_start: usize,
    ) {
        use crate::mandoc::inline::CompletedRowOrigin;
        for run in rows.chunk_by(|left, right| left == right) {
            if run[0] == CompletedRowOrigin::Layout {
                self.output.push(Block::VerticalSpace {
                    lines: u16::try_from(run.len()).unwrap_or(u16::MAX),
                    source: None,
                });
                continue;
            }
            // A completed alignment row is part of the current hard-line
            // flow. A separate spacing request must not absorb it.
            if self.output.len() > output_start
                && let Some(Block::Paragraph { children, .. }) = self.output.last_mut()
            {
                let close_visible = crate::mandoc::inline::has_rendered_formatter_glyph(children)
                    && !crate::mandoc::inline::ends_with_executed_line_break(children);
                children.extend(std::iter::repeat_n(
                    Inline::line_break(),
                    run.len() + usize::from(close_visible),
                ));
            } else {
                self.output.push(Block::Paragraph {
                    inline_layout: mant_ir::InlineLayout::default(),
                    children: vec![Inline::line_break(); run.len()],
                    layout: mant_ir::LayoutHint::default(),
                    source: None,
                });
            }
        }
    }

    pub(in crate::mandoc::blocks) fn flush_preformatted(&mut self) {
        self.flush_preformatted_with_retirement(true);
    }

    fn flush_preformatted_with_retirement(&mut self, retire: bool) {
        let output_start = self.output.len();
        let closed_tail = self.literal.closed_graph_tail();
        let retires_active_owner =
            self.literal.has_output() && self.inline_output_is_literal(self.formatter.no_fill);
        self.output.extend(self.literal.take(self.indent_columns));
        if retire && retires_active_owner {
            // D1/Dl's post already consumed its native row (mdoc_term.c:
            // termp_bl_post). This Vec's indices cannot address a later
            // paragraph. Retire only projection ownership, not the native
            // offset, input cells, NOSPACE or the enclosing Nm BODY scope.
            self.formatter.execution.retire_output_owner();
        }
        if self.output.len() > output_start {
            self.closed_output_tail = closed_tail.is_closed().then_some(ClosedOutputTail {
                output_end: self.output.len(),
                // Completed layout rows can add separate gap/empty-literal
                // fragments. The tail proof belongs to the final projected
                // owner, before source-proven assembly changes their topology.
                owner: self.output.len() - 1,
                row_end: closed_tail,
            });
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
        self.literal_assembly.after_drain(self.output.len());
    }

    /// Unlike an ordinary break, native `term_flushln` emits a row even
    /// after an embedded spacing/font request emptied its pending content.
    pub(in crate::mandoc::blocks) fn flush_requested_line(
        &mut self,
        _source: Option<mant_ir::SourceSpan>,
    ) {
        // ce/rj explicitly select NOSPACE before their unconditional
        // term_flushln() (roff_term.c:103). The flush itself does not select
        // it; direct Fn/Nm field flushes must retain their different flag.
        self.formatter.execution.select_native_no_space();
        // D1/Dl's literal presentation does not make rejected filled input
        // an authored literal row. Native source mode determines row kind.
        let flush: fn(&mut InlineBuilder) = if self.formatter.no_fill {
            InlineBuilder::flush_literal_execution_line
        } else {
            InlineBuilder::flush_execution_line
        };
        self.with_row_boundary(self.row_owner(self.formatter.no_fill), flush);
        self.finish_aligned_row_output();
    }

    pub(in crate::mandoc::blocks) fn finish_aligned_row_output(&mut self) {
        self.flush_preformatted();
        if self.inline_output_is_literal(self.formatter.no_fill) {
            self.flush_paragraph();
        } else if self.hanging_origin.is_some() {
            // roff_term_pre_ce() calls pre_br() before the aligned rows.
            // In a filled HP, BRIND moves the source offset to the hanging
            // margin without executing another print_bvspace(). Keep the
            // completed first row and its layout in the same paragraph.
            self.paragraph
                .remember_hanging_first_line(self.indent_columns);
        }
        self.consume_hanging_first_line();
    }

    #[cfg(test)]
    pub(in crate::mandoc::blocks) fn finish(mut self) -> Vec<Block> {
        self.settle(super::super::FormatterRowBoundary::Settle, None);
        self.output
    }

    /// Execute all pending formatter boundaries before exporting persistent
    /// state to an enclosing structural driver.
    pub(in crate::mandoc::blocks) fn finish_with_formatter(
        mut self,
        formatter: &mut crate::mandoc::formatter::FormatterState,
        row_boundary: super::super::FormatterRowBoundary,
        next_column_entry: Option<&libmandoc_rs::Node>,
    ) -> (Vec<Block>, bool, super::LiteralAssembly) {
        let break_after = self.settle(row_boundary, next_column_entry);
        *formatter = self.formatter;
        (self.output, break_after, self.literal_assembly)
    }

    pub(super) fn settle(
        &mut self,
        row_boundary: super::super::FormatterRowBoundary,
        next_column_entry: Option<&libmandoc_rs::Node>,
    ) -> bool {
        let mut column_closed_row = false;
        if matches!(
            row_boundary,
            super::super::FormatterRowBoundary::Column { .. }
        ) {
            let finish_column = |builder: &mut InlineBuilder| {
                let mut closed = builder.finish_nested_column_part();
                if let Some(node) = next_column_entry {
                    // print_mdoc_node() enters the next actual BODY after
                    // the prior It post cleared NOBREAK, and before the
                    // next It pre establishes its field (mdoc_term.c:314-
                    // 321, 930-964). Consume that event in the prior row's
                    // owner; moving it into an empty next cell would lose
                    // its graph-row end or invent an empty row.
                    let row_was_open = builder.execution.has_open_native_device_row();
                    builder.observe_no_fill_source_lines(true);
                    builder.begin_executed_node(node);
                    closed |= row_was_open && !builder.execution.has_open_native_device_row();
                }
                builder.observe_no_fill_source_lines(false);
                closed
            };
            // Bd can leave the column's device row open in its literal
            // destination. Its subsequent words and the actual It post
            // consume that same owner (mdoc_term.c:1482 with 953), so a
            // rejected receipt must not target an empty paragraph Vec.
            column_closed_row = self.with_row_boundary(
                RowOwner::from_literal(self.uses_literal_output()),
                finish_column,
            );
            if let Some(node) = next_column_entry {
                self.formatter.no_fill = node.flags.no_fill;
            }
        } else if row_boundary == super::super::FormatterRowBoundary::MdocItemBody {
            // This is the actual It BODY post, before its node geometry
            // returns. Generic output settlement can follow a display post
            // which already consumed the row; it is not this native event.
            let origin = if self.inline_output_is_literal(self.formatter.no_fill) {
                crate::mandoc::inline::CompletedRowOrigin::LiteralText
            } else {
                crate::mandoc::inline::CompletedRowOrigin::Layout
            };
            self.with_active_output_builder(|builder| builder.finish_mdoc_item_body(origin));
            self.formatter.retire_no_fill_row_projection();
        } else if row_boundary == super::super::FormatterRowBoundary::Settle {
            // The caller identified a native BODY post, not an IR owner
            // return. mdoc_term.c::termp_it_post() executes term_newln()
            // for inset/diag BODY here before retiring the field flags.
            self.paragraph
                .with_inline_builder(&mut self.formatter, InlineBuilder::finish_run_in_field_row);
        }
        let handoff = row_boundary == super::super::FormatterRowBoundary::SectionHeadHandoff;
        if handoff && self.inline_output_is_literal(self.formatter.no_fill) {
            self.literal
                .handoff_pending_native_input(&mut self.formatter);
        }
        let consumed = row_boundary == super::super::FormatterRowBoundary::SectionHeadConsumed;
        self.flush_preformatted_with_retirement(!consumed);
        self.flush_paragraph_with(if handoff {
            ParagraphDrain::BeforeNativeFlush
        } else if consumed {
            ParagraphDrain::AfterNativeFlush
        } else {
            ParagraphDrain::Ordinary
        });
        let closed_tail = self
            .closed_output_tail
            .filter(|tail| tail.output_end == self.output.len());
        let break_after = (column_closed_row || closed_tail.is_some())
            && matches!(
                row_boundary,
                super::super::FormatterRowBoundary::Column { last: false, .. }
            );
        if matches!(
            row_boundary,
            super::super::FormatterRowBoundary::Column { .. }
        ) && let Some(tail) = closed_tail
            && tail.row_end == OutputRowEnd::GeneratedClose
            && let Some(
                Block::Paragraph {
                    children,
                    inline_layout,
                    ..
                }
                | Block::Preformatted {
                    children,
                    inline_layout,
                    ..
                },
            ) = self.output.get_mut(tail.owner)
        {
            // Every column BODY post retires its proved generated delimiter,
            // including the last field: the whole table row owns that close.
            // Only a non-last cell exports breakAfter for the next field.
            // LiteralClose preserves the author's LB and empty TEXT,
            // including after transparent structural output.
            if crate::mandoc::inline::consume_one_row_ending(children) {
                let rows = mant_ir::logical_row_count(children);
                inline_layout
                    .row_hints
                    .retain(|hint| usize::try_from(hint.row).is_ok_and(|row| row < rows));
            }
        }
        if let Some((lines, source)) = self.pending_spacing.take() {
            self.output.push(Block::VerticalSpace { lines, source });
        }
        let output_end = self.output.len();
        self.attach_pending_to_structural_output(output_end);
        break_after
    }
}
