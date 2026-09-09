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

fn assert_markdown_literal_rows(query: &mant_engine::ResolvedContent, expected: &str) {
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
    let reloaded = mant_engine::query_markdown_text(&markdown, None).unwrap();
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
            self.0.push(item.layout.inline_term);
            walk_definition_item(self, item);
        }
    }
    for (boundary, inline_second) in [("PP", false), ("P", false), ("LP", false), ("HP", true)] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.TP 15\nFIRSTLONGTAG\nFIRST\n.{boundary}\nBETWEEN\n.TP\nSECONDLONGTAG\nSECOND\n"
        );
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
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
            let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
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
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
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
        let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
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
                    let query = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
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
