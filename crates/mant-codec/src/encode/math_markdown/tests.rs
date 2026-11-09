//! Source-neutral export tests, independent from roff parsing behavior.
use super::*;
use mant_ir::{
    Block, EquationAtomRole as Role, EquationExpression as Expr, EquationFont, EquationKind,
    EquationOperandScope, EquationOperator, Inline, LayoutHint,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn atom(value: &str) -> Expr {
    Expr {
        kind: EquationKind::Text,
        font: EquationFont::None,
        operator: EquationOperator::None,
        atom_role: Role::Identifier,
        operand_scope: EquationOperandScope::Atomic,
        text: Some(value.into()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    }
}

fn inline(expression: Expr) -> Inline {
    Inline::Equation {
        value: expression.readable_text(),
        expression,
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn display(expression: Option<Expr>, value: &str) -> Block {
    Block::Equation {
        value: expression
            .as_ref()
            .map_or_else(|| value.into(), Expr::readable_text),
        expression,
        display: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn query(blocks: Vec<Block>) -> ResolvedContent {
    let mut document = crate::markdown::parse_markdown("# Math\n", None)
        .unwrap()
        .document;
    document.blocks = blocks;
    ResolvedContent {
        label: "math".into(),
        address: None,
        document: Some(document),
        tldr: None,
    }
}

fn math_events(source: &str) -> Vec<(bool, String)> {
    Parser::new_ext(source, Options::ENABLE_MATH)
        .filter_map(|event| match event {
            Event::InlineMath(body) => Some((false, body.into_string())),
            Event::DisplayMath(body) => Some((true, body.into_string())),
            _ => None,
        })
        .collect()
}

#[test]
fn default_documents_and_fragments_share_latex_and_readable_policies() {
    let nodes = vec![inline(atom("x")), inline(atom("y"))];
    let source = query(vec![paragraph(nodes.clone())]);
    let document = super::super::render_markdown(&source);
    let fragment = super::super::render_inline_fragment(
        &nodes,
        super::super::MarkdownFragmentOptions::default(),
    );
    assert_eq!(math_events(&document), math_events(&fragment));
    assert_eq!(math_events(&document).len(), 2);
    let readable = super::super::render_inline_fragment(
        &nodes,
        super::super::MarkdownFragmentOptions {
            math: super::super::MarkdownMath::Readable,
            preserve_anchors: false,
        },
    );
    assert_eq!(readable, "`xy`");
}

#[test]
fn default_export_recovers_one_large_formula_and_keeps_later_math() {
    let mut expression = atom("x");
    for _ in 0..crate::math::MAX_MATH_DEPTH {
        expression = Expr {
            kind: EquationKind::Operation,
            operator: EquationOperator::SquareRoot,
            text: None,
            atom_role: Role::Unknown,
            children: vec![expression],
            ..atom("")
        };
    }
    let source = query(vec![paragraph(vec![inline(expression), inline(atom("y"))])]);
    let artifact = super::super::render_addressable_markdown(&source);
    assert_eq!(math_events(artifact.text()), [(false, "{y}".into())]);
    assert_eq!(artifact.math_summary().fallback_formulas, 1);
    assert_eq!(
        artifact.math_issues()[0].kind,
        MarkdownMathIssueKind::EncodingLimit(crate::math::MathLimit::Depth)
    );
    assert!(!artifact.math_summary().budget_exhausted);
}

#[test]
fn default_export_does_not_apply_math_tree_limits_to_ordinary_documents() {
    let source = query(vec![paragraph(
        (0..=MAX_MARKDOWN_MATH_NODES)
            .map(|_| Inline::Text { value: "x".into() })
            .collect(),
    )]);
    let artifact = super::super::render_addressable_markdown(&source);
    assert_eq!(artifact.math_summary(), MarkdownMathSummary::default());
    assert!(
        artifact
            .text()
            .ends_with(&"x".repeat(MAX_MARKDOWN_MATH_NODES + 1))
    );
}

#[test]
fn default_export_retains_all_content_when_formula_tracking_fills() {
    let mut nodes = (0..=MAX_MARKDOWN_MATH_FORMULAS)
        .map(|_| inline(atom("x")))
        .collect::<Vec<_>>();
    nodes.push(Inline::Text {
        value: " END".into(),
    });
    let source = query(vec![paragraph(nodes)]);
    let artifact = super::super::render_addressable_markdown(&source);
    assert_eq!(
        math_events(artifact.text()).len(),
        MAX_MARKDOWN_MATH_FORMULAS
    );
    assert!(
        artifact.text().ends_with("`x` END"),
        "tracking exhaustion preserves the trailing source"
    );
    assert!(artifact.math_summary().budget_exhausted);
}

#[test]
fn temporary_projection_owners_never_reuse_a_previous_expression_cache() {
    let context = MathContext::export(false);
    let mut expression = atom("x");
    assert_eq!(context.latex(&expression, "x"), Some("{x}".into()));
    expression.text = Some("y".into());
    assert_eq!(context.latex(&expression, "y"), Some("{y}".into()));
}

#[test]
fn exhausted_tracking_still_updates_known_owners_in_the_final_readable_branch() {
    let nodes = (0..=MAX_MARKDOWN_MATH_FORMULAS)
        .map(|_| inline(atom("x")))
        .collect::<Vec<_>>();
    let context = MathContext::export(true);
    for node in &nodes {
        let Inline::Equation { expression, value } = node else {
            unreachable!("equation fixture");
        };
        context.latex(expression, value);
    }
    context.reading_inlines(&nodes, MarkdownMathIssueKind::DelimiterContext);
    let (_, summary) = context.finish().unwrap();
    assert_eq!(summary.encoded_formulas, 0);
    assert_eq!(summary.fallback_formulas, MAX_MARKDOWN_MATH_FORMULAS);
    assert!(summary.budget_exhausted);
}

#[test]
fn adjacent_formulas_stay_distinct_without_visible_seams() {
    // pulldown-cmark consumes one closer at a time for neighboring $ spans.
    assert_eq!(
        math_events("$a$$b$"),
        [(false, "a".into()), (false, "b".into())]
    );
    let source = query(vec![paragraph(vec![inline(atom("a")), inline(atom("b"))])]);
    let artifact =
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::Strict).unwrap();
    assert_eq!(
        math_events(artifact.markdown().text()),
        [(false, "{a}".into()), (false, "{b}".into())]
    );
    assert!(artifact.markdown().text().contains("${a}$${b}$"));
    assert_eq!(artifact.summary().encoded_formulas, 2);
    assert_eq!(artifact.issues(), []);
    assert_eq!(
        math_events(&super::super::render_markdown(&source)),
        math_events(artifact.markdown().text())
    );
    assert!(
        super::super::render_markdown_with_options(&source, MarkdownOptions::READABLE)
            .contains("`ab`")
    );
}

#[test]
fn math_survives_styles_links_literal_dollars_and_final_coordinates() {
    let source = query(vec![paragraph(vec![
        Inline::Text {
            value: "cost $5 ".into(),
        },
        Inline::Strong {
            children: vec![inline(atom("x"))],
        },
        Inline::Text {
            value: " and ".into(),
        },
        Inline::Link {
            target: mant_ir::LinkTarget::from_uri("https://example.com/math"),
            title: None,
            children: vec![Inline::Emphasis {
                children: vec![inline(atom("y"))],
            }],
        },
    ])]);
    let artifact =
        render_math_markdown(&source, MarkdownOptions::ADDRESSABLE, MathPolicy::Strict).unwrap();
    let text = artifact.markdown().text();
    assert!(text.contains("cost \\$5"));
    assert!(text.contains("**${x}$**"));
    assert!(text.contains("[*${y}$*](https://example.com/math)"));
    assert_eq!(math_events(text).len(), 2);
    for node in artifact.markdown().nodes() {
        assert!(text.get(node.range()).is_some());
    }
    assert_eq!(artifact.summary().encoded_formulas, 2);
}

#[test]
fn display_math_uses_an_actual_math_event_and_portable_mode_has_no_math_label() {
    let source = query(vec![display(Some(atom("x")), "")]);
    let artifact =
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::Strict).unwrap();
    assert_eq!(
        math_events(artifact.markdown().text()),
        [(true, "{x}".into())]
    );
    let portable = super::super::render_markdown_with_options(&source, MarkdownOptions::READABLE);
    assert!(portable.contains("```\nx\n```"));
    assert!(!portable.contains("```math"));
}

#[test]
fn literal_and_legacy_contexts_are_explicit_and_strict_rejects_them() {
    let source = query(vec![
        Block::Preformatted {
            children: vec![inline(atom("x"))],
            language: Some("sh".into()),
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint::default(),
            source: None,
        },
        display(None, r"\\input{unsafe}"),
    ]);
    let artifact =
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::BestEffort).unwrap();
    assert_eq!(math_events(artifact.markdown().text()), []);
    assert!(artifact.markdown().text().contains("```sh\nx\n```"));
    assert!(artifact.markdown().text().contains(r"\\input{unsafe}"));
    assert_eq!(artifact.summary().fallback_formulas, 2);
    assert_eq!(
        artifact.issues()[0].kind,
        MarkdownMathIssueKind::LiteralContext
    );
    assert_eq!(
        artifact.issues()[1].kind,
        MarkdownMathIssueKind::MissingStructure
    );
    assert!(matches!(
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::Strict),
        Err(MarkdownMathError::UnsupportedContext {
            kind: MarkdownMathIssueKind::LiteralContext,
            ..
        })
    ));
}

#[test]
fn malformed_cache_is_a_hard_error_under_both_policies() {
    let source = query(vec![paragraph(vec![Inline::Equation {
        value: "stale".into(),
        expression: atom("x"),
    }])]);
    for policy in [MathPolicy::Strict, MathPolicy::BestEffort] {
        assert!(matches!(
            render_math_markdown(&source, MarkdownOptions::default(), policy),
            Err(MarkdownMathError::InvalidEquation { .. })
        ));
    }
}

#[test]
fn semantic_reimport_and_explicit_math_cannot_be_combined() {
    let source = query(Vec::new());
    for policy in [MathPolicy::Strict, MathPolicy::BestEffort] {
        assert!(matches!(
            render_math_markdown(
                &source,
                MarkdownOptions {
                    preserve_semantics: true,
                    ..Default::default()
                },
                policy
            ),
            Err(MarkdownMathError::PreserveSemantics)
        ));
    }
}

#[test]
fn missing_structure_has_cumulative_input_limits() {
    let oversized = "x".repeat(MAX_MARKDOWN_MATH_INPUT_BYTES + 1);
    let source = query(vec![display(None, &oversized)]);
    assert!(matches!(
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::BestEffort),
        Err(MarkdownMathError::LimitExceeded {
            limit: MarkdownMathLimit::InputBytes
        })
    ));
}

#[test]
fn issue_details_are_bounded_but_counts_and_strictness_are_complete() {
    let mut opaque = atom("x");
    opaque.kind = EquationKind::Opaque;
    opaque.atom_role = Role::Unknown;
    let source = query(vec![paragraph(
        (0..MAX_MARKDOWN_MATH_ISSUES + 7)
            .map(|_| inline(opaque.clone()))
            .collect(),
    )]);
    let artifact =
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::BestEffort).unwrap();
    assert_eq!(artifact.issues().len(), MAX_MARKDOWN_MATH_ISSUES);
    assert_eq!(artifact.summary().issues, MAX_MARKDOWN_MATH_ISSUES + 7);
    assert_eq!(artifact.summary().details_omitted, 7);
    assert!(matches!(
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::Strict),
        Err(MarkdownMathError::Encoding { .. })
    ));
}

#[test]
fn rejected_candidates_do_not_commit_their_losses_or_strict_context_errors() {
    let expression = atom("x");
    let context = MathContext::new(MathPolicy::Strict);
    context.reading(&expression, "x", MarkdownMathIssueKind::DelimiterContext);
    assert!(context.latex(&expression, "x").is_some());
    let (issues, summary) = context.finish().unwrap();
    assert_eq!(issues, []);
    assert_eq!(summary.encoded_formulas, 1);
}

#[test]
fn literal_context_depth_is_bounded_before_recursive_projection() {
    let mut nodes = vec![inline(atom("x"))];
    for _ in 0..crate::math::MAX_MATH_DEPTH + 2 {
        nodes = vec![Inline::Strong { children: nodes }];
    }
    let source = query(vec![Block::Preformatted {
        children: nodes,
        language: None,
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }]);
    assert!(matches!(
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::BestEffort),
        Err(MarkdownMathError::LimitExceeded {
            limit: MarkdownMathLimit::Depth
        })
    ));
}

#[test]
fn empty_container_fanout_is_checked_before_renderer_work() {
    let source = query(vec![Block::List {
        kind: mant_ir::ListKind::Bullet,
        compact: true,
        items: (0..MAX_MARKDOWN_MATH_NODES)
            .map(|_| mant_ir::ListItem {
                blocks: Vec::new(),
                entry: None,
                source: None,
                layout: mant_ir::ListItemLayout::default(),
            })
            .collect(),
        layout: LayoutHint::default(),
        source: None,
    }]);
    assert!(matches!(
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::BestEffort),
        Err(MarkdownMathError::LimitExceeded {
            limit: MarkdownMathLimit::Nodes
        })
    ));
}

#[test]
fn table_reading_never_claims_active_math_and_reports_the_actual_context() {
    let source = query(vec![Block::Table {
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            column_borders: Vec::new(),
            cells: vec![mant_ir::TableCell {
                blocks: vec![paragraph(vec![inline(atom("x"))])],
                kind: mant_ir::TableCellKind::Text,
                break_after: false,
                column_span: 1,
                row_span: 1,
                alignment: None,
                preferred_width_columns: None,
            }],
        }],
        borders: mant_ir::TableBorders::default(),
        alignment: None,
        column_count: None,
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    }]);
    let artifact =
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::BestEffort).unwrap();
    assert_eq!(math_events(artifact.markdown().text()), []);
    assert!(artifact.markdown().text().contains("```\nx\n```"));
    assert_eq!(
        artifact.issues()[0].kind,
        MarkdownMathIssueKind::TableContext
    );
    assert!(matches!(
        render_math_markdown(&source, MarkdownOptions::default(), MathPolicy::Strict),
        Err(MarkdownMathError::UnsupportedContext {
            kind: MarkdownMathIssueKind::TableContext,
            ..
        })
    ));
}

#[test]
fn final_readable_branch_suppresses_rejected_encoding_losses() {
    let mut expression = atom("x");
    expression.kind = EquationKind::Opaque;
    expression.atom_role = Role::Unknown;
    let context = MathContext::new(MathPolicy::BestEffort);
    assert!(context.latex(&expression, "x").is_some());
    context.reading(&expression, "x", MarkdownMathIssueKind::DelimiterContext);
    let (issues, summary) = context.finish().unwrap();
    assert_eq!(summary.issues, 1);
    assert_eq!(issues[0].kind, MarkdownMathIssueKind::DelimiterContext);
}

#[test]
fn portable_backtick_rows_are_charged_before_their_framing_is_allocated() {
    let mut expression = atom(&"`\n".repeat(MAX_MARKDOWN_MATH_OUTPUT_BYTES / 16));
    expression.kind = EquationKind::Opaque;
    expression.atom_role = Role::Unknown;
    let context = MathContext::new(MathPolicy::BestEffort);
    context.reading(
        &expression,
        expression.text.as_deref().unwrap(),
        MarkdownMathIssueKind::LiteralContext,
    );
    assert!(matches!(
        context.finish(),
        Err(MarkdownMathError::LimitExceeded {
            limit: MarkdownMathLimit::OutputBytes
        })
    ));
}

fn code_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = None;
    for event in Parser::new_ext(markdown, Options::ENABLE_MATH) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => current = Some(String::new()),
            Event::Text(text) => {
                if let Some(value) = &mut current {
                    value.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let mut value = current.take().expect("matching code block");
                if value.ends_with('\n') {
                    value.pop();
                }
                blocks.push(value);
            }
            _ => {}
        }
    }
    blocks
}

fn assert_export_budget(
    source: &ResolvedContent,
    expected_encoded: usize,
    global_limit: MarkdownMathLimit,
    local_limit: crate::math::MathLimit,
) {
    let artifact = super::super::render_addressable_markdown(source);
    let summary = artifact.math_summary();
    assert!(summary.budget_exhausted);
    assert_eq!(summary.encoded_formulas, expected_encoded);
    assert_eq!(math_events(artifact.text()).len(), expected_encoded);
    assert_eq!(
        summary.formulas,
        summary.encoded_formulas + summary.fallback_formulas
    );
    let expected_readable: Vec<_> = source.document.as_ref().unwrap().blocks[expected_encoded..]
        .iter()
        .map(|block| {
            let Block::Equation { value, .. } = block else {
                panic!("independent mathematical owners");
            };
            value.clone()
        })
        .collect();
    assert_eq!(code_blocks(artifact.text()), expected_readable);
    assert_eq!(
        artifact.math_issues().last().unwrap().kind,
        MarkdownMathIssueKind::BudgetLimit(global_limit)
    );
    let expected_error = if expected_encoded == 0 {
        assert_eq!(
            artifact.math_issues()[0].kind,
            MarkdownMathIssueKind::EncodingLimit(local_limit)
        );
        MarkdownMathError::Encoding {
            formula: 0,
            error: crate::math::MathError::LimitExceeded { limit: local_limit },
        }
    } else {
        MarkdownMathError::LimitExceeded {
            limit: global_limit,
        }
    };
    for policy in [MathPolicy::Strict, MathPolicy::BestEffort] {
        let error = render_math_markdown(source, MarkdownOptions::default(), policy)
            .err()
            .expect("checked export reports the limiting resource");
        assert_eq!(error, expected_error);
    }
}

#[test]
fn production_node_budget_charges_failed_formulas_and_preserves_all_reading() {
    for (owners, nodes_per_formula, encoded) in [
        (4, crate::math::MAX_MATH_NODES, 4),
        (5, crate::math::MAX_MATH_NODES + 1, 0),
    ] {
        let mut blocks = Vec::new();
        for _ in 0..owners {
            let expression = Expr {
                kind: EquationKind::Sequence,
                atom_role: Role::Unknown,
                text: None,
                children: vec![atom("x"); nodes_per_formula - 1],
                ..atom("")
            };
            expression
                .validate(mant_ir::EquationValidationLimits::default())
                .unwrap();
            blocks.push(display(Some(expression), ""));
        }
        blocks.push(display(Some(atom("TAIL")), ""));
        assert_export_budget(
            &query(blocks),
            encoded,
            MarkdownMathLimit::Nodes,
            crate::math::MathLimit::Nodes,
        );
    }
}

#[test]
fn production_byte_budget_charges_failed_formulas_and_preserves_all_reading() {
    for (bytes_per_formula, encoded) in [
        (crate::math::MAX_MATH_INPUT_BYTES, 4),
        (crate::math::MAX_MATH_INPUT_BYTES + 1, 0),
    ] {
        let mut blocks = Vec::new();
        for _ in 0..4 {
            let expression = atom(&"x".repeat(bytes_per_formula));
            expression
                .validate(mant_ir::EquationValidationLimits::default())
                .unwrap();
            blocks.push(display(Some(expression), ""));
        }
        blocks.push(display(Some(atom("TAIL")), ""));
        assert_export_budget(
            &query(blocks),
            encoded,
            MarkdownMathLimit::InputBytes,
            crate::math::MathLimit::InputBytes,
        );
    }
}
