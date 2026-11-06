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
    assert_eq!(
        crate::explanation_ranges::assert_ranges(&explanation, label),
        1
    );
    assert!(
        !explanation.evidence.is_empty(),
        "{label}: complete source owner must remain explainable"
    );
}
