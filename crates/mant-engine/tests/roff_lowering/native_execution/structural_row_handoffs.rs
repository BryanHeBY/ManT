//! Structural macro entry consumes a live device row before changing IR owners.

use libmandoc_rs::{Node, NodeKind, Parser};

fn outer_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It") {
        return Some(node);
    }
    node.children.iter().find_map(outer_item)
}

#[test]
fn nested_list_entry_consumes_occupied_head_rows_without_borrowing_their_padding() {
    // Each complete source ran the registered pristine five profiles before
    // this fixture and assertion. termp_bl_pre() calls term_newln() at BLOCK
    // entry (mdoc_term.c:1128-1134), even when an earlier NOBREAK HEAD already
    // flushed its input but left viscol/minbl live (term.c:235,250-253).
    // That true pre consumes the row; changing its IR owner does not.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/structural_row_handoffs/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 16);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let parsed = Parser::default()
            .parse_bytes("structural-row-handoffs.1", source.as_bytes())
            .unwrap();
        let item = outer_item(&parsed.document.root).unwrap();
        let body = item
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Body)
            .unwrap();
        assert!(
            body.children.iter().any(|node| {
                node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("Bl")
            }),
            "{name}: nested Bl must execute in the outer BODY"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private receipt leaked"
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let actual = super::node_body_rows::description_rows(&restored.into(), Some("NEXT"));
        let expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{name}: {source}");
    }
}
