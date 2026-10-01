//! Accepted native rows remain literal content after unprinted blanks retire.

use super::{Block, parse_manual_bytes};

const MAN_HEADER: &str = ".TH TEST 1 \"2026-09-28\"\n.SH DESCRIPTION\n";
const MDOC_HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn literal_text(header: &str, body: &str) -> String {
    let decoded = literal_document(header, body);
    let section = decoded
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .expect("description section");
    let [Block::Preformatted { children, .. }] = section.blocks.as_slice() else {
        panic!(
            "literal content must keep a single owner: {:#?}",
            section.blocks
        );
    };
    mant_ir::inline_plain_text(children)
}

fn literal_document(header: &str, body: &str) -> mant_ir::Document {
    let next = if header == MAN_HEADER {
        ".SH NEXT\nEND\n"
    } else {
        ".Sh NEXT\n.No END\n"
    };
    let source = format!("{header}.nf\n{body}.fi\n{next}");
    let document = parse_manual_bytes(
        std::path::Path::new("literal-space-rows.1"),
        source.as_bytes(),
    )
    .expect("lower exact literal-row source");
    let json = serde_json::to_string(&document).expect("serialize literal-row document");
    let decoded: mant_ir::Document = serde_json::from_str(&json).expect("decode actual JSON text");
    assert_eq!(decoded, document);
    decoded
}

#[test]
fn accepted_zero_width_rows_survive_unprinted_tail_retirement() {
    // Exact sources ran on fixed CVS ASCII/UTF-8/HTML/tree/lint first.
    // term_fill()340-349 counts ASCII_NBRZW as graph, while term_field()
    //389-427 emits neither that cell nor its trailing breakable blanks.
    // The no-fill source gate still closes this accepted physical row.
    // mdoc reports a trailing-whitespace warning for the raw padded form;
    // its deterministic rendering is covered without calling it lint-clean.
    for header in [MAN_HEADER, MDOC_HEADER] {
        for middle in ["\\&        \n", "\\&\n", "\\&        \\p\n"] {
            assert_eq!(
                literal_text(header, &format!("ALPHA\n{middle}BETA\n")),
                "ALPHA\n\nBETA",
                "{header:?}, {middle:?}"
            );
        }
        for (body, expected) in [
            ("\\&        \nBETA\n", "\nBETA"),
            ("ALPHA\n\\&        \n", "ALPHA\n"),
        ] {
            assert_eq!(literal_text(header, body), expected, "{header:?}, {body:?}");
        }
    }
    // The exact styled man source likewise accepts the zero-width row.
    assert_eq!(
        literal_text(MAN_HEADER, "ALPHA\n.B \"\\&        \"\nBETA\n"),
        "ALPHA\n\nBETA"
    );
}

#[test]
fn source_continuation_and_bare_backtracking_do_not_invent_rows() {
    // man_term.c::print_man_node and mdoc_term.c::print_mdoc_node apply
    // NODE_LINE before the word; NONEWLINE suppresses that one boundary.
    // A bare BACKAFTER has no buffered graph, so term_newln()475-481
    // leaves it armed for B, unlike a buffered \zX followed by blanks.
    for header in [MAN_HEADER, MDOC_HEADER] {
        for (middle, expected) in [
            ("\\&\\c\n", "ALPHA\nBETA"),
            ("\\&        \\c\n", "ALPHA\n        BETA"),
            ("\\z\n", "ALPHA\nETA"),
            ("\\fB\n", "ALPHA\nBETA"),
        ] {
            assert_eq!(
                literal_text(header, &format!("ALPHA\n{middle}BETA\n")),
                expected,
                "{header:?}, {middle:?}"
            );
        }
    }
    assert_eq!(
        literal_text(MAN_HEADER, "ALPHA\n\\zX        \nBETA\n"),
        "ALPHA\nX\nBETA"
    );
    assert_eq!(
        literal_text(MAN_HEADER, "ALPHA\nALPHA        \nBETA\n"),
        "ALPHA\nALPHA\nBETA"
    );
}

#[test]
fn whitespace_content_and_empty_word_calls_keep_distinct_rows() {
    // Exact native runs distinguish empty TEXT (term_vspace) from an
    // empty term_word(), and fixed Unicode spaces from deferred blanks.
    // Plain ASCII source whitespace may remain in source-neutral literal
    // IR; only its exact physical-row count is asserted in that counter.
    for header in [MAN_HEADER, MDOC_HEADER] {
        for (middle, expected) in [
            ("\n", "ALPHA\n\nBETA"),
            ("\u{a0}\u{a0}\n", "ALPHA\n\u{a0}\u{a0}\nBETA"),
            ("\\~\\~\n", "ALPHA\n\u{a0}\u{a0}\nBETA"),
            ("\\0\n", "ALPHA\n\u{a0}\nBETA"),
        ] {
            assert_eq!(
                literal_text(header, &format!("ALPHA\n{middle}BETA\n")),
                expected,
                "{header:?}, {middle:?}"
            );
        }
        let spaces = literal_text(header, "ALPHA\n        \nBETA\n");
        let rows: Vec<_> = spaces.split('\n').collect();
        assert_eq!(rows.len(), 3, "{header:?}: {spaces:?}");
        assert_eq!(rows[0], "ALPHA");
        assert!(rows[1].chars().all(|character| character == ' '));
        assert_eq!(rows[2], "BETA");
    }
    for (header, middle, expected) in [
        (MAN_HEADER, ".B \"\"\n", "ALPHA\n\nBETA"),
        (MAN_HEADER, ".BR \"\" \"\"\n", "ALPHA\nBETA"),
        (MDOC_HEADER, ".No \"\"\n", "ALPHA\nBETA"),
        (MDOC_HEADER, ".No \"\" No \"\"\n", "ALPHA\n\nBETA"),
    ] {
        assert_eq!(
            literal_text(header, &format!("ALPHA\n{middle}BETA\n")),
            expected,
            "{header:?}, {middle:?}"
        );
    }
}

#[test]
fn repeated_accepted_zero_width_rows_keep_every_content_row() {
    // Complete 64- and 1024-row sources ran on all five fixed profiles
    // before this assertion. Each accepted NBRZW row ends independently;
    // the tail query's separate visit-count unit test bounds its work.
    for count in [64, 1024] {
        let body = format!("ALPHA\n{}BETA\n", "\\&        \n".repeat(count));
        assert_eq!(
            literal_text(MAN_HEADER, &body),
            format!("ALPHA{}BETA", "\n".repeat(count + 1))
        );
    }
}

#[test]
fn native_marker_passes_keep_printed_prefixes_and_separate_empty_rows() {
    // All complete sources ran fixed CVS ASCII/UTF-8/HTML/tree/lint first.
    // NBRZW sets graph without printing (term.c:340-349,397). Each genuine
    // remaining field executes loop endline once (217), followed by the
    // rejected suffix's own tail endline (143-146,250-253). A preceding
    // continued word occupies only the first accepted physical row.
    for count in [1, 2, 64, 1024] {
        let prefix = "\\&\\p ".repeat(count);
        for (inherited, before) in [(false, ""), (true, ".No BEFORE\\c\n")] {
            let rows_before = if inherited { "BEFORE" } else { "" };
            for (tail, expected) in [
                (
                    "\\p REJECTED",
                    format!("{rows_before}{}AFTER", "\n".repeat(count + 1)),
                ),
                (
                    "ACCEPTED",
                    format!("{rows_before}{}ACCEPTED\nAFTER", "\n".repeat(count)),
                ),
                ("", format!("{rows_before}\nAFTER")),
            ] {
                let body = format!("{before}.No \"{prefix}{tail}\"\n.No AFTER\n");
                assert_eq!(literal_text(MDOC_HEADER, &body), expected, "{body:?}");
            }
        }
    }
}

#[test]
fn retired_prefixes_and_semantic_wrappers_do_not_replay_empty_pass_rows() {
    // Exact sources ran all five fixed profiles first. Lk executes its
    // label/colon/target through term_word (mdoc_term.c:1881-1915), so
    // rejected visible ranges must keep the link identity. The complete
    // br/sp/mc layout matrix is exercised by the final engine consumer.
    for count in [1, 2, 64, 1024] {
        let word = format!("{}\\p REJECTED", "\\&\\p ".repeat(count));
        for (wrapper, argument) in [("No", ""), ("Em", ""), ("Lk", "https://example.org ")] {
            let body = format!(".No BEFORE\\c\n.{wrapper} {argument}\"{word}\"\n.No AFTER\n");
            assert_eq!(
                literal_text(MDOC_HEADER, &body),
                format!("BEFORE{}AFTER", "\n".repeat(count + 1)),
                "{body:?}"
            );
            if wrapper == "Lk" {
                let document = literal_document(MDOC_HEADER, &body);
                let targets = super::control_boundaries::document_link_targets(&document);
                assert_eq!(targets.len(), 1, "{body:?}");
                assert_eq!(
                    targets[0],
                    mant_ir::LinkTarget::External {
                        uri: "https://example.org".to_owned()
                    },
                    "{body:?}"
                );
            }
        }
    }
}

const LITERAL_ROW_CASES: [(&str, [&str; 3]); 12] = [
    ("\\&        \n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
    ("\\&\n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
    ("        \n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
    ("\n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
    (
        "\u{a0}\u{a0}\n",
        [
            "ALPHA\n\u{a0}\u{a0}\nBETA",
            "\u{a0}\u{a0}\nBETA",
            "ALPHA\n\u{a0}\u{a0}",
        ],
    ),
    (
        "\\~\\~\n",
        [
            "ALPHA\n\u{a0}\u{a0}\nBETA",
            "\u{a0}\u{a0}\nBETA",
            "ALPHA\n\u{a0}\u{a0}",
        ],
    ),
    (
        "\\0\n",
        ["ALPHA\n\u{a0}\nBETA", "\u{a0}\nBETA", "ALPHA\n\u{a0}"],
    ),
    ("\\fB\n", ["ALPHA\nBETA", "BETA", "ALPHA"]),
    ("\\z\n", ["ALPHA\nETA", "ETA", "ALPHA"]),
    ("\\&\\c\n", ["ALPHA\nBETA", "BETA", "ALPHA\n"]),
    (
        "\\&        \\c\n",
        ["ALPHA\n        BETA", "        BETA", "ALPHA\n"],
    ),
    ("\\&        \\p\n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
];

#[test]
fn literal_row_sources_keep_all_entered_and_retired_boundary_positions() {
    // This complete finite 2 macro sets x14 word forms x3 placements
    // corresponds to all84 distinct sources recorded before assertions in
    // the fixed ASCII/UTF-8/HTML/tree/lint probe. No LF is folded or removed;
    // mdoc warns on its six raw trailing-ASCII-space inputs as expected.
    // term_newln()475-481 leaves bare BACKAFTER armed when no cell exists.
    // pre_SH()/termp_sh_pre() add space but do not consume it, so the next
    // heading's N is overwritten and its native visible title is EXT.
    let mut cases_run = 0;
    for header in [MAN_HEADER, MDOC_HEADER] {
        let macro_cases = if header == MAN_HEADER {
            [
                (".B \"\"\n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
                (".BR \"\" \"\"\n", ["ALPHA\nBETA", "BETA", "ALPHA"]),
            ]
        } else {
            [
                (".No \"\"\n", ["ALPHA\nBETA", "BETA", "ALPHA"]),
                (".No \"\" No \"\"\n", ["ALPHA\n\nBETA", "\nBETA", "ALPHA\n"]),
            ]
        };
        for (middle, expected) in LITERAL_ROW_CASES.into_iter().chain(macro_cases) {
            for (index, body) in [
                format!("ALPHA\n{middle}BETA\n"),
                format!("{middle}BETA\n"),
                format!("ALPHA\n{middle}"),
            ]
            .into_iter()
            .enumerate()
            {
                let document = literal_document(header, &body);
                let section = document
                    .sections
                    .iter()
                    .find(|section| section.heading.plain_text() == "DESCRIPTION")
                    .unwrap();
                let [Block::Preformatted { children, .. }] = section.blocks.as_slice() else {
                    panic!("{header:?}/{body:?}: {:#?}", section.blocks);
                };
                let actual = mant_ir::inline_plain_text(children);
                if middle == "        \n" {
                    // Keep the existing source-neutral ASCII padding facet.
                    // The device prints no trailing cells; every hard row
                    // and every non-padding scalar is compared separately.
                    let rows: Vec<_> = actual.split('\n').collect();
                    let native: Vec<_> = expected[index].split('\n').collect();
                    assert_eq!(rows.len(), native.len(), "{header:?}/{body:?}");
                    for (row, native) in rows.into_iter().zip(native) {
                        assert_eq!(row.trim_end_matches(' '), native, "{header:?}/{body:?}");
                    }
                } else {
                    assert_eq!(actual, expected[index], "{header:?}/{body:?}");
                }
                let next = document.sections.last().unwrap();
                let expected_heading = if middle == "\\z\n" && index == 2 {
                    "EXT"
                } else {
                    "NEXT"
                };
                assert_eq!(
                    next.heading.plain_text(),
                    expected_heading,
                    "{header:?}/{body:?}"
                );
                cases_run += 1;
            }
        }
    }
    assert_eq!(cases_run, 84);
}
