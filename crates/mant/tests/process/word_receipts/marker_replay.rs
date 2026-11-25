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
