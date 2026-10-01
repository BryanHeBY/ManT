use super::*;

type HeadControlCase = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

const HEAD_CONTROL_CASES: [HeadControlCase; 16] = [
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

#[test]
fn definition_head_controls_settle_the_same_native_field() {
    // Verified against the pinned CVS `termp_it_pre/post()`,
    // `roff_term_pre_br/sp/ti()`, and `term_flushln()`.  NOBREAK, BRIND,
    // HANG, trailspace, and the body origin form one formatter field; none of
    // these requests may be lowered as an unrelated paragraph break.

    for (label, style, width, request, native_expected, lowered_expected) in HEAD_CONTROL_CASES {
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
