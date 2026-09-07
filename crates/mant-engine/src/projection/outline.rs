//! Outline DTOs and relationship projection of already indexed semantic nodes.
use super::{ProjectionError, TLDR_TITLE, semantics_complete};
use crate::{
    ResolvedContent,
    selectors::{
        DOCUMENT_ROOT_TITLE, DocumentSelectorIndex, TLDR_ID, collect_root_entries, collect_sections,
    },
};
use mant_ir::{
    DOCUMENT_ROOT_ID, EntryKindCount, EntrySummary, OutlinePath, Section, SemanticEntry,
    SemanticIndex,
};
use mant_protocol::{
    EntryDocumentTarget, EntryProjection, EntryValueDomain, NodeSelector, OutlineDetail,
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
