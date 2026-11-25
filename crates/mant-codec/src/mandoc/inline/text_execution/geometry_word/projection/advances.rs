//! Keep replayed horizontal advances and authored blanks in their reading positions.

use std::ops::Range;

use super::{AdvanceReading, FieldCell, FillSlot, ReadingCell, ReadingGlyph};
use crate::mandoc::inline::flow::field_buffer::{DeferredSourceBlank, MAX_EDITABLE_APPEND_CELLS};

/// Project buffered `HORIZ` advances (`term.c:691-699`) and `bufferc` gaps
/// (`term.c:860-872`) into reading spaces or deferred blanks pending
/// `term_field()` acceptance (`term.c:389-428`).
pub(super) fn prepare_reading_blanks(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    earlier: usize,
    starts_at_field_origin: bool,
    word_owner: Option<u64>,
    advances: AdvanceReading,
    advance_marker: Option<usize>,
) {
    match advances {
        AdvanceReading::Inner => {
            if let Some(owner) = word_owner {
                defer_inner_trailing_advances(slots, reading, earlier, owner);
            }
            publish_inner_advances(slots, reading, earlier);
        }
        AdvanceReading::DeferredTail => {
            if let Some(owner) = word_owner {
                let start = slots.len().saturating_sub(1);
                defer_trailing_advances(slots, reading, owner, start, true);
                publish_inner_advances(slots, reading, earlier);
            }
        }
        AdvanceReading::DeferredBlankTail => {
            if let Some(owner) = word_owner {
                let replayable_tail = !slots.iter().any(|slot| slot.cell == FieldCell::BreakMarker);
                defer_trailing_advances(slots, reading, owner, 0, replayable_tail);
            }
        }
        AdvanceReading::Unprojected => {}
    }
    let advance =
        advance_marker.and_then(|marker| split_buffered_advance(slots, reading, earlier, marker));
    defer_retained_separator_advances(
        slots,
        reading,
        earlier,
        starts_at_field_origin,
        word_owner,
        advance.as_ref(),
    );
}

/// `term_fill()` (`term.c:288-302, 363-366`) and `term_field()` (`term.c:389-428`)
/// drop trailing `ASCII_NBRSP` advances unless a later printable cell
/// triggers output; zero-width controls (`term.c:395-401`) do not flush them.
fn defer_inner_trailing_advances(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    earlier: usize,
    owner: u64,
) {
    let Some(last_ink) = slots.iter().rposition(|slot| {
        slot.owner.is_some_and(|owner| {
            owner >= earlier
                && matches!(
                    reading[owner].glyph,
                    ReadingGlyph::Scalar(ch) if !ch.is_whitespace() || ch == '\u{a0}'
                )
        })
    }) else {
        return;
    };
    let tail_start = last_ink + 1;
    let mut width = 0usize;
    let eligible = slots[tail_start..].iter().all(|slot| {
        if slot.owner.is_some_and(|id| {
            slot.drawing_advance && matches!(reading[id].glyph, ReadingGlyph::DeviceAdvance)
        }) {
            width = width.saturating_add(slot.count);
            true
        } else {
            // Replayed zero-width controls and unpaired backlines carry no
            // advance, and trailing `SP` after the last ink is also queued
            // without printing trailing `ASCII_NBRSP` advances (`term.c:389-428`).
            matches!(slot.cell, FieldCell::BreakableBlank)
                || slot
                    .owner
                    .is_none_or(|id| matches!(reading[id].glyph, ReadingGlyph::Earlier(0)))
                    && matches!(
                        slot.cell,
                        FieldCell::Backline
                            | FieldCell::BreakMarker
                            | FieldCell::Breakpoint
                            | FieldCell::ZeroWidthGraph
                            | FieldCell::TabReference
                    )
        }
    });
    if eligible && (1..=MAX_EDITABLE_APPEND_CELLS).contains(&width) {
        let replayable_tail = !slots.iter().any(|slot| slot.cell == FieldCell::BreakMarker);
        defer_trailing_advances(slots, reading, owner, tail_start, replayable_tail);
    }
}

fn defer_trailing_advances(
    slots: &mut Vec<FillSlot>,
    reading: &mut Vec<ReadingCell<'_>>,
    owner: u64,
    start: usize,
    replayable_tail: bool,
) {
    // term.c::term_field() delays trailing NBRSP until it finds the
    // next printable cell. Keep the native slots, with no reading
    // scalars until the accepted range proves that output.
    let mut end = slots.iter().map(|slot| slot.count).sum::<usize>();
    for index in (start..slots.len()).rev() {
        let tail = &slots[index];
        let count = tail.count;
        end -= count;
        let Some(style) = tail.owner.map(|id| reading[id]).filter(|read| {
            tail.drawing_advance && matches!(read.glyph, ReadingGlyph::DeviceAdvance)
        }) else {
            continue;
        };
        slots.splice(
            index..=index,
            (0..count).map(|offset| {
                let id = reading.len();
                reading.push(ReadingCell {
                    glyph: ReadingGlyph::DeferredBlank {
                        source: DeferredSourceBlank {
                            owner,
                            write: end + offset,
                            font: style.font,
                            replayable_tail,
                        },
                        imported_cell: None,
                    },
                    link: None,
                    ..style
                });
                FillSlot {
                    cell: FieldCell::NonBreakingBlank,
                    count: 1,
                    owner: Some(id),
                    drawing_advance: true,
                    advance_source: None,
                    underline_decoration: false,
                    owned_deferred_blank: false,
                }
            }),
        );
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
            owned_deferred_blank: false,
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
    starts_at_field_origin: bool,
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
            || motion.owned_deferred_blank
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
            if !motion.drawing_advance
                && reading_ink(&slots[next_ink], reading).is_some_and(|owner| owner < earlier)
                && (motion.advance_source.is_some()
                    && is_isolated_owned_advance(slots, reading, index, next_ink)
                    || starts_at_field_origin
                        && !slots[..index]
                            .iter()
                            .any(|slot| reading_ink(slot, reading).is_some()))
            {
                // term_field() (`term.c:389-428`) releases a leading advance
                // or a single already-owned advance at the first surviving
                // earlier printable glyph, not at a later incoming glyph.
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

fn is_isolated_owned_advance(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    advance: usize,
    next_ink: usize,
) -> bool {
    // term_field() flushes all adjacent advances together (`term.c:389-428`).
    // Retain this already-projected single advance only when it is not part
    // of a compound advance run and the earlier glyph that releases it is not
    // immediately covered by a following backspace.
    let another_advance = |slot: &FillSlot| {
        slot.advance_source.is_some()
            || matches!(slot.cell, FieldCell::NonBreakingBlank | FieldCell::Tab)
    };
    !slots[..advance]
        .iter()
        .rev()
        .take_while(|slot| reading_ink(slot, reading).is_none())
        .chain(slots[advance + 1..next_ink].iter())
        .any(another_advance)
        && slots
            .get(next_ink + 1)
            .is_none_or(|slot| slot.cell != FieldCell::Backline)
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
/// line (`term.c:691-699, 868-872, 389-428`). Give its spaces their own
/// cells instead of attaching a collapsed separator to the next graph:
/// that graph may be overwritten later.
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
                && matches!(
                    reading[owner].glyph,
                    ReadingGlyph::Scalar(ch) if !ch.is_whitespace() || ch == '\u{a0}'
                )
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
                // occupies that same column (term.c:911-914).
                end -= 2;
            }
            if let Some((advances, width)) = collect_inner_advances(slots, reading, at + 1..end) {
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

fn collect_inner_advances(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    mut between: Range<usize>,
) -> Option<(Vec<(usize, usize)>, usize)> {
    let mut advances = Vec::new();
    let mut inherited_advances = Vec::new();
    let mut width = 0usize;
    let usable = between.all(|index| {
        let gap = &slots[index];
        match gap.owner {
            Some(owner) if matches!(reading[owner].glyph, ReadingGlyph::DeviceAdvance) => {
                advances.push((index, owner));
                width = width.saturating_add(gap.count);
                true
            }
            Some(owner) if matches!(reading[owner].glyph, ReadingGlyph::Scalar(' ')) => true,
            // A traversed deferred NBRSP already has a claim on the
            // accepted row. Allow new motion beside it without copying or
            // swallowing its existing claim (term.c:389–428, 860–872).
            Some(owner)
                if gap.owned_deferred_blank
                    && gap.cell == FieldCell::NonBreakingBlank
                    && matches!(reading[owner].glyph, ReadingGlyph::Earlier(0)) =>
            {
                true
            }
            Some(owner)
                if matches!(reading[owner].glyph, ReadingGlyph::Earlier(0))
                    && !gap.owned_deferred_blank
                    && matches!(
                        gap.cell,
                        FieldCell::NonBreakingBlank | FieldCell::BreakableBlank
                    ) =>
            {
                // term.c:868-869: bufferc(ASCII_NBRSP) advances past an
                // existing unprojected blank at col < lastcol without
                // replacing it. Publish it alongside the incoming HORIZ once
                // the later graph proves that pass.
                inherited_advances.push(index);
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
    if !usable || advances.is_empty() || width == 0 {
        return None;
    }
    let style_owner = advances[0].1;
    if !inherited_advances.is_empty() {
        advances.extend(
            inherited_advances
                .into_iter()
                .map(|slot| (slot, style_owner)),
        );
        advances.sort_unstable_by_key(|&(slot, _)| slot);
    }
    Some((advances, width))
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
                    owned_deferred_blank: false,
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
