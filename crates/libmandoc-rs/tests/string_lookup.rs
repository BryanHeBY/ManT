//! String namespaces preserve exact lookup and lifecycle behavior.
use libmandoc_rs::{Node, NodeKind, Parser};

fn text_nodes(node: &Node, texts: &mut Vec<String>) {
    if node.kind == NodeKind::Text {
        texts.push(node.text.clone().unwrap());
    }
    for child in &node.children {
        text_nodes(child, texts);
    }
}

#[test]
fn cleared_empty_live_and_renamed_definitions_keep_native_lookup_precedence() {
    // Every exact source ran the registered pristine ASCII/UTF-8/HTML/tree/lint
    // profiles before expectedTexts was taken from the native tree. roff.c's
    // roff_Dd, roff_ds, roff_rm, roff_als and roff_rn update the two namespaces;
    // roff_getstrn must skip NULL values but retain empty non-NULL definitions,
    // exact key lengths, predefined/standard fallbacks and undefined side effects.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/string_lookup.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 18);
    for case in fixture["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let expected: Vec<String> = serde_json::from_value(case["expectedTexts"].clone()).unwrap();
        // Repeat independently to cover retirement of private per-session tables.
        for _ in 0..2 {
            let report = Parser::default()
                .parse_bytes("lookup.7", source.as_bytes())
                .unwrap();
            let mut actual = Vec::new();
            text_nodes(&report.document.root, &mut actual);
            assert_eq!(actual, expected, "{}", case["id"]);
        }
    }
}
