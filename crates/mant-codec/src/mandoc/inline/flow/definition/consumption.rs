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

use super::super::native_field::FieldFlags;
use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, TrailingOutput, WordEndBreak,
    last_visible_character,
};
use super::device::NativeFieldDevice;
use super::flush::retain_unprinted_field_targets;
use super::state::{DefinitionFieldStyle, NoBreakField};

impl InlineBuilder {
    pub(in crate::mandoc::inline::flow) fn native_field_closes_unprinted_row(&self) -> bool {
        self.native_field_device(false).is_some_and(|device| {
            device.ends_row
                && device.emission == super::device::NativeFieldEmission::Unprinted
                && device.printed_row.is_none()
        })
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
        use super::super::field_buffer::FlushReceipt;
        let Some(targets) = self.native_field_targets(no_break_flush, None) else {
            return false;
        };

        let Some(receipt) = self.execution.definition.as_ref().and_then(|state| {
            (!state.field_buffer.is_empty())
                .then(|| state.field_buffer.flush_receipt(targets, false))
        }) else {
            return false;
        };

        // term_flushln() only reaches loop endline (term.c:217) when an
        // accepted pass has a remaining field. Its single accepted pass or
        // first-pass rejection has no loop rows to classify; the actual
        // device-tail retirement below still runs with its original flags.
        let empty_pass_ends = if owner_boundary || !receipt_has_loop_rows(&receipt) {
            Vec::new()
        } else {
            self.native_field_device(no_break_flush)
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
        };

        if let FlushReceipt::Accepted { passes } = &receipt {
            if !owner_boundary {
                self.project_accepted_field_passes(passes);
            }
            self.definition_state_mut().hang_row.field_discarded = false;
            self.project_completed_empty_passes(&empty_pass_ends);
            return false;
        }
        if !owner_boundary && let FlushReceipt::Rejected { passes, .. } = &receipt {
            // term_flushln() executes every accepted pass before nbr == 0
            // rejects its suffix. NBRZW can end a pass without a visible
            // scalar (term.c:340-349), so projecting only the final boundary
            // would merge two different endline events at the same scalar.
            self.project_accepted_field_passes(passes);
        }
        let definition = self.execution.definition.as_mut().expect("native field");
        let (passes, rejected_from) = match receipt {
            FlushReceipt::Accepted { passes } => {
                assert!(!passes.is_empty());
                definition.hang_row.field_discarded = false;
                return false;
            }
            FlushReceipt::Rejected {
                passes,
                rejected_from,
                definitive,
            } => {
                if owner_boundary && !definitive {
                    return false;
                }
                (passes, rejected_from)
            }
        };
        definition.hang_row.field_discarded = true;
        let accepted_owners = super::super::output::native_passes::accepted_owner_lengths(
            &definition.field_buffer,
            &definition.field_word_anchors,
            &passes,
        );
        let accepted_owned_prefix = !passes.is_empty()
            && definition
                .field_word_anchors
                .iter()
                .any(|anchor| anchor.start < rejected_from);
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
        self.project_completed_empty_passes(&empty_pass_ends);
        self.finish_rejected_field_state(owner_boundary, no_break_flush)
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
        let state = self.execution.definition.as_ref().expect("native field");
        let start = self
            .execution
            .author_execution
            .as_ref()
            .map_or(0, |author| author.field_output_start);
        let projected = super::super::output::native_passes::project_accepted_native_passes(
            &mut self.nodes,
            &state.field_buffer,
            &state.field_word_anchors,
            passes,
            state.projected_passes,
            start,
        );
        self.execution
            .definition
            .as_mut()
            .expect("native field")
            .projected_passes = projected;
    }

    /// Retire only the rejected native buffer's registers after its exact
    /// output interval was projected. The ordinary field tail is a separate
    /// device event and is returned to the actual flush caller.
    fn finish_rejected_field_state(&mut self, owner_boundary: bool, no_break_flush: bool) -> bool {
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
            self.execution.row_zero_graph = false;
            self.execution.pending_breakable_spaces = 0;
            self.execution.trailing_output = TrailingOutput::None;
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
            }
        }
        // The same numeric tail rule handles accepted and rejected final
        // passes; an IR owner drain executes neither device endline.
        let flags_end_row = self
            .native_field_device(no_break_flush)
            .is_some_and(|field| field.ends_row);
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
        self.execution.definition.is_some() && self.execution.author_execution.is_some()
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

    /// A real `term_flushln()` commits this input field irreversibly. Keep
    /// device viscol/minbl and the enclosing BODY lifetime, but retire its
    /// cells and projection ranges before another formatter word executes.
    pub(super) fn retire_consumed_native_field(&mut self) {
        let device = self.native_field_device(false);
        self.retire_native_field_with_device(device.as_ref());
    }

    pub(in crate::mandoc::inline::flow) fn retire_native_field_with_device(
        &mut self,
        device: Option<&NativeFieldDevice>,
    ) {
        self.retire_native_field_with_device_at(device, super::flush::FieldFlushBoundary::Continue);
    }

    pub(super) fn retire_native_field_with_device_at(
        &mut self,
        device: Option<&NativeFieldDevice>,
        boundary: super::flush::FieldFlushBoundary,
    ) {
        if let Some(device) = device {
            let mut output_start = device.output_start;
            self.retire_unprinted_no_break_separator(
                device.emission,
                device.separator_retention,
                device.separator_field,
                &mut output_start,
            );
            super::super::output::row_origins::project_native_positions(
                &mut self.nodes,
                &device.row_origins,
                &device.field_padding,
                output_start,
                self.execution
                    .definition
                    .as_ref()
                    .is_some_and(|state| state.column_origin_units.is_none()),
            );
            if let Some(author) = &mut self.execution.author_execution {
                author.field_output_start = self.nodes.len();
            }
        }
        if self.execution.has_column_output_scope()
            && boundary != super::flush::FieldFlushBoundary::ColumnPost
            && device.is_some_and(|device| device.ends_row && device.printed_row.is_some())
        {
            self.record_device_row_end();
        }
        if let Some(state) = &mut self.execution.definition {
            if let Some(device) = device {
                state.field_buffer.set_tab_offset(device.tab_offset);
                // Every real flush owns the final device position, including
                // a detached HEAD post. NOBREAK may leave that printed row
                // occupied after the input cells have retired (term.c:250-253).
                state.hang_row.viscol = device.viscol;
                state.hang_row.minbl = device.next_field_gap_cells;
                // Geometry return retires no printed device content. Real
                // flushes atomically transfer their row receipt, while a
                // real endline consumes the current row's origin advances.
                state.hang_row.unprojected_origin_units = if device.ends_row {
                    0
                } else {
                    device.unprojected_origin_units
                };
            }
            state.field_buffer.clear_consumed_field();
            state.field_word_anchors.clear();
            state.projected_passes = 0;
        }
    }

    pub(super) fn finish_definition_field_control(
        &mut self,
        field: NoBreakField,
        hang_gap_cells: u8,
        consume_body_gap: bool,
    ) {
        if field.style == DefinitionFieldStyle::Hang {
            let had_cell = self.has_formatter_cell();
            let definition = self.definition_state_mut();
            if had_cell || definition.hang_row.viscol > 0 {
                definition.hang_row.flush(usize::from(hang_gap_cells));
            }
        }
        // pre_br moves offset under BRIND for TAG and HANG alike; this
        // numeric origin remains scoped until a node restores it. Only a
        // later accepted native print turns it into projected positioning.
        self.move_definition_field_origin_to_body(
            u16::try_from(field.body_width).unwrap_or(u16::MAX),
        );
        if consume_body_gap {
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_body_gap_consumed();
        }
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            execution.break_effect = match field.style {
                DefinitionFieldStyle::Tag => AuthorBreakEffect::Line,
                DefinitionFieldStyle::Hang => AuthorBreakEffect::Field {
                    gap_cells: hang_gap_cells,
                    body_width_columns: u16::try_from(field.body_width).unwrap_or(u16::MAX),
                    field_width_columns: field.field_capacity_columns,
                    flags: FieldFlags::hang(),
                },
            };
        }
        self.execution.word_end_break = WordEndBreak::Clear;
        self.execution.wipe_remainder = false;
        self.execution.row_zero_graph = false;
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.empty_word = false;
        self.execution.final_word_join = Some(false);
        self.retire_consumed_native_field();
    }
}

fn receipt_has_loop_rows(receipt: &super::super::field_buffer::FlushReceipt) -> bool {
    use super::super::field_buffer::FlushReceipt;
    match receipt {
        FlushReceipt::Accepted { passes } => passes.len() > 1,
        FlushReceipt::Rejected { passes, .. } => !passes.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::field_buffer::{FieldBuffer, FieldCell, FieldWrite, FillTargets};
    use super::*;

    #[test]
    fn device_loop_views_are_needed_only_after_an_accepted_prefix() {
        // Complete accepted, NBRZW/p and bare-p refused sources ran fixed
        // CVS first. No actual remaining field means no loop endline at
        // term.c:217; first-pass nbr=0 likewise exits before that event.
        for (writes, expected) in [
            (FieldWrite::literal("ACCEPTED"), false),
            (vec![FieldWrite::Cell(FieldCell::ZeroWidthGraph)], false),
            (
                vec![
                    FieldWrite::Cell(FieldCell::BreakMarker),
                    FieldWrite::UnprojectedBlank,
                    FieldWrite::Cell(FieldCell::Graph {
                        text: 'R',
                        width: 1,
                    }),
                ],
                false,
            ),
            (
                vec![
                    FieldWrite::Cell(FieldCell::ZeroWidthGraph),
                    FieldWrite::Cell(FieldCell::BreakMarker),
                    FieldWrite::UnprojectedBlank,
                    FieldWrite::Cell(FieldCell::BreakMarker),
                    FieldWrite::UnprojectedBlank,
                    FieldWrite::Cell(FieldCell::Graph {
                        text: 'R',
                        width: 1,
                    }),
                ],
                true,
            ),
        ] {
            let mut buffer = FieldBuffer::default();
            buffer.apply_writes(&writes);
            let receipt = buffer.flush_receipt(
                FillTargets {
                    first: usize::MAX / 2,
                    rest: usize::MAX / 2,
                    unbounded: true,
                },
                false,
            );
            assert_eq!(receipt_has_loop_rows(&receipt), expected);
        }
    }
}
