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

use super::{
    EN, FieldBuffer, FieldCell, FillBoundary, FillPass, FillRegisters, FillScanner, PassStop,
    advance_tab, cell_units, finish_pass, signed,
};

impl FieldBuffer {
    /// One `term_fill()` pass from `resume` (term.c:263-367).
    ///
    /// Returns `None` for the nbr=0 rejection (143-146): nothing in this
    /// slice may print and the caller wipes the remainder. Otherwise the
    /// accepted slice is `cells[..end]`; the breaking blank (if
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
            if native {
                Self::normalize_scanned_cell(
                    &mut self.cells[ic],
                    ic,
                    &mut self.nonbreaking_positions,
                    &mut self.blank_positions,
                );
                self.normalized_until = self.normalized_until.max(ic + 1);
            }
            match cell {
                FieldCell::Backline => {
                    let width = ic
                        .checked_sub(1)
                        .and_then(|previous| self.cells.get(previous))
                        .map_or(0, cell_units);
                    registers.vis = registers.vis.saturating_sub(width);
                }
                FieldCell::BreakableBlank | FieldCell::Breakpoint => {
                    let vn = registers.vis
                        + EN * usize::from(matches!(self.cells[ic], FieldCell::BreakableBlank));
                    if registers.breakline || vn > vtarget {
                        let boundary = if registers.breakline {
                            FillBoundary::WordEndBreak
                        } else {
                            FillBoundary::Width
                        };
                        let result =
                            finish_pass(*registers, vtarget).map(|pass| pass.stopped_at(boundary));
                        scan.stopped = Some(PassStop::from(result));
                        return result;
                    }
                    if registers.graph {
                        registers.nbr = ic;
                        registers.vbr = registers.vis;
                        registers.graph = false;
                        registers.break_candidate = Some(FillBoundary::Width);
                    }
                    registers.vis = vn;
                }
                FieldCell::BreakMarker => registers.breakline = true,
                FieldCell::Hyphen => {
                    if let Some(stop) = registers.consume_hyphen(vtarget) {
                        scan.stopped = Some(stop);
                        return stop.accepted();
                    }
                }
                FieldCell::ZeroWidthGraph => registers.graph = true,
                FieldCell::NonBreakingBlank | FieldCell::Graph { .. } => {
                    let width = match cell {
                        FieldCell::Graph { width, .. } => width,
                        _ => 1,
                    };
                    registers.vis += width.saturating_mul(EN);
                    registers.graph = true;
                    if registers.vis > vtarget && registers.nbr > 0 {
                        let result =
                            Some(FillPass::new(registers.nbr, registers.vbr).stopped_at(
                                registers.break_candidate.unwrap_or(FillBoundary::Width),
                            ));
                        scan.stopped = Some(PassStop::from(result));
                        return result;
                    }
                }
                FieldCell::Tab => {
                    // term.c:327-338: TABREF and the persistent tcol offset
                    // determine the origin under the current .ta settings.
                    registers.vis = advance_tab(&self.tabs, registers.vis, registers.tab_offset);
                    registers.graph = true;
                    if registers.vis > vtarget && registers.nbr > 0 {
                        let result =
                            Some(FillPass::new(registers.nbr, registers.vbr).stopped_at(
                                registers.break_candidate.unwrap_or(FillBoundary::Width),
                            ));
                        scan.stopped = Some(PassStop::from(result));
                        return result;
                    }
                }
                FieldCell::TabReference => {
                    registers.tab_offset = -signed(registers.vis).saturating_add(signed(EN));
                }
            }
            registers.index += 1;
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
