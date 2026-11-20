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
    WordWriteReceipt, WriteCursor,
};

#[cfg(test)]
std::thread_local! {
    static SEPARATOR_CELLS_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl FieldBuffer {
    pub(super) fn push_cell(&mut self, cell: FieldCell) {
        if self.word_first_content.is_none() {
            self.word_first_content = Some(self.cells.len());
        }
        match &cell {
            FieldCell::BreakableBlank | FieldCell::DrawnSpaces(_) => {
                self.blank_positions.insert(self.cells.len());
            }
            FieldCell::NonBreakingBlank | FieldCell::FixedDrawingAdvance(_) => {
                self.nonbreaking_positions.insert(self.cells.len());
            }
            FieldCell::BreakMarker => self.break_markers.push(self.cells.len()),
            FieldCell::Tab
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
        self.break_markers
            .last()
            .copied()
            .is_some_and(|marker| marker >= self.resume)
    }

    /// A committed scan prefix is still part of this field until a real
    /// flush retires its cells. An explicit marker's field policy therefore
    /// survives scanning past it, but never survives field retirement.
    pub(in crate::mandoc::inline::flow) fn has_break_markers(&self) -> bool {
        !self.break_markers.is_empty()
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
            if !self.cells.is_empty() && !matches!(self.word_scan, WordScanPolicy::UnmodeledRewind)
            {
                self.word_scan = WordScanPolicy::AwaitFlush;
            }
            return true;
        }
        false
    }

    pub(in crate::mandoc::inline::flow) const fn word_scan_deferred(&self) -> bool {
        !matches!(self.word_scan, WordScanPolicy::Incremental)
    }

    pub(in crate::mandoc::inline::flow) fn note_tab_reference(&mut self) {
        // A source row only inserts TABREF after native buffer writes.
        if !self.cells.is_empty() && self.write_cursor.retreat() == 0 {
            if let Some(tail) = &mut self.device_tail
                && !tail.note_tab_reference()
            {
                return;
            }
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
        self.device_tail = None;
        self.raw_slot_fence = 0;
        self.write_cursor = WriteCursor::End;
        self.raw_tail_font = None;
        self.raw_copy_aliases.clear();
        self.projection_prefix.clear();
        self.deferred_blanks.clear();
        self.pass_retreats.clear();
        self.staged_backline_projection = false;
        self.backline_projection_target = None;
        self.backbefore_graph = None;
        self.resume = 0;
        self.backbefore_armed = false;
        self.backafter_armed = false;
        self.committed_passes.clear();
        self.normalized_until = 0;
        self.word_scan = WordScanPolicy::Incremental;
        self.break_markers.clear();
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

    /// Execute decoded native writes without consulting projected IR.
    pub(in crate::mandoc::inline::flow) fn apply_writes(
        &mut self,
        writes: &[FieldWrite],
    ) -> WordWriteReceipt {
        self.word_first_content = None;
        self.begin_word_writes(writes);
        self.device_tail = None;
        let mut first_graph = true;
        let mut leading_retreat = None;
        let entry = self.cells.len();
        let mut drawing_start = entry;
        let mut consumed_motion_padding = Vec::new();
        for write in writes {
            match write {
                FieldWrite::Cell(FieldCell::Graph { text, width })
                | FieldWrite::DecoratedGraph {
                    cell: FieldCell::Graph { text, width },
                    ..
                }
                | FieldWrite::UnprojectedGraph { text, width } => {
                    leading_retreat = leading_retreat.or(self.encode_graph(
                        *text,
                        *width,
                        !matches!(write, FieldWrite::UnprojectedGraph { .. }),
                        &mut first_graph,
                    ));
                }
                FieldWrite::Cell(FieldCell::Hyphen)
                | FieldWrite::DecoratedGraph {
                    cell: FieldCell::Hyphen,
                    ..
                } => {
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
                FieldWrite::Cell(cell) | FieldWrite::DecoratedGraph { cell, .. } => {
                    self.push_cell(cell.clone());
                }
                FieldWrite::StageBacklineProjection => {
                    self.staged_backline_projection = self.backbefore_armed;
                }
                FieldWrite::RecoveryGlyph { projected_scalars } => {
                    self.push_cell(FieldCell::ZeroWidthGraph);
                    *self.projection_prefix.last_mut().expect("recovery cell") += projected_scalars;
                }
                FieldWrite::DrawSpaces(count) => drawing_start = self.draw_spaces(*count),
                FieldWrite::MoveInDrawing { absolute, units } => {
                    self.move_in_drawing(drawing_start, *absolute, *units);
                }
                FieldWrite::UnprovedMotion => {
                    self.word_scan = WordScanPolicy::UnmodeledRewind;
                    self.scan = None;
                    self.clear_backtracking();
                }
                FieldWrite::Placed { .. }
                | FieldWrite::KeepPlacedRetreat { .. }
                | FieldWrite::ReplacePlacedTail { .. }
                | FieldWrite::RetainDeviceTail(_)
                | FieldWrite::ProveRawSlots { .. }
                | FieldWrite::OpaqueRawSlots { .. }
                | FieldWrite::ReplaceLivePrefix { .. }
                | FieldWrite::PlacedZeroWidthRetreat { .. } => self.apply_slot_placement(write),
                FieldWrite::UnprojectedBlank => self.push_unprojected_blank(),
                FieldWrite::MotionPadding {
                    consumed_by_backbefore,
                } => {
                    if self.write_motion_padding(*consumed_by_backbefore) {
                        let offset = self.projection_length(entry, self.cells.len());
                        consumed_motion_padding.push(offset + consumed_motion_padding.len());
                    }
                }
                FieldWrite::DeferredBlanks { source, count } => {
                    self.defer_source_blanks(*source, *count);
                }
                FieldWrite::ArmBackafter => self.backafter_armed = true,
                FieldWrite::CancelBackafter => self.backafter_armed = false,
                FieldWrite::RewindWordEntry(columns) => {
                    // A rewind left in the stream was either unmodeled
                    // at entry or occurred inside a word. term.c:678–714
                    // skips it with BACKAFTER, otherwise moves col.
                    if !std::mem::take(&mut self.backafter_armed)
                        && (*columns > 0 || self.backbefore_armed)
                    {
                        self.word_scan = WordScanPolicy::UnmodeledRewind;
                        self.scan = None;
                        self.clear_backtracking();
                    }
                }
                FieldWrite::CancelBackbefore => {
                    self.clear_backtracking();
                }
            }
        }
        self.finish_word_writes(leading_retreat, consumed_motion_padding)
    }

    fn defer_source_blanks(&mut self, source: super::cells::DeferredSourceBlank, count: usize) {
        let start = self.cells.len();
        for _ in 0..count {
            self.push_unprojected_blank();
        }
        self.deferred_blanks.push(super::cells::DeferredBlankRun {
            start,
            end: self.cells.len(),
            source,
        });
    }

    fn write_motion_padding(&mut self, consumed_by_backbefore: bool) -> bool {
        // ESCAPE_HORIZ clears BACKAFTER without buffering an advance.
        // Otherwise its first positive cell clears BACKBEFORE before
        // the remaining cells are written (term.c:678-698).
        if std::mem::take(&mut self.backafter_armed) {
            return true;
        }
        let consumed = self.backbefore_armed && consumed_by_backbefore;
        self.clear_backtracking();
        if !consumed {
            self.push_non_breaking_blank();
        }
        consumed
    }

    fn begin_word_writes(&mut self, writes: &[FieldWrite]) {
        if matches!(
            std::mem::take(&mut self.write_cursor),
            WriteCursor::Retreated(1..)
        ) && !matches!(
            writes.first(),
            Some(FieldWrite::ReplacePlacedTail { .. } | FieldWrite::ReplaceLivePrefix { .. })
        ) {
            self.word_scan = WordScanPolicy::UnmodeledRewind;
            self.scan = None;
        }
    }

    pub(in crate::mandoc::inline::flow) fn retain_word_cursor(&mut self, retreat: usize) {
        self.write_cursor = WriteCursor::Retreated(retreat);
    }

    pub(in crate::mandoc::inline::flow) const fn has_retained_word_cursor(&self) -> bool {
        self.write_cursor.retreat() > 0
    }

    fn finish_word_writes(
        &mut self,
        leading_retreat: Option<LeadingBlankRetreat>,
        consumed_motion_padding: Vec<usize>,
    ) -> WordWriteReceipt {
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
            consumed_motion_padding,
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
            self.backbefore_graph = self.cells.len().checked_sub(1);
            self.backline_projection_target = self.backbefore_graph;
        }
        leading_retreat
    }

    fn push_unprojected_blank(&mut self) {
        self.push_cell(FieldCell::BreakableBlank);
        *self.projection_prefix.last_mut().expect("buffered blank") -= 1;
    }

    fn leading_blank_retreat(&self) -> Option<LeadingBlankRetreat> {
        if !self.backbefore_armed {
            return None;
        }
        let start = self.word_first_content?;
        let last = self.cells.len().checked_sub(1)?;
        let retracts_raw_blank =
            matches!(self.cells[last], FieldCell::BreakableBlank | FieldCell::Tab);
        let affected = if retracts_raw_blank {
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
            // A staged backline's accepted pass edits the source range.
            // Removing the blank here too would shift that edit onto
            // the next glyph. A raw blank retraction owns its edit now.
            offset: ((retracts_raw_blank || !self.staged_backline_projection)
                && self.projection_length(affected, affected + 1) > 0)
                .then(|| self.projection_length(start, affected)),
            character,
            rearmed: self.backafter_armed,
        })
    }

    /// A drained IR owner is immutable. Backtracking remains native state,
    /// but cannot revoke a glyph already assigned to the previous owner.
    pub(in crate::mandoc::inline::flow) fn detach_projection_owner(&mut self) {
        self.staged_backline_projection = false;
        self.backbefore_graph = None;
        self.backline_projection_target = None;
    }

    pub(in crate::mandoc::inline::flow) fn push_separator_blank(&mut self) {
        self.push_cell(FieldCell::BreakableBlank);
    }

    /// `bufferc('\n')` for `\p` (term.c:657-658).
    #[cfg(test)]
    pub(in crate::mandoc::inline::flow) fn push_break_marker(&mut self) {
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
        let staged = std::mem::take(&mut self.staged_backline_projection);
        if self.backbefore_armed {
            self.backbefore_armed = false;
            self.backbefore_graph = None;
            let target = self.backline_projection_target.take();
            match self.cells.last() {
                Some(FieldCell::DrawnSpaces(_)) => {
                    let position = self.cells.len() - 1;
                    self.invalidate_projection_scan(position);
                    if let Some(FieldCell::DrawnSpaces(count)) = self.cells.last_mut() {
                        *count -= 1;
                        if *count == 0 {
                            self.cells.pop();
                            self.blank_positions.pop_last();
                            self.projection_prefix.pop();
                            self.normalized_until = self.normalized_until.min(position);
                        }
                    }
                }
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
                        self.project_backline_retreat(cell, width, staged);
                    }
                    self.push_cell(FieldCell::Backline);
                }
                _ => {}
            }
        }
        self.push_cell(FieldCell::Graph { text, width });
    }

    pub(super) fn retire_deferred_blank_at(&mut self, cell: usize) {
        if let Some(run) = self.deferred_blanks.last_mut()
            && run.end == cell + 1
        {
            run.end = cell;
            if run.start == run.end {
                self.deferred_blanks.pop();
            }
        }
    }

    fn project_backline_retreat(&mut self, cell: usize, incoming_width: usize, staged: bool) {
        if matches!(self.cells[cell], FieldCell::Graph { width: 0, .. }) {
            // A cached zero-width graph was emitted by term_field() and
            // its backline also has zero width. The new graph cannot cover it.
            return;
        }
        if staged
            || incoming_width == 0
                && matches!(
                    self.cells[cell],
                    FieldCell::Graph { width: 1.., .. } | FieldCell::Hyphen
                )
        {
            // encode1() still writes the backline before the mark. A mark
            // emits no advancing cell at term_field()421-439. Keep the
            // old scalar quota until the accepted pass proves a cover.
            self.pass_retreats.push((cell, self.cells.len() + 1));
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
        self.staged_backline_projection = false;
        self.backbefore_armed = false;
        self.backbefore_graph = None;
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
