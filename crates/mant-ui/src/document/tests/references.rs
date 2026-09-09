//! Reference inventory and exact source-row geometry remain independent of entries.
use super::*;

fn document_link(label: &str, fragment: Option<&str>) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::Document {
            name: "target".into(),
            fragment: fragment.map(str::to_owned),
        },
        title: None,
        children: vec![Inline::Text {
            value: label.into(),
        }],
    }
}

fn linked_block(prefix: &str, label: &str) -> Block {
    Block::Paragraph {
        children: vec![
            Inline::Text {
                value: prefix.into(),
            },
            document_link(label, None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn associated_heading_and_forms_do_not_create_redundant_reference_groups() {
    let query = mant_engine::query_markdown_text(
        "# [Catalog](catalog.md)\n\n## [Commands](commands.md)\n\n<!-- mant:entries role=command case=sensitive -->\n- [`git-add`](git-add.md): See [tutorial](tutorial.md).\n",
        None,
    ).unwrap();
    let before = query.clone();
    let view = DocumentView::new(&query);
    let command = view
        .navigation
        .iter()
        .find(|node| node.title == "git-add")
        .unwrap();
    assert!(matches!(
        command.kind,
        NavKind::Entry(mant_ir::EntryKind::Command)
    ));
    assert_eq!(view.reference_badges[&command.id], "↗ git-add");
    assert_eq!(view.associated_reference_choices(&command.id).len(), 1);
    assert_eq!(view.reference_badges.len(), 3);
    assert_eq!(view.references.len(), 4);
    let body = view
        .navigation
        .iter()
        .filter(|node| node.kind == NavKind::Reference)
        .collect::<Vec<_>>();
    assert_eq!(body.len(), 1);
    assert!(body[0].title.contains("tutorial"));
    assert_eq!(query, before);
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            assert!(rendered.anchor_row(&reference.id).is_some());
        }
    }
}

#[test]
fn native_command_form_and_body_references_share_inventory_not_presentation() {
    let query = mant_engine::query_roff_bytes(b".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Xr git-add 1\nSee\n.Xr git-add 1\nand\n.Xr gittutorial 7 .\n.El\n").unwrap();
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 3);
    assert_eq!(
        view.reference_text(&view.references[0].id).as_deref(),
        Some("man:git-add(1)")
    );
    assert_eq!(
        view.reference_badges.len(),
        1,
        "{:?}",
        crate::document::references::ReferenceNavigation::build(query.document.as_ref().unwrap())
    );
    let owner = view.reference_badges.keys().next().unwrap();
    assert!(
        view.navigation
            .iter()
            .any(|node| node.id == *owner && matches!(node.kind, NavKind::Entry(_)))
    );
    assert_eq!(view.associated_reference_choices(owner).len(), 1);
    assert_eq!(
        view.navigation
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        2
    );
}

#[test]
fn invalid_form_association_falls_back_without_hiding_any_source() {
    let mut query = mant_engine::query_markdown_text(
        "# Demo\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`command`](command.md): Description.\n", None).unwrap();
    let document = query.document.as_mut().unwrap();
    let Block::List { items, .. } = &mut document.sections[0].blocks[0] else {
        panic!("expected list")
    };
    items[0].entry.as_mut().unwrap().forms[0].parts[0].path = vec![999];
    let view = DocumentView::new(&query);
    assert!(view.reference_badges.is_empty());
    assert_eq!(view.references.len(), 1);
    assert_eq!(
        view.navigation
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        1
    );
}

#[test]
fn hidden_or_invalid_form_owner_uses_body_fallback_not_section_badge() {
    let original = mant_engine::query_markdown_text(
        "# Demo\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`command`](command.md): Description.\n", None).unwrap();
    let mut view = DocumentView::new(&original);
    view.navigation
        .retain(|node| !matches!(node.kind, NavKind::Entry(_) | NavKind::EntryGroup));
    let mut references = crate::document::references::ReferenceNavigation::build(
        original.document.as_ref().unwrap(),
    );
    references.check_source_owners(
        original.document.as_ref().unwrap(),
        &mant_ir::SemanticIndex::build(original.document.as_ref().unwrap()),
    );
    references.append_navigation(&mut view.navigation);
    assert!(references.associated.is_empty());
    assert_eq!(
        view.navigation
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        1
    );

    let mut invalid = original;
    let Block::List { items, .. } = &mut invalid.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("expected list")
    };
    items[0].entry.as_mut().unwrap().id = "Invalid.Owner".into();
    let view = DocumentView::new(&invalid);
    assert!(view.reference_badges.is_empty());
    assert_eq!(view.references.len(), 1);
    assert!(
        view.navigation
            .iter()
            .any(|node| node.kind == NavKind::Reference)
    );
}

#[test]
fn hidden_owner_cannot_lend_its_badge_to_an_unrelated_visible_same_id() {
    let mut query = mant_engine::query_markdown_text(
        "# Demo\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`first`](first.md): First.\n- `second`: Unlinked.\n", None).unwrap();
    let Block::List { items, .. } = &mut query.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        panic!("expected list")
    };
    let id = items[0].entry.as_ref().unwrap().id.clone();
    items[1].entry.as_mut().unwrap().id = id.clone();
    let view = DocumentView::new(&query);
    let mut nodes = view
        .navigation
        .into_iter()
        .filter(|node| {
            node.kind != NavKind::Reference
                && node.kind != NavKind::ReferenceGroup
                && node.title != "first"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        nodes.iter().filter(|node| node.id == id.as_str()).count(),
        1
    );
    let mut references =
        crate::document::references::ReferenceNavigation::build(query.document.as_ref().unwrap());
    references.check_source_owners(
        query.document.as_ref().unwrap(),
        &mant_ir::SemanticIndex::build(query.document.as_ref().unwrap()),
    );
    references.append_navigation(&mut nodes);
    assert!(references.associated.is_empty());
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        1
    );
    assert!(
        nodes
            .iter()
            .filter(|node| node.kind == NavKind::ReferenceGroup)
            .all(|node| node.parent_id.as_deref() != Some(id.as_str()))
    );
}

#[test]
fn limited_associated_inventory_never_claims_a_single_target_is_unique() {
    let mut query = bundle();
    query.document.as_mut().unwrap().heading = Some(mant_ir::Heading {
        content: (0..1001)
            .map(|_| document_link("same", Some("part")))
            .collect(),
        source: None,
    });
    let view = DocumentView::new(&query);
    assert!(view.references_limited());
    assert!(view.references.len() <= 1000);
    let badge = &view.reference_badges[mant_ir::DOCUMENT_ROOT_ID];
    assert!(badge.contains("known") && badge.contains("more may exist"));
    assert_eq!(
        view.associated_reference_choices(mant_ir::DOCUMENT_ROOT_ID)
            .len(),
        view.references.len()
    );
}

#[test]
fn references_group_full_targets_without_promoting_entries_or_rewriting_body() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: vec![
            document_link("first", None),
            Inline::Text {
                value: " then ".into(),
            },
            document_link("second", None),
            document_link("fragment", Some("part")),
            document_link("", None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let before = query.clone();
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 4);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| matches!(node.kind, NavKind::Entry(_)))
            .count(),
        0
    );
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        4
    );
    assert!(
        view.navigation()
            .iter()
            .any(|node| node.title.ends_with("3 locations"))
    );
    assert!(
        view.references
            .iter()
            .any(|reference| reference.label.contains("unlabelled"))
    );
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            assert!(
                rendered.anchor_row(&reference.id).is_some(),
                "{reference:?}"
            );
        }
        assert!(!rendered.text.to_string().contains("unlabelled"));
        assert!(!rendered.text.to_string().contains("DOCUMENT REFERENCES"));
    }
    assert_eq!(query, before);
    let second = DocumentView::new(&query);
    assert_eq!(
        view.references
            .iter()
            .map(|r| (&r.id, &r.location))
            .collect::<Vec<_>>(),
        second
            .references
            .iter()
            .map(|r| (&r.id, &r.location))
            .collect::<Vec<_>>()
    );
}

#[test]
fn reference_origins_follow_actual_occurrence_through_wrapping_and_table_stacking() {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.heading = Some(mant_ir::Heading {
        content: vec![document_link("ROOTLINK", None)],
        source: None,
    });
    document.sections[0].heading = mant_ir::Heading {
        content: vec![document_link("HEADLINK", None)],
        source: None,
    };
    document.sections[0].blocks = vec![
        linked_block("日本 e\u{301}\tbefore before before ", "BODYLINK"),
        Block::List {
            kind: mant_ir::ListKind::Bullet,
            compact: true,
            items: vec![ListItem {
                blocks: vec![linked_block("list prefix ", "LISTLINK")],
                entry: None,
                layout: mant_ir::ListItemLayout::default(),
                source: None,
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        blocks: vec![linked_block("left ", "CELLLINK")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
                        blocks: vec![paragraph("right column has substantial wrapped content")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 5);
    for width in [8, 12, 24, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            let found = rendered.search(&reference.label);
            assert!(
                !found.is_empty(),
                "width={width} {reference:?}\n{}",
                rendered.text
            );
            assert_eq!(
                rendered.anchor_row(&reference.id),
                Some(found[0].row),
                "width={width} {reference:?}\n{}",
                rendered.text
            );
        }
    }
}

#[test]
fn bounded_inventory_exposes_truncation_without_removing_body_links() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: (0..1002).map(|_| document_link("x", None)).collect(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 1000);
    assert!(
        view.navigation()
            .iter()
            .any(|node| node.kind == NavKind::ReferenceNotice)
    );
    assert_eq!(view.render(80).text.to_string().matches('x').count(), 1002);
}

#[test]
fn empty_only_reference_keeps_a_reveal_location_without_manufactured_text() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks =
        vec![linked_block("", ""), paragraph("AFTER")];
    let view = DocumentView::new(&query);
    let rendered = view.render(80);
    assert_eq!(view.references.len(), 1);
    assert_eq!(
        rendered.anchor_row(&view.references[0].id),
        Some(rendered.search("AFTER")[0].row)
    );
    assert!(!rendered.text.to_string().contains("target"));
}

#[test]
fn definition_term_and_run_in_description_keep_separate_source_origins() {
    for inline_term in [false, true] {
        let mut query = bundle();
        query.document.as_mut().unwrap().sections[0].blocks = vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                terms: vec![vec![
                    Inline::Text {
                        value: "日本 ".into(),
                    },
                    document_link("TERM", None),
                ]],
                description: vec![linked_block("body prefix ", "TAILREF")],
                entry: None,
                source: None,
                layout: mant_ir::DefinitionLayout {
                    inline_term,
                    ..Default::default()
                },
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }];
        let view = DocumentView::new(&query);
        assert_eq!(view.references.len(), 2);
        for width in [8, 12, 40, 80] {
            let rendered = view.render(width);
            for reference in &view.references {
                assert_eq!(
                    rendered.anchor_row(&reference.id),
                    Some(rendered.search(&reference.label)[0].row),
                    "inline={inline_term} width={width}\n{}",
                    rendered.text
                );
            }
        }
    }
}

#[test]
fn duplicate_invalid_owner_ids_do_not_duplicate_or_misassign_references() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![linked_block("", "FIRST")];
    let mut second = query.document.as_ref().unwrap().sections[0].clone();
    second.blocks = vec![linked_block("", "SECOND")];
    query.document.as_mut().unwrap().sections.push(second);
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 2);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        2
    );
    let rendered = view.render(80);
    for reference in &view.references {
        assert_eq!(
            rendered.anchor_row(&reference.id),
            Some(rendered.search(&reference.label)[0].row)
        );
    }
}
