//! Locks the public JSON shapes used for outline discovery and excerpts.

use mant_ast::{
    DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta, DocumentSource,
    ExcerptSchema, ExcerptSelection, OutlineDetail, OutlineNode, OutlineReference, OutlineSchema,
    Producer, QueryExcerpt, QueryOutline, Section, SourceFormat, TldrDocument,
};

fn source() -> DocumentSource {
    DocumentSource {
        format: SourceFormat::Man,
        path: Some("/man/demo.1".to_owned()),
        renderer: None,
    }
}

#[test]
fn outline_contract_exposes_both_human_paths_and_document_ids() {
    let outline = QueryOutline {
        schema: OutlineSchema::V3,
        detail: OutlineDetail::Options,
        label: "demo(1)".to_owned(),
        source: Some(source()),
        meta: Some(DocumentMeta::default()),
        nodes: vec![OutlineNode::DocumentSection {
            path: "2".to_owned(),
            id: "options-2".to_owned(),
            title: "OPTIONS".to_owned(),
            children: vec![OutlineNode::DocumentEntry {
                path: "2/o1".to_owned(),
                id: "all".to_owned(),
                title: "-a, --all".to_owned(),
                role: DefinitionRole::Option,
                names: vec!["-a".to_owned(), "--all".to_owned()],
            }],
        }],
    };

    let value = serde_json::to_value(outline).expect("outline JSON");
    assert_eq!(value["schema"], "mant.outline/v3");
    assert_eq!(value["detail"], "options");
    assert_eq!(value["label"], "demo(1)");
    assert_eq!(value["nodes"][0]["kind"], "document-section");
    assert_eq!(value["nodes"][0]["path"], "2");
    assert_eq!(value["nodes"][0]["children"][0]["kind"], "document-entry");
    assert_eq!(value["nodes"][0]["children"][0]["names"][1], "--all");
}

#[test]
fn excerpt_contract_keeps_breadcrumbs_separate_from_complete_sections() {
    let section = Section {
        id: "common-3".to_owned(),
        title: "Common options".to_owned(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V3,
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
            path: "2.1".to_owned(),
            id: section.id.clone(),
            title: section.title.clone(),
            breadcrumbs: vec![OutlineReference {
                path: "2".to_owned(),
                id: "options-2".to_owned(),
                title: "OPTIONS".to_owned(),
            }],
            section,
        }],
    };

    let value = serde_json::to_value(excerpt).expect("excerpt JSON");
    assert_eq!(value["schema"], "mant.excerpt/v3");
    assert_eq!(value["selections"][0]["kind"], "document-section");
    assert_eq!(value["selections"][0]["breadcrumbs"][0]["path"], "2");
    assert_eq!(value["selections"][0]["section"]["id"], "common-3");
    assert!(value.get("diagnostics").is_none());
}

#[test]
fn excerpt_contract_can_return_one_semantic_definition() {
    let entry = DefinitionItem {
        inline_term: false,
        identity: Some(DefinitionIdentity {
            id: "all".to_owned(),
            role: DefinitionRole::Option,
            names: vec!["-a".to_owned(), "--all".to_owned()],
        }),
        terms: Vec::new(),
        description: Vec::new(),
        spacing_before_lines: None,
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V3,
        label: "demo(1)".to_owned(),
        producer: None,
        source: Some(source()),
        meta: None,
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection::DocumentEntry {
            path: "2/o1".to_owned(),
            id: "all".to_owned(),
            title: "-a, --all".to_owned(),
            breadcrumbs: Vec::new(),
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
fn tldr_uses_the_reserved_zero_path_in_outline_and_excerpt_contracts() {
    let document = TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A demonstration.".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "/tldr/demo.md".to_owned(),
    };
    let outline = QueryOutline {
        schema: OutlineSchema::V3,
        detail: OutlineDetail::Sections,
        label: "demo".to_owned(),
        source: None,
        meta: None,
        nodes: vec![OutlineNode::Tldr {
            path: "0".to_owned(),
            id: "tldr".to_owned(),
            title: "TLDR QUICK REFERENCE".to_owned(),
        }],
    };
    let excerpt = QueryExcerpt {
        schema: ExcerptSchema::V3,
        label: "demo".to_owned(),
        producer: None,
        source: None,
        meta: None,
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection::Tldr {
            path: "0".to_owned(),
            id: "tldr".to_owned(),
            title: "TLDR QUICK REFERENCE".to_owned(),
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
