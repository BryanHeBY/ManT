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

/// Rows after the DESCRIPTION section head with page furniture removed and
/// overstrikes collapsed; interior blank rows stay part of the contract.
fn body_rows(output: &str) -> Vec<String> {
    let mut in_body = false;
    let mut rows = Vec::new();
    for line in apply_terminal_backspaces(output).lines() {
        if !in_body {
            in_body = line.trim() == "DESCRIPTION";
            continue;
        }
        let row = line.trim_start().trim_end();
        if row.ends_with("(1)") || row.starts_with("Linux ") || row.starts_with("September ") {
            continue;
        }
        rows.push(row.to_owned());
    }
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn assert_rows_match_reference(source: &str) {
    let native = body_rows(&native_terminal(source));
    let lowered = body_rows(&lowered_terminal(source));
    assert_eq!(
        lowered, native,
        "lowered rows must keep the pinned reference structure"
    );
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
