//! Headings are authoritative inline content, not labels reconstructed as text.
use mant_engine::{
    parse_markdown, query_markdown_text, query_roff_bytes, render_markdown, render_query_text,
    search_query,
};
use mant_ir::{
    Document, Inline, LinkTarget,
    visit::{self, Visit},
};

fn links(document: &Document) -> Vec<LinkTarget> {
    #[derive(Default)]
    struct Links(Vec<LinkTarget>);
    impl<'a> Visit<'a> for Links {
        fn visit_inline(&mut self, inline: &'a Inline) {
            if let Inline::Link { target, .. } = inline {
                self.0.push(target.clone());
            }
            visit::walk_inline(self, inline);
        }
    }
    let mut collector = Links::default();
    collector.visit_document(document);
    collector.0
}

#[test]
fn atx_and_setext_headings_preserve_links_styles_and_source_once() {
    for source in [
        "# [Catalog](index.md)\n\n## **[Get-Item](Get-Item.md)** and `code`\n\nBody.\n",
        "[Catalog](index.md)\n===================\n\n**[Get-Item](Get-Item.md)** and `code`\n-------------------------------------\n\nBody.\n",
    ] {
        let query = query_markdown_text(source, None).unwrap();
        let document = query.document.as_ref().unwrap();
        assert!(document.meta.title.is_none());
        assert_eq!(document.display_title().as_deref(), Some("Catalog"));
        assert!(document.heading.as_ref().unwrap().source.is_some());
        assert_eq!(
            document.sections[0].heading.plain_text(),
            "Get-Item and code"
        );
        assert!(document.sections[0].heading.source.is_some());
        assert_eq!(links(document).len(), 2);
        let markdown = render_markdown(&query);
        assert!(markdown.contains("# [Catalog](index.md)"), "{markdown}");
        assert!(
            markdown.contains("**[Get-Item](Get-Item.md)**"),
            "{markdown}"
        );
        let reparsed = parse_markdown(&markdown, None).unwrap().document;
        assert_eq!(links(&reparsed), links(document));
        let text = render_query_text(&query);
        assert_eq!(text.matches("Catalog").count(), 1, "{text}");
        assert_eq!(text.matches("Get-Item").count(), 1, "{text}");
        for pattern in ["Catalog", "Get-Item"] {
            let search = serde_json::from_value(serde_json::json!({"pattern": pattern})).unwrap();
            assert_eq!(
                search_query(&query, &search).unwrap().matches.len(),
                1,
                "{pattern}"
            );
        }
        assert!(
            document.diagnostics.is_empty(),
            "{:?}",
            document.diagnostics
        );
    }
}

#[test]
fn extracted_heading_is_readable_without_body_and_keeps_its_own_fragments() {
    let query = query_markdown_text("# [Catalog](index.md) {#Mixed.Target}\n", None).unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(document.blocks.is_empty() && document.sections.is_empty());
    assert_eq!(links(document).len(), 1);
    assert_eq!(
        mant_ir::DocumentIndex::build(document)
            .fragment_target("catalog")
            .map(mant_ir::NodeId::as_str),
        Some(mant_ir::DOCUMENT_ROOT_ID)
    );
    let excerpt =
        mant_engine::select_excerpt(&query, &[mant_protocol::ContentSelector::path("root")])
            .unwrap();
    assert!(mant_engine::render_excerpt_markdown(&excerpt).contains("[Catalog](index.md)"));
    let outline = mant_engine::build_outline_projection(
        &query,
        mant_protocol::EntryProjection::All,
        Some(mant_protocol::ContentSelector::path("root")),
    )
    .unwrap();
    assert_eq!(outline.nodes.len(), 1);
    assert!(matches!(
        &outline.nodes[0],
        mant_protocol::OutlineNode::DocumentRoot { .. }
    ));
    let parsed = parse_markdown(
        "# Catalog {#Mixed.Target}\n\n## Other\n\n[title](#Mixed%2ETarget)\n",
        None,
    )
    .unwrap()
    .document;
    assert!(
        matches!(&links(&parsed)[0], LinkTarget::Section { id } if id.as_str() == mant_ir::DOCUMENT_ROOT_ID)
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

#[test]
fn native_section_references_survive_heading_lowering_without_fake_document_heading() {
    let query = query_roff_bytes(b".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd heading reference\n.Sh DESCRIPTION\n.Ss Xr printf 3\nBody.\n").unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(document.heading.is_none());
    assert_eq!(document.meta.title.as_deref(), Some("PROBE"));
    let heading = &document.sections[1].children[0].heading;
    assert_eq!(heading.plain_text(), "printf(3)");
    assert!(
        matches!(&links(document)[0], LinkTarget::Manual { name, manual_section: Some(section) } if name == "printf" && section == "3")
    );
    let markdown = render_markdown(&query);
    assert!(markdown.contains("man:printf\\(3\\)"), "{markdown}");
    let reparsed = parse_markdown(&markdown, None).unwrap().document;
    assert_eq!(links(&reparsed), links(document));
    assert_eq!(render_query_text(&query).matches("printf(3)").count(), 1);
}

#[test]
fn heading_manual_uris_have_explicit_bounded_grammar_and_decode_once() {
    for (uri, name, section) in [
        ("man:printf", "printf", None),
        ("MAN:printf(3)", "printf", Some("3")),
        ("man:errno(2const)", "errno", Some("2const")),
        ("man:literal%252Fname(1)", "literal%2Fname", Some("1")),
    ] {
        let source = format!("# Catalog\n\n## [topic]({uri})\n");
        let document = parse_markdown(&source, None).unwrap().document;
        assert!(
            matches!(&links(&document)[0], LinkTarget::Manual { name: actual_name, manual_section } if actual_name == name && manual_section.as_deref() == section),
            "{uri}"
        );
    }
    for uri in [
        "man:",
        "man://host/topic",
        "man:foo/bar",
        "man:foo%2Fbar",
        "man:foo%5Cbar",
        "man:foo%00bar",
        "man:foo%20bar",
        "man:printf(qgroup)",
        "man:printf(3)?query",
        "man:printf(3)#fragment",
    ] {
        let source = format!("# [topic](<{uri}>)\n");
        let document = parse_markdown(&source, None).unwrap().document;
        assert!(
            !matches!(&links(&document)[0], LinkTarget::Manual { .. }),
            "{uri}"
        );
    }
}

#[test]
fn heading_all_target_kinds_and_local_fragments_round_trip_in_addressable_mode() {
    let query = query_markdown_text("# [Catalog](index.md)\n\n## [External](https://example.test) [Mail](mailto:user@example.test) [Local](#Mixed.Target)\n\n## Target {#Mixed.Target}\n", None).unwrap();
    let before = links(query.document.as_ref().unwrap());
    let markdown = mant_engine::render_markdown_with_options(
        &query,
        mant_engine::MarkdownOptions::ADDRESSABLE,
    );
    let reparsed = parse_markdown(&markdown, None).unwrap().document;
    assert_eq!(links(&reparsed), before, "{markdown}");
    assert!(
        !reparsed.diagnostics.iter().any(|diagnostic| diagnostic
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("ir."))),
        "{:?}",
        reparsed.diagnostics
    );
}

#[test]
fn heading_targets_do_not_change_ids_or_infer_entries() {
    let one = parse_markdown("# Catalog\n\n## [Topic](one.md)\n", None)
        .unwrap()
        .document;
    let two = parse_markdown("# Catalog\n\n## [Topic](two.md)\n", None)
        .unwrap()
        .document;
    assert_eq!(one.sections[0].id, two.sections[0].id);
    assert_eq!(
        one.sections[0].heading.plain_text(),
        two.sections[0].heading.plain_text()
    );
    let empty = parse_markdown("# Catalog\n\n## [](empty.md)\n", None)
        .unwrap()
        .document;
    assert_eq!(
        links(&empty).len(),
        1,
        "empty labels are real link occurrences"
    );
    assert!(mant_ir::SemanticIndex::build(&one).root().is_empty());
}

#[test]
fn setext_heading_breaks_preserve_inline_structure_in_markdown() {
    let query = query_markdown_text("[First](first.md)  \n[Second](second.md)\n===\n\n[Third](third.md)  \n[Fourth](fourth.md)\n---\n\nBody.\n", None).unwrap();
    let original = query.document.as_ref().unwrap();
    let markdown = render_markdown(&query);
    let reparsed = parse_markdown(&markdown, None).unwrap().document;
    assert_eq!(
        reparsed.heading.as_ref().unwrap().content,
        original.heading.as_ref().unwrap().content,
        "{markdown}"
    );
    assert_eq!(
        reparsed.sections[0].heading.content, original.sections[0].heading.content,
        "{markdown}"
    );
    assert_eq!(links(&reparsed), links(original));
}

#[test]
fn deep_multiline_headings_keep_hierarchy_and_links_in_portable_markdown() {
    let mut query = query_markdown_text("# Catalog\n\n## Parent\n\n### Child\n", None).unwrap();
    let child = &mut query.document.as_mut().unwrap().sections[0].children[0];
    let link = |name: &str| Inline::Link {
        target: LinkTarget::Document {
            name: name.to_owned(),
            fragment: None,
        },
        title: None,
        children: vec![Inline::Text {
            value: name.to_owned(),
        }],
    };
    child.heading.content = vec![link("First"), Inline::LineBreak, link("Second")];
    let markdown = render_markdown(&query);
    assert!(
        markdown.contains("### [First](First.md) [Second](Second.md)"),
        "{markdown}"
    );
    let reparsed = parse_markdown(&markdown, None).unwrap().document;
    assert_eq!(reparsed.sections[0].children.len(), 1);
    assert_eq!(
        reparsed.sections[0].children[0].heading.plain_text(),
        "First Second"
    );
    assert_eq!(links(&reparsed), links(query.document.as_ref().unwrap()));
}

#[test]
fn local_heading_links_force_addressable_export_even_when_semantics_were_requested() {
    let query = query_markdown_text("# [Catalog](#catalog)\n\n## [Run](#command-run)\n\n<!-- mant:entries role=command case=sensitive -->\n- `run`: Run the program.\n", None).unwrap();
    let before = links(query.document.as_ref().unwrap());
    for options in [
        mant_engine::MarkdownOptions::default(),
        mant_engine::MarkdownOptions::ADDRESSABLE,
        mant_engine::MarkdownOptions {
            preserve_semantics: true,
            preserve_anchors: false,
        },
    ] {
        let markdown = mant_engine::render_markdown_with_options(&query, options);
        assert!(
            markdown.contains("[Catalog](#document-overview)"),
            "{markdown}"
        );
        assert!(markdown.contains("[Run](#command-run)"), "{markdown}");
        assert!(!markdown.contains("mant:entries"), "{markdown}");
        let root = markdown.find("<a id=\"document-overview\"").unwrap();
        assert!(root < markdown.find("# [Catalog]").unwrap(), "{markdown}");
        assert_eq!(
            markdown.matches("<a id=\"document-overview\"").count(),
            1,
            "{markdown}"
        );
        let reparsed = parse_markdown(&markdown, None).unwrap().document;
        assert_eq!(links(&reparsed), before, "{markdown}");
        // Addressable output retains HTML destinations, not a promise to
        // reimport semantic identities from otherwise unsupported raw HTML.
        for target in &before {
            if let LinkTarget::Section { id } = target {
                assert!(markdown.contains(&format!("<a id=\"{id}\"")), "{markdown}");
            }
        }
    }
    let plain = query_markdown_text("# Catalog\n\n## Topic\n\nBody.\n", None).unwrap();
    assert!(!render_markdown(&plain).contains("<a "));
}

#[test]
fn root_anchor_precedes_the_real_heading_and_never_moves_after_tldr() {
    for body in ["", "\n\nBody."] {
        let source = format!(
            "<!-- mant:tldr:start -->\n# tool\n\n> Quick help.\n\n- Show help:\n\n`tool --help`\n<!-- mant:tldr:end -->\n\n# [Catalog](#catalog){body}\n"
        );
        let query = query_markdown_text(&source, None).unwrap();
        let markdown = render_markdown(&query);
        assert!(
            markdown.find("<a id=\"document-overview\"").unwrap()
                < markdown.find("# [Catalog]").unwrap(),
            "{markdown}"
        );
        assert!(
            markdown.find("# [Catalog]").unwrap() < markdown.find("## TLDR").unwrap(),
            "{markdown}"
        );
        for (pattern, expected_path) in [("Catalog", "root"), ("Quick help", "0")] {
            let search = serde_json::from_value(serde_json::json!({"pattern": pattern})).unwrap();
            let matches = search_query(&query, &search).unwrap().matches;
            assert_eq!(matches.len(), 1, "{pattern}: {markdown}");
            assert_eq!(
                matches[0].outline.path(),
                expected_path,
                "{pattern}: {markdown}"
            );
        }
    }
}

#[test]
fn heading_links_to_inline_anchors_preserve_both_destination_and_occurrence() {
    let mut query = query_markdown_text("# Catalog\n\n## Local\n", None).unwrap();
    let section = &mut query.document.as_mut().unwrap().sections[0];
    section.heading.content = vec![Inline::Link {
        target: LinkTarget::Section {
            id: "inline-target".into(),
        },
        title: None,
        children: vec![Inline::Text {
            value: "Spot".into(),
        }],
    }];
    section.blocks.push(mant_ir::Block::Paragraph {
        children: vec![
            Inline::Anchor {
                id: "inline-target".into(),
                fragment_aliases: vec![],
                owner_source: None,
            },
            Inline::Text {
                value: "Body.".into(),
            },
        ],
        layout: mant_ir::LayoutHint::default(),
        source: None,
    });
    let markdown = render_markdown(&query);
    assert!(markdown.contains("[Spot](#inline-target)"), "{markdown}");
    assert!(markdown.contains("<a id=\"inline-target\""), "{markdown}");
}
