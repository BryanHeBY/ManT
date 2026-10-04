use super::*;

#[test]
fn navigation_only_tables_emit_targets_without_empty_fences() {
    let paragraph = |children| Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children,
        layout: LayoutHint::default(),
        source: None,
    };
    let table = |cells| Block::Table {
        rows: vec![TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells,
        }],
        column_preferences: native_column_preferences(&[3, 3]),
        layout: LayoutHint::default(),
        source: None,
    };
    let cell = |blocks| TableCell {
        break_after: false,
        blocks,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: None,
    };
    let navigation = cell(vec![paragraph(vec![Inline::Strong {
        children: vec![Inline::anchor_with_aliases(
            "target",
            vec!["Exact.Target".into()],
        )],
    }])]);
    for preserve_anchors in [false, true] {
        let output = super::render_blocks_fragment(
            &[table(vec![navigation.clone()])],
            super::MarkdownFragmentOptions { preserve_anchors },
        )
        .join("\n\n");
        if preserve_anchors {
            assert_eq!(output, "<a id=\"target\"></a>\n<a id=\"Exact.Target\"></a>");
        } else {
            assert_eq!(output, "");
        }
    }
    for cells in [
        vec![],
        vec![cell(vec![])],
        vec![navigation.clone(), cell(vec![])],
        vec![cell(vec![]), navigation],
    ] {
        let output = super::render_blocks_fragment(
            &[table(cells)],
            super::MarkdownFragmentOptions::default(),
        )
        .join("\n\n");
        assert!(output.starts_with("```\n"), "{output:?}");
        assert!(output.ends_with("\n```"), "{output:?}");
    }
}

#[test]
fn large_entry_source_maps_keep_monotonic_exact_ownership() {
    use std::fmt::Write;
    let mut source = "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n".to_owned();
    for index in 0..1000 {
        writeln!(source, "- `--flag-{index}`: Payload{index}.").unwrap();
    }
    let query = parse_content(&source, None).unwrap();
    let document = query.document.unwrap();
    let rendered = super::blocks::render_blocks_with_entries(
        &document.blocks,
        MarkdownOptions {
            preserve_anchors: true,
            ..MarkdownOptions::default()
        },
        true,
    );
    assert_eq!(rendered.entries.len(), 1000);
    for (index, entry) in rendered.entries.iter().enumerate() {
        assert!(rendered.text[entry.start..entry.end].contains(&format!("Payload{index}.")));
        if let Some(next) = rendered.entries.get(index + 1) {
            assert!(entry.end <= next.start);
        }
    }
}

#[test]
fn document_export_skips_maps_but_keeps_identical_addressable_bytes() {
    let source = "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--root`: Root payload.\n\n## Parent\n\n<!-- mant:entries role=option case=sensitive -->\n- `--first`: First payload.\n\n### Child\n\n<!-- mant:entries role=option case=sensitive -->\n- `--second`: Second payload.\n";
    let query = parse_content(source, None).unwrap();
    let mapped = render_addressable_markdown(&query);
    let plain = super::render_markdown_artifact(&query, MarkdownOptions::ADDRESSABLE, false);
    assert_eq!(mapped.text(), plain.text());
    assert!(plain.nodes().is_empty());
    assert!(plain.sections.is_empty());
    assert!(plain.anchors.get().is_none());
    assert_eq!(mapped.sections.len(), 2);
    assert_eq!(mapped.sections[0].parent, None);
    assert_eq!(mapped.sections[1].parent, Some(0));
    let document = query.document.as_ref().unwrap();
    assert!(std::ptr::eq(
        mapped.sections[0].section,
        &raw const document.sections[0]
    ));
    assert!(std::ptr::eq(
        mapped.sections[1].section,
        &raw const document.sections[0].children[0]
    ));
    let mut entries = 0;
    for mapped in mapped.nodes() {
        if let MarkdownNode::DocumentEntry { owner, names, .. } = &mapped.node {
            entries += 1;
            assert!(std::ptr::eq(
                names.as_ptr(),
                owner.facts().unwrap().names.as_ptr()
            ));
        }
    }
    assert_eq!(entries, 3);
}

#[test]
fn maps_borrow_owners_from_their_exact_source_snapshot() {
    fn owner<'a>(node: &MarkdownNode<'a>) -> Option<mant_ir::EntryOwner<'a>> {
        match node {
            MarkdownNode::DocumentEntry { owner, .. } => Some(*owner),
            _ => None,
        }
    }
    let source =
        "# Tool\n\n<!-- mant:entries role=option case=sensitive -->\n- `--flag`: Payload.\n";
    let first = parse_content(source, None).unwrap();
    let second = first.clone();
    let a = render_addressable_markdown(&first);
    let b = render_addressable_markdown(&second);
    let a_owner = a
        .nodes()
        .iter()
        .find_map(|mapped| owner(&mapped.node))
        .unwrap();
    let b_owner = b
        .nodes()
        .iter()
        .find_map(|mapped| owner(&mapped.node))
        .unwrap();
    assert_eq!(a_owner.facts().unwrap().id, b_owner.facts().unwrap().id);
    assert!(!std::ptr::eq(
        a_owner.facts().unwrap(),
        b_owner.facts().unwrap()
    ));
    let original = mant_ir::content_entries(&first.document.as_ref().unwrap().blocks);
    assert!(std::ptr::eq(
        a_owner.facts().unwrap(),
        original[0].owner().facts().unwrap()
    ));
}

#[test]
fn addressable_markdown_emits_canonical_and_authored_fragments() {
    let mut section = section(
        "Mixed target",
        vec![paragraph(vec![Inline::anchor_with_aliases(
            "option",
            vec!["--option".into()],
        )])],
        Vec::new(),
    );
    section.id = "mixed-target".into();
    section.fragment_aliases = vec!["Mixed.Target".into()];
    let query = ResolvedContent {
        address: None,
        label: "fragments".to_owned(),
        document: Some(manual(vec![section])),
        tldr: None,
    };

    let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(markdown.contains("<a id=\"mixed-target\"></a>"));
    assert!(markdown.contains("<a id=\"Mixed.Target\"></a>"));
    assert!(markdown.contains("<a id=\"option\"></a>"));
    assert!(markdown.contains("<a id=\"--option\"></a>"));
}

#[test]
fn addressable_markdown_emits_document_root_fragments() {
    let mut document = manual(Vec::new());
    document.blocks = vec![paragraph(vec![Inline::Text {
        value: "Preface.".to_owned(),
    }])];
    document.fragment_aliases = vec!["Mixed.Root".into()];
    let query = ResolvedContent {
        address: None,
        label: "fragments".to_owned(),
        document: Some(document),
        tldr: None,
    };

    let markdown = render_markdown_with_options(&query, MarkdownOptions::ADDRESSABLE);
    assert!(markdown.contains("<a id=\"document-overview\"></a>"));
    assert!(markdown.contains("<a id=\"Mixed.Root\"></a>"));
}

#[test]
fn addressable_rendering_returns_exact_semantic_node_ranges() {
    let entry = DefinitionItem {
        head_body_relation: mant_ir::HeadBodyRelation::from(false),
        source: None,
        entry: Some(EntryFacts {
            name_bindings: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            forms: Vec::new(),
            id: "help-entry".into(),
            kind: EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["--help".to_owned()],
            value_domain: None,
        }),
        terms: (vec![vec![
            Inline::anchor("help-entry"),
            Inline::Code {
                value: "--help".to_owned(),
            },
        ]])
        .into_iter()
        .map(Into::into)
        .collect(),
        description: vec![paragraph(vec![Inline::Text {
            value: "Show help.".to_owned(),
        }])],
        layout: mant_ir::DefinitionLayout {
            body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
            spacing_before_lines: None,
            ..Default::default()
        },
    };
    let query = ResolvedContent {
        address: None,
        label: "demo".to_owned(),
        document: Some(manual(vec![section(
            "OPTIONS",
            vec![
                Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![entry],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                },
                paragraph(vec![Inline::Text {
                    value: "Following section prose.".to_owned(),
                }]),
            ],
            Vec::new(),
        )])),
        tldr: None,
    };

    let artifact = render_addressable_markdown(&query);
    let mapped = artifact
        .nodes()
        .iter()
        .find(|mapped| matches!(mapped.node, MarkdownNode::DocumentEntry { .. }))
        .expect("semantic entry range");
    let MarkdownNode::DocumentEntry { path, owner, .. } = &mapped.node else {
        unreachable!();
    };
    assert_eq!(path.to_string(), "1/e1");
    assert_eq!(owner.facts().unwrap().id, "help-entry");
    let rendered = &artifact.text()[mapped.range.clone()];
    assert!(rendered.contains("--help"));
    assert!(rendered.contains("Show help."));
    assert!(!rendered.contains("Following section prose."));
}

#[test]
fn final_artifact_owns_only_real_anchor_ranges() {
    let mut builder = super::ArtifactBuilder {
        track: true,
        ..super::ArtifactBuilder::default()
    };
    builder.begin_root(0);
    builder.push("<a id=\"real\"></a>\n`<a id=\"literal\"></a>`\n\n ");
    let artifact = builder.finish();
    assert!(artifact.anchors.get().is_none());
    let ranges = artifact.anchor_ranges();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 0.."<a id=\"real\"></a>".len());
    assert_eq!(&artifact.text()[ranges[0].clone()], "<a id=\"real\"></a>");
    assert!(std::ptr::eq(ranges, artifact.anchor_ranges()));
    assert!(artifact.text().ends_with('`'));
    assert_eq!(artifact.nodes().len(), 1);
    assert_eq!(artifact.nodes()[0].range, 0..artifact.text().len());
    assert!(matches!(
        artifact.nodes()[0].node,
        MarkdownNode::DocumentRoot
    ));
    let final_text = artifact.text().to_owned();
    assert_eq!(artifact.into_text(), final_text);
}

#[test]
fn public_artifact_section_lookup_rejects_out_of_range_slots() {
    let content = parse_content("# Tool\n\n## Parent\n\n### Child\n\nBody.\n", None).unwrap();
    let artifact = render_addressable_markdown(&content);
    assert_eq!(artifact.section(0).unwrap().path().to_string(), "1");
    let child = artifact.section(1).unwrap();
    assert_eq!(child.parent(), Some(0));
    assert_eq!(child.section().heading.plain_text(), "Child");
    assert!(artifact.section(2).is_none());
    assert!(artifact.section(usize::MAX).is_none());
    for mapped in artifact.nodes() {
        assert!(artifact.text().get(mapped.range()).is_some());
        if let MarkdownNode::DocumentSection { section, .. } = mapped.node() {
            assert!(artifact.section(*section).is_some());
        }
    }
}
