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

//! Native field targets and the numeric device receipt of a real flush.

use super::super::native_field::{FieldFlag, FieldFlags};
use super::super::{AuthorBreakEffect, InlineBuilder};
use super::state::{DefinitionFieldStyle, NoBreakField};

/// Device facts of one real `term_flushln()`, independent of its semantic
/// owner or of whether a Link/style wrapper contains the printed glyphs.
///
/// `width` is the last printed pass's fill width; `overruns` and `ends_row`
/// already carry the term.c:250-253 decision over the sweep-widened `vbr`
/// (computed inside `native_field_device_with_resume`).
pub(super) struct NativeFieldDevice {
    pub(super) width: usize,
    pub(super) viscol: usize,
    pub(super) ends_row: bool,
    pub(super) overruns: bool,
    pub(super) final_pass_continued: bool,
    pub(super) tab_offset: i64,
}

impl InlineBuilder {
    /// Execute the numeric pass/print/tail rules from term.c:113-253 and
    /// term_field():374-444. Semantic recovery and hidden URI projection
    /// cannot establish native width or device occupancy.
    pub(super) fn native_field_device(&self, force_no_break: bool) -> Option<NativeFieldDevice> {
        self.native_field_device_with_resume(force_no_break, None)
    }

    pub(in crate::mandoc::inline::flow) fn native_field_row_ends(&self) -> bool {
        self.native_field_device(false)
            .is_some_and(|field| field.ends_row)
    }

    fn native_field_parameters(
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
                    Some(field) => (
                        match field.style {
                            DefinitionFieldStyle::Tag => FieldFlags::tag(false),
                            DefinitionFieldStyle::Hang => FieldFlags::hang(),
                        },
                        field.body_width,
                        field.trailspace_cells,
                    ),
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

    fn native_no_break(
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

    fn native_margin_units(&self, fallback_columns: usize) -> usize {
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
    ) -> Option<super::super::field_buffer::FillTargets> {
        let state = self.execution.definition.as_ref()?;
        let (flags, rmargin, _) = self.native_field_parameters(force_no_break, resumed)?;
        let row = &state.hang_row;
        let rmargin = self.native_margin_units(rmargin);
        let viscol = row.viscol.saturating_mul(24);
        let vbl = state
            .field_offset_units
            .saturating_sub(viscol)
            .max(row.minbl.saturating_mul(24));
        let no_break = self.native_no_break(flags, force_no_break, resumed);
        // term.c:113-137,229-230 computes each pass from its current device
        // position. Zero is a real target: a leading blank can reject the
        // whole remainder. A prior flush or IR owner does not restore width.
        Some(super::super::field_buffer::FillTargets {
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

    pub(super) fn native_field_device_with_resume(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<NativeFieldDevice> {
        use super::super::field_buffer::{FieldCell, FlushReceipt};
        let state = self.execution.definition.as_ref()?;
        let targets = self.native_field_targets(force_no_break, resumed)?;
        let receipt = (!state.field_buffer.is_empty())
            .then(|| state.field_buffer.flush_receipt(targets, false))?;
        let (passes, rejected) = match &receipt {
            FlushReceipt::Accepted { passes } => (passes, false),
            FlushReceipt::Rejected { passes, .. } => (passes, true),
        };
        let (flags, rmargin, trailspace) = self.native_field_parameters(force_no_break, resumed)?;
        let no_break = self.native_no_break(flags, force_no_break, resumed);
        let rmargin = self.native_margin_units(rmargin);
        let row = &state.hang_row;
        let mut viscol = row.viscol;
        let mut vbl = state
            .field_offset_units
            .saturating_sub(viscol.saturating_mul(24))
            .max(row.minbl.saturating_mul(24));
        let mut start = 0;
        let mut vfield = rmargin.saturating_sub(viscol.saturating_mul(24).saturating_add(vbl));
        let mut width = 0;
        let mut width_units = 0;
        let mut tab_offset = state.field_buffer.tab_offset();
        for (index, pass) in passes.iter().enumerate() {
            vfield = rmargin.saturating_sub(viscol.saturating_mul(24).saturating_add(vbl));
            let tab_target = if no_break {
                // The responsive reading projection has no screen margin.
                usize::MAX / 2
            } else {
                vfield
            };
            width = pass.width;
            width_units = pass.units;
            if let Some(printed) = state
                .field_buffer
                .printed_columns(start, pass.end, tab_offset, vbl)
            {
                viscol = viscol.saturating_add(printed);
            }
            tab_offset = tab_offset
                // term.c:165-168 clamps to the actual vtarget, even if
                // BRNEVER allowed an unbounded term_fill() pass.
                .saturating_add(i64::try_from(width_units.min(tab_target)).unwrap_or(i64::MAX))
                .saturating_add(24);
            start = pass.end;
            while matches!(
                state.field_buffer.cells().get(start),
                Some(FieldCell::BreakableBlank)
            ) {
                start += 1;
            }
            if index + 1 < passes.len() || rejected {
                // A genuine remaining field executes loop endline(), then
                // BRIND selects its right-margin origin for the next pass.
                viscol = 0;
                vbl = if flags.contains(FieldFlag::Brind) {
                    rmargin
                } else {
                    state.field_offset_units
                };
            }
        }
        if rejected {
            width = 0;
            width_units = 0;
            vfield = rmargin.saturating_sub(viscol.saturating_mul(24).saturating_add(vbl));
        }
        // The nbr==0 pass exits before the tail sweep (term.c:143-146),
        // with its own freshly reset vbr=0. Only an accepted final pass
        // widens vbr over its ignorable tail (177-196). Keep basic units
        // through the half-EN comparison (250-253), before IR rounding.
        let final_vbr = if rejected {
            0
        } else {
            state.field_buffer.brtrsp_tail_sweep(
                passes.last().map_or(0, |pass| pass.end),
                width_units,
                flags.contains(FieldFlag::BrTrsp),
            )
        };
        let overruns =
            final_vbr.saturating_add(trailspace.saturating_mul(24)) > vfield.saturating_add(12);
        let ends_row = !flags.contains(FieldFlag::Hang) && (!no_break || overruns);
        Some(NativeFieldDevice {
            width,
            viscol: if ends_row { 0 } else { viscol },
            ends_row,
            overruns,
            final_pass_continued: passes.len() > 1 && !rejected,
            tab_offset,
        })
    }
}
