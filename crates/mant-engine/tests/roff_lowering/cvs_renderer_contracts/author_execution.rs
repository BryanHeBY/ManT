use super::*;

#[test]
fn run_in_definition_handoff_preserves_native_gap_and_source_row_contracts() {
    let cases = [
        ("inset", "", "BC", ""),
        ("inset", "\\&", "BC", ""),
        ("inset", "A", " BC", "A"),
        ("diag", "A", " BC", "A"),
        ("diag", "", " BC", ""),
    ];
    for (style, head, body, term) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Bl -{style}\n.It {head}\n{body}\n.El\n"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower run-in item");
        let item = first_definition_item(query.document.as_ref().unwrap());
        let term_text = item
            .terms
            .iter()
            .map(|term| super::inline_text(term))
            .collect::<String>();
        assert_eq!(term_text, term, "{style} {head:?}: {item:?}");
        let description = item
            .description
            .iter()
            .find_map(|block| match block {
                Block::Paragraph { children, .. } => Some(super::inline_text(children)),
                _ => None,
            })
            .unwrap();
        let expected = match (style, head, body.starts_with(' ')) {
            ("inset", "", _) => "BC",
            ("inset", "\\&", _) => " BC",
            ("inset", _, true) => " \n BC",
            ("diag", _, true) => "  \n BC",
            _ => unreachable!(),
        };
        assert_eq!(description, expected, "{style} {head:?}: {item:?}");
        assert!(item.inline_term(), "{style} {head:?}: {item:?}");
        assert_eq!(item.layout.min_term_gap_columns, 0);
    }
}

#[test]
fn transparent_target_does_not_reset_a_run_in_formatter_boundary() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -diag\n.It A\n.Tg mark\n BC\n.El\n",
    );
    let native = native_terminal(source);
    assert!(native.contains("A\u{a0}\u{a0}\n      BC"), "{native:?}");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower target run-in");
    let item = first_definition_item(query.document.as_ref().unwrap());
    let description = item
        .description
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(super::inline_text(children)),
            _ => None,
        })
        .unwrap();
    assert_eq!(description, "  \n BC");
    assert!(
        serde_json::to_string(item).unwrap().contains("mark"),
        "target was not attached to its run-in owner: {item:?}"
    );
}

#[test]
fn inherited_zero_advance_cannot_change_an_sx_destination() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n",
        ".Sh ABC\n.No FIRST\n.Sh BC\n.No SECOND\n",
        ".Sh SEE ALSO\n.No \\z\n.Sx ABC\n",
    );
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("sx-zero.1", source.as_bytes())
        .expect("render native Sx zero-advance identity")
        .output;
    assert!(
        native.contains("href=\"#ABC\""),
        "native HTML lost authored target: {native}"
    );

    let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower Sx zero identity");
    let document = query.document.as_ref().expect("lowered document");
    let mut correct = AuthoredSectionLink {
        id: "abc",
        found: false,
    };
    correct.visit_document(document);
    assert!(correct.found, "display BC incorrectly selected section BC");
}

#[test]
fn heading_and_diagnostic_scopes_preserve_previous_font_execution() {
    let cases = [
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
            ".Sh NEXT\\fI\n\\fPTAIL\n",
        ),
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
            ".Ss NEXT\\fI\n\\fPTAIL\n",
        ),
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
            ".Bl -diag\n.It A\\fI\n\\fPTAIL\n.El\n",
        ),
    ];
    for source in cases {
        let native = native_terminal_raw(source);
        assert!(
            native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
            "native terminal did not retain bold TAIL: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower font scope");
        let mut strong_tail = StrongText(false);
        strong_tail.visit_document(query.document.as_ref().expect("lowered document"));
        assert!(strong_tail.0, "lowered IR did not retain bold TAIL");
    }

    // mdoc_term.c applies the diagnostic bold scope in termp_it_pre(), but
    // inset has no such scope.  term_fontpopq() restores the current stack
    // while leaving the previous-font register updated for a later `\fP`.
    let inset = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n",
        ".Bl -inset\n.It A\\fI\n\\fPTAIL\n.El\n",
    );
    let native = native_terminal_raw(inset);
    assert!(
        !native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
        "inset made TAIL bold: {native:?}"
    );
    let query = mant_loader::load_roff_bytes(inset.as_bytes()).expect("lower inset font scope");
    let mut strong_tail = StrongText(false);
    strong_tail.visit_document(query.document.as_ref().expect("lowered document"));
    assert!(!strong_tail.0, "inset inherited diagnostic bold scope");
}

#[test]
fn man_and_mdoc_headings_keep_distinct_previous_font_contracts() {
    for heading in ["SH", "SS"] {
        let source = format!(
            ".TH PROBE 1 \"September 13, 2026\"\n.SH NAME\nprobe \\- test\n.{heading} NEXT\\fI\n\\fPTAIL\n"
        );
        let native = native_terminal_raw(&source);
        assert!(native.contains("TAIL"), "man {heading}: {native:?}");
        assert!(
            !native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
            "man {heading} left TAIL bold: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower man heading");
        let mut tail = ExactTailInline(None);
        tail.visit_document(query.document.as_ref().expect("lowered document"));
        assert_eq!(
            tail.0,
            Some(Inline::Text {
                value: "TAIL".to_owned()
            }),
            "man {heading} retained the wrong font state"
        );
    }

    for heading in ["Sh", "Ss"] {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.{heading} NEXT\\fI\n\\fPTAIL\n"
        );
        let native = native_terminal_raw(&source);
        assert!(
            native.contains("T\u{8}TA\u{8}AI\u{8}IL\u{8}L"),
            "mdoc {heading} lost bold TAIL: {native:?}"
        );
        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower mdoc heading");
        let mut tail = ExactTailInline(None);
        tail.visit_document(query.document.as_ref().expect("lowered document"));
        assert_eq!(
            tail.0,
            Some(Inline::Strong {
                children: vec![Inline::Text {
                    value: "TAIL".to_owned()
                }]
            }),
            "mdoc {heading} lost the exact previous-font state"
        );
    }
}

#[test]
fn definition_heads_execute_author_modes_in_native_node_order() {
    let cases = [
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
            ".Sh DESCRIPTION\n.Bl -inset\n.It Xo An -split An Alice An Bob Xc\n",
            ".No BODY\n.El\n.No END\n",
        ),
        concat!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
            ".Sh DESCRIPTION\n.Bl -inset\n.It Xo\n.Ao\n.An -split\n.An Alice\n",
            ".An Bob\n.Ac\n.Xc\n.No BODY\n.El\n.No END\n",
        ),
    ];
    for source in cases {
        let native = native_terminal(source);
        assert!(
            native.contains("Alice\n"),
            "native author split: {native:?}"
        );
        assert!(native.contains("Bob"), "native author output: {native:?}");

        let query = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower author head");
        let item = first_definition_item(query.document.as_ref().unwrap());
        let terms = item
            .terms
            .iter()
            .map(|term| super::inline_text(term))
            .collect::<Vec<_>>();
        assert!(
            terms.iter().any(|term| term.contains("Alice"))
                && terms.iter().any(|term| term.contains("Bob")),
            "author alternatives: {item:?}"
        );
        let lowered = without_line_indentation(&lowered_terminal(source));
        assert!(
            lowered.contains("Alice\nBob") && lowered.contains("BODY"),
            "author split vanished: {lowered:?}"
        );
    }
}

#[test]
fn nested_paragraph_authors_use_the_same_ordered_execution_stream() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh DESCRIPTION\n.Dq An -split An Alice An Bob\n.No END\n",
    );
    let native = without_line_indentation(&native_terminal(source));
    assert!(native.contains("“\nAlice\nBob” END"), "native: {native:?}");
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(
        lowered.contains("“\nAlice\nBob” END"),
        "nested author execution: {lowered:?}"
    );
}

#[test]
fn block_authors_execute_each_mode_transition_once() {
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh AUTHORS\n.No PREFIX\n.An Alice\n.An Bob\n",
    );
    let native = without_line_indentation(&native_terminal(source));
    assert!(native.contains("PREFIX Alice\nBob"), "native: {native:?}");
    let lowered = without_line_indentation(&lowered_terminal(source));
    assert!(
        lowered.contains("PREFIX Alice\nBob"),
        "author mode executed more than once: {lowered:?}"
    );
}

#[test]
fn detached_definition_heads_project_author_flushes_by_list_style() {
    let cases = [
        ("inset", "Alice\nBob BODY"),
        ("tag", "Alice  Bob\n"),
        ("hang", "Alice Bob BODY"),
        ("ohang", "Alice\nBob\nBODY"),
    ];
    for (style, expected) in cases {
        let source = format!(
            ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo An -split An Alice An Bob Xc\n.No BODY\n.El\n"
        );
        let native = without_line_indentation(&native_terminal(&source)).replace('\u{a0}', " ");
        assert!(native.contains(expected), "native {style}: {native:?}");
        let lowered = without_line_indentation(&lowered_terminal(&source)).replace('\u{a0}', " ");
        assert!(lowered.contains(expected), "lowered {style}: {lowered:?}");
    }
}

#[test]
fn control_only_authors_preserve_native_rows_and_field_origins() {
    for control in [r"\&", r"\p"] {
        for style in ["inset", "tag", "hang", "ohang"] {
            let source = format!(
                ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n.Sh DESCRIPTION\n.Bl -{style}\n.It Xo\n.An -split\n.An {control}\n.An Bob\n.Xc\n.No BODY\n.El\n"
            );
            let native = native_terminal(&source).replace('\u{a0}', " ");
            let lowered = lowered_terminal(&source).replace('\u{a0}', " ");
            match style {
                "inset" => {
                    assert!(
                        native.contains("DESCRIPTION\n\n     Bob BODY"),
                        "native: {native:?}"
                    );
                    assert!(
                        lowered.contains("DESCRIPTION\n\nBob BODY"),
                        "lowered: {lowered:?}"
                    );
                }
                "ohang" => {
                    assert!(
                        native.contains("DESCRIPTION\n\n     Bob\n     BODY"),
                        "native: {native:?}"
                    );
                    assert!(
                        lowered.contains("DESCRIPTION\n\nBob\nBODY"),
                        "lowered: {lowered:?}"
                    );
                }
                "tag" | "hang" => {
                    let native_line = native.lines().find(|line| line.contains("Bob")).unwrap();
                    let lowered_line = lowered.lines().find(|line| line.contains("Bob")).unwrap();
                    assert_eq!(
                        lowered_line,
                        native_line.trim_start(),
                        "{style} {control} must retain the native term origin and body column"
                    );
                }
                _ => unreachable!(),
            }
        }
    }
}
