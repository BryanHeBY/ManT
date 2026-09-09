#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod entry_presentation;
mod explanation;
mod projection;
#[cfg(test)]
mod query_fixture;
mod scope_query;
mod search;
mod selectors;

pub use explanation::{
    ExplanationError, explain_query, resolve_explanation_block, validate_explanation_query,
};
pub use mant_ir::ResolvedContent;
pub use projection::{
    ProjectionError, ReferenceProjectionLimits, SelectorCandidate, build_outline,
    build_outline_projection, build_outline_with_detail, build_outline_with_references,
    project_references, project_references_with_limits, select_excerpt, select_explanation,
    semantics_complete,
};
pub use scope_query::{
    QueryScopeView, ScopeExecutionError, ScopeInputError, explain_scope, search_scope,
};
pub use search::{SearchError, search_query, validate_search_query};
