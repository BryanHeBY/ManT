//! Structural padding remains separate from author cells in copying.
use super::super::{
    Block, DefinitionItem, DocumentBuilder, DocumentView, Inline, LayoutHint, LinkTarget, ListItem,
    ListKind, RenderedSelection, TableCell, TableRow, TextPosition, bundle, paragraph,
};
use super::fixtures::layout;

#[test]
fn ordinary_open_tail_trims_once_and_hint_only_roots_stay_invisible() {
    // The exact A/blank/B no-fill input ran pristine CVS ASCII before this
    // model assertion. term.c::term_newln ends occupied output; term_vspace
    // independently completes blank rows. EOF literal settlement is the
    // existing reader contract; hints never create row content.
    for (value, paragraph_rows, literal_rows) in [("A\n", 1, 2), ("A\n\n", 2, 3), ("\n", 1, 2)] {
        for literal in [false, true] {
            let children = vec![Inline::Text {
                value: value.into(),
            }];
            let hints = layout(&[(0, 2)]);
            let block = if literal {
                Block::Preformatted {
                    children,
                    inline_layout: hints,
                    language: None,
                    layout: LayoutHint::default(),
                    source: None,
                }
            } else {
                Block::Paragraph {
                    children,
                    inline_layout: hints,
                    layout: LayoutHint::default(),
                    source: None,
                }
            };
            let mut builder = DocumentBuilder::new("tails".into(), None);
            builder.blocks(&[block], 0);
            assert_eq!(
                builder.lines.len(),
                if literal {
                    literal_rows
                } else {
                    paragraph_rows
                },
                "{value:?}"
            );
        }
    }
    let mut builder = DocumentBuilder::new("zero".into(), None);
    builder.blocks(
        &[Block::Paragraph {
            children: vec![Inline::anchor("empty-owner")],
            inline_layout: layout(&[(0, 6)]),
            layout: LayoutHint::default(),
            source: None,
        }],
        0,
    );
    let finished = builder.finish();
    assert!(finished.content.lines.is_empty());
    assert_eq!(finished.content.anchors.get("empty-owner"), Some(&0));

    let mut builder = DocumentBuilder::new("tail-target".into(), None);
    builder.blocks(
        &[
            Block::Paragraph {
                children: vec![
                    Inline::Text {
                        value: "A\n".into(),
                    },
                    Inline::anchor("open-tail"),
                ],
                inline_layout: layout(&[(1, 6)]),
                layout: LayoutHint::default(),
                source: None,
            },
            paragraph("NEXT"),
        ],
        0,
    );
    let finished = builder.finish();
    assert_eq!(finished.content.lines.len(), 2);
    assert_eq!(finished.content.anchors.get("open-tail"), Some(&1));
}

#[test]
fn empty_hard_rows_copy_no_structural_padding_and_authored_spaces_remain() {
    for literal in [false, true] {
        for authored_spaces in ["", "  "] {
            let mut content = bundle();
            let document = content.document.as_mut().unwrap();
            document.sections.clear();
            let children = vec![Inline::Text {
                value: format!("A\n{authored_spaces}\n"),
            }];
            let hints = layout(&[(1, 3)]);
            let geometry = LayoutHint {
                indent_columns: 2,
                ..Default::default()
            };
            document.blocks = vec![if literal {
                Block::Preformatted {
                    children,
                    inline_layout: hints,
                    language: None,
                    layout: geometry,
                    source: None,
                }
            } else {
                Block::Paragraph {
                    children,
                    inline_layout: hints,
                    layout: geometry,
                    source: None,
                }
            }];
            let rendered = DocumentView::new(&content).render(80);
            let blank_row = rendered.search("A")[0].row + 1;
            let copy = |column| {
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: blank_row,
                        column,
                    },
                    focus: TextPosition {
                        row: blank_row,
                        column: 79,
                    },
                })
            };
            if authored_spaces.is_empty() {
                assert_eq!(copy(0), "");
            } else {
                // The structural prefix remains part of visual copying when
                // source cells exist; the three hint cells are always omitted.
                assert_eq!(copy(0), "    ");
                assert_eq!(copy(5), authored_spaces);
            }
        }
    }
}

#[test]
fn empty_cell_hard_rows_do_not_copy_the_parent_column_origin() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![TableCell {
                break_after: false,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "A\n\n".into(),
                    }],
                    inline_layout: layout(&[(1, 3)]),
                    layout: LayoutHint {
                        indent_columns: 2,
                        ..Default::default()
                    },
                    source: None,
                }],
                alignment: None,
                kind: mant_ir::TableCellKind::Text,
                column_span: 1,
                row_span: 1,
            }],
            kind: mant_ir::TableRowKind::Data,
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint {
            indent_columns: 2,
            ..Default::default()
        },
        source: None,
    }];
    for width in [12, 80] {
        let rendered = DocumentView::new(&content).render(width);
        let blank_row = rendered.search("A")[0].row + 1;
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: blank_row,
                    column: 0,
                },
                focus: TextPosition {
                    row: blank_row,
                    column: 79,
                },
            }),
            "",
            "width={width}"
        );
    }
}

#[test]
fn marker_hint_padding_is_excluded_from_copy_without_touching_body_spaces() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "BODY  \n\n".into(),
                }],
                inline_layout: layout(&[(0, 3)]),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::ListItemLayout::default(),
            entry: None,
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&content);
    assert_eq!(view.lines.len(), 2, "only the provisional tail is removed");
    let rendered = view.render(80);
    let body = &rendered.search("BODY")[0];
    assert_eq!(body.start_column, 5);
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: body.row,
                column: 0
            },
            focus: TextPosition {
                row: body.row,
                column: 79
            },
        }),
        "• BODY  "
    );
}

#[test]
fn shared_term_and_body_hints_keep_separate_copy_regions() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::DefinitionList {
        items: vec![DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::Shared {
                word_boundary: mant_ir::DefinitionWordBoundary::Separated,
            },
            terms: vec![mant_ir::DefinitionTerm {
                content: vec![Inline::Text {
                    value: "HEAD".into(),
                }],
                inline_layout: layout(&[(0, 2)]),
            }],
            description: vec![Block::Paragraph {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "target".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "BODY  \n\n".into(),
                    }],
                }],
                inline_layout: layout(&[(0, 4)]),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                body_indent_columns: 12,
                min_term_gap_columns: 1,
                spacing_before_lines: None,
            },
            entry: None,
            source: None,
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&content);
    assert_eq!(view.lines.len(), 2);
    let rendered = view.render(80);
    let body = &rendered.search("BODY")[0];
    assert_eq!(body.start_column, 16);
    assert!(rendered.link_target_at(body.row, 15).is_none());
    assert_eq!(
        rendered.link_target_at(body.row, 16),
        Some(&LinkTarget::Section("target".into()))
    );
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: body.row,
                column: 0
            },
            focus: TextPosition {
                row: body.row,
                column: 79
            },
        }),
        "HEAD      BODY  "
    );
}

#[test]
fn table_placement_moves_hint_copy_regions_with_their_cells() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    let cell = |word: &str, correction| TableCell {
        break_after: false,
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: format!(" {word}  "),
                }],
            }],
            inline_layout: layout(&[(0, correction)]),
            layout: LayoutHint::default(),
            source: None,
        }],
        alignment: None,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
    };
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![cell("LEFT", 3), cell("RIGHT", 1)],
            kind: mant_ir::TableRowKind::Data,
        }],
        column_preferences: mant_ir::ColumnPreferences::default(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&content);
    for width in [12, 80, 12] {
        let rendered = view.render(width);
        for (word, correction) in [("LEFT", 3), ("RIGHT", 1)] {
            let hit = &rendered.search(word)[0];
            let original = format!(" {word}  ");
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: hit.start_column - correction - 1
                    },
                    focus: TextPosition {
                        row: hit.row,
                        column: hit.end_column + 1
                    },
                }),
                original
            );
            assert_eq!(
                rendered.link_target_at(hit.row, hit.start_column),
                Some(&LinkTarget::Section("target".into()))
            );
        }
    }
}
