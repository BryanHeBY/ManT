use mant_ir::visit::{self, Visit};
use mant_ir::{
    ContentOwnerKind, ContentRootKind, ContentStoreBuilder, ContentStyle, Document, DocumentIndex,
    Heading, Inline, LinkTarget, Provenance, Section,
};

fn document() -> Document {
    serde_json::from_value(serde_json::json!({
        "sources": [{"key":1,"identity":{"kind":"anonymous","name":"test"},"format":"markdown","decodedByteLength":0,"coordinates":{"kind":"decoded-utf8-bytes"}}],
        "rootSource": 1, "meta": {},
        "body": {"kind":"flow", "contentStore": {
            "owners":[{"key":1,"kind":"document","roots":[1,2],"provenance":{"kind":"unknown"}}],
            "roots":[
                {"key":1,"owner":1,"kind":"heading","atoms":[1],"points":[],"provenance":{"kind":"unknown"}},
                {"key":2,"owner":1,"kind":"heading","atoms":[2],"points":[],"provenance":{"kind":"unknown"}}
            ],
            "atoms":[
                {"key":1,"root":1,"owner":1,"kind":"text","text":"Catalog","style":{"literal":true},"link":1,"provenance":{"kind":"unknown"}},
                {"key":2,"root":2,"owner":1,"kind":"text","text":"Topic","style":{"emphasis":true},"link":2,"provenance":{"kind":"unknown"}}
            ],
            "points":[],
            "links":[
                {"key":1,"owner":1,"target":{"kind":"document","name":"catalog"},"label":[{"kind":"content","content":{"atom":1,"bytes":{"start":0,"end":7}}}],"provenance":{"kind":"unknown"}},
                {"key":2,"owner":1,"target":{"kind":"section","id":"topic"},"label":[{"kind":"content","content":{"atom":2,"bytes":{"start":0,"end":5}}}],"provenance":{"kind":"unknown"}}
            ]
        },
        "heading": {"content": [{"type":"link", "occurrence":1, "children": [{"type":"code", "content":{"atom":1,"bytes":{"start":0,"end":7}}}]}]},
        "sections": [{"id":"topic", "heading":{"content":[{"type":"link", "occurrence":2, "children":[{"type":"emphasis", "children":[{"type":"text", "content":{"atom":2,"bytes":{"start":0,"end":5}}}]}]}]}, "blocks":[], "children":[]}]}
    })).unwrap()
}

#[test]
fn headings_are_authoritative_visited_once_and_rebuild_after_serde() {
    #[derive(Default)]
    struct Links(usize);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            self.0 += usize::from(matches!(inline, Inline::Link { .. }));
            visit::walk_inline(self, inline);
        }
    }
    let mut doc = document();
    assert_eq!(doc.display_title().as_deref(), Some("Catalog"));
    assert!(doc.meta.title.is_none());
    assert_eq!(
        doc.flow().unwrap().sections[0]
            .heading
            .plain_text(doc.content()),
        "Topic"
    );
    assert!(DocumentIndex::build(&doc).contains(mant_ir::DOCUMENT_ROOT_ID));
    assert!(mant_ir::validate_document(&doc).is_empty());
    let mut links = Links::default();
    links.visit_document(&doc);
    assert_eq!(links.0, 2);
    let Inline::Link { occurrence, .. } = doc.flow().unwrap().heading.as_ref().unwrap().content[0]
    else {
        unreachable!();
    };
    let LinkTarget::Document { name, .. } = &mut doc
        .flow_mut()
        .unwrap()
        .content_store
        .link_mut(occurrence)
        .expect("heading occurrence is retained")
        .target
    else {
        unreachable!();
    };
    *name = "other".into();
    let restored: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(restored, doc);
    assert_eq!(DocumentIndex::build(&restored), DocumentIndex::build(&doc));
    assert!(
        matches!(restored.flow().unwrap().content_store.link(occurrence).map(|link| &link.target), Some(LinkTarget::Document {name, ..}) if name=="other")
    );
}

#[test]
fn obsolete_plain_section_titles_and_unknown_heading_fields_are_rejected() {
    let mut value = serde_json::to_value(&document().flow().unwrap().sections[0]).unwrap();
    value.as_object_mut().unwrap().remove("heading");
    value["title"] = "Topic".into();
    assert!(serde_json::from_value::<Section>(value).is_err());
    assert!(
        serde_json::from_value::<Heading>(serde_json::json!({"content":[], "title":"hidden"}))
            .is_err()
    );
}

#[test]
fn single_line_heading_labels_normalize_structural_and_dynamic_breaks() {
    let mut store = ContentStoreBuilder::new();
    let owner = store.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
    let root = store.push_root(owner, ContentRootKind::Heading, Provenance::Unknown);
    let before = store.push_text(
        root,
        "before\t".into(),
        None,
        ContentStyle::default(),
        None,
        None,
        Provenance::Unknown,
    );
    let line_break = store.push_hard_break(root, None, Provenance::Unknown);
    let after = store.push_text(
        root,
        "after\nlast".into(),
        None,
        ContentStyle {
            emphasis: true,
            ..ContentStyle::default()
        },
        None,
        None,
        Provenance::Unknown,
    );
    let heading = Heading {
        content: vec![
            Inline::Text { content: before },
            Inline::LineBreak { atom: line_break },
            Inline::Emphasis {
                children: vec![Inline::Text { content: after }],
            },
        ],
        source: None,
    };
    let store = store.finish();
    assert_eq!(heading.plain_text(store.content()), "before\t\nafter\nlast");
    assert_eq!(
        heading.single_line_text(store.content()),
        "before after last"
    );
}
