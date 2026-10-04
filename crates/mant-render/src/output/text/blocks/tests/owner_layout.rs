//! Source-neutral owner layout contracts; these inputs are constructed IR.
use super::*;
use mant_ir::{InlineLayout, RowLayoutHint};

fn hints(values: &[(u32, i32)]) -> InlineLayout {
    InlineLayout {
        row_hints: values
            .iter()
            .map(|&(row, indent_columns)| RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}

#[test]
fn signed_owner_hints_follow_all_hard_rows_without_changing_authored_text() {
    let expression = mant_ir::EquationExpression {
        kind: mant_ir::EquationKind::Text,
        font: mant_ir::EquationFont::None,
        position: mant_ir::EquationPosition::None,
        size: None,
        expected_args: Some(0),
        actual_args: 0,
        summarized_operand_group: false,
        text: Some("E\nF".into()),
        left: None,
        right: None,
        top: None,
        bottom: None,
        children: vec![],
    };
    let children = vec![
        Inline::Text {
            value: "  A\n".into(),
        },
        Inline::Strong {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                title: None,
                children: vec![Inline::Code {
                    value: "B\nC".into(),
                }],
            }],
        },
        Inline::line_break(),
        Inline::Equation {
            value: "E\nF".into(),
            expression,
        },
        Inline::line_break(),
        Inline::Text { value: "G".into() },
    ];
    let layout = hints(&[(0, -2), (1, 3), (2, -20), (3, -1), (4, 2)]);
    let renderer = super::super::super::plain_renderer();
    for literal in [false, true] {
        let geometry = LayoutHint {
            indent_columns: 2,
            continuation_indent_columns: 1,
            ..Default::default()
        };
        let block = if literal {
            Block::Preformatted {
                children: children.clone(),
                inline_layout: layout.clone(),
                language: None,
                layout: geometry,
                source: None,
            }
        } else {
            Block::Paragraph {
                children: children.clone(),
                inline_layout: layout.clone(),
                layout: geometry,
                source: None,
            }
        };
        let before = block.clone();
        assert_eq!(
            renderer.render_blocks(std::slice::from_ref(&block), 3),
            "     A\n         B\nC\n     E\n        F\n      G"
        );
        assert_eq!(block, before);
    }
    assert_eq!(mant_ir::inline_plain_text(&children), "  A\nB\nC\nE\nF\nG");
    assert_eq!(mant_ir::logical_row_count(&children), 6);
}

#[test]
fn literal_hanging_geometry_applies_to_later_hard_rows_with_signed_corrections() {
    let block = Block::Preformatted {
        children: vec![Inline::Text {
            value: "A\nB\nC".into(),
        }],
        inline_layout: hints(&[(1, -1)]),
        language: None,
        layout: LayoutHint {
            indent_columns: 2,
            continuation_indent_columns: 4,
            ..Default::default()
        },
        source: None,
    };
    assert_eq!(
        super::super::super::plain_renderer().render_blocks(&[block], 0),
        "  A\n     B\n      C"
    );
}

#[test]
fn owner_hints_do_not_print_empty_or_navigation_only_roots() {
    let renderer = super::super::super::plain_renderer();
    for children in [
        vec![],
        vec![Inline::Strong {
            children: vec![Inline::anchor("target")],
        }],
    ] {
        let layout = hints(&[(0, 7)]);
        for block in [
            Block::Paragraph {
                children: children.clone(),
                inline_layout: layout.clone(),
                layout: LayoutHint::default(),
                source: None,
            },
            Block::Preformatted {
                children: children.clone(),
                inline_layout: layout.clone(),
                language: None,
                layout: LayoutHint::default(),
                source: None,
            },
            Block::DefinitionList {
                items: vec![DefinitionItem {
                    head_body_relation: mant_ir::HeadBodyRelation::Separate,
                    terms: vec![mant_ir::DefinitionTerm {
                        content: children,
                        inline_layout: layout,
                    }],
                    description: vec![],
                    source: None,
                    entry: None,
                    layout: mant_ir::DefinitionLayout::default(),
                }],
                declaration_groups: vec![],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            },
        ] {
            assert_eq!(renderer.render_blocks(&[block], 0), "");
        }
    }
}

#[test]
fn paragraph_closes_only_its_open_tail_and_retains_completed_blank_rows() {
    for paint in [false, true] {
        let decorate = |_: TextPresentation, text: &str| {
            if paint {
                format!("\x1b[1m{text}\x1b[0m")
            } else {
                text.into()
            }
        };
        let renderer = BlockRenderer {
            names: None,
            locations: None,
            decorate: &decorate,
        };
        for (value, alone, joined) in [
            ("A\n", "A", "A\nB"),
            ("A\n\n", "A\n\n", "A\n\nB"),
            ("A\n\n\n", "A\n\n\n", "A\n\n\nB"),
            ("\n", "\n", "\nB"),
            ("\n\n", "\n\n", "\n\nB"),
        ] {
            let mut first = paragraph(value, 3);
            let Block::Paragraph { inline_layout, .. } = &mut first else {
                unreachable!()
            };
            *inline_layout = hints(&[(0, -3), (1, 7)]);
            let strip = |text: String| text.replace("\x1b[1m", "").replace("\x1b[0m", "");
            assert_eq!(
                strip(renderer.render_blocks(std::slice::from_ref(&first), 0)),
                alone
            );
            assert_eq!(
                strip(renderer.render_blocks(&[first, paragraph("B", 0)], 0)),
                joined
            );
        }
    }
}

#[test]
fn section_heading_rows_compose_depth_before_clipping_signed_hints() {
    let section = Section {
        id: "heading".into(),
        fragment_aliases: vec![],
        heading: mant_ir::Heading {
            content: vec![Inline::Text {
                value: "HEAD\nNEXT\nLAST".into(),
            }],
            inline_layout: hints(&[(0, -2), (1, 3)]),
            source: None,
        },
        spacing_before_lines: 0,
        blocks: vec![],
        children: vec![],
        source: None,
    };
    assert_eq!(
        super::super::super::plain_renderer().render_section(&section, 2),
        "  HEAD\n       NEXT\n    LAST"
    );
}

#[test]
fn marker_run_in_uses_the_first_owner_row_correction_once() {
    let mut block = plain_list(vec![paragraph("A\nB", 0)], 0);
    let Block::List { kind, items, .. } = &mut block else {
        unreachable!()
    };
    *kind = ListKind::Bullet;
    let Block::Paragraph { inline_layout, .. } = &mut items[0].blocks[0] else {
        unreachable!()
    };
    *inline_layout = hints(&[(0, 3), (1, -1)]);
    assert_eq!(
        super::super::super::plain_renderer().render_blocks(&[block], 0),
        "•    A\n B"
    );
}

#[test]
fn run_in_containers_preserve_completed_paragraph_rows_at_joins_and_eof() {
    let renderer = super::super::super::plain_renderer();
    for value in ["A\n", "A\n\n", "A\n\n\n"] {
        let closed = value.strip_suffix('\n').unwrap();
        let mut list = plain_list(vec![paragraph(value, 0)], 0);
        let Block::List { kind, .. } = &mut list else {
            unreachable!()
        };
        *kind = ListKind::Bullet;
        let definition = Block::DefinitionList {
            items: vec![DefinitionItem {
                head_body_relation: mant_ir::HeadBodyRelation::from(true),
                terms: vec![
                    vec![Inline::Text {
                        value: "TERM".into(),
                    }]
                    .into(),
                ],
                description: vec![paragraph(value, 0)],
                source: None,
                entry: None,
                layout: mant_ir::DefinitionLayout {
                    body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                    ..Default::default()
                },
            }],
            declaration_groups: vec![],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        for (block, prefix) in [(list, "• "), (definition, "TERM ")] {
            let trailing = if closed.ends_with('\n') { "\n" } else { "" };
            assert_eq!(
                renderer.render_blocks(std::slice::from_ref(&block), 0),
                format!("{prefix}{closed}{trailing}")
            );
            assert_eq!(
                renderer.render_blocks(&[block, paragraph("B", 0)], 0),
                format!("{prefix}{closed}\nB")
            );
        }
    }
}
