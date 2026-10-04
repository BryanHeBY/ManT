//! Ownership and actual recognition work at the hanging-definition boundary.
use super::*;
use mant_ir::{Inline, LinkTarget, SourceSpan};
use std::cell::Cell;

thread_local! {
    static RECOGNITIONS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_recognition() {
    RECOGNITIONS.with(|count| count.set(count.get() + 1));
}

fn take_recognitions() -> usize {
    RECOGNITIONS.with(|count| count.replace(0))
}

fn paragraph(value: &str, indent_columns: i32) -> Block {
    Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Text {
            value: value.into(),
        }],
        layout: LayoutHint {
            indent_columns,
            ..LayoutHint::default()
        },
        source: None,
    }
}

fn space(lines: u16) -> Block {
    Block::VerticalSpace {
        lines,
        source: None,
    }
}

fn table(indent_columns: i32) -> Block {
    Block::Table {
        rows: Vec::new(),
        column_widths: vec![4],
        layout: LayoutHint {
            indent_columns,
            ..LayoutHint::default()
        },
        source: None,
    }
}

#[test]
fn unreachable_descriptions_do_not_run_the_head_grammar() {
    // These are already lowered IR boundaries, not an alternative macro
    // interpreter. In pinned man_term.c, pre/post_RS owns the deeper origin;
    // pre_PP alone cannot establish an indented description.
    for boundary in [
        vec![],
        vec![paragraph("--next", 0)],
        vec![paragraph("Outer text.", -1)],
        vec![Block::ThematicBreak { source: None }],
        vec![table(4)],
    ] {
        let mut blocks = vec![paragraph("--owner", 0), space(1), space(2)];
        blocks.extend(boundary);
        let before = blocks.clone();
        take_recognitions();
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(blocks, before);
        assert_eq!(take_recognitions(), 0);
    }
}

#[test]
fn ordinary_sibling_growth_does_not_invoke_the_head_grammar() {
    for length in [1, 32, 1024] {
        let mut blocks = (0..length)
            .map(|_| paragraph("Ordinary description text.", 0))
            .collect::<Vec<_>>();
        blocks.extend((0..length).map(|_| space(1)));
        let before = blocks.clone();
        take_recognitions();
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(blocks, before);
        assert_eq!(take_recognitions(), 0, "sibling count {length}");
    }
}

#[test]
fn one_reachable_head_is_recognized_once_for_all_of_its_description() {
    for length in [1, 32, 1024] {
        let mut blocks = vec![paragraph("--owner", 0)];
        blocks.extend((0..length).flat_map(|_| [space(2), paragraph("Description.", 4)]));
        blocks.extend([space(3), paragraph("--next", 0)]);
        take_recognitions();
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(take_recognitions(), 1, "description count {length}");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1], space(3));
        assert_eq!(blocks[2], paragraph("--next", 0));
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("reachable complete head was not normalized")
        };
        assert_eq!(items[0].description.len(), length * 2);
        for pair in items[0].description.as_chunks::<2>().0 {
            assert_eq!(pair, &[space(2), paragraph("Description.", 0)]);
        }
        let once = blocks.clone();
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(blocks, once);
        assert_eq!(take_recognitions(), 0);
    }
}

#[test]
fn reachable_prose_still_requires_complete_head_recognition() {
    let mut blocks = vec![
        paragraph("Ordinary description text.", 0),
        paragraph("Indented prose.", 4),
    ];
    let before = blocks.clone();
    take_recognitions();
    normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
    assert_eq!(blocks, before);
    assert_eq!(take_recognitions(), 1);
}

#[test]
fn invisible_layout_successors_remain_eligible_and_tables_remain_boundaries() {
    for value in ["", " ", "\u{a0}"] {
        let mut blocks = vec![
            paragraph("--owner", 0),
            space(1),
            paragraph(value, 4),
            space(2),
            table(4),
            paragraph("Unrelated table tail.", 4),
        ];
        let boundary = blocks[3..].to_vec();
        take_recognitions();
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(take_recognitions(), 1);
        assert_eq!(&blocks[1..], boundary);
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            panic!("invisible indented description remains a layout successor")
        };
        assert_eq!(items[0].description, [space(1), paragraph(value, 0)]);
    }
}

#[test]
fn normalization_moves_original_styled_content_and_source_without_flattening() {
    let source = Some(SourceSpan {
        byte_range: None,
        line: 4,
        column: 1,
        end_line: Some(5),
        end_column: Some(9),
    });
    let children = vec![
        Inline::anchor("head-target"),
        Inline::Strong {
            children: vec![Inline::Link {
                target: LinkTarget::External {
                    uri: "https://example.org/--owner".into(),
                },
                title: Some("Advisory title".into()),
                children: vec![Inline::Text {
                    value: "--owner".into(),
                }],
            }],
        },
    ];
    let literal = Block::Preformatted {
        inline_layout: mant_ir::InlineLayout {
            row_hints: vec![mant_ir::RowLayoutHint {
                row: 1,
                indent_columns: 2,
            }],
        },
        children: vec![
            Inline::Code {
                value: "First".into(),
            },
            Inline::line_break(),
            Inline::Code {
                value: "Second".into(),
            },
        ],
        language: Some("text".into()),
        layout: LayoutHint {
            indent_columns: 9,
            continuation_indent_columns: 2,
            spacing_before_lines: 1,
        },
        source,
    };
    let head_layout = mant_ir::InlineLayout {
        row_hints: vec![mant_ir::RowLayoutHint {
            row: 0,
            indent_columns: -2,
        }],
    };
    let mut blocks = vec![
        Block::Paragraph {
            inline_layout: head_layout.clone(),
            children: children.clone(),
            layout: LayoutHint {
                indent_columns: 5,
                spacing_before_lines: 2,
                ..LayoutHint::default()
            },
            source,
        },
        space(1),
        literal.clone(),
    ];
    take_recognitions();
    normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
    assert_eq!(take_recognitions(), 1);
    let Block::DefinitionList {
        items,
        layout,
        source: list_source,
        ..
    } = &blocks[0]
    else {
        panic!("complete styled head with literal description")
    };
    assert_eq!(*list_source, source);
    assert_eq!(layout.indent_columns, 5);
    assert_eq!(items[0].source, source);
    assert_eq!(
        items[0].terms,
        [mant_ir::DefinitionTerm {
            content: children,
            inline_layout: head_layout
        }]
    );
    assert_eq!(items[0].layout.body_indent_columns, 4);
    assert_eq!(items[0].layout.spacing_before_lines, Some(2));
    assert_eq!(
        items[0].layout.head_body_relation,
        HeadBodyRelation::Separate
    );
    let mut expected_literal = literal;
    let Block::Preformatted { layout, .. } = &mut expected_literal else {
        unreachable!()
    };
    layout.indent_columns = 0;
    assert_eq!(items[0].description, [space(1), expected_literal]);
}
