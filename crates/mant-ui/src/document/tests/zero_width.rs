//! Zero-width targets preserve ownership without becoming visible content.
use super::*;

#[test]
fn empty_native_list_target_adds_navigation_without_an_extra_row() {
    let source = ".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh DESCRIPTION\nBEFORE\n.Bl -tag -width Ds\n.Tg empty-target\n.El\n.Pp\nAFTER\n";
    let annotated = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let plain =
        mant_loader::load_roff_bytes(source.replace(".Tg empty-target\n", "").as_bytes()).unwrap();
    let annotated = DocumentView::new(&annotated).render(80);
    let plain = DocumentView::new(&plain).render(80);
    assert_eq!(annotated.text, plain.text);
    let after = annotated.search("AFTER");
    assert_eq!(after.len(), 1);
    assert_eq!(annotated.anchor_row("empty-target"), Some(after[0].row));
}

#[test]
fn native_list_spacing_distinguishes_source_items_from_navigation_carriers() {
    // The exact sources were checked with pristine CVS ASCII/UTF-8 and tree.
    // mdoc_term.c::termp_bl_pre/post call term_newln without vertical space;
    // It BLOCK pre alone calls print_bvspace. Empty column It nodes are
    // removed by validation, unlike empty tag/plain It nodes.
    for (style, arguments) in [
        ("tag", "-tag -width Ds"),
        ("plain", "-item"),
        ("column", "-column One Two"),
    ] {
        for compact in [false, true] {
            for paragraph in [false, true] {
                for variant in ["no-item", "anchor-only", "actual-item", "empty-item"] {
                    let payload = match variant {
                        "anchor-only" => ".Tg empty-target\n",
                        "actual-item" if style == "plain" => ".Tg item-target\n.It\nCONTENT\n",
                        "actual-item" => ".Tg item-target\n.It CONTENT\n",
                        "empty-item" => ".Tg item-target\n.It\n",
                        _ => "",
                    };
                    let source = format!(
                        ".Dd September 9, 2026\n.Dt TARGET 1\n.Os\n.Sh NAME\n.Nm target\n.Nd list-boundary probe\n.Sh DESCRIPTION\nBEFORE\n.Bl {arguments}{}\n{payload}.El\n{}AFTER\n",
                        if compact { " -compact" } else { "" },
                        if paragraph { ".Pp\n" } else { "" },
                    );
                    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                    let view = DocumentView::new(&content);
                    let native_item =
                        variant == "actual-item" || (variant == "empty-item" && style != "column");
                    let distance = 1
                        + usize::from(variant == "actual-item")
                        + usize::from(native_item && !compact)
                        + usize::from(paragraph);
                    for width in [80, 120] {
                        let rendered = view.render(width);
                        let before = rendered.search("BEFORE")[0].row;
                        let after = rendered.search("AFTER")[0].row;
                        assert_eq!(
                            after - before,
                            distance,
                            "{style}/{compact}/{paragraph}/{variant} at {width}: {}",
                            rendered.text
                        );
                        if variant == "anchor-only" {
                            assert_eq!(rendered.anchor_row("empty-target"), Some(after));
                        }
                    }
                }
            }
        }
    }
}

fn navigation_table(widths: &[u16], cells: Vec<Vec<Block>>) -> Block {
    Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: cells
                .into_iter()
                .map(|blocks| TableCell {
                    blocks,
                    kind: mant_ir::TableCellKind::Text,
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .collect(),
        }],
        column_widths: widths.to_vec(),
        layout: LayoutHint::default(),
        source: None,
    }
}

fn assert_navigation_table_case(
    widths: &[u16],
    origin: i32,
    cells: Vec<Vec<Block>>,
    extra_rows: usize,
) {
    let mut table = navigation_table(widths, cells);
    if let Block::Table { layout, .. } = &mut table {
        layout.indent_columns = origin;
    }
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![paragraph("BEFORE"), table, paragraph("AFTER")];
    let view = DocumentView::new(&content);
    for width in [8, 80, 120] {
        let rendered = view.render(width);
        let before = rendered.search("BEFORE")[0].row;
        let after = rendered.search("AFTER")[0].row;
        assert_eq!(
            after - before,
            1 + extra_rows,
            "{widths:?}/origin {origin}/width {width}: {}; {:#?}",
            rendered.text,
            content.document.as_ref().unwrap().blocks
        );
        if let Some(target) = rendered.anchor_row("target") {
            assert_eq!(target, if extra_rows == 0 { after } else { before + 1 });
            assert_eq!(rendered.anchor_row("Exact.Target"), Some(target));
        }
    }
}

#[test]
fn navigation_only_table_rows_preserve_targets_gaps_and_physical_row_counters() {
    let navigation = Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Strong {
            children: vec![Inline::Emphasis {
                children: vec![Inline::anchor_with_aliases(
                    "target",
                    vec!["Exact.Target".into()],
                )],
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    for widths in [vec![], vec![3, 3], vec![u16::MAX]] {
        for (cells, extra_rows) in [
            (vec![vec![navigation.clone()]], 0),
            (vec![], 1),
            (vec![vec![]], 1),
            (vec![vec![], vec![]], 1),
            (vec![vec![paragraph("")]], 1),
            (vec![vec![navigation.clone()], vec![]], 1),
            (vec![vec![], vec![navigation.clone()]], 1),
            (
                vec![vec![Block::Preformatted {
                    inline_layout: mant_ir::InlineLayout::default(),
                    children: vec![Inline::Text {
                        value: String::new(),
                    }],
                    language: None,
                    layout: LayoutHint::default(),
                    source: None,
                }]],
                1,
            ),
            (
                vec![vec![Block::VerticalSpace {
                    lines: 1,
                    source: None,
                }]],
                1,
            ),
            (
                vec![vec![Block::VerticalSpace {
                    lines: 0,
                    source: None,
                }]],
                1,
            ),
        ] {
            for origin in [0, -2] {
                assert_navigation_table_case(&widths, origin, cells.clone(), extra_rows);
            }
        }
        let mut builder = DocumentBuilder::new("targets".into(), None);
        builder.blocks(
            &[
                paragraph("BEFORE"),
                Block::VerticalSpace {
                    lines: 2,
                    source: None,
                },
                navigation_table(&widths, vec![vec![navigation.clone()]]),
                Block::VerticalSpace {
                    lines: 3,
                    source: None,
                },
                paragraph("AFTER"),
            ],
            0,
        );
        let built = builder.finish();
        assert_eq!(built.content.anchors.get("target"), Some(&6));
        assert_eq!(built.content.lines.len(), 7);
    }
}

fn target_only_definition(description: Vec<Block>, inline_term: bool) -> Block {
    Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms: vec![
                vec![Inline::anchor_with_aliases(
                    "target",
                    vec!["Exact.Target".into()],
                )]
                .into(),
            ],
            description,
            entry: None,
            source: None,
            layout: mant_ir::DefinitionLayout {
                head_body_relation: mant_ir::HeadBodyRelation::from(inline_term),
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
        inline_layout: mant_ir::InlineLayout::default(),
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
            inline_layout: mant_ir::InlineLayout::default(),
            language: None,
            children: vec![
                Inline::anchor("first"),
                Inline::line_break(),
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
    assert_eq!(built.content.lines[0].spans.len(), 0);
    assert_eq!(built.content.lines[1].spans.len(), 0);
    assert_eq!(built.content.lines[2].spans[0].content, "BODY");
    assert_eq!(built.content.anchors.get("deferred"), Some(&0));
    assert_eq!(built.content.anchors.get("first"), Some(&0));
    assert_eq!(built.content.anchors.get("second"), Some(&1));
}
