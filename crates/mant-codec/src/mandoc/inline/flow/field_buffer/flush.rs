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

//! Final native receipts, accepted pass commits and projection positions.

use std::collections::BTreeSet;

use super::{
    EN, FieldBuffer, FieldCell, FillPass, FillTargets, FlushReceipt, advance_tab, cell_units,
    columns, signed,
};

fn resolve_pending_tabs(
    tabs: &mut Option<&mut Vec<(usize, usize)>>,
    pending: &mut usize,
    units: usize,
    non_tab_units: usize,
) {
    let Some(tabs) = tabs else { return };
    let mut remaining =
        columns(units.min(256 * EN)).saturating_sub(columns(non_tab_units.min(256 * EN)));
    let count = tabs.len();
    for (index, (_, cells)) in tabs[*pending..].iter_mut().enumerate() {
        let advance = if *pending + index + 1 == count {
            remaining
        } else {
            columns(*cells).min(remaining)
        };
        *cells = advance;
        remaining -= advance;
    }
    *pending = count;
}

#[cfg(test)]
std::thread_local! {
    static PRINT_CELLS_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl FieldBuffer {
    pub(in crate::mandoc::inline::flow) fn flush_receipt(
        &self,
        targets: FillTargets,
        brtrsp: bool,
    ) -> FlushReceipt {
        let mut scan = self.native_flush_scan();
        // A width prediction has not executed term_fill(). Start the real
        // remaining pass from its native col, preserving only committed
        // marker passes and their in-place byte normalization.
        let mut passes = self.committed_passes.clone();
        loop {
            let first = passes.is_empty();
            let Some(pass) = scan.scan_fill_pass(targets.scan(first), true) else {
                return FlushReceipt::Rejected {
                    passes,
                    native_cells: std::mem::take(&mut scan.cells),
                    rejected_from: scan.resume_offset(),
                    definitive: scan
                        .scan
                        .as_ref()
                        .is_some_and(|scanner| scanner.stopped.is_some()),
                };
            };
            passes.push(pass);
            // term.c:165-168 advances the reference after each printed pass.
            scan.advance_tab_offset(pass.units, targets.actual(first));
            scan.advance_past(pass.end);
            // term_flushln() tests the remaining buffer before consuming
            // ordinary blanks at a genuine continuation boundary.
            if scan.resume_offset() >= scan.cells.len() || scan.only_ignorable_remainder(brtrsp) {
                return FlushReceipt::Accepted {
                    passes,
                    native_cells: std::mem::take(&mut scan.cells),
                };
            }
            scan.consume_break_blanks();
        }
    }

    /// A final sweep needs native cells and tail/tab registers, not the
    /// projection prefix, word receipts or already-emitted owner ledger.
    /// Keep the same scanner; only avoid copying unrelated output state.
    pub(super) fn native_flush_scan(&self) -> Self {
        Self {
            cells: self.cells.clone(),
            tabs: self.tabs.clone(),
            tab_offset: self.tab_offset,
            pass_tab_offset: self.pass_tab_offset,
            resume: self.resume,
            normalized_until: self.normalized_until,
            last_graph_position: self.last_graph_position,
            nonbreaking_positions: self.nonbreaking_positions.clone(),
            blank_positions: self.blank_positions.clone(),
            projection_prefix: Vec::new(),
            pending_projection_graph: None,
            backbefore_armed: false,
            backafter_armed: false,
            committed_passes: Vec::new(),
            word_scan: super::WordScanPolicy::Incremental,
            last_break_marker: None,
            projected_pass_ends: BTreeSet::new(),
            scan: None,
            word_first_content: None,
            completed_empty_pass_end: None,
            #[cfg(test)]
            scan_work: 0,
            #[cfg(test)]
            projection_work: 0,
        }
    }

    /// `term_field()`374-444 prints buffered padding only when a real
    /// encoded glyph follows it. Internal NBRZW and recovery spellings do
    /// not advance the device; Unicode whitespace glyphs do.
    pub(in crate::mandoc::inline::flow) fn printed_columns(
        &self,
        start: usize,
        end: usize,
        tab_offset: i64,
        initial_padding: usize,
    ) -> Option<usize> {
        self.printed_columns_with_tabs(start, end, tab_offset, initial_padding, None)
    }

    /// `term_field()` only commits deferred blanks and tabs at a real
    /// graph or backspace. NBRZW can make `term_fill()` accept the tail
    /// without printing that padding (term.c:389-427).
    pub(in crate::mandoc::inline::flow) fn printed_content_end(
        &self,
        start: usize,
        end: usize,
    ) -> usize {
        let end = end.min(self.cells.len());
        let start = start.min(end);
        self.cells[start..end]
            .iter()
            .rposition(|cell| {
                matches!(
                    cell,
                    FieldCell::Graph { .. } | FieldCell::Hyphen | FieldCell::Backline
                )
            })
            .map_or(start, |index| start + index + 1)
    }

    pub(in crate::mandoc::inline::flow) fn has_unprinted_padding(
        &self,
        start: usize,
        end: usize,
    ) -> bool {
        let printed_end = self.printed_content_end(start, end);
        self.cells[printed_end..end.min(self.cells.len())]
            .iter()
            .any(|cell| {
                matches!(
                    cell,
                    FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab
                )
            })
    }

    /// The same accepted `term_field` scan can record how literal TAB cells
    /// actually printed. Deferred tail TABs consume no device cells.
    pub(in crate::mandoc::inline::flow) fn printed_columns_with_tabs(
        &self,
        start: usize,
        end: usize,
        mut tab_offset: i64,
        initial_padding: usize,
        mut tabs: Option<&mut Vec<(usize, usize)>>,
    ) -> Option<usize> {
        let mut column = 0usize;
        let mut pending_units = initial_padding;
        let mut non_tab_units = initial_padding;
        let mut pending_tabs = tabs.as_ref().map_or(0, |tabs| tabs.len());
        let mut device_columns = 0usize;
        let mut printed = None;
        for index in start..end.min(self.cells.len()) {
            #[cfg(test)]
            PRINT_CELLS_VISITED.with(|work| work.set(work.get().saturating_add(1)));
            match self.cells[index] {
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => {
                    column += EN;
                    pending_units += EN;
                    non_tab_units += EN;
                }
                FieldCell::Graph { .. } | FieldCell::Hyphen => {
                    let width = match self.cells[index] {
                        FieldCell::Graph { width, .. } => width,
                        _ => 1,
                    };
                    // term_field() advances deferred blanks before EACH
                    // graph. ascii_advance() rounds that individual advance
                    // with half-EN tolerance, capped at 256 EN; rounding the
                    // complete field instead changes later vfield decisions
                    // after fractional tabs (term_ascii.c:279-299).
                    resolve_pending_tabs(
                        &mut tabs,
                        &mut pending_tabs,
                        pending_units,
                        non_tab_units,
                    );
                    device_columns += columns(pending_units.min(256 * EN)) + width;
                    pending_units = 0;
                    non_tab_units = 0;
                    column += width.saturating_mul(EN);
                    printed = Some(device_columns);
                }
                FieldCell::Backline => {
                    let width = index
                        .checked_sub(1)
                        .and_then(|previous| self.cells.get(previous))
                        .map_or(0, cell_units);
                    resolve_pending_tabs(
                        &mut tabs,
                        &mut pending_tabs,
                        pending_units,
                        non_tab_units,
                    );
                    device_columns = (device_columns + columns(pending_units.min(256 * EN)))
                        .saturating_sub(columns(width));
                    pending_units = 0;
                    non_tab_units = 0;
                    column = column.saturating_sub(width);
                    printed = Some(device_columns);
                }
                FieldCell::Tab => {
                    // term_field() treats a tab like deferred whitespace:
                    // the advance only flushes when a later graph prints.
                    let next = advance_tab(&self.tabs, column, tab_offset);
                    if let Some(tabs) = &mut tabs {
                        tabs.push((index, next.saturating_sub(column)));
                    }
                    pending_units += next.saturating_sub(column);
                    column = next;
                }
                FieldCell::TabReference => {
                    tab_offset = -signed(column).saturating_add(signed(EN));
                }
                FieldCell::BreakMarker | FieldCell::ZeroWidthGraph | FieldCell::Breakpoint => {}
            }
        }
        if let Some(tabs) = tabs {
            for (_, cells) in &mut tabs[pending_tabs..] {
                *cells = 0;
            }
        }
        printed
    }

    /// Translate only projection positions, never native execution facts.
    /// Invisible native cells have no corresponding scalar in semantic IR.
    pub(in crate::mandoc::inline::flow) fn projection_length(
        &self,
        start: usize,
        end: usize,
    ) -> usize {
        let low = start.min(self.cells.len());
        let high = end.min(self.cells.len());
        self.projection_prefix
            .get(high)
            .copied()
            .unwrap_or(0)
            .saturating_sub(self.projection_prefix.get(low).copied().unwrap_or(0))
    }

    /// A pass with a genuine following suffix is established for ordinary
    /// appends. A changed Tab configuration invalidates these pending scans;
    /// a real flush retires them. Neither operation re-feeds printed fields.
    pub(in crate::mandoc::inline::flow) fn commit_pass(
        &mut self,
        pass: FillPass,
        tab_target: usize,
    ) {
        if let Some(scan) = &self.scan {
            self.normalize_scanned_cells(scan.scanned_end);
        }
        self.advance_tab_offset(pass.units, tab_target);
        self.committed_passes.push(pass);
        self.projected_pass_ends.insert(pass.end);
        self.scan = None;
        self.advance_past(pass.end);
        self.consume_break_blanks();
    }

    pub(super) fn normalize_scanned_cells(&mut self, through: usize) {
        let end = through.min(self.cells.len());
        for index in self.normalized_until.min(end)..end {
            Self::normalize_scanned_cell(
                &mut self.cells[index],
                index,
                &mut self.nonbreaking_positions,
                &mut self.blank_positions,
            );
        }
        self.normalized_until = self.normalized_until.max(end);
    }

    pub(super) fn normalize_scanned_cell(
        cell: &mut FieldCell,
        index: usize,
        nonbreaking: &mut BTreeSet<usize>,
        blanks: &mut BTreeSet<usize>,
    ) {
        // term.c:316/340 normalizes each sentinel before its own current
        // pass guard, including lookahead beyond the accepted prefix.
        match cell {
            FieldCell::Hyphen => {
                *cell = FieldCell::Graph {
                    text: '-',
                    width: 1,
                }
            }
            FieldCell::NonBreakingBlank => {
                *cell = FieldCell::BreakableBlank;
                nonbreaking.remove(&index);
                blanks.insert(index);
            }
            _ => {}
        }
    }

    pub(in crate::mandoc::inline::flow) fn has_committed_pass(&self) -> bool {
        !self.committed_passes.is_empty()
    }

    pub(in crate::mandoc::inline::flow) fn committed_pass_count(&self) -> usize {
        self.committed_passes.len()
    }

    pub(in crate::mandoc::inline::flow) fn has_non_ignorable_after(
        &self,
        position: usize,
        brtrsp: bool,
    ) -> bool {
        self.last_graph_position
            .is_some_and(|index| index >= position)
            || self
                .nonbreaking_positions
                .last()
                .is_some_and(|&index| index >= position)
            || (brtrsp
                && self
                    .blank_positions
                    .last()
                    .is_some_and(|&index| index >= position))
    }

    /// term.c:205-207: input blanks at the resume point are consumed by
    /// the automatic line break itself.
    pub(in crate::mandoc::inline::flow) fn consume_break_blanks(&mut self) {
        while self.resume < self.cells.len()
            && matches!(self.cells[self.resume], FieldCell::BreakableBlank)
        {
            self.resume += 1;
        }
    }

    /// The nbr=0 wipe (term.c:233-237 reached through 145-146): the whole
    /// unprinted remainder dies with the row.
    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn wipe_remainder(&mut self) {
        self.clear();
    }

    /// `term_field()` committed `nbr` cells and advanced `col`
    /// (term.c:443); drop them so later passes index the remainder.
    pub(in crate::mandoc::inline::flow) fn advance_past(&mut self, accepted_end: usize) {
        self.resume = self.resume.max(accepted_end.min(self.cells.len()));
        self.scan = None;
    }

    /// term.c:177-198: whether anything but ignorable cells (blanks,
    /// markers; trailing blanks under BRTRSP count) remains unprinted.
    pub(in crate::mandoc::inline::flow) fn only_ignorable_remainder(&self, brtrsp: bool) -> bool {
        !self.has_non_ignorable_after(self.resume, brtrsp)
    }

    /// term.c:177-196: the trailing ignorable-cell sweep `term_flushln()`
    /// runs after each printed pass. Starting from the last pass's
    /// `accepted_end` with its `vbr`, blanks add one EN and a tab jumps
    /// to the next configured stop while `TERMP_BRTRSP` is set; markers and
    /// zero-width cells never stop the sweep, everything else does. The
    /// result is the `vbr` the final row decision at term.c:250-253 sees.
    pub(in crate::mandoc::inline::flow) fn brtrsp_tail_sweep(
        &self,
        native_cells: &[FieldCell],
        from: usize,
        vbr: usize,
        brtrsp: bool,
    ) -> usize {
        let mut vbr = vbr;
        for cell in native_cells.iter().skip(from.min(native_cells.len())) {
            match cell {
                FieldCell::BreakableBlank => {
                    if brtrsp {
                        vbr += EN;
                    }
                }
                FieldCell::Tab => {
                    if brtrsp {
                        vbr = self.tabs.next_stop(vbr);
                    }
                }
                FieldCell::BreakMarker
                | FieldCell::TabReference
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint => {}
                FieldCell::Graph { .. }
                | FieldCell::Hyphen
                | FieldCell::NonBreakingBlank
                | FieldCell::Backline => {
                    break;
                }
            }
        }
        vbr
    }

    pub(in crate::mandoc::inline::flow) fn cells(&self) -> &[FieldCell] {
        &self.cells
    }

    pub(in crate::mandoc::inline::flow) fn resume_offset(&self) -> usize {
        self.resume
    }

    fn advance_tab_offset(&mut self, width: usize, target: usize) {
        self.pass_tab_offset = self
            .pass_tab_offset
            .saturating_add(signed(width.min(target)))
            .saturating_add(signed(EN));
    }
}

#[cfg(test)]
mod tab_tests {
    use super::super::{FieldWrite, TabStops};
    use super::*;
    use std::sync::Arc;

    #[test]
    fn native_tab_receipts_share_rounding_and_one_capped_deferred_advance() {
        // Exact quoted TAB/TAB/Z/TAB column sources with T3.5n and T300n
        // ran all five pristine profiles first. term.c:394-427 defers one
        // run; term_ascii.c:279-299 rounds/caps that advance, not each TAB.
        for (period, expected, printed) in [
            ("3.5n", vec![(0, 3), (1, 4), (3, 0)], 8),
            ("300n", vec![(0, 256), (1, 0), (3, 0)], 257),
        ] {
            let mut buffer = FieldBuffer::default();
            buffer.configure_tabs(&Arc::new(TabStops::from_arguments(
                ["T", period].into_iter(),
            )));
            buffer.apply_writes(&FieldWrite::literal("\t\tZ\t"));
            let mut tabs = Vec::new();
            assert_eq!(
                buffer.printed_columns_with_tabs(0, 4, 0, 0, Some(&mut tabs)),
                Some(printed)
            );
            assert_eq!(tabs, expected);
            assert!(tabs.iter().map(|(_, cells)| cells).sum::<usize>() <= 256);
        }
    }

    #[test]
    fn consumed_tab_projection_visits_each_native_cell_once() {
        // Exact 1k/4k A-TAB words plus Z ran five pristine profiles before
        // this work counter. It counts the actual shared term_field loop.
        for words in [1024, 4096] {
            let source = format!("{}Z", "A\t".repeat(words));
            let mut buffer = FieldBuffer::default();
            buffer.configure_tabs(&Arc::new(TabStops::literal()));
            buffer.apply_writes(&FieldWrite::literal(&source));
            PRINT_CELLS_VISITED.with(|work| work.set(0));
            let mut tabs = Vec::new();
            assert_eq!(
                buffer.printed_columns_with_tabs(0, source.len(), 0, 0, Some(&mut tabs)),
                Some(words * 8 + 1)
            );
            assert_eq!(
                PRINT_CELLS_VISITED.with(std::cell::Cell::get),
                words * 2 + 1
            );
            assert_eq!(tabs.len(), words);
            assert!(tabs.iter().all(|(_, cells)| *cells == 7));
        }
    }
}
