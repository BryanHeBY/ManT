//! Projects complete manual documents into outlines and selectable excerpts.

use std::{collections::HashSet, error::Error, fmt};

use mant_ast::{
    ExcerptSchema, ExcerptSelection, OutlineNode, OutlineNodeKind, OutlineReference, OutlineSchema,
    QueryBundle, QueryExcerpt, QueryOutline, Section,
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
    if query.tldr.is_none() && query.manual.is_none() {
        return Err(ProjectionError::MissingContent {
            topic: query.topic.clone(),
        });
    }
    let mut nodes = Vec::new();
    if query.tldr.is_some() {
        nodes.push(OutlineNode {
            kind: OutlineNodeKind::Tldr,
            path: TLDR_PATH.to_owned(),
            id: TLDR_ID.to_owned(),
            title: TLDR_TITLE.to_owned(),
            children: Vec::new(),
        });
    }
    if let Some(manual) = &query.manual {
        nodes.extend(outline_nodes(&manual.sections, &[]));
    }
    Ok(QueryOutline {
        schema: OutlineSchema::V1,
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
            .find(|candidate| candidate.path == selector || candidate.section.id == selector)
            .ok_or_else(|| ProjectionError::UnknownSelector {
                topic: query.topic.clone(),
                selector: selector.to_owned(),
            })?;
        if selected_ids.insert(candidate.section.id.as_str()) {
            selected.push(candidate);
        }
    }
    let selected_coordinates = selected
        .iter()
        .map(|candidate| candidate.coordinates.clone())
        .collect::<Vec<_>>();
    selected.retain(|candidate| {
        !selected_coordinates.iter().any(|other| {
            other != &candidate.coordinates && is_ancestor(other, &candidate.coordinates)
        })
    });
    selected.sort_by_key(|candidate| candidate.order);

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
    selections.extend(
        selected
            .into_iter()
            .map(|candidate| ExcerptSelection::ManualSection {
                path: candidate.path.clone(),
                id: candidate.section.id.clone(),
                title: candidate.section.title.clone(),
                breadcrumbs: candidate.breadcrumbs.clone(),
                section: candidate.section.clone(),
            }),
    );

    Ok(QueryExcerpt {
        schema: ExcerptSchema::V1,
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

fn outline_nodes(sections: &[Section], parent: &[usize]) -> Vec<OutlineNode> {
    sections
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let mut coordinates = parent.to_vec();
            coordinates.push(index + 1);
            OutlineNode {
                kind: OutlineNodeKind::ManualSection,
                path: format_path(&coordinates),
                id: section.id.clone(),
                title: section.title.clone(),
                children: outline_nodes(&section.children, &coordinates),
            }
        })
        .collect()
}

struct LocatedSection<'a> {
    order: usize,
    coordinates: Vec<usize>,
    path: String,
    breadcrumbs: Vec<OutlineReference>,
    section: &'a Section,
}

fn collect_sections<'a>(
    sections: &'a [Section],
    parent_coordinates: &[usize],
    breadcrumbs: &[OutlineReference],
    output: &mut Vec<LocatedSection<'a>>,
) {
    for (index, section) in sections.iter().enumerate() {
        let mut coordinates = parent_coordinates.to_vec();
        coordinates.push(index + 1);
        let path = format_path(&coordinates);
        let order = output.len();
        output.push(LocatedSection {
            order,
            coordinates: coordinates.clone(),
            path: path.clone(),
            breadcrumbs: breadcrumbs.to_vec(),
            section,
        });
        let mut child_breadcrumbs = breadcrumbs.to_vec();
        child_breadcrumbs.push(OutlineReference {
            path,
            id: section.id.clone(),
            title: section.title.clone(),
        });
        collect_sections(&section.children, &coordinates, &child_breadcrumbs, output);
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
        DocumentMeta, DocumentSchema, DocumentSource, ExcerptSelection, MantDocument,
        OutlineNodeKind, Producer, QueryBundle, QuerySchema, Section, SourceFormat, TldrDocument,
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
            schema: QuerySchema::V1,
            topic: "demo".to_owned(),
            section: None,
            manual: Some(MantDocument {
                schema: DocumentSchema::V1,
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
        assert_eq!(outline.nodes[1].path, "2");
        assert_eq!(outline.nodes[1].id, "options-2");
        assert_eq!(outline.nodes[1].children[0].path, "2.1");
        assert_eq!(outline.nodes[1].children[1].path, "2.2");
    }

    #[test]
    fn prepends_tldr_as_zero_without_renumbering_manual_sections() {
        let mut query = query();
        query.tldr = Some(tldr());

        let outline = build_outline(&query).expect("combined outline");

        assert_eq!(outline.nodes[0].kind, OutlineNodeKind::Tldr);
        assert_eq!(outline.nodes[0].path, "0");
        assert_eq!(outline.nodes[0].id, "tldr");
        assert_eq!(outline.nodes[1].path, "1");
        assert_eq!(outline.nodes[2].path, "2");
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
                | ExcerptSelection::ManualSection { path, .. } => path.as_str(),
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
        assert_eq!(outline.nodes[0].path, "0");
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
