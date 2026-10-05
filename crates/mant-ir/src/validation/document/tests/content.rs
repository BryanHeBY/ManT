//! Existing content invariant contracts.
use super::*;

#[test]
fn rejects_equation_text_caches_that_disagree_with_the_structure() {
    let expression = crate::EquationExpression {
        kind: crate::EquationKind::Text,
        font: crate::EquationFont::None,
        position: crate::EquationPosition::None,
        size: None,
        expected_args: Some(0),
        actual_args: 0,
        summarized_operand_group: false,
        text: Some("x".into()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: Vec::new(),
    };
    let document = document(
        Vec::new(),
        vec![
            Block::Equation {
                value: "other".into(),
                expression: Some(expression.clone()),
                display: true,
                layout: LayoutHint::default(),
                source: None,
            },
            Block::Paragraph {
                inline_layout: crate::InlineLayout::default(),
                children: vec![Inline::Equation {
                    value: "other".into(),
                    expression,
                }],
                layout: LayoutHint::default(),
                source: None,
            },
        ],
    );
    let restored: Document =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    let findings = validate_document(&restored);
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.code.as_deref() == Some("ir.equation-projection-mismatch"))
            .count(),
        2
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding.impact == crate::DiagnosticImpact::ContentCoverage)
    );
}

#[test]
fn rejects_rule_rows_with_data_or_without_layout_strengths() {
    let blocks = vec![Block::Table {
        column_preferences: crate::ColumnPreferences::default(),
        rows: vec![
            TableRow {
                kind: crate::TableRowKind::HorizontalRule,
                cells: vec![TableCell {
                    break_after: false,
                    kind: crate::TableCellKind::Text,
                    blocks: Vec::new(),
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            },
            TableRow {
                kind: crate::TableRowKind::LayoutRule { cells: Vec::new() },
                cells: Vec::new(),
            },
            TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![TableCell {
                    break_after: false,
                    kind: crate::TableCellKind::HorizontalRule,
                    blocks: vec![Block::Paragraph {
                        inline_layout: crate::InlineLayout::default(),
                        children: Vec::new(),
                        layout: LayoutHint::default(),
                        source: None,
                    }],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let diagnostics = validate_document(&document(Vec::new(), blocks));
    let codes = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ir.invalid-table-rule-cells"), "{codes:?}");
    assert!(codes.contains(&"ir.empty-table-layout-rule"), "{codes:?}");
    assert!(
        codes.contains(&"ir.invalid-table-rule-content"),
        "{codes:?}"
    );
}

#[test]
fn validates_source_spans_owned_by_document_diagnostics() {
    let source = SourceSpan {
        byte_range: Some(TextRange {
            start: TextSize::new(8),
            end: TextSize::new(3),
        }),
        line: 0,
        column: 0,
        end_line: Some(0),
        end_column: Some(0),
    };
    let mut document = document(Vec::new(), Vec::new());
    document.diagnostics.push(Diagnostic {
        impact: crate::DiagnosticImpact::None,
        level: DiagnosticLevel::Warning,
        code: Some("producer.finding".to_owned()),
        message: "producer finding".to_owned(),
        source: Some(source),
    });

    let codes = validate_document(&document)
        .into_iter()
        .filter_map(|diagnostic| diagnostic.code)
        .collect::<Vec<_>>();
    assert!(
        codes
            .iter()
            .any(|code| code == "ir.invalid-source-position")
    );
    assert!(codes.iter().any(|code| code == "ir.reverse-source-range"));
}

#[test]
fn reports_invalid_cross_document_entry_domains() {
    let mut definition = DefinitionItem {
        head_body_relation: HeadBodyRelation::from(false),
        source: None,
        entry: Some(EntryFacts {
            name_bindings: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: Vec::new(),
            id: "option-output".into(),
            kind: EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["--output".to_owned()],
            value_domain: Some(crate::ValueDomain::EntrySet {
                reference: crate::DocumentReference::Manual {
                    name: String::new(),
                    manual_section: Some(String::new()),
                },
                entry_kinds: Vec::new(),
                source: None,
            }),
        }),
        terms: (vec![vec![Inline::anchor("option-output")]])
            .into_iter()
            .map(Into::into)
            .collect(),
        description: Vec::new(),
        layout: crate::DefinitionLayout {
            body_alignment: crate::DefinitionBodyAlignment::Indented,
            spacing_before_lines: None,
            ..Default::default()
        },
    };
    let blocks = vec![Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![definition.clone()],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }];
    let diagnostics = validate_document(&document(Vec::new(), blocks));
    let codes = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ir.empty-semantic-document-reference"));
    assert!(codes.contains(&"ir.empty-entry-value-domain"));

    definition.entry.as_mut().expect("identity").value_domain =
        Some(crate::ValueDomain::EntrySet {
            reference: crate::DocumentReference::Manual {
                name: "ssh_config".to_owned(),
                manual_section: Some("qgroup".to_owned()),
            },
            entry_kinds: vec![
                crate::EntryKind::ConfigurationKey,
                crate::EntryKind::ConfigurationKey,
            ],
            source: None,
        });
    let diagnostics = validate_document(&document(
        Vec::new(),
        vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![definition],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }],
    ));
    let codes = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ir.invalid-semantic-document-reference"));
    assert!(codes.contains(&"ir.duplicate-entry-value-kind"));
}

#[test]
fn classifies_only_semantic_invariant_diagnostics_as_incomplete() {
    for code in [
        "ir.invalid-identity",
        "ir.ambiguous-fragment-alias",
        "ir.invalid-semantic-document-reference",
        "ir.empty-entry-value-domain",
    ] {
        assert!(is_semantic_completeness_diagnostic(code), "{code}");
    }
    for code in [
        "ir.invalid-table-span",
        "ir.invalid-source-position",
        "ir.invalid-external-uri",
    ] {
        assert!(!is_semantic_completeness_diagnostic(code), "{code}");
    }
}
