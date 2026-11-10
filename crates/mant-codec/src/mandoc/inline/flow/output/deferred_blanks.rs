//! Restore authored blanks only where the accepted field printed them.

use super::super::field_buffer::{DeferredSourceBlank, FieldBuffer, FillPass};
use super::super::{Font, Inline};
use crate::mandoc::inline::font::style::styled_segment;
use std::collections::BTreeMap;

const PROJECTED_BLANK: &str = "\0mant:output-scope:projected-blank:";

pub(super) fn projected_blank_count(node: &Inline) -> Option<usize> {
    let Inline::Anchor { id, .. } = node else {
        return None;
    };
    id.as_str().strip_prefix(PROJECTED_BLANK)?.parse().ok()
}

pub(in crate::mandoc::inline::flow) fn project(
    nodes: &mut Vec<Inline>,
    buffer: &FieldBuffer,
    passes: &[FillPass],
    start: usize,
) {
    if !buffer.has_deferred_blanks() {
        return;
    }
    let receipts = buffer.printed_deferred_blanks(passes);
    project_from(nodes, start, &receipts);
}

pub(in crate::mandoc::inline::flow) fn project_from(
    nodes: &mut Vec<Inline>,
    start: usize,
    receipts: &[(DeferredSourceBlank, usize)],
) {
    if !receipts.is_empty() {
        let mut suffix = nodes.split_off(start.min(nodes.len()));
        apply(std::slice::from_mut(&mut suffix), receipts);
        nodes.extend(suffix);
    }
}

pub(in crate::mandoc::inline::flow) fn project_unmodeled(
    nodes: &mut Vec<Inline>,
    buffer: &FieldBuffer,
    start: usize,
) {
    project_from(nodes, start, &buffer.conservative_deferred_blanks());
}

pub(in crate::mandoc::inline::flow) fn apply(
    terms: &mut [Vec<Inline>],
    receipts: &[(DeferredSourceBlank, usize)],
) {
    let mut printed = BTreeMap::new();
    for &(blank, count) in receipts {
        let entry = printed.entry(blank.marker()).or_insert((blank.font, 0));
        entry.1 += count;
    }
    for term in terms {
        resolve(term, &mut printed);
    }
}

fn resolve(nodes: &mut Vec<Inline>, printed: &mut BTreeMap<String, (Font, usize)>) {
    if printed.is_empty() {
        return;
    }
    let mut output = Vec::with_capacity(nodes.len());
    let mut pending = Vec::new();
    let mut next_row = Vec::new();
    let mut input = std::mem::take(nodes).into_iter();
    while let Some(mut node) = input.next() {
        if let Inline::Anchor { id, .. } = &node
            && let Some((font, count)) = printed.remove(id.as_str())
        {
            // This printed blank is outside the source-word scalar stream:
            // the native buffer recorded its cell without a projection slot.
            // Keep that fact beside the text for later layout receipts.
            pending.push((Inline::anchor(format!("{PROJECTED_BLANK}{count}")), true));
            pending.push((styled_segment(" ".repeat(count), font), true));
        } else if matches!(node, Inline::LineBreak { .. }) {
            // A zero-scalar marker can precede an already projected
            // boundary at the same source offset. Its printed graph
            // belongs to the following row. Move only confirmed blanks,
            // keeping the existing break and owner metadata in order.
            for (pending, is_blank) in pending.drain(..) {
                if is_blank {
                    next_row.push(pending);
                } else {
                    output.push(pending);
                }
            }
            output.push(node);
        } else if super::is_private_output_marker(&node)
            && (!pending.is_empty() || !next_row.is_empty())
        {
            pending.push((node, false));
        } else {
            output.append(&mut next_row);
            output.extend(pending.drain(..).map(|(node, _)| node));
            let mut trailing_metadata = Vec::new();
            if let Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } = &mut node
            {
                // A native pass can close immediately outside the source
                // annotation that owns its following printed blank. Carry
                // that same endline into the wrapper so the blank advances
                // on the next row (term.c:205-220 and 389-427).
                if !printed.is_empty() && trailing_printed_blank(children, printed) == Some(true) {
                    let metadata = input
                        .as_slice()
                        .iter()
                        .take_while(|node| super::native_passes::native_word_marker(node).is_some())
                        .count();
                    if matches!(
                        input.as_slice().get(metadata),
                        Some(Inline::LineBreak { .. })
                    ) {
                        trailing_metadata.extend(input.by_ref().take(metadata));
                        children.extend(input.by_ref().take(1));
                    }
                }
                resolve(children, printed);
            }
            output.push(node);
            output.extend(trailing_metadata);
        }
    }
    output.append(&mut next_row);
    output.extend(pending.into_iter().map(|(node, _)| node));
    *nodes = output;
}

fn trailing_printed_blank(
    nodes: &[Inline],
    printed: &BTreeMap<String, (Font, usize)>,
) -> Option<bool> {
    nodes.iter().rev().find_map(|node| match node {
        Inline::Anchor { id, .. } if printed.contains_key(id.as_str()) => Some(true),
        _ if super::native_passes::native_word_marker(node).is_some() => None,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => trailing_printed_blank(children, printed),
        _ => Some(false),
    })
}
