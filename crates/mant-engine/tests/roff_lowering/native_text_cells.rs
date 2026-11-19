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
                if marker.is_empty() {
                    assert_eq!(projected, format!("PT {words}"), "{source}");
                } else {
                    assert_eq!(projected.lines().next(), Some("PT"), "{source}");
                    assert_eq!(
                        projected.split_whitespace().skip(1).collect::<Vec<_>>(),
                        words.split_whitespace().collect::<Vec<_>>(),
                        "{source}"
                    );
                }
            }
        }
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
