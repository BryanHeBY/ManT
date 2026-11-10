//! Per-item resolved boundaries precede the marker and survive presentation.
use super::*;

fn list(compact: bool, gap: u16, items: Vec<ListItem>) -> Block {
    Block::List {
        kind: ListKind::Bullet,
        compact,
        items,
        layout: LayoutHint {
            spacing_before_lines: gap,
            ..Default::default()
        },
        source: None,
    }
}

fn item(gap: Option<u16>, blocks: Vec<Block>) -> ListItem {
    ListItem {
        blocks,
        entry: None,
        source: None,
        layout: mant_ir::ListItemLayout {
            spacing_before_lines: gap,
        },
    }
}

fn row(lines: &[String], token: &str) -> usize {
    lines.iter().position(|line| line.contains(token)).unwrap()
}

#[test]
fn native_pd_two_zero_two_matches_text_and_tui_item_boundaries() {
    let query = mant_loader::load_roff_bytes(b".TH GAPS 1\n.SH DESCRIPTION\nBEFORE\n.PD 2\n.IP 1. 4\nFIRST\n.PD 0\n.IP 2. 4\nSECOND\n.PD 2\n.IP 3. 4\nTHIRD\n").unwrap();
    let document = query.document.as_ref().unwrap();
    let Block::List { items, .. } = document.sections[0]
        .blocks
        .iter()
        .find(|block| matches!(block, Block::List { .. }))
        .unwrap()
    else {
        unreachable!();
    };
    assert_eq!(
        items
            .iter()
            .map(|item| item.layout.spacing_before_lines)
            .collect::<Vec<_>>(),
        [Some(2), Some(0), Some(2)]
    );
    let text = mant_render::render_query_text(&query)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let before = query.clone();
    for width in [40, 80, 120] {
        let rendered = DocumentView::new(&query).render(width);
        let ui = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for (first, second, distance) in [
            ("BEFORE", "FIRST", 3),
            ("FIRST", "SECOND", 1),
            ("SECOND", "THIRD", 3),
        ] {
            assert_eq!(row(&text, second) - row(&text, first), distance, "{text:?}");
            assert_eq!(row(&ui, second) - row(&ui, first), distance, "{ui:?}");
        }
    }
    assert_eq!(query, before);
}

#[test]
fn explicit_zero_is_distinct_from_inherited_loose_list_spacing() {
    for compact in [false, true] {
        let mut builder = DocumentBuilder::new("item gaps".into(), None);
        builder.blocks(
            &[list(
                compact,
                0,
                vec![
                    item(None, vec![paragraph("FIRST")]),
                    item(Some(0), vec![paragraph("SECOND")]),
                    item(None, vec![paragraph("THIRD")]),
                    item(Some(2), vec![paragraph("FOURTH")]),
                ],
            )],
            0,
        );
        let lines = builder
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(row(&lines, "SECOND") - row(&lines, "FIRST"), 1);
        assert_eq!(
            row(&lines, "THIRD") - row(&lines, "SECOND"),
            if compact { 1 } else { 2 }
        );
        assert_eq!(row(&lines, "FOURTH") - row(&lines, "THIRD"), 3);
    }
}

#[test]
fn nonparagraph_body_keeps_item_gap_before_the_whole_marker() {
    let code = Block::Preformatted {
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        children: vec![Inline::Text {
            value: "CODE".into(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let blocks = vec![
        paragraph("BEFORE"),
        list(true, 0, vec![item(Some(3), vec![code])]),
    ];
    let mut builder = DocumentBuilder::new("code item".into(), None);
    builder.blocks(&blocks, 0);
    assert_eq!(builder.lines.len(), 6);
    assert_eq!(builder.lines[4].spans[0].content, "• ");
    assert_eq!(builder.lines[5].spans[0].content, "CODE");
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = blocks;
    let text = mant_render::render_query_text(&query);
    assert_eq!(text, "demo\n\nBEFORE\n\n\n\n•\n  CODE");
}

#[test]
fn container_and_item_boundary_share_one_bounded_gap_before_marker() {
    for (outer, inner, bounded) in [(3000, 1096, false), (3000, 1097, true)] {
        let blocks = vec![
            paragraph("BEFORE"),
            list(
                true,
                outer,
                vec![item(Some(inner), vec![paragraph("AFTER")])],
            ),
        ];
        assert_eq!(mant_ir::geometry::has_bounded_gap(&blocks), bounded);
        let mut builder = DocumentBuilder::new("bounded item".into(), None);
        builder.blocks(&blocks, 0);
        assert_eq!(builder.lines.len(), 4098);
        assert_eq!(builder.lines[4097].spans[0].content, "• ");
        assert!(
            builder.lines[4097]
                .spans
                .iter()
                .any(|span| span.content == "AFTER")
        );
    }
}

#[test]
fn marker_and_first_row_share_a_gap_budget_only_when_they_share_a_line() {
    const URI: &str = "https://example.test/list-body";
    for (indent, row_indent, first_indent, shares_line) in [
        (0, 0, -1, false),
        (-1, 1, 1, true),
        (-1, 2, -2, false),
        (1, -1, 0, true),
    ] {
        let body = Block::Paragraph {
            children: vec![
                Inline::anchor("body-target"),
                Inline::Link {
                    target: mant_ir::LinkTarget::External { uri: URI.into() },
                    title: None,
                    children: vec![Inline::Text {
                        value: "BODY".into(),
                    }],
                },
            ],
            inline_layout: mant_ir::InlineLayout {
                row_hints: vec![mant_ir::RowLayoutHint {
                    indent_columns: row_indent,
                    first_line_indent_columns: first_indent,
                    ..Default::default()
                }],
            },
            layout: LayoutHint {
                indent_columns: indent,
                spacing_before_lines: 3000,
                ..Default::default()
            },
            source: None,
        };
        let mut query = bundle();
        let document = query.document.as_mut().unwrap();
        document.sections.clear();
        document.blocks = vec![
            paragraph("BEFORE"),
            list(true, 0, vec![item(Some(3000), vec![body])]),
        ];
        assert_eq!(mant_ir::validate_document(document), []);
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
        let query: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
            .unwrap()
            .into();
        let blocks = &query.document.as_ref().unwrap().blocks;
        let case = (indent, row_indent, first_indent);
        let text = mant_render::render_query_text(&query)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let rendered = DocumentView::new(&query).render(80);
        let ui = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        for lines in [&text, &ui] {
            let before = row(lines, "BEFORE");
            let marker = row(lines, "•");
            let body = row(lines, "BODY");
            let distance = if shares_line {
                usize::from(mant_ir::geometry::MAX_GAP_ROWS) + 1
            } else {
                3001
            };
            assert_eq!(marker - before, distance, "{case:?}");
            assert_eq!(
                body - marker,
                if shares_line { 0 } else { 3001 },
                "{case:?}"
            );
        }
        let hit = &rendered.search("BODY")[0];
        assert_eq!(rendered.anchor_row("body-target"), Some(hit.row));
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::External(ExternalUri::parse(URI).unwrap()))
        );
        assert_eq!(
            mant_ir::geometry::has_bounded_gap(blocks),
            shares_line,
            "{case:?}"
        );
    }
}
