//! Hand device-resolved writes to the same fill scanner as ordinary words.

use std::ops::Range;

use super::{FieldBuffer, FieldCell, FieldWrite, FillBoundary};

impl FieldBuffer {
    pub(super) fn apply_slot_placement(&mut self, write: &FieldWrite) {
        match write {
            FieldWrite::Placed { cell, scalars } => self.push_placed(cell, *scalars),
            FieldWrite::KeepPlacedRetreat { from_end } => self.keep_placed_retreat(*from_end),
            FieldWrite::ReplacePlacedTail { cells, .. } => self.replace_placed_tail(*cells),
            FieldWrite::RetainDeviceTail(tail) => self.device_tail = Some(tail.clone()),
            FieldWrite::ProveRawSlots {
                count,
                cursor_retreat,
                backbefore,
                duplicates,
            } => {
                self.raw_slot_fence = self.cells.len() - count;
                self.cursor_retreat = *cursor_retreat;
                self.backbefore_armed = *backbefore;
                self.raw_copy_aliases.clear();
                self.raw_copy_aliases
                    .extend(duplicates.iter().map(|&(copy, owner)| {
                        (self.raw_slot_fence + copy, self.raw_slot_fence + owner)
                    }));
            }
            FieldWrite::OpaqueRawSlots {
                styled_suffix,
                keeps_prior_slots,
            } => self.record_raw_tail_font(*styled_suffix, *keeps_prior_slots),
            FieldWrite::ReplaceLivePrefix { start: 0, .. } => self.clear(),
            FieldWrite::ReplaceLivePrefix { start, .. } => {
                self.replace_placed_tail(self.cells.len() - start);
            }
            FieldWrite::PlacedZeroWidthRetreat {
                target_from_end,
                mark_from_end,
            } => {
                let end = self.cells.len();
                self.zero_width_retreats
                    .push((end - target_from_end, end - mark_from_end));
            }
            _ => unreachable!("only resolved slot writes reach this path"),
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
        // A proved styled suffix on an otherwise exact append leaves the
        // previous prefix usable. Keep at most one compressed style run.
        if !(keeps_prior_slots && tail.is_some() && self.raw_tail_font.is_none()) {
            self.raw_slot_fence = tail.as_ref().map_or(end, |(range, _)| range.start);
            self.raw_copy_aliases.clear();
        }
        self.raw_tail_font = tail;
    }

    pub(in crate::mandoc::inline::flow) fn field_prefix(
        &self,
        automatic: usize,
        kept: bool,
    ) -> Option<super::super::native_slots::FieldPrefix> {
        const MAX_REPLAY_PREFIX_CELLS: usize = 64;
        if self.resume != 0
            || matches!(self.word_scan, super::WordScanPolicy::UnmodeledRewind)
            || !self.projected_pass_ends.is_empty()
            || !self.deferred_blanks.is_empty()
        {
            return None;
        }
        let start = self
            .raw_slot_fence
            .max(self.cells.len().saturating_sub(MAX_REPLAY_PREFIX_CELLS));
        // A covered glyph outside this suffix still needs the later
        // overstrike on the same accepted pass. Do not cut through it.
        let first_retreat = self
            .zero_width_retreats
            .partition_point(|&(_, mark)| mark < start);
        // Both copies of a bold glyph must lie in the replayable suffix.
        let first_alias = self
            .raw_copy_aliases
            .partition_point(|&(copy, _)| copy < start);
        if self.cursor_retreat > self.cells.len() - start
            || *self.cells.get(start)? == FieldCell::Backline
            || start != 0 && self.projection_length(start, self.cells.len()) == 0
            || first_alias != 0 && self.raw_copy_aliases[first_alias - 1].1 >= start
            || self
                .raw_copy_aliases
                .get(first_alias)
                .is_some_and(|&(_, owner)| owner < start)
            || self
                .zero_width_retreats
                .get(first_retreat)
                .is_some_and(|&(old, _)| old < start)
        {
            return None;
        }
        let mut cells = Vec::with_capacity(self.cells.len() - start);
        for (offset, cell) in self.cells[start..].iter().enumerate() {
            let index = start + offset;
            if matches!(
                cell,
                FieldCell::DrawnSpaces(2..) | FieldCell::FixedDrawingAdvance(2..)
            ) {
                return None;
            }
            let length = self.projection_length(index, index + 1);
            if length > 1
                || length > 0
                    && !matches!(
                        cell,
                        FieldCell::Graph { .. }
                            | FieldCell::Hyphen
                            | FieldCell::BreakableBlank
                            | FieldCell::NonBreakingBlank
                            | FieldCell::Tab
                    )
            {
                return None;
            }
            let raw = match cell {
                FieldCell::DrawnSpaces(1) => FieldCell::BreakableBlank,
                FieldCell::FixedDrawingAdvance(1) => FieldCell::NonBreakingBlank,
                _ => cell.clone(),
            };
            cells.push((raw, length));
        }
        Some(super::super::native_slots::FieldPrefix {
            start,
            cells,
            automatic,
            cursor_retreat: self.cursor_retreat,
            kept,
            tail_font: self
                .raw_tail_font
                .as_ref()
                .filter(|(range, _)| range.end > start)
                .map(|(range, font)| (range.start.saturating_sub(start)..range.end - start, *font)),
            duplicates: self.raw_copy_aliases[first_alias..]
                .iter()
                .map(|&(copy, owner)| (copy - start, owner - start))
                .collect(),
            backbefore: self.backbefore_armed,
            backafter: self.backafter_armed,
            pending_cell: self
                .backbefore_graph
                .and_then(|index| index.checked_sub(start)),
        })
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

    pub(in crate::mandoc::inline::flow) fn has_live_tab_reference(&self) -> bool {
        self.resume < self.cells.len() && self.cells.last() == Some(&FieldCell::TabReference)
    }

    pub(in crate::mandoc::inline::flow) fn has_device_gap_before(
        &self,
        end: usize,
        replayed: bool,
    ) -> bool {
        let end = end.min(self.cells.len());
        let gap = self.nonbreaking_positions.range(..end).next_back().max(
            self.blank_positions
                .range(..end)
                .next_back()
                .filter(|&&gap| replayed || matches!(self.cells[gap], FieldCell::DrawnSpaces(1..))),
        );
        let Some(&gap) = gap else {
            return false;
        };
        if gap < self.resume {
            return false;
        }
        // term_field() defers these cells until the next printed graph.
        // Source-row references and zero-width operands do not end that
        // pending advance. A prior author graph makes it a word separator.
        let mut fixed = false;
        for index in (self.resume..end).rev() {
            match self.cells[index] {
                FieldCell::FixedDrawingAdvance(1..) | FieldCell::DrawnSpaces(1..) => fixed = true,
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank if replayed => fixed = true,
                FieldCell::TabReference | FieldCell::ZeroWidthGraph | FieldCell::Breakpoint => {}
                FieldCell::Backline if replayed && fixed => {}
                FieldCell::Graph { width: 1.., .. } | FieldCell::Hyphen => {
                    return fixed && self.projection_length(index, index + 1) > 0;
                }
                _ => return false,
            }
        }
        false
    }

    pub(super) fn replace_placed_tail(&mut self, count: usize) {
        let start = self.cells.len().saturating_sub(count);
        debug_assert!(start >= self.resume);
        self.invalidate_projection_scan(start);
        self.cells.truncate(start);
        self.truncate_raw_slot_proof(start);
        self.projection_prefix.truncate(start + 1);
        self.zero_width_retreats.truncate(
            self.zero_width_retreats
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
        self.clear_backtracking();
        self.backafter_armed = false;
        if *cell == FieldCell::BreakMarker {
            self.last_break_marker = Some(self.cells.len());
        }
        let previous = self.projection_prefix.last().copied().unwrap_or(0);
        self.push_cell(cell.clone());
        *self.projection_prefix.last_mut().expect("placed cell") = previous + scalars;
    }

    pub(super) fn truncate_raw_slot_proof(&mut self, end: usize) {
        self.raw_slot_fence = self.raw_slot_fence.min(end);
        if let Some((range, _)) = &mut self.raw_tail_font {
            range.end = range.end.min(end);
            if range.start >= range.end {
                self.raw_tail_font = None;
            }
        }
        self.raw_copy_aliases
            .retain(|&(copy, owner)| copy < end && owner < end);
    }

    pub(super) fn keep_placed_retreat(&mut self, from_end: usize) {
        let graph = self.cells.len().checked_sub(from_end);
        self.backbefore_armed = graph.is_some();
        self.backbefore_graph = graph;
        self.backline_projection_target = graph;
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
        .filter(|_| field.last_break_marker.is_some() || field.has_zero_width_retreats())
        .unwrap_or(usize::MAX / 4);
    let mut remainder = false;
    while let Some(pass) = field.scan_fill_pass(target, true) {
        remainder = field.has_non_ignorable_after(pass.end, false);
        passes.push(pass);
        if !remainder {
            break;
        }
        field.advance_past(pass.end);
        field.consume_break_blanks();
    }
    let covered = field.covered_zero_width_retreats(&passes);
    let mut covered_iter = covered.iter().copied().peekable();
    let mut retreats = field.zero_width_retreats.iter().copied().peekable();
    let mut ranges = Vec::new();
    let mut start = 0;
    for (i, pass) in passes.iter().enumerate() {
        // Keep a device boundary when the next pass must not cover the
        // previous glyph. Ordinary width folds retain their source spaces
        // in the same reading range, including their styles and links.
        let mut split_retreat = false;
        while let Some((old, _)) = retreats.next_if(|&(old, _)| old < pass.end) {
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
            let leading_marker = (pass.end..next.end)
                .take_while(|&cell| field.projection_length(next.start, cell.max(next.start)) == 0)
                .any(|cell| field.cells[cell] == FieldCell::BreakMarker);
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
