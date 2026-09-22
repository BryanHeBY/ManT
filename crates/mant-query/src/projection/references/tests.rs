use super::*;
use mant_ir::{
    ContentOwnerKind, ContentReveal, ContentRootKind, ContentStoreBuilder, ContentStyle,
    FragmentAlias, Inline, LinkOccurrenceKey, PointBoundary, Provenance,
};
use mant_protocol::{
    LoadedFragment, ReferenceCoverageStatus, ReferenceResolution, UnloadedFragment,
};
use serde_json::{Value, json};

fn document(children: Vec<Value>) -> Document {
    fn lower(
        builder: &mut ContentStoreBuilder,
        owner: mant_ir::ContentOwnerKey,
        root: mant_ir::ContentRootKey,
        value: &Value,
        active_link: Option<LinkOccurrenceKey>,
    ) -> Inline {
        let kind = value["type"].as_str().unwrap();
        match kind {
            "text" => {
                let content = builder.push_text(
                    root,
                    value["value"].as_str().unwrap().to_owned(),
                    None,
                    ContentStyle::default(),
                    None,
                    active_link,
                    Provenance::Unknown,
                );
                Inline::Text { content }
            }
            "link" => {
                let target = serde_json::from_value(value["target"].clone()).unwrap();
                let title = value["title"].as_str().map(ToOwned::to_owned);
                let occurrence = builder.push_link(owner, target, title, Provenance::Unknown);
                let children = value["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|child| lower(builder, owner, root, child, Some(occurrence)))
                    .collect();
                Inline::Link {
                    occurrence,
                    children,
                }
            }
            "anchor" => {
                let atom_boundary =
                    u32::try_from(builder.content_store().root(root).unwrap().atoms.len()).unwrap();
                let scalar_boundary = u32::try_from(
                    builder
                        .content_store()
                        .root_logical_text(root)
                        .unwrap()
                        .chars()
                        .count(),
                )
                .unwrap();
                let point = builder.push_point(
                    root,
                    PointBoundary::BetweenAtoms { atom_boundary },
                    scalar_boundary,
                    Provenance::Unknown,
                );
                let aliases = value
                    .get("fragmentAliases")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|alias| FragmentAlias::from(alias.as_str().unwrap()))
                    .collect();
                Inline::anchor_with_aliases(point, value["id"].as_str().unwrap(), aliases)
            }
            _ => panic!("unsupported reference fixture inline {kind}"),
        }
    }

    let mut builder = ContentStoreBuilder::new();
    let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
    let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
    let children = children
        .into_iter()
        .map(|child| lower(&mut builder, owner, root, &child, None))
        .collect();
    Document {
        parser: None,
        sources: vec![mant_ir::SourceRecord {
            key: mant_ir::SourceKey::FIRST,
            identity: mant_ir::SourceIdentity::Anonymous {
                name: "test".to_owned(),
            },
            format: mant_ir::SourceFormat::Markdown,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: mant_ir::SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: mant_ir::SourceKey::FIRST,
        content_store: builder.finish(),
        meta: mant_ir::DocumentMeta::default(),
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: vec![mant_ir::Block::Paragraph {
            children,
            layout: mant_ir::LayoutHint::default(),
            source: None,
        }],
        sections: Vec::new(),
    }
}
fn link(name: &str, label: &str) -> Value {
    json!({"type":"link","target":{"kind":"document","name":name},"children":[{"type":"text","value":label}]})
}
fn local(name: &str) -> Value {
    json!({"type":"link","target":{"kind":"section","id":name},"children":[]})
}
fn all() -> ReferenceProjection {
    ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        target_types: vec![
            ReferenceTargetType::Document,
            ReferenceTargetType::Manual,
            ReferenceTargetType::Local,
            ReferenceTargetType::External,
            ReferenceTargetType::Email,
        ],
        ..Default::default()
    }
}

fn separate_root_links() -> Document {
    let mut document = document(Vec::new());
    let mut builder = ContentStoreBuilder::new();
    let owner = builder.push_owner(ContentOwnerKind::Document, Provenance::Unknown);
    let mut blocks = Vec::new();
    for (name, label) in [("first", "A".to_owned()), ("second", "X".repeat(800))] {
        let root = builder.push_root(owner, ContentRootKind::Body, Provenance::Unknown);
        let occurrence = builder.push_link(
            owner,
            LinkTarget::Document {
                name: name.to_owned(),
                fragment: None,
            },
            None,
            Provenance::Unknown,
        );
        let content = builder.push_text(
            root,
            label,
            None,
            ContentStyle::default(),
            None,
            Some(occurrence),
            Provenance::Unknown,
        );
        blocks.push(mant_ir::Block::Paragraph {
            children: vec![Inline::Link {
                occurrence,
                children: vec![Inline::Text { content }],
            }],
            layout: mant_ir::LayoutHint::default(),
            source: None,
        });
    }
    document.content_store = builder.finish();
    document.blocks = blocks;
    document
}

#[test]
fn projection_budget_preserves_closed_prefix_and_advances_page() {
    let document = separate_root_links();
    let mut first_page = None;
    for bytes in (256..=8192).step_by(64) {
        let result = project_references_with_limits(
            &document,
            None,
            ReferenceScope::Document,
            &all(),
            ReferenceProjectionLimits {
                materialization_bytes: bytes,
                ..Default::default()
            },
        );
        if result.page.returned == 1
            && result.page.limited == Some(ReferencePageLimit::MaterializationBytes)
            && result.page.next_offset == Some(1)
        {
            first_page = Some(result);
            break;
        }
    }
    let first_page = first_page.expect("the first closed link fits before the second root");
    assert_eq!(first_page.occurrences, ReferenceCount::Exact { value: 2 });
    assert_eq!(first_page.records[0].label_preview, "A");
    assert_eq!(
        first_page
            .content_projection
            .as_ref()
            .unwrap()
            .content()
            .occurrence_plain_text(first_page.records[0].occurrence)
            .unwrap(),
        "A"
    );
    let decoded: ReferenceInventory =
        serde_json::from_str(&serde_json::to_string(&first_page).unwrap()).unwrap();
    assert_eq!(decoded, first_page);
    let second_page = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection { offset: 1, ..all() },
    );
    assert_eq!(second_page.page.returned, 1);
    assert_eq!(second_page.records[0].label_preview, "X".repeat(800));
}

#[test]
fn summary_does_not_clone_labels_and_filters_before_target_bytes() {
    let document = document(vec![
        link("same", &"é".repeat(100_000)),
        json!({"type":"link","target":{"kind":"external","uri":format!("https://{}","x".repeat(100_000))},"children":[]}),
        link("same", ""),
    ]);
    let result = project_references_with_limits(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection::default(),
        ReferenceProjectionLimits {
            scan: ReferenceScanLimits {
                bytes: 136,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 2 });
    assert_eq!(result.targets, ReferenceCount::Exact { value: 1 });
    assert!(result.records.is_empty());
    assert_eq!(result.coverage.bytes, 136);
    let disabled = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::None,
            ..Default::default()
        },
    );
    assert_eq!(disabled.coverage.steps, 0);
    assert!(matches!(
        disabled.occurrences,
        ReferenceCount::Unknown { .. }
    ));
}

#[test]
fn occurrence_paging_keeps_duplicates_exact_positions_and_target_fragments() {
    let document = document(vec![
        link("same", "first"),
        link("same", "second"),
        json!({"type":"link","target":{"kind":"document","name":"same","fragment":"Mixed.Target"},"children":[]}),
    ]);
    let result = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            offset: 1,
            limit: 1,
            ..all()
        },
    );
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 3 });
    assert_eq!(result.targets, ReferenceCount::Exact { value: 2 });
    assert_eq!(result.page.returned, 1);
    assert_eq!(result.page.next_offset, Some(2));
    assert_eq!(result.records[0].label_preview, "second");
    let Some(Inline::Link {
        occurrence: original_key,
        ..
    }) = result.records[0].origin.resolve_link(&document)
    else {
        panic!("origin must resolve to the original link wrapper");
    };
    let projected_key = result.records[0].occurrence;
    // The original and response-local keys inhabit distinct domains even
    // when dense remapping happens to assign the same numeric value.
    let original = document.content_store.link(*original_key).unwrap();
    let projected = result
        .content_projection
        .as_ref()
        .unwrap()
        .content_store
        .link(projected_key)
        .unwrap();
    assert_eq!(original.target, projected.target);
    assert_eq!(original.title, projected.title);
    assert_eq!(
        document
            .content()
            .occurrence_plain_text(*original_key)
            .unwrap(),
        result
            .content_projection
            .as_ref()
            .unwrap()
            .content()
            .occurrence_plain_text(projected_key)
            .unwrap(),
    );
    let rebuilt: ReferenceInventory =
        serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    assert_eq!(result, rebuilt);
    let last = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection { offset: 2, ..all() },
    );
    let last: ReferenceInventory =
        serde_json::from_str(&serde_json::to_string(&last).unwrap()).unwrap();
    let occurrence = last
        .content_projection
        .as_ref()
        .unwrap()
        .content_store
        .link(last.records[0].occurrence)
        .unwrap();
    assert!(
        matches!(&occurrence.target,LinkTarget::Document{fragment:Some(fragment),..} if fragment=="Mixed.Target")
    );
}

#[test]
fn distinct_limits_and_return_limits_do_not_lie_about_scan_counts() {
    let document = document(vec![link("a", "A"), link("b", "B"), link("c", "C")]);
    let result = project_references_with_limits(
        &document,
        None,
        ReferenceScope::Document,
        &all(),
        ReferenceProjectionLimits {
            distinct_targets: 1,
            materialization_bytes: 0,
            ..Default::default()
        },
    );
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 3 });
    assert_eq!(result.targets, ReferenceCount::LowerBound { value: 1 });
    assert!(matches!(
        result.coverage.status,
        ReferenceCoverageStatus::Complete {}
    ));
    assert_eq!(
        result.page.limited,
        Some(ReferencePageLimit::MaterializationBytes)
    );
    assert_eq!(result.page.next_offset, None);
    let result = project_references_with_limits(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            offset: u32::MAX,
            ..all()
        },
        ReferenceProjectionLimits {
            scan: ReferenceScanLimits {
                steps: 5,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert!(matches!(
        result.occurrences,
        ReferenceCount::LowerBound { .. }
    ));
    assert_eq!(result.page.next_offset, None);
    assert!(result.records.is_empty());
}

#[test]
fn labels_are_utf8_bounded_and_empty_labels_are_not_replaced() {
    let document = document(vec![link("a", &"é".repeat(3000)), link("b", "")]);
    let result = project_references(&document, None, ReferenceScope::Document, &all());
    assert_eq!(result.records[0].label_preview.len(), 4096);
    assert!(result.records[0].label_preview_truncated);
    assert!(result.records[1].label_preview.is_empty());
    assert!(!result.records[1].label_preview_truncated);
}

#[test]
fn local_resolution_checks_exact_anchors_and_duplicate_logical_locations() {
    let document = document(vec![
        local("Mixed.Target"),
        local("missing"),
        local("duplicate"),
        json!({"type":"anchor","id":"mixed-target","fragmentAliases":["Mixed.Target"]}),
        json!({"type":"anchor","id":"duplicate"}),
        json!({"type":"anchor","id":"duplicate"}),
    ]);
    let result = project_references(&document, None, ReferenceScope::Document, &all());
    assert!(
        matches!(&result.records[0].resolution,ReferenceResolution::Loaded{fragment:LoadedFragment::Valid{reveal:ContentReveal::Inline{location}},..} if matches!(location.resolve(&document),Some([Inline::Anchor{id,..}]) if id.as_str()=="mixed-target"))
    );
    assert!(matches!(
        result.records[1].resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Missing {},
            ..
        }
    ));
    assert!(matches!(
        result.records[2].resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Ambiguous {},
            ..
        }
    ));
    assert!(result.target_coverage.is_some());
}

#[test]
fn entry_anchor_sharing_identity_is_one_logical_destination_not_ambiguity() {
    let mut document = document(vec![
        local("entry"),
        json!({"type":"anchor","id":"entry","fragmentAliases":["Entry.Alias"]}),
    ]);
    let anchor = match &mut document.blocks[0] {
        mant_ir::Block::Paragraph { children, .. } => children.pop().unwrap(),
        _ => unreachable!(),
    };
    document.blocks.push(mant_ir::Block::List {
        kind: mant_ir::ListKind::Bullet,
        compact: false,
        items: vec![mant_ir::ListItem {
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            entry: Some(mant_ir::EntryFacts {
                name_bindings: Vec::new(),
                alias_groups: Vec::new(),
                alias_of: None,
                forms: Vec::new(),
                id: "entry".into(),
                kind: mant_ir::EntryKind::Command,
                case: mant_ir::NameCase::Sensitive,
                names: Vec::new(),
                value_domain: None,
            }),
            blocks: vec![mant_ir::Block::Paragraph {
                children: vec![anchor],
                layout: mant_ir::LayoutHint::default(),
                source: None,
            }],
        }],
        layout: mant_ir::LayoutHint::default(),
        source: None,
    });
    let result = project_references(&document, None, ReferenceScope::Document, &all());
    assert!(matches!(
        result.records[0].resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Valid {
                reveal: ContentReveal::Owner { item_index: 0, .. }
            },
            ..
        }
    ));
}

#[test]
fn section_filter_and_read_selector_remain_source_local() {
    let query = crate::query_fixture::markdown(
        "# [Catalog](catalog.md)\n\n## [One](one.md)\n\n[Body](body.md)\n\n## [Two](two.md)\n",
        None,
    )
    .unwrap();
    let document = query.document.as_ref().unwrap();
    let result = project_references(document, None, ReferenceScope::Section(&[0]), &all());
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 2 });
    assert!(
        result
            .records
            .iter()
            .all(|record| record.source_read == mant_protocol::ContentSelector::path("1"))
    );
    let overview = project_references(document, None, ReferenceScope::Overview, &all());
    assert_eq!(overview.occurrences, ReferenceCount::Exact { value: 1 });
    assert_eq!(
        overview.records[0].source_read,
        mant_protocol::ContentSelector::path("root")
    );
}

#[test]
fn cross_document_address_never_claims_unloaded_fragment_validity() {
    let document = document(vec![
        json!({"type":"link","target":{"kind":"document","name":"other","fragment":"target"},"children":[]}),
        json!({"type":"link","target":{"kind":"document","name":"source","fragment":"target"},"children":[]}),
        json!({"type":"link","target":{"kind":"manual","name":"printf"},"children":[]}),
        json!({"type":"link","target":{"kind":"external","uri":"https://example.test"},"children":[]}),
        json!({"type":"anchor","id":"target"}),
    ]);
    let address = DocumentAddress::parse_catalog_path("documents/dir/source").unwrap();
    let result = project_references(&document, Some(&address), ReferenceScope::Document, &all());
    assert!(
        matches!(&result.records[0].resolution,ReferenceResolution::LogicalAddress{address,fragment:UnloadedFragment::Unchecked{}} if address.catalog_path()=="documents/dir/other")
    );
    assert!(matches!(
        result.records[1].resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Valid { .. },
            ..
        }
    ));
    assert!(matches!(
        result.records[2].resolution,
        ReferenceResolution::NotQueried { .. }
    ));
    assert!(matches!(
        result.records[3].resolution,
        ReferenceResolution::NotApplicable {}
    ));
    let direct = project_references(&document, None, ReferenceScope::Document, &all());
    assert!(matches!(
        direct.records[0].resolution,
        ReferenceResolution::MissingContext { .. }
    ));
}

#[test]
fn local_validation_exhaustion_is_not_missing_and_does_not_change_exact_inventory() {
    let document = document(vec![
        local("target"),
        json!({"type":"anchor","id":"target"}),
    ]);
    let baseline = project_references(&document, None, ReferenceScope::Document, &all());
    let result = project_references_with_limits(
        &document,
        None,
        ReferenceScope::Document,
        &all(),
        ReferenceProjectionLimits {
            scan: ReferenceScanLimits {
                steps: baseline.coverage.steps as usize,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 1 });
    assert!(matches!(
        result.records[0].resolution,
        ReferenceResolution::Loaded {
            fragment: LoadedFragment::Limited {},
            ..
        }
    ));
    assert!(matches!(
        result.target_coverage.unwrap().status,
        ReferenceCoverageStatus::Limited { .. }
    ));
}

#[test]
fn hard_distinct_cap_freezes_a_proven_lower_bound_while_scan_finishes() {
    let document = document(
        (0..5000)
            .map(|index| link(&format!("target-{index}"), "label"))
            .collect(),
    );
    let result = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection::default(),
    );
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 5000 });
    assert_eq!(result.targets, ReferenceCount::LowerBound { value: 4096 });
    assert!(result.records.is_empty());
}

#[test]
fn repeated_targets_page_by_occurrence_and_large_offset_does_not_allocate_records() {
    let source = "[label](same.md)\n\n".repeat(10_000);
    let document = crate::query_fixture::markdown(&source, None)
        .unwrap()
        .document
        .unwrap();
    let result = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            offset: 9999,
            ..all()
        },
    );
    assert_eq!(result.occurrences, ReferenceCount::Exact { value: 10_000 });
    assert_eq!(result.targets, ReferenceCount::Exact { value: 1 });
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.page.next_offset, None);
    let limited = project_references_with_limits(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            offset: 9999,
            ..all()
        },
        ReferenceProjectionLimits {
            scan: ReferenceScanLimits {
                steps: 100,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert!(limited.records.is_empty());
    assert!(matches!(
        limited.occurrences,
        ReferenceCount::LowerBound { .. }
    ));
    assert_eq!(limited.page.next_offset, None);
}
