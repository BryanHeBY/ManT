//! Projects complete manual documents into outlines and selectable excerpts.

use std::{collections::HashSet, error::Error, fmt};

use mant_ast::{
    Block, DefinitionItem, ExcerptSchema, ExcerptSelection, OutlineDetail, OutlineNode,
    OutlineReference, OutlineSchema, QueryBundle, QueryExcerpt, QueryOutline, Section,
};

const TLDR_PATH: &str = "0";
const TLDR_ID: &str = "tldr";
const TLDR_TITLE: &str = "TLDR QUICK REFERENCE";

/// Failure to derive an addressable view from a complete query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    MissingContent { topic: String },
    EmptySelection,
    EmptySelector,
    UnknownSelector { topic: String, selector: String },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContent { topic } => {
                write!(formatter, "document '{topic}' has no available content")
            }
            Self::EmptySelection => formatter.write_str("at least one outline node is required"),
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::UnknownSelector { topic, selector } => write!(
                formatter,
                "document '{topic}' has no outline node '{selector}'; run 'mant-cli {topic} --outline'"
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
pub fn build_outline(query: &QueryBundle) -> Result<QueryOutline, ProjectionError> {
    build_outline_with_detail(query, OutlineDetail::Sections)
}

/// Build an outline with optional semantic definition entries.
///
/// # Errors
///
/// Returns [`ProjectionError::MissingContent`] when neither tldr nor a manual
/// is available.
pub fn build_outline_with_detail(
    query: &QueryBundle,
    detail: OutlineDetail,
) -> Result<QueryOutline, ProjectionError> {
    if query.tldr.is_none() && query.manual.is_none() {
        return Err(ProjectionError::MissingContent {
            topic: query.topic.clone(),
        });
    }
    let mut nodes = Vec::new();
    if query.tldr.is_some() {
        nodes.push(OutlineNode::Tldr {
            path: TLDR_PATH.to_owned(),
            id: TLDR_ID.to_owned(),
            title: TLDR_TITLE.to_owned(),
        });
    }
    if let Some(manual) = &query.manual {
        nodes.extend(outline_nodes(&manual.sections, &[], detail));
    }
    Ok(QueryOutline {
        schema: OutlineSchema::V2,
        detail,
        topic: query.topic.clone(),
        manual_section: resolved_manual_section(query),
        source: query.manual.as_ref().map(|manual| manual.source.clone()),
        meta: query.manual.as_ref().map(|manual| manual.meta.clone()),
        nodes,
    })
}

/// Select tldr or complete manual section subtrees by outline path or ID.
///
/// Duplicate selections and descendants of another selected node are omitted.
/// The result always follows source order, independent of argument order.
///
/// # Errors
///
/// Returns an error when no content exists or any selector is empty or unknown.
pub fn select_excerpt(
    query: &QueryBundle,
    selectors: &[String],
) -> Result<QueryExcerpt, ProjectionError> {
    if selectors.is_empty() {
        return Err(ProjectionError::EmptySelection);
    }
    if query.tldr.is_none() && query.manual.is_none() {
        return Err(ProjectionError::MissingContent {
            topic: query.topic.clone(),
        });
    }
    let mut located = Vec::new();
    if let Some(manual) = &query.manual {
        collect_sections(&manual.sections, &[], &[], &mut located);
    }

    let mut tldr_selected = false;
    let mut selected_ids = HashSet::new();
    let mut selected = Vec::new();
    for raw_selector in selectors {
        let selector = raw_selector.trim();
        if selector.is_empty() {
            return Err(ProjectionError::EmptySelector);
        }
        if matches!(selector, TLDR_PATH | TLDR_ID) && query.tldr.is_some() {
            tldr_selected = true;
            continue;
        }
        let candidate = located
            .iter()
            .find(|candidate| candidate.matches(selector))
            .ok_or_else(|| ProjectionError::UnknownSelector {
                topic: query.topic.clone(),
                selector: selector.to_owned(),
            })?;
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

    let manual = if selected.is_empty() {
        None
    } else {
        query.manual.as_ref()
    };
    let mut selections = Vec::new();
    if let (true, Some(document)) = (tldr_selected, query.tldr.clone()) {
        selections.push(ExcerptSelection::Tldr {
            path: TLDR_PATH.to_owned(),
            id: TLDR_ID.to_owned(),
            title: TLDR_TITLE.to_owned(),
            document,
        });
    }
    selections.extend(selected.into_iter().map(LocatedNode::selection));

    Ok(QueryExcerpt {
        schema: ExcerptSchema::V2,
        topic: query.topic.clone(),
        manual_section: manual.and_then(|_| resolved_manual_section(query)),
        producer: manual.map(|manual| manual.producer.clone()),
        source: manual.map(|manual| manual.source.clone()),
        meta: manual.map(|manual| manual.meta.clone()),
        diagnostics: manual
            .map(|manual| manual.diagnostics.clone())
            .unwrap_or_default(),
        selections,
    })
}

fn resolved_manual_section(query: &QueryBundle) -> Option<String> {
    query
        .manual
        .as_ref()
        .and_then(|manual| manual.meta.section.clone())
        .or_else(|| query.section.clone())
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
            let path = format_path(&coordinates);
            let mut children = Vec::new();
            if detail == OutlineDetail::Options {
                let mut entries = Vec::new();
                collect_definition_entries(&section.blocks, &mut entries);
                children.extend(
                    entries
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, entry)| {
                            let identity = entry.identity.as_ref()?;
                            Some(OutlineNode::ManualEntry {
                                path: format!("{path}/o{}", index + 1),
                                id: identity.id.clone(),
                                title: identity.names.join(", "),
                                role: identity.role,
                                names: identity.names.clone(),
                            })
                        }),
                );
            }
            children.extend(outline_nodes(&section.children, &coordinates, detail));
            OutlineNode::ManualSection {
                path,
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
        path: String,
        breadcrumbs: Vec<OutlineReference>,
        section: &'a Section,
    },
    Entry {
        order: usize,
        coordinates: Vec<usize>,
        path: String,
        title: String,
        breadcrumbs: Vec<OutlineReference>,
        entry: &'a DefinitionItem,
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

    fn path(&self) -> &str {
        match self {
            Self::Section { path, .. } | Self::Entry { path, .. } => path,
        }
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

    fn matches(&self, selector: &str) -> bool {
        if self.path() == selector || self.id() == selector {
            return true;
        }
        match self {
            Self::Entry { entry, .. } => entry.identity.as_ref().is_some_and(|identity| {
                identity
                    .names
                    .iter()
                    .any(|name| name == selector || name.trim_start_matches('-') == selector)
            }),
            Self::Section { .. } => false,
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
            } => ExcerptSelection::ManualSection {
                path: path.clone(),
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
            } => ExcerptSelection::ManualEntry {
                path: path.clone(),
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

fn collect_sections<'a>(
    sections: &'a [Section],
    parent_coordinates: &[usize],
    breadcrumbs: &[OutlineReference],
    output: &mut Vec<LocatedNode<'a>>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent_coordinates.to_vec();
        coordinates.push(index + 1);
        let path = format_path(&coordinates);
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
            path: path.clone(),
            id: section.id.clone(),
            title: section.title.clone(),
        });
        let mut entries = Vec::new();
        collect_definition_entries(&section.blocks, &mut entries);
        for (index, entry) in entries.into_iter().enumerate() {
            let Some(identity) = &entry.identity else {
                continue;
            };
            output.push(LocatedNode::Entry {
                order: output.len(),
                coordinates: coordinates.clone(),
                path: format!("{path}/o{}", index + 1),
                title: identity.names.join(", "),
                breadcrumbs: child_breadcrumbs.clone(),
                entry,
            });
        }
        collect_sections(&section.children, &coordinates, &child_breadcrumbs, output);
    }
}

fn collect_definition_entries<'a>(blocks: &'a [Block], output: &mut Vec<&'a DefinitionItem>) {
    for block in blocks {
        match block {
            Block::List { items, .. } => {
                for item in items {
                    collect_definition_entries(&item.blocks, output);
                }
            }
            Block::DefinitionList { items, .. } => {
                for item in items {
                    if item.identity.is_some() {
                        output.push(item);
                    }
                    collect_definition_entries(&item.description, output);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        collect_definition_entries(&cell.blocks, output);
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::Unsupported { .. } => {}
        }
    }
}

fn format_path(coordinates: &[usize]) -> String {
    coordinates
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn is_ancestor(ancestor: &[usize], descendant: &[usize]) -> bool {
    ancestor.len() < descendant.len() && descendant.starts_with(ancestor)
}

#[cfg(test)]
mod tests {
    use mant_ast::{
        DocumentMeta, DocumentSchema, DocumentSource, ExcerptSelection, MantDocument, OutlineNode,
        Producer, QueryBundle, QuerySchema, Section, SourceFormat, TldrDocument,
    };

    use super::{ProjectionError, build_outline, select_excerpt};

    fn section(id: &str, title: &str, children: Vec<Section>) -> Section {
        Section {
            id: id.to_owned(),
            title: title.to_owned(),
            spacing_before_lines: 0,
            blocks: Vec::new(),
            children,
            source: None,
        }
    }

    fn query() -> QueryBundle {
        QueryBundle {
            schema: QuerySchema::V2,
            topic: "demo".to_owned(),
            section: None,
            manual: Some(MantDocument {
                schema: DocumentSchema::V2,
                producer: Producer {
                    name: "test".to_owned(),
                    version: "1".to_owned(),
                    engine: None,
                },
                source: DocumentSource {
                    format: SourceFormat::Man,
                    path: Some("/man/demo.1".to_owned()),
                    renderer: None,
                },
                meta: DocumentMeta {
                    section: Some("1".to_owned()),
                    ..DocumentMeta::default()
                },
                diagnostics: Vec::new(),
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
        }
    }

    #[test]
    fn builds_one_based_tree_paths_without_copying_blocks() {
        let outline = build_outline(&query()).expect("outline");

        assert_eq!(outline.manual_section.as_deref(), Some("1"));
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
                | ExcerptSelection::ManualSection { path, .. }
                | ExcerptSelection::ManualEntry { path, .. } => path.as_str(),
            })
            .collect::<Vec<_>>();
        assert_eq!(paths, ["2", "3"]);
        let ExcerptSelection::ManualSection {
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

        let ExcerptSelection::ManualSection {
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
            [ExcerptSelection::Tldr { path, .. }, ExcerptSelection::ManualSection { .. }]
                if path == "0"
        ));

        let mut tldr_only = combined;
        tldr_only.manual = None;
        let outline = build_outline(&tldr_only).expect("tldr-only outline");
        assert_eq!(outline.nodes.len(), 1);
        assert_eq!(outline.nodes[0].path(), "0");
        assert!(outline.source.is_none());
        assert!(outline.meta.is_none());
    }

    #[test]
    fn reports_missing_content_and_unknown_or_empty_selectors() {
        let mut empty = query();
        empty.manual = None;
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
