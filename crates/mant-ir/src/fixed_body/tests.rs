use super::*;

fn key(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn empty_selection() -> TextSelection {
    TextSelection {
        parts: Vec::new(),
        joins: Vec::new(),
    }
}

#[test]
fn borrowed_entry_view_requires_the_mark_in_its_own_fixed_body() {
    let mut body = sample_body();
    let head = TextSelection {
        parts: vec![OutputSlice {
            run: key(1),
            start_byte: 0,
            end_byte: 1,
        }],
        joins: Vec::new(),
    };
    let owner = &mut body.owners[0];
    owner.head = head.clone();
    owner.entry = Some(crate::EntryFacts {
        name_bindings: vec![crate::EntryNameBinding {
            name: 0,
            occurrences: vec![head.clone()],
            evidence: crate::EntryNameEvidence::Lexical,
        }],
        alias_groups: Vec::new(),
        alias_of: None,
        forms: vec![head],
        id: owner.id.clone(),
        kind: crate::EntryKind::Term,
        case: crate::NameCase::Sensitive,
        names: vec!["a".to_owned()],
        value_domain: None,
    });
    let view = crate::EntryOwnerView::fixed(&body, &body.owners[0])
        .expect("a mark within this body has a checked head");
    assert_eq!(view.semantic_entry().unwrap().unwrap().forms, ["a"]);
    let detached = body.owners[0].clone();
    assert!(crate::EntryOwnerView::fixed(&body, &detached).is_none());
}

// One complete fixture keeps cross-mark relation tests on the same surface.
#[allow(clippy::too_many_lines)]
fn sample_body() -> FixedBody {
    FixedBody {
        surface: DisplaySurface {
            text: "a界".to_owned(),
            rows: vec![DisplayRow {
                key: key(1),
                first_run: key(1),
                run_count: 2,
                column_count: 3,
                break_after: false,
            }],
            runs: vec![
                DisplayRun {
                    key: key(1),
                    row: key(1),
                    column: 0,
                    width: 1,
                    byte_start: 0,
                    byte_count: 1,
                    label: DisplayLabel {
                        owner: Some(key(1)),
                        link: Some(key(1)),
                        source: SourceKey::new(1),
                        style: DisplayStyle {
                            bold: false,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                },
                DisplayRun {
                    key: key(2),
                    row: key(1),
                    column: 1,
                    width: 2,
                    byte_start: 1,
                    byte_count: 3,
                    label: DisplayLabel {
                        owner: Some(key(1)),
                        link: Some(key(1)),
                        source: SourceKey::new(1),
                        style: DisplayStyle {
                            bold: true,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                },
            ],
        },
        headings: vec![HeadingMark {
            key: key(1),
            id: crate::NodeId::from("heading"),
            fragment_aliases: Vec::new(),
            generated_fragment_aliases: Vec::new(),
            rendered_fragment_aliases: Vec::new(),
            parent: None,
            level_hint: 1,
            at: DisplayPoint::RunBoundary {
                run: key(1),
                byte: 0,
            },
            title: TextSelection {
                parts: vec![OutputSlice {
                    run: key(1),
                    start_byte: 0,
                    end_byte: 1,
                }],
                joins: Vec::new(),
            },
            direct_body: empty_selection(),
            source: None,
        }],
        owners: vec![OwnerMark {
            key: key(1),
            id: crate::NodeId::from("native-owner-1"),
            parent: None,
            preceding_owner: None,
            section: Some(key(1)),
            role: OwnerRole::Definition,
            head_role: None,
            head_role_prefix: None,
            head_components: Vec::new(),
            entry: None,
            head: empty_selection(),
            direct_body: empty_selection(),
            empty_point: Some(DisplayPoint::RunBoundary {
                run: key(1),
                byte: 0,
            }),
            source: None,
        }],
        links: vec![LinkMark {
            key: key(1),
            target: Some(LinkTarget::External {
                uri: "https://example.test".to_owned(),
            }),
            label: TextSelection {
                parts: vec![
                    OutputSlice {
                        run: key(1),
                        start_byte: 0,
                        end_byte: 1,
                    },
                    OutputSlice {
                        run: key(2),
                        start_byte: 0,
                        end_byte: 3,
                    },
                ],
                joins: vec![TextJoin::DirectContact],
            },
            source: None,
        }],
        anchors: vec![AnchorMark {
            key: key(1),
            id: crate::NodeId::from("target"),
            section: Some(key(1)),
            name: "target".to_owned(),
            rendered_fragment: "target".into(),
            authored: true,
            at: DisplayPoint::RunBoundary {
                run: key(2),
                byte: 3,
            },
            source: None,
        }],
        regions: vec![RegionMark {
            key: key(1),
            parent: None,
            owner: Some(key(1)),
            section: Some(key(1)),
            kind: RegionKind::OwnerHead,
            selection: empty_selection(),
            empty_point: Some(DisplayPoint::DocumentEnd { row_count: 1 }),
            source: None,
        }],
    }
}

#[test]
fn region_section_must_match_its_native_owner() {
    let mut body = sample_body();
    body.regions[0].section = None;
    assert!(body.validate().is_err());
}

#[test]
fn one_arena_and_borrowed_slices_round_trip() {
    let body = sample_body();
    body.validate().unwrap();
    assert_eq!(body.surface.run_text(key(2)), Some("界"));
    assert_eq!(
        body.source_keys().collect::<Vec<_>>(),
        vec![SourceKey::FIRST; 2]
    );
    let wire = serde_json::to_value(&body).unwrap();
    assert_eq!(serde_json::from_value::<FixedBody>(wire).unwrap(), body);
}

#[test]
fn native_predecessor_is_structural_evidence_not_shared_body() {
    let mut body = sample_body();
    body.owners[0].head_role = Some(OwnerHeadRole::Lexical);
    body.owners[0].head_role_prefix = Some("-x".to_owned());
    let mut next = body.owners[0].clone();
    next.key = key(2);
    next.id = crate::NodeId::from("native-owner-2");
    next.preceding_owner = Some(key(1));
    body.owners.push(next);
    body.validate().unwrap();
    let wire = serde_json::to_value(&body).unwrap();
    assert_eq!(wire["owners"][1]["precedingOwner"], 1);
    assert_eq!(serde_json::from_value::<FixedBody>(wire).unwrap(), body);

    body.owners[1].section = None;
    assert!(body.validate().is_err());
    body.owners[1].section = Some(key(1));
    body.owners[1].parent = Some(key(1));
    assert!(body.validate().is_err());
    body.owners[1].parent = None;
    // Typed IR has no man macro token: a TP/TQ lexical predecessor may
    // legitimately lack the IP-style source prefix, while C/FFI check its
    // native sibling family before transfer.
    body.owners[1].head_role_prefix = None;
    body.validate().unwrap();
    body.owners[1].head_role = None;
    assert!(body.validate().is_err());
}

#[test]
fn predecessor_cannot_skip_another_owner_in_the_same_scope() {
    let mut body = sample_body();
    body.owners[0].head_role = Some(OwnerHeadRole::Lexical);
    let mut middle = body.owners[0].clone();
    middle.key = key(2);
    middle.id = crate::NodeId::from("native-owner-2");
    let mut last = body.owners[0].clone();
    last.key = key(3);
    last.id = crate::NodeId::from("native-owner-3");
    last.preceding_owner = Some(key(1));
    body.owners.extend([middle, last]);
    assert!(body.validate().is_err());
    body.owners[2].preceding_owner = Some(key(2));
    body.validate().unwrap();
}

#[test]
fn empty_surface_keeps_document_end_without_fake_run() {
    let mut body = sample_body();
    body.surface = DisplaySurface {
        text: String::new(),
        rows: Vec::new(),
        runs: Vec::new(),
    };
    body.headings.clear();
    body.owners.clear();
    body.links.clear();
    body.anchors = vec![AnchorMark {
        key: key(1),
        id: crate::NodeId::from("empty"),
        section: None,
        name: "empty".to_owned(),
        rendered_fragment: "empty".into(),
        authored: false,
        at: DisplayPoint::DocumentEnd { row_count: 0 },
        source: None,
    }];
    body.regions.clear();
    body.validate().unwrap();
}

#[test]
fn checked_logical_subrange_maps_only_final_glyphs() {
    let body = sample_body();
    let direct = TextSelection {
        parts: vec![
            OutputSlice {
                run: key(1),
                start_byte: 0,
                end_byte: 1,
            },
            OutputSlice {
                run: key(2),
                start_byte: 0,
                end_byte: 3,
            },
        ],
        joins: vec![TextJoin::DirectContact],
    };
    assert_eq!(body.selection_text(&direct).as_deref(), Some("a界"));
    let clipped = body.selection_subrange(&direct, 1..4).unwrap();
    assert_eq!(clipped.parts, vec![direct.parts[1]]);
    assert!(clipped.joins.is_empty());
    assert_eq!(body.selection_text(&clipped).as_deref(), Some("界"));
    assert!(body.selection_subrange(&direct, 2..4).is_none());
    assert_eq!(body.selection_subrange(&direct, 0..4), Some(direct.clone()));

    let separated = TextSelection {
        parts: direct.parts,
        joins: vec![TextJoin::AuthoredSeparator(" ".to_owned())],
    };
    assert_eq!(body.selection_text(&separated).as_deref(), Some("a 界"));
    assert!(body.selection_subrange(&separated, 0..5).is_none());
    assert_eq!(
        body.selection_text(&body.selection_subrange(&separated, 2..5).unwrap())
            .as_deref(),
        Some("界")
    );
    // Exact native AUTO_SPACE is logical text, but has no final run bytes to
    // project as an isolated source-backed occurrence.
    let generated = TextSelection {
        joins: vec![TextJoin::GeneratedSeparator(" ".to_owned())],
        ..separated
    };
    assert_eq!(body.selection_text(&generated).as_deref(), Some("a 界"));
    assert!(body.selection_subrange(&generated, 0..5).is_none());
}

#[test]
fn rejects_dangling_and_non_utf8_slices_on_wire() {
    let mut wire = serde_json::to_value(sample_body()).unwrap();
    wire["links"][0]["label"]["parts"][1]["startByte"] = 1.into();
    assert!(serde_json::from_value::<FixedBody>(wire).is_err());

    let mut wire = serde_json::to_value(sample_body()).unwrap();
    wire["links"][0]["label"]["parts"][1]["run"] = 3.into();
    assert!(serde_json::from_value::<FixedBody>(wire).is_err());

    let mut wire = serde_json::to_value(sample_body()).unwrap();
    wire["surface"]["runs"][1]["byteStart"] = 0.into();
    assert!(serde_json::from_value::<FixedBody>(wire).is_err());
}

#[test]
fn fixed_navigation_id_and_authored_alias_have_distinct_wire_rules() {
    let mut body = sample_body();
    body.headings[0].fragment_aliases = vec![crate::FragmentAlias::from("Mixed.Target")];
    body.headings[0].rendered_fragment_aliases = vec![crate::FragmentAlias::from("Mixed.Target")];
    body.validate().unwrap();

    let mut wire = serde_json::to_value(&body).unwrap();
    wire["headings"][0]["id"] = "Mixed.Target".into();
    assert!(serde_json::from_value::<FixedBody>(wire).is_err());

    let mut wire = serde_json::to_value(&body).unwrap();
    wire["anchors"][0]["name"] = "two words".into();
    assert!(serde_json::from_value::<FixedBody>(wire).is_err());
}

#[test]
fn rejects_wrong_join_density_parent_and_point() {
    let mut body = sample_body();
    body.links[0].label.joins.clear();
    assert!(body.validate().is_err());

    let mut body = sample_body();
    body.headings[0].parent = Some(key(1));
    assert!(body.validate().is_err());

    let mut body = sample_body();
    body.anchors[0].at = DisplayPoint::DocumentEnd { row_count: 0 };
    assert!(body.validate().is_err());

    let mut body = sample_body();
    body.anchors[0].at = DisplayPoint::RowColumn {
        row: key(1),
        column: 2,
    };
    body.validate().unwrap(); // A device boundary can lie inside a wide cell.
    body.anchors[0].at = DisplayPoint::RowColumn {
        row: key(1),
        column: 4,
    };
    assert!(body.validate().is_err());
    body.anchors[0].at = DisplayPoint::RowColumn {
        row: key(2),
        column: 0,
    };
    assert!(body.validate().is_err());
}

#[test]
fn rejects_unknown_wire_fields_and_invalid_run_label_references() {
    let mut wire = serde_json::to_value(sample_body()).unwrap();
    wire["surface"]["runs"][0]["label"]["extra"] = true.into();
    assert!(serde_json::from_value::<FixedBody>(wire).is_err());

    let mut body = sample_body();
    body.surface.runs[0].label.link = Some(key(2));
    assert!(body.validate().is_err());

    let mut body = sample_body();
    body.surface.runs[0].label.link = None;
    assert!(body.validate().is_err());

    let mut body = sample_body();
    body.owners[0].head = TextSelection {
        parts: vec![OutputSlice {
            run: key(1),
            start_byte: 0,
            end_byte: 1,
        }],
        joins: Vec::new(),
    };
    body.owners[0].empty_point = None;
    body.surface.runs[0].label.owner = None;
    assert!(body.validate().is_err());
}
