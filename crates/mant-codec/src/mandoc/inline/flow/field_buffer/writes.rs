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
    FieldBuffer, FieldCell, FieldWrite, LeadingBlankRetreat, PassStop, TabStops, WordScanPolicy,
    WordWriteReceipt,
};

#[cfg(test)]
std::thread_local! {
    static SEPARATOR_CELLS_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct EntryRewind {
    start: usize,
    traversed_blanks: Vec<usize>,
}

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
                self.previous_graph_position = self.last_graph_position.replace(self.cells.len());
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
        if self.backbefore_armed && projection > 0 {
            // term_field() flushes queued SP/TAB/NBRSP before it emits a
            // backspace, skipping zero-width controls (term.c:389-439).
            self.backline_projection_target = Some(self.cells.len());
        }
        if self.projection_prefix.is_empty() {
            self.projection_prefix.push(0);
        }
        self.projection_prefix
            .push(self.projection_prefix.last().copied().unwrap_or(0) + projection);
        self.cells.push(cell);
    }

    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn begin_word(&mut self, tight: bool, kept: bool) -> usize {
        self.begin_word_with_spacing(tight, kept, 1)
    }

    /// Write only the already executed word-entry receipt. NOSPACE and
    /// NONOSPACE are formatter-global registers, not input-buffer history.
    pub(in crate::mandoc::inline::flow) fn begin_word_with_spacing(
        &mut self,
        tight: bool,
        kept: bool,
        ordinary_spaces: usize,
    ) -> usize {
        debug_assert!((1..=2).contains(&ordinary_spaces));
        let separator = usize::from(!tight) * if kept { 1 } else { ordinary_spaces };
        if separator > 0 {
            if kept {
                self.push_non_breaking_blank();
            } else {
                for _ in 0..separator {
                    self.push_separator_blank();
                }
            }
        }
        separator
    }

    /// At most the two automatic SENTENCE cells can survive encode1's
    /// BACKBEFORE retreat; a KEEP boundary contributes one nonbreaking cell
    /// unless the backspace overstrikes it (term.c:901-908).
    pub(in crate::mandoc::inline::flow) fn surviving_word_separator(
        &self,
        start: usize,
        separator: usize,
    ) -> usize {
        (0..separator)
            .take_while(|offset| {
                #[cfg(test)]
                SEPARATOR_CELLS_VISITED.with(|count| count.set(count.get() + 1));
                let cell = start + *offset;
                matches!(
                    self.cells.get(cell),
                    Some(FieldCell::BreakableBlank | FieldCell::NonBreakingBlank)
                ) && self.projection_length(cell, cell + 1) > 0
            })
            .count()
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
        self.deferred_blanks.clear();
        self.zero_width_retreats.clear();
        self.backline_projection_target = None;
        self.resume = 0;
        self.backbefore_armed = false;
        self.backafter_armed = false;
        self.committed_passes.clear();
        self.normalized_until = 0;
        self.word_scan = WordScanPolicy::Incremental;
        self.last_break_marker = None;
        self.last_breakpoint_fence = 0;
        self.projected_pass_ends.clear();
        self.word_first_content = None;
        self.completed_empty_pass_end = None;
        self.last_graph_position = None;
        self.previous_graph_position = None;
        self.nonbreaking_positions.clear();
        self.blank_positions.clear();
        self.scan = None;
        self.pass_tab_offset = self.tab_offset;
    }

    /// `term_flushln()`235-237 consumes the input field without resetting
    /// `term_word()`'s NOSPACE lifecycle. A later `term_newln()` or macro pre
    /// decides that boundary separately; device viscol/minbl live elsewhere.
    pub(in crate::mandoc::inline::flow) fn clear_consumed_field(&mut self) {
        self.clear();
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

    /// HORIZ rewinds the input cursor without shortening lastcol (term.c
    /// 701–714). Preserve the old prefix traversed by raw SP, then replay
    /// the fully replaced suffix. Only generated padding and the live
    /// BACKBEFORE glyph can be overwritten by this bounded projection.
    pub(in crate::mandoc::inline::flow) fn rewind_word_entry(
        &mut self,
        writes: &mut Vec<FieldWrite>,
        word_start: usize,
    ) -> (usize, usize) {
        let Some(EntryRewind {
            start,
            traversed_blanks,
        }) = self.plan_entry_rewind(writes, word_start)
        else {
            return (0, 0);
        };
        let unprinted_blanks = traversed_blanks.len();
        if unprinted_blanks > 0 {
            let mut traversed_blanks = traversed_blanks.into_iter().peekable();
            let mut index = 0;
            writes.retain(|_| {
                let keep = traversed_blanks.next_if_eq(&index).is_none();
                index += 1;
                keep
            });
        }
        let removed = self.cells.len() - start.max(word_start);
        for index in (start..self.cells.len()).rev() {
            self.blank_positions.remove(&index);
            self.nonbreaking_positions.remove(&index);
            self.retire_deferred_blank_at(index);
        }
        self.cells.truncate(start);
        self.projection_prefix.truncate(start + 1);
        self.zero_width_retreats.truncate(
            self.zero_width_retreats
                .partition_point(|&(_, mark)| mark.saturating_sub(1) < start),
        );
        self.normalized_until = self.normalized_until.min(start);
        if self
            .scan
            .as_ref()
            .is_some_and(|scan| scan.registers.index > start)
        {
            self.scan = None;
        }
        if self.last_graph_position.is_some_and(|last| last >= start) {
            self.last_graph_position = self.previous_graph_position.take();
        }
        (removed, unprinted_blanks)
    }

    fn plan_entry_rewind(&self, writes: &[FieldWrite], word_start: usize) -> Option<EntryRewind> {
        let Some(FieldWrite::RewindWordEntry(columns)) = writes.first() else {
            return None;
        };
        let count = columns.saturating_add(usize::from(self.backbefore_armed));
        let start = self.cells.len().checked_sub(count)?;
        if count == 0 || start < self.resume {
            return None;
        }
        let pending_glyph = self.backline_projection_target.filter(|&cell| {
            self.backbefore_armed && matches!(self.cells.get(cell), Some(FieldCell::Graph { .. }))
        });
        let mut at = start;
        let mut rewritten = None;
        let mut traversed_blanks = Vec::new();
        let mut armed = self.backafter_armed;
        let mut backbefore = false;
        for (index, write) in writes.iter().enumerate().skip(1) {
            match write {
                FieldWrite::ArmBackafter => {
                    armed = true;
                    continue;
                }
                FieldWrite::CancelBackafter => {
                    armed = false;
                    continue;
                }
                FieldWrite::CancelBackbefore => {
                    backbefore = false;
                    continue;
                }
                FieldWrite::Cell(FieldCell::Graph { .. } | FieldCell::Hyphen) => {
                    if backbefore {
                        // A following encode1() can pop or add a cell.
                        // Leave that earlier owner's bytes out of this rewind.
                        return None;
                    }
                    backbefore = std::mem::take(&mut armed);
                }
                FieldWrite::Cell(FieldCell::BreakableBlank) if rewritten.is_none() => {
                    // encode() advances over an already buffered cell but
                    // leaves it unchanged for SP (term.c:948–952).
                    traversed_blanks.push(index);
                    at += 1;
                    if at == self.cells.len() {
                        break;
                    }
                    continue;
                }
                FieldWrite::Cell(FieldCell::BreakableBlank)
                    if matches!(self.cells[at], FieldCell::BreakableBlank) => {}
                other if other.overwrites_entry_padding() => {}
                _ => return None,
            }
            if at < word_start
                && pending_glyph != Some(at)
                && !matches!(self.cells[at], FieldCell::TabReference)
            {
                return None;
            }
            rewritten.get_or_insert(at);
            at += 1;
            if at == self.cells.len() {
                break;
            }
        }
        (at == self.cells.len()).then_some(EntryRewind {
            start: rewritten.unwrap_or(at),
            traversed_blanks,
        })
    }

    /// Execute decoded native writes without consulting projected IR.
    pub(in crate::mandoc::inline::flow) fn apply_writes(
        &mut self,
        writes: &[FieldWrite],
    ) -> WordWriteReceipt {
        self.word_first_content = None;
        let mut first_graph = true;
        let mut leading_retreat = None;
        for write in writes {
            match write {
                FieldWrite::Cell(FieldCell::Graph { text, width }) => {
                    leading_retreat = leading_retreat.or(self.encode_graph(
                        *text,
                        *width,
                        true,
                        &mut first_graph,
                    ));
                }
                FieldWrite::Cell(FieldCell::Hyphen) => {
                    leading_retreat =
                        leading_retreat.or(self.encode_graph('-', 1, true, &mut first_graph));
                    *self.cells.last_mut().expect("encoded hyphen") = FieldCell::Hyphen;
                }
                FieldWrite::OwnedBlank { projected } => {
                    leading_retreat = leading_retreat.or(self.encode_graph(
                        '\u{a0}',
                        1,
                        *projected,
                        &mut first_graph,
                    ));
                }
                FieldWrite::Cell(cell) => {
                    if matches!(cell, FieldCell::BreakMarker) {
                        self.last_break_marker = Some(self.cells.len());
                    } else if matches!(cell, FieldCell::Breakpoint) {
                        self.fence_backline_at_breakpoint();
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
                FieldWrite::DeferredBlanks { source, count } => {
                    let start = self.cells.len();
                    for _ in 0..*count {
                        self.push_cell(FieldCell::BreakableBlank);
                        *self.projection_prefix.last_mut().expect("buffered blank") -= 1;
                    }
                    self.deferred_blanks.push(super::cells::DeferredBlankRun {
                        start,
                        end: self.cells.len(),
                        source: *source,
                    });
                }
                FieldWrite::ArmBackafter => self.backafter_armed = true,
                FieldWrite::CancelBackafter => self.backafter_armed = false,
                FieldWrite::CancelBackbefore | FieldWrite::RewindWordEntry(_) => {
                    self.backbefore_armed = false;
                    self.backline_projection_target = None;
                }
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
            leading_retreat,
        }
    }
    pub(in crate::mandoc::inline::flow) fn has_projected_pass(&self, end: usize) -> bool {
        self.projected_pass_ends.contains(&end)
    }

    pub(in crate::mandoc::inline::flow) fn has_projected_rows(&self) -> bool {
        !self.projected_pass_ends.is_empty()
    }

    fn fence_backline_at_breakpoint(&mut self) {
        // term_fill() restarts breakline after the first ASCII_BREAK
        // following the marker (term.c:276-305). Later breakpoints cannot
        // fence the newly buffered target on behalf of that same marker.
        if self
            .last_break_marker
            .is_some_and(|marker| marker >= self.resume.max(self.last_breakpoint_fence))
        {
            self.last_breakpoint_fence = self.cells.len();
            self.backline_projection_target = None;
        }
    }

    fn encode_graph(
        &mut self,
        text: char,
        width: usize,
        projected: bool,
        first_graph: &mut bool,
    ) -> Option<LeadingBlankRetreat> {
        let leading_retreat = std::mem::take(first_graph)
            .then(|| self.leading_blank_retreat())
            .flatten();
        self.push_graph(text, width);
        if !projected {
            let prefix = self.projection_prefix.last_mut().expect("encoded graph");
            *prefix = prefix.saturating_sub(1);
        }
        if self.backafter_armed {
            self.backafter_armed = false;
            self.backbefore_armed = true;
            self.backline_projection_target = self.cells.len().checked_sub(1);
        }
        leading_retreat
    }

    fn leading_blank_retreat(&self) -> Option<LeadingBlankRetreat> {
        if !self.backbefore_armed {
            return None;
        }
        let start = self.word_first_content?;
        let last = self.cells.len().checked_sub(1)?;
        let affected = if matches!(self.cells[last], FieldCell::BreakableBlank | FieldCell::Tab) {
            last
        } else {
            // encode1() emits Backline across intervening controls.
            // term_field() skips those controls and prints its deferred
            // advance before the backspace (term.c:389-439, 901-908).
            self.backline_projection_target
                .filter(|&cell| cell >= self.resume)?
        };
        if affected < start {
            return None;
        }
        let character = match self.cells.get(affected)? {
            FieldCell::BreakableBlank => ' ',
            FieldCell::Tab => '\t',
            _ => return None,
        };
        // Examine the authored prefix once, before its first graph.
        // The leading term_word() padding has its own cell receipt.
        if !self
            .cells
            .get(start..affected)?
            .iter()
            .enumerate()
            .all(|(offset, cell)| match cell {
                FieldCell::BreakableBlank | FieldCell::Tab => true,
                // term_word() buffers these controls without calling
                // encode1(). A reading recovery spelling has a nonzero
                // projection and is outside the raw blank prefix.
                FieldCell::BreakMarker
                | FieldCell::Breakpoint
                | FieldCell::TabReference
                | FieldCell::ZeroWidthGraph => {
                    self.projection_length(start + offset, start + offset + 1) == 0
                }
                _ => false,
            })
        {
            return None;
        }
        Some(LeadingBlankRetreat {
            offset: (self.projection_length(affected, affected + 1) > 0)
                .then(|| self.projection_length(start, affected)),
            character,
            rearmed: self.backafter_armed,
        })
    }

    /// A drained IR owner is immutable. Backtracking remains native state,
    /// but cannot revoke a glyph already assigned to the previous owner.
    pub(in crate::mandoc::inline::flow) fn detach_projection_owner(&mut self) {
        self.backline_projection_target = None;
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
            let target = self.backline_projection_target.take();
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
                    self.retire_deferred_blank_at(self.cells.len());
                    self.normalized_until = self.normalized_until.min(self.cells.len());
                    self.projection_prefix.pop();
                }
                Some(_) => {
                    if let Some(cell) = target.filter(|&cell| cell >= self.resume) {
                        self.project_backline_retreat(cell, width);
                    }
                    self.push_cell(FieldCell::Backline);
                }
                _ => {}
            }
        }
        self.push_cell(FieldCell::Graph { text, width });
    }

    fn retire_deferred_blank_at(&mut self, cell: usize) {
        if let Some(run) = self.deferred_blanks.last_mut()
            && run.end == cell + 1
        {
            run.end = cell;
            if run.start == run.end {
                self.deferred_blanks.pop();
            }
        }
    }

    fn project_backline_retreat(&mut self, cell: usize, incoming_width: usize) {
        if matches!(self.cells[cell], FieldCell::Graph { width: 0, .. }) {
            // A cached zero-width graph was emitted by term_field() and
            // its backline also has zero width. The new graph cannot cover it.
            return;
        }
        if incoming_width == 0
            && matches!(
                self.cells[cell],
                FieldCell::Graph { width: 1.., .. } | FieldCell::Hyphen
            )
        {
            // encode1() still writes the backline before the mark. A mark
            // emits no advancing cell at term_field()421-439. Keep the
            // old scalar quota until the accepted pass proves a cover.
            self.zero_width_retreats.push((cell, self.cells.len() + 1));
            return;
        }
        // term_field() advances to a pending blank and can rewind over it
        // before printing the new graph (term.c:420-434,901-908).
        self.retire_deferred_blank_at(cell);
        let contribution = self.projection_length(cell, cell + 1);
        // An arm can cross invisible controls, but the next arm starts
        // with a newly appended glyph. Adjust only its still-live suffix.
        for prefix in &mut self.projection_prefix[cell + 1..] {
            #[cfg(test)]
            {
                self.projection_work += 1;
            }
            *prefix = prefix.saturating_sub(contribution);
        }
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
        self.backline_projection_target = None;
    }
}

#[cfg(test)]
mod sentence_tests {
    use super::*;

    #[test]
    fn sentence_separators_are_real_cells_and_backbefore_consumes_only_one() {
        // Exact plain-line/KEEP/\z sources ran pinned profiles before
        // these assertions. term_word():573-589 inserts the cells before
        // encode1():901-908 can retreat over the final ordinary blank.
        for (tight, kept, sentence, expected) in [
            (false, false, false, 1),
            (false, false, true, 2),
            (false, true, true, 1),
            (true, false, true, 0),
        ] {
            let mut buffer = FieldBuffer::default();
            buffer.begin_word(true, false);
            buffer.push_graph('F', 1);
            let start = buffer.cells().len();
            let separator = buffer.begin_word_with_spacing(tight, kept, 1 + usize::from(sentence));
            assert_eq!(separator, expected);
            assert_eq!(buffer.cells().len(), start + expected);
            buffer.push_graph('N', 1);
            assert_eq!(buffer.surviving_word_separator(start, separator), expected);
        }
        let mut buffer = FieldBuffer::default();
        buffer.begin_word(true, false);
        buffer.push_graph('F', 1);
        let separator = buffer.begin_word_with_spacing(false, false, 2);
        buffer.arm_backbefore();
        buffer.push_graph('N', 1);
        assert_eq!(buffer.surviving_word_separator(1, separator), 1);
        assert_eq!(buffer.cells().len(), 3);
    }

    #[test]
    fn sentence_separator_work_never_revisits_the_buffer_prefix() {
        for words in [128, 4096] {
            let mut buffer = FieldBuffer::default();
            SEPARATOR_CELLS_VISITED.with(|count| count.set(0));
            for index in 0..words {
                let start = buffer.cells().len();
                let separator = buffer.begin_word_with_spacing(index == 0, false, 2);
                buffer.push_graph('X', 1);
                assert_eq!(buffer.surviving_word_separator(start, separator), separator);
            }
            assert_eq!(buffer.cells().len(), 3 * words - 2);
            assert_eq!(
                SEPARATOR_CELLS_VISITED.with(std::cell::Cell::get),
                2 * (words - 1)
            );
        }
    }
}
