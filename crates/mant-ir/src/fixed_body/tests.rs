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
            section: Some(key(1)),
            role: OwnerRole::Definition,
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
