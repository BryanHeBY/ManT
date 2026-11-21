//! Retain author ownership when an accepted terminal pass revisits columns.

use super::super::placement::MAX_REPLAY_PREFIX_CELLS;
use super::{FieldBuffer, FieldCell};

pub(super) struct DeviceOverprint {
    pub(super) covered: Vec<(usize, Option<usize>)>,
    pub(super) ordered: Vec<usize>,
    pub(super) overhang: Option<String>,
}

impl FieldBuffer {
    /// `term_field()` emits backspaces and queues blanks independently of
    /// zero-width fill controls. Before acceptance, a repeated backspace
    /// may appear to leave its local graph run because a marker or blank
    /// separates it from the earlier glyph. This pass knows the real row.
    pub(super) fn prove_device_overprint(
        &self,
        start: usize,
        end: usize,
    ) -> Option<DeviceOverprint> {
        if start < self.raw_slot_fence {
            return None;
        }
        let end = self.printed_content_end(start, end);
        let cells = &self.cells[start..end];
        if !cells[..cells.len().min(MAX_REPLAY_PREFIX_CELLS)]
            .windows(2)
            .any(|cells| cells == [FieldCell::Backline, FieldCell::Backline])
        {
            return None;
        }
        let last_backline = cells
            .iter()
            .rposition(|cell| *cell == FieldCell::Backline)?;
        let (mut proof, stepped_left) =
            self.paint_device_interval(start, cells, last_backline, 1)?;
        if stepped_left {
            // term_field() writes the backspace (term.c:430–434). Whether
            // the terminal can step one cell before this field depends on
            // the device origin. Require the same owned ink with and
            // without that one cell. Larger retreats remain unproved.
            let (clipped, _) = self.paint_device_interval(start, cells, last_backline, 0)?;
            if proof.ordered != clipped.ordered
                || !proof
                    .covered
                    .iter()
                    .map(|&(target, _)| target)
                    .eq(clipped.covered.iter().map(|&(target, _)| target))
            {
                return None;
            }
            for ((_, by), (_, other)) in proof.covered.iter_mut().zip(clipped.covered) {
                if *by != other {
                    *by = None;
                }
            }
            proof.overhang = None;
        }
        Some(proof)
    }

    fn paint_device_interval(
        &self,
        start: usize,
        cells: &[FieldCell],
        last_backline: usize,
        origin: usize,
    ) -> Option<(DeviceOverprint, bool)> {
        let mut cursor = origin;
        let mut extent = origin;
        let mut stepped_left = false;
        let mut row = [None; MAX_REPLAY_PREFIX_CELLS + 1];
        let mut covered_by = [None; MAX_REPLAY_PREFIX_CELLS];
        let aliases = &self.raw_copy_aliases;
        let owner_at = |cell| {
            aliases
                .binary_search_by_key(&cell, |&(copy, _)| copy)
                .ok()
                .map_or(cell, |index| aliases[index].1)
        };
        for (offset, cell) in cells.iter().enumerate() {
            // Beyond the last retreat and its landing area the device can
            // only advance. That suffix needs neither storage nor a proved
            // character width to decide what is still visible to its left.
            if offset > last_backline && cursor == extent {
                break;
            }
            if offset >= MAX_REPLAY_PREFIX_CELLS {
                return None;
            }
            let index = start + offset;
            match cell {
                FieldCell::Graph { text, width: 1 } if text.is_ascii() => {}
                FieldCell::Hyphen
                | FieldCell::BreakableBlank
                | FieldCell::NonBreakingBlank
                | FieldCell::FixedDrawingAdvance(1) => {}
                FieldCell::BreakMarker
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint
                | FieldCell::TabReference => continue,
                FieldCell::Backline => {
                    cursor = if origin == 0 {
                        cursor.saturating_sub(1)
                    } else {
                        cursor.checked_sub(1)?
                    };
                    stepped_left |= cursor < origin;
                    continue;
                }
                _ => return None,
            }
            if let Some(previous) = row[cursor].replace(index) {
                let owner = owner_at(previous).checked_sub(start)?;
                if owner >= MAX_REPLAY_PREFIX_CELLS {
                    return None;
                }
                covered_by[owner] = Some(index);
            }
            cursor += 1;
            extent = extent.max(cursor);
        }
        let overhang = self.prove_ink_overhang(&row[..extent], cursor, owner_at);
        let mut ordered = Vec::new();
        for cell in row.into_iter().flatten() {
            // Bold bytes are two copies of one author's glyph. Keep its
            // publication wherever either copy survives.
            let cell = owner_at(cell);
            let owner = cell.checked_sub(start)?;
            if owner >= MAX_REPLAY_PREFIX_CELLS {
                return None;
            }
            covered_by[owner] = None;
            if self.projected_ink(cell) && !ordered.contains(&cell) {
                ordered.push(cell);
            }
        }
        let mut source_order = ordered.clone();
        source_order.sort_unstable();
        let first = ordered.iter().zip(&source_order).position(|(a, b)| a != b);
        let ordered = first.map_or_else(Vec::new, |first| {
            let tail = ordered
                .iter()
                .zip(&source_order)
                .rev()
                .take_while(|(a, b)| a == b)
                .count();
            ordered[first..ordered.len() - tail].to_vec()
        });
        let proof = DeviceOverprint {
            covered: covered_by
                .into_iter()
                .enumerate()
                .filter_map(|(offset, by)| {
                    let target = start + offset;
                    by.filter(|_| self.projected_ink(target) || self.has_deferred_blank_at(target))
                        .map(|by| (target, Some(by)))
                })
                .collect(),
            ordered,
            overhang,
        };
        Some((proof, stepped_left))
    }

    // A retreat can leave already-painted ink beyond viscol. Keep a
    // proof only when the suffix has complete, distinct glyph owners.
    fn prove_ink_overhang(
        &self,
        row: &[Option<usize>],
        start: usize,
        owner_at: impl Fn(usize) -> usize,
    ) -> Option<String> {
        if start == row.len() {
            return None;
        }
        row[start..]
            .iter()
            .enumerate()
            .map(|(offset, &cell)| {
                let cell = owner_at(cell?);
                if !self.projected_ink(cell)
                    || row[..start + offset]
                        .iter()
                        .flatten()
                        .any(|&other| owner_at(other) == cell)
                {
                    return None;
                }
                match self.cells[cell] {
                    FieldCell::Graph { text, width: 1 } => Some(text),
                    FieldCell::Hyphen => Some('-'),
                    _ => None,
                }
            })
            .collect()
    }

    fn projected_ink(&self, cell: usize) -> bool {
        matches!(
            self.cells[cell],
            FieldCell::Graph { .. } | FieldCell::Hyphen
        ) && self.projection_length(cell, cell + 1) == 1
    }

    fn has_deferred_blank_at(&self, cell: usize) -> bool {
        let index = self.deferred_blanks.partition_point(|run| run.end <= cell);
        self.deferred_blanks
            .get(index)
            .is_some_and(|run| run.start <= cell)
    }
}
