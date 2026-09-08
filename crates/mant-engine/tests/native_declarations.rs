//! Source-owned declaration boundaries, independent of formatting conventions.
use mant_ir::{
    Block, DefinitionItem, Document,
    visit::{self, Visit},
};
use mant_protocol::{EvidenceClass, ExplanationOptions, ExplanationQuery};

fn definitions(document: &Document) -> Vec<&DefinitionItem> {
    struct Items<'a>(Vec<&'a DefinitionItem>);
    impl<'a> Visit<'a> for Items<'a> {
        fn visit_definition_item(&mut self, item: &'a DefinitionItem) {
            self.0.push(item);
            visit::walk_definition_item(self, item);
        }
    }
    let mut result = Items(Vec::new());
    result.visit_document(document);
    result.0
}

#[test]
fn compact_independent_heads_do_not_share_explain_bodies_or_sources() {
    for source in [
        ".TH PROBE 1\n.SH OPTIONS\n.IP --first 4\n.PD 0\n.IP --second 4\n.PD\nSECOND_BODY\n.IP --last 4\n",
        ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\n.PD 0\n.TP\n.B --second\n.PD\nSECOND_BODY\n.TP\n.B --last\n",
        ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.Tg First.Target\n.It Fl -first\n.Tg Second.Target\n.It Fl -second\nSECOND_BODY\n.Tg Last.Target\n.It Fl -last\n.El\n",
    ] {
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        assert!(mant_ir::validate_document(document).is_empty());
        let items = definitions(document);
        assert_eq!(items.len(), 3);
        assert_ne!(items[0].source, items[1].source);
        for (index, name) in ["--first", "--second", "--last"].iter().enumerate() {
            let entry = items[index].entry.as_ref().unwrap();
            assert_eq!(entry.names, [*name]);
            assert!(entry.alias_groups.is_empty());
            let result = mant_engine::explain_query(
                &query,
                &ExplanationQuery {
                    entry: (*name).into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            let direct = result
                .evidence
                .iter()
                .filter(|e| e.class == EvidenceClass::DirectEntry)
                .collect::<Vec<_>>();
            assert_eq!(direct.len(), 1);
            assert_eq!(direct[0].source, items[index].source);
            let excerpt = mant_engine::select_excerpt(&query, &[direct[0].outline.path()]).unwrap();
            let text = mant_engine::render_excerpt_text(&excerpt);
            assert_eq!(text.contains("SECOND_BODY"), index == 1, "{name}: {text}");
        }
        if source.contains("First.Target") {
            let json = serde_json::to_string(document).unwrap();
            for target in ["First.Target", "Second.Target", "Last.Target"] {
                assert!(json.contains(target), "lost {target}");
            }
        }
    }
}

#[test]
fn explicit_tq_groups_only_the_immediately_preceding_empty_head() {
    for tail in ["", "BODY\n"] {
        let source = format!(
            ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --orphan\n.TP\n.B --first\n.TQ\n.B --second\n.TQ\n.B --third\n{tail}"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
        let items = definitions(query.document.as_ref().unwrap());
        assert_eq!(items.len(), 2);
        assert!(items[0].description.is_empty());
        assert_eq!(
            items[1].entry.as_ref().unwrap().names,
            ["--first", "--second", "--third"]
        );
        assert_eq!(items[1].description.is_empty(), tail.is_empty());
        assert!(items[1].entry.as_ref().unwrap().alias_groups.is_empty());
        assert_eq!(items[1].source.unwrap().line, 5);
    }
}

#[test]
fn an_explicit_tq_after_a_body_does_not_steal_that_body() {
    let source =
        b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B --first\nFIRST_BODY\n.TQ\n.B --second\nSECOND_BODY\n";
    let query = mant_engine::query_roff_bytes(source).unwrap();
    let items = definitions(query.document.as_ref().unwrap());
    assert_eq!(items.len(), 2);
    for (item, expected) in items.iter().zip(["FIRST_BODY", "SECOND_BODY"]) {
        let [Block::Paragraph { children, .. }] = item.description.as_slice() else {
            panic!("body")
        };
        assert!(serde_json::to_string(children).unwrap().contains(expected));
    }
}
