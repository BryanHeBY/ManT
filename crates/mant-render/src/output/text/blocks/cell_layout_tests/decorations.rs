use super::*;

#[test]
fn visible_rows_keep_unicode_clusters_and_authored_tail_padding_across_styles() {
    // Exact ASCII full/short and CJK roff fields were checked before writing
    // these expectations (mdoc_term.c::termp_it_pre, term.c::term_flushln).
    // The remaining cases exercise source-neutral Unicode/style composition;
    // width belongs to the complete visible row, never individual ANSI spans.
    for (children, expected) in [
        (vec![text("AAAA B ")], "AAAA B C"),
        (vec![text("AAAA BB")], "AAAA BB\n       C"),
        (vec![text("X          ")], "X          C"),
        (vec![text("中")], "中     C"),
        (
            vec![
                text("e"),
                Inline::Strong {
                    children: vec![text("\u{301}")],
                },
            ],
            "e\u{301}      C",
        ),
        (
            vec![
                text("👩"),
                Inline::Emphasis {
                    children: vec![text("\u{200d}")],
                },
                Inline::Strong {
                    children: vec![text("💻")],
                },
            ],
            "👩\u{200d}💻     C",
        ),
        (vec![text("X\u{a0}")], "X\u{a0}     C"),
    ] {
        let block = table(
            &[3, 3],
            vec![
                cell(vec![paragraph(children)]),
                cell(vec![paragraph(vec![text("C")])]),
            ],
            0,
        );
        for decorate in [plain as fn(TextPresentation, &str) -> String, ansi] {
            let output = renderer(&decorate).render_blocks(std::slice::from_ref(&block), 0);
            assert_eq!(undecorated(&output), expected);
        }
    }
}

#[test]
fn measurement_reuses_name_and_match_decorated_fragments_without_revisiting() {
    use crate::output::styles::LocatedStyles;
    use std::cell::RefCell;
    let block = table(
        &[20, 3],
        vec![
            cell(vec![named_item()]),
            cell(vec![paragraph(vec![text("C")])]),
        ],
        0,
    );
    let evidence = matched_evidence(block);
    let mant_protocol::ExplanationContent::Block { block } = evidence.content.as_ref().unwrap()
    else {
        unreachable!()
    };
    let styles = LocatedStyles::for_support(block, std::iter::once((&evidence, &[][..])));
    let seen = RefCell::new(Vec::new());
    let decorate = |presentation: TextPresentation, value: &str| {
        seen.borrow_mut().push((presentation, value.to_owned()));
        ansi(presentation, value)
    };
    let renderer = BlockRenderer {
        locations: Some(&styles),
        names: None,
        decorate: &decorate,
    };
    let (output, counts) =
        visits::observe(|| renderer.render_blocks(std::slice::from_ref(block), 0));
    assert_eq!(
        undecorated(&output),
        format!("é名-param{}C", " ".repeat(15))
    );
    assert_eq!(counts.blocks, 4);
    assert_eq!(counts.inline_fragments, 4);
    assert_eq!(
        seen.borrow()
            .iter()
            .filter(|(p, _)| p.inline.entry_kind.is_some())
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>(),
        ["é名"]
    );
    assert_eq!(
        seen.borrow()
            .iter()
            .filter(|(p, _)| p.matched)
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>(),
        ["-p"]
    );
}

#[test]
fn trimming_visible_delimiters_preserves_opaque_decoration_closures() {
    let blocks = [
        paragraph(vec![text("X\n")]),
        paragraph(vec![text("Y"), Inline::line_break()]),
        Block::Unsupported {
            text: "\nZ\n".into(),
            source: None,
            layout: LayoutHint::default(),
            name: Some("source-neutral-test".into()),
        },
        table(&[], vec![cell(vec![literal(vec![text("T\n")])])], 0),
    ];
    for block in blocks {
        let output = renderer(&ansi).render_blocks(std::slice::from_ref(&block), 0);
        assert_eq!(
            undecorated(&output),
            renderer(&plain).render_blocks(&[block], 0)
        );
        assert_eq!(
            output.matches("\x1b[1m").count(),
            output.matches("\x1b[0m").count(),
            "{output:?}"
        );
        assert_eq!(
            output.matches("\x1b]8;;https://example.test\x1b\\").count(),
            output.matches("\x1b]8;;\x1b\\").count(),
            "{output:?}"
        );
        assert!(output.ends_with("\x1b]8;;\x1b\\"), "{output:?}");
    }
    let block = table(
        &[3, 3],
        vec![
            cell(vec![literal(vec![text("X"), Inline::line_break()])]),
            cell(vec![paragraph(vec![text("C")])]),
        ],
        0,
    );
    let output = renderer(&ansi).render_blocks(&[block], 0);
    assert_eq!(undecorated(&output), "X\n       C");
    assert_eq!(
        output.matches("\x1b[1m").count(),
        output.matches("\x1b[0m").count()
    );
}

#[test]
fn dense_empty_tail_segments_keep_opaque_closures_and_real_blank_rows() {
    // This is a source-neutral decorator/IR counter, not an invented roff
    // expectation. tbl_term.c consumes physical rows while any cell remains;
    // retiring empty tail separators cannot retire the actual row or a style.
    for (value, blank_rows) in [("A\n\n", 1), ("A\n\n\n", 2)] {
        for empty_cells in [0, 1, 2] {
            let mut cells = vec![cell(vec![literal(vec![text(value)])])];
            cells.extend((0..empty_cells).map(|_| cell(vec![])));
            let block = table(&[], cells, 0);
            let output = renderer(&ansi).render_blocks(std::slice::from_ref(&block), 0);
            assert_eq!(
                undecorated(&output),
                format!(
                    "A{}{}",
                    " | ".repeat(empty_cells),
                    "\n".repeat(blank_rows + 1)
                )
            );
            assert_eq!(
                output.matches("\x1b[1m").count(),
                output.matches("\x1b[0m").count(),
                "{output:?}"
            );
            assert_eq!(
                output.matches("\x1b]8;;https://example.test\x1b\\").count(),
                output.matches("\x1b]8;;\x1b\\").count(),
                "{output:?}"
            );
            // Closing a completed empty physical row happens after opaque
            // style/link closures, without retiring that row at EOF.
            assert!(output.ends_with("\x1b]8;;\x1b\\\n"), "{output:?}");
        }
    }
    let blocks = [
        paragraph(vec![text("BEFORE")]),
        table(&[], vec![cell(vec![])], 0),
        paragraph(vec![text("AFTER")]),
    ];
    assert_eq!(
        renderer(&plain).render_blocks(&blocks, 0),
        "BEFORE\n\nAFTER"
    );
    assert_eq!(
        undecorated(&renderer(&ansi).render_blocks(&blocks, 0)),
        "BEFORE\n\nAFTER"
    );
}
