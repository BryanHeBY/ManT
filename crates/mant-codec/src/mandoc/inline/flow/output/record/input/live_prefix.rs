//! Apply the validated slot overlay to the previous word owners.

use super::super::super::super::field_buffer::{FieldCell, RewoundGlyph};
use super::{FieldBuffer, FieldWrite, Inline, NativeWordAnchor, earlier_word_after_cell};

#[derive(Default)]
pub(super) struct PrefixEdits {
    first_incoming: Option<usize>,
    pub(super) word_start: Option<usize>,
    pub(super) entry_blanks: usize,
    removed: Vec<(usize, RewoundGlyph)>,
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
        let mut removed = Vec::new();
        for (offset, position) in positions.iter().enumerate() {
            let cell = start + offset;
            if position.is_some()
                || *unpublished == Some(cell)
                || buffer.projection_length(cell, cell + 1) == 0
            {
                continue;
            }
            let following_owner = earlier_word_after_cell(earlier, cell);
            let Some(owner) = following_owner.checked_sub(1) else {
                continue;
            };
            let end = earlier
                .get(following_owner)
                .map_or(start + positions.len(), |anchor| {
                    anchor.start.min(anchor.content)
                });
            let scalar = match buffer.cells()[cell] {
                FieldCell::Graph { text, .. } => text,
                FieldCell::Hyphen => '-',
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => ' ',
                FieldCell::Tab => '\t',
                _ => continue,
            };
            removed.push((
                owner,
                RewoundGlyph {
                    cell,
                    scalar,
                    following_scalars: buffer.projection_length(cell + 1, end).saturating_sub(
                        usize::from(unpublished.is_some_and(|old| {
                            old > cell && old < end && positions[old - start].is_none()
                        })),
                    ),
                },
            ));
        }
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
            removed,
        }
    }

    pub(super) fn first_incoming(&self) -> Option<usize> {
        self.first_incoming
    }

    pub(super) fn project(
        &self,
        nodes: &mut [Inline],
        incoming: &mut [Inline],
        earlier: &[NativeWordAnchor],
    ) {
        // Suffix-relative offsets of later drops survive earlier removals.
        for (owner, glyph) in &self.removed {
            super::remove_rewound_glyph(nodes, incoming, &earlier[*owner].owner, glyph);
        }
    }
}
