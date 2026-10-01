//! Ordinary formatter buffers share the native field's ordered acceptance.

use libmandoc_rs::{Node, Parser};
use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    source: String,
    payload: String,
    rows: Vec<String>,
}

fn contains_payload(node: &Node, payload: &str) -> bool {
    node.text.as_deref() == Some(payload)
        || node
            .children
            .iter()
            .any(|child| contains_payload(child, payload))
}

fn description_rows(query: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(query);
    let rows = rendered.lines().collect::<Vec<_>>();
    let start = rows.iter().position(|row| *row == "DESCRIPTION").unwrap() + 1;
    let boundary = rows.iter().position(|row| *row == "NEXT").unwrap();
    let spacing = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "NEXT")
        .unwrap()
        .spacing_before_lines;
    let end = boundary - usize::from(spacing);
    assert!(rows[end..boundary].iter().all(|row| row.is_empty()));
    rows[start..end]
        .iter()
        .map(|row| (*row).to_owned())
        .collect()
}

#[test]
fn plain_flush_units_preserve_invisible_passes_and_reject_only_their_suffix() {
    // Each complete source ran the registered pristine ASCII/UTF-8/HTML/
    // tree/lint profiles before recording this fixture. term.c::term_fill()
    // accepts NBRZW as graph without a scalar (340-349); term_flushln()
    // executes both accepted-pass endline (217) and a rejected tail's
    // endline (250-253). Plain text has the same buffer rules as fields,
    // including when the last accepted pass contained only NBRZW.
    // Bare BACKAFTER wrote no cell, whereas completed \zX did (encode1()).
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/plain_field_rows/cases.json")).unwrap();
    assert_eq!(cases.len(), 32);
    let mut failures = Vec::new();
    for case in cases {
        let parsed = Parser::default()
            .parse_bytes("plain-field-rows.1", case.source.as_bytes())
            .unwrap();
        assert!(
            contains_payload(&parsed.document.root, &case.payload),
            "{}: exact TEXT operand must remain in the AST",
            case.name
        );
        let loaded = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{}: private native receipt escaped",
            case.name
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let actual = description_rows(&query);
        if actual != case.rows {
            failures.push(format!(
                "{}\n{}\nactual {actual:?}\nreference {:?}",
                case.name, case.source, case.rows
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
