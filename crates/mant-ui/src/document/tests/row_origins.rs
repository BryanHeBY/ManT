//! Hard row origins remain presentation facts across source, wire, and visual consumers.
use super::*;

#[test]
fn native_definition_origins_survive_query_json_and_visual_copy() {
    // Exact inputs were run with registered pristine CVS ASCII, UTF-8 and
    // lint before adding this assertion. print_mdoc_node() handles NODE_LINE
    // before saving geometry; roff requests return without restoring offset,
    // while outer non-text scopes restore it before the last buffered word
    // flushes (mdoc_term.c:314-330, 393-397, 437-439).
    for (head, expected) in [
        (
            ".No Beta\n.No Gamma\n.No Delta\n",
            vec![("Alpha", 0), ("Beta", 6), ("Gamma", 6), ("Delta", 0)],
        ),
        (".No Beta\n", vec![("Alpha", 0), ("Beta", 0)]),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n.No Alpha\n.nf\n{head}.Xc\n.No BodyWord\n.El\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = decoded.into();
        let plain = mant_render::render_query_text(&content);
        for (word, origin) in &expected {
            let row = plain
                .lines()
                .find(|row| row.trim() == *word)
                .expect("CLI row");
            assert_eq!(row, format!("{}{word}", " ".repeat(*origin)), "{plain}");
        }
        let body = plain.lines().find(|row| row.trim() == "BodyWord").unwrap();
        assert_eq!(body, "      BodyWord", "{plain}");
        let markdown = mant_codec::encode::render_markdown(&content);
        if expected.len() > 2 {
            assert!(
                markdown.contains("&#160;&#160;&#160;&#160;&#160;&#160;Beta"),
                "{markdown}"
            );
        }
        let view = DocumentView::new(&content);
        for width in [20, 80, 120] {
            let rendered = view.render(width);
            let area =
                ratatui::layout::Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            ratatui::widgets::Widget::render(
                ratatui::widgets::Paragraph::new(rendered.text.clone()),
                area,
                &mut buffer,
            );
            let alpha = &rendered.search("Alpha")[0];
            for (word, origin) in &expected {
                let hit = &rendered.search(word)[0];
                assert_eq!(
                    hit.start_column,
                    alpha.start_column + origin,
                    "{word}, width={width}"
                );
                assert_eq!(
                    buffer[(
                        u16::try_from(hit.start_column).unwrap(),
                        u16::try_from(hit.row).unwrap()
                    )]
                        .symbol(),
                    &word[..1],
                );
                let copy = rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: 0,
                    },
                    focus: TextPosition {
                        row: hit.row,
                        column: hit.end_column - 1,
                    },
                });
                assert_eq!(copy, format!("{}{word}", " ".repeat(hit.start_column)));
            }
        }
    }
}

#[test]
fn structural_row_origins_leave_link_search_and_copy_coordinates_consistent() {
    let children = vec![
        Inline::Text {
            value: "Alpha".into(),
        },
        Inline::line_break_indented(6),
        Inline::Strong {
            children: vec![
                Inline::anchor_at("beta", None),
                Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "destination".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "Beta".into(),
                    }],
                },
            ],
        },
        Inline::line_break_indented(6),
        Inline::Text {
            value: "Gamma".into(),
        },
        Inline::line_break(),
        Inline::Text {
            value: "Delta".into(),
        },
    ];
    assert_eq!(mant_ir::inline_scalar_len(&children), 22);
    for block in [
        Block::Paragraph {
            children: children.clone(),
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Preformatted {
            children: children.clone(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
        Block::DefinitionList {
            compact: true,
            declaration_groups: vec![],
            layout: LayoutHint::default(),
            source: None,
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                layout: mant_ir::DefinitionLayout::default(),
                terms: vec![children.clone()],
                description: vec![],
            }],
        },
    ] {
        let mut content = bundle();
        content.document.as_mut().unwrap().sections[0].blocks = vec![block];
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = decoded.into();
        let view = DocumentView::new(&content);
        for width in [40, 120] {
            let rendered = view.render(width);
            let beta = &rendered.search("Beta")[0];
            let gamma = &rendered.search("Gamma")[0];
            let delta = &rendered.search("Delta")[0];
            assert_eq!(beta.start_column, delta.start_column + 6, "width={width}");
            assert_eq!(gamma.start_column, delta.start_column + 6, "width={width}");
            assert_eq!(
                rendered.link_target_at(beta.row, beta.start_column),
                Some(&LinkTarget::Section("destination".into()))
            );
            assert!(rendered.link_target_at(beta.row, 0).is_none());
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: beta.row,
                        column: 0
                    },
                    focus: TextPosition {
                        row: beta.row,
                        column: beta.end_column - 1
                    },
                }),
                format!("{}Beta", " ".repeat(beta.start_column))
            );
        }
    }
}

#[test]
fn row_origins_compose_before_visible_padding_and_paragraph_continuations() {
    let mut builder = DocumentBuilder::new("origins".into(), None);
    builder.blocks(
        &[Block::Paragraph {
            children: vec![
                Inline::Text {
                    value: "Alpha".into(),
                },
                Inline::line_break_indented(6),
                Inline::Text {
                    value: "Beta".into(),
                },
            ],
            layout: LayoutHint {
                indent_columns: -2,
                continuation_indent_columns: 1,
                ..Default::default()
            },
            source: None,
        }],
        0,
    );
    assert_eq!(builder.lines[0].indent, 0);
    assert_eq!(builder.lines[1].indent, 5);
    assert_eq!(builder.lines[1].continuation_indent, 5);
}

#[test]
fn nested_link_breaks_preserve_row_origins_and_reference_locations() {
    let nodes = vec![Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: "destination".into(),
        },
        title: None,
        children: vec![Inline::Emphasis {
            children: vec![
                Inline::Text {
                    value: "Alpha".into(),
                },
                Inline::line_break_indented(6),
                Inline::Text {
                    value: "Beta".into(),
                },
            ],
        }],
    }];
    let lines = crate::document::inline::styled_inline_lines(&nodes, Style::default(), None);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].indent_columns, 6);
    assert_eq!(lines[1].links[0].start_scalar, 0);
    assert_eq!(lines[1].links[0].end_scalar, 4);
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: nodes,
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&content).render(40);
    let alpha = &rendered.search("Alpha")[0];
    let hit = &rendered.search("Beta")[0];
    assert_eq!(
        (hit.start_column, hit.end_column),
        (alpha.start_column + 6, alpha.start_column + 10)
    );
    assert!(
        rendered
            .link_target_at(hit.row, hit.start_column - 1)
            .is_none()
    );
    assert_eq!(
        rendered.link_target_at(hit.row, hit.start_column),
        Some(&LinkTarget::Section("destination".into()))
    );
}
