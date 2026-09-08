//! Existing regressions grouped by navigation behavior; expected values remain independent.
use super::*;

#[test]
fn ordinary_list_entry_anchors_preserve_rows_and_numbering() {
    let mut query = bundle();
    let paragraph = |name: &str| Block::Paragraph {
        children: vec![
            Inline::Code { value: name.into() },
            Inline::Text {
                value: ": visible | body".into(),
            },
        ],
        layout: LayoutHint {
            spacing_before_lines: 2,
            ..LayoutHint::default()
        },
        source: None,
    };
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::List {
        kind: ListKind::Ordered { start: Some(7) },
        compact: false,
        items: vec![
            mant_ir::ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![paragraph("intro")],
            },
            mant_ir::ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: Some(mant_ir::EntryFacts {
                    id: "run".into(),
                    names: vec!["run".into()],
                    kind: mant_ir::EntryKind::Command,
                    case: mant_ir::NameCase::Sensitive,
                    value_domain: None,
                    name_bindings: vec![mant_ir::EntryNameBinding {
                        name: 0,
                        evidence: mant_ir::EntryNameEvidence::Declared,
                        occurrences: vec![mant_ir::EntryForm {
                            parts: vec![mant_ir::EntryContentSlice {
                                root: mant_ir::EntryInlineRoot::Block { index: 0 },
                                path: vec![0],
                                bytes: None,
                            }],
                        }],
                    }],
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: vec![mant_ir::EntryForm {
                        parts: vec![mant_ir::EntryContentSlice {
                            root: mant_ir::EntryInlineRoot::Block { index: 0 },
                            path: vec![0],
                            bytes: None,
                        }],
                    }],
                }),
                blocks: vec![paragraph("run")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    query
        .document
        .as_mut()
        .unwrap()
        .blocks
        .push(Block::Paragraph {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::Section { id: "run".into() },
                title: None,
                children: vec![Inline::Text {
                    value: "Jump to run".into(),
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        });
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    let annotated = DocumentView::new(&query);
    let Block::List { items, .. } = &mut query.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        unreachable!()
    };
    items[1].entry = None;
    let ordinary = DocumentView::new(&query);
    assert_entry_layout_is_unchanged(&annotated, &ordinary);
}

fn assert_entry_layout_is_unchanged(annotated: &DocumentView, ordinary: &DocumentView) {
    for width in [12, 40, 80] {
        let rendered = annotated.render(width);
        let unannotated = ordinary.render(width);
        // An annotation may add a validated name color, never change geometry.
        assert_eq!(rendered.text.to_string(), unannotated.text.to_string());
        assert_eq!(
            rendered.search("visible | body"),
            unannotated.search("visible | body")
        );
        assert_eq!(rendered.links, unannotated.links);
        let row = rendered.anchor_row("run").expect("semantic landing row");
        assert!(
            rendered
                .links
                .iter()
                .any(|link| link.target == LinkTarget::Section("run".into()))
        );
        let line = rendered.text.lines[row].to_string();
        assert!(
            line.trim_start().starts_with("8. run"),
            "landing row {row}: {line:?}"
        );
    }
}

#[test]
fn unsafe_tldr_more_information_remains_visible_but_inert() {
    let mut bundle = geometry_bundle();
    bundle.tldr.as_mut().expect("tldr").more_information = Some("file:///etc/passwd".to_owned());

    let view = DocumentView::new(&bundle);
    let link_line = view
        .lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("file:///etc/passwd"))
        })
        .expect("visible more-information line");

    assert!(link_line.links.is_empty());
}

#[test]
fn width_matrix_keeps_rows_anchors_links_and_search_inside_the_rendered_geometry() {
    let view = DocumentView::new(&geometry_bundle());

    for width in [1, 2, 3, 4, 7, 12, 24, 40, 80, 160] {
        let rendered = view.render(width);
        let width = usize::from(width);
        assert_eq!(rendered.text.lines.len(), rendered.row_count);
        assert_eq!(rendered.logical_rows.len(), view.lines.len() + 1);
        assert_eq!(rendered.logical_rows.last(), Some(&rendered.row_count));
        assert!(
            rendered
                .logical_rows
                .windows(2)
                .all(|rows| rows[0] <= rows[1])
        );
        for line in &rendered.text.lines {
            let visible = line.to_string();
            assert!(
                UnicodeWidthStr::width(visible.as_str()) <= width,
                "rendered row exceeds width {width}: {visible:?}"
            );
        }
        assert!(
            rendered
                .anchor_rows
                .values()
                .all(|row| *row <= rendered.row_count)
        );

        for link in &rendered.links {
            assert!(link.row < rendered.row_count);
            assert!(link.start_column < link.end_column);
            assert!(link.end_column <= width);
            assert_eq!(
                rendered.link_target_at(link.row, link.start_column),
                Some(&link.target)
            );
        }
        for row in 0..rendered.row_count {
            let anchor = rendered.viewport_anchor(row).expect("row anchor");
            assert_eq!(rendered.row_for_viewport_anchor(anchor), Some(row));
        }

        assert_eq!(rendered.search("多语言 documentation").len(), 1);
        assert!(!rendered.search("git status --short").is_empty());
        assert_eq!(rendered.search("alpha beta gamma").len(), 1);
    }
}

#[test]
fn authored_fragments_jump_to_their_canonical_target_rows() {
    let mut bundle = bundle();
    let section = &mut bundle.document.as_mut().expect("document").sections[0];
    section.fragment_aliases = vec!["Mixed.Section".into()];
    section.blocks.insert(
        0,
        Block::Paragraph {
            children: vec![Inline::anchor_with_aliases(
                "option",
                vec!["--option".into()],
            )],
            layout: LayoutHint::default(),
            source: None,
        },
    );

    let rendered = DocumentView::new(&bundle).render(40);
    assert_eq!(rendered.anchor_row("Mixed.Section"), Some(0));
    assert_eq!(
        rendered.anchor_row("--option"),
        rendered.anchor_row("option")
    );
}

#[test]
fn manual_references_are_typed_clickable_links_when_the_section_is_known() {
    let lines = styled_inline_lines(
        &[Inline::Link {
            target: mant_ir::LinkTarget::Manual {
                name: "printf".to_owned(),
                manual_section: Some("3".to_owned()),
            },
            title: None,
            children: vec![Inline::Text {
                value: "printf(3)".to_owned(),
            }],
        }],
        Style::default(),
        None,
    );

    assert_eq!(lines[0].spans[0].style.fg, Some(theme::LINK));
    assert!(
        lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
    assert_eq!(
        lines[0].links[0].target,
        LinkTarget::Document {
            address: DocumentAddress::Manual {
                name: "printf".to_owned(),
                manual_section: "3".to_owned(),
            },
            fragment: None,
        }
    );
}

#[test]
fn markdown_references_keep_the_current_source_and_fragment() {
    let current = DocumentAddress::Markdown {
        path: "about_Profiles".to_owned(),
        origin: mant_protocol::MarkdownOrigin::Source {
            name: "pwsh7".to_owned(),
        },
    };
    let lines = styled_inline_lines(
        &[Inline::Link {
            target: mant_ir::LinkTarget::Document {
                name: "Start-Process".to_owned(),
                fragment: Some("examples".to_owned()),
            },
            title: None,
            children: vec![Inline::Text {
                value: "Start-Process".to_owned(),
            }],
        }],
        Style::default(),
        Some(&current),
    );

    assert_eq!(
        lines[0].links[0].target,
        LinkTarget::Document {
            address: DocumentAddress::Markdown {
                path: "Start-Process".to_owned(),
                origin: mant_protocol::MarkdownOrigin::Source {
                    name: "pwsh7".to_owned(),
                },
            },
            fragment: Some("examples".to_owned()),
        }
    );
}

#[test]
fn typed_email_links_use_the_shared_mailto_serializer() {
    for (address, expected_uri) in [
        ("user%tag@example.test", "mailto:user%25tag@example.test"),
        ("a/b@example.test", "mailto:a%2Fb@example.test"),
        ("user=tag@example.test", "mailto:user%3Dtag@example.test"),
    ] {
        let lines = styled_inline_lines(
            &[Inline::Link {
                target: mant_ir::LinkTarget::Email {
                    address: address.to_owned(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "email".to_owned(),
                }],
            }],
            Style::default(),
            None,
        );
        assert_eq!(lines[0].spans[0].content, "email");
        assert_eq!(
            lines[0].links[0].target,
            LinkTarget::External(
                ExternalUri::parse(expected_uri).expect("serialized email URI remains valid")
            )
        );
    }

    let invalid = styled_inline_lines(
        &[Inline::Link {
            target: mant_ir::LinkTarget::Email {
                address: ".user@example.test".to_owned(),
            },
            title: None,
            children: vec![Inline::Text {
                value: "invalid email".to_owned(),
            }],
        }],
        Style::default(),
        None,
    );
    assert_eq!(invalid[0].spans[0].content, "invalid email");
    assert!(invalid[0].links.is_empty());
}

#[test]
fn inline_definitions_hang_the_description_and_expose_their_anchor() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            source: None,
            entry: Some(EntryFacts {
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                forms: Vec::new(),
                id: "help-option".to_owned().into(),
                kind: EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option,
                },
                case: NameCase::Sensitive,
                names: vec!["-h".to_owned()],
                value_domain: None,
            }),
            terms: vec![vec![Inline::Strong {
                children: vec![Inline::Text {
                    value: "-h".to_owned(),
                }],
            }]],
            description: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "Show detailed command help".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::DefinitionLayout {
                inline_term: true,
                spacing_before_lines: None,
                ..Default::default()
            },
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(18);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rendered.anchor_row("help-option"), Some(1));
    assert_eq!(rows[1], "   -h  Show");
    assert!(rows[2].starts_with("       detailed"));
}

#[test]
fn table_anchors_follow_their_cell_content_through_wrapping_and_stacking() {
    let paragraph = |children| Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |blocks| TableCell {
        blocks,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let table = |cells| Block::Table {
        rows: vec![TableRow { cells }],
        layout: LayoutHint::default(),
        source: None,
    };
    for nested in [false, true] {
        let mut bundle = bundle();
        let content = vec![
            paragraph(vec![Inline::Text {
                value: "preceding words take several wrapped rows".into(),
            }]),
            paragraph(vec![
                Inline::anchor_with_aliases("destination", vec!["Mixed.Target".into()]),
                Inline::Text {
                    value: "DESTINATION".into(),
                },
            ]),
        ];
        let content = if nested {
            vec![table(vec![cell(content)])]
        } else {
            content
        };
        bundle.document.as_mut().unwrap().sections[0].blocks = vec![table(vec![
            cell(vec![paragraph(vec![Inline::Text {
                value: "NEIGHBOUR".into(),
            }])]),
            cell(content),
            cell(vec![paragraph(vec![Inline::anchor("empty-target")])]),
        ])];
        for width in [8, 24, 48, 90] {
            let rendered = DocumentView::new(&bundle).render(width);
            let found = rendered.search("DESTINATION");
            assert_eq!(
                found.len(),
                1,
                "nested={nested}, width={width}: {:?}",
                rendered.text
            );
            assert_eq!(rendered.anchor_row("destination"), Some(found[0].row));
            assert_eq!(rendered.anchor_row("Mixed.Target"), Some(found[0].row));
            assert!(
                rendered
                    .anchor_row("empty-target")
                    .is_some_and(|row| row < rendered.row_count)
            );
            assert!(found[0].row > rendered.search("NEIGHBOUR")[0].row);
        }
    }
}

#[test]
fn section_reference_hit_regions_follow_wrapped_link_text() {
    let mut bundle = bundle();
    let document = bundle.document.as_mut().expect("document");
    document.sections[0].blocks = vec![Block::Paragraph {
        children: vec![
            Inline::Text {
                value: "Read ".to_owned(),
            },
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "details".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "the detailed section".to_owned(),
                }],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    document.sections[0].children.push(Section {
        id: "details".to_owned().into(),
        fragment_aliases: Vec::new(),
        title: "Details".to_owned(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    });

    let rendered = DocumentView::new(&bundle).render(12);
    let regions = rendered
        .links
        .iter()
        .filter(|link| link.target == LinkTarget::Section("details".to_owned()))
        .collect::<Vec<_>>();

    assert!(regions.len() >= 2, "reference should wrap across rows");
    for region in regions {
        assert_eq!(
            rendered.link_target_at(region.row, region.start_column),
            Some(&LinkTarget::Section("details".to_owned()))
        );
    }
}

#[test]
fn safe_tldr_more_information_stays_activatable() {
    let mut bundle = geometry_bundle();
    bundle.tldr.as_mut().expect("tldr").more_information =
        Some("https://example.test/tldr".to_owned());

    let view = DocumentView::new(&bundle);
    let link_line = view
        .lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("https://example.test/tldr"))
        })
        .expect("visible more-information line");

    assert_eq!(link_line.links.len(), 1);
    assert_eq!(
        link_line.links[0].target,
        LinkTarget::External(
            ExternalUri::parse("https://example.test/tldr").expect("valid external URI")
        )
    );
}

#[test]
fn records_section_rows_after_wrapping() {
    let view = DocumentView::new(&bundle());
    let rendered = view.render(12);

    assert_eq!(rendered.anchor_row("description"), Some(0));
    assert!(rendered.row_count >= 4);
    assert_eq!(view.navigation()[0].title, "Description");
}

#[test]
fn terminal_chrome_keeps_the_manual_section_out_of_the_sidebar_label() {
    let mut bundle = bundle();
    let document = bundle.document.as_mut().expect("document");
    document.meta.manual_section = Some("1".to_owned());
    document.blocks.push(Block::Paragraph {
        children: vec![Inline::Text {
            value: "overview".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    });

    let view = DocumentView::new(&bundle);

    assert_eq!(view.label(), "demo");
    assert_eq!(view.terminal_label(), "demo(1)");
    assert_eq!(view.top_level_count(), 1);
}

#[test]
fn section_spacing_is_not_coalesced_with_existing_blank_rows() {
    let mut bundle = bundle();
    let document = bundle.document.as_mut().expect("document");
    document.blocks = vec![Block::VerticalSpace {
        lines: 1,
        source: None,
    }];
    document.sections[0].spacing_before_lines = 2;

    let rendered = DocumentView::new(&bundle).render(80);

    assert_eq!(rendered.anchor_row("description"), Some(3));
}
