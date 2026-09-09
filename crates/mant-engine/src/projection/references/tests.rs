use super::*;
use mant_ir::{ContentReveal, Inline};
use mant_protocol::{
    LoadedFragment, ReferenceCoverageStatus, ReferenceResolution, UnloadedFragment,
};
use serde_json::{Value, json};

fn document(children: Vec<Value>) -> Document {
    let mut value = json!({"parser":null,"source":{"format":"markdown"},"meta":{},"sections":[],"blocks":[{"type":"paragraph","children":[]}]});
    value["blocks"][0]["children"] = children.into();
    serde_json::from_value(value).unwrap()
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
    assert_eq!(result.records[0].label, "second");
    assert!(matches!(
        result.records[0].origin.resolve_link(&document),
        Some(Inline::Link { .. })
    ));
    let rebuilt: ReferenceInventory =
        serde_json::from_str(&serde_json::to_string(&result).unwrap()).unwrap();
    assert_eq!(result, rebuilt);
    let last = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection { offset: 2, ..all() },
    );
    assert!(
        matches!(&last.records[0].target,LinkTarget::Document{fragment:Some(fragment),..} if fragment=="Mixed.Target")
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
    assert_eq!(result.records[0].label.len(), 4096);
    assert!(result.records[0].label_truncated);
    assert!(result.records[1].label.is_empty());
    assert!(!result.records[1].label_truncated);
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
    let mut document = document(vec![local("entry")]);
    document.blocks.push(serde_json::from_value(json!({"type":"list","kind":{"kind":"bullet"},"items":[{"entry":{"id":"entry","kind":{"kind":"command"},"case":"sensitive","names":[],"forms":[],"valueDomain":null},"blocks":[{"type":"paragraph","children":[{"type":"anchor","id":"entry","fragmentAliases":["Entry.Alias"]}]}]}]})).unwrap());
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
    let query = crate::query_markdown_text(
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
    let document = document((0..10_000).map(|_| link("same", "label")).collect());
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

#[test]
fn snapshot_relative_positions_can_remain_legal_after_an_unrelated_insertion() {
    let before =
        crate::query_markdown_text("# Catalog\n\n## Old\n\n[Old](old.md)\n", None).unwrap();
    let after = crate::query_markdown_text(
        "# Catalog\n\n## Inserted\n\n[New](new.md)\n\n## Old\n\n[Old](old.md)\n",
        None,
    )
    .unwrap();
    let old = project_references(
        before.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &all(),
    );
    let old_record = &old.records[0];
    let target = old_record
        .origin
        .resolve_link(after.document.as_ref().unwrap())
        .unwrap();
    assert!(matches!(target,Inline::Link{target:LinkTarget::Document{name,..},..} if name=="new"));
    let excerpt =
        crate::select_excerpt(&after, std::slice::from_ref(&old_record.source_read)).unwrap();
    assert!(crate::render_excerpt_text(&excerpt).contains("New"));
    assert_eq!(
        old_record.source_read,
        mant_protocol::ContentSelector::path("1")
    );
    // Typed paths describe the actually loaded snapshot, not caller intent in
    // an earlier revision; no expected-snapshot token is promised this release.
}

#[test]
fn offline_reference_rendering_uses_only_serialized_facts_and_preserves_decorated_text() {
    let document = document(vec![
        json!({"type":"link","target":{"kind":"document","name":"target","fragment":"Mixed.Target"},"children":[{"type":"text","value":"label\u{1b}[2J"}]}),
    ]);
    let inventory = project_references(&document, None, ReferenceScope::Document, &all());
    let wire = serde_json::to_vec(&inventory).unwrap();
    drop(inventory);
    drop(document);
    let rebuilt: ReferenceInventory = serde_json::from_slice(&wire).unwrap();
    let plain = mant_protocol::render_reference_inventory(&rebuilt);
    assert!(plain.contains("target#Mixed.Target"));
    assert!(plain.contains("readSource=path:root"));
    assert!(!plain.contains('\u{1b}'));
    let decorated = mant_protocol::render_reference_inventory_with(&rebuilt, |_, text| {
        format!("<paint>{text}</paint>")
    });
    assert_eq!(
        plain,
        decorated.replace("<paint>", "").replace("</paint>", "")
    );
}
