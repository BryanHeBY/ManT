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
        // One compressed drawing run can stand for many raw bytes. This
        // conservative flag costs nothing to recompute during tail edits.
        if matches!(
            cell,
            FieldCell::DrawnSpaces(0 | 2..) | FieldCell::FixedDrawingAdvance(0 | 2..)
        ) {
            self.first_uncompact_slot.get_or_insert(self.cells.len());
        }
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
        if let Some(rejection) = &mut self.proved_rejection {
            match &cell {
                FieldCell::BreakableBlank
                | FieldCell::NonBreakingBlank
                | FieldCell::BreakMarker
                | FieldCell::Tab
                | FieldCell::TabReference
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint => {
                    rejection.min_raw_cursor = rejection.min_raw_cursor.saturating_add(1);
                }
                FieldCell::DrawnSpaces(count) | FieldCell::FixedDrawingAdvance(count) => {
                    rejection.min_raw_cursor = rejection.min_raw_cursor.saturating_add(*count);
                }
                // encode1() can consume BACKBEFORE on a preceding blank for a
                // net 0 advance (term.c:903-925), so do not raise the bound.
                FieldCell::Graph { .. } | FieldCell::Hyphen | FieldCell::Backline => {}
            }
        }
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
        self.has_break_marker_at_or_after(self.resume)
    }

    pub(in crate::mandoc::inline::flow) fn has_break_marker_at_or_after(
        &self,
        from: usize,
    ) -> bool {
        self.break_markers
            .last()
            .is_some_and(|&marker| marker >= from)
    }

    /// A committed scan prefix is still part of this field until a real
    /// flush retires its cells. An explicit marker's field policy therefore
    /// survives scanning past it, but never survives field retirement.
    pub(in crate::mandoc::inline::flow) fn has_break_markers(&self) -> bool {
        !self.break_markers.is_empty()
    }

    pub(in crate::mandoc::inline::flow) fn writes_at_buffer_end(&self) -> bool {
        self.write_cursor.retreat() == 0
            && !self.backbefore_armed
            && !self.backafter_armed
            && !matches!(self.word_scan, WordScanPolicy::UnmodeledRewind)
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
        self.proved_rejection.is_some()
            || self
                .scan
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
            self.proved_rejection = None;
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

    pub(in crate::mandoc::inline::flow) const fn awaits_flush(&self) -> bool {
        matches!(self.word_scan, WordScanPolicy::AwaitFlush)
    }

    pub(in crate::mandoc::inline::flow) const fn has_proved_rejection(&self) -> bool {
        self.proved_rejection.is_some()
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
            self.proved_rejection = None;
        }
    }

    /// term.c:233-237: the row ends and the buffer restarts empty (used
    /// both by the accepted-exit and by the nbr==0 wipe).
    pub(in crate::mandoc::inline::flow) fn clear(&mut self) {
        self.cells.clear();
        self.first_uncompact_slot = None;
        self.device_tail = None;
        self.raw_slot_fence = 0;
        self.raw_edit_fence = 0;
        self.deferred_tail_tape = super::super::native_slots::DeferredTail::Absent;
        self.write_cursor = WriteCursor::End;
        self.raw_tail_fonts.clear();
        self.raw_copy_aliases.clear();
        self.projection_owner = super::FieldProjectionOwner::Active;
        self.plain_paint_origins.clear();
        self.projection_prefix.clear();
        self.deferred_blanks.clear();
        self.underline_strokes.clear();
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
        self.deferred_projected_gaps.clear();
        self.word_first_content = None;
        self.completed_empty_pass_end = None;
        self.last_graph_position = None;
        self.previous_graph_position = None;
        self.nonbreaking_positions.clear();
        self.blank_positions.clear();
        self.scan = None;
        self.proved_rejection = None;
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

    /// The output word has emitted this slot as positioning. Keep the
    /// stored cell for `term_fill()` and the editable cursor, but stop
    /// charging it to the semantic source owner's scalar range.
    pub(in crate::mandoc::inline::flow) fn transfer_positioning_slot(&mut self, cell: usize) {
        if !matches!(self.word_scan, WordScanPolicy::AwaitFlush)
            || cell < self.resume
            || self.cells.len().saturating_sub(cell) > super::MAX_EDITABLE_APPEND_CELLS
            || !matches!(
                self.cells.get(cell),
                Some(FieldCell::BreakableBlank | FieldCell::NonBreakingBlank)
            )
            || self.projection_length(cell, cell + 1) != 1
        {
            return;
        }
        for prefix in &mut self.projection_prefix[cell + 1..] {
            *prefix -= 1;
        }
    }

    /// Execute decoded native writes without consulting projected IR.
    pub(in crate::mandoc::inline::flow) fn apply_writes(
        &mut self,
        writes: &[FieldWrite],
    ) -> WordWriteReceipt {
        let prior_raw_fence = self.proven_prefix_fence(writes);
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
                    let retreat = self.encode_graph('\u{a0}', 1, *projected, &mut first_graph);
                    leading_retreat = leading_retreat.or(retreat);
                }
                FieldWrite::DeferredRunInBlank(source) => {
                    let retreat = self.encode_graph('\u{a0}', 1, false, &mut first_graph);
                    leading_retreat = leading_retreat.or(retreat);
                    self.record_deferred_blank(*source, self.cells.len() - 1);
                }
                FieldWrite::AdvanceOverBufferedBlanks(count) => {
                    self.advance_run_in_blanks(*count);
                }
                FieldWrite::Cell(cell) | FieldWrite::DecoratedGraph { cell, .. } => {
                    self.push_cell(cell.clone());
                }
                FieldWrite::StageBacklineProjection => {
                    self.staged_backline_projection = self.backbefore_armed;
                }
                FieldWrite::RecoveryGlyph { projected_scalars } => {
                    self.push_recovery_glyph(*projected_scalars);
                }
                FieldWrite::DrawSpaces(count) => drawing_start = self.draw_spaces(*count),
                FieldWrite::MoveInDrawing { absolute, units } => {
                    self.move_in_drawing(drawing_start, *absolute, *units);
                }
                FieldWrite::UnprovedMotion => self.invalidate_rewound_scan(),
                FieldWrite::Placed { .. }
                | FieldWrite::PlacedUnderlineStroke
                | FieldWrite::KeepPlacedRetreat { .. }
                | FieldWrite::ReplacePlacedTail { .. }
                | FieldWrite::RetainDeviceTail(_)
                | FieldWrite::ProveRawSlots { .. }
                | FieldWrite::OpaqueRawSlots { .. }
                | FieldWrite::ReplaceLivePrefix { .. }
                | FieldWrite::PlacedZeroWidthRetreat { .. } => {
                    self.apply_slot_placement(write, prior_raw_fence);
                }
                FieldWrite::ProveAppendedSlots(proof) => self.prove_appended_slots(entry, proof),
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
                FieldWrite::PlacedBlank { source, cell } => {
                    self.place_deferred_blank(*source, cell);
                }
                FieldWrite::ArmBackafter => self.backafter_armed = true,
                FieldWrite::CancelBackafter => self.backafter_armed = false,
                FieldWrite::RewindWordEntry(columns) => {
                    self.unmodeled_word_rewind(*columns);
                }
                FieldWrite::CancelBackbefore => {
                    self.clear_backtracking();
                }
            }
        }
        self.finish_word_writes(leading_retreat, consumed_motion_padding)
    }

    fn push_recovery_glyph(&mut self, scalars: usize) {
        self.push_cell(FieldCell::ZeroWidthGraph);
        *self.projection_prefix.last_mut().expect("recovery cell") += scalars;
        if scalars > 0 {
            self.first_uncompact_slot
                .get_or_insert(self.cells.len() - 1);
        }
    }

    fn proven_prefix_fence(&self, writes: &[FieldWrite]) -> Option<usize> {
        // Replaying a suffix cannot invalidate raw bytes before that suffix.
        match writes.first() {
            Some(FieldWrite::ReplaceLivePrefix { start, .. })
                if (self.projection_owner == super::FieldProjectionOwner::Detached
                    || self.projection_owner == super::FieldProjectionOwner::RunInHead
                        && self.raw_slot_fence == 0
                        && self.raw_edit_fence == 0
                    || matches!(self.word_scan, WordScanPolicy::AwaitFlush))
                    && self.raw_slot_fence < *start =>
            {
                Some(self.raw_slot_fence)
            }
            _ => None,
        }
    }

    /// Prove an un-flushed appended word's exact `tcol->buf` bytes and font
    /// runs so later `term_word()` motion can replay them (`term.c:620-661,
    /// 888-969`).
    fn prove_appended_slots(&mut self, entry: usize, proof: &super::AppendedSlotProof) {
        if self.cells.len() == entry.saturating_add(proof.cells) {
            // The exact ending font run can be replayed. Earlier paint
            // remains valid even when a later cursor cannot revisit it.
            // An exact fresh append whose cells are all proved one-column
            // glyphs, blanks, or zero-width controls keeps raw_edit_fence at 0
            // even across multiple font runs or trailing controls:
            // term_word()/encode() have not flushed any of those bytes.
            let has_breakable_blank = self.cells.contains(&FieldCell::BreakableBlank);
            // encode1() adds at most 4 extra decoration/backspace bytes per
            // decorated glyph (term.c:911-925), while ZeroWidthGraph cells from
            // `\X'tty: link'` occupy 1 raw byte each (term.c:620-638).
            let decorated_count: usize = proof.fonts.iter().map(|(range, _)| range.len()).sum();
            let complete_fresh_word = entry == 0
                && self.projection_owner != super::FieldProjectionOwner::Detached
                && !self.backafter_armed
                && self.raw_slot_fence == 0
                && self.resume == 0
                && self.projected_pass_ends.is_empty()
                && self.deferred_blanks.is_empty()
                && proof.cells > 0
                && self
                    .cells
                    .len()
                    .saturating_add(decorated_count.saturating_mul(4))
                    <= self.replay_slot_limit()
                && self.cells.iter().enumerate().all(|(index, cell)| {
                    matches!(
                        cell,
                        FieldCell::BreakableBlank
                            | FieldCell::ZeroWidthGraph
                            | FieldCell::Breakpoint
                            | FieldCell::Hyphen
                    ) || matches!(cell, FieldCell::BreakMarker) && index + 1 == self.cells.len()
                        || matches!(
                            cell,
                            FieldCell::Graph { text, width: 1 }
                                if !text.is_whitespace()
                                    || has_breakable_blank && *text == '\u{a0}'
                        )
                });
            if complete_fresh_word {
                if matches!(
                    self.cells.last(),
                    Some(
                        FieldCell::BreakableBlank
                            | FieldCell::ZeroWidthGraph
                            | FieldCell::BreakMarker
                    )
                ) {
                    self.hold_editable_passes();
                }
            } else if !(entry == 0 && proof.pending)
                && let Some(tail) = proof.replay_tail
            {
                self.raw_edit_fence = self.cells.len().saturating_sub(tail);
            }
            for (range, font) in &proof.fonts {
                self.record_appended_font(entry + range.start..entry + range.end, *font);
            }
        } else {
            self.apply_slot_placement(
                &FieldWrite::OpaqueRawSlots {
                    styled_suffix: None,
                    keeps_prior_slots: false,
                },
                None,
            );
        }
    }

    fn place_deferred_blank(
        &mut self,
        source: super::cells::DeferredSourceBlank,
        cell: &FieldCell,
    ) {
        let start = self.cells.len();
        self.push_placed(cell, 0);
        self.record_deferred_blank(source, start);
    }

    fn record_deferred_blank(&mut self, source: super::cells::DeferredSourceBlank, start: usize) {
        let index = self
            .deferred_blanks
            .partition_point(|run| run.start < start);
        self.deferred_blanks.insert(
            index,
            super::cells::DeferredBlankRun {
                start,
                end: start + 1,
                source,
                run_in_overlap: false,
                settled_trailing_run_in: false,
            },
        );
    }

    /// Latch a proved `term_fill()` `nbr == 0` rejection (`term.c:143-146,
    /// 281-366`) before switching to `UnmodeledRewind` (`term.c:701-718`) so
    /// rewinds that cannot reach `resume..scanned_end` retain the rejection.
    pub(super) fn latch_proved_rejection(&mut self) {
        if self.proved_rejection.is_none()
            && let Some(scan) = &self.scan
            && matches!(scan.stopped, Some(PassStop::Rejected))
        {
            let min_raw_cursor = self.cells.len().saturating_sub(self.write_cursor.retreat());
            if min_raw_cursor >= scan.scanned_end {
                self.proved_rejection = Some(super::ProvedRejection {
                    scanned_end: scan.scanned_end,
                    min_raw_cursor,
                });
            }
        }
    }

    fn unmodeled_word_rewind(&mut self, columns: usize) {
        // A rewind left in the stream was either unmodeled
        // at entry or occurred inside a word. term.c:678–714
        // skips it with BACKAFTER, otherwise moves col.
        if !std::mem::take(&mut self.backafter_armed) && (columns > 0 || self.backbefore_armed) {
            self.latch_proved_rejection();
            if let Some(rejection) = self.proved_rejection {
                // Conservatively charge an extra cell for a possible
                // native BACKBEFORE decrement (term.c:701-705).
                let next = rejection
                    .min_raw_cursor
                    .saturating_sub(columns.saturating_add(1));
                self.proved_rejection =
                    (next >= rejection.scanned_end).then_some(super::ProvedRejection {
                        scanned_end: rejection.scanned_end,
                        min_raw_cursor: next,
                    });
            }
            self.word_scan = WordScanPolicy::UnmodeledRewind;
            self.scan = None;
            self.clear_backtracking();
        }
    }

    fn invalidate_rewound_scan(&mut self) {
        self.word_scan = WordScanPolicy::UnmodeledRewind;
        self.scan = None;
        self.proved_rejection = None;
        self.clear_backtracking();
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
            run_in_overlap: false,
            settled_trailing_run_in: false,
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

    /// Direct-write fallback when `p->col < p->tcol->lastcol` (`term.c:573-590,
    /// 678-718, 860-872`): both `Retreated(1..)` and `Placed(1..)` downgrade
    /// to `UnmodeledRewind` while preserving any proved `nbr == 0` rejection
    /// whose `scanned_end` lies at or before `cells.len() - retreat`.
    fn begin_word_writes(&mut self, writes: &[FieldWrite]) {
        if matches!(
            writes.first(),
            Some(FieldWrite::AdvanceOverBufferedBlanks(_))
        ) {
            return;
        }
        if let WriteCursor::Retreated(retreat @ 1..) | WriteCursor::Placed(retreat @ 1..) =
            std::mem::take(&mut self.write_cursor)
            && !matches!(
                writes.first(),
                Some(FieldWrite::ReplacePlacedTail { .. } | FieldWrite::ReplaceLivePrefix { .. })
            )
        {
            let min_raw_cursor = self.cells.len().saturating_sub(retreat);
            if let Some(rejection) = &mut self.proved_rejection {
                rejection.min_raw_cursor = rejection.min_raw_cursor.min(min_raw_cursor);
                if rejection.min_raw_cursor < rejection.scanned_end {
                    self.proved_rejection = None;
                }
            } else if let Some(scan) = &self.scan
                && matches!(scan.stopped, Some(PassStop::Rejected))
                && min_raw_cursor >= scan.scanned_end
            {
                self.proved_rejection = Some(super::ProvedRejection {
                    scanned_end: scan.scanned_end,
                    min_raw_cursor,
                });
            }
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

    pub(in crate::mandoc::inline::flow) const fn has_positioned_slots(&self) -> bool {
        !matches!(self.write_cursor, WriteCursor::End)
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
    pub(in crate::mandoc::inline::flow) fn defer_projected_gap(
        &mut self,
        end: usize,
        restart: usize,
    ) {
        self.deferred_projected_gaps.insert((end, restart));
    }

    pub(in crate::mandoc::inline::flow) fn projects_skipped_gap(
        &self,
        end: usize,
        restart: usize,
    ) -> bool {
        self.deferred_projected_gaps.contains(&(end, restart))
    }

    pub(in crate::mandoc::inline::flow) fn has_projected_pass(&self, end: usize) -> bool {
        self.projected_pass_ends.contains_key(&end)
    }

    pub(in crate::mandoc::inline::flow) fn projected_pass_cut(&self, end: usize) -> Option<usize> {
        self.projected_pass_ends.get(&end).copied()
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

    pub(in crate::mandoc::inline::flow) fn resume_run_in_owner(&mut self) {
        // mdoc_term.c::termp_it_pre() keeps the original inset/diag
        // field live across the HEAD and BODY projection owners.
        self.projection_owner = super::FieldProjectionOwner::Detached;
    }

    pub(in crate::mandoc::inline::flow) fn begin_run_in_head(&mut self) {
        self.projection_owner = super::FieldProjectionOwner::RunInHead;
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
                    self.retire_replaced_cell_receipts(self.cells.len());
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

    pub(super) fn retire_replaced_cell_receipts(&mut self, cell: usize) {
        if let Ok(index) = self.underline_strokes.binary_search(&cell) {
            self.underline_strokes.remove(index);
        }
        if let Some(run) = self.deferred_blanks.last_mut()
            && run.end == cell + 1
        {
            run.end = cell;
            if run.start == run.end {
                self.deferred_blanks.pop();
            }
        }
    }

    pub(super) fn record_pass_retreat(&mut self, target: usize, mark: usize) {
        let key = (mark, target);
        if self
            .pass_retreats
            .last()
            .is_none_or(|&(target, mark)| (mark, target) < key)
        {
            // Ordinary encode1() writes advance to a new slot.
            self.pass_retreats.push((target, mark));
            return;
        }
        // Prefix replay can restate a retreat or add an earlier mark.
        // Ordered retirement must see a surviving mark exactly once.
        let index = self
            .pass_retreats
            .partition_point(|&(target, mark)| (mark, target) < key);
        if self.pass_retreats.get(index) != Some(&(target, mark)) {
            self.pass_retreats.insert(index, (target, mark));
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
            self.record_pass_retreat(cell, self.cells.len() + 1);
            return;
        }
        // term_field() advances to a pending blank and can rewind over it
        // before printing the new graph (term.c:420-434,901-908).
        self.retire_replaced_cell_receipts(cell);
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

    #[test]
    fn deferred_blanks_stay_sorted_when_placed_out_of_order() {
        let mut buffer = FieldBuffer::default();
        for _ in 0..4 {
            buffer.push_cell(FieldCell::NonBreakingBlank);
        }
        let later = super::super::cells::DeferredSourceBlank {
            owner: 2,
            write: 0,
            font: super::super::super::Font::Regular,
            replayable_tail: false,
            generated_run_in: false,
        };
        let earlier = super::super::cells::DeferredSourceBlank {
            owner: 1,
            write: 0,
            font: super::super::super::Font::Regular,
            replayable_tail: false,
            generated_run_in: false,
        };
        // Record a run at cell 4, rewind the tail back to cell 2, then place a
        // new deferred blank at cell 2 and verify sorted order is preserved.
        buffer.place_deferred_blank(later, &FieldCell::NonBreakingBlank);
        buffer.replace_placed_tail(3);
        buffer.place_deferred_blank(earlier, &FieldCell::NonBreakingBlank);
        assert!(buffer.has_deferred_blank_at(2));
        assert!(buffer.has_deferred_blank_at(4));
        assert!(!buffer.has_deferred_blank_at(3));
        assert_eq!(
            buffer
                .deferred_blanks
                .iter()
                .map(|run| (run.start, run.end))
                .collect::<Vec<_>>(),
            vec![(2, 3), (4, 5)]
        );
    }
}
