//! Content identity is explicit and local; explain is independent semantic discovery.
use mant_engine::{ProjectionError, build_outline_projection, query_markdown_text, select_excerpt};
use mant_protocol::{ContentSelector, EntryProjection, EvidenceBasis, ExplanationQuery};

#[test]
fn duplicate_section_ids_cannot_redirect_path_selected_entry_metadata() {
    let mut query = query_markdown_text("# Tool\n\n## First\n\n<!-- mant:entries role=command case=sensitive -->\n- `first`: First body.\n\n## Second\n\n<!-- mant:entries role=command case=sensitive -->\n- `second`: Second body.\n", None).unwrap();
    let document = query.document.as_mut().unwrap();
    document.sections[1].id = document.sections[0].id.clone();
    let index = mant_ir::SemanticIndex::build(document);
    assert!(index.section(&document.sections[0].id).is_empty());
    assert_eq!(index.section_at(&[0])[0].names, ["first"]);
    assert_eq!(index.section_at(&[1])[0].names, ["second"]);
    for projection in [
        EntryProjection::All,
        EntryProjection::Kinds {
            kinds: vec![mant_ir::EntryKind::Command],
        },
    ] {
        for (path, name) in [("1", "first"), ("2", "second")] {
            let outline = build_outline_projection(
                &query,
                projection.clone(),
                Some(ContentSelector::path(path)),
            )
            .unwrap();
            let child = &outline.nodes[0].children()[0];
            assert_eq!(child.title(), name);
            assert_eq!(child.path(), format!("{path}/e1"));
            let entry_outline = build_outline_projection(
                &query,
                projection.clone(),
                Some(ContentSelector::path(format!("{path}/e1"))),
            )
            .unwrap();
            assert_eq!(entry_outline.nodes[0].title(), name);
            let excerpt =
                select_excerpt(&query, &[ContentSelector::path(format!("{path}/e1"))]).unwrap();
            assert_eq!(excerpt.selections[0].outline().title(), name);
        }
    }
}

#[test]
fn synthetic_root_ids_do_not_hide_conflicting_public_ir_owners() {
    for (id, path) in [(mant_ir::DOCUMENT_ROOT_ID, "root"), ("tldr", "0")] {
        let mut query = query_markdown_text(
            "# Tool\n\nOverview.\n\n## Conflicting\n\nReal body.\n",
            None,
        )
        .unwrap();
        query.document.as_mut().unwrap().sections[0].id = id.into();
        query.tldr = Some(mant_ir::TldrDocument {
            title: "Tool".into(),
            description: vec![],
            more_information: None,
            examples: vec![],
            platform: "common".into(),
            language: "en".into(),
            source_path: String::new(),
            origin: mant_ir::TldrOrigin::Embedded,
        });
        let selected = ContentSelector::id(id);
        assert!(
            matches!(select_excerpt(&query, std::slice::from_ref(&selected)), Err(ProjectionError::AmbiguousSelector { candidates, .. }) if candidates.len() == 2)
        );
        assert!(
            matches!(build_outline_projection(&query, EntryProjection::Summary, Some(selected)), Err(ProjectionError::AmbiguousSelector { candidates, .. }) if candidates.len() == 2)
        );
        assert_eq!(
            select_excerpt(&query, &[ContentSelector::path(path)])
                .unwrap()
                .selections[0]
                .outline()
                .path(),
            path
        );
        assert_eq!(
            build_outline_projection(
                &query,
                EntryProjection::Summary,
                Some(ContentSelector::path(path))
            )
            .unwrap()
            .nodes[0]
                .path(),
            path
        );
        assert_eq!(
            select_excerpt(&query, &[ContentSelector::path("1")])
                .unwrap()
                .selections[0]
                .outline()
                .path(),
            "1"
        );
    }
}

#[test]
fn namespaces_do_not_fall_through_and_duplicate_ids_remain_readable_by_path() {
    let mut query = query_markdown_text(
        "# Title\n\nOverview.\n\n## First\n\nOne.\n\n## Second\n\nTwo.\n",
        None,
    )
    .unwrap();
    let document = query.document.as_mut().unwrap();
    document.sections[0].id = "root".into();
    document.sections[1].id = "root".into();
    let overview = select_excerpt(&query, &[ContentSelector::path("root")]).unwrap();
    assert_eq!(overview.selections[0].outline().path(), "root");
    assert!(matches!(
        select_excerpt(&query, &[ContentSelector::id("root")]),
        Err(ProjectionError::AmbiguousSelector { .. })
    ));
    let both = select_excerpt(
        &query,
        &[ContentSelector::path("1"), ContentSelector::path("2")],
    )
    .unwrap();
    assert_eq!(
        both.selections.len(),
        2,
        "paths must not be deduplicated by duplicate IDs"
    );
    assert!(matches!(
        build_outline_projection(
            &query,
            EntryProjection::None,
            Some(ContentSelector::id("root"))
        ),
        Err(ProjectionError::AmbiguousSelector { .. })
    ));
    assert_eq!(
        build_outline_projection(
            &query,
            EntryProjection::None,
            Some(ContentSelector::path("2"))
        )
        .unwrap()
        .nodes[0]
            .path(),
        "2"
    );
}

#[test]
fn names_remain_multiple_explain_evidence_not_content_selectors() {
    let query = query_markdown_text("# Tool\n\n## force\n\nSection.\n\n## Commands\n\n<!-- mant:entries role=command case=sensitive -->\n- `force`: First.\n- `force`: Second.\n", None).unwrap();
    assert!(query.document.as_ref().unwrap().diagnostics.is_empty());
    assert_eq!(
        select_excerpt(&query, &[ContentSelector::id("force")])
            .unwrap()
            .selections[0]
            .outline()
            .path(),
        "1"
    );
    assert!("force".parse::<ContentSelector>().is_err());
    let evidence = mant_engine::explain_query(
        &query,
        &ExplanationQuery {
            entry: "force".into(),
            options: mant_protocol::ExplanationOptions::default(),
        },
    )
    .unwrap();
    let named = evidence
        .evidence
        .iter()
        .filter(|entry| {
            entry
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        })
        .collect::<Vec<_>>();
    assert_eq!(named.len(), 2);
    for entry in named {
        assert_eq!(
            select_excerpt(&query, &[ContentSelector::path(entry.outline.path())])
                .unwrap()
                .selections
                .len(),
            1
        );
    }
}

#[test]
fn direct_public_apis_reject_malformed_and_oversized_values_before_selection() {
    let query = query_markdown_text("# Tool\n", None).unwrap();
    for selector in [
        ContentSelector::path(""),
        ContentSelector::path(" 1"),
        ContentSelector::path("01"),
        ContentSelector::id("Mixed.ID"),
        ContentSelector::id("x\n"),
        ContentSelector::id("x".repeat(513)),
    ] {
        assert_eq!(
            select_excerpt(&query, std::slice::from_ref(&selector)).unwrap_err(),
            ProjectionError::InvalidSelector
        );
        assert_eq!(
            build_outline_projection(&query, EntryProjection::Summary, Some(selector)).unwrap_err(),
            ProjectionError::InvalidSelector
        );
    }
    assert!(matches!(
        select_excerpt(&query, &vec![ContentSelector::path("root"); 17]),
        Err(ProjectionError::TooManySelections { maximum: 16 })
    ));
}

#[test]
fn compact_summary_matches_materialized_counts_without_copying_invalid_forms() {
    let mut query = query_markdown_text("# Tool\n\n<!-- mant:entries role=command case=sensitive -->\n- `run`: Main.\n\n  <!-- mant:entries role=option case=sensitive -->\n  - `--help`: Help.\n\n## More\n\n<!-- mant:entries role=command case=sensitive -->\n- `last`: End.\n", None).unwrap();
    for invalid in [false, true] {
        if invalid {
            let mant_ir::Block::List { items, .. } =
                &mut query.document.as_mut().unwrap().blocks[0]
            else {
                panic!("list")
            };
            items[0].entry.as_mut().unwrap().forms[0].parts[0].path = vec![999];
        }
        let summary = build_outline_projection(&query, EntryProjection::Summary, None).unwrap();
        let all = build_outline_projection(&query, EntryProjection::All, None).unwrap();
        for (compact, expanded) in summary.nodes.iter().zip(&all.nodes) {
            let get_summary = |node: &mant_protocol::OutlineNode| match node {
                mant_protocol::OutlineNode::DocumentRoot { entry_summary, .. }
                | mant_protocol::OutlineNode::DocumentSection { entry_summary, .. } => {
                    entry_summary.clone()
                }
                _ => None,
            };
            assert_eq!(get_summary(compact), get_summary(expanded));
        }
    }
}

#[test]
fn rooted_references_use_original_owner_coordinates_independent_of_entry_visibility() {
    let query = query_markdown_text("# [Title](title.md)\n\n[Overview](overview.md).\n\n## Group\n\n- Transparent container.\n\n  <!-- mant:entries role=command case=sensitive -->\n  - [`run`](run.md): [Body](body.md).\n\n    <!-- mant:entries role=option case=sensitive -->\n    - `--help`: [Help](help.md).\n\n  - `other`: [Sibling](sibling.md).\n", None).unwrap();
    let policy = mant_protocol::ReferenceProjection {
        mode: mant_protocol::ReferenceProjectionMode::All,
        ..Default::default()
    };
    for entries in [
        EntryProjection::None,
        EntryProjection::Summary,
        EntryProjection::All,
    ] {
        let outline = mant_engine::build_outline_with_references(
            &query,
            entries,
            Some(ContentSelector::path("1/e1")),
            &policy,
        )
        .unwrap();
        let names = outline
            .references
            .records
            .iter()
            .map(|record| match &record.target {
                mant_ir::LinkTarget::Document { name, .. } => name.as_str(),
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["run", "body", "help"]);
        for record in &outline.references.records {
            assert!(
                record
                    .origin
                    .resolve(query.document.as_ref().unwrap())
                    .is_some()
            );
        }
    }
    let overview = mant_engine::build_outline_with_references(
        &query,
        EntryProjection::None,
        Some(ContentSelector::path("root")),
        &policy,
    )
    .unwrap();
    assert_eq!(overview.references.records.len(), 2);
}
