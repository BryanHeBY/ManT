//! Scope references: preserve request-local ownership and source order.
use super::{
    Block, DefinitionItem, DocumentAddress, DocumentEdgeKind, DocumentSelector, Inline,
    ResolvedContent, SemanticDocumentReference, ValueDomain, Visit, walk_block,
    walk_definition_item, walk_inline,
};

#[derive(Clone)]
pub(super) struct DocumentReference {
    pub(super) target: SemanticDocumentReference,
    pub(super) kind: DocumentEdgeKind,
    pub(super) source_offset: Option<u32>,
    pub(super) sequence: usize,
}

impl DocumentReference {
    pub(super) fn exact_address(&self, from: &DocumentAddress) -> Option<DocumentAddress> {
        self.target.resolve_from(from)
    }

    pub(super) fn selector(&self, from: &DocumentAddress) -> Option<DocumentSelector> {
        match &self.target {
            SemanticDocumentReference::Document { name, .. } => {
                let address = from.resolve_document_reference(name)?;
                Some(DocumentSelector {
                    selector: address.catalog_path(),
                    source: None,
                    manual_section: None,
                })
            }
            SemanticDocumentReference::Manual {
                name,
                manual_section,
            } => Some(DocumentSelector {
                selector: name.clone(),
                source: None,
                manual_section: manual_section.clone(),
            }),
        }
    }

    pub(super) fn fallback_selector(&self) -> DocumentSelector {
        let selector = match &self.target {
            SemanticDocumentReference::Document { name, .. }
            | SemanticDocumentReference::Manual { name, .. } => name.clone(),
        };
        DocumentSelector {
            selector,
            source: None,
            manual_section: None,
        }
    }
}

pub(super) fn document_references(bundle: &ResolvedContent) -> Vec<DocumentReference> {
    struct Collector {
        references: Vec<DocumentReference>,
        source_offset: Option<u32>,
        sequence: usize,
    }
    impl Collector {
        fn entry_domain(&mut self, owner: mant_ir::EntryOwner<'_>) {
            if let Some(ValueDomain::EntrySet {
                reference, source, ..
            }) = owner.facts().and_then(|facts| facts.value_domain.as_ref())
            {
                self.push(
                    reference.clone(),
                    reference_edge_kind(reference),
                    source
                        .and_then(|source| source.byte_range)
                        .map(|range| range.start.get()),
                );
            }
        }

        fn push(
            &mut self,
            target: SemanticDocumentReference,
            kind: DocumentEdgeKind,
            source_offset: Option<u32>,
        ) {
            self.references.push(DocumentReference {
                target,
                kind,
                source_offset,
                sequence: self.sequence,
            });
            self.sequence += 1;
        }
    }
    impl<'ir> Visit<'ir> for Collector {
        fn visit_block(&mut self, block: &'ir Block) {
            let previous = self.source_offset;
            self.source_offset = crate::block::block_source(block)
                .and_then(|source| source.byte_range)
                .map(|range| range.start.get());
            walk_block(self, block);
            self.source_offset = previous;
        }

        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link { target, .. } = inline
                && let Some(target) = SemanticDocumentReference::from_link_target(target)
            {
                let kind = reference_edge_kind(&target);
                self.push(target, kind, self.source_offset);
            }
            walk_inline(self, inline);
        }

        fn visit_definition_item(&mut self, item: &'ir DefinitionItem) {
            let previous = self.source_offset;
            self.source_offset = item
                .description
                .first()
                .and_then(crate::block::block_source)
                .and_then(|source| source.byte_range)
                .map(|range| range.start.get())
                .or(previous);
            walk_definition_item(self, item);
            self.entry_domain(mant_ir::EntryOwner::Definition(item));
            self.source_offset = previous;
        }

        fn visit_list_item(&mut self, item: &'ir mant_ir::ListItem) {
            mant_ir::visit::walk_list_item(self, item);
            self.entry_domain(mant_ir::EntryOwner::List(item));
        }
    }
    let mut collector = Collector {
        references: Vec::new(),
        source_offset: None,
        sequence: 0,
    };
    if let Some(document) = bundle.document.as_ref() {
        collector.visit_document(document);
    }
    collector.references.sort_by_key(|reference| {
        (
            reference.source_offset.unwrap_or(u32::MAX),
            reference.sequence,
        )
    });
    collector.references
}

const fn reference_edge_kind(reference: &SemanticDocumentReference) -> DocumentEdgeKind {
    match reference {
        SemanticDocumentReference::Document { .. } => DocumentEdgeKind::Document,
        SemanticDocumentReference::Manual { .. } => DocumentEdgeKind::Manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_item_domains_follow_earlier_head_and_body_links() {
        let query = crate::query_markdown_text(
            "# Tools\n\n<!-- mant:entries role=command case=sensitive -->\n- [`target`](target.md): See [body](body.md).\n\n  <!-- mant:domain entries=domain.md roles=command -->\n", None,
        ).unwrap();
        assert!(
            query.document.as_ref().unwrap().blocks.iter().any(
                |block| matches!(block, Block::List { items, .. } if items[0].entry.is_some())
            )
        );
        let references = document_references(&query);
        assert_eq!(references.len(), 3);
        for (reference, expected) in references.iter().zip(["target", "body", "domain"]) {
            assert!(
                matches!(&reference.target, SemanticDocumentReference::Document { name, .. } if name == expected)
            );
        }
    }
}
