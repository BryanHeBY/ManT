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
        fn visit_heading(&mut self, heading: &'ir mant_ir::Heading) {
            let previous = self.source_offset;
            self.source_offset = heading
                .source
                .and_then(|source| source.byte_range)
                .map(|range| range.start.get());
            mant_ir::visit::walk_heading(self, heading);
            self.source_offset = previous;
        }
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
    fn heading_references_keep_source_order_without_metadata_or_body_duplicates() {
        let query = crate::query_markdown_text("# [Catalog](index.md)\n\n[before](before.md)\n\n## [Topic](topic.md)\n\n[after](after.md)\n", None).unwrap();
        let references = document_references(&query);
        let names = references
            .iter()
            .map(|reference| match &reference.target {
                SemanticDocumentReference::Document { name, .. } => name.as_str(),
                SemanticDocumentReference::Manual { .. } => panic!("Markdown reference"),
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["index", "before", "topic", "after"]);
        assert!(
            references
                .windows(2)
                .all(|pair| pair[0].source_offset < pair[1].source_offset)
        );
    }

    #[test]
    fn ordinary_item_domains_follow_earlier_head_and_body_links() {
        let mut query = crate::query_markdown_text(
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
        let document = query.document.as_mut().unwrap();
        let blocks = std::mem::take(&mut document.blocks);
        document.blocks = vec![
            serde_json::from_value(serde_json::json!({
                "type": "definition-list", "items": [{"terms": [], "description": [{
                    "type": "table", "rows": [{"cells": [{"blocks": blocks}]}]
                }]}]
            }))
            .unwrap(),
        ];
        let nested = document_references(&query);
        assert_eq!(nested.len(), references.len());
        for (nested, original) in nested.iter().zip(&references) {
            assert_eq!(nested.target, original.target);
        }
    }
}
