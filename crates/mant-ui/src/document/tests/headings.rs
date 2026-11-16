//! Heading labels are derived; body rendering retains the original inline facts.
use super::*;

#[test]
fn virtual_overview_rows_use_one_verified_destination() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.heading = Some(mant_ir::Heading {
        content: vec![
            Inline::anchor(mant_ir::DOCUMENT_ROOT_ID),
            Inline::Text {
                value: "ROOTHEADING".into(),
            },
        ],
        ..mant_ir::Heading::default()
    });
    for conflict in [false, true] {
        if conflict {
            content.document.as_mut().unwrap().sections[0].id = mant_ir::DOCUMENT_ROOT_ID.into();
        }
        let view = DocumentView::new(&content);
        assert_eq!(
            view.validate_fragment(mant_ir::DOCUMENT_ROOT_ID).is_err(),
            conflict
        );
        for width in [12, 40, 78] {
            let rendered = view.render(width);
            assert_eq!(
                rendered.anchor_row(mant_ir::DOCUMENT_ROOT_ID).is_none(),
                conflict
            );
            if let Some(row) = rendered.anchor_row(mant_ir::DOCUMENT_ROOT_ID) {
                assert_eq!(rendered.text.lines[row].to_string().trim(), "ROOTHEADING");
            }
            assert!(rendered.text.to_string().contains("ROOTHEADING"));
            assert!(rendered.text.to_string().contains("Description"));
        }
    }
}

#[test]
fn typed_relative_document_links_share_grammar_before_activation() {
    let mut content = bundle();
    content.address = Some(mant_ir::DocumentAddress::Markdown {
        path: "dir/source".into(),
        origin: mant_ir::MarkdownOrigin::Documents,
    });
    for (name, fragment, allowed) in [
        ("a?b", None, false),
        ("page", Some(""), false),
        ("scope:page", None, false),
        ("../peer", Some("一 二"), true),
    ] {
        let target = mant_ir::LinkTarget::Document {
            name: name.into(),
            fragment: fragment.map(str::to_owned),
        };
        let mut block = paragraph("LINKLABEL");
        let Block::Paragraph { children, .. } = &mut block else {
            unreachable!("test helper returns a paragraph");
        };
        let original = std::mem::take(children);
        children.push(Inline::Link {
            target: target.clone(),
            title: None,
            children: original,
        });
        content.document.as_mut().unwrap().sections[0].blocks = vec![block];
        let view = DocumentView::new(&content);
        assert_eq!(view.activation_target(&target).is_some(), allowed);
        for width in [12, 40, 78] {
            let rendered = view.render(width);
            assert!(rendered.text.to_string().contains("LINKLABEL"));
            assert_eq!(!rendered.links.is_empty(), allowed);
        }
    }
}

fn link(value: &str, target: mant_ir::LinkTarget) -> Inline {
    Inline::Link {
        target,
        title: None,
        children: vec![Inline::Emphasis {
            children: vec![Inline::Code {
                value: value.into(),
            }],
        }],
    }
}

#[test]
fn heading_only_root_retains_links_styles_anchors_and_hard_lines_once() {
    let mut query = bundle();
    query.address = Some(DocumentAddress::Markdown {
        path: "guides/catalog".into(),
        origin: mant_ir::MarkdownOrigin::Documents,
    });
    query.label = "query-label-is-not-body".into();
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.meta.title = Some("metadata-is-not-body".into());
    document.heading = Some(mant_ir::Heading {
        inline_layout: mant_ir::InlineLayout::default(),
        content: vec![
            link(
                "Catalog",
                mant_ir::LinkTarget::Document {
                    name: "index".into(),
                    fragment: Some("Mixed.Target".into()),
                },
            ),
            Inline::line_break(),
            Inline::anchor_with_aliases("second-line", vec!["Second.Line".into()]),
            Inline::Text {
                value: "Second row".into(),
            },
        ],
        source: None,
    });
    let original = document.clone();
    let view = DocumentView::new(&query);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Root)
            .count(),
        1
    );
    assert_eq!(view.navigation()[0].kind, NavKind::Root);
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        let text = rendered.text.to_string();
        assert_eq!(text.matches("Catalog").count(), 1);
        assert!(!text.contains("query-label-is-not-body"));
        assert!(!text.contains("metadata-is-not-body"));
        assert_eq!(rendered.search("Catalog").len(), 1);
        assert_eq!(rendered.anchor_row("second-line"), Some(1));
        assert_eq!(rendered.anchor_row("Second.Line"), Some(1));
        assert_eq!(rendered.links.len(), 1);
        assert_eq!(
            rendered.links[0].target,
            LinkTarget::Document {
                address: DocumentAddress::Markdown {
                    path: "guides/index".into(),
                    origin: mant_ir::MarkdownOrigin::Documents,
                },
                fragment: Some("Mixed.Target".into()),
            }
        );
        let style = rendered.text.lines[0].spans[0].style;
        assert_eq!(style.fg, Some(theme::LINK));
        assert!(
            style
                .add_modifier
                .contains(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED)
        );
    }
    assert_eq!(query.document.as_ref().unwrap(), &original);
}

#[test]
fn section_labels_do_not_replace_linked_body_heading_content() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].heading = mant_ir::Heading {
        inline_layout: mant_ir::InlineLayout::default(),
        content: vec![
            link(
                "Heading",
                mant_ir::LinkTarget::External {
                    uri: "https://example.com/".into(),
                },
            ),
            Inline::Text {
                value: " and ".into(),
            },
            link(
                "mail",
                mant_ir::LinkTarget::Email {
                    address: "help@example.com".into(),
                },
            ),
            Inline::line_break(),
            Inline::anchor("heading-tail"),
            Inline::Text {
                value: "tail".into(),
            },
        ],
        source: None,
    };
    let view = DocumentView::new(&query);
    assert_eq!(view.navigation()[0].title, "Heading and mail tail");
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        assert_eq!(rendered.search("Heading").len(), 1);
        assert_eq!(rendered.search("mail").len(), 1);
        assert!(
            rendered.search("tail")[0].row > rendered.search("mail")[0].row,
            "the body heading must retain its authoritative structural break"
        );
        assert_eq!(rendered.anchor_row("description"), Some(0));
        assert_eq!(
            rendered.anchor_row("heading-tail"),
            Some(rendered.search("tail")[0].row)
        );
        assert!(rendered.links.iter().any(|region| matches!(&region.target, LinkTarget::External(uri) if uri.as_str() == "https://example.com/")));
        assert!(rendered.links.iter().any(|region| matches!(&region.target, LinkTarget::External(uri) if uri.as_str() == "mailto:help@example.com")));
    }
}

#[test]
fn parsed_h1_local_link_reveals_the_canonical_root_after_tldr() {
    for body in ["", "\n\n[Return](#catalog)\n"] {
        for tldr in [false, true] {
            let mut query =
                mant_loader::load_markdown_text(&format!("# [Catalog](#catalog){body}\n"), None)
                    .unwrap();
            if tldr {
                query.tldr = geometry_bundle().tldr;
            }
            let document = query.document.as_ref().unwrap();
            assert!(
                document.diagnostics.is_empty(),
                "{:?}",
                document.diagnostics
            );
            let view = DocumentView::new(&query);
            let root = view
                .navigation()
                .iter()
                .find(|node| node.kind == NavKind::Root)
                .unwrap();
            assert_eq!(root.id, mant_ir::DOCUMENT_ROOT_ID);
            assert_eq!(root.target_id, mant_ir::DOCUMENT_ROOT_ID);
            for width in [12, 40, 80] {
                let rendered = view.render(width);
                let title_row = rendered.search("Catalog")[0].row;
                assert_eq!(
                    rendered.anchor_row(mant_ir::DOCUMENT_ROOT_ID),
                    Some(title_row)
                );
                assert_eq!(rendered.anchor_row("catalog"), Some(title_row));
                assert_eq!(
                    rendered
                        .links
                        .iter()
                        .filter(|link| link.target
                            == LinkTarget::Section(mant_ir::DOCUMENT_ROOT_ID.into()))
                        .count(),
                    1 + usize::from(!body.is_empty())
                );
                if tldr {
                    assert!(title_row > rendered.anchor_row(TLDR_ID).unwrap());
                }
            }
        }
    }
}
