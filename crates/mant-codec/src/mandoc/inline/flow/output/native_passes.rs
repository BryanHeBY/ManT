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
    let start = output_start.min(nodes.len());
    let mut boundaries: BTreeMap<String, Vec<RecordedRowSplit>> = BTreeMap::new();
    let mut positions = BTreeMap::new();
    let first_owners = std::cell::OnceCell::new();
    let has_drawing_advance = buffer.has_deferred_drawing_advances();
    let unowned_prefix = handoff.is_none() && has_unowned_prefix_rows(buffer, anchors, passes);
    let mut preceding = 0;
    for (pass, next) in passes[..count].iter().zip(passes.iter().skip(1)) {
        if buffer.has_projected_pass(pass.end) || pass.boundary == FillBoundary::Hyphen {
            // An encoded ASCII_HYPH candidate locates the final device row
            // for BODY. The responsive reading projection keeps that source
            // word complete rather than hardening its discretionary split
            // (term.c:307-324); authored \p events remain independent.
            continue;
        }
        // The final flush can reach a pass that was still editable when
        // words were recorded. Its skipped SP still has provisional
        // reading positions. Keep those positions until owner acceptance
        // removes them, rather than letting them spend the next glyph's
        // scalar quota (term.c:205-220).
        let pending_gap = pass.boundary == FillBoundary::WordEndBreak
            && pass.end < next.start
            && buffer.projection_length(pass.end, next.start) != 0;
        if pending_gap {
            buffer.defer_projected_gap(pass.end, next.start);
        }
        let cell = next.start;
        let owner = projected_restart_owner(buffer, anchors, pass, next, unowned_prefix);
        if let Some(mut index) = owner.checked_sub(1) {
            if project_definition_handoff(handoff, buffer, anchors, pass, next, &mut boundaries) {
                continue;
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
                    &first_owners,
                    &mut boundaries,
                )
            {
                continue;
            }
            let mut cut = cell;
            let (span_end, accepted_end) = pass_cut_bounds(buffer, pass, cell, has_drawing_advance);
            if !section_seam
                && (pending_gap
                    || projected_restart_needs_earlier_cut(
                        buffer,
                        anchors,
                        index,
                        pass,
                        cell,
                        span_end,
                        &first_owners,
                    ))
                && let Some(last) = buffer.last_projected_cell(pass.start..accepted_end)
            {
                // A replay can give this word an earlier fragment with
                // the same scalar position. Anchor the native endline
                // after the last accepted ink instead (term.c:205-220).
                let actual = anchors.partition_point(|anchor| anchor.content <= last);
                if let Some(actual) = actual.checked_sub(1)
                    && actual >= preceding
                    && actual < index
                    && (pending_gap || anchors[actual].owner != anchors[index].owner)
                    // A detached label is outside the current output span.
                    // Keep its closing boundary with the first BODY owner.
                    && !matches!(
                        handoff,
                        Some(NativePassHandoff::Definition(head))
                            if head.owns_word(&anchors[actual].owner)
                    )
                {
                    index = actual;
                    cut = last + 1;
                }
            }
            let anchor = &anchors[index];
            cut = exclude_skipped_gap(buffer, anchor.content, cut, pass.end..next.start);
            // Native replay can move part of one word after another
            // owner's glyph. The projection cursor counts an owner's
            // scalars across all of its fragments, not just this span.
            for pair in anchors[preceding..=index].windows(2) {
                let (previous, next) = (&pair[0], &pair[1]);
                *positions.entry(previous.owner.as_str()).or_insert(0) += buffer.projection_length(
                    previous.content,
                    next.start.min(next.content).max(previous.content),
                );
            }
            preceding = index;
            boundaries
                .entry(anchor.owner.clone())
                .or_default()
                .push(RecordedRowSplit {
                    offset: positions.get(anchor.owner.as_str()).copied().unwrap_or(0)
                        + buffer.projection_length(anchor.content, cut),
                    pass_end: pass.end,
                    before_cached_glyph: false,
                    section_seam,
                });
        }
    }
    if !boundaries.is_empty() {
        // Earlier flushed fields are immutable output. Only the active
        // suffix is traversed, once per real flush (term.c:233-237).
        let pending = nodes.split_off(start);
        nodes.extend(project_recorded_passes(&pending, boundaries, buffer, true));
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
    let Some(gap) = following.projected_gap_cell else {
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

fn project_definition_handoff(
    handoff: Option<NativePassHandoff<'_>>,
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    pass: &super::super::field_buffer::FillPass,
    next: &super::super::field_buffer::FillPass,
    boundaries: &mut BTreeMap<String, Vec<RecordedRowSplit>>,
) -> bool {
    if let Some(NativePassHandoff::Definition(head)) = handoff
        && let Some(last) = buffer.last_projected_cell(pass.start..pass.end)
        && anchors[..anchors.partition_point(|anchor| anchor.content <= last)]
            .last()
            .is_some_and(|anchor| head.owns_word(&anchor.owner))
        && let Some(first) =
            (next.start..next.end).find(|&cell| buffer.projection_length(cell, cell + 1) > 0)
        && let Some(body) = anchors[..anchors.partition_point(|anchor| anchor.content <= first)]
            .last()
            .filter(|anchor| !head.owns_word(&anchor.owner))
    {
        // term_flushln closes the HEAD before skipping invisible entry
        // cells. The detached HEAD has no marker in this output.
        boundaries
            .entry(body.owner.clone())
            .or_default()
            .push(RecordedRowSplit {
                offset: 0,
                pass_end: pass.end,
                before_cached_glyph: false,
                section_seam: false,
            });
        return true;
    }
    false
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
        && buffer.printed_content_end(pass.start, pass.end) == pass.start
        && let Some(first) =
            (next.start..next.end).find(|&cell| buffer.projection_length(cell, cell + 1) != 0)
    {
        // An invisible-only pass still ends a line. Its boundary needs
        // to precede the first accepted glyph, not the last source word
        // within that next native pass (term.c:205-220).
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
    let Some(anchor) = anchors.get(owner).filter(|anchor| anchor.start <= cell) else {
        return owner;
    };
    let end = anchors
        .get(owner + 1)
        .map_or(buffer.cells().len(), |next| next.start.min(next.content));
    let restart_before_payload = buffer.projection_length(cell, anchor.content) == 0
        && buffer.projection_length(anchor.content, end) > 0;
    owner + usize::from(anchor.entry_gap_cell().is_some() || restart_before_payload)
}

/// An accepted pass to project in the active formatter word.
pub(super) struct RecordedRowSplit {
    pub(super) offset: usize,
    pub(super) pass_end: usize,
    /// The following native scalar already exists in this word but its
    /// cached zero-advance glyph cannot appear in the current fragment.
    pub(super) before_cached_glyph: bool,
    pub(super) section_seam: bool,
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
            buffer.acknowledge_projected_pass(split.pass_end);
        }
    }
    output
}

#[cfg(test)]
#[path = "native_passes/tests.rs"]
mod native_pass_tests;
