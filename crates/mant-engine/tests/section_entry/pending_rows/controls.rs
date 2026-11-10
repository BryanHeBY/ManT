//! Negative controls at other positions on the same physical rows.

use super::*;

#[test]
fn new_body_runs_its_leading_break_marker_after_the_heading_flush() {
    // Pinned post_SH(HEAD) ends its input; the BODY's own term_fill may
    // reject the first pass or accept it when it contains ASCII_NBRZW.
    for mode in ["", ".nf\n"] {
        for (control, accepts) in [("\\p", false), ("\\&\\p", true)] {
            let source = format!("{HEADER}{mode}ALPHA\\p\n.SS NEXT\n{control} AFTERMARK\n{FOOTER}");
            let input = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let rendered = mant_render::render_query_man(&input);
            let mut rows = vec!["ALPHA", "NEXT", ""];
            if accepts {
                rows.push(BODY);
            }
            assert_eq!(interval(&rendered), rows, "{source}");
            let first = &input.document.as_ref().unwrap().sections[1];
            let next = &first.children[0];
            assert_eq!(body_text(first), ["ALPHA"], "{source}");
            assert_eq!(
                next.heading.plain_text().trim_start_matches(' '),
                "NEXT",
                "{source}"
            );
            if accepts {
                assert!(
                    matches!(next.blocks.as_slice(), [Block::Paragraph { children, .. }]
                    if inline_plain_text(children) == "\nAFTERMARK"),
                    "{source}"
                );
            } else {
                assert!(
                    matches!(
                        next.blocks.as_slice(),
                        [Block::VerticalSpace { lines: 1, .. }]
                    ),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn retained_break_and_vertical_space_finish_in_the_old_owner() {
    // roff_term.c::roff_term_pre_br -> term_newln commits the blank.
    // The \& successor keeps .br in the AST, unlike the preceding control.
    let input = "\\p BETA\n.br\n\\&\n.SS NEXT\nAFTERMARK\n";
    assert_case(Case {
        input,
        id: "next",
        title: "NEXT",
        old_rows: &[],
        body: BODY,
        rows: &["", "NEXT", BODY],
    });
    let source = format!("{HEADER}{input}{FOOTER}");
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let first = &content.document.as_ref().unwrap().sections[1];
    assert!(
        matches!(
            first.blocks.as_slice(),
            [Block::VerticalSpace { lines: 1, .. }]
        ),
        "{:#?}",
        first.blocks,
    );
    assert_eq!(first.children[0].spacing_before_lines, 0);

    // No section action: the same completed blank survives exactly once.
    let source = format!("{HEADER}\\p BETA\n.br\n\\&\nAFTERMARK\n{FOOTER}");
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    assert_eq!(
        interval(&mant_render::render_query_man(&content)),
        ["", BODY]
    );

    // Unlike the trailing marker, .sp commits an additional real row.
    assert_case(Case {
        input: ".nf\nALPHA\\p\n.sp 1\n.SS NEXT\nAFTERMARK\n",
        id: "next",
        title: "NEXT",
        old_rows: &["ALPHA"],
        body: BODY,
        rows: &["ALPHA", "", "NEXT", BODY],
    });
    assert_case(Case {
        input: ".nf\n\\p\\&\n\\p\\&\n.SS NEXT\nAFTERMARK\n",
        id: "next",
        title: "\nNEXT",
        old_rows: &[],
        body: BODY,
        rows: &["", "", "NEXT", BODY],
    });
}

#[test]
fn section_handoff_does_not_cancel_live_zero_advance_or_continuation() {
    // Pinned term.c preserves BACKBEFORE and NONEWLINE until their real
    // consumers. The bare advance consumes N in NEXT, whereas a buffered
    // X is left before NEXT. Native XNEXT and ALPHANEXT share device rows;
    // structured reading continues to separate the two source owners.
    for (input, title, old_rows, rows) in [
        (
            ".nf\n\\z\n.SS NEXT\nAFTERMARK\n",
            "EXT",
            &[][..],
            &["EXT", BODY][..],
        ),
        (
            ".nf\n\\zX\n.SS NEXT\nAFTERMARK\n",
            "NEXT",
            &["X"][..],
            &["X", "NEXT", BODY][..],
        ),
        (
            ".nf\nALPHA\\c\n.SS NEXT\nAFTERMARK\n",
            "NEXT",
            &["ALPHA"][..],
            &["ALPHA", "NEXT", BODY][..],
        ),
    ] {
        assert_case(Case {
            input,
            id: "next",
            title,
            old_rows,
            body: BODY,
            rows,
        });
    }
}

#[test]
fn section_owner_does_not_expand_the_single_initial_literal_row_exception() {
    // All 24 full sources were checked with pinned CVS. mdoc_term.c's
    // NODE_LINE execution applies to TEXT, Lk and Nm alike. Native keeps
    // each initial row; the complete literal reading policy may omit
    // exactly one, with or without an intervening subsection heading.
    const MDOC: &str =
        ".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd audit\n.Sh FIRST\n.nf\n";
    for heading in ["", ".Ss NEXT\n"] {
        for rows in 0..=3 {
            for (body, shown) in [
                ("AFTERMARK\n", BODY),
                (
                    ".Lk https://audit.test AFTERMARK\n",
                    "AFTERMARK: https://audit.test",
                ),
                (".Nm AFTERMARK\n", BODY),
            ] {
                let controls = "\\&\n".repeat(rows);
                let source = format!("{MDOC}{heading}{controls}{body}.Sh ENDTEST\n.No FINISH\n");
                let input = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                let rendered = mant_render::render_query_man(&input);
                let mut expected = Vec::new();
                if !heading.is_empty() {
                    expected.extend(["", "NEXT"]);
                }
                if rows != 1 {
                    expected.extend(std::iter::repeat_n("", rows));
                }
                expected.extend([shown, ""]);
                assert_eq!(interval(&rendered), expected, "{source}");
            }
        }
    }
}

#[test]
fn an_accepted_empty_pass_is_distinct_from_closing_the_preceding_glyph_row() {
    // term_fill accepts ASCII_NBRZW as graph, even with zero column
    // width. But HEAD's initial marker closes the same old pass if BETA
    // has not yet ended; only a separate accepted pass is a blank row.
    for mode in ["", ".nf\n"] {
        for (body, id, title, old_rows, rows) in [
            (
                "ALPHA\\p \\p\\&\n.SS NEXT\nAFTERMARK\n",
                "next",
                "NEXT",
                &["ALPHA"][..],
                &["ALPHA", "", "NEXT", BODY][..],
            ),
            (
                "ALPHA\\p \\p\\&\n.SS \\p NEXT\nAFTERMARK\n",
                "p-next",
                "",
                &["ALPHA"][..],
                &["ALPHA", "", "", BODY][..],
            ),
            (
                "ALPHA\\p BETA\n.SS \\&\\p NEXT\nAFTERMARK\n",
                "p-next",
                "NEXT",
                &["ALPHA", "BETA"][..],
                &["ALPHA", "BETA", "NEXT", BODY][..],
            ),
        ] {
            assert_case(Case {
                input: &format!("{mode}{body}"),
                id,
                title,
                old_rows,
                body: BODY,
                rows,
            });
        }
    }
}
