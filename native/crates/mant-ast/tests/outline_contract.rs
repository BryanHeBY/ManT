//! Locks the public JSON shapes used for outline discovery and excerpts.

use mant_ast::{
    DocumentMeta, DocumentSource, ExcerptSchema, ExcerptSelection, ManualExcerpt, ManualOutline,
    OutlineNode, OutlineReference, OutlineSchema, Producer, Section, SourceFormat,
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
    let outline = ManualOutline {
        schema: OutlineSchema::V1,
        topic: "demo".to_owned(),
        manual_section: Some("1".to_owned()),
        source: source(),
        meta: DocumentMeta::default(),
        nodes: vec![OutlineNode {
            path: "2".to_owned(),
            id: "options-2".to_owned(),
            title: "OPTIONS".to_owned(),
            children: vec![OutlineNode {
                path: "2.1".to_owned(),
                id: "common-3".to_owned(),
                title: "Common options".to_owned(),
                children: Vec::new(),
            }],
        }],
    };

    let value = serde_json::to_value(outline).expect("outline JSON");
    assert_eq!(value["schema"], "mant.outline/v1");
    assert_eq!(value["manualSection"], "1");
    assert_eq!(value["nodes"][0]["path"], "2");
    assert_eq!(value["nodes"][0]["children"][0]["id"], "common-3");
}

#[test]
fn excerpt_contract_keeps_breadcrumbs_separate_from_complete_sections() {
    let section = Section {
        id: "common-3".to_owned(),
        title: "Common options".to_owned(),
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    };
    let excerpt = ManualExcerpt {
        schema: ExcerptSchema::V1,
        topic: "demo".to_owned(),
        manual_section: Some("1".to_owned()),
        producer: Producer {
            name: "mant".to_owned(),
            version: "1".to_owned(),
            engine: None,
        },
        source: source(),
        meta: DocumentMeta::default(),
        diagnostics: Vec::new(),
        selections: vec![ExcerptSelection {
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
    assert_eq!(value["schema"], "mant.excerpt/v1");
    assert_eq!(value["selections"][0]["breadcrumbs"][0]["path"], "2");
    assert_eq!(value["selections"][0]["section"]["id"], "common-3");
    assert!(value.get("diagnostics").is_none());
}
