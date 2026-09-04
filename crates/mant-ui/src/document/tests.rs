use mant_ir::{
    DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Document, DocumentMeta,
    DocumentSource, LayoutHint, ListItem, SourceFormat, TableCell, TableRow, TldrDocument,
    TldrExample,
};
use unicode_width::UnicodeWidthStr;

use super::*;

fn bundle() -> ResolvedContent {
    ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "description".to_owned().into(),
                fragment_aliases: Vec::new(),
                title: "Description".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "a deliberately long sentence".to_owned(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            }],
        }),
        tldr: None,
    }
}

fn geometry_bundle() -> ResolvedContent {
    let mut bundle = bundle();
    bundle.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["A compact 多语言 reference.".to_owned()],
        more_information: None,
        examples: vec![TldrExample {
            description: "Inspect the working tree.".to_owned(),
            command: "git status --short".to_owned(),
            command_parts: Vec::new(),
        }],
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: String::new(),
        origin: TldrOrigin::Embedded,
    });
    let document = bundle.document.as_mut().expect("document");
    document.sections[0].blocks = vec![
        Block::Paragraph {
            children: vec![
                Inline::Text {
                    value: "Read 多语言 documentation in ".to_owned(),
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
                Inline::Text {
                    value: ".".to_owned(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Preformatted {
            children: vec![Inline::Text {
                value: "git status --short\n路径/with spaces".to_owned(),
            }],
            language: Some("sh".to_owned()),
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        blocks: vec![paragraph("alpha beta gamma")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
                        blocks: vec![paragraph("right hand value")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    document.sections[0].children.push(Section {
        id: "details".to_owned().into(),
        fragment_aliases: Vec::new(),
        title: "Details".to_owned(),
        spacing_before_lines: 0,
        blocks: vec![paragraph("Nothing is lost after resizing.")],
        children: Vec::new(),
        source: None,
    });
    bundle
}

fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }
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
fn records_section_rows_after_wrapping() {
    let view = DocumentView::new(&bundle());
    let rendered = view.render(12);

    assert_eq!(rendered.anchor_row("description"), Some(0));
    assert!(rendered.row_count >= 4);
    assert_eq!(view.navigation()[0].title, "Description");
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

#[test]
fn indented_continuation_without_spacing_follows_its_lead_row() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        paragraph("alternate object database"),
        Block::Paragraph {
            children: vec![Inline::Text {
                value: "Via the alternates mechanism, a repository can inherit objects.".to_owned(),
            }],
            layout: LayoutHint {
                indent_columns: 4,
                spacing_before_lines: 0,
            },
            source: None,
        },
    ];

    let rendered = DocumentView::new(&bundle).render(100);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let term = rows
        .iter()
        .position(|row| row.trim() == "alternate object database")
        .expect("visible glossary term");

    assert!(
        rows.get(term + 1)
            .is_some_and(|row| row.trim_start().starts_with("Via the alternates mechanism")),
        "an indented continuation with zero spacing must occupy the next row: {rows:?}",
    );
}

#[test]
fn a_tldr_only_result_explains_why_no_manual_body_follows() {
    let mut bundle = bundle();
    bundle.document = None;
    bundle.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["Quick reference".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    });

    let rendered = DocumentView::new(&bundle).render(80);
    let output = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(output.contains("No local man page was found"));
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
    assert_eq!(spans[2].style.fg, Some(theme::SUBTEXT));
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
fn unsafe_external_schemes_remain_visible_but_inert() {
    let lines = styled_inline_lines(
        &[Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "file:///etc/passwd".to_owned(),
            },
            title: None,
            children: vec![Inline::Text {
                value: "local file".to_owned(),
            }],
        }],
        Style::default(),
        None,
    );

    assert_eq!(lines[0].spans[0].content, "local file");
    assert!(lines[0].links.is_empty());
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
fn external_uri_schemes_are_matched_case_insensitively() {
    assert_eq!(
        ExternalUri::parse("HTTPS://example.test")
            .as_ref()
            .map(ExternalUri::as_str),
        Some("HTTPS://example.test")
    );
    assert_eq!(
        ExternalUri::parse("MAILTO:docs@example.test")
            .as_ref()
            .map(ExternalUri::as_str),
        Some("MAILTO:docs@example.test")
    );
}

#[test]
fn external_uri_activation_requires_a_host_or_mailbox() {
    for invalid in [
        "https:relative",
        "https:///missing-host",
        "https://",
        "https://example.test:",
        "https://[::1",
        "https://[::1]:invalid",
        "https://%ZZ@example.test/path",
        "https://example.test/%ZZ",
        "https://user]name@example.test/path",
        "https://example.test/path#one#two",
        "mailto:",
        "mailto:?subject=x",
        "mailto:a..b@example.test",
        "mailto:.a@example.test",
        "mailto:a.@example.test",
        "mailto:user%ZZ@example.test",
        "mailto:%2Euser@example.test",
        "mailto:user%2E%2Ename@example.test",
        "mailto:user%40evil@example.test",
        "mailto:user%2Csecond@example.test",
        "mailto:%2Euser@example.test?subject=x",
        "mailto:user%2E%2Ename@example.test?subject=x",
        "mailto:user%40evil@example.test?subject=x",
        "mailto:%2Euser@example.test#fragment",
        "https://example.test/white space",
    ] {
        assert!(
            !mant_ir::is_valid_external_uri(invalid),
            "IR accepted {invalid}"
        );
        assert!(ExternalUri::parse(invalid).is_none(), "accepted {invalid}");
    }
    for valid in [
        "https://example.test/path",
        "http://user@example.test:8080/path",
        "https://user%40name@example.test/path",
        "https://[::1]:8443/path",
        "https://[::1]:8443/path?q=x#part",
        "mailto:docs@example.test",
        "mailto:docs@example.test?subject=hello",
        "mailto:user%25tag@example.test",
        "mailto:a%2Fb@example.test",
        "mailto:docs@example.test,second@example.test",
    ] {
        assert!(mant_ir::is_valid_external_uri(valid), "IR rejected {valid}");
        assert!(ExternalUri::parse(valid).is_some(), "rejected {valid}");
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
fn wrapped_rows_preserve_their_indent() {
    let line = LogicalLine::plain(3, "abcdefgh", Style::default());
    let rows = wrap_line(&line, 7);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].to_string(), "   abcd");
    assert_eq!(rows[1].to_string(), "   efgh");
}

#[test]
fn wrapping_prefers_word_boundaries() {
    let line = LogicalLine::plain(2, "alpha beta", Style::default());
    let rows = wrap_line(&line, 8);

    assert_eq!(rows[0].to_string(), "  alpha");
    assert_eq!(rows[1].to_string(), "  beta");
}

#[test]
fn code_surfaces_fill_the_document_width_after_the_body_indent() {
    let line = LogicalLine::plain(3, "code", Style::default()).surface(LineSurface::Code);
    let rows = wrap_line(&line, 12);

    assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 12);
    assert_eq!(rows[0].spans[0].content, "   ");
    assert_eq!(rows[0].spans[0].style.bg, None);
    assert_eq!(rows[0].spans[1].style.bg, Some(theme::SURFACE));
    assert_eq!(rows[0].spans.last().expect("surface fill").content, "     ");
}

#[test]
fn preformatted_rows_share_one_full_width_surface() {
    let mut builder = DocumentBuilder::new("demo".to_owned(), None);
    builder.inline_lines_with_surface(
        &[
            Inline::Text {
                value: "short".to_owned(),
            },
            Inline::LineBreak,
            Inline::Text {
                value: "longer code".to_owned(),
            },
        ],
        3,
        Style::default().fg(theme::TEXT),
        LineSurface::Code,
    );

    let rows = builder
        .lines
        .iter()
        .flat_map(|line| wrap_line(line, 40))
        .collect::<Vec<_>>();

    assert_eq!(rows.len(), 2);
    assert!(rows[0].to_string().starts_with("   short"));
    assert!(rows[1].to_string().starts_with("   longer code"));
    assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 40);
    assert_eq!(UnicodeWidthStr::width(rows[1].to_string().as_str()), 40);
    assert_eq!(rows[0].spans[0].style.bg, None);
    assert_eq!(
        rows[0].spans.last().and_then(|span| span.style.bg),
        Some(theme::SURFACE)
    );
}

#[test]
fn preformatted_character_wrapping_preserves_significant_spaces() {
    let line = LogicalLine::plain(2, "ab  cd", Style::default())
        .surface(LineSurface::Code)
        .wrap_mode(WrapMode::Character);
    let rows = wrap_line(&line, 7);

    assert_eq!(&rows[0].to_string()[..7], "  ab  c");
    assert!(rows[1].to_string().starts_with("  d"));
}

#[test]
fn tldr_is_rendered_as_a_bordered_full_width_panel() {
    let mut bundle = bundle();
    bundle.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["Quick reference".to_owned()],
        more_information: None,
        examples: vec![TldrExample {
            description: "Run the command".to_owned(),
            command: "demo --output file".to_owned(),
            command_parts: vec![
                TldrCommandPart::Text {
                    value: "demo --output ".to_owned(),
                },
                TldrCommandPart::Placeholder {
                    value: "file".to_owned(),
                },
            ],
        }],
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    });

    let rendered = DocumentView::new(&bundle).render(32);

    assert!(rendered.text.lines[0].to_string().starts_with('┌'));
    assert_eq!(
        UnicodeWidthStr::width(rendered.text.lines[0].to_string().as_str()),
        32
    );
    assert!(rendered.text.lines.iter().any(|line| {
        line.to_string().contains("Quick reference")
            && line
                .spans
                .iter()
                .all(|span| span.style.bg == Some(theme::TLDR_SURFACE))
    }));
    assert!(
        rendered
            .text
            .lines
            .iter()
            .any(|line| line.to_string() == "─".repeat(32))
    );
    assert_eq!(
        rendered.text.lines[1].to_string(),
        format!("│{}│", " ".repeat(30))
    );
    let bottom = rendered
        .text
        .lines
        .iter()
        .position(|line| line.to_string().starts_with('└'))
        .expect("bottom border");
    assert_eq!(
        rendered.text.lines[bottom - 1].to_string(),
        format!("│{}│", " ".repeat(30))
    );
    let command = rendered
        .text
        .lines
        .iter()
        .find(|line| line.to_string().contains("demo --output file"))
        .expect("tldr command");
    assert!(
        command.spans.iter().any(|span| {
            span.content.contains("--output") && span.style.fg == Some(theme::PEACH)
        })
    );
    assert!(
        command
            .spans
            .iter()
            .any(|span| span.content == "file" && span.style.fg == Some(theme::TEXT))
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
fn manual_children_keep_the_same_gaps_as_the_established_layout() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").source.format = SourceFormat::Man;
    bundle.tldr = Some(TldrDocument {
        title: "demo".to_owned(),
        description: vec!["Quick reference".to_owned()],
        more_information: None,
        examples: Vec::new(),
        platform: "common".to_owned(),
        language: "en".to_owned(),
        source_path: "demo.md".to_owned(),
        origin: TldrOrigin::TldrPages,
    });

    let rendered = DocumentView::new(&bundle).render(32);
    let bottom = rendered
        .text
        .lines
        .iter()
        .position(|line| line.to_string().starts_with('└'))
        .expect("bottom border");

    assert!(rendered.text.lines[bottom + 1].to_string().is_empty());
    assert_eq!(rendered.text.lines[bottom + 2].to_string(), "─".repeat(32));
    assert_eq!(rendered.text.lines[bottom + 3].to_string(), "MANUAL");
    assert!(rendered.text.lines[bottom + 4].to_string().is_empty());
}

#[test]
fn bullet_lists_share_the_first_row_and_use_a_hanging_indent() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
        kind: ListKind::Bullet,
        start: None,
        compact: true,
        items: vec![ListItem {
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "alpha beta gamma".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(16);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1], "   • alpha beta");
    assert_eq!(rows[2], "     gamma");
}

#[test]
fn inline_definitions_hang_the_description_and_expose_their_anchor() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::DefinitionList {
        items: vec![DefinitionItem {
            identity: Some(DefinitionIdentity {
                id: "help-option".to_owned().into(),
                role: DefinitionRole::Option,
                case: DefinitionCase::Sensitive,
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
            inline_term: true,
            spacing_before_lines: None,
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
    assert_eq!(rows[1], "   -h Show");
    assert!(rows[2].starts_with("      detailed"));
}

#[test]
fn definition_lists_honour_compact_and_per_item_spacing() {
    let definition = |term: &str, description: &str, spacing_before_lines| DefinitionItem {
        identity: None,
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
        inline_term: false,
        spacing_before_lines,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::DefinitionList {
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

#[test]
fn adjacent_blocks_add_only_explicit_vertical_space() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        paragraph("before"),
        Block::Preformatted {
            children: vec![Inline::Text {
                value: "display".to_owned(),
            }],
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
        paragraph("after"),
        Block::VerticalSpace {
            lines: 1,
            source: None,
        },
        paragraph("spaced"),
    ];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let before = rows.iter().position(|row| row.contains("before")).unwrap();
    let display = rows.iter().position(|row| row.contains("display")).unwrap();
    let after = rows.iter().position(|row| row.contains("after")).unwrap();
    let spaced = rows.iter().position(|row| row.contains("spaced")).unwrap();

    assert_eq!(display, before + 1);
    assert_eq!(after, display + 1);
    assert_eq!(spaced, after + 2);
}

#[test]
fn table_cells_use_shared_content_driven_columns_and_independent_wrapping() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![
                TableCell {
                    blocks: vec![paragraph("alpha beta gamma")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
                TableCell {
                    blocks: vec![paragraph("right hand")],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                },
            ],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(24);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1].trim_end(), "   alpha       right", "{rows:#?}");
    assert_eq!(rows[2].trim_end(), "   beta gamma  hand", "{rows:#?}");
    assert_eq!(UnicodeWidthStr::width(rows[1].as_str()), 24);
    let left_match = rendered.search("alpha beta gamma");
    assert_eq!(left_match.len(), 1);
    assert_eq!(left_match[0].row, 1);
    assert_eq!(left_match[0].additional_fragments[0].row, 2);
    assert_eq!(rendered.search("right hand").len(), 1);
}

#[test]
fn short_table_keys_do_not_claim_half_of_a_wide_viewport() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |value: &str| TableCell {
        blocks: vec![paragraph(value)],
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                cells: vec![cell("1"), cell("Executable programs and shell commands")],
            },
            TableRow {
                cells: vec![cell("8"), cell("System administration commands")],
            },
        ],
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

    assert_eq!(rows[1], "   1  Executable programs and shell commands");
    assert_eq!(rows[2].trim_end(), "   8  System administration commands");
    assert!(UnicodeWidthStr::width(rows[2].as_str()) < 50);
}

#[test]
fn narrow_tables_stack_cells_instead_of_dropping_content() {
    let cells = vec![
        LogicalTableCell::new(vec![LogicalLine::plain(0, "a", Style::default())], None),
        LogicalTableCell::new(vec![LogicalLine::plain(0, "b", Style::default())], None),
    ];
    let layout = Arc::new(LogicalTableLayout::for_rows(std::slice::from_ref(&cells)));
    let line = LogicalLine::table(0, cells, layout);

    let rows = wrap_line(&line, 1);
    assert_eq!(
        rows.iter().map(ToString::to_string).collect::<String>(),
        "ab"
    );
}

#[test]
fn ordered_list_markers_saturate_instead_of_overflowing() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
        kind: ListKind::Ordered,
        start: Some(u64::MAX),
        compact: true,
        items: vec![
            ListItem {
                blocks: vec![paragraph("first")],
            },
            ListItem {
                blocks: vec![paragraph("second")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];

    let output = DocumentView::new(&bundle).render(80).text.to_string();
    assert_eq!(output.matches("18446744073709551615. ").count(), 2);
}

#[test]
fn thematic_breaks_fill_the_remaining_content_width() {
    let rows = wrap_line(&LogicalLine::rule(3), 12);

    assert_eq!(rows[0].to_string(), "   ─────────");
}

#[test]
fn rendered_search_finds_literal_options_and_decorates_every_match() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Use --acls, then repeat --acls.".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(42);

    let matches = rendered.search("--ACLS");
    let highlighted = rendered.highlighted_text(&matches, Some(1));

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].row, 1);
    assert!(
        highlighted.lines[1]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_MATCH))
    );
    assert!(
        highlighted.lines[1]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
    );

    let viewport = rendered.viewport_text(1, 1, &matches, Some(1), None);
    assert_eq!(viewport.lines.len(), 1);
    assert!(
        viewport.lines[0]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_MATCH))
    );
    assert!(
        viewport.lines[0]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
    );
}

#[test]
fn case_folding_maps_expanding_unicode_back_to_the_source_character() {
    let rendered = RenderedDocument {
        text: Text::from(Line::from("İstanbul")),
        row_count: 1,
        surfaces: vec![LineSurface::Normal],
        logical_rows: vec![0, 1],
        anchor_rows: HashMap::new(),
        links: Vec::new(),
        search_records: vec![RenderedSearchRecord {
            text: "İstanbul".to_owned(),
            cells: "İstanbul"
                .char_indices()
                .scan(0, |column, (source_start, character)| {
                    let start_column = *column;
                    *column += character.width().unwrap_or(0);
                    Some(RenderedSearchSourceCell {
                        source_start,
                        source_end: source_start + character.len_utf8(),
                        fragment: RenderedSearchFragment {
                            row: 0,
                            start_column,
                            end_column: *column,
                        },
                    })
                })
                .collect(),
        }],
    };

    assert_eq!(
        rendered.search("i"),
        vec![RenderedSearchMatch {
            row: 0,
            start_column: 0,
            end_column: 1,
            additional_fragments: Vec::new(),
        }]
    );
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
fn search_matches_one_logical_phrase_across_soft_wrapping() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "alpha searchable phrase omega".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(15);

    let matches = rendered.search("searchable phrase");
    let highlighted = rendered.highlighted_text(&matches, Some(0));

    assert_eq!(matches.len(), 1);
    assert!(!matches[0].additional_fragments.is_empty());
    let highlighted_rows = highlighted
        .lines
        .iter()
        .filter(|line| {
            line.spans
                .iter()
                .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
        })
        .count();
    assert_eq!(highlighted_rows, 2);
}

#[test]
fn search_preserves_a_space_wrapped_exactly_after_the_row_boundary() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Relative inset end".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(11);

    let matches = rendered.search("Relative inset end");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].additional_fragments.len(), 2);
}

#[test]
fn character_wrapped_code_remains_contiguous_for_search() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Preformatted {
        children: vec![Inline::Text {
            value: "abcdefghijklmnop".to_owned(),
        }],
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&bundle);
    for width in [1, 2, 4, 10] {
        let rendered = view.render(width);
        let matches = rendered.search("ghijkl");

        assert_eq!(matches.len(), 1, "lost code at width {width}");
        assert!(
            !matches[0].additional_fragments.is_empty(),
            "code did not wrap at width {width}"
        );
    }
}

#[test]
fn forced_word_splitting_does_not_insert_a_search_space() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "supercalifragilistic".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(10);

    assert_eq!(rendered.search("fragilistic").len(), 1);
}
