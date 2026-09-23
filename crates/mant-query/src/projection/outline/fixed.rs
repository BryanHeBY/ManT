//! Fixed outline projection from native mark parentage and shared semantic index.
//!
//! This path never makes a Flow tree or treats terminal indentation as depth.

use std::num::NonZeroU32;

use super::{ProjectionError, ResolvedContent, project_entries, projected_summary};
use mant_ir::{
    DOCUMENT_ROOT_ID, Document, FixedBody, FixedSectionReader, OutlinePath, SemanticIndex,
};
use mant_protocol::{ContentSelector, EntryProjection, OutlineNode};

pub(super) fn append_nodes(
    query: &ResolvedContent,
    manual: &Document,
    fixed: &FixedBody,
    entries: &EntryProjection,
    nodes: &mut Vec<OutlineNode>,
) -> Result<(), ProjectionError> {
    let reader = FixedSectionReader::new(fixed).map_err(|_| ProjectionError::ContentProjection)?;
    let index = SemanticIndex::build(manual);
    let preface = reader
        .root_preface_parts()
        .map_err(|_| ProjectionError::ContentProjection)?;
    let root_entries = index.root();
    if !preface.is_empty() || !root_entries.is_empty() {
        let children = project_entries(
            root_entries,
            None,
            &[],
            entries,
            query.address.as_ref(),
            &|path| index.owner_at(path).cloned(),
        );
        let root = OutlineNode::DocumentRoot {
            path: OutlinePath::DocumentRoot.to_string().into(),
            id: DOCUMENT_ROOT_ID.into(),
            title: super::DOCUMENT_ROOT_TITLE.to_owned(),
            entry_summary: projected_summary(root_entries, entries),
            children,
        };
        if !matches!(entries, EntryProjection::Kinds { .. }) || !root.children().is_empty() {
            nodes.push(root);
        }
    }
    for &key in reader.roots() {
        if let Some(node) = section_node(&reader, &index, key, entries, query)? {
            nodes.push(node);
        }
    }
    Ok(())
}

fn section_node(
    reader: &FixedSectionReader<'_>,
    index: &SemanticIndex,
    key: NonZeroU32,
    entries: &EntryProjection,
    query: &ResolvedContent,
) -> Result<Option<OutlineNode>, ProjectionError> {
    let heading = reader
        .heading(key)
        .ok_or(ProjectionError::ContentProjection)?;
    let path = reader.path(key).ok_or(ProjectionError::ContentProjection)?;
    let OutlinePath::Section(coordinates) = &path else {
        return Err(ProjectionError::ContentProjection);
    };
    let one_based = coordinates
        .iter()
        .map(|coordinate| coordinate.get())
        .collect::<Vec<_>>();
    let source_path = one_based
        .iter()
        .map(|coordinate| coordinate - 1)
        .collect::<Vec<_>>();
    let owned_entries = index.section_at(&source_path);
    let mut children = project_entries(
        owned_entries,
        Some(&one_based),
        &[],
        entries,
        query.address.as_ref(),
        &|path| index.owner_at(path).cloned(),
    );
    for &child in reader
        .children(key)
        .ok_or(ProjectionError::ContentProjection)?
    {
        if let Some(node) = section_node(reader, index, child, entries, query)? {
            children.push(node);
        }
    }
    let node = OutlineNode::DocumentSection {
        path: path.to_string().into(),
        id: heading.id.clone(),
        title: reader
            .label(key)
            .ok_or(ProjectionError::ContentProjection)?,
        entry_summary: projected_summary(owned_entries, entries),
        children,
    };
    Ok(
        (!matches!(entries, EntryProjection::Kinds { .. }) || !node.children().is_empty())
            .then_some(node),
    )
}

pub(super) fn resolve_root(
    query: &ResolvedContent,
    nodes: &[OutlineNode],
    selector: &ContentSelector,
) -> Result<OutlineNode, ProjectionError> {
    let mut matches = Vec::new();
    collect_matches(nodes, selector, &mut matches);
    match matches.as_slice() {
        [selected] => Ok((**selected).clone()),
        [] => Err(ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_string(),
        }),
        candidates => Err(ProjectionError::AmbiguousSelector {
            document: query.label.clone(),
            selector: selector.to_string(),
            candidates: candidates
                .iter()
                .map(|node| crate::selectors::SelectorCandidate {
                    path: node.path().to_owned(),
                    id: node.id().to_owned(),
                })
                .collect(),
        }),
    }
}

fn collect_matches<'a>(
    nodes: &'a [OutlineNode],
    selector: &ContentSelector,
    matches: &mut Vec<&'a OutlineNode>,
) {
    for node in nodes {
        let selected = match selector {
            ContentSelector::Path { path } => node.path() == path.as_str(),
            ContentSelector::Id { id } => node.id() == id.as_str(),
        };
        if selected {
            matches.push(node);
        }
        collect_matches(node.children(), selector, matches);
    }
}
