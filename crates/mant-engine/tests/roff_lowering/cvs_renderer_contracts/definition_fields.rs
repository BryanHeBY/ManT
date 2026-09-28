use super::*;

#[test]
fn continued_literal_definition_body_reaches_the_text_consumer_on_the_head_row() {
    // Both exact sources passed the pinned reference -Tascii/-Tutf8/-Tlint.
    // mdoc_term.c::print_mdoc_node() observes NODE_LINE before BODY dispatch;
    // TERMP_NONEWLINE from \c alone keeps that physical row open.
    fn content_lines(output: &str) -> Vec<String> {
        output
            .lines()
            .skip_while(|line| line.trim() != "DESCRIPTION")
            .skip(1)
            .take_while(|line| !line.contains("Linux 6."))
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }
    let prefix = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.Bl -hang -width 4n\n.It Xo\n";
    for (head, same_row) in [(".No X\\c", true), (".No X", false)] {
        let source = format!("{prefix}{head}\n.Xc\n.No BODY\n.El\n");
        let native = content_lines(&native_terminal(&source));
        let lowered = content_lines(&lowered_terminal(&source));
        assert_eq!(
            native.len(),
            if same_row { 1 } else { 2 },
            "{head}: {native:?}"
        );
        assert_eq!(lowered.len(), native.len(), "{head}: {lowered:?}");
        if same_row {
            assert!(lowered[0].contains('X') && lowered[0].contains("BODY"));
        } else {
            assert_eq!(lowered[0], "X");
            assert_eq!(lowered[1], "BODY");
        }
    }
}

#[test]
fn one_authored_link_keeps_one_identity_across_committed_and_rejected_fields() {
    // These exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint. Its
    // mdoc_html.c::mdoc_lk_pre() opens one anchor per Lk; term.c::term_flushln()
    // may accept X before a later field returns nbr=0, but cannot create a
    // second source link or revoke the accepted prefix.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for style in ["tag", "hang"] {
        for (label, expected) in [(r"X\p Y", "X\nY"), (r#"X\p "\p Y""#, "X\n")] {
            let source = format!(
                "{prefix}.Bl -{style} -width 4n\n.It Xo\n.Lk https://example.com {label}\n.Xc\n.No BODY\n.El\n"
            );
            let native = without_line_indentation(&native_terminal(&source));
            assert!(native.contains("BODY"), "CVS {style} {label}: {native:?}");
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let item = first_definition_item(query.document.as_ref().unwrap());
            let links = item
                .terms
                .iter()
                .flatten()
                .filter_map(|inline| match inline {
                    Inline::Link { children, .. } => Some(children),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(links.len(), 1, "{style} {label}: {item:?}");
            assert_eq!(inline_text(links[0]), expected, "{style} {label}: {item:?}");
        }
    }

    let prose = format!("{prefix}.Lk https://example.com X\\p Y\n");
    let query = mant_loader::load_roff_bytes(prose.as_bytes()).unwrap();
    let paragraph_links = query.document.as_ref().unwrap().sections[1]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(
                children
                    .iter()
                    .filter(|inline| matches!(inline, Inline::Link { .. }))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(paragraph_links, 1, "single CVS mdoc_html.c anchor");

    // Two distinct Lk source nodes with the same target remain two links.
    // Fixed CVS mdoc_html.c opens two anchors for this exact lint-clean input.
    let distinct =
        format!("{prefix}.Lk https://example.com first\n.Lk https://example.com second\n");
    let query = mant_loader::load_roff_bytes(distinct.as_bytes()).unwrap();
    let distinct_links = query.document.as_ref().unwrap().sections[1]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(
                children
                    .iter()
                    .filter(|inline| matches!(inline, Inline::Link { .. }))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(distinct_links, 2);
}

#[test]
fn leading_word_end_break_rejects_a_link_label_field_without_graph() {
    // Both exact TAG/HANG inputs passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_word() writes the separator after the empty \p word;
    // term_fill() then returns nbr=0 before it reaches QAXAQ's graph.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for style in ["tag", "hang"] {
        let source = format!(
            "{prefix}.Bl -{style} -width 4n\n.It Xo\n.Lk https://example.com \\p QAXAQ\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let item = first_definition_item(query.document.as_ref().unwrap());
        let term = item
            .terms
            .iter()
            .map(|part| inline_text(part))
            .collect::<String>();
        assert!(
            native.contains("BODY") && !native.contains("QAXAQ"),
            "CVS {style}: {native:?}"
        );
        assert!(!term.contains("QAXAQ"), "{style}: {item:?}");
        assert!(
            lowered_terminal(&source).contains("BODY"),
            "{style}: {item:?}"
        );
    }
}

#[test]
fn pending_zero_advance_graph_is_not_discarded_with_its_hang_field() {
    // Exact TAG/HANG inputs passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::encode1() writes the graph before BACKBEFORE delays its IR
    // glyph; term_fill() therefore sees X before the later empty word.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for style in ["hang", "tag"] {
        for (word, expected_term) in [(r"\zX\p", "X\nZ"), (r"\zX", "X Z"), (r"\z", "Z")] {
            let source = format!(
                "{prefix}.Bl -{style} -width 4n\n.It Xo\n.No {word}\n.No \"\"\n.No Z\n.Xc\n.No BODY\n.El\n"
            );
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let item = first_definition_item(query.document.as_ref().unwrap());
            let term = item
                .terms
                .iter()
                .map(|part| inline_text(part))
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                term.contains('Z') && (word != r"\zX\p" || term.contains('X')),
                "{style} {word}: {item:?}"
            );
            let native = without_line_indentation(&native_terminal(&source));
            assert!(
                native.contains(expected_term),
                "CVS {style} {word}: {native:?}"
            );
            let lowered = lowered_terminal(&source);
            assert!(lowered.contains('Z'), "{style} {word}: {lowered:?}");
        }
        for middle in [".No \"\"", r".No \fB", r".No \&", r".No \~", r".No \0"] {
            let source = format!(
                "{prefix}.Bl -{style} -width 4n\n.It Xo\n.No \\zX\\p\n{middle}\n.No Z\n.Xc\n.No BODY\n.El\n"
            );
            let native = without_line_indentation(&native_terminal(&source));
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let item = first_definition_item(query.document.as_ref().unwrap());
            let term = item
                .terms
                .iter()
                .map(|part| inline_text(part))
                .collect::<Vec<_>>()
                .join(" ");
            let normalized = native
                .replace('\u{a0}', " ")
                .lines()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                normalized.contains("X\n") && normalized.contains('Z'),
                "CVS {style} {middle}: {native:?}"
            );
            assert!(
                term.contains('X') && term.contains('Z'),
                "{style} {middle}: {item:?}"
            );
        }
        let source = format!(
            "{prefix}.Bl -{style} -width 4n\n.It Xo\n.No \\z\\p\n.No \"\"\n.No Z\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let item = first_definition_item(query.document.as_ref().unwrap());
        let term = item
            .terms
            .iter()
            .map(|part| inline_text(part))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!native.contains('Z'), "CVS {style}: {native:?}");
        assert!(!term.contains('Z'), "{style}: {item:?}");
    }
}

#[test]
fn discarded_field_cannot_revoke_a_committed_styled_or_linked_prefix() {
    // All exact variants passed fixed CVS -Tutf8/-Tlint. term_flushln()
    // emits the accepted X slice before the next field's leading \p makes
    // term_fill() return nbr=0; wrappers do not change this commit order.
    let prefix = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo\n";
    for first in ["No", "Em", "Sy", "Li"] {
        for second in ["No", "Em", "Sy", "Li", "Lk https://example.com"] {
            let second = format!(".{second} \"\\p Y\"");
            let source = format!("{prefix}.{first} X\\p\n{second}\n.Xc\n.No BODY\n.El\n");
            let native = native_terminal(&source);
            let lowered = lowered_terminal(&source);
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let item = first_definition_item(query.document.as_ref().unwrap());
            let term = item
                .terms
                .iter()
                .map(|part| inline_text(part))
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                native.contains('X') && !native.contains("Y BODY"),
                "CVS {first} {second}: {native:?}"
            );
            assert!(
                term.contains('X') && !term.contains('Y'),
                "{first} {second}: {item:?}"
            );
            assert!(lowered.contains('X'), "{first} {second}: {lowered:?}");
        }
    }
}

#[test]
fn tag_head_post_and_inset_body_own_distinct_physical_rows() {
    // Both exact inputs passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // mdoc_term.c::termp_it_post() closes TAG HEAD after its inner .br;
    // a later BODY \& and .br close a new row, never the old HEAD row.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    let tag =
        format!("{prefix}.Bl -tag -width 4n\n.It Xo X\n.br\n.Xc\n.No \\&\n.br\n.No BODY\n.El\n");
    let native = without_line_indentation(&native_terminal(&tag));
    let lowered = lowered_terminal(&tag)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(native.contains("X\n\nBODY"), "CVS: {native:?}");
    assert!(lowered.contains("X\n\nBODY"), "IR: {lowered:?}");

    let inset = format!(
        "{prefix}.nf\n.Bl -inset\n.It Xo first\nsecond\n.Xc\n.No \\&\n.br\n.No BODY\n.El\n.fi\n"
    );
    let native = without_line_indentation(&native_terminal(&inset));
    let lowered = lowered_terminal(&inset)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        native.contains("first\nsecond\n\n\nBODY"),
        "CVS: {native:?}"
    );
    assert!(
        lowered.contains("first\nsecond\n\n\nBODY"),
        "IR: {lowered:?}"
    );

    for (body, blank_rows) in [
        (".No \\& No \"\"\n.br\n.No BODY\n", 2),
        (".No \\&\n.sp 1\n.No BODY\n", 3),
        (".No \"\"\n.br\n.No BODY\n", 1),
    ] {
        let source =
            format!("{prefix}.nf\n.Bl -inset\n.It Xo first\nsecond\n.Xc\n{body}.El\n.fi\n");
        let rows = |text: String| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
        let native = rows(without_line_indentation(&native_terminal(&source)));
        let lowered = rows(lowered_terminal(&source));
        let boundary = format!("second{}BODY", "\n".repeat(blank_rows + 1));
        assert!(native.contains(&boundary), "CVS {body}: {native:?}");
        assert!(lowered.contains(&boundary), "IR {body}: {lowered:?}");
    }

    // CVS mdoc_macro.c::blk_exp_close() breaks the intermediate It HEAD
    // when Fo/Fc or Bo/Bc closes there; these are the same ownership event
    // as Xo/Xc, independent of the visible spelling of the HEAD.
    for head in [".It Fo call\n.Fa arg\n.Fc\n", ".It Bo X\n.Bc\n"] {
        let source = format!("{prefix}.nf\n.Bl -inset\n{head}.No \\&\n.br\n.No BODY\n.El\n.fi\n");
        let rows = |text: String| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
        let native = rows(without_line_indentation(&native_terminal(&source)));
        let lowered = rows(lowered_terminal(&source));
        assert!(native.contains("\n\n\nBODY"), "CVS {head}: {native:?}");
        assert!(lowered.contains("\n\n\nBODY"), "IR {head}: {lowered:?}");
    }
}

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
fn run_in_fixed_cells_keep_completed_head_glyph_in_its_term() {
    // Fixed CVS mdoc_term.c::termp_it_pre() sends inset/diag cells through
    // term_word("\\ ") / term_word("\\ \\ "). term.c::encode1() retains a
    // completed nonblank BACKBEFORE glyph under the first escaped space.
    // The raw terminal rows for these exact inputs contain X/Y, while bare
    // or blank \z operands do not contribute a visible HEAD glyph.
    for style in ["inset", "diag"] {
        for (head, expected_term) in [(r"\zX", "X"), (r"x\zY", "xY"), (r"\z", ""), ("\\z ", "")] {
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
            .sections
            .iter()
            .any(|section| section.id == "details")
    );
    assert!(
        document
            .sections
            .iter()
            .any(|section| section.id == "details-2")
    );

    for id in ["details", "details-2"] {
        let mut link = AuthoredSectionLink { id, found: false };
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
    let mut strong_tail = StrongText(false);
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

#[test]
fn invisible_run_in_head_row_has_one_output_owner() {
    // Each exact input was run with the pinned CVS -Tascii/-Tlint. In
    // term.c::term_word(), the second empty word writes a separator cell;
    // \& writes an invisible cell, while one empty word or Ns writes none.
    // term_newln() closes only an occupied cell, and term_vspace() adds its
    // own row. IR term visibility cannot decide whether the row existed.
    let cases = [
        ("pair-br", ".No \"\" No \"\"\n.br", 2),
        ("ignore-br", ".No \\&\n.br", 2),
        ("single-br", ".No \"\"\n.br", 1),
        ("joined-br", ".No \"\" Ns No \"\"\n.br", 1),
        ("pair-sp1", ".No \"\" No \"\"\n.sp 1", 3),
        ("ignore-twice", ".No \\&\n.br\n.No \\&\n.br", 3),
    ];
    for (label, head, expected_newlines) in cases {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -inset\n.It Xo\n{head}\n.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        for (kind, output) in [("native", native), ("lowered", lowered)] {
            let (_, tail) = output.split_once("DESCRIPTION").unwrap();
            let (before_body, _) = tail.split_once("BODY").unwrap();
            assert_eq!(
                before_body.matches('\n').count(),
                expected_newlines,
                "{label} {kind}: {output:?}"
            );
        }
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
fn completed_head_rows_and_body_rows_match_the_pinned_terminal() {
    // All 160 exact documents were checked with the fixed CVS binary using
    // -Tutf8 and -Tlint before recording this matrix. In mdoc_term.c,
    // termp_it_pre() enters the BODY after the HEAD post; roff_term.c's
    // pre_br()/pre_sp() then close only the currently occupied device row.
    // A source blank made only of breakable spaces is discarded by
    // term.c::term_fill(), whereas \~ is a fixed cell on the UTF-8 device.
    fn rows_before_body(output: &str) -> usize {
        let description = output
            .split_once("DESCRIPTION")
            .expect("rendered section heading")
            .1;
        description
            .lines()
            .take_while(|line| !line.contains("BODY"))
            .count()
    }
    for style in ["inset", "diag", "hang -width 4n", "tag -width 4n", "ohang"] {
        for label in ["X", "\" \"", r"\~", r"\&"] {
            for head_request in [".br", ".sp 0", ".sp 1", ".sp 2"] {
                for body_request in [".br", ".sp 1"] {
                    let source = format!(
                        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo {label}\n{head_request}\n.Xc\n{body_request}\n.No BODY\n.El\n"
                    );
                    let native = native_terminal(&source);
                    let lowered = lowered_terminal(&source);
                    assert_eq!(
                        rows_before_body(&lowered),
                        rows_before_body(&native),
                        "{style}, {label}, {head_request}, {body_request}: native={native:?}, lowered={lowered:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn empty_formatter_words_after_a_break_occupy_their_new_row() {
    // Both complete inputs passed fixed CVS -Tascii/-Tutf8/-Tlint.
    // term.c::term_word() writes the second empty word's separator even
    // though the last visible character belongs to the preceding row;
    // roff_term.c::pre_br()/pre_sp() then close that occupied row.
    for request in [".br", ".sp 0"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.No X\n.br\n.No \"\" No \"\"\n{request}\n.No BODY\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        for (kind, output) in [("native", native), ("lowered", lowered)] {
            assert!(output.contains("X\n\nBODY"), "{request} {kind}: {output:?}");
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
        assert!(lowered.contains(expected_tail), "{field}: {lowered:?}");
    }
    // Exact fixed CVS -Tascii/-Tutf8/-Tlint probe: ASCII wraps at the
    // invisible \: breakpoint, whereas UTF-8 keeps this short field on one
    // line. The source-neutral projection cannot prove the final column and
    // therefore preserves the BODY word boundary in either presentation.
    let source = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No YYYYY\\:Z\n.Xc\n.No BODY\n.El\n";
    let lowered = lowered_terminal(source);
    assert!(lowered.contains("YYYYYZ BODY"), "{lowered:?}");
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

#[test]
fn a_hang_field_discarded_by_word_end_break_does_not_export_its_projection() {
    // Each exact input passed fixed CVS -Tutf8/-Tlint. term.c::term_fill()
    // returns nbr=0 when \p precedes a breakable blank before the field's
    // first graph. This also applies within one quoted TEXT and when HEAD
    // post, rather than .br, settles the field. An emitted Link target may
    // remain in IR, but neither its label nor its buffered line break prints.
    for field in [
        ".No \"\\p Y\"\n.br\n",
        ".No \\p\n.No Y\n.No Z\n",
        ".No \\p\n.Lk https://example.com visible\n.br\n",
        ".No \\p\n.Lk https://example.com \"\"\n.br\n",
        ".Lk https://example.com \"\\p Y\"\n.br\n",
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo\n.No X\n.br\n{field}.Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source));
        let lowered = lowered_terminal(&source);
        assert!(native.contains("X     BODY"), "{field}: {native:?}");
        assert!(lowered.contains("X     BODY"), "{field}: {lowered:?}");
    }
    let source = concat!(
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n",
        ".Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo\n.No X\n.br\n",
        ".No \\p\n.No Y\n.sp 1\n.Xc\n.No BODY\n.El\n",
    );
    // term_vspace() emits a real row after discarding the pending field.
    let native = without_line_indentation(&native_terminal(source));
    let lowered = lowered_terminal(source);
    assert!(native.contains("X\nBODY"), "native: {native:?}");
    assert!(
        lowered.contains("X     \n      BODY"),
        "lowered: {lowered:?}"
    );
}

#[test]
fn tag_and_hang_fields_share_the_native_word_end_and_graph_rules() {
    // Both exact inputs passed fixed CVS -Tutf8/-Tlint. mdoc_term.c gives
    // TAG and HANG the same NOBREAK field execution; term.c::term_fill()
    // discards a field broken before its first graph, but treats a literal
    // TAB as graph rather than an ordinary breakable space.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    let tag = format!(
        "{prefix}.Bl -tag -width 4n\n.It Xo X\n.br\n.No \\p\n.No Y\n.br\n.Xc\n.No BODY\n.El\n"
    );
    let native = without_line_indentation(&native_terminal(&tag));
    let lowered = lowered_terminal(&tag);
    assert!(native.contains("X\nBODY"), "native TAG: {native:?}");
    let lowered_rows = lowered.lines().map(str::trim).collect::<Vec<_>>();
    assert!(
        lowered_rows.windows(2).any(|rows| rows == ["X", "BODY"]),
        "lowered TAG: {lowered:?}"
    );

    let tab = format!(
        "{prefix}.nf\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No \"\\p\t\"\n.No Y\n.br\n.Xc\n.No BODY\n.El\n.fi\n"
    );
    let native = native_terminal(&tab);
    let lowered = lowered_terminal(&tab);
    assert!(native.contains("X     Y"), "native TAB: {native:?}");
    assert!(
        lowered
            .lines()
            .any(|line| line.contains('X') && line.contains('Y')),
        "lowered TAB: {lowered:?}"
    );
}

#[test]
fn discarded_tag_head_buffer_preserves_prior_rows_and_waits_for_real_flush() {
    // All three exact inputs passed fixed CVS -Tutf8/-Tlint. term_fill()
    // discards the entire pending buffer after a leading \p and separator;
    // a later term_newln() releases subsequent words but never erases rows
    // already completed by roff_term_pre_br()/pre_sp().
    let prefix = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo X\n";
    for (tail, expected_rows) in [
        (".br\n.No \\p\n.No Y\n.No Z\n", vec!["X", "BODY"]),
        (".br\n.No \\p\n.No Y\n.br\n.No Z\n", vec!["X", "Z", "BODY"]),
        (".sp 1\n.No \\p\n.No Y\n.br\n", vec!["X", "", "BODY"]),
    ] {
        let source = format!("{prefix}{tail}.Xc\n.No BODY\n.El\n");
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        let rows = |value: &str| {
            value
                .split("DESCRIPTION\n")
                .nth(1)
                .unwrap()
                .lines()
                .map(str::trim)
                .take_while(|row| *row != "Linux 6.18.33.2-microsoft-standard-WSL2")
                .filter(|row| !row.is_empty() || expected_rows.contains(&""))
                .take(expected_rows.len())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&native), expected_rows, "native {tail}: {native:?}");
        assert_eq!(rows(&lowered), expected_rows, "lowered {tail}: {lowered:?}");
    }
    let long_tag = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo XXXXXX\n.br\n.No \\p\n.No Y\n.br\n.Xc\n.No BODY\n.El\n";
    let native = native_terminal(long_tag);
    let lowered = lowered_terminal(long_tag);
    assert!(native.contains("XXXXXX\n\n"), "native: {native:?}");
    assert!(lowered.contains("XXXXXX\n\n"), "lowered: {lowered:?}");
}

#[test]
fn discarded_terminal_fields_keep_authored_link_destinations() {
    // Each exact input passed fixed CVS -Tutf8/-Thtml/-Tlint. term_fill()
    // discards the visible NOBREAK field, while mdoc_html.c still emits the
    // Lk/Mt href from the authored operand. Preserve the typed IR target.
    for (link, target) in [
        (
            ".Lk https://example.com visible",
            mant_ir::LinkTarget::External {
                uri: "https://example.com".to_owned(),
            },
        ),
        (
            ".Lk https://example.com",
            mant_ir::LinkTarget::External {
                uri: "https://example.com".to_owned(),
            },
        ),
        (
            ".Mt user@example.com",
            mant_ir::LinkTarget::Email {
                address: "user@example.com".to_owned(),
            },
        ),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -hang -width 4n\n.It Xo X\n.br\n.No \\p\n{link}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        assert!(native.contains("X     BODY"), "{link}: {native:?}");
        assert!(lowered.contains("X     BODY"), "{link}: {lowered:?}");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let item = first_definition_item(query.document.as_ref().unwrap());
        let targets = item
            .terms
            .iter()
            .flatten()
            .filter_map(|inline| match inline {
                Inline::Link { target, .. } => Some(target),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, vec![&target], "{link}: {item:?}");
    }
}

#[test]
fn blank_link_heads_do_not_claim_a_printed_definition_row() {
    // Each exact input passed fixed CVS -Tascii/-Tutf8/-Tlint. A field made
    // solely of ordinary spaces is discarded by term.c::term_fill(), even
    // when its IR node is a link; a real .Lk target remains visible.
    for head in [
        ".Sx \" \"",
        ".Mt \" \"",
        ".Lk \" \"",
        ".Lk https://example.com \" \"",
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n{head}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        let rows_before_body = |output: &str| {
            output
                .split_once("DESCRIPTION")
                .unwrap()
                .1
                .lines()
                .take_while(|line| !line.contains("BODY"))
                .count()
        };
        assert_eq!(
            rows_before_body(&lowered),
            rows_before_body(&native),
            "{head}: native={native:?}, lowered={lowered:?}"
        );
        if head.starts_with(".Lk https") {
            assert!(lowered.contains("https://example.com"), "{lowered:?}");
        }
    }
}

#[test]
fn styled_breakable_only_head_does_not_create_a_device_row() {
    // These exact tag heads passed fixed CVS -Tascii/-Tutf8/-Tlint. The
    // renderer calls term_fill() after the style macros, so ordinary spaces
    // alone still print no field. A styled \~ remains a fixed UTF-8 cell.
    for macro_name in ["No", "Em", "Sy", "Li"] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n.{macro_name} \" \"\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        let native_section = native.split_once("DESCRIPTION").unwrap().1;
        let lowered_section = lowered.split_once("DESCRIPTION").unwrap().1;
        assert_eq!(
            native_section
                .lines()
                .take_while(|line| !line.contains("BODY"))
                .count(),
            lowered_section
                .lines()
                .take_while(|line| !line.contains("BODY"))
                .count(),
            "{macro_name}: native={native:?}, lowered={lowered:?}"
        );
    }
}
