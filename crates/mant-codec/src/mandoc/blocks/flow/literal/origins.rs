//! Accepted physical-row origins become existing per-block layout hints.

use mant_ir::{Block, Inline, LayoutHint, SourceSpan};

struct Segment {
    origin: usize,
    nodes: Vec<Inline>,
}

pub(super) fn literal_blocks(
    nodes: Vec<Inline>,
    layout: LayoutHint,
    source: Option<SourceSpan>,
) -> Vec<Block> {
    let (segments, _) = split_origins(nodes, 0);
    segments
        .into_iter()
        .filter_map(|mut segment| {
            crate::mandoc::inline::strip_native_projection_markers(&mut segment.nodes);
            let inline_layout = crate::mandoc::inline::take_inline_layout(&mut segment.nodes);
            if segment.nodes.is_empty() {
                return None;
            }
            let mut layout = layout;
            layout.indent_columns = layout
                .indent_columns
                .saturating_add(i32::try_from(segment.origin).unwrap_or(i32::MAX));
            Some(Block::Preformatted {
                inline_layout,
                children: segment.nodes,
                language: None,
                layout,
                source,
            })
        })
        .collect()
}

fn split_origins(nodes: Vec<Inline>, inherited: usize) -> (Vec<Segment>, usize) {
    let mut segments = vec![Segment {
        origin: inherited,
        nodes: Vec::new(),
    }];
    let mut origin = inherited;
    for mut node in nodes {
        if let Some(next) = crate::mandoc::inline::native_row_origin(&node) {
            origin = next;
            append_segment(
                &mut segments,
                Segment {
                    origin,
                    nodes: Vec::new(),
                },
            );
            continue;
        }
        let children = match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => Some(std::mem::take(children)),
            _ => None,
        };
        if let Some(children) = children {
            let (pieces, last) = split_origins(children, origin);
            let mut first = true;
            for piece in pieces {
                if !first && matches!(node, Inline::Link { .. }) {
                    append_segment(&mut segments, piece);
                    continue;
                }
                let mut wrapped = node.clone();
                match &mut wrapped {
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => {
                        *children = piece.nodes;
                    }
                    // A semantic owner remains one identity. Later physical
                    // rows retain its accepted glyphs without inventing a
                    // second authored link or portable fallback.
                    _ => unreachable!("only transparent inline containers have children"),
                }
                append_segment(
                    &mut segments,
                    Segment {
                        origin: piece.origin,
                        nodes: vec![wrapped],
                    },
                );
                first = false;
            }
            origin = last;
        } else {
            segments
                .last_mut()
                .expect("open origin segment")
                .nodes
                .push(node);
        }
    }
    (segments, origin)
}

fn append_segment(segments: &mut Vec<Segment>, mut incoming: Segment) {
    let previous = segments.last_mut().expect("open origin segment");
    if previous.origin == incoming.origin {
        previous.nodes.append(&mut incoming.nodes);
    } else if !mant_ir::geometry::has_literal_rows(&previous.nodes) {
        previous.origin = incoming.origin;
        previous.nodes.append(&mut incoming.nodes);
    } else {
        // A new actually printed origin starts a native physical row. The
        // block join owns its delimiter once, rather than adding a second
        // newline to the existing inline row close.
        crate::mandoc::inline::consume_one_row_ending(&mut previous.nodes);
        segments.push(incoming);
    }
}
