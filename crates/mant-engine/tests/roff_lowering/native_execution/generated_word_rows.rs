//! Generated formatter words share pending glyph and held-cell execution.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::ResolvedContent;

fn first_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It") {
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
        serde_json::from_str(include_str!("fixtures/generated_word_rows/cases.json")).unwrap();
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
            assert!(head.children[0].macro_token.is_none(), "{name}");
            let body = item
                .children
                .iter()
                .find(|node| node.kind == NodeKind::Body)
                .unwrap();
            assert!(
                body.children
                    .iter()
                    .any(|node| node.macro_token.as_deref() == Some("No")),
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

#[test]
fn inset_head_paragraph_marker_overwrites_only_addressed_raw_slots() {
    // Frozen UTF-8 reference rows from pristine CVS mandoc. Both inline
    // words execute in It HEAD; the following .br/.Pp performs the first
    // real flush. The glyphs overwritten by LF must not leak into output.
    let cases: &[(&str, &str, &str, &[&str])] = &[
        (r"\zC\p\:J", "-2n", "br", &["C", "", "X"]),
        (r"\:\zC\pJ", "-2n", "br", &["", "X"]),
        (r"\zC\h'1n'\pJJ", "-2n", "Pp", &["CJ", "", "X"]),
        // These negative controls land on the separator or append at
        // lastcol; neither should erase J.
        (r"\zC\pJ", "-1n", "br", &["J", "X"]),
        (r"\zC\pJ", "0n", "br", &["J", "", "X"]),
        (r"\zC\pJ", "|1n", "br", &["_J", "X"]),
        (r"\zC\pJ", "|0n", "br", &["J", "X"]),
    ];
    for (head, retreat, flush, expected) in cases {
        let source = format!(
            ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n\
             .Sh DESCRIPTION\n.Bl -inset\n.It Em \"{head}\" No \"\\h'{retreat}'\\p\"\n\
             .{flush}\n.No X\n.El\n.Sh NEXT\nDone.\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut rows = body_rows(&mant_render::render_query_man(&query));
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        assert_eq!(&rows, expected, "{source}");
    }
}

#[test]
fn inset_head_marker_replay_keeps_decoded_controls_and_fonts() {
    // A lowercase escaped first word is literal to the mdoc parser; its
    // authored separator and the quoted argument share one term_word().
    // CVS UTF-8 fixtures also cover NBSP, an arm before SP, and a hyphen
    // whose single native slot is written before the paragraph marker.
    let cases: &[(&str, &str, &[&str])] = &[
        (
            r#"\~m "\zC\h'1n'\pJ" No "\h'-2n'\p""#,
            ".Pp\n.No X",
            &["m C", "", "X"],
        ),
        (
            r#"\:Em "\zCC\pJ" No "\h'-2n'\p""#,
            ".br\n.No Z",
            &["Em C", "Z"],
        ),
        (
            r#"Em "\z C\:\pJJ" No "\h'-2n'\p""#,
            ".br\n.No X",
            &["J", "X"],
        ),
        (
            r#"E\&m "\zC\p\h'1n'J" No "\h'-2n'\p""#,
            ".Pp\n.No X",
            &["Em C", "", "X"],
        ),
        (
            r#"Em "\z\0C\p\p\:J" No "\h'-2n'\p""#,
            ".Bd -literal\n.No X\n.Ed",
            &["C", "", "", "X"],
        ),
        (
            r#"\h'-1n'm "\zC\pJ" No "\h'-2n'\(hy\p""#,
            ".br\n.No X",
            &["m ‐", "X"],
        ),
        (
            r#"\(hyEm "\z\h'-1n'C\pJ" No "\h'-2n'\p""#,
            ".br\n.No X",
            &["‐Em C", "X"],
        ),
    ];
    for (head, body, expected) in cases {
        let source = format!(
            ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n\
             .Sh DESCRIPTION\n.Bl -inset\n.It {head}\n{body}\n.El\n.Sh NEXT\nDone.\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut rows = body_rows(&mant_render::render_query_man(&query));
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        assert_eq!(&rows, expected, "{source}");
    }
}

#[test]
fn macro_generated_words_use_the_current_absolute_writer() {
    // Full inputs checked using pristine CVS lint, ASCII and UTF-8.
    // Ordinary words, Xr, enclosure and Lk keep their existing writer;
    // motion is restricted to the same native row by a real .br.
    let cases = [
        (".Ap No B", "R'BDEND", "R'B END"),
        (".In stdio.h", "RE<stdio.h> END", "R <stdio.h> END"),
        (".Bx", "REBSD END", "R BSD END"),
        (".Bx 4.4 Lite", "RE4.4BSD-Lite END", "R 4.4BSD-Lite END"),
        (".No B", "REBDEND", "R B END"),
        (".Xr printf 3", "REprintf(3) END", "R printf(3) END"),
        (".Pq No T", "RE(T) END", "R (T) END"),
        (
            ".Lk https://z.test Z",
            "REZ:Yhttps://z.test END",
            "R Z: https://z.test END",
        ),
    ];
    let mut failures = Vec::new();
    for (node, absolute, no_motion) in cases {
        for (mid, expected) in [
            (".No \"\\h'|0n'R\"", vec![absolute.to_owned()]),
            (".No R", vec![format!("HEAD Y {no_motion}")]),
            (
                ".br\n.No \"\\h'|0n'R\"",
                vec!["HEAD Y".to_owned(), no_motion.to_owned()],
            ),
        ] {
            let source = format!(
                ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n\
                 .Sh DESCRIPTION\n.No HEAD\nY\n{mid}\n{node}\nEND\n.Sh NEXT\nDone.\n"
            );
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
            let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
            let mut actual = body_rows(&mant_render::render_query_man(&decoded.into()));
            while actual.last().is_some_and(String::is_empty) {
                actual.pop();
            }
            if actual != expected {
                failures.push((node, mid, actual, expected));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
