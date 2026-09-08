//! Zero-width targets preserve ownership without becoming visible content.
use super::*;

#[test]
fn empty_native_list_target_adds_navigation_without_an_extra_row() {
    let source = ".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -tag -width Ds\n.Tg empty-target\n.El\n.Pp\nAFTER\n";
    let annotated = mant_engine::query_roff_bytes(source.as_bytes()).unwrap();
    let plain =
        mant_engine::query_roff_bytes(source.replace(".Tg empty-target\n", "").as_bytes()).unwrap();
    let annotated = DocumentView::new(&annotated).render(80);
    let plain = DocumentView::new(&plain).render(80);
    assert_eq!(annotated.text, plain.text);
    let after = annotated.search("AFTER");
    assert_eq!(after.len(), 1);
    assert_eq!(annotated.anchor_row("empty-target"), Some(after[0].row));
}

fn target_only_definition(description: Vec<Block>, inline_term: bool) -> Block {
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms: vec![vec![Inline::anchor_with_aliases(
                "target",
                vec!["Exact.Target".into()],
            )]],
            description,
            entry: None,
            source: None,
            layout: mant_ir::DefinitionLayout {
                inline_term,
                body_indent_columns: 0,
                ..Default::default()
            },
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn target_only_definitions_do_not_create_rows_or_reset_pending_gaps() {
    for inline_term in [false, true] {
        for (before, after) in [(2, 3), (3000, 2000)] {
            let mut builder = DocumentBuilder::new("targets".into(), None);
            builder.blocks(
                &[
                    paragraph("BEFORE"),
                    Block::VerticalSpace {
                        lines: before,
                        source: None,
                    },
                    target_only_definition(vec![], inline_term),
                    Block::VerticalSpace {
                        lines: after,
                        source: None,
                    },
                    paragraph("AFTER"),
                ],
                0,
            );
            let row = 1 + usize::from((before + after).min(4096));
            let built = builder.finish();
            assert_eq!(built.content.lines.len(), row + 1);
            assert_eq!(built.content.anchors.get("target"), Some(&row));
            assert_eq!(built.content.anchors.get("Exact.Target"), Some(&row));
            assert_eq!(built.content.lines[row].spans[0].content, "AFTER");
        }
    }
}

#[test]
fn body_only_definition_uses_body_origin_without_synthetic_term_gap() {
    for inline_term in [false, true] {
        let mut body = paragraph("BODY");
        if let Block::Paragraph { layout, .. } = &mut body {
            layout.spacing_before_lines = 2;
        }
        let mut builder = DocumentBuilder::new("body-only".into(), None);
        builder.blocks(&[target_only_definition(vec![body], inline_term)], 0);
        let built = builder.finish();
        assert_eq!(built.content.lines.len(), 3);
        assert_eq!(built.content.lines[2].indent, 0);
        assert_eq!(built.content.lines[2].spans[0].content, "BODY");
        assert_eq!(built.content.anchors.get("target"), Some(&2));
    }
    // Also cover the inline-eligible zero-gap body, which formerly gained
    // min_term_gap_columns despite having no printable term.
    let mut builder = DocumentBuilder::new("body-only".into(), None);
    builder.blocks(&[target_only_definition(vec![paragraph("BODY")], true)], 0);
    assert_eq!(builder.lines.len(), 1);
    assert_eq!(builder.lines[0].indent, 0);
    assert_eq!(builder.lines[0].spans[0].content, "BODY");
}

#[test]
fn standalone_zero_width_targets_cross_spacing_and_use_an_eof_sentinel() {
    let target = Block::Paragraph {
        children: vec![Inline::anchor_with_aliases(
            "target",
            vec!["Exact.Target".into()],
        )],
        layout: LayoutHint::default(),
        source: None,
    };
    for eof in [false, true] {
        let mut builder = DocumentBuilder::new("target-only".into(), None);
        builder.blocks(
            &[
                paragraph("BEFORE"),
                target.clone(),
                Block::VerticalSpace {
                    lines: 3,
                    source: None,
                },
            ],
            0,
        );
        if !eof {
            builder.blocks(&[paragraph("AFTER")], 0);
        }
        let built = builder.finish();
        assert_eq!(built.content.lines.len(), if eof { 4 } else { 5 });
        assert_eq!(built.content.anchors.get("target"), Some(&4));
        assert_eq!(built.content.anchors.get("Exact.Target"), Some(&4));
    }
    let mut builder = DocumentBuilder::new("empty-definition".into(), None);
    builder.blocks(&[target_only_definition(vec![], true)], 0);
    let built = builder.finish();
    assert!(built.content.lines.is_empty());
    assert_eq!(built.content.anchors.get("target"), Some(&0));
}

#[test]
fn real_literal_empty_lines_keep_their_rows_and_precise_anchor_positions() {
    let mut builder = DocumentBuilder::new("literal".into(), None);
    builder.inline_lines(&[Inline::anchor("deferred")], 0, Style::default());
    builder.blocks(
        &[Block::Preformatted {
            language: None,
            children: vec![
                Inline::anchor("first"),
                Inline::LineBreak,
                Inline::anchor("second"),
                Inline::Text {
                    value: "\nBODY".into(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }],
        0,
    );
    let built = builder.finish();
    assert_eq!(built.content.lines.len(), 3);
    assert!(built.content.lines[0].spans.is_empty());
    assert!(built.content.lines[1].spans.is_empty());
    assert_eq!(built.content.lines[2].spans[0].content, "BODY");
    assert_eq!(built.content.anchors.get("deferred"), Some(&0));
    assert_eq!(built.content.anchors.get("first"), Some(&0));
    assert_eq!(built.content.anchors.get("second"), Some(&1));
}
