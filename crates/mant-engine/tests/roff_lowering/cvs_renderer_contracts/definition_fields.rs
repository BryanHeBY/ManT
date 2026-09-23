use super::*;

#[test]
fn no_break_flush_releases_a_word_boundary_after_fixed_run_in_cells() {
    for (style, spaces) in [("inset", 2), ("diag", 3)] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It A\n.mc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let expected = format!("A{}BODY", " ".repeat(spaces));
        let normalized_native = native.replace('\u{a0}', " ");
        assert!(
            normalized_native.contains(&expected),
            "native {style} gap: {native:?}"
        );

        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower run-in margin");
        let document = query.document.as_ref().unwrap();
        let item = first_definition_item(document);
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => {
                    Some(super::inline_text(document.content(), children))
                }
                _ => None,
            })
            .expect("run-in description");
        assert_eq!(description, format!("{}BODY", " ".repeat(spaces)));
    }
}

#[test]
fn no_break_field_separator_survives_spacing_modes() {
    for (style, spaces) in [("inset", 2), ("diag", 3)] {
        for spacing in ["", ".Sm off\n"] {
            let source = format!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It A\n{spacing}.mc\n.No BODY\n.El\n"
            );
            let native = native_terminal(&source).replace('\u{a0}', " ");
            let expected = format!("A{}BODY", " ".repeat(spaces));
            assert!(native.contains(&expected), "native {style}: {native:?}");
            let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
            assert!(
                lowered.contains(&expected),
                "lowered {style} {spacing:?}: {lowered:?}"
            );
        }
    }
}

#[test]
fn no_break_flush_trims_word_padding_but_keeps_its_field_separator() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.No A \"\"\n.mc\n.No BODY\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("A BODY"), "native: {native:?}");
    assert!(!native.contains("A  BODY"), "native: {native:?}");

    let lowered = lowered_terminal(source);
    assert!(lowered.contains("A BODY"), "lowered: {lowered:?}");
    assert!(!lowered.contains("A  BODY"), "lowered: {lowered:?}");
}

#[test]
fn no_break_field_separator_survives_transparent_and_tight_nodes() {
    let cases = [
        (".Tg mark\n.No BODY", "A BODY"),
        (".Ns\n.No BODY", "A BODY"),
        (".No )", "A )"),
    ];
    for (tail, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.mc\n{tail}\n"
        );
        let native = native_terminal(&source);
        assert!(native.contains(expected), "native {tail:?}: {native:?}");
        let lowered = lowered_terminal(&source);
        assert!(lowered.contains(expected), "lowered {tail:?}: {lowered:?}");
    }
}

#[test]
fn no_break_flush_keeps_nonbreaking_formatter_cells_distinct_from_padding() {
    for escape in [r"\~", r"\0"] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No {escape}\n.mc\n.No BODY\n"
        );
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(native.contains("  BODY"), "native {escape}: {native:?}");
        let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
        assert!(lowered.contains("  BODY"), "lowered {escape}: {lowered:?}");
    }
}

#[test]
fn no_break_field_preserves_formatter_word_order_for_empty_fixed_and_zero_width_words() {
    let cases = [
        ("empty", ".No \"\"", "A  BODY"),
        ("empty-spacing-off", ".Sm off\n.No \"\"", "A BODY"),
        ("empty-tight", ".Ns\n.No \"\"", "A  BODY"),
        ("nonbreaking-space", r".No \~", "A   BODY"),
        ("fixed-width-space", r".No \0", "A   BODY"),
        ("zero-advance", r".No \zX", "A XBODY"),
    ];
    for (label, middle, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No A\n.mc\n{middle}\n.No BODY\n"
        );
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(native.contains(expected), "native {label}: {native:?}");
        let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
        assert!(lowered.contains(expected), "lowered {label}: {lowered:?}");
    }
}

#[test]
fn invisible_formatter_fields_still_own_their_empty_word_boundary() {
    for (label, first) in [
        ("zero-width", r"\&"),
        ("word-end-break", r"\p"),
        ("zero-advance-break", r"\z\p"),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.No {first}\n.mc\n.No \"\"\n.No BODY\n"
        );
        let native = native_terminal(&source);
        assert!(
            native.contains("\n       BODY"),
            "native {label}: {native:?}"
        );

        let query = mant_loader::load_roff_bytes(source.as_bytes())
            .expect("lower invisible no-break field");
        let document = query.document.as_ref().expect("lowered document");
        let paragraph = document.flow().unwrap().sections[1]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => {
                    Some(super::inline_text(document.content(), children))
                }
                _ => None,
            })
            .expect("description paragraph");
        assert_eq!(paragraph, "  BODY", "lowered {label}");
    }
}

#[test]
fn control_only_word_end_break_drops_a_trailing_no_break_field_separator() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.No A\n.mc\n.No \\p\n.No BODY\n",
    );

    // Pinned CVS `term_field()` correctly drops the separator because the
    // control-only field has no printable cell.  Its current renderer then
    // also loses BODY at this edge; ManT deliberately follows groff's
    // content-preserving result while retaining CVS's no-trailing-blank
    // field contract.
    let native = native_terminal(source);
    assert!(native.contains("\n     A\n"), "native: {native:?}");
    assert!(!native.contains("\n     A \n"), "native: {native:?}");

    let lowered = lowered_terminal(source);
    assert!(lowered.contains("\nA\nBODY"), "lowered: {lowered:?}");
    assert!(!lowered.contains("\nA \n"), "lowered: {lowered:?}");
}

#[test]
fn run_in_no_break_flush_preserves_fixed_and_pending_cells() {
    for (style, head, spaces) in [
        ("inset", "", 0),
        ("diag", "", 3),
        ("inset", r"A\z", 2),
        ("diag", r"A\z", 2),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n.mc\n.No BODY\n.El\n"
        );
        let expected = format!("{}BODY", " ".repeat(spaces));
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(
            native.contains(&expected),
            "native {style} {head:?}: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower run-in cells");
        let document = query.document.as_ref().unwrap();
        let item = first_definition_item(document);
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => {
                    Some(super::inline_text(document.content(), children))
                }
                _ => None,
            })
            .expect("run-in description");
        assert_eq!(description, expected, "lowered {style} {head:?}: {item:?}");
    }
}

#[test]
fn repeated_authored_section_titles_remain_ambiguous() {
    // CVS HTML resolves this to the first duplicate fragment.  ManT's stricter
    // navigation contract deliberately refuses to choose between two authored
    // destinations, while retaining both sections under unique stable IDs.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Sx DETAILS\n",
        ".Sh DETAILS\n.No ONE\n.Sh DETAILS\n.No TWO\n",
    );
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower duplicate headings");
    let document = query.document.as_ref().expect("lowered document");
    assert!(
        document
            .flow()
            .unwrap()
            .sections
            .iter()
            .any(|section| section.id == "details")
    );
    assert!(
        document
            .flow()
            .unwrap()
            .sections
            .iter()
            .any(|section| section.id == "details-2")
    );

    for id in ["details", "details-2"] {
        let mut link = AuthoredSectionLink {
            id,
            found: false,
            content: document.content(),
        };
        link.visit_document(document);
        assert!(!link.found, "ambiguous authored title resolved to {id}");
    }
    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        })
    );
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
fn font_stack_divergence_is_pinned_in_native_html_and_lowered_ir() {
    // CVS term.c::term_fontlast()/term_fontpop() retain the previous explicit
    // selection across this mdoc scope pop.  GNU groff differs; ManT selects
    // the pinned CVS behavior, so both layers must keep TAIL bold.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh DESCRIPTION\n.No \\fBWORD\\fIINNER\n\\fPTAIL\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("font-stack.1", source.as_bytes())
        .expect("render the native font-stack contract")
        .output;
    assert!(native.contains("<b>TAIL</b>"), "native HTML: {native}");

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower the font-stack case");
    let mut strong_tail = StrongText(false, query.document.as_ref().unwrap().content());
    strong_tail.visit_document(query.document.as_ref().expect("lowered document"));
    assert!(strong_tail.0, "lowered IR did not retain bold TAIL");
}

#[test]
#[allow(clippy::too_many_lines)] // This table is one pinned native field ledger.
fn definition_head_controls_settle_the_same_native_field() {
    // Verified against the pinned CVS `termp_it_pre/post()`,
    // `roff_term_pre_br/sp/ti()`, and `term_flushln()`.  NOBREAK, BRIND,
    // HANG, trailspace, and the body origin form one formatter field; none of
    // these requests may be lowered as an unrelated paragraph break.
    let cases = [
        (
            "tag br overrun",
            "tag",
            "6n",
            ".br",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n        A\nBob\n        BODY",
        ),
        (
            "tag br fit",
            "tag",
            "12n",
            ".br",
            "LONGTEXT      A\nBob\nBODY",
            "LONGTEXT      A\nBob\n              BODY",
        ),
        (
            "tag temporary indent",
            "tag",
            "6n",
            ".ti 2n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\nA\nBob\n        BODY",
        ),
        (
            "tag temporary indent fit",
            "tag",
            "12n",
            ".ti 2n",
            "LONGTEXT  A\nBob\nBODY",
            "LONGTEXT  A\nBob\n              BODY",
        ),
        (
            "tag vertical space",
            "tag",
            "6n",
            ".sp 1",
            "LONGTEXT\n\nA\nBob\nBODY",
            "LONGTEXT\n\n        A\nBob\n        BODY",
        ),
        (
            "tag fill boundary",
            "tag",
            "6n",
            ".nf",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n        A\nBob\n        BODY",
        ),
        (
            "tag fill boundary fit",
            "tag",
            "12n",
            ".nf",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag vertical space fit",
            "tag",
            "12n",
            ".sp 1",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "hang br overrun",
            "hang",
            "6n",
            ".br",
            "LONGTEXT ABobBODY",
            "LONGTEXT ABobBODY",
        ),
        (
            "hang temporary indent overrun",
            "hang",
            "6n",
            ".ti 2n",
            "LONGTEXT ABobBODY",
            "LONGTEXT ABobBODY",
        ),
        (
            "hang vertical space closes its occupied row",
            "hang",
            "6n",
            ".sp 1",
            "LONGTEXT\nABobBODY",
            "LONGTEXT\n        ABobBODY",
        ),
        (
            "hang br fit",
            "hang",
            "12n",
            ".br",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang temporary indent",
            "hang",
            "12n",
            ".ti 2n",
            "LONGTEXT ABob BODY",
            "LONGTEXT ABob BODY",
        ),
        (
            "hang vertical space fit",
            "hang",
            "12n",
            ".sp 1",
            "LONGTEXT\nABobBODY",
            "LONGTEXT\n              ABobBODY",
        ),
        (
            "hang fill boundary overrun",
            "hang",
            "6n",
            ".nf",
            "LONGTEXTABob\nBODY",
            "LONGTEXTABob\n        BODY",
        ),
        (
            "hang fill boundary fit",
            "hang",
            "12n",
            ".nf",
            "LONGTEXT      ABob\nBODY",
            "LONGTEXT      ABob\n              BODY",
        ),
    ];

    for (label, style, width, request, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width {width}\n.It Xo\n.No LONGTEXT\n{request}\n.No A\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
fn temporary_indent_keeps_the_field_boundary_without_printing_its_operand() {
    // Verified against pinned CVS roff_term.c::roff_term_pre_ti(): every
    // spelling first executes roff_term_pre_br(), while valid signed and
    // unsigned operands only update p->ti/tcol->offset. ManT intentionally
    // omits that device position, but must retain the tag field's two cells.
    for (label, operand) in [
        ("absolute", " 10n"),
        ("relative-positive", " +10n"),
        ("relative-negative", " -2n"),
        ("invalid", " bogus"),
        ("missing", ""),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -tag -width 8n\n.It Xo\n.No A\n.ti{operand}\n.No B\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        assert!(native.contains("A  B"), "native {label}: {native:?}");

        let lowered = lowered_terminal(&source);
        assert!(lowered.contains("A  B"), "lowered {label}: {lowered:?}");
        assert!(
            !lowered.contains("A          B"),
            "temporary indent leaked as text for {label}: {lowered:?}"
        );
    }
}

#[test]
fn repeated_margin_flushes_keep_one_definition_field_lifecycle() {
    // Verified against pinned CVS roff_term_pre_mc() and term_flushln(). Each
    // occupied field is committed under NOBREAK, while BRIND/HANG and list
    // geometry remain live. The second request therefore retains the same
    // three-cell tag separator instead of degrading to an ordinary blank.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.Bl -tag -width 8n\n.It Xo\n.No A\n.mc\n.No C\n.mc\n",
        ".No D\n.Xc\n.No BODY\n.El\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("A   C   D"), "native: {native:?}");
    let lowered = lowered_terminal(source);
    assert!(lowered.contains("A   C   D"), "lowered: {lowered:?}");

    // A following vertical request closes the retained field. Its temporary
    // BRIND geometry is local to the request in the native mdoc traversal;
    // later head content resumes at the list origin rather than the body
    // column. The source-neutral IR keeps that ownership while omitting the
    // right-margin decoration itself.
    let with_space = source.replace(".No D\n", "\\&\n.sp 1\n.No D\n");
    let native = native_terminal(&with_space);
    assert!(native.contains("\n     D\n"), "native space: {native:?}");
    assert!(
        !native.contains("\n               D\n"),
        "native D moved to body column: {native:?}"
    );
    let lowered = lowered_terminal(&with_space);
    assert!(lowered.contains("\nD\n"), "lowered space: {lowered:?}");
    assert!(
        !lowered.contains("\n          D\n"),
        "lowered D inherited stale body geometry: {lowered:?}"
    );
}

#[test]
fn repeated_margin_flushes_recompute_the_remaining_field_geometry() {
    // Pinned CVS `term_flushln()` recomputes `vfield` from the current
    // `viscol` for every `.mc`; neither the first overrun decision nor the
    // first separator can be reused by a later field.  These expected rows
    // were also checked with the pinned reference binary and `-Tlint`.
    let cases = [
        (
            "narrow field",
            "4n",
            "A",
            "C",
            "\n     A   C\n      D\n",
            "\nA   C\n D\n",
        ),
        (
            "ordinary field",
            "8n",
            "A",
            "C",
            "\n     A   C   D\n",
            "\nA   C   D\n",
        ),
        (
            "wide field",
            "12n",
            "A",
            "C",
            "\n     A   C   D\n",
            "\nA   C   D\n",
        ),
        (
            "first field overrun",
            "6n",
            "LONGTEXT",
            "C",
            "\n     LONGTEXT\n      C   D\n",
            "\nLONGTEXT\n C   D\n",
        ),
        (
            "later field overrun",
            "8n",
            "A",
            "CDEFG",
            "\n     A   CDEFG\n      D\n",
            "\nA   CDEFG\n D\n",
        ),
    ];

    for (label, width, first, middle, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -tag -width {width}\n.It Xo\n.No {first}\n.mc\n.No {middle}\n.mc\n.No D\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        assert!(
            native.contains(native_expected),
            "native {label}: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "lowered {label}: {lowered:?}"
        );
    }
}

#[test]
fn repeated_margin_flushes_distinguish_empty_and_fixed_width_cells() {
    // `term_fill()` commits fixed/non-breaking glyphs but no bytes for an
    // empty word, `\&`, or a still-armed bare `\z`.  All of them nevertheless
    // execute the request, so the next real field retains exactly one pending
    // separator.  Assert native and lowering together because the distinction
    // is lost once U+00A0 is normalized for terminal comparison.
    let cases = [
        ("empty", r#""""#, "A   D"),
        ("zero-width", r"\&", "A   D"),
        ("armed-zero-advance", r"\z", "A   D"),
        ("zero-advance-glyph", r"\zX", "A   X   D"),
        ("nonbreaking-space", r"\~", "A       D"),
        ("fixed-width-space", r"\0", "A       D"),
    ];

    for (label, middle, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -tag -width 12n\n.It Xo\n.No A\n.mc\n.No {middle}\n.mc\n.No D\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source).replace('\u{a0}', " ");
        assert!(native.contains(expected), "native {label}: {native:?}");
        let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
        assert!(lowered.contains(expected), "lowered {label}: {lowered:?}");
    }
}

#[test]
fn no_break_field_retains_brind_and_hang_for_following_controls() {
    // Verified against CVS `roff_term_pre_mc()`: only NOBREAK and NOSPACE
    // are cleared after the flush. BRIND/HANG and the field geometry remain
    // live for a following request, even though `.mc` itself emitted no text.
    let cases = [
        (
            "tag/br",
            "tag",
            ".br",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/ti",
            "tag",
            ".ti 4n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\nA\nBob\n              BODY",
        ),
        (
            "tag/sp",
            "tag",
            ".sp 1",
            "LONGTEXT\n\nA\nBob\nBODY",
            "LONGTEXT\n\nA\nBob\n              BODY",
        ),
        (
            "tag/nf",
            "tag",
            ".nf",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "hang/br",
            "hang",
            ".br",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang/ti",
            "hang",
            ".ti 4n",
            "LONGTEXT ABob BODY",
            "LONGTEXT ABob BODY",
        ),
        (
            "hang/sp",
            "hang",
            ".sp 1",
            "LONGTEXT\nABobBODY",
            "LONGTEXT\n              ABobBODY",
        ),
        (
            "hang/nf",
            "hang",
            ".nf",
            "LONGTEXT      ABob\nBODY",
            "LONGTEXT      ABob\n              BODY",
        ),
    ];

    for (label, style, request, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width 12n\n.It Xo\n.No LONGTEXT\n.mc\n{request}\n.No A\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
fn no_break_field_classifies_following_words_before_a_control() {
    // Verified against CVS `term_word()`, `term_fill()`, and
    // `roff_term_pre_br()`. Empty/NBRZW words do not advance the visual
    // field, while a visible word does; BRIND and HANG remain independent of
    // both facts after `.mc` clears NOBREAK and NOSPACE.
    let cases = [
        (
            "tag/empty",
            "tag",
            r#""""#,
            ".No A\n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/ignore",
            "tag",
            r"\&",
            ".No A\n",
            "LONGTEXT\nA\nBob\nBODY",
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/visible",
            "tag",
            "A",
            "",
            "LONGTEXT   A\nBob\nBODY",
            "LONGTEXT   A\nBob\n              BODY",
        ),
        (
            "hang/empty",
            "hang",
            r#""""#,
            ".No A\n",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang/ignore",
            "hang",
            r"\&",
            ".No A\n",
            "LONGTEXT      ABobBODY",
            "LONGTEXT      ABobBODY",
        ),
        (
            "hang/visible",
            "hang",
            "A",
            "",
            "LONGTEXT  ABobBODY",
            "LONGTEXT  ABobBODY",
        ),
        (
            "tag/empty before control-only author",
            "tag",
            r#""""#,
            "",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "tag/ignore before control-only author",
            "tag",
            r"\&",
            "",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "hang/empty before control-only author",
            "hang",
            r#""""#,
            "",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang/ignore before control-only author",
            "hang",
            r"\&",
            "",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
    ];

    for (label, style, operand, after_control, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width 12n\n.It Xo\n.No LONGTEXT\n.mc\n.No {operand}\n.br\n{after_control}.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}

#[test]
#[allow(clippy::too_many_lines)] // This table is one pinned native field ledger.
fn control_only_author_handoffs_settle_buffer_and_device_rows_separately() {
    // Verified first with the pinned CVS renderer.  An empty word has no
    // native field cell at a tight list-head boundary, `\&` does have one,
    // and a bare `\z` only arms BACKAFTER.  After `.mc`, `viscol` keeps the
    // device row occupied even when the current field buffer is empty, so a
    // later control must flush and clear BACKAFTER before Bob executes.
    let cases = [
        (
            "tag device row, empty before ti",
            "tag",
            ".No LONGTEXT\n.mc\n",
            r#""""#,
            ".ti 4n",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "tag device row, zero-width cell before ti",
            "tag",
            ".No LONGTEXT\n.mc\n",
            r"\&",
            ".ti 4n",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "tag device row, bare zero before ti",
            "tag",
            ".No LONGTEXT\n.mc\n",
            r"\z",
            ".ti 4n",
            "LONGTEXT\nBob\nBODY",
            "LONGTEXT\nBob\n              BODY",
        ),
        (
            "hang device row, empty before ti",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r#""""#,
            ".ti 4n",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang device row, zero-width cell before ti",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r"\&",
            ".ti 4n",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang device row, bare zero before ti",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r"\z",
            ".ti 4n",
            "LONGTEXTBob   BODY",
            "LONGTEXTBob   BODY",
        ),
        (
            "hang device row, bare zero before fill boundary",
            "hang",
            ".No LONGTEXT\n.mc\n",
            r"\z",
            ".nf",
            "LONGTEXTBob\nBODY",
            "LONGTEXTBob\n              BODY",
        ),
        (
            "tag empty before ti",
            "tag",
            "",
            r#""""#,
            ".ti 4n",
            "Bob\nBODY",
            "Bob\n              BODY",
        ),
        (
            "tag zero-width cell before ti",
            "tag",
            "",
            r"\&",
            ".ti 4n",
            "Bob\nBODY",
            "Bob\n              BODY",
        ),
        (
            "tag bare zero before br",
            "tag",
            "",
            r"\z",
            ".br",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "tag bare zero before ti",
            "tag",
            "",
            r"\z",
            ".ti 4n",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "tag bare zero before vertical space",
            "tag",
            "",
            r"\z",
            ".sp 1",
            "\nob\nBODY",
            "\nob\n              BODY",
        ),
        (
            "tag bare zero before fill boundary",
            "tag",
            "",
            r"\z",
            ".nf",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "hang zero-width cell before ti",
            "hang",
            "",
            r"\&",
            ".ti 4n",
            "Bob           BODY",
            "Bob           BODY",
        ),
        (
            "hang zero-width cell before fill boundary",
            "hang",
            "",
            r"\&",
            ".nf",
            "Bob\nBODY",
            "Bob\n              BODY",
        ),
        (
            "hang bare zero before br",
            "hang",
            "",
            r"\z",
            ".br",
            "ob            BODY",
            "ob            BODY",
        ),
        (
            "hang bare zero before ti",
            "hang",
            "",
            r"\z",
            ".ti 4n",
            "ob            BODY",
            "ob            BODY",
        ),
        (
            "hang bare zero before vertical space",
            "hang",
            "",
            r"\z",
            ".sp 1",
            "\nob            BODY",
            "\nob            BODY",
        ),
        (
            "hang bare zero before fill boundary",
            "hang",
            "",
            r"\z",
            ".nf",
            "ob\nBODY",
            "ob\n              BODY",
        ),
        (
            "tag empty before explicit break and visible word",
            "tag",
            "",
            r#""""#,
            ".br\n.No A",
            "A\nBob\nBODY",
            "              A\nBob\n              BODY",
        ),
        (
            "tag bare zero before explicit break and visible word",
            "tag",
            "",
            r"\z",
            ".br\n.No A",
            "A\nBob\nBODY",
            "              A\nBob\n              BODY",
        ),
        (
            "hang empty before explicit break and visible word",
            "hang",
            "",
            r#""""#,
            ".br\n.No A",
            "ABobBODY",
            "              ABobBODY",
        ),
        (
            "hang bare zero before explicit break and visible word",
            "hang",
            "",
            r"\z",
            ".br\n.No A",
            "ABobBODY",
            "              ABobBODY",
        ),
    ];

    for (label, style, prefix, operand, control, native_expected, lowered_expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style} -width 12n\n.It Xo\n{prefix}.No {operand}\n{control}\n.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(
            native.contains(native_expected),
            "{label} native output: {native:?}"
        );
        let lowered = lowered_terminal(&source);
        assert!(
            lowered.contains(lowered_expected),
            "{label} lowered output: {lowered:?}"
        );
    }
}
