//! Heading labels are derived; body rendering retains the original inline facts.
use super::*;

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
        content: vec![
            link(
                "Catalog",
                mant_ir::LinkTarget::Document {
                    name: "index".into(),
                    fragment: Some("Mixed.Target".into()),
                },
            ),
            Inline::LineBreak,
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
            Inline::LineBreak,
            Inline::anchor("heading-tail"),
            Inline::Text {
                value: "tail".into(),
            },
        ],
        source: None,
    };
    let view = DocumentView::new(&query);
    assert_eq!(view.navigation()[0].title, "Heading and mail\ntail");
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        assert_eq!(rendered.search("Heading").len(), 1);
        assert_eq!(rendered.search("mail").len(), 1);
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
