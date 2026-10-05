//! Drain output owners and settle their current row, target and spacing receipts.
use super::{
    BlockState, ClosedOutputTail, InlineBuilder, OutputRowEnd, has_flushed_row,
    has_formatter_text_cell, layout,
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
        self.flush_paragraph_with(false);
    }

    pub(in crate::mandoc::blocks) fn flush_paragraph_for_line_request(&mut self) {
        self.flush_paragraph_with(true);
    }

    pub(super) fn flush_paragraph_with(&mut self, line_request: bool) {
        if self.column_uses_literal_output() && self.paragraph.is_empty() {
            // The live column buffer belongs to LiteralFlow. Draining an
            // empty paragraph must not retire its native cells or claim its
            // completed-row receipts at a different IR destination.
            return;
        }
        if self.formatter.no_fill && self.paragraph.is_empty() && !line_request {
            // A block output boundary has no filled content to drain. The
            // current no-fill formatter row remains live until term_newln().
            return;
        }
        let output_start = self.output.len();
        let (block, empty_word_end_break, completed_vertical_rows, closed_tail) = self
            .paragraph
            .take(&mut self.formatter, self.indent_columns, line_request);
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
        if completed_vertical_rows > 0 {
            // A detached HEAD may already represent the device row that
            // this flush ended (termp_it_post followed by term_flushln).
            // Claim its row once regardless of whether the receipt came
            // from an invisible word or a rejected plain-buffer suffix.
            let head_row = !suppressed_head_row && self.formatter.consume_definition_head_row();
            let body_rows = completed_vertical_rows.saturating_sub(u16::from(head_row));
            if body_rows > 0 {
                self.output.push(Block::VerticalSpace {
                    lines: body_rows,
                    source: None,
                });
            }
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
            if let Some(origin) = self.hanging_origin
                && let Some(Block::Paragraph { layout, .. }) = self.output.last_mut()
            {
                layout.continuation_indent_columns = origin.offset_from(self.indent_columns);
            }
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
    }

    pub(in crate::mandoc::blocks) fn flush_preformatted(&mut self) {
        let output_start = self.output.len();
        let closed_tail = self.literal.closed_graph_tail();
        self.output.extend(self.literal.take(self.indent_columns));
        if self.output.len() > output_start {
            self.closed_output_tail = closed_tail.is_closed().then_some(ClosedOutputTail {
                output_end: self.output.len(),
                // Literal origins can split one accepted vector into
                // several blocks. Its tail proof belongs to the final
                // projected owner, including a trailing literal group.
                owner: self.output.len() - 1,
                row_end: closed_tail,
            });
            self.consume_hanging_first_line();
        }
        self.attach_pending_to_new_output(output_start);
    }

    /// Unlike an ordinary break, native `term_flushln` emits a row even
    /// after an embedded spacing/font request emptied its pending content.
    pub(in crate::mandoc::blocks) fn flush_requested_line(
        &mut self,
        source: Option<mant_ir::SourceSpan>,
    ) {
        let start = self.output.len();
        self.flush_preformatted();
        self.flush_paragraph();
        if !has_flushed_row(&self.output[start..]) {
            let start = self.output.len();
            self.output.push(Block::Preformatted {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![Inline::Text {
                    value: String::new(),
                }],
                language: None,
                layout: layout(self.indent_columns),
                source,
            });
            self.attach_pending_to_new_output(start);
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
    ) -> (Vec<Block>, bool) {
        let break_after = self.settle(row_boundary, next_column_entry);
        *formatter = self.formatter;
        (self.output, break_after)
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
            column_closed_row = if self.column_uses_literal_output() {
                self.literal
                    .with_inline_builder(&mut self.formatter, finish_column)
            } else {
                self.paragraph
                    .with_inline_builder(&mut self.formatter, finish_column)
            };
            self.formatter.no_fill_inline.retire_consumed_cell();
            if let Some(node) = next_column_entry {
                self.formatter.no_fill = node.flags.no_fill;
            }
        } else if row_boundary == super::super::FormatterRowBoundary::Settle {
            // The caller identified a native BODY post, not an IR owner
            // return. mdoc_term.c::termp_it_post() executes term_newln()
            // for inset/diag BODY here before retiring the field flags.
            self.paragraph
                .with_inline_builder(&mut self.formatter, InlineBuilder::finish_run_in_field_row);
        }
        self.flush_preformatted();
        self.flush_paragraph();
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
