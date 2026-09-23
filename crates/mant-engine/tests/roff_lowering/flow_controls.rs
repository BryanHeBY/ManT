//! Native paragraph, literal-line and styling-container flow contracts.

// A literal display can contain separate literal runs and executed spacing
// requests. Verify their complete row stream rather than requiring one block
// (which would erase the distinction needed for bounded request accounting).
fn literal_flow(content: mant_ir::ContentContext<'_>, blocks: &[mant_ir::Block]) -> String {
    let mut output = String::new();
    let mut occupied = false;
    let mut gap = 0usize;
    for block in blocks {
        match block {
            mant_ir::Block::VerticalSpace { lines, .. } => gap += usize::from(*lines),
            mant_ir::Block::Preformatted {
                children, layout, ..
            } => {
                gap += usize::from(layout.spacing_before_lines);
                if occupied {
                    output.push('\n');
                }
                output.push_str(&"\n".repeat(gap));
                output.push_str(&super::inline_text(content, children));
                gap = 0;
                occupied = true;
            }
            other => panic!("unexpected block in literal flow: {other:?}"),
        }
    }
    output.push_str(&"\n".repeat(gap));
    output
}

fn assert_markdown_literal_rows(query: &mant_ir::ResolvedContent, expected: &str) {
    use mant_ir::visit::{Visit, walk_block};
    struct LiteralRows<'a>(Vec<String>, mant_ir::ContentContext<'a>);
    impl<'a> Visit<'a> for LiteralRows<'a> {
        fn visit_block(&mut self, block: &'a mant_ir::Block) {
            if let mant_ir::Block::Preformatted { children, .. } = block {
                self.0.extend(
                    super::inline_text(self.1, children)
                        .split('\n')
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned),
                );
            }
            walk_block(self, block);
        }
    }
    let markdown = mant_codec::encode::render_markdown(query).expect("valid Flow export");
    let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let document = reloaded.document.as_ref().unwrap();
    let mut rows = LiteralRows(Vec::new(), document.content());
    rows.visit_document(document);
    // Markdown owns fence separators: rereading may normalize inter-fence
    // gaps, but it must retain every visible literal row in its exact order.
    assert_eq!(
        rows.0.join("\n"),
        expected
            .split('\n')
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        "{markdown}"
    );
}

#[test]
fn ordinary_man_paragraphs_reset_prevailing_definition_width() {
    use mant_ir::visit::{Visit, walk_definition_item};
    struct Widths(Vec<bool>);
    impl<'a> Visit<'a> for Widths {
        fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
            self.0.push(item.layout.inline_term);
            walk_definition_item(self, item);
        }
    }
    for (boundary, inline_second) in [("PP", false), ("P", false), ("LP", false), ("HP", true)] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.TP 15\nFIRSTLONGTAG\nFIRST\n.{boundary}\nBETWEEN\n.TP\nSECONDLONGTAG\nSECOND\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let mut widths = Widths(Vec::new());
        widths.visit_document(query.document.as_ref().unwrap());
        assert_eq!(widths.0, [true, inline_second], "{source}");
        let text = mant_render::render_query_text(&query);
        assert!(
            text.lines().any(|line| line == "FIRSTLONGTAG   FIRST"),
            "{text}"
        );
        assert_eq!(
            text.lines()
                .any(|line| line.contains("SECONDLONGTAG") && line.ends_with("SECOND")),
            inline_second,
            "{text}"
        );
    }
}

#[test]
fn leading_word_end_break_survives_a_vertical_space_block_split() {
    for (escape, spacing) in [
        (r"\p", ".sp 1"),
        (r"\p\c", ".sp 1"),
        (r"\p", ".sp bogus"),
        (r"\p\c", ".sp bogus"),
        (r"\z\p", ".sp 1"),
        (r"\z\p\c", ".sp bogus"),
        (r"\z\c\p", ".sp 1"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {escape}\n{spacing}\n.No B C\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let rendered = mant_render::render_query_text(&query);
        assert!(
            rendered.ends_with("DESCRIPTION\n\n\nB C"),
            "{source}: {rendered:?}"
        );
    }

    for escape in [r"\p", r"\p\c"] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {escape}\n.Sh NEXT\n.No C\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let rendered = mant_render::render_query_text(&query);
        assert!(
            rendered.contains("DESCRIPTION\n\n\nNEXT"),
            "{source}: {rendered:?}"
        );
    }
}

#[test]
fn negative_vertical_space_debt_is_bounded_and_resets_on_words() {
    fn rendered_body(body: &str) -> String {
        let source = format!(".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n{body}\n");
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        mant_render::render_query_text(&query)
    }

    for (label, body, expected) in [
        ("top-level", ".No A\n.sp -1\n.sp 1\n.No B", "A\nB"),
        ("accumulated", ".No A\n.sp -2\n.sp 1\n.sp 1\n.No B", "A\nB"),
        (
            "literal-display",
            ".Bd -literal\n.No A\n.sp -1\n.sp 1\n.No B\n.Ed",
            "A\nB",
        ),
        (
            "inline-container",
            ".Eo [\n.No A\n.sp -1\n.sp 1\n.No B\n.Ec",
            "[A\nB",
        ),
    ] {
        let rendered = rendered_body(body);
        assert!(rendered.contains(expected), "{label}: {rendered:?}");
        assert!(
            !rendered.contains(&expected.replace('\n', "\n\n")),
            "{label}: {rendered:?}"
        );
    }

    // CVS `term_word()` clears skip-vspace debt.  A word between the
    // negative and positive requests therefore makes the later gap visible.
    let reset = rendered_body(".sp -1\n.No A\n.sp 1\n.No B");
    assert!(reset.contains("A\n\nB"), "{reset:?}");

    for display in ["literal", "unfilled"] {
        let carried = rendered_body(&format!(
            ".No BEFORE\n.sp -1\n.Bd -{display}\n.sp 1\n.No AFTER\n.Ed"
        ));
        assert!(
            carried.contains("BEFORE\n\nAFTER"),
            "{display}: {carried:?}"
        );
        assert!(
            !carried.contains("BEFORE\n\n\nAFTER"),
            "{display}: {carried:?}"
        );
    }

    for (label, transparent) in [("font", ".Bf -emphasis\n.Ef"), ("keep", ".Bk -words\n.Ek")] {
        let carried = rendered_body(&format!(".No A\n.sp -1\n{transparent}\n.sp 1\n.No B"));
        assert!(carried.contains("A\nB"), "{label}: {carried:?}");
        assert!(!carried.contains("A\n\nB"), "{label}: {carried:?}");
    }

    let reset_inside_display = rendered_body(".sp -1\n.Bd -literal\n.No A\n.sp 1\n.No B\n.Ed");
    assert!(
        reset_inside_display.contains("A\n\nB"),
        "{reset_inside_display:?}"
    );

    // man no-fill owns a separate presentation buffer, but its words still
    // execute on the same formatter and clear CVS `skipvsp`.
    let source = b".TH PROBE 1\n.SH DESCRIPTION\n.nf\n.sp -1\nWORD\n.sp 1\nAFTER\n.fi\n";
    let rendered = mant_render::render_query_text(&mant_loader::load_roff_bytes(source).unwrap());
    assert!(rendered.contains("WORD\n\nAFTER"), "{rendered:?}");
}

#[test]
fn zero_width_cells_trigger_real_formatter_boundaries() {
    for (dialect, prefix, word) in [
        (
            "mdoc",
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\&",
            ".No B",
        ),
        ("man", ".TH PROBE 1\n.SH DESCRIPTION\n\\&", "B"),
    ] {
        for (request, expected) in [
            (".ti 4n", "DESCRIPTION\n\nB"),
            (".br", "DESCRIPTION\n\nB"),
            (".sp 0", "DESCRIPTION\n\nB"),
            (".sp 1", "DESCRIPTION\n\n\nB"),
            (".sp 2", "DESCRIPTION\n\n\n\nB"),
        ] {
            let source = format!("{prefix}\n{request}\n{word}\n");
            let rendered = mant_render::render_query_text(
                &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
            );
            assert!(
                rendered.contains(expected),
                "{dialect} {request}: {rendered:?}"
            );
        }

        // `.mc` flushes the occupied zero-width field without ending the
        // visual row. CVS `term_flushln()` commits one field separator before
        // the following word even though `\&` has no visible glyph.
        let source = format!("{prefix}\n.mc |\n{word}\n");
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        assert!(
            rendered.contains("DESCRIPTION\n B"),
            "{dialect}: {rendered:?}"
        );
        assert!(
            !rendered.contains("DESCRIPTION\n  B"),
            "{dialect}: {rendered:?}"
        );
        assert!(
            !rendered.contains("DESCRIPTION\n\n"),
            "{dialect}: {rendered:?}"
        );

        // A row marker and a pending word-end break occupy the same native
        // formatter cell. A boundary flushes that cell once; positive `.sp`
        // then contributes its own requested rows.
        for controls in ["\\&\\p", "\\&\\z\\p"] {
            for (request, expected) in [
                (".br", "DESCRIPTION\n\nB"),
                (".sp 0", "DESCRIPTION\n\nB"),
                (".sp 1", "DESCRIPTION\n\n\nB"),
                (".sp 2", "DESCRIPTION\n\n\n\nB"),
            ] {
                let controlled_prefix = if dialect == "mdoc" {
                    format!(
                        ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No {controls}"
                    )
                } else {
                    format!(".TH PROBE 1\n.SH DESCRIPTION\n{controls}")
                };
                let source = format!("{controlled_prefix}\n{request}\n{word}\n");
                let rendered = mant_render::render_query_text(
                    &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
                );
                assert!(
                    rendered.contains(expected),
                    "{dialect} {controls} {request}: {rendered:?}"
                );
            }
        }
    }
}

#[test]
fn mdoc_definition_terms_share_only_their_native_pending_row() {
    for style in ["tag", "diag", "hang", "inset", "ohang"] {
        for (request, requested_rows) in [(".br", 0), (".sp 0", 0), (".sp 1", 1), (".Pp", 1)] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style} -width Ds\n.It x\n.No \\&\n.Tg item-gap\n{request}\n.No BODY\n.El\n"
            );
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let rendered = mant_render::render_query_text(&query);
            let lines = rendered.lines().collect::<Vec<_>>();
            let head = lines
                .iter()
                .position(|line| line.trim_start().starts_with('x'))
                .unwrap();
            let body = lines
                .iter()
                .enumerate()
                .skip(head + 1)
                .find_map(|(index, line)| (line.trim() == "BODY").then_some(index))
                .unwrap();
            let term_already_stacked = usize::from(style == "ohang");
            assert_eq!(
                body - head - 1,
                term_already_stacked + requested_rows,
                "{style} {request}: {rendered:?}"
            );
            assert!(
                serde_json::to_string(query.document.as_ref().unwrap())
                    .unwrap()
                    .contains("item-gap"),
                "{style} {request}"
            );
        }

        for (request, requested_rows) in [
            (".br", 0),
            (".ti 4n", 0),
            (".fi", 0),
            (".sp 0", 0),
            (".sp 1", 1),
            (".Pp", 1),
        ] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style} -width Ds\n.It x\n{request}\n.No BODY\n.El\n"
            );
            let rendered = mant_render::render_query_text(
                &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
            );
            let lines = rendered.lines().collect::<Vec<_>>();
            let head = lines
                .iter()
                .position(|line| line.trim_start().starts_with('x'))
                .unwrap();
            let body = lines
                .iter()
                .enumerate()
                .skip(head + 1)
                .find_map(|(index, line)| (line.trim() == "BODY").then_some(index))
                .unwrap();
            assert_eq!(
                body - head - 1,
                requested_rows,
                "plain {style} {request}: {rendered:?}"
            );
        }

        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style} -width Ds\n.It x\n.No \\&\\p\n.No BODY\n.El\n"
        );
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        let lines = rendered.lines().collect::<Vec<_>>();
        let head = lines
            .iter()
            .position(|line| line.trim_start().starts_with('x'))
            .unwrap();
        let body = lines
            .iter()
            .enumerate()
            .skip(head + 1)
            .find_map(|(index, line)| (line.trim() == "BODY").then_some(index))
            .unwrap();
        assert_eq!(
            body - head - 1,
            usize::from(style == "ohang"),
            "word-end {style}: {rendered:?}"
        );
    }
}

#[test]
fn no_fill_control_only_cells_survive_mode_and_spacing_boundaries() {
    for (label, request, expected) in [
        ("fill", ".fi", "DESCRIPTION\n\nAFTER"),
        ("break", ".br", "DESCRIPTION\n\nAFTER"),
        ("space", ".sp 1", "DESCRIPTION\n\n\nAFTER"),
    ] {
        let source = format!(".TH PROBE 1\n.SH DESCRIPTION\n.nf\n\\p\n{request}\nAFTER\n.fi\n");
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
        );
        assert!(rendered.contains(expected), "{label}: {rendered:?}");
    }

    // A zero-width glyph occupies and flushes the old cell, so a trailing
    // bare `\z` cannot cross the structural display boundary.
    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No \\&\\z\n.Bd -literal -compact\n.No B C\n.Ed\n";
    let rendered = mant_render::render_query_text(&mant_loader::load_roff_bytes(source).unwrap());
    assert!(rendered.contains("DESCRIPTION\n\nB C"), "{rendered:?}");
    assert!(!rendered.contains("\nBC"), "{rendered:?}");

    // This assertion crosses the IR block boundary and the text renderer;
    // helper projections that concatenate block text cannot prove the row.
    let source = b".TH PROBE 1\n.SH DESCRIPTION\n.nf\nA\\z\n.fi\nB C\n";
    let rendered = mant_render::render_query_text(&mant_loader::load_roff_bytes(source).unwrap());
    assert!(rendered.contains("DESCRIPTION\nA\nB C"), "{rendered:?}");
}

#[test]
fn generated_mdoc_list_markers_clear_negative_vertical_space_debt() {
    fn line_before_after(list: &str) -> String {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.No BEFORE\n.sp -1\n.Bl {list} -compact\n.It\n.sp 1\n.No AFTER\n.El\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let rendered = mant_render::render_query_text(&query);
        let lines = rendered.lines().collect::<Vec<_>>();
        let after = lines
            .iter()
            .position(|line| line.trim() == "AFTER")
            .unwrap_or_else(|| panic!("missing AFTER in {list}: {rendered:?}"));
        lines[after.saturating_sub(1)].to_owned()
    }

    for kind in ["-bullet", "-dash", "-hyphen", "-enum"] {
        assert!(line_before_after(kind).is_empty(), "{kind}");
    }
    assert_eq!(line_before_after("-item"), "BEFORE");

    let source = b".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.sp -1\n.Bl -bullet -compact\n.It\n.Bl -enum -compact\n.It\n.sp 1\n.No AFTER\n.El\n.El\n";
    let query = mant_loader::load_roff_bytes(source).unwrap();
    let rendered = mant_render::render_query_text(&query);
    let lines = rendered.lines().collect::<Vec<_>>();
    let after = lines
        .iter()
        .position(|line| line.trim() == "AFTER")
        .expect("nested marker body");
    assert!(lines[after - 1].is_empty(), "{rendered:?}");
}

#[test]
fn literal_display_controls_preserve_physical_rows_and_continuation() {
    for display in ["literal", "unfilled"] {
        for (body, expected) in [
            ("BEFORE\n.sp 2\nAFTER", "BEFORE\n\n\nAFTER"),
            ("BEFORE\n.sp 1\nAFTER", "BEFORE\n\nAFTER"),
            ("BEFORE\n.sp 0\nAFTER", "BEFORE\nAFTER"),
            ("BEFORE\n.br\nAFTER", "BEFORE\nAFTER"),
            ("BEFORE\n.Sm off\nAFTER", "BEFORE\nAFTER"),
            ("BEFORE\\c\nAFTER", "BEFOREAFTER"),
            ("BEFORE\\c\n.Sm off\nAFTER", "BEFOREAFTER"),
            ("BEFORE\n\nAFTER", "BEFORE\n\nAFTER"),
            ("BEFORE\n.sp 1\n.sp 1\nAFTER", "BEFORE\n\n\nAFTER"),
            ("BEFORE\\c\n.br\nAFTER", "BEFORE\nAFTER"),
            ("\\fBBEFORE\nAFTER\\fR", "BEFORE\nAFTER"),
            (
                ".Bf -emphasis\nBEFORE\n.sp 2\nAFTER\n.Ef",
                "BEFORE\n\n\nAFTER",
            ),
        ] {
            let source = format!(
                ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bd -{display} -offset left\n{body}\n.Ed\n"
            );
            let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
            let document = query.document.as_ref().unwrap();
            assert_eq!(
                literal_flow(
                    document.content(),
                    &document.flow().unwrap().sections[0].blocks
                ),
                expected,
                "{source}"
            );
            assert!(
                mant_render::render_query_text(&query).contains(expected),
                "{source}: {}",
                mant_render::render_query_text(&query)
            );
        }
    }
}

/// A styling container is not a logical-line boundary. These inputs are
/// authored here from that contract, not copied from a formatter's tests.
#[test]
fn literal_continuations_cross_styling_containers_without_phantom_rows() {
    for body in [
        "FIRST\\c\n.Bf -emphasis\nSECOND\\c\n.Ef\nTHIRD",
        "FIRST\\c\n.Bf -emphasis\n.Sm off\n.Ef\nSECOND\\c\nTHIRD",
        ".Bf -emphasis\nFIRST\\c\n.Ef\nSECOND\\c\nTHIRD",
        "FIRST\\c\n.Bf -symbolic\n.Bf -emphasis\nSECOND\\c\n.Ef\n.Ef\nTHIRD",
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt FLOW 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal -offset left\n{body}\n.Ed\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(
            literal_flow(
                query.document.as_ref().unwrap().content(),
                &query.document.as_ref().unwrap().flow().unwrap().sections[0].blocks
            ),
            "FIRSTSECONDTHIRD",
            "{body}"
        );
        assert!(mant_render::render_query_text(&query).contains("FIRSTSECONDTHIRD"));
        assert_markdown_literal_rows(&query, "FIRSTSECONDTHIRD");
    }
}

#[test]
fn styled_literal_breaks_and_eof_keep_exact_content_boundaries() {
    for (body, expected) in [
        (
            "FIRST\\c\n.Bf -emphasis\n.br\nSECOND\n.Ef\nTHIRD",
            "FIRST\nSECOND\nTHIRD",
        ),
        (
            "FIRST\n.Bf -emphasis\nSECOND\n.sp 2\n.Ef\nTHIRD",
            "FIRST\nSECOND\n\n\nTHIRD",
        ),
        (
            "FIRST\n.Bf -emphasis\n.Sm off\n.Ef\nSECOND",
            "FIRST\nSECOND",
        ),
        ("FIRST\n.Bf -emphasis\nSECOND\\c\n.Ef", "FIRST\nSECOND"),
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt FLOW 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal -offset left\n{body}\n.Ed\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(
            literal_flow(
                query.document.as_ref().unwrap().content(),
                &query.document.as_ref().unwrap().flow().unwrap().sections[0].blocks
            ),
            expected,
            "{body}"
        );
        assert!(mant_render::render_query_text(&query).contains(expected));
        assert_markdown_literal_rows(&query, expected);
    }
}

#[test]
fn explicit_literal_breaks_are_not_repeated_at_styling_boundaries() {
    for (control, gap) in [
        (".br", "\n"),
        (".sp 0", "\n"),
        (".sp 1", "\n\n"),
        (".sp 2", "\n\n\n"),
    ] {
        for font in ["emphasis", "literal", "symbolic"] {
            for first in ["FIRST", "FIRST\\c"] {
                for (body, expected) in [
                    (
                        format!("{first}\n{control}\n.Bf -{font}\nSECOND\n.Ef\nTHIRD"),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!("{first}\n.Bf -{font}\n{control}\nSECOND\n.Ef\nTHIRD"),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!(
                            "{first}\n{control}\n.Bf -symbolic\n.Bf -{font}\nSECOND\n.Ef\n.Ef\nTHIRD"
                        ),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!("{first}\n{control}\n.Bf -{font}\n.Ef\nSECOND\nTHIRD"),
                        format!("FIRST{gap}SECOND\nTHIRD"),
                    ),
                    (
                        format!("{first}\n{control}\n.Bf -{font}\nSECOND\\c\n.Ef"),
                        format!("FIRST{gap}SECOND"),
                    ),
                    (
                        format!("FIRST\n.Bf -{font}\nSECOND\n{control}\n.Ef\nTHIRD"),
                        format!("FIRST\nSECOND{gap}THIRD"),
                    ),
                    (
                        format!("FIRST\n.Bf -{font}\nSECOND\n.Ef\n{control}\nTHIRD"),
                        format!("FIRST\nSECOND{gap}THIRD"),
                    ),
                ] {
                    let source = format!(
                        ".Dd September 7, 2026\n.Dt FLOW 1\n.Os\n.Sh DESCRIPTION\n.Bd -literal -offset left\n{body}\n.Ed\n"
                    );
                    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                    let document = query.document.as_ref().unwrap();
                    let flow = literal_flow(
                        document.content(),
                        &document.flow().unwrap().sections[0].blocks,
                    );
                    assert_eq!(flow, expected, "{body}");
                    assert_eq!(
                        flow.matches('\n').count(),
                        expected.matches('\n').count(),
                        "{body}"
                    );
                    assert!(
                        mant_render::render_query_text(&query).contains(&expected),
                        "{body}"
                    );
                    assert_markdown_literal_rows(&query, &expected);
                }
            }
        }
    }
}
