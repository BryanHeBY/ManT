//! Restore authored blanks only where the accepted field printed them.

use super::super::field_buffer::{DeferredSourceBlank, FieldBuffer, FillPass};
use super::super::{Font, Inline};
use crate::mandoc::inline::font::style::styled_segment;
use std::collections::BTreeMap;

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
    for mut node in std::mem::take(nodes) {
        if let Inline::Anchor { id, .. } = &node
            && let Some((font, count)) = printed.remove(id.as_str())
        {
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
            if let Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } = &mut node
            {
                resolve(children, printed);
            }
            output.push(node);
        }
    }
    output.append(&mut next_row);
    output.extend(pending.into_iter().map(|(node, _)| node));
    *nodes = output;
}
