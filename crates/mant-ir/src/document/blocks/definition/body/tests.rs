//! Source-neutral owner selection, not replay of empty roff TEXT execution.
use super::*;
use crate::{DefinitionLayout, HeadBodyRelation, InlineLayout};

fn paragraph(content: Vec<Inline>) -> Block {
    Block::Paragraph {
        children: content,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(content: Vec<Inline>) -> Block {
    Block::Preformatted {
        children: content,
        inline_layout: InlineLayout::default(),
        layout: LayoutHint::default(),
        source: None,
        language: None,
    }
}

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn item(description: Vec<Block>, relation: HeadBodyRelation) -> DefinitionItem {
    DefinitionItem {
        terms: vec![vec![text("FIRST")].into(), vec![text("X")].into()],
        description,
        head_body_relation: relation,
        layout: DefinitionLayout::default(),
        source: None,
        entry: None,
    }
}

fn transparent() -> Vec<Block> {
    vec![
        paragraph(vec![]),
        paragraph(vec![text("")]),
        paragraph(vec![Inline::anchor("prefix")]),
        paragraph(vec![Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![text("")],
            }],
        }]),
        paragraph(vec![Inline::Code {
            value: String::new(),
        }]),
        Block::VerticalSpace {
            lines: 0,
            source: None,
        },
        literal(vec![]),
        literal(vec![Inline::anchor("literal-prefix")]),
    ]
}

#[test]
fn transparent_prefixes_keep_the_original_effective_body_address() {
    for relation in [
        HeadBodyRelation::Separate,
        HeadBodyRelation::joined(),
        HeadBodyRelation::separated(),
    ] {
        for prefix in [vec![], transparent()] {
            let index = prefix.len();
            let mut description = prefix;
            description.extend([paragraph(vec![text("Y")]), paragraph(vec![text("LATER")])]);
            let original = item(description, relation);
            let wire = serde_json::to_string(&original).unwrap();
            let restored: DefinitionItem = serde_json::from_str(&wire).unwrap();
            assert_eq!(restored, original);
            for value in [&original, &restored] {
                let view = value.description_start().unwrap();
                assert_eq!(view.block_index, index);
                assert_eq!(view.leading_blocks.len(), index);
                assert!(std::ptr::eq(
                    view.block,
                    &raw const value.description[index]
                ));
                assert!(std::ptr::eq(
                    view.leading_blocks.as_ptr(),
                    value.description.as_ptr()
                ));
                assert!(!view.has_leading_spacing);
                let (root, _) = view.inline_content().unwrap();
                assert_eq!(crate::inline_plain_text(root.content), "Y");
                let shares = relation != HeadBodyRelation::Separate;
                assert_eq!(value.shared_description().is_some(), shares);
                assert_eq!(value.inline_description().is_some(), shares);
                if shares {
                    assert!(std::ptr::eq(
                        value.inline_description().unwrap().0.as_ptr(),
                        root.content.as_ptr()
                    ));
                }
            }
        }
    }
}

#[test]
fn real_inline_rows_and_structural_blocks_stop_selection() {
    for block in [
        paragraph(vec![Inline::line_break()]),
        paragraph(vec![text(" ")]),
        paragraph(vec![text("\u{a0}")]),
        literal(vec![text("")]),
        literal(vec![Inline::Code {
            value: String::new(),
        }]),
        literal(vec![Inline::line_break()]),
        Block::ThematicBreak { source: None },
    ] {
        let value = item(
            vec![
                paragraph(vec![Inline::anchor("before")]),
                block,
                paragraph(vec![text("Y")]),
            ],
            HeadBodyRelation::joined(),
        );
        let view = value.description_start().unwrap();
        assert_eq!(view.block_index, 1);
        assert!(std::ptr::eq(view.block, &raw const value.description[1]));
        assert_eq!(
            value.shared_description().is_some(),
            !matches!(view.block, Block::ThematicBreak { .. })
        );
    }
}

#[test]
fn positive_leading_spacing_is_preserved_even_on_transparent_owners() {
    for position in 0..3 {
        let mut prefix = paragraph(vec![Inline::anchor("prefix")]);
        let mut body = paragraph(vec![text("Y")]);
        let mut gap = Block::VerticalSpace {
            lines: 0,
            source: None,
        };
        if position == 0 {
            let Block::Paragraph { layout, .. } = &mut prefix else {
                unreachable!()
            };
            layout.spacing_before_lines = 1;
        } else if position == 1 {
            gap = Block::VerticalSpace {
                lines: 1,
                source: None,
            };
        } else {
            let Block::Paragraph { layout, .. } = &mut body else {
                unreachable!()
            };
            layout.spacing_before_lines = 1;
        }
        let original = item(vec![prefix, gap, body], HeadBodyRelation::joined());
        let wire = serde_json::to_string(&original).unwrap();
        let restored: DefinitionItem = serde_json::from_str(&wire).unwrap();
        let view = restored.description_start().unwrap();
        assert_eq!(view.block_index, 2);
        assert!(view.has_leading_spacing);
        assert!(restored.shared_description().is_none());
        assert!(restored.inline_description_content().is_none());
        assert_eq!(restored, original);
    }
}

#[test]
fn transparent_only_bodies_do_not_invent_a_shared_row() {
    let value = item(transparent(), HeadBodyRelation::joined());
    assert!(value.description_start().is_none());
    assert!(value.shared_description().is_none());
    assert_eq!(value.description.len(), 8);
}
