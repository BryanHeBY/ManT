//! Projects complete structured documents into outlines and selectable excerpts.

use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    fmt,
};

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic,
    DiagnosticLevel, OutlinePath, Section, SourceSpan,
};
use mant_protocol::{
    ExcerptSchema, ExcerptSelection, OutlineDetail, OutlineNode, OutlineReference, OutlineSchema,
    QueryExcerpt, QueryOutline,
};

use crate::{ResolvedContent, definitions::definition_entries};

pub(crate) const TLDR_ID: &str = "tldr";
const TLDR_TITLE: &str = "TLDR QUICK REFERENCE";
pub(crate) use mant_ir::DOCUMENT_ROOT_ID;
pub(crate) const DOCUMENT_ROOT_TITLE: &str = "OVERVIEW";

/// Whether an identifier belongs to the selector namespace rather than a
/// document-defined node.
///
/// Section paths use dotted positive indices (`2.1`), while semantic entries
/// append an option index (`2.1/o3`). The parser reserves the complete grammar,
/// not only selectors present in one particular document, so source-defined
/// IDs can never make excerpt lookup ambiguous.
pub(crate) fn is_reserved_selector(value: &str) -> bool {
    matches!(value, TLDR_ID | DOCUMENT_ROOT_ID) || value.parse::<OutlinePath>().is_ok()
}

/// Failure to derive an addressable view from a complete query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    MissingContent {
        document: String,
    },
    EmptySelection,
    EmptySelector,
    UnknownSelector {
        document: String,
        selector: String,
    },
    AmbiguousSelector {
        document: String,
        selector: String,
        candidates: Vec<SelectorCandidate>,
    },
    ExplanationRequiresEntry {
        document: String,
        selector: String,
    },
}

/// One stable qualification offered when a semantic alias is ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorCandidate {
    pub path: String,
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
                "document '{document}' has no outline node '{selector}'; inspect its entries outline as JSON for available selectors and diagnostics"
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
                "document '{document}' outline node '{selector}' is not a semantic entry; use --node for sections"
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
    build_outline_with_detail(query, OutlineDetail::Sections)
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
        !diagnostic
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("markdown.semantic-entry"))
    });
    let mut nodes = Vec::new();
    if query.tldr.is_some() {
        nodes.push(OutlineNode::Tldr {
            path: OutlinePath::Tldr.to_string().into(),
            id: TLDR_ID.into(),
            title: TLDR_TITLE.to_owned(),
        });
    }
    if let Some(manual) = &query.document {
        if !manual.blocks.is_empty() {
            nodes.push(OutlineNode::DocumentRoot {
                path: OutlinePath::DocumentRoot.to_string().into(),
                id: DOCUMENT_ROOT_ID.into(),
                title: DOCUMENT_ROOT_TITLE.to_owned(),
            });
            if detail == OutlineDetail::Entries {
                nodes.extend(
                    definition_entries(&manual.blocks)
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, (entry, _))| {
                            let identity = entry.identity.as_ref()?;
                            Some(OutlineNode::DocumentEntry {
                                path: OutlinePath::entry(None, index + 1)?.to_string().into(),
                                id: identity.id.clone(),
                                title: identity.names.join(", "),
                                role: identity.role,
                                case: identity.case,
                                names: identity.names.clone(),
                            })
                        }),
                );
            }
        }
        nodes.extend(outline_nodes(&manual.sections, &[], detail));
    }
    Ok(QueryOutline {
        schema: OutlineSchema::V7,
        detail,
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
pub fn select_excerpt(
    query: &ResolvedContent,
    selectors: &[String],
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

    let mut tldr_selected = false;
    let mut document_root_selected = false;
    let mut selected_ids = HashSet::new();
    let mut selected = Vec::new();
    for raw_selector in selectors {
        let selector = raw_selector.trim();
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
        let candidate = resolve_candidate(query, &located, selector)?;
        if selected_ids.insert(candidate.id()) {
            selected.push(candidate);
        }
    }
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
            path: OutlinePath::Tldr.to_string().into(),
            id: TLDR_ID.into(),
            title: TLDR_TITLE.to_owned(),
            document,
        });
    }
    if let (true, Some(document)) = (document_root_selected, query.document.as_ref()) {
        selections.push(ExcerptSelection::DocumentRoot {
            path: OutlinePath::DocumentRoot.to_string().into(),
            id: DOCUMENT_ROOT_ID.into(),
            title: DOCUMENT_ROOT_TITLE.to_owned(),
            blocks: document.blocks.clone(),
        });
    }
    selections.extend(selected.into_iter().map(LocatedNode::selection));

    Ok(QueryExcerpt {
        schema: ExcerptSchema::V7,
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
    if let Some(candidate) = located.iter().find(|candidate| {
        !candidate.is_section() && (candidate.matches_path(selector) || candidate.id() == selector)
    }) {
        return Ok(candidate);
    }

    let matches = matching_aliases(located, selector).1;
    match matches.as_slice() {
        [candidate] => return Ok(candidate),
        [] => {}
        _ => {
            return Err(ProjectionError::AmbiguousSelector {
                document: query.label.clone(),
                selector: selector.to_owned(),
                candidates: matches
                    .into_iter()
                    .map(|candidate| SelectorCandidate {
                        path: candidate.path().to_string(),
                        id: candidate.id().into(),
                    })
                    .collect(),
            });
        }
    }

    let selects_tldr =
        (selector == TLDR_ID || selector.parse() == Ok(OutlinePath::Tldr)) && query.tldr.is_some();
    let selects_root = (selector == DOCUMENT_ROOT_ID
        || selector.parse() == Ok(OutlinePath::DocumentRoot))
        && query
            .document
            .as_ref()
            .is_some_and(|document| !document.blocks.is_empty());
    let selects_section = located.iter().any(|candidate| {
        candidate.is_section() && (candidate.matches_path(selector) || candidate.id() == selector)
    });
    if selects_tldr || selects_root || selects_section {
        return Err(ProjectionError::ExplanationRequiresEntry {
            document: query.label.clone(),
            selector: selector.to_owned(),
        });
    }

    Err(ProjectionError::UnknownSelector {
        document: query.label.clone(),
        selector: selector.to_owned(),
    })
}

fn resolve_candidate<'a>(
    query: &ResolvedContent,
    located: &'a [LocatedNode<'a>],
    selector: &str,
) -> Result<&'a LocatedNode<'a>, ProjectionError> {
    if let Some(candidate) = located
        .iter()
        .find(|candidate| candidate.matches_path(selector) || candidate.id() == selector)
    {
        return Ok(candidate);
    }

    let matches = matching_aliases(located, selector).1;
    match matches.as_slice() {
        [] => Err(ProjectionError::UnknownSelector {
            document: query.label.clone(),
            selector: selector.to_owned(),
        }),
        [candidate] => Ok(candidate),
        _ => Err(ProjectionError::AmbiguousSelector {
            document: query.label.clone(),
            selector: selector.to_owned(),
            candidates: matches
                .into_iter()
                .map(|candidate| SelectorCandidate {
                    path: candidate.path().to_string(),
                    id: candidate.id().into(),
                })
                .collect(),
        }),
    }
}

fn outline_nodes(
    sections: &[Section],
    parent: &[usize],
    detail: OutlineDetail,
) -> Vec<OutlineNode> {
    sections
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let mut coordinates = parent.to_vec();
            coordinates.push(index + 1);
            let path =
                OutlinePath::section(&coordinates).expect("enumerated section paths are one-based");
            let mut children = Vec::new();
            if detail == OutlineDetail::Entries {
                children.extend(
                    definition_entries(&section.blocks)
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, (entry, _))| {
                            let identity = entry.identity.as_ref()?;
                            Some(OutlineNode::DocumentEntry {
                                path: OutlinePath::entry(Some(&coordinates), index + 1)
                                    .expect("enumerated entry paths are one-based")
                                    .to_string()
                                    .into(),
                                id: identity.id.clone(),
                                title: identity.names.join(", "),
                                role: identity.role,
                                case: identity.case,
                                names: identity.names.clone(),
                            })
                        }),
                );
            }
            children.extend(outline_nodes(&section.children, &coordinates, detail));
            OutlineNode::DocumentSection {
                path: path.to_string().into(),
                id: section.id.clone(),
                title: section.title.clone(),
                children,
            }
        })
        .collect()
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
                path: path.to_string().into(),
                id: section.id.clone(),
                title: section.title.clone(),
                breadcrumbs: breadcrumbs.clone(),
                section: (*section).clone(),
            },
            Self::Entry {
                path,
                title,
                breadcrumbs,
                entry,
                ..
            } => ExcerptSelection::DocumentEntry {
                path: path.to_string().into(),
                id: entry
                    .identity
                    .as_ref()
                    .expect("located entries have identities")
                    .id
                    .clone(),
                title: title.clone(),
                breadcrumbs: breadcrumbs.clone(),
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
        DefinitionRole::EnvironmentVariable => name
            .strip_prefix("$env:")
            .or_else(|| name.strip_prefix("$ENV:")),
        DefinitionRole::Command | DefinitionRole::Variable => None,
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
) -> Vec<Diagnostic> {
    let mut located = Vec::new();
    collect_root_entries(blocks, &mut located);
    collect_sections(sections, &[], &[], &mut located);
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

    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();
    for selector in selectors {
        let (kind, matches) = matching_aliases(&located, &selector);
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
            code: Some("markdown.semantic-entry.ambiguous-selector".to_owned()),
            message: format!(
                "semantic selector '{selector}' has multiple {} matches: {candidates}; select by path or ID",
                kind.label()
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
        for (index, (entry, source)) in definition_entries(&section.blocks).into_iter().enumerate()
        {
            let Some(identity) = &entry.identity else {
                continue;
            };
            output.push(LocatedNode::Entry {
                order: output.len(),
                coordinates: coordinates.clone(),
                path: OutlinePath::entry(Some(&coordinates), index + 1)
                    .expect("enumerated entry paths are one-based"),
                title: identity.names.join(", "),
                breadcrumbs: child_breadcrumbs.clone(),
                entry,
                source,
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
    for (index, (entry, source)) in definition_entries(blocks).into_iter().enumerate() {
        let Some(identity) = &entry.identity else {
            continue;
        };
        output.push(LocatedNode::Entry {
            order: output.len(),
            coordinates: Vec::new(),
            path: OutlinePath::entry(None, index + 1)
                .expect("enumerated entry paths are one-based"),
            title: identity.names.join(", "),
            breadcrumbs: breadcrumbs.clone(),
            entry,
            source,
        });
    }
}

fn is_ancestor(ancestor: &[usize], descendant: &[usize]) -> bool {
    ancestor.len() < descendant.len() && descendant.starts_with(ancestor)
}

#[cfg(test)]
mod tests {
    use crate::ResolvedContent;
    use mant_ir::{
        Block, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, Section, SourceFormat,
        TldrDocument, TldrOrigin,
    };
    use mant_protocol::{ExcerptSelection, OutlineNode};

    use super::{ProjectionError, build_outline, select_excerpt};

    fn section(id: &str, title: &str, children: Vec<Section>) -> Section {
        Section {
            id: id.to_owned().into(),
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
                    section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
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

    #[test]
    fn builds_one_based_tree_paths_without_copying_blocks() {
        let outline = build_outline(&query()).expect("outline");

        assert_eq!(
            outline
                .meta
                .as_ref()
                .and_then(|meta| meta.section.as_deref()),
            Some("1")
        );
        assert_eq!(outline.nodes[1].path(), "2");
        assert_eq!(outline.nodes[1].id(), "options-2");
        assert_eq!(outline.nodes[1].children()[0].path(), "2.1");
        assert_eq!(outline.nodes[1].children()[1].path(), "2.2");
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
            OutlineNode::DocumentRoot { path, id, title }
                if path == "root" && id == "document-overview" && title == "OVERVIEW"
        ));
        // Heading paths remain stable and independent from the synthetic root.
        assert_eq!(outline.nodes[1].path(), "1");

        let excerpt = select_excerpt(&query, &["document-overview".to_owned(), "root".to_owned()])
            .expect("root excerpt");
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentRoot { path, blocks, .. }]
                if path == "root" && blocks.len() == 1
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
            .map(|selection| match selection {
                ExcerptSelection::Tldr { path, .. }
                | ExcerptSelection::DocumentRoot { path, .. }
                | ExcerptSelection::DocumentSection { path, .. }
                | ExcerptSelection::DocumentEntry { path, .. } => path.as_str(),
            })
            .collect::<Vec<_>>();
        assert_eq!(paths, ["2", "3"]);
        let ExcerptSelection::DocumentSection {
            section,
            breadcrumbs,
            ..
        } = &excerpt.selections[0]
        else {
            panic!("expected manual selection");
        };
        assert_eq!(section.children.len(), 2);
        assert!(breadcrumbs.is_empty());
    }

    #[test]
    fn child_selection_retains_ancestor_breadcrumbs() {
        let excerpt = select_excerpt(&query(), &["2.2".to_owned()]).expect("excerpt");

        let ExcerptSelection::DocumentSection {
            title, breadcrumbs, ..
        } = &excerpt.selections[0]
        else {
            panic!("expected manual selection");
        };
        assert_eq!(title, "Other options");
        assert_eq!(breadcrumbs[0].path, "2");
        assert_eq!(breadcrumbs[0].title, "OPTIONS");
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
            [ExcerptSelection::Tldr { path, .. }, ExcerptSelection::DocumentSection { .. }]
                if path == "0"
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
            select_excerpt(&query(), &[]),
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
