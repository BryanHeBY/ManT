//! Source sibling boundaries, recorded before formatter/normalization flattening.
use crate::definitions::NativeHeadEvidence;
use libmandoc_rs::{Node, NodeKind};

pub(super) fn record(root: &Node, evidence: &mut NativeHeadEvidence) {
    fn declaration(node: &Node) -> bool {
        node.kind == NodeKind::Block
            && matches!(node.macro_name.as_deref(), Some("IP" | "TP" | "TQ" | "It"))
    }
    fn transparent(node: &Node) -> bool {
        matches!(node.macro_name.as_deref(), Some("PD" | "Sm" | "Tg" | "ft"))
    }
    let mut previous: Option<&Node> = None;
    for node in &root.children {
        if declaration(node) {
            if let Some(left) = previous
                && left.flow_epoch == node.flow_epoch
            {
                evidence.groups.adjacent(
                    std::ptr::from_ref(left) as usize,
                    std::ptr::from_ref(node) as usize,
                );
            }
            previous = Some(node);
        } else if !transparent(node) {
            previous = None;
        }
        record(node, evidence);
    }
}
