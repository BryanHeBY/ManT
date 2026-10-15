//! Prepared cell alignment uses final slots without changing intrinsic rows.
use super::super::{
    Block, DocumentView, Inline, LayoutHint, LinkTarget, RenderedSelection, ResolvedContent,
    TableCell, TableRow, TextPosition, bundle,
};
use mant_ir::{InlineLayout, RowLayoutHint, TextAlignment};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

fn cell(value: &str, alignment: TextAlignment, linked: bool) -> TableCell {
    let text = vec![Inline::Text {
        value: value.into(),
    }];
    let children = if linked {
        vec![
            Inline::anchor(format!("cell-{value}")),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![Inline::Strong { children: text }],
            },
        ]
    } else {
        text
    };
    TableCell {
        kind: mant_ir::TableCellKind::Text,
        break_after: false,
        column_span: 1,
        row_span: 1,
        alignment: None,
        blocks: vec![Block::Paragraph {
            children,
            inline_layout: InlineLayout {
                row_hints: if alignment == TextAlignment::Left {
                    vec![]
                } else {
                    vec![RowLayoutHint {
                        alignment,
                        ..Default::default()
                    }]
                },
            },
            layout: LayoutHint::default(),
            source: None,
        }],
    }
}

fn query(alignment: TextAlignment, declared: bool, stacked: bool) -> ResolvedContent {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        alignment: None,
        column_count: None,
        column_preferences: mant_ir::ColumnPreferences {
            widths: if declared { vec![6, 6] } else { vec![] },
            ..Default::default()
        },
        rows: vec![
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell("X", alignment, true), cell("Y", alignment, true)],
            },
            TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![
                    cell("XXXXXX", TextAlignment::Left, false),
                    cell("YYYYYY", TextAlignment::Left, false),
                ],
            },
        ],
        layout: LayoutHint {
            indent_columns: if stacked { -1 } else { 0 },
            ..Default::default()
        },
        source: None,
    }];
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into()
}

fn offset(alignment: TextAlignment, available: usize) -> usize {
    match alignment {
        TextAlignment::Left => 0,
        TextAlignment::Center => (available - 1) / 2,
        TextAlignment::Right => available - 1,
    }
}

#[test]
fn table_alignment_consumes_known_slots_in_plain_ansi_and_real_buffers() {
    for width in [20_u16, 40] {
        for alignment in [
            TextAlignment::Left,
            TextAlignment::Center,
            TextAlignment::Right,
        ] {
            for declared in [false, true] {
                for stacked in [false, true] {
                    check_case(width, alignment, declared, stacked);
                }
            }
        }
    }
}

fn check_case(width: u16, alignment: TextAlignment, declared: bool, stacked: bool) {
    let query = query(alignment, declared, stacked);
    let rendered = DocumentView::new(&query).render(width);
    let x = &rendered.search("X")[0];
    let y = &rendered.search("Y")[0];
    let (x_column, y_column) = if stacked {
        (
            offset(alignment, usize::from(width)),
            offset(alignment, usize::from(width)),
        )
    } else {
        (
            offset(alignment, 6),
            8 + offset(alignment, usize::from(width) - 8),
        )
    };
    assert_eq!(
        (x.start_column, y.start_column),
        (x_column, y_column),
        "{width} {alignment:?} {declared} {stacked}"
    );
    assert_eq!(y.row, x.row + usize::from(stacked));
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    for (word, hit) in [("X", x), ("Y", y)] {
        assert_eq!(
            buffer[(
                hit.start_column.try_into().unwrap(),
                hit.row.try_into().unwrap()
            )]
                .symbol(),
            word
        );
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("target".into()))
        );
        assert_eq!(rendered.anchor_row(&format!("cell-{word}")), Some(hit.row));
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1
                },
            }),
            word
        );
    }
    for paint in [false, true] {
        let text =
            mant_render::render_query_text_at_width(&query, usize::from(width), |_, value| {
                if paint {
                    format!("\x1b[1m{value}\x1b[0m")
                } else {
                    value.into()
                }
            })
            .replace("\x1b[1m", "")
            .replace("\x1b[0m", "");
        let rows = text
            .lines()
            .filter(|row| row.contains('X') || row.contains('Y'))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), if stacked { 4 } else { 2 });
        let expected = if stacked {
            format!("{}X\n{}Y", " ".repeat(x_column), " ".repeat(y_column))
        } else if declared {
            format!(
                "{}X{}Y",
                " ".repeat(x_column),
                " ".repeat(y_column - x_column - 1)
            )
        } else if alignment == TextAlignment::Left {
            "X | Y".into()
        } else {
            let end = 9 + offset(alignment, usize::from(width) - 9);
            format!(
                "{}X{} | {}Y",
                " ".repeat(x_column),
                " ".repeat(6 - x_column - 1),
                " ".repeat(end - 9)
            )
        };
        assert_eq!(
            rows[..if stacked { 2 } else { 1 }].join("\n"),
            expected,
            "{width} {alignment:?} {declared} {stacked}"
        );
    }
}

#[test]
fn declared_alignment_does_not_cross_the_next_actual_field_after_a_capped_advance() {
    for alignment in [
        TextAlignment::Left,
        TextAlignment::Center,
        TextAlignment::Right,
    ] {
        let mut query = query(alignment, true, false);
        let Block::Table {
            column_preferences,
            rows,
            ..
        } = &mut query.document.as_mut().unwrap().blocks[0]
        else {
            unreachable!()
        };
        column_preferences.max_alignment_padding_columns = Some(2);
        rows.truncate(1);
        let rendered = DocumentView::new(&query).render(20);
        let x = &rendered.search("X")[0];
        let y = &rendered.search("Y")[0];
        // The intrinsic first graph ends at 1, then the capped advance adds
        // only 2 cells. No generated owner padding may displace this field.
        let y_column = 3 + offset(alignment, 17);
        assert_eq!((x.row, x.start_column), (y.row, 0));
        assert_eq!(y.start_column, y_column);
        let area = Rect::new(0, 0, 20, rendered.row_count.try_into().unwrap());
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
        assert_eq!(buffer[(0, x.row.try_into().unwrap())].symbol(), "X");
        assert_eq!(
            buffer[(y_column.try_into().unwrap(), y.row.try_into().unwrap())].symbol(),
            "Y"
        );
        for paint in [false, true] {
            let text = mant_render::render_query_text_at_width(&query, 20, |_, value| {
                if paint {
                    format!("\x1b[1m{value}\x1b[0m")
                } else {
                    value.into()
                }
            })
            .replace("\x1b[1m", "")
            .replace("\x1b[0m", "");
            let row = text.lines().find(|row| row.contains('X')).unwrap();
            assert_eq!(
                row,
                format!("X{}Y", " ".repeat(y_column - 1)),
                "{alignment:?}"
            );
        }
    }
}

#[test]
fn composed_first_only_origins_keep_declared_rows_and_root_navigation() {
    // The 54 filled column/pre-ti sources reran on current pristine before
    // this regression. mdoc_term.c::print_mdoc_node restores source offset,
    // while ti remains independent; a negative first-only component can
    // cancel a positive persistent correction without requiring stacking.
    for (continued, first_only, outdent) in [
        (3, -3, false),
        (3, -2, false),
        (-3, 3, true),
        (-3, 6, true),
        (0, -3, true),
    ] {
        for parent in [0, 5, -2] {
            for width in [20_u16, 40] {
                let mut content = query(TextAlignment::Left, true, false);
                let Block::Table { rows, layout, .. } =
                    &mut content.document.as_mut().unwrap().blocks[0]
                else {
                    unreachable!()
                };
                rows.truncate(1);
                layout.indent_columns = parent;
                let Block::Paragraph { inline_layout, .. } = &mut rows[0].cells[0].blocks[0] else {
                    unreachable!()
                };
                inline_layout.row_hints = vec![RowLayoutHint {
                    indent_columns: continued,
                    first_line_indent_columns: first_only,
                    ..Default::default()
                }];
                let mut root = content.clone();
                let Block::Table { rows, .. } = &content.document.as_ref().unwrap().blocks[0]
                else {
                    unreachable!()
                };
                root.document.as_mut().unwrap().blocks = rows[0].cells[0].blocks.clone();
                let Block::Paragraph { layout, .. } =
                    &mut root.document.as_mut().unwrap().blocks[0]
                else {
                    unreachable!()
                };
                layout.indent_columns = parent;
                let root = wire_roundtrip(&root);
                let rendered_root = DocumentView::new(&root).render(width);
                let hit = &rendered_root.search("X")[0];
                let x_column = usize::try_from((parent + continued + first_only).max(0)).unwrap();
                assert_eq!(hit.start_column, x_column);
                assert_eq!(rendered_root.anchor_row("cell-X"), Some(hit.row));
                let stacked = outdent || parent < 0;
                let y_column = if stacked { parent.max(0) } else { parent + 8 };
                assert_composed_table(
                    &wire_roundtrip(&content),
                    width,
                    x_column,
                    usize::try_from(y_column).unwrap(),
                    stacked,
                );
            }
        }
    }
}

fn wire_roundtrip(content: &ResolvedContent) -> ResolvedContent {
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(content)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into();
    assert_eq!(restored.document, content.document);
    restored
}

fn assert_composed_table(
    query: &ResolvedContent,
    width: u16,
    x_column: usize,
    y_column: usize,
    stacked: bool,
) {
    let rendered = DocumentView::new(query).render(width);
    let x = &rendered.search("X")[0];
    let y = &rendered.search("Y")[0];
    assert_eq!((x.start_column, y.start_column), (x_column, y_column));
    assert_eq!(y.row, x.row + usize::from(stacked));
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    for (word, hit) in [("X", x), ("Y", y)] {
        let cell = &buffer[(
            hit.start_column.try_into().unwrap(),
            hit.row.try_into().unwrap(),
        )];
        assert_eq!(cell.symbol(), word);
        assert!(cell.modifier.contains(ratatui::style::Modifier::BOLD));
        assert_eq!(rendered.anchor_row(&format!("cell-{word}")), Some(hit.row));
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("target".into()))
        );
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1
                },
            }),
            word
        );
    }
    let expected = if stacked {
        format!("{}X\n{}Y", " ".repeat(x_column), " ".repeat(y_column))
    } else {
        format!(
            "{}X{}Y",
            " ".repeat(x_column),
            " ".repeat(y_column - x_column - 1)
        )
    };
    for paint in [false, true] {
        let text =
            mant_render::render_query_text_at_width(query, usize::from(width), |_, value| {
                if paint {
                    format!("\x1b[1m{value}\x1b[0m")
                } else {
                    value.into()
                }
            })
            .replace("\x1b[1m", "")
            .replace("\x1b[0m", "");
        let rows = text
            .lines()
            .filter(|row| row.contains('X') || row.contains('Y'))
            .collect::<Vec<_>>();
        assert_eq!(rows.join("\n"), expected);
    }
}
