//! Materialize accepted row events in the source owners' scalar order.

use std::collections::BTreeMap;

use crate::mandoc::inline::{
    is_formatter_word_blank, links::presentation::retains_authored_identity,
};

use super::super::super::Inline;
use super::super::split::{PassBoundary, advance_boundary, split_text_at_boundaries};
use super::super::{INTERNAL_FIELD_PREFIX, INTERNAL_FIELD_WORD};

#[cfg(test)]
std::thread_local! {
    pub(super) static OWNER_NODES_VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Project the final native pass receipt once, in output-owner order. The
/// receipt supplies scalar offsets inside each formatter word; semantic
/// wrappers do not begin another word or reset its cursor. No formatter
/// register changes here: term.c:205-220 already chose the accepted ranges.
#[cfg(test)]
pub(super) fn split_native_field_passes(
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

type OwnerPassPositions = BTreeMap<String, (usize, usize)>;

pub(super) fn split_native_field_passes_mode(
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
    marker.starts_with(INTERNAL_FIELD_WORD).then_some(marker)
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
        if matches!(node, Inline::Anchor { id, .. } if id.as_str().starts_with(INTERNAL_FIELD_PREFIX))
        {
            // term_field() adds positioning before printing the word.
            // Its generated prefix does not advance the owner's scalar
            // cursor; the following native word marker resumes that cursor.
            project_owner_tail_events(cursor, &mut output);
            if let Some(owner) = cursor.owner.take() {
                cursor
                    .positions
                    .insert(owner, (cursor.scalar, cursor.next_boundary));
            }
            cursor.boundaries = &[];
            output.push(node.clone());
            continue;
        }
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

pub(in crate::mandoc::inline::flow::output) fn starts_with_native_break(nodes: &[Inline]) -> bool {
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

pub(in crate::mandoc::inline::flow::output) fn trim_native_breakable_tail(
    nodes: &mut Vec<Inline>,
) -> bool {
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
        let identity = retains_authored_identity(&node);
        let consumed = match &mut node {
            Inline::Text { value } | Inline::Code { value } if !value.is_empty() => {
                while value
                    .chars()
                    .next_back()
                    .is_some_and(is_formatter_word_blank)
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
