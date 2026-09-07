//! Projects complete structured documents into outlines and selectable excerpts.

use crate::ResolvedContent;
#[cfg(test)]
use crate::selectors::semantic_selector_diagnostics;
use crate::selectors::{
    DOCUMENT_ROOT_TITLE, DocumentSelectorIndex, LocatedBreadcrumb, LocatedNode, TLDR_ID,
    collect_root_entries, collect_sections,
};
pub use crate::selectors::{ProjectionError, SelectorCandidate};
use mant_ir::{
    DOCUMENT_ROOT_ID, Diagnostic, EntryKindCount, EntrySummary, OutlinePath, Section,
    SemanticEntry, SemanticIndex,
};
use mant_protocol::{
    EntryDocumentTarget, EntryProjection, EntryValueDomain, ExcerptSchema, ExcerptSelection,
    NodeSelector, OutlineDetail, OutlineNode, OutlineNodeReference, OutlineReference,
    OutlineSchema, OutlineTrail, QueryExcerpt, QueryOutline,
};
use std::collections::HashSet;

const TLDR_TITLE: &str = "TLDR QUICK REFERENCE";

/// Build a block-free, addressable outline for one complete query.
///
/// # Errors
///
/// Returns [`ProjectionError::MissingContent`] when neither tldr nor a manual
/// is available.
pub fn build_outline(query: &ResolvedContent) -> Result<QueryOutline, ProjectionError> {
    build_outline_projection(query, EntryProjection::Summary, None)
}

/// Build an outline with optional semantic definition entries.
///
/// # Errors
///
/// Returns [`ProjectionError::MissingContent`] when neither tldr nor a manual
/// is available.
pub fn build_outline_with_detail(
    query: &ResolvedContent,
    detail: OutlineDetail,
) -> Result<QueryOutline, ProjectionError> {
    build_outline_projection(query, detail.into(), None)
}

/// Build a structural outline with an explicit semantic-entry projection.
///
/// # Errors
///
/// Returns [`ProjectionError::MissingContent`] when no content is available,
/// or [`ProjectionError::UnknownSelector`] when `root` matches no outline node.
pub fn build_outline_projection(
    query: &ResolvedContent,
    entries: EntryProjection,
    root: Option<NodeSelector>,
) -> Result<QueryOutline, ProjectionError> {
    if query.tldr.is_none() && query.document.is_none() {
        return Err(ProjectionError::MissingContent {
            document: query.label.clone(),
        });
    }
    let diagnostics = query
        .document
        .as_ref()
        .map_or_else(Vec::new, |document| document.diagnostics.clone());
    let semantics_complete = semantics_complete(&diagnostics);
    let materialized_entries = if root.is_some() {
        EntryProjection::All
    } else {
        entries.clone()
    };
    let mut nodes = Vec::new();
    if query.tldr.is_some() && !matches!(&materialized_entries, EntryProjection::Kinds { .. }) {
        nodes.push(OutlineNode::Tldr {
            path: OutlinePath::Tldr.to_string().into(),
            id: TLDR_ID.into(),
            title: TLDR_TITLE.to_owned(),
        });
    }
    if let Some(manual) = &query.document {
        let index = SemanticIndex::build(manual);
        if !manual.blocks.is_empty() {
            let root_entries = index.root();
            let children = project_entries(
                root_entries,
                None,
                &[],
                &materialized_entries,
                query.address.as_ref(),
            );
            let root = OutlineNode::DocumentRoot {
                path: OutlinePath::DocumentRoot.to_string().into(),
                id: DOCUMENT_ROOT_ID.into(),
                title: DOCUMENT_ROOT_TITLE.to_owned(),
                entry_summary: projected_summary(root_entries, &materialized_entries),
                children,
            };
            if !matches!(&materialized_entries, EntryProjection::Kinds { .. })
                || !root.children().is_empty()
            {
                nodes.push(root);
            }
        }
        nodes.extend(outline_nodes(
            &manual.sections,
            &[],
            &index,
            &materialized_entries,
            query.address.as_ref(),
        ));
    }
    if let Some(selector) = root.as_ref() {
        let mut selected = resolve_outline_root(query, &nodes, selector.as_str())?.clone();
        reproject_selected_node(&mut selected, &entries, true);
        nodes = vec![selected];
    }
    Ok(QueryOutline {
        schema: OutlineSchema::V0Dot11,
        entries,
        root,
        label: query.label.clone(),
        address: query.address.clone(),
        source: query
            .document
            .as_ref()
            .map(|document| document.source.clone()),
        meta: query
            .document
            .as_ref()
            .map(|document| document.meta.clone()),
        diagnostics,
        semantics_complete,
        nodes,
    })
}

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

fn semantics_complete(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().all(|diagnostic| {
        !diagnostic.code.as_deref().is_some_and(|code| {
            crate::markdown::is_semantic_entry_rejection_code(code)
                || code == "manual.semantic-entry.unclassified-definition"
                || mant_ir::is_semantic_completeness_diagnostic(code)
        })
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

/// Select exactly one semantic entry by stable path, ID, or alias.
///
/// Exact paths and IDs take precedence over aliases. Repeated aliases are
/// rejected with deterministic candidates instead of silently choosing the
/// first entry in source order.
///
/// # Errors
///
/// Returns an error when the selector is empty, unknown, names a section, or
/// matches more than one semantic entry.
pub fn select_explanation(
    query: &ResolvedContent,
    selector: &str,
) -> Result<QueryExcerpt, ProjectionError> {
    if query.tldr.is_none() && query.document.is_none() {
        return Err(ProjectionError::MissingContent {
            document: query.label.clone(),
        });
    }
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(ProjectionError::EmptySelector);
    }
    let mut located = Vec::new();
    if let Some(manual) = &query.document {
        collect_root_entries(&manual.blocks, &mut located);
        collect_sections(&manual.sections, &[], &[], &mut located);
    }
    let index = DocumentSelectorIndex::new(&located);
    let candidate = resolve_explanation_candidate(query, &index, selector)?;
    select_excerpt(query, &[candidate.path().to_string()])
}

fn resolve_explanation_candidate<'a>(
    query: &ResolvedContent,
    index: &DocumentSelectorIndex<'a>,
    selector: &str,
) -> Result<&'a LocatedNode<'a>, ProjectionError> {
    let selects_tldr =
        (selector == TLDR_ID || selector.parse() == Ok(OutlinePath::Tldr)) && query.tldr.is_some();
    let selects_root = (selector == DOCUMENT_ROOT_ID
        || selector.parse() == Ok(OutlinePath::DocumentRoot))
        && query
            .document
            .as_ref()
            .is_some_and(|document| !document.blocks.is_empty());
    if selects_tldr || selects_root {
        return Err(ProjectionError::ExplanationRequiresEntry {
            document: query.label.clone(),
            selector: selector.to_owned(),
        });
    }
    let candidate = index.resolve(&query.label, selector)?;
    if candidate.is_section() {
        return Err(ProjectionError::ExplanationRequiresEntry {
            document: query.label.clone(),
            selector: selector.to_owned(),
        });
    }
    Ok(candidate)
}

fn outline_nodes(
    sections: &[Section],
    parent: &[usize],
    index: &SemanticIndex,
    entries: &EntryProjection,
    current_address: Option<&mant_ir::DocumentAddress>,
) -> Vec<OutlineNode> {
    sections
        .iter()
        .enumerate()
        .filter_map(|(section_index, section)| {
            let mut coordinates = parent.to_vec();
            coordinates.push(section_index + 1);
            let path =
                OutlinePath::section(&coordinates).expect("enumerated section paths are one-based");
            let semantic_entries = index.section(&section.id);
            let mut children = project_entries(
                semantic_entries,
                Some(&coordinates),
                &[],
                entries,
                current_address,
            );
            children.extend(outline_nodes(
                &section.children,
                &coordinates,
                index,
                entries,
                current_address,
            ));
            let node = OutlineNode::DocumentSection {
                path: path.to_string().into(),
                id: section.id.clone(),
                title: section.title.clone(),
                entry_summary: projected_summary(semantic_entries, entries),
                children,
            };
            (!matches!(entries, EntryProjection::Kinds { .. }) || !node.children().is_empty())
                .then_some(node)
        })
        .collect()
}

fn projected_summary(
    entries: &[SemanticEntry],
    projection: &EntryProjection,
) -> Option<EntrySummary> {
    let summary = match projection {
        EntryProjection::None => return None,
        EntryProjection::Summary | EntryProjection::All => EntrySummary::for_entries(entries),
        EntryProjection::Kinds { kinds } => filtered_entry_summary(entries, kinds),
    };
    (!summary.is_empty()).then_some(summary)
}

fn filtered_entry_summary(entries: &[SemanticEntry], kinds: &[mant_ir::EntryKind]) -> EntrySummary {
    let mut summary = EntrySummary::default();
    for entry in entries {
        summarize_filtered_entry(entry, kinds, &mut summary, true);
    }
    summary
}

fn summarize_filtered_entry(
    entry: &SemanticEntry,
    kinds: &[mant_ir::EntryKind],
    summary: &mut EntrySummary,
    direct: bool,
) {
    if kinds.contains(&entry.kind) {
        record_projected_summary(summary, entry.kind, entry.forms.len(), direct);
    }
    for child in &entry.children {
        summarize_filtered_entry(child, kinds, summary, false);
    }
}

fn record_projected_summary(
    summary: &mut EntrySummary,
    kind: mant_ir::EntryKind,
    forms: usize,
    direct: bool,
) {
    if direct {
        summary.direct = summary.direct.saturating_add(1);
    } else {
        summary.descendants = summary.descendants.saturating_add(1);
    }
    summary.forms = summary
        .forms
        .saturating_add(u32::try_from(forms).unwrap_or(u32::MAX));
    if let Some(count) = summary.by_kind.iter_mut().find(|count| count.kind == kind) {
        count.count = count.count.saturating_add(1);
    } else {
        summary.by_kind.push(EntryKindCount { kind, count: 1 });
        summary.by_kind.sort_by_key(|count| count.kind);
    }
}

fn project_entries(
    entries: &[SemanticEntry],
    section: Option<&[usize]>,
    parent: &[usize],
    projection: &EntryProjection,
    current_address: Option<&mant_ir::DocumentAddress>,
) -> Vec<OutlineNode> {
    if matches!(projection, EntryProjection::None | EntryProjection::Summary) {
        return Vec::new();
    }
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let mut coordinates = parent.to_vec();
            coordinates.push(index + 1);
            let children = project_entries(
                &entry.children,
                section,
                &coordinates,
                projection,
                current_address,
            );
            let selected = match projection {
                EntryProjection::All => true,
                EntryProjection::Kinds { kinds } => kinds.contains(&entry.kind),
                EntryProjection::None | EntryProjection::Summary => false,
            };
            if !selected && children.is_empty() {
                return None;
            }
            let title = (!entry.forms.is_empty())
                .then(|| entry.forms.join(" | "))
                .or_else(|| entry.aliases.first().cloned())
                .unwrap_or_else(|| entry.id.to_string());
            Some(OutlineNode::DocumentEntry {
                path: OutlinePath::nested_entry(section, &coordinates)?
                    .to_string()
                    .into(),
                id: entry.id.clone(),
                title,
                entry_kind: entry.kind,
                case: entry.case,
                aliases: entry.aliases.clone(),
                forms: entry.forms.clone(),
                document_targets: entry
                    .document_targets
                    .iter()
                    .map(|target| EntryDocumentTarget {
                        label: target.label.clone(),
                        reference: target.reference.clone(),
                        address: current_address
                            .and_then(|address| target.reference.resolve_from(address)),
                    })
                    .collect(),
                value_domain: entry
                    .value_domain
                    .as_ref()
                    .map(|domain| Box::new(project_value_domain(domain, current_address))),
                entry_summary: projected_summary(&entry.children, projection),
                children,
            })
        })
        .collect()
}

fn project_value_domain(
    domain: &mant_ir::ValueDomain,
    current_address: Option<&mant_ir::DocumentAddress>,
) -> EntryValueDomain {
    match domain {
        mant_ir::ValueDomain::Choices { exhaustive } => EntryValueDomain::Choices {
            exhaustive: *exhaustive,
        },
        mant_ir::ValueDomain::EntrySet {
            reference,
            entry_kinds,
            ..
        } => EntryValueDomain::EntrySet {
            reference: reference.clone(),
            address: current_address.and_then(|address| reference.resolve_from(address)),
            entry_kinds: entry_kinds.clone(),
        },
    }
}

fn find_outline_node<'a>(
    nodes: &'a [OutlineNode],
    predicate: &impl Fn(&OutlineNode) -> bool,
) -> Option<&'a OutlineNode> {
    for node in nodes {
        if predicate(node) {
            return Some(node);
        }
        if let Some(found) = find_outline_node(node.children(), predicate) {
            return Some(found);
        }
    }
    None
}

fn resolve_outline_root<'a>(
    query: &ResolvedContent,
    nodes: &'a [OutlineNode],
    selector: &str,
) -> Result<&'a OutlineNode, ProjectionError> {
    if (selector == TLDR_ID || selector.parse() == Ok(OutlinePath::Tldr)) && query.tldr.is_some() {
        return find_outline_node(nodes, &|node| node.path() == OutlinePath::Tldr.to_string())
            .ok_or_else(|| ProjectionError::UnknownSelector {
                document: query.label.clone(),
                selector: selector.to_owned(),
            });
    }
    if (selector == DOCUMENT_ROOT_ID || selector.parse() == Ok(OutlinePath::DocumentRoot))
        && query
            .document
            .as_ref()
            .is_some_and(|document| !document.blocks.is_empty())
    {
        return find_outline_node(nodes, &|node| {
            node.path() == OutlinePath::DocumentRoot.to_string()
        })
        .ok_or_else(|| ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_owned(),
        });
    }

    let mut located = Vec::new();
    if let Some(manual) = &query.document {
        collect_root_entries(&manual.blocks, &mut located);
        collect_sections(&manual.sections, &[], &[], &mut located);
    }
    let index = DocumentSelectorIndex::new(&located);
    let path = index.resolve(&query.label, selector)?.path().to_string();
    find_outline_node(nodes, &|node| node.path() == path).ok_or_else(|| {
        ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_owned(),
        }
    })
}

fn reproject_selected_node(
    node: &mut OutlineNode,
    projection: &EntryProjection,
    keep_self: bool,
) -> bool {
    match node {
        OutlineNode::Tldr { .. } => true,
        OutlineNode::DocumentRoot {
            entry_summary,
            children,
            ..
        }
        | OutlineNode::DocumentSection {
            entry_summary,
            children,
            ..
        } => {
            if matches!(projection, EntryProjection::None) {
                *entry_summary = None;
            }
            children.retain_mut(|child| reproject_selected_node(child, projection, false));
            if let EntryProjection::Kinds { kinds } = projection {
                *entry_summary = projected_outline_summary(children, kinds);
            }
            true
        }
        OutlineNode::DocumentEntry {
            entry_kind,
            entry_summary,
            children,
            ..
        } => {
            if matches!(projection, EntryProjection::None) {
                *entry_summary = None;
            }
            if matches!(projection, EntryProjection::None | EntryProjection::Summary) {
                children.clear();
            } else {
                children.retain_mut(|child| reproject_selected_node(child, projection, false));
            }
            if let EntryProjection::Kinds { kinds } = projection {
                *entry_summary = projected_outline_summary(children, kinds);
            }
            keep_self
                || match projection {
                    EntryProjection::All => true,
                    EntryProjection::Kinds { kinds } => {
                        kinds.contains(entry_kind) || !children.is_empty()
                    }
                    EntryProjection::None | EntryProjection::Summary => false,
                }
        }
    }
}

fn projected_outline_summary(
    nodes: &[OutlineNode],
    kinds: &[mant_ir::EntryKind],
) -> Option<EntrySummary> {
    fn visit(
        node: &OutlineNode,
        kinds: &[mant_ir::EntryKind],
        summary: &mut EntrySummary,
        direct: bool,
    ) {
        let OutlineNode::DocumentEntry {
            entry_kind,
            forms,
            children,
            ..
        } = node
        else {
            return;
        };
        if kinds.contains(entry_kind) {
            record_projected_summary(summary, *entry_kind, forms.len(), direct);
        }
        for child in children {
            visit(child, kinds, summary, false);
        }
    }

    let mut summary = EntrySummary::default();
    for node in nodes {
        visit(node, kinds, &mut summary, true);
    }
    (!summary.is_empty()).then_some(summary)
}

impl LocatedNode<'_> {
    fn selection(&self) -> ExcerptSelection {
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
                        let identity = entry
                            .identity
                            .as_ref()
                            .expect("located entries have identities");
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
                entry: (*entry).clone(),
            },
        }
    }
}

fn project_breadcrumbs(breadcrumbs: &[LocatedBreadcrumb]) -> Vec<OutlineReference> {
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

#[cfg(test)]
mod tests {
    use crate::ResolvedContent;
    use mant_ir::{
        Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic,
        DiagnosticLevel, Document, DocumentMeta, DocumentSource, EntryKind, Inline, LayoutHint,
        ParameterKind, Section, SourceFormat, TldrDocument, TldrOrigin,
    };
    use mant_protocol::{EntryProjection, ExcerptSelection, NodeSelector, OutlineNode};

    use super::{
        ProjectionError, build_outline, build_outline_projection, select_excerpt,
        semantic_selector_diagnostics,
    };

    fn section(id: &str, title: &str, children: Vec<Section>) -> Section {
        Section {
            id: id.to_owned().into(),
            fragment_aliases: Vec::new(),
            title: title.to_owned(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children,
            source: None,
        }
    }

    fn query() -> ResolvedContent {
        ResolvedContent {
            address: None,
            label: "demo".to_owned(),
            document: Some(Document {
                parser: None,
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: Some("/man/demo.1".to_owned()),
                },
                meta: DocumentMeta {
                    manual_section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                fragment_aliases: Vec::new(),
                diagnostics: Vec::new(),
                blocks: Vec::new(),
                sections: vec![
                    section("name-1", "NAME", Vec::new()),
                    section(
                        "options-2",
                        "OPTIONS",
                        vec![
                            section("common-3", "Common options", Vec::new()),
                            section("other-4", "Other options", Vec::new()),
                        ],
                    ),
                    section("files-5", "FILES", Vec::new()),
                ],
            }),
            tldr: None,
        }
    }

    fn tldr() -> TldrDocument {
        TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A small demonstration.".to_owned()],
            more_information: Some("https://example.com/demo".to_owned()),
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/tldr/pages/common/demo.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }
    }

    fn definition(
        id: &str,
        role: DefinitionRole,
        aliases: &[&str],
        forms: &[&str],
        description: Vec<Block>,
    ) -> DefinitionItem {
        DefinitionItem {
            identity: Some(DefinitionIdentity {
                id: id.into(),
                role,
                case: DefinitionCase::Sensitive,
                names: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
                value_domain: None,
            }),
            terms: forms
                .iter()
                .map(|form| {
                    vec![Inline::Code {
                        value: (*form).to_owned(),
                    }]
                })
                .collect(),
            description,
            inline_term: false,
            spacing_before_lines: None,
        }
    }

    fn query_with_semantic_entries() -> ResolvedContent {
        let value = definition(
            "value-yes",
            DefinitionRole::Value,
            &["yes"],
            &["yes"],
            Vec::new(),
        );
        let local_forward = definition(
            "option-local-forward",
            DefinitionRole::Option,
            &["-L"],
            &["-L port:host:hostport", "-L socket:remote_socket"],
            vec![Block::DefinitionList {
                items: vec![value],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        let marker = definition(
            "marker-end-options",
            DefinitionRole::Marker,
            &["--"],
            &["--"],
            Vec::new(),
        );
        let mut query = query();
        query.document.as_mut().expect("document").sections[1]
            .blocks
            .push(Block::DefinitionList {
                items: vec![local_forward, marker],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            });
        query
    }

    #[test]
    fn indexed_selector_diagnostics_preserve_case_policy_and_deduplicate_aliases() {
        let sensitive = definition(
            "command-sensitive-mode",
            DefinitionRole::Command,
            &["Mode"],
            &["Mode"],
            Vec::new(),
        );
        let mut insensitive = definition(
            "command-insensitive-mode",
            DefinitionRole::Command,
            &["MODE", "mode"],
            &["MODE"],
            Vec::new(),
        );
        insensitive.identity.as_mut().expect("identity").case = DefinitionCase::Insensitive;
        let blocks = vec![Block::DefinitionList {
            items: vec![sensitive, insensitive],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }];
        let sections = vec![section("mode", "Mode", Vec::new())];

        let diagnostics = semantic_selector_diagnostics(&blocks, &sections, "manual");
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.code.as_deref() == Some("manual.semantic-entry.ambiguous-selector")
                })
                .count(),
            1
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.semantic-entry.shadowed-selector")
                && diagnostic.message.contains("semantic selector 'mode'")
                && diagnostic
                    .message
                    .matches("command-insensitive-mode")
                    .count()
                    == 1
        }));
    }

    #[test]
    fn builds_one_based_tree_paths_without_copying_blocks() {
        let outline = build_outline(&query()).expect("outline");

        assert_eq!(
            outline
                .meta
                .as_ref()
                .and_then(|meta| meta.manual_section.as_deref()),
            Some("1")
        );
        assert_eq!(outline.nodes[1].path(), "2");
        assert_eq!(outline.nodes[1].id(), "options-2");
        assert_eq!(outline.nodes[1].children()[0].path(), "2.1");
        assert_eq!(outline.nodes[1].children()[1].path(), "2.2");
    }

    #[test]
    fn default_outline_summarizes_entries_without_materializing_them() {
        let outline = build_outline(&query_with_semantic_entries()).expect("summary outline");
        let OutlineNode::DocumentSection {
            entry_summary,
            children,
            ..
        } = &outline.nodes[1]
        else {
            panic!("expected options section");
        };
        let summary = entry_summary.as_ref().expect("non-empty entry summary");
        assert_eq!(
            (summary.direct, summary.descendants, summary.forms),
            (2, 1, 4)
        );
        assert!(
            children
                .iter()
                .all(|child| !matches!(child, OutlineNode::DocumentEntry { .. }))
        );
        assert!(matches!(
            &outline.nodes[0],
            OutlineNode::DocumentSection {
                entry_summary: None,
                ..
            }
        ));
    }

    #[test]
    fn full_and_filtered_outlines_preserve_forms_nesting_and_paths() {
        let query = query_with_semantic_entries();
        let full = build_outline_projection(&query, EntryProjection::All, None)
            .expect("full semantic outline");
        let OutlineNode::DocumentEntry {
            path,
            title,
            forms,
            children,
            ..
        } = &full.nodes[1].children()[0]
        else {
            panic!("expected option entry");
        };
        assert_eq!(path.as_str(), "2/e1");
        assert_eq!(title, "-L port:host:hostport | -L socket:remote_socket");
        assert_eq!(forms.len(), 2);
        assert_eq!(children[0].path(), "2/e1/e1");

        let filtered = build_outline_projection(
            &query,
            EntryProjection::Kinds {
                kinds: vec![EntryKind::Value],
            },
            None,
        )
        .expect("value outline");
        assert_eq!(filtered.nodes.len(), 1);
        let option_section = filtered
            .nodes
            .iter()
            .find(|node| node.id() == "options-2")
            .expect("filtered ancestor section");
        let OutlineNode::DocumentSection {
            entry_summary: Some(summary),
            ..
        } = option_section
        else {
            panic!("filtered value summary");
        };
        assert_eq!((summary.direct, summary.descendants), (0, 1));
        assert_eq!(summary.by_kind.len(), 1);
        assert_eq!(summary.by_kind[0].kind, EntryKind::Value);
        let option = &option_section.children()[0];
        assert!(matches!(
            option,
            OutlineNode::DocumentEntry {
                entry_kind: EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option
                },
                ..
            }
        ));
        assert!(matches!(
            option.children(),
            [OutlineNode::DocumentEntry {
                entry_kind: EntryKind::Value,
                ..
            }]
        ));
    }

    #[test]
    fn kind_filter_with_no_matches_returns_an_explicitly_empty_projection() {
        let outline = build_outline_projection(
            &query_with_semantic_entries(),
            EntryProjection::Kinds {
                kinds: vec![EntryKind::EnvironmentVariable],
            },
            None,
        )
        .expect("empty environment projection");

        assert!(outline.nodes.is_empty());
    }

    #[test]
    fn every_projected_entry_path_round_trips_through_read_and_explain() {
        fn collect_entry_paths(nodes: &[OutlineNode], output: &mut Vec<String>) {
            for node in nodes {
                if matches!(node, OutlineNode::DocumentEntry { .. }) {
                    output.push(node.path().to_owned());
                }
                collect_entry_paths(node.children(), output);
            }
        }

        let mut query = query_with_semantic_entries();
        query.document.as_mut().expect("document").sections[1].children[0]
            .blocks
            .push(Block::DefinitionList {
                items: vec![definition(
                    "generic-readline-term",
                    DefinitionRole::Term,
                    &[],
                    &["operate-and-get-next (C-o)"],
                    vec![Block::Paragraph {
                        children: vec![Inline::Text {
                            value: "Accept the current line and fetch the next history entry."
                                .to_owned(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                )],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            });

        let outline = build_outline_projection(&query, EntryProjection::All, None)
            .expect("complete semantic outline");
        let mut paths = Vec::new();
        collect_entry_paths(&outline.nodes, &mut paths);
        assert_eq!(paths, ["2/e1", "2/e1/e1", "2/e2", "2.1/e1"]);

        for path in paths {
            let excerpt = select_excerpt(&query, std::slice::from_ref(&path))
                .unwrap_or_else(|error| panic!("read must accept projected path {path}: {error}"));
            assert!(matches!(
                excerpt.selections.as_slice(),
                [ExcerptSelection::DocumentEntry { outline, .. }] if outline.path() == path
            ));
            let explanation = super::select_explanation(&query, &path).unwrap_or_else(|error| {
                panic!("explain must accept projected path {path}: {error}")
            });
            assert!(matches!(
                explanation.selections.as_slice(),
                [ExcerptSelection::DocumentEntry { outline, .. }] if outline.path() == path
            ));
        }
    }

    #[test]
    fn outline_root_preserves_identity_excludes_siblings_and_rejects_ambiguous_aliases() {
        let mut query = query_with_semantic_entries();
        let section_rooted = build_outline_projection(
            &query,
            EntryProjection::All,
            Some(NodeSelector::new("options-2")),
        )
        .expect("section-rooted outline");
        let [
            OutlineNode::DocumentSection {
                path, id, children, ..
            },
        ] = section_rooted.nodes.as_slice()
        else {
            panic!("expected one rooted section");
        };
        assert_eq!(path.as_str(), "2", "rooting must not rebase paths");
        assert_eq!(id.as_str(), "options-2");
        assert!(
            children
                .iter()
                .any(|node| node.id() == "option-local-forward")
        );
        assert!(children.iter().any(|node| node.id() == "common-3"));
        assert!(children.iter().any(|node| node.id() == "other-4"));
        assert!(
            !section_rooted
                .nodes
                .iter()
                .any(|node| node.id() == "name-1" || node.id() == "files-5"),
            "unrelated siblings must not leak into a rooted projection"
        );

        let rooted = build_outline_projection(
            &query,
            EntryProjection::Summary,
            Some(NodeSelector::new("option-local-forward")),
        )
        .expect("entry-rooted outline");
        assert!(matches!(
            rooted.nodes.as_slice(),
            [OutlineNode::DocumentEntry { id, children, .. }]
                if id == "option-local-forward" && children.is_empty()
        ));

        query.document.as_mut().expect("document").sections[2]
            .blocks
            .push(Block::DefinitionList {
                items: vec![definition(
                    "option-other-local-forward",
                    DefinitionRole::Option,
                    &["-L"],
                    &["-L path"],
                    Vec::new(),
                )],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            });
        let error =
            build_outline_projection(&query, EntryProjection::All, Some(NodeSelector::new("-L")))
                .expect_err("ambiguous aliases must require qualification");
        let ProjectionError::AmbiguousSelector { candidates, .. } = error else {
            panic!("expected ambiguous selector");
        };
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].path, "2/e1");
        assert_eq!(candidates[1].path, "3/e1");
    }

    #[test]
    fn section_ids_win_consistently_before_entry_aliases() {
        let mut query = query();
        query.document.as_mut().expect("document").sections[0] = Section {
            id: "force".into(),
            fragment_aliases: Vec::new(),
            title: "Force".to_owned(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children: Vec::new(),
            source: None,
        };
        query.document.as_mut().expect("document").sections[1]
            .blocks
            .push(Block::DefinitionList {
                items: vec![definition(
                    "command-force",
                    DefinitionRole::Command,
                    &["force"],
                    &["force"],
                    Vec::new(),
                )],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            });

        let excerpt = select_excerpt(&query, &["force"]).expect("exact section ID");
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentSection { outline, .. }] if outline.path() == "1"
        ));
        assert!(matches!(
            super::select_explanation(&query, "force"),
            Err(ProjectionError::ExplanationRequiresEntry { .. })
        ));
        let outline = build_outline_projection(
            &query,
            EntryProjection::All,
            Some(NodeSelector::new("force")),
        )
        .expect("outline root uses the same exact-ID precedence");
        assert!(matches!(
            outline.nodes.as_slice(),
            [OutlineNode::DocumentSection { path, .. }] if path == "1"
        ));

        assert!(super::select_explanation(&query, "command-force").is_ok());
    }

    #[test]
    fn prepends_tldr_as_zero_without_renumbering_manual_sections() {
        let mut query = query();
        query.tldr = Some(tldr());

        let outline = build_outline(&query).expect("combined outline");

        assert!(matches!(outline.nodes[0], OutlineNode::Tldr { .. }));
        assert_eq!(outline.nodes[0].path(), "0");
        assert_eq!(outline.nodes[0].id(), "tldr");
        assert_eq!(outline.nodes[1].path(), "1");
        assert_eq!(outline.nodes[2].path(), "2");
    }

    #[test]
    fn semantic_completeness_distinguishes_rejections_from_author_warnings() {
        let mut markdown_query = query();
        {
            let document = markdown_query.document.as_mut().expect("document");
            document.diagnostics.push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("markdown.semantic-entry.ambiguous-selector".to_owned()),
                message: "author warning".to_owned(),
                source: None,
            });
        }
        assert!(
            build_outline(&markdown_query)
                .expect("complete outline")
                .semantics_complete
        );

        markdown_query
            .document
            .as_mut()
            .expect("document")
            .diagnostics
            .push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("markdown.semantic-entry-list".to_owned()),
                message: "rejected declaration".to_owned(),
                source: None,
            });
        assert!(
            !build_outline(&markdown_query)
                .expect("partial outline")
                .semantics_complete
        );

        markdown_query
            .document
            .as_mut()
            .expect("document")
            .diagnostics
            .push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("markdown.semantic-entry.invalid-entry-name".to_owned()),
                message: "rejected entry".to_owned(),
                source: None,
            });
        assert!(
            !build_outline(&markdown_query)
                .expect("partial outline")
                .semantics_complete
        );

        let mut ir_query = query();
        ir_query
            .document
            .as_mut()
            .expect("document")
            .diagnostics
            .push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("ir.invalid-semantic-document-reference".to_owned()),
                message: "invalid producer relationship".to_owned(),
                source: None,
            });
        assert!(
            !build_outline(&ir_query)
                .expect("IR-invalid outline")
                .semantics_complete
        );
        ir_query.address = Some(mant_ir::DocumentAddress::Manual {
            name: "demo".into(),
            manual_section: "1".into(),
        });
        let excerpt =
            select_excerpt(&ir_query, &["1"]).expect("excerpt with invalid producer semantics");
        assert!(!excerpt.semantics_complete);
        assert_eq!(excerpt.address, ir_query.address);
        assert!(crate::render_excerpt_text(&excerpt).contains("Semantic entries are incomplete"));
        assert!(
            crate::render_excerpt_markdown(&excerpt).contains("Semantic entries are incomplete")
        );
        // MCP strips parser findings but must not erase completeness.
        let mut compact = excerpt;
        compact.diagnostics.clear();
        assert!(
            crate::render_excerpt_markdown(&compact).contains("Semantic entries are incomplete")
        );
    }

    #[test]
    fn addresses_document_content_before_the_first_heading_as_root() {
        let mut query = query();
        let document = query.document.as_mut().expect("document");
        document.source.format = SourceFormat::Markdown;
        document.blocks.push(Block::Paragraph {
            children: vec![Inline::Text {
                value: "Document preface.".to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        });

        let outline = build_outline(&query).expect("Markdown outline");
        assert!(matches!(
            &outline.nodes[0],
            OutlineNode::DocumentRoot { path, id, title, .. }
                if path == "root" && id == "document-overview" && title == "OVERVIEW"
        ));
        // Heading paths remain stable and independent from the synthetic root.
        assert_eq!(outline.nodes[1].path(), "1");

        let excerpt = select_excerpt(&query, &["document-overview".to_owned(), "root".to_owned()])
            .expect("root excerpt");
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentRoot { outline, blocks, .. }]
                if outline.path() == "root" && blocks.len() == 1
        ));
        assert_eq!(
            excerpt.source.as_ref().map(|source| source.format),
            Some(SourceFormat::Markdown)
        );
    }

    #[test]
    fn selects_paths_or_ids_in_source_order_and_suppresses_descendant_duplicates() {
        let excerpt = select_excerpt(
            &query(),
            &[
                "files-5".to_owned(),
                "2.1".to_owned(),
                "2".to_owned(),
                "options-2".to_owned(),
            ],
        )
        .expect("excerpt");

        let paths = excerpt
            .selections
            .iter()
            .map(|selection| selection.outline().path())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["2", "3"]);
        let ExcerptSelection::DocumentSection {
            section, outline, ..
        } = &excerpt.selections[0]
        else {
            panic!("expected manual selection");
        };
        assert_eq!(section.children.len(), 2);
        assert!(outline.ancestors.is_empty());
    }

    #[test]
    fn child_selection_retains_ancestor_breadcrumbs() {
        let excerpt = select_excerpt(&query(), &["2.2".to_owned()]).expect("excerpt");

        let ExcerptSelection::DocumentSection { outline, .. } = &excerpt.selections[0] else {
            panic!("expected manual selection");
        };
        assert_eq!(outline.title(), "Other options");
        assert_eq!(outline.ancestors[0].path, "2");
        assert_eq!(outline.ancestors[0].title, "OPTIONS");
    }

    #[test]
    fn structural_paths_take_precedence_over_colliding_entry_ids() {
        let mut query = query();
        query.document.as_mut().expect("document").sections[1]
            .blocks
            .push(Block::DefinitionList {
                items: vec![DefinitionItem {
                    identity: Some(DefinitionIdentity {
                        id: "3".into(),
                        role: DefinitionRole::Option,
                        case: DefinitionCase::Sensitive,
                        names: vec!["-3".to_owned()],
                        value_domain: None,
                    }),
                    terms: vec![vec![Inline::Code {
                        value: "-3".to_owned(),
                    }]],
                    description: Vec::new(),
                    inline_term: false,
                    spacing_before_lines: None,
                }],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            });

        let excerpt = select_excerpt(&query, &["3"]).expect("section path wins");
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentSection { outline, .. }] if outline.path() == "3"
        ));
        assert!(matches!(
            super::select_explanation(&query, "3"),
            Err(ProjectionError::ExplanationRequiresEntry { .. })
        ));
    }

    #[test]
    fn selects_tldr_by_zero_or_id_and_supports_tldr_only_outlines() {
        let mut combined = query();
        combined.tldr = Some(tldr());
        let excerpt = select_excerpt(
            &combined,
            &["2".to_owned(), "tldr".to_owned(), "0".to_owned()],
        )
        .expect("combined excerpt");
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::Tldr { outline, .. }, ExcerptSelection::DocumentSection { .. }]
                if outline.path() == "0"
        ));

        let mut tldr_only = combined;
        tldr_only.document = None;
        let outline = build_outline(&tldr_only).expect("tldr-only outline");
        assert_eq!(outline.nodes.len(), 1);
        assert_eq!(outline.nodes[0].path(), "0");
        assert!(outline.source.is_none());
        assert!(outline.meta.is_none());
    }

    #[test]
    fn reports_missing_content_and_unknown_or_empty_selectors() {
        let mut empty = query();
        empty.document = None;
        assert!(matches!(
            build_outline(&empty),
            Err(ProjectionError::MissingContent { .. })
        ));
        assert_eq!(
            select_excerpt(&query(), &[] as &[String]),
            Err(ProjectionError::EmptySelection)
        );
        assert_eq!(
            select_excerpt(&query(), &[" ".to_owned()]),
            Err(ProjectionError::EmptySelector)
        );
        assert!(matches!(
            select_excerpt(&query(), &["9".to_owned()]),
            Err(ProjectionError::UnknownSelector { .. })
        ));
    }
}
