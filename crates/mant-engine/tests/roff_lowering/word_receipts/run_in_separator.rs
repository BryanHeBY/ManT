//! Accepted reading of a generated inset separator after a BODY rewind.

use super::{description_rows, json_string_roundtrip, verify_cases};
use mant_ir::{Block, Inline, inline_plain_text};
use std::fmt::Write;

type Case = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
);

const CASES: &[Case] = &[
    // Positive HORIZ can consume BACKBEFORE without writing a slot
    // (term.c:678–699). BODY pre then appends the generated U+00A0
    // (mdoc_term.c:767–769); BODY writes can reach before it.
    (
        "F199",
        "",
        r#"No "\zA\p\h'1n'""#,
        r#""\h'-4n'" No B No C"#,
        &["AB C"],
    ),
    (
        "F759",
        "",
        r#"Em "\zA\p\h'1n'""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["B C"],
    ),
    (
        "F760",
        "",
        r#"Em "\zA\p\h'1n'""#,
        r#""\h'-4n'" No B No C"#,
        &["AB C"],
    ),
    (
        "F2442",
        "B",
        r#"Em "\zA\p\h'1n'""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["B C"],
    ),
    (
        "F3003",
        "B",
        r#"Sy "\zA\p\h'1n'""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["B C"],
    ),
    (
        "F4125",
        "I",
        r#"Em "\zA\p\h'1n'""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["B C"],
    ),
    (
        "F4686",
        "I",
        r#"Sy "\zA\p\h'1n'""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["B C"],
    ),
    // F198 overwrites the separator and retains a word-end hard row.
    // In F200 the separator survives as a slot but C covers its column.
    (
        "F198",
        "",
        r#"No "\zA\p\h'1n'""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["AB", "C"],
    ),
    (
        "F200",
        "",
        r#"No "\zA\p\h'1n'""#,
        r#""\h'-4n'" No "\zB" No C"#,
        &["ABC"],
    ),
    // An intervening graph after LF also consumes BACKBEFORE via
    // encode1() (term.c:903–929), leaving the generated U+00A0 editable.
    (
        "F266",
        "",
        r#"No "\zA\pX""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["AB C"],
    ),
    (
        "F267",
        "",
        r#"No "\zA\pX""#,
        r#""\h'-4n'" No B No C"#,
        &["B C"],
    ),
    (
        "F301",
        "",
        r#"No "\zA\p X""#,
        r#""\h'-4n'" No B No C"#,
        &["AB C"],
    ),
    (
        "F539",
        "",
        r#"No "\zA\pX\p""#,
        r#""\h'-4n'" No B No C"#,
        &["XB C"],
    ),
    (
        "F828",
        "",
        r#"Em "\zA\pX""#,
        r#""\h'-4n'" No B No C"#,
        &["B C"],
    ),
    (
        "F862",
        "",
        r#"Em "\zA\p X""#,
        r#""\h'-4n'" No B No C"#,
        &["AB C"],
    ),
    (
        "F1100",
        "",
        r#"Em "\zA\pX\p""#,
        r#""\h'-4n'" No B No C"#,
        &["XB C"],
    ),
    (
        "F1949",
        "B",
        r#"No "\zA\pX""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["AB C"],
    ),
    (
        "F3632",
        "I",
        r#"No "\zA\pX""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["AB C"],
    ),
    // Covered blanks and neighboring absolute-motion and style cases.
    (
        "F268",
        "",
        r#"No "\zA\pX""#,
        r#""\h'-4n'" No "\zB" No C"#,
        &["BC"],
    ),
    (
        "F302",
        "",
        r#"No "\zA\p X""#,
        r#""\h'-4n'" No "\zB" No C"#,
        &["ABC"],
    ),
    (
        "F540",
        "",
        r#"No "\zA\pX\p""#,
        r#""\h'-4n'" No "\zB" No C"#,
        &["XBC"],
    ),
    ("F265", "", r#"No "\zA\pX""#, r#""\h'|0n'B" No C"#, &["BCX"]),
    (
        "F300",
        "",
        r#"No "\zA\p X""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["ABC"],
    ),
    (
        "F538",
        "",
        r#"No "\zA\pX\p""#,
        r#""\h'|1n'" No "\pB" No C"#,
        &["ABC"],
    ),
    (
        "F829",
        "",
        r#"Em "\zA\pX""#,
        r#""\h'-4n'" No "\zB" No C"#,
        &["BC"],
    ),
];

#[test]
fn inset_run_in_separator_reads_at_its_accepted_position() {
    verify_cases(
        CASES.iter().copied(),
        |case| case.0,
        |&(_, font, head, body, rows)| {
            let mut source = String::from(
                ".Dd November 22, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test the manual renderer\n.Sh DESCRIPTION\n",
            );
            if !font.is_empty() {
                writeln!(source, ".ft {font}").unwrap();
            }
            writeln!(source, ".Bl -inset\n.It {head}\n.No {body}\n.El").unwrap();
            if !font.is_empty() {
                source.push_str(".ft R\n");
            }
            source.push_str(".Sh ENDTEST\nDone.\n");

            let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let transported = json_string_roundtrip(&original);
            let rendered = mant_render::render_query_man(&transported);
            let actual: Vec<_> = description_rows(&rendered, &source)
                .iter()
                .map(|row| {
                    row.split(|ch: char| !ch.is_ascii_uppercase())
                        .filter(|word| !word.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|row| !row.is_empty())
                .collect();
            assert_eq!(
                actual, rows,
                "accepted reading and hard rows from {rendered:?}"
            );
            if !font.is_empty() {
                let [Block::DefinitionList { items, .. }] =
                    transported.document.as_ref().unwrap().sections[1]
                        .blocks
                        .as_slice()
                else {
                    panic!("expected an inset definition list");
                };
                let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
                    panic!("expected an inset body paragraph");
                };
                assert_eq!(inline_plain_text(children), "B C");
                assert!(
                    children.iter().any(|inline| match (font, inline) {
                        ("B", Inline::Strong { children })
                        | ("I", Inline::Emphasis { children }) =>
                            inline_plain_text(children) == " ",
                        _ => false,
                    }),
                    "the accepted separator keeps the generated word's font"
                );
            }
        },
    );
}

#[test]
fn inset_absolute_body_replays_a_decorated_combining_head() {
    // term.c::encode1() writes underscore and backspace for every codepoint
    // above 127 even when its display width is zero. mdoc_term.c::termp_it_pre()
    // writes the inset separator before ESCAPE_HORIZ revisits column zero.
    for suffix in ["", "\\p "] {
        let source = format!(
            ".Dd November 26, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd observer\n.Sh DESCRIPTION\n.Bl -inset\n.It No \"\\fIa\u{301}\\fP{suffix}\"\n\\h'|0n'e\u{301} i\u{301}\n.El\n.Sh ENDTEST\nDone.\n"
        );
        let content =
            json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let rendered = mant_render::render_query_man(&content);
        let rows = description_rows(&rendered, &source);
        let rows: Vec<_> = rows
            .iter()
            .map(|row| row.trim())
            .filter(|row| !row.is_empty())
            .collect();
        assert_eq!(rows, ["e\u{301}ai\u{301}\u{301}"], "{source}\n{rendered}");
    }
}
