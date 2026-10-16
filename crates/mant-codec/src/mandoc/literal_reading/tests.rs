use mant_ir::{
    DefinitionLayout, DefinitionTerm, DefinitionWordBoundary, HeadBodyRelation, InlineLayout,
    LayoutHint, LinkTarget, RowLayoutHint, TextAlignment,
};

use super::*;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn literal(children: Vec<Inline>) -> Block {
    Block::Preformatted {
        children,
        inline_layout: InlineLayout::default(),
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn content(block: &Block) -> String {
    let Block::Preformatted { children, .. } = block else {
        panic!("literal")
    };
    mant_ir::inline_plain_text(children)
}

#[test]
fn exactly_one_empty_initial_row_requires_later_nonblank_body() {
    // Pure resolved-IR policy tests; independent native cases are recorded in
    // tests/fixtures/roff/literal_leading_rows, not inferred from this function.
    for (before, after) in [
        ("CODE", "CODE"),
        ("\nCODE", "CODE"),
        ("\n\nCODE", "\n\nCODE"),
        ("\n\n\nCODE", "\n\n\nCODE"),
        ("", ""),
        ("\n", "\n"),
        ("\n\n", "\n\n"),
        ("\n \n", "\n \n"),
        ("\n\t", "\n\t"),
        ("\n\u{a0}", "\n\u{a0}"),
        (" \nCODE", " \nCODE"),
        ("\t\nCODE", "\t\nCODE"),
        ("\u{a0}\nCODE", "\u{a0}\nCODE"),
        ("\n \nCODE\n\n", " \nCODE\n\n"),
        ("\n甲\n\n乙\n\n", "甲\n\n乙\n\n"),
    ] {
        let mut block = literal(vec![text(before)]);
        let source = block.clone();
        assert_eq!(omit_initial_row(&mut block), usize::from(before != after));
        assert_eq!(content(&block), after, "{before:?}");
        if before == after {
            assert_eq!(block, source);
        }
    }
}

#[test]
fn removing_one_break_keeps_style_link_and_zero_width_identity() {
    let target = LinkTarget::External {
        uri: "https://example.test".into(),
    };
    let anchor = Inline::anchor("before");
    let empty_link = Inline::Link {
        target: target.clone(),
        title: None,
        children: Vec::new(),
    };
    let mut block = literal(vec![
        anchor.clone(),
        empty_link.clone(),
        Inline::Strong {
            children: vec![
                Inline::Link {
                    target: target.clone(),
                    title: None,
                    children: vec![Inline::LineBreak {}],
                },
                Inline::Emphasis {
                    children: vec![Inline::Code {
                        value: "CODE\nTAIL\n".into(),
                    }],
                },
            ],
        },
    ]);
    assert_eq!(omit_initial_row(&mut block), 1);
    let expected = literal(vec![
        anchor,
        empty_link,
        Inline::Strong {
            children: vec![
                Inline::Link {
                    target,
                    title: None,
                    children: Vec::new(),
                },
                Inline::Emphasis {
                    children: vec![Inline::Code {
                        value: "CODE\nTAIL\n".into(),
                    }],
                },
            ],
        },
    ]);
    assert_eq!(block, expected);
}

#[test]
fn row_origins_and_layout_exceptions_follow_the_remaining_rows() {
    for (indent, continuation) in [(2, 4), (-9, 3), (8, -4)] {
        let mut block = literal(vec![text("\nCODE\nTAIL\n")]);
        let Block::Preformatted {
            layout,
            inline_layout,
            ..
        } = &mut block
        else {
            unreachable!()
        };
        *layout = LayoutHint {
            indent_columns: indent,
            continuation_indent_columns: continuation,
            spacing_before_lines: 2,
        };
        inline_layout.row_hints = (0..4)
            .map(|row| RowLayoutHint {
                row,
                indent_columns: 3,
                first_line_indent_columns: -1,
                alignment: TextAlignment::Right,
            })
            .collect();
        let before = layout.to_owned();
        let before_hints = inline_layout.clone();
        assert_eq!(omit_initial_row(&mut block), 1);
        let Block::Preformatted {
            layout,
            inline_layout,
            ..
        } = &block
        else {
            unreachable!()
        };
        assert_eq!(layout.indent_columns, indent + continuation);
        assert_eq!(layout.continuation_indent_columns, 0);
        assert_eq!(layout.spacing_before_lines, 2);
        assert_eq!(inline_layout.row_hints.len(), 3);
        for row in 0..3 {
            let old = before_hints.row_layout(row + 1);
            let new = inline_layout.row_layout(row);
            assert_eq!(
                new.resolve(layout.indent_columns, layout.indent_columns),
                old.resolve(
                    before.indent_columns + before.continuation_indent_columns,
                    before.indent_columns + before.continuation_indent_columns
                )
            );
        }
    }
    let mut overflow = literal(vec![text("\nCODE")]);
    if let Block::Preformatted { layout, .. } = &mut overflow {
        layout.indent_columns = i32::MAX;
        layout.continuation_indent_columns = 1;
    }
    let before = overflow.clone();
    assert_eq!(omit_initial_row(&mut overflow), 0);
    assert_eq!(overflow, before);
}

#[test]
fn shared_body_preserves_its_break_but_independent_owners_normalize() {
    for shared in [false, true] {
        for spacing in [0, 1] {
            for empty_head in [false, true] {
                let paragraph: Block = serde_json::from_value(serde_json::json!({
                    "type": "paragraph", "children": [{"type": "anchor", "id": "prefix"}]
                }))
                .unwrap();
                let mut blocks = vec![Block::DefinitionList {
                    compact: true,
                    declaration_groups: Vec::new(),
                    items: vec![DefinitionItem {
                        terms: vec![DefinitionTerm::from(vec![text(if empty_head {
                            ""
                        } else {
                            "HEAD"
                        })])],
                        description: vec![
                            paragraph,
                            Block::VerticalSpace {
                                lines: spacing,
                                source: None,
                            },
                            literal(vec![text("\nBODY")]),
                            literal(vec![text("\nLATER")]),
                        ],
                        head_body_relation: if shared {
                            HeadBodyRelation::Shared {
                                word_boundary: DefinitionWordBoundary::Joined,
                            }
                        } else {
                            HeadBodyRelation::Separate
                        },
                        entry: None,
                        layout: DefinitionLayout::default(),
                        source: None,
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }];
                normalize(&mut blocks, &mut [], &mut NativeHeadEvidence::default());
                let Block::DefinitionList { items, .. } = &blocks[0] else {
                    unreachable!()
                };
                let description = &items[0].description;
                assert_eq!(
                    content(&description[2]),
                    if shared && spacing == 0 {
                        "\nBODY"
                    } else {
                        "BODY"
                    }
                );
                assert_eq!(content(&description[3]), "LATER");
                assert!(
                    matches!(&description[0], Block::Paragraph { children, .. } if children == &[Inline::anchor("prefix")])
                );
            }
        }
    }
}

#[test]
fn only_complete_literal_roots_are_normalized_in_nested_containers() {
    let literal = serde_json::json!({
        "type": "preformatted", "children": [{"type":"text","value":"\nCODE"}]
    });
    let mut document: mant_ir::Document = serde_json::from_value(serde_json::json!({
        "source": {"format":"man"}, "meta":{},
        "blocks": [
            {"type":"paragraph", "children":[{"type":"text","value":"\nPROSE"}]},
            literal,
            {"type":"list","kind":{"kind":"bullet"},"compact":true,"items":[{"blocks":[literal]}]},
            {"type":"table","rows":[{"cells":[{"blocks":[literal]}]}]}
        ],
        "sections": [{"id":"section", "heading":{"content":[{"type":"text","value":"\nTITLE"}]},
            "blocks":[literal], "children":[]}]
    }))
    .unwrap();
    normalize(
        &mut document.blocks,
        &mut document.sections,
        &mut NativeHeadEvidence::default(),
    );
    let encoded = serde_json::to_value(&document).unwrap();
    assert_eq!(encoded["blocks"][0]["children"][0]["value"], "\nPROSE");
    assert_eq!(
        encoded["sections"][0]["heading"]["content"][0]["value"],
        "\nTITLE"
    );
    for block in [
        &encoded["blocks"][1],
        &encoded["blocks"][2]["items"][0]["blocks"][0],
        &encoded["blocks"][3]["rows"][0]["cells"][0]["blocks"][0],
        &encoded["sections"][0]["blocks"][0],
    ] {
        assert_eq!(block["children"][0]["value"], "CODE");
    }
}

#[test]
fn column_gap_and_single_initial_literal_row_have_separate_owners() {
    // Exact source ran pristine ASCII/UTF-8/tree/lint first. Bd pre executes
    // print_bvspace; its independent row and the enclosing Bl leading distance
    // are separate. Reading must never erase the table's external distance.
    let source = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.nf\n.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No \"\\p D\"\n.Bd -literal\nMarker\n.Ed\n.No AFTER\n.Xc Ta RightWord\n.El\n.fi\n.Sh NEXT\n.No END\n";
    let native = crate::mandoc::parse_native_manual(
        std::path::Path::new("column-owner.1"),
        source.as_bytes(),
    )
    .unwrap();
    let reading = crate::mandoc::parse_plain_manual(
        std::path::Path::new("column-owner.1"),
        source.as_bytes(),
    )
    .unwrap();
    let mut expected = native;
    let Block::Table { rows, layout, .. } = &mut expected.sections[1].blocks[0] else {
        panic!("column owner")
    };
    assert_eq!(layout.spacing_before_lines, 1, "external table row remains");
    let Block::Preformatted { children, .. } = &mut rows[0].cells[0].blocks[0] else {
        panic!("complete literal owner")
    };
    assert_eq!(mant_ir::inline_plain_text(children), "\nMarker AFTER");
    assert!(matches!(children.first(), Some(Inline::LineBreak {})));
    children.remove(0);
    assert_eq!(
        reading, expected,
        "only the independent code row is omitted"
    );
}
