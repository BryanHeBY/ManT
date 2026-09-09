//! Application adapters validate complete requests before loading or querying.
use super::{
    DocumentLoader, LoadPolicy, LoadSpec, PreparedQueryRequest, QueryError, QueryExecutionError,
    QueryInput, QueryRequest, QueryViewResult, ResolvedContent, project_query_view,
};
use mant_protocol::{CatalogQuery, DocumentCatalog};

/// Application composition over an explicit local document snapshot.
pub struct DocumentResolver {
    loader: DocumentLoader,
}
impl DocumentResolver {
    /// Capture source discovery for related application operations.
    #[must_use]
    pub fn from_system() -> Self {
        Self {
            loader: DocumentLoader::from_system(),
        }
    }
    /// Validate the entire request before resolving its source.
    ///
    /// # Errors
    /// Returns loading or query-view validation errors.
    pub fn resolve(
        &self,
        request: &QueryRequest,
        policy: LoadPolicy,
    ) -> Result<ResolvedContent, QueryError> {
        PreparedQueryRequest::new(request, policy)?.resolve(self)
    }
    pub(super) fn resolve_validated(
        &self,
        request: &QueryRequest,
        policy: LoadPolicy,
    ) -> Result<ResolvedContent, QueryError> {
        self.loader
            .load(load_spec(&request.input), policy)
            .map_err(QueryError::Load)
    }
    /// Load and materialize the view encoded by a complete request.
    ///
    /// # Errors
    /// Returns loading, validation, projection or search failure.
    pub fn execute(
        &self,
        request: &QueryRequest,
        policy: LoadPolicy,
    ) -> Result<QueryViewResult, QueryExecutionError> {
        PreparedQueryRequest::new(request, policy)
            .map_err(QueryExecutionError::Query)?
            .execute(self)
    }
    pub(super) fn execute_validated(
        &self,
        request: &QueryRequest,
        policy: LoadPolicy,
    ) -> Result<QueryViewResult, QueryExecutionError> {
        let content = self
            .resolve_validated(request, policy)
            .map_err(QueryExecutionError::Query)?;
        project_query_view(content, &request.view)
    }
    /// Discover candidates from the same local snapshot.
    ///
    /// # Errors
    /// Returns source configuration or catalog filter failures.
    pub fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, String> {
        self.loader.discover(query)
    }

    /// Discover with a query prepared before this environment was captured.
    ///
    /// # Errors
    /// Returns source configuration or catalog acquisition failures.
    pub fn discover_prepared(
        &self,
        query: &mant_loader::PreparedCatalogQuery<'_>,
    ) -> Result<DocumentCatalog, String> {
        self.loader.discover_prepared(query)
    }
    pub(crate) fn loader(&self) -> &DocumentLoader {
        &self.loader
    }
}

pub(super) fn load_spec(input: &QueryInput) -> LoadSpec<'_> {
    match input {
        QueryInput::Document {
            selector,
            source,
            manual_section,
        } => LoadSpec::Document {
            selector,
            source: source.as_deref(),
            manual_section: manual_section.as_deref(),
        },
        QueryInput::File { path, format } => LoadSpec::File {
            path,
            format: *format,
        },
    }
}

#[cfg(test)]
pub(super) fn query_with(
    request: &QueryRequest,
    policy: LoadPolicy,
    load: impl FnOnce() -> Result<ResolvedContent, QueryError>,
) -> Result<ResolvedContent, QueryError> {
    PreparedQueryRequest::new(request, policy)?;
    load()
}

/// Prepare in-memory Markdown for an application query without source discovery.
///
/// # Errors
/// Returns a loading error for malformed or empty document content.
pub fn query_markdown_text(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, QueryError> {
    mant_loader::load_markdown_text(source, source_path).map_err(QueryError::Load)
}

/// Prepare bounded standalone roff bytes without MANPATH or include traversal.
///
/// # Errors
/// Returns a loading error for invalid or empty document content.
#[cfg(feature = "roff")]
pub fn query_roff_bytes(source: &[u8]) -> Result<ResolvedContent, QueryError> {
    mant_loader::load_roff_bytes(source).map_err(QueryError::Load)
}
