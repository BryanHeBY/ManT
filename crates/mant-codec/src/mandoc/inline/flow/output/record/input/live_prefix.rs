//! Apply the validated slot overlay to the previous word owners.

use super::super::super::super::field_buffer::{FieldCell, RewoundGlyph};
use super::{FieldBuffer, FieldWrite, Inline, NativeWordAnchor, earlier_word_after_cell};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct PrefixEdits {
    first_incoming: Option<usize>,
    pub(super) word_start: Option<usize>,
    pub(super) entry_blanks: usize,
    moved: Vec<(usize, usize)>,
    edits: Vec<(usize, RewoundGlyph, Option<usize>)>,
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
            first_incoming,
            entry_blanks,
            unpublished,
        }) = writes.first()
        else {
            return Self::default();
        };
        let locations = projection_locations(buffer, earlier, *start, positions, *unpublished);
        let mut edits = Vec::new();
        let mut moved = Vec::new();
        for (offset, (position, location)) in positions.iter().zip(locations).enumerate() {
            let cell = start + offset;
            let relocated = position.filter(|pos| pos >= first_incoming).map(|_| offset);
            if position.is_some() && relocated.is_none() {
                continue;
            }
            let Some((owner, following_scalars)) = location else {
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
            let scalar = match buffer.cells()[cell] {
                FieldCell::Graph { text, .. } => text,
                FieldCell::Hyphen => '-',
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => ' ',
                FieldCell::Tab => '\t',
                _ => continue,
            };
            edits.push((
                owner,
                RewoundGlyph {
                    cell,
                    scalar,
                    following_scalars,
                },
                relocated,
            ));
        }
        moved.sort_unstable_by_key(|&(position, _)| position);
        let first_touched = earlier.partition_point(|owner| owner.content < *start);
        for owner in &mut earlier[first_touched..] {
            let remap = |index: usize| {
                let Some(offset) = index.checked_sub(*start) else {
                    return index;
                };
                positions
                    .get(offset..)
                    .and_then(|suffix| suffix.iter().find_map(|cell| *cell))
                    .unwrap_or(*first_incoming)
                    .min(*first_incoming)
            };
            owner.start = remap(owner.start);
            owner.content = remap(owner.content);
        }
        Self {
            first_incoming: Some(*first_incoming),
            // Replay can change the number of slots owned by earlier
            // words (e.g. encode1's duplicate bold glyph). Do not cut
            // their accepted ranges at the old incoming word start.
            word_start: Some(
                positions
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|&position| position < *first_incoming)
                    .max()
                    .map_or(*start, |position| position + 1),
            ),
            entry_blanks: *entry_blanks,
            moved,
            edits,
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
            }
            let mut resumed = incoming.clone();
            resumed.start = position + 1;
            resumed.content = position + 1;
            resumed.projected_field_prefix = false;
            resumed.projected_device_padding = 0;
            anchors.push(NativeWordAnchor {
                start: position,
                content: position,
                projected_field_prefix: false,
                projected_device_padding: 0,
                ..old
            });
            anchors.push(resumed);
        }
    }

    pub(super) fn first_incoming(&self) -> Option<usize> {
        self.first_incoming
    }

    pub(super) fn project(
        &self,
        nodes: &mut [Inline],
        incoming: &mut Vec<Inline>,
        incoming_owner: &str,
        earlier: &[NativeWordAnchor],
    ) {
        // Suffix-relative offsets of later drops survive earlier removals.
        let mut relocated = std::collections::BTreeMap::new();
        for (owner, glyph, slot) in &self.edits {
            let owner = &earlier[*owner].owner;
            let projection = super::super::super::source_blanks::take_rewound_glyph(
                nodes,
                incoming,
                owner,
                glyph,
                slot.is_some(),
            );
            if let (Some(slot), Some(projection)) = (slot, projection) {
                relocated.insert(*slot, (owner, projection));
            }
        }
        if self.edits.iter().any(|(_, _, slot)| slot.is_some()) {
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
                if let Some((owner, glyph)) = slot
                    .parse::<usize>()
                    .ok()
                    .and_then(|slot| relocated.remove(&slot))
                {
                    incoming.push(Inline::anchor(owner.as_str()));
                    incoming.push(glyph);
                    incoming.push(Inline::anchor(incoming_owner));
                }
            }
        }
    }
}

fn projection_locations(
    buffer: &FieldBuffer,
    earlier: &[NativeWordAnchor],
    start: usize,
    positions: &[Option<usize>],
    unpublished: Option<usize>,
) -> Vec<Option<(usize, usize)>> {
    // The native overlay is bounded. Walk it backwards in display order
    // per word marker, including every surviving fragment of that word.
    // A moved glyph keeps its original marker even when another word now
    // separates it from the first fragment. A local interval length alone
    // cannot identify it in the word's projected stream.
    let mut suffix = HashMap::new();
    let mut locations = vec![None; positions.len()];
    for (offset, position) in positions.iter().enumerate().rev() {
        let cell = start + offset;
        let count = buffer.projection_length(cell, cell + 1);
        if count == 0 {
            continue;
        }
        let Some(owner) = earlier_word_after_cell(earlier, cell).checked_sub(1) else {
            continue;
        };
        let remaining = suffix.entry(earlier[owner].owner.as_str()).or_insert(0);
        locations[offset] = Some((owner, *remaining));
        if unpublished != Some(cell) || position.is_some() {
            *remaining += count;
        }
    }
    locations
}
