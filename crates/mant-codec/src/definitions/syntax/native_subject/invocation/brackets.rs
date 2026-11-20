//! Bracket evidence retained by the original accepted Ar fragments.

use crate::definitions::{NativeOperand, NativeOperandRole};
use std::ops::Range;

pub(in crate::definitions::syntax::native_subject) struct ParameterEdges(Vec<Edge>);

#[derive(Clone, Copy)]
struct Edge {
    offset: usize,
    glyph: char,
    suffix: bool,
}

impl ParameterEdges {
    pub(in crate::definitions::syntax::native_subject) fn new(
        text: &str,
        span: Range<usize>,
        operands: &[NativeOperand],
    ) -> Self {
        let mut edges = Vec::new();
        let mut opens = Vec::<Edge>::new();
        let mut previous = None;
        for word in operands
            .iter()
            .skip_while(|word| word.bytes.end <= span.start)
            .take_while(|word| word.bytes.start < span.end)
        {
            let NativeOperandRole::Placeholder(id) = word.role else {
                finish_parameter(&mut edges, &mut opens);
                previous = None;
                continue;
            };
            let start = word.bytes.start.max(span.start);
            let end = word.bytes.end.min(span.end);
            if !previous.is_some_and(|(last, end)| {
                text.get(end..start).is_some_and(|gap| {
                    gap.is_empty() || last == id && gap.bytes().all(|b| matches!(b, b' ' | b'\t'))
                })
            }) {
                finish_parameter(&mut edges, &mut opens);
            }
            previous = Some((id, end));
            let fragment = text[start..end].trim_end();
            let suffix = fragment.trim_end_matches(['(', ')', '[', ']']).len();
            for (at, glyph) in fragment.char_indices() {
                let edge = Edge {
                    offset: start + at,
                    glyph,
                    suffix: at >= suffix,
                };
                match glyph {
                    '(' | '[' => opens.push(edge),
                    ')' | ']' => {
                        let open = if glyph == ')' { '(' } else { '[' };
                        if opens.last().is_some_and(|earlier| earlier.glyph == open) {
                            opens.pop();
                        } else {
                            edges.push(edge);
                        }
                    }
                    _ => {}
                }
            }
        }
        finish_parameter(&mut edges, &mut opens);
        edges.sort_unstable_by_key(|edge| edge.offset);
        Self(edges)
    }

    pub(in crate::definitions::syntax::native_subject) fn exposes(
        &self,
        offset: usize,
        closer: Option<char>,
    ) -> bool {
        self.0
            .binary_search_by_key(&offset, |edge| edge.offset)
            .is_ok_and(|i| {
                let edge = self.0[i];
                edge.suffix || closer == Some(edge.glyph)
            })
    }

    pub(super) fn within(&self, range: Range<usize>) -> impl Iterator<Item = (usize, char)> + '_ {
        let first = self.0.partition_point(|edge| edge.offset < range.start);
        self.0[first..]
            .iter()
            .take_while(move |edge| edge.offset < range.end)
            .map(|edge| (edge.offset, edge.glyph))
    }
}

fn finish_parameter(edges: &mut Vec<Edge>, opens: &mut Vec<Edge>) {
    // Pair markup inside one Ar (possibly split by in_line), including
    // bracketed names joined tightly by Ns. Any remaining edge retains its
    // own fragment's boundary. An interior opening byte in a name does not
    // start an invocation group.
    edges.extend(opens.drain(..).filter(|edge| edge.suffix));
}
