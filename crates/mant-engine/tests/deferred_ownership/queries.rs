//! Public byte/scalar search maps and actual response-local explain ranges.
use crate::cases;
use mant_ir::ResolvedContent;
use mant_protocol::{
    ExplanationOptions, ExplanationQuery, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};
use std::collections::BTreeSet;

pub(super) fn check(content: &ResolvedContent, case: &cases::Case) {
    let artifact = mant_codec::encode::render_addressable_markdown_with_options(
        content,
        mant_codec::encode::MarkdownOptions::ADDRESSABLE,
    );
    let mut terms = BTreeSet::from(["AFTER".to_owned()]);
    for recovery in &case.recoveries {
        terms.insert(recovery.text.clone());
    }
    for link in &case.links {
        if !link.text.is_empty() {
            terms.insert(link.text.clone());
        }
    }
    for word in terms {
        for scope in [SearchScope::Visible, SearchScope::Markdown] {
            // Escaped recovery spelling has different Markdown bytes. Use
            // its stable literal name for the raw-markup search and the full
            // recovered spelling for visible search.
            let pattern = if scope == SearchScope::Markdown {
                word.split(|character: char| !character.is_alphanumeric() && character != '-')
                    .max_by_key(|part| part.len())
                    .unwrap()
            } else {
                &word
            };
            let response = mant_query::search_query(
                content,
                &SearchQuery {
                    pattern: pattern.into(),
                    scope,
                    syntax: SearchSyntax::Literal,
                    case: SearchCase::Sensitive,
                    word: false,
                    context_lines: 0,
                    limit: 100,
                    offset: 0,
                },
            )
            .unwrap();
            let restored: mant_protocol::QuerySearch =
                serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
            assert_eq!(restored, response);
            let occurrences = response
                .matches
                .iter()
                .flat_map(|hit| &hit.occurrences)
                .collect::<Vec<_>>();
            assert!(!occurrences.is_empty(), "{} {scope:?} {word}", case.id);
            if scope == SearchScope::Markdown {
                assert_eq!(
                    occurrences.len(),
                    artifact
                        .text()
                        .match_indices(pattern)
                        .filter(|(at, value)| {
                            // Query presentation deliberately removes generated
                            // navigation anchors; their IDs are not body hits.
                            !artifact
                                .anchor_ranges()
                                .iter()
                                .any(|range| *at < range.end && range.start < *at + value.len())
                        })
                        .count(),
                    "{} exact markup occurrences",
                    case.id
                );
            } else if let Some(recovery) = case
                .recoveries
                .iter()
                .find(|recovery| recovery.text == word)
            {
                assert_eq!(
                    occurrences.len(),
                    recovery.occurrences,
                    "{} exact visible recovery occurrences",
                    case.id
                );
            }
            for occurrence in occurrences {
                check_occurrence(occurrence, artifact.text(), pattern, scope, &case.id);
            }
        }
        check_explanation(content, case, &word);
    }
}

pub(super) fn check_explanation(content: &ResolvedContent, case: &cases::Case, word: &str) {
    // Explanation uses finite executable-name boundaries, unlike substring
    // search. A recovery glued to X/Y is not a standalone full name. Its
    // bracketed/quoted payload is a genuine bounded mention; the full
    // spelling remains covered by visible search above.
    let original = word;
    let word = if let Some(name) = word
        .strip_prefix("\\[")
        .and_then(|word| word.strip_suffix(']'))
    {
        name
    } else if let Some(number) = word
        .strip_prefix("\\N'")
        .and_then(|word| word.strip_suffix('\''))
    {
        number
    } else {
        word
    };
    let response = mant_query::explain_query(
        content,
        &ExplanationQuery {
            entry: word.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let restored: mant_protocol::QueryExplanation =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    assert_eq!(restored, response);
    let selected = crate::explanation_ranges::assert_ranges(&response, word);
    if selected == 0 {
        // An exact label can remain a fragment of a larger executable name,
        // e.g. Sm-off Y:https://... . Search still finds its scalars, but
        // Explain deliberately excludes that unbounded substring.
        assert_ne!(original, "AFTER");
        assert!(
            !case
                .recoveries
                .iter()
                .any(|recovery| recovery.text == original)
                || owner_mentions(&cases::snapshot(content), word) == 0,
            "{} recovery payload must have real evidence",
            case.id
        );
        assert!(
            !has_literal_name(&cases::snapshot(content), word),
            "{} unexplained bounded label {word}",
            case.id
        );
        assert_eq!(
            response.evidence,
            Vec::<mant_protocol::ExplanationEvidence>::new()
        );
    }
    if let Some(recovery) = case
        .recoveries
        .iter()
        .find(|recovery| recovery.text == original)
    {
        assert!(selected <= recovery.occurrences);
        assert_eq!(
            selected,
            owner_mentions(&cases::snapshot(content), word),
            "{} exact original recovery occurrences",
            case.id
        );
        if !case.source.contains(".Bl -tag") && !case.source.contains(".Bl -hang") {
            assert!(
                !response
                    .evidence
                    .iter()
                    .any(|evidence| evidence.bases.iter().any(|basis| matches!(
                        basis,
                        mant_protocol::EvidenceBasis::Name { .. }
                            | mant_protocol::EvidenceBasis::Form { .. }
                    ))),
                "{} ordinary recovery is not a direct declaration name",
                case.id
            );
        }
    }
}

pub(super) fn check_occurrence(
    occurrence: &mant_protocol::SearchOccurrence,
    markdown: &str,
    pattern: &str,
    scope: SearchScope,
    id: &str,
) {
    assert_eq!(occurrence.matched_text, pattern);
    let start = usize::try_from(occurrence.markdown.start_byte).unwrap();
    let end = usize::try_from(occurrence.markdown.end_byte).unwrap();
    let selected = markdown.get(start..end).unwrap();
    let mut literal = false;
    let mut code_slice = false;
    for (event, range) in
        pulldown_cmark::Parser::new_ext(markdown, pulldown_cmark::Options::empty())
            .into_offset_iter()
    {
        match event {
            pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_)) => literal = true,
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::CodeBlock) => literal = false,
            pulldown_cmark::Event::Text(_)
                if literal && range.start <= start && end <= range.end =>
            {
                code_slice = true;
            }
            pulldown_cmark::Event::Code(_) if range.start <= start && end <= range.end => {
                code_slice = true;
            }
            _ => {}
        }
    }
    if scope == SearchScope::Markdown || code_slice {
        assert_eq!(selected, pattern);
    } else {
        let decoded = pulldown_cmark::Parser::new_ext(selected, pulldown_cmark::Options::empty())
            .filter_map(|event| match event {
                pulldown_cmark::Event::Text(value) | pulldown_cmark::Event::Code(value) => {
                    Some(value.into_string())
                }
                _ => None,
            })
            .collect::<String>();
        assert_eq!(
            decoded, pattern,
            "{id} visible search must select its original scalar text in the artifact"
        );
    }
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
        let prefix = &markdown[..at];
        assert_eq!(
            usize::try_from(line).unwrap(),
            prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
        );
        assert_eq!(
            usize::try_from(column).unwrap(),
            prefix.rsplit('\n').next().unwrap().chars().count() + 1
        );
    }
}

fn owner_texts(snapshot: &cases::Snapshot) -> Vec<String> {
    let mut owners = std::collections::BTreeMap::<&str, String>::new();
    for cell in &snapshot.cells {
        // Definition terms are matched only as complete forms, never as literal substrings (explanation/collect.rs).
        if cell.owner.contains("/term/") {
            continue;
        }
        owners.entry(&cell.owner).or_default().push(cell.value);
    }
    owners.into_values().collect()
}
fn owner_mentions(snapshot: &cases::Snapshot, word: &str) -> usize {
    owner_texts(snapshot)
        .iter()
        .filter(|text| text.contains(word))
        .count()
}
fn has_literal_name(snapshot: &cases::Snapshot, word: &str) -> bool {
    // Frozen finite name-separator policy, using only the ASCII punctuation
    // present in this source domain. Source owners remain independent roots.
    let separator = |c: char| c.is_whitespace() || "()[]{}<>\"'`,;=".contains(c);
    owner_texts(snapshot).iter().any(|text| {
        text.match_indices(word).any(|(at, value)| {
            let tail = &text[at + value.len()..];
            let mut after = tail.chars();
            let right = match after.next() {
                None => true,
                Some('.' | ':') => after.next().is_none_or(separator),
                Some(c) => separator(c),
            };
            text[..at].chars().next_back().is_none_or(separator) && right
        })
    })
}
