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

//! Source and explicit line-request entry points on the shared formatter.

use super::super::native_field::FieldFlag;
#[cfg(test)]
use super::super::native_field::FieldFlags;
use super::super::{AuthorBreakEffect, Inline, InlineBuilder, PendingBoundary};
use super::state::PendingFieldGapOrigin;

impl InlineBuilder {
    /// Every definition HEAD eventually reaches `term_fill()`, including
    /// inset/diag/ohang heads without a NOBREAK field. Track accepted and
    /// rejected word-end slices in their shared native input buffer.
    pub(in crate::mandoc) fn begin_definition_head_consumption(&mut self) {
        self.definition_state_mut();
        if self.execution.has_column_output_scope() {
            // A new semantic HEAD owns its authored words, not the preceding
            // column field's deferred positioning pad. Preserve native minbl
            // for the real flush; its display origin is separate IR geometry.
            self.execution.pending_field_spaces = 0;
        }
    }

    pub(in crate::mandoc) fn set_definition_native_margin(&mut self, units: usize) {
        self.definition_state_mut().native_margin_units = Some(units);
    }

    pub(in crate::mandoc) fn has_definition_head(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|state| !state.head_flags_cleared && !state.run_in_continuation)
            && self.execution.author_execution.is_some()
    }

    /// CVS `mdoc_term.c` enters `NODE_LINE` before each no-fill child, but
    /// `term_newln()` flushes an active `NOBREAK` definition field. `BRIND` may
    /// start a new row when a tag overruns its width; HANG keeps that row.
    pub(in crate::mandoc) fn no_fill_source_line(&mut self) {
        self.execute_native_newline();
    }

    /// Execute `term_newln()` before any enclosing node restores geometry.
    /// Source `NODE_LINE` and macro posts share this flush; `roff_pre_br()` is
    /// separate because it also changes the field flags and row origin.
    pub(in crate::mandoc) fn execute_native_newline(&mut self) {
        self.commit_definition_row_origin();
        if let Some(definition) = &mut self.execution.definition {
            definition.row.commit_at_flush();
        }
        let field = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            });
        if let Some((start, gap, body, field_width, flags)) = field {
            self.flush_definition_field(start, gap, body, field_width, flags, false);
            if self.execution.pending_field_spaces > 0 {
                self.definition_state_mut().pending_gap_origin =
                    PendingFieldGapOrigin::CommittedFlush;
            }
            // NODE_LINE and termp_fd_post() both request term_newln().
            // Even if that flush has no buffered glyph, term_newln() sets
            // NOSPACE for the next term_word(). The retained trailspace is
            // separate and can still position the next HANG field.
            self.execution.boundary = PendingBoundary::Tight;
            self.definition_state_mut()
                .hang_row
                .suppress_next_auto_space = true;
        } else {
            self.hard_break();
        }
        self.reset_native_tab_origin();
    }

    /// Execute an explicit formatter line request inside a definition HEAD.
    ///
    /// CVS keeps `LIST_tag/LIST_hang` in a NOBREAK field until `term_newln()`
    /// has settled that field.  A plain `hard_break()` loses BRIND geometry,
    /// so `.br`, `.ti`, and the break phase of `.sp` must use this entrypoint.
    pub(in crate::mandoc) fn control_line_break(&mut self) -> bool {
        self.commit_definition_row_origin();
        if let Some(field) = self.take_no_break_field() {
            let capacity = field.field_capacity_columns;
            self.settle_no_break_field_line(field, 0);
            self.note_field_control_cleared_no_break(true, capacity);
            self.reset_native_tab_origin();
            return true;
        }
        let Some((start, gap, body, field_width_columns, flags)) = self
            .execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some((
                    execution.field_output_start,
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                )),
                AuthorBreakEffect::Line => None,
            })
        else {
            let had_cell = self.has_formatter_cell();
            self.hard_break();
            return had_cell;
        };
        let changed = self.flush_definition_field(
            start,
            gap,
            body,
            field_width_columns,
            flags,
            flags.contains(FieldFlag::Brind),
        );
        self.note_field_control_cleared_no_break(
            flags.contains(FieldFlag::Brind),
            field_width_columns,
        );
        self.reset_native_tab_origin();
        changed
    }

    /// `roff_term_pre_br()` clears `TERMP_NOBREAK` (with `TERMP_BRIND`) for the
    /// rest of this field (roff_term.c:71-78); the enclosing request node
    /// returns before any flag restore (mdoc_term.c:394-396).
    pub(super) fn note_field_control_cleared_no_break(&mut self, brind: bool, capacity: u16) {
        if !brind {
            return;
        }
        let definition = self.definition_state_mut();
        definition.no_break_cleared = true;
        definition.margin_override = Some(usize::MAX / 2);
        definition.cleared_field_capacity_columns = capacity;
    }

    /// Whether the next word already has a request-armed concatenation
    /// (`TERMP_NOSPACE`, `roff_term.c:78`) pending.
    pub(in crate::mandoc) fn concat_word_armed(&self) -> bool {
        self.execution.concat_next_word
    }

    /// A word's `\p` marker met a surviving breakable blank with no graph
    /// recorded in the flush unit (term.c:143-146): the unprinted remainder
    /// of the unit dies with the row reset (term.c:233-237). Every word
    /// appended until the next real row retirement projects nothing.
    pub(in crate::mandoc) fn note_definitive_word_rejection(&mut self) {
        self.execution.wipe_remainder = true;
    }

    /// A zero-width graph class cell armed `graph` for the current row
    /// without printing (term.c:349): a later marker decision must treat
    /// the row as carrying input, not as whitespace-only.
    pub(in crate::mandoc) fn note_row_zero_graph(&mut self) {
        self.execution.row_zero_graph = true;
    }

    /// Whether the current device row carries any graph input: printable
    /// projection, a buffered `\\z` glyph, or an NBRZW-class cell.
    pub(in crate::mandoc) fn current_row_has_graph(&self) -> bool {
        self.nodes
            .iter()
            .rev()
            .take_while(|node| !matches!(node, Inline::LineBreak { .. }))
            .any(|node| {
                !mant_ir::inline_plain_text(std::slice::from_ref(node))
                    .chars()
                    .all(char::is_whitespace)
            })
            || self.execution.row_zero_graph
            || self.execution.zero_advance.has_pending_glyph()
    }

    /// Arm the next word's concatenation for a filled cleared field
    /// (term.c:250-253 with 205-207): same no-separator word, but the
    /// body starts at the description column rather than against the
    pub(in crate::mandoc) fn note_flushed_at_body_column(&mut self) {
        self.execution.concat_next_word = true;
        self.execution.concat_flush_source = true;
    }

    pub(in crate::mandoc) fn note_discretionary_hang_field_break(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            definition.hang_row.field_discretionary_break = true;
        }
    }

    /// The It HEAD post executed (mdoc_term.c:961-962): the field's
    /// NOBREAK/BRTRSP/BRIND/HANG flags and trailspace are gone, but the
    /// run-in kinds keep the shared input buffer (939-945 runs no
    /// `term_newln` at HEAD post). Later flush decisions over the surviving
    /// cells must use the cleared flag set.
    pub(in crate::mandoc) fn note_definition_head_flags_cleared(&mut self) {
        if let Some(state) = &mut self.execution.definition {
            state.head_flags_cleared = true;
        }
    }

    /// A pending `\p` breaks through a graphless pass in whichever native
    /// buffer owns the current word (term.c:143-146): the definition field
    /// when an author session holds it, otherwise the plain flush unit.
    /// No-fill words keep the text executor's own decision path.
    pub(in crate::mandoc) fn pending_flush_break_has_no_graph(&self) -> bool {
        if self.in_definition_field() {
            return self
                .execution
                .definition
                .as_ref()
                .is_some_and(|state| state.field_buffer.pending_pass_is_graphless());
        }
        !self.execution.no_fill_word_active
            && !self.execution.flush_unit.is_empty()
            && self.execution.flush_unit.pending_pass_is_graphless()
    }
}

#[cfg(test)]
mod recording_tests;
