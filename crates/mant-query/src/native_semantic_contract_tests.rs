use mant_ir::{Block, Document, FragmentAlias, Inline, LayoutHint, NodeId, ResolvedContent};
use mant_protocol::{ContentSelector, EvidenceBasis, ExcerptSelection};

fn content() -> ResolvedContent {
    let document =
        serde_json::from_str::<Document>(include_str!("fixtures/NATIVE_SEMANTIC_ENTRY.json"))
            .expect("checked-in native semantic contract");
    assert!(mant_ir::validate_document(&document).is_empty());
    ResolvedContent {
        label: "probe(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    }
}

fn content_with_repeated_name() -> ResolvedContent {
    let mut content = content();
    let document = content.document.as_mut().unwrap();
    let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
        panic!("checked-in native definition list")
    };
    let mut duplicate = items[0].clone();
    duplicate.entry.as_mut().unwrap().id = NodeId::new("command-git-add-2");
    let mut anchors = duplicate.terms[0]
        .iter_mut()
        .filter_map(|inline| match inline {
            Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } => Some((id, fragment_aliases)),
            _ => None,
        });
    *anchors.next().unwrap().0 = NodeId::new("command-git-add-2");
    let (id, aliases) = anchors.next().unwrap();
    *id = NodeId::new("target-git-add-2");
    *aliases = vec![FragmentAlias::from("git-add~2")];
    items.push(duplicate);
    let diagnostics = mant_ir::validate_document(document);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    content
}

fn content_with_nested_native_owner() -> ResolvedContent {
    let mut content = content();
    let document = content.document.as_mut().unwrap();
    let Block::DefinitionList { items, .. } = &mut document.sections[1].blocks[0] else {
        panic!("checked-in native definition list")
    };
    let mut child = items[0].clone();
    child.entry.as_mut().unwrap().id = NodeId::new("command-git-add-child");
    let mut anchors = child.terms[0].iter_mut().filter_map(|inline| match inline {
        Inline::Anchor {
            id,
            fragment_aliases,
            ..
        } => Some((id, fragment_aliases)),
        _ => None,
    });
    *anchors.next().unwrap().0 = NodeId::new("command-git-add-child");
    let (id, aliases) = anchors.next().unwrap();
    *id = NodeId::new("target-git-add-child");
    *aliases = vec![FragmentAlias::from("git-add~2")];
    items[0].description.push(Block::DefinitionList {
        items: vec![child],
        declaration_groups: Vec::new(),
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    });
    let diagnostics = mant_ir::validate_document(document);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    content
}

#[test]
fn staged_native_entry_round_trips_through_strict_read_and_explain() {
    let content = content();
    let excerpt = crate::select_excerpt(&content, &[ContentSelector::id("command-git-add")])
        .expect("stable entry ID is a strict read selector");
    assert!(matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::DocumentEntry { outline, .. }]
            if outline.path() == "2/e1"
    ));
    assert!(
        crate::select_excerpt(&content, &[ContentSelector::id("git-add")]).is_err(),
        "a semantic name is not silently accepted as a read selector"
    );

    let by_name = crate::select_explanation(&content, "git-add").unwrap();
    assert_eq!((by_name.total, by_name.returned), (1, 1));
    assert!(
        by_name.evidence[0]
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    );
    assert_eq!(
        by_name.evidence[0].entry.as_ref().unwrap().names,
        ["git-add"]
    );

    for identity in ["command-git-add", "2/e1"] {
        let response = crate::select_explanation(&content, identity).unwrap();
        assert_eq!(response.returned, 1);
        assert!(
            response.evidence[0]
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Identity { .. }))
        );
    }
}

#[test]
fn repeated_native_name_remains_ambiguous_after_ir_round_trip() {
    let content = content_with_repeated_name();
    assert!(crate::select_excerpt(&content, &[ContentSelector::id("git-add")]).is_err());
    let response = crate::select_explanation(&content, "git-add").unwrap();
    assert_eq!((response.total, response.returned), (2, 2));
    let mut ids = response
        .evidence
        .iter()
        .map(|evidence| evidence.outline.node.id())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, ["command-git-add", "command-git-add-2"]);
}

#[test]
fn nested_native_owner_keeps_strict_id_and_structural_path_selectors() {
    let content = content_with_nested_native_owner();
    for (query, selector) in [
        (
            "command-git-add-child",
            ContentSelector::id("command-git-add-child"),
        ),
        ("2/e1/e1", ContentSelector::path("2/e1/e1")),
    ] {
        let excerpt = crate::select_excerpt(&content, &[selector])
            .expect("nested native identity remains a strict read selector");
        assert!(matches!(
            excerpt.selections.as_slice(),
            [ExcerptSelection::DocumentEntry { outline, .. }]
                if outline.path() == "2/e1/e1"
        ));
        let explanation = crate::select_explanation(&content, query).unwrap();
        assert_eq!((explanation.total, explanation.returned), (1, 1));
        assert!(
            explanation.evidence[0]
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Identity { .. }))
        );
    }
}
