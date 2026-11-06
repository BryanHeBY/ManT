//! Source-row witnesses distinguish CR fragments, true LF, and explicit breaks.
use super::*;

#[test]
fn original_code_row_hint_cases_keep_their_exact_accepted_payloads() {
    for (value, row, expected) in [
        ("A\rB\nC", 1, "A\nB\n   C"),
        ("A\r\nB\nC", 1, "A\n   B\nC"),
        ("A\r\nB\nC", 2, "A\nB\n   C"),
        ("A\rB\nC\rD\nE", 2, "A\nB\nC\nD\n   E"),
        ("A\nB\nC", 1, "A\n   B\nC"),
        ("A\rB\n", 1, "A\nB\n"),
    ] {
        let literal = literal(
            vec![text(value)],
            InlineLayout {
                row_hints: vec![hint(row, 3, 0)],
            },
        );
        assert_projection(&literal, expected);
        assert_projection(&table(vec![literal]), expected);
    }
}

#[test]
fn common_origin_survives_code_fragments_but_first_line_correction_does_not() {
    for (value, hints, expected) in [
        ("A\rB", vec![hint(0, 2, 3)], "     A\n  B"),
        ("A\r\nB", vec![hint(0, 2, 3)], "     A\nB"),
        ("A\rB", vec![hint(0, -2, 3)], " A\nB"),
        ("A\rB\r", vec![hint(0, 2, -3)], "A\n  B\n"),
        (
            "A\rB\nC\rD\nE",
            vec![hint(0, 1, 2), hint(1, 2, -1), hint(2, -2, 4)],
            "   A\n B\n C\n  D\n  E",
        ),
        (
            "界😀e\u{301}\r字\nZ",
            vec![hint(0, 1, 2), hint(1, -3, 5)],
            "   界😀e\u{301}\n 字\n  Z",
        ),
    ] {
        let block = literal(vec![text(value)], InlineLayout { row_hints: hints });
        assert_projection(&block, expected);
        assert_projection(&table(vec![block]), expected);
    }
}

#[test]
fn fragmented_crlf_and_explicit_hard_boundaries_have_distinct_original_rows() {
    for (children, hints, expected) in [
        (
            vec![
                text("A\r"),
                Inline::anchor("between"),
                text(""),
                Inline::Strong {
                    children: vec![text("\nB\nC")],
                },
            ],
            vec![hint(1, 3, 0)],
            "A\n   B\nC",
        ),
        (
            vec![text("A\r"), Inline::line_break(), text("B\nC")],
            vec![hint(1, 3, 0)],
            "A\n\n   B\nC",
        ),
        (
            vec![
                text("A"),
                Inline::line_break(),
                text("B"),
                Inline::line_break(),
                text("C"),
            ],
            vec![hint(1, 3, 0)],
            "A\n   B\nC",
        ),
        (
            vec![
                text("A\r"),
                Inline::Emphasis {
                    children: vec![text("\r")],
                },
                text("\nB\nC"),
            ],
            vec![hint(0, 2, 1), hint(1, 3, 0)],
            "   A\n\n   B\nC",
        ),
    ] {
        let block = literal(children, InlineLayout { row_hints: hints });
        assert_projection(&block, expected);
        assert_projection(&table(vec![block]), expected);
    }
}

#[test]
fn reading_and_inline_code_roles_do_not_steal_each_others_row_identity() {
    let paragraph = |children| Block::Paragraph {
        children,
        inline_layout: InlineLayout {
            row_hints: vec![hint(1, 3, 0)],
        },
        layout: LayoutHint::default(),
        source: None,
    };
    for (children, expected) in [
        (vec![text("A\rB\nC")], "A�B\n   C"),
        (
            vec![Inline::Code {
                value: "A\rB\nC".into(),
            }],
            "A\nB\n   C",
        ),
        (
            vec![
                Inline::Code {
                    value: "A\r".into(),
                },
                text("\nB\nC"),
            ],
            "A\n\n   B\nC",
        ),
        (
            vec![
                text("A\r"),
                Inline::Code {
                    value: "\nB\nC".into(),
                },
            ],
            "A�\n   B\nC",
        ),
        (
            vec![
                Inline::Code {
                    value: "A\r".into(),
                },
                text(""),
                Inline::anchor("between-code"),
                Inline::Code {
                    value: "\nB\nC".into(),
                },
            ],
            "A\n   B\nC",
        ),
    ] {
        assert_projection(&table(vec![paragraph(children)]), expected);
    }
    let unsupported = Block::Unsupported {
        name: None,
        text: "\nA\rB\nC\n".into(),
        layout: LayoutHint::default(),
        source: None,
    };
    assert_projection(&table(vec![unsupported]), "A�B\nC");
}

#[test]
fn accepted_rows_keep_other_control_and_unicode_roles_unchanged() {
    let value = "A\u{a0}B\u{2028}C\u{2029}D\u{200b}E\x0cF\u{81}G\rZ\nY";
    let expected = "  A\u{a0}B\u{2028}C\u{2029}D\u{200b}E F  G\n Z\n   Y";
    let block = literal(
        vec![text(value)],
        InlineLayout {
            row_hints: vec![hint(0, 1, 1), hint(1, 3, 0)],
        },
    );
    assert_projection(&block, expected);
    assert_projection(&table(vec![block]), expected);
}

#[test]
fn open_code_tail_coordinates_follow_the_original_row_until_content_arrives() {
    for (value, hints, expected) in [
        ("A\r", vec![hint(0, 3, 2)], "     A\n    | B"),
        ("A\r", vec![hint(0, 0, 5)], "     A\n | B"),
        (
            "A\r\n",
            vec![hint(0, 3, 2), hint(1, 4, 1)],
            "     A\n      | B",
        ),
        (
            "A\rB\n",
            vec![hint(0, 3, 0), hint(1, 7, 0)],
            "   A\n   B\n        | B",
        ),
    ] {
        let mut block = table(vec![literal(
            vec![text(value)],
            InlineLayout { row_hints: hints },
        )]);
        let Block::Table { rows, .. } = &mut block else {
            unreachable!()
        };
        rows[0].cells.push(cell(vec![Block::Paragraph {
            children: vec![text("B")],
            inline_layout: InlineLayout::default(),
            layout: LayoutHint::default(),
            source: None,
        }]));
        assert_projection(&block, expected);
    }
}

#[test]
fn row_layout_alone_never_authors_literal_text_or_empty_tail_padding() {
    let layout = InlineLayout {
        row_hints: vec![hint(0, 5, 3)],
    };
    assert_projection(&literal(vec![text("")], layout.clone()), "");
    for children in [vec![], vec![Inline::anchor("only-destination")]] {
        let block = literal(children, layout.clone());
        literal_content(&block)
            .layout
            .validate(literal_content(&block).content)
            .unwrap();
        let markdown =
            render_blocks_fragment(&[block], MarkdownFragmentOptions::default()).join("\n\n");
        assert!(!markdown.contains("```"));
        assert_eq!(markdown, "");
    }
}
