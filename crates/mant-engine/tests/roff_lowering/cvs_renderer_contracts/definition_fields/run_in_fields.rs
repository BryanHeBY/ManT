use super::*;

#[test]
fn run_in_fixed_cells_fold_completed_head_glyph_under_generated_cells() {
    // Fixed CVS mdoc_term.c::termp_it_pre() sends inset/diag cells through
    // term_word("\\ ") / term_word("\\ \\ "). On this UTF-8 device the
    // generated cell executes encode1(U+00A0) and consumes the pending HEAD
    // glyph's BACKBEFORE retreat (term.c:901-908): the raw terminal rows
    // still carry the overstruck glyph bytes (`X^H<NBSP>`), but the
    // coverage folds the pending glyph out of the readable term — only
    // glyphs before the `\z` remain visible. Bare or blank \z operands
    // never contributed a visible HEAD glyph.
    for style in ["inset", "diag"] {
        for (head, expected_term) in [(r"\zX", ""), (r"x\zY", "x"), (r"\z", ""), ("\\z ", "")] {
            let source = format!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n.No Q\n.El\n"
            );
            let native = native_terminal_raw(&source);
            if !expected_term.is_empty() {
                assert!(native.contains(expected_term), "{style} {head}: {native:?}");
            }
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let item = first_definition_item(query.document.as_ref().unwrap());
            let term = item
                .terms
                .iter()
                .map(|part| mant_ir::inline_plain_text(part))
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(term, expected_term, "{style} {head}: {item:?}");
            if style == "diag" && !expected_term.is_empty() {
                assert!(
                    item.terms
                        .iter()
                        .flatten()
                        .any(|inline| matches!(inline, Inline::Strong { .. })),
                    "{style} {head}: {item:?}"
                );
            }
            let rendered = mant_render::render_query_text(&query);
            assert!(rendered.contains('Q'), "{style} {head}: {rendered:?}");
        }
    }
}

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
        let item = first_definition_item(query.document.as_ref().unwrap());
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
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
        let paragraph = document.sections[1]
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
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
        let item = first_definition_item(query.document.as_ref().unwrap());
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
                _ => None,
            })
            .expect("run-in description");
        assert_eq!(description, expected, "lowered {style} {head:?}: {item:?}");
    }
}
