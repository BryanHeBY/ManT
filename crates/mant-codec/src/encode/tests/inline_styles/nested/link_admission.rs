//! Literal Text and typed-target admission are independent from syntax framing.

use super::*;
use mant_ir::LinkTarget;

#[derive(Clone)]
struct TargetCase {
    target: LinkTarget,
    /// Independent accepted URI, or an explicitly invalid typed target.
    uri: Option<&'static str>,
}

fn targets() -> Vec<TargetCase> {
    let mut cases = [
        ("https://a.test/path", Some("https://a.test/path")),
        ("https://a.test/a%20b", Some("https://a.test/a%20b")),
        ("https://a.test/a%1Bb", Some("https://a.test/a%1Bb")),
        ("https://a.test/&amp;", Some("https://a.test/&amp;")),
        ("https://a.test/a b", None),
        ("https://a.test/\x0c", None),
        ("https://a.test/\x1b", None),
        ("https://a.test/\u{85}", None),
        ("https://a.test/\r", None),
        ("https://a.test/\n", None),
        ("https://a.test/\t", None),
        ("https://a.test/中", None),
        ("https://a.test/\\a", None),
        ("https://a.test/%", None),
        ("relative.html", None),
        // A URI valid as a different typed target is not an External alias.
        ("man:printf(3)", None),
    ]
    .map(|(uri, accepted)| TargetCase {
        target: LinkTarget::External { uri: uri.into() },
        uri: accepted,
    })
    .to_vec();
    cases.extend([
        TargetCase {
            target: LinkTarget::Document {
                name: "space name".into(),
                fragment: Some("part.target".into()),
            },
            uri: Some("space%20name.md#part.target"),
        },
        TargetCase {
            target: LinkTarget::Manual {
                name: "printf".into(),
                manual_section: Some("3".into()),
            },
            uri: Some("man:printf(3)"),
        },
        TargetCase {
            target: LinkTarget::Section { id: "body".into() },
            uri: Some("#body"),
        },
        TargetCase {
            target: LinkTarget::Email {
                address: "user%tag@example.test".into(),
            },
            uri: Some("mailto:user%25tag@example.test"),
        },
    ]);
    cases
}

fn labels() -> Vec<Vec<Inline>> {
    vec![
        vec![text("label")],
        vec![text("<https://other.test/x>")],
        vec![text("<<https://other.test/x>>")],
        vec![styled(true, vec![text("<https://other.test/x>")])],
        vec![styled(false, vec![text("<https://other.test/x>")])],
        vec![Inline::Code {
            value: "<https://other.test/x>".into(),
        }],
        vec![text("prefix<https://other.test/x>suffix")],
        vec![text("中e\u{301}")],
        vec![text(" label ")],
        vec![text("![literal](https://other.test/x)")],
    ]
}

fn wrapped(case: &TargetCase, anchors: bool, heading: bool) -> bool {
    case.uri.is_some()
        && match &case.target {
            LinkTarget::Manual { .. } => heading,
            LinkTarget::Section { .. } => anchors,
            _ => true,
        }
}

fn source_and_expected(
    case: &TargetCase,
    label: &[Inline],
    retained: bool,
) -> (Vec<Inline>, Vec<Inline>) {
    let link = Inline::Link {
        target: case.target.clone(),
        title: Some("original title\\&amp;".into()),
        children: label.to_vec(),
    };
    let original = vec![text("!"), link.clone(), text("END")];
    let expected = if retained {
        original.clone()
    } else {
        std::iter::once(text("!"))
            .chain(label.iter().cloned())
            .chain([text("END")])
            .collect()
    };
    (original, expected)
}

fn checked_fragment_rounds(original: &[Inline], expected: &[Inline], anchors: bool, heading: bool) {
    let expected = meaning(expected);
    let mut current = original.to_vec();
    for round in 1..=2 {
        let options = MarkdownFragmentOptions {
            preserve_anchors: anchors,
        };
        let markdown = if heading {
            let mut title = mant_ir::Heading::from("unused");
            title.content = current;
            render_heading_fragment(2, &title, options)
        } else {
            render_inline_fragment(&current, options)
        };
        assert!(
            !Parser::new(&markdown).any(|event| matches!(event, Event::Start(Tag::Image { .. })))
        );
        let parsed = crate::parse_markdown(&markdown, None).unwrap().document;
        current = if heading {
            parsed.sections[0].heading.content.clone()
        } else {
            let [Block::Paragraph { children, .. }] = parsed.blocks.as_slice() else {
                panic!("one public fragment root: {markdown:?}");
            };
            children.clone()
        };
        assert_allowed_styles(
            &expected,
            &meaning(&current),
            &format!("{round}: {markdown}"),
        );
    }
}

#[test]
fn target_label_policy_matrix_uses_original_ir_for_public_fragments_and_heading_rows() {
    let mut configurations = 0;
    for case in targets() {
        assert_eq!(case.target.to_uri().as_deref(), case.uri);
        for label in labels() {
            for anchors in [false, true] {
                for heading in [false, true] {
                    let (original, expected) =
                        source_and_expected(&case, &label, wrapped(&case, anchors, heading));
                    let frozen = original.clone();
                    checked_fragment_rounds(&original, &expected, anchors, heading);
                    assert_eq!(original, frozen);
                    configurations += 1;
                }
            }
        }
    }
    assert_eq!(configurations, 800);
}

#[test]
fn literal_text_never_acquires_link_identity_in_public_or_document_exports() {
    for source in [
        r"\<https://a.test/x\>",
        r"\<\<https://a.test/x\>\>",
        r"!\<https://a.test/x\>",
        r"**\<https://a.test/x\>**",
        r"*\<https://a.test/x\>*",
        r"\<https://a.test/a&amp;b\>",
    ] {
        let original = parsed_children(source);
        assert_eq!(meaning(&original).links, []);
        checked_fragment_rounds(&original, &original, false, false);
        for context in [
            "paragraph",
            "heading",
            "list",
            "definition-head",
            "definition-body",
        ] {
            finalization::final_rounds(&original, context, MarkdownOptions::default());
        }
    }
    let original = parsed_children("<https://a.test/x>");
    assert_eq!(
        meaning(&original).links.len(),
        1,
        "source autolinks remain semantic links"
    );
    checked_fragment_rounds(&original, &original, false, false);
}

#[test]
fn admitted_or_declined_targets_share_string_artifact_and_owner_serialization() {
    let mut configurations = 0;
    for case in targets() {
        for label in labels().into_iter().take(6) {
            for context in [
                "paragraph",
                "heading",
                "list",
                "definition-head",
                "definition-body",
            ] {
                // Heading-local links deliberately enable destination export.
                let retained = wrapped(&case, context == "heading", context == "heading");
                let (original, expected) = source_and_expected(&case, &label, retained);
                let expected = meaning(&expected);
                let mut query = finalization::enclosed(original, context);
                if matches!(case.target, LinkTarget::Section { .. }) {
                    // A complete document supplies the actual destination;
                    // detached fragments intentionally borrow outside context.
                    query
                        .document
                        .as_mut()
                        .unwrap()
                        .sections
                        .push(section("body", vec![], vec![]));
                }
                for _ in 0..2 {
                    let frozen = query.clone();
                    let artifact = super::super::super::render_markdown_artifact(
                        &query,
                        MarkdownOptions::default(),
                        true,
                    );
                    let plain = render_markdown_with_options(&query, MarkdownOptions::default());
                    assert_eq!(artifact.text(), plain);
                    for node in artifact.nodes() {
                        assert!(
                            artifact.text().get(node.range()).is_some(),
                            "final UTF-8 owner range"
                        );
                    }
                    let markdown = artifact.into_text();
                    assert_eq!(query, frozen, "encoding must not mutate original IR");
                    query = parse_content(&markdown, None).unwrap();
                    let actual = finalization::restored_nodes(&query, context);
                    assert_allowed_styles(
                        &expected,
                        &meaning(&actual),
                        &format!("{context}: {markdown}"),
                    );
                }
                configurations += 1;
            }
        }
    }
    assert_eq!(configurations, 600);
}

#[test]
fn invalid_entity_targets_keep_first_diagnostics_then_export_only_literal_labels() {
    for entity in [
        "&#32;", "&#12;", "&#27;", "&#133;", "&#13;", "&#10;", "&#9;",
    ] {
        let source = format!(r"[\<https://other.test\>](https://a.test/{entity})");
        let parsed = crate::parse_markdown(&source, None).unwrap().document;
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.code.as_deref() == Some("ir.invalid-external-uri"))
        );
        let [Block::Paragraph { children, .. }] = parsed.blocks.as_slice() else {
            panic!("source paragraph")
        };
        let frozen = parsed.clone();
        checked_fragment_rounds(children, &[text("<https://other.test>")], false, false);
        assert_eq!(parsed, frozen);
    }
}

#[test]
fn uri_metadata_is_explicit_and_checked_while_ordinary_tldr_text_stays_literal() {
    for (value, readable, target) in [
        (
            "https://a.test/path",
            "https://a.test/path",
            Some("https://a.test/path"),
        ),
        (
            "https://a.test/a%20b",
            "https://a.test/a%20b",
            Some("https://a.test/a%20b"),
        ),
        (
            "https://a.test/a%1Bb",
            "https://a.test/a%1Bb",
            Some("https://a.test/a%1Bb"),
        ),
        ("https://a.test/a b", "https://a.test/a b", None),
        ("https://a.test/\x1bx", "https://a.test/ x", None),
        ("https://a.test/\u{85}x", "https://a.test/  x", None),
        ("https://a.test/中", "https://a.test/中", None),
        ("<https://a.test/path>", "<https://a.test/path>", None),
    ] {
        let mut expected = vec![styled(true, vec![text("More information:")]), text(" ")];
        expected.push(target.map_or_else(
            || text(readable),
            |uri| Inline::Link {
                target: LinkTarget::External { uri: uri.into() },
                title: None,
                children: vec![text(readable)],
            },
        ));
        let markdown = crate::encode::render_more_information(value);
        let original = parsed_children(&markdown);
        assert_allowed_styles(&meaning(&expected), &meaning(&original), &markdown);
        checked_fragment_rounds(&original, &expected, false, false);
    }
    let mut page = crate::parse_tldr_page(
        "# probe\n> Description.\n> More information: <https://metadata.test>.\n\n- Example:\n\n`probe`\n",
        crate::TldrPageLocation { platform: "common".into(), language: "en".into(), source_path: "probe.md".into() },
    ).unwrap();
    // The constrained tldr source parser has its own older presentation
    // flattening policy. Here the already-admitted IR is the independent
    // witness for ordinary descriptions and explicit URI metadata callers.
    page.description = vec!["<https://literal.test>.".into()];
    page.examples[0].description = "Keep <https://example.test> literal:".into();
    let encoded = crate::encode::render_tldr(&page).join("\n\n");
    let links = Parser::new(&encoded)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.into_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(links, ["https://metadata.test"]);
    assert!(encoded.contains(r"\<https\://literal.test\>"));
    assert!(encoded.contains(r"\<https\://example.test\>"));
}
