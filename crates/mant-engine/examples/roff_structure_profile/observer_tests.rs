//! Exact pristine-derived spans; mutations change observations, never gold.
use super::*;

#[path = "observer_tests/head_witnesses.rs"]
mod head_witnesses;

#[test]
fn a_definition_head_equation_cannot_be_relocated_to_another_ordinal_item() {
    let (source, _) = super::ordinal_labels::cases::inputs()
        .find(|(source, _)| source.contains(".EQ\n(2)\n.EN"))
        .unwrap();
    let (expected, native, mut document) = super::tests::parsed_structure(&source);
    let (observed, ir) = super::ir_profile(&document);
    assert_eq!(expected.inline_equations, 1);
    assert_eq!(expected.display_equations, 0);
    assert!(super::compare_structure(&expected, &observed, &native, &ir).is_empty());
    let mut violations = Vec::new();
    native.head_payloads.compare(&document, &mut violations);
    assert!(violations.is_empty(), "{violations:?}");

    let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
        panic!("math term must keep its definition owner");
    };
    let term = &mut items[1].terms[0].content;
    let equation = term
        .iter()
        .position(|node| matches!(node, Inline::Equation { .. }))
        .unwrap();
    let equation = term.remove(equation);
    items[0].terms[0].content.push(equation);
    native.head_payloads.compare(&document, &mut violations);
    assert!(
        violations
            .iter()
            .any(|message| message.contains("definition-head-equation")),
        "{violations:?}"
    );
}

fn cases() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!("observer_cases.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn parsed(source: &str) -> (AstStructure, AstTopology, Document) {
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, topology) = ast_profile(&report);
    let document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    (expected, topology, document)
}

const DISPATCH_HEADER: &str =
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";

#[test]
fn only_executed_item_heads_change_later_display_spacing() {
    // mdoc_term.c::termp_it_pre() skips the arguments of -item/-bullet.
    // Fixed ASCII and UTF-8 keep the space after a skipped Sm off.
    for list in ["item", "bullet", "tag -width a"] {
        for state in ["off", "on"] {
            let source = format!(
                "{DISPATCH_HEADER}.Bl -{list}\n.It Xo\n.Sm {state}\n.Xc\nBODY\n.El\n.Bl -column a b\n.It C Ta D\n.Dl Em FIRST Sy SECOND\n.El\n"
            );
            let (expected, native, mut document) = parsed(&source);
            let (observed, topology) = super::ir_profile(&document);
            assert_eq!(
                super::compare_structure(&expected, &observed, &native, &topology),
                [] as [String; 0],
                "{source}"
            );
            let Block::Table { rows, .. } = document.sections[1].blocks.last_mut().unwrap() else {
                panic!("expected outer column table");
            };
            let Block::Preformatted { children, .. } = &mut rows[0].cells[1].blocks[0] else {
                panic!("expected literal");
            };
            let visible = mant_ir::inline_plain_text(children);
            let first = if list.starts_with("tag") && state == "off" {
                "D\nFIRSTSECOND"
            } else {
                "D\nFIRST SECOND"
            };
            assert_eq!(visible, first, "{source}");
            if first.contains(' ') {
                let space = children
                    .iter_mut()
                    .find_map(|inline| match inline {
                        Inline::Text { value } if value == " " => Some(value),
                        _ => None,
                    })
                    .unwrap();
                space.clear();
                let (observed, topology) = super::ir_profile(&document);
                let issues = super::compare_structure(&expected, &observed, &native, &topology);
                assert!(
                    issues
                        .iter()
                        .any(|issue| issue.starts_with("literal-displays:")),
                    "{source}: {issues:?}"
                );
            }
        }
    }
}

#[test]
fn structures_inside_executed_heads_are_checked_at_the_term() {
    // Fixed tree retains both forms. The terminal and HTML visit the
    // tag HEAD, and skip the same children in a marker list's HEAD.
    for list in ["item", "tag -width a"] {
        for (payload, kind) in [
            (".Bl -column a b\n.It FIRST Ta SECOND\n.El\n", "table"),
            (".Bl -tag -width a\n.It ONE\nSECOND\n.El\n", "list"),
            (".EQ\nalpha\n.EN\n", "equation"),
        ] {
            let source = format!("{DISPATCH_HEADER}.Bl -{list}\n.It Xo\n{payload}.Xc\nBODY\n.El\n");
            let (expected, native, mut document) = parsed(&source);
            let (observed, topology) = super::ir_profile(&document);
            assert_eq!(
                super::compare_structure(&expected, &observed, &native, &topology),
                [] as [String; 0],
                "{source}"
            );
            assert_eq!(expected.table_rows, 0);
            assert_eq!(expected.display_equations, 0);
            let skipped = list == "item";
            assert_eq!(
                expected.inline_equations,
                usize::from(kind == "equation" && !skipped)
            );
            assert_eq!(
                document
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("skipping all arguments")),
                skipped
            );
            if skipped {
                continue;
            }
            let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
                panic!("expected definition list");
            };
            let term = &mut items[0].terms[0].content;
            term.retain(|inline| !matches!(inline, Inline::Equation { .. }));
            for inline in term {
                if let Inline::Text { value } = inline {
                    *value = value.replace("SECOND", "");
                }
            }
            let mut issues = Vec::new();
            native.head_payloads.compare(&document, &mut issues);
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.starts_with(&format!("definition-head-{kind}"))),
                "{source}: {issues:?}"
            );
        }
    }
}

#[test]
fn missing_table_cell_in_a_head_cannot_be_replaced_by_the_same_word_in_body() {
    // Pinned HTML prints XYZ and SECOND as separate td elements of the
    // native HEAD, followed by a dd that happens to use SECOND as well.
    let source = concat!(
        ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n",
        ".Bl -tag -width Ds\n.It Xo\n.TS\ntab(|);\nl l.\nXYZ|SECOND\n.TE\n",
        ".No AFTER\n.Xc\nSECOND END\n.El\n",
    );
    let (expected, native, mut document) = parsed(source);
    assert_eq!(expected.table_rows, 0);
    assert!(native.table_rows.is_empty());
    let mut violations = Vec::new();
    native.head_payloads.compare(&document, &mut violations);
    assert!(violations.is_empty(), "{violations:?}");
    let [mant_ir::Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_mut_slice()
    else {
        panic!("expected one definition list");
    };
    let [item] = items.as_mut_slice() else {
        panic!("expected one definition");
    };
    for inline in &mut item.terms[0].content {
        if let mant_ir::Inline::Text { value } = inline {
            *value = value.replace("SECOND", "");
        }
    }
    native.head_payloads.compare(&document, &mut violations);
    assert_eq!(
        violations,
        ["definition-head-table at 9:2, occurrence 1: missing or out-of-order \"SECOND\" in term"]
    );
}

fn structured_term(source: &str) -> String {
    format!(
        "{DISPATCH_HEADER}.Bl -tag -width a\n.It Xo\n{source}.Xc\nBODY\n.El\n.Sh ENDPROBE\nDone.\n"
    )
}

fn first_term(document: &mut Document) -> &mut Vec<Inline> {
    let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
        panic!("expected definition list");
    };
    &mut items[0].terms[0].content
}

fn assert_term_observation(native: &AstTopology, document: &Document) {
    let mut violations = Vec::new();
    let reviews = native.head_payloads.compare(document, &mut violations);
    assert!(violations.is_empty(), "{violations:?}");
    assert!(reviews.is_empty());
}

#[test]
fn term_witnesses_consume_an_ordered_stream_across_mixed_parts() {
    // termp_it_pre() and termp_bd_pre() execute cells and their children
    // in order. Both fixed devices and HTML retained all four words.
    let source = structured_term(".Bl -column a b\n.It ALPHA Ta BRAVO\n.Dl CHARLIE\nDELTA\n.El\n");
    let (_, native, document) = parsed(&source);
    assert_term_observation(&native, &document);
    for (word, axis) in [
        ("ALPHA", "table"),
        ("BRAVO", "table"),
        ("CHARLIE", "literal"),
        ("DELTA", "table"),
    ] {
        let mut observed = document.clone();
        first_term(&mut observed)
            .retain(|inline| !matches!(inline, Inline::Text { value } if value == word));
        let mut violations = Vec::new();
        native.head_payloads.compare(&observed, &mut violations);
        assert!(
            violations.iter().any(|item| {
                item.starts_with(&format!("definition-head-{axis}")) && item.contains(word)
            }),
            "{word}: {violations:?}"
        );
    }
    // roff_term_pre_br() closes BRAVO before CENTRE enters the
    // same cell. The observer needs the earlier field as well.
    let source = structured_term(
        ".Bl -column a b\n.It ALPHA Ta BRAVO\n.br\nCENTRE\n.Dl CHARLIE\nDELTA\n.El\n",
    );
    let (_, native, mut document) = parsed(&source);
    assert_term_observation(&native, &document);
    first_term(&mut document)
        .retain(|inline| !matches!(inline, Inline::Text { value } if value == "BRAVO"));
    let mut violations = Vec::new();
    native.head_payloads.compare(&document, &mut violations);
    assert!(
        violations.iter().any(|issue| issue.contains("BRAVO")),
        "{violations:?}"
    );
}

#[test]
fn nested_terms_keep_execution_order_and_cannot_reuse_one_equal_word() {
    // CVS renders the inner list before the second outer item. The
    // second source writes SAME in two distinct executed structures.
    for (body, words, swap) in [
        (
            ".Bl -tag -width a\n.It ONE\nTWO\n.Bl -tag -width a\n.It THREE\nFOUR\n.El\nFIVE\n.It SIX\nSEVEN\n.El\n",
            ["THREE", "SIX"],
            true,
        ),
        (
            ".Bl -item\n.It\nSAME\n.El\n.Bl -column a\n.It SAME\n.El\n",
            ["SAME", "SAME"],
            false,
        ),
    ] {
        let (_, native, document) = parsed(&structured_term(body));
        assert_term_observation(&native, &document);
        let mut observed = document.clone();
        let term = first_term(&mut observed);
        let (word_a, word_b) = (words[0], words[1]);
        let a = term
            .iter()
            .position(|inline| matches!(inline, Inline::Text { value } if value == word_a))
            .unwrap();
        let b = term
            .iter()
            .rposition(|inline| matches!(inline, Inline::Text { value } if value == word_b))
            .unwrap();
        assert_ne!(a, b);
        if swap {
            term.swap(a, b);
        } else {
            term.remove(a);
        }
        let mut violations = Vec::new();
        native.head_payloads.compare(&observed, &mut violations);
        assert_eq!(violations.len(), 1, "{violations:?}");
        let mut observed = document.clone();
        let term = first_term(&mut observed);
        term.extend([Inline::line_break(), term[a].clone()]);
        let mut violations = Vec::new();
        native.head_payloads.compare(&observed, &mut violations);
        assert_eq!(violations.len(), 1, "{violations:?}");
    }
}

#[test]
fn term_witness_preserves_unicode_spacing_on_both_sides() {
    // Fixed UTF-8 preserves NBRSP from \~ and literal Unicode spaces.
    for (input, printed) in [
        (r"\~", '\u{a0}'),
        ("\u{2003}", '\u{2003}'),
        ("\u{2009}", '\u{2009}'),
        (" ", ' '),
    ] {
        let source = structured_term(&format!(
            ".Bl -column a b\n.It FIRST{input}SECOND Ta BRAVO\n.El\n"
        ));
        let (_, native, mut document) = parsed(&source);
        assert!(
            mant_ir::inline_plain_text(first_term(&mut document))
                .contains(&format!("FIRST{printed}SECOND"))
        );
        assert_term_observation(&native, &document);
        for node in first_term(&mut document) {
            if let Inline::Text { value } = node {
                *value = value.replace("SECOND", "");
            }
        }
        let mut violations = Vec::new();
        native.head_payloads.compare(&document, &mut violations);
        assert_eq!(violations.len(), 1, "{violations:?}");
    }
}

#[test]
fn a_literal_head_uses_its_term_rows_and_column_equations_use_their_cell() {
    // termp_bd_post() closes the second literal row in the HEAD; only
    // observations on that term can satisfy it. Inline eqn delimiters
    // and a display EQ are independently checked in the column list.
    let source = structured_term(".Bd -literal\nALPHA\nBRAVO\n.Ed\n");
    let (expected, native, mut document) = parsed(&source);
    let (ir, observed) = ir_profile(&document);
    assert!(compare_structure(&expected, &ir, &native, &observed).is_empty());
    assert_term_observation(&native, &document);
    let term = first_term(&mut document);
    let between = term
        .iter()
        .rposition(|node| matches!(node, Inline::LineBreak {}))
        .unwrap();
    term[between] = Inline::Text { value: " ".into() };
    let mut violations = Vec::new();
    native.head_payloads.compare(&document, &mut violations);
    assert!(
        violations
            .iter()
            .any(|item| item.contains("missing hard line")),
        "{violations:?}"
    );

    for (body, context) in [
        (
            ".Bl -column a b\n.It LEFT Ta RIGHT\n.Bl -tag -width a\n.It Xo\n.EQ\nalpha\n.EN\n.Xc\nbody\n.El\n.El\n",
            EquationContext::TableCell,
        ),
        (
            ".EQ\ndelim $$\n.EN\n.Bl -column a b\n.It LEFT Ta RIGHT\ntext $alpha$ tail\n.El\n",
            EquationContext::TableCell,
        ),
        (
            ".Bl -column a b\n.It LEFT Ta RIGHT\n.EQ\nalpha\n.EN\n.El\n",
            EquationContext::Display,
        ),
    ] {
        let (expected, native, document) = parsed(&format!("{DISPATCH_HEADER}{body}"));
        let (ir, observed) = ir_profile(&document);
        assert_eq!(native.equations[0].context, context);
        let violations = compare_structure(&expected, &ir, &native, &observed);
        assert!(violations.is_empty(), "{body}: {violations:?}");
        assert_term_observation(&native, &document);
    }
}

#[test]
fn recovered_ordinals_do_not_spend_a_definition_term_occurrence() {
    // Mandoc emits the three It heads at the same invocation position.
    // The first two form the independently proved ordinal list.
    let source = format!(
        "{DISPATCH_HEADER}.de XX\n.Bl -tag -width a\n.It 1.\nFIRST\n.It 2.\nSECOND\n.El\n.Bl -tag -width a\n.It Xo\n.Bl -column a b\n.It LEFT Ta RIGHT\n.El\n.Xc\nBODY\n.El\n..\n.XX\n.Sh ENDPROBE\nDone.\n"
    );
    let (_, native, document) = parsed(&source);
    assert_term_observation(&native, &document);
}

fn row_kind(kind: &mant_ir::TableRowKind) -> &'static str {
    match kind {
        mant_ir::TableRowKind::Data => "data",
        mant_ir::TableRowKind::HorizontalRule => "horizontal-rule",
        mant_ir::TableRowKind::DoubleHorizontalRule => "double-horizontal-rule",
        mant_ir::TableRowKind::LayoutRule { .. } => "layout-rule",
    }
}

#[test]
fn all_native_table_spans_keep_their_owner_order_and_kind() {
    // Every exact source ran pristine ASCII/UTF8/HTML/tree/lint first.
    // tbl_data allocates spans for standalone rules, layout rules and empty
    // data rows; term_tbl renders them even when they have no native cells.
    for case in cases()
        .into_iter()
        .filter(|case| case.get("rowKinds").is_some())
    {
        let (expected, native, document) = parsed(case["source"].as_str().unwrap());
        let (observed, lowered) = ir_profile(&document);
        let expected_kinds = case["rowKinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            native
                .table_rows
                .iter()
                .map(|row| row_kind(&row.kind))
                .collect::<Vec<_>>(),
            expected_kinds,
            "{}",
            case["id"]
        );
        assert_eq!(expected.table_rows, observed.table_rows, "{}", case["id"]);
        let mut violations = Vec::new();
        compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
        assert_eq!(violations, Vec::<String>::new(), "{}", case["id"]);
    }
}

fn two_tables() -> (AstTopology, IrTopology) {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "two-tables")
        .unwrap();
    let (_, native, document) = parsed(case["source"].as_str().unwrap());
    (native, ir_profile(&document).1)
}

#[test]
fn deleting_a_rule_cannot_shift_cells_or_be_hidden_by_another_table() {
    let (native, mut lowered) = two_tables();
    lowered.table_rows.remove(1);
    let mut violations = Vec::new();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected 3 rows, observed 2"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected kind HorizontalRule"))
    );
}

#[test]
fn missing_tables_and_repeated_source_origins_remain_observable() {
    let (native, mut lowered) = two_tables();
    let origin = (
        lowered.table_rows[0].table_source_line,
        lowered.table_rows[0].table_source_column,
    );
    for row in lowered.table_rows.iter_mut().skip(3) {
        row.table_source_line = origin.0;
        row.table_source_column = origin.1;
    }
    let mut violations = Vec::new();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("ambiguous repeated table source identity"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected table, observed none"))
    );
}

#[test]
fn layout_rule_strength_and_empty_data_are_not_interchangeable() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "layout-rule")
        .unwrap();
    let (_, native, document) = parsed(case["source"].as_str().unwrap());
    let (_, mut lowered) = ir_profile(&document);
    lowered.table_rows[1].kind = mant_ir::TableRowKind::LayoutRule {
        cells: vec![mant_ir::TableRuleCellKind::Horizontal; 2],
    };
    let mut violations = Vec::new();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected kind LayoutRule"))
    );
    lowered.table_rows[1].kind = mant_ir::TableRowKind::Data;
    violations.clear();
    compare_table_topology(&mut violations, &native.table_rows, &lowered.table_rows);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("expected kind LayoutRule"))
    );
}

#[test]
fn bounded_item_census_keeps_actual_owner_witnesses() {
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "item-census")
        .unwrap();
    let source = case["source"].as_str().unwrap();
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    let census = serde_json::to_value(structure_items::item_census(
        &report.document.root,
        &document,
    ))
    .unwrap();
    assert_eq!(census["nativeTotal"], 3);
    assert_eq!(census["irTotal"], 3);
    assert_eq!(census["nativeOmitted"], 0);
    assert_eq!(census["irOmitted"], 0);
    let rows = census["native"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["origin"]["line"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [3, 5, 7]
    );
    for (row, body) in rows.iter().zip(["FirstBody", "BulletBody", "SecondBody"]) {
        assert_eq!(row["body"], body);
        assert!(!row["ancestorIds"].as_array().unwrap().is_empty());
    }
}

fn continuation_is_retained(census: &serde_json::Value) -> bool {
    census["ir"].as_array().unwrap().iter().any(|item| {
        item["origin"]["line"] == 13 && item["body"] == "powershell: rclone cat remote:path [flags]"
    })
}

#[test]
fn headless_continuation_is_located_without_waiving_a_lower_item_count() {
    // This exact original rclone slice ran all five pristine profiles first.
    // man_term.c::pre_IP/post_IP retain its separate physical paragraph; the
    // documented reading-owner normalization attaches an empty-head IP after
    // RS to the previous tag, preserving its original body and spacing.
    let case = cases()
        .into_iter()
        .find(|case| case["id"] == "headless-continuation-owner")
        .unwrap();
    let source = case["source"].as_str().unwrap();
    let report = Parser::new(ParseOptions {
        includes: IncludePolicy::Deny,
        compression: Compression::Plain,
    })
    .parse_bytes("audit.1", source.as_bytes())
    .unwrap();
    let (expected, _) = ast_profile(&report);
    let mut document =
        mant_codec::parse_roff_bytes(std::path::Path::new("audit.1"), source.as_bytes()).unwrap();
    let observed = ir_profile(&document).0;
    // Retain the original lower category/count observation. It is not proof
    // of deletion or an automatic allowance for other unmatched owners.
    assert_eq!(
        (expected.generic_list_items, expected.definition_items),
        (2, 3)
    );
    assert_eq!(
        (observed.generic_list_items, observed.definition_items),
        (0, 4)
    );
    let census = serde_json::to_value(structure_items::item_census(
        &report.document.root,
        &document,
    ))
    .unwrap();
    assert_eq!(
        (census["nativeTotal"].as_u64(), census["irTotal"].as_u64()),
        (Some(5), Some(4))
    );
    assert_eq!(census["unmatchedNative"], json!([4]));
    assert_eq!(census["unmatchedIr"], json!([]));
    assert_eq!(census["ambiguousOrigins"], json!([]));
    assert_eq!(
        census["native"][4]["origin"],
        json!({"line": 23, "column": 2})
    );
    assert_eq!(census["native"][4]["head"], "");
    assert!(continuation_is_retained(&census));
    let Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[1] else {
        panic!("original labelled powershell owner");
    };
    let Block::Preformatted {
        children,
        source,
        layout,
        ..
    } = &items[0].description[2]
    else {
        panic!("headless command's original literal carrier");
    };
    assert_eq!(
        children,
        &[Inline::Code {
            value: "rclone cat remote:path [flags]".to_owned()
        }]
    );
    assert_eq!(source.map(|span| (span.line, span.column)), Some((25, 1)));
    assert_eq!(layout.spacing_before_lines, 1);
    items[0].description.remove(2);
    let damaged = serde_json::to_value(structure_items::item_census(
        &report.document.root,
        &document,
    ))
    .unwrap();
    // Removing actual body text leaves the same item totals: owner evidence,
    // rather than aggregate count forgiveness, detects the destructive change.
    assert_eq!(damaged["irTotal"], census["irTotal"]);
    assert!(!continuation_is_retained(&damaged));
}

#[test]
fn a_display_equation_outside_a_table_cannot_enter_a_cell() {
    // The CVS tree places EQ after the Bl's BODY, not in either It cell.
    let source = format!(
        "{DISPATCH_HEADER}.Bl -column a b\n.It LEFT Ta RIGHT\n.El\n.EQ\nx\n.EN\n.Sh ENDPROBE\nDone.\n"
    );
    let (expected, native, mut document) = parsed(&source);
    let check = |document: &Document| {
        let (observed, ir) = ir_profile(document);
        compare_structure(&expected, &observed, &native, &ir)
    };
    assert!(check(&document).is_empty());
    let blocks = &mut document.sections[1].blocks;
    let eq = blocks.remove(
        blocks
            .iter()
            .position(|block| matches!(block, Block::Equation { .. }))
            .unwrap(),
    );
    let Block::Table { rows, .. } = &mut blocks[0] else {
        panic!("expected table")
    };
    rows[0].cells[1].blocks.push(eq);
    let issues = check(&document);
    assert!(
        issues
            .iter()
            .any(|message| message.contains("no matching IR value"))
    );
    assert!(
        issues
            .iter()
            .any(|message| message.contains("unexpected occurrence"))
    );
}

#[test]
fn equation_obligations_check_order_and_multiplicity_even_inside_one_cell() {
    // roff_eqndelim() allocates x then y in the same It BODY, even when
    // both invocations are reparsed from the same source line.
    let source = format!(
        "{DISPATCH_HEADER}.EQ\ndelim $$\n.EN\n.Bl -column a b\n.It LEFT Ta RIGHT\n$x$ + $y$\n.El\n.Sh ENDPROBE\nDone.\n"
    );
    let (expected, native, document) = parsed(&source);
    let check = |document: &Document| {
        let (observed, ir) = ir_profile(document);
        compare_structure(&expected, &observed, &native, &ir)
    };
    assert!(check(&document).is_empty());
    for (change, diagnostic) in [
        (0, "out of order"),
        (1, "unexpected occurrence"),
        (2, "unexpected occurrence"),
    ] {
        let mut document = document.clone();
        let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
            panic!("expected table")
        };
        let right_cell = &mut rows[0].cells[1];
        let near = right_cell.blocks.iter().position(|block| {
            matches!(block, Block::Paragraph { children, .. } if children.iter().any(|node| matches!(node, Inline::Equation { .. })))
        }).unwrap();
        let Block::Paragraph { children, .. } = &mut right_cell.blocks[near] else {
            unreachable!()
        };
        let equation_nodes: Vec<_> = children
            .iter()
            .enumerate()
            .filter_map(|(index, node)| matches!(node, Inline::Equation { .. }).then_some(index))
            .collect();
        assert_eq!(equation_nodes.len(), 2);
        match change {
            0 => children.swap(equation_nodes[0], equation_nodes[1]),
            1 => children.push(children[equation_nodes[0]].clone()),
            _ => {
                let duplicate = children[equation_nodes[0]].clone();
                rows[0].cells[0].blocks.push(Block::Paragraph {
                    children: vec![duplicate],
                    source: None,
                    layout: Default::default(),
                    inline_layout: Default::default(),
                });
            }
        }
        let issues = check(&document);
        assert!(
            issues.iter().any(|message| message.contains(diagnostic)),
            "{change}: {issues:?}"
        );
    }
}

#[path = "observer_tests/paragraph_witnesses.rs"]
mod paragraph_witnesses;
