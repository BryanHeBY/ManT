#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod query;
mod scope;

pub use query::{
    DocumentResolver, PreparedQueryRequest, QueryError, QueryExecutionError, QueryValidationError,
    QueryViewResult, execute_query, project_query_view, resolve_query, resolve_query_with_policy,
    validate_query_request,
};
pub use scope::{
    PreparedScopeQuery, ScopeQueryError, execute_scope_query, validate_scope_query_request,
};
