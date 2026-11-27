//! Preserve inline styles and accepted native roles in disjoint head spans.
use std::{ops::Range, rc::Rc};

use crate::definitions::{
    NativeOperand,
    evidence::{AcceptedOperands, TermOperands},
    syntax::eligibility::intersecting_operands,
};
use mant_ir::Inline;

pub(super) fn local_operands(operands: Vec<NativeOperand>) -> TermOperands {
    TermOperands::Accepted(Rc::new(AcceptedOperands {
        operands,
        bodies: Vec::new(),
    }))
}

pub(super) fn scoped_operands(
    operands: &[NativeOperand],
    bytes: Range<usize>,
) -> Vec<NativeOperand> {
    intersecting_operands(operands, bytes.clone())
        .map(|operand| NativeOperand {
            replaces_native_ink: operand.replaces_native_ink,
            bytes: operand.bytes.start.max(bytes.start) - bytes.start
                ..operand.bytes.end.min(bytes.end) - bytes.start,
            role: operand.role,
        })
        .collect()
}

struct InlineInterval {
    bytes: Range<usize>,
    content: Vec<Inline>,
}

/// Visit the source once for ordered disjoint spans, retaining transparent
/// style and link wrappers around their selected visible children.
pub(super) fn split_inline_intervals(
    nodes: &[Inline],
    ranges: impl IntoIterator<Item = Range<usize>>,
) -> Vec<Vec<Inline>> {
    let mut intervals: Vec<_> = ranges
        .into_iter()
        .map(|bytes| InlineInterval {
            bytes,
            content: Vec::new(),
        })
        .collect();
    collect_inline_intervals(nodes, &mut intervals, &mut 0, &mut 0);
    intervals.into_iter().map(|span| span.content).collect()
}

fn collect_inline_intervals(
    nodes: &[Inline],
    segments: &mut [InlineInterval],
    segment_index: &mut usize,
    offset: &mut usize,
) {
    for node in nodes {
        if let Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } = node
        {
            let first = *segment_index;
            let Some(initial) = segments.get(first).map(|segment| segment.content.len()) else {
                return;
            };
            collect_inline_intervals(children, segments, segment_index, offset);
            for (index, segment) in segments
                .iter_mut()
                .enumerate()
                .take(*segment_index + 1)
                .skip(first)
            {
                let start = if index == first { initial } else { 0 };
                if start < segment.content.len() {
                    let children = segment.content.split_off(start);
                    segment.content.push(wrap_interval(node, children));
                }
            }
            continue;
        }
        let text = match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                value
            }
            Inline::LineBreak { .. } => "\n",
            Inline::Anchor { .. } => continue,
            _ => unreachable!("the wrappers have already been traversed"),
        };
        let start = *offset;
        let end = start + text.len();
        while let Some(segment) = segments.get_mut(*segment_index) {
            if segment.bytes.start >= end {
                break;
            }
            let clip_start = segment.bytes.start.max(start);
            let clip_end = segment.bytes.end.min(end);
            if clip_start < clip_end
                && let Some(slice) = text.get(clip_start - start..clip_end - start)
            {
                segment.content.push(if slice.len() == text.len() {
                    node.clone()
                } else if matches!(node, Inline::Code { .. }) {
                    Inline::Code {
                        value: slice.into(),
                    }
                } else {
                    Inline::Text {
                        value: slice.into(),
                    }
                });
            }
            if segment.bytes.end > end {
                break;
            }
            *segment_index += 1;
        }
        *offset = end;
    }
}

fn wrap_interval(wrapper: &Inline, children: Vec<Inline>) -> Inline {
    match wrapper {
        Inline::Strong { .. } => Inline::Strong { children },
        Inline::Emphasis { .. } => Inline::Emphasis { children },
        Inline::Link { target, title, .. } => Inline::Link {
            target: target.clone(),
            title: title.clone(),
            children,
        },
        _ => unreachable!("only wrappers own child intervals"),
    }
}
