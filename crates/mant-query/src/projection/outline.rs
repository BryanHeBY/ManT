//! Outline DTOs and relationship projection of already indexed semantic nodes.
use super::{ProjectionError, TLDR_TITLE, content_complete, semantics_complete};
use crate::{
    ResolvedContent,
    selectors::{DOCUMENT_ROOT_TITLE, TLDR_ID},
};

mod rooted;
use mant_ir::{
    DOCUMENT_ROOT_ID, EntryKindCount, EntrySummary, OutlinePath, Section, SemanticEntry,
    SemanticIndex,
};
use mant_protocol::{
    ContentSelector, EntryDocumentTarget, EntryProjection, EntryValueDomain, OutlineDetail,
    OutlineNode, OutlineSchema, QueryOutline,
};

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
    root: Option<ContentSelector>,
) -> Result<QueryOutline, ProjectionError> {
    build_outline_with_references(
        query,
        entries,
        root,
        &mant_protocol::ReferenceProjection::default(),
    )
}

/// Project exact local content and independently scan its real references.
///
/// # Errors
/// Rejects invalid selectors, reference bounds, or unavailable selected content.
pub fn build_outline_with_references(
    query: &ResolvedContent,
    entries: EntryProjection,
    root: Option<ContentSelector>,
    reference_policy: &mant_protocol::ReferenceProjection,
) -> Result<QueryOutline, ProjectionError> {
    validate_outline_request(query, root.as_ref(), reference_policy)?;
    let diagnostics = query
        .document
        .as_ref()
        .map_or_else(Vec::new, |document| document.diagnostics.clone());
    let semantics_complete = semantics_complete(&diagnostics);
    let content_complete = content_complete(&diagnostics);
    let (nodes, references) = if let Some(selector) = root.as_ref() {
        let selected = rooted::RootedOutline::resolve(query, selector)?;
        (
            vec![selected.project(query, &entries)],
            selected.references(query, reference_policy),
        )
    } else {
        let references = query.document.as_ref().map_or_else(
            || mant_protocol::ReferenceInventory::not_scanned(reference_policy.clone()),
            |document| {
                super::project_references(
                    document,
                    query.address.as_ref(),
                    mant_ir::ReferenceScope::Document,
                    reference_policy,
                )
            },
        );
        (project_document(query, &entries), references)
    };
    Ok(QueryOutline {
        references,
        display_title: query
            .document
            .as_ref()
            .and_then(mant_ir::Document::display_title)
            .map(std::borrow::Cow::into_owned),
        schema: OutlineSchema::V0Dot14,
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
        content_complete,
        nodes,
    })
}

fn validate_outline_request(
    query: &ResolvedContent,
    root: Option<&ContentSelector>,
    policy: &mant_protocol::ReferenceProjection,
) -> Result<(), ProjectionError> {
    policy
        .validate()
        .map_err(ProjectionError::InvalidReferenceProjection)?;
    if let Some(selector) = root {
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

fn tldr_node() -> OutlineNode {
    OutlineNode::Tldr {
        path: OutlinePath::Tldr.to_string().into(),
        id: TLDR_ID.into(),
        title: TLDR_TITLE.to_owned(),
    }
}

fn overview_node(entry_summary: Option<EntrySummary>, children: Vec<OutlineNode>) -> OutlineNode {
    OutlineNode::DocumentRoot {
        path: OutlinePath::DocumentRoot.to_string().into(),
        id: DOCUMENT_ROOT_ID.into(),
        title: DOCUMENT_ROOT_TITLE.to_owned(),
        entry_summary,
        children,
    }
}

fn project_document(query: &ResolvedContent, entries: &EntryProjection) -> Vec<OutlineNode> {
    let mut nodes = Vec::new();
    if query.tldr.is_some() && !matches!(entries, EntryProjection::Kinds { .. }) {
        nodes.push(tldr_node());
    }
    if let Some(manual) = &query.document {
        // Compact outlines inspect borrowed facts only. Forms and names are
        // materialized exclusively when entry rows are actually requested.
        let index = (!matches!(entries, EntryProjection::None | EntryProjection::Summary))
            .then(|| SemanticIndex::build(manual));
        if manual.heading.is_some() || !manual.blocks.is_empty() {
            let root_entries = index.as_ref().map_or(&[][..], SemanticIndex::root);
            let children = project_entries(
                root_entries,
                None,
                &[],
                entries,
                query.address.as_ref(),
                &|path| index.as_ref()?.owner_at(path).cloned(),
            );
            if !matches!(entries, EntryProjection::Kinds { .. }) || !children.is_empty() {
                nodes.push(overview_node(
                    if index.is_some() {
                        projected_summary(root_entries, entries)
                    } else {
                        borrowed_summary(&manual.blocks, entries)
                    },
                    children,
                ));
            }
        }
        nodes.extend(outline_nodes(
            &manual.sections,
            &[],
            index.as_ref(),
            entries,
            query.address.as_ref(),
        ));
    }
    nodes
}

fn outline_nodes(
    sections: &[Section],
    parent: &[usize],
    index: Option<&SemanticIndex>,
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
            let semantic_entries = index.map_or(&[][..], |index| {
                let source_path = coordinates
                    .iter()
                    .map(|coordinate| coordinate - 1)
                    .collect::<Vec<_>>();
                index.section_at(&source_path)
            });
            let mut children = project_entries(
                semantic_entries,
                Some(&coordinates),
                &[],
                entries,
                current_address,
                &|path| index?.owner_at(path).cloned(),
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
                title: section.heading.single_line_text(),
                entry_summary: if index.is_some() {
                    projected_summary(semantic_entries, entries)
                } else {
                    borrowed_summary(&section.blocks, entries)
                },
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

fn borrowed_summary(
    blocks: &[mant_ir::Block],
    projection: &EntryProjection,
) -> Option<EntrySummary> {
    fn collect(blocks: &[mant_ir::Block], direct: bool, summary: &mut EntrySummary) {
        mant_ir::visit_child_entries(blocks, &mut |owner| {
            if let Some(facts) = owner.facts() {
                record_projected_summary(
                    summary,
                    facts.kind,
                    owner.validated_form_count().unwrap_or(0),
                    direct,
                );
                collect(owner.blocks(), false, summary);
            }
        });
    }
    if matches!(projection, EntryProjection::None) {
        return None;
    }
    let mut summary = EntrySummary::default();
    collect(blocks, true, &mut summary);
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
    owner_at: &dyn Fn(&OutlinePath) -> Option<mant_ir::ContentReveal>,
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
                owner_at,
            );
            let selected = match projection {
                EntryProjection::All => true,
                EntryProjection::Kinds { kinds } => kinds.contains(&entry.kind),
                EntryProjection::None | EntryProjection::Summary => false,
            };
            if !selected && children.is_empty() {
                return None;
            }
            let path = OutlinePath::nested_entry(section, &coordinates)?;
            let owner = owner_at(&path)?;
            Some(entry_node(
                entry,
                &path,
                owner,
                current_address,
                projected_summary(&entry.children, projection),
                children,
            ))
        })
        .collect()
}

fn entry_node(
    entry: &SemanticEntry,
    path: &OutlinePath,
    owner: mant_ir::ContentReveal,
    current_address: Option<&mant_ir::DocumentAddress>,
    entry_summary: Option<EntrySummary>,
    children: Vec<OutlineNode>,
) -> OutlineNode {
    let title = mant_protocol::entry_label(
        mant_protocol::EntryLabelMode::Forms,
        &entry.id,
        &entry.names,
        &entry.forms,
    );
    OutlineNode::DocumentEntry {
        owner: Box::new(owner),
        path: path.to_string().into(),
        id: entry.id.clone(),
        title,
        entry_kind: entry.kind,
        case: entry.case,
        names: entry.names.clone(),
        alias_groups: entry.alias_groups.clone(),
        alias_of: entry.alias_of.clone(),
        forms: entry.forms.clone(),
        document_targets: entry
            .document_targets
            .iter()
            .map(|target| EntryDocumentTarget {
                label: target.label.clone(),
                reference: target.reference.clone(),
                address: match target.reference {
                    // A term citation is a query, even with a section.
                    // Only catalog resolution can select its address.
                    mant_ir::DocumentReference::Manual { .. } => None,
                    mant_ir::DocumentReference::Document { .. } => {
                        current_address.and_then(|address| target.reference.resolve_from(address))
                    }
                },
            })
            .collect(),
        value_domain: entry
            .value_domain
            .as_ref()
            .map(|domain| Box::new(project_value_domain(domain, current_address))),
        entry_summary,
        children,
    }
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
