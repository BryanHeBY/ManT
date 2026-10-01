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

//! Executed vertical rows and skip-space debt, independent of IR drains.

use super::super::native_field::FieldFlag;
use super::super::{AuthorBreakEffect, InlineBuilder, PendingBoundary};

impl InlineBuilder {
    /// Execute a visited empty TEXT at its actual node position. Native
    /// `print_man_node()`/`print_mdoc_node()` call `term_newln()` for an active \c;
    /// otherwise `term_vspace()` consumes skipvsp before emitting a blank row.
    pub(in crate::mandoc) fn execute_visited_empty_text(&mut self, no_fill: bool) {
        if self.final_source_continuation_or(false) {
            self.hard_break();
            // term_newln() does not clear TERMP_NONEWLINE. Another empty
            // source TEXT therefore also takes this branch, without adding
            // a vertical row or consuming skipvsp.
            self.continue_source_line(true);
        } else {
            // Empty TEXT calls term_vspace(), not roff_term_pre_sp().
            // Its resolved rows are recorded by that single execution
            // entry; it must not add a second receipt or execute pre_br.
            self.native_vertical_space_with_origin(
                1,
                if no_fill {
                    super::super::CompletedRowOrigin::LiteralText
                } else {
                    super::super::CompletedRowOrigin::Layout
                },
            );
        }
    }

    /// Execute an inline vertical-space request without retaining its
    /// numeric operand as document text.  `term_vspace(n)` first closes an
    /// occupied row, then emits `n` empty rows.
    // term_vspace() must settle the active field before asserting rows.
    pub(in crate::mandoc) fn vertical_space(&mut self, rows: usize) {
        if let Some(field) = self.take_no_break_field() {
            self.vertical_space_in_definition_field(field, rows);
            self.finish_native_vertical_row(rows);
            if rows > 0
                && self
                    .execution
                    .author_execution
                    .as_ref()
                    .is_some_and(|execution| {
                        matches!(execution.break_effect, AuthorBreakEffect::Line)
                    })
            {
                self.execution
                    .author_execution
                    .as_mut()
                    .unwrap()
                    .field_output_start = self.nodes.len();
            }
            return;
        }
        if self
            .active_definition_field()
            .is_some_and(|field| !field.flags.contains(FieldFlag::Brind))
        {
            // Column fields have no BRIND transition after term_vspace().
            // It emits real device rows while retaining their NOBREAK state.
            self.native_vertical_space(u16::try_from(rows).unwrap_or(u16::MAX));
            self.control_line_break();
            return;
        }
        if rows == 0 {
            // roff_term_pre_sp() calls no term_vspace() for zero rows. Its
            // remaining pre_br() follows the active HANG/TAG field rules.
            // term_newln() does not clear TERMP_NONEWLINE: a preceding \c
            // still suppresses the next no-fill NODE_LINE after this request.
            self.control_line_break();
            self.execution.final_word_join = Some(false);
            return;
        }
        // term_vspace() first runs term_newln(). Settle an unprintable HANG
        // field there, before the request's vertical rows are projected.
        self.discard_unprinted_definition_field_output();
        let field = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    body_width_columns,
                    field_width_columns,
                    flags,
                    ..
                } => Some((body_width_columns, field_width_columns, flags)),
                AuthorBreakEffect::Line => None,
            });
        if let Some((body_width_columns, field_width_columns, flags)) = field {
            // roff_term.c::roff_term_pre_sp() runs its term_vspace() rows
            // BEFORE the trailing roff_term_pre_br() clears NOBREAK/BRIND
            // and trailspace. Evaluate the leading term_newln()'s tail rule
            // (term.c:250-253) from the still-live device first; the clear
            // and its spent-minbl bookkeeping only run afterwards.
            let leading_newln_closed_row = self.leading_sp_newln_closed_row(flags);
            self.note_field_control_cleared_no_break(
                flags.contains(FieldFlag::Brind),
                field_width_columns,
            );
            if !self.has_formatter_cell() {
                self.vertical_space_after_empty_field(
                    rows,
                    body_width_columns,
                    field_width_columns,
                    flags,
                    leading_newln_closed_row,
                );
                return;
            }
            self.vertical_space_after_occupied_field(
                rows,
                body_width_columns,
                field_width_columns,
                flags,
                leading_newln_closed_row,
            );
        } else {
            self.hard_break();
            self.retain_line_breaks(rows);
        }
        self.finish_native_vertical_row(rows);
        if let Some((body, _, flags)) = field
            && flags.contains(FieldFlag::Brind)
        {
            self.move_definition_field_origin_to_body(body);
        }
        if self.execution.definition.is_some()
            && self
                .execution
                .author_execution
                .as_ref()
                .is_some_and(|execution| matches!(execution.break_effect, AuthorBreakEffect::Line))
        {
            self.execution
                .author_execution
                .as_mut()
                .unwrap()
                .field_output_start = self.nodes.len();
        }
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
    }

    /// The row fact of the `term_newln()` leading the `term_vspace()` rows
    /// of `roff_term_pre_sp()`: it only flushes a live buffer or an
    /// occupied device row, and its `term_flushln()` tail then ends the
    /// row unless HANG holds it open. With this request's own
    /// `roff_term_pre_br()` still pending, NOBREAK is live unless an
    /// earlier request already cleared it, so only an overrun — or that
    /// earlier clear — closes the row here (term.c:250-253).
    fn leading_sp_newln_closed_row(&self, flags: super::super::native_field::FieldFlags) -> bool {
        let Some(state) = self.execution.definition.as_ref() else {
            return false;
        };
        let flushes_live_field = state.hang_row.viscol > 0
            || state.field_buffer.resume_offset() < state.field_buffer.cells().len();
        flushes_live_field
            && !flags.contains(FieldFlag::Hang)
            && (state.no_break_cleared
                || self
                    .native_field_device(false)
                    .is_some_and(|device| device.overruns))
    }

    fn vertical_space_after_occupied_field(
        &mut self,
        rows: usize,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: super::super::native_field::FieldFlags,
        leading_newln_closed_row: bool,
    ) {
        // `roff_term_pre_sp()` runs term_vspace() while the field still
        // carries its NOBREAK/BRIND/HANG flags and trailspace; pre_br()
        // only afterwards clears them and moves the origin
        // (roff_term.c:195-215, 69-78). Every requested row is one
        // endline() (term.c:489-497), and the leading term_newln()'s
        // term_flushln() can itself end a row through its tail decision
        // (term.c:250-253): with NOBREAK still live, that decision is the
        // overrun rule the device receipt already evaluated. Count the
        // endline events from those device facts. The projected width of
        // the field's IR nodes is a second row authority: rejected
        // prefixes and rows an in-field `\p` break already closed make it
        // disagree with the device, so it is not consulted here.
        let endlines = usize::from(leading_newln_closed_row) + rows;
        let nodes_before = self.nodes.len();
        self.flush_zero_advance();
        self.hard_break();
        let hard_break_ended_row = self.nodes.len() > nodes_before;
        self.retain_line_breaks(endlines - usize::from(hard_break_ended_row));
        self.definition_state_mut().pending_indent = Some(usize::from(body_width_columns));
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            if flags.wraps() {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_field_exited();
                execution.break_effect = AuthorBreakEffect::Line;
            } else {
                self.execution
                    .definition
                    .as_mut()
                    .expect("definition field session")
                    .outcome
                    .mark_body_gap_consumed();
                execution.break_effect = AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns,
                    field_width_columns,
                    flags,
                };
            }
        }
        // term.c:233-237: the committed flush ends the field; the
        // input buffer restarts empty for whatever follows this row.
        if let Some(definition) = &mut self.execution.definition {
            definition.field_buffer.clear();
            definition.field_word_anchors.clear();
        }
    }

    fn vertical_space_after_empty_field(
        &mut self,
        rows: usize,
        body_width_columns: u16,
        field_width_columns: u16,
        flags: super::super::native_field::FieldFlags,
        leading_newln_closed_row: bool,
    ) {
        // `term_vspace()` always emits its requested empty row, but
        // its leading `term_newln()` leaves a bare BACKAFTER armed
        // when neither tcol nor viscol is occupied.  The following
        // BRIND phase still ends a tag field; HANG keeps its run-in
        // body contract.  Preserve those independent effects.
        // term.c:475-480,489-497: the conditional term_newln()
        // emits nothing with no buffered cell or occupied row.
        // Only the requested endline events exist — plus, when the
        // leading newln did flush a live field and its tail ended the
        // row, that one earlier endline; an IR helper return must not
        // contribute another row close.
        self.retain_line_breaks(rows + usize::from(leading_newln_closed_row));
        self.execution.boundary = PendingBoundary::Tight;
        if flags.wraps() {
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            if let Some(execution) = &mut self.execution.author_execution {
                execution.field_output_start = self.nodes.len();
                execution.break_effect = AuthorBreakEffect::Line;
            }
        }
        self.execution.final_word_join = Some(false);
        self.execution.final_source_continuation = Some(false);
        self.finish_native_vertical_row(rows);
        self.move_definition_field_origin_to_body(body_width_columns);
        if !flags.wraps()
            && let Some(execution) = &mut self.execution.author_execution
        {
            // pre_br clears trailspace even when term_newln had
            // no cell to flush. HANG survives; offset is not viscol.
            execution.break_effect = AuthorBreakEffect::Field {
                gap_cells: 0,
                body_width_columns,
                field_width_columns,
                flags,
            };
        }
    }

    pub(super) fn finish_native_vertical_row(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        if let Some(definition) = &mut self.execution.definition {
            // term_vspace() emits an endline after the current field. Unlike
            // a HANG term_newln(), the next word starts a new device row.
            definition.hang_row.endline();
            definition.vertical_started_row = true;
        }
    }

    /// Plain `term_vspace()`, as used by `print_bvspace()`; unlike roff `.sp`,
    /// this does not execute `pre_br` or clear BRIND/NOBREAK afterwards.
    pub(in crate::mandoc) fn native_vertical_space(&mut self, rows: u16) {
        self.native_vertical_space_with_origin(rows, super::super::CompletedRowOrigin::Layout);
    }

    fn native_vertical_space_with_origin(
        &mut self,
        rows: u16,
        origin: super::super::CompletedRowOrigin,
    ) {
        self.execute_native_newline();
        let rows = self.execution.resolve_vertical_space(i32::from(rows));
        // NOBREAK/HANG can leave an already printed device row alive after
        // term_newln(). The first backend endline then closes that graph;
        // only later endlines complete empty rows (term.c:489-497). Observe
        // the device after the flush, not the pre-flush buffer or IR tail.
        let closes_printed_row = rows > 0 && self.execution.has_open_native_device_row();
        let completed_rows = rows.saturating_sub(u16::from(closes_printed_row));
        self.retain_line_breaks(usize::from(rows));
        self.finish_native_vertical_row(usize::from(rows));
        self.asserted_vertical_row |= completed_rows > 0;
        // term_vspace() already emitted these empty rows after resolving
        // skipvsp. They survive an output-owner return independently of
        // the ordinary row end from its leading term_newln().
        self.record_completed_rows(completed_rows, origin);
    }
}
