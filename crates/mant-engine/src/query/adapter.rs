//! Application adapters validate complete requests before loading or querying.
use super::{
    CatalogQuery, DocumentCatalog, DocumentLoader, LoadError, LoadHost, LoadSpec, QueryError,
    QueryExecutionError, QueryInput, QueryPolicy, QueryRequest, QueryViewResult, ResolvedContent,
    load_with, project_query_view, validate_query_request,
};

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
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, QueryError> {
        query_with(request, policy, &self.loader)
    }
    /// Load and materialize the view encoded by a complete request.
    ///
    /// # Errors
    /// Returns loading, validation, projection or search failure.
    pub fn execute(
        &self,
        request: &QueryRequest,
        policy: QueryPolicy,
    ) -> Result<QueryViewResult, QueryExecutionError> {
        let content = self
            .resolve(request, policy)
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
    pub(crate) fn load(
        &self,
        spec: LoadSpec<'_>,
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, LoadError> {
        self.loader.load(spec, policy)
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

pub(super) fn query_with(
    request: &QueryRequest,
    policy: QueryPolicy,
    host: &dyn LoadHost,
) -> Result<ResolvedContent, QueryError> {
    validate_query_request(request, policy)?;
    load_with(load_spec(&request.input), policy, host).map_err(QueryError::Load)
}

/// Prepare in-memory Markdown for an application query without source discovery.
///
/// # Errors
/// Returns a loading error for malformed or empty document content.
pub fn query_markdown_text(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, QueryError> {
    super::load_markdown_text(source, source_path).map_err(QueryError::Load)
}

/// Prepare bounded standalone roff bytes without MANPATH or include traversal.
///
/// # Errors
/// Returns a loading error for invalid or empty document content.
pub fn query_roff_bytes(source: &[u8]) -> Result<ResolvedContent, QueryError> {
    super::load_roff_bytes(source).map_err(QueryError::Load)
}
