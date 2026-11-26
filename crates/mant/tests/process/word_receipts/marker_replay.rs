//! Native marker, cursor, and raw advance interactions.

use super::{EDITABLE_FIELD, description, render, without_sgr};

#[test]
fn marked_native_buffer_edits_preserve_the_reference_rows() {
    // Pinned mandoc ASCII and UTF-8: term.c::term_word preserves individual
    // forward-motion slots; term_fill consumes LF after the native overwrite.
    // Assert rows as well as ink so a compacted advance cannot pass this test.
    for (case, source, expected) in [
        (
            "marked advance with a partial retreat",
            r#".Em "A\p\:" Ns Em "\z\h'-1n'\h'1n'" Ns Em "FG\p\c\h'3n'" No "\h'-2n'X""#,
            "A\nFG  X",
        ),
        (
            "continuation across a zero-width prefix and author blank",
            r#".Em "A" Em "\z\h'-1n'\h'1n'"  m "F\p\h'-1n' G\p\h'-1n'\c" Em "\h'-10n'X""#,
            "A   _XFG",
        ),
        (
            "deferred retreat through styled glyph and zero-width control",
            r#".Em "A\zC\fPD\h'2n'" Ns Em "\fP\h'-8n'\fBF\pG" Em "\h'0n'\&\fP" Em "\z\h'-5n'\cX""#,
            "FG\nX",
        ),
        (
            "a completed overwrite can be rearmed without losing prior paint",
            r#".Em "A\zC\fPD\z" Em "\z\h'-8n'\h'2n'" Ns No "F\pG\p" Ns Em "\h'-5n'\cX""#,
            "AD  XG",
        ),
        (
            "inherited backafter cancels motion before a terminal marker",
            r#".Em "AB" Em "\h'-4n'F\z" Em "\h'1n'\p\z" Em "\h'-1n' \h'-1n'X""#,
            "AF\n",
        ),
        (
            "repeated zero advance can leave both native registers armed",
            r#".Em "A\z\zC\z\fPD\z" Em "\z\h'-8n'\h'2n'" Ns No "F\pG" Ns Em "\h'-5n'\cX""#,
            "ADXG",
        ),
        (
            "deferred retreat across an unpainted advance",
            r#".No "A\zC\fPD\h'1n'" Em "\z\h'-8n'\h'2n'" Ns No "F\pG" Ns Em "\h'-9n'\cX""#,
            "ACX   FG",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{source}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{case}");
        let ansi = render(&source, "roff", "text", "always", false);
        assert_eq!(without_sgr(&ansi), plain, "{case}");
    }
}

#[test]
fn retreated_marker_and_explicit_blanks_remain_editable_until_field_flush() {
    // Pinned ASCII/UTF-8: term.c:659–660 and 678–699. The retreat and
    // new graph remain in the same native field, even across the marker.
    let prefix = r#".Sy "AB" No "\h'1n'\h'-6n'\z\fP\:C""#;
    for (middle, tail, expected) in [
        (
            r#"Sy "\zFG\h'2n'\p\~""#,
            r#"Sy "\h'1n'\h'-2n'\fPX\h'-4n'""#,
            "G  X",
        ),
        (
            r#"Sy "\zFG\h'2n'\~""#,
            r#"Sy "\h'1n'\h'-2n'\fPX\h'-4n'""#,
            "G  X",
        ),
        (r#"Sy "\zFG\h'2n'\p\~""#, r#"Sy "\&X""#, "G  \u{a0}X"),
    ] {
        let source = format!("{EDITABLE_FIELD}{prefix} {middle} Ns {tail}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{source}");
        let ansi = render(&source, "roff", "text", "always", false);
        assert_eq!(without_sgr(&ansi), plain, "{source}");
    }
}

#[test]
fn a_marked_append_keeps_its_local_retreat_until_the_next_word() {
    // Pinned UTF-8: term_word leaves col retreated inside the newly written
    // suffix, without flushing earlier markers (term.c:659–718). The next
    // operand can replace a decorative byte or an unaccepted graph there.
    for (words, expected) in [
        (
            r#".Sy "\fPA" Em "\zAB\h'1n' " Sy "F\pG\p\c\h'-1n'" No "\h'-4n'X""#,
            "A B   FG",
        ),
        (
            r#".Sy "AA" Em "\z\0\h'1n' " Sy "\h'-1n'F\pG\p\c\h'-1n'" No "\h'-4n'X""#,
            "AA \u{a0} FG",
        ),
        (
            r#".Em "\z" Em "\z\h'-1n'\h'1n'" Em "F\pG\p\c\h'-4n'" No "\h'3n'\h'-4n'X""#,
            "   FG",
        ),
        (
            r#".No "A\p" Em "\z\h'-10n'\h'1n'" Em "F\pG\p\c\h'-4n'" Em "\h'3n'\h'-4n'X""#,
            "A\n  FX",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{words}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{words}");
        let ansi = render(&source, "roff", "text", "always", false);
        assert_eq!(without_sgr(&ansi), plain, "{words}");
    }
}

#[test]
fn hanging_body_can_overwrite_a_graphless_marker_before_its_first_word() {
    // Pinned ASCII/UTF-8: the BODY starts a new native field even when the
    // HEAD row remains open. A later HORIZ revisits BODY's LF/NBRZW/blank
    // slots before term_fill commits the row (term.c:621–628, 659–718).
    for (head, body, expected) in [
        (
            r#".It Em "C D" Em "\h'-6n'\zF""#,
            r#".No "\p\& B" No "\h'-3n'Y""#,
            "D    YB",
        ),
        (
            r#".It Em "\X'tty: link https://k.test'C\X'tty: link' D" No "\h'-1n' ""#,
            r#".No "\p\& B" Ns No "\h'-2n'Y""#,
            "C D  YB",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}.Bl -hang -width 3n\n{head}\n{body}\n.El\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{source}");
        let ansi = render(&source, "roff", "text", "always", false);
        assert_eq!(without_sgr(&ansi), plain, "{source}");
    }
}

#[test]
fn diagnostic_body_can_rewrite_a_marker_in_the_retained_tab_field() {
    // Pinned ASCII/UTF-8: the DIAG HEAD and BODY still share one tab
    // field. Y replaces LF before B rather than moving onto a new row.
    let words = r#".Bl -diag
.It Sy "\X'tty: link https://k.test'C\X'tty: link'DV" Ns No "\h'-1n'\&"
.No "\p B" No "\h'-3n'Y"
.El
"#;
    let source = format!("{EDITABLE_FIELD}{words}");
    let plain = render(&source, "roff", "text", "never", false);
    assert_eq!(description(&plain), "Sy CDV Ns No  YB");
    assert_eq!(
        without_sgr(&render(&source, "roff", "text", "always", false)),
        plain
    );
    assert_eq!(
        super::document_links(&source, "roff"),
        [(
            super::LinkTarget::External {
                uri: "https://k.test".into()
            },
            "C".into()
        )]
    );
}

#[test]
fn a_marked_advance_can_rewrite_a_live_backline_before_flush() {
    // Pinned ASCII/UTF-8: these have a complete prior native field and
    // either a held BACKBEFORE or inherited BACKAFTER. The later positive
    // HORIZ advances through the retained LF/NBRSP without collapsing
    // cells, so the final rewind sees the right bytes (term.c:659–718).
    for (words, expected) in [
        (
            r#".No "AB" No "\h'1n'\h'-6n'\z\:C" Ns No "\:FG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'1n'\h'-2n'\fPX""#,
            "FG    X",
        ),
        (
            r#".Sy "AB" No "\h'1n'\h'-4n'\zZ\pC\z" Ns No "\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            "ABCG   X",
        ),
        (
            r#".Sy "AB" Em "\h'1n'\h'-4n'\z \pC" Em "\h'1n'\zFG\h'2n'\p\h'2n'" Ns Sy "\h'1n'\h'-2n'\fPX""#,
            "AC\nG   X",
        ),
        (
            r#".Sy "A\h'2n'\h'-1n'B\h'2n'\h'-1n'" Ns Em "\h'1n'\h'-5n'\z\pC" Ns No "\zFG\h'2n'\p\0" Sy "\h'1n'\h'-2n'\fPX ""#,
            "A G  \u{a0}X",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{words}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{source}");
        assert_eq!(
            without_sgr(&render(&source, "roff", "text", "always", false)),
            plain
        );
    }
}

#[test]
fn a_drawn_blank_does_not_hide_bold_copies_on_the_accepted_row() {
    // Pinned terminal output keeps both copies when the rewind writes
    // between them. A one-column drawn SP elsewhere on the same row is
    // not an opaque paint operation (term.c:389–434, 915–925).
    for (words, expected) in [
        (
            r#".Em "A\&" Sy "\z\h'-1n'\h'1n' K" Ns Sy "F\p\pG\p\p\c" No "\h'-4n'X""#,
            "A   KFGXG",
        ),
        (
            r#".Em "A\pB" Sy "\h'-1n'\h'-5n'\h'0n'\&F " Sy "\&F\h'-2n'\z\p" Ns Sy " \h'-8n'\cX""#,
            "FB FFX",
        ),
    ] {
        let source = format!("{EDITABLE_FIELD}{words}\n");
        let plain = render(&source, "roff", "text", "never", false);
        assert_eq!(description(&plain), expected, "{source}");
        assert_eq!(
            without_sgr(&render(&source, "roff", "text", "always", false)),
            plain
        );
    }
}

#[test]
fn a_long_styled_head_remains_editable_until_the_native_break() {
    // Pinned ASCII and UTF-8: the quoted operand starts with a marker.
    // Decoration exceeds a 64-slot proof before the following HORIZ
    // covers A. The link-like text is literal, not an annotation.
    let words = r#".Bl -inset
.It Em "\\pX'tty: link https://k.test'C\X'tty: link' DAB" Sy "\h'-6n'\&"
.No "\p"
.br
.No "\h'-1n'Y"
.El
"#;
    let source = format!("{EDITABLE_FIELD}{words}");
    let plain = render(&source, "roff", "text", "never", false);
    let rows: Vec<_> = description(&plain).lines().map(str::trim_start).collect();
    assert_eq!(rows, ["X'tty:", "link https://k.test'C D_B", "Y"]);
    assert_eq!(
        without_sgr(&render(&source, "roff", "text", "always", false)),
        plain
    );
    assert_eq!(
        super::document_links(&source, "roff"),
        Vec::<(mant_ir::LinkTarget, String)>::new()
    );
}

#[test]
fn inset_body_replays_the_raw_marker_tail_before_retiring_head_ink() {
    // Inset BODY writes an NBSP into the same open buffer as HEAD
    // (mdoc_term.c:757–772). A pending backspace consumes a raw SP or
    // appends a raw backline; it does not necessarily replace the
    // previous visible glyph (term.c:659–660, 903–929).
    for (head, body, word, linked) in [
        (
            r#".It No "\zA\p""#,
            r#".No "\h'-4n'" No "\pB" No C"#,
            "ABC",
            false,
        ),
        (
            r#".It No "\z\X'tty: link https://x.io'A\X'tty: link'\p""#,
            r#".No "\h'-4n'" No "\pB" No C"#,
            "ABC",
            true,
        ),
        (r#".It Em "\zA\p ""#, r#".No "\h'-4n'\zB" No C"#, "C", false),
        (
            r#".It Em "\z\X'tty: link https://x.io'A\X'tty: link'\p ""#,
            r#".No "\h'-4n'\zB" No C"#,
            "AC",
            true,
        ),
    ] {
        let mut source = format!("{EDITABLE_FIELD}.Bl -inset\n{head}\n{body}\n.El\n");
        for round in 0..=2 {
            let format = if round == 0 { "roff" } else { "markdown" };
            let plain = render(&source, format, "text", "never", false);
            let visible = description(&plain).trim();
            // Ordinary Markdown represents the inset as a list item.
            assert_eq!(
                visible.strip_prefix("• ").unwrap_or(visible),
                word,
                "{head}, round {round}"
            );
            assert_eq!(
                super::search_visible_word_with_format(&source, format, word),
                1
            );
            assert_eq!(
                super::search_visible_word_with_format(&source, format, "A"),
                0
            );
            assert_eq!(
                super::search_visible_word_with_format(&source, format, "BC"),
                0
            );
            if linked {
                super::assert_link(&source, format, "A");
            } else {
                assert_eq!(
                    super::document_links(&source, format),
                    Vec::<(mant_ir::LinkTarget, String)>::new()
                );
            }
            source = render(&source, format, "markdown", "never", false);
        }
    }
}

#[test]
fn accepted_run_in_link_beats_its_empty_head_identity() {
    // CVS mdoc_term.c:757–772 and term.c:659–719: HEAD and BODY write
    // to the same field, so the later word can move surviving HEAD ink
    // into the BODY's accepted range before either owner publishes.
    let source = r#".Dd November 22, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual renderer
.Sh DESCRIPTION
.Bl -inset
.It Em "\z\X'tty: link https://a.test'A\X'tty: link'\p "
.No "\h'|0n'B" No C
.El
.Sh ENDTEST
Done.
"#;
    let links = super::document_links(source, "roff");
    assert_eq!(
        links,
        [(
            mant_ir::LinkTarget::External {
                uri: "https://a.test".into(),
            },
            "A".into(),
        )],
    );
}

fn review_body(text: &str) -> &str {
    description(text).split_once("\n\n").unwrap().0.trim()
}

#[test]
fn raw_slot_retreat_preserves_the_native_source_line_boundary() {
    // man_term.c:915–923 and term.c:659–719: a no-fill input line
    // bypasses the automatic flush only after the preceding terminal
    // continuation. An ordinary filled source line adds TABREF instead.
    for (source, expected) in [
        (
            r#".TH PROBE 1 "November 26, 2026" "test"
.SH NAME
probe \- test the manual
.SH DESCRIPTION
.nf
\zJ\h'1n'\c
\h'-1n'Q X
.fi
.SH ENDTEST
Done.
"#,
            "Q X",
        ),
        (
            r#".TH PROBE 1 "November 26, 2026" "test"
.SH NAME
probe \- test the manual
.SH DESCRIPTION
\zJ\h'1n'\c
\h'-1n'Q X
.SH ENDTEST
Done.
"#,
            "JQ X",
        ),
        (
            r#".TH PROBE 1 "November 26, 2026" "test"
.SH NAME
probe \- test the manual
.SH DESCRIPTION
.nf
\zJ\h'1n'
.br
\h'-1n'Q X
.fi
.SH ENDTEST
Done.
"#,
            "J\nQ X",
        ),
    ] {
        let text = render(source, "roff", "text", "never", false);
        assert_eq!(review_body(&text), expected);
    }
}

#[test]
fn a_column_body_continuation_overwrites_the_linked_glyph_at_its_native_slot() {
    // mdoc_term.c:939–950: the column BODY post, not its source word,
    // flushes the native field; it can replace earlier cells in that column.
    let source = r#".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual
.Sh DESCRIPTION
.Bl -column "xxxxxx" "xxxxxxxxxx"
.It label Ta Xo
\z\X'tty: link https://j.test'J\X'tty: link'\p\:\c
\h'|0n'\X'tty: link https://q.test'Q\X'tty: link' X
.Xc
.El
.Sh ENDTEST
Done.
"#;
    let text = render(source, "roff", "text", "never", false);
    assert_eq!(
        review_body(&text).split_whitespace().collect::<Vec<_>>(),
        ["label", "QX"]
    );
    let linked: Vec<_> = super::document_links(source, "roff")
        .into_iter()
        .filter(|(_, label)| !label.is_empty())
        .collect();
    assert_eq!(
        linked,
        [(
            mant_ir::LinkTarget::External {
                uri: "https://q.test".into()
            },
            "Q".into()
        )],
    );
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "QX"),
        1
    );
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "JQ"),
        0
    );
}

#[test]
fn an_absolute_cursor_edit_orders_equal_labels_by_their_accepted_slots() {
    // term.c:659–719: the marker remains editable through the absolute
    // cursor move. The pinned terminal replay with distinct J/K/L glyphs
    // paints KJL, witnessing the link origins even when the glyphs match.
    let source = r#".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm test
.Nd glyph attribution
.Sh DESCRIPTION
.Em "\X'tty: link https://a.test/item'X\X'tty: link'\p" No "\h'|0n'\X'tty: link https://b.test/item'X\X'tty: link'" No "\X'tty: link https://c.test/item'X\X'tty: link'"
.Sh ENDTEST
done.
"#;
    for anchors in [false, true] {
        let mut input = source.to_owned();
        let mut format = "roff";
        for _ in 0..=2 {
            let text = render(&input, format, "text", "never", false);
            assert_eq!(review_body(&text), "XXX");
            let rendered = super::document_links(&input, format);
            assert_eq!(
                rendered,
                ["b", "a", "c"].map(|host| (
                    mant_ir::LinkTarget::External {
                        uri: format!("https://{host}.test/item")
                    },
                    "X".into(),
                ))
            );
            input = render(&input, format, "markdown", "never", anchors);
            format = "markdown";
        }
    }
}

#[test]
fn source_nbsp_and_each_authored_space_occupy_separate_native_slots() {
    // term.c:678–699 and 903–929: one EN of positive motion resolves
    // BACKBEFORE without writing. The differing later space counts are
    // real slots, so the five-column rewind only removes J in the first.
    for (source, expected) in [
        (
            r".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual
.Sh DESCRIPTION
J \z\[u00A0]\h'1n' \c
\h'-5n'Q X
.Sh ENDTEST
Done.
",
            "Q X",
        ),
        (
            r".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual
.Sh DESCRIPTION
J \z\[u00A0]\h'1n'  \c
\h'-5n'Q X
.Sh ENDTEST
Done.
",
            "JQ X",
        ),
    ] {
        let text = render(source, "roff", "text", "never", false);
        assert_eq!(
            review_body(&text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            expected
        );
    }
}

#[test]
fn a_body_rewind_keeps_the_generated_separator_after_relocated_head_ink() {
    // Inset adds a generated fixed space before the BODY but HEAD post
    // does not flush (mdoc_term.c:757–772, 939–945). After absolute
    // movement the retained label tail still precedes that space.
    let source = r".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual
.Sh DESCRIPTION
.Bl -inset
.It Sy label
J \h'|0n'Q X
.El
.Sh ENDTEST
Done.
";
    let text = render(source, "roff", "text", "never", false);
    assert_eq!(review_body(&text), "Xabel J");
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "J"),
        1
    );
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "XabelJ"),
        0
    );
}

#[test]
fn alternating_no_fill_operands_edit_the_same_native_tape() {
    // man_term.c:303–310 switches fonts around term_word(), not buffers.
    // Both movements replace J in the shared no-fill row.
    for operands in [
        r#""\zJ\p\:" "\h'|0n'Q" "X""#,
        r#""\zJ\h'1n'" "\h'-1n'Q" "X""#,
    ] {
        let source = format!(
            ".TH PROBE 1 \"November 26, 2026\" \"test\"\n.SH NAME\nprobe \\- test the manual\n\
             .SH DESCRIPTION\n.nf\n.BR {operands}\n.fi\n.SH ENDTEST\nDone.\n"
        );
        let text = render(&source, "roff", "text", "never", false);
        assert_eq!(review_body(&text), "QX", "{operands}");
    }
}

#[test]
fn the_continued_tag_body_retires_the_replaced_link_owner() {
    // A physical continuation in mdoc's tag BODY shares its raw tape.
    // Read both the final ink and the surviving link rather than its URI alone.
    let source = r".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual
.Sh DESCRIPTION
.Bl -tag -width 6n
.It Sy label
\z\X'tty: link https://j.test'J\X'tty: link'\p\:\c
\h'|0n'\X'tty: link https://q.test'Q\X'tty: link' X
.El
.Sh ENDTEST
Done.
";
    let text = render(source, "roff", "text", "never", false);
    assert_eq!(
        review_body(&text).split_whitespace().collect::<Vec<_>>(),
        ["label", "QX"]
    );
    assert_eq!(
        super::document_links(source, "roff"),
        [(
            mant_ir::LinkTarget::External {
                uri: "https://q.test".into(),
            },
            "Q".into(),
        )]
    );
}

#[test]
fn an_inset_body_keeps_the_generated_fixed_blank_through_a_marked_edit() {
    // CVS ASCII and UTF-8 both finish the retained bold tape as Xabel,
    // fixed blank, J. Only the NBSP spelling differs between devices.
    let source = r".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual
.Sh DESCRIPTION
.Bl -inset
.It Sy label
\fB\zJ\p\fR
\h'|0n'Q X
.El
.Sh ENDTEST
Done.
";
    let text = render(source, "roff", "text", "never", false);
    assert_eq!(review_body(&text), "Xabel J");
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "J"),
        1
    );
}

#[test]
fn a_run_in_marker_does_not_rebind_retained_or_covered_glyphs() {
    // The two fixed terminal devices paint J and L at different columns;
    // K's later overwritten column cannot take the accepted L link.
    let source = r#".Dd November 26, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm test
.Nd glyph attribution
.Sh DESCRIPTION
.Bl -inset
.It No "\z\X'tty: link https://a.test/item'J\X'tty: link'\p\~"
.No "\h'-4n'" Ns Em "\z\X'tty: link https://b.test/item'K\X'tty: link'" Ns No "\X'tty: link https://c.test/item'L\X'tty: link'"
.El
.Sh ENDTEST
done.
"#;
    let text = render(source, "roff", "text", "never", false);
    assert_eq!(review_body(&text), "JL");
    assert_eq!(
        super::document_links(source, "roff"),
        [
            (
                mant_ir::LinkTarget::External {
                    uri: "https://a.test/item".into(),
                },
                "J".into(),
            ),
            (
                mant_ir::LinkTarget::External {
                    uri: "https://c.test/item".into(),
                },
                "L".into(),
            ),
        ]
    );
}

#[test]
fn an_inset_body_rewind_addresses_the_generated_bold_space_copies() {
    // "No" sets Roman inside HEAD; the generated U+00A0 then inherits the
    // restored outer bold font. Its two extra native cells remain writable
    // until BODY's final pass (mdoc_term.c:760–772; term.c:903–929).
    let source = r#".Dd November 22, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual renderer
.Sh DESCRIPTION
.ft B
.Bl -inset
.It No "\zA\:\p"
.No "\h'|1n'" No "\pB" No C
.El
.ft R
.Sh ENDTEST
Done.
"#;
    let text = render(source, "roff", "text", "never", false);
    assert_eq!(review_body(&text), "AB C");
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "AB"),
        1
    );
    assert_eq!(
        super::search_visible_word_with_format(source, "roff", "BC"),
        0
    );
}

#[test]
fn a_diagonal_body_rewinds_over_the_generated_fixed_blank_pair() {
    // The source HEAD's NBRZW does not cancel BACKBEFORE. CVS writes a
    // backline before the BODY's pair, then lets BODY's movement replace
    // one blank without closing the HEAD's two accepted passes.
    for (word, expected_words) in [("B", vec!["A", "B", "C"]), (r"\pB", vec!["A", "BC"])] {
        let source = format!(
            r#".Dd November 22, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual renderer
.Sh DESCRIPTION
.Bl -diag
.It "\zA\p \&"
.No "\h'-4n'" No "{word}" No C
.El
.Sh ENDTEST
Done.
"#
        );
        let text = render(&source, "roff", "text", "never", false);
        let body = review_body(&text);
        assert_eq!(
            body.lines()
                .map(|row| row.replace(' ', ""))
                .collect::<Vec<_>>(),
            ["A", "BC"],
            "{word}"
        );
        if word == "B" {
            // In the second case ASCII and UTF-8 differ on the second
            // row's internal gap; assert only the agreed physical rows.
            assert_eq!(body.split_whitespace().collect::<Vec<_>>(), expected_words);
            assert_eq!(
                super::search_visible_word_with_format(&source, "roff", "B"),
                1
            );
            assert_eq!(
                super::search_visible_word_with_format(&source, "roff", "BC"),
                0
            );
        }
    }
}

#[test]
fn an_inset_separator_replaces_the_trailing_source_space_before_a_body_rewind() {
    // The generated fixed blank backs onto the last authored SP before it
    // writes. BACKBEFORE cannot make an extra blank at the end of this HEAD.
    // A one-space tail then keeps AB / C as words; a second authored space
    // and rearm allow the complete ABC word on the original physical row.
    for (spaces, body, expected, query) in [
        (" ", r#""\h'-4n'" No B No C"#, "AB C", "AB"),
        ("  ", r#""\h'-4n'\zB" No C"#, "ABC", "ABC"),
    ] {
        let source = format!(
            r#".Dd November 22, 2026
.Dt PROBE 1
.Os
.Sh NAME
.Nm probe
.Nd test the manual renderer
.Sh DESCRIPTION
.Bl -inset
.It No "\zA\p{spaces}"
.No {body}
.El
.Sh ENDTEST
Done.
"#
        );
        let text = render(&source, "roff", "text", "never", false);
        assert_eq!(review_body(&text), expected);
        assert_eq!(
            super::search_visible_word_with_format(&source, "roff", query),
            1
        );
        assert_eq!(
            super::search_visible_word_with_format(&source, "roff", "BC"),
            0
        );
    }
}
