//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

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
        Inline::Text { .. } | Inline::Code { .. } | Inline::Anchor { .. } | Inline::LineBreak => {
            false
        }
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
