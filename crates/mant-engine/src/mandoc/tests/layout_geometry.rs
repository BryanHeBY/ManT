//! Original source probes with coordinates independently checked against
//! mandoc CVS HEAD's character-device `man_term`/`mdoc_term` layout rules.

fn man(body: &str) -> String {
    let source = format!(".TH GEOMETRY 1\n.SH DESCRIPTION\n{body}\n");
    crate::render_query_text(&crate::query_roff_bytes(source.as_bytes()).unwrap())
}

fn mdoc(body: &str) -> String {
    let source = format!(".Dd September 9, 2026\n.Dt GEOMETRY 1\n.Os\n.Sh DESCRIPTION\n{body}\n");
    crate::render_query_text(&crate::query_roff_bytes(source.as_bytes()).unwrap())
}

fn column(text: &str, token: &str) -> usize {
    text.lines()
        .find_map(|line| line.find(token))
        .unwrap_or_else(|| panic!("missing {token}:\n{text}"))
}

#[test]
fn man_tag_width_is_body_geometry_not_only_inline_fit() {
    for width in [0, 2, 7, 20] {
        let text = man(&format!(".TP {width}\nLONGTAG\nBODY\n"));
        assert_eq!(column(&text, "LONGTAG"), 0, "{text}");
        assert_eq!(column(&text, "BODY"), width, "{text}");
    }
    let text = man(".IP x 7\nFIRST\n.IP \"\" 12\nSECOND\n");
    assert_eq!(column(&text, "FIRST"), 7, "{text}");
    assert_eq!(column(&text, "SECOND"), 12, "{text}");
}

#[test]
fn rs_uses_source_distance_and_prevailing_tag_width() {
    for (width, expected) in [
        ("0", 0),
        ("2", 2),
        ("7n", 7),
        ("12n", 12),
        ("-2n", 0),
        ("1i", 10),
    ] {
        let text = man(&format!(".RS {width}\nBODY\n.RE\n"));
        assert_eq!(column(&text, "BODY"), expected, "{text}");
    }
    let text =
        man(".TP 20\nTAG\nINITIAL\n.RS\nCONTINUATION\n.RE\n.PP\nRESET\n.RS\nAFTERRESET\n.RE\n");
    assert_eq!(column(&text, "INITIAL"), 20, "{text}");
    assert_eq!(column(&text, "CONTINUATION"), 20, "{text}");
    assert_eq!(column(&text, "AFTERRESET"), 7, "{text}");
    let text = man(".RS 0.4n\n.RS 0.4n\n.RS 0.4n\nBODY\n.RE\n.RE\n.RE\n");
    assert_eq!(column(&text, "BODY"), 1, "{text}");
}

#[test]
fn definition_styles_resolve_distinct_body_and_term_geometry() {
    for (style, body, same_line) in [
        ("tag", 17, true),
        ("hang", 17, true),
        ("inset", 7, true),
        ("diag", 8, true),
        ("ohang", 3, false),
    ] {
        let text = mdoc(&format!(
            ".Bl -{style} -width 12n -offset 3n\n.It TAG\nBODY\n.El\n"
        ));
        assert_eq!(column(&text, "TAG"), 3, "{style}: {text}");
        assert_eq!(column(&text, "BODY"), body, "{style}: {text}");
        assert_eq!(
            text.lines()
                .any(|line| line.contains("TAG") && line.contains("BODY")),
            same_line,
            "{style}: {text}"
        );
    }
}

#[test]
fn mdoc_offsets_accept_printable_samples_and_display_defaults() {
    for (offset, expected) in [
        (None, 0),
        (Some("left"), 0),
        (Some("indent"), 6),
        (Some("indent-two"), 12),
        (Some("3n"), 3),
        (Some("12"), 2),
        (Some("abc"), 3),
    ] {
        let option = offset.map_or(String::new(), |value| format!(" -offset {value}"));
        let text = mdoc(&format!(".Bd -ragged{option}\nBODY\n.Ed\n"));
        assert_eq!(column(&text, "BODY"), expected, "{offset:?}: {text}");
    }
    for display in ["D1", "Dl"] {
        let text = mdoc(&format!(".{display} BODY\n"));
        assert_eq!(column(&text, "BODY"), 6, "{text}");
    }
}

#[test]
fn converted_tag_and_native_list_keep_their_declared_body_column() {
    for style in ["tag", "enum"] {
        let text = mdoc(&format!(
            ".Bl -{style} -width 12n -offset 3n\n.It 1.\nFIRST\n.It 2.\nSECOND\n.El\n"
        ));
        assert_eq!(column(&text, "FIRST"), 17, "{style}: {text}");
        assert_eq!(column(&text, "SECOND"), 17, "{style}: {text}");
    }
}
