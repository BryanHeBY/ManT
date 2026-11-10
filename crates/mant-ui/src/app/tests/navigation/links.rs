//! Link activation, destination safety, and source-relative navigation.

use super::super::*;

#[test]
fn clicking_unsafe_tldr_more_information_does_not_reach_the_host() {
    let mut bundle = tldr_bundle();
    bundle.tldr.as_mut().expect("tldr").more_information = Some("file:///etc/passwd".to_owned());
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("file:///etc/passwd")
        .into_iter()
        .next()
        .expect("visible tldr URL");

    click_document_cell(&mut app, region.start_column, region.row);

    assert!(app.take_external_request().is_none());
}

#[test]
fn clicking_safe_tldr_more_information_reaches_the_host() {
    let mut bundle = tldr_bundle();
    bundle.tldr.as_mut().expect("tldr").more_information =
        Some("https://example.test/tldr".to_owned());
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("https://example.test/tldr")
        .into_iter()
        .next()
        .expect("visible tldr URL");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.take_external_request()
            .as_ref()
            .map(crate::ExternalUri::as_str),
        Some("https://example.test/tldr")
    );
}

#[test]
fn repeated_tldr_link_clicks_use_the_parsed_destination_without_prose_punctuation() {
    for (authored, expected) in [
        (
            "<https://git-scm.com/docs/git-add>.",
            "https://git-scm.com/docs/git-add",
        ),
        (
            "<https://example.test/path.>.",
            "https://example.test/path.",
        ),
        (
            "[Official docs](https://example.test/a_(b)).",
            "https://example.test/a_(b)",
        ),
    ] {
        let page = mant_codec::parse_tldr_page(
            &format!("# demo\n> More information: {authored}\n"),
            mant_codec::TldrPageLocation {
                platform: "common".into(),
                language: "en".into(),
                source_path: "demo.md".into(),
            },
        )
        .unwrap();
        for width in [72, 110] {
            let mut bundle = tldr_bundle();
            bundle.tldr = Some(page.clone());
            let mut terminal = Terminal::new(TestBackend::new(width, 18)).unwrap();
            let mut app = App::new(&bundle);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let region =
                &app.session.rendered_cache[&app.geometry.content.width].search(expected)[0];
            let (column, row) = (region.start_column, region.row);
            for _ in 0..3 {
                click_document_cell(&mut app, column, row);
                assert_eq!(
                    app.take_external_request()
                        .as_ref()
                        .map(crate::ExternalUri::as_str),
                    Some(expected)
                );
                assert_eq!(
                    app.session
                        .current_bundle
                        .tldr
                        .as_ref()
                        .unwrap()
                        .more_information
                        .as_deref(),
                    Some(expected)
                );
            }
        }
    }
}

#[test]
fn clicking_a_wrapped_section_reference_opens_its_target() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0]
        .blocks
        .insert(
            0,
            AstBlock::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![
                    Inline::Text {
                        value: "Continue with ".to_owned(),
                    },
                    Inline::Link {
                        target: mant_ir::LinkTarget::Section {
                            id: "details".into(),
                        },
                        title: None,
                        children: vec![Inline::Text {
                            value: "the nested details section".to_owned(),
                        }],
                    },
                ],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    app.expanded.clear();
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("nested")
        .into_iter()
        .next()
        .expect("visible reference text");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    assert!(app.expanded.contains("options"));
    assert_eq!(
        app.session.content_scroll,
        app.session.rendered_cache[&width]
            .anchor_row("details")
            .expect("details anchor")
    );
}

#[test]
fn clicking_a_parsed_markdown_fragment_jumps_and_participates_in_history() {
    let bundle = mant_loader::load_markdown_text(
        "# Demo\n\nContinue with [the detailed section](#details).\n\n## Details\n\nDone.\n",
        Some("demo.md".to_owned()),
    )
    .expect("parse Markdown fragment link");
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("detailed")
        .into_iter()
        .next()
        .expect("visible parsed fragment link");

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    assert_eq!(app.navigation.history_lengths().0, 1);
    app.navigate_history(true);
    assert_ne!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
    app.navigate_history(false);
    assert_eq!(
        app.session.document.navigation()[app.selected].id,
        "details"
    );
}

#[test]
fn clicking_a_relative_markdown_link_preserves_its_source_and_fragment() {
    let mut bundle = mant_loader::load_markdown_text(
        "# Index\n\nContinue with [Build](../commands/build.md#usage).\n",
        Some("/documents/guides/index.md".to_owned()),
    )
    .expect("parse relative Markdown link");
    bundle.address = Some(DocumentAddress::Markdown {
        path: "guides/index".to_owned(),
        origin: MarkdownOrigin::Source {
            name: "team".to_owned(),
        },
    });
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("Build")
        .into_iter()
        .next()
        .expect("visible Markdown link");

    click_document_cell(&mut app, region.start_column, region.row);

    let request = app.take_open_request().expect("Markdown request");
    assert_eq!(
        request.address(),
        &DocumentAddress::Markdown {
            path: "commands/build".to_owned(),
            origin: MarkdownOrigin::Source {
                name: "team".to_owned(),
            },
        }
    );
    assert_eq!(request.target.id(), Some("usage"));
}

#[test]
fn clicking_an_external_link_returns_the_uri_to_the_host() {
    let mut bundle = navigation_bundle();
    bundle.document.as_mut().expect("manual").sections[0]
        .blocks
        .insert(
            0,
            AstBlock::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.test/docs".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "external docs".to_owned(),
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            },
        );
    let backend = TestBackend::new(72, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut app = App::new(&bundle);
    terminal.draw(|frame| app.draw(frame)).expect("draw app");
    let width = app.geometry.content.width;
    let region = app.session.rendered_cache[&width]
        .search("external docs")
        .into_iter()
        .next()
        .expect("visible external link");

    let column = app.geometry.content.x + u16::try_from(region.start_column).expect("link column");
    let row = app.geometry.content.y + u16::try_from(region.row).expect("link row");
    for (kind, pointer_column) in [
        (MouseEventKind::Down(MouseButton::Left), column),
        (MouseEventKind::Drag(MouseButton::Left), column + 1),
        (MouseEventKind::Up(MouseButton::Left), column),
    ] {
        app.handle_mouse(MouseEvent {
            kind,
            column: pointer_column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }
    assert!(app.take_external_request().is_none());

    click_document_cell(&mut app, region.start_column, region.row);

    assert_eq!(
        app.take_external_request()
            .as_ref()
            .map(crate::ExternalUri::as_str),
        Some("https://example.test/docs")
    );
}

#[test]
fn clicking_encoded_invalid_mailto_links_never_reaches_the_host() {
    for (index, uri) in [
        "mailto:%2Euser@example.test?subject=x",
        "mailto:user%2E%2Ename@example.test?subject=x",
        "mailto:user%40evil@example.test?subject=x",
        "mailto:%2Euser@example.test#fragment",
    ]
    .into_iter()
    .enumerate()
    {
        let label = format!("invalid mail target {index}");
        let mut bundle = navigation_bundle();
        bundle.document.as_mut().expect("manual").sections[0]
            .blocks
            .insert(
                0,
                AstBlock::Paragraph {
                    inline_layout: mant_ir::InlineLayout::default(),
                    children: vec![Inline::Link {
                        target: mant_ir::LinkTarget::External {
                            uri: uri.to_owned(),
                        },
                        title: None,
                        children: vec![Inline::Text {
                            value: label.clone(),
                        }],
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                },
            );
        let backend = TestBackend::new(72, 18);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = App::new(&bundle);
        terminal.draw(|frame| app.draw(frame)).expect("draw app");
        let width = app.geometry.content.width;
        let region = app.session.rendered_cache[&width]
            .search(&label)
            .into_iter()
            .next()
            .expect("invalid mail label remains visible");

        click_document_cell(&mut app, region.start_column, region.row);

        assert!(
            app.take_external_request().is_none(),
            "unsafe mailto URI reached host activation: {uri}"
        );
    }
}
