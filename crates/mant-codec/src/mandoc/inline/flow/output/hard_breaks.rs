//! Consume the active native buffer at real formatter line boundaries.

use super::super::LeadingLineBoundary;
use super::{
    CompletedRowOrigin, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, has_rendered_formatter_glyph, push_row_break,
    tab_projection,
};

impl InlineBuilder {
    /// Preserve a formatter-requested line boundary without creating empty
    /// leading, repeated, or trailing rows around the paragraph.
    pub(in crate::mandoc) fn hard_break(&mut self) {
        self.hard_break_with_origin(CompletedRowOrigin::Layout);
    }

    pub(in crate::mandoc::inline::flow) fn hard_break_with_origin(
        &mut self,
        origin: CompletedRowOrigin,
    ) {
        // term_newln() selects NOSPACE before even testing lastcol/viscol
        // (term.c:475-481). An empty word may have cleared it without
        // buffering a cell, so a no-output return still owns this update.
        self.tighten_next_boundary();
        if self.execution.definition.is_some()
            && self.execution.word_input().0.is_empty()
            && !self.execution.has_open_native_device_row()
        {
            // term_newln() tests lastcol/viscol before term_flushln(). An
            // empty speculative device sweep would consume a pending ti
            // even though no native endline ran (term.c:475-481).
            self.reset_native_tab_origin();
            self.join_projected_boundary();
            return;
        }
        self.project_hard_break_with_origin(origin);
        self.tighten_next_boundary();
    }

    /// A `term_fill` pass may end the projected row inside a word. That
    /// endline does not execute `term_newln` or alter the word-end NOSPACE.
    pub(in crate::mandoc::inline::flow) fn project_hard_break_with_origin(
        &mut self,
        origin: CompletedRowOrigin,
    ) {
        if let Some(mut native) = self.native_field_device(false) {
            // term_newln consumes the actual old field before clearing its
            // cells. Preserve accepted row ownership even when that owner
            // is a HEAD already drained into a different IR container.
            self.project_captured_definition_field(&native);
            for edits in self.project_consumed_row_layout(&native) {
                native.remap_output_projection(&edits);
            }
            self.hard_break_using_field(Some(&native), true, origin);
            // This standalone term_newln() has no later field-retirement
            // caller. Its row reset already consumed the live buffer, so
            // deliver the captured text receipt without re-running the tail
            // or changing the existing row-origin/padding projection.
            let tabs = self.capture_consumed_native_tabs(&native);
            tab_projection::apply_from(&mut self.nodes, native.current_output_start(), tabs);
            if let Some(receiver) = self
                .execution
                .definition
                .as_ref()
                .and_then(|state| state.deferred_head_projection.as_ref())
            {
                receiver.accept_tabs(tabs);
            }
        } else {
            self.hard_break_using_field(None, false, origin);
        }
        // The separator that caused this projected row end belongs to the
        // consumed row, not to the new IR line. This is a projection join;
        // the global outgoing word flag still belongs to term_word().
        self.join_projected_boundary();
    }

    /// The field's real flush already applied acceptance. Project its tail
    /// without re-querying the old buffer or rejecting it a second time.
    pub(in crate::mandoc::inline::flow) fn hard_break_after_field_projection_with_origin(
        &mut self,
        native: Option<&super::super::definition::NativeFieldDevice>,
        origin: CompletedRowOrigin,
    ) {
        self.hard_break_using_field(native, true, origin);
    }

    fn hard_break_using_field(
        &mut self,
        captured: Option<&super::super::definition::NativeFieldDevice>,
        projected: bool,
        origin: CompletedRowOrigin,
    ) {
        let had_native_buffer = self.definition.as_ref().is_some_and(|state| {
            state.field_buffer.resume_offset() < state.field_buffer.cells().len()
        }) || (self.execution.definition.is_none()
            && self.execution.plain_input.buffer.resume_offset()
                < self.execution.plain_input.buffer.cells().len());
        let unprinted_field_tail = had_native_buffer
            && if projected {
                captured
                    .is_some_and(super::super::definition::NativeFieldDevice::closes_unprinted_row)
            } else {
                self.native_field_closes_unprinted_row()
            };
        let exited_discarded_buffer = self.discarded_exited_definition_buffer();
        let exited_definition_row = self
            .execution
            .detached_device_row
            .is_some_and(|row| row.page_origin_printed || row.viscol > 0)
            || self
                .execution
                .definition
                .as_ref()
                .is_some_and(|definition| {
                    definition.hang_row.native_row_occupied()
                        && self
                            .execution
                            .author_execution
                            .as_ref()
                            .is_some_and(|execution| {
                                matches!(
                                    execution.break_effect,
                                    super::super::AuthorBreakEffect::Line
                                )
                            })
                });
        // term_newln() flushes the plain flush unit through the same
        // term_flushln(): a definitive rejection ends its own row here.
        let plain_flush = self.consume_hard_break_buffer(projected);
        let plain_flush_rejection = plain_flush.rejected();
        self.observe_hard_break_boundary(plain_flush);
        // term_newln() flushes only an occupied terminal cell. A completed
        // `\zX` glyph and a buffered `\p` both advanced the native buffer;
        // a bare armed `\z` did not and remains ordered before the next word.
        // A definitively rejected flush unit still occupied the native
        // buffer: term_newln() flushed it (lastcol > 0), term_flushln()
        // reset the buffer (term.c:233-237), and the nbr == 0 pass ended
        // its own row (term.c:250-253). The rejection must retire here.
        let retiring_rejection = self.execution.wipe_remainder || plain_flush_rejection;
        if !self.has_formatter_cell()
            && !had_native_buffer
            && !exited_definition_row
            && !exited_discarded_buffer
            && !retiring_rejection
        {
            if self.external_head_row_pending {
                // term_newln() still flushes the detached native tag row.
                // term_flushln() clears a bare \\z before the next BODY word.
                self.execution.zero_advance.discard_at_row_end();
                self.external_head_row_pending = false;
            }
            self.reset_native_tab_origin();
            return;
        }
        // term.c:143-146 with 233-237: a definitively rejected unit dies
        // whole - including a `\z`-generated glyph still pending in the
        // zero-advance register. A bare armed `\z` wrote no cell and keeps
        // its request alive across the boundary (term.c:836-838), so it
        // still settles through the ordinary flush.
        self.flush_zero_advance();
        self.external_head_row_pending = false;
        let completed_unprinted_row = plain_flush_rejection
            || unprinted_field_tail
            || plain_flush.closes_unprinted_row()
            || !self.in_definition_field()
                && self.execution.completed_vertical_rows > 0
                && self.execution.formatter_column == FormatterColumn::Advanced
                && !self
                    .nodes
                    .iter()
                    .rev()
                    .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                    .any(|node| has_rendered_formatter_glyph(std::slice::from_ref(node)));
        let current_row_has_printable = self
            .nodes
            .iter()
            .rev()
            .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
            .any(|node| has_printable_character(std::slice::from_ref(node)));
        if (had_native_buffer
            || self.execution.formatter_column == FormatterColumn::Advanced
            || (self.execution.word_end_break == WordEndBreak::Pending
                && matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))))
            && !current_row_has_printable
        {
            // ESCAPE_IGNORE (`\&`) and ESCAPE_BREAK (`\p`) can occupy a
            // fresh native buffer after an earlier line was already closed.
            // Its own term_newln() must remain distinct from that break.
            self.nodes.push(Inline::Text {
                value: String::new(),
            });
        }
        if completed_unprinted_row {
            // term_field() can accept only deferred padding without printing
            // it. Its empty-word witness belongs before the same row's real
            // endline (term.c:389-427,250-253), so it cannot reopen that close.
            self.record_completed_rows(1, origin);
        }
        self.finish_hard_break_projection(
            exited_definition_row,
            exited_discarded_buffer,
            retiring_rejection,
        );
        // term_newln() does not clear TERMP_NONEWLINE; the next word does.
    }

    fn consume_hard_break_buffer(
        &mut self,
        projected: bool,
    ) -> super::super::plain_input::PlainFlushOutcome {
        use super::super::plain_input::PlainFlushOutcome;
        let rejected = if projected {
            self.execution
                .definition
                .as_ref()
                .is_some_and(|state| state.hang_row.field_discarded)
        } else if self.in_definition_field() {
            // term_newln() flushes the same tcol->buf after a TAG request
            // cleared NOBREAK. A definition session still owns that input;
            // consume its receipt before the row reset retires the cells.
            self.discard_unprinted_definition_field_output()
        } else {
            return self.retire_plain_flush_unit_with_tail();
        };
        if rejected {
            PlainFlushOutcome::Rejected
        } else {
            PlainFlushOutcome::Ordinary
        }
    }

    fn observe_hard_break_boundary(
        &mut self,
        plain_flush: super::super::plain_input::PlainFlushOutcome,
    ) {
        if (!self.execution.has_printable_content || plain_flush.closes_represented_row())
            && self.execution.leading_line_boundary == LeadingLineBoundary::None
        {
            // An ordinary blank can set the projection's printable flag,
            // while its real flush only closes the inherited HEAD row.
            // Publish that boundary before its vspace successor, so the
            // existing BODY observer retires HEAD rather than the new gap.
            self.execution.leading_line_boundary =
                LeadingLineBoundary::AtVisibleCheckpoint(self.execution.visible_glyph_epoch);
        }
    }

    fn finish_hard_break_projection(
        &mut self,
        exited_definition_row: bool,
        exited_discarded_buffer: bool,
        retiring_rejection: bool,
    ) {
        // This is a committed device-row end, including a detached HEAD
        // whose buffer already belongs to another IR owner. ascii_endline()
        // consumes ti here; an owner drain or a retained HANG field does not.
        let offset = self
            .execution
            .definition
            .as_ref()
            .map_or(0, |state| state.field_offset_units);
        self.execution.row_presentation.endline_at(offset);
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.empty_word = false;
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        if (exited_discarded_buffer && !exited_definition_row)
            || !matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
            || retiring_rejection
        {
            let row_indent = self.take_definition_row_indent();
            push_row_break(&mut self.nodes, row_indent);
            self.note_definition_output_row();
            self.execution.last_visible_character = Some('\n');
        }
        self.execution.final_word_join = Some(false);
        if let Some(definition) = &mut self.execution.definition {
            // A committed hard break closes the native device row even when
            // the HEAD is still in its TAG field. Otherwise an overrun tag
            // leaves stale viscol for a later discarded buffer.
            definition.hang_row.endline();
            definition.field_buffer.clear();
            definition.field_buffer.set_tab_offset(0);
            definition.field_word_anchors.clear();
        }
        if self.execution.definition.is_none() {
            self.execution.detached_device_row = None;
            // term_flushln() resets the plain unit with the same row
            // (term.c:233-237): the next word starts a fresh buffer whose
            // output interval begins after this committed row.
            self.execution.plain_input.retire_row_at(self.nodes.len());
        }
        if let Some(author) = &mut self.execution.author_execution {
            // term_flushln() commits its accepted prefix before it starts
            // another field. A later nbr=0 may discard only the new suffix.
            author.field_output_start = self.nodes.len();
        }
    }

    pub(in crate::mandoc) fn has_formatter_cell(&self) -> bool {
        self.execution.formatter_column == FormatterColumn::Advanced
            || self.execution.zero_advance.has_buffered_glyph()
            || self.execution.word_end_break == WordEndBreak::Pending
    }

    pub(in crate::mandoc) fn has_invisible_formatter_cell(&self) -> bool {
        self.execution.formatter_column == FormatterColumn::Advanced
            && !self
                .nodes
                .iter()
                .rev()
                .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
                .any(|node| has_printable_character(std::slice::from_ref(node)))
    }
}
