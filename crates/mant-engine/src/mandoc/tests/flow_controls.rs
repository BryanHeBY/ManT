//! Native paragraph, literal-line and styling-container flow contracts.

#[test]
fn ordinary_man_paragraphs_reset_prevailing_definition_width() {
    use mant_ir::visit::{Visit, walk_definition_item};
    struct Widths(Vec<bool>);
    impl<'a> Visit<'a> for Widths {
        fn visit_definition_item(&mut self, item: &'a mant_ir::DefinitionItem) {
            self.0.push(item.inline_term);
            walk_definition_item(self, item);
        }
    }
    for (boundary, inline_second) in [("PP", false), ("P", false), ("LP", false), ("HP", true)] {
        let source = format!(
            ".TH PROBE 1\n.SH DESCRIPTION\n.TP 15\nFIRSTLONGTAG\nFIRST\n.{boundary}\nBETWEEN\n.TP\nSECONDLONGTAG\nSECOND\n"
        );
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let mut widths = Widths(Vec::new());
        widths.visit_document(query.document.as_ref().unwrap());
        assert_eq!(widths.0, [true, inline_second], "{source}");
        let text = crate::render_query_text(&query);
        assert!(
            text.lines()
                .any(|line| line.contains("FIRSTLONGTAG") && line.contains("FIRSTLONGTAG FIRST")),
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
            let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
            let document = query.document.as_ref().unwrap();
            let mant_ir::Block::Preformatted { children, .. } = &document.sections[0].blocks[0]
            else {
                panic!("{document:?}")
            };
            assert_eq!(super::inline_text(children), expected, "{source}");
            assert!(
                crate::render_query_text(&query).contains(expected),
                "{source}: {}",
                crate::render_query_text(&query)
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
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let mant_ir::Block::Preformatted { children, .. } =
            &query.document.as_ref().unwrap().sections[0].blocks[0]
        else {
            panic!("{query:?}")
        };
        assert_eq!(super::inline_text(children), "FIRSTSECONDTHIRD", "{body}");
        assert!(crate::render_query_text(&query).contains("FIRSTSECONDTHIRD"));
        assert!(crate::render_markdown(&query).contains("FIRSTSECONDTHIRD"));
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
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let mant_ir::Block::Preformatted { children, .. } =
            &query.document.as_ref().unwrap().sections[0].blocks[0]
        else {
            panic!("{query:?}")
        };
        assert_eq!(super::inline_text(children), expected, "{body}");
        assert!(crate::render_query_text(&query).contains(expected));
        assert!(crate::render_markdown(&query).contains(expected));
    }
}

#[test]
fn explicit_literal_breaks_are_not_repeated_at_styling_boundaries() {
    fn line_breaks(nodes: &[mant_ir::Inline]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                mant_ir::Inline::LineBreak => 1,
                mant_ir::Inline::Strong { children }
                | mant_ir::Inline::Emphasis { children }
                | mant_ir::Inline::Link { children, .. } => line_breaks(children),
                _ => 0,
            })
            .sum()
    }
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
                    let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
                    let mant_ir::Block::Preformatted { children, .. } =
                        &query.document.as_ref().unwrap().sections[0].blocks[0]
                    else {
                        panic!("{query:?}")
                    };
                    assert_eq!(super::inline_text(children), expected, "{body}");
                    assert_eq!(
                        line_breaks(children),
                        expected.matches('\n').count(),
                        "{body}"
                    );
                    assert!(
                        crate::render_query_text(&query).contains(&expected),
                        "{body}"
                    );
                    assert!(crate::render_markdown(&query).contains(&expected), "{body}");
                }
            }
        }
    }
}
