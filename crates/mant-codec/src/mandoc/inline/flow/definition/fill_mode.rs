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

//! Filled/no-fill transitions over the current native field and open row.

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{
    AuthorBreakEffect, Inline, InlineBuilder, PendingBoundary, has_printable_character,
};
use super::device::NativeFieldDevice;

pub(super) struct ActiveDefinitionField {
    start: usize,
    gap: u8,
    body: u16,
    field_width_columns: u16,
    pub(super) flags: FieldFlags,
}

impl InlineBuilder {
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

    pub(super) fn active_definition_field(&self) -> Option<ActiveDefinitionField> {
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
}
