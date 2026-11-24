//! Schedule accepted native field events for reading projection.

use std::collections::BTreeMap;

use super::super::Inline;
use super::split::PassBoundary;

mod acceptance;
mod rows;

pub(super) use acceptance::OrderedGlyph;
pub(in crate::mandoc::inline::flow) use acceptance::{OwnerAcceptance, accepted_owner_lengths};
pub(in crate::mandoc::inline::flow) use rows::{
    native_word_marker, split_retired_native_field_passes,
};
pub(super) use rows::{starts_with_native_break, trim_native_breakable_tail};

#[derive(Clone, Copy)]
pub(in crate::mandoc::inline::flow) enum NativePassHandoff<'a> {
    Section(&'a super::SectionHandoff),
    Definition(&'a super::super::definition::head_presentation::DeferredHeadProjection),
}

/// Consume accepted `term_flushln()` passes at their real retirement point.
/// Word-time marker passes may already have produced a boundary; a deferred
/// scan supplies the remaining ones here. Definition and plain owners share
/// this projection, including an accepted prefix followed by rejection.
pub(in crate::mandoc::inline::flow) fn project_accepted_native_passes(
    nodes: &mut Vec<Inline>,
    buffer: &mut super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    output_start: usize,
    handoff: Option<NativePassHandoff<'_>>,
) {
    use super::super::field_buffer::FillBoundary;

    let count = passes.len().saturating_sub(1);
    let mut boundaries: BTreeMap<String, Vec<RecordedRowSplit>> = BTreeMap::new();
    let mut offsets = OwnerOffsets {
        anchors,
        preceding: 0,
        lengths: BTreeMap::new(),
        first_owners: std::cell::OnceCell::new(),
    };
    let has_drawing_advance = buffer.has_deferred_drawing_advances();
    let unowned_prefix = handoff.is_none() && has_unowned_prefix_rows(buffer, anchors, passes);
    for (pass_index, (pass, next)) in passes[..count]
        .iter()
        .zip(passes.iter().skip(1))
        .enumerate()
    {
        if buffer.has_projected_pass(pass.end) || pass.boundary == FillBoundary::Hyphen {
            // An encoded ASCII_HYPH candidate locates the final device row
            // for BODY. The responsive reading projection keeps that source
            // word complete rather than hardening its discretionary split
            // (term.c:307-324); authored \p events remain independent.
            continue;
        }
        // The final flush can reach a pass that was still editable when
        // words were recorded. Its skipped SP still has provisional
        // reading positions. Defer them until owner acceptance unless
        // `record_accepted_pass_boundary` lets `trim_native_pass_rows`
        // consume an intra-owner leading blank in place (term.c:205-220).
        let pending_gap = pass.boundary == FillBoundary::WordEndBreak
            && pass.end < next.start
            && buffer.projection_length(pass.end, next.start) != 0;
        let cell = next.start;
        let owner = projected_restart_owner(buffer, anchors, pass, next, unowned_prefix);
        if let Some(index) = owner.checked_sub(1) {
            record_accepted_pass_boundary(
                (buffer, anchors),
                (pass, next, pass_index + 2 < passes.len()),
                (cell, index, owner),
                (has_drawing_advance, pending_gap),
                handoff,
                (&mut offsets, &mut boundaries),
            );
        } else if pending_gap {
            buffer.defer_projected_gap(pass.end, next.start);
        }
    }
    if !boundaries.is_empty() {
        // Earlier flushed fields are immutable output. Only the active
        // suffix is traversed, once per real flush (term.c:233-237).
        let pending = nodes.split_off(output_start.min(nodes.len()));
        nodes.extend(project_recorded_passes(&pending, boundaries, buffer, true));
    }
}

/// Map an accepted `term_flushln()` pass boundary (`term.c:143-163, 205-220`)
/// to its owner cut, handling detached definition heads (`mdoc_term.c:783-835`),
/// section seams, and skipped blanks (`term.c:389-428`) before calling
/// `acknowledge_projected_pass_cut`.
fn record_accepted_pass_boundary<'a>(
    (buffer, anchors): (
        &mut super::super::field_buffer::FieldBuffer,
        &'a [super::super::NativeWordAnchor],
    ),
    (pass, next, has_following_pass): (
        &super::super::field_buffer::FillPass,
        &super::super::field_buffer::FillPass,
        bool,
    ),
    (cell, mut index, owner): (usize, usize, usize),
    (has_drawing_advance, pending_gap): (bool, bool),
    handoff: Option<NativePassHandoff<'_>>,
    (offsets, boundaries): (
        &mut OwnerOffsets<'a>,
        &mut BTreeMap<String, Vec<RecordedRowSplit>>,
    ),
) {
    let definition = definition_handoff(handoff, buffer, anchors, pass, next, has_following_pass);
    if let Some(DefinitionBoundary::Body(index)) = definition {
        if pending_gap {
            buffer.defer_projected_gap(pass.end, next.start);
        }
        // The detached HEAD cannot carry its closing boundary in
        // the BODY output, so put it before the first BODY scalar.
        boundaries
            .entry(anchors[index].owner.clone())
            .or_default()
            .push(RecordedRowSplit::hard(0, pass.end));
        return;
    }
    let section_seam = matches!(
        handoff,
        Some(NativePassHandoff::Section(section))
            if section.closes_at_section_seam(
                buffer, pass.start..pass.end, anchors[index].content,
            )
    );
    if handoff.is_none()
        && project_detached_gap_restart(
            buffer,
            anchors,
            pass,
            (cell, owner),
            &offsets.first_owners,
            boundaries,
        )
    {
        if pending_gap {
            buffer.defer_projected_gap(pass.end, next.start);
        }
        return;
    }
    let mut cut = cell;
    let detached = if let Some(DefinitionBoundary::Head {
        receiver,
        last,
        anchor,
    }) = definition
        && anchor >= offsets.preceding
    {
        index = anchor;
        cut = last + 1;
        Some(receiver)
    } else {
        None
    };
    let mut used_earlier_cut = false;
    if detached.is_none()
        && !section_seam
        && let Some((actual, earlier_cut)) = offsets.earlier_cut(
            buffer,
            pass,
            (index, cell),
            pass_cut_bounds(buffer, pass, cell, has_drawing_advance),
            pending_gap,
            handoff,
        )
    {
        index = actual;
        cut = earlier_cut;
        used_earlier_cut = true;
    }
    let anchor = &anchors[index];
    let trim_leading_gap_inside_owner = pending_gap
        && handoff.is_none()
        && detached.is_none()
        && !section_seam
        && !used_earlier_cut
        && trims_leading_gap_inside_owner(buffer, anchors, index, pass, next, offsets);
    if pending_gap && !trim_leading_gap_inside_owner {
        buffer.defer_projected_gap(pass.end, next.start);
    }
    cut = exclude_skipped_gap(buffer, anchor.content, cut, pass.end..next.start);
    let offset = offsets.at(buffer, index, cut);
    if let Some(receiver) = detached {
        receiver.accept_hard_row(&anchor.owner, offset);
        // Mark the committed term_flushln() pass (term.c:163) as projected at `cut`.
        buffer.acknowledge_projected_pass_cut(pass.end, cut);
    } else {
        boundaries
            .entry(anchor.owner.clone())
            .or_default()
            .push(RecordedRowSplit {
                cut,
                section_seam,
                ..RecordedRowSplit::hard(offset, pass.end)
            });
    }
}

/// Address the author's glyph directly before a closed native paint stroke.
/// Its source scalar interval stays valid until owner acceptance runs.
pub(in crate::mandoc::inline::flow) fn closed_underline_positions(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    closes: impl Fn(usize, &super::super::field_buffer::FillPass) -> bool,
) -> Vec<(String, usize)> {
    let mut offsets = OwnerOffsets {
        anchors,
        preceding: 0,
        lengths: BTreeMap::new(),
        first_owners: std::cell::OnceCell::new(),
    };
    buffer
        .closed_underline_graphs(passes, closes)
        .into_iter()
        .filter_map(|graph| {
            let index = anchors
                .partition_point(|anchor| anchor.content <= graph)
                .checked_sub(1)?;
            Some((
                anchors[index].owner.clone(),
                offsets.at(buffer, index, graph + 1),
            ))
        })
        .collect()
}

struct OwnerOffsets<'a> {
    anchors: &'a [super::super::NativeWordAnchor],
    preceding: usize,
    lengths: BTreeMap<&'a str, usize>,
    first_owners: std::cell::OnceCell<std::collections::HashMap<&'a str, usize>>,
}

impl OwnerOffsets<'_> {
    fn earlier_cut(
        &self,
        buffer: &super::super::field_buffer::FieldBuffer,
        pass: &super::super::field_buffer::FillPass,
        (index, restart): (usize, usize),
        (span_end, accepted_end): (usize, usize),
        pending_gap: bool,
        handoff: Option<NativePassHandoff<'_>>,
    ) -> Option<(usize, usize)> {
        if !pending_gap
            && !projected_restart_needs_earlier_cut(
                buffer,
                self.anchors,
                index,
                pass,
                restart,
                span_end,
                &self.first_owners,
            )
        {
            return None;
        }
        // A replay can give this word an earlier fragment with the same
        // scalar position. Close after the last accepted ink (term.c:205-220).
        let last = buffer.last_projected_cell(pass.start..accepted_end)?;
        let before = self
            .anchors
            .partition_point(|anchor| anchor.content <= last);
        let actual = before.checked_sub(1)?;
        let owner = &self.anchors[actual].owner;
        (actual >= self.preceding
            && actual < index
            && (pending_gap || owner != &self.anchors[index].owner)
            // A detached label carries its own cuts. Keep any closing
            // boundary with the first BODY owner.
            && !matches!(handoff, Some(NativePassHandoff::Definition(head)) if head.owns_word(owner)))
        .then_some((actual, last + 1))
    }

    fn at(
        &mut self,
        buffer: &super::super::field_buffer::FieldBuffer,
        index: usize,
        cut: usize,
    ) -> usize {
        // Native replay can move part of one word after another owner's
        // glyph. Sum an owner's earlier fragments at each new cursor.
        for pair in self.anchors[self.preceding..=index].windows(2) {
            let (previous, next) = (&pair[0], &pair[1]);
            *self.lengths.entry(previous.owner.as_str()).or_default() += buffer.projection_length(
                previous.content,
                next.start.min(next.content).max(previous.content),
            );
        }
        self.preceding = index;
        let anchor = &self.anchors[index];
        self.lengths
            .get(anchor.owner.as_str())
            .copied()
            .unwrap_or(0)
            + buffer.projection_length(anchor.content, cut)
    }
}

fn pass_cut_bounds(
    buffer: &super::super::field_buffer::FieldBuffer,
    pass: &super::super::field_buffer::FillPass,
    restart: usize,
    has_drawing_advance: bool,
) -> (usize, usize) {
    if has_drawing_advance {
        let end = buffer.printed_content_end(pass.start, pass.end);
        (end, end)
    } else {
        (restart, pass.end)
    }
}

fn projected_restart_needs_earlier_cut<'a>(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &'a [super::super::NativeWordAnchor],
    index: usize,
    pass: &super::super::field_buffer::FillPass,
    restart: usize,
    span_end: usize,
    first_owners: &std::cell::OnceCell<std::collections::HashMap<&'a str, usize>>,
) -> bool {
    use super::super::field_buffer::FillBoundary;
    let owner = &anchors[index];
    owner.content < restart
        && buffer.projection_length(owner.content, span_end.max(owner.content)) == 0
        || pass.boundary == FillBoundary::WordEndBreak
            && owner.content >= pass.end
            && first_owner_positions(anchors, first_owners)[owner.owner.as_str()] < index
}

fn exclude_skipped_gap(
    buffer: &super::super::field_buffer::FieldBuffer,
    content: usize,
    cut: usize,
    gap: std::ops::Range<usize>,
) -> usize {
    // Keep skipped source positions until owner acceptance removes them.
    // The row ends before that discarded run.
    if buffer.projects_skipped_gap(gap.start, gap.end) && content < gap.start {
        cut.min(gap.start)
    } else {
        cut
    }
}

/// When `term_flushln()` skips ordinary `SP` inside the same owner before that
/// owner's continuing text (`term.c:205-207`), keep `cut = next.start` like
/// incremental `record/passes.rs` so `trim_native_pass_rows` consumes the blank
/// inside any enclosing inline wrapper (such as `\X'tty: link'`).
fn trims_leading_gap_inside_owner(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    index: usize,
    pass: &super::super::field_buffer::FillPass,
    next: &super::super::field_buffer::FillPass,
    offsets: &OwnerOffsets<'_>,
) -> bool {
    let anchor = &anchors[index];
    let anchor_end = anchors
        .get(index + 1)
        .map_or(buffer.cells().len(), |following| {
            following.start.min(following.content)
        });
    anchor.content <= pass.end
        && next.start < anchor_end
        && buffer.projection_length(anchor.content, pass.end) == 0
        && offsets
            .lengths
            .get(anchor.owner.as_str())
            .copied()
            .unwrap_or(0)
            == 0
        && buffer.projection_length(next.start, anchor_end.min(next.end)) > 0
        && (pass.end..next.start).all(|gap_cell| {
            buffer.projection_length(gap_cell, gap_cell + 1) == 0
                || matches!(
                    buffer.cells()[gap_cell],
                    super::super::field_buffer::FieldCell::BreakableBlank
                ) && !buffer.has_deferred_blank_at(gap_cell)
        })
}

fn project_detached_gap_restart<'a>(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &'a [super::super::NativeWordAnchor],
    pass: &super::super::field_buffer::FillPass,
    (restart, owner): (usize, usize),
    first_owners: &std::cell::OnceCell<std::collections::HashMap<&'a str, usize>>,
    boundaries: &mut BTreeMap<String, Vec<RecordedRowSplit>>,
) -> bool {
    let Some(following) = anchors.get(owner) else {
        return false;
    };
    let Some(gap) = following.projected_gap_cell() else {
        return false;
    };
    if gap >= restart
        || following.start < restart
        || gap < buffer.printed_content_end(pass.start, pass.end)
    {
        return false;
    }
    // term_field() defers trailing padding. If replay assigned it to a
    // fresh next owner, the cut belongs before that owner's first scalar.
    // A raw count on the old owner can include several writes to one glyph.
    if first_owner_positions(anchors, first_owners)[following.owner.as_str()] != owner {
        return false;
    }
    boundaries
        .entry(following.owner.clone())
        .or_default()
        .push(RecordedRowSplit {
            offset: 0,
            pass_end: pass.end,
            cut: pass.end,
            before_cached_glyph: false,
            section_seam: false,
        });
    true
}

fn first_owner_positions<'a, 'map>(
    anchors: &'a [super::super::NativeWordAnchor],
    positions: &'map std::cell::OnceCell<std::collections::HashMap<&'a str, usize>>,
) -> &'map std::collections::HashMap<&'a str, usize> {
    positions.get_or_init(|| {
        let mut first = std::collections::HashMap::new();
        for (index, anchor) in anchors.iter().enumerate() {
            first.entry(anchor.owner.as_str()).or_insert(index);
        }
        first
    })
}

enum DefinitionBoundary<'a> {
    Head {
        receiver: &'a super::super::definition::head_presentation::DeferredHeadProjection,
        last: usize,
        anchor: usize,
    },
    Body(usize),
}

fn definition_handoff<'a>(
    handoff: Option<NativePassHandoff<'a>>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    pass: &super::super::field_buffer::FillPass,
    next: &super::super::field_buffer::FillPass,
    has_following_pass: bool,
) -> Option<DefinitionBoundary<'a>> {
    let Some(NativePassHandoff::Definition(head)) = handoff else {
        return None;
    };
    let last = buffer.last_projected_cell(pass.start..pass.end)?;
    let anchor = anchors
        .partition_point(|anchor| anchor.content <= last)
        .checked_sub(1)?;
    if !head.owns_word(&anchors[anchor].owner) {
        return None;
    }
    let Some(first) =
        (next.start..next.end).find(|&cell| buffer.projection_length(cell, cell + 1) > 0)
    else {
        return (pass.boundary == super::super::field_buffer::FillBoundary::WordEndBreak
            && (has_following_pass
                || buffer.printed_content_end(next.start, next.end) > next.start))
            .then_some(DefinitionBoundary::Head {
                receiver: head,
                last,
                anchor,
            });
    };
    let following = anchors
        .partition_point(|anchor| anchor.content <= first)
        .checked_sub(1)?;
    if head.owns_word(&anchors[following].owner) {
        // Both accepted intervals still belong to the suspended term. Keep
        // its authored hard row even when BODY flushes it (term.c:205-220).
        // Device-width wrapping remains the reader's responsibility.
        (pass.boundary == super::super::field_buffer::FillBoundary::WordEndBreak).then_some(
            DefinitionBoundary::Head {
                receiver: head,
                last,
                anchor,
            },
        )
    } else {
        Some(DefinitionBoundary::Body(following))
    }
}

fn has_unowned_prefix_rows(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
) -> bool {
    anchors.first().is_some_and(|anchor| {
        passes
            .first()
            .is_some_and(|pass| pass.end <= anchor.content)
            && passes.last().is_some_and(|pass| {
                buffer.printed_content_end(anchor.content, pass.end) > anchor.content
            })
    })
}

fn projected_restart_owner(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    pass: &super::super::field_buffer::FillPass,
    next: &super::super::field_buffer::FillPass,
    unowned_prefix: bool,
) -> usize {
    use super::super::field_buffer::FillBoundary;
    if pass.boundary == FillBoundary::WordEndBreak
        && (buffer.printed_content_end(pass.start, pass.end) == pass.start
            || buffer.projection_length(pass.start, pass.end) == 0)
        && let Some(first) =
            (next.start..next.end).find(|&cell| buffer.projection_length(cell, cell + 1) != 0)
    {
        // An invisible-only or unprojected-blank pass still ends a line.
        // Its boundary needs to precede the first accepted glyph, not the
        // last source word within that next native pass (term.c:205-220).
        return anchors.partition_point(|anchor| anchor.content <= first);
    }

    // term_flushln() may restart within the next word's invisible prefix,
    // even when replay has removed its original entry-gap receipt.
    let cell = next.start;
    let owner = anchors.partition_point(|anchor| anchor.content <= cell);
    if owner == 0
        && unowned_prefix
        && pass.boundary == FillBoundary::WordEndBreak
        && buffer.printed_content_end(pass.start, pass.end) == pass.start
    {
        // An IR-only drain can leave several earlier NBRZW marker passes
        // without scalar owners. term_flushln() still ends each row
        // before the first later owner with accepted ink (term.c:205-220).
        return 1;
    }
    for candidate in owner..anchors.len() {
        let anchor = &anchors[candidate];
        if buffer.projection_length(cell, anchor.content) != 0 {
            break;
        }
        if anchor.start > cell {
            continue;
        }
        let end = anchors
            .get(candidate + 1)
            .map_or(buffer.cells().len(), |next| next.start.min(next.content));
        if anchor.entry_gap_cell().is_some() || buffer.projection_length(anchor.content, end) > 0 {
            return candidate + 1;
        }
    }
    owner
}

/// An accepted pass to project in the active formatter word.
pub(super) struct RecordedRowSplit {
    pub(super) offset: usize,
    pub(super) pass_end: usize,
    /// Native cell index where the `LineBreak` was actually placed. Blanks in
    /// `cut..pass_end` remain after the `LineBreak` and are not trimmed by
    /// `trim_native_pass_rows`.
    pub(super) cut: usize,
    /// The following native scalar already exists in this word but its
    /// cached zero-advance glyph cannot appear in the current fragment.
    pub(super) before_cached_glyph: bool,
    pub(super) section_seam: bool,
}

impl RecordedRowSplit {
    fn hard(offset: usize, pass_end: usize) -> Self {
        Self {
            offset,
            pass_end,
            cut: pass_end,
            before_cached_glyph: false,
            section_seam: false,
        }
    }
}

pub(super) fn split_recorded_native_passes(
    nodes: &[Inline],
    splits: BTreeMap<String, Vec<RecordedRowSplit>>,
    buffer: &mut super::super::field_buffer::FieldBuffer,
) -> Vec<Inline> {
    project_recorded_passes(nodes, splits, buffer, false)
}

fn project_recorded_passes(
    nodes: &[Inline],
    mut splits: BTreeMap<String, Vec<RecordedRowSplit>>,
    buffer: &mut super::super::field_buffer::FieldBuffer,
    retired: bool,
) -> Vec<Inline> {
    let mut positions = BTreeMap::new();
    for (owner, splits) in &mut splits {
        splits.sort_unstable_by_key(|split| (split.offset, split.pass_end));
        positions.insert(
            owner.clone(),
            splits
                .iter()
                .map(|split| PassBoundary {
                    scalar: split.offset,
                    section_seam: split.section_seam,
                })
                .collect(),
        );
    }
    let (output, projected) =
        rows::split_native_field_passes_mode(nodes, positions, &|owner, index| {
            retired || splits[owner][index].before_cached_glyph
        });
    for (owner, splits) in splits {
        let count = projected.get(&owner).map_or(0, |&(_, count)| count);
        for split in splits.into_iter().take(count) {
            // Record each projected term_flushln() split cut (term.c:163, 389-428).
            buffer.acknowledge_projected_pass_cut(split.pass_end, split.cut);
        }
    }
    output
}

#[cfg(test)]
#[path = "native_passes/tests.rs"]
mod native_pass_tests;
