//! Project accepted native field passes without executing formatter state.

use super::super::Inline;
use super::split::{advance_boundary, split_text_at_boundaries};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::mandoc::inline::flow) struct OwnerAcceptance {
    pub(in crate::mandoc::inline::flow) scalars: usize,
    pub(in crate::mandoc::inline::flow) native_cells: bool,
    /// The first accepted interval printed this word's deferred device pad.
    /// An accepted NBRZW prefix alone cannot print it (term.c:389-427).
    pub(in crate::mandoc::inline::flow) prefix_printed: bool,
}

/// Accepted projection range of each stable source word. A delayed glyph
/// can re-enter that owner after a later word marker without becoming its
/// rejected suffix. Native cell intervals, not append order, decide limits.
pub(in crate::mandoc::inline::flow) fn accepted_owner_lengths(
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
) -> BTreeMap<String, OwnerAcceptance> {
    let mut lengths = BTreeMap::new();
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
            if start < accepted_end {
                acceptance.scalars += buffer.projection_length(start, accepted_end);
                acceptance.native_cells = true;
                if start == *content {
                    acceptance.prefix_printed =
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
    buffer: &super::super::field_buffer::FieldBuffer,
    anchors: &[super::super::NativeWordAnchor],
    passes: &[super::super::field_buffer::FillPass],
    projected_passes: usize,
    output_start: usize,
) -> usize {
    use super::super::field_buffer::{FieldCell, FillBoundary};

    let first = projected_passes.max(buffer.committed_pass_count());
    let count = passes.len().saturating_sub(1);
    if first >= count {
        return count;
    }
    let start = output_start.min(nodes.len());
    let mut boundaries: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, pass) in passes.iter().take(count).enumerate() {
        let mut cell = pass.end;
        while matches!(buffer.cells().get(cell), Some(FieldCell::BreakableBlank)) {
            cell += 1;
        }
        let represented = buffer.has_projected_pass(pass.end);
        if index < first || represented || pass.boundary == FillBoundary::Hyphen {
            // An encoded ASCII_HYPH candidate locates the final device row
            // for BODY. The responsive reading projection keeps that source
            // word complete rather than hardening its discretionary split
            // (term.c:307-324); authored \p events remain independent.
            continue;
        }
        let owner = anchors.partition_point(|anchor| anchor.content <= cell);
        if let Some(anchor) = owner.checked_sub(1).and_then(|index| anchors.get(index)) {
            boundaries
                .entry(anchor.owner.clone())
                .or_default()
                .push(buffer.projection_length(anchor.content, cell));
        }
    }
    if !boundaries.is_empty() {
        // Earlier flushed fields are immutable output. Only the active
        // suffix is traversed, once per real flush (term.c:233-237).
        let pending = nodes.split_off(start);
        nodes.extend(split_native_field_passes(&pending, &boundaries));
    }
    count
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
pub(in crate::mandoc::inline::flow) fn split_native_field_passes(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> Vec<Inline> {
    split_native_field_passes_mode(nodes, boundaries, false)
}

/// Rebuild one complete active native interval, including loop events at the
/// last accepted scalar of an owner. Incremental projection leaves those
/// edges to existing word-time hints; a retired receipt has replaced them.
pub(in crate::mandoc::inline::flow) fn split_retired_native_field_passes(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
) -> Vec<Inline> {
    split_native_field_passes_mode(nodes, boundaries, true)
}

fn split_native_field_passes_mode(
    nodes: &[Inline],
    boundaries: &BTreeMap<String, Vec<usize>>,
    complete: bool,
) -> Vec<Inline> {
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
        complete,
    };
    let mut output = project_native_pass_nodes(nodes, &ordered, &mut cursor);
    if complete {
        project_owner_tail_events(&mut cursor, &mut output);
    }
    trim_native_pass_rows(&mut output);
    output
}

struct NativePassCursor<'a> {
    boundaries: &'a [usize],
    scalar: usize,
    next_boundary: usize,
    owner: Option<String>,
    positions: BTreeMap<String, (usize, usize)>,
    complete: bool,
}

/// `term_flushln()`217 emits the accepted pass's endline even if its final
/// graph is the final scalar before a rejected suffix (term_fill()299-312).
/// Equal scalar positions can still carry separate physical row events.
fn project_owner_tail_events(cursor: &mut NativePassCursor<'_>, output: &mut Vec<Inline>) {
    while cursor.boundaries.get(cursor.next_boundary) == Some(&cursor.scalar) {
        output.push(Inline::line_break());
        cursor.next_boundary += 1;
    }
}

fn native_word_marker(node: &Inline) -> Option<&str> {
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
            if cursor.complete && cursor.owner.as_deref() != Some(marker) {
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
