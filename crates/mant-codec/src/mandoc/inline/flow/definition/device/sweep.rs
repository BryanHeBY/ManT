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

struct PresentedPass {
    print: NativeFieldPrint,
    placement: super::super::super::presentation::FieldPlacement,
    width: usize,
    margin: usize,
}

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
            presentation: self.execution.row_presentation,
            partial_alignment: false,
            cleared_alignment: None,
            row_presentations: Vec::new(),
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
        let mut row_origin_units = state.field_offset_units;
        let mut placement = sweep.presentation.field_placement(
            row_origin_units,
            sweep.viscol,
            row.minbl,
            sweep.printed_row.is_some(),
        );
        let mut start = 0;
        let mut vfield = placement.capacity(rmargin);
        let mut width = 0;
        let mut width_units = 0;
        let mut tab_offset = state.field_buffer.tab_offset();
        for (index, pass) in passes.iter().enumerate() {
            vfield = placement.capacity(rmargin);
            let tab_target = if no_break {
                // The responsive reading projection has no screen margin.
                usize::MAX / 2
            } else {
                vfield
            };
            width = pass.width;
            width_units = pass.units;
            self.print_presented_native_pass(
                &mut sweep,
                &PresentedPass {
                    print: NativeFieldPrint {
                        start,
                        end: pass.end,
                        first: index == 0,
                        padding_units: placement.projection_padding,
                        origin_units: row_origin_units,
                        tab_offset,
                    },
                    placement,
                    width: width_units,
                    margin: if no_break { usize::MAX / 2 } else { rmargin },
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
                row_origin_units = end_native_loop_pass(&mut sweep, pass, state, flags, rmargin);
                placement = sweep
                    .presentation
                    .field_placement(row_origin_units, 0, 0, false);
            }
        }
        if rejected {
            width = 0;
            width_units = 0;
            vfield = placement.capacity(rmargin);
        }
        NativePassSweep {
            sweep,
            width,
            width_units,
            tab_offset,
            vfield,
        }
    }

    fn print_presented_native_pass(
        &self,
        sweep: &mut NativeFieldSweep,
        pass: &PresentedPass,
        resumed: Option<NoBreakField>,
    ) {
        let state = self.execution.definition.as_ref().expect("field session");
        let fresh = sweep.printed_row.is_none();
        if fresh
            && let Some(receipt) = super::super::super::presentation::pass_receipt(
                &state.field_buffer,
                &state.field_word_anchors,
                pass.print.start,
                pass.print.end,
                sweep
                    .presentation
                    .hint_at(self.execution.text_alignment, pass.print.origin_units),
            )
        {
            sweep.row_presentations.push(receipt);
        }
        let old_presentation = sweep.presentation;
        let placement = pass.placement.aligned(
            self.execution.text_alignment,
            pass.width,
            pass.margin,
            sweep.presentation.parent,
            !fresh,
        );
        sweep.presentation.record_field_placement(placement);
        let print = NativeFieldPrint {
            padding_units: placement.projection_padding,
            ..pass.print
        };
        if !self.print_native_field_pass(sweep, &print, resumed) {
            // Graphless NBRZW/rejected cells do not call device advance().
            sweep.presentation = old_presentation;
        } else if fresh {
            if let Some(receipt) = sweep.row_presentations.last() {
                sweep.presentation.remember_alignment(receipt);
            }
        } else if self.execution.text_alignment != mant_ir::TextAlignment::Left {
            // Whole-row alignment cannot reposition this accepted suffix.
            sweep.partial_alignment = true;
            sweep.cleared_alignment = sweep.presentation.retire_partial_alignment();
        }
    }
}

/// term.c:217-228: the loop endline is real even while HANG keeps
/// the final tail open. BRIND chooses only the following print origin.
fn end_native_loop_pass(
    sweep: &mut NativeFieldSweep,
    pass: &super::super::super::field_buffer::FillPass,
    state: &DefinitionFieldState,
    flags: FieldFlags,
    rmargin: usize,
) -> usize {
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
    sweep.presentation.endline_at(state.field_offset_units);
    if flags.contains(FieldFlag::Brind) {
        rmargin
    } else {
        state.field_offset_units
    }
}
