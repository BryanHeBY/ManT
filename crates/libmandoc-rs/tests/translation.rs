//! Translation keeps native text while reusing private geometric storage.
use libmandoc_rs::{Node, NodeKind, Parser};

fn native_texts(node: &Node, texts: &mut Vec<String>) {
    if node.kind == NodeKind::Text {
        texts.push(
            node.native_text
                .as_ref()
                .or(node.text.as_ref())
                .unwrap()
                .clone(),
        );
    }
    for child in &node.children {
        native_texts(child, texts);
    }
}

#[test]
fn translated_words_keep_lookup_escapes_unicode_and_independent_sessions() {
    // roff.c::roff_tr and roff_char record ASCII and escape translations. roff_strdup
    // protects nonmatching escapes and does not recursively translate a
    // replacement. Exact inputs ran the pristine ASCII/UTF-8/HTML/tree/lint
    // profiles before expectedTexts was captured from the native tree.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/translation.json")).unwrap();
    assert_eq!(fixture["header"]["count"], 38);
    for case in fixture["cases"].as_array().unwrap() {
        let expected: Vec<String> = serde_json::from_value(case["expectedTexts"].clone()).unwrap();
        for _ in 0..2 {
            let report = Parser::default()
                .parse_bytes("translation.7", case["source"].as_str().unwrap().as_bytes())
                .unwrap();
            let mut actual = Vec::new();
            native_texts(&report.document.root, &mut actual);
            assert_eq!(actual, expected, "{}", case["id"]);
        }
    }
}
