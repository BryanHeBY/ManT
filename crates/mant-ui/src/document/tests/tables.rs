//! Existing regressions grouped by tables behavior; expected values remain independent.
use super::*;

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
