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

//! Ordered native field passes and their real loop-endline receipts.

use super::super::super::InlineBuilder;
use super::super::super::field_buffer::{FillBoundary, FlushReceipt};
use super::super::super::native_field::{FieldFlag, FieldFlags};
use super::super::state::{DefinitionFieldState, NoBreakField};
use super::{
    LoopRowEnd, NativeFieldEmission, NativeFieldPrint, NativeFieldSweep, NativeLoopRow,
    NativePassSweep,
};

impl InlineBuilder {
    /// Run the `term_flushln()` pass loop (term.c:143-230) over one
    /// receipt: print each accepted pass at its computed origin, execute
    /// the loop `endline()` while a genuine remaining field exists, and
    /// keep BRIND's restart origin for the following pass.
    pub(super) fn sweep_native_field_passes(
        &self,
        state: &DefinitionFieldState,
        receipt: &FlushReceipt,
        flags: FieldFlags,
        no_break: bool,
        rmargin: usize,
        resumed: Option<NoBreakField>,
    ) -> NativePassSweep {
        use super::super::super::field_buffer::FieldCell;
        let (passes, rejected) = match receipt {
            FlushReceipt::Accepted { passes, .. } => (passes.as_slice(), false),
            FlushReceipt::Rejected { passes, .. } => (passes.as_slice(), true),
        };
        let row = &state.hang_row;
        let mut sweep = NativeFieldSweep {
            viscol: row.viscol,
            printed_row: row.native_row_occupied().then_some(row.viscol),
            emission: NativeFieldEmission::Unprinted,
            separator_retention: None,
            row_origins: Vec::new(),
            field_padding: Vec::new(),
            unprojected_origin_units: row.unprojected_origin_units,
            loop_row_end: LoopRowEnd::Open,
            loop_rows: Vec::new(),
        };
        let mut vbl = row.padding_units(state.field_offset_units);
        let mut row_origin_units = state.field_offset_units;
        let mut start = 0;
        let mut vfield =
            rmargin.saturating_sub(sweep.viscol.saturating_mul(24).saturating_add(vbl));
        let mut width = 0;
        let mut width_units = 0;
        let mut tab_offset = state.field_buffer.tab_offset();
        for (index, pass) in passes.iter().enumerate() {
            vfield = rmargin.saturating_sub(sweep.viscol.saturating_mul(24).saturating_add(vbl));
            let tab_target = if no_break {
                // The responsive reading projection has no screen margin.
                usize::MAX / 2
            } else {
                vfield
            };
            width = pass.width;
            width_units = pass.units;
            self.print_native_field_pass(
                &mut sweep,
                &NativeFieldPrint {
                    start,
                    end: pass.end,
                    first: index == 0,
                    padding_units: vbl,
                    origin_units: row_origin_units,
                    tab_offset,
                },
                resumed,
            );
            tab_offset = tab_offset
                // term.c:165-168 clamps to the actual vtarget, even if
                // BRNEVER allowed an unbounded term_fill() pass.
                .saturating_add(i64::try_from(width_units.min(tab_target)).unwrap_or(i64::MAX))
                .saturating_add(24);
            start = pass.end;
            while matches!(
                receipt.native_cells().get(start),
                Some(FieldCell::BreakableBlank)
            ) {
                start += 1;
            }
            if index + 1 < passes.len() || rejected {
                // A genuine remaining field executes loop endline(), then
                // BRIND selects its right-margin origin for the next pass.
                // That endline ends the row this pass printed when the
                // boundary came from an authored `\p` marker (term.c:294-
                // 305 armed breakline; 217 endline); a device-width guess
                // (vn > vtarget) stays responsive placement instead.
                sweep.loop_rows.push(NativeLoopRow {
                    end_cell: pass.end,
                    printed: sweep.printed_row.is_some(),
                    boundary: pass.boundary,
                });
                sweep.loop_row_end = if pass.boundary == FillBoundary::WordEndBreak {
                    LoopRowEnd::Authored
                } else {
                    LoopRowEnd::Responsive
                };
                sweep.viscol = 0;
                sweep.printed_row = None;
                sweep.unprojected_origin_units = 0;
                row_origin_units = if flags.contains(FieldFlag::Brind) {
                    rmargin
                } else {
                    state.field_offset_units
                };
                vbl = row_origin_units;
            }
        }
        if rejected {
            width = 0;
            width_units = 0;
            vfield = rmargin.saturating_sub(sweep.viscol.saturating_mul(24).saturating_add(vbl));
        }
        NativePassSweep {
            sweep,
            width,
            width_units,
            tab_offset,
            vfield,
        }
    }
}
