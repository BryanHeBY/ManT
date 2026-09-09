//! Associated badges are an offline view of exact returned content positions.
use mant_engine::{
    build_outline_with_references, query_markdown_text, render_outline_text,
    render_outline_text_with,
};
use mant_protocol::{
    EntryProjection, OutlineNode, QueryOutline, ReferenceAssociation, ReferenceProjection,
    ReferenceProjectionMode, TextRole,
};

const SOURCE: &str = "# [Catalog](catalog.md#root)\n\n## [Commands](commands.md#all)\n\n<!-- mant:entries role=command case=sensitive -->\n- [`run`](run.md#usage): See [body](body.md).\n- [`stop`](stop.md): Stop.\n";

fn outline(mode: ReferenceProjectionMode, entries: EntryProjection, limit: u32) -> QueryOutline {
    let query = query_markdown_text(SOURCE, None).unwrap();
    build_outline_with_references(
        &query,
        entries,
        None,
        &ReferenceProjection {
            mode,
            limit,
            ..Default::default()
        },
    )
    .unwrap()
}

fn tree(outline: &QueryOutline) -> String {
    render_outline_text(outline)
        .split("References:")
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn headings_and_forms_get_badges_but_body_remains_inventory_only() {
    let outline = outline(ReferenceProjectionMode::All, EntryProjection::All, 100);
    let text = tree(&outline);
    assert!(text.contains("OVERVIEW ↗ catalog#root"), "{text}");
    assert!(text.contains("Commands ↗ commands#all"), "{text}");
    assert!(text.contains("run ↗ run#usage"), "{text}");
    assert!(!text.contains("↗ body"), "{text}");
    assert_eq!(outline.references.records.len(), 5);
    let restored: QueryOutline =
        serde_json::from_str(&serde_json::to_string(&outline).unwrap()).unwrap();
    assert_eq!(
        render_outline_text(&restored),
        render_outline_text(&outline)
    );
    let decorated = render_outline_text_with(&outline, |style, text| {
        if style.role == TextRole::Reference {
            format!("\u{1b}[36m{text}\u{1b}[0m")
        } else {
            text.to_owned()
        }
    });
    assert!(decorated.contains("\u{1b}[36m↗"));
    assert_eq!(
        decorated.replace("\u{1b}[36m", "").replace("\u{1b}[0m", ""),
        render_outline_text(&outline)
    );
}

#[test]
fn nonmaterialized_and_partial_pages_do_not_claim_complete_capabilities() {
    for mode in [
        ReferenceProjectionMode::None,
        ReferenceProjectionMode::Summary,
    ] {
        assert!(!tree(&outline(mode, EntryProjection::All, 100)).contains('↗'));
    }
    let partial = tree(&outline(
        ReferenceProjectionMode::All,
        EntryProjection::All,
        1,
    ));
    assert!(
        partial.contains("↗ catalog#root (known; more may exist)"),
        "{partial}"
    );
    let hidden = tree(&outline(
        ReferenceProjectionMode::All,
        EntryProjection::None,
        100,
    ));
    assert!(hidden.contains("Commands ↗ commands#all"));
    assert!(!hidden.contains("↗ run"));
}

#[test]
fn invalid_empty_and_limited_associations_never_create_form_badges() {
    let base = outline(ReferenceProjectionMode::All, EntryProjection::All, 100);
    for association in [
        ReferenceAssociation::Invalid {},
        ReferenceAssociation::Limited {},
        ReferenceAssociation::Unrecorded {},
    ] {
        let mut outline = base.clone();
        outline.references.records[2].association = association;
        assert!(!tree(&outline).contains("run ↗"));
    }
    let mut outline = base;
    if let ReferenceAssociation::Valid { forms, .. } =
        &mut outline.references.records[2].association
    {
        forms.clear();
    }
    assert!(!tree(&outline).contains("run ↗"));
}

#[test]
fn duplicate_ids_do_not_merge_structural_owners_or_hide_distinct_fragments() {
    let mut outline = outline(ReferenceProjectionMode::All, EntryProjection::All, 100);
    let OutlineNode::DocumentSection { children, .. } = &mut outline.nodes[1] else {
        panic!("section")
    };
    if let OutlineNode::DocumentEntry { id, .. } = &mut children[1] {
        *id = "command-run".into();
    }
    let text = tree(&outline);
    assert!(text.contains("run ↗ run#usage"));
    assert!(text.contains("stop ↗ stop"));
    let mut second = outline.references.records[2].clone();
    second.target = mant_ir::LinkTarget::Document {
        name: "run".into(),
        fragment: Some("other".into()),
    };
    outline.references.records.push(second);
    outline.references.occurrences = mant_protocol::ReferenceCount::Exact { value: 6 };
    assert!(tree(&outline).contains("run ↗ 2 targets"));
    let mut duplicate = children_snapshot(&outline);
    if let OutlineNode::DocumentSection { children, .. } = &mut outline.nodes[1] {
        children.push(duplicate.remove(0));
    }
    assert!(
        !tree(&outline).contains("run ↗"),
        "ambiguous exact owner must not attach twice"
    );
}

fn children_snapshot(outline: &QueryOutline) -> Vec<OutlineNode> {
    outline.nodes[1].children().to_vec()
}

#[test]
fn transparent_nested_item_and_selected_summary_keep_exact_owner() {
    let query = query_markdown_text("# Nested\n\n## Commands\n\n- Transparent container.\n\n  <!-- mant:entries role=command case=sensitive -->\n  - [`run`](run.md): Execute.\n", None).unwrap();
    let policy = ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        ..Default::default()
    };
    let all = build_outline_with_references(&query, EntryProjection::All, None, &policy).unwrap();
    let node = all.nodes[1].children().first().unwrap();
    let OutlineNode::DocumentEntry { owner, path, .. } = node else {
        panic!("entry")
    };
    let ReferenceAssociation::Valid {
        owner: binding_owner,
        ..
    } = &all.references.records[0].association
    else {
        panic!("valid form")
    };
    assert_eq!(owner.as_ref(), binding_owner);
    let mant_ir::ContentReveal::Owner { blocks, .. } = owner.as_ref() else {
        panic!("item")
    };
    assert_eq!(
        blocks,
        &vec![
            mant_ir::ContentBlockStep::Block { index: 0 },
            mant_ir::ContentBlockStep::ListItem { index: 0 },
            mant_ir::ContentBlockStep::Block { index: 1 }
        ]
    );
    assert!(tree(&all).contains("run ↗ run"));
    let selected = build_outline_with_references(
        &query,
        EntryProjection::Summary,
        Some(mant_protocol::ContentSelector::path(path.to_string())),
        &policy,
    )
    .unwrap();
    let OutlineNode::DocumentEntry {
        owner: selected_owner,
        ..
    } = &selected.nodes[0]
    else {
        panic!("selected")
    };
    assert_eq!(owner, selected_owner);
    assert!(tree(&selected).contains("run ↗ run"));
}

#[test]
fn required_owner_fields_reject_legacy_or_incomplete_dtos() {
    let outline = outline(ReferenceProjectionMode::All, EntryProjection::All, 100);
    let mut value = serde_json::to_value(&outline).unwrap();
    value["nodes"][1]["children"][0]
        .as_object_mut()
        .unwrap()
        .remove("owner");
    assert!(serde_json::from_value::<QueryOutline>(value).is_err());
    let mut value = serde_json::to_value(&outline).unwrap();
    value["references"]["records"][2]["association"]
        .as_object_mut()
        .unwrap()
        .remove("owner");
    assert!(serde_json::from_value::<QueryOutline>(value).is_err());
}

#[test]
fn table_cell_and_root_semantic_owner_positions_agree_with_scanner() {
    let mut query = query_markdown_text("# Root\n\n<!-- mant:entries role=command case=sensitive -->\n- [`run`](run.md): Execute.\n", None).unwrap();
    let policy = ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        ..Default::default()
    };
    for table in [false, true] {
        if table {
            let blocks = std::mem::take(&mut query.document.as_mut().unwrap().blocks);
            query.document.as_mut().unwrap().blocks = vec![mant_ir::Block::Table {
                rows: vec![mant_ir::TableRow {
                    cells: vec![mant_ir::TableCell {
                        blocks,
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    }],
                }],
                layout: mant_ir::LayoutHint::default(),
                source: None,
            }];
        }
        let outline =
            build_outline_with_references(&query, EntryProjection::All, None, &policy).unwrap();
        let OutlineNode::DocumentEntry { owner, .. } = &outline.nodes[0].children()[0] else {
            panic!("entry")
        };
        let ReferenceAssociation::Valid { owner: binding, .. } =
            &outline.references.records[0].association
        else {
            panic!("binding")
        };
        assert_eq!(owner.as_ref(), binding);
        assert!(tree(&outline).contains("run ↗ run"));
    }
}

#[test]
fn native_heading_reference_retains_qualified_manual_target() {
    let query = mant_engine::query_roff_bytes(
        b".Dd September 9, 2026\n.Dt PROBE 1\n.Os\n.Sh DESCRIPTION\n.Ss Xr printf 3\nBody.\n",
    )
    .unwrap();
    let outline = build_outline_with_references(
        &query,
        EntryProjection::All,
        None,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(tree(&outline).contains("↗ printf(3)"), "{}", tree(&outline));
}

#[test]
fn multiple_original_forms_and_aliases_keep_one_owner_capability() {
    for second_fragment in ["one", "two"] {
        let source = format!(
            "# Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- [`run`](run.md#one), [`start`](run.md#one) | [`launch`](run.md#{second_fragment}): Execute.\n"
        );
        let query = query_markdown_text(&source, None).unwrap();
        let outline = build_outline_with_references(
            &query,
            EntryProjection::All,
            None,
            &ReferenceProjection {
                mode: ReferenceProjectionMode::All,
                ..Default::default()
            },
        )
        .unwrap();
        let OutlineNode::DocumentEntry { forms, names, .. } = &outline.nodes[0].children()[0]
        else {
            panic!("entry")
        };
        assert_eq!(forms.len(), 2);
        assert_eq!(names, &["run", "start", "launch"]);
        let forms: Vec<_> = outline
            .references
            .records
            .iter()
            .map(|record| match &record.association {
                ReferenceAssociation::Valid { forms, .. } => forms.clone(),
                _ => panic!("valid"),
            })
            .collect();
        assert_eq!(forms, [vec![0], vec![0], vec![1]]);
        let expected = if second_fragment == "one" {
            "↗ run#one"
        } else {
            "↗ 2 targets"
        };
        assert!(tree(&outline).contains(expected), "{}", tree(&outline));
    }
}
