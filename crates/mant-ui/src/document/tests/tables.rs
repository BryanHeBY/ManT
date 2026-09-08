//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

#[test]
fn signed_table_cells_preserve_real_origins_links_and_anchors() {
    for (table_indent, child_indent, expected_column) in
        [(-2, 3, 1), (3, -2, 1), (4096, 3, 4096), (4090, 10, 4096)]
    {
        let cell = |id: &str, text: &str| TableCell {
            blocks: vec![Block::Paragraph {
                children: vec![
                    Inline::anchor_with_aliases(id, vec![format!("Exact.{id}").into()]),
                    Inline::Link {
                        target: mant_ir::LinkTarget::Section {
                            id: "description".into(),
                        },
                        title: None,
                        children: vec![Inline::Text { value: text.into() }],
                    },
                ],
                layout: LayoutHint {
                    indent_columns: child_indent,
                    ..Default::default()
                },
                source: None,
            }],
            column_span: 2,
            row_span: 1,
            alignment: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                cells: vec![cell("first", "FIRST"), cell("second", "SECOND")],
            }],
            layout: LayoutHint {
                indent_columns: table_indent,
                ..Default::default()
            },
            source: None,
        }];
        document.sections.clear();
        let rendered = DocumentView::new(&query).render((expected_column + 20).max(80));
        let expected_column = usize::from(expected_column);
        let rows = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for (id, text) in [("first", "FIRST"), ("second", "SECOND")] {
            let found = rendered.search(text);
            assert_eq!(found.len(), 1, "{rows:?}");
            let row = found[0].row;
            assert_eq!(rows[row].find(text), Some(expected_column), "{rows:?}");
            assert_eq!(rendered.anchor_row(id), Some(row));
            assert_eq!(rendered.anchor_row(&format!("Exact.{id}")), Some(row));
            assert_eq!(
                rendered.link_target_at(row, expected_column),
                Some(&LinkTarget::Section("description".into()))
            );
        }
        assert!(rendered.search("FIRST")[0].row < rendered.search("SECOND")[0].row);
    }
}

#[test]
fn table_cells_use_shared_content_driven_columns_and_independent_wrapping() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![
                TableCell {
                    blocks: vec![paragraph("alpha beta gamma")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    blocks: vec![paragraph("right hand")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(24);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1].trim_end(), "   alpha       right", "{rows:#?}");
    assert_eq!(rows[2].trim_end(), "   beta gamma  hand", "{rows:#?}");
    assert_eq!(UnicodeWidthStr::width(rows[1].as_str()), 24);
    let left_match = rendered.search("alpha beta gamma");
    assert_eq!(left_match.len(), 1);
    assert_eq!(left_match[0].row, 1);
    assert_eq!(left_match[0].additional_fragments[0].row, 2);
    assert_eq!(rendered.search("right hand").len(), 1);
}

#[test]
fn short_table_keys_do_not_claim_half_of_a_wide_viewport() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                cells: vec![cell("1"), cell("Executable programs and shell commands")],
            },
            TableRow {
                cells: vec![cell("8"), cell("System administration commands")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1], "   1  Executable programs and shell commands");
    assert_eq!(rows[2].trim_end(), "   8  System administration commands");
    assert!(UnicodeWidthStr::width(rows[2].as_str()) < 50);
}

#[test]
fn narrow_tables_stack_cells_instead_of_dropping_content() {
    let cells = vec![
        LogicalTableCell::new(vec![LogicalLine::plain(0, "a", Style::default())], None),
        LogicalTableCell::new(vec![LogicalLine::plain(0, "b", Style::default())], None),
    ];
    let layout = Arc::new(LogicalTableLayout::for_rows(std::slice::from_ref(&cells)));
    let line = LogicalLine::table(0, cells, layout);

    let rows = wrap_line(&line, 1);
    assert_eq!(
        rows.iter().map(ToString::to_string).collect::<String>(),
        "ab"
    );
}
