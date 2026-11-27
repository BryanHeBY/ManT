//! Rebase and project the earlier words' generated gaps after slot replay.

use super::super::super::repeated_owners::neutral_key;
use super::{
    FieldBuffer, FieldCell, Inline, NativeWordAnchor, SourceProjection, word_start_after_overlay,
};
use crate::mandoc::inline::flow::output::{INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD};
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
pub(super) struct GapEdits {
    changed_owner_keys_from: Option<usize>,
    remove: BTreeMap<String, GapRemoval>,
    relocate: BTreeMap<usize, GapLanding>,
    retained_before_incoming: Option<usize>,
    retained_ranges: Vec<std::ops::Range<usize>>,
}

pub(super) struct GapLanding {
    pub(super) marker: String,
    pub(super) cell: usize,
    pub(super) positioning_only: bool,
}

struct GapRemoval {
    lost_columns: usize,
    adopted_columns: Vec<usize>,
    relocate: bool,
}

#[derive(Clone, Copy)]
pub(super) struct GapOverlay<'a>(
    pub(super) &'a [Option<usize>],
    pub(super) &'a [SourceProjection],
    pub(super) &'a BTreeMap<usize, (usize, usize)>,
    pub(super) &'a [usize],
);

pub(super) fn rebase_earlier_words(
    (buffer, earlier): (&FieldBuffer, &mut [NativeWordAnchor]),
    overlay: GapOverlay<'_>,
    (start, first_incoming): (usize, usize),
    earlier_ink: &HashMap<usize, usize>,
) -> GapEdits {
    let GapOverlay(positions, _, _, _) = overlay;
    let first_touched = earlier.partition_point(|owner| owner.content < start);
    let mut gaps = GapEdits::default();
    let incoming_start = word_start_after_overlay(positions, start, first_incoming);
    for (index, owner) in earlier.iter_mut().enumerate().skip(first_touched) {
        let previous_key = neutral_key(owner);
        if let Some(old) = owner.projected_gap_cell()
            && let Some(offset) = old
                .checked_sub(start)
                .filter(|&offset| offset < positions.len())
        {
            gaps.rebase(
                buffer,
                overlay,
                (owner, index),
                start,
                offset,
                first_incoming,
            );
        }
        let remap = |index: usize| {
            let Some(offset) = index.checked_sub(start) else {
                return index;
            };
            if offset >= positions.len() {
                // A zero-width source word at the old endpoint must
                // precede the incoming word's anchor, not its first
                // painted graph following a separator or decoration.
                return incoming_start;
            }
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
        if previous_key != neutral_key(owner) && gaps.changed_owner_keys_from.is_none() {
            gaps.changed_owner_keys_from = Some(index);
        }
    }
    gaps
}

impl GapEdits {
    pub(super) fn changed_owner_keys_from(&self) -> Option<usize> {
        self.changed_owner_keys_from
    }

    pub(super) fn retains_blank(&self, cell: usize) -> bool {
        self.retained_before_incoming == Some(cell)
            || self
                .retained_ranges
                .iter()
                .any(|range| range.contains(&cell))
    }

    pub(super) fn relocations(&self) -> &BTreeMap<usize, GapLanding> {
        &self.relocate
    }

    pub(super) fn take_overwritten(&self, nodes: &mut [Inline]) -> BTreeMap<&str, String> {
        let mut removed = BTreeMap::new();
        if !self.remove.is_empty() {
            take_overwritten_gaps(nodes, &self.remove, &mut None, &mut removed);
        }
        removed
    }

    fn rebase(
        &mut self,
        buffer: &FieldBuffer,
        overlay: GapOverlay<'_>,
        (owner, owner_index): (&mut NativeWordAnchor, usize),
        start: usize,
        offset: usize,
        first_incoming: usize,
    ) {
        let GapOverlay(_, locations, _, adopted_advances) = overlay;
        let precise = owner
            .projected_gap
            .motion_span
            .take()
            .and_then(|span| rebase_motion_span(buffer, overlay, span, start, first_incoming));
        let has_exact_span = precise.is_some();
        let RebasedMotionSpan {
            lost_columns,
            landing: surviving,
            span,
        } = precise.unwrap_or_else(|| {
            infer_gap_landing(
                buffer,
                overlay,
                (owner, owner_index),
                start,
                offset,
                first_incoming,
            )
        });
        owner.projected_gap.motion_span = span;
        owner.projected_gap.cell = surviving.map(|(_, cell)| cell);
        let relocate = surviving.is_some_and(|(_, cell)| cell >= first_incoming);
        // The exact span already retires cells transferred to the new
        // owner. Its fallback can instead use the explicit adopted tail.
        let adopted_columns = surviving
            .filter(|_| relocate && !has_exact_span && !adopted_advances.is_empty())
            .map_or_else(Vec::new, |(survivor, _)| {
                adopted_gap_tail(
                    buffer,
                    locations,
                    adopted_advances,
                    owner_index,
                    start + offset..start + survivor + 1,
                    start,
                )
            });
        if (lost_columns == 0 || has_exact_span)
            && !relocate
            && let Some((_, cell)) = surviving
        {
            // This native slot still has its earlier geometry marker.
            // term_field() releases the blank only once before new ink.
            self.retained_before_incoming = self.retained_before_incoming.max(Some(cell));
            self.retained_ranges.push(
                owner
                    .projected_gap
                    .motion_span
                    .clone()
                    .unwrap_or(cell..cell + 1),
            );
            if lost_columns == 0 {
                return;
            }
        }
        let serial = owner
            .owner
            .strip_prefix(INTERNAL_FIELD_WORD)
            .expect("native word owner");
        let marker = format!("{INTERNAL_FIELD_PREFIX}{serial}");
        if let Some((source, cell)) = surviving.filter(|_| relocate) {
            self.relocate.insert(
                source,
                GapLanding {
                    marker: marker.clone(),
                    cell,
                    positioning_only: locations[source].is_none(),
                },
            );
        }
        owner.projected_field_prefix &= surviving.is_some();
        self.remove.insert(
            marker,
            GapRemoval {
                lost_columns: surviving.map_or(usize::MAX, |_| lost_columns),
                adopted_columns,
                relocate,
            },
        );
    }
}

struct RebasedMotionSpan {
    lost_columns: usize,
    landing: Option<(usize, usize)>,
    span: Option<std::ops::Range<usize>>,
}

fn rebase_motion_span(
    buffer: &FieldBuffer,
    GapOverlay(positions, _, retained, _): GapOverlay<'_>,
    span: std::ops::Range<usize>,
    start: usize,
    first_incoming: usize,
) -> Option<RebasedMotionSpan> {
    if span.start < start || span.end > start + positions.len() {
        return None;
    }
    let mut result = RebasedMotionSpan {
        lost_columns: 0,
        landing: None,
        span: None,
    };
    let mut exact_landing = true;
    for source in span {
        let offset = source - start;
        let slot = &buffer.cells()[source];
        let width = match slot {
            FieldCell::FixedDrawingAdvance(width) | FieldCell::DrawnSpaces(width) => *width,
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => 1,
            _ => return None,
        };
        let (position, columns) = if let Some(&(cell, kept)) = retained.get(&source) {
            // This receipt knows the total and first slot of a retained
            // advance, but not whether its width was split into later slots.
            exact_landing &= kept <= 1;
            (Some(cell), kept.min(width))
        } else {
            (positions[offset], positions[offset].map_or(0, |_| width))
        };
        result.lost_columns += width - columns;
        if let Some(position) = position.filter(|_| columns > 0) {
            if let Some(span) = &mut result.span {
                if position != span.end
                    || (position < first_incoming) != (span.start < first_incoming)
                {
                    return None;
                }
                span.end += 1;
            } else {
                result.span = Some(position..position + 1);
            }
            let can_insert = positions[offset].is_some()
                && matches!(
                    slot,
                    FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
                );
            if position < first_incoming || can_insert {
                // Only a surviving SP/NBRSP supplies a relocation marker.
                // An earlier landing already owns the preceding seam.
                result.landing = Some((offset, position));
            }
        }
    }
    if result.span.is_some() && result.landing.is_none() {
        return None;
    }
    if !exact_landing {
        result.span = None;
    }
    Some(result)
}

fn infer_gap_landing(
    buffer: &FieldBuffer,
    GapOverlay(positions, locations, retained_advances, _): GapOverlay<'_>,
    (owner, owner_index): (&NativeWordAnchor, usize),
    start: usize,
    offset: usize,
    first_incoming: usize,
) -> RebasedMotionSpan {
    let mut lost_columns = 0usize;
    let mut landing = None;
    // Without an exact mapping, retain the established neighboring-cell
    // proof for projected separators and device fields.
    for (source, &position) in positions.iter().enumerate().skip(offset) {
        let width = match buffer.cells()[start + source] {
            FieldCell::DrawnSpaces(width) | FieldCell::FixedDrawingAdvance(width) => width,
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => 1,
            _ => break,
        };
        if buffer.has_deferred_blank_at(start + source) {
            // A deferred blank carries its own DeferredBlankRun and is never
            // counted in retained_motion_gap's projected prefix width.
            continue;
        }
        if source != offset && locations[source].is_some_and(|(index, _, _)| index != owner_index) {
            // A source blank keeps its own scalar and moves independently.
            continue;
        }
        if let Some(&(cell, kept)) = retained_advances.get(&(start + source)) {
            lost_columns = lost_columns.saturating_add(width.saturating_sub(kept));
            landing = Some((source, cell));
            break;
        }
        if let Some(cell) = position {
            landing = Some((source, cell));
            break;
        }
        lost_columns = lost_columns.saturating_add(width);
    }
    if landing.is_none() && buffer.has_deferred_blank_at(start + offset) {
        // When `projected_gap` points at a deferred blank whose run had an
        // earlier non-deferred blank immediately to its left, keep the gap
        // anchored on that surviving blank instead of dropping it
        // (`term.c:389-442`).
        for source in (0..offset).rev() {
            let width = match buffer.cells()[start + source] {
                FieldCell::DrawnSpaces(width) | FieldCell::FixedDrawingAdvance(width) => width,
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => 1,
                _ => break,
            };
            if buffer.has_deferred_blank_at(start + source) {
                continue;
            }
            if locations[source].is_some_and(|(index, _, _)| index != owner_index) {
                continue;
            }
            if let Some(&(cell, kept)) = retained_advances.get(&(start + source)) {
                lost_columns = lost_columns.saturating_add(width.saturating_sub(kept));
                landing = Some((source, cell));
                break;
            }
            if let Some(cell) = positions[source] {
                landing = Some((source, cell));
                break;
            }
            lost_columns = lost_columns.saturating_add(width);
        }
    }
    if landing.is_some_and(|(_, cell)| cell >= first_incoming) {
        // term_field() queues one run before a graph. Some consumed
        // columns can precede the location of the recorded separator.
        lost_columns = lost_columns.saturating_add(clipped_gap_prefix(
            buffer,
            (positions, locations),
            owner.start.max(start)..start + offset,
            start,
        ));
    }
    RebasedMotionSpan {
        lost_columns,
        landing,
        span: None,
    }
}

fn adopted_gap_tail(
    buffer: &FieldBuffer,
    locations: &[SourceProjection],
    adopted: &[usize],
    owner: usize,
    prefix: std::ops::Range<usize>,
    start: usize,
) -> Vec<usize> {
    let mut column = 0usize;
    let mut columns = Vec::new();
    let end = start + locations.len();
    for (cell, kind) in buffer
        .cells()
        .iter()
        .enumerate()
        .take(end)
        .skip(prefix.start)
    {
        let width = match kind {
            FieldCell::DrawnSpaces(width) | FieldCell::FixedDrawingAdvance(width) => *width,
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => 1,
            _ => break,
        };
        if cell > prefix.start
            && locations
                .get(cell - start)
                .copied()
                .flatten()
                .is_some_and(|(index, _, _)| index != owner)
        {
            continue;
        }
        // A new deferred receipt may acquire an advance beyond the
        // first surviving space of this already emitted prefix. Trim
        // that column only if the actual old output covered it.
        if cell >= prefix.end && width == 1 && adopted.contains(&cell) {
            columns.push(column);
        }
        column = column.saturating_add(width);
    }
    columns
}

fn clipped_gap_prefix(
    buffer: &FieldBuffer,
    (positions, locations): (&[Option<usize>], &[SourceProjection]),
    prefix: std::ops::Range<usize>,
    start: usize,
) -> usize {
    prefix
        .rev()
        .map_while(|cell| {
            let offset = cell - start;
            if positions[offset].is_some()
                || locations[offset].is_some()
                || buffer.has_deferred_blank_at(cell)
            {
                return None;
            }
            match buffer.cells()[cell] {
                FieldCell::DrawnSpaces(width) | FieldCell::FixedDrawingAdvance(width) => {
                    Some(width)
                }
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => Some(1),
                _ => None,
            }
        })
        .fold(0, usize::saturating_add)
}

fn take_overwritten_gaps<'a>(
    nodes: &mut [Inline],
    owners: &'a BTreeMap<String, GapRemoval>,
    pending: &mut Option<(&'a str, &'a GapRemoval)>,
    removed: &mut BTreeMap<&'a str, String>,
) {
    for node in nodes {
        match node {
            Inline::Anchor { id, .. } => {
                *pending = owners
                    .get_key_value(id.as_str())
                    .map(|(owner, edit)| (owner.as_str(), edit));
            }
            Inline::Text { value } => {
                if let Some((owner, edit)) = pending.take() {
                    let columns = value.len() - value.trim_start_matches(' ').len();
                    let lost = columns.min(edit.lost_columns);
                    if edit.relocate && lost < columns {
                        let adopted = edit
                            .adopted_columns
                            .iter()
                            .filter(|&&column| column < columns)
                            .count();
                        let remaining = columns.saturating_sub(lost + adopted);
                        if remaining != 0 {
                            removed.insert(owner, value[lost..lost + remaining].to_owned());
                        }
                    }
                    value.drain(..if edit.relocate { columns } else { lost });
                }
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                take_overwritten_gaps(children, owners, pending, removed);
            }
            _ => *pending = None,
        }
    }
}
