//! Excerpt DTOs for source-ordered selections resolved by shared selector policy.
use super::{ProjectionError, TLDR_TITLE, content_complete, semantics_complete};
use crate::{
    ResolvedContent,
    selectors::{
        DOCUMENT_ROOT_TITLE, DocumentSelectorIndex, LocatedBreadcrumb, LocatedNode, TLDR_ID,
        collect_root_entries, collect_sections,
    },
};
use mant_ir::{
    AMBIGUOUS_FRAGMENT_ALIAS, DOCUMENT_ROOT_ID, Diagnostic, DiagnosticImpact, Document,
    DocumentIndex, DocumentValidation, OVERVIEW_IDENTITY_COLLISION, OutlinePath,
};
use mant_protocol::{
    ContentSelector, ExcerptSchema, ExcerptSelection, MAX_NODE_SELECTORS, OutlineNodeReference,
    OutlineReference, OutlineTrail, QueryExcerpt,
};
use std::{cell::OnceCell, collections::HashSet};

pub(super) fn selector_matches(selector: &ContentSelector, path: &OutlinePath, id: &str) -> bool {
    match selector {
        ContentSelector::Path { path: selected } => selected.as_str() == path.to_string(),
        ContentSelector::Id { id: selected } => selected.as_str() == id,
    }
}

/// Select tldr, document-root content, or complete section subtrees by path or ID.
///
/// Duplicate selections and descendants of another selected node are omitted.
/// The result always follows source order, independent of argument order.
///
/// # Errors
///
/// Returns an error when no content exists or any selector is empty or unknown.
pub fn select_excerpt(
    query: &ResolvedContent,
    selectors: &[ContentSelector],
) -> Result<QueryExcerpt, ProjectionError> {
    validate_excerpt_request(query, selectors)?;
    select_from_source(query, selectors, None)
}

/// Select and consume an excerpt together with validation of its exact source.
///
/// The callback receives the just-projected excerpt and a borrowed sidecar
/// from the same read. This operation takes original content and selectors.
///
/// # Errors
///
/// Returns the same selection errors as [`select_excerpt`].
pub fn with_selected_excerpt<R>(
    query: &ResolvedContent,
    selectors: &[ContentSelector],
    consume: impl FnOnce(&QueryExcerpt, Option<&DocumentValidation<'_>>) -> R,
) -> Result<R, ProjectionError> {
    validate_excerpt_request(query, selectors)?;
    let validation = query.document.as_ref().map(DocumentValidation::new);
    let index = validation.as_ref().map(DocumentValidation::index);
    let excerpt = select_from_source(query, selectors, index)?;
    Ok(consume(&excerpt, validation.as_ref()))
}

fn validate_excerpt_request(
    query: &ResolvedContent,
    selectors: &[ContentSelector],
) -> Result<(), ProjectionError> {
    if selectors.is_empty() {
        return Err(ProjectionError::EmptySelection);
    }
    if selectors.len() > MAX_NODE_SELECTORS {
        return Err(ProjectionError::TooManySelections {
            maximum: MAX_NODE_SELECTORS,
        });
    }
    for selector in selectors {
        selector
            .validate()
            .map_err(|_| ProjectionError::InvalidSelector)?;
    }
    if query.tldr.is_none() && query.document.is_none() {
        return Err(ProjectionError::MissingContent {
            document: query.label.clone(),
        });
    }
    Ok(())
}

fn select_from_source(
    query: &ResolvedContent,
    selectors: &[ContentSelector],
    validated_index: Option<&DocumentIndex>,
) -> Result<QueryExcerpt, ProjectionError> {
    let mut located = Vec::new();
    if let Some(manual) = &query.document {
        collect_root_entries(&manual.blocks, &mut located);
        collect_sections(&manual.sections, &[], &[], &mut located);
    }
    let index = DocumentSelectorIndex::new(&located);
    let derived = OnceCell::new();
    let source_index = || {
        validated_index.or_else(|| {
            query
                .document
                .as_ref()
                .map(|document| derived.get_or_init(|| DocumentIndex::build(document)))
        })
    };

    let (tldr_selected, document_root_selected, mut selected) =
        resolve_excerpt_candidates(query, selectors, &index, || {
            source_index().is_some_and(DocumentIndex::has_overview_conflict)
        })?;
    let selected_owners = selected.clone();
    selected.retain(|candidate| {
        if document_root_selected && candidate.path().is_document_root_entry() {
            return false;
        }
        !selected_owners
            .iter()
            .any(|ancestor| covers(ancestor.path(), candidate.path()))
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
            heading: document.heading.clone(),
            blocks: document.blocks.clone(),
        });
    }
    selections.extend(selected.into_iter().map(LocatedNode::selection));
    let diagnostics = document.map_or_else(Vec::new, |document| {
        excerpt_diagnostics(
            document,
            source_index().expect("selected document has an index"),
        )
    });

    Ok(QueryExcerpt {
        display_title: query
            .document
            .as_ref()
            .and_then(mant_ir::Document::display_title)
            .map(std::borrow::Cow::into_owned),
        schema: ExcerptSchema::V0Dot14,
        label: query.label.clone(),
        address: query.address.clone(),
        semantics_complete: semantics_complete(&diagnostics),
        content_complete: content_complete(&diagnostics),
        producer: document.map(mant_protocol::Producer::for_document),
        source: document.map(|document| document.source.clone()),
        meta: document.map(|document| document.meta.clone()),
        diagnostics,
        selections,
    })
}

fn excerpt_diagnostics(document: &Document, index: &DocumentIndex) -> Vec<Diagnostic> {
    let mut diagnostics = document.diagnostics.clone();
    // Inspect the whole source before detaching any selection. Neither the
    // selected fragments nor a custom producer's diagnostics prove that a
    // local destination has a single owner.
    let known = |code| {
        document.diagnostics.iter().any(|finding| {
            finding.code.as_deref() == Some(code)
                && finding.impact == DiagnosticImpact::SemanticCoverage
        })
    };
    if let Some(collision) = index.overview_conflict_diagnostic()
        && !known(OVERVIEW_IDENTITY_COLLISION)
    {
        diagnostics.push(collision);
    }
    if !known(AMBIGUOUS_FRAGMENT_ALIAS) {
        diagnostics.extend(index.ambiguous_fragment_diagnostics());
    }
    diagnostics
}

fn resolve_excerpt_candidates<'a>(
    query: &ResolvedContent,
    selectors: &[ContentSelector],
    index: &DocumentSelectorIndex<'a>,
    has_overview_conflict: impl Fn() -> bool,
) -> Result<(bool, bool, Vec<&'a LocatedNode<'a>>), ProjectionError> {
    let mut tldr_selected = false;
    let mut document_root_selected = false;
    let mut selected_paths = HashSet::new();
    let mut selected = Vec::new();
    for selector in selectors {
        if selector_matches(selector, &OutlinePath::Tldr, TLDR_ID) && query.tldr.is_some() {
            index.validate_synthetic_identity(&query.label, selector, TLDR_ID, "0")?;
            tldr_selected = true;
            continue;
        }
        if selector_matches(selector, &OutlinePath::DocumentRoot, DOCUMENT_ROOT_ID)
            && query
                .document
                .as_ref()
                .is_some_and(mant_ir::Document::has_overview)
        {
            index.validate_overview_identity(&query.label, selector, &has_overview_conflict)?;
            document_root_selected = true;
            continue;
        }
        let candidate = index.resolve(&query.label, selector)?;
        if selected_paths.insert(candidate.path().clone()) {
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
                        title: section.heading.single_line_text(),
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
                        let identity = entry
                            .owner()
                            .facts()
                            .expect("located entries have identities");
                        OutlineNodeReference::DocumentEntry {
                            path: path.to_string().into(),
                            id: identity.id.clone(),
                            title: title.clone(),
                            entry_kind: identity.kind,
                            case: identity.case,
                            names: entry.names().to_vec(),
                        }
                    },
                },
                entry: entry.content(),
            },
        }
    }
}

pub(crate) fn project_breadcrumbs(breadcrumbs: &[LocatedBreadcrumb]) -> Vec<OutlineReference> {
    breadcrumbs.iter().map(breadcrumb_reference).collect()
}

pub(crate) fn breadcrumb_reference(breadcrumb: &LocatedBreadcrumb) -> OutlineReference {
    OutlineReference {
        path: breadcrumb.path.to_string().into(),
        id: breadcrumb.id.clone(),
        title: breadcrumb.title.clone(),
    }
}

fn covers(ancestor: &OutlinePath, descendant: &OutlinePath) -> bool {
    match (ancestor, descendant) {
        (OutlinePath::Section(ancestor), OutlinePath::Section(descendant)) => {
            ancestor.len() < descendant.len() && descendant.starts_with(ancestor)
        }
        (
            OutlinePath::Section(ancestor),
            OutlinePath::Entry {
                section: Some(section),
                ..
            },
        ) => section.starts_with(ancestor),
        (
            OutlinePath::Entry {
                section: parent_section,
                indices: parent,
            },
            OutlinePath::Entry {
                section: child_section,
                indices: child,
            },
        ) => {
            parent_section == child_section
                && parent.len() < child.len()
                && child.starts_with(parent)
        }
        _ => false,
    }
}
