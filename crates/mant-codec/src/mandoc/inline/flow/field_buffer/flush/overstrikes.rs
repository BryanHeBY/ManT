//! Retain author ownership when an accepted terminal pass revisits columns.

use super::super::placement::MAX_REPLAY_PREFIX_CELLS;
use super::{FieldBuffer, FieldCell};

impl FieldBuffer {
    /// `term_field()` emits backspaces and queues blanks independently of
    /// zero-width fill controls. Before acceptance, a repeated backspace
    /// may appear to leave its local graph run because a marker or blank
    /// separates it from the earlier glyph. This pass knows the real row.
    pub(super) fn prove_repeated_device_covers(
        &self,
        start: usize,
        end: usize,
    ) -> Option<Vec<(usize, usize)>> {
        if start < self.raw_slot_fence
            || end - start > MAX_REPLAY_PREFIX_CELLS
            || !self.cells[start..end]
                .windows(2)
                .any(|cells| cells == [FieldCell::Backline, FieldCell::Backline])
        {
            return None;
        }
        let end = self.printed_content_end(start, end);
        let mut cursor = 0usize;
        let mut row = [None; MAX_REPLAY_PREFIX_CELLS];
        let mut covered_by = [None; MAX_REPLAY_PREFIX_CELLS];
        let aliases = &self.raw_copy_aliases;
        let owner_at = |cell| {
            aliases
                .binary_search_by_key(&cell, |&(copy, _)| copy)
                .ok()
                .map_or(cell, |index| aliases[index].1)
        };
        for (offset, cell) in self.cells[start..end].iter().enumerate() {
            let index = start + offset;
            match cell {
                FieldCell::Graph { text, width: 1 } if text.is_ascii() => {}
                FieldCell::Hyphen | FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => {}
                FieldCell::BreakMarker
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint
                | FieldCell::TabReference => continue,
                FieldCell::Backline => {
                    // The formatter's measuring position is separate from
                    // the emitted device backspace. An underflow would need
                    // the preceding field and is outside this proof.
                    cursor = cursor.checked_sub(1)?;
                    continue;
                }
                _ => return None,
            }
            if let Some(previous) = row[cursor].replace(index) {
                let owner = owner_at(previous).checked_sub(start)?;
                if owner >= end - start {
                    return None;
                }
                covered_by[owner] = Some(index);
            }
            cursor += 1;
        }
        for cell in row.into_iter().flatten() {
            // Bold bytes are two copies of one author's glyph. Keep its
            // publication wherever either copy survives.
            let owner = owner_at(cell).checked_sub(start)?;
            if owner >= end - start {
                return None;
            }
            covered_by[owner] = None;
        }
        Some(
            covered_by
                .into_iter()
                .enumerate()
                .filter_map(|(offset, by)| {
                    let target = start + offset;
                    by.filter(|_| {
                        matches!(
                            self.cells[target],
                            FieldCell::Graph { .. } | FieldCell::Hyphen
                        ) && self.projection_length(target, target + 1) == 1
                    })
                    .map(|by| (target, by))
                })
                .collect(),
        )
    }
}
