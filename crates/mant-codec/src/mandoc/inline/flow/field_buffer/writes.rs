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

//! Ordered writes, word boundaries and buffer retirement.

use std::sync::Arc;

use super::{
    FieldBuffer, FieldCell, FieldWrite, PassStop, TabStops, WordScanPolicy, WordWriteReceipt,
};

impl FieldBuffer {
    fn push_cell(&mut self, cell: FieldCell) {
        if self.word_first_content.is_none() {
            self.word_first_content = Some(self.cells.len());
        }
        match &cell {
            FieldCell::BreakableBlank => {
                self.blank_positions.insert(self.cells.len());
            }
            FieldCell::NonBreakingBlank => {
                self.nonbreaking_positions.insert(self.cells.len());
            }
            FieldCell::BreakMarker
            | FieldCell::Tab
            | FieldCell::TabReference
            | FieldCell::ZeroWidthGraph
            | FieldCell::Breakpoint => {}
            _ => {
                self.last_graph_position = Some(self.cells.len());
            }
        }
        let projection = usize::from(matches!(
            cell,
            FieldCell::Graph { .. }
                | FieldCell::Hyphen
                | FieldCell::BreakableBlank
                | FieldCell::NonBreakingBlank
                | FieldCell::Tab
        ));
        if self.projection_prefix.is_empty() {
            self.projection_prefix.push(0);
        }
        self.projection_prefix
            .push(self.projection_prefix.last().copied().unwrap_or(0) + projection);
        self.cells.push(cell);
    }

    pub(in crate::mandoc::inline::flow) fn begin_word(
        &mut self,
        tight: bool,
        spacing: bool,
        kept: bool,
    ) -> usize {
        let separator = usize::from(self.word_space_ready && !tight && spacing);
        self.word_space_ready = true;
        if separator > 0 {
            if kept {
                self.push_non_breaking_blank();
            } else {
                self.push_separator_blank();
            }
        }
        separator
    }

    /// A previous `term_word()` established the next word boundary even
    /// when it buffered no glyph (`term.c:573-589`). Explicit NOSPACE joins
    /// remain a separate caller-owned register.
    pub(in crate::mandoc::inline::flow) const fn word_boundary_ready(&self) -> bool {
        self.word_space_ready
    }

    /// A native handler can clear NOSPACE after retiring its buffer.
    /// That register transition still applies to the next empty word.
    pub(in crate::mandoc::inline::flow) fn release_word_boundary(&mut self) {
        self.word_space_ready = true;
    }

    /// Whether this word's automatic separator still occupies its native
    /// cell after `encode1()` ran. BACKBEFORE pops an ordinary blank; its
    /// backspace arm overwrites a KEEP blank instead (term.c:901-908).
    pub(in crate::mandoc::inline::flow) fn word_separator_survives(
        &self,
        start: usize,
        separator: usize,
    ) -> bool {
        separator > 0
            && matches!(
                self.cells.get(start),
                Some(FieldCell::BreakableBlank | FieldCell::NonBreakingBlank)
            )
            && !matches!(self.cells.get(start + 1), Some(FieldCell::Backline))
    }

    pub(in crate::mandoc::inline::flow) fn has_pending_break_markers(&self) -> bool {
        self.last_break_marker
            .is_some_and(|marker| marker >= self.resume)
    }

    /// A committed scan prefix is still part of this field until a real
    /// flush retires its cells. An explicit marker's field policy therefore
    /// survives scanning past it, but never survives field retirement.
    pub(in crate::mandoc::inline::flow) fn has_break_markers(&self) -> bool {
        self.last_break_marker.is_some()
    }

    /// The live pass scanner, rather than projected glyph booleans, proves
    /// whether a pending break's current native interval supplied graph.
    pub(in crate::mandoc::inline::flow) fn pending_pass_is_graphless(&self) -> bool {
        self.has_pending_break_markers()
            && self.scan.as_ref().is_some_and(|scan| {
                matches!(scan.stopped, Some(PassStop::Rejected))
                    || (scan.registers.nbr == 0 && !scan.registers.graph)
            })
    }

    /// An incremental pass stopped at the same nbr=0 boundary that the
    /// final flush will consume. Later buffered words cannot print in it.
    pub(in crate::mandoc::inline::flow) fn pending_pass_is_definitively_rejected(&self) -> bool {
        self.scan
            .as_ref()
            .is_some_and(|scan| matches!(scan.stopped, Some(PassStop::Rejected)))
    }

    pub(in crate::mandoc::inline::flow) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub(in crate::mandoc::inline::flow) fn configure_tabs(&mut self, tabs: &Arc<TabStops>) -> bool {
        if !Arc::ptr_eq(&self.tabs, tabs) {
            self.tabs = tabs.clone();
            // roff_term_pre_ta() changes stops without printing the buffer.
            // Word-time scan prefixes are provisional until term_flushln():
            // their widths must be reconsidered under the new stops. Truly
            // printed fields have already been retired and have no cells.
            self.scan = None;
            self.resume = 0;
            self.committed_passes.clear();
            self.pass_tab_offset = self.tab_offset;
            if !self.cells.is_empty() {
                self.word_scan = WordScanPolicy::AwaitFlush;
            }
            return true;
        }
        false
    }

    pub(in crate::mandoc::inline::flow) const fn word_scan_deferred(&self) -> bool {
        matches!(self.word_scan, WordScanPolicy::AwaitFlush)
    }

    pub(in crate::mandoc::inline::flow) fn note_tab_reference(&mut self) {
        // A source row only inserts TABREF after native buffer writes.
        if !self.cells.is_empty() {
            self.push_cell(FieldCell::TabReference);
        }
    }

    pub(in crate::mandoc::inline::flow) const fn tab_offset(&self) -> i64 {
        self.tab_offset
    }

    pub(in crate::mandoc::inline::flow) fn set_tab_offset(&mut self, offset: i64) {
        if self.tab_offset != offset || self.pass_tab_offset != offset {
            self.tab_offset = offset;
            self.pass_tab_offset = offset;
            self.scan = None;
        }
    }

    /// term.c:233-237: the row ends and the buffer restarts empty (used
    /// both by the accepted-exit and by the nbr==0 wipe).
    pub(in crate::mandoc::inline::flow) fn clear(&mut self) {
        self.cells.clear();
        self.projection_prefix.clear();
        self.pending_projection_graph = None;
        self.resume = 0;
        self.backbefore_armed = false;
        self.backafter_armed = false;
        self.committed_passes.clear();
        self.normalized_until = 0;
        self.word_scan = WordScanPolicy::Incremental;
        self.last_break_marker = None;
        self.projected_pass_ends.clear();
        self.word_space_ready = false;
        self.word_first_content = None;
        self.completed_empty_pass_end = None;
        self.last_graph_position = None;
        self.nonbreaking_positions.clear();
        self.blank_positions.clear();
        self.scan = None;
        self.pass_tab_offset = self.tab_offset;
    }

    /// `term_flushln()`235-237 consumes the input field without resetting
    /// `term_word()`'s NOSPACE lifecycle. A later `term_newln()` or macro pre
    /// decides that boundary separately; device viscol/minbl live elsewhere.
    pub(in crate::mandoc::inline::flow) fn clear_consumed_field(&mut self) {
        let word_space_ready = self.word_space_ready;
        self.clear();
        self.word_space_ready = word_space_ready;
    }

    pub(in crate::mandoc::inline::flow) fn claim_completed_empty_pass(
        &mut self,
        end: usize,
    ) -> bool {
        if self
            .completed_empty_pass_end
            .is_some_and(|previous| end <= previous)
        {
            return false;
        }
        self.completed_empty_pass_end = Some(end);
        true
    }

    /// Execute decoded native writes without consulting projected IR.
    pub(in crate::mandoc::inline::flow) fn apply_writes(
        &mut self,
        writes: &[FieldWrite],
    ) -> WordWriteReceipt {
        self.word_first_content = None;
        for write in writes {
            match write {
                FieldWrite::Cell(FieldCell::Graph { text, width }) => {
                    self.encode_graph(*text, *width, true);
                }
                FieldWrite::Cell(FieldCell::Hyphen) => {
                    self.encode_graph('-', 1, true);
                    *self.cells.last_mut().expect("encoded hyphen") = FieldCell::Hyphen;
                }
                FieldWrite::OwnedBlank { projected } => self.encode_graph('\u{a0}', 1, *projected),
                FieldWrite::Cell(cell) => {
                    if matches!(cell, FieldCell::BreakMarker) {
                        self.last_break_marker = Some(self.cells.len());
                    }
                    self.push_cell(cell.clone());
                }
                FieldWrite::RecoveryGlyph { projected_scalars } => {
                    self.push_cell(FieldCell::ZeroWidthGraph);
                    *self.projection_prefix.last_mut().expect("recovery cell") += projected_scalars;
                }
                FieldWrite::UnprojectedBlank => {
                    self.push_cell(FieldCell::BreakableBlank);
                    *self.projection_prefix.last_mut().expect("buffered blank") -= 1;
                }
                FieldWrite::ArmBackafter => self.backafter_armed = true,
                FieldWrite::CancelBackafter => self.backafter_armed = false,
            }
        }
        let end_cell = self.cells.len();
        let mut first_content_cell = self.word_first_content.take().unwrap_or(end_cell);
        if matches!(
            self.cells.get(first_content_cell),
            Some(FieldCell::Backline)
        ) {
            first_content_cell += 1;
        }
        let prints_padding = self.cells[first_content_cell..end_cell].iter().any(|cell| {
            matches!(
                cell,
                FieldCell::Graph { .. } | FieldCell::Hyphen | FieldCell::Backline
            )
        });
        WordWriteReceipt {
            first_content_cell,
            end_cell,
            prints_padding,
        }
    }
    pub(in crate::mandoc::inline::flow) fn has_projected_pass(&self, end: usize) -> bool {
        self.projected_pass_ends.contains(&end)
    }

    pub(in crate::mandoc::inline::flow) fn has_projected_rows(&self) -> bool {
        !self.projected_pass_ends.is_empty()
    }

    fn encode_graph(&mut self, text: char, width: usize, projected: bool) {
        self.push_graph(text, width);
        if !projected {
            let prefix = self.projection_prefix.last_mut().expect("encoded graph");
            *prefix = prefix.saturating_sub(1);
        }
        if self.backafter_armed {
            self.backafter_armed = false;
            self.backbefore_armed = true;
            self.pending_projection_graph = self.cells.len().checked_sub(1);
        }
    }

    /// A drained IR owner is immutable. Backtracking remains native state,
    /// but cannot revoke a glyph already assigned to the previous owner.
    pub(in crate::mandoc::inline::flow) fn detach_projection_owner(&mut self) {
        self.pending_projection_graph = None;
    }

    pub(in crate::mandoc::inline::flow) fn push_separator_blank(&mut self) {
        self.push_cell(FieldCell::BreakableBlank);
    }

    /// `bufferc('\n')` for `\p` (term.c:657-658).
    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn push_break_marker(&mut self) {
        self.last_break_marker = Some(self.cells.len());
        self.push_cell(FieldCell::BreakMarker);
    }

    /// A direct buffered fixed-width blank: automatic KEEP separators
    /// (term.c:574-580). Escaped Unicode spaces use `encode1()` instead.
    pub(in crate::mandoc::inline::flow) fn push_non_breaking_blank(&mut self) {
        self.push_cell(FieldCell::NonBreakingBlank);
    }

    /// `encode1()` writes a graph (term.c:345-349 tail with 901-908): a
    /// pending `TERMP_BACKBEFORE` retreat first consumes the blank directly
    /// before it (`col--`), or buffers `'\b'` over a non-blank cell.
    pub(in crate::mandoc::inline::flow) fn push_graph(&mut self, text: char, width: usize) {
        if self.backbefore_armed {
            self.backbefore_armed = false;
            match self.cells.last() {
                // A literal tab occupies a buffer byte exactly like a
                // blank (term.c:944-964) and the retreat pops it the same
                // way (`buf[col-1] == '\t'`, term.c:901-904). It never
                // registered in `blank_positions`, so leave that ledger
                // untouched. A NonBreakingBlank buffers ASCII_NBRSP, a
                // different byte, and keeps the `'\b'` arm below.
                Some(FieldCell::Tab | FieldCell::BreakableBlank) => {
                    self.invalidate_projection_scan(self.cells.len().saturating_sub(1));
                    if matches!(self.cells.last(), Some(FieldCell::BreakableBlank)) {
                        self.blank_positions.pop_last();
                    }
                    self.cells.pop();
                    self.normalized_until = self.normalized_until.min(self.cells.len());
                    self.projection_prefix.pop();
                    self.pending_projection_graph = None;
                }
                Some(_) => {
                    if let Some(graph) = self.pending_projection_graph.take() {
                        // A glyph can stay pending across invisible controls.
                        // Each replacement advances the pending graph, so
                        // these adjusted suffixes are disjoint over a field.
                        let contribution = self.projection_prefix[graph + 1]
                            .saturating_sub(self.projection_prefix[graph]);
                        for prefix in &mut self.projection_prefix[graph + 1..] {
                            #[cfg(test)]
                            {
                                self.projection_work += 1;
                            }
                            *prefix = prefix.saturating_sub(contribution);
                        }
                    }
                    self.push_cell(FieldCell::Backline);
                }
                _ => {}
            }
        }
        self.push_cell(FieldCell::Graph { text, width });
    }

    /// Arm the retreat for the NEXT glyph after a completed `\z` glyph
    /// (encode1 tail, term.c:924-927: BACKAFTER converts to BACKBEFORE).
    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn arm_backbefore(&mut self) {
        self.backbefore_armed = true;
    }

    /// `term_flushln()` clears both backtracking flags with the row
    /// (term.c:235-237).
    pub(in crate::mandoc::inline::flow) fn clear_backtracking(&mut self) {
        self.backbefore_armed = false;
    }
}
