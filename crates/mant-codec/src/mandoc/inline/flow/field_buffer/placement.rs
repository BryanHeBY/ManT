//! Hand device-resolved writes to the same fill scanner as ordinary words.

use std::ops::Range;

use super::{FieldBuffer, FieldCell, FieldWrite};

impl FieldBuffer {
    pub(super) fn apply_slot_placement(&mut self, write: &FieldWrite) {
        match write {
            FieldWrite::Placed { cell, scalars } => self.push_placed(cell, *scalars),
            FieldWrite::KeepPlacedRetreat { from_end } => self.keep_placed_retreat(*from_end),
            FieldWrite::ReplacePlacedTail { cells, .. } => self.replace_placed_tail(*cells),
            FieldWrite::RetainDeviceTail(tail) => self.device_tail = Some(tail.clone()),
            FieldWrite::ProveAppendSlots {
                count,
                backbefore,
                duplicate,
            } => {
                self.raw_slot_fence = self.cells.len() - count;
                self.backbefore_armed = *backbefore;
                self.raw_copy_alias = duplicate
                    .map(|(copy, owner)| (self.raw_slot_fence + copy, self.raw_slot_fence + owner));
            }
            FieldWrite::OpaqueRawSlots { last_font } => self.record_raw_tail_font(*last_font),
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

    fn record_raw_tail_font(&mut self, last_font: Option<crate::mandoc::roff_escape::RoffFont>) {
        let end = self.cells.len();
        self.raw_tail_font = last_font
            .zip(end.checked_sub(1))
            .filter(|&(_, index)| self.projection_length(index, end) == 1)
            .map(|(font, index)| (index, font));
        self.raw_slot_fence = self.raw_tail_font.map_or(end, |(index, _)| index);
        self.raw_copy_alias = None;
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
        if *self.cells.get(start)? == FieldCell::Backline
            || start != 0 && self.projection_length(start, self.cells.len()) == 0
            || self
                .raw_copy_alias
                .is_some_and(|(copy, owner)| copy < start && owner >= start)
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
            kept,
            tail_font: self
                .raw_tail_font
                .and_then(|(index, font)| Some((index.checked_sub(start)?, font))),
            duplicate: self
                .raw_copy_alias
                .and_then(|(copy, owner)| Some((copy.checked_sub(start)?, owner - start))),
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

    pub(in crate::mandoc::inline::flow) fn has_device_gap_before(&self, end: usize) -> bool {
        let end = end.min(self.cells.len());
        let Some(&gap) = self.nonbreaking_positions.range(..end).next_back() else {
            return false;
        };
        if gap < self.resume || !matches!(self.cells[gap], FieldCell::FixedDrawingAdvance(1..)) {
            return false;
        }
        // term_field() defers these cells until the next printed graph.
        // Source-row references and zero-width operands do not end that
        // pending advance. A prior author graph makes it a word separator.
        let mut fixed = false;
        for index in (self.resume..end).rev() {
            match self.cells[index] {
                FieldCell::FixedDrawingAdvance(1..) => fixed = true,
                FieldCell::TabReference | FieldCell::ZeroWidthGraph | FieldCell::Breakpoint => {}
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
        self.raw_slot_fence = self.raw_slot_fence.min(start);
        self.raw_tail_font = self.raw_tail_font.filter(|&(position, _)| position < start);
        self.raw_copy_alias = self.raw_copy_alias.filter(|&(_, owner)| owner < start);
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
    let mut ranges = Vec::new();
    let target = table_field_units.unwrap_or(usize::MAX / 4);
    while let Some(pass) = field.scan_fill_pass(target, true) {
        let end = field.printed_content_end(pass.start, pass.end);
        let closes = field.has_non_ignorable_after(pass.end, false);
        ranges.push((pass.start..end, closes));
        passes.push(pass);
        if !closes {
            break;
        }
        field.advance_past(pass.end);
        field.consume_break_blanks();
    }
    IsolatedPasses {
        ranges,
        covered: field.covered_zero_width_retreats(&passes),
    }
}
