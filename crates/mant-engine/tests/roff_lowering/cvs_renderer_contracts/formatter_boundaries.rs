use super::*;

#[test]
fn formatter_boundaries_precede_pending_zero_advance_glyphs() {
    // CVS `term_word()` buffers each word boundary before decoding `\zX`;
    // BACKBEFORE then lets X overwrite the following word's boundary.  The
    // ordering is the same for an explicit empty word, `\&`, and the fixed
    // blank glyph `\0`, with or without `.mc` settling the previous field.
    for predecessor in [r#""""#, r"\&", r"\0", r"\~"] {
        for zero_expression in [r"\zX", r"\p\zX", r"\zX\p"] {
            for margin_request in ["", ".mc\n"] {
                let source = format!(
                    ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.No {predecessor}\n{margin_request}.No {zero_expression}\n.No BODY\n"
                );
                let native = without_line_indentation(&native_terminal(&source));
                assert!(
                    native.contains("XBODY"),
                    "{predecessor:?}/{zero_expression:?}/{margin_request:?} native output: {native:?}"
                );
                let lowered = lowered_terminal(&source);
                assert!(
                    lowered.contains("XBODY"),
                    "{predecessor:?}/{zero_expression:?}/{margin_request:?} lowered output: {lowered:?}"
                );
                assert!(
                    !lowered.contains("X BODY"),
                    "boundary moved after pending glyph: {lowered:?}"
                );
            }
        }
    }
}

/// Author a separate section boundary; page furniture can contain arbitrary
/// `.Os` text (`mdoc_validate.c::post_os`, `mdoc_term.c::print_mdoc_foot`).
fn framed_source(source: &str) -> String {
    if source.starts_with(".TH ") {
        format!("{source}.SH NEXT\nEND\n")
    } else {
        format!("{source}.Sh NEXT\n.No END\n")
    }
}

/// Rows between the two authored headings, with overstrikes collapsed.
/// Only section-end spacing is trimmed; interior blank rows remain exact.
fn body_rows(output: &str) -> Vec<String> {
    let visible = apply_terminal_backspaces(output);
    let lines = visible.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == "DESCRIPTION")
        .expect("authored DESCRIPTION heading");
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim() == "NEXT")
        .map(|index| index + start + 1)
        .expect("authored NEXT heading before page furniture");
    let mut rows = lines[start + 1..end]
        .iter()
        .map(|line| line.trim().to_owned())
        .collect::<Vec<_>>();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn assert_rows_match_reference(source: &str) {
    let source = framed_source(source);
    let native = body_rows(&native_terminal(&source));
    let lowered = body_rows(&lowered_terminal(&source));
    assert_eq!(
        lowered, native,
        "lowered rows must keep the pinned reference structure"
    );
}

#[test]
fn authored_section_bounds_do_not_filter_footer_shaped_body_text() {
    // All four complete inputs ran pristine CVS in all five profiles first.
    // post_os() accepts custom text; print_mdoc_foot() can wrap it across
    // rows. Neither OS words nor footer-like BODY words identify a boundary.
    let long_footer = format!("FixtureOS {}", "long footer ".repeat(12).trim_end());
    for operating_system in [
        "Linux 6.18.33.2-microsoft-standard-WSL2",
        "Darwin 23.6.0",
        "FixtureOS",
        long_footer.as_str(),
    ] {
        let source = framed_source(&format!(
            ".Dd September 30, 2026\n.Dt TEST 1\n.Os {operating_system}\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\nLinux authored\n\nSeptember authored\nBODY(1)\n.fi\n"
        ));
        let expected = ["Linux authored", "", "September authored", "BODY(1)"];
        assert_eq!(body_rows(&native_terminal(&source)), expected);
        assert_eq!(body_rows(&lowered_terminal(&source)), expected);
    }
}

#[test]
fn wipe_rejection_retires_across_explicit_row_breaks() {
    // term_flushln() resets the buffer even when its nbr == 0 pass printed
    // nothing (term.c:143-146 with 233-237), and that pass ends its own row
    // (term.c:250-253). A `.br` after a rejected `\p` unit must therefore
    // keep the following words AND the asserted empty row.
    assert_rows_match_reference(".TH TEST 1\n.SH DESCRIPTION\n\\p Y\n.br\nAFTER\n");
    assert_rows_match_reference(
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n\\p Y\n.br\nAFTER\n",
    );
    assert_rows_match_reference(
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -item\n.It\n\\p Y\n.br\nAFTER\n.El\n",
    );
    assert_rows_match_reference(
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.nf\n\\p Y\n.br\nAFTER\n.fi\n",
    );
    // A graph before the marker keeps its committed row; the rejected
    // remainder still asserts the empty row (term.c:220 with 250-253).
    assert_rows_match_reference(".TH TEST 1\n.SH DESCRIPTION\nX \\p Y\n.br\nAFTER\n");
}

#[test]
fn rejected_units_retire_glyphs_and_word_state_at_real_flushes() {
    // Every exact input below was run with the registered pristine CVS
    // -Tutf8 and -Tlint before these assertions were added. term_fill()'s
    // nbr=0 pass rejects the unit; term_flushln() resets its complete buffer
    // and BACKBEFORE/BACKAFTER (term.c:143-146, 233-237), including a cached
    // \zX. roff_term_pre_mc() runs that reset with NOBREAK, while .br ends
    // the row. In no-fill, print_man_node()/print_mdoc_node() execute the
    // next NODE_LINE first: X then belongs to a new, accepted unit, and a
    // bare \z must still overstrike the A of AFTER.
    for header in [
        ".TH TEST 1\n.SH DESCRIPTION\n",
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n",
    ] {
        for no_fill in [false, true] {
            for pending in ["", "\\zX\n", "\\z\n", "\\zX\\z\n", "\\zX\\z\\fB\n"] {
                for request in [".br", ".mc"] {
                    let source = format!(
                        "{header}{}\\p Y\n{pending}{request}\nAFTER\n{}",
                        if no_fill { ".nf\n" } else { "" },
                        if no_fill { ".fi\n" } else { "" },
                    );
                    let source = framed_source(&source);
                    let expected = if no_fill {
                        match pending {
                            "\\zX\n" | "\\zX\\z\n" | "\\zX\\z\\fB\n" => {
                                vec!["", "X", "AFTER"]
                            }
                            "\\z\n" => vec!["", "FTER"],
                            _ => vec!["", "AFTER"],
                        }
                    } else if request == ".br" {
                        vec!["", "AFTER"]
                    } else {
                        vec!["AFTER"]
                    };
                    assert_eq!(
                        body_rows(&native_terminal(&source)),
                        expected,
                        "native: {source:?}"
                    );
                    assert_eq!(
                        body_rows(&lowered_terminal(&source)),
                        expected,
                        "lowered: {source:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn non_rejected_zero_advance_state_survives_only_empty_flushes() {
    // Exact sources verified with the registered CVS -Tutf8: encode1()
    // writes \zX into the native buffer, so term_flushln() accepts X; a
    // bare \z writes no cell, so both term_newln() and roff_term_pre_mc()
    // leave BACKAFTER armed (term.c::encode1/term_newln, roff_term.c::pre_mc).
    for (pending, request, expected) in [
        ("\\zX", ".br", vec!["X", "AFTER"]),
        ("\\zX", ".mc", vec!["X AFTER"]),
        ("\\z", ".br", vec!["FTER"]),
        ("\\z", ".mc", vec!["FTER"]),
        ("A\\z", ".mc", vec!["A AFTER"]),
        ("\\zX\\z", ".mc", vec!["X AFTER"]),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n{pending}\n{request}\nAFTER\n");
        let source = framed_source(&source);
        assert_eq!(
            body_rows(&native_terminal(&source)),
            expected,
            "native: {source:?}"
        );
        assert_eq!(
            body_rows(&lowered_terminal(&source)),
            expected,
            "lowered: {source:?}"
        );
    }
}

#[test]
fn margin_flush_retires_accepted_run_in_cells_before_the_next_word() {
    // These exact inset inputs passed the registered pristine CVS -Tutf8
    // and -Tlint. Inset HEAD post leaves its buffer active; BODY's generated
    // fixed blank is a graph even after \p (mdoc_term.c::termp_it_pre/post).
    // .mc accepts that buffer under NOBREAK and clears it at term.c:233-237:
    // the next BODY word must not re-feed its consumed break marker. Pin
    // physical row membership here; normalize spacing inside each row
    // because the run-in fixed blank's columns are device geometry.
    for (head, expected) in [("\\p", vec!["BodyWord"]), ("X\\p", vec!["X BodyWord"])] {
        let source = if head == "\\p" {
            include_str!("../field_retirement_matrix/cases/buffer_mc_pX_noX.1").to_owned()
        } else {
            format!(
                ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -inset\n.It Xo\n.No \"{head}\"\n.Xc\n.mc\n.No BodyWord\n.El\n"
            )
        };
        let source = framed_source(&source);
        let row_words = |output: &str| {
            body_rows(output)
                .into_iter()
                .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            row_words(&native_terminal(&source)),
            expected,
            "native: {source:?}"
        );
        assert_eq!(
            row_words(&lowered_terminal(&source)),
            expected,
            "lowered: {source:?}"
        );
    }
}

#[test]
fn marker_blank_separator_follows_the_written_separator_receipt() {
    // The surviving separator of the `ph` shape is the blank term_word()
    // actually wrote before the marker (term.c:573-580): `.Sm off` from the
    // second word on suppresses it (TERMP_NONOSPACE re-arms NOSPACE), while
    // the first fragment after the transition still prints it.
    let shape = |spacing: &str| {
        format!(
            ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n{spacing}\n.No \\zA\n.No \"\\p B\"\n.Sm on\n.No C\n"
        )
    };
    assert_rows_match_reference(&shape(".Sm on"));
    assert_rows_match_reference(&shape(".Sm off"));
    assert_rows_match_reference(
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.No \\zA\n.Sm off\n.No \"\\p B\"\n.Sm on\n.No C\n",
    );
    assert_rows_match_reference(
        ".Dd September 30, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Sm off\n.No \\zA\n.No \"\\p B\"\n.No C\n",
    );
}
