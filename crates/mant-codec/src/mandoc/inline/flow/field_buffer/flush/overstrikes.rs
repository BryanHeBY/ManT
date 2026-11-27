//! Retain author ownership when an accepted terminal pass revisits columns.

use super::super::placement::MAX_REPLAY_PREFIX_CELLS;
use super::{FieldBuffer, FieldCell, FillPass};

pub(super) struct DeviceOverprint {
    pub(super) covered: Vec<(usize, Option<usize>)>,
    pub(super) order_groups: Vec<Vec<usize>>,
    pub(super) overhang: Option<String>,
}

impl FieldBuffer {
    pub(in crate::mandoc::inline::flow) fn record_plain_paint_origins(
        &mut self,
        passes: &[FillPass],
        first: usize,
        rest: usize,
    ) {
        // A word-time pass may have used an earlier row origin. Only the
        // real retirement knows the origins of the passes it still owns.
        for (index, pass) in passes.iter().enumerate() {
            if self.fresh_paint_pass(pass)
                && (emits_backspace_after_control_or_advance(&self.cells[pass.start..pass.end])
                    || self.edited_font_copy_end(pass.start, pass.end).is_some())
            {
                self.plain_paint_origins
                    .push((pass.start, if index == 0 { first } else { rest }));
            }
        }
    }

    /// Record the `vbl` leading-padding origin (`term.c:113-116, 229-230`) for
    /// fresh definition passes whose first printed `Backline` or edited font
    /// copy (`term.c:425-436`) can step back into emitted padding.
    pub(in crate::mandoc::inline::flow) fn record_definition_paint_origins(
        &mut self,
        passes: &[FillPass],
        origins: &[(usize, usize)],
    ) {
        if origins.is_empty() {
            return;
        }
        for pass in passes {
            if !self.fresh_paint_pass(pass)
                || self
                    .plain_paint_origins
                    .last()
                    .is_some_and(|&(start, _)| start >= pass.start)
            {
                continue;
            }
            let Ok(index) = origins.binary_search_by_key(&pass.start, |&(start, _)| start) else {
                continue;
            };
            let printed_end = self.printed_content_end(pass.start, pass.end);
            let printed = &self.cells[pass.start..printed_end];
            if emits_backspace_after_control_or_advance(printed)
                || printed.last() == Some(&FieldCell::Backline)
                || self.edited_font_copy_end(pass.start, pass.end).is_some()
            {
                self.plain_paint_origins
                    .push((pass.start, origins[index].1));
            }
        }
    }

    pub(in crate::mandoc::inline::flow) const fn has_plain_paint_origins(&self) -> bool {
        !self.plain_paint_origins.is_empty()
    }

    pub(super) fn plain_paint_origin(&self, pass: &FillPass) -> Option<usize> {
        let index = self
            .plain_paint_origins
            .binary_search_by_key(&pass.start, |&(start, _)| start)
            .ok()?;
        Some(self.plain_paint_origins[index].1)
    }

    fn fresh_paint_pass(&self, pass: &FillPass) -> bool {
        pass.start >= self.raw_slot_fence
            && pass.start >= self.resume
            && !self.has_projected_pass(pass.end)
    }

    /// True when a cursor retreat (`term.c:694-718, 903-910`) left an
    /// `encode1()` font copy (`term.c:911-925`) separated from its owner by
    /// something other than an adjacent `'\b'`.
    pub(in crate::mandoc::inline::flow) fn has_edited_font_copies(&self) -> bool {
        self.raw_copy_aliases.iter().any(|&(copy, owner)| {
            let low = copy.min(owner);
            let high = copy.max(owner);
            high != low + 2 || self.cells.get(low + 1) != Some(&FieldCell::Backline)
        })
    }

    /// An editable raw word can overwrite a font backspace and leave both
    /// copies printable. Ordinary decoration is exactly glyph, BS, glyph
    /// (`term.c:911-925`) and never needs a paint pass on its own.
    fn edited_font_copy_end(&self, start: usize, end: usize) -> Option<usize> {
        let end = end.min(start.saturating_add(self.replay_slot_limit()));
        let first = self
            .raw_copy_aliases
            .partition_point(|&(copy, _)| copy < start);
        self.raw_copy_aliases[first..]
            .iter()
            .take_while(|&&(copy, _)| copy < end)
            .filter_map(|&(copy, owner)| {
                let low = copy.min(owner);
                let high = copy.max(owner);
                (start <= low
                    && high < end
                    && (high != low + 2 || self.cells[low + 1] != FieldCell::Backline))
                    .then_some(high - start)
            })
            .max()
    }

    /// `term_field()` (`term.c:387-443`) skips zero-width controls (`394-398`),
    /// queues blanks until the next non-blank (`402-428`), and emits `'\b'`
    /// backspaces against `viscol` (`432-436`). Before acceptance, a repeated
    /// backspace may appear separated by a marker or blank; this pass proves
    /// the real row.
    pub(super) fn prove_device_overprint(
        &self,
        start: usize,
        end: usize,
        blank_origin: Option<usize>,
    ) -> Option<DeviceOverprint> {
        if start < self.raw_slot_fence {
            return None;
        }
        let end = self.printed_content_end(start, end);
        let cells = &self.cells[start..end];
        let detached = self.projection_owner == super::super::FieldProjectionOwner::Detached;
        let edited_font_end = self.edited_font_copy_end(start, end);
        if !detached
            && !if blank_origin.is_some() {
                edited_font_end.is_some()
                    || emits_backspace_after_control_or_advance(cells)
                    || cells.last() == Some(&FieldCell::Backline)
            } else {
                edited_font_end.is_some()
                    || cells[..cells.len().min(MAX_REPLAY_PREFIX_CELLS)]
                        .windows(2)
                        .any(|cells| cells == [FieldCell::Backline, FieldCell::Backline])
            }
        {
            return None;
        }
        let last_edit = cells
            .iter()
            .rposition(|cell| *cell == FieldCell::Backline)
            .max(edited_font_end)?;
        if let Some(origin) = blank_origin {
            // term_field() releases the verified padding before emitting
            // the backspaces. A retreat at physical column zero clips; a
            // retreat into that padding can keep earlier ink on its right.
            let origin = origin.min(MAX_REPLAY_PREFIX_CELLS);
            return self
                .paint_device_interval(start, cells, last_edit, origin, true, true)
                .map(|(proof, _)| proof);
        }
        let keep_distinct = edited_font_end.is_some();
        let (mut proof, stepped_left) =
            self.paint_device_interval(start, cells, last_edit, 1, false, keep_distinct)?;
        if stepped_left {
            // term_field() writes the backspace (term.c:430–434). Whether
            // the terminal can step one cell before this field depends on
            // the device origin. Require the same owned ink with and
            // without that one cell. Larger retreats remain unproved.
            let (clipped, _) =
                self.paint_device_interval(start, cells, last_edit, 0, true, keep_distinct)?;
            if proof.order_groups != clipped.order_groups
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
        last_edit: usize,
        origin: usize,
        clip_left: bool,
        keep_distinct_copies: bool,
    ) -> Option<(DeviceOverprint, bool)> {
        let mut cursor = origin;
        let mut extent = origin;
        let mut stepped_left = false;
        let slot_limit = self.replay_slot_limit();
        let mut row = vec![None; slot_limit * 2];
        let mut covered_by = vec![None; cells.len().min(slot_limit)];
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
            if offset > last_edit && cursor == extent {
                break;
            }
            if offset >= slot_limit {
                return None;
            }
            let index = start + offset;
            match cell {
                FieldCell::Graph { width: 1, .. }
                | FieldCell::Hyphen
                | FieldCell::BreakableBlank
                | FieldCell::NonBreakingBlank
                // A queued one-column drawn SP has the same width as
                // an ordinary SP in the admitted row (term.c:389–434).
                | FieldCell::DrawnSpaces(1) => {}
                FieldCell::FixedDrawingAdvance(width)
                    if *width > 0 && cursor.saturating_add(*width) <= row.len() =>
                {
                    for _ in 0..*width {
                        if let Some(previous) = row[cursor].replace(index) {
                            let owner = owner_at(previous).checked_sub(start)?;
                            *covered_by.get_mut(owner)? = Some(index);
                        }
                        cursor += 1;
                    }
                    extent = extent.max(cursor);
                    continue;
                }
                FieldCell::BreakMarker
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint
                | FieldCell::TabReference => continue,
                FieldCell::Backline => {
                    cursor = if clip_left {
                        cursor.saturating_sub(1)
                    } else {
                        cursor.checked_sub(1)?
                    };
                    stepped_left |= cursor < origin;
                    continue;
                }
                _ => return None,
            }
            let column = row.get_mut(cursor)?;
            // term_field() emits both the backspace and the stroke
            // (term.c:425–436). An unpaired automatic underline can paint
            // over the existing character without owning its reading.
            if self.projection_owner == super::super::FieldProjectionOwner::Detached
                && self.underline_strokes.binary_search(&index).is_ok()
                && cells.get(offset + 1) != Some(&FieldCell::Backline)
                && column.is_some_and(|previous| self.projected_ink(owner_at(previous)))
            {
                cursor += 1;
                continue;
            }
            if let Some(previous) = column.replace(index) {
                let owner = owner_at(previous).checked_sub(start)?;
                *covered_by.get_mut(owner)? = Some(index);
            }
            cursor += 1;
            extent = extent.max(cursor);
        }
        let overhang = self.prove_ink_overhang(&row[..extent], cursor, owner_at);
        let mut ordered = Vec::new();
        for cell in row.into_iter().flatten() {
            // Copies overlaying the same column are already collapsed by
            // the device row. When an edit leaves both copies at different
            // columns, both are visible in that accepted row.
            let cell = owner_at(cell);
            let owner = cell.checked_sub(start)?;
            *covered_by.get_mut(owner)? = None;
            if self.projected_ink(cell) && (keep_distinct_copies || !ordered.contains(&cell)) {
                ordered.push(cell);
            }
        }
        let order_groups = changed_paint_groups(start, &ordered);
        let retreat_targets = self.paint_retreat_targets(start, covered_by.len());
        let proof = DeviceOverprint {
            covered: covered_by
                .into_iter()
                .enumerate()
                .filter_map(|(offset, by)| {
                    let target = start + offset;
                    by.filter(|_| self.projected_coverable_cell(target, retreat_targets[offset]))
                        .map(|by| (target, Some(by)))
                })
                .collect(),
            order_groups,
            overhang,
        };
        Some((proof, stepped_left))
    }

    fn paint_retreat_targets(&self, start: usize, cells: usize) -> Vec<bool> {
        let end = start + cells;
        let first = self
            .pass_retreats
            .partition_point(|&(_, mark)| mark < start);
        let mut targets = vec![false; cells];
        for &(target, _) in self.pass_retreats[first..]
            .iter()
            .take_while(|&&(_, mark)| mark < end)
        {
            if let Some(offset) = target
                .checked_sub(start)
                .filter(|&offset| offset < targets.len())
            {
                targets[offset] = true;
            }
        }
        targets
    }

    fn projected_coverable_cell(&self, cell: usize, recorded_retreat: bool) -> bool {
        self.projected_ink(cell)
            || self.has_deferred_blank_at(cell)
            // A recorded retreat may target an automatic separator
            // whose reading position is owned by the following word.
            // term_field() first emits that advance, then backs over it
            // (term.c:420-434). Native paint owns the accepted cover
            // even when the input slot has no independent scalar.
            || recorded_retreat
            || self.projection_owner == super::super::FieldProjectionOwner::Detached
                && self.projection_length(cell, cell + 1) > 0
                && matches!(
                    self.cells[cell],
                    FieldCell::BreakableBlank
                        | FieldCell::NonBreakingBlank
                        | FieldCell::Graph { text: '\u{a0}', .. }
                )
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

    pub(in crate::mandoc::inline::flow) fn has_deferred_blank_at(&self, cell: usize) -> bool {
        let index = self.deferred_blanks.partition_point(|run| run.end <= cell);
        self.deferred_blanks
            .get(index)
            .is_some_and(|run| run.start <= cell)
    }
}

fn changed_paint_groups(start: usize, ordered: &[usize]) -> Vec<Vec<usize>> {
    let mut source_order = ordered.to_vec();
    source_order.sort_unstable();
    source_order.dedup();

    if source_order.len() == ordered.len() {
        // Preserve the existing single interval for a pure reorder.
        let Some(first) = ordered.iter().zip(&source_order).position(|(a, b)| a != b) else {
            return Vec::new();
        };
        let tail = ordered
            .iter()
            .zip(&source_order)
            .rev()
            .take_while(|(a, b)| a == b)
            .count();
        return vec![ordered[first..ordered.len() - tail].to_vec()];
    }

    // A duplicated owner need not include every survivor in its span.
    // Close an interval when the output has visited every authored owner
    // in that interval, including its last visible copy. Independent
    // edits may be separated by real spaces, which have no paint rank.
    let last_owner = source_order.last().expect("duplicated paint owner");
    let mut last = vec![0; last_owner - start + 1];
    for (position, &owner) in ordered.iter().enumerate() {
        last[owner - start] = position + 1;
    }
    let mut groups = Vec::new();
    let mut first_source = 0;
    let mut first_output = 0;
    let mut next_source = 0;
    let mut end_output = 0;
    for (position, owner) in ordered.iter().enumerate() {
        let source = source_order
            .binary_search(owner)
            .expect("known paint owner");
        while next_source <= source {
            end_output = end_output.max(last[source_order[next_source] - start]);
            next_source += 1;
        }
        if position + 1 == end_output {
            let observed = &ordered[first_output..end_output];
            if observed != &source_order[first_source..next_source] {
                groups.push(observed.to_vec());
            }
            first_source = next_source;
            first_output = end_output;
        }
    }
    groups
}

fn emits_backspace_after_control_or_advance(cells: &[FieldCell]) -> bool {
    // term_field() skips controls, but releases queued padding just
    // before emitting a backspace (term.c:389–434). Neither separates
    // authored ink into an independent overstrike.
    let mut prior_advance_or_retreat = false;
    for cell in cells.iter().take(MAX_REPLAY_PREFIX_CELLS) {
        match cell {
            FieldCell::BreakMarker
            | FieldCell::ZeroWidthGraph
            | FieldCell::Breakpoint
            | FieldCell::TabReference => {}
            FieldCell::Backline if prior_advance_or_retreat => return true,
            cell => {
                prior_advance_or_retreat = matches!(
                    cell,
                    FieldCell::Backline
                        | FieldCell::FixedDrawingAdvance(1..)
                        | FieldCell::BreakableBlank
                        | FieldCell::NonBreakingBlank
                        | FieldCell::Graph { text: '\u{a0}', .. }
                );
            }
        }
    }
    false
}
