//! Snapshot ownership and cross-phase budget regressions for the public scanner.
use super::*;
use crate::{ContentBlockStep as Step, Document, EntryOwnerLocationRef, Inline, LinkTarget};
use serde_json::{Value, json};
use std::ops::ControlFlow;

fn link(name: &str) -> Value {
    json!({"type":"link","target":{"kind":"document","name":name},"children":[{"type":"text","value":name}]})
}
fn paragraph(children: Vec<Value>) -> Value {
    let mut value = json!({"type":"paragraph"});
    value["children"] = children.into();
    value
}
fn document(blocks: Vec<Value>, sections: Vec<Value>) -> Document {
    let mut value = json!({"parser":null,"source":{"format":"markdown"},"meta":{}});
    value["blocks"] = blocks.into();
    value["sections"] = sections.into();
    serde_json::from_value(value).unwrap()
}
fn facts(id: &str) -> Value {
    json!({"id":id,"kind":{"kind":"term"},"case":"sensitive","names":[],"forms":[],"valueDomain":null})
}

#[test]
fn all_content_roots_and_repeated_links_rebuild_after_serde() {
    let mut document = document(
        vec![
            paragraph(vec![link("root"), link("same"), link("same")]),
            json!({"type":"preformatted","children":[link("pre")]}),
            json!({"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[paragraph(vec![link("item")])]}]}),
            json!({"type":"definition-list","items":[{"entry":null,"terms":[[link("term")]],"description":[paragraph(vec![link("description")])]}]}),
            json!({"type":"table","rows":[{"cells":[{"blocks":[paragraph(vec![link("cell")])]}]}]}),
        ],
        vec![
            json!({"id":"section","heading":{"content":[link("heading")]},"blocks":[paragraph(vec![link("section")])],"children":[
                {"id":"child","heading":{"content":[link("child-heading")]},"blocks":[],"children":[]}
            ]}),
        ],
    );
    document.heading = Some(crate::Heading {
        content: vec![serde_json::from_value(link("document-heading")).unwrap()],
        source: None,
    });
    let collect = |document: &Document| {
        let mut locations = Vec::new();
        let report = scan_references(document, ReferenceScanLimits::default(), |occurrence| {
            let location = occurrence.location.to_owned().unwrap();
            assert!(std::ptr::eq(
                location.resolve_link(document).unwrap(),
                occurrence.link
            ));
            locations.push((location, occurrence.target.clone()));
            ControlFlow::Continue(())
        });
        assert!(report.complete(), "{report:?}");
        assert_eq!(report.occurrences, locations.len());
        locations
    };
    let expected = collect(&document);
    assert_eq!(expected.len(), 12);
    assert_ne!(expected[2].0, expected[3].0);
    assert_eq!(expected[2].1, expected[3].1);
    let reparsed = serde_json::from_slice(&serde_json::to_vec(&document).unwrap()).unwrap();
    assert_eq!(collect(&reparsed), expected);
    let mut section_names = Vec::new();
    let report = scan_section_references(
        &document,
        &[0],
        ReferenceScanLimits::default(),
        |occurrence| {
            section_names.push(occurrence.target.clone());
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    assert_eq!(section_names.len(), 3);
    assert_eq!(
        scan_section_references(
            &document,
            &[99],
            ReferenceScanLimits::default(),
            |_| unreachable!()
        )
        .stopped,
        Some(ReferenceScanStop::InvalidRoot)
    );
}

#[test]
fn transparent_items_keep_nearest_semantic_owner_without_copying_forms() {
    let document = document(
        vec![json!({"type":"list","kind":{"kind":"bullet"},"items":[{
            "entry":facts("outer"),"blocks":[
                paragraph(vec![link("outer")]),
                {"type":"table","rows":[{"cells":[{"blocks":[
                    {"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[paragraph(vec![link("transparent")])]}]},
                    {"type":"definition-list","items":[{"entry":facts("child"),"terms":[[link("child")]],"description":[]}]}
                ]}]}]}
            ]
        }]})],
        vec![],
    );
    let mut names = Vec::new();
    let report = scan_references(&document, ReferenceScanLimits::default(), |occurrence| {
        let semantic = occurrence.semantic_owner.unwrap();
        let resolved = semantic.location.resolve(&document).unwrap();
        assert_eq!(
            resolved.facts().unwrap().id,
            semantic.owner.facts().unwrap().id
        );
        names.push(semantic.owner.facts().unwrap().id.to_string());
        if names.len() == 2 {
            assert!(occurrence.content_owner.unwrap().owner.facts().is_none());
        }
        ControlFlow::Continue(())
    });
    assert!(report.complete());
    assert_eq!(names, ["outer", "outer", "child"]);
}

#[test]
fn nested_owner_frames_restore_both_axes_before_visiting_siblings() {
    let document = document(
        vec![
            json!({"type":"list","kind":{"kind":"bullet"},"items":[
                {"entry":facts("outer"),"blocks":[
                    paragraph(vec![link("before")]),
                    {"type":"list","kind":{"kind":"bullet"},"items":[
                        {"entry":facts("child"),"blocks":[paragraph(vec![link("child")])]},
                        {"blocks":[paragraph(vec![link("transparent")])]}
                    ]},
                    paragraph(vec![link("after")])
                ]},
                {"blocks":[paragraph(vec![link("sibling")])]}
            ]}),
            paragraph(vec![link("outside")]),
        ],
        vec![],
    );
    let mut owners = Vec::new();
    let report = scan_references(&document, ReferenceScanLimits::default(), |occurrence| {
        let read = |owner: Option<ReferenceOwnerRef<'_, '_>>| {
            owner.map(|owner| {
                let resolved = owner.location.resolve(&document).unwrap();
                assert_eq!(resolved.facts(), owner.owner.facts());
                (
                    owner.owner.facts().map(|facts| facts.id.to_string()),
                    owner.location.item_index,
                )
            })
        };
        owners.push((
            read(occurrence.content_owner),
            read(occurrence.semantic_owner),
        ));
        ControlFlow::Continue(())
    });
    let owner = |id: &str, index| Some((Some(id.to_owned()), index));
    assert!(report.complete());
    assert_eq!(report.occurrences, 6);
    assert_eq!(
        owners,
        [
            (owner("outer", 0), owner("outer", 0)),
            (owner("child", 0), owner("child", 0)),
            (Some((None, 1)), owner("outer", 0)),
            (owner("outer", 0), owner("outer", 0)),
            (Some((None, 1)), None),
            (None, None),
        ]
    );
}

#[test]
fn explicit_owner_and_block_roots_preserve_ancestor_context_and_exclude_siblings() {
    let document = document(
        vec![json!({"type":"list","kind":{"kind":"bullet"},"items":[
            {"entry":facts("outer"),"blocks":[{"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[paragraph(vec![link("nested")])]}]}]},
            {"blocks":[paragraph(vec![link("sibling")])]}
        ]})],
        vec![
            json!({"id":"section","heading":{"content":[]},"blocks":[paragraph(vec![link("excluded-section")])],"children":[]}),
        ],
    );
    let mut count = 0;
    let report = scan_reference_scope(
        &document,
        ReferenceScope::Overview,
        ReferenceScanLimits::default(),
        |_, _| {
            count += 1;
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    assert_eq!(count, 2);
    let path = [
        Step::Block { index: 0 },
        Step::ListItem { index: 0 },
        Step::Block { index: 0 },
    ];
    let owner = EntryOwnerLocationRef {
        sections: &[],
        blocks: &path,
        item_index: 0,
    };
    let report = scan_owner_references(
        &document,
        owner,
        ReferenceScanLimits::default(),
        |occurrence| {
            assert_eq!(
                occurrence
                    .semantic_owner
                    .unwrap()
                    .owner
                    .facts()
                    .unwrap()
                    .id
                    .as_str(),
                "outer"
            );
            assert!(occurrence.content_owner.unwrap().owner.facts().is_none());
            assert!(std::ptr::eq(
                occurrence.location.resolve_link(&document).unwrap(),
                occurrence.link
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    assert_eq!(report.occurrences, 1);
    let report = scan_block_references(
        &document,
        &[],
        &path,
        ReferenceScanLimits::default(),
        |occurrence| {
            assert_eq!(
                occurrence
                    .semantic_owner
                    .unwrap()
                    .owner
                    .facts()
                    .unwrap()
                    .id
                    .as_str(),
                "outer"
            );
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    assert_eq!(report.occurrences, 1);
}

#[test]
fn callback_stop_and_zero_budget_do_not_inspect_later_nodes() {
    let document = document(
        vec![paragraph(vec![link("first"), link(&"x".repeat(1_000_000))])],
        vec![],
    );
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            bytes: 10,
            ..Default::default()
        },
        |_| ControlFlow::Break(()),
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Visitor));
    assert_eq!(report.occurrences, 1);
    assert_eq!(report.bytes, 5);
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            steps: 0,
            ..Default::default()
        },
        |_| panic!("zero work budget called visitor"),
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
    assert_eq!(report.steps, 0);
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            bytes: 4,
            ..Default::default()
        },
        |_| panic!("target inspection exceeded its byte budget"),
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Bytes));
    assert_eq!(report.occurrences, 0);
}

#[test]
fn empty_labels_are_occurrences_and_depth_is_bounded_before_callback() {
    let empty = json!({"type":"link","target":{"kind":"document","name":"empty"},"children":[]});
    let document = document(vec![paragraph(vec![empty])], vec![]);
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            bytes: 5,
            ..Default::default()
        },
        |occurrence| {
            assert!(occurrence.label.is_empty());
            assert!(occurrence.location.resolve_link(&document).is_some());
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    assert_eq!(report.occurrences, 1);
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            depth: 1,
            ..Default::default()
        },
        |_| unreachable!(),
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Depth));
    assert_eq!(report.occurrences, 0);
}

#[test]
fn callback_budget_failure_remains_visible_even_at_end_or_explicit_stop() {
    let document = document(
        vec![paragraph(vec![
            json!({"type":"link","target":{"kind":"document","name":"target"},"children":[]}),
        ])],
        vec![],
    );
    for stop in [false, true] {
        let report = scan_reference_scope(
            &document,
            ReferenceScope::Document,
            ReferenceScanLimits::default(),
            |_, budget| {
                assert_eq!(
                    budget.consume(0, budget.remaining_steps() + 1, 0),
                    Err(ReferenceScanStop::Steps)
                );
                assert_eq!(budget.consume(0, 0, 0), Err(ReferenceScanStop::Steps));
                if stop {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );
        assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
        assert_eq!(report.occurrences, 1);
    }
}

#[test]
fn nested_links_are_distinct_original_occurrences() {
    let nested = json!({"type":"link","target":{"kind":"document","name":"outer"},"children":[{"type":"strong","children":[link("inner")]}]});
    let document = document(vec![paragraph(vec![nested])], vec![]);
    let mut names = Vec::new();
    let report = scan_references(&document, ReferenceScanLimits::default(), |occurrence| {
        if let LinkTarget::Document { name, .. } = occurrence.target {
            names.push(name.clone());
        }
        assert!(std::ptr::eq(
            occurrence.location.resolve_link(&document).unwrap(),
            occurrence.link
        ));
        ControlFlow::Continue(())
    });
    assert!(report.complete());
    assert_eq!(names, ["outer", "inner"]);
}

#[test]
fn thousands_of_repeated_targets_are_bounded_by_scan_not_unique_count() {
    let document = document(
        vec![paragraph((0..10_000).map(|_| link("same")).collect())],
        vec![],
    );
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            steps: 100,
            ..Default::default()
        },
        |_| ControlFlow::Continue(()),
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
    assert!(report.occurrences > 0 && report.occurrences < 100);
    assert!(report.steps <= 100);
}

#[test]
fn repeated_deep_locations_charge_coordinate_inspection_before_callback() {
    let mut nested = paragraph((0..100).map(|_| link("same")).collect());
    for _ in 0..20 {
        nested = json!({"type":"list","kind":{"kind":"bullet"},"items":[{"blocks":[nested]}]});
    }
    let document = document(vec![nested], vec![]);
    let mut inspected_coordinates = 0;
    let report = scan_references(
        &document,
        ReferenceScanLimits {
            steps: 200,
            ..Default::default()
        },
        |occurrence| {
            inspected_coordinates += occurrence.location.depth();
            ControlFlow::Continue(())
        },
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Steps));
    assert!(report.occurrences > 0 && report.occurrences < 4);
    assert!(inspected_coordinates <= report.steps);
    assert!(report.steps <= 200);
}

#[test]
fn optional_navigation_events_share_locations_without_becoming_links() {
    let mut document = document(
        vec![paragraph(vec![
            link("target"),
            json!({"type":"anchor","id":"anchor","fragmentAliases":["Mixed.Target"]}),
        ])],
        vec![],
    );
    document.fragment_aliases.push("Document.Alias".into());
    let mut links = 0;
    let mut targets = Vec::new();
    let report = scan_navigation_scope(
        &document,
        ReferenceScope::Document,
        ReferenceScanLimits::default(),
        NavigationScanOptions {
            targets: true,
            ..Default::default()
        },
        |event, _| {
            match event {
                NavigationEvent::Link(_) => links += 1,
                NavigationEvent::Target(target) => targets.push((
                    target.id.as_str().to_owned(),
                    target.reveal.to_owned().unwrap(),
                )),
                NavigationEvent::EntrySet(_) => panic!("no relation declared"),
            }
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete());
    assert_eq!(report.occurrences, 1);
    assert_eq!(links, 1);
    assert_eq!(targets.len(), 2);
    assert!(matches!(targets[0].1, ContentReveal::Document {}));
    let ContentReveal::Inline { location } = &targets[1].1 else {
        panic!("anchor must be exact inline")
    };
    assert!(
        matches!(location.resolve(&document),Some([Inline::Anchor{id,..}]) if id.as_str()=="anchor")
    );
}

#[test]
fn unrequested_aliases_and_targets_do_not_spend_summary_inspection_budget() {
    let document = document(
        vec![paragraph(vec![
            json!({"type":"anchor","id":"anchor","fragmentAliases":["X".repeat(10000)]}),
            json!({"type":"link","target":{"kind":"external","uri":"X".repeat(10000)},"children":[]}),
            link("ok"),
        ])],
        vec![],
    );
    let options = NavigationScanOptions {
        links: ReferenceLinkFilter::DOCUMENTS,
        ..Default::default()
    };
    let limits = ReferenceScanLimits {
        bytes: 2,
        ..Default::default()
    };
    let report = scan_navigation_scope(
        &document,
        ReferenceScope::Document,
        limits,
        options,
        |_, _| ControlFlow::Continue(()),
    );
    assert!(report.complete());
    assert_eq!(report.occurrences, 1);
    assert_eq!(report.bytes, 2);
    let report = scan_navigation_scope(
        &document,
        ReferenceScope::Document,
        limits,
        NavigationScanOptions {
            targets: true,
            ..options
        },
        |_, _| ControlFlow::Continue(()),
    );
    assert_eq!(report.stopped, Some(ReferenceScanStop::Bytes));
}

#[test]
fn separate_navigation_phases_cannot_reset_shared_operation_budget() {
    let document = document(vec![paragraph(vec![link("ok")])], vec![]);
    let mut budget = ReferenceWorkBudget::new(ReferenceScanLimits {
        bytes: 2,
        ..Default::default()
    });
    let first = scan_navigation_scope_with_budget(
        &document,
        ReferenceScope::Document,
        &mut budget,
        NavigationScanOptions::default(),
        |_, _| ControlFlow::Continue(()),
    );
    assert!(first.complete());
    let second = scan_navigation_scope_with_budget(
        &document,
        ReferenceScope::Document,
        &mut budget,
        NavigationScanOptions::default(),
        |_, _| panic!("second phase bypassed bytes"),
    );
    assert_eq!(second.stopped, Some(ReferenceScanStop::Bytes));
    assert_eq!(second.bytes, 2);
}
