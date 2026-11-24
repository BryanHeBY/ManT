//! Project accepted native slots to reading text and its owners.

use super::super::{WordOutput, references};
use super::{
    AdvanceReading, FieldCell, FieldWrite, Font, GeometrySeed, Placement, PriorPendingSlot,
    PriorSlotEdit, ReadingCell, ReadingGlyph, ReplayDomain, TextExecutionContext, WordExecution,
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
    pending_slot: Option<usize>,
    split_copy_max: Option<usize>,
    armed: bool,
    prior: Option<PriorSlotProjection>,
}

#[derive(Clone, Copy)]
enum PriorSlotProjection {
    Buffered { owner: usize, survives: bool },
    ConsumedProjected,
}

impl<'a> PlacedWord<'a> {
    /// Finalize raw `tcol->buf` slots, overprint/backline receipts, and
    /// unowned `DeviceTail` cell counts after `expand_replay_advances()`
    /// (`term.c:678-718, 860-872, 911-929`).
    pub(super) fn prepare(
        seed: GeometrySeed<'a>,
        retain_suffix: bool,
        advances: AdvanceReading,
        editable_marker: bool,
        domain: ReplayDomain,
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
        let raw_backbefore_slot = slots.backbefore_slot();
        let pending = pending_owner.filter(|&owner| owner >= earlier);
        let (extent, cursor_retreat) = slots.extent_and_retreat();
        let mut next_tail = retain_suffix.then(|| slots.unowned_tail()).flatten();
        let mut slots = slots.into_fill_slots();
        let tail_fill_sum = device_tail_raw_sum(next_tail.as_ref(), &slots);
        let can_edit_tail = retain_suffix && extent <= MAX_EDITABLE_APPEND_CELLS;
        if can_edit_tail && (editable_pass || domain == ReplayDomain::Filled) {
            release_unpaired_underlines(&mut slots, &mut reading, domain);
        }
        let replaced_advance_marker =
            advance_marker.filter(|_| can_edit_tail && word_owner.is_some());
        prepare_reading_blanks(
            &mut slots,
            &mut reading,
            earlier,
            word_owner,
            advances,
            replaced_advance_marker,
        );
        let settled = settle_replay_slots(
            &mut slots,
            (editable_pass || editable_marker || cursor_retreat > 0)
                && extent <= MAX_EDITABLE_APPEND_CELLS,
            cursor_retreat > 0,
        );
        sync_device_tail_cells(&mut next_tail, &slots, tail_fill_sum);
        let duplicates = keep_one_copy_per_owner(&mut slots, reading.len());
        // A native edit that overwrites the backspace between a bold glyph's
        // two copies leaves both copies visible (`term.c:915-925`). Keep
        // the alias proof and hold pass retirement until `retire_plain_paint`
        // can run `edited_font_copy_end()` on the accepted row.
        let split_copy_max = split_bold_copy_max(&duplicates, &slots);
        let split_bold_copy = matches!(
            domain,
            ReplayDomain::Filled | ReplayDomain::Scoped | ReplayDomain::DefinitionLine
        ) && split_copy_max.is_some();
        let retain_suffix = retain_suffix || split_bold_copy;
        let editable_marker = editable_marker || split_bold_copy;
        let zero_width_retreats = resolve_backlines(&mut slots, retain_suffix, &settled);
        retire_trailing_entry_blanks(&mut slots, &mut reading, earlier);
        preserve_author_separators(&slots, &mut reading);
        let (inherited_pending, prior) =
            settle_prior_slot(prior_pending, pending_owner, &slots, &reading, earlier)?;
        let defer_strokes = retain_suffix
            && editable_pass
            && domain == ReplayDomain::DefinitionLine
            && extent <= MAX_EDITABLE_APPEND_CELLS;
        let mut writes = placed_slot_writes(
            &slots,
            &reading,
            (prefix_start, earlier, prior_pending),
            (replace_tail, replayed_entry),
            defer_strokes,
        );
        let end = slots.len();
        let proof = retain_suffix.then_some(FieldWrite::ProveRawSlots {
            count: end,
            cursor_retreat,
            backbefore,
            duplicates,
            editable_marker,
        });
        record_slot_proof(&mut writes, &settled, &slots, extent, proof);
        append_placement_receipts(
            &mut writes,
            end,
            zero_width_retreats,
            inherited_pending,
            next_tail,
        );
        let pending_slot =
            resolve_pending_slot(&slots, pending, raw_backbefore_slot, backbefore, extent);
        Ok(Self {
            slots,
            reading,
            links,
            writes,
            pending,
            pending_slot,
            split_copy_max,
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

    /// Project a generated `-inset`/`-diag` run-in word (`\ ` / `\ \ `,
    /// `mdoc_term.c:757-772`, `chars.c:50`, `term.c:620-636, 888-930`) after
    /// `NativeSlots::with_field_prefix` executed its `encode1(U+00A0)` writes.
    ///
    /// Resolves `self.prior` (`term.c:903-929`) into `pending_head_ink` only
    /// when the prior `TERMP_BACKBEFORE` slot survived `encode1(U+00A0)`, so
    /// the caller can restore HEAD-owned pending ink via
    /// `DeferredHeadProjection::restore_pending_ink` instead of leaking it
    /// into BODY output.
    pub(super) fn project_run_in(
        mut self,
        zero_advance: &mut super::super::ZeroAdvanceState,
        output: &mut Vec<mant_ir::Inline>,
        pending_head_ink: &mut Vec<mant_ir::Inline>,
    ) -> Vec<FieldWrite> {
        match self.prior {
            Some(PriorSlotProjection::Buffered { survives, .. }) => {
                let restored = zero_advance.project_prior_slot(survives);
                if survives {
                    pending_head_ink.extend(restored);
                }
            }
            Some(PriorSlotProjection::ConsumedProjected) => {
                let discarded = zero_advance.project_prior_slot(false);
                debug_assert_eq!(discarded, [] as [mant_ir::Inline; 0]);
            }
            None => {}
        }
        let mut entered_word = false;
        for slot in &self.slots {
            let Some(id) = slot.owner else { continue };
            let current = self.reading[id];
            match current.glyph {
                ReadingGlyph::DeferredBlank { source: blank, .. } => {
                    output.push(mant_ir::Inline::anchor(blank.marker()));
                    entered_word = true;
                }
                ReadingGlyph::Scalar(ch) => {
                    // `release_unpaired_underlines` publishes a literal `'_'`
                    // when `encode1(U+00A0)` overwrites the `\b` after an
                    // italic underline decoration (`term.c:911-914`).
                    output.push(mant_ir::Inline::Text {
                        value: ch.to_string(),
                    });
                    entered_word = true;
                }
                ReadingGlyph::Earlier(scalars)
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
                            )) =>
                {
                    output.push(mant_ir::Inline::anchor(format!(
                        "{}{id}",
                        crate::mandoc::inline::flow::RELOCATED_GLYPH
                    )));
                }
                _ => {}
            }
        }
        // Synchronize zero_advance with post-encode1 BACKAFTER/BACKBEFORE
        // state (`term.c:903-929`): clear consumed BACKAFTER, and when a
        // generated U+00A0 armed BACKBEFORE, keep its slot as `pending_cell`
        // so the following BODY word can replay `can_replay_projected_prior_slot`.
        zero_advance.take_armed();
        if let Some(slot) = self.pending_slot {
            self.writes.push(FieldWrite::KeepPlacedRetreat {
                from_end: self.slots.len() - slot,
            });
            zero_advance.note_projected_pending_glyph();
        } else if self.armed {
            zero_advance.inherit_armed(true);
            self.writes.push(FieldWrite::ArmBackafter);
        }
        self.writes
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
        // A split bold copy after `index` occupies later columns in the
        // accepted row (term.c:915-925) and cannot be hidden behind a
        // tail glyph buffered in zero_advance.
        if self.split_copy_max.is_some_and(|copy| copy > index)
            || self.slots[index + 1..]
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

fn device_tail_raw_sum(
    tail: Option<&crate::mandoc::inline::flow::native_slots::DeviceTail>,
    slots: &[FillSlot],
) -> Option<usize> {
    let tail = tail?;
    let tail_start = slots.len().saturating_sub(tail.fill_cells);
    Some(
        slots[tail_start..]
            .iter()
            .map(|slot| slot.count)
            .sum::<usize>(),
    )
}

/// Keep `DeviceTail::fill_cells` aligned with the final expanded `FillSlot`
/// count after `expand_replay_advances()` splits multi-cell unowned advances
/// (`term.c:678-718, 860-872`). Drop the tail if any trailing slot acquired
/// an owner during `release_unpaired_underlines` or `prepare_reading_blanks`.
fn sync_device_tail_cells(
    next_tail: &mut Option<crate::mandoc::inline::flow::native_slots::DeviceTail>,
    slots: &[FillSlot],
    raw_sum: Option<usize>,
) {
    let (Some(tail), Some(mut remaining)) = (next_tail.as_mut(), raw_sum) else {
        return;
    };
    let mut fill_cells = 0;
    for slot in slots.iter().rev() {
        if remaining == 0 {
            break;
        }
        if slot.owner.is_some() || slot.count > remaining {
            *next_tail = None;
            return;
        }
        remaining -= slot.count;
        fill_cells += 1;
    }
    if remaining == 0 {
        tail.fill_cells = fill_cells;
    } else {
        *next_tail = None;
    }
}

/// Locate the `TERMP_BACKBEFORE` slot left armed by `encode1()` (`term.c:903-929`).
fn resolve_pending_slot(
    slots: &[FillSlot],
    pending: Option<usize>,
    raw_backbefore_slot: Option<usize>,
    backbefore: bool,
    extent: usize,
) -> Option<usize> {
    pending
        .and_then(|owner| slots.iter().rposition(|slot| slot.owner == Some(owner)))
        .or_else(|| {
            raw_backbefore_slot
                .filter(|&slot| backbefore && slot < slots.len() && extent == slots.len())
        })
}

/// Find the furthest bold duplicate whose connecting `\b` was overwritten
/// (`term.c:915-925`: `encode1` emits `c, '\b', c`, so removing `\b` leaves
/// both copies visible).
fn split_bold_copy_max(duplicates: &[(usize, usize)], slots: &[FillSlot]) -> Option<usize> {
    duplicates
        .iter()
        .filter_map(|&(copy, owner)| {
            let low = copy.min(owner);
            let high = copy.max(owner);
            (high != low + 2
                || slots
                    .get(low + 1)
                    .is_none_or(|slot| slot.cell != FieldCell::Backline))
            .then_some(high)
        })
        .max()
}

fn append_placement_receipts(
    writes: &mut Vec<FieldWrite>,
    end: usize,
    zero_width_retreats: Vec<(usize, usize)>,
    inherited_pending: Option<usize>,
    next_tail: Option<crate::mandoc::inline::flow::native_slots::DeviceTail>,
) {
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
}

/// Resolve an inherited `TERMP_BACKBEFORE` glyph after `encode1()` retreat or
/// overwrite (`term.c:903-929`).
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

fn release_unpaired_underlines(
    slots: &mut [FillSlot],
    reading: &mut Vec<ReadingCell<'_>>,
    domain: ReplayDomain,
) {
    // encode1() writes '_' then BS followed by the glyph. A proved
    // edit can overwrite the BS, leaving the stroke at its own column.
    // Plain filled fields can also retain the BS but overwrite the
    // paired glyph with a marker. Definition fields need their eventual
    // row placement to establish where such a trailing BS lands.
    for index in 0..slots.len() {
        if !slots[index].underline_decoration {
            continue;
        }
        let retains_backline = slots
            .get(index + 1)
            .is_some_and(|next| next.cell == FieldCell::Backline);
        let glyph = index + 2;
        let overwritten_by_marker = domain == ReplayDomain::Filled
            && (slots
                .get(glyph)
                .is_some_and(|next| next.cell == FieldCell::BreakMarker)
                || slots.get(glyph..glyph + 3).is_some_and(|tail| {
                    // NBRZW contributes graph but no paint to this pass.
                    // The marker and separator end it before later ink.
                    tail[0].cell == FieldCell::ZeroWidthGraph
                        && tail[1].cell == FieldCell::BreakMarker
                        && matches!(
                            tail[2].cell,
                            FieldCell::Breakpoint | FieldCell::BreakableBlank
                        )
                }));
        if !retains_backline || overwritten_by_marker {
            slots[index].owner = Some(Placement::push_reading(
                reading,
                ReadingGlyph::Scalar('_'),
                Font::Regular,
                None,
            ));
        }
    }
}

fn settle_replay_slots(
    slots: &mut Vec<FillSlot>,
    keep_raw_advances: bool,
    expand_unowned_blanks: bool,
) -> Vec<std::ops::Range<usize>> {
    if keep_raw_advances {
        expand_replay_advances(slots, expand_unowned_blanks);
    }
    super::overprint::settle(slots)
}

fn expand_replay_advances(slots: &mut Vec<FillSlot>, expand_unowned_blanks: bool) {
    // Keep raw advances addressable through the next term_word(). When a
    // live retreated cursor is proved (cursor_retreat > 0), expand multi-cell
    // unowned blanks too so `cells.len() == extent >= cursor_retreat`
    // (term.c:678-718). The field scanner may compress them once consumed.
    for index in (0..slots.len()).rev() {
        let slot = &slots[index];
        let expandable = slot.owner.is_none()
            && slot.count > 1
            && (slot.drawing_advance
                || expand_unowned_blanks
                    && matches!(
                        slot.cell,
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
                    ));
        if expandable {
            let count = slot.count;
            let cell = slot.cell.clone();
            let drawing_advance = slot.drawing_advance;
            let advance_source = slot.advance_source;
            slots.splice(
                index..=index,
                (0..count).map(|_| FillSlot {
                    cell: cell.clone(),
                    count: 1,
                    owner: None,
                    drawing_advance,
                    advance_source,
                    underline_decoration: false,
                    owned_deferred_blank: false,
                }),
            );
        }
    }
}

fn placed_slot_writes(
    slots: &[FillSlot],
    reading: &[ReadingCell<'_>],
    (start, earlier, prior): (usize, usize, Option<PriorPendingSlot>),
    (replace_tail, replayed_entry): (usize, bool),
    defer_strokes: bool,
) -> Vec<FieldWrite> {
    let mut writes = Vec::new();
    if earlier != 0 {
        writes.push(replay_prefix(
            slots,
            reading,
            start,
            earlier,
            prior.and_then(PriorPendingSlot::unpublished),
        ));
    } else if replace_tail > 0 {
        writes.push(replay_device_tail(
            slots,
            reading,
            replace_tail,
            replayed_entry,
        ));
    }
    writes.extend(slots.iter().enumerate().map(|(index, slot)| {
        if defer_strokes
            && slot.underline_decoration
            && slots.get(index + 1..index + 3).is_some_and(|tail| {
                tail[0].cell == FieldCell::Backline && tail[1].cell == FieldCell::BreakMarker
            })
        {
            FieldWrite::PlacedUnderlineStroke
        } else {
            slot_write(slot, reading)
        }
    }));
    writes
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
            // Both copies of bold ink have one author (`term.c:915-925`). If
            // they still share a device column, the final copy owns the
            // reading glyph. If a new write replaced their backspace, retain
            // the original reading position and keep the second copy as a raw
            // alias so a later overwrite of `index` leaves `previous` intact.
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

/// Summarize surviving earlier slots and entry blanks after replaying the live
/// `tcol->buf` prefix (`term.c:573-589, 678-718, 860-872`).
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
    let adopted_advances = slots
        .iter()
        .filter(|slot| slot.count == 1)
        .filter_map(|slot| match reading[slot.owner?].glyph {
            ReadingGlyph::DeferredBlank {
                imported_cell: Some(cell),
                ..
            } => Some(start + cell),
            _ => None,
        })
        .collect();
    FieldWrite::ReplaceLivePrefix {
        start,
        positions,
        retained_advances: retain_earlier_advances(slots, start, first_incoming),
        adopted_advances,
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
    reading: &mut [ReadingCell<'_>],
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
    // An entry blank that survives between incoming glyphs (after a rewound
    // first_incoming glyph and before a later graph) is not counted in
    // replay_prefix().entry_blanks. Project it as an inline space so its
    // 1-scalar native cell matches the emitted word output.
    if let Some(first_incoming) = slots.iter().position(|slot| {
        slot.owner.is_some_and(|id| {
            id >= earlier && !matches!(reading[id].glyph, ReadingGlyph::EntryBlank)
        })
    }) {
        for slot in &slots[first_incoming + 1..=graph] {
            if let Some(id) = slot.owner
                && matches!(reading[id].glyph, ReadingGlyph::EntryBlank)
            {
                reading[id].glyph = ReadingGlyph::Scalar(' ');
            }
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
    if let Some(ReadingGlyph::DeferredBlank { source, .. }) =
        slot.owner.map(|owner| reading[owner].glyph)
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
        if let ReadingGlyph::DeferredBlank { source: blank, .. } = current.glyph {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mandoc::inline::flow::native_slots::NativeSlots;
    use crate::mandoc::roff_escape::RoffFont;

    #[test]
    fn prepare_syncs_device_tail_fill_cells_after_expanding_unowned_advances() {
        // term.c:678-718, 860-872: `A\p\h'3n'` expands the unowned 3-cell
        // `DrawingAdvance` into 3 single-cell `FixedDrawingAdvance(1)` writes
        // when `editable_marker` is set. `DeviceTail::fill_cells` must match
        // all 4 unowned tail cells (`BreakMarker` + 3 advances), not the
        // pre-expansion count of 2.
        let mut slots = NativeSlots::after_entry(true);
        let mut reading = Vec::new();
        let owner_a =
            Placement::push_reading(&mut reading, ReadingGlyph::Scalar('A'), Font::Regular, None);
        slots.native_write(
            FieldWrite::Cell(FieldCell::Graph {
                text: 'A',
                width: 1,
            }),
            RoffFont::Regular,
            Some(owner_a),
        );
        slots.native_write(
            FieldWrite::Cell(FieldCell::BreakMarker),
            RoffFont::Regular,
            None,
        );
        let _ = slots.motion(Some("3n"), None);
        assert_eq!(slots.unowned_tail().expect("tail").fill_cells, 2);

        let placement = Placement {
            slots,
            reading,
            marker_boundary: super::super::MarkerBoundary::Clear,
            links: Vec::new(),
            active_link: None,
            marker_after_blank: false,
            reposition: None,
            retreated: false,
            pre_column_retreat_units: 0,
        };
        let seed = GeometrySeed {
            placement,
            replace_tail: 0,
            replayed_entry: false,
            prefix_start: 0,
            earlier: 0,
            prior_pending: None,
            advance_marker: None,
            editable_pass: false,
        };
        let placed = PlacedWord::prepare(
            seed,
            true,
            AdvanceReading::Unprojected,
            true,
            ReplayDomain::Filled,
            None,
        )
        .expect("prepared");
        let tail = placed
            .writes
            .iter()
            .find_map(|write| match write {
                FieldWrite::RetainDeviceTail(tail) => Some(tail),
                _ => None,
            })
            .expect("retained device tail");
        assert_eq!(tail.fill_cells, 4);
    }
}
