//! Ordered native text writes retain glyphs and hard rows through JSON.

use libmandoc_rs::{Node, NodeKind};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    expected_rows: Vec<String>,
}

fn contains_extended_head(node: &Node) -> bool {
    (node.kind == NodeKind::Head
        && node.macro_token.as_deref() == Some("It")
        && node
            .children
            .iter()
            .any(|child| child.macro_token.as_deref() == Some("Xo")))
        || node.children.iter().any(contains_extended_head)
}

fn assert_rows(prefix: &str) {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("native_text_cells/cases.json")).unwrap();
    let mut checked = 0;
    for case in cases.iter().filter(|case| case.id.starts_with(prefix)) {
        if case.id.starts_with("hang-") || case.id.starts_with("tag-") {
            // The Xo operands must really execute in It HEAD, rather than
            // merely occur in the source of a differently parsed list kind.
            let parsed = libmandoc_rs::Parser::default()
                .parse_bytes("native-text-cells.1", case.source.as_bytes())
                .unwrap();
            assert!(contains_extended_head(&parsed.document.root), "{}", case.id);
        }
        let query = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let json = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let rendered = mant_render::render_query_man(&decoded.into());
        let region = rendered
            .split_once("DESCRIPTION\n")
            .unwrap()
            .1
            .split_once("ENDTEST\n")
            .unwrap()
            .0;
        let mut rows: Vec<_> = region.split_terminator('\n').collect();
        // Exactly one section separator is furniture. Preserve all other
        // empty rows, indentation, trailing spaces and word boundaries.
        assert_eq!(rows.pop(), Some(""), "{}: section separator", case.id);
        let expected =
            crate::oracle_migrations::literal_reading::rows(&case.source, &case.expected_rows);
        assert_eq!(rows, expected, "{}", case.id);
        checked += 1;
    }
    assert!(checked > 0, "missing {prefix} cases");
}

// Every fixture ran through the locked pristine CVS before these assertions.
// term.c::term_word/encode/encode1 preserve ordered scalar/sentinel writes;
// term_fill handles break markers and zero-width graphs independently of
// visible output. These cases protect the bulk ordinary-text fast path and
// the pending/suppressed-blank slow path, including styled words.
#[test]
fn literal_text_writes_preserve_unicode_markers_and_zero_width_graphs() {
    assert_rows("literal-");
}

#[test]
fn hanging_field_writes_keep_accepted_and_rejected_prefixes() {
    assert_rows("hang-");
}

#[test]
fn tagged_field_writes_keep_accepted_and_rejected_prefixes() {
    assert_rows("tag-");
}

#[test]
fn every_printable_ascii_scalar_survives_native_recording() {
    assert_rows("all-printable-ascii");
}

#[test]
fn edited_table_prose_keeps_soft_width_and_hard_markers_distinct() {
    // On both fixed terminal profiles, tbl_term.c::tbl_word writes PT in
    // these continued cells. term_fill wraps the remaining words according
    // to w(), and the marked case additionally forces a break after PT.
    let header = ".TH PROBE 1 \"November 19, 2026\"\n.SH DESCRIPTION\n";
    let words = "ALPHA BRAVO CHARLIE DELTA EXTRA GAMMA PLAIN MORE TEXT HERE";
    for width in ["8n", "16n", "48n"] {
        for motion in [r"\h'-1n'", r"\h'|1n'"] {
            for marker in ["", r"\p"] {
                let source = format!(
                    "{header}.TS\nbox tab(|);\nl lw({width}).\n{}|T{{\nPM{motion}T{marker} {words}\nT}}\n.TE\n",
                    "W".repeat(42)
                );
                let decoded = super::json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
                );
                let [mant_ir::Block::Table { rows, .. }] =
                    decoded.document.as_ref().unwrap().sections[0]
                        .blocks
                        .as_slice()
                else {
                    panic!("{source}");
                };
                let [mant_ir::Block::Paragraph { children, .. }] =
                    rows[0].cells[1].blocks.as_slice()
                else {
                    panic!("{source}");
                };
                let projected = mant_ir::inline_plain_text(children);
                let separator = if marker.is_empty() { ' ' } else { '\n' };
                assert_eq!(projected, format!("PT{separator}{words}"), "{source}");
            }
        }
    }
}

#[test]
fn edited_tbl_word_end_markers_attach_to_the_word_they_execute_with() {
    // term.c::term_fill() arms the end of the current word at \p. A
    // width stop before that word must not create a second hard break.
    let head =
        ".TH PAIR 1 \"November 19, 2026\"\n.SH NAME\npair \\- table probe\n.SH DESCRIPTION\n";
    for alignment in ['l', 'r', 'c'] {
        for width in [8, 80] {
            for word in [r"\pBRAVO", r"BR\pAVO", r"BRAVO\p", r"\pBRAVO\p"] {
                for prefix in ["PT", r"PM\h'-1n'T"] {
                    let source = format!(
                        "{head}.TS\n{alignment}w({width}n).\nT{{\n{prefix} ALPHA {word} CHARLIE DELTA\nT}}\n.TE\n.SH AFTER\nEnd.\n"
                    );
                    assert_cell_word_end(&source);
                }
            }
        }
    }
}

fn assert_cell_word_end(source: &str) {
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let restored = super::json_roundtrip::json_string_roundtrip(&loaded);
    let [mant_ir::Block::Table { rows, .. }] = restored.document.as_ref().unwrap().sections[1]
        .blocks
        .as_slice()
    else {
        panic!("{source}");
    };
    let [mant_ir::Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("{source}");
    };
    assert_eq!(
        mant_ir::inline_plain_text(children),
        "PT ALPHA BRAVO\nCHARLIE DELTA",
        "{source}"
    );
    let markdown = mant_codec::encode::render_markdown(&restored);
    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for projection in [&restored, &decoded] {
        use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax};
        let found = mant_query::search_query(
            projection,
            &SearchQuery {
                pattern: "ALPHA BRAVO".into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope: SearchScope::Visible,
                word: true,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(found.total, 1, "{source}\n{markdown}");
    }
}

#[test]
fn a_width_choice_still_controls_whether_tbl_overstrikes_cover_a_glyph() {
    // Fixed ASCII and UTF-8 term_field() retain b when the break point
    // takes Y onto the next pass, and cover b when the two are on one pass.
    let header = ".TH PROBE 1 \"November 19, 2026\"\n.SH DESCRIPTION\n.TS\nl.\n";
    for (witness, expected) in [("", "LEFTb\nYTAIL"), ("WWWWWWWWWW\n", "LEFTYTAIL")] {
        let source = format!("{header}LEFT\\zb\\:YTAIL\n{witness}.TE\n");
        let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let [mant_ir::Block::Table { rows, .. }] = content.document.as_ref().unwrap().sections[0]
            .blocks
            .as_slice()
        else {
            panic!("{source}");
        };
        let [mant_ir::Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice()
        else {
            panic!("{source}");
        };
        assert_eq!(mant_ir::inline_plain_text(children), expected, "{source}");
    }
}
