//! Owned table/column payloads and normalized equations.

use super::*;

#[test]
fn parser_preserves_infix_eqn_operators() {
    let report = Parser::default()
        .parse_bytes(
            "equation.3",
            b".TH EQUATION 3\n.SH DESCRIPTION\n.EQ\nx + {width over 2}\ny sub 1 sup 2\n.EN\n",
        )
        .expect("parse infix eqn operators");
    let equation = find_kind(&report.document.root, NodeKind::Equation)
        .and_then(|node| node.equation.as_ref())
        .expect("native equation")
        .normalized_text();

    assert!(equation.contains("width / 2"), "{equation}");
    assert!(equation.contains("y _ 1 ^ 2"), "{equation}");
}

#[test]
fn parser_normalizes_the_common_gnu_ldots_equation_macro() {
    let report = Parser::default()
        .parse_bytes(
            "equation-ldots.3",
            b".TH EQUATION 3\n.SH DESCRIPTION\n.EQ\nx sub 1 ldots x sub n\n.EN\n",
        )
        .expect("parse GNU ldots equation macro");
    let equation = find_kind(&report.document.root, NodeKind::Equation)
        .and_then(|node| node.equation.as_ref())
        .expect("native equation")
        .normalized_text();

    assert_eq!(equation, "x _ 1 ... x _ n");
}

#[test]
fn parser_preserves_eqn_decorations_from_native_boxes() {
    let report = Parser::default()
        .parse_bytes(
            "equation-decorators.1",
            b".TH EQUATION 1\n.SH DESCRIPTION\n.EQ\nx dot = f(t) bar\ny dotdot bar ~=~ n under\nx vec ~=~ y dyad\n.EN\n",
        )
        .expect("parse decorated equations");
    let equations = find_kind(&report.document.root, NodeKind::Equation)
        .and_then(|node| node.equation.as_ref())
        .expect("native equation")
        .normalized_text();

    // CVS eqn.c records these on eqn_box::top/bottom, not as children.
    // Keep their resolved native spellings in the owned AST so downstream
    // lowering can apply the same character catalog as ordinary roff text.
    for decorator in [r"\[a.]", r"\[rn]", r"\[ad]", r"\[->]", r"\[<>]"] {
        assert!(equations.contains(decorator), "{decorator}: {equations}");
    }
    assert!(equations.contains("n_"), "{equations}");
}

#[test]
fn parser_preserves_native_equation_structure_and_empty_roots() {
    // Fixed CVS `eqn.h` retains artificial roots, native box kinds, fonts,
    // positions, fences, and decorations independently of terminal output.
    // Oracle checked first with:
    // target/mandoc-migration/reference/mandoc -Ttree
    // target/k21-oracles/exact-typed-tree.1
    let report = Parser::default()
        .parse_bytes(
            "equation-structure.1",
            b".TH EQN 1\n.SH BODY\n.EQ\nleft ( bold x sub 1 sup 2 over sqrt { italic y } right )\n.EN\n.EQ\nx dot under\n.EN\n.EQ\ndelim $$\n.EN\n",
        )
        .expect("parse structured equations");
    let mut equations = Vec::new();
    collect_equations(&report.document.root, &mut equations);
    assert_eq!(equations.len(), 3);

    let expression = &equations[0].root.children[0];
    assert_eq!(expression.kind, EquationBoxKind::List);
    assert_eq!(expression.left.as_deref(), Some("("));
    assert_eq!(expression.right.as_deref(), Some(")"));
    let fraction = &expression.children[0];
    assert_eq!(fraction.position, EquationPosition::Over);
    assert_eq!(
        fraction.children[0].position,
        EquationPosition::SubscriptSuperscript
    );
    assert_eq!(fraction.children[0].children[0].font, EquationFont::Bold);
    assert_eq!(fraction.children[1].position, EquationPosition::SquareRoot);
    assert_eq!(
        fraction.children[1].children[0].children[0].font,
        EquationFont::Italic
    );

    let decorated = &equations[1].root.children[0];
    assert_eq!(decorated.bottom.as_deref(), Some(r"\[ul]"));
    assert_eq!(decorated.children[0].top.as_deref(), Some(r"\[a.]"));

    let configuration = &equations[2].root;
    assert!(configuration.children.is_empty());
    assert_eq!(configuration.actual_args, 0);
}

fn collect_equations<'a>(node: &'a Node, output: &mut Vec<&'a Equation>) {
    if let Some(equation) = &node.equation {
        output.push(equation);
    }
    for child in &node.children {
        collect_equations(child, output);
    }
}

#[test]
fn parser_marks_tbl_text_block_cells() {
    let path = source_path("tbl-text-block");
    fs::write(
        &path,
        ".Dd August 19, 2026\n.Dt TBL-TEXT-BLOCK 3\n.Os\n.Sh NAME\n.Nm demo\n.Nd demo\n.Sh ATTRIBUTES\n.TS\nallbox;\nl l.\nInterface\tValue\nT{\n.Nm\nT}\tMT-Safe\n.TE\n",
    )
    .expect("write tbl text block source");
    let document = parse_file(&path, false).expect("parse tbl text block source");
    fs::remove_file(path).expect("remove tbl text block source");
    let row = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.table_cells.iter().any(|cell| cell.text_block)
    })
    .expect("tbl row containing a text block");
    assert_eq!(row.table_cells.len(), 2);
    assert_eq!(row.table_cells[0].text.as_deref(), Some(""));
    assert!(row.table_cells[0].text_block);
    assert!(!row.table_cells[1].text_block);
}

#[test]
fn parser_retains_the_parse_time_value_of_tbl_text_block_strings() {
    let document = Parser::default()
        .parse_bytes(
            "tbl-expanded-string.1",
            b".TH TBL-EXPANDED-STRING 1\n.ds Aq \\(aq\n.SH DESCRIPTION\n.TS\nl.\nT{\nThere\\*(Aqs\nT}\n.TE\n",
        )
        .expect("parse tbl source with a string expansion")
        .document;
    let row = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.table_cells.iter().any(|cell| cell.text_block)
    })
    .expect("tbl row containing a text block");
    assert!(row.table_cells[0].text_block);
    // roff.c expands the string before tbl_read() persists tbl_dat::string,
    // while retaining the resulting named-character escape for later output.
    // This is native evaluated content, not source spelling that a later
    // parser may safely reinterpret without the original string table.
    assert_eq!(row.table_cells[0].text.as_deref(), Some(r"There\(aqs"));
}

#[test]
fn parser_marks_both_tbl_vertical_continuation_forms() {
    let document = Parser::default()
        .parse_bytes(
            "tbl-vertical-continuations.1",
            b".TH TBL-VERTICAL-CONTINUATIONS 1\n.SH TABLES\n.TS\nl l.\nfirst\tvalue\n\\^\tcontinued\n.TE\n.TS\nl l,\n^ l.\nfirst\tvalue\n\tcontinued\n.TE\n",
        )
        .expect("parse tbl vertical continuations")
        .document;

    let explicit = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.line == 6
    })
    .expect("explicit continuation row");
    assert!(explicit.table_cells[0].vertical_continuation);

    let layout = find_node(&document.root, &|node| {
        node.kind == NodeKind::Table && node.line == 12
    })
    .expect("layout continuation row");
    assert!(layout.table_cells[0].vertical_continuation);
}

#[test]
fn parser_retains_column_list_cells() {
    let report = Parser::default()
        .parse_bytes(
            "columns.3",
            b".Dd August 19, 2026\n.Dt COLUMNS 3\n.Os\n.Sh DESCRIPTION\n\
.Bl -column name type description\n.It Dv CLSET_TIMEOUT Ta \"struct timeval *\" Ta \"set total timeout\"\n.El\n",
        )
        .expect("parse mdoc column list");
    let item = find_macro(&report.document.root, "It").expect("column item");
    let bodies = item
        .children
        .iter()
        .filter(|child| child.kind == NodeKind::Body)
        .collect::<Vec<_>>();

    assert_eq!(bodies.len(), 3);
    assert_eq!(
        bodies
            .iter()
            .map(|body| {
                body.children
                    .iter()
                    .flat_map(|child| child.children.iter())
                    .chain(body.children.iter())
                    .filter_map(|child| child.text.as_deref())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [
            vec!["CLSET_TIMEOUT"],
            vec!["struct timeval *"],
            vec!["set total timeout"],
        ]
    );
}

#[test]
fn parser_copies_table_cells_and_equation_text() {
    let path = source_path("structured-payload-mandoc-session");
    fs::write(
        &path,
        ".TH PAYLOAD 1\n.SH TABLE\n.TS\ntab(|);\nl r.\nleft|right\n.TE\n\
         .SH EQUATION\n.EQ\nx sup 2\n.EN\n",
    )
    .expect("write table and equation source");

    let document = parse_file(&path, false).expect("parse table and equation source");
    fs::remove_file(path).expect("remove table and equation source");

    let table = find_kind(&document.root, NodeKind::Table).expect("table row node");
    assert_eq!(table.table_cells.len(), 2);
    assert_eq!(table.table_cells[0].text.as_deref(), Some("left"));
    assert_eq!(table.table_cells[1].alignment, TableAlignment::Right);
    let equation = find_kind(&document.root, NodeKind::Equation).expect("equation node");
    assert!(
        equation
            .equation
            .as_ref()
            .is_some_and(|value| value.normalized_text().contains('x'))
    );
}

#[test]
fn parser_distinguishes_empty_tbl_data_rows_from_whole_row_rules() {
    fn collect(node: &Node, rows: &mut Vec<(TableRowKind, usize)>) {
        if node.kind == NodeKind::Table {
            rows.push((
                node.table_row_kind.clone().expect("native table row kind"),
                node.table_cells.len(),
            ));
        }
        for child in &node.children {
            collect(child, rows);
        }
    }

    // Fixed CVS tbl_data.c creates an empty TBL_SPAN_DATA for the blank input
    // line and distinct HORIZ/DHORIZ spans for `_` and `=`.
    let document = Parser::default()
        .parse_bytes(
            "tbl-empty-and-rules.1",
            b".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl.\nBEFORE\n\n_\n=\nAFTER\n.TE\n",
        )
        .expect("parse empty and ruled tbl rows")
        .document;
    let mut rows = Vec::new();
    collect(&document.root, &mut rows);
    assert_eq!(
        rows,
        [
            (TableRowKind::Data, 1),
            (TableRowKind::Data, 0),
            (TableRowKind::HorizontalRule, 0),
            (TableRowKind::DoubleHorizontalRule, 0),
            (TableRowKind::Data, 1),
        ]
    );
}

#[test]
fn parser_preserves_layout_only_rule_rows_per_column() {
    fn collect(node: &Node, rows: &mut Vec<TableRowKind>) {
        if let Some(kind) = &node.table_row_kind {
            rows.push(kind.clone());
        }
        for child in &node.children {
            collect(child, rows);
        }
    }

    // Fixed CVS tbl_data.c creates these as TBL_SPAN_DATA rows with no
    // tbl_dat cells; tbl_term.c renders the associated layout cells.
    let document = Parser::default()
        .parse_bytes(
            "tbl-layout-rule-rows.1",
            b".TH PROBE 1\n.SH DESCRIPTION\n.TS\n_\nl.\nSINGLE\n.TE\n.TS\n=\nl.\nDOUBLE\n.TE\n.TS\n_ =\nl l.\nLEFT\tRIGHT\n.TE\n.TS\n_.\nIGNORED\n.TE\n.TS\n=.\nIGNORED\n.TE\n.TS\n_ =.\nLEFT\tRIGHT\n.TE\n",
        )
        .expect("parse layout-only rule rows")
        .document;
    let mut rows = Vec::new();
    collect(&document.root, &mut rows);
    assert_eq!(
        rows,
        [
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::Horizontal],
            },
            TableRowKind::Data,
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::DoubleHorizontal],
            },
            TableRowKind::Data,
            TableRowKind::LayoutRule {
                cells: vec![
                    TableRuleCellKind::Horizontal,
                    TableRuleCellKind::DoubleHorizontal,
                ],
            },
            TableRowKind::Data,
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::Horizontal],
            },
            TableRowKind::LayoutRule {
                cells: vec![TableRuleCellKind::DoubleHorizontal],
            },
            TableRowKind::LayoutRule {
                cells: vec![
                    TableRuleCellKind::Horizontal,
                    TableRuleCellKind::DoubleHorizontal,
                ],
            },
        ]
    );
}

#[test]
fn parser_marks_a_leading_rule_as_the_start_of_its_native_table() {
    fn collect(node: &Node, rows: &mut Vec<(TableRowKind, bool)>) {
        if node.kind == NodeKind::Table {
            rows.push((
                node.table_row_kind.clone().expect("native table row kind"),
                node.flags.table_start,
            ));
        }
        for child in &node.children {
            collect(child, rows);
        }
    }

    // Fixed CVS tbl_data.c links `_` and the following data row into one
    // tbl_node span chain.  The rule is the first span; T& only restarts the
    // layout of the existing chain.
    let document = Parser::default()
        .parse_bytes(
            "tbl-leading-rule-boundary.1",
            b".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl.\nFIRST\n.T&\nr.\nSECOND\n.TE\n.TS\nl.\n_\nTHIRD\n.TE\n",
        )
        .expect("parse adjacent tables with a leading rule")
        .document;
    let mut rows = Vec::new();
    collect(&document.root, &mut rows);
    assert_eq!(
        rows,
        [
            (TableRowKind::Data, true),
            (TableRowKind::Data, false),
            (TableRowKind::HorizontalRule, true),
            (TableRowKind::Data, false),
        ]
    );
}

#[test]
fn parser_records_whether_tbl_content_bypassed_user_macro_execution() {
    for (label, source, expected) in [
        (
            "direct",
            b".TH PROBE 1\n.SH DESCRIPTION\n.TS\nl.\nT{\n.MR printf 3\nT}\n.TE\n".as_slice(),
            true,
        ),
        (
            "redefined",
            b".TH PROBE 1\n.de MR\nprintf 3\n..\n.SH DESCRIPTION\n.TS\nl.\nT{\n.MR printf 3\nT}\n.TE\n".as_slice(),
            false,
        ),
        (
            "appended",
            b".TH PROBE 1\n.am1 B\nADDED_TEXT\n..\n.SH DESCRIPTION\n.TS\nl.\nT{\n.B ORIGINAL_OPERAND\nT}\n.TE\n".as_slice(),
            false,
        ),
        (
            "custom-control",
            b".TH PROBE 1\n.SH DESCRIPTION\n.cc @\n@TS\nl.\nT{\n@MR printf 3\nT}\n@TE\n".as_slice(),
            false,
        ),
    ] {
        let report = Parser::default()
            .parse_bytes(format!("tbl-provenance-{label}.1"), source)
            .expect("parse tbl provenance fixture");
        let row = find_kind(&report.document.root, NodeKind::Table).expect("table row");
        assert_eq!(row.table_source_recovery_safe, expected, "{label}");
    }
}

#[test]
fn parser_records_direct_mdoc_table_macro_provenance() {
    for source in [
        b".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.TS\nl.\nT{\n.Oo Fl a Oc No TOKENA\nT}\n.TE\n".as_slice(),
        b".Dd September 5, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.TS\nl.\nT{\n.MR printf 3\nT}\n.TE\n".as_slice(),
    ] {
        let report = Parser::default()
            .parse_bytes("tbl-direct-mdoc.1", source)
            .expect("parse direct mdoc table macro");
        let row = find_kind(&report.document.root, NodeKind::Table).expect("table row");
        assert!(row.table_source_recovery_safe, "{row:#?}");
    }
}

#[test]
fn parser_tracks_source_recovery_provenance_per_tbl_text_block() {
    for (label, definition) in [
        ("empty", ".de Fl\n..\n"),
        ("return", ".de Fl\n.return\n..\n"),
    ] {
        let source = format!(
            ".Dd September 12, 2026\n.Dt PROBE 1\n.Os\n{definition}.Sh DESCRIPTION\n.TS\nl l.\nT{{\n.Fl\nhelp\nT}}\tT{{\n.Em WORD\nT}}\n.TE\n"
        );
        let report = Parser::default()
            .parse_bytes(format!("tbl-empty-macro-{label}.1"), source.as_bytes())
            .expect("parse tbl cell after an empty user macro")
            .document;
        let row = find_kind(&report.root, NodeKind::Table).expect("tbl row");
        assert_eq!(row.table_cells.len(), 2, "{label}: {row:#?}");
        assert!(row.table_cells[0].text_block, "{label}: {row:#?}");
        assert!(
            !row.table_cells[0].source_recovery_safe,
            "{label}: {row:#?}"
        );
        assert!(row.table_cells[1].text_block, "{label}: {row:#?}");
        assert!(row.table_cells[1].source_recovery_safe, "{label}: {row:#?}");
    }
}
