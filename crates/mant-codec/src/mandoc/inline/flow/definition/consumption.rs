// Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Accepted and rejected definition-field receipts over one execution state.

#[cfg(test)]
use super::super::native_field::FieldFlags;
use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, TrailingOutput, WordEndBreak,
    last_visible_character,
};
use super::device::NativeFieldDevice;
use super::flush::retain_unprinted_field_targets;

mod retirement;

impl InlineBuilder {
    pub(in crate::mandoc::inline::flow) fn native_field_closes_unprinted_row(&self) -> bool {
        self.native_field_device(false)
            .is_some_and(|device| device.closes_unprinted_row())
    }

    pub(in crate::mandoc) fn discard_unprinted_definition_field_output(&mut self) -> bool {
        self.project_definition_field_receipt(false, false)
    }

    /// The `.mc` variant: `roff_term_pre_mc()` sets `TERMP_NOBREAK` around
    /// its `term_flushln()` (roff_term.c:147-150), so the field tail this
    /// projection returns must be decided with NOBREAK held on - the row
    /// only ends on overrun, not unconditionally.
    pub(in crate::mandoc) fn discard_unprinted_definition_field_output_no_break(&mut self) -> bool {
        self.project_definition_field_receipt(false, true)
    }

    pub(in crate::mandoc) fn project_definition_owner_prefix(&mut self) {
        self.project_definition_field_receipt(true, false);
    }

    /// Returns the ordinary field tail's native row end owed by this real
    /// flush. It runs even on first-pass rejection and is independent of
    /// any accepted-pass endline. Owner drains do not execute that tail.
    fn project_definition_field_receipt(
        &mut self,
        owner_boundary: bool,
        no_break_flush: bool,
    ) -> bool {
        self.project_definition_field_receipt_using(owner_boundary, no_break_flush, None)
    }

    /// Project the same old-state acceptance that the real request captured.
    pub(in crate::mandoc::inline::flow) fn project_captured_definition_field(
        &mut self,
        native: &NativeFieldDevice,
    ) -> bool {
        #[cfg(test)]
        super::control_trace::projected();
        // This is an actual flush, unlike owner-prefix or word-time views.
        // Its row close can clear the buffer before retirement projects text.
        self.capture_consumed_run_in_boundaries(native);
        let _ = self.capture_consumed_native_tabs(native);
        self.project_definition_field_receipt_using(false, false, Some(native))
    }

    pub(in crate::mandoc::inline::flow) fn capture_consumed_native_tabs<'a>(
        &self,
        device: &'a NativeFieldDevice,
    ) -> &'a [super::super::output::tab_projection::TabExpansion] {
        device.tab_projection.get_or_init(|| {
            self.execution
                .definition
                .as_ref()
                .map_or_else(Vec::new, |state| {
                    super::super::output::tab_projection::capture(
                        &state.field_buffer,
                        &state.field_word_anchors,
                        &device.tab_spaces,
                    )
                })
        })
    }

    fn project_definition_field_receipt_using(
        &mut self,
        owner_boundary: bool,
        no_break_flush: bool,
        native: Option<&NativeFieldDevice>,
    ) -> bool {
        use super::super::field_buffer::FlushReceipt;
        if self
            .execution
            .definition
            .as_ref()
            .is_none_or(|state| state.field_buffer.is_empty())
        {
            return false;
        }
        let uncaptured;
        let receipt = if let Some(native) = native {
            &native.receipt
        } else {
            let Some(targets) = self.native_field_targets(no_break_flush, None) else {
                return false;
            };
            uncaptured = self
                .execution
                .definition
                .as_ref()
                .expect("native field")
                .field_buffer
                .flush_receipt(targets, false);
            &uncaptured
        };
        if matches!(receipt, FlushReceipt::UnmodeledRewind) {
            let state = self.execution.definition.as_mut().expect("native field");
            let receipts = state.field_buffer.conservative_deferred_blanks();
            if let Some(receiver) = &state.deferred_head_projection {
                receiver.accept_blanks(&receipts);
            }
            let start = self
                .execution
                .author_execution
                .as_ref()
                .map_or(0, |author| author.field_output_start);
            super::super::output::deferred_blanks::project_from(&mut self.nodes, start, &receipts);
            state.hang_row.field_discarded = false;
            return false;
        }

        // term_flushln() only reaches loop endline (term.c:217) when an
        // accepted pass has a remaining field. Only authored marker ends
        // can supply the unprinted rows consumed here; device-width ends
        // do not. The actual device tail still runs with its original flags.
        let empty_pass_ends = if owner_boundary || !receipt_has_authored_loop_rows(receipt) {
            Vec::new()
        } else {
            self.empty_authored_loop_ends(native, no_break_flush)
        };

        if let FlushReceipt::Accepted { passes, .. } = receipt {
            if !owner_boundary {
                self.project_accepted_field_passes(passes);
                self.project_accepted_field_omissions(passes);
                let start = self
                    .execution
                    .author_execution
                    .as_ref()
                    .map_or(0, |author| author.field_output_start);
                self.project_deferred_field_blanks(passes, start, true);
            }
            self.definition_state_mut().hang_row.field_discarded = false;
            self.project_completed_empty_passes(&empty_pass_ends);
            return false;
        }
        if !owner_boundary && let FlushReceipt::Rejected { passes, .. } = receipt {
            // term_flushln() executes every accepted pass before nbr == 0
            // rejects its suffix. NBRZW can end a pass without a visible
            // scalar (term.c:340-349), so projecting only the final boundary
            // would merge two different endline events at the same scalar.
            self.project_accepted_field_passes(passes);
        }
        let FlushReceipt::Rejected {
            passes, definitive, ..
        } = receipt
        else {
            unreachable!("accepted field returned before rejection projection")
        };
        if owner_boundary && !*definitive {
            return false;
        }
        self.project_rejected_field_prefix(passes, !owner_boundary);
        self.project_completed_empty_passes(&empty_pass_ends);
        self.finish_rejected_field_state(
            owner_boundary,
            no_break_flush,
            native.map(|device| device.ends_row),
        )
    }

    fn project_rejected_field_prefix(
        &mut self,
        passes: &[super::super::field_buffer::FillPass],
        return_to_head: bool,
    ) {
        let definition = self.execution.definition.as_mut().expect("native field");
        definition.hang_row.field_discarded = true;
        let accepted_owners = super::super::output::native_passes::accepted_owner_lengths(
            &definition.field_buffer,
            &definition.field_word_anchors,
            passes,
            false,
        );
        // The detached HEAD keeps native owner anchors for deferred blanks
        // and layout. Its accepted final pass has its own structural close;
        // the rejecting BODY flush cannot project that endline a second
        // time. Actual accepted BODY cells still end their own pass.
        let accepted_owned_prefix = accepted_owners.iter().any(|(owner, acceptance)| {
            acceptance.native_cells
                && !definition
                    .deferred_head_projection
                    .as_ref()
                    .is_some_and(|head| head.owns_word(owner))
        });
        definition.hang_row.accepted_prefix_before_rejection = accepted_owned_prefix;
        let current_field_start = self
            .execution
            .author_execution
            .as_ref()
            .map_or(0, |author| author.field_output_start)
            .min(self.nodes.len());
        let mut pending_output = self.nodes.split_off(current_field_start);
        let owned = super::super::output::split::retain_native_field_owners(
            &mut pending_output,
            &accepted_owners,
        );
        if !owned {
            // A detached or hidden owner can have an explicitly empty
            // projection. It never grants acceptance to the new owner's
            // current field; its rejected interval still has an identity.
            retain_unprinted_field_targets(&mut pending_output);
        }
        self.nodes.extend(pending_output);
        self.project_deferred_field_blanks(passes, current_field_start, return_to_head);
        if accepted_owned_prefix
            && !super::super::output::ends_with_executed_line_break(&self.nodes)
        {
            // term_flushln() ended the last accepted pass before discovering
            // nbr=0. The semantic owner must expose that exact native event,
            // including when its cells came from an overstrike projection.
            self.nodes.push(Inline::line_break());
            self.note_definition_output_row();
        }
        if let Some(author) = &mut self.execution.author_execution {
            author.field_output_start = self.nodes.len();
        }
    }

    fn project_deferred_field_blanks(
        &mut self,
        passes: &[super::super::field_buffer::FillPass],
        start: usize,
        return_to_head: bool,
    ) {
        use super::super::output::deferred_blanks;
        let definition = self.execution.definition.as_ref().expect("native field");
        if return_to_head && let Some(receiver) = &definition.deferred_head_projection {
            let receipts = definition.field_buffer.printed_deferred_blanks(passes);
            receiver.accept_blanks(&receipts);
            deferred_blanks::project_from(&mut self.nodes, start, &receipts);
        } else {
            deferred_blanks::project(&mut self.nodes, &definition.field_buffer, passes, start);
        }
    }

    /// Project `term_field`'s unprinted padding and covered glyphs at the
    /// native flush, using the same source owner intervals as rejection.
    fn project_accepted_field_omissions(
        &mut self,
        passes: &[super::super::field_buffer::FillPass],
    ) {
        let definition = self.execution.definition.as_ref().expect("native field");
        let unprinted_padding = passes.iter().any(|pass| {
            definition
                .field_buffer
                .has_unprinted_padding(pass.start, pass.end)
        });
        if !unprinted_padding && !definition.field_buffer.has_zero_width_retreats() {
            return;
        }
        let accepted = super::super::output::native_passes::accepted_owner_lengths(
            &definition.field_buffer,
            &definition.field_word_anchors,
            passes,
            true,
        );
        let start = self
            .execution
            .author_execution
            .as_ref()
            .map_or(0, |author| author.field_output_start)
            .min(self.nodes.len());
        let mut output = self.nodes.split_off(start);
        super::super::output::split::retain_native_field_owners(&mut output, &accepted);
        self.nodes.extend(output);
    }

    /// Observe only authored loop endlines of this exact flush. Captured
    /// control receipts already own the numeric device sweep; legacy owner
    /// drains obtain a view only when such an empty loop actually exists.
    fn empty_authored_loop_ends(
        &self,
        native: Option<&NativeFieldDevice>,
        no_break_flush: bool,
    ) -> Vec<usize> {
        let uncaptured_device;
        let device = if let Some(native) = native {
            Some(native)
        } else {
            #[cfg(test)]
            EMPTY_LOOP_DEVICE_VIEWS.with(|views| views.set(views.get().saturating_add(1)));
            uncaptured_device = self.native_field_device(no_break_flush);
            uncaptured_device.as_ref()
        };
        device
            .filter(|device| device.printed_row.is_none())
            .map_or_else(Vec::new, |device| {
                device
                    .loop_rows
                    .iter()
                    .rev()
                    .take_while(|row| {
                        !row.printed
                            && row.boundary
                                == super::super::field_buffer::FillBoundary::WordEndBreak
                    })
                    .map(|row| row.end_cell)
                    .collect::<Vec<_>>()
            })
    }

    fn project_completed_empty_passes(&mut self, ends: &[usize]) {
        let mut rows = 0u16;
        for end in ends.iter().rev() {
            if self
                .definition_state_mut()
                .field_buffer
                .claim_completed_empty_pass(*end)
            {
                rows = rows.saturating_add(1);
            }
        }
        // term_fill accepts NBRZW as graph, term_field prints no scalar,
        // and the pass loop nevertheless calls endline (term.c:217).
        // This completed empty row is separate from the later rejected
        // pass's unconditional tail endline (250-253).
        self.record_completed_vertical_rows(rows);
    }

    fn project_accepted_field_passes(&mut self, passes: &[super::super::field_buffer::FillPass]) {
        let state = self.execution.definition.as_mut().expect("native field");
        let start = self
            .execution
            .author_execution
            .as_ref()
            .map_or(0, |author| author.field_output_start);
        super::super::output::native_passes::project_accepted_native_passes(
            &mut self.nodes,
            &mut state.field_buffer,
            &state.field_word_anchors,
            passes,
            start,
        );
    }

    /// Retire only the rejected native buffer's registers after its exact
    /// output interval was projected. The ordinary field tail is a separate
    /// device event and is returned to the actual flush caller.
    fn finish_rejected_field_state(
        &mut self,
        owner_boundary: bool,
        no_break_flush: bool,
        captured_row_end: Option<bool>,
    ) -> bool {
        let Some(exited_field) = self
            .execution
            .author_execution
            .as_ref()
            .filter(|_| self.execution.definition.is_some())
            .map(|execution| matches!(execution.break_effect, AuthorBreakEffect::Line))
        else {
            return false;
        };
        if !self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.field_discarded)
        {
            return false;
        }
        // term.c::term_flushln() clears both BACKAFTER and BACKBEFORE even
        // when term_fill() returns nbr=0. The rejected field can still own a
        // completed \z glyph that has not entered the IR suffix yet.
        self.execution.zero_advance.discard_at_row_end();
        if let Some(definition) = &mut self.execution.definition {
            definition.field_buffer.clear_backtracking();
        }
        self.execution.last_visible_character = last_visible_character(&self.nodes);
        if exited_field {
            // A TAG .br may have already ended NOBREAK, but subsequent words
            // still share one term_fill() input buffer until the next actual
            // line request. Dropping that buffer leaves no current cell.
            self.execution.formatter_column = FormatterColumn::Origin;
            self.execution.word_end_break = WordEndBreak::Clear;
            self.execution.wipe_remainder = false;
            self.execution.pending_word_spaces.clear();
            self.execution.trailing_output = TrailingOutput::None;
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
            }
        }
        // The same numeric tail rule handles accepted and rejected final
        // passes; an IR owner drain executes neither device endline.
        let flags_end_row = captured_row_end.unwrap_or_else(|| {
            self.native_field_device(no_break_flush)
                .is_some_and(|field| field.ends_row)
        });
        !owner_boundary && flags_end_row
    }

    pub(in crate::mandoc) fn discarded_exited_definition_buffer(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|definition| {
                definition.hang_row.field_discarded
                    && self
                        .execution
                        .author_execution
                        .as_ref()
                        .is_some_and(|execution| {
                            matches!(execution.break_effect, AuthorBreakEffect::Line)
                        })
            })
    }

    pub(in crate::mandoc) fn in_definition_field(&self) -> bool {
        self.execution.uses_definition_word_buffer()
    }

    /// A head field configured with `AuthorBreakEffect::Field` IS a
    /// definition field, even when a paragraph drain retired the lazily
    /// created session state. Re-establish it so marker bookkeeping
    /// (`term.c` buffer rules) applies for the whole head.
    pub(in crate::mandoc) fn ensure_definition_field_session(&mut self) {
        if self
            .execution
            .author_execution
            .as_ref()
            .is_some_and(|author| matches!(author.break_effect, AuthorBreakEffect::Field { .. }))
        {
            self.definition_state_mut();
        }
    }

    /// Preserve only a marker still present in the native unconsumed
    /// suffix. This register restoration writes no new formatter cell.
    pub(in crate::mandoc) fn retain_buffered_field_word_end_break(&mut self) {
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.field_buffer.has_pending_break_markers())
        {
            self.execution.word_end_break = WordEndBreak::Pending;
        }
    }

    pub(in crate::mandoc) fn note_provisional_definition_break(&mut self) {
        if let Some(definition) = &mut self.execution.definition
            && matches!(self.nodes.last(), Some(Inline::LineBreak { .. }))
        {
            definition.hang_row.provisional_trailing_break = Some(self.nodes.len() - 1);
        }
    }

    pub(in crate::mandoc) fn settle_provisional_definition_break(&mut self) {
        let Some(definition) = &mut self.execution.definition else {
            return;
        };
        if definition.hang_row.provisional_trailing_break == self.nodes.len().checked_sub(1)
            && !definition.hang_row.field_discarded
            && self.execution.word_end_break == WordEndBreak::Pending
        {
            // An earlier \p made this possible break, but the final \p had
            // no following graph to complete another term_fill() slice.
            // HEAD post consumes the buffer without closing the device row.
            self.nodes.pop();
        }
        definition.hang_row.provisional_trailing_break = None;
    }
}

fn receipt_has_authored_loop_rows(receipt: &super::super::field_buffer::FlushReceipt) -> bool {
    use super::super::field_buffer::FillBoundary;
    receipt.accepted_passes().is_some_and(|accepted| {
        accepted
            .with_remaining_field()
            .iter()
            .any(|pass| pass.boundary == FillBoundary::WordEndBreak)
    })
}

#[cfg(test)]
std::thread_local! {
    static EMPTY_LOOP_DEVICE_VIEWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests;
