//! Project accepted native slots to reading text and its owners.

use super::super::{WordOutput, references};
use super::{
    FieldCell, FieldWrite, Font, GeometrySeed, Placement, PriorSlotEdit, ReadingCell, ReadingGlyph,
    TextExecutionContext, WordExecution,
};
use crate::mandoc::inline::{
    flow::{field_buffer::isolated_passes, native_slots::FillSlot},
    font::style::styled_link,
};

pub(super) struct PlacedWord<'a> {
    slots: Vec<FillSlot>,
    reading: Vec<ReadingCell<'a>>,
    links: Vec<&'a str>,
    writes: Vec<FieldWrite>,
    pending: Option<usize>,
    armed: bool,
    prior: Option<bool>,
}

impl<'a> PlacedWord<'a> {
    pub(super) fn prepare(
        seed: GeometrySeed<'a>,
        retain_suffix: bool,
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
        let append_extent = slots.append_extent();
        let next_tail = retain_suffix.then(|| slots.unowned_tail()).flatten();
        let mut slots = slots.into_fill_slots();
        let duplicates = keep_one_copy_per_owner(&mut slots, reading.len());
        let zero_width_retreats = resolve_backlines(&mut slots, retain_suffix);
        preserve_author_separators(&slots, &mut reading);
        let mut writes = Vec::new();
        if earlier != 0 {
            writes.push(replay_prefix(
                &slots,
                &reading,
                prefix_start,
                earlier,
                unpublished,
            )?);
        } else if replace_tail > 0 {
            let entry_blanks = replayed_entry.then(|| {
                slots
                    .iter()
                    .filter(|slot| {
                        slot.owner
                            .is_some_and(|id| matches!(reading[id].glyph, ReadingGlyph::EntryBlank))
                    })
                    .count()
            });
            writes.push(FieldWrite::ReplacePlacedTail {
                cells: replace_tail,
                entry_blanks,
                first_incoming: replayed_entry.then(|| {
                    slots
                        .iter()
                        .position(|slot| {
                            slot.owner.is_some_and(|id| {
                                !matches!(reading[id].glyph, ReadingGlyph::EntryBlank)
                            })
                        })
                        .unwrap_or(slots.len())
                }),
            });
        }
        let inherited_pending = pending_owner
            .filter(|owner| Some(*owner) == unpublished)
            .and_then(|owner| slots.iter().rposition(|slot| slot.owner == Some(owner)));
        let prior = unpublished
            .filter(|_| inherited_pending.is_none())
            .map(|id| slots.iter().any(|slot| slot.owner == Some(id)));
        writes.extend(slots.iter().map(|slot| slot_write(slot, &reading)));
        let end = slots.len();
        if retain_suffix && append_extent == Some(end) {
            writes.push(FieldWrite::ProveAppendSlots {
                count: end,
                backbefore,
                duplicates,
            });
        }
        writes.extend(zero_width_retreats.into_iter().map(|(target, mark)| {
            FieldWrite::PlacedZeroWidthRetreat {
                target_from_end: end - target,
                mark_from_end: end - mark,
            }
        }));
        if let Some(index) = inherited_pending {
            // A control such as an empty overstrike can leave both the
            // old glyph and BACKBEFORE intact. The next real glyph, not
            // this word boundary, must decide whether to cover it.
            writes.push(FieldWrite::KeepPlacedRetreat {
                from_end: end - index,
            });
        }
        if let Some(tail) = next_tail {
            writes.push(FieldWrite::RetainDeviceTail(Box::new(tail)));
        }
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
        if let Some(survives) = self.prior {
            word.output
                .extend(context.zero_advance.project_prior_slot(survives));
        }
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
            append_range(word, &self.slots, &self.reading, &links, context);
        } else {
            let passes = isolated_passes(&self.writes, context.table_field_units);
            for covered in passes.covered {
                self.slots[covered].owner = None;
            }
            for (range, closes) in passes.ranges {
                append_range(word, &self.slots[range], &self.reading, &links, context);
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

fn keep_one_copy_per_owner(slots: &mut [FillSlot], owners: usize) -> Vec<(usize, usize)> {
    let mut latest = vec![None; owners];
    let mut duplicates = Vec::new();
    for (index, slot) in slots.iter_mut().enumerate().rev() {
        let Some(owner) = slot.owner else { continue };
        if let Some(last) = latest[owner] {
            // Both encoded copies retain an author while the field is being
            // edited. Publish its scalar once and retain the earlier byte's
            // ownership for any later word that overlaps this suffix.
            slot.owner = None;
            duplicates.push((index, last));
        } else {
            latest[owner] = Some(index);
        }
    }
    duplicates.reverse();
    duplicates
}

fn replay_prefix(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    start: usize,
    earlier: usize,
    unpublished: Option<usize>,
) -> Result<FieldWrite, PriorSlotEdit> {
    let mut positions = vec![None; earlier];
    let mut first_incoming = slots.len();
    let mut entry_blanks = 0;
    for (position, slot) in slots.iter().enumerate() {
        let Some(owner) = slot.owner else { continue };
        if owner < earlier {
            if first_incoming < position && reading[owner].glyph.scalars() > 0 {
                // IR carries a single earlier owner followed by this word.
                // Do not silently reorder interleaved author projections.
                return Err(PriorSlotEdit::Unresolved);
            }
            positions[owner] = Some(start + position);
        } else if matches!(reading[owner].glyph, ReadingGlyph::EntryBlank) {
            if position < first_incoming {
                entry_blanks += 1;
            }
        } else {
            first_incoming = first_incoming.min(position);
        }
    }
    Ok(FieldWrite::ReplaceLivePrefix {
        start,
        positions,
        first_incoming: start + first_incoming,
        entry_blanks,
        unpublished: unpublished.map(|index| start + index),
    })
}

fn resolve_backlines(slots: &mut [FillSlot], editable: bool) -> Vec<(usize, usize)> {
    // term_field() skips controls, including NBRZW written by tty
    // annotation and ASCII_BREAK. A backspace still reaches the last
    // emitted graph. A break opportunity or zero-width mark defers
    // coverage until the subsequent advance is accepted in that pass.
    let mut covers = Vec::new();
    let mut previous: Option<usize> = None;
    let mut retreat = None;
    for index in 0..slots.len() {
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
                previous = None;
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
) {
    let mut wrote_source = false;
    let mut previous_source = None;
    for slot in slots {
        let Some(id) = slot.owner else { continue };
        let current = reading[id];
        if matches!(
            current.glyph,
            ReadingGlyph::Earlier(_) | ReadingGlyph::EntryBlank
        ) {
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
