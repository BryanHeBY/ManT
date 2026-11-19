//! Apply the validated slot overlay to the previous word owners.

use super::super::super::super::field_buffer::{FieldCell, RewoundGlyph};
use super::{FieldBuffer, FieldWrite, Inline, NativeWordAnchor, earlier_word_after_cell};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default)]
pub(super) struct PrefixEdits {
    first_incoming: Option<usize>,
    pub(super) word_start: Option<usize>,
    pub(super) entry_blanks: usize,
    moved: Vec<(usize, usize)>,
    edits: Vec<(usize, RewoundGlyph, Option<usize>)>,
    gaps: GapEdits,
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
        let mut earlier_ink = HashMap::<usize, usize>::new();
        for (offset, (position, location)) in positions.iter().zip(locations).enumerate() {
            let cell = start + offset;
            let relocated = position.filter(|pos| pos >= first_incoming).map(|_| offset);
            if let (Some(position), Some((owner, _))) = (position, location)
                && relocated.is_none()
            {
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
        let gaps = rebase_earlier_words(earlier, positions, *start, *first_incoming, &earlier_ink);
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
            gaps,
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
            resumed.projected_gap_cell = None;
            resumed.projected_device_padding = 0;
            anchors.push(NativeWordAnchor {
                start: position,
                content: position,
                projected_field_prefix: false,
                projected_gap_cell: None,
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
        if !self.gaps.remove.is_empty() {
            // Edit the owned gap after using the old scalar coordinates
            // to detach surviving glyphs from their source words.
            remove_overwritten_gaps(nodes, &self.gaps.remove, &mut false);
        }
        if self.first_incoming.is_some() {
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
                if let Some(marker) = self.gaps.relocate.get(&slot) {
                    incoming.push(Inline::anchor(marker.as_str()));
                    incoming.push(Inline::Text { value: " ".into() });
                    incoming.push(Inline::anchor(incoming_owner));
                } else if let Some((owner, glyph)) = relocated.remove(&slot) {
                    incoming.push(Inline::anchor(owner.as_str()));
                    incoming.push(glyph);
                    incoming.push(Inline::anchor(incoming_owner));
                }
            }
        }
    }
}

#[derive(Default)]
struct GapEdits {
    remove: BTreeSet<String>,
    relocate: BTreeMap<usize, String>,
}

fn rebase_earlier_words(
    earlier: &mut [NativeWordAnchor],
    positions: &[Option<usize>],
    start: usize,
    first_incoming: usize,
    earlier_ink: &HashMap<usize, usize>,
) -> GapEdits {
    let first_touched = earlier.partition_point(|owner| owner.content < start);
    let mut gaps = GapEdits::default();
    for (index, owner) in earlier.iter_mut().enumerate().skip(first_touched) {
        if let Some(old) = owner.projected_gap_cell
            && let Some(position) = old
                .checked_sub(start)
                .and_then(|offset| positions.get(offset))
        {
            owner.projected_gap_cell = *position;
            if position.is_none_or(|position| position >= first_incoming) {
                let serial = owner
                    .owner
                    .strip_prefix(super::super::super::INTERNAL_FIELD_WORD)
                    .expect("native word owner");
                let marker = format!("{}{serial}", super::super::super::INTERNAL_FIELD_PREFIX);
                gaps.remove.insert(marker.clone());
                if position.is_some() {
                    gaps.relocate.insert(old - start, marker);
                } else {
                    owner.projected_field_prefix = false;
                }
            }
        }
        let remap = |index: usize| {
            let Some(offset) = index.checked_sub(start) else {
                return index;
            };
            positions
                .get(offset..)
                .and_then(|suffix| suffix.iter().find_map(|cell| *cell))
                .unwrap_or(first_incoming)
                .min(first_incoming)
        };
        owner.start = remap(owner.start);
        owner.content = remap(owner.content);
        if let Some(&position) = earlier_ink.get(&index) {
            owner.start = owner.start.min(position);
            owner.content = owner.content.min(position);
        }
    }
    gaps
}

fn remove_overwritten_gaps(nodes: &mut [Inline], owners: &BTreeSet<String>, remove: &mut bool) {
    for node in nodes {
        match node {
            Inline::Anchor { id, .. } => *remove = owners.contains(id.as_str()),
            Inline::Text { value } if *remove => {
                if value.starts_with(' ') {
                    value.remove(0);
                }
                *remove = false;
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => remove_overwritten_gaps(children, owners, remove),
            _ => {}
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
