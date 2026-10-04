use super::*;

#[test]
fn no_break_flush_retires_its_native_cells_without_ending_the_device_row() {
    // Six exact sources ran pristine UTF-8/ASCII/tree/lint before these
    // assertions. roff_term_pre_mc() holds NOBREAK through term_flushln();
    // term.c:233-237 still destroys the consumed buffer while 250-253 keeps
    // this row open. A NBRZW/\p marker cannot execute again in the next word.
    for (operand, controls, expected) in [
        (r"A\p\c", "", "A B C"),
        (r"\&\p\c", "", " B C"),
        (r"\zX\p\c", "", "X B C"),
        (r"\&\p\c", ".mc !\n", " B C"),
        (r"\&\p\c", ".Ns\n", " B C"),
        (r"\&\p\c", ".Sm off\n", " BC"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No {operand}\n.mc |\n{controls}.No B C\n.Ed\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("no-break-native-buffer-retirement.1"),
            source.as_bytes(),
        )
        .unwrap();
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{source}: unexpected blocks {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{source}: {children:?}");
    }
}

#[test]
fn hang_field_flush_preserves_native_trailspace_without_reusing_br_gap() {
    // Both exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint. term.c's
    // term_flushln() restores minbl=trailspace even when nbr=0; explicit
    // roff_term_pre_br() clears BRIND, while NODE_LINE sets NOSPACE.
    for (middle, expected) in [(".No \\p", "X Y"), (".br\n.No \"\"", "XY")] {
        let item = definition_item_from_source(&format!(
            ".nf\n.Bl -hang -width 4n\n.It Xo\n.No X\n{middle}\n.No Y\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{middle}: {item:#?}");
        if middle == ".No \\p" {
            assert!(
                !item
                    .entry
                    .as_ref()
                    .is_some_and(|entry| entry.names.iter().any(|name| name == "XY")),
                "a lost native separator changed entry identity: {item:#?}"
            );
        }
    }
}

#[test]
fn hang_field_rejects_only_the_unaccepted_word_end_suffix() {
    // Exact single-TEXT and cross-TEXT cases passed fixed CVS
    // -Tascii/-Tutf8/-Tlint. term.c::term_fill() restarts from the remaining
    // buffer after every accepted prefix, resetting graph for each pass.
    for (head, expected) in [
        (".No \"X\\p Y\"\n.No \"\\p Z\"", "X\nY"),
        (".No X\\p\n.No \\p\n.No \"\"\n.No Z", "X"),
        (".No X\n.No \\p\n.No Y", "X"),
        // Filled NODE_LINE buffers TABREF (mdoc_term.c:321,
        // term.c:873-878); it stops the break-blank sweep at term.c:205.
        // The next word's separator remains at the restarted row origin.
        (".No X\\p\n.No \"\"\n.No Y", "X\n Y"),
    ] {
        let item = definition_item_from_source(&format!(
            ".Bl -hang -width 4n\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{head}: {item:#?}");
        assert!(!inline_text(&item.terms[0]).contains('Z'));
    }
    // The same term_fill() consumption runs for OHANG even though its HEAD
    // does not use HANG geometry. The pinned reference drops Y here too.
    let item = definition_item_from_source(
        ".Bl -ohang\n.It Xo\n.No X\n.No \\p\n.No Y\n.Xc\n.No BODY\n.El\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "X\n", "{item:#?}");
}

#[test]
fn hang_field_restarts_term_fill_from_the_actual_breakable_blank() {
    // Exact sources passed fixed CVS -Tascii/-Tutf8/-Tlint. term.c::term_fill()
    // records nbr at the first ordinary space, then restarts from that space
    // after \p. With "X \p Y", the next pass has no graph and rejects Y/Z;
    // with "X\p Y", it accepts Y in the next pass.
    for (first, expected, rejected) in [("X \\p Y", "X", true), ("X\\p Y", "X\nY Z", false)] {
        let item = definition_item_from_source(&format!(
            ".Bl -hang -width 4n\n.It Xo\n.No \"{first}\"\n.No Z\n.Xc\n.No BODY\n.El\n"
        ));
        assert_eq!(inline_text(&item.terms[0]), expected, "{first}: {item:#?}");
        assert_eq!(inline_text(&item.terms[0]).contains('Z'), !rejected);
    }
}

#[test]
fn tag_margin_flush_does_not_print_an_unconsumed_next_field_separator() {
    // This exact source passed fixed CVS -Tascii/-Tutf8/-Tlint. In
    // roff_term.c::roff_term_pre_mc(), TERMP_NOBREAK flushes the TAG field;
    // mdoc_term.c::termp_it_post() then closes HEAD. The ordinary separator
    // for a *later* term_word() has not occupied a second physical row.
    let item = definition_item_from_source(
        ".nf\n.Bl -tag -width 4n\n.It Xo\n.No QHEADQ\\c\n.mc |\n.Xc\n.No QBODYQ\n.El\n.fi\n",
    );
    assert_eq!(inline_text(&item.terms[0]), "QHEADQ", "{item:#?}");
    assert!(!item.inline_term(), "{item:#?}");
    assert!(!matches!(
        item.description.first(),
        Some(Block::VerticalSpace { .. })
    ));
}

#[test]
fn margin_control_flushes_rejected_native_field_before_starting_another() {
    // These exact HANG/TAG sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // roff_term.c::roff_term_pre_mc() calls term_flushln() when p->col is
    // occupied; term.c::term_fill() can reject the suffix after \\p there,
    // before `.mc` changes NOBREAK and prepares the following field.
    for style in ["hang", "tag"] {
        let item = definition_item_from_source(&format!(
            ".Bl -{style} -width 4n\n.It Xo\n.No \"QAA \\p QBB\"\n.mc |\n.Xc\n.No QBODYQ\n.El\n"
        ));
        assert_eq!(
            inline_text(&item.terms[0]).trim_end(),
            "QAA",
            "{style}: {item:#?}"
        );
    }
    let empty = definition_item_from_source(
        ".Bl -hang -width 4n\n.It Xo\n.No \"\\p QAA\"\n.mc |\n.Xc\n.No QBODYQ\n.El\n",
    );
    assert!(!inline_text(&empty.terms[0]).contains("QAA"), "{empty:#?}");
}

#[test]
fn every_post_margin_field_flush_rejects_the_pending_suffix() {
    // Exact br/sp0/ce0/rj0 sources passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // roff_term.c::pre_br() calls term_newln(), which reaches term_fill()
    // even after a prior `.mc` opened another NOBREAK field. The accepted
    // QBB prefix survives; the later QCC suffix is never printed.
    for request in [".br", ".sp 0", ".ce 0", ".rj 0"] {
        let item = definition_item_from_source(&format!(
            ".Bl -hang -width 4n\n.It Xo\n.No QAA\n.mc |\n.No \"QBB \\p QCC\"\n{request}\n.Xc\n.No QBODYQ\n.El\n"
        ));
        let term = inline_text(&item.terms[0]);
        assert!(
            term.contains("QAA") && term.contains("QBB"),
            "{request}: {item:#?}"
        );
        assert!(!term.contains("QCC"), "{request}: {item:#?}");
    }
}

#[test]
fn final_hang_field_reestablishes_body_word_gap() {
    // Exact br/sp 0/sp 1 forms checked with fixed CVS -Tascii/-Tlint.
    // term_flushln() settles each HANG field independently; an earlier
    // completed field cannot consume the final Y/BODY word separator.
    for request in [".br", ".sp 0", ".sp 1"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width Ds\n.It Xo X\n{request}\n.No Y\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("hang-final-field-gap.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{request}: {document:#?}");
        };
        let item = &items[0];
        assert!(inline_text(&item.terms[0]).ends_with('Y'), "{item:#?}");
        assert!(item.inline_term(), "{request}: {item:#?}");
        assert_eq!(item.layout.min_term_gap_columns, 1, "{item:#?}");
        assert!(
            matches!(&item.description[0], Block::Paragraph { children, .. } if inline_text(children) == "BODY"),
            "{request}: {item:#?}"
        );
    }
}

#[test]
fn final_hang_field_only_consumes_a_proven_body_gap() {
    // All exact fields passed fixed CVS -Tutf8 (the `YYYYY\:Z` entry sits on
    // the device fork: ascii wraps at ASCII_BREAK and proves gap 1, chars.c:53).
    // term.c::term_fill() may break a field at ordinary spaces after .br;
    // cumulative field width is not the final native row column. An
    // indivisible last word spanning the BODY origin still proves no gap.
    for (words, expected_gap) in [
        ("AA BB CC", 1),
        ("YYYYY Z", 1),
        ("YYYYY ZZZZZZZZ", 0),
        // Fixed CVS -Tutf8: `\:` is an unbreakable zero-width graph here
        // (chars.c:53), so the operand never wraps and BODY concatenates
        // on the head row: no gap is proven.
        (r"YYYYY\:Z", 0),
        (r"YYY\:Z", 0),
        (r"YYY\pZ", 0),
        (r"A BBBBBB\zC", 0),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No {words}\n.Xc\n.No BODY\n.El\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("hang-final-physical-field.1"),
            source.as_bytes(),
        )
        .unwrap();
        let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
            panic!("{words}: {document:#?}");
        };
        assert_eq!(
            items[0].layout.min_term_gap_columns, expected_gap,
            "{items:#?}"
        );
    }
}

#[test]
fn filled_margin_flush_settles_the_current_cell_without_a_hard_break() {
    // In filled mode CVS roff_term_pre_mc() flushes with TERMP_NOBREAK.
    // A trailing `\p` is settled with that cell instead of becoming a hard
    // line; literal mode above has already closed its physical source row.
    for (label, keep_open, keep_close, first, expected) in [
        ("filled-word-break", "", "", r".No A\p", "A B C"),
        (
            "filled-zero-width-word-break",
            "",
            "",
            r".No A\zX\p",
            "AX B C",
        ),
        (
            "filled-canceled-arm-word-break",
            "",
            "",
            r".No A\z\p\c",
            "A B C",
        ),
        (
            "kept-word-break",
            ".Bk -words\n",
            ".Ek\n",
            r".No A\p",
            "A B C",
        ),
        ("generated-word-break", "", "", r".Bx \p", "BSD B C"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{keep_open}{first}\n.mc |\n.No B C\n{keep_close}"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse filled no-break margin fixture");
        let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}

#[test]
fn margin_flush_obeys_current_cell_and_continuation_state() {
    for (label, header, opening, first, request, closing, expected) in [
        (
            "mdoc-column",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".No A",
            ".mc |",
            ".Ed",
            "A\nB",
        ),
        (
            "mdoc-zero-column",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION",
            ".Bd -literal",
            ".No \\zX",
            ".mc |",
            ".Ed",
            "X\nB",
        ),
        (
            "man-column",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            "A",
            ".mc |",
            ".fi",
            "A\nB",
        ),
        (
            "man-zero-column",
            ".TH PROBE 1\n.SH DESCRIPTION",
            ".nf",
            r"\zX",
            ".mc |",
            ".fi",
            "X\nB",
        ),
    ] {
        let manual = format!("{header}\n{opening}\n{first}\n{request}\nB\n{closing}\n");
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-control-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse conditional margin flush fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }

    let document = parse_manual_bytes(
        std::path::Path::new("inline-nested-vertical-space.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Eo [\n.No A\n.sp 2\n.No B\n.Ec\n",
    )
    .expect("parse nested vertical-space fixture");
    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "unexpected nested vertical-space blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(children), "[A\n\n\nB", "{children:?}");

    let document = parse_manual_bytes(
        std::path::Path::new("inline-repeated-margin-flush.1"),
        b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No A\\c\n.mc |\n.Tg between-margins\n.mc !\n.No B\n.Ed\n",
    )
    .expect("parse repeated margin flush fixture");
    let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!(
            "unexpected repeated-margin blocks: {:#?}",
            document.sections
        );
    };
    assert_eq!(inline_text(children), "A B", "{children:?}");
    assert!(children.iter().any(|node| matches!(
        node,
        Inline::Anchor { id, .. } if id == "between-margins"
    )));

    for (label, first, expected) in [
        // With `\c`, CVS retains the occupied zero-width field until `.mc`;
        // its NOBREAK flush commits one relative separator before the next
        // source word even though the field has no visible glyph.
        ("word-break", r"\p\c", " B C"),
        ("zero-width-word-break", r"\&\p\c", " B C"),
        ("uncontinued-word-break", r"\p", "\nB C"),
        ("armed-word-break", r"\z\p", "\nB C"),
        ("canceled-arm-word-break", r"\z\p\c", "\nB C"),
    ] {
        let manual = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal\n.No {first}\n.mc |\n.No B C\n.Ed\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new(&format!("inline-margin-{label}.1")),
            manual.as_bytes(),
        )
        .expect("parse pending formatter-cell margin fixture");
        let [Block::Preformatted { children, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("{label}: unexpected blocks: {:#?}", document.sections);
        };
        assert_eq!(inline_text(children), expected, "{label}: {children:?}");
    }
}
