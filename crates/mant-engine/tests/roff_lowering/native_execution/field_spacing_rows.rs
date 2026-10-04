//! Physical rows survive native spacing, geometry returns and JSON owners.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::ResolvedContent;

fn item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It") {
        return Some(node);
    }
    node.children.iter().find_map(item)
}

fn contains_scope(node: &Node, name: &str) -> bool {
    node.macro_token.as_deref() == Some(name)
        || node
            .children
            .iter()
            .any(|child| contains_scope(child, name))
}

fn rendered_rows(query: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(query);
    let rows = rendered.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let next = rows.iter().position(|row| *row == "NEXT").unwrap();
    let separator = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.id.as_str() == "next")
        .unwrap()
        .spacing_before_lines;
    let end = next - usize::from(separator);
    assert!(rows[end..next].iter().all(|row| row.is_empty()));
    // Device padding is responsive; word adjacency and every completed
    // leading/interior/trailing physical row remain observable.
    rows[start..end]
        .iter()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn assert_fixture(json: &str, count: usize) {
    let cases: serde_json::Value = serde_json::from_str(json).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), count);
    let mut failures = Vec::new();
    for case in cases {
        let source = case["source"].as_str().unwrap();
        let parsed = Parser::default()
            .parse_bytes("field-spacing.1", source.as_bytes())
            .unwrap();
        let item = item(&parsed.document.root).unwrap();
        let head = item
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Head)
            .unwrap();
        assert!(contains_scope(head, "Xo"), "HEAD ownership: {source}");
        assert!(
            item.children
                .iter()
                .any(|child| child.kind == NodeKind::Body)
        );

        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let text = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(!text.contains("\\u0000mant:"), "private receipt: {source}");
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&text).unwrap();
        let actual = rendered_rows(&restored.into());
        // Only cases carrying a BODY URI have a wider pristine record:
        // its terminal device-width wrap is responsive. Authored HEAD \p
        // rows remain in both records; no empty row is trimmed or folded.
        let rows = case.get("wide_rows").unwrap_or(&case["rows"]);
        let expected = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if actual != expected {
            failures.push(format!(
                "{}\n{source}\nactual: {actual:?}\nreference: {expected:?}",
                case["name"].as_str().unwrap()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn spacing_debt_executes_flushes_before_native_field_padding_is_projected() {
    // Each of the 72 exact sources ran pristine CVS ASCII/UTF-8/HTML/tree/
    // lint before recording. roff_term_pre_sp() banks skipvsp or invokes
    // term_vspace(), then always pre_br (roff_term.c:195-214); term_field()
    // prints vbl only at accepted graph consumption (term.c:389-434).
    // A later Xo return restores offset before HEAD post (mdoc_term.c:437).
    assert_fixture(include_str!("fixtures/field_spacing_rows/cases.json"), 72);
}

#[test]
fn empty_text_with_continuation_uses_the_existing_native_field_flags() {
    // Every exact source ran pristine CVS first. NODE_NOFILL empty TEXT
    // takes term_newln when NONEWLINE is set (mdoc_term.c:361-375), so TAG/
    // HANG still decide whether that buffer closes; no forced hard-break
    // substitutes for this conditional call. The filled blank-source-line
    // counterparts parse as a spacing request and retain their own rows.
    assert_fixture(
        include_str!("fixtures/empty_text_continuation_rows/cases.json"),
        8,
    );
}
