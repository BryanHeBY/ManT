//! Projects complete manual documents into outlines and selectable excerpts.

use std::{collections::HashSet, error::Error, fmt};

use mant_ast::{
    ExcerptSchema, ExcerptSelection, ManualExcerpt, ManualOutline, OutlineNode, OutlineReference,
    OutlineSchema, QueryBundle, Section,
};

/// Failure to derive a manual-only view from a complete query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    MissingManual { topic: String },
    EmptySelection,
    EmptySelector,
    UnknownSelector { topic: String, selector: String },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingManual { topic } => {
                write!(formatter, "manual page '{topic}' is unavailable")
            }
            Self::EmptySelection => formatter.write_str("at least one outline node is required"),
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::UnknownSelector { topic, selector } => write!(
                formatter,
                "manual '{topic}' has no outline node '{selector}'; run 'mant-cli {topic} --outline'"
            ),
        }
    }
}

impl Error for ProjectionError {}

/// Build a block-free, addressable outline for one queried manual.
///
/// # Errors
///
/// Returns [`ProjectionError::MissingManual`] for a tldr-only query.
pub fn build_outline(query: &QueryBundle) -> Result<ManualOutline, ProjectionError> {
    let manual = query
        .manual
        .as_ref()
        .ok_or_else(|| ProjectionError::MissingManual {
            topic: query.topic.clone(),
        })?;
    Ok(ManualOutline {
        schema: OutlineSchema::V1,
        topic: query.topic.clone(),
        manual_section: resolved_manual_section(query),
        source: manual.source.clone(),
        meta: manual.meta.clone(),
        nodes: outline_nodes(&manual.sections, &[]),
    })
}

/// Select complete section subtrees by one-based outline path or document ID.
///
/// Duplicate selections and descendants of another selected node are omitted.
/// The result always follows source order, independent of argument order.
///
/// # Errors
///
/// Returns an error when no manual exists or any selector is empty or unknown.
pub fn select_excerpt(
    query: &QueryBundle,
    selectors: &[String],
) -> Result<ManualExcerpt, ProjectionError> {
    if selectors.is_empty() {
        return Err(ProjectionError::EmptySelection);
    }
    let manual = query
        .manual
        .as_ref()
        .ok_or_else(|| ProjectionError::MissingManual {
            topic: query.topic.clone(),
        })?;
    let mut located = Vec::new();
    collect_sections(&manual.sections, &[], &[], &mut located);

    let mut selected_ids = HashSet::new();
    let mut selected = Vec::new();
    for raw_selector in selectors {
        let selector = raw_selector.trim();
        if selector.is_empty() {
            return Err(ProjectionError::EmptySelector);
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

    Ok(ManualExcerpt {
        schema: ExcerptSchema::V1,
        topic: query.topic.clone(),
        manual_section: resolved_manual_section(query),
        producer: manual.producer.clone(),
        source: manual.source.clone(),
        meta: manual.meta.clone(),
        diagnostics: manual.diagnostics.clone(),
        selections: selected
            .into_iter()
            .map(|candidate| ExcerptSelection {
                path: candidate.path.clone(),
                id: candidate.section.id.clone(),
                title: candidate.section.title.clone(),
                breadcrumbs: candidate.breadcrumbs.clone(),
                section: candidate.section.clone(),
            })
            .collect(),
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
        DocumentMeta, DocumentSchema, DocumentSource, MantDocument, Producer, QueryBundle,
        QuerySchema, Section, SourceFormat,
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

        assert_eq!(
            excerpt
                .selections
                .iter()
                .map(|selection| selection.path.as_str())
                .collect::<Vec<_>>(),
            ["2", "3"]
        );
        assert_eq!(excerpt.selections[0].section.children.len(), 2);
        assert!(excerpt.selections[0].breadcrumbs.is_empty());
    }

    #[test]
    fn child_selection_retains_ancestor_breadcrumbs() {
        let excerpt = select_excerpt(&query(), &["2.2".to_owned()]).expect("excerpt");

        assert_eq!(excerpt.selections[0].title, "Other options");
        assert_eq!(excerpt.selections[0].breadcrumbs[0].path, "2");
        assert_eq!(excerpt.selections[0].breadcrumbs[0].title, "OPTIONS");
    }

    #[test]
    fn reports_missing_manual_and_unknown_or_empty_selectors() {
        let mut tldr_only = query();
        tldr_only.manual = None;
        assert!(matches!(
            build_outline(&tldr_only),
            Err(ProjectionError::MissingManual { .. })
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
