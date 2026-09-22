//! Match facts and response-relative locations must survive standalone serde.
use mant_ir::*;
use mant_loader::load_roff_bytes;
use mant_protocol::*;
use mant_query::explain_query;

fn query(name: &str, content_bytes: u32) -> ExplanationQuery {
    ExplanationQuery {
        entry: name.into(),
        options: ExplanationOptions {
            content_bytes,
            ..Default::default()
        },
    }
}

fn test_inline(
    document: &mut Document,
    owner: ContentOwnerKey,
    value: impl Into<String>,
    literal: bool,
) -> Inline {
    let value = value.into();
    let store = &mut document.content_store;
    let root = ContentRootKey::new(u32::try_from(store.roots.len() + 1).unwrap()).unwrap();
    store
        .owners
        .iter_mut()
        .find(|record| record.key == owner)
        .unwrap()
        .roots
        .push(root);
    store.roots.push(ContentRoot {
        key: root,
        owner,
        kind: ContentRootKind::Term,
        atoms: Vec::new(),
        points: Vec::new(),
        provenance: Provenance::Unknown,
    });
    inline_in_root(document, root, value, literal)
}

fn inline_in_root(
    document: &mut Document,
    root: ContentRootKey,
    value: String,
    literal: bool,
) -> Inline {
    let store = &mut document.content_store;
    let owner = store.root(root).unwrap().owner;
    let atom = ContentAtomKey::new(u32::try_from(store.atoms.len() + 1).unwrap()).unwrap();
    store
        .roots
        .iter_mut()
        .find(|record| record.key == root)
        .unwrap()
        .atoms
        .push(atom);
    let bytes = ContentByteRange {
        start: 0,
        end: u32::try_from(value.len()).unwrap(),
    };
    store.atoms.push(ContentAtom {
        key: atom,
        root,
        owner,
        kind: ContentAtomKind::Text {
            text: value,
            display_override: None,
        },
        style: ContentStyle {
            literal,
            ..ContentStyle::default()
        },
        role: None,
        link: None,
        provenance: Provenance::Unknown,
    });
    let content = ContentRef { atom, bytes };
    if literal {
        Inline::Code { content }
    } else {
        Inline::Text { content }
    }
}

fn rewrite_inline(document: &mut Document, inline: &Inline, value: &str) -> Inline {
    let Inline::Code { content } = inline else {
        panic!("synthetic term is code")
    };
    let atom = document
        .content_store
        .atoms
        .iter_mut()
        .find(|atom| atom.key == content.atom)
        .unwrap();
    atom.kind = ContentAtomKind::Text {
        text: value.to_owned(),
        display_override: None,
    };
    Inline::Code {
        content: ContentRef {
            atom: content.atom,
            bytes: ContentByteRange {
                start: 0,
                end: u32::try_from(value.len()).unwrap(),
            },
        },
    }
}

fn add_wrapper_style(document: &mut Document, inline: &Inline, strong: bool, emphasis: bool) {
    let (Inline::Text { content } | Inline::Code { content }) = inline else {
        panic!("synthetic leaf")
    };
    let atom = document
        .content_store
        .atoms
        .iter_mut()
        .find(|atom| atom.key == content.atom)
        .unwrap();
    atom.style.strong = strong;
    atom.style.emphasis = emphasis;
}

fn fixture_owner(document: &Document) -> ContentOwnerKey {
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("definition fixture")
    };
    let Inline::Code { content } = &items[0].terms[0][0] else {
        panic!("first fixture term")
    };
    document.content_store.atom(content.atom).unwrap().owner
}

fn content_block(evidence: &ExplanationEvidence) -> &Block {
    match evidence.content.as_ref().unwrap() {
        ExplanationContent::Entry { block } | ExplanationContent::Block { block } => block,
        ExplanationContent::DeclarationMember { .. } | ExplanationContent::SharedEntry { .. } => {
            panic!("fixture has no shared context")
        }
    }
}

fn text_at(
    result: &QueryExplanation,
    evidence: &ExplanationEvidence,
    range: &ExplanationContentRange,
) -> String {
    let content = result.content_projection.as_ref().unwrap().content();
    let text = range
        .resolve(content, content_block(evidence))
        .expect("valid response-local root")
        .safe_text(content)
        .unwrap();
    text.chars()
        .skip(range.char_range().start)
        .take(range.char_range().len())
        .collect()
}

fn occurrences(evidence: &ExplanationEvidence) -> impl Iterator<Item = &ExplanationOccurrence> {
    let matches = evidence.bases.iter().flat_map(|basis| match basis {
        EvidenceBasis::Name { matches } => matches
            .iter()
            .flat_map(|record| &record.occurrences)
            .collect::<Vec<_>>(),
        EvidenceBasis::Form { matches } => matches
            .iter()
            .flat_map(|record| &record.occurrences)
            .collect(),
        _ => Vec::new(),
    });
    matches.chain(
        evidence
            .entry
            .iter()
            .flat_map(|entry| &entry.name_bindings)
            .flat_map(|binding| &binding.occurrences),
    )
}

fn validate_positions(result: &QueryExplanation, evidence: &ExplanationEvidence) {
    let content = result
        .content_projection
        .as_ref()
        .map(|projection| projection.content());
    let mut count = 0;
    for occurrence in occurrences(evidence) {
        assert!(occurrence.forms.len() <= MAX_EXPLANATION_FRAGMENTS);
        assert!(occurrence.content.len() <= MAX_EXPLANATION_FRAGMENTS);
        for range in &occurrence.forms {
            assert!(content.is_some_and(|content| {
                range
                    .resolve(content, &evidence.entry.as_ref().unwrap().forms)
                    .is_some()
            }));
        }
        for range in &occurrence.content {
            assert!(
                content.is_some_and(|content| range
                    .resolve(content, content_block(evidence))
                    .is_some())
            );
        }
        count += occurrence.forms.len() + occurrence.content.len();
    }
    for preview in &evidence.previews {
        for range in &preview.content_ranges {
            assert!(
                content.is_some_and(|content| range
                    .resolve(content, content_block(evidence))
                    .is_some())
            );
        }
        count += preview.content_ranges.len();
    }
    assert!(count <= MAX_EXPLANATION_POSITIONS, "{count}");
}

#[test]
fn direct_names_other_names_and_literal_ranges_are_independent_and_remapped() {
    let content = load_roff_bytes(include_bytes!("fixtures/entry-presentation.1")).unwrap();
    let result = explain_query(&content, &query("-x", 1_048_576)).unwrap();
    let decoded: QueryExplanation =
        serde_json::from_slice(&serde_json::to_vec(&result).unwrap()).unwrap();
    assert_eq!(decoded, result);
    assert_eq!(result.total, 3);
    for evidence in &result.evidence {
        validate_positions(&result, evidence);
    }
    let direct = &result.evidence[0];
    let name = direct
        .bases
        .iter()
        .find_map(|basis| match basis {
            EvidenceBasis::Name { matches } => Some(matches),
            _ => None,
        })
        .unwrap();
    assert_eq!(name.len(), 1);
    assert_eq!(name[0].name, "-x");
    let range = &name[0].occurrences[0].content[0];
    assert!(matches!(
        range,
        ExplanationContentRange::DefinitionTerm { item_index: 0, .. }
    ));
    assert_eq!(text_at(&result, direct, range), "-x");
    let entry = direct.entry.as_ref().unwrap();
    assert_eq!(entry.name_bindings.len(), 2);
    let language = entry
        .name_bindings
        .iter()
        .find(|binding| entry.names[binding.name_index as usize] == "--language")
        .unwrap();
    assert_eq!(
        text_at(&result, direct, &language.occurrences[0].content[0]),
        "--language"
    );
    for evidence in &result.evidence {
        assert_eq!(
            text_at(&result, evidence, &evidence.previews[0].content_ranges[0]),
            "-x"
        );
        assert!(!evidence.match_details_omitted);
        assert!(!evidence.name_bindings_omitted);
    }
    // The second source item becomes item zero in its independent excerpt.
    let mention = &result.evidence[1];
    assert!(
        matches!(&mention.previews[0].content_ranges[0], ExplanationContentRange::BlockText { path, .. }
        if matches!(path.first(), Some(ExplanationBlockStep::DefinitionItem { index: 0 })))
    );
}

fn synthetic(names: usize, repeats: usize) -> ResolvedContent {
    // The synthetic list replaces all section blocks.  Start with an empty
    // section so the source document has no atom orphaned by that replacement.
    let mut content = load_roff_bytes(b".TH PROBE 1\n.SH TERMS\n").unwrap();
    let document = content.document.as_mut().unwrap();
    let mut terms = Vec::new();
    let mut forms = Vec::new();
    let mut bindings = Vec::new();
    let mut spellings = Vec::new();
    let owner =
        ContentOwnerKey::new(u32::try_from(document.content_store.owners.len() + 1).unwrap())
            .unwrap();
    document.content_store.owners.push(ContentOwner {
        key: owner,
        kind: ContentOwnerKind::DefinitionItem,
        roots: Vec::new(),
        provenance: Provenance::Unknown,
    });
    for name in 0..names {
        let spelling = format!("name{name}");
        let mut occurrences = Vec::new();
        for _ in 0..repeats {
            let index = terms.len();
            terms.push(vec![test_inline(
                document,
                owner,
                format!("{spelling} ARG"),
                true,
            )]);
            forms.push(EntryForm::term(index));
            occurrences.push(EntryForm {
                parts: vec![EntryContentSlice {
                    root: EntryInlineRoot::Term { index },
                    path: vec![0],
                    bytes: Some(0..spelling.len()),
                }],
            });
        }
        bindings.push(EntryNameBinding {
            name,
            evidence: EntryNameEvidence::Declared,
            occurrences,
        });
        spellings.push(spelling);
    }
    document.sections[0].blocks = vec![Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            terms,
            description: vec![],
            source: None,
            layout: DefinitionLayout::default(),
            entry: Some(EntryFacts {
                id: "subject".into(),
                kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: spellings,
                forms,
                name_bindings: bindings,
                alias_groups: vec![],
                alias_of: None,
                value_domain: None,
            }),
        }],
        compact: false,
        layout: LayoutHint::default(),
        source: None,
    }];
    assert!(validate_document(content.document.as_ref().unwrap()).is_empty());
    content
}

#[test]
fn split_occurrences_follow_authored_form_order_not_term_indices() {
    let mut content = synthetic(1, 1);
    let document = content.document.as_mut().unwrap();
    let owner = fixture_owner(document);
    let name = test_inline(document, owner, "名", false);
    let accented = test_inline(document, owner, "é", true);
    add_wrapper_style(document, &name, false, true);
    add_wrapper_style(document, &accented, true, false);
    let ending = test_inline(document, owner, "終", false);
    let Inline::Text {
        content: ending_ref,
    } = &ending
    else {
        unreachable!()
    };
    let ending_root = document.content_store.atom(ending_ref.atom).unwrap().root;
    document
        .content_store
        .roots
        .iter_mut()
        .find(|record| record.key == ending_root)
        .unwrap()
        .kind = ContentRootKind::Body;
    let Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[0] else {
        unreachable!()
    };
    let item = &mut items[0];
    let unreferenced = std::mem::take(&mut item.terms[0]);
    item.terms = vec![
        unreferenced,
        vec![Inline::Strong {
            children: vec![accented],
        }],
        vec![Inline::Emphasis {
            children: vec![name],
        }],
    ];
    item.description = vec![Block::Paragraph {
        children: vec![ending],
        layout: LayoutHint::default(),
        source: None,
    }];
    let occurrence = EntryForm {
        parts: vec![
            EntryContentSlice {
                root: EntryInlineRoot::Term { index: 1 },
                path: vec![],
                bytes: None,
            },
            EntryContentSlice {
                root: EntryInlineRoot::Term { index: 2 },
                path: vec![],
                bytes: None,
            },
            EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![],
                bytes: None,
            },
        ],
    };
    // A skipped first term makes formIndex 0 distinct from termIndex 1.
    let facts = item.entry.as_mut().unwrap();
    facts.names = vec!["é名終".into()];
    facts.forms = vec![occurrence.clone()];
    facts.name_bindings[0].occurrences = vec![occurrence];
    let diagnostics = validate_document(content.document.as_ref().unwrap());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let result = explain_query(&content, &query("é名終", 1_048_576)).unwrap();
    let evidence = &result.evidence[0];
    validate_positions(&result, evidence);
    let EvidenceBasis::Name { matches } = &evidence.bases[0] else {
        panic!("name")
    };
    let occurrence = &matches[0].occurrences[0];
    assert_eq!(
        occurrence
            .content
            .iter()
            .map(|r| text_at(&result, evidence, r))
            .collect::<String>(),
        "é名終"
    );
    assert!(matches!(
        occurrence.content[0],
        ExplanationContentRange::DefinitionTerm { term_index: 1, .. }
    ));
    assert_eq!(
        occurrence
            .forms
            .iter()
            .map(|r| r.form_index)
            .collect::<Vec<_>>(),
        [0, 0, 0]
    );
    assert!(!evidence.match_details_omitted);
}

#[test]
fn case_insensitive_names_keep_authored_spelling_and_identity_names_its_field() {
    let mut content = synthetic(1, 1);
    let Block::DefinitionList { items, .. } =
        &mut content.document.as_mut().unwrap().sections[0].blocks[0]
    else {
        unreachable!()
    };
    items[0].entry.as_mut().unwrap().case = NameCase::Insensitive;
    let result = explain_query(&content, &query("NAME0", 1_048_576)).unwrap();
    assert!(
        matches!(&result.evidence[0].bases[0], EvidenceBasis::Name { matches } if matches[0].name == "name0")
    );
    let identity = explain_query(&content, &query("subject", 1_048_576)).unwrap();
    assert!(identity.evidence[0].bases.iter().any(|b| matches!(b, EvidenceBasis::Identity { fields } if fields == &[ExplanationIdentityField::Id])));
}

#[test]
fn records_occurrences_and_total_positions_have_independent_omissions() {
    for (names, repeats, request) in [(35, 1, "subject"), (1, 35, "name0"), (32, 32, "name0")] {
        let content = synthetic(names, repeats);
        let result =
            explain_query(&content, &query(request, MAX_EXPLANATION_CONTENT_BYTES)).unwrap();
        let evidence = &result.evidence[0];
        validate_positions(&result, evidence);
        let entry = evidence.entry.as_ref().unwrap();
        assert_eq!(entry.names.len(), names);
        assert_eq!(entry.forms.len(), names * repeats);
        assert!(entry.name_bindings.len() <= 32);
        assert!(
            entry
                .name_bindings
                .iter()
                .all(|binding| binding.occurrences.len() <= 32)
        );
        assert!(evidence.name_bindings_omitted);
        assert_eq!(evidence.match_details_omitted, request != "subject");
        assert!(result.truncation.content);
        if names == 32 && repeats == 32 {
            let count: usize = occurrences(evidence)
                .map(|o| o.forms.len() + o.content.len())
                .sum();
            assert_eq!(count, MAX_EXPLANATION_POSITIONS);
        }
    }
    let content = synthetic(1, 35);
    let result =
        explain_query(&content, &query("name0 ARG", MAX_EXPLANATION_CONTENT_BYTES)).unwrap();
    let evidence = &result.evidence[0];
    let EvidenceBasis::Form { matches } = &evidence.bases[0] else {
        panic!("form basis")
    };
    assert_eq!(matches.len(), 32);
    assert_eq!(matches.last().unwrap().source_form_index, 31);
    assert!(evidence.match_details_omitted);
}

#[test]
fn all_budget_sizes_keep_locations_resolvable_and_account_for_new_payload() {
    let content = synthetic(2, 2);
    for bytes in [1, 80, 160, 500, 1500, 4096, 16384] {
        let result = explain_query(&content, &query("name0", bytes)).unwrap();
        assert_eq!(result.total, 1);
        let evidence = &result.evidence[0];
        assert_eq!(evidence.class, EvidenceClass::DirectEntry);
        validate_positions(&result, evidence);
        let mut used = 0;
        if let Some(entry) = &evidence.entry {
            used += serde_json::to_vec(entry).unwrap().len();
        }
        if let Some(body) = &evidence.content {
            used += serde_json::to_vec(body).unwrap().len();
        }
        used += evidence
            .previews
            .iter()
            .map(|p| serde_json::to_vec(p).unwrap().len())
            .sum::<usize>();
        for basis in &evidence.bases {
            used += match basis {
                EvidenceBasis::Name { matches } if !matches.is_empty() => {
                    serde_json::to_vec(matches).unwrap().len()
                }
                EvidenceBasis::Form { matches } if !matches.is_empty() => {
                    serde_json::to_vec(matches).unwrap().len()
                }
                _ => 0,
            };
        }
        assert!(used <= bytes as usize, "budget {bytes}: copied {used}");
        assert_eq!(result.truncation.content, evidence.has_omitted_content());
    }
}

#[test]
fn matched_facts_and_body_survive_when_expanded_form_metadata_does_not_fit() {
    let mut content = synthetic(1, 1);
    let document = content.document.as_mut().unwrap();
    let Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[0] else {
        unreachable!()
    };
    let item = &mut items[0];
    let original = item.terms[0].remove(0);
    let expanded = rewrite_inline(document, &original, &format!("name0 {}", "填".repeat(400)));
    let Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[0] else {
        unreachable!()
    };
    let item = &mut items[0];
    item.terms[0] = vec![expanded];
    item.entry.as_mut().unwrap().forms = vec![EntryForm::term(0); 100];
    // Key-backed forms and their shared projection leave this intermediate
    // budget large enough for the body but not the expanded form metadata.
    let result = explain_query(&content, &query("name0", 12000)).unwrap();
    let evidence = &result.evidence[0];
    assert!(evidence.details_omitted && evidence.entry.is_none());
    assert!(!evidence.content_omitted && evidence.content.is_some());
    assert!(!evidence.name_bindings_omitted && !evidence.match_details_omitted);
    let EvidenceBasis::Name { matches } = &evidence.bases[0] else {
        panic!("name")
    };
    assert_eq!(matches[0].name, "name0");
    assert!(matches[0].occurrences[0].forms.is_empty());
    assert_eq!(
        text_at(&result, evidence, &matches[0].occurrences[0].content[0]),
        "name0"
    );
    validate_positions(&result, evidence);
}

#[test]
fn a_fragment_limit_never_returns_half_a_name_occurrence() {
    let mut content = synthetic(1, 1);
    let document = content.document.as_mut().unwrap();
    let Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[0] else {
        panic!("definition")
    };
    let original = items[0].terms[0].remove(0);
    let first = rewrite_inline(document, &original, "é");
    let Inline::Code { content: first_ref } = first else {
        unreachable!()
    };
    let root = document.content_store.atom(first_ref.atom).unwrap().root;
    let mut fragments = vec![Inline::Code { content: first_ref }];
    fragments.extend((1..33).map(|_| inline_in_root(document, root, "é".into(), true)));
    let Block::DefinitionList { items, .. } = &mut document.sections[0].blocks[0] else {
        panic!("definition")
    };
    let item = &mut items[0];
    item.terms[0] = fragments;
    let facts = item.entry.as_mut().unwrap();
    facts.names[0] = "é".repeat(33);
    facts.name_bindings[0].occurrences = vec![EntryForm {
        parts: (0..33)
            .map(|index| EntryContentSlice {
                root: EntryInlineRoot::Term { index: 0 },
                path: vec![index],
                bytes: None,
            })
            .collect(),
    }];
    assert!(validate_document(content.document.as_ref().unwrap()).is_empty());
    let result = explain_query(&content, &query(&"é".repeat(33), 1_048_576)).unwrap();
    let evidence = &result.evidence[0];
    let EvidenceBasis::Name { matches } = &evidence.bases[0] else {
        panic!("name")
    };
    assert!(matches[0].occurrences.is_empty());
    assert!(
        evidence.entry.as_ref().unwrap().name_bindings[0]
            .occurrences
            .is_empty()
    );
    assert!(evidence.match_details_omitted && evidence.name_bindings_omitted);
    assert!(!evidence.content_omitted);
}
