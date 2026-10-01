//! Direct regression guards for one cell layout, then measurement and placement.

use super::{BlockRenderer, LayoutText, visits};
use crate::presentation::TextPresentation;
use mant_ir::{Block, Inline, LayoutHint, TableCell, TableCellKind, TableRow, TableRowKind};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn paragraph(children: Vec<Inline>) -> Block {
    Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn literal(children: Vec<Inline>) -> Block {
    Block::Preformatted {
        children,
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        blocks,
        kind: TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

fn table(widths: &[u16], cells: Vec<TableCell>, origin: i32) -> Block {
    Block::Table {
        rows: vec![TableRow {
            cells,
            kind: TableRowKind::Data,
        }],
        column_widths: widths.into(),
        layout: LayoutHint {
            indent_columns: origin,
            ..LayoutHint::default()
        },
        source: None,
    }
}

fn nested(depth: usize) -> Block {
    if depth == 0 {
        return paragraph(vec![text("LEAF")]);
    }
    table(
        &[2],
        vec![cell(vec![paragraph(vec![text("HEAD")]), nested(depth - 1)])],
        0,
    )
}

fn renderer(decorate: &dyn Fn(TextPresentation, &str) -> String) -> BlockRenderer<'_> {
    BlockRenderer {
        names: None,
        locations: None,
        decorate,
    }
}

fn plain(_: TextPresentation, value: &str) -> String {
    value.into()
}

fn ansi(_: TextPresentation, value: &str) -> String {
    format!("\x1b]8;;https://example.test\x1b\\\x1b[1m{value}\x1b[0m\x1b]8;;\x1b\\")
}

fn undecorated(value: &str) -> String {
    value
        .replace("\x1b]8;;https://example.test\x1b\\", "")
        .replace("\x1b]8;;\x1b\\", "")
        .replace("\x1b[1m", "")
        .replace("\x1b[0m", "")
}

#[test]
fn nested_columns_visit_each_block_and_fragment_once_for_plain_and_ansi() {
    // RV01's exact depth 0/1/2/4/8/12/16 sources were run with pristine
    // CVS before this assertion, including lint0 for all supported seeds.
    // termp_it_pre reads declaration widths; it does not re-render children.
    // This source-neutral counterpart directly counts the recursive entries,
    // independently of machine speed and without raising the supported depth.
    for depth in [0, 1, 2, 4, 8, 12, 16] {
        let block = nested(depth);
        let expected = std::iter::repeat_n("HEAD", depth)
            .chain(["LEAF"])
            .collect::<Vec<_>>()
            .join("\n");
        for decorate in [plain as fn(TextPresentation, &str) -> String, ansi] {
            let (output, counts) = visits::observe(|| {
                renderer(&decorate).render_blocks(std::slice::from_ref(&block), 0)
            });
            assert_eq!(undecorated(&output), expected, "depth={depth}");
            assert_eq!(counts.blocks, 2 * depth + 1, "depth={depth}");
            assert_eq!(counts.inline_fragments, depth + 1, "depth={depth}");
        }
    }
}

#[test]
fn sibling_subtrees_are_layout_once_even_in_topology_and_budget_fallbacks() {
    for variant in [
        "declared",
        "signed",
        "span",
        "rule",
        "width-budget",
        "cell-budget",
        "tbl",
    ] {
        let mut widths = vec![3, 3];
        let mut cells = vec![cell(vec![nested(4)]), cell(vec![nested(2)])];
        let origin = if variant == "signed" { -2 } else { 0 };
        match variant {
            "span" => cells[0].column_span = 2,
            "rule" => cells.push(TableCell {
                kind: TableCellKind::IsolatedHorizontalRule,
                ..cell(vec![])
            }),
            "width-budget" => widths = vec![3; 257],
            "cell-budget" => {
                cells.extend((2..257).map(|_| cell(vec![paragraph(vec![text("EXTRA")])])));
            }
            "tbl" => widths.clear(),
            _ => {}
        }
        let extras = usize::from(variant == "cell-budget") * 255;
        let block = table(&widths, cells, origin);
        let (plain_output, plain_counts) =
            visits::observe(|| renderer(&plain).render_blocks(std::slice::from_ref(&block), 0));
        let (styled_output, styled_counts) =
            visits::observe(|| renderer(&ansi).render_blocks(&[block], 0));
        assert_eq!(
            undecorated(&styled_output),
            plain_output,
            "variant={variant}"
        );
        assert_eq!(plain_counts.blocks, 15 + extras, "variant={variant}");
        assert_eq!(
            plain_counts.inline_fragments,
            8 + extras,
            "variant={variant}"
        );
        assert_eq!(styled_counts, plain_counts, "variant={variant}");
        assert_eq!(plain_output.matches("LEAF").count(), 2, "variant={variant}");
    }
}

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
fn completed_rows_and_open_rows_are_distinct_in_nested_cell_results() {
    // Exact roff display/no-fill seeds were run first. term_vspace executes
    // completed blank rows; a LineBreak closes graph and leaves an open row.
    // These explicit IR facts must survive ANSI decoration and a parent cell.
    for (blocks, expected) in [
        (
            vec![literal(vec![text("X"), Inline::line_break()])],
            "X\n       C",
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
fn invalid_newline_decoration_does_not_truncate_source_rows() {
    let value = LayoutText::decorated("ONE\nTWO", "ONE".into());
    assert_eq!(value.visible, "ONE\nTWO");
    assert_eq!(value.rendered, "ONE\nTWO");
    let block = table(&[3], vec![cell(vec![literal(vec![text("ONE\nTWO")])])], 0);
    let output = renderer(&|_, text| text.replace('\n', "")).render_blocks(&[block], 0);
    assert_eq!(output, "ONE\nTWO");
}

fn named_item() -> Block {
    use mant_ir::{
        EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot, EntryKind, EntryNameBinding,
        EntryNameEvidence, ListItem, ListKind, NameCase,
    };
    let slice = |bytes| EntryContentSlice {
        root: EntryInlineRoot::Block { index: 0 },
        path: vec![0],
        bytes,
    };
    Block::List {
        kind: ListKind::Plain,
        items: vec![ListItem {
            blocks: vec![paragraph(vec![Inline::Code {
                value: "é名-param".into(),
            }])],
            entry: Some(EntryFacts {
                id: "term-name".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: vec!["é名".into()],
                alias_groups: vec![],
                alias_of: None,
                value_domain: None,
                forms: vec![EntryForm {
                    parts: vec![slice(None)],
                }],
                name_bindings: vec![EntryNameBinding {
                    name: 0,
                    evidence: EntryNameEvidence::Declared,
                    occurrences: vec![EntryForm {
                        parts: vec![slice(Some(0..5))],
                    }],
                }],
            }),
            layout: mant_ir::ListItemLayout::default(),
            source: None,
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn matched_evidence(block: Block) -> mant_protocol::ExplanationEvidence {
    use mant_protocol::{
        EvidenceClass, ExplanationContent, ExplanationContentRange, ExplanationPreview,
        OutlineNodeReference, OutlineTrail,
    };
    mant_protocol::ExplanationEvidence {
        support: None,
        support_omitted: false,
        class: EvidenceClass::ContextMention,
        ordinal: 0,
        outline: OutlineTrail {
            ancestors: Vec::new(),
            node: OutlineNodeReference::DocumentRoot {
                path: "root".into(),
                id: "root".into(),
                title: "ROOT".into(),
            },
        },
        block_path: None,
        source: None,
        bases: vec![],
        previews: vec![ExplanationPreview {
            block_path: "root/b0/r0/c0/b0/i0/b0".into(),
            source: None,
            text: "é名-param".into(),
            match_start_char: 2,
            match_end_char: 4,
            content_ranges: vec![ExplanationContentRange::BlockText {
                path: vec![
                    mant_ir::ContentBlockStep::TableCell { row: 0, column: 0 },
                    mant_ir::ContentBlockStep::Block { index: 0 },
                    mant_ir::ContentBlockStep::ListItem { index: 0 },
                    mant_ir::ContentBlockStep::Block { index: 0 },
                ],
                start_char: 2,
                end_char: 4,
            }],
            clipped_before: false,
            clipped_after: false,
        }],
        previews_omitted: false,
        entry: None,
        content: Some(ExplanationContent::Block { block }),
        details_omitted: false,
        match_details_omitted: false,
        name_bindings_omitted: false,
        content_omitted: false,
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
fn defensive_placement_fallback_reuses_prepared_rows_without_a_tree_access() {
    use mant_ir::geometry::{ColumnFieldWidth, DeclaredColumns};
    let cells = vec![
        vec![LayoutText::decorated(
            "LEFT",
            ansi(TextPresentation::default(), "LEFT"),
        )],
        vec![LayoutText::decorated(
            "RIGHT",
            ansi(TextPresentation::default(), "RIGHT"),
        )],
    ];
    // This inconsistent measurement cannot come from from_text. A defensive
    // plan failure nevertheless receives only already prepared text results,
    // with no renderer/IR reference from which to repeat cell traversal.
    let widths = vec![
        vec![ColumnFieldWidth {
            content: 5,
            output: 4,
            completed: false,
        }],
        vec![ColumnFieldWidth::from_text("RIGHT")],
    ];
    let (output, counts) = visits::observe(|| {
        BlockRenderer::placed_column_row(&DeclaredColumns::new(&[3, 3]).unwrap(), cells, &widths, 2)
            .finish(false)
    });
    assert_eq!(undecorated(&output), "  LEFT\n  RIGHT");
    assert_eq!(counts, visits::Counts::default());
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
                format!("A{}{}", " | ".repeat(empty_cells), "\n".repeat(blank_rows))
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
