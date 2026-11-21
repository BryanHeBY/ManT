//! Project accepted native slots to reading text and its owners.

use super::super::{WordOutput, references};
use super::{
    FieldCell, FieldWrite, Font, GeometrySeed, Placement, PriorSlotEdit, ReadingCell, ReadingGlyph,
    TextExecutionContext, WordExecution,
};
use crate::mandoc::inline::{
    flow::{
        field_buffer::{DeferredSourceBlank, isolated_passes},
        native_slots::FillSlot,
    },
    font::style::styled_link,
};

pub(super) struct PlacedWord<'a> {
    slots: Vec<FillSlot>,
    reading: Vec<ReadingCell<'a>>,
    links: Vec<&'a str>,
    writes: Vec<FieldWrite>,
    pending: Option<usize>,
    armed: bool,
    prior: Option<(usize, bool)>,
}

impl<'a> PlacedWord<'a> {
    pub(super) fn prepare(
        seed: GeometrySeed<'a>,
        retain_suffix: bool,
        exact_inner_blanks: bool,
        editable_marker: bool,
        word_owner: Option<u64>,
    ) -> Result<Self, PriorSlotEdit> {
        let GeometrySeed {
            placement,
            prefix_start,
            earlier,
            unpublished,
            replace_tail,
            replayed_entry,
        } = seed;
        let Placement {
            slots,
            mut reading,
            links,
            ..
        } = placement;
        let armed = slots.backafter();
        let backbefore = slots.backbefore();
        let pending_owner = slots.pending_owner();
        let pending = pending_owner.filter(|&owner| owner >= earlier);
        let (extent, cursor_retreat) = slots.extent_and_retreat();
        let next_tail = retain_suffix.then(|| slots.unowned_tail()).flatten();
        let mut slots = slots.into_fill_slots();
        if exact_inner_blanks {
            publish_inner_advances(&mut slots, &mut reading, earlier);
        }
        defer_retained_separator_advances(&mut slots, &mut reading, earlier, word_owner);
        let settled = super::overprint::settle(&mut slots);
        let duplicates = keep_one_copy_per_owner(&mut slots, reading.len());
        let zero_width_retreats = resolve_backlines(&mut slots, retain_suffix, &settled);
        retire_trailing_entry_blanks(&mut slots, &reading, earlier);
        preserve_author_separators(&slots, &mut reading);
        let mut writes = Vec::new();
        if earlier != 0 {
            writes.push(replay_prefix(
                &slots,
                &reading,
                prefix_start,
                earlier,
                unpublished,
            ));
        } else if replace_tail > 0 {
            writes.push(replay_device_tail(
                &slots,
                &reading,
                replace_tail,
                replayed_entry,
            ));
        }
        let inherited_pending = pending_owner
            .filter(|owner| Some(*owner) == unpublished)
            .and_then(|owner| slots.iter().rposition(|slot| slot.owner == Some(owner)));
        let prior = unpublished
            .filter(|_| inherited_pending.is_none())
            .map(|id| (id, slots.iter().any(|slot| slot.owner == Some(id))));
        if let Some(position) = inherited_pending
            && slots[..position].iter().any(|slot| {
                slot.owner.is_some_and(|owner| {
                    owner >= earlier && !matches!(reading[owner].glyph, ReadingGlyph::EntryBlank)
                })
            })
        {
            // An inherited glyph still under BACKBEFORE cannot be
            // published at a new position before its replacement is known.
            return Err(PriorSlotEdit::Unresolved);
        }
        writes.extend(slots.iter().map(|slot| slot_write(slot, &reading)));
        let end = slots.len();
        if retain_suffix {
            record_slot_proof(
                &mut writes,
                &settled,
                &slots,
                extent,
                FieldWrite::ProveRawSlots {
                    count: end,
                    cursor_retreat,
                    backbefore,
                    duplicates,
                    editable_marker,
                },
            );
        }
        writes.extend(zero_width_retreats.into_iter().map(|(target, mark)| {
            FieldWrite::PlacedZeroWidthRetreat {
                target_from_end: end - target,
                mark_from_end: end - mark,
            }
        }));
        writes.extend(
            inherited_pending.map(|index| FieldWrite::KeepPlacedRetreat {
                from_end: end - index,
            }),
        );
        writes.extend(next_tail.map(|tail| FieldWrite::RetainDeviceTail(Box::new(tail))));
        Ok(Self {
            slots,
            reading,
            links,
            writes,
            pending,
            armed,
            prior,
        })
    }

    pub(super) fn project(
        mut self,
        context: &mut TextExecutionContext<'_>,
        word: &mut WordExecution,
    ) {
        let prior = self.prior.and_then(|(id, survives)| {
            let output = context.zero_advance.project_prior_slot(survives);
            survives.then_some((id, output))
        });
        let links: Vec<_> = self
            .links
            .iter()
            .map(|uri| {
                let target = context
                    .zero_advance
                    .glyph_annotation_target(mant_ir::LinkTarget::External { uri: (*uri).into() });
                let mant_ir::LinkTarget::External { uri } = target else {
                    unreachable!()
                };
                uri
            })
            .collect();
        word.table_device_buffer_written = !self.slots.is_empty();
        if context.policy.field_authoritative {
            self.buffer_tail_glyph(context, &links);
            append_range(word, &self.slots, &self.reading, &links, context, prior);
        } else {
            let passes = isolated_passes(&self.writes, context.table_field_units);
            word.table_reading_breaks =
                Some(passes.ranges.iter().filter(|(_, hard)| *hard).count());
            for covered in passes.covered {
                self.slots[covered].owner = None;
            }
            for (range, closes) in passes.ranges {
                append_range(
                    word,
                    &self.slots[range],
                    &self.reading,
                    &links,
                    context,
                    None,
                );
                if closes {
                    word.flush_segment();
                    word.output.push(mant_ir::Inline::line_break());
                }
            }
        }
        word.flush_segment();
        context.zero_advance.inherit_armed(self.armed);
        if self.armed {
            self.writes.push(FieldWrite::ArmBackafter);
        }
        word.native_writes = self.writes;
    }

    fn buffer_tail_glyph(&mut self, context: &mut TextExecutionContext<'_>, links: &[String]) {
        let Some(owner) = self.pending else { return };
        let Some(index) = self
            .slots
            .iter()
            .rposition(|slot| slot.owner == Some(owner))
        else {
            return;
        };
        if self.slots[index + 1..]
            .iter()
            .any(|slot| slot.owner.is_some())
        {
            return;
        }
        let reading = self.reading[owner];
        let mut value = String::new();
        reading.glyph.append(&mut value);
        context.zero_advance.inherit_buffered_glyph(styled_link(
            value,
            reading.font,
            reading.link.map(|link| links[link].as_str()),
        ));
        self.writes.push(FieldWrite::KeepPlacedRetreat {
            from_end: self.slots.len() - index,
        });
        self.slots[index].owner = None;
    }
}

/// The next word can leave a short motion after an authored separator in
/// the same accepted pass. Read it from the replayed slots: an edit can
/// overwrite that motion, and `term_field()` can still drop it at row end.
fn defer_retained_separator_advances(
    slots: &mut [FillSlot],
    reading: &mut Vec<ReadingCell<'_>>,
    earlier: usize,
    word_owner: Option<u64>,
) {
    let Some(word_owner) = word_owner else { return };
    for index in 1..slots.len() {
        let motion = &slots[index];
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
        if !motion.drawing_advance && !after_authored_break {
            continue;
        }
        if !slots[..index]
            .iter()
            .rev()
            .find(|slot| !slot.drawing_advance)
            .is_some_and(|slot| {
                slot.cell == FieldCell::BreakMarker
                    && slot.owner.is_some_and(|owner| owner < earlier)
            })
        {
            continue;
        }
        let Some(owner) = slots[index + 1..].iter().find_map(|slot| {
            slot.owner.filter(|&owner| owner >= earlier && matches!(reading[owner].glyph, ReadingGlyph::Scalar(ch) if !ch.is_whitespace()))
        }) else { continue };
        let id = reading.len();
        reading.push(ReadingCell {
            glyph: ReadingGlyph::DeferredBlank(DeferredSourceBlank {
                owner: word_owner,
                write: index,
                font: reading[owner].font,
            }),
            link: None,
            ..reading[owner]
        });
        slots[index].owner = Some(id);
    }
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
                }
            })
            .collect::<Vec<_>>();
        if projected < slot.count {
            blanks.push(FillSlot {
                cell: slot.cell.clone(),
                count: slot.count - projected,
                owner: None,
                drawing_advance: slot.drawing_advance,
            });
        }
        slots.splice(index..=index, blanks);
    }
}

fn record_slot_proof(
    writes: &mut Vec<FieldWrite>,
    settled: &[std::ops::Range<usize>],
    slots: &[FillSlot],
    extent: usize,
    proof: FieldWrite,
) {
    if !settled.is_empty() {
        // The visible column order cannot prove one owner per raw byte.
        writes.push(FieldWrite::OpaqueRawSlots {
            styled_suffix: None,
            keeps_prior_slots: false,
        });
    } else if slots.iter().all(|slot| {
        slot.count == 1
            || matches!(
                slot.cell,
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
            ) && slot.owner.is_none()
    }) && extent == slots.iter().map(|slot| slot.count).sum::<usize>()
    {
        writes.push(proof);
    }
}

fn keep_one_copy_per_owner(slots: &mut [FillSlot], owners: usize) -> Vec<(usize, usize)> {
    let mut first = vec![None::<usize>; owners];
    let mut duplicates = Vec::new();
    for index in 0..slots.len() {
        let Some(owner) = slots[index].owner else {
            continue;
        };
        if let Some(previous) = first[owner] {
            // Both copies of bold ink have one author. If they still share
            // a device column, the final copy owns the reading glyph. If a
            // new write replaced their backspace, retain the original
            // reading position and keep the second copy as a raw alias.
            if index == previous + 2 && slots[previous + 1].cell == FieldCell::Backline {
                slots[previous].owner = None;
                first[owner] = Some(index);
                duplicates.push((previous, index));
            } else {
                slots[index].owner = None;
                duplicates.push((index, previous));
            }
        } else {
            first[owner] = Some(index);
        }
    }
    duplicates.sort_unstable_by_key(|&(copy, _)| copy);
    duplicates
}

fn replay_device_tail(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    cells: usize,
    replayed_entry: bool,
) -> FieldWrite {
    let is_entry = |id: usize| matches!(reading[id].glyph, ReadingGlyph::EntryBlank);
    FieldWrite::ReplacePlacedTail {
        cells,
        entry_blanks: replayed_entry.then(|| {
            slots
                .iter()
                .filter(|slot| slot.owner.is_some_and(is_entry))
                .count()
        }),
        first_incoming: replayed_entry.then(|| {
            slots
                .iter()
                .position(|slot| slot.owner.is_some_and(|id| !is_entry(id)))
                .unwrap_or(slots.len())
        }),
    }
}

fn replay_prefix(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    start: usize,
    earlier: usize,
    unpublished: Option<usize>,
) -> FieldWrite {
    let mut positions = vec![None; earlier];
    let mut first_incoming = slots.len();
    let mut entry_blanks = 0;
    let mut entry_gap = None;
    for (position, slot) in slots.iter().enumerate() {
        let Some(owner) = slot.owner else { continue };
        if owner < earlier {
            positions[owner] = Some(start + position);
        } else if matches!(reading[owner].glyph, ReadingGlyph::EntryBlank) {
            if position < first_incoming {
                entry_blanks += 1;
                entry_gap = Some(start + position);
            }
        } else {
            first_incoming = first_incoming.min(position);
        }
    }
    FieldWrite::ReplaceLivePrefix {
        start,
        positions,
        first_incoming: start + first_incoming,
        entry_blanks,
        entry_gap: entry_gap.filter(|_| entry_blanks == 1),
        unpublished: unpublished.map(|index| start + index),
    }
}

fn retire_trailing_entry_blanks(
    slots: &mut [FillSlot],
    reading: &[ReadingCell<'_>],
    earlier: usize,
) {
    let Some(graph) = slots.iter().rposition(|slot| {
        matches!(slot.cell, FieldCell::Graph { .. } | FieldCell::Hyphen)
            && slot.owner.is_some_and(|id| reading[id].glyph.scalars() > 0)
    }) else {
        return;
    };
    if !slots[..=graph].iter().any(|slot| {
        matches!(slot.cell, FieldCell::Graph { .. } | FieldCell::Hyphen)
            && slot.owner.is_some_and(|id| id >= earlier)
    }) {
        return;
    }
    // An entry blank can end up to the right of the incoming glyph.
    // term_field() defers that advance until another graph prints it.
    // Keep the byte available for later writes without claiming a scalar.
    for slot in &mut slots[graph + 1..] {
        if slot
            .owner
            .is_some_and(|id| matches!(reading[id].glyph, ReadingGlyph::EntryBlank))
        {
            slot.owner = None;
        }
    }
}

fn resolve_backlines(
    slots: &mut [FillSlot],
    editable: bool,
    settled: &[std::ops::Range<usize>],
) -> Vec<(usize, usize)> {
    // term_field() skips controls, including NBRZW written by tty
    // annotation and ASCII_BREAK. A backspace still reaches the last
    // emitted graph. A break opportunity or zero-width mark defers
    // coverage until the subsequent advance is accepted in that pass.
    let mut covers = Vec::new();
    let mut previous: Option<usize> = None;
    let mut retreat = None;
    let mut spans = settled.iter().peekable();
    for index in 0..slots.len() {
        if let Some(span) = spans.peek()
            && span.contains(&index)
        {
            if index == span.start {
                retreat = None;
                previous = None;
            }
            if slots[index].owner.is_some() {
                previous = Some(index);
            }
            if index + 1 == span.end {
                spans.next();
            }
            continue;
        }
        match slots[index].cell {
            FieldCell::BreakMarker
            | FieldCell::Breakpoint
            | FieldCell::TabReference
            | FieldCell::ZeroWidthGraph => {}
            FieldCell::Backline => {
                retreat = previous.take().map(|target| (target, index));
            }
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => {
                if let Some((target, _)) = retreat.take() {
                    // term_field() advances over a buffered blank only
                    // if a later device write prints on the same pass.
                    covers.push((target, index));
                }
                // Defer coverage of a blank when the backspace follows
                // skipped cells. The blank still prints before the next
                // graph in term_field, so the backspace reaches that cell.
                previous = Some(index);
            }
            FieldCell::Graph { .. } | FieldCell::Hyphen => {
                if let Some((target, backline)) = retreat.take() {
                    if editable
                        || matches!(slots[index].cell, FieldCell::Graph { width: 0, .. })
                        || target + 1 != backline
                        || backline + 1 != index
                        || slots[target].cell == FieldCell::Hyphen
                    {
                        covers.push((target, index));
                    } else {
                        slots[target].owner = None;
                    }
                }
                previous = (slots[index].owner.is_some()
                    && !matches!(slots[index].cell, FieldCell::Graph { width: 0, .. }))
                .then_some(index);
            }
            _ => {
                previous = None;
                retreat = None;
            }
        }
    }
    covers
}

fn slot_write(slot: &FillSlot, reading: &[ReadingCell<'_>]) -> FieldWrite {
    if let Some(ReadingGlyph::DeferredBlank(source)) = slot.owner.map(|owner| reading[owner].glyph)
    {
        return FieldWrite::PlacedBlank { source };
    }
    let count = slot.count;
    let scalars = slot.owner.map_or(0, |id| {
        let read = reading[id];
        read.glyph.scalars()
            + usize::from(
                read.preserve_separator && !matches!(read.glyph, ReadingGlyph::Earlier(_)),
            )
    });
    let cell = match slot.cell {
        FieldCell::BreakableBlank if scalars == 0 => FieldCell::DrawnSpaces(count),
        FieldCell::NonBreakingBlank if scalars == 0 => FieldCell::FixedDrawingAdvance(count),
        FieldCell::Graph { text, width } => FieldCell::Graph {
            text,
            width: width.saturating_mul(count),
        },
        _ => slot.cell.clone(),
    };
    FieldWrite::Placed { cell, scalars }
}

fn preserve_author_separators(slots: &[FillSlot], reading: &mut [ReadingCell<'_>]) {
    // An authored blank after a marker can traverse an older glyph or
    // overstrike byte without writing native SP (term.c::bufferc). Keep
    // its reading boundary when that pass still contains both words.
    let mut after_author = false;
    let mut blank_between = false;
    let mut break_at_next_blank = false;
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
            _ => {
                if let Some(id) = slot.owner {
                    reading[id].preserve_separator =
                        after_author && (reading[id].authored_separator || blank_between);
                    after_author = true;
                    blank_between = false;
                }
            }
        }
    }
}

fn append_range(
    word: &mut WordExecution,
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    links: &[String],
    context: &mut TextExecutionContext<'_>,
    mut prior: Option<(usize, Vec<mant_ir::Inline>)>,
) {
    let mut wrote_source = false;
    let mut previous_source = None;
    for slot in slots {
        let Some(id) = slot.owner else { continue };
        if prior.as_ref().is_some_and(|(owner, _)| *owner == id) {
            word.flush_segment();
            word.output
                .extend(prior.take().expect("matched pending glyph").1);
            continue;
        }
        let current = reading[id];
        if let ReadingGlyph::DeferredBlank(blank) = current.glyph {
            word.flush_segment();
            word.output.push(mant_ir::Inline::anchor(blank.marker()));
            continue;
        }
        if let ReadingGlyph::Earlier(scalars) = current.glyph {
            if wrote_source
                && (scalars != 0
                    || matches!(
                        slot.cell,
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
                    ))
            {
                word.flush_segment();
                word.output.push(mant_ir::Inline::anchor(format!(
                    "{}{id}",
                    crate::mandoc::inline::flow::RELOCATED_GLYPH
                )));
            }
            continue;
        }
        if matches!(current.glyph, ReadingGlyph::EntryBlank) {
            continue;
        }
        let link = current.link.map(|link| links[link].as_str());
        if current.font != word.font || link != word.link.as_deref() {
            word.flush_segment();
            word.font = current.font;
            word.link = link.map(str::to_owned);
        }
        if current.preserve_separator && wrote_source {
            word.buffer.push(' ');
        }
        current.glyph.append(&mut word.buffer);
        wrote_source = true;
        if current.sphinx_marker && previous_source == id.checked_sub(1) {
            let eligible = context.policy.recognize_generated_references
                && link.is_none()
                && !matches!(
                    current.font,
                    Font::Code | Font::CodeStrong | Font::CodeEmphasis
                );
            references::qualify_empty_destination(
                &mut WordOutput {
                    output: &mut word.output,
                    buffer: &mut word.buffer,
                    font: word.font,
                    link,
                    zero_advance: context.zero_advance,
                },
                eligible,
            );
        }
        previous_source = Some(id);
    }
}
