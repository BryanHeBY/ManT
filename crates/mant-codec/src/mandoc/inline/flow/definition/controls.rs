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

    /// The original Bd BODY post sets BRNEVER for literal/unfilled output,
    /// consumes the live buffer, then clears it even when that device row
    /// remains open under NOBREAK (mdoc_term.c:1474-1483).
    pub(in crate::mandoc) fn finish_display_body(
        &mut self,
        display: Option<libmandoc_rs::DisplayKind>,
    ) {
        if matches!(
            display,
            Some(libmandoc_rs::DisplayKind::Literal | libmandoc_rs::DisplayKind::Unfilled)
        ) {
            self.execution.no_fill_word_active = true;
        }
        self.execute_native_newline();
        self.execution.no_fill_word_active = false;
    }

    /// Execute `term_newln()` before any enclosing node restores geometry.
    /// Source `NODE_LINE` and macro posts share this flush; `roff_pre_br()` is
    /// separate because it also changes the field flags and row origin.
    pub(in crate::mandoc) fn execute_native_newline(&mut self) {
        let _ = self.execute_native_newline_with_tail();
    }

    /// Return the consumed native tail to callers with a real post effect.
    /// This is the same execution, never another IR-width observation.
    pub(in crate::mandoc::inline::flow) fn execute_native_newline_with_tail(&mut self) -> bool {
        let _ = self.commit_definition_row_origin();
        self.execution.boundary = PendingBoundary::Tight;
        if self.execution.definition.as_ref().is_some_and(|state| {
            state.field_buffer.is_empty() && !state.hang_row.native_row_occupied()
        }) {
            // term.c::term_newln tests lastcol || viscol before flushing.
            // A second vspace after endline still selects NOSPACE, but may
            // not revive the previous field's padding or reset BACKAFTER.
            self.execution.boundary = PendingBoundary::Tight;
            self.reset_native_tab_origin();
            return false;
        }
        let resumed = self.begin_resumed_native_line();
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
        let ends_row;
        if let Some((start, gap, body, field_width, flags)) = field {
            let native = self.native_field_device_at(false, None, start);
            ends_row = native
                .as_ref()
                .is_some_and(super::NativeFieldDevice::ends_row);
            self.flush_captured_definition_field(
                super::flush::FieldFlush {
                    start,
                    gap,
                    body,
                    width: field_width,
                    flags,
                    boundary: super::flush::FieldFlushBoundary::Continue,
                },
                native,
            );
            if self.execution.pending_field_spaces > 0 {
                self.execution.pending_field_gap_origin = PendingFieldGapOrigin::CommittedFlush;
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
            // A cleared TAG field still calls term_flushln() with the
            // current node's offset before print_mdoc_node restores it
            // (term.c:475-481; mdoc_term.c:437-439). Capture the accepted
            // print positions before hard_break retires its native cells.
            let native = self.native_field_device(false);
            ends_row = native
                .as_ref()
                .is_some_and(super::NativeFieldDevice::ends_row);
            self.hard_break();
            self.retire_native_field_with_device(native.as_ref());
        }
        if let Some(device) = resumed {
            self.finish_resumed_native_line(&device);
        }
        // term_newln() selects NOSPACE before its conditional buffer flush
        // (term.c:475-480). An empty Line owner can return from hard_break()
        // without any output, but that cannot skip the register transition.
        self.execution.boundary = PendingBoundary::Tight;
        self.reset_native_tab_origin();
        ends_row
    }

    /// Execute an explicit formatter line request inside a definition HEAD.
    ///
    /// CVS keeps `LIST_tag/LIST_hang` in a NOBREAK field until `term_newln()`
    /// has settled that field.  A plain `hard_break()` loses BRIND geometry,
    /// so `.br`, `.ti`, and the break phase of `.sp` must use this entrypoint.
    pub(in crate::mandoc) fn control_line_break(&mut self) -> bool {
        let _ = self.commit_definition_row_origin();
        // term_newln selects NOSPACE before its lastcol/viscol test or flush
        // (term.c:475-481). BRIND/NOBREAK still describe the old buffer here.
        self.execution.boundary = PendingBoundary::Tight;
        let resumed = self
            .execution
            .definition
            .as_ref()
            .and_then(|state| state.no_break);
        let configuration = resumed
            .map(|field| super::flush::FieldFlush {
                start: field.resumed_output_start,
                gap: u8::try_from(field.trailspace_cells).unwrap_or(u8::MAX),
                body: u16::try_from(field.body_width).unwrap_or(u16::MAX),
                width: field.field_capacity_columns,
                flags: field.flags,
                boundary: super::flush::FieldFlushBoundary::Continue,
            })
            .or_else(|| {
                self.execution.author_execution.as_ref().and_then(|author| {
                    match author.break_effect {
                        AuthorBreakEffect::Field {
                            gap_cells,
                            body_width_columns,
                            field_width_columns,
                            flags,
                        } => Some(super::flush::FieldFlush {
                            start: author.field_output_start,
                            gap: gap_cells,
                            body: body_width_columns,
                            width: field_width_columns,
                            flags,
                            boundary: super::flush::FieldFlushBoundary::Continue,
                        }),
                        AuthorBreakEffect::Line => None,
                    }
                })
            });
        let Some(mut field) = configuration else {
            let occupied = self.has_formatter_cell();
            self.execute_native_newline();
            return occupied;
        };
        if let Some((flags, _, trailspace)) = self.native_field_parameters(false, resumed) {
            field.flags = flags;
            field.gap = u8::try_from(trailspace).unwrap_or(u8::MAX);
        }
        #[cfg(test)]
        {
            let state = self.execution.definition.as_ref().expect("field session");
            super::control_trace::begin(
                field.flags,
                state.field_buffer.cells().len(),
                state.hang_row.viscol,
                self.execution.boundary == PendingBoundary::Tight,
            );
        }
        let brind = field.flags.contains(FieldFlag::Brind);
        let (body, capacity, old_flags) = (field.body, field.width, field.flags);
        field.boundary = if brind {
            super::flush::FieldFlushBoundary::ExitField
        } else {
            super::flush::FieldFlushBoundary::Continue
        };
        // Capture once while the old field and flags still exist. Projection
        // and the field tail borrow this receipt; the flush owns retirement.
        let occupied = self.execution.definition.as_ref().is_some_and(|state| {
            !state.field_buffer.is_empty() || state.hang_row.native_row_occupied()
        });
        let changed = if occupied {
            let native = self.native_field_device_at(false, resumed, field.start);
            self.flush_captured_definition_field(field, native)
        } else {
            // An empty term_newln has no flush. pre-br nevertheless performs
            // its BRIND post below; it cannot revive a previous separator.
            self.execution.pending_field_spaces = 0;
            if brind {
                self.exit_empty_definition_field(body, capacity, old_flags);
            }
            false
        };
        self.definition_state_mut().no_break = None;
        if brind {
            self.note_field_control_cleared_no_break(true, capacity);
            self.move_definition_field_origin_to_body(body);
            self.definition_state_mut().pending_indent = None;
        }
        // Neither clearing BRIND nor changing fill mode executes a new word.
        // Preserve an authored NONEWLINE continuation; NOSPACE is independent.
        self.execution.boundary = PendingBoundary::Tight;
        self.definition_state_mut()
            .hang_row
            .suppress_next_auto_space = true;
        self.reset_native_tab_origin();
        #[cfg(test)]
        super::control_trace::finish(
            self.native_field_parameters(false, None)
                .map(|value| value.0),
        );
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

    /// Finish `pre_br`'s BRIND geometry after its conditional `term_newln`.
    /// A positive .sp executes backend endlines first; they reset viscol,
    /// then `pre_br` moves offset to rmargin without printing a cell
    /// (roff_term.c:69-78,195-214). That origin stays scoped to this node.
    pub(super) fn move_definition_field_origin_to_body(&mut self, body: u16) {
        let definition = self.definition_state_mut();
        definition.hang_row.field_offset = usize::from(body);
        definition.field_offset_units = definition
            .native_margin_units
            .unwrap_or_else(|| usize::from(body).saturating_mul(24));
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

    /// Arm the next word from the captured open row and zero minbl.
    /// BODY prefers its description origin, while the receipt proves that
    /// reaching that origin adds no cell (term.c:113-116,233-253).
    pub(in crate::mandoc) fn note_flushed_at_body_column(&mut self) {
        self.execution.concat_next_word = true;
        self.execution.concat_flush_source = true;
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
