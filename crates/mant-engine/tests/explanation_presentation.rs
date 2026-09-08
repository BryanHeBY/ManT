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
        text.contains("Kind: option\nMatched by: documented name \"-x\"; text mention\nForms:"),
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
fn metadata_cannot_break_lines_and_original_fake_fields_remain_quoted() {
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
    assert!(body.lines().all(|line| line.starts_with("| ")), "{body}");
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
