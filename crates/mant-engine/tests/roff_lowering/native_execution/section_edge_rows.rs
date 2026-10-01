//! Completed source rows survive definition owners and section/EOF drains.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::ResolvedContent;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    source: String,
    rows: Vec<String>,
    native_rows: Vec<String>,
    reading_rule: String,
    heading: Option<String>,
    owner: String,
    body_word: bool,
}

fn definition(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("It") {
        return Some(node);
    }
    node.children.iter().find_map(definition)
}

fn contains_body_word(node: &Node) -> bool {
    node.text.as_deref() == Some("BodyWord") || node.children.iter().any(contains_body_word)
}

fn description_rows(query: &ResolvedContent, heading: Option<&str>) -> Vec<String> {
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
            .find(|section| section.heading.plain_text() == heading)
            .unwrap()
            .spacing_before_lines;
        let end = boundary - usize::from(spacing);
        assert!(rows[end..boundary].iter().all(|row| row.is_empty()));
        end
    });
    rows[start..end]
        .iter()
        .map(|row| row.trim_start_matches(' ').to_owned())
        .collect()
}

#[test]
fn executed_empty_rows_have_one_owner_at_definition_and_section_edges() {
    // All 297 complete sources ran registered pristine ASCII/UTF-8/HTML/
    // tree/lint before these golds were written. term.c::term_vspace()
    // executes newln before skipvsp/endline; mdoc_term.c::termp_it_post()
    // cannot retract those completed rows when HEAD changes IR owner.
    // mdoc_validate.c can prune leading paragraph requests; a retained Pp
    // still executes vspace even when the output container is empty.
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/section_edge_rows/cases.json")).unwrap();
    assert_eq!(cases.len(), 297);
    let mut failures = Vec::new();
    for case in cases {
        let projected_rows = match case.reading_rule.as_str() {
            "common-left-margin-only" => case.native_rows.clone(),
            "generated-inset-body-blank-row" => {
                assert!(case.name.starts_with("inset-"));
                assert!(!case.source.contains("\\~") && !case.source.contains('\u{a0}'));
                case.native_rows
                    .iter()
                    .map(|row| row.strip_prefix('\u{a0}').unwrap_or(row).to_owned())
                    .collect()
            }
            rule => panic!("unknown reading rule {rule}"),
        };
        assert_eq!(
            projected_rows, case.rows,
            "{}: bounded reading projection",
            case.name
        );
        let parsed = Parser::default()
            .parse_bytes("section-edge-rows.1", case.source.as_bytes())
            .unwrap();
        if case.owner == "It" {
            let item = definition(&parsed.document.root).unwrap();
            let head = item
                .children
                .iter()
                .find(|node| node.kind == NodeKind::Head)
                .unwrap();
            let body = item
                .children
                .iter()
                .find(|node| node.kind == NodeKind::Body)
                .unwrap();
            assert!(
                !contains_body_word(head),
                "{}: BODY belongs outside HEAD",
                case.name
            );
            assert_eq!(
                contains_body_word(body),
                case.body_word,
                "{}: asserted BODY owner",
                case.name
            );
        }
        let loaded = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{}: private row receipt escaped",
            case.name
        );
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = restored.into();
        let actual = description_rows(&query, case.heading.as_deref());
        if actual != case.rows {
            failures.push(format!(
                "{}\n{}\nactual {actual:?}\nreference {:?}",
                case.name, case.source, case.rows
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
