//! Pure IR row positions, owner validation and coordinate-domain contracts.

use super::*;
use crate::{Block, Heading, LayoutHint};
use serde_json::json;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn layout(rows: &[(u32, i32)]) -> InlineLayout {
    InlineLayout {
        row_hints: rows
            .iter()
            .map(|&(row, indent_columns)| RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}

fn equation(value: &str) -> Inline {
    Inline::Equation {
        value: value.into(),
        expression: crate::EquationExpression {
            kind: crate::EquationKind::Text,
            font: crate::EquationFont::None,
            position: crate::EquationPosition::None,
            size: None,
            expected_args: Some(0),
            actual_args: 0,
            summarized_operand_group: false,
            text: Some(value.into()),
            left: None,
            right: None,
            top: None,
            bottom: None,
            children: vec![],
        },
    }
}

#[test]
fn logical_positions_include_one_open_tail_without_inventing_content() {
    for (content, positions, plain) in [
        (vec![], 1, ""),
        (vec![Inline::anchor("target")], 1, ""),
        (vec![text("A")], 1, "A"),
        (vec![text("A\n")], 2, "A\n"),
        (vec![text("A\n\n")], 3, "A\n\n"),
    ] {
        assert_eq!(logical_row_count(&content), positions);
        assert_eq!(crate::inline_plain_text(&content), plain);
        assert!(layout(&[(0, 4)]).validate(&content).is_ok());
        assert!(
            layout(&[(positions.try_into().unwrap(), 4)])
                .validate(&content)
                .is_err()
        );
    }
}

#[test]
fn every_inline_text_leaf_and_transparent_wrapper_shares_the_row_cursor() {
    let content = vec![Inline::Strong {
        children: vec![
            text("T\n"),
            Inline::Emphasis {
                children: vec![
                    Inline::Code {
                        value: "C\n".into(),
                    },
                    Inline::Link {
                        target: crate::LinkTarget::External {
                            uri: "https://example.org".into(),
                        },
                        title: None,
                        children: vec![
                            equation("E\n"),
                            Inline::line_break(),
                            Inline::anchor("tail"),
                            text("Z"),
                        ],
                    },
                ],
            },
        ],
    }];
    assert_eq!(logical_row_count(&content), 5);
    assert_eq!(crate::inline_plain_text(&content), "T\nC\nE\n\nZ");
    assert!(layout(&[(4, -3)]).validate(&content).is_ok());
    assert!(layout(&[(5, -3)]).validate(&content).is_err());
}

#[test]
fn sparse_hints_are_local_and_zero_is_omitted_from_owner_output() {
    let hints = layout(&[(0, 0), (1, 6), (3, -2)]);
    let content = vec![text("A\nB\nC\nD")];
    hints.validate(&content).unwrap();
    assert_eq!(
        (0..5).map(|row| hints.row_indent(row)).collect::<Vec<_>>(),
        [0, 6, 0, -2, 0]
    );
    assert_eq!(
        serde_json::to_value(&hints).unwrap(),
        json!({
            "rowHints": [{"row":1,"indentColumns":6}, {"row":3,"indentColumns":-2}]
        })
    );
    let heading = Heading {
        content: vec![text("A")],
        inline_layout: layout(&[(0, 0)]),
        source: None,
    };
    assert!(heading.inline_layout.is_empty());
    let encoded = serde_json::to_value(&heading).unwrap();
    assert!(encoded.get("inlineLayout").is_none());
    let restored: Heading = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored.inline_layout.row_hints, []);
    assert_eq!(restored.content, heading.content);
}

#[test]
fn duplicate_unordered_and_excess_displacements_are_rejected_before_normalizing() {
    let content = vec![text("A\nB\nC")];
    for hints in [
        layout(&[(0, 0), (0, 0)]),
        layout(&[(1, 3), (1, 4)]),
        layout(&[(2, 3), (1, 4)]),
        layout(&[(0, -MAX_ROW_INDENT_COLUMNS - 1)]),
        layout(&[(0, MAX_ROW_INDENT_COLUMNS + 1)]),
    ] {
        assert!(hints.validate(&content).is_err());
        let wire = json!({ "rowHints": hints.row_hints });
        assert!(serde_json::from_value::<InlineLayout>(wire).is_err());
    }
    for correction in [-MAX_ROW_INDENT_COLUMNS, MAX_ROW_INDENT_COLUMNS] {
        let accepted = layout(&[(0, correction)]);
        accepted.validate(&content).unwrap();
        assert_eq!(
            serde_json::from_value::<InlineLayout>(json!({ "rowHints": accepted.row_hints }))
                .unwrap(),
            accepted
        );
    }
    for invalid in [
        json!({"rowHints":[{"row":-1,"indentColumns":0}]}),
        json!({"rowHints":[{"row":0,"indentColumns":null}]}),
        json!({"rowHints":[{"row":0,"indentColumns":1.5}]}),
        json!({"rowHints":[{"row":0,"indentColumns":1,"offset":1}]}),
        json!({"rowHints":[],"other":0}),
    ] {
        assert!(serde_json::from_value::<InlineLayout>(invalid).is_err());
    }
}

#[test]
fn hint_budget_counts_explicit_zero_and_accepts_the_exact_boundary() {
    let content = vec![text(&"\n".repeat(MAX_INLINE_ROW_HINTS - 1))];
    let mut hints = InlineLayout {
        row_hints: (0..MAX_INLINE_ROW_HINTS)
            .map(|row| RowLayoutHint {
                row: row.try_into().unwrap(),
                indent_columns: 0,
            })
            .collect(),
    };
    hints.validate(&content).unwrap();
    let wire = json!({"rowHints": hints.row_hints});
    assert_eq!(
        serde_json::from_value::<InlineLayout>(wire)
            .unwrap()
            .row_hints
            .len(),
        MAX_INLINE_ROW_HINTS
    );
    hints.row_hints.push(RowLayoutHint {
        row: MAX_INLINE_ROW_HINTS.try_into().unwrap(),
        indent_columns: 0,
    });
    assert!(hints.validate(&content).is_err());
    assert!(serde_json::from_value::<InlineLayout>(json!({"rowHints":hints.row_hints})).is_err());
}

#[test]
fn zero_outside_the_owner_cannot_bypass_checked_deserialization() {
    let invalid = json!({"rowHints":[{"row":1,"indentColumns":0}]});
    let hints: InlineLayout = serde_json::from_value(invalid.clone()).unwrap();
    assert!(hints.is_empty());
    assert!(hints.validate(&[]).is_err());
    assert!(
        serde_json::from_value::<Heading>(json!({"content":[],"inlineLayout":invalid})).is_err()
    );
    assert!(
        serde_json::from_value::<DefinitionTerm>(json!({"content":[],"inlineLayout":invalid}))
            .is_err()
    );
    for kind in ["paragraph", "preformatted"] {
        assert!(
            serde_json::from_value::<Block>(
                json!({"type":kind,"children":[],"inlineLayout":invalid})
            )
            .is_err()
        );
    }
}

#[test]
fn unicode_scalar_slices_rebase_hints_without_counting_utf8_bytes() {
    let content = vec![Inline::Emphasis {
        children: vec![
            text("é名\n"),
            Inline::Code {
                value: "中\n".into(),
            },
            equation("β\n"),
            text("Z"),
        ],
    }];
    let hints = layout(&[(0, 2), (1, 4), (2, -2), (3, 6)]);
    let view = InlineContentRef {
        content: &content,
        layout: &hints,
    };
    assert_eq!(crate::inline_scalar_len(&content), 8);
    assert_eq!(
        view.sliced_layout(3..7),
        Some(layout(&[(0, 4), (1, -2), (2, 6)]))
    );
    assert_eq!(view.sliced_layout(3..4), Some(layout(&[(0, 4)])));
    assert_eq!(
        view.sliced_layout(2..3),
        Some(layout(&[(0, 2), (1, 4)])),
        "a selected newline retains both its closed row and open tail"
    );
    assert_eq!(view.sliced_layout(7..8), Some(layout(&[(0, 6)])));
    assert!(view.sliced_layout(8..9).is_none());
    assert!(
        view.sliced_layout(std::ops::Range { start: 4, end: 3 })
            .is_none()
    );
    assert_eq!(crate::inline_plain_text(&content), "é名\n中\nβ\nZ");
}

#[test]
fn dual_visual_origins_compose_signed_corrections_before_final_padding() {
    for (first, continuation, correction, expected) in [
        (2, 6, 0, (2, 6)),
        (2, 6, 3, (5, 9)),
        (2, 6, -3, (-1, 3)),
        (-2, 2, 3, (1, 5)),
    ] {
        let origins = resolve_row_origins(first, continuation, correction);
        assert_eq!(
            (origins.first_visual_origin, origins.continuation_origin),
            expected
        );
    }
    let origins = resolve_row_origins(2, 6, -3);
    assert_eq!(
        (
            crate::geometry::padding(origins.first_visual_origin),
            crate::geometry::padding(origins.continuation_origin)
        ),
        (0, 3)
    );
    assert_eq!(
        resolve_row_origins(i32::MAX, i32::MIN, 1),
        RowOrigins {
            first_visual_origin: i32::MAX,
            continuation_origin: i32::MIN + 1
        }
    );
}

#[test]
fn validator_rechecks_every_mutated_inline_owner_instead_of_trusting_construction() {
    struct RemoveBreaks;
    impl crate::visit::VisitMut for RemoveBreaks {
        fn visit_inline_mut(&mut self, node: &mut Inline) {
            if let Inline::Text { value } = node {
                value.retain(|character| character != '\n');
            }
            crate::visit::walk_inline_mut(self, node);
        }
    }
    let heading = || Heading {
        content: vec![text("A\nB")],
        inline_layout: layout(&[(1, 0)]),
        source: None,
    };
    let mut document = crate::Document {
        parser: None,
        source: crate::DocumentSource {
            format: crate::SourceFormat::Markdown,
            path: None,
        },
        meta: crate::DocumentMeta::default(),
        heading: Some(heading()),
        fragment_aliases: vec![],
        diagnostics: vec![],
        blocks: vec![
            Block::Paragraph {
                children: vec![text("A\nB")],
                inline_layout: layout(&[(1, 0)]),
                layout: LayoutHint::default(),
                source: None,
            },
            Block::Preformatted {
                children: vec![text("A\nB")],
                inline_layout: layout(&[(1, 0)]),
                layout: LayoutHint::default(),
                language: None,
                source: None,
            },
            Block::DefinitionList {
                items: vec![crate::DefinitionItem {
                    terms: vec![DefinitionTerm {
                        content: vec![text("A\nB")],
                        inline_layout: layout(&[(1, 0)]),
                    }],
                    description: vec![],
                    layout: crate::DefinitionLayout::default(),
                    source: None,
                    entry: None,
                }],
                compact: true,
                declaration_groups: vec![],
                layout: LayoutHint::default(),
                source: None,
            },
        ],
        sections: vec![crate::Section {
            id: "topic".into(),
            fragment_aliases: vec![],
            heading: heading(),
            spacing_before_lines: 0,
            blocks: vec![],
            children: vec![],
            source: None,
        }],
    };
    assert_eq!(crate::validate_document(&document), []);
    crate::visit::VisitMut::visit_document_mut(&mut RemoveBreaks, &mut document);
    let diagnostics = crate::validate_document(&document);
    assert_eq!(diagnostics.len(), 5, "{diagnostics:?}");
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() == Some("ir.invalid-inline-layout"))
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("owner's logical rows"))
    );
}

#[test]
fn schema_publishes_the_hint_count_and_signed_displacement_bounds() {
    let layout_schema = serde_json::to_value(schemars::schema_for!(InlineLayout)).unwrap();
    assert_eq!(
        layout_schema["properties"]["rowHints"]["maxItems"].as_u64(),
        Some(4096)
    );
    let hint_schema = serde_json::to_value(schemars::schema_for!(RowLayoutHint)).unwrap();
    assert_eq!(
        hint_schema["properties"]["indentColumns"]["minimum"].as_f64(),
        Some(-65535.0)
    );
    assert_eq!(
        hint_schema["properties"]["indentColumns"]["maximum"].as_f64(),
        Some(65535.0)
    );
}
