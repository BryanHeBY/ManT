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

#[cfg(feature = "native-link-layout")]
#[test]
fn native_snapshot_routes_mouse_only_to_authored_same_label_occurrences() {
    // The second native Lk has the same target and identical visible label,
    // but was independently authored and wholly owns its visible glyphs.
    let source = ".Dd November 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd use\n.Sh DESCRIPTION\n.No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n.br\n.No OTHER\n.Lk https://z.test ZYU\n.Sh NEXT\nDone.\n";
    let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
    assert_eq!(snapshot.receipts().len(), 1);
    let mut app = App::from_native_snapshot(&snapshot, DocumentCatalog::default());
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let width = app.geometry.content.width;
    let groups = app.session.rendered_cache[&width].search("ZYU");
    let [mixed, authored] = groups.as_slice() else {
        panic!("two equal visible labels");
    };
    for (region, expected) in [(mixed, [true, false, true]), (authored, [true, true, true])] {
        for (offset, owned) in expected.into_iter().enumerate() {
            click_document_cell(&mut app, region.start_column + offset, region.row);
            assert_eq!(
                app.take_external_request()
                    .as_ref()
                    .map(crate::ExternalUri::as_str),
                owned.then_some("https://z.test"),
            );
        }
    }
}

#[cfg(feature = "native-link-layout")]
#[test]
fn unrepresentable_native_members_leave_initial_and_follow_on_reader_open() {
    // The combining mark is authored by the Lk, but belongs to old Y's
    // physical grapheme. A safe reader cannot offer either half as a target.
    let source = ".Dd November 28, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd use\n.Sh DESCRIPTION\n.No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z \\[u0301]U\nEND\n.br\n.Lk https://z.test AGAIN\n.br\n.Xr printf 3\n.Sh NEXT\nDone.\n";
    let initial = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
    let follow_on = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
    assert_eq!(
        initial.receipts().len(),
        1,
        "native proof includes the unsupported split"
    );
    let mut app = App::from_native_snapshot(&initial, DocumentCatalog::default());
    let mut terminal = Terminal::new(TestBackend::new(80, 25)).unwrap();
    let region = |app: &App, text: &str| {
        let width = app.geometry.content.width;
        app.session.rendered_cache[&width]
            .search(text)
            .into_iter()
            .next()
            .unwrap()
    };
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let mixed = region(&app, "ZY\u{301}U");
    for offset in 0..3 {
        click_document_cell(&mut app, mixed.start_column + offset, mixed.row);
        assert!(app.take_external_request().is_none());
    }
    let good = region(&app, "AGAIN");
    click_document_cell(&mut app, good.start_column, good.row);
    assert_eq!(
        app.take_external_request()
            .as_ref()
            .map(crate::ExternalUri::as_str),
        Some("https://z.test")
    );

    let next = region(&app, "printf(3)");
    click_document_cell(&mut app, next.start_column, next.row);
    let mut called = false;
    assert!(
        app.service_pending_with_native_links(&mut crate::ReaderServices::default(), |_| {
            called = true;
            Ok(follow_on.clone())
        })
    );
    assert!(called);
    assert!(std::sync::Arc::ptr_eq(
        &app.session.current_bundle,
        &follow_on.content_arc()
    ));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let mixed = region(&app, "ZY\u{301}U");
    click_document_cell(&mut app, mixed.start_column + 1, mixed.row);
    assert!(app.take_external_request().is_none());
}
#[cfg(feature = "native-link-layout")]
const AU_MEMBER_HEADER: &str = ".Dd November 28, 2026\n.Dt MEMBER 1\n.Os\n.Sh NAME\n.Nm member\n.Nd source membership\n.Sh DESCRIPTION\n";

#[cfg(feature = "native-link-layout")]
const AU_MIXED_LINK: &str = ".No HEAD\nY\n.No \"\\h'|1n'R\"\n.Lk https://z.test Z U\nEND\n";

#[cfg(feature = "native-link-layout")]
fn au_native_hits(app: &mut App, text: &str, index: usize, expected: [bool; 3]) {
    native_member_hits(app, text, index, expected, "https://z.test");
}

#[cfg(feature = "native-link-layout")]
fn native_member_hits(app: &mut App, text: &str, index: usize, expected: [bool; 3], uri: &str) {
    let width = app.geometry.content.width;
    let group = &app.session.rendered_cache[&width].search(text)[index];
    assert_eq!(
        group.end_column - group.start_column,
        3,
        "{text} must fit on a row"
    );
    assert!(group.row >= app.session.content_scroll);
    assert!(group.row - app.session.content_scroll < usize::from(app.geometry.content.height));
    for (offset, linked) in expected.into_iter().enumerate() {
        click_document_cell(app, group.start_column + offset, group.row);
        assert_eq!(
            app.take_external_request()
                .as_ref()
                .map(crate::ExternalUri::as_str),
            linked.then_some(uri),
            "width {width}, label {text}[{index}] offset {offset}"
        );
    }
}

#[cfg(feature = "native-link-layout")]
#[test]
fn native_link_au_receipt_cap_keeps_a_verified_prefix_and_restricts_tail() {
    // Frozen AU count-255/256/257: CVS termp_lk_pre paints ZYU to ASCII/UTF-8;
    // mdoc_lk_pre emits only the Z U author link in HTML before the real .br.
    for count in [255, 256, 257] {
        let source = format!(
            "{AU_MEMBER_HEADER}{}.br\n.Xr printf 3\n.Sh ENDTEST\nDone.\n",
            std::iter::repeat_n(AU_MIXED_LINK, count)
                .collect::<Vec<_>>()
                .join(".br\n")
        );
        let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
        let pair = snapshot.content_arc();
        let layout = snapshot.layout_for(&pair).unwrap();
        assert_eq!(layout.receipts().len(), count.min(256));
        assert_eq!(layout.restrictions().len(), count.saturating_sub(256));
        assert!(!layout.restrict_unidentified());
        let legacy = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        assert_eq!(pair.document, legacy.document, "legacy IR remains intact");
        for width in [60, 80, 140] {
            let mut app = App::from_native_snapshot(&snapshot, DocumentCatalog::default());
            let mut terminal = Terminal::new(TestBackend::new(width, 400)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            au_native_hits(&mut app, "ZYU", 0, [true, false, true]);
            let content = app.geometry.content.width;
            app.session.content_scroll =
                app.session.rendered_cache[&content].search("ZYU")[count - 1].row;
            terminal.draw(|frame| app.draw(frame)).unwrap();
            au_native_hits(
                &mut app,
                "ZYU",
                count - 1,
                if count <= 256 {
                    [true, false, true]
                } else {
                    [false; 3]
                },
            );
        }
    }
}

#[cfg(feature = "native-link-layout")]
#[test]
fn native_link_au_label_cap_does_not_expand_hits() {
    // Physical paint always starts ZYU followed by x; HTML from the same
    // pinned termp_lk_pre/mdoc_lk_pre pair owns only Z and U+x… in all cases.
    for length in [4095, 4096, 4097] {
        let part = AU_MIXED_LINK.replace(" Z U", &format!(" Z U{}", "x".repeat(length - 3)));
        let source = format!("{AU_MEMBER_HEADER}{part}.br\n.Xr printf 3\n.Sh ENDTEST\nDone.\n");
        let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
        let pair = snapshot.content_arc();
        let layout = snapshot.layout_for(&pair).unwrap();
        let fits = length <= 4096;
        assert_eq!(layout.receipts().len(), usize::from(fits));
        assert_eq!(layout.restrictions().len(), usize::from(!fits));
        assert!(!layout.restrict_unidentified());
        assert_eq!(
            pair.document,
            mant_loader::load_roff_bytes(source.as_bytes())
                .unwrap()
                .document
        );
        for width in [60, 80, 140] {
            let mut app = App::from_native_snapshot(&snapshot, DocumentCatalog::default());
            let mut terminal = Terminal::new(TestBackend::new(width, 25)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            au_native_hits(&mut app, "ZYU", 0, [fits, false, fits]);
        }
    }
}

#[cfg(feature = "native-link-layout")]
#[test]
fn native_link_au_independent_prior_link_does_not_take_the_new_owners_receipt() {
    // Pinned CVS termp_lk_pre final ink puts K/L/M between Z and U after the
    // offset move. HTML author links are independent: Q→p.test, Z U→z.test.
    let header = ".Dd November 27, 2026\n.Dt GLYPHS 1\n.Os\n.Sh NAME\n.Nm glyphs\n.Nd small roff inputs\n.Sh DESCRIPTION\n";
    for (position, old) in [("1", "K"), ("2", "L"), ("3", "M")] {
        let source = format!(
            "{header}.No HEAD\n.No KLM\n.Lk https://p.test Q\n.No \"\\h'|{position}n'R\"\n.Lk https://z.test Z U\nEND\n.Sh ENDTEST\nDone.\n"
        );
        let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
        let pair = snapshot.content_arc();
        let layout = snapshot.layout_for(&pair).unwrap();
        assert_eq!(layout.receipts().len(), 1, "{old}: {layout:?}");
        assert_eq!(layout.receipts()[0].members, [0..1, 2..3]);
        assert_eq!(layout.restrictions(), []);
        assert_eq!(
            pair.document,
            mant_loader::load_roff_bytes(source.as_bytes())
                .unwrap()
                .document
        );
        for width in [60, 80, 140] {
            let mut app = App::from_native_snapshot(&snapshot, DocumentCatalog::default());
            let mut terminal = Terminal::new(TestBackend::new(width, 25)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            au_native_hits(&mut app, &format!("Z{old}U"), 0, [true, false, true]);
        }
    }
}

#[cfg(feature = "native-link-layout")]
#[test]
fn native_link_av_mailto_uses_the_same_owned_cells_as_https() {
    // Frozen AV oracle: termp_lk_pre paints ZYU (old Y between two owned
    // characters), while mdoc_lk_pre writes only Z U inside the HTML link.
    for (authored, opened) in [
        ("mailto:a@z.test", "mailto:a@z.test"),
        ("MAILTO:a@z.test", "mailto:a@z.test"),
        ("mailto:a@z.test?subject=hi", "mailto:a@z.test?subject=hi"),
    ] {
        let part = AU_MIXED_LINK.replace("https://z.test", authored);
        let source = format!(
            "{AU_MEMBER_HEADER}{part}.br\n.Mt plain@y.test\n.br\n.Xr printf 3\n.Sh ENDTEST\nDone.\n"
        );
        let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
        let [receipt] = snapshot.receipts() else {
            panic!("{authored}: expected one link membership");
        };
        assert_eq!(receipt.members, [0..1, 2..3]);
        for width in [60, 80, 140] {
            let mut app = App::from_native_snapshot(&snapshot, DocumentCatalog::default());
            let mut terminal = Terminal::new(TestBackend::new(width, 25)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            native_member_hits(&mut app, "ZYU", 0, [true, false, true], opened);
        }
    }
}

#[cfg(feature = "native-link-layout")]
#[test]
fn native_link_av_unlocated_mixed_target_restricts_only_its_target_type() {
    // Pinned termp_lk_pre paints tail X Y V on one physical line;
    // mdoc_lk_pre confirms the link authored X and V, never old ink Y.
    for (count, tail_uri, blocked) in [
        (1280, "mailto:a@z.test", [false, false]),
        (1281, "mailto:a@z.test", [false, true]),
        (1281, "https://z.test", [true, false]),
    ] {
        let middle = format!("{AU_MIXED_LINK}.br\n").repeat(count - 1);
        let tail = AU_MIXED_LINK
            .replace("https://z.test", tail_uri)
            .replace(" Z U", " X V");
        let source = format!(
            "{AU_MEMBER_HEADER}.Mt plain@y.test\n.br\n.Lk https://plain.test DOCUMENT\n.br\n{middle}{tail}.br\n.Xr printf 3\n.Sh ENDTEST\nDone.\n"
        );
        let snapshot = mant_loader::load_roff_bytes_with_link_layout(source.as_bytes()).unwrap();
        let pair = snapshot.content_arc();
        let layout = snapshot.layout_for(&pair).unwrap();
        assert_eq!(layout.receipts().len(), 256);
        assert_eq!(layout.restrictions().len(), 1024);
        assert_eq!(
            [
                layout.restrict_unidentified(),
                layout.restrict_unidentified_email()
            ],
            blocked,
            "tail {count}: {tail_uri}",
        );
        for width in [60, 80, 140] {
            let mut app = App::from_native_snapshot(&snapshot, DocumentCatalog::default());
            let mut terminal = Terminal::new(TestBackend::new(width, 25)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let content_width = app.geometry.content.width;
            for (text, uri, restricted) in [
                ("DOCUMENT", "https://plain.test", blocked[0]),
                ("plain@y.test", "mailto:plain@y.test", blocked[1]),
            ] {
                let region = &app.session.rendered_cache[&content_width].search(text)[0];
                click_document_cell(&mut app, region.start_column, region.row);
                assert_eq!(
                    app.take_external_request()
                        .as_ref()
                        .map(crate::ExternalUri::as_str),
                    (!restricted).then_some(uri),
                    "independent {text}: tail {count} {tail_uri} width {width}",
                );
            }
            au_native_hits(&mut app, "ZYU", 0, [true, false, true]);
            app.session.content_scroll =
                app.session.rendered_cache[&content_width].search("XYV")[0].row;
            terminal.draw(|frame| app.draw(frame)).unwrap();
            native_member_hits(&mut app, "XYV", 0, [false; 3], tail_uri);
        }
    }
}
