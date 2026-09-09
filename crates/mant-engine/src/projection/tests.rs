use crate::ResolvedContent;
use mant_ir::{
    Block, DefinitionItem, Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource,
    EntryFacts, EntryKind, Inline, LayoutHint, NameCase, ParameterKind, Section, SourceFormat,
    TldrDocument, TldrOrigin,
};
use mant_protocol::{ContentSelector, EntryProjection, ExcerptSelection, OutlineNode};

use super::{ProjectionError, build_outline, build_outline_projection, select_excerpt};

fn section(id: &str, title: &str, children: Vec<Section>) -> Section {
    Section {
        id: id.to_owned().into(),
        fragment_aliases: Vec::new(),
        heading: title.into(),
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
            heading: None,
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
    role: EntryKind,
    names: &[&str],
    forms: &[&str],
    description: Vec<Block>,
) -> DefinitionItem {
    DefinitionItem {
        source: None,
        entry: Some(EntryFacts {
            name_bindings: names
                .iter()
                .enumerate()
                .map(|(name, spelling)| mant_ir::EntryNameBinding {
                    name,
                    evidence: mant_ir::EntryNameEvidence::Declared,
                    occurrences: forms
                        .iter()
                        .enumerate()
                        .filter_map(|(index, form)| {
                            form.find(spelling).map(|start| mant_ir::EntryForm {
                                parts: vec![mant_ir::EntryContentSlice {
                                    root: mant_ir::EntryInlineRoot::Term { index },
                                    path: vec![0],
                                    bytes: Some(start..start + spelling.len()),
                                }],
                            })
                        })
                        .collect(),
                })
                .collect(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: (0..forms.len()).map(mant_ir::EntryForm::term).collect(),
            id: id.into(),
            kind: role,
            case: NameCase::Sensitive,
            names: names.iter().map(|alias| (*alias).to_owned()).collect(),
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
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
            ..Default::default()
        },
    }
}

fn query_with_semantic_entries() -> ResolvedContent {
    let value = definition(
        "value-yes",
        EntryKind::Value,
        &["yes"],
        &["yes"],
        Vec::new(),
    );
    let local_forward = definition(
        "option-local-forward",
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        },
        &["-L"],
        &["-L port:host:hostport", "-L socket:remote_socket"],
        vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![value],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
    );
    let marker = definition(
        "marker-end-options",
        EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Marker,
        },
        &["--"],
        &["--"],
        Vec::new(),
    );
    let mut query = query();
    query.document.as_mut().expect("document").sections[1]
        .blocks
        .push(Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![local_forward, marker],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });
    query
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
            declaration_groups: Vec::new(),
            items: vec![definition(
                "generic-readline-term",
                EntryKind::Term,
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
        let excerpt = select_excerpt(&query, &[ContentSelector::path(path.as_str())])
            .unwrap_or_else(|error| panic!("read must accept projected path {path}: {error}"));
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { outline, .. }] if outline.path() == path
        ));
        let explanation = super::select_explanation(&query, &path)
            .unwrap_or_else(|error| panic!("explain must accept projected path {path}: {error}"));
        assert!(explanation.evidence.iter().any(|e| {
            e.outline.path() == path
                && e.bases
                    .iter()
                    .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Identity { .. }))
        }));
    }
}

#[test]
fn outline_root_preserves_identity_excludes_siblings_and_rejects_names() {
    let mut query = query_with_semantic_entries();
    let section_rooted = build_outline_projection(
        &query,
        EntryProjection::All,
        Some(ContentSelector::id("options-2")),
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
        Some(ContentSelector::id("option-local-forward")),
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
            declaration_groups: Vec::new(),
            items: vec![definition(
                "option-other-local-forward",
                EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option,
                },
                &["-L"],
                &["-L path"],
                Vec::new(),
            )],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });
    let error = build_outline_projection(
        &query,
        EntryProjection::All,
        Some(ContentSelector::id("-L")),
    )
    .expect_err("ambiguous names must require qualification");
    assert_eq!(error, ProjectionError::InvalidSelector);
}

#[test]
fn explicit_section_ids_are_independent_from_semantic_explanations() {
    let mut query = query();
    query.document.as_mut().expect("document").sections[0] = Section {
        id: "force".into(),
        fragment_aliases: Vec::new(),
        heading: "Force".into(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    };
    query.document.as_mut().expect("document").sections[1]
        .blocks
        .push(Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![definition(
                "command-force",
                EntryKind::Command,
                &["force"],
                &["force"],
                Vec::new(),
            )],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });

    let excerpt =
        select_excerpt(&query, &[ContentSelector::id("force")]).expect("exact section ID");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::DocumentSection { outline, .. }] if outline.path() == "1"
    ));
    let explanation = super::select_explanation(&query, "force").unwrap();
    assert!(explanation.evidence.iter().any(|e| {
        e.outline.node.id() == "command-force"
            && e.bases
                .iter()
                .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. }))
    }));
    let outline = build_outline_projection(
        &query,
        EntryProjection::All,
        Some(ContentSelector::id("force")),
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
fn custom_producer_impact_reaches_outline_and_excerpt_without_known_codes() {
    for impact in [
        mant_ir::DiagnosticImpact::None,
        mant_ir::DiagnosticImpact::SemanticCoverage,
    ] {
        let mut content = query();
        content
            .document
            .as_mut()
            .unwrap()
            .diagnostics
            .push(Diagnostic {
                impact,
                level: DiagnosticLevel::Style,
                code: Some("custom-producer.rejected-binding".into()),
                message: "producer coverage".into(),
                source: None,
            });
        let complete = impact == mant_ir::DiagnosticImpact::None;
        assert_eq!(
            build_outline(&content).unwrap().semantics_complete,
            complete
        );
        assert_eq!(
            select_excerpt(&content, &[ContentSelector::path("1")])
                .unwrap()
                .semantics_complete,
            complete
        );
    }
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

    let excerpt = select_excerpt(
        &query,
        &[
            mant_protocol::ContentSelector::id("document-overview"),
            mant_protocol::ContentSelector::path("root"),
        ],
    )
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
            ContentSelector::id("files-5"),
            ContentSelector::path("2.1"),
            ContentSelector::path("2"),
            ContentSelector::id("options-2"),
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
    let excerpt =
        select_excerpt(&query(), &[mant_protocol::ContentSelector::path("2.2")]).expect("excerpt");

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
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                source: None,
                entry: Some(EntryFacts {
                    name_bindings: Vec::new(),
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: Vec::new(),
                    id: "3".into(),
                    kind: EntryKind::Parameter {
                        parameter_kind: mant_ir::ParameterKind::Option,
                    },
                    case: NameCase::Sensitive,
                    names: vec!["-3".to_owned()],
                    value_domain: None,
                }),
                terms: vec![vec![Inline::Code {
                    value: "-3".to_owned(),
                }]],
                description: Vec::new(),
                layout: mant_ir::DefinitionLayout {
                    inline_term: false,
                    spacing_before_lines: None,
                    ..Default::default()
                },
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });

    let excerpt = select_excerpt(&query, &[mant_protocol::ContentSelector::path("3")])
        .expect("section path wins");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::DocumentSection { outline, .. }] if outline.path() == "3"
    ));
    let explanation = super::select_explanation(&query, "3").unwrap();
    assert!(explanation.evidence.iter().all(|e| {
        !matches!(
            e.outline.node,
            mant_protocol::OutlineNodeReference::DocumentSection { .. }
        ) || !e
            .bases
            .iter()
            .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Identity { .. }))
    }));
}

#[test]
fn selects_tldr_by_zero_or_id_and_supports_tldr_only_outlines() {
    let mut combined = query();
    combined.tldr = Some(tldr());
    let excerpt = select_excerpt(
        &combined,
        &[
            ContentSelector::path("2"),
            ContentSelector::id("tldr"),
            ContentSelector::path("0"),
        ],
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
        select_excerpt(&query(), &[] as &[mant_protocol::ContentSelector]),
        Err(ProjectionError::EmptySelection)
    );
    assert_eq!(
        select_excerpt(&query(), &[mant_protocol::ContentSelector::id(" ")]),
        Err(ProjectionError::InvalidSelector)
    );
    assert!(matches!(
        select_excerpt(&query(), &[mant_protocol::ContentSelector::path("9")]),
        Err(ProjectionError::UnknownSelector { .. })
    ));
}
