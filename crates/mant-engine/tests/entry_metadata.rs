//! Independent authoring expectations: relations annotate, never rewrite content.
use mant_engine::query_markdown_text;
use mant_ir::{
    Document, EntryFacts, ListItem,
    visit::{self, Visit},
};
use std::collections::BTreeMap;

fn parse(body: &str) -> Document {
    query_markdown_text(
        &format!("# Probe\n\n<!-- mant:entries role=option case=sensitive -->\n{body}\n"),
        None,
    )
    .unwrap()
    .document
    .unwrap()
}

fn facts(document: &Document) -> BTreeMap<String, EntryFacts> {
    struct Collect(BTreeMap<String, EntryFacts>);
    impl<'a> Visit<'a> for Collect {
        fn visit_list_item(&mut self, item: &'a ListItem) {
            if let Some(facts) = &item.entry {
                self.0.insert(facts.id.to_string(), facts.clone());
            }
            visit::walk_list_item(self, item);
        }
    }
    let mut result = Collect(BTreeMap::new());
    result.visit_document(document);
    result.0
}

#[test]
fn explicit_groups_and_forward_relations_keep_independent_owners() {
    let doc = parse(
        r#"- `-h`, `--help`: Help. <!-- mant:entry {"id":"help","aliasGroups":[["-h","--help"]]} -->
- `-S`, `--since`, `-U`, `--until`: Bounds. <!-- mant:entry {"id":"bounds","aliasGroups":[["-S","--since"],["-U","--until"]]} -->
- `--data-ascii <data>`: Alias body. <!-- mant:entry {"id":"ascii","aliasOf":"data"} -->
- `-d <data>`, `--data <data>`: Main body. <!-- mant:entry {"id":"data","aliasGroups":[["-d","--data"]]} -->
- `-x, --extra`: Extra. <!-- mant:entry {"id":"extra","aliasGroups":[["-x","--extra"]]} -->
"#,
    );
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    let entries = facts(&doc);
    assert_eq!(entries.len(), 5);
    assert_eq!(entries["help"].alias_groups, vec![vec!["-h", "--help"]]);
    assert_eq!(
        entries["bounds"].alias_groups,
        vec![vec!["-S", "--since"], vec!["-U", "--until"]]
    );
    assert_eq!(entries["ascii"].alias_of.as_deref(), Some("data"));
    assert_eq!(entries["extra"].alias_groups, vec![vec!["-x", "--extra"]]);
    let copied: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(doc, copied);
    assert!(mant_ir::validate_document(&doc).is_empty());
}

#[test]
fn invalid_objects_are_atomic_and_always_nonvisible() {
    for json in [
        r#"{"id":"bad","unknown":true}"#,
        r#"{"id":"bad","id":"other"}"#,
        r#"{"id":"bad","aliasGroups":false}"#,
        r#"{"id":null}"#,
        r#"{"id":"bad","aliasOf":3}"#,
        r#"{"id":"bad""#,
        "{\n\"id\":\"bad\"\n}",
    ] {
        let doc = parse(&format!(
            "- `-h`, `--help`: Help. <!-- mant:entry {json} -->"
        ));
        assert!(!doc.diagnostics.is_empty(), "{json}");
        assert!(
            facts(&doc)
                .values()
                .all(|entry| entry.alias_groups.is_empty()
                    && entry.alias_of.is_none()
                    && entry.id.as_str() != "bad")
        );
        assert!(
            !mant_engine::render_query_text(&mant_ir::ResolvedContent {
                address: None,
                label: "Probe".into(),
                document: Some(doc),
                tldr: None
            })
            .contains("mant:entry")
        );
    }
    for second in [r#"{"id":"second"}"#, r#"{"id":"first"}"#, "bad-json"] {
        let doc = parse(&format!(
            "- `--help`: Help. <!-- mant:entry {{\"id\":\"first\"}} --> <!-- mant:entry {second} -->"
        ));
        assert!(!doc.diagnostics.is_empty());
        assert!(facts(&doc).contains_key("option-help"));
    }
    let doc = parse(
        "- `--help`: Help. <!-- mant:entry {\"id\":\"first\"} -->\n\n  Later paragraph. <!-- mant:entry {\"id\":\"second\"} -->",
    );
    assert!(facts(&doc).contains_key("option-help"));
    assert!(!doc.diagnostics.is_empty());
}

#[test]
fn field_rejections_preserve_ids_content_and_independent_facts() {
    for groups in [
        r#"[["-h"]]"#,
        r#"[["-h","hidden"]]"#,
        r#"[["-h","--help"],["--help","-h"]]"#,
    ] {
        let doc = parse(&format!(
            "- `-h`, `--help`: Help. <!-- mant:entry {{\"id\":\"help\",\"aliasGroups\":{groups}}} -->"
        ));
        assert_eq!(facts(&doc)["help"].names, ["-h", "--help"]);
        assert!(facts(&doc)["help"].alias_groups.is_empty());
        assert!(doc.diagnostics.iter().any(|d| d.code.as_deref()
            == Some("ir.invalid-entry-alias-groups")
            && d.source.is_some()));
    }
    let doc = parse(
        r#"- `-h`, `--help`: Help. <!-- mant:entry {"id":"Invalid.ID","aliasGroups":[["-h","--help"]]} -->"#,
    );
    assert_eq!(
        facts(&doc)["option-h"].alias_groups,
        vec![vec!["-h", "--help"]]
    );
    assert!(!doc.diagnostics.is_empty());
}

#[test]
fn dangling_ambiguous_duplicate_incompatible_and_cyclic_relations_are_rejected() {
    for body in [
        r#"- `--one`: One. <!-- mant:entry {"id":"one","aliasOf":"missing"} -->"#,
        r#"- `--one`: One. <!-- mant:entry {"id":"one","aliasOf":"one"} -->"#,
        "- `--one`: One. <!-- mant:entry {\"id\":\"one\",\"aliasOf\":\"two\"} -->\n- `--two`: Two. <!-- mant:entry {\"id\":\"two\",\"aliasOf\":\"one\"} -->",
        "- `--one`: One. <!-- mant:entry {\"id\":\"one\",\"aliasOf\":\"bounds\"} -->\n- `--since`, `--until`: Bounds. <!-- mant:entry {\"id\":\"bounds\"} -->",
        "- `--one`: One. <!-- mant:entry {\"id\":\"same\"} -->\n- `--two`: Two. <!-- mant:entry {\"id\":\"same\"} -->\n- `--three`: Three. <!-- mant:entry {\"aliasOf\":\"same\"} -->",
    ] {
        let doc = parse(body);
        assert!(!doc.diagnostics.is_empty(), "{body}");
        assert!(facts(&doc).values().all(|e| e.alias_of.is_none()), "{body}");
        assert!(mant_ir::validate_document(&doc).is_empty(), "{body}");
    }
}

#[test]
fn block_and_inline_metadata_use_structural_item_ownership() {
    let doc = parse(
        "- `--parent`: Parent.\n\n  <!-- mant:entry {\"id\":\"parent\"} -->\n\n  <!-- mant:entries role=option case=sensitive -->\n  -\n    <!-- mant:entry {\"id\":\"child\"} -->\n\n    `--child`: Child.\n",
    );
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    let entries = facts(&doc);
    assert_eq!(entries["parent"].names, ["--parent"]);
    assert_eq!(entries["child"].names, ["--child"]);
    for body in [
        "- `--help`: Help. <!-- mant:entry {\"id\":\"wrong\"} --> More prose.",
        "- `--help`: Help.\n\n  Second paragraph. <!-- mant:entry {\"id\":\"wrong\"} -->",
        "- `--help`: Help.\n\n  > <!-- mant:entry {\"id\":\"wrong\"} -->",
    ] {
        let doc = parse(body);
        assert!(!facts(&doc).contains_key("wrong"), "{body}");
        assert!(!doc.diagnostics.is_empty(), "{body}");
    }
    let doc = query_markdown_text(
        "- `--help`: Help. <!-- mant:entry {\"id\":\"wrong\"} -->",
        None,
    )
    .unwrap()
    .document
    .unwrap();
    assert!(!facts(&doc).contains_key("wrong"));
    assert!(!doc.diagnostics.is_empty());
}

#[test]
fn partial_groups_case_roles_and_literal_commands_remain_distinct() {
    let doc = parse(
        r#"- `-S`, `--since`, `--until`: Bounds. <!-- mant:entry {"id":"bounds","aliasGroups":[["-S","--since"]]} -->
- `--when`: When. <!-- mant:entry {"id":"when","aliasOf":"bounds"} -->"#,
    );
    assert_eq!(
        facts(&doc)["bounds"].alias_groups,
        vec![vec!["-S", "--since"]]
    );
    assert!(facts(&doc)["when"].alias_of.is_none());
    for (role, case) in [("command", "sensitive"), ("option", "insensitive")] {
        let body = format!(
            "<!-- mant:entries role=option case=sensitive -->\n- `--one`: One. <!-- mant:entry {{\"id\":\"one\",\"aliasOf\":\"other\"}} -->\n\n<!-- mant:entries role={role} case={case} -->\n- `{}`: Other. <!-- mant:entry {{\"id\":\"other\"}} -->",
            if role == "command" {
                "other"
            } else {
                "--other"
            }
        );
        let doc = query_markdown_text(&body, None).unwrap().document.unwrap();
        assert!(facts(&doc)["one"].alias_of.is_none());
        assert!(
            doc.diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("ir.invalid-entry-alias-of"))
        );
    }
    let doc = query_markdown_text("<!-- mant:entries role=command case=sensitive -->\n- `[`: Test. <!-- mant:entry {\"id\":\"test-open\"} -->\n- `:`: No operation. <!-- mant:entry {\"id\":\"noop\"} -->", None).unwrap().document.unwrap();
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    assert_eq!(facts(&doc)["test-open"].names, ["["]);
    assert_eq!(facts(&doc)["noop"].names, [":"]);
}

#[test]
fn metadata_limits_and_examples_never_invent_facts() {
    let huge = "x".repeat(8192);
    for json in [
        format!("{{\"id\":\"{huge}\"}}"),
        format!(
            "{{\"id\":\"wrong\",\"aliasGroups\":[{}]}}",
            std::iter::repeat_n("[\"-h\",\"--help\"]", 33)
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        let doc = parse(&format!(
            "- `-h`, `--help`: Help. <!-- mant:entry {json} -->"
        ));
        assert!(facts(&doc).contains_key("option-h"));
        assert!(!doc.diagnostics.is_empty());
    }
    let doc = parse(
        "- `--help`: Help; example `<!-- mant:entry {\"id\":\"wrong\"} -->`.\n\n  ```markdown\n  <!-- mant:entry {\"id\":\"wrong\"} -->\n  ```\n",
    );
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    assert!(!facts(&doc).contains_key("wrong"));
}
