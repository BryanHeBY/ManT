//! Native text growth retains ownership, bytes and parser lifetime boundaries.

use super::*;

fn all_text<'a>(node: &'a Node, output: &mut Vec<&'a str>) {
    if let Some(text) = node.text.as_deref() {
        output.push(text);
    }
    for child in &node.children {
        all_text(child, output);
    }
}

fn all_cells<'a>(node: &'a Node, output: &mut Vec<&'a crate::TableCell>) {
    output.extend(&node.table_cells);
    for child in &node.children {
        all_cells(child, output);
    }
}

#[test]
fn append_runs_preserve_native_text_and_cells_across_lifetime_boundaries() {
    // All 66 exact sources ran through the registered pristine CVS five
    // profiles before this fixture was written. The tree profile supplies the
    // native TEXT/cell contents, without collapsing empty operands or spaces.
    // tbl_data.c::tbl_cdata preserves a space between appended lines and strips
    // T} before a word fallthrough. A T{ with no payload keeps string == NULL;
    // one empty payload line instead allocates "" and sets block = 1.
    // roff.c::roff_word_append calls roff_strdup before joining. man/man_validate
    // and mdoc/mdoc_validate may shorten, replace or transfer TEXT afterward.
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("append_growth/cases.json")).unwrap();
    let parser = Parser::default();
    for case in fixtures.as_array().unwrap() {
        if case["kind"] == "rejected" {
            continue;
        }
        let label = case["id"].as_str().unwrap();
        let report = parser
            .parse_bytes(label, case["source"].as_str().unwrap().as_bytes())
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        let mut actual = Vec::new();
        all_text(&report.document.root, &mut actual);
        let expected: Vec<_> = case["expected_all_texts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|text| text.as_str().unwrap())
            .collect();
        assert_eq!(actual, expected, "{label}: native TEXT ownership and bytes");
        if let Some(expected) = case["expected_cells"].as_array() {
            let mut cells = Vec::new();
            all_cells(&report.document.root, &mut cells);
            assert_eq!(cells.len(), expected.len(), "{label}: native topology");
            for (actual, expected) in cells.iter().zip(expected) {
                assert_eq!(actual.text.as_deref(), expected["text"].as_str(), "{label}");
                assert_eq!(
                    actual.text_block,
                    expected["text_block"].as_bool().unwrap(),
                    "{label}: NULL/no-lines differs from an allocated empty line"
                );
            }
        }
        #[cfg(feature = "serde")]
        {
            let encoded = serde_json::to_string(&report.document).unwrap();
            let decoded: Document = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, report.document, "{label}: actual JSON roundtrip");
        }
    }
}

#[test]
fn append_receipts_retire_before_rejected_tree_and_next_session() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("append_growth/cases.json")).unwrap();
    let cases = fixtures.as_array().unwrap();
    let rejected = cases
        .iter()
        .find(|case| case["id"] == "native-rejected-tree")
        .unwrap();
    let clean = cases
        .iter()
        .find(|case| case["id"] == "clean-next-session")
        .unwrap();
    let parser = Parser::default();
    for _ in 0..8 {
        // The exact input also ran through pristine CVS before this assertion.
        // The existing local 512-parent construction guard remains selected;
        // this test does not reinterpret the pristine result as that guard.
        let error = parser
            .parse_bytes(
                "rejected.1",
                rejected["source"].as_str().unwrap().as_bytes(),
            )
            .expect_err("existing native construction guard rejects deep input");
        assert!(error.message.contains("nesting limit"), "{error}");
        let report = parser
            .parse_bytes("clean.1", clean["source"].as_str().unwrap().as_bytes())
            .unwrap();
        let mut actual = Vec::new();
        all_text(&report.document.root, &mut actual);
        assert_eq!(actual, ["DESCRIPTION", "next clean"]);
    }
}
