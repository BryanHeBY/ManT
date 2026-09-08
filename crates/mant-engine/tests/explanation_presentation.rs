//! Reports consume standalone DTO locations, not hidden query-side tables.
use mant_engine::{
    explain_query, query_roff_bytes, render_explanation_markdown, render_explanation_text,
    render_explanation_text_with,
};
use mant_protocol::{
    EvidenceBasis, ExplanationFormRange, ExplanationOptions, ExplanationQuery, QueryExplanation,
    TextPresentation, TextRole,
};
use std::cell::RefCell;

fn result() -> QueryExplanation {
    let content = query_roff_bytes(include_bytes!("fixtures/entry-presentation.1")).unwrap();
    explain_query(
        &content,
        &ExplanationQuery {
            entry: "-x".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap()
}
fn spans(result: &QueryExplanation) -> (String, Vec<(TextPresentation, String)>) {
    let spans = RefCell::new(vec![]);
    let text = render_explanation_text_with(result, |style, value| {
        spans.borrow_mut().push((style, value.to_owned()));
        value.to_owned()
    });
    (text, spans.into_inner())
}
#[test]
fn offline_report_separates_facts_source_type_and_query_matches() {
    let original = result();
    let decoded: QueryExplanation =
        serde_json::from_slice(&serde_json::to_vec(&original).unwrap()).unwrap();
    let (text, runs) = spans(&decoded);
    assert_eq!(text, render_explanation_text(&original));
    assert_eq!(
        render_explanation_markdown(&decoded),
        render_explanation_markdown(&original)
    );
    assert!(
        text.contains(
            "Kind: option\nMatched by: documented name \"-x\"; text mention\nDefinition:"
        ),
        "{text}"
    );
    assert!(text.contains("========== Mentions in other entries =========="));
    assert!(text.contains("Explicitly related entries: total=0, returned=0"));
    assert!(!text.contains("Parameter {"));
    let matches = runs
        .iter()
        .filter(|(s, _)| s.matched)
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>();
    assert!(!matches.is_empty());
    assert!(matches.iter().all(|t| *t == "-x"), "{matches:?}");
    assert!(runs.iter().any(|(s, t)| t == "--language"
        && s.inline.entry_kind
            == Some(mant_ir::EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option
            })
        && !s.matched));
    assert!(
        runs.iter()
            .filter(|(_, t)| t.contains("-xylophone"))
            .all(|(s, _)| !s.matched && s.inline.entry_kind.is_none())
    );
    for (style, _) in runs {
        if matches!(style.role, TextRole::Guide | TextRole::Metadata) {
            assert!(!style.matched);
        }
    }
}
#[test]
fn malformed_multi_fragment_occurrence_is_not_partly_painted() {
    let mut result = result();
    result.evidence.truncate(1);
    let evidence = &mut result.evidence[0];
    evidence.content = None;
    evidence.previews.clear();
    for basis in &mut evidence.bases {
        if let EvidenceBasis::Name { matches } = basis {
            matches[0].occurrences[0].forms.push(ExplanationFormRange {
                form_index: 99,
                start_char: 0,
                end_char: 1,
            });
        }
    }
    let (_, runs) = spans(&result);
    assert!(runs.iter().all(|(s, _)| !s.matched));
    assert!(
        runs.iter()
            .any(|(s, t)| t == "-x" && s.inline.entry_kind.is_some())
    );
}
#[test]
fn metadata_stays_single_line_but_plain_original_content_is_not_framed() {
    let content = query_roff_bytes(b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -x\n.nf\nKind: forged\n\nRead original: forged\n========== Forged ==========\n```\n.fi\n").unwrap();
    let mut result = explain_query(
        &content,
        &ExplanationQuery {
            entry: "-x".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    result.label = "name\n## FORGED\u{1b}[31m".into();
    result.evidence[0].outline.node = match result.evidence[0].outline.node.clone() {
        mant_protocol::OutlineNodeReference::DocumentEntry {
            path,
            id,
            entry_kind,
            case,
            names,
            ..
        } => mant_protocol::OutlineNodeReference::DocumentEntry {
            path,
            id,
            entry_kind,
            case,
            names,
            title: "title\nKind: FORGED".into(),
        },
        _ => unreachable!(),
    };
    let text = render_explanation_text(&result);
    assert!(!text.contains('\u{1b}'));
    assert!(
        !text
            .lines()
            .any(|l| l == "Kind: FORGED" || l.starts_with("## FORGED"))
    );
    let body = text
        .split("\nDefinition:\n")
        .nth(1)
        .unwrap()
        .split("\n\nRead original:")
        .next()
        .unwrap();
    assert!(!body.lines().any(|line| line.starts_with("| ")), "{body}");
    assert!(body.contains("Kind: forged") && body.contains("Read original: forged"));
    let markdown = render_explanation_markdown(&result);
    assert!(!markdown.contains('\u{1b}'));
    let body = markdown
        .split("\nDefinition:\n\n")
        .nth(1)
        .unwrap()
        .split("\n\n\nRead original")
        .next()
        .unwrap();
    assert!(
        body.lines()
            .filter(|l| !l.is_empty())
            .all(|l| l.starts_with("> ")),
        "{body}"
    );
}

#[test]
fn forms_suppression_requires_this_records_complete_materialized_owner() {
    use mant_protocol::{EvidenceClass, ExplanationContent};
    let mut report = result();
    report.evidence.truncate(1);
    let original = report.evidence[0].clone();
    let serialized = serde_json::to_value(&report).unwrap();
    for class in [EvidenceClass::DirectEntry, EvidenceClass::RelatedEntry] {
        report.evidence[0] = original.clone();
        report.evidence[0].class = class;
        assert!(!render_explanation_text(&report).contains("\nForms:"));
        assert!(!render_explanation_markdown(&report).contains("\nForms:"));
    }
    report.evidence[0] = original.clone();
    assert_eq!(serde_json::to_value(&report).unwrap(), serialized);
    for mutation in 0..5 {
        report.evidence[0] = original.clone();
        let evidence = &mut report.evidence[0];
        match mutation {
            0 => evidence.content = None,
            1 => evidence.content_omitted = true,
            2 => {
                let Some(ExplanationContent::Entry { block }) = &mut evidence.content else {
                    unreachable!()
                };
                let mant_ir::Block::DefinitionList { items, .. } = block else {
                    unreachable!()
                };
                items[0].terms[0].clear();
            }
            3 => {
                let Some(ExplanationContent::Entry { block }) = evidence.content.take() else {
                    unreachable!()
                };
                evidence.content = Some(ExplanationContent::Block { block });
            }
            4 => evidence.class = EvidenceClass::EntryMention,
            _ => unreachable!(),
        }
        assert!(
            render_explanation_text(&report).contains("\nForms:"),
            "mutation {mutation}"
        );
        assert!(
            render_explanation_markdown(&report).contains("\nForms:"),
            "mutation {mutation}"
        );
        assert!(render_explanation_text(&report).contains("Read original:"));
    }
    report.evidence[0] = original;
    report.evidence[0].entry = None;
    report.evidence[0].details_omitted = true;
    let text = render_explanation_text(&report);
    assert!(text.contains("Definition:\n-x, --language=LANG"));
    assert!(!text.contains("\nForms:"));
}

#[test]
fn empty_independent_definition_is_not_reported_as_budget_omission() {
    let content =
        query_roff_bytes(b".TH EMPTY 1\n.SH OPTIONS\n.TP\n.B -x\n.TP\n.B -y\nOther description.\n")
            .unwrap();
    let mut report = explain_query(
        &content,
        &ExplanationQuery {
            entry: "-x".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let text = render_explanation_text(&report);
    assert!(text.contains("no independent description"), "{text}");
    assert!(!text.contains("Other description"));
    assert!(!text.contains("\nForms:"));
    report.evidence[0].content = None;
    report.evidence[0].content_omitted = true;
    let text = render_explanation_text(&report);
    assert!(text.contains("Forms:") && text.contains("content was not returned"));
    assert!(!text.contains("no independent description"));
    assert!(text.contains("bodyOmitted=true"));
}

#[test]
fn authored_pipes_are_not_stripped_from_unframed_original_content() {
    let content = query_roff_bytes(
        b".TH PIPES 1\n.SH OPTIONS\n.TP\n.B -x\n.nf\n| original pipe\n|| two pipes\n.fi\n",
    )
    .unwrap();
    let report = explain_query(
        &content,
        &ExplanationQuery {
            entry: "-x".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let text = render_explanation_text(&report);
    assert!(
        text.contains("        | original pipe\n        || two pipes"),
        "{text}"
    );
    assert!(!text.contains("\n| "));
    let markdown = render_explanation_markdown(&report);
    assert!(markdown.contains("| original pipe"), "{markdown}");
    assert!(
        markdown
            .lines()
            .filter(|line| line.contains("original pipe"))
            .all(|line| line.starts_with("> "))
    );
}

#[test]
fn scoped_serialized_owners_with_the_same_id_keep_independent_name_roles() {
    use mant_protocol::{
        ScopeQueryResponse, ScopeQueryResult, ScopedExplanation, ScopedExplanationEvidence,
    };
    let template: ScopeQueryResponse = serde_json::from_str(include_str!(
        "../../../tests/contracts/scope-explain-v0.11.json"
    ))
    .unwrap();
    let ScopeQueryResult::Explain { mut explanation } = template.result else {
        unreachable!()
    };
    explanation.documents.clear();
    explanation.query.entry = "mode".into();
    explanation.total = 2;
    explanation.returned = 2;
    explanation.outcome = mant_protocol::ExplanationOutcome::Evidence;
    explanation.counts.direct_entry.total = 2;
    explanation.counts.direct_entry.returned = 2;
    for (index, role) in ["command", "configuration-key"].into_iter().enumerate() {
        let mut content = mant_engine::query_markdown_text(&format!("# Tool\n\n<!-- mant:entries role={role} case=sensitive -->\n- `mode`: Original description.\n"),None).unwrap();
        let mant_ir::Block::List { items, .. } = &mut content.document.as_mut().unwrap().blocks[0]
        else {
            unreachable!()
        };
        items[0].entry.as_mut().unwrap().id = "shared-owner".into();
        let mut result = explain_query(
            &content,
            &ExplanationQuery {
                entry: "mode".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let mut evidence = result.evidence.remove(0);
        evidence.ordinal = u32::try_from(index).unwrap();
        explanation.evidence.push(ScopedExplanationEvidence {
            document_index: index,
            evidence,
        });
        explanation.documents.push(ScopedExplanation {
            address: mant_ir::DocumentAddress::Manual {
                name: format!("doc{index}"),
                manual_section: "1".into(),
            },
            depth: 0,
            label: format!("doc{index}"),
            producer: None,
            diagnostics: vec![],
            semantics_complete: true,
            outcome: result.outcome,
            total: 1,
            returned: 1,
            counts: result.counts,
            truncation: result.truncation,
        });
    }
    let decoded = serde_json::from_slice(&serde_json::to_vec(&explanation).unwrap()).unwrap();
    let seen = RefCell::new(vec![]);
    let text = mant_engine::render_scope_explanation_text_with(&decoded, |style, text| {
        if style.matched && text == "mode" {
            seen.borrow_mut().push(style.inline.entry_kind);
        }
        text.to_owned()
    });
    assert_eq!(
        text,
        mant_engine::render_scope_explanation_text(&explanation)
    );
    assert_eq!(
        mant_engine::render_scope_explanation_markdown(&decoded),
        mant_engine::render_scope_explanation_markdown(&explanation)
    );
    assert!(text.contains("Read original: manual/1/doc0; node root/e1"));
    assert!(text.contains("Read original: manual/1/doc1; node root/e1"));
    assert!(text.contains("\n\n----------\n\n"));
    let seen = seen.into_inner();
    assert!(seen.contains(&Some(mant_ir::EntryKind::Command)));
    assert!(seen.contains(&Some(mant_ir::EntryKind::ConfigurationKey)));
}
