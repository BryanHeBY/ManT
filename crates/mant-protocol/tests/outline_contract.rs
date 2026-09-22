//! Locks the public JSON shapes used for outline discovery and excerpts.

use mant_ir::{
    Block, DefinitionItem, DocumentAddress, DocumentMeta, DocumentReference, EntryFacts, EntryKind,
    EntrySummary, Inline, LayoutHint, NameCase, ParameterKind, Section, SourceCoordinates,
    SourceFormat, SourceIdentity, SourceKey, SourceRecord, TldrDocument, TldrOrigin,
};
use mant_protocol::{
    EntryDocumentTarget, EntryProjection, EntryValueDomain, ExcerptSchema, ExcerptSelection,
    OutlineNode, OutlineNodeReference, OutlineReference, OutlineSchema, OutlineTrail, Producer,
    QueryExcerpt, QueryOutline, SourceContext,
};

fn source(format: SourceFormat, name: &str) -> SourceContext {
    SourceContext {
        sources: vec![SourceRecord {
            key: SourceKey::FIRST,
            identity: SourceIdentity::Path {
                name: name.to_owned(),
            },
            format,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: SourceKey::FIRST,
    }
}

#[test]
fn outline_contract_exposes_both_human_paths_and_document_ids() {
    let outline = QueryOutline {
        references: mant_protocol::ReferenceInventory::default(),
        display_title: None,
        schema: OutlineSchema::V0Dot12,
        entries: EntryProjection::All,
        root: None,
        label: "demo(1)".to_owned(),
        address: Some(DocumentAddress::Manual {
            name: "demo".to_owned(),
            manual_section: "1".to_owned(),
        }),
        source_context: Some(source(SourceFormat::Man, "/man/demo.1")),
        meta: Some(DocumentMeta::default()),
        diagnostics: Vec::new(),
        semantics_complete: true,
        nodes: vec![OutlineNode::DocumentSection {
            path: "2".to_owned().into(),
            id: "options-2".to_owned().into(),
            title: "OPTIONS".to_owned(),
            entry_summary: Some(EntrySummary::default()),
            children: vec![OutlineNode::DocumentEntry {
                owner: Box::new(mant_ir::ContentReveal::Owner {
                    sections: vec![0],
                    blocks: vec![mant_ir::ContentBlockStep::Block { index: 0 }],
                    item_index: 0,
                }),
                path: "2/e1".to_owned().into(),
                id: "all".to_owned().into(),
                title: "-a, --all".to_owned(),
                entry_kind: EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                case: NameCase::Sensitive,
                names: vec!["-a".to_owned(), "--all".to_owned()],
                alias_groups: vec![vec!["-a".to_owned(), "--all".to_owned()]],
                alias_of: None,
                forms: vec!["-a, --all".to_owned()],
                document_targets: vec![EntryDocumentTarget {
                    label: "help(1)".to_owned(),
                    reference: DocumentReference::Manual {
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
    assert_eq!(value["schema"], "mant.outline/v0.12");
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
    assert_eq!(value["nodes"][0]["children"][0]["names"][1], "--all");
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
        "schema": "mant.outline/v0.12",
        "references": mant_protocol::ReferenceInventory::default(),
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
    let mut store = mant_ir::ContentStoreBuilder::new();
    let owner = store.push_owner(
        mant_ir::ContentOwnerKind::Section,
        mant_ir::Provenance::Unknown,
    );
    let root = store.push_root(
        owner,
        mant_ir::ContentRootKind::Heading,
        mant_ir::Provenance::Unknown,
    );
    let heading = store.push_text(
        root,
        "Common options".into(),
        None,
        mant_ir::ContentStyle::default(),
        None,
        None,
        mant_ir::Provenance::Unknown,
    );
    let content_projection = mant_ir::ContentProjection {
        content_store: store.finish(),
    };
    let section = Section {
        id: "common-3".to_owned().into(),
        fragment_aliases: Vec::new(),
        heading: mant_ir::Heading {
            content: vec![Inline::Text { content: heading }],
            source: None,
        },
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    };
    let excerpt = QueryExcerpt {
        display_title: None,
        schema: ExcerptSchema::V0Dot12,
        address: None,
        semantics_complete: true,
        label: "demo(1)".to_owned(),
        producer: Some(Producer {
            name: "mant".to_owned(),
            version: "1".to_owned(),
            engine: None,
        }),
        source_context: Some(source(SourceFormat::Man, "/man/demo.1")),
        meta: Some(DocumentMeta::default()),
        diagnostics: Vec::new(),
        content_projection: Some(content_projection),
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
                    title: "Common options".to_owned(),
                },
            },
            section,
        }],
    };

    let value = serde_json::to_value(excerpt).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.excerpt/v0.12");
    assert_eq!(value["selections"][0]["kind"], "document-section");
    assert_eq!(
        value["selections"][0]["outline"]["ancestors"][0]["path"],
        "2"
    );
    assert_eq!(value["selections"][0]["outline"]["node"]["path"], "2.1");
    assert_eq!(value["selections"][0]["section"]["id"], "common-3");
    assert!(value.get("diagnostics").is_none());
    serde_json::from_value::<QueryExcerpt>(value.clone()).expect("closed projection");
    let mut missing = value.clone();
    missing
        .as_object_mut()
        .expect("excerpt object")
        .remove("contentProjection");
    assert!(serde_json::from_value::<QueryExcerpt>(missing).is_err());
    let mut dangling = value;
    dangling["selections"][0]["section"]["heading"]["content"][0]["content"]["atom"] = 2.into();
    assert!(serde_json::from_value::<QueryExcerpt>(dangling).is_err());
}

#[test]
fn excerpt_contract_can_return_one_semantic_definition() {
    let entry = DefinitionItem {
        source: None,
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
            ..Default::default()
        },
        entry: Some(EntryFacts {
            name_bindings: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: Vec::new(),
            id: "all".to_owned().into(),
            kind: EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["-a".to_owned(), "--all".to_owned()],
            value_domain: None,
        }),
        terms: Vec::new(),
        description: Vec::new(),
    };
    let excerpt = QueryExcerpt {
        display_title: None,
        schema: ExcerptSchema::V0Dot12,
        label: "demo(1)".to_owned(),
        address: None,
        semantics_complete: true,
        producer: None,
        source_context: Some(source(SourceFormat::Man, "/man/demo.1")),
        meta: None,
        diagnostics: Vec::new(),
        content_projection: Some(mant_ir::ContentProjection {
            content_store: mant_ir::ContentStore::default(),
        }),
        selections: vec![ExcerptSelection::DocumentEntry {
            outline: OutlineTrail {
                ancestors: Vec::new(),
                node: OutlineNodeReference::DocumentEntry {
                    path: "2/e1".to_owned().into(),
                    id: "all".to_owned().into(),
                    title: "-a, --all".to_owned(),
                    entry_kind: EntryKind::Parameter {
                        parameter_kind: mant_ir::ParameterKind::Option,
                    },
                    case: NameCase::Sensitive,
                    names: vec!["-a".to_owned(), "--all".to_owned()],
                },
            },
            entry: Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![entry],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            },
        }],
    };

    let value = serde_json::to_value(excerpt).expect("entry excerpt JSON");
    assert_eq!(value["selections"][0]["kind"], "document-entry");
    assert_eq!(
        value["selections"][0]["entry"]["items"][0]["entry"]["kind"]["parameterKind"],
        "option"
    );
}

#[test]
fn document_root_contract_addresses_content_before_the_first_heading() {
    let mut store = mant_ir::ContentStoreBuilder::new();
    let owner = store.push_owner(
        mant_ir::ContentOwnerKind::Document,
        mant_ir::Provenance::Unknown,
    );
    let root = store.push_root(
        owner,
        mant_ir::ContentRootKind::Body,
        mant_ir::Provenance::Unknown,
    );
    let preface = store.push_text(
        root,
        "Document preface.".into(),
        None,
        mant_ir::ContentStyle::default(),
        None,
        None,
        mant_ir::Provenance::Unknown,
    );
    let blocks = vec![Block::Paragraph {
        children: vec![Inline::Text { content: preface }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let outline = QueryOutline {
        references: mant_protocol::ReferenceInventory::default(),
        display_title: None,
        schema: OutlineSchema::V0Dot12,
        entries: EntryProjection::None,
        root: None,
        label: "guide.md".to_owned(),
        address: None,
        source_context: Some(source(SourceFormat::Markdown, "guide.md")),
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
        display_title: None,
        schema: ExcerptSchema::V0Dot12,
        label: "guide.md".to_owned(),
        address: None,
        semantics_complete: true,
        producer: None,
        source_context: outline.source_context.clone(),
        meta: outline.meta.clone(),
        diagnostics: Vec::new(),
        content_projection: Some(mant_ir::ContentProjection {
            content_store: store.finish(),
        }),
        selections: vec![ExcerptSelection::DocumentRoot {
            heading: None,
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
        excerpt["contentProjection"]["contentStore"]["atoms"][0]["text"],
        "Document preface."
    );
    assert_eq!(
        excerpt["selections"][0]["blocks"][0]["children"][0]["content"]["atom"],
        1
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
        references: mant_protocol::ReferenceInventory::default(),
        display_title: None,
        schema: OutlineSchema::V0Dot12,
        entries: EntryProjection::None,
        root: None,
        label: "demo".to_owned(),
        address: None,
        source_context: None,
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
        display_title: None,
        schema: ExcerptSchema::V0Dot12,
        label: "demo".to_owned(),
        address: None,
        semantics_complete: true,
        producer: None,
        source_context: None,
        meta: None,
        diagnostics: Vec::new(),
        content_projection: None,
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
    assert!(outline.get("sourceContext").is_none());
    assert_eq!(excerpt["selections"][0]["kind"], "tldr");
    assert_eq!(excerpt["selections"][0]["document"]["title"], "demo");
    assert!(excerpt.get("producer").is_none());
}
