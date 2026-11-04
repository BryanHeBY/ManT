//! Read-only native logical sibling lookup, distinct from container routing.
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, NodeKind, RoffMacro,
};

/// The original parent's sibling relations, before selecting an output sink.
/// `Pf` reads the raw successor; `Fl` and synopsis pre read logical siblings
/// using `roff.c::roff_node_prev/next` and `roff_node_transparent`.
#[derive(Clone, Copy, Default)]
pub(super) struct Siblings<'a> {
    pub(super) raw_next: Option<&'a Node>,
    pub(super) previous: Option<&'a Node>,
    pub(super) next: Option<&'a Node>,
}

/// Walk one complete parent sequence with two monotone cursors. Transparent
/// runs are scanned once, without copying siblings or searching from every
/// dispatched fragment. Recursive consumers start a new parent-local walk.
pub(super) fn with_siblings(nodes: &[Node]) -> impl Iterator<Item = (&Node, Siblings<'_>)> {
    let mut previous = None;
    let mut next_index = 0;
    nodes.iter().enumerate().map(move |(index, node)| {
        next_index = next_index.max(index + 1);
        while next_index < nodes.len() && !is_logical_sibling(&nodes[next_index]) {
            next_index += 1;
        }
        let siblings = Siblings {
            raw_next: nodes.get(index + 1),
            previous,
            next: nodes.get(next_index),
        };
        if is_logical_sibling(node) {
            previous = Some(node);
        }
        (node, siblings)
    })
}

/// Mirror mandoc's `roff_node_next` contract: skip nonprinting controls, but
/// never skip a visible scope such as Bf/Bk simply because its body is routed
/// transparently. Looking ahead must not execute font or spacing transitions.
pub(super) fn next(nodes: &[Node]) -> Option<&Node> {
    nodes.iter().find(|node| is_logical_sibling(node))
}

/// Shared next/previous boundary from `roff_node_transparent`. Existence is
/// independent of emitted IR: a font-only word or retained empty scope still
/// counts, while suppressed nodes and pure control requests do not. Scopes
/// removed by native validation (for example an empty Bk) never reach here.
pub(super) fn is_logical_sibling(node: &Node) -> bool {
    !node.flags.no_print
        && node.kind != NodeKind::Comment
        && !matches!(
            node.macro_token.as_ref(),
            Some(
                Roff(RoffMacro::Ft | RoffMacro::Ll | RoffMacro::Mc | RoffMacro::Po | RoffMacro::Ta)
                    | Mdoc(MdocMacro::Db | MdocMacro::Es | MdocMacro::Sm | MdocMacro::Tg)
                    | Man(ManMacro::Dt | ManMacro::Uc | ManMacro::Pd | ManMacro::At)
            )
        )
}
