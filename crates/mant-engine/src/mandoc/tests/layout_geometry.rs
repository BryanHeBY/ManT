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

#[test]
fn man_in_restores_macro_base_instead_of_swapping_previous_requests() {
    let text = man(".in 10n\nFIRST\n.in +2n\nSECOND\n.in\nTHIRD\n.in\nFOURTH\n");
    for (token, expected) in [("FIRST", 5), ("SECOND", 7), ("THIRD", 0), ("FOURTH", 0)] {
        assert_eq!(column(&text, token), expected, "{text}");
    }
    let text = man(".RS 3n\n.in 20n\nFIRST\n.in\nSECOND\n.in\nTHIRD\n.RE\nAFTER\n");
    for (token, expected) in [("FIRST", 15), ("SECOND", 3), ("THIRD", 3), ("AFTER", 0)] {
        assert_eq!(column(&text, token), expected, "{text}");
    }
    let text = mdoc(".in 20n\nBODY\n");
    assert_eq!(
        column(&text, "BODY"),
        0,
        "mdoc does not execute man in: {text}"
    );
}

#[test]
fn hp_keeps_first_and_continuation_origins_and_updates_tag_width() {
    let text = man(".HP 12\nFIRST\n.br\nSECOND\n.TP\nTAG\nBODY\n");
    assert_eq!(column(&text, "FIRST"), 0, "{text}");
    assert_eq!(column(&text, "SECOND"), 12, "{text}");
    assert_eq!(column(&text, "BODY"), 12, "{text}");
    let text = man(".in 10n\nFIRST\n.PP\nSECOND\n.br\nTHIRD\n");
    assert_eq!(column(&text, "SECOND"), 0, "{text}");
    assert_eq!(column(&text, "THIRD"), 0, "{text}");
}

#[test]
fn nested_mdoc_offsets_keep_basic_units_after_marker_layout() {
    let text = mdoc(
        ".Bl -bullet -offset 0.4n -width 0.4n\n.It\n.Bd -ragged -offset 0.4n\nBODY\n.Ed\n.El\n",
    );
    assert_eq!(column(&text, "BODY"), 3, "{text}");
}

#[test]
fn tq_fit_uses_composed_body_origin_after_fractional_rs() {
    let text =
        man(".RS 0.4n\n.TP 20.4n\nABCDEFGHIJKLMNOPQRST\n.TQ\nabcdefghijklmnopqrst\nBODY\n.RE\n");
    assert!(
        text.lines().any(|line| line == "abcdefghijklmnopqrst BODY"),
        "{text}"
    );
}

#[test]
fn in_is_inherited_by_passive_structures_but_not_paragraph_resets() {
    let text = man(".in 10n\nBEFORE\n.TS\nl.\nCELL\n.TE\nAFTER\n.PP\nRESET\n");
    for token in ["BEFORE", "CELL", "AFTER"] {
        assert_eq!(column(&text, token), 5, "{text}");
    }
    assert_eq!(column(&text, "RESET"), 0, "{text}");
}

#[test]
fn hp_blank_space_consumes_first_line_without_ending_hanging_scope() {
    let text = man(".HP 12\nFIRST\n.sp 1\nSECOND\n.br\nTHIRD\n");
    assert_eq!(column(&text, "FIRST"), 0, "{text}");
    for token in ["SECOND", "THIRD"] {
        assert_eq!(column(&text, token), 12, "{text}");
    }
    let text = man(".HP 12\n.sp 1\nFIRST\n.br\nSECOND\n");
    for token in ["FIRST", "SECOND"] {
        assert_eq!(column(&text, token), 12, "{text}");
    }
}

#[test]
fn explicit_in_overrides_hanging_and_restores_real_macro_base_inside_definitions() {
    for (request, expected) in [("30n", 25), ("+2n", 14), ("", 0)] {
        let text = man(&format!(
            ".HP 12\nFIRST\n.sp 1\n.in {request}\nSECOND\n.br\nTHIRD\n"
        ));
        for token in ["SECOND", "THIRD"] {
            assert_eq!(column(&text, token), expected, "{text}");
        }
    }
    for (prefix, suffix, base) in [("", "", 0), (".RS 3n\n", ".RE\n", 3)] {
        let text = man(&format!(
            "{prefix}.TP 20\nTAG\nBEFORE\n.in\nAFTER\n{suffix}"
        ));
        assert_eq!(column(&text, "BEFORE"), base + 20, "{text}");
        assert_eq!(column(&text, "AFTER"), base, "{text}");
    }
    let text = man(".HP 12\n.TP\nTAG\nBODY\n");
    assert_eq!(
        column(&text, "BODY"),
        7,
        "empty HP must not assign a new width: {text}"
    );
}
