//! Native row ends remain distinct from completed gaps in declared cells.

use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView, RenderedDocument};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

#[path = "support/reader_actions.rs"]
mod reader_actions;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn query(control: &str) -> ResolvedContent {
    let source = format!(
        "{HEADER}.Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n.No \"\\p D\"\n{control}.No AFTER\n.Xc Ta Lk https://e.example/x RightWord\n.El\n.Sh NEXT\n.No END\n"
    );
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&loaded, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn body_hit(rendered: &RenderedDocument, word: &str) -> mant_ui::RenderedSearchMatch {
    let start = rendered.anchor_row("description").unwrap();
    let end = rendered.anchor_row("next").unwrap();
    let hits = rendered
        .search(word)
        .into_iter()
        .filter(|hit| hit.row > start && hit.row < end)
        .collect::<Vec<_>>();
    assert_eq!(hits.len(), 1, "{word}: {:?}", rendered.text);
    hits.into_iter().next().unwrap()
}

#[test]
fn paragraph_and_literal_row_closes_keep_search_order_without_extra_gaps() {
    // These exact complete sources ran pristine ASCII/UTF-8/HTML/tree/lint
    // before the assertions, lint=0. Nested It clears NOBREAK and closes
    // AFTER's filled row; Bd keeps Marker/AFTER together. A following Ta
    // begins the next physical row without a term_vspace() empty row.
    for (control, marker_delta) in [
        (".Bl -item -compact\n.It\n.No Marker\n.El\n", 1),
        (".Bd -literal -compact\nMarker\n.Ed\n", 0),
    ] {
        let query = query(control);
        let view = DocumentView::new(&query);
        for width in [240, 120, 80, 24, 16, 8, 240] {
            let rendered = view.render(width);
            let marker = body_hit(&rendered, "Marker");
            let after = body_hit(&rendered, "AFTER");
            let right = body_hit(&rendered, "RightWord");
            assert!(right.row > after.row, "{width}: {:?}", rendered.text);
            if width >= 80 {
                assert_eq!(after.row - marker.row, marker_delta);
                assert_eq!(right.row - after.row, 1);
            }
            for row in &rendered.text.lines[after.row + 1..right.row] {
                assert!(
                    !row.to_string().trim().is_empty(),
                    "invented completed row at {width}: {:?}",
                    rendered.text
                );
            }
        }
        assert_pointer_and_copy(&query, "RightWord", "https://e.example/x");
    }
}

#[test]
fn stacked_cells_reuse_only_an_open_logical_tail() {
    use mant_ir::{Block, Inline, TableCell, TableCellKind, TableRow, TableRowKind};

    let text = |value: &str| Inline::Text {
        value: value.into(),
    };

    // The complete source controls above were run on pristine CVS before
    // this model counter: It/Bd post exposes a hard row, not term_vspace.
    // These public IR variants also distinguish a completed VerticalSpace
    // and author-written blank cells from the open delimiter itself.
    for (literal, authored_blank, completed_gap, expected_distance) in [
        (false, false, false, 1),
        (true, false, false, 1),
        (false, true, false, 2),
        (true, true, false, 2),
        (false, false, true, 2),
        // C08: a literal tail is authored row content, so VerticalSpace(1)
        // completes another row. A Paragraph terminator is instead closed
        // before that block gap. The source-neutral CLI renderer gives
        // FIRST\n\n\nSECOND here, and FIRST\n\nSECOND for the Paragraph.
        (true, false, true, 3),
    ] {
        let mut children = vec![text("FIRST"), Inline::line_break()];
        if authored_blank {
            children.extend([text(" "), Inline::line_break()]);
        }
        children.push(Inline::anchor("tail-owner"));
        let first = if literal {
            Block::Preformatted {
                inline_layout: mant_ir::InlineLayout::default(),
                children,
                language: None,
                layout: mant_ir::LayoutHint::default(),
                source: None,
            }
        } else {
            Block::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children,
                layout: mant_ir::LayoutHint::default(),
                source: None,
            }
        };
        let mut first = vec![first];
        if completed_gap {
            first.push(Block::VerticalSpace {
                lines: 1,
                source: None,
            });
        }
        let target = mant_ir::LinkTarget::External {
            uri: "https://e.example/cell".into(),
        };
        let second = Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children: vec![Inline::Link {
                target,
                title: None,
                children: vec![text("SECOND")],
            }],
            layout: mant_ir::LayoutHint::default(),
            source: None,
        };
        let cell = |blocks| TableCell {
            blocks,
            kind: TableCellKind::Text,
            column_span: 1,
            row_span: 1,
            alignment: None,
        };
        let mut query = query(".Bd -literal -compact\nMarker\n.Ed\n");
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![Block::Table {
            rows: vec![TableRow {
                cells: vec![cell(first), cell(vec![second])],
                kind: TableRowKind::Data,
            }],
            column_widths: vec![8, 4],
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }];
        let rendered = DocumentView::new(&query).render(8);
        let first = rendered.search("FIRST");
        let second = rendered.search("SECOND");
        assert_eq!(first.len(), 1, "{:?}", rendered.text);
        assert_eq!(second.len(), 1, "{:?}", rendered.text);
        assert_eq!(
            second[0].row - first[0].row,
            expected_distance,
            "literal={literal}, authored_blank={authored_blank}, completed_gap={completed_gap}: {:?}",
            rendered.text,
        );
        assert_pointer_and_copy(&query, "SECOND", "https://e.example/cell");
        if !completed_gap {
            assert_eq!(rendered.anchor_row("tail-owner"), Some(second[0].row));
        }
    }
}

fn nested_cell_query(definition: bool, literal: bool, gap: u16) -> ResolvedContent {
    use mant_ir::{
        Block, DefinitionItem, DefinitionLayout, Inline, ListItem, ListKind, TableCell,
        TableCellKind, TableRow, TableRowKind,
    };

    let text = |value: &str| Inline::Text {
        value: value.into(),
    };
    let children = vec![text("FIRST"), Inline::line_break()];
    let payload = if literal {
        Block::Preformatted {
            inline_layout: mant_ir::InlineLayout::default(),
            children,
            language: None,
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }
    } else {
        Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children,
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }
    };
    let first = if definition {
        Block::DefinitionList {
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                terms: vec![vec![text("HEAD")].into()],
                description: vec![payload],
                layout: DefinitionLayout {
                    body_indent_columns: 0,
                    ..Default::default()
                },
            }],
            declaration_groups: Vec::new(),
            compact: true,
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }
    } else {
        Block::List {
            kind: ListKind::Plain,
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![payload],
            }],
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }
    };
    let cell = |blocks| TableCell {
        blocks,
        kind: TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut query = query(".Bd -literal -compact\nMarker\n.Ed\n");
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![
                cell(vec![
                    first,
                    Block::VerticalSpace {
                        lines: gap,
                        source: None,
                    },
                ]),
                cell(vec![Block::Paragraph {
                    inline_layout: mant_ir::InlineLayout::default(),
                    children: vec![text("SECOND")],
                    layout: mant_ir::LayoutHint::default(),
                    source: None,
                }]),
            ],
            kind: TableRowKind::Data,
        }],
        column_widths: vec![8, 4],
        layout: mant_ir::LayoutHint::default(),
        source: None,
    }];
    query
}

#[test]
fn nested_paragraph_terminators_do_not_become_completed_cell_rows() {
    // The exact cw10_list source ran pristine in all five profiles first:
    // B, one empty row, C (lint=2 for its filled blank line/standalone Ta).
    // Nested It post executes term_newln(), not term_vspace() itself
    // (mdoc_term.c:930-963). These source-neutral counters retain the CLI
    // distinction: only a final direct cell Paragraph has an open tail;
    // nested ordinary paragraphs close at their block boundary. Literal
    // delimiters remain content even before an explicit VerticalSpace.
    for definition in [false, true] {
        for literal in [false, true] {
            for gap in [0, 1] {
                let query = nested_cell_query(definition, literal, gap);
                let expected = match (literal, gap) {
                    (false, 0) => 0,
                    (true, 0) => 1,
                    (false, _) => 2,
                    (true, _) => 3,
                };
                let plain = mant_render::render_query_text(&query);
                let first = plain.lines().position(|row| row.contains("FIRST")).unwrap();
                let second = plain
                    .lines()
                    .position(|row| row.contains("SECOND"))
                    .unwrap();
                assert_eq!(second - first, expected, "{plain:?}");
                for width in [8, 80, 240, 8] {
                    let rendered = DocumentView::new(&query).render(width);
                    let first = rendered.search("FIRST");
                    let second = rendered.search("SECOND");
                    assert_eq!(first.len(), 1, "{:?}", rendered.text);
                    assert_eq!(second.len(), 1, "{:?}", rendered.text);
                    let expected = if width == 8 {
                        expected.max(1)
                    } else {
                        expected
                    };
                    assert_eq!(
                        second[0].row - first[0].row,
                        expected,
                        "definition={definition}, literal={literal}, gap={gap}, width={width}: {:?}",
                        rendered.text
                    );
                }
            }
        }
    }
}

fn word_position(buffer: &Buffer, word: &str) -> (u16, u16) {
    positions(buffer, word)
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .unwrap()
}

fn assert_pointer_and_copy(query: &ResolvedContent, label: &str, target: &str) {
    let mut app = App::new(query);
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    for width in [80, 128, 240, 80] {
        terminal.backend_mut().resize(width, 40);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = word_position(terminal.backend().buffer(), label);
        click(&mut app, column, row);
        let activated = opened_targets(&mut app);
        assert_eq!(activated, [target], "width={width}");
        let last = column + u16::try_from(label.len()).unwrap() - 1;
        select_span(&mut app, column, row, last, None);
        let copied = copied_selections(&mut app, |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("visual selection expected")
            };
            text
        });
        assert_eq!(copied, [label], "width={width}");
    }
}
