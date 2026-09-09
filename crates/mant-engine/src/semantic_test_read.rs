//! Test bridge: discover exact semantic names with explain, then explicitly read paths.
//! This deliberately does not implement the removed selector alias/shorthand fallback.
use mant_ir::ResolvedContent;
use mant_protocol::{ContentSelector, EvidenceBasis, ExplanationQuery, QueryExcerpt};
use mant_query::ProjectionError;

pub fn semantic_excerpt<S: AsRef<str>>(
    content: &ResolvedContent,
    names: &[S],
) -> Result<QueryExcerpt, ProjectionError> {
    let mut selectors = Vec::new();
    for name in names {
        let query = ExplanationQuery {
            entry: name.as_ref().to_owned(),
            options: mant_protocol::ExplanationOptions::default(),
        };
        let response =
            mant_query::explain_query(content, &query).expect("valid semantic test query");
        let before = selectors.len();
        selectors.extend(
            response
                .evidence
                .iter()
                .filter(|evidence| {
                    evidence
                        .bases
                        .iter()
                        .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
                })
                .map(|evidence| ContentSelector::path(evidence.outline.path())),
        );
        if selectors.len() == before {
            return Err(ProjectionError::UnknownSelector {
                document: content.label.clone(),
                selector: name.as_ref().to_owned(),
            });
        }
    }
    mant_query::select_excerpt(content, &selectors)
}
