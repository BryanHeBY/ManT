//! Source-neutral export tests, independent from roff parsing behavior.
use super::*;
use mant_ir::{
    Block, EquationAtomRole as Role, EquationExpression as Expr, EquationFont, EquationKind,
    EquationOperandScope, EquationOperator, Inline, LayoutHint,
};
use pulldown_cmark::{Event, Options, Parser};

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
    assert!(super::super::render_markdown(&source).contains("`ab`"));
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
    let portable = super::super::render_markdown(&source);
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
