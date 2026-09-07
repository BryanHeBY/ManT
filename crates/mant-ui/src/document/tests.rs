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
fn horizontal_spans_align_the_following_cell_with_later_rows() {
    let mut bundle = bundle();
    let cell = |text: &str, column_span| TableCell {
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Text { value: text.into() }],
            layout: LayoutHint::default(),
            source: None,
        }],
        column_span,
        row_span: 1,
        alignment: None,
    };
    bundle.document.as_mut().unwrap().sections[0].blocks = vec![Block::Table {
        rows: vec![
            TableRow {
                cells: vec![cell("TOPSPAN", 2), cell("RIGHT", 1)],
            },
            TableRow {
                cells: vec![cell("LEFT", 1), cell("MIDDLE", 1), cell("END", 1)],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    for width in [40, 80] {
        let rendered = DocumentView::new(&bundle).render(width);
        let rows = rendered
            .text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let top = rows.iter().find(|row| row.contains("RIGHT")).unwrap();
        let bottom = rows.iter().find(|row| row.contains("END")).unwrap();
        assert_eq!(top.find("RIGHT"), bottom.find("END"), "{rows:?}");
    }
    let rendered = DocumentView::new(&bundle).render(8);
    assert_eq!(rendered.search("TOPSPAN").len(), 1);
    assert_eq!(rendered.search("RIGHT").len(), 1);
}

#[test]
fn thematic_breaks_fill_the_remaining_content_width() {
    let rows = wrap_line(&LogicalLine::rule(3), 12);

    assert_eq!(rows[0].to_string(), "   ─────────");
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

mod entries;
mod layout;
mod navigation;
mod search;
mod tables;
