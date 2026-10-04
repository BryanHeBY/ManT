//! Read-only native logical sibling lookup, distinct from container routing.
use libmandoc_rs::{
    MacroToken::{Man, Mdoc, Roff},
    ManMacro, MdocMacro, Node, NodeKind, RoffMacro,
};

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
