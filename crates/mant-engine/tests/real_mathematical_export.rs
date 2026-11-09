//! Complete licensed manuals exercise mathematical owners through public consumers.
#![cfg(feature = "roff")]

use std::path::PathBuf;

use mant_codec::encode::{
    MarkdownMathIssueKind, MarkdownOptions, render_addressable_markdown,
    render_markdown_with_options,
};
use mant_codec::math::{MathFormat, MathOptions, MathPolicy, encode_equation};
use mant_ir::{
    Block, EquationAtomRole, EquationExpression, EquationOperator, Inline, ResolvedContent,
    inline_plain_text, visit,
};
use mant_protocol::QueryBundle;
use pulldown_cmark::{Event, Options, Parser};
use visit::Visit as _;

struct Formula<'a> {
    expression: &'a EquationExpression,
    value: &'a str,
    display: bool,
}

#[derive(Default)]
struct Inventory<'a> {
    formulas: Vec<Formula<'a>>,
    paragraphs: Vec<&'a [Inline]>,
    literals: Vec<&'a [Inline]>,
}

impl<'a> visit::Visit<'a> for Inventory<'a> {
    fn visit_block(&mut self, block: &'a Block) {
        match block {
            Block::Equation {
                expression,
                value,
                display,
                ..
            } => self.formulas.push(Formula {
                expression: expression.as_ref().expect("native mathematical structure"),
                value,
                display: *display,
            }),
            Block::Paragraph { children, .. } => self.paragraphs.push(children),
            Block::Preformatted { children, .. } => self.literals.push(children),
            _ => {}
        }
        visit::walk_block(self, block);
    }

    fn visit_inline(&mut self, inline: &'a Inline) {
        if let Inline::Equation { expression, value } = inline {
            self.formulas.push(Formula {
                expression,
                value,
                display: false,
            });
        }
        visit::walk_inline(self, inline);
    }
}

fn fixture(file: &str) -> ResolvedContent {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/mathematical-export")
        .join(file);
    ResolvedContent {
        label: file.into(),
        address: None,
        document: Some(mant_loader::parse_manual_source(&path).unwrap()),
        tldr: None,
    }
}

fn equations(markdown: &str) -> Vec<bool> {
    Parser::new_ext(markdown, Options::ENABLE_MATH)
        .filter_map(|event| match event {
            Event::InlineMath(_) => Some(false),
            Event::DisplayMath(_) => Some(true),
            _ => None,
        })
        .collect()
}

fn expression_nodes(expression: &EquationExpression) -> Vec<&EquationExpression> {
    let mut pending = vec![expression];
    let mut nodes = Vec::new();
    while let Some(node) = pending.pop() {
        nodes.push(node);
        pending.extend(&node.children);
    }
    nodes
}

#[test]
fn real_formulas_keep_their_owners_after_json_and_mathematical_export() {
    // Every complete input ran through pristine CVS ASCII/UTF-8/HTML/tree/lint
    // before these assertions. The source-bound receipts are in manifest.json.
    // roff_eqndelim resolves inline origin; eqn_html.c::eqn_box retains each
    // nonempty root and its fraction, script, matrix, pile and fence operands.
    for (file, total, displays, fallbacks) in [
        ("posix-float.h.0p", 19, 6, 0),
        ("posix-stdint.h.0p", 15, 0, 0),
        ("posix-drand48.3p", 27, 0, 0),
        ("openbsd-erf.3", 1, 1, 0),
        ("openbsd-lgamma.3", 2, 2, 0),
        ("freebsd-stdc_bit_width.3", 1, 1, 0),
        ("illumos-eqn.1", 11, 0, 0),
        ("xorg-XDrawArc.3.gz", 10, 0, 1),
    ] {
        let original = fixture(file);
        let json = serde_json::to_vec(&QueryBundle::from(&original)).unwrap();
        let decoded: QueryBundle = serde_json::from_slice(&json).unwrap();
        let content: ResolvedContent = decoded.into();
        assert_eq!(content, original, "{file}: actual JSON round trip");
        let document = content.document.as_ref().unwrap();
        assert_eq!(mant_ir::validate_document(document), [], "{file}");
        let mut inventory = Inventory::default();
        inventory.visit_document(document);
        assert_eq!(inventory.formulas.len(), total, "{file}");
        assert_eq!(
            inventory.formulas.iter().filter(|f| f.display).count(),
            displays,
            "{file}: explicit display owners"
        );
        for formula in &inventory.formulas {
            assert_eq!(formula.expression.readable_text(), formula.value, "{file}");
            for format in [MathFormat::Mathml, MathFormat::Latex] {
                let artifact = encode_equation(
                    formula.expression,
                    MathOptions {
                        format,
                        display: formula.display,
                        policy: MathPolicy::BestEffort,
                    },
                )
                .unwrap();
                assert!(!artifact.text.is_empty(), "{file}: {format:?}");
            }
        }
        let artifact = render_addressable_markdown(&content);
        let summary = artifact.math_summary();
        assert_eq!(summary.formulas, total, "{file}");
        assert_eq!(summary.encoded_formulas, total - fallbacks, "{file}");
        assert_eq!(summary.fallback_formulas, fallbacks, "{file}");
        assert!(!summary.budget_exhausted, "{file}");
        assert_eq!(
            artifact
                .math_issues()
                .iter()
                .filter(|issue| issue.kind == MarkdownMathIssueKind::LiteralContext)
                .count(),
            fallbacks,
            "{file}: readable literal contexts"
        );
        let events = equations(artifact.text());
        assert_eq!(events.len(), total - fallbacks, "{file}");
        assert_eq!(events.iter().filter(|display| **display).count(), displays);
        let readable = render_markdown_with_options(&content, MarkdownOptions::READABLE);
        assert!(equations(&readable).is_empty(), "{file}");
    }
}

#[test]
fn posix_inline_formulas_preserve_complete_sentences_and_literal_notation() {
    // roff_eqndelim's generated EQ can start a source line while remaining
    // inline. The exact complete drand48/stdint sources and their prose ran
    // through the five pinned profiles before these ownership assertions.
    let content = fixture("posix-drand48.3p");
    let mut inventory = Inventory::default();
    inventory.visit_document(content.document.as_ref().unwrap());
    let paragraph = inventory
        .paragraphs
        .iter()
        .find(|children| inline_plain_text(children).starts_with("The parameter"))
        .unwrap();
    let text = inline_plain_text(paragraph);
    assert!(text.contains("hence 48-bit integer arithmetic is performed."));
    assert!(text.ends_with("are given by:"));
    assert_eq!(
        paragraph
            .iter()
            .filter(|inline| matches!(inline, Inline::Equation { .. }))
            .count(),
        3
    );
    assert!(inventory.formulas.iter().any(|formula| {
        expression_nodes(formula.expression)
            .iter()
            .any(|node| node.operator == EquationOperator::Subscript)
    }));
    let integer_limits = fixture("posix-stdint.h.0p");
    let mut inventory = Inventory::default();
    inventory.visit_document(integer_limits.document.as_ref().unwrap());
    assert!(inventory.formulas.iter().any(|formula| {
        expression_nodes(formula.expression).iter().any(|node| {
            node.atom_role == EquationAtomRole::Literal && node.text.as_deref() == Some("^")
        })
    }));
}

#[test]
fn illumos_matrix_keeps_authored_columns_rows_and_searchable_content() {
    // eqn_html.c's EQN_MATRIX path transposes authored columns for presentation.
    // The exact full-page native tree/HTML show x_i,1 then y_2,2 as the two rows.
    let content = fixture("illumos-eqn.1");
    let mut inventory = Inventory::default();
    inventory.visit_document(content.document.as_ref().unwrap());
    let matrix = inventory
        .formulas
        .iter()
        .flat_map(|formula| expression_nodes(formula.expression))
        .find_map(EquationExpression::matrix_view)
        .unwrap();
    let columns: Vec<Vec<_>> = matrix
        .columns
        .iter()
        .map(|column| {
            column
                .children
                .iter()
                .map(EquationExpression::readable_text)
                .collect()
        })
        .collect();
    assert_eq!(columns, [["x _ i", "y _ 2"], ["1", "2"]]);
    let query = serde_json::from_value(serde_json::json!({
        "pattern": "matrix(x _ i, 1; y _ 2, 2)", "syntax": "literal"
    }))
    .unwrap();
    assert_eq!(mant_query::search_query(&content, &query).unwrap().total, 1);
}

#[test]
fn xorg_formula_in_literal_example_retains_one_readable_code_owner() {
    // The pinned HTML wraps this formula in PRE. man_term.c's no-fill path
    // retains the complete authored literal row independently of math syntax.
    let content = fixture("xorg-XDrawArc.3.gz");
    let mut inventory = Inventory::default();
    inventory.visit_document(content.document.as_ref().unwrap());
    let literal = inventory
        .literals
        .iter()
        .find(|children| inline_plain_text(children).starts_with("skewed-angle"))
        .unwrap();
    assert_eq!(
        inline_plain_text(literal),
        "skewed-angle = atan(tan ( normal-angle ) * width / height) + adjust"
    );
    assert_eq!(
        literal
            .iter()
            .filter(|inline| matches!(inline, Inline::Equation { .. }))
            .count(),
        1
    );
}
