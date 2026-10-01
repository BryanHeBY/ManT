use super::*;

#[test]
fn completed_rows_and_open_rows_are_distinct_in_nested_cell_results() {
    // Exact roff display/no-fill seeds were run first. term_vspace executes
    // completed blank rows; a LineBreak closes graph and leaves an open row.
    // These explicit IR facts must survive ANSI decoration and a parent cell.
    for (blocks, expected) in [
        (
            vec![paragraph(vec![text("X"), Inline::line_break()])],
            "X\n       C",
        ),
        (
            vec![literal(vec![text("X"), Inline::line_break()])],
            "X\n       C",
        ),
        (
            vec![
                paragraph(vec![text("X"), Inline::line_break()]),
                Block::VerticalSpace {
                    lines: 1,
                    source: None,
                },
            ],
            "X\n\n       C",
        ),
        (
            vec![
                literal(vec![text("X")]),
                Block::VerticalSpace {
                    lines: 1,
                    source: None,
                },
            ],
            "X\n\n       C",
        ),
        (vec![literal(vec![text("")])], "       C"),
        (vec![literal(vec![text(" ")])], "       C"),
    ] {
        let block = table(
            &[3, 3],
            vec![cell(blocks), cell(vec![paragraph(vec![text("C")])])],
            0,
        );
        for decorate in [plain as fn(TextPresentation, &str) -> String, ansi] {
            assert_eq!(
                undecorated(&renderer(&decorate).render_blocks(std::slice::from_ref(&block), 0)),
                expected
            );
        }
    }
}

#[test]
fn paragraph_tail_opens_a_cell_row_without_changing_ordinary_block_joins() {
    // Exact nested-It -> final filled paragraph -> Ta sources ran pristine
    // first: It BODY post closes the represented row, and the next column
    // starts the next physical row (mdoc_term.c:944/948; term.c:250-253).
    // The .No X / .br / .No C counterpart closes an ordinary block join
    // once. No VerticalSpace is requested by either native row close.
    let first = paragraph(vec![
        Inline::Strong {
            children: vec![text("X"), Inline::line_break()],
        },
        Inline::anchor("closed-row"),
    ]);
    for decorate in [plain as fn(TextPresentation, &str) -> String, ansi] {
        let renderer = renderer(&decorate);
        let block = table(
            &[3, 3],
            vec![
                cell(vec![first.clone()]),
                cell(vec![paragraph(vec![text("C")])]),
            ],
            0,
        );
        assert_eq!(
            undecorated(&renderer.render_blocks(&[block], 0)),
            "X\n       C"
        );
        assert_eq!(
            undecorated(&renderer.render_blocks(&[first.clone(), paragraph(vec![text("C")])], 0)),
            "X\nC"
        );
        let block = table(
            &[3, 3],
            vec![
                cell(vec![first.clone(), paragraph(vec![text("Y")])]),
                cell(vec![paragraph(vec![text("C")])]),
            ],
            0,
        );
        assert_eq!(
            undecorated(&renderer.render_blocks(&[block], 0)),
            "X\nY      C"
        );
    }
}

#[test]
fn paragraph_origins_do_not_write_cells_on_empty_physical_rows() {
    // Exact RS 4 / \&\c / B "\p D" sources ran pristine first. The
    // NBRZW pass ends an empty row, without ascii_advance(offset). The
    // following D starts at the saved offset. Authored blanks remain cells.
    for decorate in [plain as fn(TextPresentation, &str) -> String, ansi] {
        let renderer = renderer(&decorate);
        let mut first = paragraph(vec![Inline::line_break(), text("D")]);
        if let Block::Paragraph { layout, .. } = &mut first {
            layout.indent_columns = 4;
        }
        assert_eq!(undecorated(&renderer.render_blocks(&[first], 0)), "\n    D");

        let mut first = paragraph(vec![text(" "), Inline::line_break(), text("D")]);
        if let Block::Paragraph { layout, .. } = &mut first {
            layout.indent_columns = 4;
        }
        assert_eq!(
            undecorated(&renderer.render_blocks(&[first], 0)),
            "     \n    D"
        );

        let mut first = paragraph(vec![text("X"), Inline::line_break()]);
        if let Block::Paragraph { layout, .. } = &mut first {
            layout.indent_columns = 4;
        }
        let block = table(
            &[3, 3],
            vec![cell(vec![first]), cell(vec![paragraph(vec![text("C")])])],
            0,
        );
        assert_eq!(
            undecorated(&renderer.render_blocks(&[block], 0)),
            "    X\n       C"
        );
    }
}

#[test]
fn invalid_newline_decoration_does_not_truncate_source_rows() {
    let value = LayoutText::decorated("ONE\nTWO", "ONE".into());
    assert_eq!(value.visible, "ONE\nTWO");
    assert_eq!(value.rendered, "ONE\nTWO");
    let block = table(&[3], vec![cell(vec![literal(vec![text("ONE\nTWO")])])], 0);
    let output = renderer(&|_, text| text.replace('\n', "")).render_blocks(&[block], 0);
    assert_eq!(output, "ONE\nTWO");
}
