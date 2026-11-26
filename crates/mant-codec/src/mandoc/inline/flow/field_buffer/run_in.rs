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

//! Apply generated definition-body separators to the live heading slots.

use super::{FieldBuffer, FieldCell, WriteCursor, cells};

pub(in crate::mandoc::inline::flow) struct RunInWordSlots {
    pub(in crate::mandoc::inline::flow) start: usize,
    write: RunInWordWrite,
}

enum RunInWordWrite {
    ReplacesSpace,
    ReplacesSpaceWithBackline,
    AppendsBackline,
}

impl RunInWordSlots {
    pub(in crate::mandoc::inline::flow) const fn overwritten(&self) -> usize {
        match self.write {
            RunInWordWrite::ReplacesSpace | RunInWordWrite::ReplacesSpaceWithBackline => 1,
            RunInWordWrite::AppendsBackline => 0,
        }
    }

    pub(in crate::mandoc::inline::flow) const fn has_backline(&self) -> bool {
        !matches!(self.write, RunInWordWrite::ReplacesSpace)
    }

    pub(in crate::mandoc::inline::flow) const fn appends_backline(&self) -> bool {
        matches!(self.write, RunInWordWrite::AppendsBackline)
    }
}

impl FieldBuffer {
    /// With an unread marker, BACKBEFORE refers to the raw tail, not to
    /// the nearest visible glyph. Replaying the generated NBSP can either
    /// append a backline or replace the preceding SP. The latter must
    /// retire that SP from its HEAD owner too (term.c:903–929).
    pub(in crate::mandoc::inline::flow) fn has_marked_backbefore_tail(&self) -> bool {
        self.backbefore_armed
            && matches!(
                self.cells.last(),
                Some(FieldCell::BreakMarker | FieldCell::BreakableBlank)
            )
            && self.has_pending_break_markers()
    }

    pub(in crate::mandoc::inline::flow) fn generated_blank_overlap(
        &self,
        count: usize,
    ) -> (usize, bool) {
        // termp_it_pre() uses a nonbreaking blank word, including the
        // native slots it rewrites before lastcol.
        match self.write_cursor {
            WriteCursor::Placed(retreat) if !self.backbefore_armed => {
                if !self.backafter_armed {
                    return (retreat.min(count), false);
                }
                if (count == 1 || retreat == 1)
                    && count > 0
                    && retreat >= 1
                    && let Some(cursor) = self.cells.len().checked_sub(retreat)
                    && (self.cells[cursor] == FieldCell::BreakableBlank
                        || self.projection_length(cursor, cursor + 1) == 0
                            && (self.cells[cursor] == FieldCell::Backline
                                || cursor == 0
                                    && self.cells[cursor]
                                        == FieldCell::Graph {
                                            text: '_',
                                            width: 1,
                                        }))
                {
                    // The first generated graph consumes BACKAFTER at col
                    // and arms BACKBEFORE; any second write continues from
                    // the next slot (term.c:920-929).
                    return (1, true);
                }
            }
            _ => {}
        }
        (0, false)
    }

    pub(in crate::mandoc::inline::flow) fn retained_run_in_word(
        &self,
        count: usize,
    ) -> Option<RunInWordSlots> {
        if !matches!(count, 1 | 2) || self.backafter_armed {
            return None;
        }
        // encode1() inspects col-1, not lastcol-1 (term.c:903-908).
        // A proved one-cell retreat onto SP replaces it with either
        // U+00A0 or a backspace, depending on the BACKBEFORE register.
        // With col at the end and BACKBEFORE set, the backspace appends.
        let (start, write) = match self.write_cursor {
            WriteCursor::Placed(1) if self.cells.last() == Some(&FieldCell::BreakableBlank) => {
                let write = if self.backbefore_armed {
                    RunInWordWrite::ReplacesSpaceWithBackline
                } else {
                    RunInWordWrite::ReplacesSpace
                };
                (self.cells.len() - 1, write)
            }
            // Only an exact slot replay proves the pending glyph at col-1.
            // Ordinary End words keep the established zero-advance path.
            WriteCursor::Placed(0)
                if self.backbefore_armed && self.has_independent_run_in_glyph() =>
            {
                (self.cells.len(), RunInWordWrite::AppendsBackline)
            }
            _ => return None,
        };
        let previous = start.checked_sub(1)?;
        (self.resume <= previous
            && self.raw_slot_fence <= previous
            && matches!(
                self.cells.get(previous),
                Some(FieldCell::Graph { width: 1, .. })
            ))
        .then_some(RunInWordSlots { start, write })
    }

    fn has_independent_run_in_glyph(&self) -> bool {
        // Publishing the pending raw owner is safe only when its glyph
        // starts on an independent column in the still-unbroken field.
        // A preceding backspace can have put the glyph over a different
        // source owner. That compound cover still uses the held glyph.
        !self.has_break_markers()
            && self.cells.len().checked_sub(1).is_some_and(|last| {
                let start = self.raw_copy_aliases.last().map_or(last, |&(copy, owner)| {
                    if owner == last { copy.min(owner) } else { last }
                });
                start
                    .checked_sub(1)
                    .is_none_or(|previous| self.cells[previous] != FieldCell::Backline)
            })
    }

    pub(super) fn advance_run_in_blanks(&mut self, count: usize) {
        let retreat = self.write_cursor.retreat();
        if std::mem::take(&mut self.backafter_armed) {
            // encode1 writes the generated fixed blank at the retained
            // cursor, then arms BACKBEFORE (term.c:920-929). An old SP
            // must not remain before it.
            let cell = self.cells.len() - retreat;
            self.backbefore_armed = true;
            self.backbefore_graph = Some(cell);
            self.backline_projection_target = Some(cell);
        }
        self.write_cursor = WriteCursor::Placed(retreat - count);
    }

    /// Record `-inset`/`-diag` run-in `encode1(0x00A0)` writes overlapping
    /// live HEAD slots at `p->col` (`mdoc_term.c:757-772, 939-945`,
    /// `chars.c:50`, `term.c:620-636, 888-930`).
    pub(in crate::mandoc::inline::flow) fn defer_run_in_overlap(
        &mut self,
        traversed: usize,
        projected: usize,
        source: cells::DeferredSourceBlank,
    ) -> bool {
        let cursor = self.cells.len() - self.write_cursor.retreat();
        let end = cursor + traversed;
        // A run-in separator may traverse the HEAD's native trailing SP.
        // HEAD's term drain trimmed that spacing, but term_field() can
        // still print it when a subsequent BODY graph reaches the field.
        let printed_end = self.printed_content_end(self.resume, self.cells.len());
        let projected_start = end.saturating_sub(projected);
        let pending_break = self.has_pending_break_markers() || self.backafter_armed;
        let mut added = false;
        let mut previous_font_copy = false;
        for cell in cursor..end {
            let replaces_backline = self.cells[cell] == FieldCell::Backline;
            let replaces_break_marker = self.cells[cell] == FieldCell::BreakMarker;
            let followed_by_backline = self.cells.get(cell + 1) == Some(&FieldCell::Backline);
            let is_alias = self
                .raw_copy_aliases
                .binary_search_by_key(&cell, |&(copy, _)| copy)
                .is_ok();
            let bold_copy = followed_by_backline && is_alias;
            let split_bold_copy =
                !followed_by_backline && is_alias && self.projection_length(cell, cell + 1) == 0;
            let replaces_font_pair = cell + 1 < end && bold_copy;
            let replaces_bold_copy = !replaces_font_pair && bold_copy;
            // A projected U+00A0 also overwrites the anonymous underline
            // slot in encode1(). Leave a bold copy of a literal '_' on
            // its existing ownership path (term.c:909-925).
            let replaces_underline = followed_by_backline
                && !bold_copy
                && self.cells[cell]
                    == (FieldCell::Graph {
                        text: '_',
                        width: 1,
                    })
                && self.projection_length(cell, cell + 1) == 0;
            let replaces_decoration = replaces_backline
                || replaces_break_marker
                || replaces_font_pair
                || replaces_bold_copy
                || split_bold_copy
                || replaces_underline;
            let inside_font_pair = previous_font_copy || replaces_font_pair;
            previous_font_copy = replaces_font_pair;
            if pending_break && self.cells[cell] == FieldCell::BreakableBlank {
                // A generated NBSP is a graph for `term_fill()`, including
                // when it rewrites the ordinary SP after a saved marker.
                self.cells[cell] = FieldCell::Graph {
                    text: '\u{a0}',
                    width: 1,
                };
                self.blank_positions.remove(&cell);
                self.note_graph_position(cell);
                self.invalidate_projection_scan(cell);
            }
            if replaces_decoration {
                // The UTF-8 generated blank goes through encode1() as
                // U+00A0 and overwrites earlier font bytes. Track the
                // surviving authored slots through the next word overlay.
                if replaces_underline
                    || replaces_bold_copy
                    || replaces_break_marker
                    || self.backafter_armed && cell == cursor
                {
                    self.cells[cell] = FieldCell::Graph {
                        text: '\u{a0}',
                        width: 1,
                    };
                    self.note_graph_position(cell);
                } else {
                    self.cells[cell] = FieldCell::FixedDrawingAdvance(1);
                    self.nonbreaking_positions.insert(cell);
                }
                self.raw_copy_aliases.retain(|&(copy, _)| copy != cell);
                if replaces_backline {
                    self.pass_retreats.retain(|&(_, mark)| mark != cell + 1);
                }
                if replaces_break_marker {
                    self.break_markers.retain(|&mark| mark != cell);
                }
                self.invalidate_projection_scan(cell);
            }
            // These advances replace the earlier label's own font bytes.
            // Its lexical separator already precedes that label scalar.
            if cell < projected_start
                || (inside_font_pair || split_bold_copy || !replaces_decoration)
                    && cell < printed_end
            {
                continue;
            }
            if !matches!(
                self.cells[cell],
                FieldCell::BreakableBlank
                    | FieldCell::NonBreakingBlank
                    | FieldCell::FixedDrawingAdvance(1)
                    | FieldCell::Graph { text: '\u{a0}', .. }
            ) {
                continue;
            }
            added |= self.insert_deferred_run_in_blank(cell, source);
        }
        added
    }

    /// Track the latest two graph positions so `term_flushln()`'s remainder
    /// check (`term.c:177-198`) observes overwritten `U+00A0` cells and still
    /// falls back cleanly on tail truncation.
    fn note_graph_position(&mut self, cell: usize) {
        match self.last_graph_position {
            None => self.last_graph_position = Some(cell),
            Some(last) if cell > last => {
                self.previous_graph_position = Some(last);
                self.last_graph_position = Some(cell);
            }
            Some(last) if cell < last => {
                self.previous_graph_position = Some(
                    self.previous_graph_position
                        .map_or(cell, |prev| prev.max(cell)),
                );
            }
            Some(_) => {}
        }
    }

    fn insert_deferred_run_in_blank(
        &mut self,
        cell: usize,
        source: cells::DeferredSourceBlank,
    ) -> bool {
        let index = self.deferred_blanks.partition_point(|run| run.end <= cell);
        if self
            .deferred_blanks
            .get(index)
            .is_some_and(|run| run.start <= cell)
        {
            return false;
        }
        self.deferred_blanks.insert(
            index,
            cells::DeferredBlankRun {
                start: cell,
                end: cell + 1,
                source,
                run_in_overlap: true,
                settled_trailing_run_in: false,
            },
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defer_run_in_overlap_tracks_both_graph_positions_across_tail_replacement() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('C', 1);
        buffer.push_cell(FieldCell::BreakMarker);
        buffer.push_cell(FieldCell::BreakableBlank);
        buffer.push_cell(FieldCell::BreakableBlank);
        // Simulate a 1-cell retreat onto the final BreakableBlank (cell 3)
        // followed by inset BODY's generated U+00A0 overwriting it.
        buffer.write_cursor = super::super::WriteCursor::Placed(1);
        let source = super::super::cells::DeferredSourceBlank {
            owner: 1,
            write: 0,
            font: super::super::super::Font::Regular,
            replayable_tail: false,
            generated_run_in: false,
        };
        let _ = buffer.defer_run_in_overlap(1, 0, source);
        assert_eq!(
            buffer.cells()[3],
            FieldCell::Graph {
                text: '\u{a0}',
                width: 1
            }
        );
        assert_eq!(buffer.last_graph_position, Some(3));
        assert_eq!(buffer.previous_graph_position, Some(0));
        assert!(!buffer.only_ignorable_remainder(false));

        // Truncating cell 3 (count = 1) must fall back to previous_graph_position (0).
        buffer.replace_placed_tail(1);
        assert_eq!(buffer.last_graph_position, Some(0));
    }
}
