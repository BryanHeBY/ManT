//! Existing regressions grouped by navigation behavior; expected values remain independent.
use super::*;

#[test]
fn section_ids_do_not_shadow_independent_semantic_names() {
    let parsed = parse_markdown(
        "# Tool\n\n## force\n\nStructural prose.\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `force`: Force an operation.\n",
        None,
    )
    .expect("section and entry selector collision remains readable");

    assert!(
        parsed.document.diagnostics.is_empty(),
        "{:?}",
        parsed.document.diagnostics
    );
}

#[test]
fn unrelated_semantic_looking_sections_do_not_perturb_entry_ids() {
    let entry_id = |source: &str| {
        let parsed = parse_markdown(source, None).expect("semantic ID fixture");
        let entries = mant_ir::content_entries(&parsed.document.sections[1].blocks);
        entries[0]
            .owner()
            .facts()
            .expect("option identity")
            .id
            .to_string()
    };
    let original = entry_id(
        "# Tool\n\n## Notes\n\nText.\n\n## Options\n\n<!-- mant:entries role=option -->\n- `-v`: Verbose.\n",
    );
    let edited = entry_id(
        "# Tool\n\n## option-v\n\nUnrelated text.\n\n## Options\n\n<!-- mant:entries role=option -->\n- `-v`: Verbose.\n",
    );

    assert_eq!(original, "option-v");
    assert_eq!(edited, original);
}

#[test]
fn preserves_titles_for_every_supported_markdown_link_target() {
    let document = parse_document(
        "[section](#target \"section title\") [document](other.md \"document title\") [mail](mailto:user@example.test \"mail title\") [web](https://example.test \"web title\")\n\n## Target\n",
        Some("/docs/tool.md".to_owned()),
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("link paragraph");
    };
    let titles = children
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link { title, .. } => title.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        titles,
        ["section title", "document title", "mail title", "web title"]
    );
}

#[test]
fn markdown_link_components_decode_once_and_validate_before_navigation() {
    use mant_ir::LinkTarget;
    for (uri, expected_name, expected_fragment) in [
        (
            "space%20name.md#Mixed%2ETarget",
            "space name",
            Some("Mixed.Target"),
        ),
        (
            "literal%2520name.md#Mixed%252ETarget",
            "literal%20name",
            Some("Mixed%2ETarget"),
        ),
        ("%E6%97%A5%E6%9C%AC.md", "日本", None),
        ("other.md#part:two", "other", Some("part:two")),
        ("other.md#part%3Atwo", "other", Some("part:two")),
        ("../other.md", "../other", None),
    ] {
        let document = parse_document(&format!("[LINK]({uri})\n"), None);
        let Block::Paragraph { children, .. } = &document.blocks[0] else {
            panic!("link paragraph")
        };
        assert!(
            matches!(&children[0], Inline::Link { target: LinkTarget::Document { name, fragment }, .. } if name == expected_name && fragment.as_deref() == expected_fragment),
            "{uri}: {children:?}"
        );
    }
    for uri in [
        "bad%2Fname.md",
        "bad%5Cname.md",
        "bad%00.md",
        "bad%FF.md",
        "bad%GG.md",
        "bad%2.md",
        "bad%3Fquery.md",
        "bad%23fragment.md",
        "C%3A/page.md",
        "#bad%00",
        "#bad%FF",
    ] {
        let document = parse_document(&format!("[LINK]({uri})\n"), None);
        let Block::Paragraph { children, .. } = &document.blocks[0] else {
            panic!("link paragraph")
        };
        assert!(
            matches!(
                &children[0],
                Inline::Link {
                    target: LinkTarget::External { .. },
                    ..
                }
            ),
            "{uri}: {children:?}"
        );
    }
    let document = parse_document("## Mixed {#Mixed.Target}\n[LINK](#Mixed%2ETarget)\n", None);
    assert!(
        !document
            .diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.dangling-section-link"))
    );
}

#[test]
fn lowers_hierarchical_markdown_links_into_same_source_document_references() {
    let document = parse_document(
        "[Start](Start-Process.md) [Guide](about_Profiles.markdown#examples) [Nested](../other.md)\n",
        Some("/docs/current.md".to_owned()),
    );
    let Block::Paragraph { children, .. } = &document.blocks[0] else {
        panic!("links are a paragraph");
    };

    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Document { name, fragment: None }, .. } if name == "Start-Process"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Document { name, fragment: Some(fragment) }, .. }
            if name == "about_Profiles" && fragment == "examples"
    )));
    assert!(children.iter().any(|inline| matches!(
        inline,
        Inline::Link { target: mant_ir::LinkTarget::Document { name, fragment: None }, .. } if name == "../other"
    )));
}

#[test]
fn document_title_fragments_follow_the_normalized_root_destination() {
    let document = parse_document(
        "# Guide {#Mixed.Root}\n\nPreface.\n\n## Details\n\nBody.\n",
        None,
    );

    assert_eq!(
        document
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["Mixed.Root", "guide"]
    );
    assert_eq!(
        mant_ir::DocumentIndex::build(&document)
            .fragment_target("Mixed.Root")
            .map(mant_ir::NodeId::as_str),
        Some(mant_ir::DOCUMENT_ROOT_ID)
    );

    let document = parse_document("# Guide {#Empty.Root}\n\n## Details\n\nBody.\n", None);
    assert!(document.sections[0].fragment_aliases.is_empty());
    assert_eq!(
        document
            .fragment_aliases
            .iter()
            .map(mant_ir::FragmentAlias::as_str)
            .collect::<Vec<_>>(),
        ["Empty.Root", "guide"]
    );
    assert_eq!(
        mant_ir::DocumentIndex::build(&document)
            .fragment_target("Empty.Root")
            .map(mant_ir::NodeId::as_str),
        Some(mant_ir::DOCUMENT_ROOT_ID)
    );
}
