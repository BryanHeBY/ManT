//! Apply the validated slot overlay to the previous word owners.

use super::super::super::super::ProjectedNativeGap;
use super::super::super::super::field_buffer::{FieldCell, RewoundGlyph};
use super::{FieldBuffer, FieldWrite, Inline, NativeWordAnchor, earlier_word_after_cell};
use crate::mandoc::inline::flow::definition::head_presentation::DeferredHeadProjection;
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
            let Some(scalar) = rewound_scalar(&buffer.cells()[cell]) else {
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

    pub(super) fn first_incoming(&self) -> Option<usize> {
        self.first_incoming
    }

    pub(super) fn retains_blank(&self, cell: usize) -> bool {
        self.gaps.retained_before_incoming == Some(cell)
            || self
                .gaps
                .retained_ranges
                .iter()
                .any(|range| range.contains(&cell))
            || self
                .retained_blanks
                .iter()
                .any(|&(position, _, _)| position == cell)
    }

    pub(super) fn project(
        &mut self,
        nodes: &mut [Inline],
        incoming: &mut Vec<Inline>,
        incoming_owner: &str,
        earlier: &[NativeWordAnchor],
        head: Option<&DeferredHeadProjection>,
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
                relocated.insert(slot.marker, (projection_owner, projection));
            }
        }
        let mut removed_gaps = BTreeMap::new();
        if !self.gaps.remove.is_empty() {
            // Edit the owned gap after using the old scalar coordinates
            // to detach surviving glyphs from their source words.
            take_overwritten_gaps(nodes, &self.gaps.remove, &mut None, &mut removed_gaps);
        }
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
                (&self.gaps.relocate, &mut self.deferred_blanks),
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
        mut relocated: BTreeMap<usize, (String, Inline)>,
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
            } else if let Some((owner, glyph)) = relocated.remove(&slot) {
                incoming.push(Inline::anchor(owner));
                incoming.push(glyph);
                incoming.push(Inline::anchor(incoming_owner));
            }
        }
    }
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

fn rewound_scalar(cell: &FieldCell) -> Option<char> {
    match cell {
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

#[derive(Default)]
struct GapEdits {
    remove: BTreeMap<String, GapRemoval>,
    relocate: BTreeMap<usize, GapLanding>,
    retained_before_incoming: Option<usize>,
    retained_ranges: Vec<std::ops::Range<usize>>,
}

struct GapLanding {
    marker: String,
    cell: usize,
    positioning_only: bool,
}

struct GapRemoval {
    lost_columns: usize,
    adopted_columns: Vec<usize>,
    relocate: bool,
}

type SourceProjection = Option<(usize, usize, usize)>;

#[derive(Clone, Copy)]
struct GapOverlay<'a>(
    &'a [Option<usize>],
    &'a [SourceProjection],
    &'a BTreeMap<usize, (usize, usize)>,
    &'a [usize],
);

fn rebase_earlier_words(
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
    }
    gaps
}

impl GapEdits {
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
            .strip_prefix(super::super::super::INTERNAL_FIELD_WORD)
            .expect("native word owner");
        let marker = format!("{}{serial}", super::super::super::INTERNAL_FIELD_PREFIX);
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
