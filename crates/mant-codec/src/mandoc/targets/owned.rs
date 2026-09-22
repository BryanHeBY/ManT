//! Keep target spelling and its chosen native owner together while pending.
use super::{Block, LayoutHint, SourceSpan, attach_targets};

#[derive(Clone, Debug)]
pub(in crate::mandoc) struct OwnedTarget {
    pub(in crate::mandoc) name: String,
    pub(in crate::mandoc) owner_source: Option<SourceSpan>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::Inline;

    #[test]
    fn pending_batches_preserve_anchor_order_and_owner_provenance() {
        let source = |line| {
            Some(SourceSpan {
                source: mant_ir::SourceKey::FIRST,
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
        let content = crate::mandoc::content::LegacyContent::default();
        pending.attach_leading(&content, &mut blocks, LayoutHint::default());

        assert!(pending.is_empty());
        let [
            Block::Paragraph {
                children,
                layout,
                source: owner,
            },
        ] = blocks.as_slice()
        else {
            panic!("one target paragraph: {blocks:?}");
        };
        assert_eq!(*layout, LayoutHint::default());
        // Reverse-prepend creates the fallback at the last batch; prepending
        // earlier anchors must not overwrite that source.
        assert_eq!(*owner, source(9));
        assert_eq!(
            children
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Anchor { id, .. } => Some(id.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            ["first", "alias", "last"]
        );
        let attached = blocks.clone();
        pending.attach_leading(&content, &mut blocks, LayoutHint::default());
        assert_eq!(blocks, attached);

        let store = content.finish();
        let view = store.content();
        let Block::Paragraph { children, .. } = &blocks[0] else {
            unreachable!()
        };
        let lines = children
            .iter()
            .filter_map(|inline| match inline {
                Inline::Anchor { point, .. } => view.point(*point),
                _ => None,
            })
            .map(|point| match point.provenance {
                mant_ir::Provenance::Authored { span } => span.line,
                _ => 0,
            })
            .collect::<Vec<_>>();
        assert_eq!(lines, [3, 3, 9]);
    }
}

impl OwnedTarget {
    pub(in crate::mandoc) fn new(name: String, owner_source: Option<SourceSpan>) -> Self {
        Self { name, owner_source }
    }

    pub(in crate::mandoc) fn into_draft(self) -> crate::mandoc::inline::DraftInline {
        crate::mandoc::inline::DraftInline::anchor_at(self.name, self.owner_source)
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
        content: &crate::mandoc::content::LegacyContent,
        blocks: &mut Vec<Block>,
        layout: LayoutHint,
    ) {
        // Each call prepends. Reverse batches, not names within a batch, so
        // both source order and the existing fallback owner remain unchanged.
        for batch in std::mem::take(&mut self.batches).into_iter().rev() {
            attach_targets(content, blocks, batch.names, layout, batch.owner_source);
        }
    }
}
