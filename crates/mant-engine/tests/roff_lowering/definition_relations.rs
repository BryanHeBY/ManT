//! Final definition relations preserve accepted words and actual hard rows.
use super::*;

#[test]
fn source_neutral_definition_relations_keep_native_words_rows_and_wire_paths() {
    // All 96 exact sources ran ASCII/UTF-8/HTML/tree/lint on the authenticated
    // reference. Eight have the known pristine HTML assertion failure; their
    // expectations are terminal-only, never an HTML compatibility PASS. Shared
    // row decisions follow mdoc_term.c::termp_it_pre/post and term_flushln();
    // first-row preferred alignment is separate from those executed seams.
    // Backspaces select the final device cell, NBSP selects its occupied blank.
    // Only left origins and repeated device padding are responsive here: do
    // not discard empty rows or merge words across a Joined/Separated seam.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("definition_relations/cases.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 96);
    let mut ids = HashSet::new();
    let mut failures = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        assert!(ids.insert(id));
        let source = case["source"].as_str().unwrap();
        assert_ast_owner(source);
        for profile in ["ascii", "utf8", "tree", "lint"] {
            assert_eq!(case["profiles"][profile]["status"], 0, "{id}/{profile}");
        }
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = mant_render::render_query_json(&query, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let decoded: ResolvedContent = restored.into();
        assert_eq!(
            query.document, decoded.document,
            "{id}: accepted tree changed on the wire"
        );
        assert!(!wire.contains("joined-no-space"));
        assert!(!wire.contains("flush-at-body"));
        let rendered = mant_render::render_query_man(&decoded);
        let body = rendered.split_once("DESCRIPTION\n").unwrap().1;
        let body = body.split_once("\n\nNEXT\n").map_or(body, |(body, _)| body);
        let actual = body
            .trim_end_matches('\n')
            .split('\n')
            .map(reading_row)
            .collect::<Vec<_>>();
        let expected = case["nativeRows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| reading_row(row.as_str().unwrap()))
            .collect::<Vec<_>>();
        if actual != expected {
            failures.push(format!("{id}: expected={expected:?}, actual={actual:?}"));
        }
        // Every source places the marker in the original It BODY. The final
        // schema change cannot transfer it into term/name bindings or omit it.
        let Block::DefinitionList { items, .. } =
            &decoded.document.as_ref().unwrap().sections[1].blocks[0]
        else {
            panic!("{id}: definition container disappeared");
        };
        assert_eq!(items.len(), 1);
        assert_authored_fixed_spaces(id, &items[0]);
        assert!(
            items[0]
                .terms
                .iter()
                .all(|term| !mant_ir::inline_plain_text(term).contains("BODY")
                    && !mant_ir::inline_plain_text(term).contains("BodyWord"))
        );
        assert!(items[0].description.iter().any(|block| matches!(block, Block::Paragraph { children, .. } | Block::Preformatted { children, .. } if mant_ir::inline_plain_text(children).contains("BODY") || mant_ir::inline_plain_text(children).contains("BodyWord"))), "{id}: body owner lost");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn assert_authored_fixed_spaces(id: &str, item: &mant_ir::DefinitionItem) {
    // term.c::term_word encodes \\~ and \\0 as occupied fixed-space cells;
    // \\& is ASCII_NBRZW and never becomes that authored scalar. These exact
    // sources ran the pristine reference before freezing the row expectations.
    // This independent axis is not covered by the responsive-padding comparator.
    let terms = item
        .terms
        .iter()
        .map(|term| mant_ir::inline_plain_text(term))
        .collect::<Vec<_>>();
    let fixed_cells = terms
        .iter()
        .map(|term| {
            term.chars()
                .filter(|&character| character == '\u{a0}')
                .count()
        })
        .sum::<usize>();
    if id.contains("-fixed-space-") {
        assert_eq!(fixed_cells, 1, "{id}: authored fixed seam duplicated");
        assert!(
            terms.iter().any(|term| term.contains("X\u{a0}Y")),
            "{id}: authored fixed seam lost"
        );
    } else if id.ends_with("-current-fixed") {
        assert_eq!(fixed_cells, 1, "{id}: authored fixed cell duplicated");
        assert!(
            terms.iter().any(|term| term.contains('\u{a0}')),
            "{id}: authored fixed cell lost"
        );
    } else if id.ends_with("-current-zerowidth") {
        assert!(
            terms.iter().all(|term| !term.contains('\u{a0}')),
            "{id}: zero-width cell became fixed space"
        );
    }
}

fn reading_row(row: &str) -> String {
    row.replace('\u{a0}', " ")
        .split(' ')
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_ast_owner(source: &str) {
    use libmandoc_rs::{Node, NodeKind, Parser};
    fn find(node: &Node, predicate: fn(&Node) -> bool) -> Option<&Node> {
        if predicate(node) {
            Some(node)
        } else {
            node.children
                .iter()
                .find_map(|child| find(child, predicate))
        }
    }
    fn marker(node: &Node) -> bool {
        matches!(node.text.as_deref(), Some("BODY" | "BodyWord"))
    }
    let report = Parser::default()
        .parse_bytes("definition-relations.1", source.as_bytes())
        .unwrap();
    let item = find(&report.document.root, |node| {
        node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It")
    })
    .unwrap();
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
        find(head, marker).is_none(),
        "BODY marker moved into pristine-shaped HEAD"
    );
    assert!(
        find(body, marker).is_some(),
        "BODY execution path not exercised by fixture"
    );
}
