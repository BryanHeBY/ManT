//! Hard and soft row origins compose before visible layout.
use super::super::{
    Block, DefinitionItem, DocumentBuilder, DocumentView, HashMap, Inline, LayoutHint, LinkTarget,
    RenderedSelection, ResolvedContent, Style, TextPosition, bundle,
};
use super::fixtures::layout;

#[test]
fn structural_row_origins_leave_link_search_and_copy_coordinates_consistent() {
    let children = vec![
        Inline::Text {
            value: "Alpha".into(),
        },
        Inline::line_break(),
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
        Inline::line_break(),
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
            inline_layout: layout(&[(1, 6), (2, 6)]),
            children: children.clone(),
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Preformatted {
            inline_layout: layout(&[(1, 6), (2, 6)]),
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
                head_body_relation: mant_ir::HeadBodyRelation::Separate,
                source: None,
                entry: None,
                layout: mant_ir::DefinitionLayout::default(),
                terms: vec![mant_ir::DefinitionTerm {
                    content: children.clone(),
                    inline_layout: layout(&[(1, 6), (2, 6)]),
                }],
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
                format!("{}Beta", " ".repeat(beta.start_column.saturating_sub(6)))
            );
        }
    }
}

#[test]
fn row_origins_compose_before_visible_padding_and_paragraph_continuations() {
    let mut builder = DocumentBuilder::new("origins".into(), None);
    builder.blocks(
        &[Block::Paragraph {
            inline_layout: layout(&[(1, 6)]),
            children: vec![
                Inline::Text {
                    value: "Alpha".into(),
                },
                Inline::line_break(),
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
                Inline::line_break(),
                Inline::Text {
                    value: "Beta".into(),
                },
            ],
        }],
    }];
    let hints = layout(&[(1, 6)]);
    let lines = crate::document::inline::styled_reference_content_lines(
        mant_ir::InlineContentRef {
            content: &nodes,
            layout: &hints,
        },
        Style::default(),
        None,
        &[],
        false,
        &HashMap::default(),
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].indent_columns, 6);
    assert_eq!(lines[1].links[0].start_scalar, 0);
    assert_eq!(lines[1].links[0].end_scalar, 4);
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        inline_layout: hints,
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

#[test]
fn first_and_soft_origins_keep_hanging_increment_through_resize() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Alpha Beta Gamma\nDelta Epsilon".into(),
        }],
        inline_layout: layout(&[(0, 1), (1, -1)]),
        layout: LayoutHint {
            indent_columns: 2,
            continuation_indent_columns: 4,
            ..Default::default()
        },
        source: None,
    }];
    let view = DocumentView::new(&content);
    assert_eq!(
        (view.lines[0].indent, view.lines[0].continuation_indent),
        (3, 7)
    );
    assert_eq!(
        (view.lines[1].indent, view.lines[1].continuation_indent),
        (5, 5)
    );
    for width in [12, 80, 12] {
        let rendered = view.render(width);
        // The existing narrow-view policy translates both origins by one
        // cell so six columns remain available; their four-cell delta stays.
        assert_eq!(
            rendered.search("Alpha")[0].start_column,
            if width == 12 { 2 } else { 3 }
        );
        assert_eq!(rendered.search("Delta")[0].start_column, 5);
        if width == 12 {
            assert_eq!(rendered.search("Beta")[0].start_column, 6);
            assert_eq!(rendered.search("Gamma")[0].start_column, 6);
            assert_eq!(rendered.search("Epsilon")[0].start_column, 5);
        }
        let area = ratatui::layout::Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(rendered.text.clone()),
            area,
            &mut buffer,
        );
        for word in ["Alpha", "Beta", "Gamma", "Delta", "Epsilon"] {
            let hit = &rendered.search(word)[0];
            assert_eq!(
                buffer[(
                    hit.start_column.try_into().unwrap(),
                    hit.row.try_into().unwrap()
                )]
                    .symbol(),
                &word[..1]
            );
        }
    }
}

#[test]
fn heading_hints_do_not_become_text_or_trim_author_whitespace() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.heading = Some(mant_ir::Heading {
        content: vec![
            Inline::Strong {
                children: vec![Inline::Text {
                    value: " Alpha\u{a0} ".into(),
                }],
            },
            Inline::line_break(),
            Inline::anchor("heading-second"),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![Inline::Code {
                    value: " Beta  ".into(),
                }],
            },
        ],
        inline_layout: layout(&[(0, 4), (1, 2)]),
        source: None,
    });
    let view = DocumentView::new(&content);
    let rendered = view.render(80);
    for (word, origin, original) in [("Alpha", 4, " Alpha\u{a0} "), ("Beta", 2, " Beta  ")] {
        let hit = &rendered.search(word)[0];
        assert_eq!(hit.start_column, origin + 1);
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: 0
                },
                focus: TextPosition {
                    row: hit.row,
                    column: 79
                },
            }),
            original
        );
        assert_eq!(
            rendered.search(&format!("{}{}", " ".repeat(origin + 1), word)),
            []
        );
    }
    let beta = &rendered.search("Beta")[0];
    assert_eq!(rendered.anchor_row("heading-second"), Some(beta.row));
    assert!(rendered.link_target_at(beta.row, 0).is_none());
    assert_eq!(
        rendered.link_target_at(beta.row, beta.start_column),
        Some(&LinkTarget::Section("target".into()))
    );
}
