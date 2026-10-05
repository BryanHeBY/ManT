//! Link labels, typed identity, and original owners stay independent of row hints.
use super::super::{
    Block, Document, EntryFacts, EntryKind, Event, Inline, LayoutHint, ListItem, ListKind,
    MarkdownFragmentOptions, MarkdownNode, NameCase, Parser, ResolvedContent, Tag, TagEnd, manual,
    parse_content, render_addressable_markdown,
};
use super::fixtures::{layout, visible};
use mant_ir::InlineContentRef;

#[test]
fn uri_equal_labels_keep_style_and_title_independently_of_row_hints() {
    let uri = "https://example.org";
    for kind in 0..5 {
        for title in [None, Some("kept title".to_owned())] {
            let text = Inline::Text { value: uri.into() };
            let label = match kind {
                0 | 4 => text,
                1 => Inline::Strong {
                    children: vec![text],
                },
                2 => Inline::Emphasis {
                    children: vec![text],
                },
                3 => Inline::Code { value: uri.into() },
                _ => unreachable!(),
            };
            let children = if kind == 4 {
                vec![
                    Inline::Text {
                        value: "https://".into(),
                    },
                    Inline::Text {
                        value: "example.org".into(),
                    },
                ]
            } else {
                vec![label]
            };
            let nodes = [Inline::Link {
                target: mant_ir::LinkTarget::External { uri: uri.into() },
                title: title.clone(),
                children,
            }];
            let none = layout(&[]);
            let hints = layout(&[(0, 3)]);
            let outputs = [&none, &hints].map(|rows| {
                super::super::render_inline_content_fragment(
                    InlineContentRef {
                        content: &nodes,
                        layout: rows,
                    },
                    MarkdownFragmentOptions::default(),
                )
            });
            assert_eq!(outputs[0], outputs[1]);
            assert_eq!(visible(&outputs[0]), uri);
            let parsed = parse_content(&outputs[0], None).unwrap();
            let Block::Paragraph { children, .. } = &parsed.document.as_ref().unwrap().blocks[0]
            else {
                panic!("typed label readback")
            };
            let [
                Inline::Link {
                    target,
                    title: restored_title,
                    children: restored_label,
                },
            ] = children.as_slice()
            else {
                panic!("one typed link: {children:?}")
            };
            assert_eq!(target.to_uri().as_deref(), Some(uri));
            assert_eq!(restored_title, &title);
            assert_eq!(mant_ir::inline_plain_text(restored_label), uri);
            match kind {
                0 | 4 => assert!(matches!(restored_label.as_slice(), [Inline::Text { .. }])),
                1 => assert!(matches!(restored_label.as_slice(), [Inline::Strong { .. }])),
                2 => assert!(matches!(
                    restored_label.as_slice(),
                    [Inline::Emphasis { .. }]
                )),
                3 => assert!(matches!(restored_label.as_slice(), [Inline::Code { .. }])),
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn uri_equal_labels_keep_navigation_children_out_of_the_autolink_shortcut() {
    let uri = "https://example.org";
    let nodes = [Inline::Link {
        target: mant_ir::LinkTarget::External { uri: uri.into() },
        title: None,
        children: vec![
            Inline::Anchor {
                id: "label-destination".into(),
                fragment_aliases: ["label-destination", "Alias", "Alias", "Second.Alias"]
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                owner_source: None,
            },
            Inline::Text { value: uri.into() },
        ],
    }];
    for preserve_anchors in [false, true] {
        let none = layout(&[]);
        let hints = layout(&[(0, 3)]);
        let outputs = [&none, &hints].map(|rows| {
            super::super::render_inline_content_fragment(
                InlineContentRef {
                    content: &nodes,
                    layout: rows,
                },
                MarkdownFragmentOptions { preserve_anchors },
            )
        });
        let marker = if preserve_anchors {
            "<a id=\"label-destination\"></a><a id=\"Alias\"></a><a id=\"Second.Alias\"></a>"
        } else {
            ""
        };
        assert_eq!(outputs[0], outputs[1]);
        assert_eq!(
            outputs[0],
            format!("[{marker}https\\://example.org]({uri})")
        );
        assert_eq!(visible(&outputs[0]), uri);
        for id in ["label-destination", "Alias", "Second.Alias"] {
            assert_eq!(
                outputs[0].matches(&format!("<a id=\"{id}\"></a>")).count(),
                usize::from(preserve_anchors)
            );
        }
        let parsed = parse_content(&outputs[0], None).unwrap();
        let Block::Paragraph { children, .. } = &parsed.document.as_ref().unwrap().blocks[0] else {
            panic!("typed label readback")
        };
        let [
            Inline::Link {
                target,
                title,
                children,
            },
        ] = children.as_slice()
        else {
            panic!("one typed link: {children:?}")
        };
        assert_eq!(target.to_uri().as_deref(), Some(uri));
        assert_eq!(title, &None);
        // Attributed HTML retains its established literal-source reimport policy.
        assert_eq!(
            mant_ir::inline_plain_text(children),
            format!("{marker}{uri}")
        );
    }
}

fn reference_counts(document: &Document) -> (usize, usize) {
    let mut occurrences = 0;
    let mut targets = std::collections::BTreeSet::new();
    let report = mant_ir::scan_references(
        document,
        mant_ir::ReferenceScanLimits::default(),
        |reference| {
            occurrences += 1;
            targets.insert(reference.target.to_uri().unwrap());
            std::ops::ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    (occurrences, targets.len())
}

#[test]
fn anchors_before_a_positioned_link_label_do_not_create_empty_links() {
    for children in [
        vec![Inline::anchor("before"), Inline::Text { value: "A".into() }],
        vec![Inline::Strong {
            children: vec![
                Inline::anchor("before"),
                Inline::Emphasis {
                    children: vec![Inline::Text { value: "A".into() }],
                },
            ],
        }],
    ] {
        let nodes = [Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://example.org".into(),
            },
            title: None,
            children,
        }];
        let rows = layout(&[(0, 2)]);
        let markdown = super::super::render_inline_content_fragment(
            InlineContentRef {
                content: &nodes,
                layout: &rows,
            },
            MarkdownFragmentOptions {
                preserve_anchors: true,
            },
        );
        let mut in_link = false;
        let mut starts = 0;
        let mut label = String::new();
        for event in Parser::new(&markdown) {
            match event {
                Event::Start(Tag::Link { .. }) => {
                    in_link = true;
                    starts += 1;
                }
                Event::End(TagEnd::Link) => in_link = false,
                Event::Text(value) if in_link => label.push_str(&value),
                _ => {}
            }
        }
        assert_eq!(starts, 1, "{markdown}");
        assert_eq!(label, "A", "{markdown}");
        assert!(markdown.contains("<a id=\"before\"></a>"));
        let parsed = parse_content(&markdown, None).unwrap();
        assert_eq!(reference_counts(parsed.document.as_ref().unwrap()), (1, 1));
    }
}

#[test]
fn multiline_link_export_retains_one_occurrence_and_the_original_artifact_owner() {
    let mut document = manual(Vec::new());
    document.blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            entry: Some(EntryFacts {
                id: "layout-owner".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: Vec::new(),
                forms: Vec::new(),
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            }),
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.org".into(),
                    },
                    title: Some("kept title".into()),
                    children: vec![Inline::Strong {
                        children: vec![Inline::Text {
                            value: "A\nB".into(),
                        }],
                    }],
                }],
                inline_layout: layout(&[(0, 1), (1, 2)]),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::ListItemLayout::default(),
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    assert_eq!(reference_counts(&document), (1, 1));
    let decoded: Document =
        serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
    assert_eq!(reference_counts(&decoded), (1, 1));
    let original = mant_ir::content_entries(&document.blocks);
    let original_owner = original[0].owner();
    let query = ResolvedContent {
        address: None,
        label: "layout".into(),
        document: Some(document.clone()),
        tldr: None,
    };
    let artifact = render_addressable_markdown(&query);
    let entries: Vec<_> = artifact
        .nodes()
        .iter()
        .filter_map(|mapped| match mapped.node() {
            MarkdownNode::DocumentEntry { owner, .. } => Some((mapped.range(), *owner)),
            _ => None,
        })
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].1.facts().unwrap().id,
        original_owner.facts().unwrap().id
    );
    let query_owners = mant_ir::content_entries(&query.document.as_ref().unwrap().blocks);
    assert!(std::ptr::eq(
        entries[0].1.facts().unwrap(),
        query_owners[0].owner().facts().unwrap()
    ));
    let syntax = "[**A  \n  B**](https://example.org \"kept title\")";
    let start = artifact.text().find(syntax).unwrap();
    assert!(entries[0].0.start <= start && start + syntax.len() <= entries[0].0.end);
    assert!(!artifact.text().contains("&#160;"));
    let parsed = parse_content(artifact.text(), None).unwrap();
    let parsed_document = parsed.document.as_ref().unwrap();
    assert_eq!(reference_counts(parsed_document), (1, 1));
    mant_ir::scan_references(
        parsed_document,
        mant_ir::ReferenceScanLimits::default(),
        |reference| {
            assert!(
                matches!(reference.link, Inline::Link { title: Some(title), children, .. }
                    if title == "kept title" && mant_ir::inline_plain_text(children) == "A\nB")
            );
            std::ops::ControlFlow::Continue(())
        },
    );
}
