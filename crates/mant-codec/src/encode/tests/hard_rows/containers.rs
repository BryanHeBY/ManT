//! Source-neutral containers preserve phrasing rows and literal fence rows.
use super::*;

fn enclosed(children: Vec<Inline>, container: u8) -> Block {
    match container {
        0 => paragraph(children),
        1 | 2 => Block::List {
            kind: if container == 1 {
                ListKind::Bullet
            } else {
                ListKind::Ordered { start: Some(3) }
            },
            compact: true,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![paragraph(children)],
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        3 | 4 => Block::DefinitionList {
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                layout: mant_ir::DefinitionLayout::default(),
                terms: (if container == 3 {
                    vec![children.clone()]
                } else {
                    vec![]
                })
                .into_iter()
                .map(Into::into)
                .collect(),
                description: if container == 4 {
                    vec![paragraph(children)]
                } else {
                    vec![]
                },
            }],
            declaration_groups: vec![],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        },
        5 => Block::Table {
            rows: vec![TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![TableCell {
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph(children)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                }],
            }],
            column_widths: vec![],
            layout: LayoutHint::default(),
            source: None,
        },
        _ => Block::Preformatted {
            inline_layout: mant_ir::InlineLayout::default(),
            children,
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
    }
}

fn restored_children(block: &Block) -> &[Inline] {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => children,
        Block::List { items, .. } => restored_children(&items[0].blocks[0]),
        other => panic!("uncovered reimport container: {other:?}"),
    }
}

fn assert_container(children: &[Inline], container: u8) {
    let mut original = query(vec![]);
    original.document.as_mut().unwrap().sections[0].blocks =
        vec![enclosed(children.to_vec(), container)];
    let wire = serde_json::to_string(original.document.as_ref().unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<Document>(&wire).unwrap(),
        *original.document.as_ref().unwrap()
    );
    let markdown = render_markdown_with_options(&original, MarkdownOptions::default());
    let restored = parse_content(&markdown, None).unwrap();
    let actual = restored_children(&restored.document.as_ref().unwrap().sections[0].blocks[0]);
    let actual_text = mant_ir::inline_plain_text(actual);
    let expected = mant_ir::inline_plain_text(children);
    if container >= 5 {
        // The dedicated closing syntax newline is removed by the reader,
        // keeping every authored row, including a final hard boundary.
        // They deliberately do not recreate fonts or active link ranges.
        assert_eq!(actual_text, expected, "container={container}: {markdown}");
    } else {
        assert_eq!(actual_text, expected, "{markdown}");
        let mut before = vec![];
        let mut after = vec![];
        styles(children, 0, &mut before);
        styles(actual, 0, &mut after);
        before.retain(|(character, _)| !character.is_whitespace());
        after.retain(|(character, _)| !character.is_whitespace());
        assert_eq!(before, after, "{markdown}");
    }
}

#[test]
fn hard_rows_cross_list_definition_body_and_fenced_container_boundaries() {
    for value in [
        "Alpha",
        "中e\u{301}🦀",
        "Alpha\u{a0}Beta",
        "<br /> &amp; literal",
    ] {
        for (leading, middle, trailing) in [
            (0, 0, 0),
            (1, 0, 0),
            (2, 0, 0),
            (0, 1, 0),
            (0, 2, 0),
            (0, 0, 1),
            (0, 0, 2),
            (2, 2, 2),
        ] {
            let mut rows = vec![Inline::line_break(); leading];
            rows.push(text(value));
            rows.extend(vec![Inline::line_break(); middle]);
            if middle > 0 {
                rows.push(text(value));
            }
            rows.extend(vec![Inline::line_break(); trailing]);
            for carrier in 0..6 {
                let children = wrap(&rows, carrier);
                for container in 0..7 {
                    assert_container(&children, container);
                }
            }
        }
    }
}

#[test]
fn row_only_containers_and_unsupported_block_quotes_keep_their_contracts() {
    for container in [5, 6] {
        assert_container(&[], container);
    }
    for rows in [1, 2, 3] {
        for container in 0..7 {
            assert_container(&vec![Inline::line_break(); rows], container);
        }
    }
    // There is no BlockQuote variant in public IR. That reader container
    // remains an exact Unsupported source, not a counterfeit paragraph axis.
    let source = "# Tool\n\n## TEXT\n\n> <br />\n> AFTER\n";
    let restored = parse_content(source, None).unwrap();
    assert!(
        matches!(&restored.document.as_ref().unwrap().sections[0].blocks[..],
        [Block::Unsupported { text, .. }] if text == "> <br />\n> AFTER\n")
    );
}

#[test]
fn canonical_paragraph_spelling_does_not_change_single_line_heading_export() {
    for depth in 3..=6 {
        let heading = mant_ir::Heading {
            inline_layout: mant_ir::InlineLayout::default(),
            content: vec![
                Inline::line_break(),
                text("Label"),
                Inline::line_break(),
                text("End"),
            ],
            source: None,
        };
        // Deep ATX headings have the established single-line projection.
        // Leading paragraph opt-in cannot introduce a new heading/body line.
        assert_eq!(
            super::super::super::render_heading(depth, &heading, MarkdownOptions::default()),
            format!("{}  Label End", "#".repeat(depth))
        );
    }
}
