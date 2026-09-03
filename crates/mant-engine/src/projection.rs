//! Projects complete structured documents into outlines and selectable excerpts.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    error::Error,
    fmt,
};

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic,
    DiagnosticLevel, EntryKindCount, EntrySummary, OutlinePath, Section, SemanticEntry,
    SemanticIndex, SourceSpan,
};
use mant_protocol::{
    EntryProjection, ExcerptSchema, ExcerptSelection, NodeSelector, OutlineDetail, OutlineNode,
    OutlineNodeReference, OutlineReference, OutlineSchema, OutlineTrail, QueryExcerpt,
    QueryOutline,
};

use crate::{
    ResolvedContent,
    definitions::{definition_entries, environment_variable_body},
    inline::plain_text,
};

pub(crate) const TLDR_ID: &str = "tldr";
const TLDR_TITLE: &str = "TLDR QUICK REFERENCE";
pub(crate) use mant_ir::DOCUMENT_ROOT_ID;
pub(crate) const DOCUMENT_ROOT_TITLE: &str = "OVERVIEW";

/// Whether an identifier belongs to the selector namespace rather than a
/// document-defined node.
///
/// Section paths use dotted positive indices (`2.1`), while semantic entries
/// append a semantic-entry index (`2.1/e3`). The parser reserves the complete grammar,
/// not only selectors present in one particular document, so source-defined
/// IDs can never make excerpt lookup ambiguous.
pub(crate) fn is_reserved_selector(value: &str) -> bool {
    matches!(value, TLDR_ID | DOCUMENT_ROOT_ID)
        || value.parse::<OutlinePath>().is_ok()
        || [
            "option-",
            "marker-",
            "operand-",
            "command-",
            "configuration-",
            "environment-",
            "variable-",
            "value-",
            "term-",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

/// Failure to derive an addressable view from a complete query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    /// Neither an authoritative document nor a quick reference is available.
    MissingContent {
        /// Requested document label.
        document: String,
    },
    /// Excerpt projection received no selectors.
    EmptySelection,
    /// One selector was empty after trimming.
    EmptySelector,
    /// No addressable node matched a selector.
    UnknownSelector {
        /// Requested document label.
        document: String,
        /// Unresolved selector.
        selector: String,
    },
    /// Explanation lookup found no semantic entry, but the same text occurs
    /// elsewhere in the rendered document.
    SelectorFoundOnlyInText {
        /// Requested document label.
        document: String,
        /// Unresolved semantic-entry selector.
        selector: String,
        /// Canonical path of the nearest addressable node.
        path: String,
        /// Display title of the nearest addressable node.
        title: String,
        /// One-based rendered line containing the first occurrence.
        line: u32,
    },
    /// An alias matched more than one semantic entry.
    AmbiguousSelector {
        /// Requested document label.
        document: String,
        /// Ambiguous selector.
        selector: String,
        /// Stable paths and IDs that disambiguate the match.
        candidates: Vec<SelectorCandidate>,
    },
    /// Explanation lookup selected a non-entry node.
    ExplanationRequiresEntry {
        /// Requested document label.
        document: String,
        /// Selector naming the non-entry node.
        selector: String,
    },
}

/// One stable qualification offered when a semantic alias is ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorCandidate {
    /// Canonical structural outline path.
    pub path: String,
    /// Stable document-local identity.
    pub id: String,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContent { document } => {
                write!(formatter, "document '{document}' has no available content")
            }
            Self::EmptySelection => formatter.write_str("at least one outline node is required"),
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::UnknownSelector { document, selector } => write!(
                formatter,
                "document '{document}' has no outline node '{selector}'; inspect its entries outline for available selectors and diagnostics"
            ),
            Self::SelectorFoundOnlyInText {
                document,
                selector,
                path,
                title,
                line,
            } => write!(
                formatter,
                "document '{document}' has no semantic entry '{selector}', but that text appears in outline node {path} ({title}) at line {line}"
            ),
            Self::AmbiguousSelector {
                document,
                selector,
                candidates,
            } => {
                write!(
                    formatter,
                    "document '{document}' has multiple semantic entries named '{selector}': "
                )?;
                for (index, candidate) in candidates.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{} ({})", candidate.path, candidate.id)?;
                }
                formatter.write_str("; select one by path or ID")
            }
            Self::ExplanationRequiresEntry { document, selector } => write!(
                formatter,
                "document '{document}' outline node '{selector}' is not a semantic entry; select a semantic entry instead"
            ),
        }
    }
}

impl Error for ProjectionError {}

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
    let entries_complete = diagnostics.iter().all(|diagnostic| {
        !diagnostic.code.as_deref().is_some_and(|code| {
            crate::markdown::is_semantic_entry_rejection_code(code)
                || code == "manual.semantic-entry.unclassified-definition"
        })
    });
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
            let children = project_entries(root_entries, None, &[], &materialized_entries);
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
        source: query
            .document
            .as_ref()
            .map(|document| document.source.clone()),
        meta: query
            .document
            .as_ref()
            .map(|document| document.meta.clone()),
        diagnostics,
        entries_complete,
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

    let (tldr_selected, document_root_selected, mut selected) =
        resolve_excerpt_candidates(query, selectors, &located)?;
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
    located: &'a [LocatedNode<'a>],
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
        let candidate = resolve_candidate(query, located, selector)?;
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
    let candidate = resolve_explanation_candidate(query, &located, selector)?;
    select_excerpt(query, &[candidate.path().to_string()])
}

fn resolve_explanation_candidate<'a>(
    query: &ResolvedContent,
    located: &'a [LocatedNode<'a>],
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
    let candidate = resolve_candidate(query, located, selector)?;
    if candidate.is_section() {
        return Err(ProjectionError::ExplanationRequiresEntry {
            document: query.label.clone(),
            selector: selector.to_owned(),
        });
    }
    Ok(candidate)
}

fn resolve_candidate<'a>(
    query: &ResolvedContent,
    located: &'a [LocatedNode<'a>],
    selector: &str,
) -> Result<&'a LocatedNode<'a>, ProjectionError> {
    if let Some(candidate) = located
        .iter()
        .find(|candidate| candidate.matches_path(selector))
    {
        return Ok(candidate);
    }
    let ids = located
        .iter()
        .filter(|candidate| candidate.id() == selector)
        .collect::<Vec<_>>();
    match ids.as_slice() {
        [candidate] => return Ok(candidate),
        [] => {}
        _ => return Err(ambiguous_selector(query, selector, ids)),
    }

    let matches = matching_aliases(located, selector).1;
    match matches.as_slice() {
        [] => Err(ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_owned(),
        }),
        [candidate] => Ok(candidate),
        _ => Err(ambiguous_selector(query, selector, matches)),
    }
}

fn ambiguous_selector(
    query: &ResolvedContent,
    selector: &str,
    matches: Vec<&LocatedNode<'_>>,
) -> ProjectionError {
    ProjectionError::AmbiguousSelector {
        document: query.label.clone(),
        selector: selector.to_owned(),
        candidates: matches
            .into_iter()
            .map(|candidate| SelectorCandidate {
                path: candidate.path().to_string(),
                id: candidate.id().into(),
            })
            .collect(),
    }
}

fn outline_nodes(
    sections: &[Section],
    parent: &[usize],
    index: &SemanticIndex,
    entries: &EntryProjection,
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
            let mut children = project_entries(semantic_entries, Some(&coordinates), &[], entries);
            children.extend(outline_nodes(
                &section.children,
                &coordinates,
                index,
                entries,
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
            let children = project_entries(&entry.children, section, &coordinates, projection);
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
                targets: entry.targets.clone(),
                value_domain: entry.value_domain.clone(),
                entry_summary: projected_summary(&entry.children, projection),
                children,
            })
        })
        .collect()
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
    let path = resolve_candidate(query, &located, selector)?
        .path()
        .to_string();
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

enum LocatedNode<'a> {
    Section {
        order: usize,
        coordinates: Vec<usize>,
        path: OutlinePath,
        breadcrumbs: Vec<OutlineReference>,
        section: &'a Section,
    },
    Entry {
        order: usize,
        coordinates: Vec<usize>,
        path: OutlinePath,
        title: String,
        breadcrumbs: Vec<OutlineReference>,
        entry: &'a DefinitionItem,
        source: Option<SourceSpan>,
    },
}

impl LocatedNode<'_> {
    fn order(&self) -> usize {
        match self {
            Self::Section { order, .. } | Self::Entry { order, .. } => *order,
        }
    }

    fn coordinates(&self) -> &[usize] {
        match self {
            Self::Section { coordinates, .. } | Self::Entry { coordinates, .. } => coordinates,
        }
    }

    fn path(&self) -> &OutlinePath {
        match self {
            Self::Section { path, .. } | Self::Entry { path, .. } => path,
        }
    }

    fn matches_path(&self, selector: &str) -> bool {
        selector
            .parse::<OutlinePath>()
            .is_ok_and(|path| path == *self.path())
    }

    fn id(&self) -> &str {
        match self {
            Self::Section { section, .. } => &section.id,
            Self::Entry { entry, .. } => {
                &entry
                    .identity
                    .as_ref()
                    .expect("located entries have identities")
                    .id
            }
        }
    }

    fn matches_exact_alias(&self, selector: &str) -> bool {
        match self {
            Self::Entry { entry, .. } => entry.identity.as_ref().is_some_and(|identity| {
                identity
                    .names
                    .iter()
                    .any(|name| semantic_name_equivalent(identity.case, name, selector))
            }),
            Self::Section { .. } => false,
        }
    }

    fn matches_shorthand_alias(&self, selector: &str) -> bool {
        match self {
            Self::Entry { entry, .. } => entry.identity.as_ref().is_some_and(|identity| {
                identity.names.iter().any(|name| {
                    semantic_name_shorthand(identity.role, name).is_some_and(|shorthand| {
                        semantic_name_equivalent(identity.case, shorthand, selector)
                    })
                })
            }),
            Self::Section { .. } => false,
        }
    }

    fn identity(&self) -> Option<&DefinitionIdentity> {
        match self {
            Self::Entry { entry, .. } => entry.identity.as_ref(),
            Self::Section { .. } => None,
        }
    }

    fn source(&self) -> Option<SourceSpan> {
        match self {
            Self::Entry { source, .. } => *source,
            Self::Section { section, .. } => section.source,
        }
    }

    const fn is_section(&self) -> bool {
        matches!(self, Self::Section { .. })
    }

    fn selection(&self) -> ExcerptSelection {
        match self {
            Self::Section {
                path,
                breadcrumbs,
                section,
                ..
            } => ExcerptSelection::DocumentSection {
                outline: OutlineTrail {
                    ancestors: breadcrumbs.clone(),
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
                    ancestors: breadcrumbs.clone(),
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

fn semantic_name_equivalent(case: DefinitionCase, left: &str, right: &str) -> bool {
    match case {
        DefinitionCase::Sensitive => left == right,
        DefinitionCase::Insensitive => left.eq_ignore_ascii_case(right),
    }
}

fn semantic_name_shorthand(role: DefinitionRole, name: &str) -> Option<&str> {
    match role {
        DefinitionRole::Option => {
            let shorthand = name.trim_start_matches('-');
            (shorthand != name && !shorthand.is_empty()).then_some(shorthand)
        }
        DefinitionRole::EnvironmentVariable => {
            environment_variable_body(name).filter(|body| *body != name)
        }
        DefinitionRole::Command
        | DefinitionRole::ConfigurationKey
        | DefinitionRole::Marker
        | DefinitionRole::Operand
        | DefinitionRole::Variable
        | DefinitionRole::Value
        | DefinitionRole::Term => None,
    }
}

#[derive(Clone, Copy)]
enum AliasMatchKind {
    Exact,
    Shorthand,
}

impl AliasMatchKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact alias",
            Self::Shorthand => "normalized shorthand",
        }
    }
}

fn matching_aliases<'a>(
    located: &'a [LocatedNode<'a>],
    selector: &str,
) -> (AliasMatchKind, Vec<&'a LocatedNode<'a>>) {
    let exact = located
        .iter()
        .filter(|candidate| candidate.matches_exact_alias(selector))
        .collect::<Vec<_>>();
    if !exact.is_empty() {
        return (AliasMatchKind::Exact, exact);
    }
    (
        AliasMatchKind::Shorthand,
        located
            .iter()
            .filter(|candidate| candidate.matches_shorthand_alias(selector))
            .collect(),
    )
}

/// Report selectors that cannot address exactly one semantic entry.
///
/// The lookup policy itself remains usable through stable paths and IDs, but
/// Markdown authors receive a source diagnostic before an agent discovers the
/// ambiguity at query time.
pub(crate) fn semantic_selector_diagnostics(
    blocks: &[Block],
    sections: &[Section],
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut located = Vec::new();
    collect_root_entries(blocks, &mut located);
    collect_sections(sections, &[], &[], &mut located);
    let index = SelectorDiagnosticsIndex::new(&located);
    let mut selectors = BTreeSet::new();
    for candidate in &located {
        let Some(identity) = candidate.identity() else {
            continue;
        };
        for name in &identity.names {
            selectors.insert(name.clone());
            if let Some(shorthand) = semantic_name_shorthand(identity.role, name) {
                selectors.insert(shorthand.to_owned());
            }
        }
    }

    let mut diagnostics = selector_alias_diagnostics(&index, selectors, source_family);
    diagnostics.extend(duplicate_id_diagnostics(&index.ids, source_family));
    diagnostics
}

#[derive(Default)]
struct AliasIndex<'a> {
    sensitive: HashMap<&'a str, Vec<&'a LocatedNode<'a>>>,
    insensitive: HashMap<String, Vec<&'a LocatedNode<'a>>>,
}

impl<'a> AliasIndex<'a> {
    fn insert(&mut self, case: DefinitionCase, alias: &'a str, candidate: &'a LocatedNode<'a>) {
        let bucket = match case {
            DefinitionCase::Sensitive => self.sensitive.entry(alias).or_default(),
            DefinitionCase::Insensitive => self
                .insensitive
                .entry(alias.to_ascii_lowercase())
                .or_default(),
        };
        if bucket
            .last()
            .is_none_or(|existing| existing.order() != candidate.order())
        {
            bucket.push(candidate);
        }
    }

    fn matches(&self, selector: &str) -> Vec<&'a LocatedNode<'a>> {
        let mut matches = self.sensitive.get(selector).cloned().unwrap_or_default();
        if let Some(insensitive) = self.insensitive.get(&selector.to_ascii_lowercase()) {
            matches.extend(insensitive.iter().copied());
        }
        matches.sort_unstable_by_key(|candidate| candidate.order());
        matches.dedup_by_key(|candidate| candidate.order());
        matches
    }
}

struct SelectorDiagnosticsIndex<'a> {
    exact_aliases: AliasIndex<'a>,
    shorthand_aliases: AliasIndex<'a>,
    ids: BTreeMap<&'a str, Vec<&'a LocatedNode<'a>>>,
}

impl<'a> SelectorDiagnosticsIndex<'a> {
    fn new(located: &'a [LocatedNode<'a>]) -> Self {
        let mut index = Self {
            exact_aliases: AliasIndex::default(),
            shorthand_aliases: AliasIndex::default(),
            ids: BTreeMap::new(),
        };
        for candidate in located {
            index.ids.entry(candidate.id()).or_default().push(candidate);
            let Some(identity) = candidate.identity() else {
                continue;
            };
            for name in &identity.names {
                index.exact_aliases.insert(identity.case, name, candidate);
                if let Some(shorthand) = semantic_name_shorthand(identity.role, name) {
                    index
                        .shorthand_aliases
                        .insert(identity.case, shorthand, candidate);
                }
            }
        }
        index
    }

    fn matching_aliases(&self, selector: &str) -> (AliasMatchKind, Vec<&'a LocatedNode<'a>>) {
        let exact = self.exact_aliases.matches(selector);
        if !exact.is_empty() {
            return (AliasMatchKind::Exact, exact);
        }
        (
            AliasMatchKind::Shorthand,
            self.shorthand_aliases.matches(selector),
        )
    }
}

fn selector_alias_diagnostics(
    index: &SelectorDiagnosticsIndex<'_>,
    selectors: BTreeSet<String>,
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();
    for selector in selectors {
        let (kind, matches) = index.matching_aliases(&selector);
        let exact_ids = index
            .ids
            .get(selector.as_str())
            .map_or(&[][..], Vec::as_slice);
        let shadowed_matches = matches
            .iter()
            .copied()
            .filter(|candidate| {
                !exact_ids
                    .iter()
                    .any(|owner| owner.path() == candidate.path())
            })
            .collect::<Vec<_>>();
        if !shadowed_matches.is_empty() && !exact_ids.is_empty() {
            let key = format!("shadowed\u{1f}{selector}");
            if reported.insert(key) {
                let owners = exact_ids
                    .iter()
                    .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
                    .collect::<Vec<_>>()
                    .join(", ");
                let entries = shadowed_matches
                    .iter()
                    .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
                    .collect::<Vec<_>>()
                    .join(", ");
                diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    code: Some(format!(
                        "{source_family}.semantic-entry.shadowed-selector"
                    )),
                    message: format!(
                        "semantic selector '{selector}' is owned by exact outline ID {owners}; matching {} entries {entries} require their path or ID",
                        kind.label()
                    ),
                    source: shadowed_matches
                        .first()
                        .and_then(|candidate| candidate.source()),
                });
            }
        }
        if matches.len() < 2 {
            continue;
        }
        let key = matches
            .iter()
            .map(|candidate| candidate.id())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        if !reported.insert(key) {
            continue;
        }
        let candidates = matches
            .iter()
            .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some(format!(
                "{source_family}.semantic-entry.ambiguous-selector"
            )),
            message: format!(
                "semantic selector '{selector}' has multiple {} matches: {candidates}; select by path or ID",
                kind.label()
            ),
            source: matches.first().and_then(|candidate| candidate.source()),
        });
    }
    diagnostics
}

fn duplicate_id_diagnostics(
    ids: &BTreeMap<&str, Vec<&LocatedNode<'_>>>,
    source_family: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (id, matches) in ids {
        if matches.len() < 2 {
            continue;
        }
        let candidates = matches
            .iter()
            .map(|candidate| format!("{} ({})", candidate.path(), candidate.id()))
            .collect::<Vec<_>>()
            .join(", ");
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some(format!("{source_family}.outline.duplicate-id")),
            message: format!(
                "outline ID '{id}' belongs to multiple nodes: {candidates}; select by path"
            ),
            source: matches.first().and_then(|candidate| candidate.source()),
        });
    }
    diagnostics
}

fn collect_sections<'a>(
    sections: &'a [Section],
    parent_coordinates: &[usize],
    breadcrumbs: &[OutlineReference],
    output: &mut Vec<LocatedNode<'a>>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent_coordinates.to_vec();
        coordinates.push(index + 1);
        let path =
            OutlinePath::section(&coordinates).expect("enumerated section paths are one-based");
        let order = output.len();
        output.push(LocatedNode::Section {
            order,
            coordinates: coordinates.clone(),
            path: path.clone(),
            breadcrumbs: breadcrumbs.to_vec(),
            section,
        });
        let mut child_breadcrumbs = breadcrumbs.to_vec();
        child_breadcrumbs.push(OutlineReference {
            path: path.to_string().into(),
            id: section.id.clone(),
            title: section.title.clone(),
        });
        for located in definition_entries(&section.blocks) {
            let entry = located.item;
            let mut entry_breadcrumbs = child_breadcrumbs.clone();
            append_entry_breadcrumbs(
                &mut entry_breadcrumbs,
                Some(&coordinates),
                &located.indices,
                &located.ancestors,
            );
            output.push(LocatedNode::Entry {
                order: output.len(),
                coordinates: coordinates.clone(),
                path: OutlinePath::nested_entry(Some(&coordinates), &located.indices)
                    .expect("enumerated entry paths are one-based"),
                title: definition_title(entry),
                breadcrumbs: entry_breadcrumbs,
                entry,
                source: located.source,
            });
        }
        collect_sections(&section.children, &coordinates, &child_breadcrumbs, output);
    }
}

fn collect_root_entries<'a>(blocks: &'a [Block], output: &mut Vec<LocatedNode<'a>>) {
    let breadcrumbs = vec![OutlineReference {
        path: OutlinePath::DocumentRoot.to_string().into(),
        id: DOCUMENT_ROOT_ID.into(),
        title: DOCUMENT_ROOT_TITLE.to_owned(),
    }];
    for located in definition_entries(blocks) {
        let entry = located.item;
        let mut entry_breadcrumbs = breadcrumbs.clone();
        append_entry_breadcrumbs(
            &mut entry_breadcrumbs,
            None,
            &located.indices,
            &located.ancestors,
        );
        output.push(LocatedNode::Entry {
            order: output.len(),
            coordinates: Vec::new(),
            path: OutlinePath::nested_entry(None, &located.indices)
                .expect("enumerated entry paths are one-based"),
            title: definition_title(entry),
            breadcrumbs: entry_breadcrumbs,
            entry,
            source: located.source,
        });
    }
}

fn append_entry_breadcrumbs(
    breadcrumbs: &mut Vec<OutlineReference>,
    section: Option<&[usize]>,
    indices: &[usize],
    ancestors: &[&DefinitionItem],
) {
    for (depth, ancestor) in ancestors.iter().enumerate() {
        let path = OutlinePath::nested_entry(section, &indices[..=depth])
            .expect("ancestor entry paths are one-based");
        let identity = ancestor
            .identity
            .as_ref()
            .expect("semantic entry ancestors have identities");
        breadcrumbs.push(OutlineReference {
            path: path.to_string().into(),
            id: identity.id.clone(),
            title: definition_title(ancestor),
        });
    }
}

fn definition_title(entry: &DefinitionItem) -> String {
    let identity = entry
        .identity
        .as_ref()
        .expect("semantic entries have identities");
    if !identity.names.is_empty() {
        return identity.names.join(", ");
    }
    let forms = entry
        .terms
        .iter()
        .map(|term| plain_text(term))
        .filter(|form| !form.is_empty())
        .collect::<Vec<_>>();
    if !forms.is_empty() {
        return forms.join(" | ");
    }
    identity.id.to_string()
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
    fn entry_completeness_distinguishes_rejections_from_author_warnings() {
        let mut query = query();
        {
            let document = query.document.as_mut().expect("document");
            for code in [
                "markdown.semantic-entry.ambiguous-selector",
                "markdown.semantic-entry-list",
            ] {
                document.diagnostics.push(Diagnostic {
                    level: DiagnosticLevel::Warning,
                    code: Some(code.to_owned()),
                    message: "author warning".to_owned(),
                    source: None,
                });
            }
        }
        assert!(
            build_outline(&query)
                .expect("complete outline")
                .entries_complete
        );

        query
            .document
            .as_mut()
            .expect("document")
            .diagnostics
            .push(Diagnostic {
                level: DiagnosticLevel::Warning,
                code: Some("markdown.semantic-entry.invalid-entry-name".to_owned()),
                message: "rejected declaration".to_owned(),
                source: None,
            });
        assert!(
            !build_outline(&query)
                .expect("partial outline")
                .entries_complete
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
