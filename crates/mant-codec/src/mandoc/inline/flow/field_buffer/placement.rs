//! Hand device-resolved writes to the same fill scanner as ordinary words.

use std::{collections::BTreeMap, ops::Range};

use super::{FieldBuffer, FieldCell, FieldWrite, FillBoundary, WriteCursor};

// Bound native replay by raw slots, not visible columns. encode1()
// can use up to five slots per styled glyph (term.c:903–929); a short
// marked definition word with up to 32 such glyphs must remain
// editable until the following HORIZ can edit its tail.
pub(super) const MAX_REPLAY_PREFIX_CELLS: usize = 32 * 5;

pub(in crate::mandoc::inline::flow) struct RetainedMotionGap {
    pub(in crate::mandoc::inline::flow) marker: usize,
    pub(in crate::mandoc::inline::flow) width: usize,
    pub(in crate::mandoc::inline::flow) previous_graph: usize,
    pub(in crate::mandoc::inline::flow) projection_is_complete: bool,
    /// Projected blanks in this gap that lie behind a covered earlier glyph
    /// still sitting in `self.nodes` until flush (`term.c:389-434`).
    pub(in crate::mandoc::inline::flow) hidden_projected_blanks: usize,
}

impl FieldBuffer {
    pub(in crate::mandoc::inline::flow) fn replay_slot_limit(&self) -> usize {
        match self.projection_owner {
            super::FieldProjectionOwner::Active => MAX_REPLAY_PREFIX_CELLS,
            super::FieldProjectionOwner::RunInHead | super::FieldProjectionOwner::Detached => {
                super::MAX_RUN_IN_REPLAY_CELLS
            }
        }
    }

    /// Find source blank markers whose first surviving slot follows the
    /// incoming word. A marker still owns the accepted count at flush.
    pub(in crate::mandoc::inline::flow) fn relocated_blank_markers(
        &self,
        start: usize,
        positions: &[Option<usize>],
        first_incoming: usize,
    ) -> BTreeMap<usize, String> {
        if !positions
            .iter()
            .flatten()
            .any(|&slot| slot >= first_incoming)
        {
            return BTreeMap::new();
        }
        let edited = self.deferred_blanks.partition_point(|run| run.end <= start);
        let mut sources = BTreeMap::<_, ((usize, usize), super::cells::DeferredSourceBlank)>::new();
        for run in &self.deferred_blanks[edited..] {
            let Some(offset) = run.start.checked_sub(start) else {
                sources.insert(
                    (run.source.owner, run.source.write),
                    ((run.start, run.start), run.source),
                );
                continue;
            };
            let first = (offset..run.end - start)
                .filter_map(|offset| {
                    Some((positions.get(offset).copied().flatten()?, start + offset))
                })
                .min();
            if let Some(first) = first {
                sources
                    .entry((run.source.owner, run.source.write))
                    .and_modify(|(seen, _)| *seen = (*seen).min(first))
                    .or_insert((first, run.source));
            }
        }
        sources.retain(|_, ((position, _), _)| *position >= first_incoming);
        // An edited run can share a source marker with an earlier fixed
        // piece. A marker already preceding the new word stays there.
        if !sources.is_empty() {
            for run in &self.deferred_blanks[..edited] {
                sources.remove(&(run.source.owner, run.source.write));
                if sources.is_empty() {
                    break;
                }
            }
        }
        sources
            .into_values()
            .filter_map(|((_, cell), source)| {
                let slot = cell.checked_sub(start)?;
                Some((slot, source.marker()))
            })
            .collect()
    }

    pub(super) fn apply_slot_placement(
        &mut self,
        write: &FieldWrite,
        prior_raw_fence: Option<usize>,
    ) {
        match write {
            FieldWrite::Placed { cell, scalars } => self.push_placed(cell, *scalars),
            FieldWrite::PlacedUnderlineStroke => {
                let cell = self.cells.len();
                self.push_placed(
                    &FieldCell::Graph {
                        text: '_',
                        width: 1,
                    },
                    0,
                );
                self.underline_strokes.push(cell);
            }
            FieldWrite::KeepPlacedRetreat { from_end } => self.keep_placed_retreat(*from_end),
            FieldWrite::ReplacePlacedTail { cells, .. } => self.replace_placed_tail(*cells),
            FieldWrite::RetainDeviceTail(tail) => self.device_tail = Some(tail.clone()),
            FieldWrite::ProveRawSlots {
                count,
                cursor_retreat,
                backbefore,
                duplicates,
                editable_marker,
            } => {
                let appended_start = self.cells.len() - count;
                self.raw_slot_fence =
                    prior_raw_fence.map_or(appended_start, |fence| fence.min(appended_start));
                self.raw_edit_fence = appended_start;
                if self.raw_slot_fence == 0
                    && !self.deferred_blanks.is_empty()
                    && self
                        .deferred_blanks
                        .iter()
                        .all(|run| run.source.replayable_tail)
                {
                    self.deferred_tail_tape = super::super::native_slots::DeferredTail::Replayable;
                }
                if *editable_marker {
                    // An interior \p has only marked the raw word.
                    // term.c::term_fill runs when a real flush accepts
                    // the field. A later operand can still edit this pass.
                    self.hold_editable_passes();
                }
                self.write_cursor = WriteCursor::Placed(*cursor_retreat);
                self.backbefore_armed = *backbefore;
                if *backbefore && self.backline_projection_target.is_none() {
                    let cursor = self.cells.len().saturating_sub(*cursor_retreat);
                    self.backline_projection_target = (self.resume..cursor)
                        .rev()
                        .find(|&cell| self.projection_length(cell, cell + 1) > 0);
                }
                self.raw_copy_aliases.retain(|&(copy, owner)| {
                    prior_raw_fence.is_some_and(|fence| fence <= copy && owner < appended_start)
                });
                self.raw_copy_aliases.extend(
                    duplicates
                        .iter()
                        .map(|&(copy, owner)| (appended_start + copy, appended_start + owner)),
                );
            }
            FieldWrite::OpaqueRawSlots {
                styled_suffix,
                keeps_prior_slots,
            } => self.record_raw_tail_font(*styled_suffix, *keeps_prior_slots),
            FieldWrite::ReplaceLivePrefix {
                start,
                positions,
                trailing_blank_slots,
                ..
            } => self.replace_live_prefix(*start, positions, trailing_blank_slots),
            FieldWrite::PlacedZeroWidthRetreat {
                target_from_end,
                mark_from_end,
            } => {
                let end = self.cells.len();
                self.record_pass_retreat(end - target_from_end, end - mark_from_end);
            }
            _ => unreachable!("only resolved slot writes reach this path"),
        }
    }

    pub(super) fn hold_editable_passes(&mut self) {
        if self.resume == 0 && matches!(self.word_scan, super::WordScanPolicy::Incremental) {
            self.word_scan = super::WordScanPolicy::AwaitFlush;
            self.scan = None;
        }
    }

    pub(in crate::mandoc::inline::flow) fn hold_pending_glyph_passes(
        &mut self,
        writes: &[FieldWrite],
    ) {
        // ASCII_NBRZW counts as graph for term_fill() and has no reading
        // scalar. In a proved control prefix, the following space may
        // stop a pass only after every term_word() has finished writing
        // before lastcol. Keep this prefix editable from its first write.
        if self.has_editable_control_prefix()
            && self.cells.contains(&FieldCell::ZeroWidthGraph)
            && self.cells.contains(&FieldCell::BreakableBlank)
        {
            self.hold_editable_passes();
            return;
        }
        if self.backbefore_graph.is_none() || !self.has_pending_break_markers() {
            return;
        }
        let Some(marker) = writes.iter().position(FieldWrite::is_break_marker) else {
            return;
        };
        // A completed word followed by \p and an authored blank under
        // BACKAFTER can still be overwritten from the pending glyph's
        // cursor, whether the prior graph was written in this word or an
        // earlier bounded word in `tcol->buf` (`term.c:659-660, 701-718`).
        let has_prior_graph = writes[..marker].iter().any(|write| {
            matches!(
                write,
                FieldWrite::Cell(FieldCell::Graph { .. } | FieldCell::Hyphen)
            )
        }) || self.cells.len() <= MAX_REPLAY_PREFIX_CELLS
            && self.break_markers.first().is_some_and(|&pos| {
                self.cells[..pos]
                    .iter()
                    .any(|cell| matches!(cell, FieldCell::Graph { .. } | FieldCell::Hyphen))
            });
        if has_prior_graph
            && let [
                FieldWrite::ArmBackafter,
                FieldWrite::DeferredBlanks { .. },
                tail @ ..,
            ] = &writes[marker + 1..]
            && !tail.iter().any(FieldWrite::is_break_marker)
        {
            self.hold_editable_passes();
        }
    }

    fn record_raw_tail_font(
        &mut self,
        styled_suffix: Option<(usize, crate::mandoc::roff_escape::RoffFont)>,
        keeps_prior_slots: bool,
    ) {
        let end = self.cells.len();
        let tail = styled_suffix.and_then(|(count, font)| {
            let start = end.checked_sub(count)?;
            (self.projection_length(start, end) == count).then_some((start..end, font))
        });
        // A proved styled append preserves both the editable slots and
        // every earlier decoration copy. Each run expands through encode1
        // only when the bounded placement window reaches it.
        if !keeps_prior_slots || tail.is_none() {
            self.raw_slot_fence = tail.as_ref().map_or(end, |(range, _)| range.start);
            self.raw_edit_fence = self.raw_slot_fence;
            self.raw_copy_aliases.clear();
            self.raw_tail_fonts.clear();
        }
        if let Some((range, font)) = tail {
            self.record_appended_font(range, font);
        }
    }

    pub(super) fn record_appended_font(
        &mut self,
        range: Range<usize>,
        font: crate::mandoc::roff_escape::RoffFont,
    ) {
        if let Some((prior, prior_font)) = self.raw_tail_fonts.last_mut()
            && prior.end == range.start
            && *prior_font == font
        {
            prior.end = range.end;
        } else {
            self.raw_tail_fonts.push((range, font));
        }
    }

    /// A complete, still writable HEAD must keep its owners through BODY.
    /// The first ordinary BODY word need not mention a move; a later
    /// `term_word()` can still return to this origin before `termp_it_post()`
    /// calls `term_newln()` (`mdoc_term.c:939–945`; `term.c:678–718`).
    pub(in crate::mandoc::inline::flow) fn has_bounded_run_in_projection(&self) -> bool {
        self.resume == 0
            && self.raw_slot_fence == 0
            && self.projected_pass_ends.is_empty()
            && !matches!(self.word_scan, super::WordScanPolicy::UnmodeledRewind)
            && self.cells.len() <= self.replay_slot_limit()
            && self.projection_length(0, self.cells.len()) > 0
    }

    pub(in crate::mandoc::inline::flow) const fn projection_owner(
        &self,
    ) -> super::FieldProjectionOwner {
        self.projection_owner
    }

    pub(in crate::mandoc::inline::flow) fn can_replay_armed_run_in_head(&self) -> bool {
        self.projection_owner == super::FieldProjectionOwner::RunInHead
            && self.backafter_armed
            && self.has_bounded_run_in_projection()
    }

    /// BODY pre appends at lastcol after HEAD has cleared both backtracking
    /// flags. The marked field is still live, so a later BODY word can
    /// write before the generated U+00A0 (term.c:659, 678–718, 903–929).
    pub(in crate::mandoc::inline::flow) fn can_defer_inset_separator(&self) -> bool {
        self.projection_owner == super::FieldProjectionOwner::Detached
            && self.has_bounded_run_in_projection()
            && self.has_pending_break_markers()
            && matches!(self.write_cursor, WriteCursor::Placed(0))
            && !self.backbefore_armed
            && !self.backafter_armed
    }

    pub(in crate::mandoc::inline::flow) fn can_append_run_in_separator(&self) -> bool {
        self.projection_owner == super::FieldProjectionOwner::Detached
            && self.has_bounded_run_in_projection()
            && self.writes_at_buffer_end()
            && !self.has_pending_break_markers()
    }

    /// A one-cell generated inset fallback appends after a marked fixed
    /// HEAD slot. The authored graph immediately before it belongs to a
    /// different word; neither BACK register can move the generated write
    /// away from the incoming word's first content cell (term.c:903–929).
    pub(in crate::mandoc::inline::flow) fn proves_generated_run_in_blank(
        &self,
        writes: Option<&[FieldWrite]>,
    ) -> bool {
        matches!(writes, Some([FieldWrite::OwnedBlank { projected: true }]))
            && self.can_defer_inset_separator()
            && matches!(
                self.cells.as_slice(),
                [
                    ..,
                    FieldCell::Backline,
                    FieldCell::Graph {
                        text: '\u{a0}',
                        width: 1
                    }
                ]
            )
            && self.projection_length(self.cells.len() - 1, self.cells.len()) == 1
    }

    /// A simple decorated HEAD still has its native copies. Its fixed
    /// BODY separator needs an explicit, relocatable blank owner instead of
    /// an independently projected space. Leave nondecorated HEAD words on
    /// the ordinary append path; it already preserves their relative motions.
    pub(in crate::mandoc::inline::flow) fn has_bounded_decorated_run_in_projection(&self) -> bool {
        self.has_bounded_run_in_projection()
            && !self.backafter_armed
            && !self.backbefore_armed
            && self.write_cursor.retreat() == 0
            && (!self.raw_tail_fonts.is_empty()
                || !self.raw_copy_aliases.is_empty()
                || !self.underline_strokes.is_empty())
    }

    /// The diagonal BODY's two generated fixed blanks follow this
    /// complete, still-unflushed HEAD. A zero-width graph does not clear
    /// BACKBEFORE; unlike a terminal SP it makes encode1 write a backline
    /// before the two new NBSP cells (term.c:620–660, 903–929).
    pub(in crate::mandoc::inline::flow) fn has_proved_diag_separator_entry(&self) -> bool {
        self.has_bounded_run_in_projection()
            && self.backbefore_armed
            && !self.backafter_armed
            && self.has_pending_break_markers()
            && (matches!(self.write_cursor, WriteCursor::Placed(0))
                && matches!(
                    self.cells.as_slice(),
                    [.., FieldCell::BreakableBlank, FieldCell::ZeroWidthGraph]
                )
                && self.projection_length(self.cells.len() - 1, self.cells.len()) == 0
                || self.has_proved_diag_breakpoint_tail())
    }

    fn has_proved_diag_breakpoint_tail(&self) -> bool {
        // The producer proved the decorated HEAD before LF and BREAK.
        // At col-1 only an authored SP is replaced by the first NBSP.
        matches!(self.write_cursor, WriteCursor::End)
            && !self.raw_tail_fonts.is_empty()
            && match self.cells.as_slice() {
                [.., FieldCell::BreakMarker, FieldCell::Breakpoint] => true,
                [
                    ..,
                    FieldCell::BreakMarker,
                    FieldCell::Breakpoint,
                    FieldCell::BreakableBlank,
                ] => self.deferred_blanks.last().is_some_and(|run| {
                    !run.run_in_overlap && run.end == self.cells.len() && run.start + 1 == run.end
                }),
                _ => false,
            }
    }

    /// Without a terminal source-space owner, this proved diagonal
    /// prefix still needs to keep its HEAD anchors through BODY. HEAD
    /// post does not flush the tape (`mdoc_term.c:939–945`).
    pub(in crate::mandoc::inline::flow) fn has_pending_diag_head_projection(&self) -> bool {
        self.has_proved_diag_separator_entry()
            && matches!(
                self.cells.as_slice(),
                [.., FieldCell::BreakMarker, FieldCell::Breakpoint]
            )
    }

    /// After a fully placed decorated inset HEAD ends with LF, source SP
    /// and a zero-width cell, BACKBEFORE still sees the zero-width cell at
    /// col-1. The generated BODY NBSP appends a backline, not a replacement
    /// of that earlier SP (term.c:620–660, 903–929).
    pub(in crate::mandoc::inline::flow) fn has_proved_inset_zero_width_separator_entry(
        &self,
    ) -> bool {
        self.has_bounded_run_in_projection()
            && self.raw_edit_fence == 0
            && matches!(self.word_scan, super::WordScanPolicy::Incremental)
            && self.backbefore_armed
            && !self.backafter_armed
            && self.has_pending_break_markers()
            && matches!(self.write_cursor, WriteCursor::Placed(0))
            && matches!(
                self.cells.as_slice(),
                [
                    FieldCell::Graph { text: stroke, width: 1 },
                    FieldCell::Backline,
                    FieldCell::Graph { text, width: 1 },
                    FieldCell::BreakMarker,
                    FieldCell::BreakableBlank,
                    FieldCell::ZeroWidthGraph,
                ] if stroke == text || *stroke == '_'
            )
            && self.projection_length(5, 6) == 0
    }

    /// A complete one-cell plain HEAD has two non-SP tails that keep
    /// BACKBEFORE pending: LF/NBRZW, or source SP/LF. In both cases
    /// generated UTF-8 NBSP inspects the last raw cell, appends a backline,
    /// then emits the copies for the BODY's active font (term.c:903–929).
    pub(in crate::mandoc::inline::flow) fn has_proved_inset_styled_append_entry(&self) -> bool {
        self.has_bounded_run_in_projection()
            && self.raw_edit_fence == 0
            && self.backbefore_armed
            && !self.backafter_armed
            && self.has_pending_break_markers()
            && self.raw_tail_fonts.is_empty()
            && self.raw_copy_aliases.is_empty()
            && matches!(
                (self.cells.as_slice(), self.write_cursor, self.word_scan),
                (
                    [
                        FieldCell::Graph { width: 1, .. },
                        FieldCell::BreakMarker,
                        FieldCell::ZeroWidthGraph
                    ],
                    WriteCursor::Placed(0),
                    super::WordScanPolicy::AwaitFlush,
                ) | (
                    [
                        FieldCell::Graph { width: 1, .. },
                        FieldCell::BreakableBlank,
                        FieldCell::BreakMarker
                    ],
                    WriteCursor::End,
                    super::WordScanPolicy::Incremental,
                )
            )
            && self.projection_length(0, 1) == 1
            && self.projection_length(2, 3) == 0
    }

    /// An inset BODY with a proved trailing source space takes the
    /// encode1 BACKBEFORE branch that replaces that SP with its generated
    /// NBSP, rather than appending a second cell (term.c:903–929).
    /// A deferred source owner distinguishes this from an automatic
    /// separator placed by an empty HEAD operand.
    pub(in crate::mandoc::inline::flow) fn has_proved_inset_separator_entry(&self) -> bool {
        self.has_bounded_run_in_projection()
            && self.backbefore_armed
            && !self.backafter_armed
            && matches!(self.write_cursor, WriteCursor::End)
            && self.has_pending_break_markers()
            && self.deferred_blanks.last().is_some_and(|run| {
                !run.run_in_overlap
                    && run.end == self.cells.len()
                    && run.start < run.end
                    && self.cells[run.start..run.end]
                        .iter()
                        .all(|cell| *cell == FieldCell::BreakableBlank)
            })
    }

    /// Snapshot the live un-flushed `tcol->buf` suffix and cursor/backtracking
    /// registers (`term.c:573-590, 678-718, 888-930`) for `NativeSlots` replay.
    pub(in crate::mandoc::inline::flow) fn field_prefix(
        &self,
        automatic: usize,
        kept: bool,
        live_replay: bool,
    ) -> Option<super::super::native_slots::FieldPrefix> {
        let slot_limit = self.replay_slot_limit();
        if slot_limit > MAX_REPLAY_PREFIX_CELLS
            && self.cells.len() <= slot_limit
            && let Some(prefix) =
                self.field_prefix_with_limit(automatic, kept, live_replay, slot_limit)
            && prefix.start == 0
            && prefix.raw_slot_count() <= slot_limit
        {
            return Some(prefix);
        }
        self.field_prefix_with_limit(automatic, kept, live_replay, MAX_REPLAY_PREFIX_CELLS)
    }

    fn field_prefix_with_limit(
        &self,
        automatic: usize,
        kept: bool,
        live_replay: bool,
        slot_limit: usize,
    ) -> Option<super::super::native_slots::FieldPrefix> {
        let control_prefix = self.has_editable_control_prefix();
        let complete_tail = self.has_replayable_trailing_advance();
        if matches!(self.word_scan, super::WordScanPolicy::UnmodeledRewind)
            || self.resume == 0 && !self.projected_pass_ends.is_empty()
            || !(live_replay || control_prefix || complete_tail)
                && (self.resume != 0 || !self.deferred_blanks.is_empty())
        {
            return None;
        }
        // A proved landing may still edit the unread suffix. The ordinary
        // spaces immediately before a retained raw cursor are proved slots
        // too. Preserve their deferred source ownership through replay.
        let detached_replay =
            live_replay && self.projection_owner == super::FieldProjectionOwner::Detached;
        let fence = if control_prefix {
            0
        } else if detached_replay || complete_tail {
            // term.c::encode1() can overwrite only part of a decoration.
            // A generated run-in blank may attach projection metadata inside
            // that pair, but both slots still belong to the live buffer.
            self.raw_slot_fence
        } else {
            self.replay_fence(live_replay)
        };
        let fence = self.retained_cursor_fence(fence);
        let floor = fence
            .max(self.resume)
            .max(self.cells.len().saturating_sub(slot_limit));
        // A projection-only recovery spelling can fence its own native
        // position without hiding complete glyph slots appended after it.
        let mut start = (floor..self.cells.len())
            .rev()
            .find(|&index| !self.has_replayable_projection(index))
            .map_or(floor, |index| index + 1);
        // A suffix can start after a backspace dependency even when the
        // slots before it are not replayable. Geometry rejects a motion
        // that actually reaches back through this fence.
        while self.cells.get(start) == Some(&FieldCell::Backline) {
            start += 1;
        }
        // Both copies of a bold glyph must lie in the replayable suffix.
        let first_alias = self
            .raw_copy_aliases
            .partition_point(|&(copy, _)| copy < start);
        if start == self.cells.len()
            || self.write_cursor.retreat() > self.cells.len() - start
            || !detached_replay
                && start != 0
                && self.projection_length(start, self.cells.len()) == 0
                && self
                    .deferred_blanks
                    .last()
                    .is_none_or(|run| run.end <= start)
            || first_alias != 0 && self.raw_copy_aliases[first_alias - 1].1 >= start
            || self
                .raw_copy_aliases
                .get(first_alias)
                .is_some_and(|&(_, owner)| owner < start)
        {
            return None;
        }
        let cells = self.prefix_cells(start);
        Some(super::super::native_slots::FieldPrefix {
            start,
            slot_limit,
            deferred_tail: if complete_tail && start == 0 {
                if self.deferred_blanks.is_empty() {
                    super::super::native_slots::DeferredTail::Covered
                } else {
                    super::super::native_slots::DeferredTail::Replayable
                }
            } else {
                super::super::native_slots::DeferredTail::Absent
            },
            cells,
            projected_advances: Vec::new(),
            deferred_blanks: self.deferred_blank_offsets(start),
            automatic,
            cursor_retreat: self.write_cursor.retreat(),
            kept,
            tail_fonts: self.raw_tail_fonts[self
                .raw_tail_fonts
                .partition_point(|(range, _)| range.end <= start)..]
                .iter()
                .map(|(range, font)| (range.start.saturating_sub(start)..range.end - start, *font))
                .collect(),
            duplicates: self.raw_copy_aliases[first_alias..]
                .iter()
                .map(|&(copy, owner)| (copy - start, owner - start))
                .collect(),
            backbefore: self.backbefore_armed,
            backafter: self.backafter_armed,
            pending_cell: self
                .backbefore_graph
                .and_then(|index| index.checked_sub(start)),
            pass: if matches!(self.word_scan, super::WordScanPolicy::AwaitFlush)
                || self.projection_owner == super::FieldProjectionOwner::Detached
            {
                super::super::native_slots::ReplayPass::Editable
            } else {
                super::super::native_slots::ReplayPass::Incremental
            },
        })
    }

    fn prefix_cells(&self, start: usize) -> Vec<(FieldCell, usize)> {
        let mut cells = Vec::with_capacity(self.cells.len() - start);
        let trailing_marker =
            (self.cells.last() == Some(&FieldCell::BreakMarker)).then(|| self.cells.len() - 1);
        for (offset, cell) in self.cells[start..].iter().enumerate() {
            let index = start + offset;
            let length = self.projection_length(index, index + 1);
            let raw = match cell {
                FieldCell::DrawnSpaces(1) => FieldCell::BreakableBlank,
                // Keep the advance's origin when replay still owns the
                // adjacent break; a later operand can write through it.
                FieldCell::FixedDrawingAdvance(1) if trailing_marker != Some(index + 1) => {
                    FieldCell::NonBreakingBlank
                }
                _ => cell.clone(),
            };
            cells.push((raw, length));
        }
        cells
    }

    fn deferred_blank_offsets(&self, start: usize) -> Vec<usize> {
        let mut offsets = Vec::new();
        let first = self.deferred_blanks.partition_point(|run| run.end <= start);
        for run in &self.deferred_blanks[first..] {
            for cell in run.start.max(start)..run.end.min(self.cells.len()) {
                offsets.push(cell - start);
            }
        }
        offsets.sort_unstable();
        offsets.dedup();
        offsets
    }

    pub(in crate::mandoc::inline::flow) fn has_replayable_trailing_advance(&self) -> bool {
        self.resume == 0
            && self.cells.len() <= MAX_REPLAY_PREFIX_CELLS
            && (matches!(self.write_cursor, WriteCursor::Placed(_))
                // An ordinary append ending at a marker can retain the
                // same complete tape. It has not retired the field (term.c::term_word).
                || self.raw_slot_fence == 0 && matches!(self.write_cursor, WriteCursor::End)
                    && self.cells.last() == Some(&FieldCell::BreakMarker))
            && if self.deferred_blanks.is_empty() {
                self.raw_slot_fence == 0
                    && self.deferred_tail_tape
                        == super::super::native_slots::DeferredTail::Replayable
            } else {
                self.deferred_blanks
                    .iter()
                    .all(|run| run.start >= self.raw_slot_fence && run.source.replayable_tail)
            }
    }

    fn retained_cursor_fence(&self, fence: usize) -> usize {
        // A placed cursor can be to the left of the newest deferred
        // blank. The earlier raw edit proof still owns that slot, and
        // ReplaceLivePrefix will map each surviving blank's source.
        let landing = self.cells.len().saturating_sub(self.write_cursor.retreat());
        if let WriteCursor::Placed(1..) = self.write_cursor
            && (self.raw_edit_fence..fence).contains(&landing)
        {
            self.raw_edit_fence
        } else {
            fence
        }
    }

    fn has_editable_control_prefix(&self) -> bool {
        // A leading marker and authored spaces can share a buffered word
        // without printing anything. Their exact raw slots remain editable
        // before the first graph, including a deferred source-space owner.
        self.resume == 0
            && self.cells.len() <= MAX_REPLAY_PREFIX_CELLS
            && self.has_pending_break_markers()
            && self.cells.iter().all(|cell| {
                matches!(
                    cell,
                    FieldCell::BreakMarker
                        | FieldCell::BreakableBlank
                        | FieldCell::NonBreakingBlank
                        | FieldCell::Breakpoint
                        | FieldCell::TabReference
                        | FieldCell::ZeroWidthGraph
                )
            })
    }

    fn replay_fence(&self, live_replay: bool) -> usize {
        let Some(deferred) = self.deferred_blanks.last() else {
            return self.raw_edit_fence;
        };
        if live_replay
            && self.resume == 0
            && self.projected_pass_ends.is_empty()
            && self.raw_slot_fence == 0
            && self.cells.len() <= MAX_REPLAY_PREFIX_CELLS
        {
            0
        } else if self.has_retained_word_cursor() && deferred.end == self.raw_edit_fence {
            deferred.start
        } else if live_replay {
            self.raw_edit_fence.max(deferred.start)
        } else {
            self.raw_edit_fence.max(deferred.end)
        }
    }

    fn replace_live_prefix(
        &mut self,
        start: usize,
        positions: &[Option<usize>],
        trailing_blank_slots: &[(usize, usize)],
    ) {
        use super::cells::DeferredBlankRun;

        self.underline_strokes
            .truncate(self.underline_strokes.partition_point(|&cell| cell < start));
        // Keep earlier BACKBEFORE evidence when both the target and
        // its marker survive the overlay, including a target to its left.
        let first_retreat = self
            .pass_retreats
            .partition_point(|&(_, mark)| mark < start);
        let crossing: Vec<_> = self.pass_retreats[first_retreat..]
            .iter()
            .filter(|&&(target, _)| {
                self.projection_owner == super::FieldProjectionOwner::Detached || target < start
            })
            .filter_map(|&(old, mark)| {
                let target = if old < start {
                    old
                } else {
                    positions.get(old - start).copied().flatten()?
                };
                let new = positions.get(mark - start).copied().flatten()?;
                let backline = mark.checked_sub(1)?;
                let surviving_backline = if backline < start {
                    Some(backline)
                } else {
                    positions.get(backline - start).copied().flatten()
                };
                (self.projection_owner != super::FieldProjectionOwner::Detached
                    || self.cells[backline] == FieldCell::Backline
                        && surviving_backline == new.checked_sub(1))
                .then_some((target, new))
            })
            .collect();
        let first = self.deferred_blanks.partition_point(|run| run.end <= start);
        let mut surviving = Vec::new();
        for run in self.deferred_blanks.drain(first..) {
            if run.start < start {
                surviving.push(DeferredBlankRun { end: start, ..run });
            }
            for cell in run.start.max(start)..run.end {
                if let Some(&Some(projected)) = positions.get(cell - start) {
                    let trailing = trailing_blank_slots.iter().find_map(|&(owner, raw)| {
                        (run.source.generated_run_in && owner == cell).then_some(raw)
                    });
                    let new = trailing.unwrap_or(projected);
                    surviving.push(DeferredBlankRun {
                        start: new,
                        end: new + 1,
                        settled_trailing_run_in: trailing.is_some(),
                        ..run
                    });
                }
            }
        }
        if start == 0 {
            let scan = self.word_scan;
            let projection_owner = self.projection_owner;
            let deferred_tail_tape = self.deferred_tail_tape;
            self.clear();
            self.projection_owner = projection_owner;
            self.deferred_tail_tape = deferred_tail_tape;
            if matches!(scan, super::WordScanPolicy::AwaitFlush) {
                self.word_scan = scan;
            }
        } else {
            self.replace_placed_tail(self.cells.len() - start);
        }
        for (target, mark) in crossing {
            self.record_pass_retreat(target, mark);
        }
        surviving.sort_unstable_by_key(|run| run.start);
        self.deferred_blanks.extend(surviving);
    }

    fn has_replayable_projection(&self, index: usize) -> bool {
        match self.projection_length(index, index + 1) {
            0 => true,
            1 => matches!(
                self.cells[index],
                FieldCell::Graph { .. }
                    | FieldCell::Hyphen
                    | FieldCell::BreakableBlank
                    | FieldCell::NonBreakingBlank
                    | FieldCell::Tab
            ),
            _ => false,
        }
    }

    pub(in crate::mandoc::inline::flow) fn device_tail(
        &self,
    ) -> Option<super::super::native_slots::DeviceTail> {
        self.device_tail
            .as_deref()
            .filter(|tail| self.cells.len().saturating_sub(tail.fill_cells) >= self.resume)
            .cloned()
    }

    pub(in crate::mandoc::inline::flow) fn has_entry_retreat(&self) -> bool {
        self.backbefore_armed
    }

    pub(in crate::mandoc::inline::flow) const fn has_backafter_armed(&self) -> bool {
        self.backafter_armed
    }

    pub(in crate::mandoc::inline::flow) fn ends_with_unresolved_backline(&self) -> bool {
        // term_field skips NBRZW and the source tab reference. The next
        // word can supply the advance that actually covers this glyph.
        self.cells[self.resume..]
            .iter()
            .rev()
            .take(MAX_REPLAY_PREFIX_CELLS)
            .find(|cell| !matches!(cell, FieldCell::ZeroWidthGraph | FieldCell::TabReference))
            == Some(&FieldCell::Backline)
    }

    pub(in crate::mandoc::inline::flow) fn has_live_tab_reference(&self) -> bool {
        self.resume < self.cells.len() && self.cells.last() == Some(&FieldCell::TabReference)
    }

    pub(in crate::mandoc::inline::flow) fn device_gap_before(
        &self,
        end: usize,
        replayed: bool,
    ) -> Option<(usize, usize)> {
        let end = end.min(self.cells.len());
        let mut scan_end = end;
        loop {
            let mut probe = scan_end;
            while probe > self.resume
                && matches!(
                    self.cells[probe - 1],
                    FieldCell::TabReference | FieldCell::ZeroWidthGraph | FieldCell::Breakpoint
                )
            {
                probe -= 1;
            }
            if probe >= self.resume + 2
                && self.cells[probe - 1] == FieldCell::Backline
                && matches!(
                    self.cells[probe - 2],
                    FieldCell::Graph { width: 1.., .. } | FieldCell::Hyphen
                )
                && self.projection_length(probe - 2, probe - 1) == 0
            {
                // A font decoration pair (`_\b` or `c\b` from term.c::encode1)
                // occupies the incoming glyph's column, not the gap before it
                // (term.c:389-434, 911-921).
                scan_end = probe - 2;
            } else {
                break;
            }
        }
        let allow_replayed_blank = replayed && scan_end == end;
        let gap = self
            .nonbreaking_positions
            .range(..scan_end)
            .next_back()
            .max(
                self.blank_positions
                    .range(..scan_end)
                    .next_back()
                    .filter(|&&gap| {
                        allow_replayed_blank
                            || matches!(self.cells[gap], FieldCell::DrawnSpaces(1..))
                    }),
            );
        let &gap = gap?;
        if gap < self.resume || scan_end < end && self.projection_length(gap, gap + 1) > 0 {
            return None;
        }
        // term_field() defers these cells until the next printed graph.
        // Source-row references and zero-width operands do not end that
        // pending advance. A prior author graph makes it a word separator.
        let mut fixed = false;
        for index in (self.resume..scan_end).rev() {
            match self.cells[index] {
                FieldCell::FixedDrawingAdvance(1..) | FieldCell::DrawnSpaces(1..) => fixed = true,
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank if allow_replayed_blank => {
                    fixed = true;
                }
                FieldCell::TabReference | FieldCell::ZeroWidthGraph | FieldCell::Breakpoint => {}
                FieldCell::Backline if allow_replayed_blank && fixed => {}
                FieldCell::Graph { width: 1.., .. } | FieldCell::Hyphen => {
                    return (fixed && self.projection_length(index, index + 1) > 0)
                        .then_some((gap, index));
                }
                _ => return None,
            }
        }
        None
    }

    /// Measure buffered `HORIZ`/blank columns queued by `term_field()` before
    /// the incoming graph (`term.c:389-442`), skipping zero-width controls and
    /// overprinted decoration pairs.
    pub(in crate::mandoc::inline::flow) fn retained_motion_gap(
        &self,
        end: usize,
        word_start: usize,
        plain_output: bool,
    ) -> Option<RetainedMotionGap> {
        let end = (end..self.cells.len())
            .take(MAX_REPLAY_PREFIX_CELLS)
            .find(|&cell| {
                !matches!(
                    self.cells[cell],
                    FieldCell::ZeroWidthGraph
                        | FieldCell::Breakpoint
                        | FieldCell::TabReference
                        | FieldCell::BreakMarker
                )
            })?;
        if !matches!(self.cells[end], FieldCell::Graph { .. }) {
            return None;
        }
        // term_field() queues all spaces between printing cells. NBRZW,
        // TABREF and an in-band marker do not print the pending columns.
        // A later overwrite can keep them even if it removes the original
        // right-hand glyph and its reading separator.
        let mut width = 0usize;
        let mut saw_motion = false;
        // A run-in BODY can borrow the HEAD's native columns even when
        // their reading owner has already been detached. Its projection
        // counters cannot prove an internal gap in this payload.
        let mut projection_is_complete =
            self.projection_owner != super::FieldProjectionOwner::Detached;
        let mut gap = end;
        let floor = self
            .pass_start_for(end)
            .max(end.saturating_sub(MAX_REPLAY_PREFIX_CELLS));
        let (before_ink, stepped_over_projected_graph) =
            self.step_over_covered_prefix(end, floor, word_start);
        let mut projected_blanks = 0usize;
        let mut covered_before_gap = None;
        let mut can_cover_preceding_glyph = plain_output;
        for cell in (floor..before_ink).rev() {
            if covered_before_gap == Some(cell) {
                continue;
            }
            match self.cells[cell] {
                FieldCell::DrawnSpaces(count) | FieldCell::FixedDrawingAdvance(count) => {
                    saw_motion = true;
                    let known = self.deferred_blanks.partition_point(|run| run.end <= cell);
                    if self
                        .deferred_blanks
                        .get(known)
                        .is_some_and(|run| run.start <= cell)
                    {
                        if gap == end {
                            gap = cell;
                        }
                        continue;
                    }
                    gap = cell;
                    width = width.saturating_add(count);
                    let projected = self.projection_length(cell, cell + 1);
                    if cell < word_start {
                        projected_blanks = projected_blanks.saturating_add(projected);
                    }
                    projection_is_complete &= projected == count;
                }
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => {
                    if self.has_deferred_blank_at(cell) {
                        continue;
                    }
                    width += 1;
                    let projected = self.projection_length(cell, cell + 1);
                    if cell < word_start {
                        projected_blanks = projected_blanks.saturating_add(projected);
                    }
                    projection_is_complete &= projected == 1;
                }
                // A word-end request can put the advance on a different
                // accepted row from the backspace or the earlier ink.
                FieldCell::BreakMarker if covered_before_gap.is_some() => return None,
                FieldCell::BreakMarker => can_cover_preceding_glyph = false,
                FieldCell::ZeroWidthGraph | FieldCell::TabReference | FieldCell::Breakpoint => {}
                FieldCell::Backline
                    if can_cover_preceding_glyph
                        && saw_motion
                        && cell > floor
                        && covered_before_gap.is_none()
                        && self.gap_covers_preceding_glyph(cell - 1, gap) =>
                {
                    covered_before_gap = Some(cell - 1);
                }
                FieldCell::Graph { text, .. } if !text.is_whitespace() => {
                    return (saw_motion && (1..=MAX_REPLAY_PREFIX_CELLS).contains(&width))
                        .then_some(RetainedMotionGap {
                            marker: gap,
                            width,
                            previous_graph: cell,
                            projection_is_complete,
                            hidden_projected_blanks: if stepped_over_projected_graph {
                                projected_blanks
                            } else {
                                0
                            },
                        });
                }
                _ => return None,
            }
        }
        None
    }

    fn pass_start_for(&self, end: usize) -> usize {
        // Once incremental term_fill() commits an earlier pass, `self.resume`
        // advances past `end`; use that committed pass's start as the floor
        // (term.c:140-237, 389-434).
        if end < self.resume {
            let index = self
                .committed_passes
                .partition_point(|pass| pass.end <= end);
            self.committed_passes
                .get(index)
                .filter(|pass| pass.start <= end)
                .map_or(self.resume, |pass| pass.start)
        } else {
            self.resume
        }
    }

    fn step_over_covered_prefix(
        &self,
        end: usize,
        floor: usize,
        word_start: usize,
    ) -> (usize, bool) {
        let mut stepped_over_projected_graph = false;
        let mut before_ink = end;
        loop {
            let mut probe = before_ink;
            while probe > floor
                && matches!(
                    self.cells[probe - 1],
                    FieldCell::ZeroWidthGraph
                        | FieldCell::Breakpoint
                        | FieldCell::TabReference
                        | FieldCell::BreakMarker
                )
            {
                probe -= 1;
            }
            if probe >= floor + 2
                && self.cells[probe - 1] == FieldCell::Backline
                && let FieldCell::Graph { text, width: 1 } = self.cells[probe - 2]
            {
                let projected_graph = self.projection_length(probe - 2, probe - 1) > 0;
                let covered_by_retreat = !text.is_whitespace()
                    && projected_graph
                    && self
                        .pass_retreats
                        .binary_search_by_key(&(probe, probe - 2), |&(target, mark)| (mark, target))
                        .is_ok();
                if text.is_whitespace() || !projected_graph || covered_by_retreat {
                    // A decoration, covered graph, or fixed source space followed by
                    // a backline (possibly separated by zero-width controls skipped
                    // by term_field()) occupies the covering glyph's column, not
                    // the earlier gap (term.c:389-434).
                    stepped_over_projected_graph |= projected_graph && probe - 2 < word_start;
                    before_ink = probe - 2;
                    continue;
                }
            }
            break;
        }
        (before_ink, stepped_over_projected_graph)
    }

    fn gap_covers_preceding_glyph(&self, glyph: usize, gap: usize) -> bool {
        // term_field() releases spaces queued after its backspace before
        // the next printing cell. The retreat receipt identifies the
        // covered glyph, leaving the earlier ink as the gap's origin.
        let index = self
            .pass_retreats
            .partition_point(|&(_, mark)| mark <= glyph + 1);
        glyph >= self.raw_slot_fence
            && self.projection_length(glyph, glyph + 1) == 1
            && matches!(self.cells[glyph], FieldCell::Graph { text, width: 1 } if !text.is_whitespace())
            && self.pass_retreats[index..]
                .iter()
                .take_while(|&&(_, mark)| mark <= gap)
                .any(|&(target, _)| target == glyph)
    }

    pub(super) fn replace_placed_tail(&mut self, count: usize) {
        let start = self.cells.len().saturating_sub(count);
        debug_assert!(start >= self.resume);
        self.invalidate_projection_scan(start);
        if let Some(rejection) = &mut self.proved_rejection {
            rejection.min_raw_cursor = rejection.min_raw_cursor.min(start);
            if rejection.min_raw_cursor < rejection.scanned_end {
                self.proved_rejection = None;
            }
        }
        self.cells.truncate(start);
        self.truncate_raw_slot_proof(start);
        self.projection_prefix.truncate(start + 1);
        self.pass_retreats.truncate(
            self.pass_retreats
                .partition_point(|&(_, mark)| mark < start),
        );
        self.normalized_until = self.normalized_until.min(start);
        self.blank_positions.split_off(&start);
        self.nonbreaking_positions.split_off(&start);
        if self.last_graph_position.is_some_and(|last| last >= start) {
            self.last_graph_position = self
                .previous_graph_position
                .take()
                .filter(|&previous| previous < start);
        }
        self.word_first_content = self.word_first_content.filter(|&position| position < start);
        self.clear_backtracking();
    }

    pub(super) fn push_placed(&mut self, cell: &FieldCell, scalars: usize) {
        self.raw_slot_fence = self.cells.len() + 1;
        self.raw_edit_fence = self.raw_slot_fence;
        self.clear_backtracking();
        self.backafter_armed = false;
        let previous = self.projection_prefix.last().copied().unwrap_or(0);
        self.push_cell(cell.clone());
        *self.projection_prefix.last_mut().expect("placed cell") = previous + scalars;
    }

    pub(super) fn truncate_raw_slot_proof(&mut self, end: usize) {
        self.break_markers
            .truncate(self.break_markers.partition_point(|&marker| marker < end));
        self.underline_strokes
            .truncate(self.underline_strokes.partition_point(|&cell| cell < end));
        self.raw_slot_fence = self.raw_slot_fence.min(end);
        self.raw_edit_fence = self.raw_edit_fence.min(end);
        self.raw_tail_fonts.truncate(
            self.raw_tail_fonts
                .partition_point(|(range, _)| range.start < end),
        );
        if let Some((range, _)) = self.raw_tail_fonts.last_mut() {
            range.end = range.end.min(end);
        }
        self.raw_copy_aliases
            .retain(|&(copy, owner)| copy < end && owner < end);
    }

    pub(super) fn keep_placed_retreat(&mut self, from_end: usize) {
        let graph = self.cells.len().checked_sub(from_end);
        self.backbefore_armed = graph.is_some();
        self.backbefore_graph = graph;
        // term_field() flushes any projected blank buffered after the pending
        // graph before emitting the later backspace, skipping zero-width
        // controls (term.c:389-439, 901-908).
        self.backline_projection_target = graph.map(|start| {
            (start..self.cells.len())
                .rev()
                .find(|&cell| self.projection_length(cell, cell + 1) > 0)
                .unwrap_or(start)
        });
    }
}

/// A tbl input word executes independently. Consumer soft-wrap is still
/// responsive outside this explicit device-slot path.
pub(in crate::mandoc::inline) struct IsolatedPasses {
    pub(in crate::mandoc::inline) ranges: Vec<(Range<usize>, bool)>,
    pub(in crate::mandoc::inline) covered: Vec<usize>,
}

pub(in crate::mandoc::inline) fn isolated_passes(
    writes: &[FieldWrite],
    table_field_units: Option<usize>,
) -> IsolatedPasses {
    let mut field = FieldBuffer::default();
    field.apply_writes(writes);
    let mut passes = Vec::new();
    // Ordinary wrapping belongs to the viewport. Use the native margin
    // when term_fill/term_field can change a marker's acceptance or when
    // a backline covers a glyph only if its replacement prints on that pass.
    let target = table_field_units
        .filter(|_| !field.break_markers.is_empty() || field.has_pass_retreats())
        .unwrap_or(usize::MAX / 4);
    let mut remainder = false;
    let mut partial_motion = BTreeMap::new();
    while let Some(pass) = field.scan_fill_pass_with_motion(target, true, Some(&mut partial_motion))
    {
        remainder = field.has_non_ignorable_after(pass.end, false);
        passes.push(pass);
        if !remainder {
            break;
        }
        field.advance_past(pass.end);
        field.consume_break_blanks();
        if let Some(visited) = partial_motion.remove(&field.resume)
            && let FieldCell::FixedDrawingAdvance(count) = &mut field.cells[field.resume]
        {
            // The automatic wrap skips the converted prefix only. A
            // still-nonbreaking suffix keeps its original slot index.
            *count -= visited;
        }
    }
    let covered = field.covered_pass_retreats(&passes);
    let mut covered_iter = covered.iter().copied().peekable();
    let mut retreat_targets: Vec<_> = field
        .pass_retreats
        .iter()
        .map(|&(target, _)| target)
        .collect();
    retreat_targets.sort_unstable();
    retreat_targets.dedup();
    let mut retreats = retreat_targets.into_iter().peekable();
    let mut ranges = Vec::new();
    let mut start = 0;
    for (i, pass) in passes.iter().enumerate() {
        // Keep a device boundary when the next pass must not cover the
        // previous glyph. Ordinary width folds retain their source spaces
        // in the same reading range, including their styles and links.
        let mut split_retreat = false;
        while let Some(old) = retreats.next_if(|&old| old < pass.end) {
            while covered_iter.next_if(|&cover| cover < old).is_some() {}
            split_retreat |= covered_iter.next_if_eq(&old).is_none() && old >= pass.start;
        }
        let last = i + 1 == passes.len();
        let hard = if last {
            remainder // A completed row followed by term_fill's rejection.
        } else {
            let next = passes[i + 1];
            // A device-only accepted row ahead of rejection still occupies
            // a row of the native field, even with no projected glyphs.
            let before_device_only_row = remainder
                && next.boundary == FillBoundary::WordEndBreak
                && field.projection_length(next.start, next.end) == 0;
            // A word-end marker can sit in the skipped device advance at
            // the head of the next pass. Its boundary still separates the
            // two reading runs even though term_fill stopped on width.
            let leading_marker = device_advance_before_word_end(&field, pass.end..next.end);
            pass.boundary == FillBoundary::WordEndBreak
                || split_retreat
                || before_device_only_row
                || leading_marker
        };
        if hard || last {
            let end = field.printed_content_end(pass.start, pass.end);
            ranges.push((start..end, hard));
            if let Some(next) = passes.get(i + 1) {
                start = next.start;
            }
        }
    }
    IsolatedPasses { ranges, covered }
}

fn device_advance_before_word_end(field: &FieldBuffer, interval: Range<usize>) -> bool {
    let mut marker = false;
    let mut device_graph = false;
    for cell in interval {
        if field.projection_length(cell, cell + 1) != 0 {
            break;
        }
        marker |= field.cells[cell] == FieldCell::BreakMarker;
        device_graph |= matches!(field.cells[cell], FieldCell::Graph { width: 1.., .. });
    }
    // term_fill() may admit a separately drawn prefix on the new line.
    // A marker before an ordinary word only arms that word's own end.
    marker && device_graph
}
