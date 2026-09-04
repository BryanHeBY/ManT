//! Locks the public JSON shapes used for outline discovery and excerpts.

use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentAddress,
    DocumentMeta, DocumentSource, EntryKind, EntrySummary, Inline, LayoutHint, ParameterKind,
    Section, SemanticDocumentReference, SourceFormat, TldrDocument, TldrOrigin,
};
use mant_protocol::{
    EntryDocumentTarget, EntryProjection, EntryValueDomain, ExcerptSchema, ExcerptSelection,
    OutlineNode, OutlineNodeReference, OutlineReference, OutlineSchema, OutlineTrail, Producer,
    QueryExcerpt, QueryOutline,
};

fn source() -> DocumentSource {
    DocumentSource {
        format: SourceFormat::Man,
        path: Some("/man/demo.1".to_owned()),
    }
}

#[test]
fn outline_contract_exposes_both_human_paths_and_document_ids() {
    let outline = QueryOutline {
        schema: OutlineSchema::V0Dot11,
        entries: EntryProjection::All,
        root: None,
        label: "demo(1)".to_owned(),
        address: Some(DocumentAddress::Manual {
            name: "demo".to_owned(),
            manual_section: "1".to_owned(),
        }),
        source: Some(source()),
        meta: Some(DocumentMeta::default()),
        diagnostics: Vec::new(),
        semantics_complete: true,
        nodes: vec![OutlineNode::DocumentSection {
            path: "2".to_owned().into(),
            id: "options-2".to_owned().into(),
            title: "OPTIONS".to_owned(),
            entry_summary: Some(EntrySummary::default()),
            children: vec![OutlineNode::DocumentEntry {
                path: "2/e1".to_owned().into(),
                id: "all".to_owned().into(),
                title: "-a, --all".to_owned(),
                entry_kind: EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                case: DefinitionCase::Sensitive,
                aliases: vec!["-a".to_owned(), "--all".to_owned()],
                forms: vec!["-a, --all".to_owned()],
                document_targets: vec![EntryDocumentTarget {
                    label: "help(1)".to_owned(),
                    reference: SemanticDocumentReference::Manual {
                        name: "help".to_owned(),
                        manual_section: Some("1".to_owned()),
                    },
                    address: Some(DocumentAddress::Manual {
                        name: "help".to_owned(),
                        manual_section: "1".to_owned(),
                    }),
                }],
                value_domain: Some(Box::new(EntryValueDomain::Choices { exhaustive: false })),
                entry_summary: Some(EntrySummary::default()),
                children: Vec::new(),
            }],
        }],
    };

    let value = serde_json::to_value(outline).expect("outline JSON");
    assert_eq!(value["schema"], "mant.outline/v0.11");
    assert_eq!(value["entries"]["kind"], "all");
    assert_eq!(value["label"], "demo(1)");
    assert_eq!(value["nodes"][0]["kind"], "document-section");
    assert_eq!(value["nodes"][0]["path"], "2");
    assert_eq!(value["nodes"][0]["children"][0]["kind"], "document-entry");
    assert_eq!(
        value["nodes"][0]["children"][0]["entryKind"]["parameterKind"],
        "option"
    );
    assert!(
        value["nodes"][0]["children"][0]["entryKind"]
            .get("parameter_kind")
            .is_none()
    );
    assert_eq!(value["nodes"][0]["children"][0]["aliases"][1], "--all");
    assert_eq!(
        value["nodes"][0]["children"][0]["documentTargets"][0]["address"]["name"],
        "help"
    );
    assert_eq!(
        value["nodes"][0]["children"][0]["valueDomain"]["kind"],
        "choices"
    );
    assert!(value.get("diagnostics").is_none());
    assert!(value.get("semanticsComplete").is_none());
}

#[test]
fn outline_optional_diagnostic_fields_default_to_a_complete_result() {
    let outline: QueryOutline = serde_json::from_value(serde_json::json!({
        "schema": "mant.outline/v0.11",
        "entries": {"kind": "summary"},
        "label": "demo",
        "nodes": [],
    }))
    .expect("outline optional-field defaults");

    assert!(outline.semantics_complete);
    assert!(outline.diagnostics.is_empty());
}

#[test]
fn excerpt_contract_keeps_breadcrumbs_separate_from_complete_sections() {
    let section = Section {
        id: "common-3".to_owned().into(),
        fragment_aliases: Vec::new(),
        title: "Common options".to_owned(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V0Dot11,
        label: "demo(1)".to_owned(),
        producer: Some(Producer {
            name: "mant".to_owned(),
            version: "1".to_owned(),
            engine: None,
        }),
        source: Some(source()),
        meta: Some(DocumentMeta::default()),
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection::DocumentSection {
            outline: OutlineTrail {
                ancestors: vec![OutlineReference {
                    path: "2".to_owned().into(),
                    id: "options-2".to_owned().into(),
                    title: "OPTIONS".to_owned(),
                }],
                node: OutlineNodeReference::DocumentSection {
                    path: "2.1".to_owned().into(),
                    id: section.id.clone(),
                    title: section.title.clone(),
                },
            },
            section,
        }],
    };

    let value = serde_json::to_value(excerpt).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.excerpt/v0.11");
    assert_eq!(value["selections"][0]["kind"], "document-section");
    assert_eq!(
        value["selections"][0]["outline"]["ancestors"][0]["path"],
        "2"
    );
    assert_eq!(value["selections"][0]["outline"]["node"]["path"], "2.1");
    assert_eq!(value["selections"][0]["section"]["id"], "common-3");
    assert!(value.get("diagnostics").is_none());
}

#[test]
fn excerpt_contract_can_return_one_semantic_definition() {
    let entry = DefinitionItem {
        inline_term: false,
        identity: Some(DefinitionIdentity {
            id: "all".to_owned().into(),
            role: DefinitionRole::Option,
            case: DefinitionCase::Sensitive,
            names: vec!["-a".to_owned(), "--all".to_owned()],
            value_domain: None,
        }),
        terms: Vec::new(),
        description: Vec::new(),
        spacing_before_lines: None,
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V0Dot11,
        label: "demo(1)".to_owned(),
        producer: None,
        source: Some(source()),
        meta: None,
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection::DocumentEntry {
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::DocumentEntry {
                    path: "2/e1".to_owned().into(),
                    id: "all".to_owned().into(),
                    title: "-a, --all".to_owned(),
                    role: DefinitionRole::Option,
                    case: DefinitionCase::Sensitive,
                    names: vec!["-a".to_owned(), "--all".to_owned()],
                },
            },
            entry,
        }],
    };

    let value = serde_json::to_value(excerpt).expect("entry excerpt JSON");
    assert_eq!(value["selections"][0]["kind"], "document-entry");
    assert_eq!(
        value["selections"][0]["entry"]["identity"]["role"],
        "option"
    );
}

#[test]
fn document_root_contract_addresses_content_before_the_first_heading() {
    let blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Document preface.".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let outline = QueryOutline {
        schema: OutlineSchema::V0Dot11,
        entries: EntryProjection::None,
        root: None,
        label: "guide.md".to_owned(),
        address: None,
        source: Some(DocumentSource {
            format: SourceFormat::Markdown,
            path: Some("guide.md".to_owned()),
        }),
        meta: Some(DocumentMeta::default()),
        diagnostics: Vec::new(),
        semantics_complete: true,
        nodes: vec![OutlineNode::DocumentRoot {
            path: "root".to_owned().into(),
            id: "document-overview".to_owned().into(),
            title: "OVERVIEW".to_owned(),
            entry_summary: None,
            children: Vec::new(),
        }],
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V0Dot11,
        label: "guide.md".to_owned(),
        producer: None,
        source: outline.source.clone(),
        meta: outline.meta.clone(),
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection::DocumentRoot {
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::DocumentRoot {
                    path: "root".to_owned().into(),
                    id: "document-overview".to_owned().into(),
                    title: "OVERVIEW".to_owned(),
                },
            },
            blocks,
        }],
    };

    let outline = serde_json::to_value(outline).expect("root outline JSON");
    let excerpt = serde_json::to_value(excerpt).expect("root excerpt JSON");
    assert_eq!(outline["nodes"][0]["kind"], "document-root");
    assert_eq!(outline["nodes"][0]["path"], "root");
    assert_eq!(excerpt["selections"][0]["kind"], "document-root");
    assert_eq!(
        excerpt["selections"][0]["blocks"][0]["children"][0]["value"],
        "Document preface."
    );
}

#[test]
fn tldr_uses_the_reserved_zero_path_in_outline_and_excerpt_contracts() {
    let document = TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A demonstration.".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/tldr/demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    };
    let outline = QueryOutline {
        schema: OutlineSchema::V0Dot11,
        entries: EntryProjection::None,
        root: None,
        label: "demo".to_owned(),
        address: None,
        source: None,
        meta: None,
        diagnostics: Vec::new(),
        semantics_complete: true,
        nodes: vec![OutlineNode::Tldr {
            path: "0".to_owned().into(),
            id: "tldr".to_owned().into(),
            title: "TLDR QUICK REFERENCE".to_owned(),
        }],
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V0Dot11,
        label: "demo".to_owned(),
        producer: None,
        source: None,
        meta: None,
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection::Tldr {
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::Tldr {
                    path: "0".to_owned().into(),
                    id: "tldr".to_owned().into(),
                    title: "TLDR QUICK REFERENCE".to_owned(),
                },
            },
            document,
        }],
    };

    let outline = serde_json::to_value(outline).expect("tldr outline JSON");
    let excerpt = serde_json::to_value(excerpt).expect("tldr excerpt JSON");
    assert_eq!(outline["nodes"][0]["kind"], "tldr");
    assert_eq!(outline["nodes"][0]["path"], "0");
    assert!(outline.get("source").is_none());
    assert_eq!(excerpt["selections"][0]["kind"], "tldr");
    assert_eq!(excerpt["selections"][0]["document"]["title"], "demo");
    assert!(excerpt.get("producer").is_none());
}
