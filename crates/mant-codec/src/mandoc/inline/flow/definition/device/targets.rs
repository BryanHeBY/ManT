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

//! Native field flags, fill targets and the final tail-space decision.

use super::super::super::native_field::{FieldFlag, FieldFlags};
use super::super::super::{AuthorBreakEffect, InlineBuilder};
use super::super::state::NoBreakField;

impl InlineBuilder {
    pub(in crate::mandoc::inline::flow::definition) fn native_field_parameters(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<(FieldFlags, usize, usize)> {
        let state = self.execution.definition.as_ref()?;
        let (flags, margin, trailspace) =
            match self.execution.author_execution.as_ref()?.break_effect {
                AuthorBreakEffect::Field {
                    flags: field_flags,
                    body_width_columns,
                    gap_cells,
                    ..
                } if !state.head_flags_cleared => (
                    field_flags,
                    usize::from(body_width_columns),
                    usize::from(gap_cells),
                ),
                // The run-in HEAD post already cleared the field flags
                // (mdoc_term.c:961-962) while keeping the shared buffer
                // (939-945): a later flush over those cells decides with
                // no BRIND restart, no HANG, and no trailspace, against
                // the ambient margin the It node restored.
                AuthorBreakEffect::Field {
                    body_width_columns, ..
                } => (FieldFlags::inset(), usize::from(body_width_columns), 0),
                AuthorBreakEffect::Line => match state.no_break.or(resumed) {
                    Some(field) => (field.flags, field.body_width, field.trailspace_cells),
                    // `.mc` recomputes an ordinary row with NOBREAK held on
                    // (roff_term.c:147-150): its vtarget and vfield are the
                    // page margin (term.c:134-136), not a field width.
                    None if force_no_break => (FieldFlags::inset(), usize::MAX / 2, 0),
                    None => (
                        FieldFlags::inset(),
                        if state.cleared_field_capacity_columns > 0 {
                            usize::from(state.cleared_field_capacity_columns)
                        } else {
                            usize::MAX / 2
                        },
                        0,
                    ),
                },
            };
        let flags = if state.no_break_cleared {
            flags.without(FieldFlag::NoBreak).without(FieldFlag::Brind)
        } else {
            flags
        };
        Some((flags, state.margin_override.unwrap_or(margin), trailspace))
    }

    pub(super) fn native_no_break(
        &self,
        flags: FieldFlags,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> bool {
        let state = self.execution.definition.as_ref().expect("field session");
        force_no_break
            || flags.contains(FieldFlag::NoBreak) && !state.no_break_cleared
                && state.no_break.is_none() && resumed.is_none()
                // It HEAD post clears NOBREAK before the shared BODY executes
                // (mdoc_term.c:961-963). Owner transfer preserves cells only.
                && !state.run_in_continuation
                && self.execution.author_execution.as_ref().is_some_and(|author| matches!(author.break_effect, AuthorBreakEffect::Field { .. }))
    }

    pub(super) fn native_margin_units(&self, fallback_columns: usize) -> usize {
        let state = self.execution.definition.as_ref().expect("field session");
        state
            .margin_override
            .map(|columns| columns.saturating_mul(24))
            .or(state.native_margin_units)
            .unwrap_or_else(|| fallback_columns.saturating_mul(24))
    }

    pub(in crate::mandoc::inline::flow) fn native_field_targets(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<super::super::super::field_buffer::FillTargets> {
        let state = self.execution.definition.as_ref()?;
        let (flags, rmargin, _) = self.native_field_parameters(force_no_break, resumed)?;
        let row = &state.hang_row;
        let rmargin = self.native_margin_units(rmargin);
        let viscol = row.viscol.saturating_mul(24);
        let vbl = row.padding_units(state.field_offset_units);
        let no_break = self.native_no_break(flags, force_no_break, resumed);
        // term.c:113-137,229-230 computes each pass from its current device
        // position. Zero is a real target: a leading blank can reject the
        // whole remainder. A prior flush or IR owner does not restore width.
        Some(super::super::super::field_buffer::FillTargets {
            first: if no_break {
                usize::MAX / 2
            } else {
                rmargin.saturating_sub(viscol.saturating_add(vbl))
            },
            rest: if no_break {
                usize::MAX / 2
            } else {
                rmargin.saturating_sub(if flags.contains(FieldFlag::Brind) {
                    rmargin
                } else {
                    state.field_offset_units
                })
            },
            // The frozen reading contract keeps source words after an
            // overrun HANG margin flush instead of deleting them solely
            // because the device has no horizontal budget left. Authored
            // break markers still use the ordered CVS rejection rule. Keep
            // the actual targets for taboff and row geometry in both cases.
            unbounded: self.execution.no_fill_word_active
                || flags.contains(FieldFlag::Hang)
                    && row.margin_flush_seen
                    && !state.field_buffer.has_break_markers(),
        })
    }

    pub(super) fn native_field_final_vbr(
        &self,
        rejected: bool,
        end: usize,
        width_units: usize,
        flags: FieldFlags,
        native_cells: &[super::super::super::field_buffer::FieldCell],
    ) -> usize {
        if rejected {
            0
        } else {
            self.execution
                .definition
                .as_ref()
                .expect("field session")
                .field_buffer
                .brtrsp_tail_sweep(
                    native_cells,
                    end,
                    width_units,
                    flags.contains(FieldFlag::BrTrsp),
                )
        }
    }
}
