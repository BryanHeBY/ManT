//! Excerpt DTOs for source-ordered selections resolved by shared selector policy.
use super::{ProjectionError, TLDR_TITLE, semantics_complete};
use crate::{
    ResolvedContent,
    selectors::{
        DOCUMENT_ROOT_TITLE, DocumentSelectorIndex, LocatedBreadcrumb, LocatedNode, TLDR_ID,
        collect_root_entries, collect_sections,
    },
};
use mant_ir::{DOCUMENT_ROOT_ID, OutlinePath};
use mant_protocol::{
    ExcerptSchema, ExcerptSelection, OutlineNodeReference, OutlineReference, OutlineTrail,
    QueryExcerpt,
};
use std::collections::HashSet;

/// Select tldr, document-root content, or complete section subtrees by path or ID.
///
/// Duplicate selections and descendants of another selected node are omitted.
/// The result always follows source order, independent of argument order.
///
/// # Errors
///
/// Returns an error when no content exists or any selector is empty or unknown.
pub fn select_excerpt<S: AsRef<str>>(
    query: &ResolvedContent,
    selectors: &[S],
) -> Result<QueryExcerpt, ProjectionError> {
    if selectors.is_empty() {
        return Err(ProjectionError::EmptySelection);
    }
    if query.tldr.is_none() && query.document.is_none() {
        return Err(ProjectionError::MissingContent {
            document: query.label.clone(),
        });
    }
    let mut located = Vec::new();
    if let Some(manual) = &query.document {
        collect_root_entries(&manual.blocks, &mut located);
        collect_sections(&manual.sections, &[], &[], &mut located);
    }
    let index = DocumentSelectorIndex::new(&located);

    let (tldr_selected, document_root_selected, mut selected) =
        resolve_excerpt_candidates(query, selectors, &index)?;
    let selected_sections = selected
        .iter()
        .filter(|candidate| candidate.is_section())
        .map(|candidate| candidate.coordinates().to_vec())
        .collect::<Vec<_>>();
    selected.retain(|candidate| {
        if document_root_selected && candidate.path().is_document_root_entry() {
            return false;
        }
        !selected_sections.iter().any(|ancestor| {
            if candidate.is_section() {
                ancestor != candidate.coordinates()
                    && is_ancestor(ancestor, candidate.coordinates())
            } else {
                ancestor == candidate.coordinates()
                    || is_ancestor(ancestor, candidate.coordinates())
            }
        })
    });
    selected.sort_by_key(|candidate| candidate.order());

    let document = if selected.is_empty() && !document_root_selected {
        None
    } else {
        query.document.as_ref()
    };
    let mut selections = Vec::new();
    if let (true, Some(document)) = (tldr_selected, query.tldr.clone()) {
        selections.push(ExcerptSelection::Tldr {
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::Tldr {
                    path: OutlinePath::Tldr.to_string().into(),
                    id: TLDR_ID.into(),
                    title: TLDR_TITLE.to_owned(),
                },
            },
            document,
        });
    }
    if let (true, Some(document)) = (document_root_selected, query.document.as_ref()) {
        selections.push(ExcerptSelection::DocumentRoot {
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::DocumentRoot {
                    path: OutlinePath::DocumentRoot.to_string().into(),
                    id: DOCUMENT_ROOT_ID.into(),
                    title: DOCUMENT_ROOT_TITLE.to_owned(),
                },
            },
            blocks: document.blocks.clone(),
        });
    }
    selections.extend(selected.into_iter().map(LocatedNode::selection));

    Ok(QueryExcerpt {
        schema: ExcerptSchema::V0Dot11,
        label: query.label.clone(),
        address: query.address.clone(),
        semantics_complete: document
            .is_none_or(|document| semantics_complete(&document.diagnostics)),
        producer: document.map(mant_protocol::Producer::for_document),
        source: document.map(|document| document.source.clone()),
        meta: document.map(|document| document.meta.clone()),
        diagnostics: document
            .map(|document| document.diagnostics.clone())
            .unwrap_or_default(),
        selections,
    })
}

fn resolve_excerpt_candidates<'a, S: AsRef<str>>(
    query: &ResolvedContent,
    selectors: &[S],
    index: &DocumentSelectorIndex<'a>,
) -> Result<(bool, bool, Vec<&'a LocatedNode<'a>>), ProjectionError> {
    let mut tldr_selected = false;
    let mut document_root_selected = false;
    let mut selected_ids = HashSet::new();
    let mut selected = Vec::new();
    for raw_selector in selectors {
        let selector = raw_selector.as_ref().trim();
        if selector.is_empty() {
            return Err(ProjectionError::EmptySelector);
        }
        if (selector == TLDR_ID || selector.parse() == Ok(OutlinePath::Tldr))
            && query.tldr.is_some()
        {
            tldr_selected = true;
            continue;
        }
        if (selector == DOCUMENT_ROOT_ID || selector.parse() == Ok(OutlinePath::DocumentRoot))
            && query
                .document
                .as_ref()
                .is_some_and(|document| !document.blocks.is_empty())
        {
            document_root_selected = true;
            continue;
        }
        let candidate = index.resolve(&query.label, selector)?;
        if selected_ids.insert(candidate.id()) {
            selected.push(candidate);
        }
    }
    Ok((tldr_selected, document_root_selected, selected))
}

impl LocatedNode<'_> {
    pub(crate) fn selection(&self) -> ExcerptSelection {
        match self {
            Self::Section {
                path,
                breadcrumbs,
                section,
                ..
            } => ExcerptSelection::DocumentSection {
                outline: OutlineTrail {
                    ancestors: project_breadcrumbs(breadcrumbs),
                    node: OutlineNodeReference::DocumentSection {
                        path: path.to_string().into(),
                        id: section.id.clone(),
                        title: section.title.clone(),
                    },
                },
                section: (*section).clone(),
            },
            Self::Entry {
                path,
                title,
                breadcrumbs,
                entry,
                ..
            } => ExcerptSelection::DocumentEntry {
                outline: OutlineTrail {
                    ancestors: project_breadcrumbs(breadcrumbs),
                    node: {
                        let identity = entry.item.facts().expect("located entries have identities");
                        OutlineNodeReference::DocumentEntry {
                            path: path.to_string().into(),
                            id: identity.id.clone(),
                            title: title.clone(),
                            role: identity.role,
                            case: identity.case,
                            names: identity.names.clone(),
                        }
                    },
                },
                entry: entry.content(),
            },
        }
    }
}

pub(crate) fn project_breadcrumbs(breadcrumbs: &[LocatedBreadcrumb]) -> Vec<OutlineReference> {
    breadcrumbs
        .iter()
        .map(|breadcrumb| OutlineReference {
            path: breadcrumb.path.to_string().into(),
            id: breadcrumb.id.clone(),
            title: breadcrumb.title.clone(),
        })
        .collect()
}

fn is_ancestor(ancestor: &[usize], descendant: &[usize]) -> bool {
    ancestor.len() < descendant.len() && descendant.starts_with(ancestor)
}
