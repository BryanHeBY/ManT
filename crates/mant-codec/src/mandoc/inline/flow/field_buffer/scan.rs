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

//! Incremental `term_fill()` scanning over the one live field buffer.

use std::collections::BTreeMap;

use super::{
    EN, FieldBuffer, FieldCell, FillBoundary, FillPass, FillRegisters, FillScanner, PassStop,
    advance_tab, cell_units, finish_pass, signed,
};

impl FieldBuffer {
    /// Additional explicit rows of one tbl cell. A nonempty remainder
    /// advances the outer multicollumn loop even if its next pass rejects.
    /// Ordinary responsive cells scan without a bound. A raw tbl word
    /// with device writes can carry a native fill target for its markers.
    pub(in crate::mandoc::inline::flow) fn table_word_endlines(
        &mut self,
        table_field_units: Option<usize>,
    ) -> usize {
        let mut rows = 0;
        let target = table_field_units.unwrap_or(usize::MAX / 4);
        while let Some(pass) = self.scan_fill_pass(target, true) {
            // term.c::term_flushln's suffix sweep precedes the MULTICOL
            // return. Markers, breakpoints and NBRZW do not keep it alive.
            if !self.has_non_ignorable_after(pass.end, false) {
                break;
            }
            rows += 1;
            self.advance_past(pass.end);
            self.consume_break_blanks();
        }
        rows
    }

    /// One `term_fill()` pass from `resume` (term.c:263-367).
    ///
    /// Returns `None` for the nbr=0 rejection (143-146): nothing in this
    /// slice may print and the caller wipes the remainder. Otherwise the
    /// accepted slice is `cells[start..end]`; the breaking blank (if
    /// any) sits at `accepted_end` and is consumed separately by
    /// [`Self::consume_break_blanks`] (term.c:205-207).
    /// Continue a pass from its last inspected cell. Appending a word does
    /// not restart `term_fill()` over the cumulative field. The last word is
    /// provisional; only a real stop commits nbr. BACKBEFORE can rewrite the
    /// final blank, so keep the checkpoint immediately before that cell.
    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn fill_pass(
        &mut self,
        vtarget: usize,
    ) -> Option<FillPass> {
        self.scan_fill_pass(vtarget.saturating_mul(EN), true)
    }

    pub(in crate::mandoc::inline::flow) fn fill_pass_units(
        &mut self,
        vtarget: usize,
    ) -> Option<FillPass> {
        self.scan_fill_pass(vtarget, false)
    }

    pub(super) fn scan_fill_pass(&mut self, vtarget: usize, native: bool) -> Option<FillPass> {
        self.scan_fill_pass_with_motion(vtarget, native, None)
    }

    pub(super) fn scan_fill_pass_with_motion(
        &mut self,
        vtarget: usize,
        native: bool,
        partial_motion: Option<&mut BTreeMap<usize, usize>>,
    ) -> Option<FillPass> {
        let mut accepted = self.scan_fill_pass_at_cursor(vtarget, native, partial_motion)?;
        accepted.start = self.resume;
        Some(accepted)
    }

    fn scan_fill_pass_at_cursor(
        &mut self,
        vtarget: usize,
        native: bool,
        mut partial_motion: Option<&mut BTreeMap<usize, usize>>,
    ) -> Option<FillPass> {
        // term_fill() compares basic units with half an EN of tolerance.
        let vtarget = vtarget.saturating_add(EN / 2);
        self.prepare_fill_scan(vtarget);
        let scan = self.scan.as_mut().expect("initialized field scan");
        if let Some(result) = scan.stopped {
            return result.accepted();
        }
        while scan.registers.index < self.cells.len() {
            #[cfg(test)]
            {
                self.scan_work += 1;
            }
            scan.before_last = scan.registers;
            let registers = &mut scan.registers;
            let ic = registers.index;
            scan.scanned_end = scan.scanned_end.max(ic + 1);
            let cell = self.cells[ic].clone();
            let mut normalize = native;
            let result = if let FieldCell::FixedDrawingAdvance(count) = cell
                && let Some(partial) = partial_motion.as_deref_mut()
            {
                let previous = partial.remove(&ic).unwrap_or_default();
                let (stop, visited) =
                    registers.consume_motion(count, previous, vtarget, &self.tabs);
                if visited < count {
                    normalize = false;
                    partial.insert(ic, visited);
                }
                stop
            } else {
                let previous_units = if cell == FieldCell::Backline {
                    ic.checked_sub(1)
                        .and_then(|previous| self.cells.get(previous))
                        .map_or(0, cell_units)
                } else {
                    0
                };
                registers.consume_cell(&cell, 1, previous_units, vtarget, &self.tabs)
            };
            if normalize {
                Self::normalize_scanned_cell(
                    &mut self.cells[ic],
                    ic,
                    &mut self.nonbreaking_positions,
                    &mut self.blank_positions,
                );
                self.normalized_until = self.normalized_until.max(ic + 1);
            }
            if let Some(stop) = result {
                scan.stopped = Some(stop);
                return stop.accepted();
            }
        }
        finish_pass(scan.registers, vtarget)
    }

    fn prepare_fill_scan(&mut self, vtarget: usize) {
        if self.scan.as_ref().is_none_or(|scan| scan.target != vtarget) {
            let registers = FillRegisters {
                index: self.resume,
                tab_offset: self.pass_tab_offset,
                ..FillRegisters::default()
            };
            self.scan = Some(FillScanner {
                target: vtarget,
                registers,
                before_last: registers,
                stopped: None,
                scanned_end: self.resume,
            });
        }
    }

    pub(super) fn invalidate_projection_scan(&mut self, position: usize) {
        if self
            .proved_rejection
            .is_some_and(|rejection| position < rejection.scanned_end)
        {
            self.proved_rejection = None;
        }
        let Some(scan) = &mut self.scan else {
            return;
        };
        if position + 1 == scan.registers.index {
            scan.registers = scan.before_last;
            scan.stopped = None;
        } else if position < scan.registers.index {
            self.scan = None;
        } else if position == scan.registers.index {
            scan.stopped = None;
        }
    }
}

/// The accepted first pass of a tbl column. A fixed blank can take up
/// native width without being printed by `term_field()`.
#[derive(Default)]
pub(in crate::mandoc::inline::flow) struct CompactPass {
    pub(in crate::mandoc::inline::flow) units: usize,
    pub(in crate::mandoc::inline::flow) prints_ink: bool,
}

/// The same first-pass decision for weighted tbl scratch cells. Motions
/// can write a long repeated run without materializing each native slot.
pub(in crate::mandoc::inline::flow) fn compact_first_pass(
    cells: impl IntoIterator<Item = (FieldCell, usize)>,
    target: usize,
    tabs: &super::TabStops,
) -> CompactPass {
    let target = target.saturating_add(EN / 2);
    let mut registers = FillRegisters::default();
    let mut previous_units = 0;
    let mut first_ink = usize::MAX;
    let mut stopped = None;
    for (cell, count) in cells {
        if first_ink == usize::MAX
            && (matches!(cell, FieldCell::Graph { text, .. } if !text.is_whitespace())
                || matches!(cell, FieldCell::Hyphen | FieldCell::Backline))
        {
            first_ink = registers.index;
        }
        if let Some(stop) = registers.consume_cell(&cell, count, previous_units, target, tabs) {
            stopped = Some(stop);
            break;
        }
        previous_units = cell_units(&cell);
    }
    stopped
        .map_or_else(|| finish_pass(registers, target), PassStop::accepted)
        .map_or_else(CompactPass::default, |pass| CompactPass {
            units: pass.units,
            prints_ink: first_ink < pass.end,
        })
}

impl FillRegisters {
    fn consume_motion(
        &mut self,
        count: usize,
        previous: usize,
        target: usize,
        tabs: &super::TabStops,
    ) -> (Option<PassStop>, usize) {
        let index = self.index;
        if previous != 0 {
            if let Some(stop) =
                self.consume_cell(&FieldCell::DrawnSpaces(previous), 1, 0, target, tabs)
            {
                return (Some(stop), previous);
            }
            self.index = index;
        }
        // term_fill() normalizes each reached NBRSP before testing the
        // width guard. An early return leaves the rest of the run intact.
        let remaining = count - previous;
        let visited = if self.nbr > 0 {
            remaining.min(target.saturating_sub(self.vis).saturating_div(EN) + 1)
        } else {
            remaining
        };
        let stop = self.consume_cell(&FieldCell::FixedDrawingAdvance(visited), 1, 0, target, tabs);
        (stop, previous + visited)
    }

    fn consume_cell(
        &mut self,
        cell: &FieldCell,
        count: usize,
        previous_units: usize,
        target: usize,
        tabs: &super::TabStops,
    ) -> Option<PassStop> {
        match cell {
            FieldCell::Backline => self.vis = self.vis.saturating_sub(previous_units),
            FieldCell::BreakableBlank | FieldCell::DrawnSpaces(_) | FieldCell::Breakpoint => {
                let units = cell_units(cell);
                let first = units.min(EN);
                if self.breakline || self.vis.saturating_add(first) > target {
                    let boundary = if self.breakline {
                        FillBoundary::WordEndBreak
                    } else {
                        FillBoundary::Width
                    };
                    return Some(PassStop::from(
                        finish_pass(*self, target).map(|pass| pass.stopped_at(boundary)),
                    ));
                }
                if self.graph {
                    self.nbr = self.index;
                    self.vbr = self.vis;
                    self.graph = false;
                    self.break_candidate = Some(FillBoundary::Width);
                }
                self.vis = self.vis.saturating_add(units.saturating_mul(count));
                if self.vis > target {
                    return Some(PassStop::from(
                        finish_pass(*self, target).map(|pass| pass.stopped_at(FillBoundary::Width)),
                    ));
                }
            }
            FieldCell::BreakMarker => self.breakline = true,
            FieldCell::Hyphen => {
                if let Some(stop) = self.consume_hyphen(target) {
                    return Some(stop);
                }
            }
            FieldCell::ZeroWidthGraph => self.graph = true,
            FieldCell::NonBreakingBlank
            | FieldCell::FixedDrawingAdvance(_)
            | FieldCell::Graph { .. } => {
                self.vis = self
                    .vis
                    .saturating_add(cell_units(cell).saturating_mul(count));
                self.graph = true;
            }
            FieldCell::Tab => {
                self.vis = advance_tab(tabs, self.vis, self.tab_offset);
                self.graph = true;
            }
            FieldCell::TabReference => {
                self.tab_offset = -signed(self.vis).saturating_add(signed(EN));
            }
        }
        if self.graph
            && self.vis > target
            && self.nbr > 0
            && matches!(
                cell,
                FieldCell::NonBreakingBlank
                    | FieldCell::FixedDrawingAdvance(_)
                    | FieldCell::Graph { .. }
                    | FieldCell::Tab
            )
        {
            return Some(PassStop::Accepted(
                FillPass::new(self.nbr, self.vbr)
                    .stopped_at(self.break_candidate.unwrap_or(FillBoundary::Width)),
            ));
        }
        self.index = self.index.saturating_add(count);
        None
    }
}
