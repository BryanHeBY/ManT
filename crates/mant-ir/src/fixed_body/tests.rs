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

fn alternating_lexical_head(argument: &str) -> (FixedBody, String) {
    let pieces = ["-L", argument, "--all ", "FILE"];
    let form = pieces.concat();
    let mut body = scaled_styled_argument_body(0).0;
    let mut offset = 0usize;
    let mut column = 0u32;
    body.surface.text = form.clone();
    body.surface.runs = pieces
        .into_iter()
        .enumerate()
        .map(|(index, piece)| {
            let width = u32::try_from(piece.chars().count()).unwrap();
            let run = DisplayRun {
                key: key(u32::try_from(index + 1).unwrap()),
                row: key(1),
                column,
                width,
                byte_start: u64::try_from(offset).unwrap(),
                byte_count: u64::try_from(piece.len()).unwrap(),
                label: DisplayLabel {
                    owner: Some(key(1)),
                    link: None,
                    source: None,
                    style: DisplayStyle {
                        bold: index % 2 == 0,
                        underline: index % 2 != 0,
                    },
                    role: DisplayRole::Body,
                },
            };
            offset += piece.len();
            column += width;
            run
        })
        .collect();
    body.surface.rows[0].run_count = 4;
    body.surface.rows[0].column_count = column;
    body.owners[0].head = TextSelection {
        parts: pieces
            .into_iter()
            .enumerate()
            .map(|(index, piece)| OutputSlice {
                run: key(u32::try_from(index + 1).unwrap()),
                start_byte: 0,
                end_byte: u64::try_from(piece.len()).unwrap(),
            })
            .collect(),
        joins: vec![TextJoin::DirectContact; 3],
    };
    body.owners[0].head_components = [0usize, 2]
        .into_iter()
        .map(|index| OwnerHeadComponent {
            role: OwnerHeadRole::Lexical,
            selection: TextSelection {
                parts: vec![body.owners[0].head.parts[index]],
                joins: Vec::new(),
            },
            source: None,
            source_key: Some(SourceKey::FIRST),
        })
        .collect();
    (body, form)
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one bounded component-count matrix checks producer and validator together"
)]
fn native_option_component_ceiling_counts_occurrences_not_distinct_spellings() {
    // Construct post-device Fl components directly: the separate vendor
    // mdoc_macro_call() safety patch stops a single source line before 64
    // recursive Fl calls, so roff cannot exercise the semantic 64/65 ceiling.
    // Both unique and repeated instances remain independent source-identified
    // components, matching mdoc_term.c::termp_fl_pre() execution.
    for (repeated, count) in [(false, 64), (false, 65), (true, 64), (true, 65)] {
        let mut body = scaled_styled_argument_body(0).0;
        let names = (0..count)
            .map(|index| {
                if repeated {
                    "-x".to_owned()
                } else {
                    format!("-n{index}")
                }
            })
            .collect::<Vec<_>>();
        let mut text = String::new();
        let mut column = 0u32;
        body.surface.runs = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let run = DisplayRun {
                    key: key(u32::try_from(index + 1).unwrap()),
                    row: key(1),
                    column,
                    width: u32::try_from(name.len()).unwrap(),
                    byte_start: u64::try_from(text.len()).unwrap(),
                    byte_count: u64::try_from(name.len()).unwrap(),
                    label: DisplayLabel {
                        owner: Some(key(1)),
                        link: None,
                        source: Some(SourceKey::FIRST),
                        style: DisplayStyle {
                            bold: true,
                            underline: false,
                        },
                        role: DisplayRole::Body,
                    },
                };
                text.push_str(name);
                column += run.width;
                run
            })
            .collect();
        body.surface.text = text;
        body.surface.rows[0].run_count = u32::try_from(count).unwrap();
        body.surface.rows[0].column_count = column;
        body.owners[0].head_role = Some(OwnerHeadRole::Option);
        body.owners[0].head_role_prefix = Some(names[0].clone());
        body.owners[0].head.parts = body
            .surface
            .runs
            .iter()
            .map(|run| OutputSlice {
                run: run.key,
                start_byte: 0,
                end_byte: run.byte_count,
            })
            .collect();
        body.owners[0].head.joins = vec![TextJoin::DirectContact; count - 1];
        body.owners[0].head_components = body.owners[0]
            .head
            .parts
            .iter()
            .copied()
            .map(|part| OwnerHeadComponent {
                role: OwnerHeadRole::Option,
                selection: TextSelection {
                    parts: vec![part],
                    joins: Vec::new(),
                },
                source: None,
                source_key: Some(SourceKey::FIRST),
            })
            .collect();
        body.validate().expect("complete synthetic display");
        let owner = &body.owners[0];
        assert_eq!(body.option_component_over_limit(owner), count == 65);
        assert_eq!(
            body.option_component_names(owner).map(|found| found.len()),
            (count == 64).then_some(64),
            "repeated={repeated}, count={count}"
        );
        assert_eq!(
            body.option_component_forms(owner).map(|forms| forms.len()),
            (!repeated && count == 64).then_some(64)
        );

        if count == 65 {
            let first = owner.head_components[0].selection.clone();
            let id = owner.id.clone();
            let head = owner.head.clone();
            body.owners[0].entry = Some(EntryFacts {
                id,
                kind: EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                case: NameCase::Sensitive,
                names: vec![names[0].clone()],
                forms: vec![head],
                name_bindings: vec![crate::EntryNameBinding {
                    name: 0,
                    occurrences: vec![first],
                    evidence: EntryNameEvidence::NativeMarkup,
                }],
                alias_groups: Vec::new(),
                alias_of: None,
                value_domain: None,
            });
            assert_eq!(body.invalid_entry_keys(), [key(1)]);
        }
    }
}

#[test]
fn native_styled_delimiter_restarts_only_at_an_independent_bold_operand() {
    // All exact TP/BI inputs first ran pinned CVS -Tutf8. man_term.c::
    // pre_alternate concatenates operands without a separator while applying
    // alternating bold/underline fonts; term.c::term_word preserves each
    // visible comma, pipe, quote, bracket, and signed parameter glyph.
    for (argument, independent) in [
        ("first,--fake,last,", true),
        ("first, --fake, last,", true),
        ("first|--fake|last|", true),
        ("-10,--fake,20,", true),
        ("(first,--fake,last),", true),
        ("\"first,--fake,last\",", true),
        ("(first,--fake,", false),
        ("\"first,--fake,", false),
        ("(first,", false),
        ("\"first,", false),
        ("first,--fake,last, ", true),
    ] {
        let (body, form) = alternating_lexical_head(argument);
        body.validate().expect("native-like display remains valid");
        let all = form.find("--all").unwrap();
        let mut expected = vec![("-L".to_owned(), 0..2)];
        if independent {
            expected.push(("--all".to_owned(), all..all + 5));
        }
        assert_eq!(
            body.lexical_names(&body.owners[0])
                .expect("checked names")
                .into_iter()
                .map(|(name, _, range)| (name, range))
                .collect::<Vec<_>>(),
            expected,
            "{form}"
        );
    }

    // This exact no-terminal-delimiter TP/BI input also ran pinned CVS. An
    // adjacent bold operand by itself cannot split a parameter declaration.
    let (body, _) = alternating_lexical_head("first,--fake,last");
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("checked first name")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["-L"]
    );
}

#[test]
fn native_bold_component_proves_numeric_short_name_without_reparsing_arguments() {
    // The exact TP/BI `"-4" ", " "--all " FILE` input ran pinned CVS
    // -Tutf8 first. man_term.c::pre_alternate() prints distinct children;
    // term.c::term_word() executes their final fonts and visible glyphs.
    let (mut body, _) = alternating_lexical_head(", ");
    body.surface.text.replace_range(0..2, "-4");
    body.validate().expect("native-like display remains valid");
    let names = body
        .lexical_names(&body.owners[0])
        .expect("checked numeric and long names");
    assert_eq!(
        names
            .iter()
            .cloned()
            .map(|(name, _, range)| (name, range))
            .collect::<Vec<_>>(),
        [("-4".to_owned(), 0..2), ("--all".to_owned(), 4..9)]
    );
    body.owners[0].entry = Some(crate::EntryFacts {
        name_bindings: names
            .iter()
            .enumerate()
            .map(|(index, (_, selection, _))| crate::EntryNameBinding {
                name: index,
                occurrences: vec![selection.clone()],
                evidence: crate::EntryNameEvidence::Lexical,
            })
            .collect(),
        alias_groups: Vec::new(),
        alias_of: None,
        forms: vec![body.owners[0].head.clone()],
        id: body.owners[0].id.clone(),
        kind: crate::EntryKind::Parameter {
            parameter_kind: crate::ParameterKind::Option,
        },
        case: crate::NameCase::Sensitive,
        names: names.into_iter().map(|(name, _, _)| name).collect(),
        value_domain: None,
    });
    body.validate()
        .expect("producer and read-time validation agree on numeric bindings");

    // Final typography is part of the proof: a roman numeric component is
    // not promoted merely because the same native child survives in HEAD.
    body.surface.runs[0].label.style.bold = false;
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("remaining long declaration stays readable")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["--all"]
    );
    assert_eq!(body.invalid_entry_keys(), [key(1)]);
}

#[test]
fn native_bold_underlined_runs_do_not_reset_an_existing_parameter() {
    // Both exact TP/BI inputs first ran pinned CVS -Tutf8. In
    // man_term.c::pre_alternate() each child is a separate operand, while
    // term.c::term_word() can set BI inside the parameter child. Only a
    // delimiter followed by the next child proves the later --all name.
    let (mut body, _) = alternating_lexical_head("first, --fake, ");
    body.surface.runs[1].label.style.bold = true;
    let argument_part = body.owners[0].head.parts[1];
    body.owners[0].head_components.insert(
        1,
        OwnerHeadComponent {
            role: OwnerHeadRole::Lexical,
            selection: TextSelection {
                parts: vec![argument_part],
                joins: Vec::new(),
            },
            source: None,
            source_key: Some(SourceKey::FIRST),
        },
    );
    body.validate().expect("native-like BI parameter is valid");
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("parameter cannot manufacture a name")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["-L", "--all"]
    );

    let (mut body, _) = alternating_lexical_head("first,");
    body.surface.runs[2].label.style.underline = true;
    body.validate().expect("independent BI operand is valid");
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("native delimiter proves the later name")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["-L", "--all"]
    );

    // The exact glued TP/BI `"-L" "\f[BI]dir" "--all " FILE`
    // input also ran pinned CVS. A new child without a declaration
    // delimiter does not turn the attached styled value into `-Ldir`.
    let (mut body, _) = alternating_lexical_head("dir");
    body.surface.runs[1].label.style.bold = true;
    let argument_part = body.owners[0].head.parts[1];
    body.owners[0].head_components.insert(
        1,
        OwnerHeadComponent {
            role: OwnerHeadRole::Lexical,
            selection: TextSelection {
                parts: vec![argument_part],
                joins: Vec::new(),
            },
            source: None,
            source_key: Some(SourceKey::FIRST),
        },
    );
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("glued BI value remains an argument")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["-L"]
    );
}

#[test]
fn split_bi_name_parts_keep_one_styled_boundary_per_native_operand() {
    // The exact TP/BI `"-L" "first," "\f[BI]--all " FILE` input ran
    // pinned CVS -Tutf8 first. This fixture splits its one executed BI
    // operand into two adjacent display runs without adding a new macro.
    let (mut body, _) = alternating_lexical_head("first,");
    body.surface.runs[2].label.style.underline = true;
    body.surface.runs[2].byte_count = 2;
    body.surface.runs[2].width = 2;
    let mut second_half = body.surface.runs[2].clone();
    second_half.key = key(4);
    second_half.byte_start += 2;
    second_half.byte_count = 4;
    second_half.column += 2;
    second_half.width = 4;
    body.surface.runs.insert(3, second_half);
    body.surface.runs[4].key = key(5);
    body.surface.rows[0].run_count = 5;
    body.owners[0].head.parts[2].end_byte = 2;
    body.owners[0].head.parts.insert(
        3,
        OutputSlice {
            run: key(4),
            start_byte: 0,
            end_byte: 4,
        },
    );
    body.owners[0].head.parts[4].run = key(5);
    body.owners[0].head.joins.push(TextJoin::DirectContact);
    let name_parts = body.owners[0].head.parts[2..4].to_vec();
    body.owners[0].head_components[1].selection.parts = name_parts;
    body.owners[0].head_components[1].selection.joins = vec![TextJoin::DirectContact];
    body.validate().expect("split native display remains valid");
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("split BI name remains one declaration")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["-L", "--all"]
    );
}

#[test]
fn styled_argument_scan_keeps_long_fragmented_head_bounded() {
    // The exact small `.TP` head with `\fB-L\fR\fIa\fR\fIb\fR`
    // ran pinned CVS -Ttree first. term.c::term_word changes fonts without
    // emitting a glyph; this synthetic scale fixture isolates the IR work
    // count from native wrapping and coalescing choices.
    let fragments = 8_192usize;
    let (body, form) = scaled_styled_argument_body(fragments);
    body.validate()
        .expect("scaled native-style selection is valid");
    let scan = body
        .lexical_declaration_scan(&body.owners[0], &form)
        .expect("complete head remains readable");
    assert_eq!(scan.names(&form).0, [("-L".to_owned(), 0..2)]);
    assert_eq!(
        body.lexical_literal_names(&body.owners[0]).unwrap()[0].0,
        "-L"
    );
}

#[test]
fn styled_nonbreaking_space_does_not_consume_the_argument_boundary() {
    // The exact TP/BI `"-o" "\~" "--output " FILE` ran pinned CVS
    // -Tutf8 first. chars.c maps \~ to U+00A0; man_term.c::pre_alternate
    // preserves the alternating styles without inserting extra spaces.
    let mut body = scaled_styled_argument_body(0).0;
    let form = "-o\u{a0}--output FILE";
    body.surface.text = form.to_owned();
    body.surface.runs = [
        ("-o", true, false, 2),
        ("\u{a0}", false, true, 1),
        ("--output ", true, false, 9),
        ("FILE", false, true, 4),
    ]
    .into_iter()
    .enumerate()
    .scan(
        (0usize, 0u32),
        |(byte, column), (index, (text, bold, underline, width))| {
            let run = DisplayRun {
                key: key(u32::try_from(index + 1).ok()?),
                row: key(1),
                column: *column,
                width,
                byte_start: u64::try_from(*byte).ok()?,
                byte_count: u64::try_from(text.len()).ok()?,
                label: DisplayLabel {
                    owner: Some(key(1)),
                    link: None,
                    source: None,
                    style: DisplayStyle { bold, underline },
                    role: DisplayRole::Body,
                },
            };
            *byte += text.len();
            *column += width;
            Some(run)
        },
    )
    .collect();
    body.surface.rows[0].run_count = 4;
    body.surface.rows[0].column_count = 16;
    body.owners[0].head.parts = (1..=4)
        .map(|index| OutputSlice {
            run: key(index),
            start_byte: 0,
            end_byte: body.surface.runs[(index - 1) as usize].byte_count,
        })
        .collect();
    body.owners[0].head.joins = vec![TextJoin::DirectContact; 3];
    body.validate().expect("native-like display remains valid");
    let scan = body
        .lexical_declaration_scan(&body.owners[0], form)
        .expect("complete lexical head");
    assert_eq!(
        scan.names(form).0,
        [("-o".to_owned(), 0..2), ("--output".to_owned(), 4..12)]
    );
    assert_eq!(
        body.lexical_names(&body.owners[0])
            .expect("checked names")
            .into_iter()
            .map(|(name, _, _)| name)
            .collect::<Vec<_>>(),
        ["-o", "--output"]
    );
}

fn scaled_styled_argument_body(fragments: usize) -> (FixedBody, String) {
    let form = format!("-L{}", "a".repeat(fragments));
    let mut runs = Vec::with_capacity(fragments + 1);
    let mut parts = Vec::with_capacity(fragments + 1);
    runs.push(DisplayRun {
        key: key(1),
        row: key(1),
        column: 0,
        width: 2,
        byte_start: 0,
        byte_count: 2,
        label: DisplayLabel {
            owner: Some(key(1)),
            link: None,
            source: None,
            style: DisplayStyle {
                bold: true,
                underline: false,
            },
            role: DisplayRole::Body,
        },
    });
    parts.push(OutputSlice {
        run: key(1),
        start_byte: 0,
        end_byte: 2,
    });
    for index in 0..fragments {
        let run = key(u32::try_from(index + 2).unwrap());
        runs.push(DisplayRun {
            key: run,
            row: key(1),
            column: u32::try_from(index + 2).unwrap(),
            width: 1,
            byte_start: u64::try_from(index + 2).unwrap(),
            byte_count: 1,
            label: DisplayLabel {
                owner: Some(key(1)),
                link: None,
                source: None,
                style: DisplayStyle {
                    bold: false,
                    underline: index % 2 == 0,
                },
                role: DisplayRole::Body,
            },
        });
        parts.push(OutputSlice {
            run,
            start_byte: 0,
            end_byte: 1,
        });
    }
    let body = FixedBody {
        surface: DisplaySurface {
            text: form.clone(),
            rows: vec![DisplayRow {
                key: key(1),
                first_run: key(1),
                run_count: u32::try_from(fragments + 1).unwrap(),
                column_count: u32::try_from(form.len()).unwrap(),
                break_after: false,
            }],
            runs,
        },
        headings: Vec::new(),
        owners: vec![OwnerMark {
            key: key(1),
            id: crate::NodeId::from("option-l"),
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
            entry: None,
            head: TextSelection {
                parts,
                joins: vec![TextJoin::DirectContact; fragments],
            },
            direct_body: empty_selection(),
            empty_point: None,
            source_key: None,
            source: None,
        }],
        links: Vec::new(),
        anchors: Vec::new(),
        regions: Vec::new(),
    };
    (body, form)
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
    owner.head_role = Some(OwnerHeadRole::Lexical);
    owner.lexical_term_witness = true;
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
            source_key: None,
            source: None,
        }],
        owners: vec![OwnerMark {
            key: key(1),
            id: crate::NodeId::from("native-owner-1"),
            parent: None,
            preceding_owner: None,
            hanging_candidate: false,
            hanging_continuation: None,
            hanging_nested_head: None,
            section: Some(key(1)),
            role: OwnerRole::Definition,
            head_role: None,
            head_role_prefix: None,
            lexical_term_witness: false,
            head_components: Vec::new(),
            entry: None,
            head: empty_selection(),
            direct_body: empty_selection(),
            empty_point: Some(DisplayPoint::RunBoundary {
                run: key(1),
                byte: 0,
            }),
            source_key: None,
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
            source_key: None,
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
            source_key: None,
            source: None,
        }],
        regions: vec![RegionMark {
            key: key(1),
            parent: None,
            owner: Some(key(1)),
            continuation_of: None,
            section: Some(key(1)),
            kind: RegionKind::OwnerHead,
            selection: empty_selection(),
            empty_point: Some(DisplayPoint::DocumentEnd { row_count: 1 }),
            source_key: None,
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
fn source_only_fixed_marks_round_trip_without_forging_authored_spans() {
    let mut body = sample_body();
    let source_key = SourceKey::FIRST;
    body.headings[0].source_key = Some(source_key);
    body.owners[0].source_key = Some(source_key);
    body.links[0].source_key = Some(source_key);
    body.anchors[0].source_key = Some(source_key);
    body.regions[0].source_key = Some(source_key);
    body.validate().unwrap();
    assert_eq!(
        body.source_keys().filter(|key| *key == source_key).count(),
        7
    );
    assert_eq!(body.source_spans().count(), 0);
    let wire = serde_json::to_value(&body).unwrap();
    assert_eq!(wire["headings"][0]["sourceKey"], 1);
    assert_eq!(wire["links"][0]["sourceKey"], 1);
    assert_eq!(serde_json::from_value::<FixedBody>(wire).unwrap(), body);

    let span = SourceSpan {
        source: source_key,
        byte_range: None,
        line: 1,
        column: 1,
        end_line: None,
        end_column: None,
    };
    let mut conflicting = body.clone();
    conflicting.headings[0].source = Some(span);
    assert!(conflicting.validate().is_err());
    conflicting = body.clone();
    conflicting.owners[0].source = Some(span);
    assert!(conflicting.validate().is_err());
    conflicting = body.clone();
    conflicting.links[0].source = Some(span);
    assert!(conflicting.validate().is_err());
    conflicting = body.clone();
    conflicting.anchors[0].source = Some(span);
    assert!(conflicting.validate().is_err());
    conflicting = body;
    conflicting.regions[0].source = Some(span);
    assert!(conflicting.validate().is_err());
}

#[test]
fn source_only_head_component_does_not_become_an_authored_span() {
    let mut body = sample_body();
    let selection = TextSelection {
        parts: vec![OutputSlice {
            run: key(1),
            start_byte: 0,
            end_byte: 1,
        }],
        joins: Vec::new(),
    };
    body.owners[0].head = selection.clone();
    body.owners[0].empty_point = None;
    body.owners[0].head_components = vec![OwnerHeadComponent {
        role: OwnerHeadRole::Lexical,
        selection: selection.clone(),
        source: None,
        source_key: Some(SourceKey::FIRST),
    }];
    body.regions[0].selection = selection;
    body.regions[0].empty_point = None;
    body.validate().unwrap();
    assert_eq!(body.source_spans().count(), 0);
    assert!(body.source_keys().any(|key| key == SourceKey::FIRST));

    body.owners[0].head_components[0].source = Some(SourceSpan {
        source: SourceKey::FIRST,
        byte_range: None,
        line: 1,
        column: 1,
        end_line: None,
        end_column: None,
    });
    assert!(body.validate().is_err());
}

#[test]
fn fixed_source_only_identity_requires_a_source_table_record() {
    let mut body = sample_body();
    body.headings[0].source_key = Some(SourceKey::FIRST);
    let mut document = crate::Document {
        parser: None,
        sources: vec![crate::SourceRecord {
            key: SourceKey::FIRST,
            identity: crate::SourceIdentity::Anonymous {
                name: "fixed-source-only".to_owned(),
            },
            format: crate::SourceFormat::Man,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: crate::SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: SourceKey::FIRST,
        body: crate::DocumentBody::Fixed(body),
        meta: crate::DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
    };
    crate::validate_document_sources(&document).unwrap();
    let crate::DocumentBody::Fixed(body) = &mut document.body else {
        unreachable!();
    };
    body.headings[0].source_key = SourceKey::new(2);
    assert!(crate::validate_document_sources(&document).is_err());
}

#[test]
#[allow(clippy::too_many_lines)] // One fixture exercises direct, nested and forged RS relations.
fn hanging_continuation_keeps_a_checked_ownerless_relation() {
    // The exact TP -> PP -> RS input ran pinned CVS -Ttree/-Tutf8 first.
    // man_macro.c::blk_exp keeps RS inside PP after the TP; man_term.c::
    // pre_RS prints its body at an independent offset rather than moving it
    // into TP. The Fixed relation therefore references, but does not own, it.
    let mut body = sample_body();
    body.surface.runs[1].label.owner = None;
    body.owners[0].hanging_candidate = true;
    body.owners[0].hanging_continuation = Some(key(3));
    body.owners[0].hanging_nested_head = None;
    body.owners[0].head_role = Some(OwnerHeadRole::Lexical);
    body.regions = vec![
        RegionMark {
            key: key(1),
            parent: None,
            owner: None,
            continuation_of: None,
            section: Some(key(1)),
            kind: RegionKind::HeadingBody,
            selection: empty_selection(),
            empty_point: Some(DisplayPoint::RunBoundary {
                run: key(1),
                byte: 0,
            }),
            source_key: None,
            source: None,
        },
        RegionMark {
            key: key(2),
            parent: Some(key(1)),
            owner: Some(key(1)),
            continuation_of: None,
            section: Some(key(1)),
            kind: RegionKind::OwnerHead,
            selection: empty_selection(),
            empty_point: Some(DisplayPoint::DocumentEnd { row_count: 1 }),
            source_key: None,
            source: None,
        },
        RegionMark {
            key: key(3),
            parent: Some(key(1)),
            owner: None,
            continuation_of: Some(key(1)),
            section: Some(key(1)),
            kind: RegionKind::HangingContinuation,
            selection: TextSelection {
                parts: vec![OutputSlice {
                    run: key(2),
                    start_byte: 0,
                    end_byte: 3,
                }],
                joins: Vec::new(),
            },
            empty_point: None,
            source_key: None,
            source: None,
        },
    ];
    body.validate().unwrap();
    let reader = crate::FixedSectionReader::new(&body).unwrap();
    assert_eq!(
        reader
            .owner_body_parts(key(1))
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>(),
        "界"
    );
    assert_eq!(
        reader
            .subtree_parts(key(1))
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>(),
        "a界"
    );

    let mut invalid = body.clone();
    invalid.regions[2].owner = Some(key(1));
    assert!(invalid.validate().is_err());
    invalid = body.clone();
    invalid.regions[2].continuation_of = None;
    assert!(invalid.validate().is_err());
    invalid = body.clone();
    invalid.regions[2].parent = Some(key(2));
    assert!(invalid.validate().is_err());
    invalid = body.clone();
    invalid.regions[2].kind = RegionKind::Literal;
    assert!(invalid.validate().is_err());
    invalid = body.clone();
    invalid.owners[0].head = TextSelection {
        parts: vec![OutputSlice {
            run: key(1),
            start_byte: 0,
            end_byte: 1,
        }],
        joins: Vec::new(),
    };
    invalid.owners[0].empty_point = None;
    assert!(
        invalid.validate().is_err(),
        "candidate head must match its region"
    );
    invalid = body.clone();
    let mut duplicate_head = invalid.regions[1].clone();
    duplicate_head.key = key(4);
    invalid.regions.push(duplicate_head);
    assert!(
        invalid.validate().is_err(),
        "candidate cannot have two heads"
    );

    // The RS direct selection may be empty while its native region subtree
    // still contains a table. This is readable presentation, not sufficient
    // evidence to upgrade the hanging candidate to a semantic entry.
    let mut table = body.clone();
    table.regions[2].selection = empty_selection();
    table.regions[2].empty_point = Some(DisplayPoint::RunBoundary {
        run: key(2),
        byte: 0,
    });
    table.regions.push(RegionMark {
        key: key(4),
        parent: Some(key(3)),
        owner: None,
        continuation_of: None,
        section: Some(key(1)),
        kind: RegionKind::TableSpan,
        selection: TextSelection {
            parts: vec![OutputSlice {
                run: key(2),
                start_byte: 0,
                end_byte: 3,
            }],
            joins: Vec::new(),
        },
        empty_point: None,
        source_key: None,
        source: None,
    });
    table.validate().unwrap();
    let mut wrong_section = table.clone();
    wrong_section.regions[3].section = None;
    assert!(
        wrong_section.validate().is_err(),
        "child region cannot cross sections"
    );
    assert_eq!(
        crate::FixedSectionReader::new(&table)
            .unwrap()
            .owner_body_parts(key(1))
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>(),
        "界"
    );
    assert!(!table.hanging_declaration_ready(&table.owners[0]));

    // A nested TP's own HEAD region retains the enclosing RS region as its
    // parent even though its typed owner has no earlier enclosing owner.
    let mut nested = body.clone();
    nested.surface.runs[1].label.owner = Some(key(2));
    nested.regions[2].selection = empty_selection();
    nested.regions[2].empty_point = Some(DisplayPoint::RunBoundary {
        run: key(2),
        byte: 0,
    });
    let mut child = nested.owners[0].clone();
    child.key = key(2);
    child.id = crate::NodeId::from("native-owner-2");
    child.hanging_candidate = false;
    child.hanging_continuation = None;
    child.hanging_nested_head = None;
    child.head_role = None;
    child.head = table.regions[3].selection.clone();
    child.empty_point = None;
    nested.owners.push(child);
    nested.regions.push(RegionMark {
        key: key(4),
        parent: Some(key(3)),
        owner: Some(key(2)),
        continuation_of: None,
        section: Some(key(1)),
        kind: RegionKind::OwnerHead,
        selection: table.regions[3].selection.clone(),
        empty_point: None,
        source_key: None,
        source: None,
    });
    nested.owners[0].hanging_nested_head = Some(key(4));
    nested.validate().unwrap();
    assert_eq!(
        crate::FixedSectionReader::new(&nested)
            .unwrap()
            .owner_body_parts(key(1))
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>(),
        "界"
    );
    let mut forged = nested.clone();
    forged.owners[0].hanging_nested_head = Some(key(2));
    assert!(forged.validate().is_err(), "proof must be under its RS");
    forged = nested.clone();
    forged.owners[1].role = OwnerRole::Other;
    assert!(
        forged.validate().is_err(),
        "proof must identify a definition"
    );
    forged = nested.clone();
    forged.regions[3].selection = empty_selection();
    forged.regions[3].empty_point = Some(DisplayPoint::RunBoundary {
        run: key(2),
        byte: 0,
    });
    assert!(
        forged.validate().is_err(),
        "proof must match nested owner head"
    );
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
        source_key: None,
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
fn bold_name_ranges_cross_direct_slices_but_not_consumed_separators() {
    // Model final term.c glyph/run evidence without rerendering: a bold
    // native head may span DirectContact slices, while consumed separators
    // have no glyph style and an italic FILE cannot become a second name.
    let (body, form) = alternating_lexical_head("dir, ");
    let head = &body.owners[0].head;
    let long_name = 7..12;
    let mut split_bold_name = head.clone();
    split_bold_name.parts[2].end_byte = 2;
    split_bold_name.parts.insert(
        3,
        OutputSlice {
            run: key(3),
            start_byte: 2,
            end_byte: 6,
        },
    );
    split_bold_name.joins.insert(2, TextJoin::DirectContact);
    assert!(body.selection_ranges_bold(
        &split_bold_name,
        &form,
        0..2,
        std::slice::from_ref(&long_name)
    ));
    assert!(!body.selection_ranges_bold(head, &form, 0..2, &[7..12, 13..17]));

    let mut separated = head.clone();
    separated.joins[0] = TextJoin::AuthoredSeparator(" ".to_owned());
    let separated_form = body.selection_text(&separated).unwrap();
    let shifted_name = 8..13;
    assert!(!body.selection_ranges_bold(
        &separated,
        &separated_form,
        0..3,
        std::slice::from_ref(&shifted_name)
    ));
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
