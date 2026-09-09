//! Document-local selector policy, independent of producer identity allocation.
mod error;
mod index;
mod located;
pub use error::{ProjectionError, SelectorCandidate};
pub(crate) use index::DocumentSelectorIndex;
pub(crate) use located::{LocatedBreadcrumb, LocatedNode, collect_root_entries, collect_sections};
pub(crate) use located::{collect_selection_root_entries, collect_selection_sections};
pub(crate) const TLDR_ID: &str = "tldr";
pub(crate) const DOCUMENT_ROOT_TITLE: &str = "OVERVIEW";

fn ambiguous_selector(
    document: &str,
    selector: &str,
    matches: Vec<&LocatedNode<'_>>,
) -> ProjectionError {
    ProjectionError::AmbiguousSelector {
        document: document.to_owned(),
        selector: selector.to_owned(),
        candidates: matches
            .into_iter()
            .map(|candidate| SelectorCandidate {
                path: candidate.path().to_string(),
                id: candidate.id().into(),
            })
            .collect(),
    }
}
