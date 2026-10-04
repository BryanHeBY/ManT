//! Native paragraph, literal-line and styling-container flow contracts.

// A literal display can contain separate literal runs and executed spacing
// requests. Verify their complete row stream rather than requiring one block
// (which would erase the distinction needed for bounded request accounting).
fn literal_flow(blocks: &[mant_ir::Block]) -> String {
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
                output.push_str(&super::inline_text(children));
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
    #[derive(Default)]
    struct LiteralRows(Vec<String>);
    impl<'a> Visit<'a> for LiteralRows {
        fn visit_block(&mut self, block: &'a mant_ir::Block) {
            if let mant_ir::Block::Preformatted { children, .. } = block {
                self.0.extend(
                    super::inline_text(children)
                        .split('\n')
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned),
                );
            }
            walk_block(self, block);
        }
    }
    let markdown = mant_codec::encode::render_markdown(query);
    let reloaded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let mut rows = LiteralRows::default();
    rows.visit_document(reloaded.document.as_ref().unwrap());
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
            self.0.push(item.inline_term());
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
fn mdoc_definition_generated_padding_does_not_count_as_body_content() {
    // mdoc_term.c::termp_it_pre() writes diag/inset padding through term_word(),
    // but that generated whitespace is not BODY content.  The first visible
    // BODY glyph, including a pending \z glyph, is the point at which the
    // definition's pending HEAD row is consumed.  Fixed CVS keeps BODY on the
    // next row after each of these exact operands and .sp 0.
    for style in ["diag", "inset", "tag", "hang"] {
        for operand in [r"\&", "\" \"", r"\fB", r"\zX", "X"] {
            let source = format!(
                ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style} -width Ds\n.It x\n.No {operand}\n.Tg item-gap\n.sp 0\n.No BODY\n.El\n"
            );
            let rendered = mant_render::render_query_text(
                &mant_loader::load_roff_bytes(source.as_bytes()).unwrap(),
            );
            let lines = rendered.lines().collect::<Vec<_>>();
            let head = lines
                .iter()
                .position(|line| line.trim_start().starts_with('x'))
                .unwrap();
            assert_eq!(
                lines[head + 1].trim(),
                "BODY",
                "{style} {operand}: {rendered:?}"
            );
            assert_eq!(
                lines[head].contains('X'),
                matches!(operand, r"\zX" | "X"),
                "{style} {operand}: {rendered:?}"
            );
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
fn man_definition_body_uses_actual_break_and_no_break_requests() {
    // Both exact inputs were run through the fixed CVS -Tutf8/-Tlint oracle.
    // man_term.c::pre_TP/pre_IP leave the tag field active; term.c settles
    // the deferred word-end break at the real row or TERMP_NOBREAK flush.
    for (name, source, same_row) in [
        (
            "tp-pending-row",
            b".TH PROBE 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.TP\nkey\n\\&\\p\nBODY\n"
                .as_slice(),
            false,
        ),
        (
            "ip-no-break",
            b".TH PROBE 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.IP key\n\\&\\p\n.mc |\nBODY\n"
                .as_slice(),
            true,
        ),
    ] {
        let query = mant_loader::load_roff_bytes(source).expect("lower man definition");
        let rendered = mant_render::render_query_text(&query);
        let lines = rendered.lines().collect::<Vec<_>>();
        let key_line = lines
            .iter()
            .position(|line| line.trim_start().starts_with("key"))
            .expect("term row");
        if same_row {
            assert!(lines[key_line].contains("BODY"), "{name}: {rendered:?}");
        } else {
            assert!(!lines[key_line].contains("BODY"), "{name}: {rendered:?}");
            assert_eq!(
                lines.get(key_line + 1).copied().map(str::trim),
                Some("BODY"),
                "{name}: {rendered:?}"
            );
        }
    }
}

#[test]
fn author_split_in_definition_body_uses_the_executed_line_boundary() {
    // Exact source checked with fixed CVS -Tutf8/-Tlint. In mdoc_term.c,
    // An's pre-handler calls term_newln() after -split before the first name.
    let source = b".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd definition boundary\n.Sh AUTHORS\n.Bl -tag -width Ds\n.It key\n.An -split\n.An Ada\n.An Babbage\n.El\n";
    let query = mant_loader::load_roff_bytes(source).expect("lower AUTHORS definition");
    let rendered = mant_render::render_query_text(&query);
    let lines = rendered.lines().collect::<Vec<_>>();
    let key = lines
        .iter()
        .position(|line| line.trim() == "key")
        .expect("term row");
    assert_eq!(
        lines.get(key + 1).copied().map(str::trim),
        Some("Ada"),
        "{rendered:?}"
    );
    assert_eq!(
        lines.get(key + 2).copied().map(str::trim),
        Some("Babbage"),
        "{rendered:?}"
    );
}

#[test]
fn nested_definition_content_closes_the_outer_pending_head_row() {
    // Both exact sources were checked with fixed CVS -Tutf8/-Tlint.
    // mdoc_term.c::termp_it_pre() executes nested items and bullet markers as
    // formatter content before the following text/control-only row.
    let nested_definition = b".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd nested definition row\n.Sh DESCRIPTION\n.Bl -tag -width outer\n.It outer\n.Bl -tag -width inner\n.It inner\ntext\n.El\n\\&\\p\nafter\n.El\n";
    let rendered = mant_render::render_query_text(
        &mant_loader::load_roff_bytes(nested_definition).expect("lower nested definition"),
    );
    let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
    let inner = lines
        .iter()
        .position(|line| *line == "inner  text")
        .expect("inner definition: {rendered:?}");
    assert_eq!(lines.get(inner + 1), Some(&""), "{rendered:?}");
    assert_eq!(lines.get(inner + 2), Some(&"after"), "{rendered:?}");

    let nested_bullet = b".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd nested bullet row\n.Sh DESCRIPTION\n.Bl -tag -width outer\n.It outer\n.Bl -bullet\n.It\n\\&\\p\ntext\n.El\nafter\n.El\n";
    let rendered = mant_render::render_query_text(
        &mant_loader::load_roff_bytes(nested_bullet).expect("lower nested bullet"),
    );
    let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
    let marker = lines
        .iter()
        .position(|line| *line == "•")
        .expect("bullet marker: {rendered:?}");
    assert_eq!(lines.get(marker + 1), Some(&"text"), "{rendered:?}");
}

#[test]
fn definition_head_row_settles_at_the_first_real_request_boundary() {
    // Each exact .TP input was checked with fixed CVS -Tutf8/-Tlint.
    // roff_term.c::roff_term_pre_br/sp() call term_newln() at the request;
    // term_vspace() adds its own row only after flushing the occupied tag row.
    for (name, body, blank_rows) in [
        ("break-before-cell", ".br\n\\&\\p\nBODY", 1),
        ("space-before-cell", ".sp 1\n\\&\\p\nBODY", 2),
        ("cell-before-break", "\\&\\p\n.br\nBODY", 0),
        ("cell-before-space", "\\&\\p\n.sp 1\nBODY", 1),
    ] {
        let source =
            format!(".TH PROBE 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.TP\nkey\n{body}\n");
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).expect("lower definition row"),
        );
        let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
        let key = lines
            .iter()
            .position(|line| *line == "key")
            .expect("key row");
        let body = lines
            .iter()
            .position(|line| *line == "BODY")
            .expect("body row");
        assert_eq!(body - key - 1, blank_rows, "{name}: {rendered:?}");
    }

    // The exact .nf/.fi and .Pp inputs were also run through fixed CVS
    // -Tutf8/-Tlint. man_term.c::print_man_node() changes the no-fill line
    // boundary at the request; mdoc_term.c::termp_pp_pre() calls term_vspace().
    for (name, body, blank_rows) in [
        ("cell-before-nofill", "\\&\\p\n.nf\nBODY\n.fi", 0),
        ("nofill-before-cell", ".nf\n\\&\\p\nBODY\n.fi", 1),
    ] {
        let source =
            format!(".TH PROBE 1 \"September 28, 2026\"\n.SH DESCRIPTION\n.TP\nkey\n{body}\n");
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).expect("lower no-fill definition"),
        );
        let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
        let key = lines
            .iter()
            .position(|line| *line == "key")
            .expect("key row");
        let body = lines
            .iter()
            .position(|line| *line == "BODY")
            .expect("body row");
        assert_eq!(body - key - 1, blank_rows, "{name}: {rendered:?}");
    }
    for (name, body, blank_rows) in [
        ("cell-before-paragraph", "\\&\\p\n.Pp\nBODY", 1),
        ("paragraph-before-cell", ".Pp\n\\&\\p\nBODY", 2),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd definition row\n.Sh DESCRIPTION\n.Bl -tag -width key\n.It key\n{body}\n.El\n"
        );
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).expect("lower paragraph definition"),
        );
        let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
        let key = lines
            .iter()
            .position(|line| *line == "key")
            .expect("key row");
        let body = lines
            .iter()
            .position(|line| *line == "BODY")
            .expect("body row");
        assert_eq!(body - key - 1, blank_rows, "{name}: {rendered:?}");
    }
}

#[test]
fn author_pre_break_settles_an_invisible_definition_head_row() {
    // All exact sources were checked with fixed CVS -Tutf8/-Tlint.
    // mdoc_term.c::termp_an_pre() calls term_newln() before Ada when split
    // mode is active, closing any invisible formatter cell on the tag row.
    for (name, body) in [
        ("cell-before-split", "\\&\n.An -split\n.An Ada"),
        ("cell-after-split", ".An -split\n\\&\n.An Ada"),
        ("word-end-before-split", "\\&\\p\n.An -split\n.An Ada"),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd author row\n.Sh AUTHORS\n.Bl -tag -width 20n\n.It key\n{body}\n.El\n"
        );
        let rendered = mant_render::render_query_text(
            &mant_loader::load_roff_bytes(source.as_bytes()).expect("lower author definition"),
        );
        let lines = rendered.lines().map(str::trim).collect::<Vec<_>>();
        let key = lines
            .iter()
            .position(|line| *line == "key")
            .expect("key row");
        assert_eq!(lines.get(key + 1), Some(&"Ada"), "{name}: {rendered:?}");
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
                literal_flow(&document.sections[0].blocks),
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
            literal_flow(&query.document.as_ref().unwrap().sections[0].blocks),
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
            literal_flow(&query.document.as_ref().unwrap().sections[0].blocks),
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
                    let flow = literal_flow(&query.document.as_ref().unwrap().sections[0].blocks);
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
