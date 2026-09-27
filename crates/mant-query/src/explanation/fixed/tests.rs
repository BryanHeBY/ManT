use std::num::NonZeroU32;

use mant_ir::{
    DisplayLabel, DisplayPoint, DisplayRole, DisplayRow, DisplayRun, DisplayStyle, DisplaySurface,
    DocumentBody, DocumentMeta, HeadingMark, LinkMark, LinkTarget, NodeId, OutputSlice, OwnerRole,
    SourceCoordinates, SourceFormat, SourceIdentity, SourceKey, SourceRecord, TextJoin,
    TextSelection,
};
use mant_protocol::{
    ContentSelector, EntryProjection, EvidenceBasis, ExplanationContent, ExplanationFormMatch,
    ExplanationNameMatch, OutlineNode,
};

use super::*;

fn key(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("fixture key is positive")
}

fn slices(keys: &[u32], joins: Vec<TextJoin>, lengths: &[u64]) -> TextSelection {
    TextSelection {
        parts: keys
            .iter()
            .map(|&run| OutputSlice {
                run: key(run),
                start_byte: 0,
                end_byte: lengths[(run - 1) as usize],
            })
            .collect(),
        joins,
    }
}

#[allow(clippy::too_many_lines)] // One complete Fixed snapshot exposes every tested owner/section edge.
fn fixture() -> ResolvedContent {
    // This is a synthetic, already-final Fixed snapshot: it tests IR and
    // projection contracts only, not a claim about roff line wrapping.
    let texts = [
        "PREFACE",
        "OPTIONS",
        "printf",
        "(3)",
        "first body",
        "empty body",
        "NESTED",
        "child text",
    ];
    let lengths = texts.map(|text| text.len() as u64);
    let mut arena = String::new();
    let mut rows = Vec::new();
    let mut runs = Vec::new();
    for (index, text) in texts.iter().enumerate() {
        let number = u32::try_from(index + 1).unwrap();
        let owner = match number {
            3..=5 => Some(key(1)),
            6 => Some(key(2)),
            _ => None,
        };
        let link = (3..=4).contains(&number).then(|| key(1));
        rows.push(DisplayRow {
            key: key(number),
            first_run: key(number),
            run_count: 1,
            column_count: u32::try_from(text.len()).unwrap(),
            break_after: index + 1 < texts.len(),
        });
        runs.push(DisplayRun {
            key: key(number),
            row: key(number),
            column: 0,
            width: u32::try_from(text.len()).unwrap(),
            byte_start: u64::try_from(arena.len()).unwrap(),
            byte_count: lengths[index],
            label: DisplayLabel {
                owner,
                link,
                source: Some(SourceKey::FIRST),
                style: DisplayStyle {
                    bold: false,
                    underline: false,
                },
                role: DisplayRole::Body,
            },
        });
        arena.push_str(text);
    }
    let hard = || TextJoin::HardBoundary;
    let mut fixed = FixedBody {
        root_configuration_hint: false,
        surface: DisplaySurface {
            text: arena,
            rows,
            runs,
        },
        headings: vec![
            HeadingMark {
                key: key(1),
                id: NodeId::from("options"),
                fragment_aliases: Vec::new(),
                generated_fragment_aliases: Vec::new(),
                rendered_fragment_aliases: Vec::new(),
                parent: None,
                level_hint: 1,
                at: DisplayPoint::RunBoundary {
                    run: key(2),
                    byte: 0,
                },
                title: slices(&[2], Vec::new(), &lengths),
                direct_body: slices(&[3, 4, 5, 6], vec![hard(), hard(), hard()], &lengths),
                source_key: None,
                source: None,
            },
            HeadingMark {
                key: key(2),
                id: NodeId::from("nested"),
                fragment_aliases: Vec::new(),
                generated_fragment_aliases: Vec::new(),
                rendered_fragment_aliases: Vec::new(),
                parent: Some(key(1)),
                level_hint: 2,
                at: DisplayPoint::RunBoundary {
                    run: key(7),
                    byte: 0,
                },
                title: slices(&[7], Vec::new(), &lengths),
                direct_body: slices(&[8], Vec::new(), &lengths),
                source_key: None,
                source: None,
            },
        ],
        owners: vec![
            OwnerMark {
                key: key(1),
                id: NodeId::from("owner-printf"),
                parent: None,
                preceding_owner: None,
                hanging_candidate: false,
                hanging_continuation: None,
                hanging_nested_head: None,
                section: Some(key(1)),
                role: OwnerRole::Definition,
                head_role: Some(mant_ir::OwnerHeadRole::Lexical),
                head_role_prefix: None,
                lexical_term_witness: true,
                head_components: Vec::new(),
                entry: None,
                head: slices(&[3, 4], vec![TextJoin::DirectContact], &lengths),
                direct_body: slices(&[5], Vec::new(), &lengths),
                empty_point: None,
                source_key: None,
                source: None,
            },
            OwnerMark {
                key: key(2),
                id: NodeId::from("owner-empty"),
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
                head: TextSelection {
                    parts: Vec::new(),
                    joins: Vec::new(),
                },
                direct_body: slices(&[6], Vec::new(), &lengths),
                empty_point: None,
                source_key: None,
                source: None,
            },
        ],
        links: vec![LinkMark {
            key: key(1),
            section: None,
            owner: None,
            target: Some(LinkTarget::External {
                uri: "https://example.test/printf".into(),
            }),
            label: slices(&[3, 4], vec![TextJoin::DirectContact], &lengths),
            source_key: None,
            source: None,
        }],
        anchors: Vec::new(),
        regions: Vec::new(),
    };
    let form = fixed.owners[0].head.clone();
    fixed.owners[0].entry = Some(mant_ir::EntryFacts {
        name_bindings: vec![mant_ir::EntryNameBinding {
            name: 0,
            occurrences: vec![form.clone()],
            evidence: mant_ir::EntryNameEvidence::Lexical,
        }],
        alias_groups: Vec::new(),
        alias_of: None,
        forms: vec![form],
        id: fixed.owners[0].id.clone(),
        kind: mant_ir::EntryKind::Term,
        case: mant_ir::NameCase::Sensitive,
        names: vec!["printf(3)".into()],
        value_domain: None,
    });
    fixed.validate().expect("self-contained Fixed fixture");
    ResolvedContent {
        label: "Fixed fixture".into(),
        address: None,
        document: Some(Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "synthetic-fixed".into(),
                },
                format: SourceFormat::Man,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::NativeNormalizedBytes,
            }],
            root_source: SourceKey::FIRST,
            body: DocumentBody::Fixed(fixed),
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }),
        tldr: None,
    }
}

#[test]
fn hinted_fixed_head_keeps_later_name_positions_in_one_form() {
    // Both exact `.TP`/`.B` inputs ran pinned CVS -Ttree.  man_term.c::
    // pre_TP keeps one visible HEAD; a first-name hint hides neither a
    // second distinct name nor a repeated occurrence of that same name.
    for (form, names, occurrences, expected) in [
        (
            "-a ARG, --all",
            vec!["-a", "--all"],
            vec![("-a", 0, 2), ("--all", 8, 13)],
            vec![vec![vec![(0, 0, 2)]], vec![vec![(0, 8, 13)]]],
        ),
        (
            "-a ARG, -a",
            vec!["-a"],
            vec![("-a", 0, 2), ("-a", 8, 10)],
            vec![vec![vec![(0, 0, 2)], vec![(0, 8, 10)]]],
        ),
    ] {
        let fixed = hinted_fixed_body(form, &names, &occurrences);
        fixed
            .validate()
            .expect("query fixture is a valid Fixed body");
        let entry = mant_ir::SemanticEntry {
            id: NodeId::from("option-a"),
            kind: mant_ir::EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            names: names.iter().map(|name| (*name).into()).collect(),
            alias_groups: Vec::new(),
            alias_of: None,
            case: mant_ir::NameCase::Sensitive,
            forms: vec![form.into()],
            document_targets: Vec::new(),
            children: Vec::new(),
            value_domain: None,
        };
        let actual = super::selection::fixed_name_positions(&fixed, &fixed.owners[0], &entry)
            .unwrap()
            .iter()
            .map(|occurrences| {
                occurrences
                    .iter()
                    .map(|forms| {
                        forms
                            .iter()
                            .map(|range| (range.form_index, range.start_scalar, range.end_scalar))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{form}");
    }
}

#[test]
fn fixed_name_binding_projection_uses_indices_occurrences_and_scalar_boundaries() {
    // This is a detached mapping unit test, not a claim that the current IR
    // validator accepts duplicate explicit forms: that closure is part of
    // EN02's separate Fixed fact change. Pinned CVS man_term.c::pre_TP and
    // term.c::term_word execute the complete HEAD; ManT's semantic names are
    // mapped against surviving run bytes, never a same-spelling substring.
    let form = "é TMPDIR, TEMP, TMP";
    let names = ["TEMP", "TMP", "TMPDIR"];
    let mut fixed = hinted_fixed_body(
        form,
        &names,
        &[("TMPDIR", 3, 9), ("TEMP", 11, 15), ("TMP", 17, 20)],
    );
    let facts = fixed.owners[0].entry.as_mut().unwrap();
    facts.name_bindings.swap(0, 2);
    facts.forms.push(facts.forms[0].clone());
    let entry = mant_ir::SemanticEntry {
        id: NodeId::from("option-a"),
        kind: facts.kind,
        names: names.iter().map(|name| (*name).into()).collect(),
        alias_groups: Vec::new(),
        alias_of: None,
        case: mant_ir::NameCase::Sensitive,
        forms: vec![form.into(), form.into()],
        document_targets: Vec::new(),
        children: Vec::new(),
        value_domain: None,
    };
    let positions =
        super::selection::fixed_name_positions(&fixed, &fixed.owners[0], &entry).unwrap();
    let projected = positions
        .iter()
        .map(|occurrences| {
            occurrences
                .iter()
                .map(|forms| {
                    forms
                        .iter()
                        .map(|range| (range.form_index, range.start_scalar, range.end_scalar))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projected,
        [
            vec![vec![(0, 10, 14), (1, 10, 14)]],
            vec![vec![(0, 16, 19), (1, 16, 19)]],
            vec![vec![(0, 2, 8), (1, 2, 8)]],
        ]
    );
    fixed.owners[0].entry.as_mut().unwrap().name_bindings[0].occurrences[0].parts[0].start_byte = 4;
    assert!(
        super::selection::fixed_name_positions(&fixed, &fixed.owners[0], &entry).is_err(),
        "wrong native bytes cannot be rescued by a same-spelling substring",
    );
}

fn hinted_fixed_body(form: &str, names: &[&str], occurrences: &[(&str, u64, u64)]) -> FixedBody {
    let length = u64::try_from(form.len()).unwrap();
    let head = slices(&[1], Vec::new(), &[length]);
    let bindings = names
        .iter()
        .enumerate()
        .map(|(index, name)| mant_ir::EntryNameBinding {
            name: index,
            occurrences: occurrences
                .iter()
                .filter(|(candidate, _, _)| candidate == name)
                .map(|(_, start, end)| TextSelection {
                    parts: vec![OutputSlice {
                        run: key(1),
                        start_byte: *start,
                        end_byte: *end,
                    }],
                    joins: Vec::new(),
                })
                .collect(),
            evidence: mant_ir::EntryNameEvidence::Lexical,
        })
        .collect();
    FixedBody {
        root_configuration_hint: false,
        surface: DisplaySurface {
            text: form.into(),
            rows: vec![DisplayRow {
                key: key(1),
                first_run: key(1),
                run_count: 1,
                column_count: u32::try_from(form.len()).unwrap(),
                break_after: false,
            }],
            runs: vec![DisplayRun {
                key: key(1),
                row: key(1),
                column: 0,
                width: u32::try_from(form.len()).unwrap(),
                byte_start: 0,
                byte_count: length,
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
            }],
        },
        headings: Vec::new(),
        owners: vec![OwnerMark {
            key: key(1),
            id: NodeId::from("option-a"),
            parent: None,
            preceding_owner: None,
            hanging_candidate: false,
            hanging_continuation: None,
            hanging_nested_head: None,
            section: None,
            role: OwnerRole::Definition,
            head_role: Some(mant_ir::OwnerHeadRole::Lexical),
            head_role_prefix: Some("-a".into()),
            lexical_term_witness: false,
            head_components: Vec::new(),
            entry: Some(mant_ir::EntryFacts {
                name_bindings: bindings,
                alias_groups: Vec::new(),
                alias_of: None,
                forms: vec![head.clone()],
                id: NodeId::from("option-a"),
                kind: mant_ir::EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option,
                },
                case: mant_ir::NameCase::Sensitive,
                names: names.iter().map(|name| (*name).into()).collect(),
                value_domain: None,
            }),
            head,
            direct_body: TextSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            },
            empty_point: None,
            source_key: None,
            source: None,
        }],
        links: Vec::new(),
        anchors: Vec::new(),
        regions: Vec::new(),
    }
}

fn two_form_option_fixture() -> ResolvedContent {
    let mut fixed = hinted_fixed_body(
        "-a, --all",
        &["-a", "--all"],
        &[("-a", 0, 2), ("--all", 4, 9)],
    );
    let lengths = [2, 2, 5];
    let mut runs = Vec::new();
    for (index, (start, length)) in [(0, 2), (2, 2), (4, 5)].into_iter().enumerate() {
        let mut run = fixed.surface.runs[0].clone();
        run.key = key(u32::try_from(index + 1).unwrap());
        run.column = start;
        run.width = length;
        run.byte_start = u64::from(start);
        run.byte_count = u64::from(length);
        runs.push(run);
    }
    fixed.surface.runs = runs;
    fixed.surface.rows[0].run_count = 3;
    let head = slices(
        &[1, 2, 3],
        vec![TextJoin::DirectContact, TextJoin::DirectContact],
        &lengths,
    );
    let first = slices(&[1], Vec::new(), &lengths);
    let second = slices(&[3], Vec::new(), &lengths);
    let owner = &mut fixed.owners[0];
    owner.head_role = Some(mant_ir::OwnerHeadRole::Option);
    owner.head = head;
    owner.head_components = [first.clone(), second.clone()]
        .into_iter()
        .map(|selection| mant_ir::OwnerHeadComponent {
            role: mant_ir::OwnerHeadRole::Option,
            selection,
            source: None,
            source_key: Some(SourceKey::FIRST),
        })
        .collect();
    let facts = owner.entry.as_mut().unwrap();
    facts.forms = vec![first.clone(), second.clone()];
    facts.name_bindings = [first, second]
        .into_iter()
        .enumerate()
        .map(|(name, selection)| mant_ir::EntryNameBinding {
            name,
            occurrences: vec![selection],
            evidence: mant_ir::EntryNameEvidence::NativeMarkup,
        })
        .collect();
    fixed
        .validate()
        .expect("native Fl component forms are valid");
    ResolvedContent {
        label: "two form option".into(),
        address: None,
        document: Some(Document {
            parser: None,
            sources: vec![SourceRecord {
                key: SourceKey::FIRST,
                identity: SourceIdentity::Anonymous {
                    name: "synthetic-options".into(),
                },
                format: SourceFormat::Man,
                decoded_byte_length: 0,
                content_sha256: None,
                coordinates: SourceCoordinates::NativeNormalizedBytes,
            }],
            root_source: SourceKey::FIRST,
            body: DocumentBody::Fixed(fixed),
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
        }),
        tldr: None,
    }
}

#[test]
fn fixed_explain_projects_checked_multiple_forms_and_only_the_matched_name_basis() {
    // The exact `.It Fl a , Fl -all` input ran pinned CVS -Tutf8 first.
    // mdoc_term.c::termp_fl_pre emits each Fl name separately;
    // this synthetic snapshot tests the already-validated Fixed DTO boundary,
    // not a new roff execution expectation.
    let resolved = two_form_option_fixture();
    let result = super::super::select_explanation(&resolved, "--all").unwrap();
    assert_eq!(result.counts.direct_entry.total, 1);
    let record = &result.evidence[0];
    let entry = record.entry.as_ref().expect("bounded Fixed entry details");
    assert_eq!(entry.names, ["-a", "--all"]);
    assert_eq!(entry.fixed_forms.len(), 2);
    assert_eq!(entry.fixed_forms[0].complete_text().as_deref(), Some("-a"));
    assert_eq!(
        entry.fixed_forms[1].complete_text().as_deref(),
        Some("--all")
    );
    assert_eq!(entry.name_bindings.len(), 2);
    assert_eq!(entry.name_bindings[0].name_index, 0);
    assert_eq!(entry.name_bindings[1].name_index, 1);
    assert_eq!(
        entry.name_bindings[0].occurrences[0].fixed_forms[0].form_index,
        0
    );
    assert_eq!(
        entry.name_bindings[1].occurrences[0].fixed_forms[0].form_index,
        1
    );
    let matched = record
        .bases
        .iter()
        .find_map(|basis| match basis {
            EvidenceBasis::Name { matches } => Some(matches),
            _ => None,
        })
        .expect("one Name basis");
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].name, "--all");
    assert_eq!(matched[0].occurrences, entry.name_bindings[1].occurrences);
    result.validate_references().unwrap();
}

#[test]
fn complete_linked_cross_row_head_is_the_only_semantic_entry() {
    let resolved = fixture();
    let document = resolved.document.as_ref().unwrap();
    let index = SemanticIndex::build(document);
    let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
        unreachable!()
    };
    assert_eq!(fixed.links[0].label, fixed.owners[0].head);
    assert!(index.root().is_empty());
    assert_eq!(index.section("options").len(), 1);
    assert_eq!(index.section("options")[0].forms, ["printf(3)"]);
    assert_eq!(index.section("options")[0].names, ["printf(3)"]);
    assert!(index.section("nested").is_empty());

    let outline =
        crate::projection::build_outline_projection(&resolved, EntryProjection::All, None)
            .expect("Fixed semantic outline");
    assert_eq!(outline.nodes.len(), 2); // root preface, then OPTIONS
    assert_eq!(outline.nodes[0].path(), "root");
    let section = &outline.nodes[1];
    assert_eq!(section.path(), "1");
    assert_eq!(section.children().len(), 2); // entry, then nested section
    assert!(matches!(
        &section.children()[0],
        OutlineNode::DocumentEntry { forms, owner, .. }
            if forms.len() == 1 && forms[0] == "printf(3)" && matches!(owner.as_ref(), mant_ir::ContentReveal::FixedOwner { key: owner_key } if *owner_key == key(1))
    ));
    assert_eq!(section.children()[1].path(), "1.1");

    let selected = crate::projection::build_outline_projection(
        &resolved,
        EntryProjection::All,
        Some(ContentSelector::path("1.1")),
    )
    .expect("nested native section path");
    assert_eq!(selected.nodes[0].path(), "1.1");
}

#[test]
fn explain_keeps_linked_form_coordinates_and_owner_local_body() {
    let resolved = fixture();
    let result = super::super::select_explanation(&resolved, "printf(3)").unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.counts.direct_entry.total, 1);
    assert_eq!(result.evidence[0].outline.path(), "1/e1");
    let record = &result.evidence[0];
    let entry = record.entry.as_ref().expect("bounded Fixed form details");
    assert_eq!(
        entry.fixed_forms[0].complete_text().as_deref(),
        Some("printf(3)")
    );
    assert_eq!(
        entry.fixed_forms[0]
            .parts
            .iter()
            .map(|part| part.row.get())
            .collect::<Vec<_>>(),
        vec![3, 4]
    );
    let occurrence = match &record.bases[0] {
        EvidenceBasis::Name { matches } => &matches[0].occurrences[0],
        other => panic!("expected native name evidence, got {other:?}"),
    };
    assert_eq!(
        occurrence.fixed_forms[0]
            .resolve(&entry.fixed_forms)
            .as_deref(),
        Some("printf(3)")
    );
    assert!(!record.name_bindings_omitted);
    assert_eq!(entry.name_bindings[0].name_index, 0);
    assert_eq!(
        entry.name_bindings[0].occurrences.as_slice(),
        std::slice::from_ref(occurrence)
    );
    let Some(ExplanationContent::FixedOwner {
        key: owner_key,
        reading_body,
    }) = &record.content
    else {
        panic!("Fixed owner body must not become a Flow block");
    };
    assert_eq!(*owner_key, key(1));
    assert_eq!(reading_body.complete_text().as_deref(), Some("first body"));
    assert!(
        !reading_body
            .parts
            .iter()
            .any(|part| part.text == "empty body")
    );
    result.validate_references().unwrap();
    serde_json::from_value::<QueryExplanation>(serde_json::to_value(&result).unwrap()).unwrap();

    // Pinned CVS man_macro.c::blk_imp retains the TP BODY after a zero-
    // width HEAD. Its visible text is ordinary context, not a declaration.
    let ordinary = super::super::select_explanation(&resolved, "empty body").unwrap();
    assert_eq!(ordinary.total, 1);
    assert_eq!(ordinary.evidence[0].class, EvidenceClass::ContextMention);
    assert_eq!(
        super::super::select_explanation(&resolved, "owner-empty")
            .unwrap()
            .total,
        0
    );
}

#[test]
fn fixed_match_facts_precede_optional_entry_and_binding_copies() {
    let resolved = fixture();
    let document = resolved.document.as_ref().unwrap();
    let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
        unreachable!()
    };
    let basis = EvidenceBasis::Name {
        matches: vec![ExplanationNameMatch {
            name: "printf(3)".into(),
            occurrences: Vec::new(),
        }],
    };
    let query = ExplanationQuery {
        entry: "printf(3)".into(),
        options: mant_protocol::ExplanationOptions {
            content_bytes: u32::try_from(serde_json::to_vec(&basis).unwrap().len()).unwrap(),
            ..Default::default()
        },
    };
    let (result, _) = super::response(&resolved, document, fixed, &query).unwrap();
    let record = &result.evidence[0];
    assert!(matches!(
        record.bases.as_slice(),
        [EvidenceBasis::Name { .. }]
    ));
    assert!(record.entry.is_none());
    assert!(record.details_omitted);
    assert!(record.match_details_omitted);
    result.validate_references().unwrap();
}

#[test]
fn section_reader_keeps_direct_subtree_and_root_preface_distinct() {
    let resolved = fixture();
    let document = resolved.document.as_ref().unwrap();
    let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
        unreachable!()
    };
    let reader = FixedSectionReader::new(fixed).unwrap();
    let preface = reader.root_preface_parts().unwrap();
    assert_eq!(
        preface.iter().map(|part| part.text).collect::<Vec<_>>(),
        ["PREFACE"]
    );
    let direct = reader.direct_parts(key(1)).unwrap();
    assert_eq!(
        direct.iter().map(|part| part.text).collect::<Vec<_>>(),
        ["OPTIONS", "printf", "(3)", "first body", "empty body"]
    );
    let subtree = reader.subtree_parts(key(1)).unwrap();
    assert_eq!(
        subtree.iter().map(|part| part.text).collect::<Vec<_>>(),
        [
            "OPTIONS",
            "printf",
            "(3)",
            "first body",
            "empty body",
            "NESTED",
            "child text"
        ]
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One cross-source response fixture exercises all three domains.
fn scoped_fixed_flow_and_tldr_share_one_candidate_page_and_source_index() {
    use mant_ir::{DocumentAddress, MarkdownOrigin};
    use mant_protocol::{
        DocumentScope, DocumentTraversal, ExplanationOptions, ResolvedDocumentScope, ScopedDocument,
    };

    let mut documents = vec![
        fixture(),
        crate::query_fixture::markdown(
            "# OPTIONS\n\n<!-- mant:entries role=term case=sensitive -->\n- `printf(3)`: Flow body.\n",
            None,
        )
        .unwrap(),
        crate::query_fixture::markdown(
            "<!-- mant:tldr:start -->\n# Short\n\n> Quick reference.\n<!-- mant:tldr:end -->\n",
            None,
        )
        .unwrap(),
    ];
    documents[2].document = None;
    let sources = ["fixed", "flow", "tldr"]
        .into_iter()
        .map(|path| ScopedDocument {
            address: DocumentAddress::Markdown {
                path: path.into(),
                origin: MarkdownOrigin::Documents,
            },
            depth: 0,
            root_indices: vec![],
            reached_from: vec![],
        })
        .collect::<Vec<_>>();
    for (source, content) in sources.iter().zip(&mut documents) {
        content.address = Some(source.address.clone());
    }
    let graph = ResolvedDocumentScope {
        reference_limits: Vec::new(),
        query: DocumentScope {
            documents: vec![],
            traversal: DocumentTraversal::default(),
        },
        documents: sources,
        edges: vec![],
        frontier: vec![],
        unresolved: vec![],
    };
    let input = crate::QueryScopeView::new(&graph, &documents).unwrap();
    let mut query = ExplanationQuery {
        entry: "printf(3)".into(),
        options: ExplanationOptions {
            limit: 1,
            ..ExplanationOptions::default()
        },
    };
    let first = super::super::scoped::explain(input, &query).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.evidence[0].document_index, 0);
    assert_eq!(first.evidence[0].evidence.ordinal, 0);
    assert!(matches!(
        first.evidence[0].evidence.content,
        Some(ExplanationContent::FixedOwner { .. })
    ));
    assert!(first.documents[0].content_projection.is_none());
    assert!(first.documents[0].source_context.is_some());
    assert!(first.documents[2].source_context.is_none());
    assert_eq!(first.documents[2].total, 0);
    assert_eq!(first.next_offset, Some(1));
    let decoded: mant_protocol::ScopeExplanation =
        serde_json::from_value(serde_json::to_value(&first).unwrap()).unwrap();
    assert_eq!(decoded.evidence[0].document_index, 0);

    query.options.offset = 1;
    let second = super::super::scoped::explain(input, &query).unwrap();
    assert_eq!(second.total, 2);
    assert_eq!(second.evidence[0].document_index, 1);
    assert_eq!(second.evidence[0].evidence.ordinal, 1);
    assert_eq!(second.next_offset, None);
    serde_json::from_value::<mant_protocol::ScopeExplanation>(
        serde_json::to_value(&second).unwrap(),
    )
    .unwrap();

    query.options.offset = 0;
    query.options.limit = 2;
    let together = super::super::scoped::explain(input, &query).unwrap();
    assert_eq!(
        together
            .evidence
            .iter()
            .map(|record| record.document_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(together.documents[0].content_projection.is_none());
    assert!(together.documents[1].content_projection.is_some());
    serde_json::from_value::<mant_protocol::ScopeExplanation>(
        serde_json::to_value(&together).unwrap(),
    )
    .unwrap();

    documents[0].document.as_mut().unwrap().sources.clear();
    let input = crate::QueryScopeView::new(&graph, &documents).unwrap();
    query.options.offset = 0;
    let failed = super::super::scoped::explain(input, &query).unwrap();
    assert_eq!(failed.failures.len(), 1);
    assert_eq!(failed.total, 1);
    assert_eq!(failed.documents.len(), 2);
    assert_eq!(failed.evidence[0].document_index, 0);
}

#[test]
fn scoped_budget_reserves_all_fixed_match_facts_before_any_body_or_entry() {
    let documents = [fixture(), fixture()];
    let plans = documents
        .iter()
        .map(|content| {
            let document = content.document.as_ref().unwrap();
            let mant_ir::DocumentBodyRef::Fixed(fixed) = document.body() else {
                unreachable!()
            };
            super::super::plan::DocumentPlan::Fixed(
                super::plan(content, document, fixed, "printf(3)").unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let name = EvidenceBasis::Name {
        matches: vec![ExplanationNameMatch {
            name: "printf(3)".into(),
            occurrences: Vec::new(),
        }],
    };
    let form = EvidenceBasis::Form {
        matches: vec![ExplanationFormMatch {
            source_form_index: 0,
            text: "printf(3)".into(),
            occurrences: Vec::new(),
        }],
    };
    let bytes =
        2 * (serde_json::to_vec(&name).unwrap().len() + serde_json::to_vec(&form).unwrap().len());
    let mut budget = Budget(bytes);
    let page =
        super::super::page::materialize(&plans, &[(0, 0, 0), (1, 0, 1)], "printf(3)", &mut budget)
            .unwrap();
    assert_eq!(budget.0, 0);
    assert_eq!(page.evidence.len(), 2);
    for (index, record) in page.evidence.iter().enumerate() {
        assert_eq!(record.document_index, index);
        assert_eq!(record.evidence.bases, [name.clone(), form.clone()]);
        assert!(record.evidence.entry.is_none());
        assert!(record.evidence.content.is_none());
        assert!(record.evidence.details_omitted);
        assert!(record.evidence.content_omitted);
    }
}
