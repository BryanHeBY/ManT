//! Fixed-body relation and failure regression tests.

use super::*;

fn key(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

fn body_with_run(text: &str, width: u32) -> FixedBody {
    FixedBody {
        surface: DisplaySurface {
            text: text.to_owned(),
            rows: vec![DisplayRow {
                key: key(1),
                first_run: key(1),
                run_count: 1,
                column_count: width,
                break_after: false,
            }],
            runs: vec![DisplayRun {
                key: key(1),
                row: key(1),
                column: 0,
                width,
                byte_start: 0,
                byte_count: text.len() as u64,
                label: DisplayLabel {
                    owner: None,
                    link: None,
                    source: None,
                    style: DisplayStyle {
                        bold: false,
                        underline: false,
                    },
                    role: DisplayRole::Body,
                },
            }],
        },
        headings: Vec::new(),
        owners: Vec::new(),
        links: Vec::new(),
        anchors: Vec::new(),
        regions: Vec::new(),
    }
}

fn selection(parts: &[(u64, u64)], joins: Vec<TextJoin>) -> TextSelection {
    TextSelection {
        parts: parts
            .iter()
            .map(|&(start_byte, end_byte)| OutputSlice {
                run: key(1),
                start_byte,
                end_byte,
            })
            .collect(),
        joins,
    }
}

#[test]
fn fixed_surface_rejects_controls_but_keeps_combining_text() {
    let combining = body_with_run("e\u{301}", 1);
    combining.validate().unwrap();
    for unsafe_text in ["a\n", "a\t", "a\u{1b}", "a\u{85}", "a\u{2028}", "a\u{2029}"] {
        let body = body_with_run(unsafe_text, 1);
        assert!(body.validate().is_err(), "{unsafe_text:?}");
        assert!(serde_json::from_value::<FixedBody>(serde_json::to_value(body).unwrap()).is_err());
    }
}

#[test]
fn sparse_geometry_has_a_bounded_row_and_total_column_count() {
    let mut body = body_with_run("a", 1);
    body.surface.rows[0].column_count = MAX_FIXED_ROW_COLUMNS + 1;
    assert!(body.validate().is_err());

    let mut body = body_with_run("a", 1);
    body.surface.rows = (1..=33)
        .map(|index| DisplayRow {
            key: key(index),
            first_run: key(if index == 1 { 1 } else { 2 }),
            run_count: u32::from(index == 1),
            column_count: MAX_FIXED_ROW_COLUMNS,
            break_after: index != 33,
        })
        .collect();
    assert!(body.validate().is_err());
}

#[test]
fn direct_contact_must_not_skip_bytes_in_one_run() {
    let mut body = body_with_run("abc", 3);
    body.links.push(LinkMark {
        key: key(1),
        target: Some(LinkTarget::External {
            uri: "https://example.test".to_owned(),
        }),
        label: selection(&[(0, 1), (2, 3)], vec![TextJoin::DirectContact]),
        source: None,
    });
    body.surface.runs[0].label.link = Some(key(1));
    assert!(body.validate().is_err());
    body.links[0].label.joins[0] = TextJoin::Unknown;
    body.surface
        .validate_selection(&body.links[0].label)
        .unwrap();
    assert!(body.validate().is_err()); // The link must still cover every labeled byte.
    body.links[0].label.parts[1].start_byte = 1;
    body.links[0].label.joins[0] = TextJoin::DirectContact;
    body.validate().unwrap();
}

#[test]
fn direct_contact_must_not_skip_a_visible_run() {
    let mut body = body_with_run("abc", 3);
    let first = body.surface.runs[0].clone();
    body.surface.runs = (0..3)
        .map(|index| DisplayRun {
            key: key(index + 1),
            column: index,
            width: 1,
            byte_start: u64::from(index),
            byte_count: 1,
            ..first.clone()
        })
        .collect();
    body.surface.rows[0].run_count = 3;
    let selection = TextSelection {
        parts: vec![
            OutputSlice {
                run: key(1),
                start_byte: 0,
                end_byte: 1,
            },
            OutputSlice {
                run: key(3),
                start_byte: 0,
                end_byte: 1,
            },
        ],
        joins: vec![TextJoin::DirectContact],
    };
    assert!(body.surface.validate_selection(&selection).is_err());
    let adjacent = TextSelection {
        parts: vec![
            OutputSlice {
                run: key(1),
                start_byte: 0,
                end_byte: 1,
            },
            OutputSlice {
                run: key(2),
                start_byte: 0,
                end_byte: 1,
            },
        ],
        joins: vec![TextJoin::DirectContact],
    };
    body.surface.validate_selection(&adjacent).unwrap();
}

#[test]
fn native_join_evidence_is_independent_of_row_indentation() {
    // Exact soft-wrap input was first run with pinned CVS -Tutf8/-Ttree.
    // term.c::term_flushln consumes the separator at WRAP, while the
    // next visible row can still begin after native indentation.
    let mut body = body_with_run("ab", 1);
    body.surface.runs[0].byte_count = 1;
    body.surface.runs.push(DisplayRun {
        key: key(2),
        row: key(2),
        column: 0,
        width: 1,
        byte_start: 1,
        byte_count: 1,
        ..body.surface.runs[0].clone()
    });
    body.surface.rows[0].column_count = 2;
    body.surface.rows[0].break_after = true;
    body.surface.rows.push(DisplayRow {
        key: key(2),
        first_run: key(2),
        run_count: 1,
        column_count: 1,
        break_after: false,
    });
    let selection = TextSelection {
        parts: vec![
            OutputSlice {
                run: key(1),
                start_byte: 0,
                end_byte: 1,
            },
            OutputSlice {
                run: key(2),
                start_byte: 0,
                end_byte: 1,
            },
        ],
        joins: vec![TextJoin::DirectContact],
    };
    body.surface.validate().unwrap();
    body.surface.validate_selection(&selection).unwrap();
    body.surface.rows[0].column_count = 1;
    body.surface.validate_selection(&selection).unwrap();
    body.surface.runs[1].column = 1;
    body.surface.rows[1].column_count = 2;
    body.surface.validate_selection(&selection).unwrap();
    let separated = TextSelection {
        joins: vec![TextJoin::AuthoredSeparator("   ".to_owned())],
        ..selection
    };
    body.surface.validate_selection(&separated).unwrap();
    let serialized = serde_json::to_value(&separated).unwrap();
    assert_eq!(serialized["joins"][0]["kind"], "authored-separator");
    assert_eq!(serialized["joins"][0]["text"], "   ");
    assert_eq!(
        serde_json::from_value::<TextSelection>(serialized).unwrap(),
        separated
    );
    let mut stale_wire = serde_json::to_value(&separated).unwrap();
    stale_wire["joins"][0] = serde_json::json!("authored-separator");
    assert!(serde_json::from_value::<TextSelection>(stale_wire).is_err());
    let mut unknown_field = serde_json::to_value(&separated).unwrap();
    unknown_field["joins"][0]["invented"] = serde_json::json!(true);
    assert!(serde_json::from_value::<TextSelection>(unknown_field).is_err());
    for bad in ["", "x", " \t"] {
        let invalid = TextSelection {
            joins: vec![TextJoin::AuthoredSeparator(bad.to_owned())],
            ..separated.clone()
        };
        assert!(body.surface.validate_selection(&invalid).is_err());
    }
}

#[test]
fn native_join_may_cross_only_unowned_layout_runs() {
    // Exact soft-wrap shape first checked with pinned CVS -Tutf8.
    // term.c::term_flushln consumes WRAP spaces before term_field()
    // emits layout indentation on the following physical row.
    let mut body = body_with_run("a b", 1);
    let template = body.surface.runs[0].clone();
    body.surface.runs = vec![
        template.clone(),
        DisplayRun {
            key: key(2),
            row: key(2),
            column: 0,
            width: 1,
            byte_start: 1,
            byte_count: 1,
            ..template.clone()
        },
        DisplayRun {
            key: key(3),
            row: key(2),
            column: 1,
            width: 1,
            byte_start: 2,
            byte_count: 1,
            ..template
        },
    ];
    body.surface.runs[0].byte_count = 1;
    body.surface.rows[0].break_after = true;
    body.surface.rows.push(DisplayRow {
        key: key(2),
        first_run: key(2),
        run_count: 2,
        column_count: 2,
        break_after: false,
    });
    let selected = TextSelection {
        parts: vec![
            OutputSlice {
                run: key(1),
                start_byte: 0,
                end_byte: 1,
            },
            OutputSlice {
                run: key(3),
                start_byte: 0,
                end_byte: 1,
            },
        ],
        joins: vec![TextJoin::AuthoredSeparator(" ".to_owned())],
    };
    body.surface.validate().unwrap();
    assert!(body.surface.validate_selection(&selected).is_err());
    body.surface.runs[1].label.role = DisplayRole::Layout;
    body.surface.validate_selection(&selected).unwrap();
    body.surface.runs[1].label.owner = Some(key(1));
    assert!(body.surface.validate().is_err());
}

#[test]
fn repeated_selection_gap_checks_share_one_surface_index() {
    // Many marks may select the same two runs. Validation must account
    // for each intermediate native layout run once, not once per mark.
    const COUNT: u32 = 2_048;
    let mut body = body_with_run("a", 1);
    let template = body.surface.runs[0].clone();
    body.surface.text = format!("a{}b", " ".repeat(COUNT as usize));
    body.surface.rows[0].column_count = COUNT + 2;
    body.surface.rows[0].run_count = COUNT + 2;
    body.surface.runs = (0..COUNT + 2)
        .map(|index| {
            let mut run = DisplayRun {
                key: key(index + 1),
                column: index,
                width: 1,
                byte_start: u64::from(index),
                byte_count: 1,
                ..template.clone()
            };
            if index > 0 && index <= COUNT {
                run.label.role = DisplayRole::Layout;
            }
            run
        })
        .collect();
    let title = TextSelection {
        parts: vec![
            OutputSlice {
                run: key(1),
                start_byte: 0,
                end_byte: 1,
            },
            OutputSlice {
                run: key(COUNT + 2),
                start_byte: 0,
                end_byte: 1,
            },
        ],
        joins: vec![TextJoin::DirectContact],
    };
    body.headings = (1..=COUNT)
        .map(|index| HeadingMark {
            key: key(index),
            id: crate::NodeId::new(format!("heading-{index}")),
            fragment_aliases: Vec::new(),
            generated_fragment_aliases: Vec::new(),
            rendered_fragment_aliases: Vec::new(),
            parent: None,
            level_hint: 1,
            at: DisplayPoint::RunBoundary {
                run: key(1),
                byte: 0,
            },
            title: title.clone(),
            direct_body: TextSelection {
                parts: vec![],
                joins: vec![],
            },
            source: None,
        })
        .collect();
    body.validate().unwrap();
}

#[test]
fn link_run_labels_require_complete_occurrence_coverage() {
    let mut body = body_with_run("abc", 3);
    body.surface.runs[0].label.link = Some(key(1));
    body.links.push(LinkMark {
        key: key(1),
        target: Some(LinkTarget::External {
            uri: "https://example.test".to_owned(),
        }),
        label: TextSelection {
            parts: Vec::new(),
            joins: Vec::new(),
        },
        source: None,
    });
    assert!(body.validate().is_err());
    body.links[0].label.parts.push(OutputSlice {
        run: key(1),
        start_byte: 0,
        end_byte: 3,
    });
    body.validate().unwrap();
}

#[test]
fn empty_region_requires_its_final_point() {
    let mut body = body_with_run("a", 1);
    body.regions.push(RegionMark {
        key: key(1),
        parent: None,
        owner: None,
        section: None,
        kind: RegionKind::Literal,
        selection: TextSelection {
            parts: Vec::new(),
            joins: Vec::new(),
        },
        empty_point: None,
        source: None,
    });
    assert!(body.validate().is_err());
    body.regions[0].empty_point = Some(DisplayPoint::DocumentEnd { row_count: 1 });
    body.validate().unwrap();
}

#[test]
fn owner_head_and_body_cannot_claim_the_same_bytes() {
    let mut body = body_with_run("abc", 3);
    body.owners.push(OwnerMark {
        key: key(1),
        id: crate::NodeId::from("native-owner-1"),
        parent: None,
        section: None,
        role: OwnerRole::Definition,
        head_role: None,
        entry: None,
        head: selection(&[(0, 2)], Vec::new()),
        direct_body: selection(&[(1, 3)], Vec::new()),
        empty_point: None,
        source: None,
    });
    body.surface.runs[0].label.owner = Some(key(1));
    assert!(body.validate().is_err());
    body.owners[0].direct_body.parts[0].start_byte = 2;
    body.validate().unwrap();
}

#[test]
fn a_native_link_instance_may_have_no_href() {
    // The exact `.TH X 1\n.SH D\n.MR\n` input was run with the pinned
    // CVS -Thtml reference. man_html.c::man_MR_pre prints <a class="Xr">
    // containing "()" without href, not an absent macro instance.
    let mut body = body_with_run("()", 2);
    body.links.push(LinkMark {
        key: key(1),
        target: None,
        label: selection(&[(0, 2)], Vec::new()),
        source: None,
    });
    body.surface.runs[0].label.link = Some(key(1));
    body.validate().unwrap();
    let wire = serde_json::to_value(&body).unwrap();
    assert!(wire["links"][0]["target"].is_null());
    let decoded: FixedBody = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded.links[0].target, None);
    decoded.validate().unwrap();
    let mut missing = wire;
    missing["links"][0]
        .as_object_mut()
        .unwrap()
        .remove("target");
    assert!(serde_json::from_value::<FixedBody>(missing).is_err());
}

#[test]
fn fixed_links_obey_the_shared_target_grammar() {
    let mut body = body_with_run("a", 1);
    body.links.push(LinkMark {
        key: key(1),
        target: Some(LinkTarget::External {
            uri: "https://example.test".to_owned(),
        }),
        label: selection(&[(0, 1)], Vec::new()),
        source: None,
    });
    body.surface.runs[0].label.link = Some(key(1));
    body.validate().unwrap();
    for invalid in [
        LinkTarget::External {
            uri: "https://bad host".to_owned(),
        },
        LinkTarget::Email {
            address: "bad@@example.test".to_owned(),
        },
        LinkTarget::Document {
            name: "../?bad".to_owned(),
            fragment: None,
        },
        LinkTarget::Manual {
            name: "bad/name".to_owned(),
            manual_section: None,
        },
        LinkTarget::Section {
            id: "Mixed.Target".into(),
        },
    ] {
        body.links[0].target = Some(invalid);
        assert!(body.validate().is_err());
        assert!(serde_json::from_value::<FixedBody>(serde_json::to_value(&body).unwrap()).is_err());
    }
}
