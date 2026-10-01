//! Links and generated scope words execute in the active native body stream.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{ReferenceScope, ResolvedContent};
use mant_protocol::{
    ReferenceCount, ReferenceProjection, ReferenceProjectionMode, ReferenceTargetType,
};

fn text_node<'a>(node: &'a Node, text: &str) -> Option<&'a Node> {
    if node.kind == NodeKind::Text && node.text.as_deref() == Some(text) {
        return Some(node);
    }
    node.children.iter().find_map(|node| text_node(node, text))
}

fn line_events(node: &Node, line: u32) -> usize {
    usize::from(node.flags.no_fill && node.flags.line_start && node.line == line)
        + node
            .children
            .iter()
            .map(|node| line_events(node, line))
            .sum::<usize>()
}

fn assert_expanded_execution_lines(name: &str, source: &str) {
    let report = Parser::default()
        .parse_bytes(name, source.as_bytes())
        .unwrap();
    let first = text_node(&report.document.root, "first").unwrap();
    let second = text_node(&report.document.root, "second").unwrap();
    assert_eq!(
        (first.line, first.column),
        (second.line, second.column),
        "{name}"
    );
    assert!(first.flags.no_fill && second.flags.no_fill, "{name}");
    // D1/Dl attach NODE_LINE to the BLOCK; raw man text attaches it to
    // TEXT. Both must deliver two distinct execution events at this point.
    assert!(
        line_events(&report.document.root, first.line) >= 2,
        "{name}"
    );
}

fn body_rows(rendered: &str) -> Vec<String> {
    let mut projected = String::with_capacity(rendered.len());
    for character in rendered.chars() {
        match character {
            '\u{8}' => {
                projected.pop();
            }
            '\u{a0}' => projected.push(' '),
            _ => projected.push(character),
        }
    }
    let rows = projected.lines().map(str::trim).collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = rows.iter().position(|row| *row == "NEXT").unwrap();
    rows[start..end]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

fn assert_authored_link_identities(name: &str, query: &ResolvedContent) {
    if !name.starts_with("link_") {
        return;
    }
    let expected = if name.contains("_empty_word_end")
        || name.contains("_empty_bare_zero")
        || name.contains("_empty_same_coordinate")
    {
        0
    } else if name.contains("_nested_") {
        2
    } else {
        1
    };
    let references = mant_query::project_references(
        query.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::External, ReferenceTargetType::Email],
            ..Default::default()
        },
    );
    assert_eq!(
        references.occurrences,
        ReferenceCount::Exact { value: expected },
        "{name}"
    );
    for reference in references.records {
        assert!(
            reference
                .origin
                .resolve_link(query.document.as_ref().unwrap())
                .is_some(),
            "{name}: authored link lost its exact public origin"
        );
    }
}

#[test]
fn body_and_generated_post_share_word_row_and_owner_execution() {
    // All 134 exact sources ran registered pristine ASCII/UTF-8/HTML/tree/
    // lint before these assertions. man_term.c::pre_UR adds no line boundary;
    // post_UR always writes <HEAD>, including an empty decoded target.
    // Both print_man_node and print_mdoc_node observe NODE_NOFILL/NODE_LINE
    // before dispatch, independently of repeated source coordinates.
    // termp_d1_pre, termp_quote_post, termp_fo_post and termp_an_pre consume
    // the same active termp; source-line coordinates cannot deduplicate them.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/generated_body_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 48 + 30 + 8 + 48);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        if case["same_coordinate"].as_bool().unwrap() {
            assert_expanded_execution_lines(name, source);
        }
        let expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private owner leaked"
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        assert_authored_link_identities(name, &query);
        let actual = body_rows(&mant_render::render_query_man(&query));
        if actual != expected {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
