//! Source sibling boundaries, recorded before formatter/normalization flattening.
use crate::definitions::NativeHeadEvidence;
use libmandoc_rs::{Node, NodeKind};

pub(super) fn record(root: &Node, evidence: &mut NativeHeadEvidence, source: Option<&str>) {
    // libmandoc can discard an empty explicit paragraph. Keep those source
    // boundaries independently of visible AST children; prefix lookup avoids
    // rescanning the source for every adjacent declaration.
    let barriers = source
        .into_iter()
        .flat_map(str::lines)
        .enumerate()
        .filter_map(|(i, line)| {
            let request = line
                .strip_prefix('.')
                .or_else(|| line.strip_prefix('\''))?
                .split_whitespace()
                .next()?;
            matches!(
                request,
                "PP" | "P"
                    | "LP"
                    | "Pp"
                    | "sp"
                    | "br"
                    | "RS"
                    | "RE"
                    | "Bd"
                    | "Ed"
                    | "TS"
                    | "TE"
                    | "SH"
                    | "SS"
                    | "Sh"
                    | "Ss"
            )
            .then(|| u32::try_from(i + 1).ok())
            .flatten()
        })
        .collect::<std::collections::BTreeSet<_>>();
    record_children(root, evidence, &barriers);
}

fn record_children(
    root: &Node,
    evidence: &mut NativeHeadEvidence,
    barriers: &std::collections::BTreeSet<u32>,
) {
    fn declaration(node: &Node) -> bool {
        node.kind == NodeKind::Block
            && matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "It"))
    }
    fn transparent(node: &Node) -> bool {
        matches!(node.macro_name.as_deref(), Some("PD" | "Sm" | "Tg" | "ft"))
    }
    fn empty_paragraph_boundary(node: &Node) -> bool {
        node.children.iter().any(|child| {
            matches!(
                child.macro_name.as_deref(),
                Some("PP" | "P" | "LP" | "Pp" | "sp")
            ) || (child.kind == NodeKind::Body && empty_paragraph_boundary(child))
        })
    }
    let mut previous: Option<&Node> = None;
    for node in &root.children {
        if declaration(node) {
            if let Some(left) = previous
                && left.line < node.line
                && barriers
                    .range((
                        std::ops::Bound::Excluded(left.line),
                        std::ops::Bound::Excluded(node.line),
                    ))
                    .next()
                    .is_none()
            {
                evidence.groups.adjacent(
                    std::ptr::from_ref(left) as usize,
                    std::ptr::from_ref(node) as usize,
                );
            }
            previous = (!empty_paragraph_boundary(node)).then_some(node);
        } else if !transparent(node) {
            previous = None;
        }
        record_children(node, evidence, barriers);
    }
}
