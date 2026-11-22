//! Project accepted native slots to reading text and its owners.

use super::super::{WordOutput, references};
use super::{
    FieldCell, FieldWrite, Font, GeometrySeed, Placement, PriorPendingSlot, PriorSlotEdit,
    ReadingCell, ReadingGlyph, TextExecutionContext, WordExecution,
};
use crate::mandoc::inline::{
    flow::{
        field_buffer::{MAX_EDITABLE_APPEND_CELLS, isolated_passes},
        native_slots::FillSlot,
    },
    font::style::styled_link,
};

mod advances;

use advances::{incoming_ink, prepare_reading_blanks, preserve_author_separators};

pub(super) struct PlacedWord<'a> {
    slots: Vec<FillSlot>,
    reading: Vec<ReadingCell<'a>>,
    links: Vec<&'a str>,
    writes: Vec<FieldWrite>,
    pending: Option<usize>,
    armed: bool,
    prior: Option<PriorSlotProjection>,
}

#[derive(Clone, Copy)]
enum PriorSlotProjection {
    Buffered { owner: usize, survives: bool },
    ConsumedProjected,
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
            prior_pending,
            replace_tail,
            replayed_entry,
            advance_marker,
            editable_pass,
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
        let replaced_advance_marker = advance_marker.filter(|_| {
            retain_suffix && extent <= MAX_EDITABLE_APPEND_CELLS && word_owner.is_some()
        });
        prepare_reading_blanks(
            &mut slots,
            &mut reading,
            earlier,
            word_owner,
            exact_inner_blanks,
            replaced_advance_marker,
        );
        let settled = settle_replay_slots(
            &mut slots,
            editable_pass && extent <= MAX_EDITABLE_APPEND_CELLS,
        );
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
                prior_pending.and_then(PriorPendingSlot::unpublished),
            ));
        } else if replace_tail > 0 {
            writes.push(replay_device_tail(
                &slots,
                &reading,
                replace_tail,
                replayed_entry,
            ));
        }
        let (inherited_pending, prior) =
            settle_prior_slot(prior_pending, pending_owner, &slots, &reading, earlier)?;
        writes.extend(slots.iter().map(|slot| slot_write(slot, &reading)));
        let end = slots.len();
        record_slot_proof(
            &mut writes,
            &settled,
            &slots,
            extent,
            retain_suffix.then_some(FieldWrite::ProveRawSlots {
                count: end,
                cursor_retreat,
                backbefore,
                duplicates,
                editable_marker,
            }),
        );
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
        let prior = self.prior.and_then(|prior| match prior {
            PriorSlotProjection::Buffered { owner, survives } => {
                let output = context.zero_advance.project_prior_slot(survives);
                survives.then_some((owner, output))
            }
            PriorSlotProjection::ConsumedProjected => {
                // This advance already has its original output owner.
                // Release BACKBEFORE without publishing another glyph.
                let output = context.zero_advance.project_prior_slot(false);
                debug_assert_eq!(output, [] as [mant_ir::Inline; 0]);
                None
            }
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

fn settle_prior_slot(
    previous: Option<PriorPendingSlot>,
    pending_owner: Option<usize>,
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    earlier: usize,
) -> Result<(Option<usize>, Option<PriorSlotProjection>), PriorSlotEdit> {
    let Some(previous) = previous else {
        return Ok((None, None));
    };
    let owner = previous.owner();
    let position = slots.iter().rposition(|slot| slot.owner == Some(owner));
    if pending_owner == Some(owner)
        && let Some(position) = position
    {
        if slots[..position].iter().any(|slot| {
            slot.owner.is_some_and(|owner| {
                owner >= earlier && !matches!(reading[owner].glyph, ReadingGlyph::EntryBlank)
            })
        }) {
            // An inherited glyph still under BACKBEFORE cannot be
            // published at a new position before its replacement is known.
            return Err(PriorSlotEdit::Unresolved);
        }
        return Ok((Some(position), None));
    }
    Ok((
        None,
        Some(match previous {
            PriorPendingSlot::Buffered(owner) => PriorSlotProjection::Buffered {
                owner,
                survives: position.is_some(),
            },
            PriorPendingSlot::Projected(_) => PriorSlotProjection::ConsumedProjected,
        }),
    ))
}

fn settle_replay_slots(
    slots: &mut Vec<FillSlot>,
    keep_raw_advances: bool,
) -> Vec<std::ops::Range<usize>> {
    if keep_raw_advances {
        expand_replay_advances(slots);
    }
    super::overprint::settle(slots)
}

fn expand_replay_advances(slots: &mut Vec<FillSlot>) {
    // Keep raw advances addressable through the next term_word().
    // The field scanner may compress them once it consumes them.
    for index in (0..slots.len()).rev() {
        let slot = &slots[index];
        if slot.drawing_advance && slot.owner.is_none() && slot.count > 1 {
            let count = slot.count;
            let cell = slot.cell.clone();
            let advance_source = slot.advance_source;
            slots.splice(
                index..=index,
                (0..count).map(|_| FillSlot {
                    cell: cell.clone(),
                    count: 1,
                    owner: None,
                    drawing_advance: true,
                    advance_source,
                }),
            );
        }
    }
}

fn record_slot_proof(
    writes: &mut Vec<FieldWrite>,
    settled: &[std::ops::Range<usize>],
    slots: &[FillSlot],
    extent: usize,
    proof: Option<FieldWrite>,
) {
    let Some(proof) = proof else { return };
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
    let mut first_incoming_ink = None;
    let mut interleaved_ink = false;
    let mut entry_blanks = 0;
    let mut entry_gap = None;
    for (position, slot) in slots.iter().enumerate() {
        let Some(owner) = slot.owner else { continue };
        if owner < earlier {
            positions[owner] = Some(start + position);
            interleaved_ink |= first_incoming_ink.is_some()
                && matches!(slot.cell, FieldCell::Graph { text, .. } if !text.is_whitespace())
                && matches!(reading[owner].glyph, ReadingGlyph::Earlier(1..));
        } else if matches!(reading[owner].glyph, ReadingGlyph::EntryBlank) {
            if position < first_incoming {
                entry_blanks += 1;
                entry_gap = Some(start + position);
            }
        } else {
            first_incoming = first_incoming.min(position);
            if incoming_ink(slot, reading, earlier).is_some() {
                first_incoming_ink.get_or_insert(start + position);
            }
        }
    }
    FieldWrite::ReplaceLivePrefix {
        start,
        positions,
        retained_advances: retain_earlier_advances(slots, start, first_incoming),
        first_incoming: start + first_incoming,
        // Interleaved old glyphs own their individual advances. They
        // cannot share an entry-width correction on the incoming word.
        first_incoming_ink: first_incoming_ink.filter(|_| !interleaved_ink),
        entry_blanks,
        entry_gap: entry_gap.filter(|_| entry_blanks == 1),
        unpublished: unpublished.map(|index| start + index),
    }
}

fn retain_earlier_advances(
    slots: &[FillSlot],
    start: usize,
    first_incoming: usize,
) -> std::collections::BTreeMap<usize, (usize, usize)> {
    let mut advances = std::collections::BTreeMap::<usize, (usize, usize)>::new();
    for (position, slot) in slots.iter().take(first_incoming).enumerate() {
        if let Some(source) = slot.advance_source {
            let (_, width) = advances
                .entry(start + source)
                .or_insert((start + position, 0));
            *width = width.saturating_add(slot.count);
        }
    }
    advances
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
        let cell = match slot.cell {
            FieldCell::Graph {
                text: '\u{a0}',
                width: 1,
            } => slot.cell.clone(),
            _ => FieldCell::FixedDrawingAdvance(1),
        };
        return FieldWrite::PlacedBlank { source, cell };
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

fn append_range(
    word: &mut WordExecution,
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    links: &[String],
    context: &mut TextExecutionContext<'_>,
    mut prior: Option<(usize, Vec<mant_ir::Inline>)>,
) {
    let mut wrote_source = false;
    let mut entered_word = false;
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
            // The native owner interval starts here even though the blank
            // will not acquire a visible scalar until the accepted pass.
            entered_word = true;
            continue;
        }
        if let ReadingGlyph::Earlier(scalars) = current.glyph {
            if entered_word
                && (scalars != 0
                    || matches!(
                        slot.cell,
                        FieldCell::BreakableBlank
                            | FieldCell::NonBreakingBlank
                            | FieldCell::Graph {
                                text: '\u{a0}',
                                width: 1
                            }
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
        entered_word = true;
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
