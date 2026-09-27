//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

#[test]
fn table_layout_font_reaches_inline_consumers() {
    let document = parse_manual_bytes(
        std::path::Path::new("table-font.7"),
        b".TH FONT 7\n.TS\ntab(|);\nlb li.\nLEFT|RIGHT\n.TE\nafter\n",
    )
    .unwrap();
    let Block::Table { rows, .. } = &document.blocks[0] else {
        panic!("table");
    };
    let [Block::Paragraph { children: left, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("left cell");
    };
    let [
        Block::Paragraph {
            children: right, ..
        },
    ] = rows[0].cells[1].blocks.as_slice()
    else {
        panic!("right cell");
    };
    // CVS tbl_html.c::print_tbl scopes layout->font to one cell.
    assert!(matches!(left.as_slice(), [Inline::Strong { .. }]));
    assert!(matches!(right.as_slice(), [Inline::Emphasis { .. }]));
}

#[test]
fn table_layout_font_can_be_overridden_and_restored_within_one_cell() {
    // Exact input checked with the fixed -Thtml/-Tutf8/-Tlint oracle.
    // CVS tbl_html.c::print_tbl selects layout->font before print_text(),
    // where html.c::print_text() executes each \fR and \fP in source order.
    let document = parse_manual_bytes(
        std::path::Path::new("table-font-override.7"),
        b".TH REVIEW 7\n.SH DESCRIPTION\n.TS\nlb.\nA\\fRB\\fPC\n.TE\nafter\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("table and following paragraph");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("table cell");
    };
    assert!(matches!(children.as_slice(),
        [Inline::Strong { children: a }, Inline::Text { value: b }, Inline::Strong { children: c }]
        if matches!(a.as_slice(), [Inline::Text { value }] if value == "A")
            && b == "B"
            && matches!(c.as_slice(), [Inline::Text { value }] if value == "C")));
    assert!(matches!(after.as_slice(), [Inline::Text { value }] if value == "after"));
}

#[test]
fn table_cell_font_selection_does_not_leak_to_next_cell_or_prose() {
    // Exact input checked with the fixed -Thtml/-Tutf8/-Tlint oracle.
    // CVS tbl_term.c::tbl_word pushes layout->font and term_fontpopq()
    // restores the prior current font before rendering the next cell.
    let document = parse_manual_bytes(
        std::path::Path::new("table-font-scope.7"),
        b".TH REVIEW 7\n.SH DESCRIPTION\n.TS\ntab(|);\nlb l.\nA\\fIB|C\n.TE\nafter\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("table and following paragraph");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0),
        [Inline::Strong { children: a }, Inline::Emphasis { children: b }]
        if matches!(a.as_slice(), [Inline::Text { value }] if value == "A")
            && matches!(b.as_slice(), [Inline::Text { value }] if value == "B")));
    assert!(matches!(cell(1), [Inline::Text { value }] if value == "C"));
    assert!(matches!(after.as_slice(), [Inline::Text { value }] if value == "after"));
}

#[test]
fn roman_table_cell_keeps_terminal_font_register_at_its_stack_level() {
    // Exact input checked with the fixed -Tutf8/-Thtml oracle.  The outputs
    // differ: CVS tbl_term.c::tbl_word does not push for ESCAPE_FONTROMAN, so
    // term.c::term_fontrepl() in the first cell changes the current stack
    // level and subsequent terminal words inherit it. HTML restores metac
    // after each cell instead. This IR follows the terminal font register.
    let document = parse_manual_bytes(
        std::path::Path::new("table-roman-register.7"),
        b".TH REVIEW 7\n.SH DESCRIPTION\n.TS\ntab(|);\nl l.\nA\\fIB|C\n.TE\nafter\n",
    )
    .unwrap();
    let [
        Block::Table { rows, .. },
        Block::Paragraph {
            children: after, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("table and following paragraph");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0),
        [Inline::Text { value: a }, Inline::Emphasis { children: b }]
        if a == "A" && matches!(b.as_slice(), [Inline::Text { value }] if value == "B")));
    assert!(matches!(cell(1), [Inline::Emphasis { children }] if
        matches!(children.as_slice(), [Inline::Text { value }] if value == "C")));
    assert!(
        matches!(after.as_slice(), [Inline::Emphasis { children }] if
        matches!(children.as_slice(), [Inline::Text { value }] if value == "after"))
    );
}

#[test]
fn native_table_constant_width_fonts_reach_ir_without_flattening_style() {
    // The exact input was checked with the fixed -Thtml/-Ttree oracle.
    // CVS tbl_layout.c::cellmod selects CR/CB/CI and tbl_html.c::print_tbl
    // applies html_setfont to each data cell independently.
    let document = parse_manual_bytes(
        std::path::Path::new("table-code-font.1"),
        b".TH FONT 1\n.SH DESCRIPTION\n.TS\nl fCR l fCB l fCI.\nplain\tbold\titalic\n.TE\n",
    )
    .unwrap();
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("table");
    };
    let cell = |index: usize| match rows[0].cells[index].blocks.as_slice() {
        [Block::Paragraph { children, .. }] => children.as_slice(),
        other => panic!("cell {index}: {other:?}"),
    };
    assert!(matches!(cell(0), [Inline::Code { value }] if value == "plain"));
    assert!(matches!(cell(1), [Inline::Strong { children }] if
        matches!(children.as_slice(), [Inline::Code { value }] if value == "bold")));
    assert!(matches!(cell(2), [Inline::Emphasis { children }] if
        matches!(children.as_slice(), [Inline::Code { value }] if value == "italic")));
}

#[test]
fn ragged_matrix_keeps_later_column_rows_through_ir_json() {
    let document = parse_manual_bytes(
        std::path::Path::new("short-matrix.7"),
        b".TH MATRIX 7\n.EQ\nmatrix { lcol { a } rcol { b above c } }\n.EN\n",
    )
    .unwrap();
    let Block::Equation {
        value,
        expression: Some(expression),
        ..
    } = &document.blocks[0]
    else {
        panic!("matrix block");
    };
    // CVS eqn.h stores a matrix as column piles. Pinned eqn_html.c uses the
    // first column's row count and drops `c`; the native tree and UTF-8 oracle
    // retain it, so the shared reading projection uses the longest column.
    assert_eq!(expression.children[0].kind, mant_ir::EquationKind::Matrix);
    assert_eq!(value, "matrix(a, b; , c)");
    // CVS eqn.c::eqn_box_new uses UINT_MAX for an unbounded list, and
    // EQN_DEFSIZE is INT_MIN. The IR has no parser sentinel numerals.
    let wire = serde_json::to_value(&document).unwrap();
    assert!(
        wire["blocks"][0]["expression"]
            .get("expectedArgs")
            .is_none()
    );
    assert!(
        wire["blocks"][0]["expression"]["children"][0]
            .get("size")
            .is_none()
    );
    let json = serde_json::to_vec(&document).unwrap();
    let restored: mant_ir::Document = serde_json::from_slice(&json).unwrap();
    let Block::Equation {
        value: restored_value,
        expression: Some(restored_expression),
        ..
    } = &restored.blocks[0]
    else {
        panic!("round-trip matrix");
    };
    assert_eq!(restored_value, &restored_expression.readable_text());
    assert!(mant_ir::validate_document(&restored).is_empty());
}

#[test]
fn ragged_matrix_keeps_earlier_column_rows_through_ir() {
    // The exact input was checked with the fixed -Ttree/-Thtml/-Tutf8 oracle.
    // CVS eqn_html.c::eqn_box traverses rows in matrix column order and emits
    // an empty trailing cell when the second column is shorter.
    let document = parse_manual_bytes(
        std::path::Path::new("long-first-matrix.7"),
        b".TH MATRIX 7\n.EQ\nmatrix { lcol { a above b } rcol { c } }\n.EN\n",
    )
    .unwrap();
    let Block::Equation {
        value,
        expression: Some(expression),
        ..
    } = &document.blocks[0]
    else {
        panic!("matrix block");
    };
    assert_eq!(value, "matrix(a, c; b, )");
    assert_eq!(value, &expression.readable_text());
    assert!(mant_ir::validate_document(&document).is_empty());
}

#[test]
fn inline_equation_stays_between_prose_siblings() {
    let document = parse_manual_bytes(
        std::path::Path::new("inline-equation.1"),
        b".TH INLINE 1\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\nleft $x sub i$ right\n",
    )
    .unwrap();
    let paragraph = document.sections[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Paragraph { children, .. } => Some(children),
            _ => None,
        })
        .expect("prose paragraph");
    let kinds = paragraph
        .iter()
        .filter_map(|part| match part {
            Inline::Text { value } if value.contains("left") => Some("left"),
            Inline::Equation { value, expression } => {
                assert_eq!(value, &expression.readable_text());
                Some("equation")
            }
            Inline::Text { value } if value.contains("right") => Some("right"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["left", "equation", "right"]);
}

#[test]
fn native_equation_depth_loss_marks_both_document_coverage_flags() {
    let source = format!(
        ".TH DEEP 1\n.SH BODY\n.EQ\n{}x{}\n.EN\n",
        "sqrt { ".repeat(260),
        " }".repeat(260),
    );
    let document =
        parse_manual_bytes(std::path::Path::new("deep-equation.1"), source.as_bytes()).unwrap();
    // The exact input was run with the pinned -Ttree oracle before this
    // assertion. Its eqn boxes continue beyond the owned 256-level limit.
    assert!(
        document
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.impact == mant_ir::DiagnosticImpact::ContentCoverage)
    );
    assert!(!mant_ir::content_complete(&document.diagnostics));
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
}

#[test]
fn bounds_distinct_tbl_equation_normalization_work() {
    let mut source =
        String::from(".TH TABLE-EQN-BUDGET 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl.\n");
    for index in 0..=MAX_INLINE_EQUATION_NORMALIZATIONS {
        writeln!(source, "%x{index}%").expect("write fixture row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation-budget.3"),
        source.as_bytes(),
    )
    .expect("lower a bounded number of table equations");

    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
        })
    );
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    assert_eq!(rows.len(), MAX_INLINE_EQUATION_NORMALIZATIONS + 1);
    // The pinned tbl tree still owns all 257 cells. Stopping the optional
    // reparse leaves the native payload readable, so this is no content loss.
    assert!(mant_ir::content_complete(&document.diagnostics));
    assert!(mant_ir::semantics_complete(&document.diagnostics));
    assert_eq!(
        document
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code.as_deref() == Some("manual.inline-equation-budget"))
            .unwrap()
            .impact,
        mant_ir::DiagnosticImpact::None
    );
}

#[test]
fn display_equations_preserve_native_eqn_decorators() {
    let document = parse_manual_bytes(
        std::path::Path::new("eqn-decorators.1"),
        b".TH EQUATION 1\n.SH DESCRIPTION\n.EQ\nx dot = f(t) bar\ny dotdot bar ~=~ n under\nx vec ~=~ y dyad\n.EN\n",
    )
    .expect("lower decorated equations");
    let values = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Equation { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    // CVS eqn_term.c emits the resolved top/bottom decorations after each
    // base expression. The semantic IR must retain those relations rather
    // than silently reducing every decorated equation to its bare operands.
    for fragment in ["x˙", "‾", "y¨", "n_", "x→", "y↔"] {
        assert!(values.contains(fragment), "{fragment}: {values}");
    }
}

#[test]
fn tbl_text_block_prefers_native_parse_time_string_expansion() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-expanded-string.1"),
        b".TH TBL-EXPANDED-STRING 1\n.ds Aq \\(aq\n.SH DESCRIPTION\n.TS\nl.\nT{\nThere\\*(Aqs\nT}\n.TE\n",
    )
    .expect("lower tbl source with a parse-time string expansion");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    assert_eq!(
        inline_text(match &rows[0].cells[0].blocks[0] {
            Block::Paragraph { children, .. } => children,
            other => panic!("expected table paragraph, got {other:?}"),
        }),
        "There's"
    );
}

#[test]
fn tbl_source_recovery_never_replays_a_redefined_macro_outside_native_context() {
    for definition in ["de", "de1"] {
        let source = format!(
            ".TH TBL-REDEFINED-MACRO 1\n.SH DESCRIPTION\n.{definition} B\nREPLACED_MACRO\n..\n.TS\nl.\nT{{\n.B ORIGINAL_OPERAND\nT}}\n.TE\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("tbl-redefined-macro.1"),
            source.as_bytes(),
        )
        .expect("lower a table with a document-local macro override");
        let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
            panic!("expected one lowered table");
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
            panic!("expected native table paragraph");
        };
        assert_eq!(inline_text(children), "REPLACED_MACRO", "{definition}");
    }
}

#[test]
fn tbl_source_recovery_requires_native_direct_call_provenance() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-redefined-manual-reference.1"),
        b".TH TBL-REDEFINED-MANUAL-REFERENCE 1\n.de MR\nprintf 3\n..\n.SH DESCRIPTION\n.TS\nl.\nT{\n.MR printf 3\nT}\n.TE\n",
    )
    .expect("lower a table with a redefined manual-reference macro");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected native table paragraph");
    };
    assert_eq!(inline_text(children), "printf 3");
    assert!(
        !contains_manual_link(children),
        "a redefined .MR must not fabricate a manual link: {children:?}"
    );
}

#[test]
fn tbl_source_recovery_does_not_reinterpret_text_after_custom_control_change() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-custom-control.1"),
        b".TH TBL-CUSTOM-CONTROL 1\n.SH DESCRIPTION\n.cc @\n@TS\nl.\nT{\n@MR printf 3\nT}\n@TE\n",
    )
    .expect("lower a table after changing the native control character");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected native table paragraph");
    };
    assert_eq!(inline_text(children), "printf 3");
    assert!(!contains_manual_link(children), "{children:?}");
}

#[test]
fn tbl_recovery_marks_empty_user_macros_per_cell_without_degrading_siblings() {
    for (label, definition) in [
        ("empty", ".de Fl\n..\n"),
        ("return", ".de Fl\n.return\n..\n"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt TBL-EMPTY-FL 1\n.Os\n{definition}.Sh DESCRIPTION\n.TS\nl l.\nT{{\n.Fl\nhelp\nT}}\tT{{\n.Em WORD\nT}}\n.TE\n"
        );
        let document = parse_manual_bytes(
            std::path::Path::new("tbl-empty-user-macro.1"),
            source.as_bytes(),
        )
        .expect("lower a table with an empty user macro");
        let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
            panic!("expected one lowered table");
        };
        let cells = rows[0]
            .cells
            .iter()
            .map(|cell| match cell.blocks.as_slice() {
                [Block::Paragraph { children, .. }] => inline_text(children),
                blocks => panic!("expected table cell paragraph: {blocks:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(cells, ["help", "WORD"], "{label}");
        let second = match rows[0].cells[1].blocks.as_slice() {
            [Block::Paragraph { children, .. }] => children,
            blocks => panic!("expected second table cell paragraph: {blocks:?}"),
        };
        assert!(contains_emphasis(second), "{label}: {second:?}");
    }
}

#[test]
fn tbl_escape_disabled_cells_keep_escape_spellings_literal() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-eo-literal.1"),
        b".TH TBL-EO-LITERAL 1\n.SH DESCRIPTION\n.eo\n.TS\nl.\nT{\n.B TOKEN \\fIITALIC\\fP\nT}\n.TE\n",
    )
    .expect("lower a tbl cell with escape processing disabled");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected native table paragraph");
    };
    assert_eq!(inline_text(children), r"TOKEN \fIITALIC\fP");
    assert!(
        !contains_emphasis(children),
        "disabled escape processing must not synthesize italics: {children:?}"
    );
}

#[test]
fn tbl_native_payload_preserves_escape_transitions_inside_one_cell() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-eo-then-ec.1"),
        b".TH TBL-EO-THEN-EC 1\n.SH DESCRIPTION\n.eo\n.TS\nl.\nT{\nLITERAL \\fIBARE\\fP\n.ec\nACTIVE \\fISTYLED\\fP\nT}\n.TE\n",
    )
    .expect("lower a tbl cell that reenables escape processing");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected native table paragraph");
    };
    assert_eq!(inline_text(children), r"LITERAL \fIBARE\fP ACTIVE STYLED");
    assert_eq!(
        children
            .iter()
            .filter(|inline| matches!(inline, Inline::Emphasis { .. }))
            .count(),
        1,
        "only the post-.ec font escape may execute: {children:?}"
    );
}

#[test]
fn tbl_source_recovery_preserves_native_whitespace_from_redefined_macro() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-redefined-whitespace.1"),
        b".TH TBL-REDEFINED-WHITESPACE 1\n.de B\nA B\n..\n.SH DESCRIPTION\n.TS\nl.\nT{\n.B AB\nT}\n.TE\n",
    )
    .expect("lower a table with a whitespace-producing macro override");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected native table paragraph");
    };
    assert_eq!(inline_text(children), "A B");
}

#[test]
fn declined_tbl_recovery_never_consumes_native_macro_expansion_siblings() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-appended-macro-sibling.1"),
        b".TH TBL-APPENDED-MACRO-SIBLING 1\n.am1 B\nADDED_TEXT\n..\n.SH DESCRIPTION\n.TS\nl.\nT{\n.B ORIGINAL_OPERAND\nT}\n.TE\n",
    )
    .expect("lower a table with an appended macro body");
    let text = visible_document_text(&document);
    assert!(text.contains("ADDED_TEXT"), "{text}");
    assert!(text.contains("ORIGINAL_OPERAND"), "{text}");
}

fn contains_manual_link(children: &[Inline]) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Link {
            target: mant_ir::LinkTarget::Manual { .. },
            ..
        } => true,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => contains_manual_link(children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. }
        | Inline::Anchor { .. }
        | Inline::LineBreak => false,
    })
}

fn contains_emphasis(children: &[Inline]) -> bool {
    children.iter().any(|inline| match inline {
        Inline::Emphasis { .. } => true,
        Inline::Strong { children } | Inline::Link { children, .. } => contains_emphasis(children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. }
        | Inline::Anchor { .. }
        | Inline::LineBreak => false,
    })
}

#[test]
fn tbl_native_field_count_wins_over_raw_tab_characters() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-executed-delimiter.1"),
        b".TH TBL-EXECUTED-DELIMITER 1\n.SH DESCRIPTION\n.TS\ntab(;);\nl l.\nLEFT\tMID\tGHOST;RIGHT\n.TE\n",
    )
    .expect("lower a table with a non-default tbl delimiter");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    assert_eq!(rows[0].cells.len(), 2);
    let cells = rows[0]
        .cells
        .iter()
        .map(|cell| match cell.blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            blocks => panic!("expected table cell paragraph: {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(cells, ["LEFT\tMID\tGHOST", "RIGHT"]);
}

#[test]
fn tbl_comments_use_native_not_lexically_guessed_escape_state() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-unexecuted-escape.1"),
        b".TH TBL-UNEXECUTED-ESCAPE 1\n.SH DESCRIPTION\n.de UNUSED\n.ec @\n..\n.TS\nl.\nT{\n.B VISIBLE \\\" HIDDEN_COMMENT\nT}\n.TE\n",
    )
    .expect("lower a table after an uncalled escape-changing macro");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected table paragraph");
    };
    assert_eq!(inline_text(children), "VISIBLE");
}

#[test]
fn tbl_comment_truncation_precedes_tab_cell_recovery() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-comment-tabs.1"),
        b".TH TBL-COMMENT-TABS 1\n.SH DESCRIPTION\n.TS\nl l.\nLEFT\tRIGHT \\\" COMMENT\tHIDDEN_CELL\n.TE\n",
    )
    .expect("lower a table with a commented trailing tab field");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    assert_eq!(rows[0].cells.len(), 2);
    let text = rows[0]
        .cells
        .iter()
        .flat_map(|cell| &cell.blocks)
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(text, "LEFT RIGHT");
}

#[test]
fn tbl_text_blocks_recover_complete_inline_macro_semantics() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-request-dispatch.1"),
        b".Dd September 12, 2026\n.Dt TBLPROBE 1\n.Os\n.Sh DESCRIPTION\n.TS\nl.\nT{\n.BR A / B .\nT}\nT{\n.Sm off\n.Em WORD\nT}\nT{\n.Fl Fl help\nT}\nT{\n.sp 1\nSPACED\nT}\n.TE\n",
    )
    .expect("lower a bounded inline tbl recovery witness");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let cells = rows
        .iter()
        .map(|row| match row.cells[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            blocks => panic!("expected one table cell paragraph: {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(cells, ["A / B .", "WORD", "--help", "SPACED"]);
}

#[test]
fn tbl_source_recovery_never_promotes_roff_comments_to_cells_or_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-inline-comment.3"),
        b".TH TBL-INLINE-COMMENT 3\n.SH DESCRIPTION\n.TS\nl l.\nleft\tright\\\" ignored ordinary-cell payload\nT{ \\\" real text-block marker with comment\n.BR linked (3) \\\" ignored text-block payload\nT}\tplain\n.TE\n",
    )
    .expect("lower table comments through the native roff lexical boundary");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let table_text = rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.blocks)
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        table_text.contains("left right linked(3) plain"),
        "{table_text}"
    );
    assert!(!table_text.contains("ignored"), "{table_text}");
    assert!(!table_text.contains("payload"), "{table_text}");
}

#[test]
fn tbl_source_recovery_uses_a_document_level_ec_escape_change() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-alternate-escape-comment.3"),
        b".TH TBL-ALTERNATE-ESCAPE-COMMENT 3\n.ec @\n.SH DESCRIPTION\n.TS\nl l.\nleft\tright\t@\" ignored third source cell\n.TE\n.ec\n",
    )
    .expect("lower a table after a native .ec escape change");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let table_text = rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.blocks)
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(table_text.contains("left right"), "{table_text}");
    assert!(!table_text.contains("ignored"), "{table_text}");
}

#[test]
fn tbl_inline_recovery_recreates_the_active_document_escape_state() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-alternate-escape-inline.3"),
        b".Dd September 12, 2026\n.Dt TBL-ALTERNATE-ESCAPE-INLINE 3\n.Os\n.ec @\n.Sh DESCRIPTION\n.TS\nl.\nT{\n.No left@|right\nT}\n.TE\n.ec\n",
    )
    .expect("lower a table cell using its active escape state");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("expected one table paragraph");
    };
    assert_eq!(inline_text(children), "leftright");
}
