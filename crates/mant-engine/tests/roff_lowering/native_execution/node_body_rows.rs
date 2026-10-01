//! HEAD posts and BODY word/line events keep their actual execution order.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::ResolvedContent;

fn definition_node<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| definition_node(child, name))
}

fn contains_text(node: &Node, value: &str) -> bool {
    node.text.as_deref() == Some(value)
        || node
            .children
            .iter()
            .any(|child| contains_text(child, value))
}

pub(super) fn description_rows(query: &ResolvedContent, heading: Option<&str>) -> Vec<String> {
    let rendered = mant_render::render_query_man(query);
    let rows = rendered.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let end = heading.map_or(rows.len(), |heading| {
        let boundary = rows.iter().position(|row| *row == heading).unwrap();
        let spacing = query
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.id.as_str() == heading.to_ascii_lowercase())
            .unwrap()
            .spacing_before_lines;
        let end = boundary - usize::from(spacing);
        assert!(rows[end..boundary].iter().all(|row| row.is_empty()));
        end
    });
    // Field padding is responsive. Preserve every leading/internal/trailing
    // empty row and every word separator while omitting device cell widths.
    rows[start..end]
        .iter()
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn definition_posts_and_body_events_preserve_word_and_edge_rows_through_json() {
    // All 310 exact complete sources ran the registered pristine CVS
    // ASCII/UTF-8/tree/lint profiles before this fixture was recorded.
    // man_term.c::pre_IP/pre_TP(BODY) sets NOSPACE|NONEWLINE, after the HEAD
    // post's real term_flushln fitting result. Empty TEXT then calls newln;
    // NBRZW buffers a cell; a bare BACKAFTER does not (term.c::term_newln).
    // mdoc_term.c::termp_lk_pre executes the description before generated
    // colon/target words settle its later break: that later event cannot
    // retroactively classify the first description word as stacked.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/node_body_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 310);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let macro_name = case["macro"].as_str().unwrap();
        let parsed = Parser::default()
            .parse_bytes("node-body-rows.1", source.as_bytes())
            .unwrap();
        let definition = definition_node(&parsed.document.root, macro_name).unwrap();
        let head = definition
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Head)
            .unwrap();
        assert!(
            contains_text(head, case["head"].as_str().unwrap()),
            "{name}: fixture must execute the asserted HEAD owner"
        );
        assert!(
            definition
                .children
                .iter()
                .any(|child| child.kind == NodeKind::Body)
        );

        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private receipt leaked"
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let actual = description_rows(&query, case["heading"].as_str());
        let expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if actual != expected {
            failures.push(format!(
                "{name}\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
