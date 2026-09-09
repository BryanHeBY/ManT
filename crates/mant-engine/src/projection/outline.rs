//! Outline DTOs and relationship projection of already indexed semantic nodes.
use super::{ProjectionError, TLDR_TITLE, semantics_complete};
use crate::{
    ResolvedContent,
    selectors::{
        DOCUMENT_ROOT_TITLE, DocumentSelectorIndex, TLDR_ID, collect_selection_root_entries,
        collect_selection_sections,
    },
};
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
    let materialized_entries =
        if root.is_some() && !matches!(entries, EntryProjection::None | EntryProjection::Summary) {
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
        // Compact outlines inspect borrowed facts only. Forms and names are
        // materialized exclusively when entry rows are actually requested.
        let index = (!matches!(
            materialized_entries,
            EntryProjection::None | EntryProjection::Summary
        ))
        .then(|| SemanticIndex::build(manual));
        if manual.heading.is_some() || !manual.blocks.is_empty() {
            let root_entries = index.as_ref().map_or(&[][..], SemanticIndex::root);
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
                entry_summary: if index.is_some() {
                    projected_summary(root_entries, &materialized_entries)
                } else {
                    borrowed_summary(&manual.blocks, &materialized_entries)
                },
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
            index.as_ref(),
            &materialized_entries,
            query.address.as_ref(),
        ));
    }
    if let Some(selector) = root.as_ref() {
        let mut selected = resolve_outline_root(query, &nodes, selector, &entries)?;
        reproject_selected_node(&mut selected, &entries, true);
        nodes = vec![selected];
    }
    Ok(QueryOutline {
        references: reference_inventory(query, root.as_ref(), reference_policy)?,
        display_title: query
            .document
            .as_ref()
            .and_then(mant_ir::Document::display_title)
            .map(std::borrow::Cow::into_owned),
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

fn reference_inventory(
    query: &ResolvedContent,
    root: Option<&ContentSelector>,
    policy: &mant_protocol::ReferenceProjection,
) -> Result<mant_protocol::ReferenceInventory, ProjectionError> {
    use mant_ir::{EntryOwnerLocationRef, ReferenceScope};
    let Some(document) = &query.document else {
        return Ok(mant_protocol::ReferenceInventory::not_scanned(
            policy.clone(),
        ));
    };
    let scan = |scope| super::project_references(document, query.address.as_ref(), scope, policy);
    let Some(root) = root else {
        return Ok(scan(ReferenceScope::Document));
    };
    if super::excerpt::selector_matches(root, &OutlinePath::Tldr, TLDR_ID) {
        return Ok(mant_protocol::ReferenceInventory::not_scanned(
            policy.clone(),
        ));
    }
    if super::excerpt::selector_matches(root, &OutlinePath::DocumentRoot, DOCUMENT_ROOT_ID) {
        return Ok(scan(ReferenceScope::Overview));
    }
    let mut located = Vec::new();
    collect_selection_root_entries(&document.blocks, &mut located);
    collect_selection_sections(&document.sections, &mut located);
    let index = DocumentSelectorIndex::new(&located);
    let candidate = index.resolve(&query.label, root)?;
    let sections = candidate
        .coordinates()
        .iter()
        .map(|value| u32::try_from(value - 1).expect("addressable section"))
        .collect::<Vec<_>>();
    Ok(match candidate {
        crate::selectors::LocatedNode::Section { .. } => scan(ReferenceScope::Section(&sections)),
        crate::selectors::LocatedNode::Entry { entry, .. } => {
            scan(ReferenceScope::Owner(EntryOwnerLocationRef {
                sections: &sections,
                blocks: &entry.block_path,
                item_index: u32::try_from(entry.item_index).expect("addressable item"),
            }))
        }
    })
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
                title: section.heading.plain_text(),
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
            let title = mant_protocol::entry_label(
                mant_protocol::EntryLabelMode::Forms,
                &entry.id,
                &entry.names,
                &entry.forms,
            );
            Some(OutlineNode::DocumentEntry {
                path: OutlinePath::nested_entry(section, &coordinates)?
                    .to_string()
                    .into(),
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

fn resolve_outline_root(
    query: &ResolvedContent,
    nodes: &[OutlineNode],
    selector: &ContentSelector,
    entries: &EntryProjection,
) -> Result<OutlineNode, ProjectionError> {
    let mut located = Vec::new();
    if let Some(manual) = &query.document {
        collect_selection_root_entries(&manual.blocks, &mut located);
        collect_selection_sections(&manual.sections, &mut located);
    }
    let index = DocumentSelectorIndex::new(&located);
    if super::excerpt::selector_matches(selector, &OutlinePath::Tldr, TLDR_ID)
        && query.tldr.is_some()
    {
        index.validate_synthetic_identity(&query.label, selector, TLDR_ID, "0")?;
        return find_outline_node(nodes, &|node| node.path() == OutlinePath::Tldr.to_string())
            .cloned()
            .ok_or_else(|| ProjectionError::UnknownSelector {
                document: query.label.clone(),
                selector: selector.to_string(),
            });
    }
    if super::excerpt::selector_matches(selector, &OutlinePath::DocumentRoot, DOCUMENT_ROOT_ID)
        && query
            .document
            .as_ref()
            .is_some_and(|document| document.heading.is_some() || !document.blocks.is_empty())
    {
        index.validate_synthetic_identity(&query.label, selector, DOCUMENT_ROOT_ID, "root")?;
        return find_outline_node(nodes, &|node| {
            node.path() == OutlinePath::DocumentRoot.to_string()
        })
        .cloned()
        .ok_or_else(|| ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_string(),
        });
    }

    let selected = index.resolve(&query.label, selector)?;
    if let Some(node) = find_outline_node(nodes, &|node| node.path() == selected.path().to_string())
    {
        return Ok(node.clone());
    }
    let crate::selectors::LocatedNode::Entry { entry, .. } = selected else {
        return Err(ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_string(),
        });
    };
    let mut metadata =
        SemanticEntry::from_owner_shallow(entry.item).expect("located semantic owner");
    if metadata.alias_of.is_some()
        && query.document.as_ref().is_some_and(|document| {
            mant_ir::entry_relation_issues(document)
                .iter()
                .any(|issue| {
                    issue.owner == metadata.id
                        && matches!(
                            issue.kind,
                            mant_ir::EntryRelationIssueKind::AliasOf
                                | mant_ir::EntryRelationIssueKind::Cycle
                        )
                })
        })
    {
        metadata.alias_of = None;
    }
    let mut node = project_entries(
        &[metadata],
        None,
        &[],
        &EntryProjection::All,
        query.address.as_ref(),
    )
    .pop()
    .expect("projected selected owner");
    if let OutlineNode::DocumentEntry {
        path,
        entry_summary,
        ..
    } = &mut node
    {
        *path = selected.path().to_string().into();
        *entry_summary = borrowed_summary(entry.item.blocks(), entries);
    }
    Ok(node)
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
