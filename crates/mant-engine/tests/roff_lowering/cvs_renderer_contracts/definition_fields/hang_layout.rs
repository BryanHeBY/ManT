use super::*;

#[test]
fn no_fill_hang_source_line_and_explicit_br_keep_distinct_field_gaps() {
    // All exact inputs passed fixed CVS -Tutf8/-Tlint. mdoc_term.c gives
    // HANG HEAD trailspace=1; NODE_LINE calls term_newln() before the next
    // word. An explicit .br then invokes roff_term_pre_br() and clears BRIND.
    let prefix = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.Bl -hang -width 4n\n.It Xo X\n";
    for (middle, expected) in [
        (".No Bob\n", "X Bob"),
        (".br\n.No Bob\n", "XBob"),
        (".Sm off\n.No Bob\n", "X Bob"),
        (".An -split\n.No Bob\n", "X Bob"),
        (".No \"\"\n.No Bob\n", "X Bob"),
        (".No \\&\n.No Bob\n", "X Bob"),
        (".No X\\c\n.No Bob\n", "X XBob"),
    ] {
        let source = format!("{prefix}{middle}.Xc\n.No BODY\n.El\n");
        let native = without_line_indentation(&native_terminal(&source));
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let item = first_definition_item(query.document.as_ref().unwrap());
        let term = item
            .terms
            .iter()
            .map(|part| inline_text(part))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(native.contains(expected), "CVS {middle}: {native:?}");
        assert_eq!(term, expected, "{middle}: {item:?}");
    }
}

#[test]
fn final_hang_field_decides_body_gap_from_executed_columns() {
    // Exact cases checked with pinned CVS -Tutf8/-Tascii/-Tlint. Unlike an
    // ordinary blank, \~ occupies a fixed field cell in the UTF-8 device.
    // mdoc_term.c::
    // termp_it_post() flushes the final HANG field; term.c::term_flushln()
    // retains minbl from a previous field, counts a pending \z glyph, but
    // term_vspace() ends its row.
    let cases = [
        ("short after br", ".No X\n.br\n.No Y", true),
        ("long after br", ".No LONGTEXT\n.br\n.No Y", false),
        (
            "four plus author",
            ".No XXXX\n.br\n.An -split\n.An Bob",
            true,
        ),
        (
            "five plus author",
            ".No XXXXX\n.br\n.An -split\n.An Bob",
            false,
        ),
        ("minbl after br", ".No XXXXXX\n.br\n.No Y", false),
        ("pending zero glyph", ".No XXXXXX\n.br\n.No \\zY", false),
        ("positive sp", ".No LONGTEXT\n.sp 1\n.No Y", true),
        (
            "positive sp then author",
            ".No LONGTEXT\n.sp 1\n.No A\n.An -split\n.An Bob",
            false,
        ),
        ("no field flush", ".No XXXX No Bob", true),
        (
            "breakable blank field",
            ".No X\n.br\n.No \" \"\n.br\n.No Y",
            true,
        ),
        (
            "fixed blank field",
            ".No X\n.br\n.No \\~\n.br\n.No Y",
            false,
        ),
        ("invisible field", ".No X\n.br\n.No \\&\n.br\n.No Y", true),
        ("empty field", ".No X\n.br\n.No \"\"\n.br\n.No Y", true),
    ];
    for (label, head, expected_gap) in cases {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 6n\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        for (kind, output) in [("native", native), ("lowered", lowered)] {
            let (_, tail) = output.split_once("DESCRIPTION").unwrap();
            let (before_body, _) = tail.split_once("BODY").unwrap();
            assert_eq!(
                before_body.trim_end().len() != before_body.len(),
                expected_gap,
                "{label} {kind}: {output:?}"
            );
        }
    }
}

#[test]
fn a_wrapping_hang_field_keeps_only_the_proven_body_word_boundary() {
    // All three fields passed fixed CVS -Tascii/-Tutf8/-Tlint. Following
    // roff_term.c::pre_br(), term.c::term_fill() can wrap inside one HEAD
    // field. The renderer-neutral projection may omit that soft wrap, but
    // it must not merge the final HEAD word with BODY.
    for (field, expected_tail) in [
        ("AA BB CC", "CC BODY"),
        ("YYYYY Z", "Z BODY"),
        ("YYYYY ZZZZZZZZ", "ZZZZZZZZBODY"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No {field}\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        assert!(
            native.contains(expected_tail) || native.contains(&expected_tail.replace(' ', "     ")),
            "{field}: {native:?}"
        );
        // The lowered renderer is column-relative: the reference's fixed
        // five-column gap after a wrapped head word survives verbatim,
        // while its tight join variant collapses to one space.
        assert!(
            lowered.contains(expected_tail)
                || lowered.contains(&expected_tail.replace(' ', "     ")),
            "{field}: {lowered:?}"
        );
    }
    // Exact fixed CVS -Tascii/-Tutf8/-Tlint probe: ASCII wraps at the
    // invisible \: breakpoint, whereas UTF-8 keeps this short field on one
    // line. The source-neutral projection cannot prove the final column and
    // therefore preserves the BODY word boundary in either presentation.
    let source = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No YYYYY\\:Z\n.Xc\n.No BODY\n.El\n";
    let lowered = lowered_terminal(source);
    // Fixed CVS -Tutf8: `\:` buffers ASCII_NBRZW here (chars.c:53, term.c
    // 631-632) — a zero-width graph that never breaks a pass (term.c
    // 340-349), so the field stays one row and BODY concatenates:
    // `X YYYYYZBODY`. The two-row wrap is the ascii-device column
    // (ASCII_BREAK, term.c:287-300), preserved for a -Tascii switch in
    // FieldCell::Breakpoint.
    assert!(lowered.contains("X     YYYYYZBODY"), "{lowered:?}");
    // Exact fixed CVS -Tascii/-Tutf8/-Tlint: roff.c::post_hyph() marks
    // this source hyphen ASCII_HYPH, and term.c::term_fill() wraps after it.
    // It is ordinary text in the AST, so the gap proof must see that marker
    // before readable IR normalizes it to '-'.
    let source = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\nYYYYY-Z\n.Xc\n.No BODY\n.El\n";
    let native = native_terminal(source);
    let lowered = lowered_terminal(source);
    assert!(native.contains("YYYYY-\n     Z     BODY"), "{native:?}");
    assert!(lowered.contains("YYYYY-Z BODY"), "{lowered:?}");
    for (field, expected) in [
        (r"YYY\:Z", "YYYZBODY"),
        (r"YYY\pZ", "YYYZBODY"),
        (r"A BBBBBB\zC", "BBBBBBCBODY"),
    ] {
        // Each exact input passed fixed CVS -Tascii/-Tutf8/-Tlint. In
        // term_fill(), a short field with an unused breakpoint still fits;
        // encode1() keeps the pending \z glyph in the current final word.
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No {field}\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        assert!(native.contains(expected), "{field}: {native:?}");
        assert!(lowered.contains(expected), "{field}: {lowered:?}");
    }
}

#[test]
fn a_control_only_hang_field_does_not_close_its_device_row() {
    // All six exact inputs passed fixed CVS -Tutf8/-Tlint. In term.c,
    // term_word() buffers \p and the following empty word's separator, but
    // term_fill() returns nbr=0 for this field. term_flushln() under
    // TERMP_HANG therefore leaves X and BODY on the same physical row.
    for word in [".No \"\"", ".No \"\" \"\"", r".No \&"] {
        for request in [".br", ".sp 0"] {
            let source = format!(
                ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo\n.No X\n.br\n.No \\p\n{word}\n{request}\n.Xc\n.No BODY\n.El\n"
            );
            let native = without_line_indentation(&native_terminal(&source));
            let lowered = lowered_terminal(&source);
            assert!(
                native.contains("X     BODY"),
                "{word} {request}: {native:?}"
            );
            assert!(
                lowered.contains("X     BODY"),
                "{word} {request}: {lowered:?}"
            );
        }
    }
    // Both exact inputs also passed fixed CVS -Tascii/-Tutf8/-Tlint. With
    // no graph before \p, the next automatic separator makes term_fill()
    // return nbr=0 and drop that field, including a later Y operand.
    for words in [".No Y", ".No \"\"\n.No Y"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo\n.No X\n.br\n.No \\p\n{words}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        assert!(native.contains("X     BODY"), "{words}: {native:?}");
        assert!(lowered.contains("X     BODY"), "{words}: {lowered:?}");
    }
}
