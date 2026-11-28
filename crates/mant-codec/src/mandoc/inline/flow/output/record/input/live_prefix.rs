//! Apply the validated slot overlay to the previous word owners.

use super::super::super::super::field_buffer::{FieldCell, RewoundGlyph};
use super::super::super::super::{ProjectedNativeGap, WordProjectionPolicy};
use super::{FieldBuffer, FieldWrite, Inline, NativeWordAnchor, earlier_word_after_cell};
use crate::mandoc::inline::flow::definition::head_presentation::DeferredHeadProjection;
use crate::mandoc::inline::flow::output::{INTERNAL_FIELD_WORD, NAVIGATION_WORD};
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod gaps;
use gaps::{GapEdits, GapLanding, GapOverlay, rebase_earlier_words};

#[derive(Default)]
pub(super) struct PrefixEdits {
    first_incoming: Option<usize>,
    pub(super) first_incoming_ink: Option<usize>,
    pub(super) word_start: Option<usize>,
    pub(super) entry_blanks: usize,
    pub(super) entry_gap: Option<usize>,
    pub(super) moved: Vec<(usize, usize)>,
    detached_moves: BTreeMap<usize, String>,
    edits: Vec<GlyphEdit>,
    gaps: GapEdits,
    retained_blanks: Vec<(usize, usize, RewoundGlyph)>,
    deferred_blanks: BTreeMap<usize, String>,
    transferred_gaps: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Relocation {
    marker: usize,
    cell: usize,
}

struct GlyphEdit {
    owner: usize,
    glyph: RewoundGlyph,
    relocation: Option<Relocation>,
    unprinted_tail: usize,
}

struct RelocatedGlyph {
    owner: String,
    glyph: Inline,
    starts_word: bool,
    may_relocate_navigation_target: bool,
    foreign_epoch: Option<u64>,
}

impl PrefixEdits {
    pub(super) fn prepare(
        buffer: &FieldBuffer,
        writes: &[FieldWrite],
        earlier: &mut [NativeWordAnchor],
    ) -> Self {
        let Some(FieldWrite::ReplaceLivePrefix {
            start,
            positions,
            retained_advances,
            adopted_advances,
            first_incoming,
            first_incoming_ink,
            entry_blanks,
            entry_gap,
            unpublished,
            ..
        }) = writes.first()
        else {
            return Self::default();
        };
        let marked_body_tail =
            marked_body_tail_overlay(buffer, writes, *start, *first_incoming_ink);
        let locations = projection_locations(buffer, earlier, *start, positions, *unpublished);
        let mut edits = Vec::new();
        let mut moved = Vec::new();
        let mut earlier_ink = HashMap::<usize, usize>::new();
        let mut retained_blanks = Vec::new();
        for (offset, (position, location)) in
            positions.iter().zip(locations.iter().copied()).enumerate()
        {
            let cell = start + offset;
            let relocated = position
                .filter(|pos| pos >= first_incoming)
                .map(|cell| Relocation {
                    marker: offset,
                    cell,
                });
            if let (Some(position), Some((owner, following_scalars, _))) = (position, location)
                && relocated.is_none()
            {
                if *unpublished != Some(cell)
                    && let Some(glyph) = retained_blank(buffer, cell, following_scalars)
                {
                    retained_blanks.push((*position, owner, glyph));
                }
                // A surviving decoration copy can be written before the
                // original owner interval. Retain its real landing.
                earlier_ink
                    .entry(owner)
                    .and_modify(|first| *first = (*first).min(*position))
                    .or_insert(*position);
            }
            if position.is_some() && relocated.is_none() {
                continue;
            }
            let Some((owner, following_scalars, unprinted_tail)) = location else {
                continue;
            };
            if let (Some(position), Some(_)) = (position, relocated) {
                moved.push((*position, owner));
            }
            if *unpublished == Some(cell) {
                // Its old owner is still held by the zero-advance register.
                // The word projector releases it at the accepted slot.
                continue;
            }
            let Some(scalar) = rewound_scalar(
                &buffer.cells()[cell],
                &earlier[owner],
                cell,
                marked_body_tail,
            ) else {
                continue;
            };
            edits.push(GlyphEdit {
                owner,
                glyph: RewoundGlyph {
                    cell,
                    scalar,
                    following_scalars,
                },
                relocation: relocated,
                unprinted_tail,
            });
        }
        moved.sort_unstable_by_key(|&(position, _)| position);
        let gaps = rebase_earlier_words(
            (buffer, earlier),
            GapOverlay(positions, &locations, retained_advances, adopted_advances),
            (*start, *first_incoming),
            &earlier_ink,
        );
        // An earlier surviving write can shift the interval's start.
        let word_start = word_start_after_overlay(positions, *start, *first_incoming);
        let deferred_blanks = buffer.relocated_blank_markers(*start, positions, *first_incoming);
        Self {
            first_incoming: Some(*first_incoming),
            first_incoming_ink: *first_incoming_ink,
            word_start: Some(word_start),
            entry_blanks: *entry_blanks,
            entry_gap: *entry_gap,
            moved,
            detached_moves: BTreeMap::new(),
            edits,
            gaps,
            retained_blanks,
            deferred_blanks,
            transferred_gaps: Vec::new(),
        }
    }

    pub(super) fn append_owner_intervals(&self, anchors: &mut Vec<NativeWordAnchor>) {
        let Some(incoming) = anchors.last().cloned() else {
            return;
        };
        for &(position, owner_index) in &self.moved {
            let old = anchors[owner_index].clone();
            if anchors.last().is_some_and(|last| last.start == position) {
                anchors.pop();
            } else if let Some(last) = anchors.last_mut()
                && last.content > position
            {
                last.content = position;
            }
            let mut resumed = incoming.clone();
            resumed.start = position + 1;
            resumed.content = position + 1;
            resumed.projected_field_prefix = false;
            resumed.projected_gap = ProjectedNativeGap::default();
            resumed.projected_device_padding = 0;
            anchors.push(NativeWordAnchor {
                owner: self
                    .detached_moves
                    .get(&position)
                    .cloned()
                    .unwrap_or(old.owner),
                start: position,
                content: position,
                projected_field_prefix: false,
                projected_gap: ProjectedNativeGap::default(),
                projected_device_padding: 0,
                ..old
            });
            anchors.push(resumed);
        }
    }

    pub(super) fn changed_owner_receipts_from(&self, initial: usize) -> Option<usize> {
        self.gaps.changed_owner_keys_from().or_else(|| {
            // Relocation can replace the just-pushed receipt or extend its run.
            (!self.moved.is_empty()).then_some(initial)
        })
    }

    pub(super) fn first_incoming(&self) -> Option<usize> {
        self.first_incoming
    }

    pub(super) fn retains_blank(&self, cell: usize) -> bool {
        self.gaps.retains_blank(cell)
            || self
                .retained_blanks
                .iter()
                .any(|&(position, _, _)| position == cell)
    }

    pub(super) fn clipped_ink_owners(&self) -> impl Iterator<Item = usize> + '_ {
        self.edits.iter().filter_map(|edit| {
            (edit.relocation.is_none() && !edit.glyph.scalar.is_whitespace()).then_some(edit.owner)
        })
    }

    pub(super) fn project(
        &mut self,
        nodes: &mut [Inline],
        incoming: &mut Vec<Inline>,
        incoming_owner: &str,
        earlier: &[NativeWordAnchor],
        (head, receiver): (Option<&DeferredHeadProjection>, Option<u64>),
    ) {
        // Suffix-relative offsets of later drops survive earlier removals.
        let mut relocated = std::collections::BTreeMap::new();
        let mut extracted = Vec::new();
        for edit in &self.edits {
            let GlyphEdit {
                owner: owner_index,
                glyph,
                relocation: slot,
                unprinted_tail,
            } = edit;
            let owner = &earlier[*owner_index].owner;
            let detached = head.filter(|head| head.owns_word(owner));
            let mut projection_owner = owner.clone();
            let edit = super::super::super::source_blanks::edit_rewound_glyph(
                nodes,
                incoming,
                owner,
                glyph,
                slot.is_some() || detached.is_some(),
            );
            let edited_live_owner = edit.is_some();
            let starts_word = edit.as_ref().is_some_and(|edit| edit.starts_word_ink)
                && !glyph.scalar.is_whitespace();
            if edited_live_owner {
                // Successful removals without retained style still shorten
                // this owner's scalar suffix for the next blank check.
                extracted.push((owner, glyph.following_scalars));
            }
            let mut projection = edit.and_then(|edit| edit.projection);
            if !edited_live_owner && let Some(head) = detached {
                projection = take_detached_ink(head, owner, glyph, *unprinted_tail);
                if let Some(slot) = slot {
                    if projection.is_some() {
                        // The original HEAD and the relocated BODY fragment
                        // have independent scalar cursors at final flush.
                        let cell = slot.cell;
                        projection_owner = format!("{incoming_owner}:relocated:{cell}");
                        self.detached_moves.insert(cell, projection_owner.clone());
                    }
                } else if projection.is_none() {
                    head.accept_rewind(owner, glyph);
                }
            }
            if let (Some(slot), Some(projection)) = (slot, projection) {
                relocated.insert(
                    slot.marker,
                    RelocatedGlyph {
                        owner: projection_owner,
                        glyph: projection,
                        starts_word,
                        foreign_epoch: receiver
                            .filter(|&epoch| earlier[*owner_index].writer_epoch < epoch)
                            .map(|_| earlier[*owner_index].writer_epoch),
                        may_relocate_navigation_target: earlier[*owner_index]
                            .may_relocate_navigation_target,
                    },
                );
            }
        }
        // Edit the owned gap after using the old scalar coordinates
        // to detach surviving glyphs from their source words.
        let removed_gaps = self.gaps.take_overwritten(nodes);
        // A slot can survive in source order before an already painted
        // graph. Only a trailing reading blank can satisfy the new seam.
        self.retained_blanks.retain(|(_, owner, glyph)| {
            // Earlier scalar addresses were measured before we relocated
            // later parts of the same owner. Test the current seam using
            // that owner's remaining suffix, not its old text length.
            let mut glyph = glyph.clone();
            glyph.following_scalars -= extracted
                .iter()
                .filter(|&&(removed, following)| {
                    removed == &earlier[*owner].owner && following < glyph.following_scalars
                })
                .count()
                .min(glyph.following_scalars);
            super::super::super::source_blanks::has_trailing_rewound_blank(
                nodes,
                &earlier[*owner].owner,
                &glyph,
            )
        });
        if self.first_incoming.is_some() {
            Self::relocate_payload(
                (self.gaps.relocations(), &mut self.deferred_blanks),
                &mut self.transferred_gaps,
                nodes,
                incoming,
                incoming_owner,
                removed_gaps,
                relocated,
            );
        }
    }

    pub(super) fn transfer_positioning_slots(&self, buffer: &mut FieldBuffer) {
        for &cell in &self.transferred_gaps {
            buffer.transfer_positioning_slot(cell);
        }
    }

    fn relocate_payload(
        (gap_markers, deferred_blanks): (
            &BTreeMap<usize, GapLanding>,
            &mut BTreeMap<usize, String>,
        ),
        transferred_gaps: &mut Vec<usize>,
        nodes: &mut [Inline],
        incoming: &mut Vec<Inline>,
        incoming_owner: &str,
        mut removed_gaps: BTreeMap<&str, String>,
        mut relocated: BTreeMap<usize, RelocatedGlyph>,
    ) {
        let payload = std::mem::take(incoming);
        for node in payload {
            let Inline::Anchor { id, .. } = &node else {
                incoming.push(node);
                continue;
            };
            let Some(slot) = id
                .as_str()
                .strip_prefix(crate::mandoc::inline::flow::RELOCATED_GLYPH)
            else {
                incoming.push(node);
                continue;
            };
            let Ok(slot) = slot.parse::<usize>() else {
                continue;
            };
            if let Some(gap) = gap_markers.get(&slot)
                && let Some(value) = removed_gaps.remove(gap.marker.as_str())
            {
                // This positioning text supersedes a source blank
                // extracted from the same native landing. Only that
                // displaced owner scalar changes role.
                let displaced = relocated.remove(&slot).is_some();
                if gap.positioning_only || displaced {
                    transferred_gaps.push(gap.cell);
                }
                incoming.push(Inline::anchor(gap.marker.as_str()));
                incoming.push(Inline::Text { value });
                incoming.push(Inline::anchor(incoming_owner));
            } else if let Some(marker) = deferred_blanks.remove(&slot)
                && let Some(blank) = take_deferred_blank_marker(nodes, &marker)
            {
                // The source marker remains zero-width until
                // term_field's accepted pass actually prints it.
                incoming.push(blank);
            } else if let Some(RelocatedGlyph {
                owner,
                glyph,
                starts_word,
                may_relocate_navigation_target,
                foreign_epoch,
            }) = relocated.remove(&slot)
            {
                // HORIZ can place another macro's ink before this word
                // (term.c:678–718). Move its validated target with the
                // first ink past its leading blanks, along with its styling.
                #[cfg(test)]
                if starts_word {
                    crate::mandoc::inline::ownership::metrics::note(|counts| {
                        counts.navigation_relocated_word_starts += 1;
                    });
                }
                #[cfg(test)]
                let may_relocate_navigation_target = may_relocate_navigation_target
                    || crate::mandoc::inline::ownership::metrics::eager_navigation_probes();
                if starts_word
                    && may_relocate_navigation_target
                    && let Some(target) = take_word_target(nodes, &owner)
                {
                    incoming.extend(target);
                }
                let source = Inline::anchor(owner);
                if let Some(epoch) = foreign_epoch {
                    // Slot relocation reorders presentation, not semantic
                    // authorship. Split the newer scope on the old payload.
                    incoming.push(crate::mandoc::inline::ownership::native_relocated_origin(
                        epoch,
                        vec![source, glyph],
                    ));
                } else {
                    incoming.extend([source, glyph]);
                }
                incoming.push(Inline::anchor(incoming_owner));
            }
        }
    }
}

fn take_word_target(nodes: &mut [Inline], owner: &str) -> Option<[Inline; 2]> {
    let serial = owner.strip_prefix(INTERNAL_FIELD_WORD)?;
    let marker = format!("{NAVIGATION_WORD}{serial}");
    take_target(nodes, &marker)
}

fn take_target(nodes: &mut [Inline], marker: &str) -> Option<[Inline; 2]> {
    let mut remaining = nodes;
    while let Some((node, preceding)) = remaining.split_last_mut() {
        remaining = preceding;
        #[cfg(test)]
        crate::mandoc::inline::ownership::metrics::note(|counts| {
            counts.navigation_target_probe_visits += 1;
        });
        match node {
            Inline::Anchor { id, .. }
                if !id.as_str().starts_with('\0')
                    && matches!(remaining.last(), Some(Inline::Anchor { id, .. }) if id.as_str() == marker || id.as_str().strip_prefix(marker).is_some_and(|tail| tail.starts_with(':'))) =>
            {
                let retired = || Inline::anchor(NAVIGATION_WORD);
                return Some([
                    std::mem::replace(remaining.last_mut()?, retired()),
                    std::mem::replace(node, retired()),
                ]);
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let Some(target) = take_target(children, marker) {
                    return Some(target);
                }
            }
            _ => {}
        }
    }
    None
}

/// A positioned BODY word can overwrite the HEAD's final backline and fixed
/// glyph with LF and first ink, leaving the generated slot between two BODY
/// operands (term.c:659, 679–718). An unmarked write or one landing at the
/// previous marker doesn't prove the same handoff.
fn marked_body_tail_overlay(
    buffer: &FieldBuffer,
    writes: &[FieldWrite],
    start: usize,
    first_incoming_ink: Option<usize>,
) -> Option<usize> {
    let ink = first_incoming_ink?;
    let marker = ink.checked_sub(1)?;
    let offset = marker.checked_sub(start)? + 1; // Skip ReplaceLivePrefix.
    let FieldWrite::ReplaceLivePrefix { positions, .. } = writes.first()? else {
        return None;
    };
    let blank = ink + 1;
    if positions.get(marker - start..=blank - start)? != [None, None, Some(blank)]
        || !matches!(
            buffer.cells().get(marker..blank)?,
            [
                FieldCell::Backline,
                FieldCell::Graph {
                    text: '\u{a0}',
                    width: 1
                }
            ]
        )
        || !matches!(
            writes.get(offset..offset + 3)?,
            [
                FieldWrite::Placed { cell: FieldCell::BreakMarker, scalars: 0 },
                FieldWrite::Placed { cell: FieldCell::Graph { text, width: 1 }, scalars: 1 },
                FieldWrite::Placed { cell: FieldCell::Graph { text: '\u{a0}', width: 1 }, scalars: 1 },
            ] if !text.is_whitespace()
        )
    {
        return None;
    }
    Some(blank)
}

fn retained_blank(
    buffer: &FieldBuffer,
    cell: usize,
    following_scalars: usize,
) -> Option<RewoundGlyph> {
    matches!(
        buffer.cells()[cell],
        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
    )
    .then_some(RewoundGlyph {
        cell,
        scalar: ' ',
        following_scalars,
    })
}

fn rewound_scalar(
    native: &FieldCell,
    anchor: &NativeWordAnchor,
    cell: usize,
    marked_body_tail: Option<usize>,
) -> Option<char> {
    match native {
        // Only this fallback stores native U+A0 while its *exact* owner
        // published SP. All authored fixed spaces still match their own scalar.
        FieldCell::Graph { text: '\u{a0}', .. }
            if anchor.projection == WordProjectionPolicy::GeneratedRunInBlank
                && anchor.content == cell
                && marked_body_tail == Some(cell) =>
        {
            Some(' ')
        }
        FieldCell::Graph { text, .. } => Some(*text),
        FieldCell::Hyphen => Some('-'),
        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => Some(' '),
        FieldCell::Tab => Some('\t'),
        _ => None,
    }
}

fn take_deferred_blank_marker(nodes: &mut [Inline], marker: &str) -> Option<Inline> {
    for node in nodes.iter_mut().rev() {
        match node {
            Inline::Anchor { id, .. } if id.as_str() == marker => {
                return Some(std::mem::replace(
                    node,
                    Inline::anchor("\0mant:output-scope:retired-blank"),
                ));
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                if let Some(blank) = take_deferred_blank_marker(children, marker) {
                    return Some(Inline::Link {
                        target: target.clone(),
                        title: title.clone(),
                        children: vec![blank],
                    });
                }
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                // Font belongs to DeferredSourceBlank's receipt.
                if let Some(blank) = take_deferred_blank_marker(children, marker) {
                    return Some(blank);
                }
            }
            _ => {}
        }
    }
    None
}

fn word_start_after_overlay(positions: &[Option<usize>], start: usize, first: usize) -> usize {
    // encode1() can add or remove bold copies. Keep earlier surviving
    // slots out of the incoming word's interval, independent of its old start.
    positions
        .iter()
        .flatten()
        .copied()
        .filter(|&position| position < first)
        .max()
        .map_or(start, |position| position + 1)
}

type SourceProjection = Option<(usize, usize, usize)>;

fn take_detached_ink(
    head: &DeferredHeadProjection,
    owner: &str,
    glyph: &RewoundGlyph,
    unprinted_tail: usize,
) -> Option<Inline> {
    head.relocate_ink(owner, glyph).or_else(|| {
        // HEAD can finish its reading payload before BODY executes.
        // term_field() leaves the raw trailing SP/TAB in the field
        // without publishing those scalars to the detached terms.
        if unprinted_tail == 0 || unprinted_tail > glyph.following_scalars {
            return None;
        }
        let mut shortened = glyph.clone();
        shortened.following_scalars -= unprinted_tail;
        head.relocate_ink(owner, &shortened)
    })
}

fn projection_locations(
    buffer: &FieldBuffer,
    earlier: &[NativeWordAnchor],
    start: usize,
    positions: &[Option<usize>],
    unpublished: Option<usize>,
) -> Vec<SourceProjection> {
    // The native overlay is bounded. Walk it backwards in display order
    // per word marker, including every surviving fragment of that word.
    // A moved glyph keeps its original marker even when another word now
    // separates it from the first fragment. A local interval length alone
    // cannot identify it in the word's projected stream.
    let mut suffix = HashMap::new();
    let mut locations = vec![None; positions.len()];
    let printed_end = buffer.printed_content_end(start, buffer.cells().len());
    let gaps: BTreeSet<_> = earlier[earlier.partition_point(|owner| owner.content < start)..]
        .iter()
        .filter_map(NativeWordAnchor::entry_gap_cell)
        .collect();
    for (offset, position) in positions.iter().enumerate().rev() {
        let cell = start + offset;
        let count = buffer.projection_length(cell, cell + 1);
        // Generated padding has a separate marker and is edited by GapEdits.
        // It cannot take a scalar position in the preceding source owner.
        if count == 0 || gaps.contains(&cell) {
            continue;
        }
        let Some(owner) = earlier_word_after_cell(earlier, cell).checked_sub(1) else {
            continue;
        };
        let remaining = suffix
            .entry(earlier[owner].owner.as_str())
            .or_insert((0, 0));
        locations[offset] = Some((owner, remaining.0, remaining.1));
        if unpublished != Some(cell) || position.is_some() {
            remaining.0 += count;
            if cell >= printed_end {
                remaining.1 += count;
            }
        }
    }
    locations
}
