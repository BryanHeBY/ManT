//! Exact accepted query spans; generated table decoration is not a hit.
use mant_ir::ResolvedContent;
use mant_protocol::{
    ExplanationOptions, ExplanationQuery, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

pub(super) fn assert_query(content: &ResolvedContent, label: &str) {
    let artifact = mant_codec::encode::render_addressable_markdown_with_options(
        content,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    );
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let response = mant_query::search_query(
            content,
            &SearchQuery {
                pattern: label.into(),
                scope,
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                word: true,
                context_lines: 0,
                limit: 20,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(response.total, 1, "{label} {scope:?}");
        let occurrences = response
            .matches
            .iter()
            .flat_map(|m| &m.occurrences)
            .collect::<Vec<_>>();
        assert_eq!(occurrences.len(), 1);
        let occurrence = occurrences[0];
        assert_eq!(occurrence.matched_text, label);
        let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
        let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
        assert_eq!(artifact.text().get(start..end), Some(label));
        for (at, line, column) in [
            (
                start,
                occurrence.markdown.start_line,
                occurrence.markdown.start_column,
            ),
            (
                end,
                occurrence.markdown.end_line,
                occurrence.markdown.end_column,
            ),
        ] {
            let prefix = &artifact.text()[..at];
            assert_eq!(
                usize::try_from(line).unwrap(),
                prefix.bytes().filter(|b| *b == b'\n').count() + 1
            );
            assert_eq!(
                usize::try_from(column).unwrap(),
                prefix.rsplit('\n').next().unwrap().chars().count() + 1
            );
        }
    }
    let explanation = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: label.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let restored: mant_protocol::QueryExplanation =
        serde_json::from_str(&serde_json::to_string(&explanation).unwrap()).unwrap();
    assert_eq!(restored, explanation);
    assert_previews(&explanation, label);
    assert!(
        !explanation.evidence.is_empty(),
        "{label}: complete source owner must remain explainable"
    );
}

fn assert_previews(explanation: &mant_protocol::QueryExplanation, label: &str) {
    let mut selected = 0;
    for evidence in &explanation.evidence {
        let Some(content) = &evidence.content else {
            continue;
        };
        for range in evidence
            .previews
            .iter()
            .flat_map(|preview| &preview.content_ranges)
        {
            let actual = content
                .resolve_range(&explanation.supports, range)
                .unwrap()
                .safe_text()
                .chars()
                .skip(range.char_range().start)
                .take(range.char_range().len())
                .collect::<String>();
            assert_eq!(actual, label);
            selected += 1;
        }
    }
    if selected == 0 {
        selected = assert_name_occurrences(explanation, label);
    }
    assert_eq!(
        selected, 1,
        "one original response-local owner range for {label}"
    );
}

fn assert_name_occurrences(explanation: &mant_protocol::QueryExplanation, label: &str) -> usize {
    let mut selected = 0;
    for evidence in &explanation.evidence {
        let has_name = evidence
            .bases
            .iter()
            .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. }));
        for basis in &evidence.bases {
            // Name and complete-form are two bases for the same physical
            // occurrence, not two source owners. Prefer the exact name domain.
            if has_name && matches!(basis, mant_protocol::EvidenceBasis::Form { .. }) {
                continue;
            }

            let occurrences = match basis {
                mant_protocol::EvidenceBasis::Name { matches } => matches
                    .iter()
                    .flat_map(|m| &m.occurrences)
                    .collect::<Vec<_>>(),
                mant_protocol::EvidenceBasis::Form { matches } => matches
                    .iter()
                    .flat_map(|m| &m.occurrences)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            for occurrence in occurrences {
                if !occurrence.content.is_empty() {
                    let body = evidence.content.as_ref().expect("returned name content");
                    let text = occurrence
                        .content
                        .iter()
                        .map(|range| {
                            body.resolve_range(&explanation.supports, range)
                                .unwrap()
                                .safe_text()
                                .chars()
                                .skip(range.char_range().start)
                                .take(range.char_range().len())
                                .collect::<String>()
                        })
                        .collect::<String>();
                    assert_eq!(text, label);
                }
                if !occurrence.forms.is_empty() {
                    let forms = &evidence.entry.as_ref().expect("returned name forms").forms;
                    let text = occurrence
                        .forms
                        .iter()
                        .map(|range| {
                            mant_ir::inline_plain_text(range.resolve(forms).unwrap())
                                .chars()
                                .skip(usize::try_from(range.start_char).unwrap())
                                .take(usize::try_from(range.end_char - range.start_char).unwrap())
                                .collect::<String>()
                        })
                        .collect::<String>();
                    assert_eq!(text, label);
                }
                if !occurrence.content.is_empty() || !occurrence.forms.is_empty() {
                    selected += 1;
                }
            }
        }
    }
    selected
}
