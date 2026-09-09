//! Resolve metadata only after all ordinary content owners have final IDs.
use std::collections::{BTreeMap, BTreeSet};

use super::super::bindings::{ItemBindings, OriginalItemId};
use super::{MetadataDeclaration, diagnostic, valid_id};
use mant_ir::{
    Block, Document, EntryRelationIssueKind, ListItem, SourceSpan,
    visit::{self, VisitMut},
};

pub(crate) fn apply(document: &mut Document, mut bindings: ItemBindings) -> Vec<(String, String)> {
    let mut declarations = std::mem::take(&mut bindings.metadata);
    let mut plans = BTreeMap::new();
    let mut diagnostics = Vec::new();
    visit_items(document, &bindings, &mut |offset, item| {
        let Some(declaration) = offset.and_then(|offset| declarations.remove(&offset)) else {
            return;
        };
        if let Some(facts) = &item.entry
            && offset.is_some_and(|offset| bindings.declared_items.contains(&offset))
        {
            plans.insert(facts.id.to_string(), declaration);
        } else {
            diagnostic(
                &mut diagnostics,
                declaration.source,
                "entry metadata requires a successfully recognized item in an explicitly declared list",
            );
        }
    });
    for declaration in declarations.into_values() {
        diagnostic(
            &mut diagnostics,
            declaration.source,
            "entry metadata did not resolve to a supported declared item",
        );
    }
    let index = mant_ir::DocumentIndex::build(document);
    let mut counts = BTreeMap::new();
    for value in plans.values().filter_map(|p| p.value.as_ref()) {
        if let Some(id) = &value.id {
            *counts.entry(id.clone()).or_insert(0usize) += 1;
        }
    }
    let rejected_ids = counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut sources = BTreeMap::new();
    let mut renamed = Vec::new();
    visit_items(document, &bindings, &mut |_, item| {
        let Some(facts) = &mut item.entry else {
            return;
        };
        let Some(MetadataDeclaration {
            value: Some(value),
            source,
        }) = plans.get(facts.id.as_str())
        else {
            return;
        };
        if let Some(id) = &value.id {
            if !valid_id(id)
                || rejected_ids.contains(id)
                || (index.contains(id) && id != facts.id.as_str())
            {
                diagnostic(
                    &mut diagnostics,
                    *source,
                    "explicit entry ID is invalid or duplicated; the derived identity was retained",
                );
            } else {
                renamed.push((facts.id.to_string(), id.clone()));
                facts.id = id.as_str().into();
            }
        }
        if let Some(groups) = &value.alias_groups {
            facts.alias_groups.clone_from(groups);
        }
        if let Some(target) = &value.alias_of {
            if !valid_id(target) || rejected_ids.contains(target) {
                diagnostic(
                    &mut diagnostics,
                    *source,
                    "aliasOf target is invalid or names a duplicated explicit identity; the relation was omitted",
                );
            } else {
                facts.alias_of = Some(target.as_str().into());
            }
        }
        sources.insert(facts.id.to_string(), *source);
    });
    // Reject groups atomically before validating subject eligibility for
    // aliasOf. The same source-neutral checker guards every IR producer.
    reject_relations(document, &bindings, &sources, true, &mut diagnostics);
    reject_relations(document, &bindings, &sources, false, &mut diagnostics);
    document.diagnostics.extend(diagnostics);
    renamed
}

fn reject_relations(
    document: &mut Document,
    bindings: &ItemBindings,
    sources: &BTreeMap<String, SourceSpan>,
    groups: bool,
    diagnostics: &mut Vec<mant_ir::Diagnostic>,
) {
    let issues = mant_ir::entry_relation_issues(document)
        .into_iter()
        .filter(|issue| {
            sources.contains_key(issue.owner.as_str())
                && if groups {
                    issue.kind == EntryRelationIssueKind::AliasGroups
                } else {
                    matches!(
                        issue.kind,
                        EntryRelationIssueKind::AliasOf | EntryRelationIssueKind::Cycle
                    )
                }
        })
        .collect::<Vec<_>>();
    let rejected = issues
        .iter()
        .map(|issue| issue.owner.to_string())
        .collect::<BTreeSet<_>>();
    for issue in issues {
        let mut finding = issue.diagnostic();
        finding.source = sources.get(issue.owner.as_str()).copied();
        diagnostics.push(finding);
    }
    visit_items(document, bindings, &mut |_, item| {
        if let Some(facts) = &mut item.entry
            && rejected.contains(facts.id.as_str())
        {
            if groups {
                facts.alias_groups.clear();
            } else {
                facts.alias_of = None;
            }
        }
    });
}

fn visit_items(
    document: &mut Document,
    positions: &ItemBindings,
    visitor: &mut impl FnMut(Option<OriginalItemId>, &mut ListItem),
) {
    struct Items<'a, F> {
        positions: &'a ItemBindings,
        visitor: &'a mut F,
    }
    impl<F: FnMut(Option<OriginalItemId>, &mut ListItem)> VisitMut for Items<'_, F> {
        fn visit_block_mut(&mut self, block: &mut Block) {
            if let Block::List { source, items, .. } = block {
                let positions = self.positions.items(*source);
                for (index, item) in items.iter_mut().enumerate() {
                    (self.visitor)(positions.get(index).copied(), item);
                }
            }
            visit::walk_block_mut(self, block);
        }
    }
    Items { positions, visitor }.visit_document_mut(document);
}
