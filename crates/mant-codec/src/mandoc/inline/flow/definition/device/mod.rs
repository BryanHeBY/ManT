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

mod print;
mod sweep;
mod targets;
#[cfg(test)]
mod tests;

use super::super::InlineBuilder;
use super::super::field_buffer::{FillBoundary, FlushReceipt};
use super::super::native_field::FieldFlag;
use super::state::NoBreakField;

/// Output of the current `term_field()` sweep, independent of inherited
/// device viscol. NBRZW, rejected and empty cells can remain unprinted.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum NativeFieldEmission {
    Unprinted,
    Printed,
}

/// Numeric state of the current print sweep. This is local to one receipt;
/// persistent formatter flags and cells remain in the execution state.
struct NativeFieldSweep {
    viscol: usize,
    printed_row: Option<usize>,
    emission: NativeFieldEmission,
    separator_retention: Option<usize>,
    row_origins: Vec<(String, usize, usize, bool)>,
    field_padding: Vec<(String, usize, usize, bool)>,
    unprojected_origin_units: usize,
    /// Outcome of the last pass-loop endline (term.c:217) of this flush.
    loop_row_end: LoopRowEnd,
    loop_rows: Vec<NativeLoopRow>,
}

/// One real pass-loop endline, tied to the accepted native cell interval.
/// Acceptance is independent of printing: an NBRZW interval reaches
/// `term_field()` and then `endline()` even when it emits no glyph.
pub(super) struct NativeLoopRow {
    pub(super) end_cell: usize,
    pub(super) printed: bool,
    pub(super) boundary: FillBoundary,
}

/// The last pass-loop `endline()` (term.c:217) of one flush, once its
/// effect on the final device row is known.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum LoopRowEnd {
    /// No endline stayed final: none ran, or a later pass of this flush
    /// printed a graph on the fresh row an earlier endline opened.
    Open,
    /// The final endline ran on a device-width boundary: the following
    /// field keeps responsive placement.
    Responsive,
    /// The final endline closed an accepted row on an authored `\p`
    /// pass boundary (term.c:294-305 armed breakline, 217 endline) and
    /// no later pass printed on the fresh row it opened. Acceptance can
    /// include NBRZW without any printed glyph (term.c:340-349, 397).
    Authored,
}

/// Position at which one accepted interval reaches `term_field()`.
struct NativeFieldPrint {
    start: usize,
    end: usize,
    first: bool,
    padding_units: usize,
    origin_units: usize,
    tab_offset: i64,
}

/// The final field's row decision after its tail-space sweep.
struct NativeFieldTail {
    ends_row: bool,
    overruns: bool,
    trailspace_cells: usize,
}

/// One completed pass sweep: the device row facts plus the fill width,
/// width units, tab reference, and field width the tail decision needs.
struct NativePassSweep {
    sweep: NativeFieldSweep,
    width: usize,
    width_units: usize,
    tab_offset: i64,
    vfield: usize,
}

impl NativeFieldSweep {
    fn into_device(
        self,
        width: usize,
        tab_offset: i64,
        tail: &NativeFieldTail,
        separator_field: Option<NoBreakField>,
        output_start: usize,
        receipt: FlushReceipt,
    ) -> NativeFieldDevice {
        NativeFieldDevice {
            receipt,
            width,
            viscol: if tail.ends_row { 0 } else { self.viscol },
            ends_row: tail.ends_row,
            overruns: tail.overruns,
            next_field_gap_cells: if tail.ends_row {
                0
            } else {
                tail.trailspace_cells
            },
            tab_offset,
            loop_row_end: self.loop_row_end,
            loop_rows: self.loop_rows,
            unprojected_origin_units: self.unprojected_origin_units,
            printed_row: self.printed_row,
            emission: self.emission,
            separator_retention: self.separator_retention,
            separator_field,
            row_origins: self.row_origins,
            field_padding: self.field_padding,
            output_start,
        }
    }
}

/// Device facts of one real `term_flushln()`, independent of its semantic
/// owner or of whether a Link/style wrapper contains the printed glyphs.
///
/// `width` is the last printed pass's fill width; `overruns` and `ends_row`
/// already carry the term.c:250-253 decision over the sweep-widened `vbr`
/// (computed inside `native_field_device_with_resume`).
pub(in crate::mandoc) struct NativeFieldDevice {
    /// Accepted passes and rejected suffix of this exact old-state sweep.
    /// Projection and retirement consume the same decision, never a rescan
    /// after a control changed BRIND/NOBREAK.
    pub(in crate::mandoc::inline::flow) receipt: FlushReceipt,
    pub(super) width: usize,
    pub(super) viscol: usize,
    pub(super) ends_row: bool,
    pub(super) overruns: bool,
    /// `term_flushln()` restores minbl for any consumed buffer, including a
    /// graphless or empty field (term.c:233-253). Backend endline clears it.
    pub(super) next_field_gap_cells: usize,
    pub(super) tab_offset: i64,
    /// Origin advances on the final printed row, including the row just
    /// closed by this flush. Unlike live viscol, this receipt survives the
    /// tail decision long enough for the column owner to deliver its break.
    pub(super) unprojected_origin_units: usize,
    /// Position of an actually printed graph row before this flush's final
    /// tail. `None` means NBRZW/empty/rejection emitted no represented graph.
    pub(super) printed_row: Option<usize>,
    /// This flush printed an accepted encoded graph, rather than merely
    /// inheriting viscol from an earlier field on the same device row.
    pub(super) emission: NativeFieldEmission,
    /// Printed next-field cells left after the accepted row's public origin
    /// already represents its device padding. The automatic native word
    /// blank is separate from that positioning (term.c:113-116,389-427).
    pub(super) separator_retention: Option<usize>,
    /// The consumed field's exact generated range remains available after
    /// a control took its live `NoBreakField` out of the execution state.
    pub(super) separator_field: Option<NoBreakField>,
    /// Stable source-word positions of actually printed new rows. The
    /// projector consumes these only after native acceptance, never at a
    /// tentative word append or from its visible IR width.
    pub(super) row_origins: Vec<(String, usize, usize, bool)>,
    /// Same-row device padding at stable source-word scalar positions.
    /// Only a real accepted print determines these cells, after node geometry
    /// restoration; already materialized field padding is subtracted.
    pub(super) field_padding: Vec<(String, usize, usize, bool)>,
    /// Outcome of the pass loop's last `endline()` (term.c:217). With
    /// [`LoopRowEnd::Authored`], an authored hard row end exists even
    /// though the final tail comparison (250-253) left the fresh row open;
    /// responsive width placement stays with `overruns`/`ends_row`.
    pub(super) loop_row_end: LoopRowEnd,
    /// Completed pass-loop rows in native buffer order (term.c:217),
    /// including accepted zero-width intervals which printed no glyph.
    pub(super) loop_rows: Vec<NativeLoopRow>,
    pub(super) output_start: usize,
}

impl NativeFieldDevice {
    pub(in crate::mandoc::inline::flow) const fn ends_row(&self) -> bool {
        self.ends_row
    }

    pub(in crate::mandoc::inline::flow) fn closes_unprinted_row(&self) -> bool {
        self.ends_row
            && self.emission == NativeFieldEmission::Unprinted
            && self.printed_row.is_none()
    }

    /// The consumed field left a printed device row open for BODY.
    /// `term_flushln()`250-253 can retain that row after its cells retire.
    pub(in crate::mandoc::inline::flow) fn has_occupied_row(&self) -> bool {
        !self.ends_row && self.printed_row.is_some()
    }

    /// The pass loop ended an accepted row on an authored `\p` boundary
    /// and printed nothing after it: the next field starts a new row.
    pub(super) fn row_closed_by_author(&self) -> bool {
        self.loop_row_end == LoopRowEnd::Authored
    }
}

impl InlineBuilder {
    /// Execute the numeric pass/print/tail rules from term.c:113-253 and
    /// term_field():374-444. Semantic recovery and hidden URI projection
    /// cannot establish native width or device occupancy.
    pub(in crate::mandoc::inline::flow) fn native_field_device(
        &self,
        force_no_break: bool,
    ) -> Option<NativeFieldDevice> {
        #[cfg(test)]
        NATIVE_FIELD_DEVICE_VIEWS.with(|views| views.set(views.get().saturating_add(1)));
        self.native_field_device_with_resume(force_no_break, None)
    }

    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn reset_native_field_device_views() {
        NATIVE_FIELD_DEVICE_VIEWS.with(|views| views.set(0));
    }

    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn native_field_device_views() -> usize {
        NATIVE_FIELD_DEVICE_VIEWS.with(std::cell::Cell::get)
    }

    /// Capture the consumed owner's range before acceptance can advance its
    /// output cursor. Numeric/native cells are unchanged by that projection.
    pub(super) fn native_field_device_at(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
        output_start: usize,
    ) -> Option<NativeFieldDevice> {
        self.native_field_device_with_resume(force_no_break, resumed)
            .map(|mut device| {
                device.output_start = output_start;
                device
            })
    }

    /// An ordinary column's NOBREAK overrun is already represented by
    /// declared-field placement. A post with NOBREAK cleared instead closes
    /// the execution row independently of column width (term.c:250-253),
    /// and its following cell needs that explicit row boundary.
    pub(in crate::mandoc::inline::flow) fn native_field_tail_is_unconditional(&self) -> bool {
        self.native_field_parameters(false, None)
            .is_some_and(|(flags, _, _)| {
                !flags.contains(FieldFlag::Hang) && !self.native_no_break(flags, false, None)
            })
    }

    /// A column BODY's unconditional empty `term_flushln()` still executes
    /// the same tail comparison as a nonempty flush (term.c:143-146,233-253).
    pub(in crate::mandoc::inline::flow) fn native_empty_field_row_ends(&self) -> bool {
        self.native_field_device(false)
            .is_none_or(|device| device.ends_row)
    }

    pub(super) fn native_field_device_with_resume(
        &self,
        force_no_break: bool,
        resumed: Option<NoBreakField>,
    ) -> Option<NativeFieldDevice> {
        let state = self.execution.definition.as_ref()?;
        let targets = self.native_field_targets(force_no_break, resumed)?;
        // term_newln() also calls term_flushln() when only sweep.viscol is live.
        // An empty tcol still reaches the same vbr=0 tail comparison
        // (term.c:143-146,233-253); it is not an absent device receipt.
        let receipt = state.field_buffer.flush_receipt(targets, false);
        let (passes, rejected) = match &receipt {
            FlushReceipt::Accepted { passes, .. } => (passes, false),
            FlushReceipt::Rejected { passes, .. } => (passes, true),
        };
        let (flags, rmargin, trailspace) = self.native_field_parameters(force_no_break, resumed)?;
        #[cfg(test)]
        super::control_trace::capture(&receipt, flags);
        let no_break = self.native_no_break(flags, force_no_break, resumed);
        let rmargin = self.native_margin_units(rmargin);
        let NativePassSweep {
            sweep,
            width,
            width_units,
            tab_offset,
            vfield,
        } = self.sweep_native_field_passes(state, &receipt, flags, no_break, rmargin, resumed);
        // The nbr==0 pass exits before the tail sweep (term.c:143-146),
        // with its own freshly reset vbr=0. Only an accepted final pass
        // widens vbr over its ignorable tail (177-196). Keep basic units
        // through the half-EN comparison (250-253), before IR rounding.
        let final_vbr = self.native_field_final_vbr(
            rejected,
            passes.last().map_or(0, |pass| pass.end),
            width_units,
            flags,
            receipt.native_cells(),
        );
        let overruns =
            final_vbr.saturating_add(trailspace.saturating_mul(24)) > vfield.saturating_add(12);
        let ends_row = !flags.contains(FieldFlag::Hang) && (!no_break || overruns);
        #[cfg(test)]
        super::control_trace::device_tail(if ends_row { 0 } else { sweep.viscol }, ends_row);
        Some(sweep.into_device(
            width,
            tab_offset,
            &NativeFieldTail {
                ends_row,
                overruns,
                trailspace_cells: trailspace,
            },
            state.no_break.or(resumed),
            self.native_field_output_start(),
            receipt,
        ))
    }

    pub(super) fn native_field_output_start(&self) -> usize {
        self.execution
            .author_execution
            .as_ref()
            .map_or(self.execution.flush_unit_output_start, |author| {
                author.field_output_start
            })
    }
}

#[cfg(test)]
std::thread_local! {
    static NATIVE_FIELD_DEVICE_VIEWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
