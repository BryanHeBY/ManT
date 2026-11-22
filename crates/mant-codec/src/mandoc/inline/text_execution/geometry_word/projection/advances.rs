//! Keep replayed horizontal advances and authored blanks in their reading positions.

use std::ops::Range;

use super::{AdvanceReading, FieldCell, FillSlot, ReadingCell, ReadingGlyph};
use crate::mandoc::inline::flow::field_buffer::{DeferredSourceBlank, MAX_EDITABLE_APPEND_CELLS};

pub(super) fn prepare_reading_blanks(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    earlier: usize,
    word_owner: Option<u64>,
    advances: AdvanceReading,
    advance_marker: Option<usize>,
) {
    match advances {
        AdvanceReading::Inner => publish_inner_advances(slots, reading, earlier),
        AdvanceReading::DeferredTail => {
            if let Some(owner) = word_owner {
                defer_trailing_advance(slots, reading, owner);
            }
        }
        AdvanceReading::Unprojected => {}
    }
    let advance =
        advance_marker.and_then(|marker| split_buffered_advance(slots, reading, earlier, marker));
    defer_retained_separator_advances(slots, reading, earlier, word_owner, advance.as_ref());
}

fn defer_trailing_advance(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    owner: u64,
) {
    // term.c::term_field() delays trailing NBRSP until it finds the
    // next printable cell. Keep the native slots, with no reading
    // scalars until the accepted range proves that output.
    let Some(tail) = slots.last().filter(|slot| slot.drawing_advance) else {
        return;
    };
    let Some(style) = tail.owner.map(|id| reading[id]) else {
        return;
    };
    let start = slots.len() - 1;
    let count = tail.count;
    slots.pop();
    for offset in 0..count {
        let id = reading.len();
        reading.push(ReadingCell {
            glyph: ReadingGlyph::DeferredBlank {
                source: DeferredSourceBlank {
                    owner,
                    write: start + offset,
                    font: style.font,
                    replayable_tail: true,
                },
                imported_cell: None,
            },
            link: None,
            ..style
        });
        slots.push(FillSlot {
            cell: FieldCell::NonBreakingBlank,
            count: 1,
            owner: Some(id),
            drawing_advance: true,
            advance_source: None,
            underline_decoration: false,
        });
    }
}

/// `term_word()` buffers a positive HORIZ before writing the break marker.
/// Later ink can replace the marker or join the same pass past it. The
/// advance contributes reading spaces only when `term_field()` accepts it.
fn split_buffered_advance(
    slots: &mut Vec<FillSlot>,
    reading: &[ReadingCell<'_>],
    earlier: usize,
    marker: usize,
) -> Option<Range<usize>> {
    let mut offset = 0;
    let boundary = slots.iter().position(|slot| {
        let at_boundary = offset == marker;
        offset += slot.count;
        at_boundary
    })?;
    let boundary_slot = &slots[boundary];
    if incoming_ink(boundary_slot, reading, earlier).is_none()
        && !(boundary_slot.cell == FieldCell::BreakMarker
            && slots[boundary + 1..]
                .iter()
                .any(|slot| incoming_ink(slot, reading, earlier).is_some()))
    {
        return None;
    }
    let mut start = boundary;
    while start != 0 && slots[start - 1].drawing_advance && slots[start - 1].owner.is_none() {
        start -= 1;
    }
    if start == boundary {
        return None;
    }
    let count = slots[start..boundary]
        .iter()
        .map(|slot| slot.count)
        .sum::<usize>();
    slots.splice(
        start..boundary,
        (0..count).map(|_| FillSlot {
            cell: FieldCell::NonBreakingBlank,
            count: 1,
            owner: None,
            drawing_advance: true,
            advance_source: None,
            underline_decoration: false,
        }),
    );
    Some(start..start + count)
}

pub(super) fn incoming_ink(
    slot: &FillSlot,
    reading: &[ReadingCell<'_>],
    earlier: usize,
) -> Option<usize> {
    slot.owner.filter(|&owner| {
        owner >= earlier
            && matches!(reading[owner].glyph, ReadingGlyph::Scalar(ch) if !ch.is_whitespace())
    })
}

/// The next word can leave a short motion after an authored separator in
/// the same accepted pass. Read it from the replayed slots: an edit can
/// overwrite that motion, and `term_field()` can still drop it at row end.
fn defer_retained_separator_advances(
    slots: &mut [FillSlot],
    reading: &mut Vec<ReadingCell<'_>>,
    earlier: usize,
    word_owner: Option<u64>,
    advance: Option<&Range<usize>>,
) {
    let Some(word_owner) = word_owner.filter(|_| earlier != 0) else {
        return;
    };
    let is_replayed_marker = |slot: &FillSlot| {
        slot.cell == FieldCell::BreakMarker && slot.owner.is_some_and(|owner| owner < earlier)
    };
    let last_marker = slots.iter().rposition(is_replayed_marker);
    if last_marker.is_none() && advance.is_none() {
        return;
    }
    let mut after_marker = false;
    let mut next_ink = 0;
    for index in 0..slots.len() {
        let motion = &slots[index];
        let follows_marker = after_marker;
        if !motion.drawing_advance {
            after_marker = is_replayed_marker(motion);
        }
        if motion.cell != FieldCell::NonBreakingBlank
            || motion.count != 1
            || !motion
                .owner
                .is_none_or(|owner| matches!(reading[owner].glyph, ReadingGlyph::Earlier(0)))
        {
            continue;
        }
        // An unprojected fallback HORIZ can retain the ordinary NBRSP
        // kind in the prefix. Its preceding authored blank distinguishes
        // it from a generated entry separator in that representation.
        let after_authored_break = index >= 2
            && slots[index - 1].cell == FieldCell::BreakMarker
            && slots[index - 2].cell == FieldCell::BreakableBlank
            && slots[index - 2]
                .owner
                .is_some_and(|owner| matches!(reading[owner].glyph, ReadingGlyph::Earlier(1)));
        let follows_marker = follows_marker && (motion.drawing_advance || after_authored_break);
        // HORIZ can also land the new word before an older marker. A
        // surviving motion then separates ink before that same marker.
        // Its placement is still decided by the accepted term_field pass.
        let precedes_marker = last_marker.is_some_and(|marker| index < marker)
            && (motion.drawing_advance || motion.owner.is_some_and(|owner| owner < earlier));
        let advances_to_ink = advance.is_some_and(|range| range.contains(&index));
        if !follows_marker && !precedes_marker && !advances_to_ink {
            continue;
        }
        let ink_end = if follows_marker || advances_to_ink {
            slots.len()
        } else {
            last_marker.expect("motion before marker")
        };
        next_ink = next_ink.max(index + 1);
        let owner = loop {
            if next_ink >= ink_end {
                break None;
            }
            if let Some(owner) = incoming_ink(&slots[next_ink], reading, earlier) {
                break Some(owner);
            }
            if motion.advance_source.is_some()
                && !motion.drawing_advance
                && reading_ink(&slots[next_ink], reading).is_some_and(|owner| owner < earlier)
                && continues_owned_advance(slots, reading, earlier, index, next_ink..ink_end)
            {
                // term_field() releases the advance at the first printable
                // slot, which can still belong to an earlier operand.
                break None;
            }
            next_ink += 1;
        };
        let Some(owner) =
            owner.or_else(|| advance_between_replayed_glyphs(slots, reading, earlier, index))
        else {
            continue;
        };
        let id = reading.len();
        reading.push(ReadingCell {
            glyph: ReadingGlyph::DeferredBlank {
                source: DeferredSourceBlank {
                    owner: word_owner,
                    write: index,
                    font: reading[owner].font,
                    replayable_tail: false,
                },
                imported_cell: motion.advance_source.or(motion.owner),
            },
            link: None,
            ..reading[owner]
        });
        slots[index].owner = Some(id);
    }
}

fn continues_owned_advance(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    earlier: usize,
    advance: usize,
    following: Range<usize>,
) -> bool {
    // term_field() flushes all adjacent advances together. Retain this
    // already-projected single advance only if it still precedes the same
    // contiguous ink. Compound advances, another separator, or a
    // backspace still need their own accepted output coordinates.
    let another_advance = |slot: &FillSlot| {
        slot.advance_source.is_some()
            || matches!(slot.cell, FieldCell::NonBreakingBlank | FieldCell::Tab)
    };
    !slots[..advance]
        .iter()
        .rev()
        .take_while(|slot| reading_ink(slot, reading).is_none())
        .chain(slots[advance + 1..following.start].iter())
        .any(another_advance)
        && slots[following]
            .iter()
            .take_while(|slot| {
                !matches!(
                    slot.cell,
                    FieldCell::BreakableBlank
                        | FieldCell::NonBreakingBlank
                        | FieldCell::Tab
                        | FieldCell::BreakMarker
                        | FieldCell::Backline
                )
            })
            .any(|slot| incoming_ink(slot, reading, earlier).is_some())
}

fn advance_between_replayed_glyphs(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    earlier: usize,
    gap: usize,
) -> Option<usize> {
    if slots.len() > MAX_EDITABLE_APPEND_CELLS {
        return None;
    }
    // A current glyph can land before an earlier glyph. The marker
    // between them decides a break only if term_fill stops at a later
    // separator. Keep the blank under term_field's accepted receipt.
    let ink = |slot: &FillSlot| reading_ink(slot, reading);
    let left = slots[..gap].iter().rev().find_map(ink)?;
    let right = slots[gap + 1..].iter().find_map(ink)?;
    (left >= earlier && right < earlier).then_some(left)
}

fn reading_ink(slot: &FillSlot, reading: &[ReadingCell<'_>]) -> Option<usize> {
    slot.owner.filter(|&owner| {
        matches!(slot.cell, FieldCell::Graph { text, .. } if !text.is_whitespace())
            && matches!(
                reading[owner].glyph,
                ReadingGlyph::Scalar(_) | ReadingGlyph::Earlier(1..)
            )
    })
}

/// A short advance inside one operand is already part of its reading
/// line. Give its spaces their own cells instead of attaching a collapsed
/// separator to the next graph: that graph may be overwritten later.
fn publish_inner_advances(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    earlier: usize,
) {
    let mut preceding = None;
    let mut replacements = Vec::new();
    let mut budget = 64usize;
    for (index, slot) in slots.iter().enumerate() {
        let ink = slot.owner.filter(|&owner| {
            owner >= earlier
                && matches!(reading[owner].glyph, ReadingGlyph::Scalar(ch) if !ch.is_whitespace())
                && matches!(slot.cell, FieldCell::Graph { .. } | FieldCell::Hyphen)
        });
        if ink.is_none() {
            // HORIZ can traverse an earlier graph without replacing it.
            // When incoming ink already began this interval, that retained
            // graph is the nearest ink before newly written advances.
            // Keep those cells distinct from the following graph's scalar.
            if preceding.is_some() && reading_ink(slot, reading).is_some() {
                preceding = Some(index);
            }
            continue;
        }
        if let Some(at) = preceding {
            let mut end = index;
            if index >= at + 3
                && slots[index - 1].cell == FieldCell::Backline
                && slots[index - 2].owner.is_none()
                && matches!(
                    slots[index - 2].cell,
                    FieldCell::Graph {
                        text: '_',
                        width: 1
                    }
                )
            {
                // The underline paint immediately before the next glyph
                // occupies that same column (term.c::encode1).
                end -= 2;
            }
            let between = at + 1..end;
            let mut advances = Vec::new();
            let mut width = 0usize;
            let usable = between.clone().all(|index| {
                let gap = &slots[index];
                match gap.owner {
                    Some(owner) if matches!(reading[owner].glyph, ReadingGlyph::DeviceAdvance) => {
                        advances.push((index, owner));
                        width = width.saturating_add(gap.count);
                        true
                    }
                    Some(_) => false,
                    None => matches!(
                        gap.cell,
                        FieldCell::ZeroWidthGraph
                            | FieldCell::TabReference
                            | FieldCell::Breakpoint
                            | FieldCell::BreakMarker
                    ),
                }
            });
            if usable && width != 0 {
                if width <= budget {
                    replacements.extend(
                        advances
                            .into_iter()
                            .map(|(slot, owner)| (slot, owner, slots[slot].count)),
                    );
                    budget -= width;
                } else if let Some(&(slot, owner)) = advances.first() {
                    // Retain one reading separator for a compact span.
                    // Attaching it to the next glyph would make that
                    // glyph's raw slot impossible to replay independently.
                    replacements.push((slot, owner, 1));
                }
            }
        }
        preceding = Some(index);
    }
    // A trailing or oversized advance has no projected scalar above.
    for slot in &mut *slots {
        if slot
            .owner
            .is_some_and(|id| matches!(reading[id].glyph, ReadingGlyph::DeviceAdvance))
        {
            slot.owner = None;
        }
    }
    assign_inner_advance_spaces(slots, reading, replacements);
}

fn assign_inner_advance_spaces(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    replacements: Vec<(usize, usize, usize)>,
) {
    for (index, owner, projected) in replacements.into_iter().rev() {
        let read = reading[owner];
        let slot = &slots[index];
        let mut blanks = (0..projected)
            .map(|_| {
                let id = reading.len();
                reading.push(ReadingCell {
                    glyph: ReadingGlyph::Scalar(' '),
                    ..read
                });
                FillSlot {
                    cell: slot.cell.clone(),
                    count: 1,
                    owner: Some(id),
                    drawing_advance: false,
                    advance_source: None,
                    underline_decoration: false,
                }
            })
            .collect::<Vec<_>>();
        if projected < slot.count {
            blanks.push(FillSlot {
                count: slot.count - projected,
                owner: None,
                ..slot.clone()
            });
        }
        slots.splice(index..=index, blanks);
    }
}

pub(super) fn preserve_author_separators(slots: &[FillSlot], reading: &mut [ReadingCell<'_>]) {
    // An authored blank after a marker can traverse an older glyph or
    // overstrike byte without writing native SP (term.c::bufferc). Keep
    // its reading boundary when that pass still contains both words.
    let mut after_author = false;
    let mut blank_between = false;
    let mut break_at_next_blank = false;
    let mut wrote_source = false;
    for slot in slots {
        match slot.cell {
            FieldCell::BreakMarker => break_at_next_blank = true,
            FieldCell::BreakableBlank | FieldCell::Breakpoint if break_at_next_blank => {
                // term_fill() ends the current pass here, even when its
                // preceding graph or separator supplies no reading text.
                after_author = false;
                blank_between = false;
                break_at_next_blank = false;
            }
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab => {
                if slot.owner.is_some() {
                    after_author = false;
                    blank_between = false;
                } else {
                    // The retained device advance is omitted from the
                    // reading view. Preserve the boundary it establishes.
                    blank_between = true;
                }
            }
            FieldCell::Graph { text, .. } if text.is_whitespace() => {
                // A fixed source space already projects a separator. Do
                // not put an additional lexical separator on its glyph slot:
                // term_word() can still replace this single cell after \p.
                after_author = false;
                blank_between = false;
            }
            _ => {
                if let Some(id) = slot.owner {
                    // The first incoming glyph has no prefix in
                    // append_range(); retained ink is already represented
                    // outside this word's scalar stream. The write receipt
                    // must count only a separator the word can publish.
                    if !matches!(
                        reading[id].glyph,
                        ReadingGlyph::Earlier(_)
                            | ReadingGlyph::EntryBlank
                            | ReadingGlyph::DeferredBlank { .. }
                    ) {
                        reading[id].preserve_separator = wrote_source
                            && after_author
                            && (reading[id].authored_separator || blank_between);
                        wrote_source = true;
                    }
                    after_author = true;
                    blank_between = false;
                }
            }
        }
    }
}
