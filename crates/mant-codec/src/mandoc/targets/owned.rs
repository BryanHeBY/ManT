//! Keep target spelling and its chosen native owner together while pending.
use std::collections::HashSet;

use super::{Block, Inline, LayoutHint, Node, SourceSpan, attach_owned_target};

#[derive(Clone, Debug)]
pub(in crate::mandoc) struct OwnedTarget {
    pub(in crate::mandoc) name: String,
    pub(in crate::mandoc) owner_source: Option<SourceSpan>,
    pub(in crate::mandoc) node_id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::Inline;

    #[test]
    fn pending_batches_preserve_anchor_order_and_owner_provenance() {
        let source = |line| {
            Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            })
        };
        let mut pending = PendingTargets::new();
        let target = |name: &str, owner, line| OwnedTarget::new(name.into(), source(line), owner);
        pending.queue([target("first", 1, 3), target("alias", 2, 3)]);
        pending.queue([
            target("first", 1, 3),
            target("first", 3, 3),
            target("last", 4, 9),
        ]);
        let mut blocks = Vec::new();
        pending.attach_leading(&mut blocks, LayoutHint::default());

        assert!(pending.is_empty());
        assert_eq!(
            blocks,
            [Block::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![
                    Inline::anchor_at("first", source(3)),
                    Inline::anchor_at("alias", source(3)),
                    Inline::anchor_at("first", source(3)),
                    Inline::anchor_at("last", source(9)),
                ],
                layout: LayoutHint::default(),
                // Reverse-prepend creates the fallback at the last batch;
                // prepending earlier anchors must not overwrite that source.
                source: source(9),
            }]
        );
        let attached = blocks.clone();
        pending.attach_leading(&mut blocks, LayoutHint::default());
        assert_eq!(blocks, attached);
    }
}

impl OwnedTarget {
    pub(in crate::mandoc) fn for_node(node: &Node, name: String) -> Self {
        Self::new(name, crate::mandoc::source_span(node), node.id)
    }

    pub(in crate::mandoc) fn new(
        name: String,
        owner_source: Option<SourceSpan>,
        node_id: u32,
    ) -> Self {
        Self {
            name,
            owner_source,
            node_id,
        }
    }

    pub(in crate::mandoc) fn into_anchor(self) -> Inline {
        Inline::anchor_at(self.name, self.owner_source)
    }
}

#[derive(Default)]
pub(in crate::mandoc) struct PendingTargets {
    targets: Vec<OwnedTarget>,
    seen: HashSet<u32>,
}

impl PendingTargets {
    pub(in crate::mandoc) fn new() -> Self {
        Self::default()
    }

    pub(in crate::mandoc) fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    pub(in crate::mandoc) fn queue(&mut self, targets: impl IntoIterator<Item = OwnedTarget>) {
        self.targets.extend(
            targets
                .into_iter()
                .filter(|target| self.seen.insert(target.node_id)),
        );
    }

    pub(in crate::mandoc) fn attach_leading(
        &mut self,
        blocks: &mut Vec<Block>,
        layout: LayoutHint,
    ) {
        // Each call prepends. Independent owners can have identical names
        // and source coordinates (e.g. two calls in a user macro expansion).
        for target in self.targets.drain(..).rev() {
            attach_owned_target(blocks, target, layout);
        }
        self.seen.clear();
    }
}
