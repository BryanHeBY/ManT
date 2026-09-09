use mant_ir::visit::{self, Visit, VisitMut};
use mant_ir::{Document, DocumentIndex, Heading, Inline, LinkTarget, Section};

fn document() -> Document {
    serde_json::from_value(serde_json::json!({
        "source": {"format": "markdown"}, "meta": {},
        "heading": {"content": [{"type":"link", "target":{"kind":"document", "name":"catalog"}, "children": [{"type":"code", "value":"Catalog"}]}]},
        "sections": [{"id":"topic", "heading":{"content":[{"type":"link", "target":{"kind":"section", "id":"topic"}, "children":[{"type":"emphasis", "children":[{"type":"text", "value":"Topic"}]}]}]}, "blocks":[], "children":[]}]
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
    struct Rename;
    impl VisitMut for Rename {
        fn visit_inline_mut(&mut self, inline: &mut Inline) {
            if let Inline::Link {
                target: LinkTarget::Document { name, .. },
                ..
            } = inline
            {
                *name = "other".into();
            }
            visit::walk_inline_mut(self, inline);
        }
    }
    let mut doc = document();
    assert_eq!(doc.display_title().as_deref(), Some("Catalog"));
    assert!(doc.meta.title.is_none());
    assert_eq!(doc.sections[0].heading.plain_text(), "Topic");
    assert!(DocumentIndex::build(&doc).contains(mant_ir::DOCUMENT_ROOT_ID));
    assert!(mant_ir::validate_document(&doc).is_empty());
    let mut links = Links::default();
    links.visit_document(&doc);
    assert_eq!(links.0, 2);
    Rename.visit_document_mut(&mut doc);
    let restored: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(restored, doc);
    assert_eq!(DocumentIndex::build(&restored), DocumentIndex::build(&doc));
    assert!(
        matches!(&restored.heading.unwrap().content[0], Inline::Link {target: LinkTarget::Document {name, ..}, ..} if name=="other")
    );
}

#[test]
fn obsolete_plain_section_titles_and_unknown_heading_fields_are_rejected() {
    let mut value = serde_json::to_value(&document().sections[0]).unwrap();
    value.as_object_mut().unwrap().remove("heading");
    value["title"] = "Topic".into();
    assert!(serde_json::from_value::<Section>(value).is_err());
    assert!(
        serde_json::from_value::<Heading>(serde_json::json!({"content":[], "title":"hidden"}))
            .is_err()
    );
}
