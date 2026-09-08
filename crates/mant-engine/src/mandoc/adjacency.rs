//! Read-only native logical sibling lookup, distinct from container routing.
use libmandoc_rs::{Node, NodeKind};

/// Mirror mandoc's `roff_node_next` contract: skip nonprinting controls, but
/// never skip a visible scope such as Bf/Bk simply because its body is routed
/// transparently. Looking ahead must not execute font or spacing transitions.
pub(super) fn next(nodes: &[Node]) -> Option<&Node> {
    nodes.iter().find(|node| {
        !node.flags.no_print
            && node.kind != NodeKind::Comment
            && !matches!(
                node.macro_name.as_deref(),
                Some(
                    "ft" | "ll"
                        | "mc"
                        | "po"
                        | "ta"
                        | "Db"
                        | "Es"
                        | "Sm"
                        | "Tg"
                        | "DT"
                        | "UC"
                        | "PD"
                        | "AT"
                )
            )
    })
}
