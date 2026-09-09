//! Original regressions: execution, not raw source adjacency, owns literal rows.
use mant_engine::query_roff_bytes;
use mant_render::render_query_text;

fn rendered(mode: &str, body: &str) -> String {
    let source = match mode {
        "nf" => format!(".TH PROBE 1\n.SH TEST\n.nf\n{body}\n.fi\nAFTER\n"),
        "EX" => format!(".TH PROBE 1\n.SH TEST\n.EX\n{body}\n.EE\nAFTER\n"),
        "SY" => format!(".TH PROBE 1\n.SH TEST\n.EX\n.SY probe\n{body}\n.YS\n.EE\nAFTER\n"),
        mode => format!(
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bd -{mode} -compact\n{body}\n.Ed\nAFTER\n"
        ),
    };
    let query = query_roff_bytes(source.as_bytes()).unwrap();
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    render_query_text(&query)
}

#[test]
fn retained_display_paragraph_requests_are_independent_of_literal_cursor_state() {
    // CVS mdoc_term.c termp_pp_pre calls term_vspace unconditionally. Native
    // validation, not our raw-source scan, removes duplicate/superseded Pp.
    for mode in ["literal", "unfilled"] {
        for (body, blanks) in [
            ("ALPHA\n.Pp\nBETA", 1),
            ("ALPHA\n.sp 1\n.Pp\nBETA", 2),
            ("ALPHA\n.sp 0\n.Pp\nBETA", 1),
            ("ALPHA\\c\n.Pp\nBETA", 1),
            ("ALPHA\n.Pp\n.sp 1\nBETA", 1),
            ("ALPHA\n.Pp\n.Pp\nBETA", 1),
            ("ALPHA\n.Bf -emphasis\n.sp 1\n.Pp\n.Ef\nBETA", 2),
        ] {
            let text = rendered(mode, body);
            assert!(
                text.contains(&format!("ALPHA{}BETA", "\n".repeat(blanks + 1))),
                "{mode}: {body}: {text:?}"
            );
        }
        let text = rendered(mode, "ALPHA\n.Pp");
        assert!(text.contains("ALPHA\n\nAFTER"), "{mode}: {text:?}");
    }
}

#[test]
fn literal_requests_execute_before_word_lowering() {
    for mode in ["nf", "EX", "SY", "literal", "unfilled"] {
        for (requests, blanks) in [
            (".sp", 1),
            (".sp 0", 0),
            (".sp 1", 1),
            (".sp 2", 2),
            (".sp 1\n.sp 2", 3),
            (".if 1 .sp 2", 2),
        ] {
            let text = rendered(mode, &format!("ALPHA\n{requests}\nBETA"));
            assert!(
                text.contains(&format!("ALPHA{}BETA", "\n".repeat(blanks + 1))),
                "{mode}: {requests}: {text:?}"
            );
        }
    }
}

#[test]
fn skipped_source_is_not_an_executed_blank_row() {
    for mode in ["nf", "EX", "SY", "literal", "unfilled"] {
        for hidden in [
            ".if 0 \\{\\\n.sp 2\n.\\}",
            ".de UNUSED\n.sp 2\n..",
            ".ig END\n.sp 2\n.END",
            ".\\\" comment\n.\\\" another",
        ] {
            let text = rendered(mode, &format!("ALPHA\n{hidden}\nBETA"));
            assert!(text.contains("ALPHA\nBETA"), "{mode}: {hidden}: {text:?}");
        }
    }
}

#[test]
fn every_executed_empty_row_survives_at_internal_and_trailing_positions() {
    for mode in ["nf", "EX", "SY", "literal", "unfilled"] {
        for rows in 1..=3 {
            let text = rendered(
                mode,
                &format!("ALPHA{}BETA{}", "\n".repeat(rows + 1), "\n".repeat(rows)),
            );
            assert!(
                text.contains(&format!(
                    "ALPHA{}BETA{}AFTER",
                    "\n".repeat(rows + 1),
                    "\n".repeat(rows + 1)
                )),
                "{mode}: {rows}: {text:?}"
            );
        }
        let text = rendered(mode, "ALPHA\\c\n\nBETA");
        assert!(text.contains("ALPHA\nBETA"), "{mode}: {text:?}");
        let text = rendered(mode, "ALPHA\\c\n.sp 2\nBETA");
        assert!(text.contains("ALPHA\n\n\nBETA"), "{mode}: {text:?}");
    }
}

#[test]
fn macro_expansion_rows_do_not_use_call_site_line_numbers_as_identity() {
    for mode in ["nf", "EX", "SY", "literal", "unfilled"] {
        let text = rendered(mode, ".de ZZ\nALPHA\n\n\nBETA\n..\n.ZZ\n.ZZ");
        assert!(
            text.contains("ALPHA\n\n\nBETA\nALPHA\n\n\nBETA"),
            "{mode}: {text:?}"
        );
    }
}

#[test]
fn detached_native_ast_keeps_executed_rows_without_source_or_line_numbers() {
    fn clear_lines(node: &mut libmandoc_rs::Node) {
        node.line = 0;
        for child in &mut node.children {
            clear_lines(child);
        }
    }
    for (open, close, header) in [
        (".nf", ".fi", ".TH PROBE 1\n.SH TEST"),
        (".EX", ".EE", ".TH PROBE 1\n.SH TEST"),
        (
            ".Bd -literal -compact",
            ".Ed",
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST",
        ),
    ] {
        let source =
            format!("{header}\n{open}\n.de ZZ\nALPHA\n\n\nBETA\n..\n.ZZ\n.ZZ\n\n{close}\nAFTER\n");
        let mut native = libmandoc_rs::Parser::default()
            .parse_bytes("probe.1", source.as_bytes())
            .unwrap();
        let expected = "ALPHA\n\n\nBETA\nALPHA\n\n\nBETA\n\nAFTER";
        for erase_line_numbers in [false, true] {
            if erase_line_numbers {
                clear_lines(&mut native.document.root);
            }
            let document =
                mant_engine::lower_mandoc_document(std::path::Path::new("probe.1"), &native);
            let text = render_query_text(&mant_ir::ResolvedContent {
                label: "probe".into(),
                address: None,
                document: Some(document),
                tldr: None,
            });
            assert!(
                text.contains(expected),
                "{open}; erase={erase_line_numbers}: {text:?}"
            );
        }
    }
}

#[test]
fn state_only_requests_do_not_materialize_phantom_literal_rows() {
    for mode in ["nf", "EX", "literal", "unfilled"] {
        for (body, expected) in [
            ("ALPHA\\c\n.ft B\nBETA", "ALPHABETA"),
            ("ALPHA\n.ft B\n\nBETA", "ALPHA\n\nBETA"),
            ("ALPHA\n.de ZZ\n..\n.ZZ\nBETA", "ALPHA\nBETA"),
        ] {
            let text = rendered(mode, body);
            assert!(text.contains(expected), "{mode}: {body}: {text:?}");
        }
    }
    let text = rendered(
        "literal",
        ".Bf -emphasis\nALPHA\n\nBETA\n.Ef\n.Tg destination\n.Em GAMMA\nDELTA",
    );
    assert!(text.contains("ALPHA\n\nBETA\nGAMMA\nDELTA"), "{text:?}");
}

#[test]
fn leading_and_all_blank_literal_rows_are_content_after_a_predecessor() {
    // Checked against CVS HEAD and groff with visible preceding content;
    // formatter no-space state at the page top is a different boundary.
    for mode in ["nf", "EX", "literal", "unfilled"] {
        for rows in 1..=3 {
            let open = if mode == "nf" {
                ".nf"
            } else if mode == "EX" {
                ".EX"
            } else {
                ".Bd -literal -compact"
            };
            let close = if mode == "nf" {
                ".fi"
            } else if mode == "EX" {
                ".EE"
            } else {
                ".Ed"
            };
            let header = if matches!(mode, "nf" | "EX") {
                ".TH PROBE 1\n.SH TEST"
            } else {
                ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST"
            };
            for middle in ["ALPHA\n", ""] {
                let source = format!(
                    "{header}\nBEFORE\n{open}\n{}{middle}{close}\nAFTER\n",
                    "\n".repeat(rows)
                );
                let text = render_query_text(&query_roff_bytes(source.as_bytes()).unwrap());
                let next = if middle.is_empty() { "AFTER" } else { "ALPHA" };
                assert!(
                    text.contains(&format!("BEFORE{}{next}", "\n".repeat(rows + 1))),
                    "{source}\n{text:?}"
                );
            }
        }
    }
}

#[test]
fn explicit_literal_spacing_retains_start_end_and_empty_display_boundaries() {
    for mode in ["nf", "literal"] {
        let (header, open, close) = if mode == "nf" {
            (".TH PROBE 1\n.SH TEST", ".nf", ".fi")
        } else {
            (
                ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST",
                ".Bd -literal -compact",
                ".Ed",
            )
        };
        for (body, expected) in [
            (".sp 2", "BEFORE\n\n\nAFTER"),
            (".sp 1\n.sp 2", "BEFORE\n\n\n\nAFTER"),
            (".sp 2\nALPHA", "BEFORE\n\n\nALPHA\nAFTER"),
            ("ALPHA\n.sp 2", "ALPHA\n\n\nAFTER"),
            ("ALPHA\n.sp 2\n", "ALPHA\n\n\n\nAFTER"),
            ("ALPHA\n.sp 1\n\n.sp 1", "ALPHA\n\n\n\nAFTER"),
            ("ALPHA\\c\n\n\nBETA", "ALPHA\n\nBETA"),
        ] {
            let source = format!("{header}\nBEFORE\n{open}\n{body}\n{close}\nAFTER\n");
            let text = render_query_text(&query_roff_bytes(source.as_bytes()).unwrap());
            assert!(text.contains(expected), "{mode} {body}: {text:?}");
        }
    }
}

#[test]
fn explicit_request_budgets_do_not_turn_into_unbounded_literal_content() {
    for (header, open, close) in [
        (".TH PROBE 1\n.SH TEST", ".nf", ".fi"),
        (
            ".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST",
            ".Bd -literal -compact",
            ".Ed",
        ),
    ] {
        // Each request is inside mandoc term_vspan's 65-row limit; only
        // their cumulative boundary exceeds ManT's presentation budget.
        let source = format!(
            "{header}\n{open}\nALPHA\n{}BETA\n{close}\n",
            ".sp 65\n".repeat(100)
        );
        let query = query_roff_bytes(source.as_bytes()).unwrap();
        let text = render_query_text(&query);
        assert!(
            text.contains(&format!("ALPHA{}BETA", "\n".repeat(4097))),
            "{open}: {} bytes, {} newlines, {:?}",
            text.len(),
            text.matches('\n').count(),
            query.document.as_ref().unwrap().diagnostics
        );
        assert!(
            query
                .document
                .as_ref()
                .unwrap()
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref()
                    == Some("manual.vertical-spacing-limit")),
            "{open}: missing bounded-spacing diagnostic"
        );
    }
}

#[test]
fn font_and_target_survive_literal_request_splits() {
    use mant_ir::visit::{Visit, walk_inline};
    #[derive(Debug, Default)]
    struct Emphasized(Vec<String>);
    impl<'a> Visit<'a> for Emphasized {
        fn visit_inline(&mut self, inline: &'a mant_ir::Inline) {
            if let mant_ir::Inline::Emphasis { children } = inline {
                self.0.push(
                    children
                        .iter()
                        .filter_map(|child| match child {
                            mant_ir::Inline::Text { value } => Some(value.as_str()),
                            _ => None,
                        })
                        .collect::<String>(),
                );
            }
            walk_inline(self, inline);
        }
    }
    let query = query_roff_bytes(b".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh TEST\n.Bd -literal -compact\n.Bf -emphasis\nALPHA\n.sp 2\n.Tg destination\n.Em BETA\n.Ef\nGAMMA\n.Ed\n").unwrap();
    let document = query.document.as_ref().unwrap();
    let mut styled = Emphasized::default();
    styled.visit_document(document);
    assert!(
        styled.0.iter().any(|value| value.contains("ALPHA")),
        "{styled:?}"
    );
    assert!(
        styled.0.iter().any(|value| value.contains("BETA")),
        "{styled:?}"
    );
    assert!(
        !styled.0.iter().any(|value| value.contains("GAMMA")),
        "{styled:?}"
    );
    assert!(mant_ir::DocumentIndex::build(document).contains("destination"));
    assert!(render_query_text(&query).contains("ALPHA\n\n\nBETA\nGAMMA"));
}
