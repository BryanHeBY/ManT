use super::*;

#[test]
fn rejected_link_fields_keep_the_accepted_head_and_its_body_position() {
    // All five exact inputs ran pristine ASCII/UTF-8/HTML/tree/lint before
    // this assertion. term_field() advances X and term_flushln() preserves
    // its device position through rejected label fields; BODY prints at
    // the six-column origin (term.c:113-116,233-253,397-427). The public
    // layout may express this with body origin or a minimum gap; assert
    // the final consumer's actual cells instead of either private choice.
    let prefix =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for link in [
        ".Lk https://example.com visible",
        ".Lk https://example.com",
        ".Lk https://example.com first\n.Lk https://example.com second",
        r#".Lk "" visible"#,
        r".Lk \zX visible",
    ] {
        let source = format!(
            "{prefix}.Bl -hang -width 4n\n.It Xo X\n.br\n.No \\p\n{link}\n.br\n.Xc\n.No BODY\n.El\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let json = mant_render::render_query_json(&loaded, false).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
        let query: mant_ir::ResolvedContent = restored.into();
        let output = mant_render::render_query_man(&query);
        let body = output.split_once("DESCRIPTION\n").unwrap().1;
        assert_eq!(body, "X     BODY", "{source}");
    }
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
    // term_field() printed X before the later field was rejected. Its
    // unprinted positioning cells are not trailing X text (term.c:389-427).
    assert!(lowered.contains("X\n      BODY"), "lowered: {lowered:?}");
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
    // All four complete framed inputs ran pristine in all five profiles.
    // term_fill()
    // discards the entire pending buffer after a leading \p and separator;
    // a later term_newln() releases subsequent words but never erases rows
    // already completed by roff_term_pre_br()/pre_sp().
    let prefix = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo X\n";
    for (tail, expected_rows) in [
        (".br\n.No \\p\n.No Y\n.No Z\n", vec!["X", "BODY"]),
        (".br\n.No \\p\n.No Y\n.br\n.No Z\n", vec!["X", "Z", "BODY"]),
        (".sp 1\n.No \\p\n.No Y\n.br\n", vec!["X", "", "BODY"]),
    ] {
        let source = format!("{prefix}{tail}.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n");
        let native = native_terminal(&source);
        let lowered = lowered_terminal(&source);
        assert_eq!(
            framed_definition_rows(&native),
            expected_rows,
            "native {tail}: {native:?}"
        );
        assert_eq!(
            framed_definition_rows(&lowered),
            expected_rows,
            "lowered {tail}: {lowered:?}"
        );
    }
    let long_tag = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo XXXXXX\n.br\n.No \\p\n.No Y\n.br\n.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n";
    let native = native_terminal(long_tag);
    let lowered = lowered_terminal(long_tag);
    // Exact pristine source run retains one physical blank row. The IR
    // may attach a line-origin hint to it; padding does not make it another
    // row or printable word (term.c::term_flushln(), roff_term_pre_br()).
    assert_eq!(
        framed_definition_rows(&native),
        ["XXXXXX", "", "BODY"],
        "native: {native:?}"
    );
    assert_eq!(
        framed_definition_rows(&lowered),
        ["XXXXXX", "", "BODY"],
        "lowered: {lowered:?}"
    );
}
