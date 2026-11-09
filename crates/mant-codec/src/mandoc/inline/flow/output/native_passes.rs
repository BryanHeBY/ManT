//! Project accepted native field passes without executing formatter state.

use super::super::Inline;
use super::split::{advance_boundary, split_text_at_boundaries};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct OwnerAcceptance {
    pub(in crate::mandoc::inline::flow) scalars: usize,
    pub(in crate::mandoc::inline::flow) native_cells: bool,
    /// The first accepted interval printed this word's deferred device pad.
    /// An accepted NBRZW prefix alone cannot print it (term.c:389-427).
    pub(in crate::mandoc::inline::flow) prefix_printed: bool,
    /// Accepted source scalars belonging to deferred native padding which
    /// no subsequent graph printed. Recovery spellings remain independent.
    pub(in crate::mandoc::inline::flow) unprinted_padding: Vec<std::ops::Range<usize>>,
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
    let mut previous_end = 0;
    let printed_ends: Vec<_> = passes
        .iter()
        .map(|pass| {
            let printed_end = buffer.printed_content_end(previous_end, pass.end);
            let printed = printed_end > previous_end;
            // Acknowledged endlines already removed trailing SP from the
            // projection. Do not charge those old scalar slots to later
            // output when term_field's receipt retires unprinted padding.
            // Preserve any intervening authored recovery spelling.
            let mut projected_end = pass.end;
            if buffer.has_projected_pass(pass.end) {
                while projected_end > printed_end {
                    let cell = projected_end - 1;
                    if !matches!(buffer.cells()[cell], FieldCell::BreakableBlank)
                        && buffer.projection_length(cell, projected_end) != 0
                    {
                        break;
                    }
                    projected_end = cell;
                }
            }
            previous_end = next_native_pass_start(buffer, pass.end);
            (printed_end, printed, projected_end)
        })
        .collect();
    let mut pass_index = 0;
    let mut pass_start = 0;
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
            lengths.insert(owner.clone(), OwnerAcceptance::default());
            continue;
        }
        while passes
            .get(pass_index)
            .is_some_and(|pass| pass.end <= *content)
        {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            pass_start = next_native_pass_start(buffer, passes[pass_index].end);
            pass_index += 1;
        }
        let mut acceptance = OwnerAcceptance::default();
        while let Some(pass) = passes.get(pass_index) {
            #[cfg(test)]
            OWNER_PASS_INTERSECTIONS.with(|work| work.set(work.get().saturating_add(1)));
            let start = (*content).max(pass_start);
            let accepted_end = end.min(pass.end);
            let (printed_end, printed, projected_end) = printed_ends[pass_index];
            if start < accepted_end {
                let projected_end = projected_end.min(accepted_end);
                let padding_start = if preserve_literal_whitespace
                    && anchor.projection
                        == super::super::WordProjectionPolicy::SourceLiteralWhitespace
                    && !printed
                {
                    // A source literal row remains nonempty authored content
                    // even when term_field prints no deferred SP/TAB. This
                    // never changes native acceptance or revives rejected cells.
                    accepted_end
                } else {
                    start.max(printed_end)
                };
                for cell in padding_start..projected_end {
                    if matches!(
                        buffer.cells()[cell],
                        FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab
                    ) {
                        let first = acceptance.scalars + buffer.projection_length(start, cell);
                        let last = acceptance.scalars + buffer.projection_length(start, cell + 1);
                        if first < last {
                            if let Some(previous) = acceptance.unprinted_padding.last_mut()
                                && previous.end == first
                            {
                                previous.end = last;
                            } else {
                                acceptance.unprinted_padding.push(first..last);
                            }
                        }
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
            pass_start = next_native_pass_start(buffer, pass.end);
            pass_index += 1;
        }
        lengths.insert(owner.clone(), acceptance);
    }
    lengths
}

fn next_native_pass_start(
    buffer: &super::super::field_buffer::FieldBuffer,
    mut cell: usize,
) -> usize {
    while matches!(
        buffer.cells().get(cell),
        Some(super::super::field_buffer::FieldCell::BreakableBlank)
    ) {
        cell += 1;
    }
    cell
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
) {
    use super::super::field_buffer::{FieldCell, FillBoundary};

    let count = passes.len().saturating_sub(1);
    let start = output_start.min(nodes.len());
    let mut boundaries: BTreeMap<String, Vec<RecordedRowSplit>> = BTreeMap::new();
    for pass in &passes[..count] {
        if buffer.has_projected_pass(pass.end) || pass.boundary == FillBoundary::Hyphen {
            // An encoded ASCII_HYPH candidate locates the final device row
            // for BODY. The responsive reading projection keeps that source
            // word complete rather than hardening its discretionary split
            // (term.c:307-324); authored \p events remain independent.
            continue;
        }
        let mut cell = pass.end;
        while matches!(buffer.cells().get(cell), Some(FieldCell::BreakableBlank)) {
            cell += 1;
        }
        let owner = anchors.partition_point(|anchor| anchor.content <= cell);
        if let Some(anchor) = owner.checked_sub(1).and_then(|index| anchors.get(index)) {
            boundaries
                .entry(anchor.owner.clone())
                .or_default()
                .push(RecordedRowSplit {
                    offset: buffer.projection_length(anchor.content, cell),
                    pass_end: pass.end,
                    before_cached_glyph: false,
                });
        }
    }
    if !boundaries.is_empty() {
        // Earlier flushed fields are immutable output. Only the active
        // suffix is traversed, once per real flush (term.c:233-237).
        let pending = nodes.split_off(start);
        nodes.extend(split_recorded_native_passes(&pending, boundaries, buffer));
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
    split_native_field_passes_mode(nodes, boundaries, &|_, _| false).0
}

/// Rebuild one complete active native interval, including loop events at the
/// last accepted scalar of an owner. Incremental projection leaves those
/// edges to existing word-time hints; a retired receipt has replaced them.
pub(in crate::mandoc::inline::flow) fn split_retired_native_field_passes(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> Vec<Inline> {
    split_native_field_passes_mode(nodes, boundaries, &|_, _| true).0
}

/// An accepted pass to project in the active formatter word.
pub(super) struct RecordedRowSplit {
    pub(super) offset: usize,
    pub(super) pass_end: usize,
    /// The following native scalar already exists in this word but its
    /// cached zero-advance glyph cannot appear in the current fragment.
    pub(super) before_cached_glyph: bool,
}

pub(super) fn split_recorded_native_passes(
    nodes: &[Inline],
    splits: BTreeMap<String, Vec<RecordedRowSplit>>,
    buffer: &mut super::super::field_buffer::FieldBuffer,
) -> Vec<Inline> {
    let mut splits = splits;
    let mut positions = BTreeMap::new();
    for (owner, splits) in &mut splits {
        splits.sort_unstable_by_key(|split| (split.offset, split.pass_end));
        positions.insert(
            owner.clone(),
            splits.iter().map(|split| split.offset).collect(),
        );
    }
    let (output, projected) = split_native_field_passes_mode(nodes, &positions, &|owner, index| {
        splits[owner][index].before_cached_glyph
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
    boundaries: &BTreeMap<String, Vec<usize>>,
    can_project_tail: &dyn Fn(&str, usize) -> bool,
) -> (Vec<Inline>, OwnerPassPositions) {
    let mut ordered = boundaries.clone();
    for positions in ordered.values_mut() {
        positions.sort_unstable();
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
    let mut output = project_native_pass_nodes(nodes, &ordered, &mut cursor);
    project_owner_tail_events(&mut cursor, &mut output);
    if let Some(owner) = cursor.owner {
        cursor
            .positions
            .insert(owner, (cursor.scalar, cursor.next_boundary));
    }
    trim_native_pass_rows(&mut output);
    (output, cursor.positions)
}

struct NativePassCursor<'a> {
    boundaries: &'a [usize],
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
    while cursor.boundaries.get(cursor.next_boundary) == Some(&cursor.scalar)
        && cursor
            .owner
            .as_deref()
            .is_some_and(|owner| (cursor.can_project_tail)(owner, cursor.next_boundary))
    {
        output.push(Inline::line_break());
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
    boundaries: &'a BTreeMap<String, Vec<usize>>,
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
            while cursor.boundaries.get(cursor.next_boundary) == Some(&cursor.scalar) {
                output.push(Inline::line_break());
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
            _ => false,
        };
        if starts_row {
            trim_native_breakable_tail(&mut output);
        }
        output.push(node);
    }
    *nodes = output;
}

fn starts_with_native_break(nodes: &[Inline]) -> bool {
    first_native_row_event(nodes) == Some(true)
}

fn first_native_row_event(nodes: &[Inline]) -> Option<bool> {
    for node in nodes {
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

fn trim_native_breakable_tail(nodes: &mut Vec<Inline>) -> bool {
    let mut metadata = Vec::new();
    let mut exhausted = true;
    while let Some(mut node) = nodes.pop() {
        if native_word_marker(&node).is_some() || matches!(node, Inline::Anchor { .. }) {
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
