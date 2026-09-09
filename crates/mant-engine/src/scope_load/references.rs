//! Scope references: preserve request-local ownership and source order.
use std::{collections::BTreeMap, ops::ControlFlow};

use mant_ir::{
    NavigationEvent, NavigationScanOptions, ReferenceLinkFilter, ReferenceScanLimits,
    ReferenceScanReport, ReferenceScope, scan_navigation_scope,
};
use mant_protocol::ReferencePageLimit;

use super::{
    DocumentAddress, DocumentEdgeKind, DocumentReference, DocumentSelector, ResolvedContent,
};

const MAX_REFERENCES: usize = 4096;
const MAX_REFERENCE_BYTES: usize = 1024 * 1024;

pub(super) struct ScopeReferences {
    pub(super) references: Vec<ScopeReference>,
    pub(super) report: ReferenceScanReport,
    pub(super) retention_limit: Option<ReferencePageLimit>,
}

/// Scope follows documents, not fragments or individual occurrences. Borrowed
/// keys bound retention before cloning and do not discard visible occurrences
/// from the independent reference inventory.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ScopeTarget<'a> {
    Document(&'a str),
    Manual(&'a str, Option<&'a str>),
}

impl ScopeTarget<'_> {
    fn bytes(self) -> usize {
        match self {
            Self::Document(name) => name.len(),
            Self::Manual(name, section) => name.len().saturating_add(section.map_or(0, str::len)),
        }
    }

    fn owned(self) -> DocumentReference {
        match self {
            Self::Document(name) => DocumentReference::Document {
                name: name.to_owned(),
                fragment: None,
            },
            Self::Manual(name, section) => DocumentReference::Manual {
                name: name.to_owned(),
                manual_section: section.map(str::to_owned),
            },
        }
    }
}

#[derive(Clone)]
pub(super) struct ScopeReference {
    pub(super) target: DocumentReference,
    pub(super) kind: DocumentEdgeKind,
    pub(super) source_offset: Option<u32>,
    pub(super) sequence: usize,
}

impl ScopeReference {
    pub(super) fn exact_address(&self, from: &DocumentAddress) -> Option<DocumentAddress> {
        self.target.resolve_from(from)
    }

    pub(super) fn selector(&self, from: &DocumentAddress) -> Option<DocumentSelector> {
        match &self.target {
            DocumentReference::Document { name, .. } => {
                let address = from.resolve_document_reference(name)?;
                Some(DocumentSelector {
                    selector: address.catalog_path(),
                    source: None,
                    manual_section: None,
                })
            }
            DocumentReference::Manual {
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
            DocumentReference::Document { name, .. } | DocumentReference::Manual { name, .. } => {
                name.clone()
            }
        };
        DocumentSelector {
            selector,
            source: None,
            manual_section: None,
        }
    }
}

pub(super) fn document_references(bundle: &ResolvedContent) -> ScopeReferences {
    collect_references(
        bundle,
        ReferenceScanLimits::default(),
        MAX_REFERENCES,
        MAX_REFERENCE_BYTES,
    )
}

fn collect_references(
    bundle: &ResolvedContent,
    limits: ReferenceScanLimits,
    max_records: usize,
    max_bytes: usize,
) -> ScopeReferences {
    let mut retained = BTreeMap::<ScopeTarget<'_>, (Option<u32>, usize)>::new();
    let mut bytes = 0usize;
    let mut sequence = 0;
    let mut retention_limit = None;
    let report = bundle
        .document
        .as_ref()
        .map_or_else(ReferenceScanReport::default, |document| {
            scan_navigation_scope(
                document,
                ReferenceScope::Document,
                limits,
                NavigationScanOptions {
                    links: ReferenceLinkFilter::DOCUMENTS,
                    targets: false,
                    entry_sets: true,
                },
                |event, budget| {
                    let Some((target, source)) = source_target(event) else {
                        return ControlFlow::Continue(());
                    };
                    // Pay for bounded tree comparisons and the eventual owned copy,
                    // even for duplicate edges. No reference string is cloned here.
                    if budget
                        .consume(0, 1, target.bytes().saturating_mul(16))
                        .is_err()
                    {
                        return ControlFlow::Break(());
                    }
                    let position = (
                        source
                            .and_then(|span| span.byte_range)
                            .map(|range| range.start.get()),
                        sequence,
                    );
                    sequence += 1;
                    if let Some(previous) = retained.get_mut(&target) {
                        if (position.0.unwrap_or(u32::MAX), position.1)
                            < (previous.0.unwrap_or(u32::MAX), previous.1)
                        {
                            *previous = position;
                        }
                        return ControlFlow::Continue(());
                    }
                    if retained.len() >= max_records {
                        retention_limit = Some(ReferencePageLimit::Records);
                        return ControlFlow::Break(());
                    }
                    if target.bytes() > max_bytes.saturating_sub(bytes) {
                        retention_limit = Some(ReferencePageLimit::MaterializationBytes);
                        return ControlFlow::Break(());
                    }
                    bytes += target.bytes();
                    retained.insert(target, position);
                    ControlFlow::Continue(())
                },
            )
        });
    let mut references: Vec<_> = retained
        .into_iter()
        .map(|(target, (source_offset, sequence))| {
            let target = target.owned();
            ScopeReference {
                kind: reference_edge_kind(&target),
                target,
                source_offset,
                sequence,
            }
        })
        .collect();
    references.sort_by_key(|reference| {
        (
            reference.source_offset.unwrap_or(u32::MAX),
            reference.sequence,
        )
    });
    ScopeReferences {
        references,
        report,
        retention_limit,
    }
}

fn source_target<'ir>(
    event: NavigationEvent<'ir, '_>,
) -> Option<(ScopeTarget<'ir>, Option<mant_ir::SourceSpan>)> {
    match event {
        NavigationEvent::Link(link) => {
            let target = match link.target {
                mant_ir::LinkTarget::Document { name, .. } => ScopeTarget::Document(name),
                mant_ir::LinkTarget::Manual {
                    name,
                    manual_section,
                } => ScopeTarget::Manual(name, manual_section.as_deref()),
                _ => return None,
            };
            Some((target, link.source))
        }
        NavigationEvent::EntrySet(relation) => {
            // Rejected public IR relationships must never become scope I/O.
            if !relation.reference.is_well_formed() {
                return None;
            }
            let target = match relation.reference {
                DocumentReference::Document { name, .. } => ScopeTarget::Document(name),
                DocumentReference::Manual {
                    name,
                    manual_section,
                } => ScopeTarget::Manual(name, manual_section.as_deref()),
            };
            Some((target, relation.source))
        }
        NavigationEvent::Target(_) => None,
    }
}

const fn reference_edge_kind(reference: &DocumentReference) -> DocumentEdgeKind {
    match reference {
        DocumentReference::Document { .. } => DocumentEdgeKind::Document,
        DocumentReference::Manual { .. } => DocumentEdgeKind::Manual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::Block;

    #[test]
    fn heading_references_keep_source_order_without_metadata_or_body_duplicates() {
        let query = crate::scope_load::tests::markdown_content("# [Catalog](index.md)\n\n[before](before.md)\n\n## [Topic](topic.md)\n\n[after](after.md)\n", None).unwrap();
        let references = document_references(&query).references;
        let names = references
            .iter()
            .map(|reference| match &reference.target {
                DocumentReference::Document { name, .. } => name.as_str(),
                DocumentReference::Manual { .. } => panic!("Markdown reference"),
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
        let mut query = crate::scope_load::tests::markdown_content(
            "# Tools\n\n<!-- mant:entries role=command case=sensitive -->\n- [`target`](target.md): See [body](body.md).\n\n  <!-- mant:domain entries=domain.md roles=command -->\n", None,
        ).unwrap();
        assert!(
            query.document.as_ref().unwrap().blocks.iter().any(
                |block| matches!(block, Block::List { items, .. } if items[0].entry.is_some())
            )
        );
        let references = document_references(&query).references;
        assert_eq!(references.len(), 3);
        for (reference, expected) in references.iter().zip(["target", "body", "domain"]) {
            assert!(
                matches!(&reference.target, DocumentReference::Document { name, .. } if name == expected)
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
        let nested = document_references(&query).references;
        assert_eq!(nested.len(), references.len());
        for (nested, original) in nested.iter().zip(&references) {
            assert_eq!(nested.target, original.target);
        }
    }

    #[test]
    fn repeated_fragments_do_not_consume_distinct_document_capacity() {
        let query = crate::scope_load::tests::markdown_content(
            "# Links\n\n[a](one.md#a) [b](one.md#b) [next](two.md) [excluded](three.md)\n",
            None,
        )
        .unwrap();
        let result = collect_references(&query, ReferenceScanLimits::default(), 2, 1024);
        assert_eq!(result.references.len(), 2);
        assert_eq!(
            result.references[0].target,
            DocumentReference::Document {
                name: "one".into(),
                fragment: None
            }
        );
        assert_eq!(
            result.references[1].target,
            DocumentReference::Document {
                name: "two".into(),
                fragment: None
            }
        );
        assert_eq!(result.retention_limit, Some(ReferencePageLimit::Records));
        assert!(!result.report.complete());
        // Inventory still observes all four real links; scope address dedup is
        // not a mutation of content or a shared occurrence-set collapse.
        let document = query.document.as_ref().unwrap();
        let mut occurrences = 0;
        mant_ir::scan_references(document, ReferenceScanLimits::default(), |_| {
            occurrences += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(occurrences, 4);
    }

    #[test]
    fn scope_reference_bounds_stop_before_retaining_oversized_targets() {
        let query =
            crate::scope_load::tests::markdown_content("# Links\n\n[x](oversized.md)\n", None)
                .unwrap();
        let result = collect_references(&query, ReferenceScanLimits::default(), 2, 3);
        assert!(result.references.is_empty());
        assert_eq!(
            result.retention_limit,
            Some(ReferencePageLimit::MaterializationBytes)
        );
        assert!(!result.report.complete());
        let result = collect_references(
            &query,
            ReferenceScanLimits {
                steps: 1,
                ..ReferenceScanLimits::default()
            },
            2,
            1024,
        );
        assert!(result.references.is_empty());
        assert_eq!(
            result.report.stopped,
            Some(mant_ir::ReferenceScanStop::Steps)
        );
    }

    #[test]
    fn scope_does_not_inspect_excluded_external_payloads_or_form_metadata() {
        let mut query = crate::scope_load::tests::markdown_content(
            "# Links\n\n[remote](https://example.org) [local](local.md)\n",
            None,
        )
        .unwrap();
        let Block::Paragraph { children, .. } = &mut query.document.as_mut().unwrap().blocks[0]
        else {
            panic!("paragraph")
        };
        let mant_ir::Inline::Link {
            target: mant_ir::LinkTarget::External { uri },
            ..
        } = &mut children[0]
        else {
            panic!("external")
        };
        *uri = "x".repeat(2 * 1024 * 1024);
        let result = collect_references(
            &query,
            ReferenceScanLimits {
                bytes: 1024,
                ..ReferenceScanLimits::default()
            },
            2,
            1024,
        );
        assert!(result.report.complete());
        assert_eq!(result.references.len(), 1);
    }
}
