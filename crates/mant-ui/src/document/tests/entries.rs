//! Existing regressions grouped by entries behavior; expected values remain independent.
use super::*;

#[test]
fn nested_code_links_preserve_emphasis_and_restore_the_following_style() {
    let nodes = [
        Inline::Strong {
            children: vec![Inline::Code {
                value: "bold-code".into(),
            }],
        },
        Inline::Text {
            value: " ordinary ".into(),
        },
        Inline::Emphasis {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                title: None,
                children: vec![Inline::Code {
                    value: "linked-code".into(),
                }],
            }],
        },
        Inline::Text {
            value: " after".into(),
        },
    ];
    let lines = styled_inline_lines(&nodes, Style::default().fg(theme::TEXT), None);
    let spans = &lines[0].spans;
    assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert!(
        spans[2]
            .style
            .add_modifier
            .contains(Modifier::ITALIC | Modifier::UNDERLINED)
    );
    for index in [1, 3] {
        assert_eq!(spans[index].style, Style::default().fg(theme::TEXT));
    }
    assert_eq!(lines[0].links[0].start_column, 19);
    assert_eq!(lines[0].links[0].end_column, 30);
}

#[test]
fn all_entry_roles_color_only_bound_source_text_not_markers_or_body_mentions() {
    for (role, name, color) in [
        ("option", "--help", theme::GREEN),
        ("marker", "--", theme::GREEN),
        ("operand", "-", theme::GREEN),
        ("command", "Launch", theme::PEACH),
        ("environment-variable", "$Env:HOME", theme::MAUVE),
        ("configuration-key", "ServerAliveInterval", theme::YELLOW),
        ("variable", "$local_name", theme::PINK),
        ("value", "automatic", theme::BLUE),
        ("term", "alpha", theme::TEXT),
    ] {
        let source = format!(
            "# Probe\n\n## Entries\n\n<!-- mant:entries role={role} case=sensitive -->\n- `{name}`: Body mentions {name} without a binding.\n\n## Prose\n\nalphabet and -xylophone remain ordinary.\n"
        );
        let content = mant_loader::load_markdown_text(&source, None).unwrap();
        assert!(
            content.document.as_ref().unwrap().diagnostics.is_empty(),
            "{role}"
        );
        let view = DocumentView::new(&content);
        let rendered = view.render(120);
        let line = rendered
            .text
            .lines
            .iter()
            .find(|line| line.to_string().contains("Body mentions"))
            .unwrap();
        assert!(
            line.spans
                .iter()
                .any(|span| span.content.starts_with(name) && span.style.fg == Some(color)),
            "{role}: {line:?}"
        );
        assert!(
            line.spans
                .iter()
                .filter(|span| span.content.contains("Body mentions"))
                .all(|span| span.style.fg == Some(theme::TEXT))
        );
        assert!(
            line.spans
                .iter()
                .filter(|span| span.content.contains('•'))
                .all(|span| span.style.fg == Some(theme::HEADING))
        );
        // No semantic-color mutation is written into the original IR or cached view.
        for width in [12, 40, 120, 12] {
            let current = view.render(width);
            assert!(!current.search(name).is_empty());
            assert_eq!(current.text, view.render(width).text);
        }
    }
}

#[test]
fn bound_link_name_keeps_type_and_modifiers_through_code_surface_and_wrapping() {
    let mut content = bundle();
    content.address = Some(DocumentAddress::Markdown {
        path: "probe".into(),
        origin: mant_ir::MarkdownOrigin::Documents,
    });
    let source = "# Probe\n\n## Options\n\n<!-- mant:entries role=option case=sensitive -->\n- [`--help`](other.md): description\n";
    let parsed = mant_loader::load_markdown_text(source, None).unwrap();
    assert!(
        parsed.document.as_ref().unwrap().diagnostics.is_empty(),
        "{:?}",
        parsed.document.as_ref().unwrap().diagnostics
    );
    let mut blocks = parsed.document.unwrap().sections.remove(0).blocks;
    let Block::List { items, .. } = &mut blocks[0] else {
        panic!("list")
    };
    let item = &mut items[0];
    let Block::Paragraph {
        children,
        layout,
        source,
    } = item.blocks.remove(0)
    else {
        panic!("paragraph")
    };
    // The preformatted root uses the same validated references and coordinates.
    item.blocks.push(Block::Preformatted {
        language: None,
        children,
        layout,
        source,
    });
    content.document.as_mut().unwrap().sections[0].blocks = blocks;
    assert!(mant_ir::validate_document(content.document.as_ref().unwrap()).is_empty());
    let view = DocumentView::new(&content);
    for width in [12, 40, 120] {
        let rendered = view.render(width);
        let hit = rendered.search("--help");
        assert_eq!(hit.len(), 1);
        let bound = rendered
            .text
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .filter(|span| span.style.fg == Some(theme::GREEN))
            .collect::<Vec<_>>();
        assert_eq!(
            bound
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "--help"
        );
        assert!(
            bound
                .iter()
                .all(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        assert!(
            rendered
                .links
                .iter()
                .any(|link| matches!(&link.target, LinkTarget::Document { .. }))
        );
        assert_link_selection(&view, &rendered, &hit, width);
    }
}

#[test]
fn inline_styles_preserve_the_renderer_neutral_ir_semantics() {
    let lines = styled_inline_lines(
        &[
            Inline::Strong {
                children: vec![Inline::Text {
                    value: "strong".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: "emphasis".to_owned(),
                }],
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Code {
                value: "--option".to_owned(),
            },
            Inline::Text {
                value: " ".to_owned(),
            },
            Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test".to_owned(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: "link".to_owned(),
                }],
            },
        ],
        Style::default().fg(theme::TEXT),
        None,
    );
    let spans = &lines[0].spans;

    assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(spans[0].style.fg, Some(theme::STRONG));
    assert!(spans[2].style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(spans[2].style.fg, Some(theme::TEXT));
    assert_eq!(spans[4].style.fg, Some(theme::HEADING));
    assert_eq!(spans[6].style.fg, Some(theme::BLUE));
    assert!(spans[6].style.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(
        lines[0].links[0].target,
        LinkTarget::External(
            ExternalUri::parse("https://example.test").expect("valid external URI")
        )
    );
}

#[test]
fn tldr_commands_use_terminal_soft_wrapping_instead_of_prose_reflow() {
    let mut bundle = bundle();
    bundle.document = None;
    bundle.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["Quick reference".to_owned()],
        more_information: None,
        examples: vec![TldrExample {
            description: "Run a long command".to_owned(),
            command: "abc defghij".to_owned(),
            command_parts: vec![TldrCommandPart::Text {
                value: "abc defghij".to_owned(),
            }],
        }],
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "demo.md".to_owned(),
        origin: TldrOrigin::Embedded,
    });

    let rendered = DocumentView::new(&bundle).render(12);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert!(rows.iter().any(|row| row.contains("abc de")));
    assert!(rows.iter().any(|row| row.contains("fghij")));
    assert_eq!(rendered.search("abc defghij").len(), 1);
}

#[test]
fn definition_lists_honour_compact_and_per_item_spacing() {
    let definition = |term: &str, description: &str, spacing_before_lines| DefinitionItem {
        source: None,
        entry: None,
        terms: vec![vec![Inline::Text {
            value: term.to_owned(),
        }]],
        description: vec![Block::Paragraph {
            children: vec![Inline::Text {
                value: description.to_owned(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines,
            ..Default::default()
        },
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![
            definition("-E", "Run the preprocessor.", None),
            definition("-S", "Run the compiler.", Some(2)),
        ],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let first_description = rows
        .iter()
        .position(|row| row.contains("Run the preprocessor."))
        .expect("first description");
    let second_term = rows
        .iter()
        .position(|row| row.contains("-S"))
        .expect("second term");

    assert_eq!(second_term, first_description + 3);
    assert!(rows[first_description + 1].trim().is_empty());
    assert!(rows[first_description + 2].trim().is_empty());
}

fn assert_link_selection(
    view: &DocumentView,
    rendered: &crate::document::RenderedDocument,
    hit: &[crate::document::RenderedSearchMatch],
    width: u16,
) {
    let first = &hit[0];
    let (last_row, last_end) = first
        .additional_fragments
        .last()
        .map_or((first.row, first.end_column), |f| (f.row, f.end_column));
    let selection = crate::document::RenderedSelection {
        anchor: crate::document::TextPosition {
            row: first.row,
            column: first.start_column,
        },
        focus: crate::document::TextPosition {
            row: last_row,
            column: last_end - 1,
        },
    };
    let highlighted = rendered.viewport_text(0, rendered.row_count, hit, Some(0), Some(selection));
    let selected: Vec<_> = highlighted
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .filter(|s| s.style.bg == Some(theme::SELECTED))
        .collect();
    assert_eq!(
        selected
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>(),
        "--help"
    );
    assert!(
        selected
            .iter()
            .all(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
    );
    assert_eq!(
        rendered.selected_text(selection).replace('\n', ""),
        "--help"
    );
    assert_eq!(view.render(width).text, rendered.text);
}
