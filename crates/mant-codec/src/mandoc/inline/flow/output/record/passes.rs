//! Project authored pass boundaries and borrowed word fragments in native order.

use super::super::native_passes::{RecordedRowSplit, split_recorded_native_passes};
use super::{Inline, InlineBuilder, RestartedPadding, WordPassProjection};
use std::collections::{BTreeMap, BTreeSet};

impl RestartedPadding {
    fn before_word(
        buffer: &super::super::super::field_buffer::FieldBuffer,
        boundary: usize,
        word_start: usize,
        word_first_cell: usize,
        borrowed: Option<&BorrowedPrefix>,
    ) -> Self {
        let current = buffer.projectable_padding(boundary.max(word_start), word_first_cell);
        if let Some(prefix) = borrowed
            && prefix.offset(buffer, boundary) == 0
        {
            Self::WithEarlierOwners {
                total: buffer
                    .projectable_padding(boundary, word_first_cell)
                    .saturating_sub(prefix.count),
                current,
            }
        } else {
            Self::CurrentWord(current)
        }
    }
}

impl InlineBuilder {
    pub(super) fn record_authored_native_passes(
        &mut self,
        incoming: &[Inline],
        word_first_cell: usize,
    ) -> WordPassProjection {
        // Width-dependent passes stay provisional until term_flushln(): a
        // later .ta may reinterpret every buffered tab. Only authored break
        // markers can settle a word's existing hard-break execution here.
        let Some(targets) = self.native_field_targets(false, None) else {
            return WordPassProjection::default();
        };
        let Some(definition) = self.execution.definition.as_mut() else {
            return WordPassProjection::default();
        };
        let mut buffer = std::mem::take(&mut definition.field_buffer);
        let mut anchors = std::mem::take(&mut definition.field_word_anchors);
        let cached_glyph = self
            .execution
            .zero_advance
            .buffered_glyph_belongs_to_current_word();
        let result = Self::native_unit_passes(
            incoming,
            &mut buffer,
            &mut anchors,
            targets,
            word_first_cell,
            cached_glyph,
        );
        let Some(definition) = self.execution.definition.as_mut() else {
            return result;
        };
        definition.field_buffer = buffer;
        definition.field_word_anchors = anchors;
        result
    }

    /// The shared incremental pass loop (definition fields and plain flush
    /// units alike): commit only authored-marker passes, and report the row
    /// boundary positions relative to the word just recorded.
    pub(super) fn native_unit_passes(
        incoming: &[Inline],
        buffer: &mut super::super::super::field_buffer::FieldBuffer,
        anchors: &mut [super::super::super::NativeWordAnchor],
        targets: super::super::super::field_buffer::FillTargets,
        word_first_cell: usize,
        cached_glyph: bool,
    ) -> WordPassProjection {
        let anchor_count = anchors.len();
        let mut inside_splits: BTreeMap<String, Vec<RecordedRowSplit>> = BTreeMap::new();
        let mut closes_before = Vec::new();
        let mut restart = None;
        let tracks_authored_passes =
            anchor_count > 0 && !buffer.word_scan_deferred() && buffer.has_pending_break_markers();
        let starts_earlier_owner =
            tracks_authored_passes && starts_with_borrowed_output(incoming, anchors.last());
        // Borrowed output need not precede this pass: a projected glyph's
        // held cells can follow its breakpoint. Recover their exact native
        // interval only when such a prefix exists, searching recent owners
        // backwards rather than scanning the growing anchor history per word.
        let borrowed = starts_earlier_owner
            .then(|| BorrowedPrefix::capture(incoming, buffer, anchors, word_first_cell))
            .flatten();
        if tracks_authored_passes {
            let word_end = buffer.cells().len();
            let mut first_pass = !buffer.has_committed_pass();
            while let Some(pass) = buffer.fill_pass_units(targets.scan(first_pass)) {
                if !buffer.has_non_ignorable_after(pass.end, false) {
                    break;
                }
                let pass_start = buffer.resume_offset();
                let boundary = buffer.after_break_blanks(pass.end);
                // An unbounded target can never stop at a device-width
                // guess (term.c:134,143-144): every accepted pass there is
                // marker-driven, even when the marker sits exactly at the
                // resumed boundary (term.c:294-295 restarts at it).
                let authored = targets.unbounded
                    || buffer.cells()[pass_start..boundary].iter().any(|cell| {
                        matches!(
                            cell,
                            super::super::super::field_buffer::FieldCell::BreakMarker
                        )
                    });
                if !authored {
                    // This pass only guessed a device width break. Do not
                    // commit its cursor or IR; the real flush owns it.
                    break;
                }
                let represented_in_ir = buffer.has_projected_pass(pass.end);
                buffer.commit_pass(pass, targets.actual(first_pass));
                let scalar_offset =
                    restarted_word_offset(buffer, anchors, boundary, word_first_cell);
                if boundary <= word_first_cell || (scalar_offset == 0 && !starts_earlier_owner) {
                    if !represented_in_ir {
                        if let Some((owner, offset)) = restart_inside_borrowed_fragment(
                            buffer,
                            boundary,
                            starts_earlier_owner,
                            borrowed.as_ref(),
                            &anchors[anchor_count - 1].owner,
                        ) {
                            record_row_split(&mut inside_splits, owner, pass.end, offset, false);
                        } else {
                            closes_before.push(pass_start..pass.end);
                        }
                    }
                    restart = Some(boundary);
                } else if boundary < word_end && !represented_in_ir {
                    record_row_split(
                        &mut inside_splits,
                        &anchors[anchor_count - 1].owner,
                        pass.end,
                        scalar_offset,
                        cached_glyph && buffer.projection_length(boundary, word_end) > 0,
                    );
                }
                if boundary >= word_end {
                    break;
                }
                first_pass = false;
            }
        }
        // A NODE_LINE TABREF can stop term_flushln()'s blank sweep.
        // Use the completed passes here: another pass in the same word
        // may accept only invisible cells and never print that padding.
        let leading_padding = restart.map_or_else(RestartedPadding::default, |boundary| {
            let padding = RestartedPadding::before_word(
                buffer,
                boundary,
                anchors[anchor_count - 1].start,
                word_first_cell,
                borrowed.as_ref(),
            );
            restore_restarted_separator(
                anchors,
                boundary,
                word_first_cell,
                !closes_before.is_empty() && padding.current_word_cells() > 0,
            );
            padding
        });
        let split_word = split_recorded_word(incoming, anchors.last(), inside_splits, buffer);
        WordPassProjection {
            closes_before,
            leading_padding,
            split_word,
            ..WordPassProjection::default()
        }
    }
}

fn starts_with_borrowed_output(
    incoming: &[Inline],
    current: Option<&super::super::super::NativeWordAnchor>,
) -> bool {
    current.is_some_and(|anchor| {
        matches!(incoming.first(), Some(Inline::Anchor { id, .. })
            if id.as_str().starts_with(super::super::INTERNAL_FIELD_WORD) && id.as_str() != anchor.owner)
    })
}

fn restart_inside_borrowed_fragment<'a>(
    buffer: &super::super::super::field_buffer::FieldBuffer,
    boundary: usize,
    starts_earlier_owner: bool,
    borrowed: Option<&'a BorrowedPrefix>,
    current_owner: &'a str,
) -> Option<(&'a str, usize)> {
    if !starts_earlier_owner {
        return None;
    }
    let Some(prefix) = borrowed else {
        return Some((current_owner, 0));
    };
    // A cached glyph and a held source blank can belong to different
    // operands. Place this pass at the next actual owner marker when only
    // native controls separate it from the restart cell (term.c:205-220).
    if let Some(owner) = prefix.owner_at_restart(buffer, boundary) {
        return Some((owner, 0));
    }
    let offset = prefix.offset(buffer, boundary);
    // With zero earlier scalars, close before the whole fragment so it
    // can transfer padding already projected by a graphless word once.
    (offset > 0).then_some((&prefix.owner, offset))
}

struct BorrowedPrefix {
    owner: String,
    content: usize,
    emitted: usize,
    count: usize,
    following: Vec<(usize, String)>,
}

impl BorrowedPrefix {
    fn capture(
        incoming: &[Inline],
        buffer: &super::super::super::field_buffer::FieldBuffer,
        anchors: &[super::super::super::NativeWordAnchor],
        word_first_cell: usize,
    ) -> Option<Self> {
        let Inline::Anchor { id, .. } = incoming.first()? else {
            return None;
        };
        let mut markers: BTreeSet<_> = incoming
            .iter()
            .skip(1)
            .filter_map(super::super::native_passes::native_word_marker)
            .collect();
        let mut following = Vec::new();
        // Pending output can span control-only source words. Search its
        // recent suffix once, retaining only owners whose markers appear
        // in this borrowed fragment. Each accepted pass then uses a lookup.
        let (index, first) = anchors.iter().enumerate().rev().find(|(_, anchor)| {
            if anchor.owner == id.as_str() {
                return true;
            }
            if markers.remove(anchor.owner.as_str()) {
                following.push((anchor.content, anchor.owner.clone()));
            }
            false
        })?;
        following.reverse();
        following.sort_by_key(|&(start, _)| start);
        let content = first.content;
        let end = anchors[index + 1].start.min(word_first_cell);
        let count = borrowed_prefix_scalars(incoming, id.as_str());
        let emitted = buffer.projection_length(content, end).saturating_sub(count);
        Some(Self {
            owner: id.as_str().to_owned(),
            content,
            emitted,
            count,
            following,
        })
    }

    fn offset(
        &self,
        buffer: &super::super::super::field_buffer::FieldBuffer,
        boundary: usize,
    ) -> usize {
        buffer
            .projection_length(self.content, boundary)
            .saturating_sub(self.emitted)
            .min(self.count)
    }

    fn owner_at_restart(
        &self,
        buffer: &super::super::super::field_buffer::FieldBuffer,
        boundary: usize,
    ) -> Option<&str> {
        let next = self
            .following
            .partition_point(|&(start, _)| start < boundary);
        let (start, owner) = self.following.get(next)?;
        (buffer.projection_length(boundary, *start) == 0).then_some(owner.as_str())
    }
}

fn split_recorded_word(
    incoming: &[Inline],
    anchor: Option<&super::super::super::NativeWordAnchor>,
    boundaries: BTreeMap<String, Vec<RecordedRowSplit>>,
    buffer: &mut super::super::super::field_buffer::FieldBuffer,
) -> Option<Vec<Inline>> {
    if boundaries.is_empty() {
        return None;
    }
    let owner = &anchor?.owner;
    let mut nodes = incoming.to_vec();
    let starts_owned = nodes.first().is_some_and(|node| {
        matches!(node,
        Inline::Anchor { id, .. } if id.as_str().starts_with(super::super::INTERNAL_FIELD_WORD))
    });
    if !starts_owned {
        nodes.insert(0, Inline::anchor(owner.clone()));
    }
    Some(split_recorded_native_passes(&nodes, boundaries, buffer))
}

fn record_row_split(
    splits: &mut BTreeMap<String, Vec<RecordedRowSplit>>,
    owner: &str,
    pass_end: usize,
    offset: usize,
    before_cached_glyph: bool,
) {
    splits
        .entry(owner.to_owned())
        .or_default()
        .push(RecordedRowSplit {
            offset,
            pass_end,
            before_cached_glyph,
        });
}

fn borrowed_prefix_scalars(nodes: &[Inline], owner: &str) -> usize {
    fn count(nodes: &[Inline], owner: &str, scalars: &mut usize) -> bool {
        for node in nodes {
            match node {
                Inline::Anchor { id, .. }
                    if id.as_str().starts_with(super::super::INTERNAL_FIELD_WORD) =>
                {
                    if id.as_str() != owner {
                        return false;
                    }
                }
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    *scalars += value.chars().count();
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => {
                    if !count(children, owner, scalars) {
                        return false;
                    }
                }
                Inline::Anchor { .. } | Inline::LineBreak { .. } => {}
            }
        }
        true
    }
    let mut scalars = 0;
    count(nodes, owner, &mut scalars);
    scalars
}

// Only the blanks skipped by term_flushln() are outside the restarted
// range. The word owns any remaining separator projected ahead of its text.
fn restarted_word_offset(
    buffer: &super::super::super::field_buffer::FieldBuffer,
    anchors: &mut [super::super::super::NativeWordAnchor],
    boundary: usize,
    word_first_cell: usize,
) -> usize {
    let offset = buffer.projection_length(word_first_cell, boundary);
    if offset == 0 {
        // term_flushln() consumes the automatic blank before a word's first
        // projected scalar (term.c:205-207). NBRZW and markers can place the
        // boundary inside that word without accepting any of its glyphs.
        let anchor = anchors.last_mut().expect("current native word");
        anchor.content = anchor.content.max(word_first_cell);
    }
    offset
}

fn restore_restarted_separator(
    anchors: &mut [super::super::super::NativeWordAnchor],
    boundary: usize,
    word_first_cell: usize,
    projected: bool,
) {
    if projected {
        let anchor = anchors.last_mut().expect("current native word");
        anchor.content = boundary.max(anchor.start).min(word_first_cell);
    }
}
