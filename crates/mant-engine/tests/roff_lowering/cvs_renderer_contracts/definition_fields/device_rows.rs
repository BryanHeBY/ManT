use super::*;

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
            // ti shares pre_br; its numerical temporary device origin is
            // intentionally omitted, so A keeps the declared BODY origin.
            "LONGTEXT\n              A\nBob\n              BODY",
        ),
        (
            "tag/sp",
            "tag",
            ".sp 1",
            "LONGTEXT\n\nA\nBob\nBODY",
            // pre_sp's trailing pre_br leaves the native body origin live
            // until A is flushed by An (roff_term.c:195-214,69-78).
            "LONGTEXT\n\n              A\nBob\n              BODY",
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
            // Native temporary positioning differs here. Reading executes
            // the same pre_br as .br without a second width-based exit.
            "LONGTEXT      ABobBODY",
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

type AuthorHandoffCase = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

const AUTHOR_HANDOFF_CASES: [AuthorHandoffCase; 23] = [
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

#[test]
fn control_only_author_handoffs_settle_buffer_and_device_rows_separately() {
    // Verified first with the pinned CVS renderer.  An empty word has no
    // native field cell at a tight list-head boundary, `\&` does have one,
    // and a bare `\z` only arms BACKAFTER.  After `.mc`, `viscol` keeps the
    // device row occupied even when the current field buffer is empty, so a
    // later control must flush and clear BACKAFTER before Bob executes.

    for (label, style, prefix, operand, control, native_expected, lowered_expected) in
        AUTHOR_HANDOFF_CASES
    {
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

#[test]
fn graphless_native_posts_retire_padding_and_retain_the_next_field_gap() {
    // Each exact source ran the pristine CVS ASCII/UTF-8/HTML/tree/lint
    // profiles before these assertions. term_field() writes positioning
    // blanks only with a graph (term.c:389-427), but term_flushln() restores
    // minbl from trailspace even for NBRZW/empty fields (233-253). Fd's post,
    // Bd's BODY post and no-fill NODE_LINE consume the same live field.
    for (label, control, expected) in [
        ("declaration post", ".Fd \\&\n", "LONGTEXT Bob  BODY"),
        (
            "display post",
            ".Bd -literal -compact\n.No \\&\n.Ed\n",
            "LONGTEXT Bob  BODY",
        ),
        (
            "source and fill-mode events",
            ".nf\n.No \\&\n.fi\n",
            "LONGTEXTBob   BODY",
        ),
        (
            "repeated invisible input",
            ".No \\&\n.Fd \\&\n",
            "LONGTEXT Bob  BODY",
        ),
    ] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -hang -width 12n\n.It Xo\n.No LONGTEXT\n.mc\n.No \"\"\n{control}.An -split\n.An Bob\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        assert!(native.contains(expected), "{label}: {native:?}");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&query, false).unwrap();
        assert!(!json.contains("\\u0000mant:"), "private owner leaked");
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let restored = decoded.into();
        let lowered = mant_render::render_query_text(&restored);
        assert!(lowered.contains(expected), "{label}: {lowered:?}");
        for word in ["LONGTEXT", "Bob", "BODY"] {
            assert_eq!(lowered.matches(word).count(), 1, "{label}: {lowered:?}");
        }
    }
}
