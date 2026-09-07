//! Query resolver boundary; public entry points remain validated.
use super::{
    CatalogQuery, Document, DocumentAddress, DocumentCatalog, MAX_MARKDOWN_BYTES, ManualIndex,
    ManualPage, ManualRequest, MarkdownOrigin, OnceLock, Path, PathBuf, QueryError,
    QueryExecutionError, QueryHost, QueryPolicy, QueryRequest, QueryViewResult,
    RegisteredDocumentIndex, RegisteredDocumentOrigin, RegisteredLookupPhase, RegisteredSelection,
    RegisteredSelectionGroup, SourceConfigError, TldrDocument, discover_manual_roots, fs,
    locate_manual_source_in, parse_manual_page, parse_manual_source, project_query_view,
    query_name_candidates, query_with, read_cached_tldr_page, read_capped_utf8,
    validate_query_request,
};

fn registered_selection(document: &mant_sources::RegisteredDocument) -> RegisteredSelection {
    RegisteredSelection {
        path: document.path.clone(),
        address: DocumentAddress::Markdown {
            path: document.logical_path.clone(),
            origin: match &document.origin {
                RegisteredDocumentOrigin::Documents => MarkdownOrigin::Documents,
                RegisteredDocumentOrigin::Source(name) => {
                    MarkdownOrigin::Source { name: name.clone() }
                }
            },
        },
    }
}

/// One explicit local document-environment snapshot.
pub struct DocumentResolver {
    registered: OnceLock<Result<RegisteredDocumentIndex, SourceConfigError>>,
    manual_roots: Vec<PathBuf>,
    manuals: OnceLock<ManualIndex>,
    available: OnceLock<Vec<crate::catalog::AvailableDocument>>,
}

impl DocumentResolver {
    /// Capture native manual roots and lazily snapshot the manual index and
    /// Markdown registration.
    #[must_use]
    pub fn from_system() -> Self {
        Self {
            registered: OnceLock::new(),
            manual_roots: discover_manual_roots(),
            manuals: OnceLock::new(),
            available: OnceLock::new(),
        }
    }

    /// Validate and resolve one request against this environment snapshot.
    ///
    /// Reusing a resolver keeps manual and registered-document precedence
    /// stable across related operations. Construct a new resolver to refresh
    /// filesystem discovery.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError`] for invalid input or unreadable local content.
    pub fn resolve(
        &self,
        request: &QueryRequest,
        policy: QueryPolicy,
    ) -> Result<ResolvedContent, QueryError> {
        validate_query_request(request, policy)?;
        query_with(request, policy, self)
    }

    /// Resolve and materialize the request's encoded view.
    ///
    /// # Errors
    ///
    /// Returns a typed loading, projection, or search failure.
    pub fn execute(
        &self,
        request: &QueryRequest,
        policy: QueryPolicy,
    ) -> Result<QueryViewResult, QueryExecutionError> {
        let query = self
            .resolve(request, policy)
            .map_err(QueryExecutionError::Query)?;
        project_query_view(query, &request.view)
    }

    /// Filter the same registered-document and manual snapshots used by
    /// [`Self::resolve`].
    ///
    /// # Errors
    ///
    /// Returns source-configuration or catalog-query failures as one host
    /// boundary diagnostic.
    pub fn discover(&self, query: &CatalogQuery) -> Result<DocumentCatalog, String> {
        let registered = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let manuals = self
            .manuals
            .get_or_init(|| ManualIndex::from_roots(self.manual_roots.clone()));
        let documents = self.available.get_or_init(|| {
            crate::catalog::list_available_documents_from(
                registered.documents().to_vec(),
                manuals.pages(),
            )
        });
        crate::catalog::query_available_documents(documents, query)
            .map_err(|error| error.to_string())
    }
}

impl QueryHost for DocumentResolver {
    fn name_candidates(&self, name: &str) -> Vec<String> {
        query_name_candidates(name)
    }

    fn locate_registered_document(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Option<RegisteredSelection>, String> {
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let selected = if source.is_some() {
            index.find(candidates, source)
        } else {
            match phase {
                RegisteredLookupPhase::BeforeBuiltin => index.find_before_builtin(candidates),
                RegisteredLookupPhase::AfterBuiltin => index.find_after_builtin(candidates),
            }
        };
        selected
            .map(|registered| registered.map(registered_selection))
            .map_err(|error| error.to_string())
    }

    fn locate_registered_document_groups(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Vec<RegisteredSelectionGroup>, String> {
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        let groups = if let Some(source) = source {
            index.matches_in_source(candidates, source)
        } else {
            Ok(match phase {
                RegisteredLookupPhase::BeforeBuiltin => index.matches_before_builtin(candidates),
                RegisteredLookupPhase::AfterBuiltin => index.matches_after_builtin(candidates),
            })
        }
        .map_err(|error| error.to_string())?;
        Ok(groups
            .into_iter()
            .map(|group| RegisteredSelectionGroup {
                documents: group.documents.iter().map(registered_selection).collect(),
            })
            .collect())
    }

    fn locate_registered_address(
        &self,
        address: &DocumentAddress,
    ) -> Result<Option<RegisteredSelection>, String> {
        let DocumentAddress::Markdown { path, origin } = address else {
            return Ok(None);
        };
        let origin = match origin {
            MarkdownOrigin::Documents => RegisteredDocumentOrigin::Documents,
            MarkdownOrigin::Source { name } => RegisteredDocumentOrigin::Source(name.clone()),
        };
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        index
            .find_address(path, &origin)
            .map(|document| {
                document.map(|document| RegisteredSelection {
                    path: document.path.clone(),
                    address: address.clone(),
                })
            })
            .map_err(|error| error.to_string())
    }

    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String> {
        let manuals = self
            .manuals
            .get_or_init(|| ManualIndex::from_roots(self.manual_roots.clone()));
        locate_manual_source_in(request, manuals).map_err(|error| error.load_detail())
    }

    fn parse_manual(&self, page: &ManualPage) -> Result<Document, String> {
        parse_manual_page(page).map_err(|error| error.to_string())
    }

    fn parse_manual_input(&self, path: &Path) -> Result<Document, String> {
        parse_manual_source(path).map_err(|error| error.to_string())
    }

    fn read_tldr(&self, name: &str) -> Result<Option<TldrDocument>, String> {
        read_cached_tldr_page(name).map_err(|error| error.to_string())
    }

    fn read_markdown(&self, path: &Path) -> Result<String, String> {
        let file = fs::File::open(path).map_err(|error| error.to_string())?;
        read_capped_utf8(file, MAX_MARKDOWN_BYTES)
    }
}
use super::ResolvedContent;
