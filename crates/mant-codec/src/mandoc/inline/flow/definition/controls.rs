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

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{
    AuthorBreakEffect, FormatterColumn, Inline, InlineBuilder, PendingBoundary, TrailingOutput,
    WordEndBreak, has_printable_character, last_visible_character,
};
use super::device::NativeFieldDevice;
use super::flush::retain_unprinted_field_targets;
use super::state::{DefinitionFieldStyle, HangRowTransition, NoBreakField, PendingFieldGapOrigin};

struct ActiveDefinitionField {
    start: usize,
    gap: u8,
    body: u16,
    field_width_columns: u16,
    flags: FieldFlags,
}

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

    /// `term.c::encode1()` buffers the character following `\\z` in the
    /// current field. It has not reached an IR node yet, but `term_fill()` and
    /// `term_field()` still count its printed width at a field boundary.
    pub(super) fn pending_hang_glyph_width(&self) -> Option<usize> {
        let mut zero_advance = self.execution.zero_advance.clone();
        let mut pending = Vec::new();
        zero_advance.finish_into(&mut pending);
        let text = super::super::super::plain_text(&pending);
        text.chars()
            .any(|ch| !ch.is_whitespace() || ch == '\u{a0}')
            .then(|| mant_ir::geometry::text_width(&text))
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

    /// A HANG head that filled its capacity while a request had cleared
    /// `TERMP_NOBREAK` reaches the body column with no trailspace
    /// (term.c:250-253 with 205-207): the body's first word concatenates.
    pub(in crate::mandoc) fn cleared_field_filled_capacity(&self) -> bool {
        let Some(state) = &self.execution.definition else {
            return false;
        };
        if !state.no_break_cleared
            || state.cleared_field_capacity_columns == 0
            || state.hang_row.field_discretionary_break
        {
            return false;
        }
        // HEAD post prints using the current stops, offset and margin.
        // term_fill()'s old capacity or cumulative width cannot prove the
        // BODY origin after a .ta or an internal wrap (term.c:113-253).
        let body_origin = state.native_margin_units.unwrap_or_else(|| {
            usize::from(state.cleared_field_capacity_columns).saturating_mul(24)
        });
        self.native_field_device(false).is_some_and(|field| {
            let column = field.viscol.saturating_mul(24);
            !field.ends_row
                && (column > body_origin || column == body_origin && field.final_pass_continued)
        })
    }

    /// Whether an author-split row end restarted the head field (its
    /// breaks do not close the head before BODY).
    pub(in crate::mandoc) fn definition_author_restarted(&self) -> bool {
        self.execution
            .definition
            .as_ref()
            .is_some_and(|state| state.outcome.is_field_restarted())
    }

    pub(in crate::mandoc) fn definition_field_exited(&self) -> bool {
        self.execution.definition.as_ref().is_some_and(|state| {
            state.outcome.field_exited()
                || matches!(
                    self.execution
                        .author_execution
                        .as_ref()
                        .map(|author| author.break_effect),
                    Some(AuthorBreakEffect::Field { .. })
                ) && self
                    .native_field_device(false)
                    .is_some_and(|field| field.ends_row)
        })
    }

    pub(in crate::mandoc::inline::flow) fn reset_native_tab_origin(&mut self) {
        // term_newln() resets taboff even if there was no buffer to flush.
        if let Some(state) = &mut self.execution.definition {
            state.field_buffer.set_tab_offset(0);
        }
    }

    pub(in crate::mandoc) fn definition_body_gap_consumed(&self) -> bool {
        let Some(state) = &self.execution.definition else {
            return false;
        };
        if !state.hang_row.margin_flush_seen
            && !state.field_buffer.is_empty()
            && (state.hang_row.transition == HangRowTransition::WordAfterFlush
                || state.no_break_cleared)
            && let Some(AuthorBreakEffect::Field {
                body_width_columns,
                gap_cells,
                flags,
                ..
            }) = self
                .execution
                .author_execution
                .as_ref()
                .map(|author| author.break_effect)
            && flags.contains(FieldFlag::Hang)
        {
            // At HEAD post, term_newln() flushes the final HANG field. Only
            let mut final_row = state.hang_row.clone();
            if let Some(width) = self.pending_hang_glyph_width() {
                final_row.pending_glyph(width);
            }
            let body_column = usize::from(body_width_columns);
            if final_row.field_discarded {
                // No new glyph reached the device. The preceding field's
                // viscol and minbl still locate BODY; source text inside the
                // discarded field cannot create a soft-wrap uncertainty.
                // A rejected final field produced no new device graph and
                // cannot supersede a separator already represented by the
                // preceding committed field. Node geometry restoration
                // may have reset offset before this final flush.
                return state.outcome.body_gap_consumed()
                    || final_row.final_column() >= body_column;
            }
            // term.c:156-229 prints the unconsumed field using the current
            // tab stops and may finish on a later physical row. Its actual
            // device column, not the widths accumulated before a .ta or a
            // field wrap, is the only column that can prove BODY's origin.
            let cumulative_column = self
                .native_field_device(false)
                .map_or_else(|| final_row.final_column(), |field| field.viscol);
            // CVS term.c::term_fill() may wrap at a breakable cell *inside*
            // this final field. Its summed width then says nothing about the
            // last physical row. Retain the word boundary unless the field
            // provably stayed on one row (or had no breakable cell).
            let final_row_proven = !final_row.field_unproven_break
                && (cumulative_column <= body_column
                    || (!final_row.field_discretionary_break
                        && (final_row.field_last_unbreakable_width >= body_column
                            || !final_row.field_breakable)));
            return gap_cells == 0 && final_row_proven && cumulative_column >= body_column;
        }
        state.outcome.body_gap_consumed()
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

        if let FlushReceipt::Accepted { passes } = &receipt {
            if !owner_boundary {
                self.project_accepted_field_passes(passes);
            }
            self.definition_state_mut().hang_row.field_discarded = false;
            return false;
        }
        let definition = self.execution.definition.as_mut().expect("native field");
        let (passes, rejected_from) = match receipt {
            FlushReceipt::Accepted { passes } => {
                debug_assert!(!passes.is_empty());
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
        let anchor = definition
            .field_word_anchors
            .iter()
            .rev()
            .find(|(cell, _, _)| *cell <= rejected_from)
            .cloned();
        let (marker, prefix_cells) = anchor.map_or((None, 0), |(_, marker, content)| {
            let length = super::super::output::native_passes::accepted_owner_prefix_length(
                &definition.field_buffer,
                &passes,
                content,
            );
            (Some(marker), length)
        });
        let accepted_owned_prefix = !passes.is_empty()
            && definition
                .field_word_anchors
                .iter()
                .any(|(cell, _, _)| *cell < rejected_from);
        definition.hang_row.accepted_prefix_before_rejection = accepted_owned_prefix;
        let current_field_start = self
            .execution
            .author_execution
            .as_ref()
            .map_or(0, |author| author.field_output_start)
            .min(self.nodes.len());
        let mut pending_output = self.nodes.split_off(current_field_start);
        let owned = marker.as_deref().is_some_and(|marker| {
            super::super::output::split::retain_native_field_prefix(
                &mut pending_output,
                marker,
                prefix_cells,
            )
        });
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
        self.finish_rejected_field_state(owner_boundary, no_break_flush)
    }

    /// The plain-flow analogue of `project_definition_field_receipt()`
    /// (`term_flushln` over the shared `tcol->buf`, term.c:233-237 reached
    /// through 143-146): a definitively rejected flush unit dies at its
    /// retirement boundary - the unprinted suffix is trimmed from IR back
    /// to the accepted prefix and the zero-advance register is discarded.
    /// Returns whether the retirement ended a native row.
    pub(in crate::mandoc) fn retire_plain_flush_unit(&mut self) -> bool {
        Self::retire_plain_flush_unit_at(&mut self.execution, &mut self.nodes)
    }

    /// Execution-state form shared with the no-fill row finisher, which
    /// retires the same native buffer without a live builder.
    pub(in crate::mandoc) fn retire_plain_flush_unit_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
    ) -> bool {
        use super::super::field_buffer::{FillTargets, FlushReceipt};
        // An author-less definition session has no field geometry: its
        // buffer is the same native `tcol->buf` as the plain unit (term.c
        // runs one term_fill() regardless of authorship) and retires with
        // the same receipt. Borrow whichever buffer is live.
        let authorless_definition =
            execution.definition.is_some() && execution.author_execution.is_none();
        if execution.definition.is_some() && !authorless_definition {
            return false;
        }
        let buffer;
        let anchors;
        let output_start;
        if authorless_definition {
            let definition = execution.definition.as_mut().expect("session");
            buffer = std::mem::take(&mut definition.field_buffer);
            anchors = std::mem::take(&mut definition.field_word_anchors);
            output_start = execution.flush_unit_output_start.min(nodes.len());
            if buffer.is_empty() {
                Self::clear_authorless_definition_at(execution, nodes);
                return false;
            }
        } else {
            if execution.flush_unit.is_empty() {
                return false;
            }
            buffer = std::mem::take(&mut execution.flush_unit);
            anchors = std::mem::take(&mut execution.flush_unit_anchors);
            output_start = execution.flush_unit_output_start.min(nodes.len());
        }
        // BRNEVER-shaped (term.c:134,143-144): responsive reflow owns the
        // device width, so a plain pass only ever ends at authored markers.
        let targets = FillTargets {
            first: usize::MAX / 2,
            rest: usize::MAX / 2,
            unbounded: true,
        };
        let receipt = buffer.flush_receipt(targets, false);
        let passes = match &receipt {
            FlushReceipt::Accepted { passes } | FlushReceipt::Rejected { passes, .. } => passes,
        };
        // A deferred scanner can accept several authored-marker passes at
        // the actual term_flushln(). Acceptance still carries their row
        // events (term.c:165-220); retirement cannot silently omit them.
        // The same projector handles a complete unit and an accepted
        // prefix whose following pass is rejected.
        super::super::output::native_passes::project_accepted_native_passes(
            nodes,
            &buffer,
            &anchors,
            passes,
            0,
            output_start,
        );
        let FlushReceipt::Rejected {
            passes,
            rejected_from,
            definitive,
        } = receipt
        else {
            // The flushed row prints the whole unit; nothing is unprinted.
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return false;
        };
        if !definitive {
            // term_flushln() still reset the buffer (term.c:235-237); a
            // non-definitive stop leaves no unprinted suffix to trim.
            Self::restore_retired_buffer(execution, nodes, authorless_definition, buffer, anchors);
            return false;
        }

        let anchor = anchors
            .iter()
            .rev()
            .find(|(cell, _, _)| *cell <= rejected_from)
            .cloned();
        let (marker, prefix_cells) = anchor.map_or((None, 0), |(_, marker, content)| {
            let length = super::super::output::native_passes::accepted_owner_prefix_length(
                &buffer, &passes, content,
            );
            (Some(marker), length)
        });
        let accepted_owned_prefix =
            !passes.is_empty() && anchors.iter().any(|(cell, _, _)| *cell < rejected_from);
        let mut pending_output = nodes.split_off(output_start);
        let owned = marker.as_deref().is_some_and(|marker| {
            crate::mandoc::inline::flow::output::split::retain_native_field_prefix(
                &mut pending_output,
                marker,
                prefix_cells,
            )
        });
        if !owned {
            retain_unprinted_field_targets(&mut pending_output);
        }
        nodes.extend(pending_output);
        if accepted_owned_prefix
            && !crate::mandoc::inline::flow::output::ends_with_executed_line_break(nodes)
        {
            // term_flushln() ended the last accepted pass before discovering
            // nbr=0; the retirement boundary must expose that native event.
            nodes.push(Inline::line_break());
        }
        // term.c::term_flushln() clears both backtracking flags; a rejected
        // unit dies whole, including a still-buffered `\z` glyph.
        execution.zero_advance.discard_at_row_end();
        drop(buffer);
        drop(anchors);
        if authorless_definition {
            Self::clear_authorless_definition_at(execution, nodes);
        } else {
            Self::clear_plain_flush_unit_at(execution, nodes);
        }
        true
    }

    /// `term_flushln()` clears the consumed buffer at every retirement
    /// (`term.c`:235-237); the next word starts a fresh flush unit whose
    /// output interval begins at the current IR end.
    /// Row-boundary reset for flows whose marker semantics live in the text
    /// executor: only the native buffer dies with the row (term.c:235-237).
    pub(in crate::mandoc) fn clear_plain_flush_unit_for_row(
        execution: &mut super::super::InlineExecutionState,
    ) {
        execution.flush_unit.clear();
        execution.flush_unit.set_tab_offset(0);
        execution.flush_unit_anchors.clear();
        execution.flush_unit_output_start = 0;
    }

    pub(in crate::mandoc::inline::flow) fn clear_plain_flush_unit_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        execution.flush_unit.clear();
        execution.flush_unit.set_tab_offset(0);
        execution.flush_unit_anchors.clear();
        execution.flush_unit_output_start = nodes.len();
    }

    /// Drop a borrowed retirement buffer after its receipt was consumed:
    /// term.c:235-237 clears it either way; the borrowed form must not
    /// re-enter the session.
    fn restore_retired_buffer(
        execution: &mut super::super::InlineExecutionState,
        nodes: &mut [Inline],
        authorless_definition: bool,
        buffer: super::super::field_buffer::FieldBuffer,
        anchors: Vec<(usize, String, usize)>,
    ) {
        drop(anchors);
        drop(buffer);
        if authorless_definition {
            Self::clear_authorless_definition_at(execution, nodes);
        } else {
            Self::clear_plain_flush_unit_at(execution, nodes);
        }
    }

    fn clear_authorless_definition_at(
        execution: &mut super::super::InlineExecutionState,
        nodes: &[Inline],
    ) {
        if let Some(definition) = &mut execution.definition {
            definition.field_buffer.clear();
            definition.field_buffer.set_tab_offset(0);
            definition.field_word_anchors.clear();
        }
        execution.flush_unit.clear();
        execution.flush_unit_anchors.clear();
        execution.flush_unit_output_start = nodes.len();
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

    pub(in crate::mandoc::inline::flow) fn append_fixed_cells(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.append_projected(vec![Inline::Text {
            value: " ".repeat(count),
        }]);
        self.execution.trailing_output = TrailingOutput::FixedBlank;
    }

    /// Execute `.ti` through its preceding `roff_term_pre_br()` boundary.
    ///
    /// `ManT` deliberately omits the device-specific temporary offset.  In
    /// particular, the numeric operand is not printable padding: the pinned
    /// renderer applies it to `p->ti` and `tcol->offset` only after flushing
    /// the current field.  Retain the field's own trailspace and boundary,
    /// then discard the temporary device position as documented.
    pub(in crate::mandoc) fn temporary_indent(&mut self) {
        if let Some(field) = self.take_no_break_field() {
            self.restore_no_break_field_projection(field, false);
            match field.style {
                DefinitionFieldStyle::Tag => {
                    self.force_output_line_break();
                    self.execution
                        .definition
                        .as_mut()
                        .expect("definition field session")
                        .outcome
                        .mark_field_exited();
                }
                DefinitionFieldStyle::Hang => {
                    // `term_newln()` leaves NOSPACE set.  HANG keeps the
                    // device row alive, so a following control-only author
                    // transition must not synthesize an ordinary word blank
                    // between the prior field and its eventual head text.
                    self.execution.boundary = PendingBoundary::Tight;
                    self.execution.pending_field_spaces = 1;
                }
            }
            self.finish_definition_field_control(field, 0, false);
            return;
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
            self.control_line_break();
            return;
        };
        if !self.has_formatter_cell() {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        let field_width = mant_ir::geometry::text_width(&super::super::super::plain_text(field));
        let overruns =
            flags.wraps() && field_width.saturating_add(usize::from(gap)) > usize::from(body);
        if overruns {
            self.hard_break();
        } else {
            self.append_fixed_cells(usize::from(gap));
        }
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
                    body_width_columns: body,
                    field_width_columns,
                    flags,
                };
            }
        }
        self.execution.boundary = PendingBoundary::Tight;
    }

    /// Enter or leave no-fill mode at a physical source-line boundary.
    /// `print_mdoc_node()` performs that boundary in addition to the request's
    /// own `roff_term_pre_br()` dispatch.
    fn finish_resumed_fill_mode_boundary(&mut self) -> bool {
        if let Some(field) = self.take_no_break_field() {
            // print_mdoc_node() runs this fill-mode boundary in addition to
            // the request's own roff_term_pre_br() (roff_term.c:45-58).
            let capacity = field.field_capacity_columns;
            self.settle_no_break_field_line(field, capacity);
            self.note_field_control_cleared_no_break(true, capacity);
            self.execution
                .definition
                .as_mut()
                .expect("definition field session")
                .outcome
                .mark_field_exited();
            return true;
        }
        false
    }

    pub(in crate::mandoc) fn fill_mode_boundary(&mut self) {
        self.commit_definition_row_origin();
        // The request's roff_term_pre_br() sets TERMP_NOSPACE after its
        // term_newln() (roff_term.c:75-78): the first word after `.nf`/
        // `.fi` concatenates onto the current row with no auto blank —
        // the reference prints `body linetail text` after `.fi`.
        self.execution.concat_next_word = true;
        self.execution.concat_flush_source = false;
        if self.finish_resumed_fill_mode_boundary() {
            return;
        }
        let Some(ActiveDefinitionField {
            start,
            gap,
            body,
            field_width_columns,
            flags,
        }) = self.active_definition_field()
        else {
            self.control_line_break();
            return;
        };
        if !flags.contains(FieldFlag::Brind) {
            self.execution.concat_next_word = false;
            // roff_term_pre_br() changes field origin/flags only under
            // BRIND. Column NOBREAK survives fi/nf; the receipt, not the
            // mode switch, decides whether this physical row closes.
            self.control_line_break();
            return;
        }
        // fi/nf share pre_br(): consume the old native buffer before the
        // request changes BRIND/NOBREAK (roff_term.c:45-58,69-78). A zero
        // glyph still waiting for IR belongs to that buffer's receipt.
        self.discard_unprinted_definition_field_output();
        if self
            .execution
            .definition
            .as_ref()
            .is_some_and(|state| state.hang_row.field_discarded)
        {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            self.note_field_control_cleared_no_break(
                flags.contains(FieldFlag::Brind),
                field_width_columns,
            );
            self.reset_native_tab_origin();
            return;
        }
        let native = self.native_field_device(false);
        self.note_field_control_cleared_no_break(
            flags.contains(FieldFlag::Brind),
            field_width_columns,
        );
        // roff.c:3581-3585 with 957-958: every node under ROFF_NOFILL prints
        // with NODE_NOFILL, and mdoc_term.c:314-318 maps that to
        // TERMP_BRNEVER — an infinite term_fill() target (term.c:143-144).
        if !self.has_formatter_cell() {
            return;
        }
        self.flush_zero_advance();
        let field = self.nodes.get(start..).unwrap_or_default();
        if !has_printable_character(field) {
            self.flush_definition_field(start, gap, body, field_width_columns, flags, true);
            return;
        }
        if flags.wraps() {
            // roff_term.c:73-75: the request's BRIND moved the row origin to
            // the field's right margin and the roff node escapes the
            // save/restore (mdoc_term.c:393-397). The word already flushed
            // this field, so the pending-indent arm cannot carry it; the
            // break itself does.
            let row_indent = if flags.contains(FieldFlag::Brind) {
                field_width_columns
            } else {
                0
            };
            self.hard_break();
            if row_indent > 0
                && let Some(Inline::LineBreak { indent_columns }) = self.nodes.last_mut()
            {
                *indent_columns = row_indent;
            }
            self.definition_state_mut().row.indent_columns = row_indent;
            self.definition_state_mut().pending_indent = Some(usize::from(body));
        } else {
            let width = mant_ir::geometry::text_width(&super::super::super::plain_text(field));
            self.finish_fill_mode_open_row(
                body,
                field_width_columns,
                gap,
                flags,
                width,
                native.as_ref(),
            );
        }
        self.execution
            .definition
            .as_mut()
            .expect("definition field session")
            .outcome
            .mark_field_exited();
        if let Some(execution) = &mut self.execution.author_execution {
            execution.field_output_start = self.nodes.len();
            execution.break_effect = if flags.wraps() {
                AuthorBreakEffect::Line
            } else {
                AuthorBreakEffect::Field {
                    gap_cells: 0,
                    body_width_columns: body,
                    field_width_columns,
                    flags,
                }
            };
        }
        self.execution.boundary = PendingBoundary::Tight;
    }

    fn active_definition_field(&self) -> Option<ActiveDefinitionField> {
        self.execution
            .author_execution
            .as_ref()
            .and_then(|execution| match execution.break_effect {
                AuthorBreakEffect::Field {
                    gap_cells,
                    body_width_columns,
                    field_width_columns,
                    flags,
                } => Some(ActiveDefinitionField {
                    start: execution.field_output_start,
                    gap: gap_cells,
                    body: body_width_columns,
                    field_width_columns,
                    flags,
                }),
                AuthorBreakEffect::Line => None,
            })
    }

    fn finish_fill_mode_open_row(
        &mut self,
        body: u16,
        field_width_columns: u16,
        gap: u8,
        flags: FieldFlags,
        width: usize,
        native: Option<&NativeFieldDevice>,
    ) {
        if flags.contains(FieldFlag::Hang) {
            // HANG kept the row open through the boundary
            // (roff_term.c:76): the following word aligns through
            // `vbl = offset - viscol` at its print (term.c:113-114).
            // Upstream has no `body - width` fixed padding on this
            // row — a deferred print past the element restore zeroes
            // the offset (mdoc_term.c:437-439) and the fill collapses,
            // which the armed-offset state machine carries.
            self.definition_state_mut()
                .row
                .arm_jump(field_width_columns);
            // term_newln() already printed this accepted field under
            // the old HANG flags. Keep that captured position while
            // retiring its cells, even though the physical row stays
            // open; changing fill mode does not print them a second time.
            if let Some(native) = native {
                let row = &mut self.definition_state_mut().hang_row;
                row.flush(usize::from(gap));
                row.viscol = native.viscol;
            }
            self.retire_native_field_with_device(native);
            self.reset_native_tab_origin();
        } else {
            self.append_fixed_cells(usize::from(body).saturating_sub(width));
        }
    }

    /// Execute a visited empty TEXT at its actual node position. Native
    /// `print_man_node()`/`print_mdoc_node()` call `term_newln()` for an active \c;
    /// otherwise `term_vspace()` consumes skipvsp before emitting a blank row.
    pub(in crate::mandoc) fn execute_visited_empty_text(&mut self) {
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
            self.native_vertical_space(1);
        }
    }

    /// Execute an inline vertical-space request without retaining its
    /// numeric operand as document text.  `term_vspace(n)` first closes an
    /// occupied row, then emits `n` empty rows.
    // term_vspace() must settle the active field before asserting rows.
    #[allow(clippy::too_many_lines)]
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
            self.note_field_control_cleared_no_break(
                flags.contains(FieldFlag::Brind),
                field_width_columns,
            );
            if !self.has_formatter_cell() {
                // `term_vspace()` always emits its requested empty row, but
                // its leading `term_newln()` leaves a bare BACKAFTER armed
                // when neither tcol nor viscol is occupied.  The following
                // BRIND phase still ends a tag field; HANG keeps its run-in
                // body contract.  Preserve those independent effects.
                // term.c:475-480,489-497: the conditional term_newln()
                // emits nothing with no buffered cell or occupied row.
                // Only the requested endline events exist; an IR helper
                // return must not contribute another row close.
                self.retain_line_breaks(rows);
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
                return;
            }
            // `roff_term_pre_sp()` executes term_vspace() before the final
            // BRIND transition. HANG can suppress term_newln(), but the
            // vertical request still ends the row; field padding is trailing
            // geometry and must not leak onto the empty row.
            // roff_term_pre_sp() executes term_vspace() before the final
            // BRIND transition. The CVS-pinned occupied-head rows keep
            // term_newln()'s close consuming the first requested row for
            // wrappable fields; HANG suppresses that close only when the
            // field did not overrun (term.c:250-252).
            let start = self
                .author_execution
                .as_ref()
                .map_or(self.nodes.len(), |execution| execution.field_output_start);
            self.flush_zero_advance();
            let width = mant_ir::geometry::text_width(&super::super::super::plain_text(
                self.nodes.get(start..).unwrap_or_default(),
            ));
            let trailspace = self
                .author_execution
                .as_ref()
                .map_or(0, |execution| match execution.break_effect {
                    AuthorBreakEffect::Field { gap_cells, .. } => usize::from(gap_cells),
                    AuthorBreakEffect::Line => 0,
                });
            let term_newln_ended_row =
                flags.wraps() && width.saturating_add(trailspace) > usize::from(body_width_columns);
            self.hard_break();
            self.retain_line_breaks(if term_newln_ended_row {
                rows
            } else {
                rows.saturating_sub(1)
            });
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
        } else {
            self.hard_break();
            self.retain_line_breaks(rows);
        }
        self.finish_native_vertical_row(rows);
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
        self.no_fill_source_line();
        let rows = self.execution.resolve_vertical_space(i32::from(rows));
        self.retain_line_breaks(usize::from(rows));
        self.finish_native_vertical_row(usize::from(rows));
        self.asserted_vertical_row |= rows > 0;
        // term_vspace() already emitted these empty rows after resolving
        // skipvsp. They survive an output-owner return independently of
        // the ordinary row end from its leading term_newln().
        self.execution.completed_vertical_rows =
            self.execution.completed_vertical_rows.saturating_add(rows);
    }

    /// A real `term_flushln()` commits this input field irreversibly. Keep
    /// device viscol/minbl and the enclosing BODY lifetime, but retire its
    /// cells and projection ranges before another formatter word executes.
    pub(super) fn retire_consumed_native_field(&mut self) {
        let device = self.native_field_device(false);
        self.retire_native_field_with_device(device.as_ref());
    }

    pub(super) fn retire_native_field_with_device(&mut self, device: Option<&NativeFieldDevice>) {
        if let Some(state) = &mut self.execution.definition {
            if let Some(device) = device {
                state.field_buffer.set_tab_offset(device.tab_offset);
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
            definition.hang_row.field_offset = field.body_width;
            definition.field_offset_units = definition
                .native_margin_units
                .unwrap_or_else(|| field.body_width.saturating_mul(24));
        }
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

    /// Collapse a jump still uncommitted at the item post: the element
    /// restore already zeroed the offset (mdoc_term.c:437-439), so the
    /// buffered word prints with `vbl = 0`.
    pub(in crate::mandoc) fn retract_head_close_jump(&mut self) {
        if let Some(definition) = &mut self.execution.definition
            && let Some(jump_node) = definition.row.retract_on_head_close()
            && let Some(Inline::Text { value }) = self.nodes.get_mut(jump_node)
        {
            value.clear();
        }
    }

    /// The row origin survives each flush until a document scope restores it.
    /// CVS `mdoc_term.c::print_mdoc_node()` saves offset after the source-line
    /// event and restores it at non-roff node exit (329, 393-397, 437-439).
    pub(in crate::mandoc) fn take_definition_row_indent(&mut self) -> u16 {
        self.execution
            .definition
            .as_ref()
            .map_or(0, |definition| definition.row.indent_columns)
    }

    pub(in crate::mandoc) fn definition_geometry_checkpoint(
        &self,
        node: &libmandoc_rs::Node,
    ) -> Option<super::DefinitionGeometryCheckpoint> {
        // Roff requests return before the geometry restore. Text restores
        // rmargin only; this ledger records the offset relevant to reading.
        if self.execution.macro_set != libmandoc_rs::MacroSet::Mdoc
            || node.kind == libmandoc_rs::NodeKind::Text
            || node
                .macro_name
                .as_deref()
                .is_some_and(|name| name.as_bytes().first().is_some_and(u8::is_ascii_lowercase))
        {
            return None;
        }
        self.execution
            .definition
            .as_ref()
            .map(|definition| super::DefinitionGeometryCheckpoint {
                indent_columns: definition.row.indent_columns,
                field_offset: definition.hang_row.field_offset,
                field_offset_units: definition.field_offset_units,
                margin_override: definition.margin_override,
            })
    }

    pub(in crate::mandoc) fn restore_definition_geometry(
        &mut self,
        checkpoint: Option<super::DefinitionGeometryCheckpoint>,
    ) {
        if let Some(checkpoint) = checkpoint
            && let Some(definition) = &mut self.execution.definition
        {
            definition.row.indent_columns = checkpoint.indent_columns;
            definition.hang_row.field_offset = checkpoint.field_offset;
            definition.field_offset_units = checkpoint.field_offset_units;
            definition.margin_override = checkpoint.margin_override;
        }
    }

    /// Assign origin when the buffered row actually prints. The last word
    /// can print after its enclosing scope restored offset, while earlier
    /// rows have already been committed by source-line events.
    pub(in crate::mandoc) fn commit_definition_row_origin(&mut self) {
        fn set_last_break(nodes: &mut [Inline], origin: u16) -> bool {
            for node in nodes.iter_mut().rev() {
                match node {
                    Inline::LineBreak { indent_columns } => {
                        *indent_columns = origin;
                        return true;
                    }
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => {
                        if set_last_break(children, origin) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
            false
        }
        if self.has_formatter_cell()
            && let Some(definition) = &self.execution.definition
            && definition.row.has_pending_origin()
            && definition.field_buffer.resume_offset() < definition.field_buffer.cells().len()
        {
            set_last_break(&mut self.nodes, definition.row.indent_columns);
            self.definition_state_mut().row.retire_row_origin();
        }
    }

    pub(in crate::mandoc) fn note_definition_output_row(&mut self) {
        if let Some(definition) = &mut self.execution.definition {
            definition.row.note_row_origin();
        }
    }

    pub(in crate::mandoc) fn force_output_line_break(&mut self) {
        // Consume the pending row indent even when a break already sits at
        // the tail: the boundary moved the upstream row origin regardless
        // (roff_term.c:73-75), and a leaked indent would misplace a later
        // row.
        let row_indent = self.take_definition_row_indent();
        if matches!(self.nodes.last(), Some(Inline::LineBreak { .. })) {
            return;
        }
        self.flush_zero_advance();
        self.nodes.push(Inline::line_break_indented(row_indent));
        self.note_definition_output_row();
        self.execution.last_visible_character = Some('\n');
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.boundary = PendingBoundary::Ordinary;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
        self.execution.formatter_column = FormatterColumn::Origin;
    }
}

#[cfg(test)]
mod recording_tests {
    use super::*;

    #[test]
    fn definition_field_session_reenters_after_owner_handoff() {
        let mut builder = InlineBuilder::with_spacing(true);
        builder.inherit_author_execution_with_effect(
            crate::mandoc::formatter::AuthorFlow::default(),
            false,
            AuthorBreakEffect::Field {
                gap_cells: 1,
                body_width_columns: 6,
                field_width_columns: 4,
                flags: FieldFlags::hang(),
            },
        );
        builder.execution.definition = None;
        assert!(!builder.in_definition_field());
        builder.ensure_definition_field_session();
        assert!(builder.in_definition_field());
    }
}
