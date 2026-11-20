//! Project accepted native field passes without executing formatter state.

use super::super::Inline;
use super::split::{PassBoundary, advance_boundary, split_text_at_boundaries};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct OwnerAcceptance {
    pub(in crate::mandoc::inline::flow) scalars: usize,
    pub(in crate::mandoc::inline::flow) native_cells: bool,
    /// The first accepted interval printed this word's deferred device pad.
    /// An accepted NBRZW prefix alone cannot print it (term.c:389-427).
    pub(in crate::mandoc::inline::flow) prefix_printed: bool,
    /// Source scalars in an accepted pass with no surviving device cell.
    /// They can be unprinted padding or covered glyphs.
    pub(in crate::mandoc::inline::flow) omitted_scalars: Vec<ScalarOmission>,
}

#[derive(Clone, Debug)]
pub(in crate::mandoc::inline::flow) struct ScalarOmission {
    pub(super) range: std::ops::Range<usize>,
    pub(super) covered: bool,
}

impl OwnerAcceptance {
    fn omit_cell(
        &mut self,
        buffer: &super::super::field_buffer::FieldBuffer,
        start: usize,
        cell: usize,
        covered: bool,
    ) {
        let first = self.scalars + buffer.projection_length(start, cell);
        let last = self.scalars + buffer.projection_length(start, cell + 1);
        if first < last {
            if let Some(previous) = self.omitted_scalars.last_mut()
                && previous.range.end == first
                && previous.covered == covered
            {
                previous.range.end = last;
            } else {
                self.omitted_scalars.push(ScalarOmission {
                    range: first..last,
                    covered,
                });
            }
        }
    }
}

/// Accepted projection range of each stable source word. A delayed glyph
/// can re-enter that owner after a later word marker without becoming its
/// rejected suffix. Native cell intervals, not append order, decide limits.
pub(in crate::mandoc::inline::flow) fn accepted_owner_lengths(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    preserve_literal_whitespace: bool,
) -> BTreeMap<String, OwnerAcceptance> {
    use super::super::field_buffer::FieldCell;

    let mut lengths = BTreeMap::new();
    let covered = buffer.covered_zero_width_retreats(passes);
    let printed_ends: Vec<_> = passes
        .iter()
        .map(|pass| {
            let printed_end = buffer.printed_content_end(pass.start, pass.end);
            let printed = printed_end > pass.start;
            let projected = buffer.has_projected_pass(pass.end);
            // Acknowledged endlines already removed trailing SP from the
            // projection. Do not charge those old scalar slots to later
            // output when term_field's receipt retires unprinted padding.
            // Preserve any intervening authored recovery spelling.
            let mut projected_end = pass.end;
            if projected {
                while projected_end > printed_end {
                    let cell = projected_end - 1;
                    if !matches!(
                        buffer.cells()[cell],
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
                    ) && buffer.projection_length(cell, projected_end) != 0
                    {
                        break;
                    }
                    projected_end = cell;
                }
            }
            (printed_end, printed, projected_end, projected)
        })
        .collect();
    let mut pass_index = 0;
    for (index, anchor) in anchors.iter().enumerate() {
        let owner = &anchor.owner;
        let content = &anchor.content;
        #[cfg(test)]
        OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
        // BACKBEFORE may replace the previous trailing blank before this
        // word's original buffer end. Its landing, not that old end, owns
        // the replacement graph (term.c:901-908).
        let end = anchors
            .get(index + 1)
            .map_or(buffer.cells().len(), |next| next.start.min(next.content));
        if end <= *content {
            // An empty word can be followed by BACKBEFORE replacing its
            // trailing native blank. Its zero-length projection interval
            // cannot consume a pass needed by that replacement's owner.
            lengths.entry(owner.clone()).or_default();
            continue;
        }
        while passes
            .get(pass_index)
            .is_some_and(|pass| pass.end <= *content)
        {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            pass_index += 1;
        }
        let acceptance: &mut OwnerAcceptance = lengths.entry(owner.clone()).or_default();
        while let Some(pass) = passes.get(pass_index) {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            let start = (*content).max(pass.start);
            let accepted_end = end.min(pass.end);
            let (printed_end, printed, projected_end, projected) = printed_ends[pass_index];
            if start < accepted_end {
                let projected_end = projected_end.min(accepted_end);
                let covered_start = covered.partition_point(|&cell| cell < start);
                for &cell in covered[covered_start..]
                    .iter()
                    .take_while(|&&cell| cell < projected_end)
                {
                    acceptance.omit_cell(buffer, start, cell, true);
                }
                let source_literal = preserve_literal_whitespace
                    && anchor.projection
                        == super::super::WordProjectionPolicy::SourceLiteralWhitespace;
                let padding_start = if source_literal && !printed {
                    // A source literal row remains nonempty authored content
                    // even when term_field prints no deferred SP/TAB. This
                    // never changes native acceptance or revives rejected cells.
                    accepted_end
                } else {
                    start.max(printed_end)
                };
                for cell in padding_start..projected_end {
                    // An already projected literal TAB is source content.
                    // In a filled word the same deferred device cell is
                    // still omitted when term_field() did not print it.
                    if source_literal && projected && matches!(buffer.cells()[cell], FieldCell::Tab)
                    {
                        continue;
                    }
                    if matches!(
                        buffer.cells()[cell],
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab
                    ) {
                        acceptance.omit_cell(buffer, start, cell, false);
                    }
                }
                acceptance.scalars += buffer.projection_length(start, projected_end);
                acceptance.native_cells = true;
                if start == *content {
                    acceptance.prefix_printed |=
                        buffer.printed_columns(start, accepted_end, 0, 0).is_some();
                }
            }
            if pass.end >= end {
                break;
            }
            pass_index += 1;
        }
    }
    retire_unprinted_prefixes(anchors, passes, &printed_ends, &mut lengths);
    lengths
}

fn retire_unprinted_prefixes(
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    printed_ends: &[(usize, bool, usize, bool)],
    lengths: &mut BTreeMap<String, OwnerAcceptance>,
) {
    // A replay can move an earlier cell's representation to a prefix
    // under a later owner. Its local graph does not re-authorize a gap
    // consumed between native passes.
    for anchor in anchors {
        if let Some(gap) = anchor.projected_gap_cell {
            let pass = passes.partition_point(|pass| pass.end <= gap);
            let printed = passes
                .get(pass)
                .is_some_and(|pass_row| pass_row.start <= gap && gap < printed_ends[pass].0);
            if !printed {
                lengths
                    .entry(anchor.owner.clone())
                    .or_default()
                    .prefix_printed = false;
            }
        }
    }
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
    section: Option<&super::SectionHandoff>,
) {
    use super::super::field_buffer::FillBoundary;

    let count = passes.len().saturating_sub(1);
    let start = output_start.min(nodes.len());
    let mut boundaries: BTreeMap<String, Vec<RecordedRowSplit>> = BTreeMap::new();
    let mut positions = BTreeMap::new();
    let mut preceding = 0;
    for (pass, next) in passes[..count].iter().zip(passes.iter().skip(1)) {
        if buffer.has_projected_pass(pass.end) || pass.boundary == FillBoundary::Hyphen {
            // An encoded ASCII_HYPH candidate locates the final device row
            // for BODY. The responsive reading projection keeps that source
            // word complete rather than hardening its discretionary split
            // (term.c:307-324); authored \p events remain independent.
            continue;
        }
        let cell = next.start;
        let owner = anchors.partition_point(|anchor| anchor.content <= cell);
        if let Some(index) = owner.checked_sub(1) {
            let anchor = &anchors[index];
            let section_seam = section.is_some_and(|section| {
                section.closes_at_section_seam(buffer, pass.start..pass.end, anchor.content)
            });
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
                        + buffer.projection_length(anchor.content, cell),
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

#[cfg(test)]
std::thread_local! {
    static OWNER_NODES_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNER_PASS_INTERSECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Project the final native pass receipt once, in output-owner order. The
/// receipt supplies scalar offsets inside each formatter word; semantic
/// wrappers do not begin another word or reset its cursor. No formatter
/// register changes here: term.c:205-220 already chose the accepted ranges.
#[cfg(test)]
fn split_native_field_passes(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> Vec<Inline> {
    split_native_field_passes_mode(nodes, hard_boundaries(boundaries), &|_, _| false).0
}

/// Rebuild one complete active native interval, including loop events at the
/// last accepted scalar of an owner. Incremental projection leaves those
/// edges to existing word-time hints; a retired receipt has replaced them.
pub(in crate::mandoc::inline::flow) fn split_retired_native_field_passes(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> Vec<Inline> {
    split_native_field_passes_mode(nodes, hard_boundaries(boundaries), &|_, _| true).0
}

fn hard_boundaries(
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> BTreeMap<String, Vec<PassBoundary>> {
    boundaries
        .iter()
        .map(|(owner, offsets)| {
            (
                owner.clone(),
                offsets.iter().copied().map(PassBoundary::hard).collect(),
            )
        })
        .collect()
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
    let (output, projected) = split_native_field_passes_mode(nodes, positions, &|owner, index| {
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

type OwnerPassPositions = BTreeMap<String, (usize, usize)>;

fn split_native_field_passes_mode(
    nodes: &[Inline],
    mut boundaries: BTreeMap<String, Vec<PassBoundary>>,
    can_project_tail: &dyn Fn(&str, usize) -> bool,
) -> (Vec<Inline>, OwnerPassPositions) {
    for positions in boundaries.values_mut() {
        positions.sort_by_key(|boundary| boundary.scalar);
        // Different native passes can end at the same semantic scalar:
        // NBRZW sets graph but contributes no glyph (term.c:340-349).
        // Keep one event per pass; scalar equality is not event identity.
    }
    let mut cursor = NativePassCursor {
        boundaries: &[],
        scalar: 0,
        next_boundary: 0,
        owner: None,
        positions: BTreeMap::new(),
        can_project_tail,
    };
    let mut output = project_native_pass_nodes(nodes, &boundaries, &mut cursor);
    project_owner_tail_events(&mut cursor, &mut output);
    if let Some(owner) = cursor.owner {
        cursor
            .positions
            .insert(owner, (cursor.scalar, cursor.next_boundary));
    }
    trim_native_pass_rows(&mut output);
    if boundaries
        .values()
        .flatten()
        .any(|boundary| boundary.section_seam)
    {
        remove_section_ends(&mut output);
    }
    (output, cursor.positions)
}

fn remove_section_ends(nodes: &mut Vec<Inline>) {
    nodes.retain_mut(|node| match node {
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => {
            remove_section_ends(children);
            true
        }
        other => !PassBoundary::is_section_end(other),
    });
}

struct NativePassCursor<'a> {
    boundaries: &'a [PassBoundary],
    scalar: usize,
    next_boundary: usize,
    owner: Option<String>,
    positions: OwnerPassPositions,
    can_project_tail: &'a dyn Fn(&str, usize) -> bool,
}

/// `term_flushln()`217 emits the accepted pass's endline even if its final
/// graph is the final scalar before a rejected suffix (term_fill()299-312).
/// Equal scalar positions can still carry separate physical row events.
fn project_owner_tail_events(cursor: &mut NativePassCursor<'_>, output: &mut Vec<Inline>) {
    while let Some(boundary) = cursor.boundaries.get(cursor.next_boundary)
        && boundary.scalar == cursor.scalar
        && cursor
            .owner
            .as_deref()
            .is_some_and(|owner| (cursor.can_project_tail)(owner, cursor.next_boundary))
    {
        output.push(boundary.projection());
        cursor.next_boundary += 1;
    }
}

pub(in crate::mandoc::inline::flow) fn native_word_marker(node: &Inline) -> Option<&str> {
    let marker = match node {
        Inline::Anchor { id, .. } => id.as_str(),
        Inline::Text { value } => value,
        _ => return None,
    };
    marker
        .starts_with(super::INTERNAL_FIELD_WORD)
        .then_some(marker)
}

fn project_native_pass_nodes<'a>(
    nodes: &[Inline],
    boundaries: &'a BTreeMap<String, Vec<PassBoundary>>,
    cursor: &mut NativePassCursor<'a>,
) -> Vec<Inline> {
    let mut output = Vec::with_capacity(nodes.len());
    for node in nodes {
        #[cfg(test)]
        OWNER_NODES_VISITED.with(|work| work.set(work.get().saturating_add(1)));
        if let Some(marker) = native_word_marker(node) {
            if cursor.owner.as_deref() != Some(marker) {
                project_owner_tail_events(cursor, &mut output);
            }
            if let Some(owner) = cursor.owner.take() {
                cursor
                    .positions
                    .insert(owner, (cursor.scalar, cursor.next_boundary));
            }
            cursor.owner = Some(marker.to_owned());
            cursor.boundaries = boundaries.get(marker).map_or(&[], Vec::as_slice);
            (cursor.scalar, cursor.next_boundary) =
                cursor.positions.get(marker).copied().unwrap_or_default();
            output.push(node.clone());
            while let Some(boundary) = cursor
                .boundaries
                .get(cursor.next_boundary)
                .filter(|end| end.scalar == cursor.scalar)
            {
                output.push(boundary.projection());
                cursor.next_boundary += 1;
            }
            continue;
        }
        match node {
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                split_text_at_boundaries(
                    value,
                    node,
                    &mut cursor.scalar,
                    &mut cursor.next_boundary,
                    cursor.boundaries,
                    &mut output,
                );
            }
            Inline::LineBreak { .. } => {
                // A device endline is a receipt event, never a native cell
                // or a scalar from the authored word (term.c:220). If a
                // receipt boundary reaches this same native position, this
                // already executed event satisfies it exactly once.
                advance_boundary(
                    &mut cursor.scalar,
                    &mut cursor.next_boundary,
                    cursor.boundaries,
                );
                output.push(node.clone());
            }
            Inline::Strong { children } => output.push(Inline::Strong {
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            Inline::Emphasis { children } => output.push(Inline::Emphasis {
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),

            Inline::Link {
                target,
                title,
                children,
            } => output.push(Inline::Link {
                target: target.clone(),
                title: title.clone(),
                children: project_native_pass_nodes(children, boundaries, cursor),
            }),
            other => output.push(other.clone()),
        }
    }
    output
}

/// A break at offset zero of a new owner consumes the previous owner's
/// breakable padding too. Do that after projection, including across style
/// and link wrappers, without dropping existing hard boundaries or identity.
fn trim_native_pass_rows(nodes: &mut Vec<Inline>) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        let starts_row = match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                trim_native_pass_rows(children);
                starts_with_native_break(children)
            }
            Inline::LineBreak { .. } => true,
            other => PassBoundary::is_section_end(other),
        };
        if starts_row {
            trim_native_breakable_tail(&mut output);
        }
        output.push(node);
    }
    *nodes = output;
}

pub(super) fn starts_with_native_break(nodes: &[Inline]) -> bool {
    first_native_row_event(nodes) == Some(true)
}

fn first_native_row_event(nodes: &[Inline]) -> Option<bool> {
    for node in nodes {
        if PassBoundary::is_section_end(node) {
            return Some(true);
        }
        if native_word_marker(node).is_some() || matches!(node, Inline::Anchor { .. }) {
            continue;
        }
        match node {
            Inline::LineBreak { .. } => return Some(true),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if let Some(event) = first_native_row_event(children) {
                    return Some(event);
                }
            }
            _ => return Some(false),
        }
    }
    None
}

pub(super) fn trim_native_breakable_tail(nodes: &mut Vec<Inline>) -> bool {
    let mut metadata = Vec::new();
    let mut exhausted = true;
    while let Some(mut node) = nodes.pop() {
        if native_word_marker(&node).is_some() || matches!(node, Inline::Anchor { .. }) {
            metadata.push(node);
            continue;
        }
        if matches!(&node, Inline::Text { value } if value.is_empty()) {
            // An empty provisional word can still carry a cell witness.
            // It does not shield padding from the accepted row end.
            metadata.push(node);
            continue;
        }
        let identity = crate::mandoc::inline::links::presentation::retains_authored_identity(&node);
        let consumed = match &mut node {
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                while value
                    .chars()
                    .next_back()
                    .is_some_and(super::super::super::is_formatter_word_blank)
                {
                    value.pop();
                }
                value.is_empty()
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                let consumed = trim_native_breakable_tail(children);
                if consumed && !children.is_empty() {
                    metadata.push(node);
                    continue;
                }
                if consumed && (identity || matches!(node, Inline::Link { .. })) {
                    metadata.push(node);
                    continue;
                }
                consumed
            }
            _ => false,
        };
        if !consumed {
            nodes.push(node);
            exhausted = false;
            break;
        }
    }
    nodes.extend(metadata.into_iter().rev());
    exhausted
}

#[cfg(test)]
#[path = "native_passes/tests.rs"]
mod native_pass_tests;
