//! A borrowed proof that complete request validation preceded environment capture.
use super::{
    DocumentResolver, LoadPolicy, QueryError, QueryExecutionError, QueryRequest, QueryViewResult,
    ResolvedContent, validate_query_request,
};

/// A complete request validated without loading documents or capturing a snapshot.
///
/// The request and policy remain bound for this value's lifetime. Preparation
/// does not cache a search matcher: pure query APIs retain their own validation.
pub struct PreparedQueryRequest<'request> {
    request: &'request QueryRequest,
    policy: LoadPolicy,
}

impl<'request> PreparedQueryRequest<'request> {
    /// Validate both source selection and the requested view before local IO.
    ///
    /// # Errors
    /// Returns the same loading-selection or view errors as complete execution.
    pub fn new(request: &'request QueryRequest, policy: LoadPolicy) -> Result<Self, QueryError> {
        validate_query_request(request, policy)?;
        Ok(Self { request, policy })
    }

    /// The immutable request whose input and view were validated together.
    #[must_use]
    pub const fn request(&self) -> &'request QueryRequest {
        self.request
    }

    /// The source-selection policy included in validation.
    #[must_use]
    pub const fn policy(&self) -> LoadPolicy {
        self.policy
    }

    /// Resolve against an explicit snapshot without repeating request validation.
    ///
    /// # Errors
    /// Returns a source-loading failure.
    pub fn resolve(&self, resolver: &DocumentResolver) -> Result<ResolvedContent, QueryError> {
        resolver.resolve_validated(self.request, self.policy)
    }

    /// Execute the shared complete-request pipeline against an explicit snapshot.
    ///
    /// # Errors
    /// Returns loading, projection or pure query execution failure.
    pub fn execute(
        &self,
        resolver: &DocumentResolver,
    ) -> Result<QueryViewResult, QueryExecutionError> {
        resolver.execute_validated(self.request, self.policy)
    }
}
