//! Generated formatter words share pending glyph and held-cell execution.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::ResolvedContent;

fn first_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some("It") {
        return Some(node);
    }
    node.children.iter().find_map(first_item)
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

fn native_tab_padding_matches(actual: &str, expected: &str) -> bool {
    let Some((prefix, suffix)) = actual.split_once('\t') else {
        return actual == expected;
    };
    let Some(remaining) = expected.strip_prefix(prefix) else {
        return false;
    };
    let padding = remaining.bytes().take_while(|byte| *byte == b' ').count();
    (0..=padding).any(|width| native_tab_padding_matches(suffix, &remaining[width..]))
}

fn same_reading_rows(source: &str, actual: &[String], expected: &[String]) -> bool {
    actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(actual, expected)| {
            if source.contains('\t') && actual.contains('\t') {
                // CVS term.c:332-339 accepts the authored Tab as a graph
                // cell; :399-420 expands it at device stops. ManT preserves
                // that scalar for responsive stops (mant-roff.md, Tabs).
                // Only a Tab's device padding can vary: every non-Tab
                // substring, ordinary space and complete row stays exact.
                native_tab_padding_matches(actual, expected)
            } else {
                actual == expected
            }
        })
}

#[test]
fn generated_words_consume_every_pending_and_held_cell_state() {
    // Exact sources ran registered pristine ASCII/UTF-8/HTML/tree/lint
    // before recording these expectations. term.c::encode1()/term_word()
    // order BACKBEFORE/BACKAFTER and held blank writes; quote/fl/in/xx
    // pre/post all call term_word. termp_it_pre emits inset/diag BODY gaps.
    // Native row glyphs, inner spaces and every blank row stay exact;
    // this projection does not assert device row origins or trailing cells.
    // No-argument Bx is native BSD, without a portable enhancement contract.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("generated_word_rows/cases.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 6 * 10 * 13 * 3);
    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let list = name.contains("_inset_") || name.contains("_diagnostic_");
        if list {
            // Diag It parses a literal HEAD: Xo would be authored text.
            // The line-start Ns variant is explicitly a no-op contrast;
            // it is not mistaken for a tight inline Ns in this grammar.
            let report = Parser::default()
                .parse_bytes(name, source.as_bytes())
                .unwrap();
            let item = first_item(&report.document.root).unwrap();
            let head = item
                .children
                .iter()
                .find(|node| node.kind == NodeKind::Head)
                .unwrap();
            assert_eq!(head.children.len(), 1, "{name}: literal HEAD expected");
            assert_eq!(head.children[0].kind, NodeKind::Text, "{name}");
            assert!(head.children[0].macro_name.is_none(), "{name}");
            let body = item
                .children
                .iter()
                .find(|node| node.kind == NodeKind::Body)
                .unwrap();
            assert!(
                body.children
                    .iter()
                    .any(|node| node.macro_name.as_deref() == Some("No")),
                "{name}: BODY word must execute after the generated gap"
            );
        }
        let expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let json = mant_render::render_query_json(&query, false).unwrap();
        assert!(
            !json.contains("\\u0000mant:"),
            "{name}: private owner leaked"
        );
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: ResolvedContent = decoded.into();
        let actual = body_rows(&mant_render::render_query_man(&query));
        if !same_reading_rows(source, &actual, &expected) {
            failures.push(format!(
                "{name}\nsource:\n{source}\nactual: {actual:?}\nreference: {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
