use super::*;

#[test]
fn continued_literal_definition_body_reaches_the_text_consumer_on_the_head_row() {
    // All eight complete inputs ran pristine in all five profiles first.
    // mdoc_term.c::print_mdoc_node() observes NODE_LINE before BODY dispatch;
    // TERMP_NONEWLINE from \c alone keeps that physical row open.
    fn content_lines(output: &str) -> Vec<String> {
        // The list's leading spacing is outside this row-connection check;
        // every later empty row remains visible to the assertion.
        framed_definition_rows(output)
            .into_iter()
            .skip_while(String::is_empty)
            .collect()
    }
    let long_footer = format!("FixtureOS {}", "long footer ".repeat(12).trim_end());
    for operating_system in [
        "Linux 6.18.33.2-microsoft-standard-WSL2",
        "Darwin 23.6.0",
        "FixtureOS",
        long_footer.as_str(),
    ] {
        for (head, same_row) in [(".No X\\c", true), (".No X", false)] {
            let source = format!(
                ".Dd September 28, 2026\n.Dt TEST 1\n.Os {operating_system}\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.Bl -hang -width 4n\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n"
            );
            let native = content_lines(&native_terminal(&source));
            let lowered = content_lines(&lowered_terminal(&source));
            assert_eq!(
                native.len(),
                if same_row { 1 } else { 2 },
                "{head}: {native:?}"
            );
            assert_eq!(lowered.len(), native.len(), "{head}: {lowered:?}");
            if same_row {
                assert!(lowered[0].contains('X') && lowered[0].contains("BODY"));
            } else {
                assert_eq!(lowered[0], "X");
                assert_eq!(lowered[1], "BODY");
            }
        }
    }
}

#[test]
fn tag_head_post_and_inset_body_own_distinct_physical_rows() {
    // Both exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // mdoc_term.c::termp_it_post() closes TAG HEAD after its inner .br;
    // a later BODY \& and .br close a new row, never the old HEAD row.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    let tag =
        format!("{prefix}.Bl -tag -width 4n\n.It Xo X\n.br\n.Xc\n.No \\&\n.br\n.No BODY\n.El\n");
    let native = without_line_indentation(&native_terminal(&tag));
    let lowered = lowered_terminal(&tag)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(native.contains("X\n\nBODY"), "CVS: {native:?}");
    assert!(lowered.contains("X\n\nBODY"), "IR: {lowered:?}");

    let inset = format!(
        "{prefix}.nf\n.Bl -inset\n.It Xo first\nsecond\n.Xc\n.No \\&\n.br\n.No BODY\n.El\n.fi\n"
    );
    let native = without_line_indentation(&native_terminal(&inset));
    let lowered = lowered_terminal(&inset)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        native.contains("first\nsecond\n\n\nBODY"),
        "CVS: {native:?}"
    );
    assert!(
        lowered.contains("first\nsecond\n\n\nBODY"),
        "IR: {lowered:?}"
    );

    for (body, blank_rows) in [
        (".No \\& No \"\"\n.br\n.No BODY\n", 2),
        (".No \\&\n.sp 1\n.No BODY\n", 3),
        (".No \"\"\n.br\n.No BODY\n", 1),
    ] {
        let source =
            format!("{prefix}.nf\n.Bl -inset\n.It Xo first\nsecond\n.Xc\n{body}.El\n.fi\n");
        let rows = |text: String| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
        let native = rows(without_line_indentation(&native_terminal(&source)));
        let lowered = rows(lowered_terminal(&source));
        let boundary = format!("second{}BODY", "\n".repeat(blank_rows + 1));
        assert!(native.contains(&boundary), "CVS {body}: {native:?}");
        assert!(lowered.contains(&boundary), "IR {body}: {lowered:?}");
    }

    // CVS mdoc_macro.c::blk_exp_close() breaks the intermediate It HEAD
    // when Fo/Fc or Bo/Bc closes there; these are the same ownership event
    // as Xo/Xc, independent of the visible spelling of the HEAD.
    for head in [".It Fo call\n.Fa arg\n.Fc\n", ".It Bo X\n.Bc\n"] {
        let source = format!("{prefix}.nf\n.Bl -inset\n{head}.No \\&\n.br\n.No BODY\n.El\n.fi\n");
        let rows = |text: String| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
        let native = rows(without_line_indentation(&native_terminal(&source)));
        let lowered = rows(lowered_terminal(&source));
        assert!(native.contains("\n\n\nBODY"), "CVS {head}: {native:?}");
        assert!(lowered.contains("\n\n\nBODY"), "IR {head}: {lowered:?}");
    }
}

#[test]
fn mdoc_definition_styles_flush_occupied_zero_advance_heads() {
    // CVS mdoc_term.c::termp_it_post() calls term_newln() for tag, hang, and
    // overhang heads.  The occupied A cell therefore clears a trailing
    // BACKAFTER before the detached BC body executes.
    for style in ["tag", "hang", "ohang"] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width Ds\n.It No A\\z\nBC\n.El\n"
        );
        let native = native_terminal(&source);
        assert!(native.contains("BC"), "{style} native output: {native:?}");
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains("BC"),
            "{style} lowered output: {lowered:?}"
        );
        assert!(!lowered.contains("\nC"), "{style} lost B: {lowered:?}");
    }
}

#[test]
fn invisible_run_in_head_row_has_one_output_owner() {
    // Each exact input was run with the pinned CVS -Tascii/-Tlint. In
    // term.c::term_word(), the second empty word writes a separator cell;
    // \& writes an invisible cell, while one empty word or Ns writes none.
    // term_newln() closes only an occupied cell, and term_vspace() adds its
    // own row. IR term visibility cannot decide whether the row existed.
    let cases = [
        ("pair-br", ".No \"\" No \"\"\n.br", 2),
        ("ignore-br", ".No \\&\n.br", 2),
        ("single-br", ".No \"\"\n.br", 1),
        ("joined-br", ".No \"\" Ns No \"\"\n.br", 1),
        ("pair-sp1", ".No \"\" No \"\"\n.sp 1", 3),
        ("ignore-twice", ".No \\&\n.br\n.No \\&\n.br", 3),
    ];
    for (label, head, expected_newlines) in cases {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -inset\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        for (kind, output) in [("native", native), ("lowered", lowered)] {
            let (_, tail) = output.split_once("DESCRIPTION").unwrap();
            let (before_body, _) = tail.split_once("BODY").unwrap();
            assert_eq!(
                before_body.matches('\n').count(),
                expected_newlines,
                "{label} {kind}: {output:?}"
            );
        }
    }
}

#[test]
fn completed_head_rows_and_body_rows_match_the_pinned_terminal() {
    // All 160 exact documents were checked with the fixed CVS binary using
    // -Tutf8 and -Tlint before recording this matrix. In mdoc_term.c,
    // termp_it_pre() enters the BODY after the HEAD post; roff_term.c's
    // pre_br()/pre_sp() then close only the currently occupied device row.
    // A source blank made only of breakable spaces is discarded by
    // term.c::term_fill(), whereas \~ is a fixed cell on the UTF-8 device.
    fn rows_before_body(output: &str) -> usize {
        let description = output
            .split_once("DESCRIPTION")
            .expect("rendered section heading")
            .1;
        description
            .lines()
            .take_while(|line| !line.contains("BODY"))
            .count()
    }
    for style in ["inset", "diag", "hang -width 4n", "tag -width 4n", "ohang"] {
        for label in ["X", "\" \"", r"\~", r"\&"] {
            for head_request in [".br", ".sp 0", ".sp 1", ".sp 2"] {
                for body_request in [".br", ".sp 1"] {
                    let source = format!(
                        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo {label}\n{head_request}\n.Xc\n{body_request}\n.No BODY\n.El\n"
                    );
                    let native = native_terminal(&source);
                    let lowered = lowered_terminal(&source);
                    assert_eq!(
                        rows_before_body(&lowered),
                        rows_before_body(&native),
                        "{style}, {label}, {head_request}, {body_request}: native={native:?}, lowered={lowered:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn empty_formatter_words_after_a_break_occupy_their_new_row() {
    // Both complete inputs passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_word() writes the second empty word's separator even
    // though the last visible character belongs to the preceding row;
    // roff_term.c::pre_br()/pre_sp() then close that occupied row.
    for request in [".br", ".sp 0"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.No X\n.br\n.No \"\" No \"\"\n{request}\n.No BODY\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        for (kind, output) in [("native", native), ("lowered", lowered)] {
            assert!(output.contains("X\n\nBODY"), "{request} {kind}: {output:?}");
        }
    }
}

#[test]
fn blank_link_heads_do_not_claim_a_printed_definition_row() {
    // Each exact input passed fixed CVS -Tascii/-Tutf8/-Tlint. A field made
    // solely of ordinary spaces is discarded by term.c::term_fill(), even
    // when its IR node is a link; a real .Lk target remains visible.
    for head in [
        ".Sx \" \"",
        ".Mt \" \"",
        ".Lk \" \"",
        ".Lk https://example.com \" \"",
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n{head}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        let rows_before_body = |output: &str| {
            output
                .split_once("DESCRIPTION")
                .unwrap()
                .1
                .lines()
                .take_while(|line| !line.contains("BODY"))
                .count()
        };
        assert_eq!(
            rows_before_body(&lowered),
            rows_before_body(&native),
            "{head}: native={native:?}, lowered={lowered:?}"
        );
        if head.starts_with(".Lk https") {
            assert!(lowered.contains("https://example.com"), "{lowered:?}");
        }
    }
}

#[test]
fn styled_breakable_only_head_does_not_create_a_device_row() {
    // These exact tag heads passed fixed CVS -Tascii/-Tutf8/-Tlint. The
    // renderer calls term_fill() after the style macros, so ordinary spaces
    // alone still print no field. A styled \~ remains a fixed UTF-8 cell.
    for macro_name in ["No", "Em", "Sy", "Li"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n.{macro_name} \" \"\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        let native_section = native.split_once("DESCRIPTION").unwrap().1;
        let lowered_section = lowered.split_once("DESCRIPTION").unwrap().1;
        assert_eq!(
            native_section
                .lines()
                .take_while(|line| !line.contains("BODY"))
                .count(),
            lowered_section
                .lines()
                .take_while(|line| !line.contains("BODY"))
                .count(),
            "{macro_name}: native={native:?}, lowered={lowered:?}"
        );
    }
}
