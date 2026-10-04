//! Public row/word policy is independent from source execution causes.

use super::*;
use mant_ir::{DefinitionBodyAlignment, DefinitionLayout, HeadBodyRelation};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn definition(
    relation: HeadBodyRelation,
    alignment: DefinitionBodyAlignment,
    literal: bool,
    multi_head: bool,
) -> Block {
    let mut head = Vec::new();
    if multi_head {
        head.extend([text("HEAD"), Inline::line_break()]);
    }
    head.push(Inline::Strong {
        children: vec![text("中e\u{301}")],
    });
    let children = vec![
        Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://ex.org".into(),
            },
            title: None,
            children: vec![Inline::Code {
                value: "BODY".into(),
            }],
        },
        Inline::line_break(),
        Inline::Emphasis {
            children: vec![text("CONT")],
        },
    ];
    let layout = LayoutHint {
        indent_columns: -2,
        continuation_indent_columns: 2,
        ..Default::default()
    };
    let body = if literal {
        Block::Preformatted {
            children,
            language: None,
            inline_layout: mant_ir::InlineLayout {
                row_hints: vec![mant_ir::RowLayoutHint {
                    row: 1,
                    indent_columns: 1,
                }],
            },
            layout,
            source: None,
        }
    } else {
        Block::Paragraph {
            children,
            inline_layout: mant_ir::InlineLayout {
                row_hints: vec![mant_ir::RowLayoutHint {
                    row: 1,
                    indent_columns: 1,
                }],
            },
            layout,
            source: None,
        }
    };
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            head_body_relation: relation,
            terms: vec![mant_ir::DefinitionTerm {
                content: head,
                inline_layout: mant_ir::InlineLayout {
                    row_hints: if multi_head {
                        vec![mant_ir::RowLayoutHint {
                            row: 1,
                            indent_columns: 2,
                        }]
                    } else {
                        vec![]
                    },
                },
            }],
            description: vec![body],
            source: None,
            entry: None,
            layout: DefinitionLayout {
                body_alignment: alignment,
                body_indent_columns: 12,
                min_term_gap_columns: 2,
                spacing_before_lines: None,
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn all_shared_word_and_alignment_policies_compose_relative_origins_once() {
    // Source-neutral constructed IR: all four combinations are supported,
    // including Joined+AfterTerm which no current native producer fixture
    // reaches. These expectations are the public placement contract.
    let renderer = super::super::super::plain_renderer();
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        for joined in [false, true] {
            let relation = if joined {
                HeadBodyRelation::joined()
            } else {
                HeadBodyRelation::separated()
            };
            for literal in [false, true] {
                for multi_head in [false, true] {
                    for origin in [0, 5] {
                        let block = definition(relation, alignment, literal, multi_head);
                        let before = block.clone();
                        let term_indent = usize::from(multi_head) * 2;
                        let gap = if joined {
                            0
                        } else if alignment == DefinitionBodyAlignment::AfterTerm {
                            2
                        } else {
                            7 - term_indent
                        };
                        let prefix = if multi_head {
                            format!("{}HEAD\n", " ".repeat(origin))
                        } else {
                            String::new()
                        };
                        assert_eq!(
                            renderer.render_blocks(
                                std::slice::from_ref(&block),
                                origin.try_into().unwrap()
                            ),
                            format!(
                                "{prefix}{}中e\u{301}{}BODY\n{}CONT",
                                " ".repeat(origin + term_indent),
                                " ".repeat(gap),
                                " ".repeat(origin + 13)
                            )
                        );
                        assert_eq!(block, before, "rendering cannot modify accepted IR");
                    }
                }
            }
        }
    }
}

#[test]
fn changing_alignment_and_distances_cannot_change_the_item_word_relation() {
    let renderer = super::super::super::plain_renderer();
    for relation in [HeadBodyRelation::Separate, HeadBodyRelation::joined()] {
        for alignment in [
            DefinitionBodyAlignment::AfterTerm,
            DefinitionBodyAlignment::Indented,
        ] {
            let mut block = definition(relation, alignment, false, false);
            let Block::DefinitionList { items, .. } = &mut block else {
                unreachable!()
            };
            items[0].layout.body_indent_columns = 40;
            items[0].layout.min_term_gap_columns = 255;
            assert_eq!(items[0].head_body_relation, relation);
            let output = renderer.render_blocks(&[block], 0);
            assert_eq!(
                output.lines().next(),
                Some(if relation == HeadBodyRelation::Separate {
                    "中e\u{301}"
                } else {
                    "中e\u{301}BODY"
                })
            );
        }
    }
}

#[test]
fn joined_words_retain_authored_head_padding_and_spacing_prevents_run_in() {
    let renderer = super::super::super::plain_renderer();
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        let mut block = definition(HeadBodyRelation::joined(), alignment, false, false);
        let Block::DefinitionList { items, .. } = &mut block else {
            unreachable!()
        };
        items[0].terms[0].content.push(text("\u{a0}"));
        assert!(
            renderer
                .render_blocks(std::slice::from_ref(&block), 0)
                .starts_with("中e\u{301}\u{a0}BODY\n")
        );
        let Block::DefinitionList { items, .. } = &mut block else {
            unreachable!()
        };
        let Block::Paragraph { layout, .. } = &mut items[0].description[0] else {
            unreachable!()
        };
        layout.spacing_before_lines = 1;
        assert!(
            renderer
                .render_blocks(&[block], 0)
                .starts_with("中e\u{301}\u{a0}\n\n          BODY\n")
        );
    }
}
