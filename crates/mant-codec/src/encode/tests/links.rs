use super::*;

#[test]
fn logical_link_serialization_preserves_literal_percent_and_unicode_components() {
    #[derive(Default)]
    struct Links(Vec<mant_ir::LinkTarget>);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link { target, .. } = inline {
                self.0.push(target.clone());
            }
            walk_inline(self, inline);
        }
    }
    let source = "## Mixed {#Mixed%2ETarget}\n[local](#Mixed%252ETarget) [doc](space%20name.md#Mixed%2ETarget) [literal](literal%2520.md#literal%252E) [unicode](%E6%97%A5%E6%9C%AC.md)\n";
    let query = parse_content(source, None).unwrap();
    let options = MarkdownOptions {
        preserve_anchors: true,
        ..MarkdownOptions::default()
    };
    let markdown = render_markdown_with_options(&query, options);
    assert!(
        markdown.contains("literal%2520.md#literal%252E"),
        "{markdown}"
    );
    let reparsed = parse_content(&markdown, None).unwrap();
    assert!(
        !reparsed
            .document
            .as_ref()
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.dangling-section-link"))
    );
    let mut original_links = Links::default();
    original_links.visit_document(query.document.as_ref().unwrap());
    let mut reparsed_links = Links::default();
    reparsed_links.visit_document(reparsed.document.as_ref().unwrap());
    assert_eq!(reparsed_links.0, original_links.0);
}

fn email_addresses(document: &Document) -> Vec<String> {
    #[derive(Default)]
    struct EmailCollector(Vec<String>);

    impl<'ir> Visit<'ir> for EmailCollector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Link {
                target: mant_ir::LinkTarget::Email { address },
                ..
            } = inline
            {
                self.0.push(address.clone());
            }
            walk_inline(self, inline);
        }
    }

    let mut collector = EmailCollector::default();
    collector.visit_document(document);
    collector.0
}

#[test]
fn serializes_typed_email_links_through_the_shared_mailto_boundary() {
    let query = ResolvedContent {
        address: None,
        label: "mail".to_owned(),
        document: Some(manual(vec![section(
            "CONTACT",
            vec![paragraph(vec![
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: "user%tag@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "percent".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: "a/b@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "slash".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: "user=tag@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "equals".to_owned(),
                    }],
                },
                Inline::Text {
                    value: " ".to_owned(),
                },
                Inline::Link {
                    target: mant_ir::LinkTarget::Email {
                        address: ".invalid@example.test".to_owned(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "invalid remains visible".to_owned(),
                    }],
                },
            ])],
            Vec::new(),
        )])),
        tldr: None,
    };

    let markdown = render_markdown(&query);
    assert!(markdown.contains("[percent](mailto:user%25tag@example.test)"));
    assert!(markdown.contains("[slash](mailto:a%2Fb@example.test)"));
    assert!(markdown.contains("[equals](mailto:user%3Dtag@example.test)"));
    assert!(markdown.contains("invalid remains visible"));
    assert!(!markdown.contains("mailto:.invalid@example.test"));
}

#[test]
fn typed_email_links_round_trip_through_markdown_without_uri_diagnostics() {
    let source = "[percent](mailto:user%25tag@example.test) \
                  [slash](mailto:a%2Fb@example.test) \
                  [equals](mailto:user%3Dtag@example.test)\n";
    let parsed =
        parse_content(source, Some("mail.md".to_owned())).expect("parse typed email links");
    let expected = vec![
        "user%tag@example.test".to_owned(),
        "a/b@example.test".to_owned(),
        "user=tag@example.test".to_owned(),
    ];
    assert_eq!(
        email_addresses(parsed.document.as_ref().expect("parsed document")),
        expected
    );

    let markdown = render_markdown(&parsed);
    let reparsed = parse_content(&markdown, Some("round-trip.md".to_owned()))
        .expect("reparse rendered Markdown");
    let document = reparsed.document.as_ref().expect("reparsed document");
    assert_eq!(email_addresses(document), expected);
    assert!(
        document.diagnostics.iter().all(|diagnostic| !matches!(
            diagnostic.code.as_deref(),
            Some("ir.invalid-external-uri" | "ir.invalid-email-address")
        )),
        "round trip introduced a URI diagnostic: {:?}",
        document.diagnostics
    );
}
