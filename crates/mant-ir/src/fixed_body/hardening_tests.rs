//! Fixed-body relation and failure regression tests.

use super::*;
use crate::{EntryKind, EntryNameEvidence, NameCase, ParameterKind};

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
        root_configuration_hint: false,
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
#[allow(clippy::too_many_lines)] // One display fixture binds both Fl components and rejects stale facts.
fn mdoc_option_components_bind_names_inside_one_parameterized_form() {
    // This exact `.It Fl a , Fl b Ar file` input ran pinned CVS -Tutf8
    // first. mdoc_macro.c::blk_full retains both Fl nodes and the Ar operand
    // in one HEAD; mdoc_term.c::termp_fl_pre prints each option separately.
    let mut body = body_with_run("-a, -b file", 11);
    let first = body.surface.runs[0].clone();
    let lengths = [2_u64, 2, 2, 1, 4];
    let mut offset = 0_u64;
    body.surface.runs = lengths
        .into_iter()
        .enumerate()
        .map(|(index, length)| {
            let run = DisplayRun {
                key: key(u32::try_from(index + 1).unwrap()),
                column: u32::try_from(offset).unwrap(),
                width: u32::try_from(length).unwrap(),
                byte_start: offset,
                byte_count: length,
                label: DisplayLabel {
                    owner: Some(key(1)),
                    role: DisplayRole::Body,
                    ..first.label
                },
                ..first.clone()
            };
            offset += length;
            run
        })
        .collect();
    body.surface.rows[0].run_count = 5;
    let part = |run| OutputSlice {
        run: key(run),
        start_byte: 0,
        end_byte: lengths[(run - 1) as usize],
    };
    let head = TextSelection {
        parts: (1..=5).map(part).collect(),
        joins: vec![TextJoin::DirectContact; 4],
    };
    let component = |run| TextSelection {
        parts: vec![part(run)],
        joins: Vec::new(),
    };
    let source = |column| SourceSpan {
        source: SourceKey::FIRST,
        line: 6,
        column,
        byte_range: None,
        end_line: None,
        end_column: None,
    };
    let owner = OwnerMark {
        key: key(1),
        id: crate::NodeId::from("native-owner-1"),
        parent: None,
        preceding_owner: None,
        hanging_candidate: false,
        hanging_continuation: None,
        hanging_nested_head: None,
        section: None,
        role: OwnerRole::Definition,
        head_role: Some(OwnerHeadRole::Option),
        head_role_prefix: Some("-a".to_owned()),
        lexical_term_witness: false,
        head_components: [1, 3]
            .into_iter()
            .map(|run| OwnerHeadComponent {
                role: OwnerHeadRole::Option,
                selection: component(run),
                source_key: None,
                source: Some(source(if run == 1 { 5 } else { 14 })),
            })
            .collect(),
        entry: None,
        head: head.clone(),
        direct_body: TextSelection {
            parts: Vec::new(),
            joins: Vec::new(),
        },
        empty_point: None,
        source_key: None,
        source: Some(source(1)),
    };
    body.owners.push(owner);
    assert!(body.option_component_forms(&body.owners[0]).is_none());
    assert_eq!(
        body.option_component_names(&body.owners[0])
            .unwrap()
            .iter()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["-a", "-b"]
    );
    body.owners[0].entry = Some(crate::EntryFacts {
        name_bindings: [1, 3]
            .into_iter()
            .enumerate()
            .map(|(name, run)| crate::EntryNameBinding {
                name,
                occurrences: vec![component(run)],
                evidence: crate::EntryNameEvidence::NativeMarkup,
            })
            .collect(),
        alias_groups: Vec::new(),
        alias_of: None,
        forms: vec![head],
        id: body.owners[0].id.clone(),
        kind: crate::EntryKind::Parameter {
            parameter_kind: crate::ParameterKind::Option,
        },
        case: crate::NameCase::Sensitive,
        names: vec!["-a".to_owned(), "-b".to_owned()],
        value_domain: None,
    });
    body.validate().unwrap();
    body.owners[0].entry.as_mut().unwrap().name_bindings[1].occurrences = vec![component(1)];
    assert!(body.validated_entry(&body.owners[0]).is_none());
    assert!(body.validate().is_err());
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
        source_key: None,
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
    // term.c::term_word() inserts AUTO_SPACE and term_flushln() may consume
    // it at a soft wrap; the exact generated bytes are a distinct join fact.
    let generated = TextSelection {
        joins: vec![TextJoin::GeneratedSeparator("  ".to_owned())],
        ..separated.clone()
    };
    body.surface.validate_selection(&generated).unwrap();
    let serialized = serde_json::to_value(&generated).unwrap();
    assert_eq!(serialized["joins"][0]["kind"], "generated-separator");
    assert_eq!(serialized["joins"][0]["text"], "  ");
    assert_eq!(
        serde_json::from_value::<TextSelection>(serialized).unwrap(),
        generated
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
        let invalid = TextSelection {
            joins: vec![TextJoin::GeneratedSeparator(bad.to_owned())],
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
            source_key: None,
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
        source_key: None,
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
        continuation_of: None,
        section: None,
        kind: RegionKind::Literal,
        selection: TextSelection {
            parts: Vec::new(),
            joins: Vec::new(),
        },
        empty_point: None,
        source_key: None,
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
        preceding_owner: None,
        hanging_candidate: false,
        hanging_continuation: None,
        hanging_nested_head: None,
        section: None,
        role: OwnerRole::Definition,
        head_role: None,
        head_role_prefix: None,
        lexical_term_witness: false,
        head_components: Vec::new(),
        entry: None,
        head: selection(&[(0, 2)], Vec::new()),
        direct_body: selection(&[(1, 3)], Vec::new()),
        empty_point: None,
        source_key: None,
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
        source_key: None,
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
        source_key: None,
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

#[test]
fn fixed_partial_name_requires_native_prefix_and_exact_surviving_slice() {
    let mut body = body_with_run("-aVALUE", 7);
    body.surface.runs[0].label.owner = Some(key(1));
    let head = selection(&[(0, 7)], Vec::new());
    let name = selection(&[(0, 2)], Vec::new());
    body.owners.push(OwnerMark {
        key: key(1),
        id: NodeId::from("option-a"),
        parent: None,
        preceding_owner: None,
        hanging_candidate: false,
        hanging_continuation: None,
        hanging_nested_head: None,
        section: None,
        role: OwnerRole::Definition,
        head_role: Some(OwnerHeadRole::Option),
        head_role_prefix: Some("-a".to_owned()),
        lexical_term_witness: false,
        head_components: Vec::new(),
        entry: Some(EntryFacts {
            id: NodeId::from("option-a"),
            kind: EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["-a".to_owned()],
            forms: vec![head.clone()],
            name_bindings: vec![crate::EntryNameBinding {
                name: 0,
                occurrences: vec![name],
                evidence: EntryNameEvidence::NativeMarkup,
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            value_domain: None,
        }),
        head,
        direct_body: selection(&[], Vec::new()),
        empty_point: None,
        source_key: None,
        source: None,
    });
    body.validate().unwrap();
    let mut forged = body.clone();
    forged.owners[0].head_role_prefix = Some("-aVALUE".to_owned());
    assert!(forged.validate().is_err());
    forged = body.clone();
    forged.owners[0].entry.as_mut().unwrap().name_bindings[0].occurrences =
        vec![selection(&[(0, 3)], Vec::new())];
    assert!(forged.validate().is_err());
    assert!(serde_json::from_value::<FixedBody>(serde_json::to_value(forged).unwrap()).is_err());

    let mut forged = body.clone();
    forged.surface.text = "foo".to_owned();
    forged.surface.runs[0].byte_count = 3;
    forged.surface.runs[0].width = 3;
    forged.surface.rows[0].column_count = 3;
    forged.owners[0].head = selection(&[(0, 3)], Vec::new());
    forged.owners[0].head_role_prefix = Some("foo".to_owned());
    let facts = forged.owners[0].entry.as_mut().unwrap();
    facts.names = vec!["foo".to_owned()];
    facts.forms = vec![selection(&[(0, 3)], Vec::new())];
    facts.name_bindings[0].occurrences = facts.forms.clone();
    assert!(forged.validate().is_err()); // Native Fl cannot render `foo`.

    forged.surface.text = "a!".to_owned();
    forged.surface.runs[0].byte_count = 2;
    forged.surface.runs[0].width = 2;
    forged.surface.rows[0].column_count = 2;
    forged.owners[0].head = selection(&[(0, 2)], Vec::new());
    forged.owners[0].head_role = Some(OwnerHeadRole::Environment);
    forged.owners[0].head_role_prefix = Some("a!".to_owned());
    let facts = forged.owners[0].entry.as_mut().unwrap();
    facts.kind = EntryKind::EnvironmentVariable;
    facts.names = vec!["a!".to_owned()];
    facts.forms = vec![selection(&[(0, 2)], Vec::new())];
    facts.name_bindings[0].occurrences = facts.forms.clone();
    assert!(forged.validate().is_err());
}

#[test]
fn lexical_option_facts_require_the_native_role_and_complete_head_binding() {
    let mut body = body_with_run("--save", 6);
    body.surface.runs[0].label.owner = Some(key(1));
    let head = selection(&[(0, 6)], Vec::new());
    body.owners.push(OwnerMark {
        key: key(1),
        id: NodeId::from("lexical-option"),
        parent: None,
        preceding_owner: None,
        hanging_candidate: false,
        hanging_continuation: None,
        hanging_nested_head: None,
        section: None,
        role: OwnerRole::Definition,
        head_role: Some(OwnerHeadRole::Lexical),
        head_role_prefix: None,
        lexical_term_witness: false,
        head_components: Vec::new(),
        entry: Some(EntryFacts {
            id: NodeId::from("lexical-option"),
            kind: EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            case: NameCase::Sensitive,
            names: vec!["--save".to_owned()],
            forms: vec![head.clone()],
            name_bindings: vec![crate::EntryNameBinding {
                name: 0,
                occurrences: vec![head.clone()],
                evidence: EntryNameEvidence::Lexical,
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            value_domain: None,
        }),
        head,
        direct_body: selection(&[], Vec::new()),
        empty_point: None,
        source_key: None,
        source: None,
    });
    body.validate().unwrap();
    serde_json::from_value::<FixedBody>(serde_json::to_value(&body).unwrap()).unwrap();

    // A lexical option cannot be hidden as a plain term, even if a forged
    // document asserts the native fallback witness. Producer and validator
    // must use the same complete-HEAD name decision.
    let mut forged_term = body.clone();
    forged_term.owners[0].entry.as_mut().unwrap().kind = EntryKind::Term;
    assert!(forged_term.validate().is_err());
    forged_term.owners[0].lexical_term_witness = true;
    assert!(forged_term.validate().is_err());

    let mut forged = body.clone();
    forged.owners[0].head_role = None;
    assert!(forged.validate().is_err());
    forged = body.clone();
    forged.owners[0].entry.as_mut().unwrap().names = vec!["--sav".to_owned()];
    assert!(forged.validate().is_err());
    forged = body.clone();
    forged.owners[0].entry.as_mut().unwrap().name_bindings[0].occurrences =
        vec![selection(&[(0, 5)], Vec::new())];
    assert!(forged.validate().is_err());
    forged = body;
    forged.owners[0].entry.as_mut().unwrap().name_bindings[0].evidence =
        EntryNameEvidence::NativeMarkup;
    assert!(forged.validate().is_err());
    assert!(serde_json::from_value::<FixedBody>(serde_json::to_value(forged).unwrap()).is_err());
}
