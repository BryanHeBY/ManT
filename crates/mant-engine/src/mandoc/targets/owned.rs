//! Keep target spelling and its chosen native owner together while pending.
use super::{Block, Inline, LayoutHint, SourceSpan, attach_targets};

#[derive(Clone, Debug)]
pub(in crate::mandoc) struct OwnedTarget {
    pub(in crate::mandoc) name: String,
    pub(in crate::mandoc) owner_source: Option<SourceSpan>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        pending.queue(["first".into(), "alias".into()], source(3));
        pending.queue(["first".into(), "last".into()], source(9));
        let mut blocks = Vec::new();
        pending.attach_leading(&mut blocks, LayoutHint::default());

        assert!(pending.is_empty());
        assert_eq!(
            blocks,
            [Block::Paragraph {
                children: vec![
                    Inline::anchor_at("first", source(3)),
                    Inline::anchor_at("alias", source(3)),
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
    pub(in crate::mandoc) fn new(name: String, owner_source: Option<SourceSpan>) -> Self {
        Self { name, owner_source }
    }

    pub(in crate::mandoc) fn into_inline(self) -> Inline {
        Inline::anchor_at(self.name, self.owner_source)
    }
}

/// A batch has one native owner. Preserve batches rather than flattening them:
/// fallback block provenance and reverse-prepend ordering are observable.
struct TargetBatch {
    names: Vec<String>,
    owner_source: Option<SourceSpan>,
}

#[derive(Default)]
pub(in crate::mandoc) struct PendingTargets {
    batches: Vec<TargetBatch>,
}

impl PendingTargets {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            batches: Vec::new(),
        }
    }

    pub(in crate::mandoc) fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    pub(in crate::mandoc) fn queue(
        &mut self,
        targets: impl IntoIterator<Item = String>,
        owner_source: Option<SourceSpan>,
    ) {
        let names = targets
            .into_iter()
            .filter(|name| !self.batches.iter().any(|batch| batch.names.contains(name)))
            .collect::<Vec<_>>();
        if !names.is_empty() {
            self.batches.push(TargetBatch {
                names,
                owner_source,
            });
        }
    }

    pub(in crate::mandoc) fn attach_leading(
        &mut self,
        blocks: &mut Vec<Block>,
        layout: LayoutHint,
    ) {
        // Each call prepends. Reverse batches, not names within a batch, so
        // both source order and the existing fallback owner remain unchanged.
        for batch in std::mem::take(&mut self.batches).into_iter().rev() {
            attach_targets(blocks, batch.names, layout, batch.owner_source);
        }
    }
}
