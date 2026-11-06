//! Bounded local proof and typed identity cache obligations.
use super::*;

#[test]
fn proof_budget_and_source_change_storage_are_bounded() {
    let original = "[a\x1bb](https://a.test)";
    let recovery = Recovery::new(original);
    let source = MarkdownSource::new(original);
    let mut diagnostics = Vec::new();
    let mut admission = TargetAdmission {
        remaining: Some(0),
        ..TargetAdmission::default()
    };
    let range = 0..original.len();
    let link = || LinkUse {
        link_type: LinkType::Inline,
        id: "",
        destination: "https://a.test",
        range: &range,
        definition: None,
    };
    assert!(!admission.accept(Some(&recovery), link(), &source, &mut diagnostics));
    let mut normal = TargetAdmission::default();
    assert!(normal.accept(Some(&recovery), link(), &source, &mut diagnostics));
    let large = "x\x1b".repeat(10_000);
    let changes = Recovery::new(&large);
    assert_eq!(changes.changes.len(), MAX_CHANGE_RANGES);
    assert!(changes.intersects(&(large.len() - 1..large.len())));
}

#[test]
fn callback_charges_the_original_reference_destination_before_copying() {
    let use_ = "[x][r\x1bf]";
    let original = format!(
        "{use_}\n\n[r f]: https://short.test\n[r\x1bf]: https://long.test/{}\n",
        "&amp;".repeat(1024)
    );
    let recovery = Recovery::new(&original);
    let source = MarkdownSource::new(&original);
    let range = 0..use_.len();
    let definition = original.find("[r f]:").unwrap()..original.find("\n[r\x1bf]:").unwrap();
    let mut admission = TargetAdmission {
        remaining: Some(100),
        ..TargetAdmission::default()
    };
    let mut diagnostics = Vec::new();
    assert!(!admission.accept(
        Some(&recovery),
        LinkUse {
            link_type: LinkType::Reference,
            id: "r f",
            destination: "https://short.test",
            range: &range,
            definition: Some(&definition)
        },
        &source,
        &mut diagnostics
    ));
    // Only source parsing was affordable. The larger original target was
    // refused before clone, without charging the recovered short target.
    assert_eq!(admission.remaining, Some(100 - use_.len()));
}

#[test]
fn reference_proof_budget_is_exact_and_cache_hits_do_not_repeat_copying() {
    let use_ = "[a\x1bb][r]";
    let destination = "https://same.test";
    let original = format!(
        "{use_}\n\n[r]: {destination} \"{}\"\n",
        "long-title".repeat(10_000)
    );
    let recovery = Recovery::new(&original);
    let definition = recovery
        .references()
        .unwrap()
        .get("r")
        .unwrap()
        .span
        .clone();
    let source = MarkdownSource::new(&original);
    let range = 0..use_.len();
    let needed = use_.len() + destination.len();
    for initial in [needed - 1, needed, needed + 1] {
        let mut admission = TargetAdmission {
            remaining: Some(initial),
            ..TargetAdmission::default()
        };
        let mut diagnostics = Vec::new();
        let link = || LinkUse {
            link_type: LinkType::Reference,
            id: "r",
            destination,
            range: &range,
            definition: Some(&definition),
        };
        let accepted = initial >= needed;
        assert_eq!(
            admission.accept(Some(&recovery), link(), &source, &mut diagnostics),
            accepted
        );
        let spent = if accepted { needed } else { use_.len() };
        assert_eq!(admission.remaining, Some(initial - spent));
        assert_eq!(diagnostics.len(), usize::from(!accepted));
        // The callback returns no title: its large metadata is neither copied
        // by this proof nor charged as if it had been. Failed copies are
        // refused before clone, after charging the local source parse.
        let remaining = admission.remaining;
        assert_eq!(
            admission.accept(Some(&recovery), link(), &source, &mut diagnostics),
            accepted
        );
        assert_eq!(admission.remaining, remaining);
        assert_eq!(diagnostics.len(), usize::from(!accepted));
    }
}

#[test]
fn equal_destination_and_title_do_not_share_a_different_definition_proof() {
    let use_ = "[X][r\x1bf]";
    let destination = "https://same.test";
    let original =
        format!("{use_}\n\n[r f]: {destination} \"SAME\"\n[r\x1bf]: {destination} \"SAME\"\n");
    let recovery = Recovery::new(&original);
    let references = recovery.references().unwrap();
    let wrong = references.get("r f").unwrap().span.clone();
    let right = references.get("r\x1bf").unwrap().span.clone();
    assert_ne!(wrong, right);
    let source = MarkdownSource::new(&original);
    let range = 0..use_.len();
    let mut admission = TargetAdmission {
        remaining: Some(2 * (use_.len() + destination.len())),
        ..TargetAdmission::default()
    };
    let mut diagnostics = Vec::new();
    let link = |definition| LinkUse {
        link_type: LinkType::Reference,
        id: "r f",
        destination,
        range: &range,
        definition: Some(definition),
    };
    assert!(!admission.accept(Some(&recovery), link(&wrong), &source, &mut diagnostics));
    assert!(admission.accept(Some(&recovery), link(&right), &source, &mut diagnostics));
    assert_eq!(admission.decisions.len(), 2);
    assert_eq!(admission.remaining, Some(0));
    assert_eq!(diagnostics.len(), 1);
}

#[test]
fn source_change_range_saturation_never_certifies_an_unexamined_suffix() {
    for count in [4095, 4096, 4097] {
        let source = "x\x1b".repeat(count);
        let recovery = Recovery::new(&source);
        assert_eq!(recovery.changes.len(), count.min(MAX_CHANGE_RANGES));
        assert!(recovery.intersects(&(source.len() - 1..source.len())));
        assert!(!recovery.intersects(&(0..1)));
    }
}
