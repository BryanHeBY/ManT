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
    section_rows(rendered, "DESCRIPTION", "NEXT")
}

fn section_rows(rendered: &str, section: &str, next_section: &str) -> Vec<String> {
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
    let start = rows.iter().position(|row| *row == section).unwrap() + 1;
    let end = rows.iter().position(|row| *row == next_section).unwrap();
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

// AU witnesses, confirmed by the pristine CVS ASCII/UTF-8 device pair.
// Cover BSD's ordinary and versioned words, the generated apostrophe,
// four fonts, and a real .br which must retire the earlier writer.
// A non-TABREF tail control has a separate accepted-spacing path.
const BACKTRACKING_WORD_ROWS: &[(&str, &[&str])] = &[
    (
        ".ft R\n.No HEAD\n.No \"\\zR\"\n.Bx\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND RBSD"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"ABC\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND4.4SD-Lite"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"\\h'|0n'R\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEENDSD-Lite"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"Q\\z\"\n.Ap\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"Q\\z\"\n.Bx\n.No \"\\h'|0n'U\"\nEND",
        &["UEENDSD"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"Q\\c\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND.4SD-Lite"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"\\&\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND4.4SD-Lite"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"ABC\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND4.4SD-Lite"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"\\h'|0n'R\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEENDSD-Lite"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"Q\\z\"\n.Ap\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"Q\\z\"\n.Bx\n.No \"\\h'|0n'U\"\nEND",
        &["UEENDSD"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"Q\\c\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND.4SD-Lite"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"\\&\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND4.4SD-Lite"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"ABC\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEN.4SD-Lite"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"\\zR\"\n.Ap\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"\\h'|0n'R\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEENDSD-Lite"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"Q\\z\"\n.Ap\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"Q\\c\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND.4SD-Lite"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"\\&\"\n.Bx \"4.4\\z\" \"lite\"\n.No \"\\h'|0n'U\"\nEND",
        &["UEEN.4SD-Lite"],
    ),
    (
        ".ft R\n.No HEAD\n.No \"\\zR\"\n.Bx\n.br\n.No \"\\h'|0n'U\"\nEND",
        &["HEAD RBSD", "U END"],
    ),
    (
        ".ft R\n.No HEAD\n.No \"\\zR\"\n.No BSD\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND RBSD"],
    ),
    (
        ".No HEAD\nY\n.No \"\\zR\"\n.Em Z\n.No \"\\h'|0n'U\"\nEND",
        &["UEEND Y RZ"],
    ),
    (
        ".ft B\n.No HEAD\n.No \"\\zR\"\n.In \"x\\z\"\n.No \"\\&\"\nEND",
        &["HEAD R<x> END"],
    ),
    (
        ".ft I\n.No HEAD\n.No \"\\zR\"\n.In \"x\\z\"\n.No \"\\&\"\nEND",
        &["HEAD R<x> END"],
    ),
    (
        ".ft BI\n.No HEAD\n.No \"\\zR\"\n.In \"x\\z\"\n.No \"\\&\"\nEND",
        &["HEAD R<x> END"],
    ),
];

#[test]
fn short_words_keep_proved_same_row_slots_addressable_through_backtracking() {
    for (body, expected) in BACKTRACKING_WORD_ROWS {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n\
             .Sh DESCRIPTION\n{body}\n.Sh NEXT\nDone.\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut actual = body_rows(&mant_render::render_query_man(&query));
        while actual.last().is_some_and(String::is_empty) {
            actual.pop();
        }
        assert_eq!(&actual, expected, "{source}");
    }
}

#[test]
fn include_delimiters_follow_their_independent_device_write_heads() {
    // Pristine CVS ASCII and UTF-8 agree, including original glyphs
    // surviving outside the include operand's presentation wrapper.
    let cases = [
        (".No HEAD\nABC\n.No \"\\h'|0n'R\"\n.In x\nEND", "RE<x>BEND"),
        (".No HEAD\nY\n.In \"\\h'|0n'Z\"\nEND", "Z>END <"),
        (".No HEAD\nABC\n.In x\nEND", "HEAD ABC <x> END"),
        (
            ".ft B\n.No HEAD\n.No \"\\h'|0n'R\"\n.No Q In x\n.No \"\\h'|0n'U\"\nEND",
            "UEEN>",
        ),
    ];
    for (body, expected) in cases {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n\
             .Sh DESCRIPTION\n{body}\n.Sh NEXT\nDone.\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut actual = body_rows(&mant_render::render_query_man(&query));
        while actual.last().is_some_and(String::is_empty) {
            actual.pop();
        }
        assert_eq!(actual, [expected], "{source}");
    }
}

#[test]
fn include_uses_native_font_and_write_head_for_every_operand_spelling() {
    // CVS mdoc_term.c::termp_in_pre opens in the entry font, then
    // print_mdoc_node unwinds that operand scope before termp_in_post.
    // ASCII and UTF-8 agree on these full visible rows, including neighbors.
    const HEADER: &str = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd exercise bounded output\n";
    let before_include = ".No HEAD\nABC\n.No \"\\h'|0n'R\"\n";
    let mut cases = [
        (".In \"x\"", "RE<x>BEND"),
        (".In \"\\N'120'\"", "RE<x>BEND"),
        (".In \"\\[u0078]\"", "RE<x>BEND"),
        (".In \"x y\"", "RE<xAy> END"),
        (".In \"x\\&\"", "RE<x>CEND"),
        (".In \"\\fRx\\fP\"", "RE<x> END"),
    ]
    .map(|(operand, expected)| {
        (
            "DESCRIPTION",
            format!("{before_include}{operand}\nEND"),
            vec![expected],
        )
    })
    .to_vec();
    cases.extend([
        (
            "DESCRIPTION",
            ".ft I\n.No HEAD\n.Tg t\nABC\n.In \"\\h'|0n'Z\"\n.No UU\nEND".into(),
            vec!["Z> UUAEN<"],
        ),
        (
            "DESCRIPTION",
            ".ft BI\n.No HEAD\n.No \"\\zR\"\n.Ap\n.In x\n.No \"\\h'|0n'U\"\nEND".into(),
            vec!["UEEND<x>"],
        ),
        (
            "DESCRIPTION",
            ".No HEAD\nABC\n.In x\nEND".into(),
            vec!["HEAD ABC <x> END"],
        ),
        ("SYNOPSIS", ".In x".into(), vec!["#include <x>"]),
        ("SYNOPSIS", ".No Prefix In x".into(), vec!["Prefix", "<x>"]),
    ]);
    for (section, body, expected) in cases {
        let source = format!("{HEADER}.Sh {section}\n{body}\n.Sh ENDTEST\nDone.\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut actual = section_rows(&mant_render::render_query_man(&query), section, "ENDTEST");
        while actual.last().is_some_and(String::is_empty) {
            actual.pop();
        }
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn cursor_only_word_does_not_resurrect_a_covered_enclosure_mark() {
    // Pinned term.c:678-718, 901-929: encode the retained opener, BACKBEFORE
    // and its covering X; a following absolute-zero HORIZ changes only col.
    // The original brace is still a raw byte, but can no longer paint ink.
    const HEADER: &str = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd exercise bounded output\n.Sh DESCRIPTION\n";
    const PREFIX: &str = ".No \"A\\p \" Sy \"Q\\z\"\n";
    const MOTION: &str = ".No \"\\h'|0n'\"\n";
    let real_break = format!(".br\n{MOTION}");
    for (prefix, wrapper, tail, row) in [
        (PREFIX, ".Pq X\n", MOTION, "Q X)"),
        (PREFIX, ".Op X\n", MOTION, "Q X]"),
        (PREFIX, ".Pq X\n", "", "Q X)"),
        (PREFIX, ".Pq X\n", &real_break, "Q X)"),
        (PREFIX, ".No ( Ns No X Ns No )\n", MOTION, "Q X)"),
        (".No \"A\\p \" Sy Q\n", ".Pq X\n", MOTION, "Q (X)"),
    ] {
        let source = format!("{HEADER}{prefix}{wrapper}{tail}.Sh NEXT\nDone.\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = serde_json::to_vec(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let rebuilt: mant_protocol::QueryBundle = serde_json::from_slice(&json).unwrap();
        let text = mant_render::render_query_man(&rebuilt.into());
        let mut rows = body_rows(&text);
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        assert_eq!(rows, ["A", row], "{wrapper:?} {tail:?}");
    }
}

#[test]
fn a_graphless_word_preserves_the_live_gap_after_a_positioned_include() {
    // term.c::bufferc() preserves the blank at the live write head.
    // A following NBRZW term_word advances over it; the next glyph
    // prints both that retained blank and its own automatic separator.
    let prefix = r#".Dd November 27, 2026
.Dt GLYPHS 1
.Os
.Sh NAME
.Nm glyphs
.Nd small roff inputs
.Sh DESCRIPTION
.No HEAD
Y
.No "\h'|0n'R"
.In "\zX"
"#;
    let cases: [(&str, &[&str]); 5] = [
        (".No \"\\&\"\nEND", &["RE<>  END"]),
        ("END", &["RE<> END"]),
        (".No \"\\h'|0n'U\"\nEND", &["UEEND>"]),
        (".br\n.No \"\\h'|0n'U\"\nEND", &["RE<>", "U END"]),
        (".Ap No T\nEND", &["RE<>'T END"]),
    ];
    for (tail, expected) in cases {
        let source = format!("{prefix}{tail}\n.Sh ENDTEST\nDone.\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut actual = section_rows(
            &mant_render::render_query_man(&query),
            "DESCRIPTION",
            "ENDTEST",
        );
        while actual.last().is_some_and(String::is_empty) {
            actual.pop();
        }
        assert_eq!(actual, expected, "{source}");
    }
}
