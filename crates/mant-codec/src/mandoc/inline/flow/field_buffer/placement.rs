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
            FieldWrite::OpaqueRawSlots => self.opaque_raw_slots = true,
            FieldWrite::ReplaceLivePrefix { .. } => self.clear(),
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

    pub(in crate::mandoc::inline::flow) fn field_prefix(
        &self,
        automatic: usize,
        kept: bool,
    ) -> Option<super::super::native_slots::FieldPrefix> {
        const MAX_REPLAY_PREFIX_CELLS: usize = 64;
        if self.cells.is_empty()
            || self.cells.len() > MAX_REPLAY_PREFIX_CELLS
            || self.opaque_raw_slots
            || self.resume != 0
            || !self.projected_pass_ends.is_empty()
            || !self.deferred_blanks.is_empty()
            || self.has_zero_width_retreats()
        {
            return None;
        }
        let mut cells = Vec::with_capacity(self.cells.len());
        for (index, cell) in self.cells.iter().enumerate() {
            if matches!(
                cell,
                FieldCell::DrawnSpaces(_) | FieldCell::FixedDrawingAdvance(_) | FieldCell::Backline
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
            cells.push((cell.clone(), length));
        }
        Some(super::super::native_slots::FieldPrefix {
            cells,
            automatic,
            kept,
            backbefore: self.backbefore_armed,
            backafter: self.backafter_armed,
            pending_cell: self.backbefore_graph,
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

    pub(super) fn replace_placed_tail(&mut self, count: usize) {
        let start = self.cells.len().saturating_sub(count);
        debug_assert!(start >= self.resume);
        self.invalidate_projection_scan(start);
        self.cells.truncate(start);
        self.projection_prefix.truncate(start + 1);
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
        self.opaque_raw_slots = true;
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
pub(in crate::mandoc::inline) fn isolated_passes(
    writes: &[FieldWrite],
    table_field_units: Option<usize>,
) -> Vec<(Range<usize>, bool)> {
    let mut field = FieldBuffer::default();
    field.apply_writes(writes);
    let mut passes = Vec::new();
    let target = table_field_units.unwrap_or(usize::MAX / 4);
    while let Some(pass) = field.scan_fill_pass(target, true) {
        let end = field.printed_content_end(pass.start, pass.end);
        let closes = field.has_non_ignorable_after(pass.end, false);
        passes.push((pass.start..end, closes));
        if !closes {
            break;
        }
        field.advance_past(pass.end);
        field.consume_break_blanks();
    }
    passes
}
