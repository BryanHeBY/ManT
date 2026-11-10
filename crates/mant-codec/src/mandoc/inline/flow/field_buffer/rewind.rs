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

//! Apply a bounded native cursor rewind before writing a word.

use super::{FieldBuffer, FieldCell, FieldWrite, WordScanPolicy};

pub(in crate::mandoc::inline::flow) enum EntryRewind {
    Absent,
    Executed {
        retired_separator: usize,
        traversed_source_blanks: Vec<usize>,
        overwritten_glyph: Option<RewoundGlyph>,
    },
    Unmodeled,
}

pub(in crate::mandoc::inline::flow) struct RewoundGlyph {
    pub(in crate::mandoc::inline::flow) cell: usize,
    pub(in crate::mandoc::inline::flow) scalar: char,
    pub(in crate::mandoc::inline::flow) following_scalars: usize,
}

struct EntryReplay {
    start: usize,
    writes: Vec<FieldWrite>,
    retired_separator: usize,
    traversed_source_blanks: Vec<usize>,
    overwritten_glyph: Option<(usize, char)>,
}

enum EntryReplacement {
    ControlOrPadding,
    PendingGlyph(char),
    Protected,
}

/// Native cursor action during a bounded entry rewind. Classify every write
/// explicitly: a compressed deferred SP has exactly the same cursor action
/// as the literal SP, even though its source projection uses a marker.
enum EntryWrite {
    Advance {
        count: usize,
        projected: bool,
    },
    Overwrite {
        encode: bool,
        scalars: usize,
        native_cells: usize,
    },
    ArmBackafter,
    CancelBackafter,
    CancelBackbefore,
    Rewind,
}

impl FieldWrite {
    fn entry_write(&self) -> EntryWrite {
        match self {
            Self::Cell(FieldCell::BreakableBlank | FieldCell::NonBreakingBlank) => {
                EntryWrite::Advance {
                    count: 1,
                    projected: true,
                }
            }
            Self::UnprojectedBlank => EntryWrite::Advance {
                count: 1,
                projected: false,
            },
            Self::DeferredBlanks { count, .. } => EntryWrite::Advance {
                count: *count,
                projected: false,
            },
            Self::Cell(FieldCell::Graph { .. } | FieldCell::Hyphen) => EntryWrite::Overwrite {
                encode: true,
                scalars: 1,
                native_cells: 1,
            },
            Self::OwnedBlank { projected } => EntryWrite::Overwrite {
                encode: true,
                scalars: usize::from(*projected),
                native_cells: 1,
            },
            Self::Cell(FieldCell::Tab) => EntryWrite::Overwrite {
                encode: false,
                scalars: 1,
                native_cells: 1,
            },
            Self::Cell(
                FieldCell::BreakMarker
                | FieldCell::TabReference
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint
                | FieldCell::Backline,
            ) => EntryWrite::Overwrite {
                encode: false,
                scalars: 0,
                native_cells: 1,
            },
            Self::RecoveryGlyph { projected_scalars } => EntryWrite::Overwrite {
                encode: false,
                scalars: *projected_scalars,
                native_cells: 1,
            },
            Self::DecoratedGraph { prefix, .. } => EntryWrite::Overwrite {
                encode: true,
                scalars: 1,
                native_cells: usize::from(*prefix) + 1,
            },
            Self::ArmBackafter => EntryWrite::ArmBackafter,
            Self::CancelBackafter => EntryWrite::CancelBackafter,
            Self::CancelBackbefore => EntryWrite::CancelBackbefore,
            Self::RewindWordEntry(_) => EntryWrite::Rewind,
        }
    }
}

impl FieldBuffer {
    /// HORIZ changes col, not lastcol (term.c:701–714). A raw space
    /// traverses the existing byte without replacing it (860–873,948–952).
    /// Replay only after the first proven overwrite. No rejected plan
    /// mutates the cells or supplies an authoritative acceptance range.
    pub(in crate::mandoc::inline::flow) fn rewind_word_entry(
        &mut self,
        writes: &mut Vec<FieldWrite>,
        word_start: usize,
        owner_end: impl FnOnce(usize) -> usize,
    ) -> EntryRewind {
        let Some(FieldWrite::RewindWordEntry(columns)) = writes.first() else {
            return EntryRewind::Absent;
        };
        if self.backafter_armed {
            // term.c:678–681 skips the motion entirely when BACKAFTER is
            // armed; a previously buffered BACKBEFORE glyph remains live.
            writes[0] = FieldWrite::CancelBackafter;
            return EntryRewind::Absent;
        }
        if matches!(self.word_scan, WordScanPolicy::UnmodeledRewind) {
            return EntryRewind::Unmodeled;
        }
        let count = columns.saturating_add(usize::from(self.backbefore_armed));
        let Some(replay) = self.plan_entry_rewind(&writes[1..], word_start, count) else {
            self.word_scan = WordScanPolicy::UnmodeledRewind;
            self.scan = None;
            return EntryRewind::Unmodeled;
        };
        let EntryReplay {
            start,
            writes: replay,
            retired_separator,
            traversed_source_blanks,
            overwritten_glyph,
        } = replay;
        let overwritten_glyph = overwritten_glyph.map(|(cell, scalar)| RewoundGlyph {
            cell,
            scalar,
            following_scalars: self.projection_length(cell + 1, owner_end(cell).min(word_start)),
        });
        for index in (start..self.cells.len()).rev() {
            self.blank_positions.remove(&index);
            self.nonbreaking_positions.remove(&index);
            self.retire_deferred_blank_at(index);
        }
        self.cells.truncate(start);
        self.projection_prefix.truncate(start + 1);
        self.zero_width_retreats.truncate(
            self.zero_width_retreats
                .partition_point(|&(_, mark)| mark.saturating_sub(1) < start),
        );
        self.normalized_until = self.normalized_until.min(start);
        self.invalidate_projection_scan(start);
        if self.last_graph_position.is_some_and(|last| last >= start) {
            self.last_graph_position = self
                .previous_graph_position
                .take()
                .filter(|&cell| cell < start);
        }
        writes.clear();
        // ESCAPE_HORIZ always clears BACKAFTER, and a negative distance
        // consumes the prior BACKBEFORE even for a sub-column displacement.
        writes.extend([FieldWrite::CancelBackafter, FieldWrite::CancelBackbefore]);
        writes.extend(replay);
        EntryRewind::Executed {
            retired_separator,
            traversed_source_blanks,
            overwritten_glyph,
        }
    }

    fn plan_entry_rewind(
        &self,
        writes: &[FieldWrite],
        word_start: usize,
        count: usize,
    ) -> Option<EntryReplay> {
        let end = self.cells.len();
        // A motion beyond the current column clamps native col to zero
        // (term.c:705–714); the remaining advance affects the device
        // offset, not the input cells. Replaying the word from zero still
        // gives a valid marker/ownership receipt for an empty literal row.
        let mut at = end.saturating_sub(count);
        if at < self.resume {
            return None;
        }
        let mut rewritten = None;
        let mut replay = Vec::new();
        let mut retired_separator = 0;
        let mut traversed_source_blanks = Vec::new();
        let mut source_scalars = 0;
        let mut wrote_source = false;
        let mut overwritten_glyph = None;
        let mut armed = false;
        let mut backbefore = false;
        for (index, write) in writes.iter().enumerate() {
            if at == end {
                replay.extend_from_slice(&writes[index..]);
                break;
            }
            match write.entry_write() {
                EntryWrite::Advance { count, projected } => {
                    let traversed = count.min(end - at);
                    for cell in at..at + traversed {
                        // A surviving automatic separator crossed after
                        // the first source graph is inside this word. Its
                        // source SP can project that cell at the actual
                        // position; emitting it at entry would move it.
                        let embedded_separator = cell >= word_start && wrote_source;
                        retired_separator += usize::from(embedded_separator);
                        if projected {
                            if !embedded_separator {
                                traversed_source_blanks.push(source_scalars);
                            }
                            source_scalars += 1;
                        }
                        if rewritten.is_some() {
                            let replayed = if embedded_separator
                                && let FieldWrite::DeferredBlanks { source, .. } = write
                                && matches!(self.cells[cell], FieldCell::BreakableBlank)
                            {
                                // A deferred authored SP carries its
                                // source identity but no early scalar.
                                FieldWrite::DeferredBlanks {
                                    source: *source,
                                    count: 1,
                                }
                            } else {
                                self.replay_entry_cell(cell)?
                            };
                            replay.push(replayed);
                        }
                    }
                    at += traversed;
                    if let FieldWrite::DeferredBlanks { source, .. } = write
                        && count > traversed
                    {
                        replay.push(FieldWrite::DeferredBlanks {
                            source: *source,
                            count: count - traversed,
                        });
                    }
                    continue;
                }
                EntryWrite::Overwrite {
                    encode,
                    scalars,
                    native_cells,
                } => {
                    wrote_source |= scalars > 0;
                    source_scalars += scalars;
                    if encode {
                        if backbefore {
                            // A second encode1 can rewind or add a backline.
                            // A plan only authorizes the original entry suffix.
                            return None;
                        }
                        backbefore = std::mem::take(&mut armed);
                    }
                    let overwritten_end = at.saturating_add(native_cells).min(end);
                    for cell in at..overwritten_end {
                        match self.entry_replacement(cell, word_start) {
                            EntryReplacement::Protected => return None,
                            EntryReplacement::PendingGlyph(text) => {
                                overwritten_glyph = Some((cell, text));
                            }
                            EntryReplacement::ControlOrPadding => {}
                        }
                        retired_separator += usize::from(cell >= word_start);
                    }
                    rewritten.get_or_insert(at);
                    at = overwritten_end;
                }
                EntryWrite::ArmBackafter => armed = true,
                EntryWrite::CancelBackafter => armed = false,
                EntryWrite::CancelBackbefore => backbefore = false,
                EntryWrite::Rewind => return None,
            }
            replay.push(write.clone());
        }
        (at == end).then_some(EntryReplay {
            start: rewritten.unwrap_or(end),
            writes: replay,
            retired_separator,
            traversed_source_blanks,
            overwritten_glyph,
        })
    }

    fn entry_replacement(&self, cell: usize, word_start: usize) -> EntryReplacement {
        if cell >= word_start || matches!(self.cells[cell], FieldCell::TabReference) {
            return EntryReplacement::ControlOrPadding;
        }
        if self.backbefore_graph == Some(cell)
            && let FieldCell::Graph { text, .. } = self.cells[cell]
        {
            if self.projection_length(cell, cell + 1) > 0 {
                EntryReplacement::PendingGlyph(text)
            } else {
                EntryReplacement::ControlOrPadding
            }
        } else {
            EntryReplacement::Protected
        }
    }

    /// Bytes traversed *after* the first overwrite must be replayed at the
    /// same cursor. Keep their old scalar quota and, for a deferred source
    /// blank, its original owner; never assign them to the traversing write.
    fn replay_entry_cell(&self, cell: usize) -> Option<FieldWrite> {
        match &self.cells[cell] {
            FieldCell::BreakableBlank if self.projection_length(cell, cell + 1) == 0 => {
                let previous = self
                    .deferred_blanks
                    .partition_point(|run| run.start <= cell);
                if let Some(run) = previous
                    .checked_sub(1)
                    .and_then(|index| self.deferred_blanks.get(index))
                    && cell < run.end
                {
                    Some(FieldWrite::DeferredBlanks {
                        source: run.source,
                        count: 1,
                    })
                } else {
                    Some(FieldWrite::UnprojectedBlank)
                }
            }
            cell @ (FieldCell::BreakableBlank
            | FieldCell::NonBreakingBlank
            | FieldCell::TabReference) => Some(FieldWrite::Cell(cell.clone())),
            _ => None,
        }
    }
}
