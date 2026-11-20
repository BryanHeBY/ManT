//! Bracket evidence retained by the original accepted Ar fragments.

use crate::definitions::{NativeOperand, NativeOperandRole};
use std::ops::Range;

pub(in crate::definitions::syntax::native_subject) struct ParameterEdges {
    edges: Vec<Edge>,
    absorbed: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Edge {
    offset: usize,
    glyph: char,
    suffix: bool,
    enclosure: Option<u32>,
}

impl ParameterEdges {
    pub(in crate::definitions::syntax::native_subject) fn new(
        text: &str,
        span: Range<usize>,
        operands: &[NativeOperand],
    ) -> Self {
        let mut edges = Vec::new();
        let mut absorbed = Vec::new();
        let mut opens = Vec::<Edge>::new();
        let mut previous = None;
        for word in operands
            .iter()
            .skip_while(|word| word.bytes.end <= span.start)
            .take_while(|word| word.bytes.start < span.end)
        {
            let NativeOperandRole::Placeholder { node, enclosure } = word.role else {
                if let NativeOperandRole::LiteralDelimiter { glyph, enclosure } = word.role
                    && previous.is_some_and(|(_, scope, end)| {
                        scope == enclosure && end == word.bytes.start
                    })
                    && opens
                        .last()
                        .is_some_and(|open| !open.suffix && closes(open.glyph, glyph))
                {
                    // in_line leaves the detached closer outside Ar, but
                    // NODE_DELIMC can still attach it to the same display
                    // word. This pair is internal to that parameter.
                    opens.pop();
                    absorbed.push(word.bytes.start);
                    previous = previous.map(|(node, scope, _)| (node, scope, word.bytes.end));
                    continue;
                }
                finish_parameter(&mut edges, &mut opens);
                previous = None;
                continue;
            };
            let start = word.bytes.start.max(span.start);
            let end = word.bytes.end.min(span.end);
            if !previous.is_some_and(|(last, scope, end)| {
                scope == enclosure
                    && text.get(end..start).is_some_and(|gap| {
                        gap.is_empty()
                            || last == node && gap.bytes().all(|b| matches!(b, b' ' | b'\t'))
                    })
            }) {
                finish_parameter(&mut edges, &mut opens);
            }
            previous = Some((node, enclosure, end));
            collect_brackets(&text[start..end], start, enclosure, &mut opens, &mut edges);
        }
        finish_parameter(&mut edges, &mut opens);
        edges.sort_unstable_by_key(|edge| edge.offset);
        Self { edges, absorbed }
    }

    pub(in crate::definitions::syntax::native_subject) fn exposes(
        &self,
        offset: usize,
        enclosure: Option<u32>,
        closer: Option<char>,
    ) -> bool {
        self.edges
            .binary_search_by_key(&offset, |edge| edge.offset)
            .is_ok_and(|i| {
                let edge = self.edges[i];
                edge.enclosure == enclosure && (edge.suffix || closer == Some(edge.glyph))
            })
    }

    pub(in crate::definitions::syntax::native_subject) fn absorbs(&self, offset: usize) -> bool {
        self.absorbed.binary_search(&offset).is_ok()
    }

    pub(super) fn within(&self, range: Range<usize>) -> impl Iterator<Item = (usize, char)> + '_ {
        let first = self.edges.partition_point(|edge| edge.offset < range.start);
        self.edges[first..]
            .iter()
            .take_while(move |edge| edge.offset < range.end)
            .map(|edge| (edge.offset, edge.glyph))
    }
}

fn closes(left: char, right: char) -> bool {
    matches!((left, right), ('(', ')') | ('[', ']'))
}

fn collect_brackets(
    text: &str,
    mut start: usize,
    enclosure: Option<u32>,
    opens: &mut Vec<Edge>,
    edges: &mut Vec<Edge>,
) {
    // mdoc in_line may store several displayed words in one quoted TEXT.
    // Their lexical edges are the same as those of separate TEXT children.
    for fragment in text.split_inclusive([' ', '\t', '\n']) {
        let word = fragment.trim_ascii_end();
        let suffix = word.trim_end_matches(['(', ')', '[', ']']).len();
        for (at, glyph) in word.match_indices(['(', ')', '[', ']']) {
            let glyph = char::from(glyph.as_bytes()[0]);
            let edge = Edge {
                offset: start + at,
                glyph,
                suffix: at >= suffix,
                enclosure,
            };
            if matches!(glyph, '(' | '[') {
                opens.push(edge);
            } else {
                let open = if glyph == ')' { '(' } else { '[' };
                if opens.last().is_some_and(|earlier| earlier.glyph == open) {
                    opens.pop();
                } else {
                    edges.push(edge);
                }
            }
        }
        start += fragment.len();
    }
}

fn finish_parameter(edges: &mut Vec<Edge>, opens: &mut Vec<Edge>) {
    // Pair markup inside one Ar (possibly split by in_line), including
    // bracketed names joined tightly by Ns. Any remaining edge retains its
    // own fragment's boundary. An interior opening byte in a name does not
    // start an invocation group.
    edges.extend(opens.drain(..).filter(|edge| edge.suffix));
}
