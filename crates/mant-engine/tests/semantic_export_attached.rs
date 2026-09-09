use mant_codec::encode::{MarkdownOptions, render_markdown_with_options};
use mant_ir::{
    Document, EntryFacts, ListItem,
    visit::{self, Visit},
};
use mant_loader::load_markdown_text;
use std::fmt::Write as _;

fn facts(doc: &Document) -> Vec<EntryFacts> {
    struct Entries(Vec<EntryFacts>);
    impl<'a> Visit<'a> for Entries {
        fn visit_list_item(&mut self, item: &'a ListItem) {
            if let Some(facts) = &item.entry {
                self.0.push(facts.clone());
            }
            visit::walk_list_item(self, item);
        }
    }
    let mut result = Entries(Vec::new());
    result.visit_document(doc);
    result.0
}
fn export(query: &mant_ir::ResolvedContent) -> String {
    render_markdown_with_options(
        query,
        MarkdownOptions {
            preserve_semantics: true,
            ..Default::default()
        },
    )
}

#[test]
fn attached_policy_is_proven_for_the_whole_list() {
    for (policy, heads) in [
        (
            " attached=fixed",
            "/F:Y\nperf=default\n--mode=auto\n--out FILE",
        ),
        ("", "/F:Y\n--extra ARG\n--mode=MODE\n--out FILE"),
    ] {
        let mut source = format!("<!-- mant:entries role=option case=insensitive{policy} -->\n");
        for (index, head) in heads.lines().enumerate() {
            writeln!(
                source,
                "- `{head}`: Description. <!-- mant:entry {{\"id\":\"entry-{index}\"}} -->"
            )
            .unwrap();
        }
        let query = load_markdown_text(&source, None).unwrap();
        assert!(query.document.as_ref().unwrap().diagnostics.is_empty());
        let exported = export(&query);
        assert_eq!(exported.contains("attached=fixed"), !policy.is_empty());
        let document = load_markdown_text(&exported, None)
            .unwrap()
            .document
            .unwrap();
        assert!(
            document.diagnostics.is_empty(),
            "{exported}\n{:?}",
            document.diagnostics
        );
        assert_eq!(facts(query.document.as_ref().unwrap()), facts(&document));
        assert!(mant_ir::validate_document(&document).is_empty());
    }
}

#[test]
fn fixed_groups_relations_and_nested_choices_survive_reimport() {
    let source = r#"<!-- mant:entries role=option case=insensitive attached=fixed -->
- `/F:Y`, `/cleanup`: Scan. <!-- mant:entry {"id":"scan","aliasGroups":[["/F:Y","/cleanup"]]} -->
- `/clean`: Clean. <!-- mant:entry {"id":"clean","aliasOf":"scan"} -->
- `--mode=auto`: Mode. <!-- mant:entry {"id":"auto"} -->

  <!-- mant:domain choices=exhaustive -->

  <!-- mant:entries role=value case=sensitive -->
  - `fast`: Fast.
  - `slow`: Slow.
"#;
    let query = load_markdown_text(source, None).unwrap();
    assert!(query.document.as_ref().unwrap().diagnostics.is_empty());
    let exported = export(&query);
    let document = load_markdown_text(&exported, None)
        .unwrap()
        .document
        .unwrap();
    assert!(
        document.diagnostics.is_empty(),
        "{exported}\n{:?}",
        document.diagnostics
    );
    assert_eq!(facts(query.document.as_ref().unwrap()), facts(&document));
    assert_eq!(facts(&document).len(), 5);
}

#[test]
fn incompatible_item_policies_fall_back_without_misleading_annotations() {
    let mut query = load_markdown_text("<!-- mant:entries role=option case=sensitive attached=fixed -->\n- `/F:Y`: Fixed.\n- `/G:X`: Placeholder.\n", None).unwrap();
    let inferred = load_markdown_text(
        "<!-- mant:entries role=option case=sensitive -->\n- `/G:X`: Placeholder.\n",
        None,
    )
    .unwrap();
    let mut inferred_facts = facts(inferred.document.as_ref().unwrap()).pop().unwrap();
    let doc = query.document.as_mut().unwrap();
    let mant_ir::Block::List { items, .. } = &mut doc.blocks[0] else {
        panic!("list")
    };
    inferred_facts.id = items[1].entry.as_ref().unwrap().id.clone();
    items[1].entry = Some(inferred_facts);
    assert!(mant_ir::validate_document(doc).is_empty());
    let exported = export(&query);
    assert!(!exported.contains("mant:entry"));
    assert!(!exported.contains("mant:entries"));
    assert!(exported.contains("/F:Y") && exported.contains("/G:X"));
}
