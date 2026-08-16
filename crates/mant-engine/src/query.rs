//! Resolves local manuals, registered Markdown, and tldr content into one query.

use std::{
    error::Error,
    ffi::OsStr,
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use mant_ir::{Document, DocumentAddress, MarkdownOrigin, ResolvedContent, TldrDocument};
use mant_protocol::{
    CatalogQuery, DocumentCatalog, InputFormat, QueryExcerpt, QueryInput, QueryOutline,
    QueryRequest, QuerySearch, QueryView, SearchQuery,
};
use mant_sources::{RegisteredDocumentIndex, RegisteredDocumentOrigin, SourceConfigError};

use crate::{
    ManualIndex, ManualPage, ManualRequest, ProjectionError, SearchError,
    build_outline_with_detail, discover_manual_roots, executable::query_name_candidates,
    locate_manual_source_in, parse_manual_bytes, parse_manual_page, parse_manual_source,
    parse_markdown, read_cached_tldr_page, search_query, select_excerpt, select_explanation,
    validate_search_query,
};

/// Upper bound on a single Markdown source, shared by every input path.
///
/// File and stdin readers both enforce this so an unbounded source (a pipe, a
/// character device such as `/dev/zero`, or a pathologically large file) cannot
/// exhaust memory. A file's reported length is not trusted: some sources report
/// zero yet stream without end, so readers cap the byte count directly.
pub const MAX_MARKDOWN_BYTES: u64 = 16 * 1024 * 1024;

/// A query cannot produce either authoritative manual content or a quick reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// A document selector was empty after trimming.
    EmptyName,
    /// A native manual category was empty or malformed.
    InvalidManualSection,
    /// A tldr command query was qualified by a non-command manual section.
    TldrManualSection {
        /// Incompatible native manual section.
        section: String,
    },
    /// An explicit Markdown source name was empty.
    InvalidSource,
    /// Markdown-source and native-manual selectors were combined.
    ConflictingSourceSelectors,
    /// A direct Markdown input path was empty.
    EmptyMarkdownPath,
    /// Automatic format inference did not recognize a direct input.
    UnsupportedInputFormat {
        /// Caller-facing input path.
        path: String,
    },
    /// Excerpt projection was requested without selectors.
    EmptySelection,
    /// An excerpt selector was empty.
    EmptySelector,
    /// An explanation entry name was empty.
    EmptyEntry,
    /// Search configuration failed validation.
    InvalidSearch(SearchError),
    /// Markdown input could not be read or parsed.
    Markdown {
        /// Caller-facing source path.
        path: String,
        /// Stable failure detail.
        detail: String,
    },
    /// Markdown parsing produced neither document nor tldr content.
    EmptyMarkdown {
        /// Selected-document label.
        label: String,
    },
    /// Registered-document discovery failed.
    Registry {
        /// Stable source-configuration or discovery detail.
        detail: String,
    },
    /// Native manual loading failed.
    Manual(ManualLoadError),
    /// No full document was found, but an optional tldr entry is available.
    ManualWithTldr {
        /// Native-manual failure retained as the authoritative lookup error.
        error: ManualLoadError,
        /// Topic that can be queried explicitly with `--tldr`.
        topic: String,
    },
    /// An explicit tldr query found no quick-reference candidate.
    TldrNotFound {
        /// Requested tldr topic.
        topic: String,
    },
    /// An explicit tldr candidate could not be read or parsed.
    Tldr {
        /// Requested tldr topic.
        topic: String,
        /// Stable cache or Markdown failure detail.
        detail: String,
    },
    /// No Markdown, manual, or quick-reference content could be resolved.
    NoReadableContent {
        /// Requested document name.
        name: String,
    },
}

/// Native-manual resolution or lowering failed after candidate selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualLoadError {
    /// No indexed native manual matched the request.
    NotFound {
        /// Requested manual name.
        name: String,
        /// Search-path and candidate detail.
        detail: String,
    },
    /// A selected manual could not be parsed or lowered.
    Parse {
        /// Requested manual name.
        name: String,
        /// Stable parser or source-policy detail.
        detail: String,
    },
    /// Parsing succeeded but produced no readable semantic content.
    Empty {
        /// Requested manual name.
        name: String,
        /// Physical selected manual path.
        path: PathBuf,
        /// Non-fatal parser findings explaining the empty result.
        diagnostics: Vec<String>,
    },
}

/// Materialized result of the view carried by a [`QueryRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryViewResult {
    /// Complete resolved content with no projection.
    Full(Box<ResolvedContent>),
    /// Lightweight structural outline.
    Outline(QueryOutline),
    /// One or more selected document nodes.
    Excerpt(QueryExcerpt),
    /// Paginatable structure-aware search result.
    Search(QuerySearch),
}

/// A valid request could not be loaded or projected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExecutionError {
    /// Input validation or document loading failure.
    Query(QueryError),
    /// Outline or selection projection failure.
    Projection(ProjectionError),
    /// Search compilation or execution failure.
    Search(SearchError),
}

/// Closed content-resolution policy kept outside the serialized request contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueryPolicy {
    /// Resolve a full document and attach a compatible quick reference.
    #[default]
    Combined,
    /// Bypass registered Markdown and tldr content.
    ManualOnly,
    /// Resolve only embedded or cached tldr content through source precedence.
    TldrOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FullDocumentMode {
    Priority,
    NativeManual,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickReferenceMode {
    AttachToCommandManual,
    Exclude,
    Only,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NamedResolutionPlan {
    document: FullDocumentMode,
    quick_reference: QuickReferenceMode,
}

impl QueryPolicy {
    fn named_resolution_plan(self, has_manual_section: bool) -> NamedResolutionPlan {
        match self {
            Self::Combined => NamedResolutionPlan {
                document: if has_manual_section {
                    FullDocumentMode::NativeManual
                } else {
                    FullDocumentMode::Priority
                },
                quick_reference: QuickReferenceMode::AttachToCommandManual,
            },
            Self::ManualOnly => NamedResolutionPlan {
                document: FullDocumentMode::NativeManual,
                quick_reference: QuickReferenceMode::Exclude,
            },
            Self::TldrOnly => NamedResolutionPlan {
                document: FullDocumentMode::None,
                quick_reference: QuickReferenceMode::Only,
            },
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("name must not be empty"),
            Self::InvalidManualSection => formatter.write_str(
                "manual section must be a conventional number or the single letter 'l' or 'n'",
            ),
            Self::TldrManualSection { section } => write!(
                formatter,
                "manual section '{section}' does not identify a command quick reference; tldr supports section families 1 and 8"
            ),
            Self::InvalidSource => formatter.write_str("document source must not be empty"),
            Self::ConflictingSourceSelectors => formatter.write_str(
                "document source cannot be combined with a manual section or manual-only policy",
            ),
            Self::EmptyMarkdownPath => formatter.write_str("Markdown path must not be empty"),
            Self::UnsupportedInputFormat { path } => write!(
                formatter,
                "could not infer the input format for '{path}'; use --input-format markdown or roff"
            ),
            Self::EmptySelection => formatter.write_str("at least one outline node is required"),
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::EmptyEntry => formatter.write_str("semantic entry must not be empty"),
            Self::InvalidSearch(error) => error.fmt(formatter),
            Self::Markdown { path, detail } => {
                write!(
                    formatter,
                    "could not load Markdown document '{path}': {detail}"
                )
            }
            Self::EmptyMarkdown { label } => {
                write!(
                    formatter,
                    "Markdown document '{label}' has no readable content"
                )
            }
            Self::Registry { detail } => formatter.write_str(detail),
            Self::Manual(error) => error.fmt(formatter),
            Self::ManualWithTldr { error, topic } => {
                error.fmt(formatter)?;
                write!(
                    formatter,
                    "\nhint: a tldr entry is available; run `mant {topic} --tldr`"
                )
            }
            Self::TldrNotFound { topic } => {
                write!(formatter, "no tldr quick reference was found for '{topic}'")
            }
            Self::Tldr { topic, detail } => {
                write!(formatter, "could not load tldr entry '{topic}': {detail}")
            }
            Self::NoReadableContent { name } => {
                write!(
                    formatter,
                    "no readable document content was found for '{name}'"
                )
            }
        }
    }
}

impl Error for QueryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSearch(error) => Some(error),
            Self::Manual(error) | Self::ManualWithTldr { error, .. } => Some(error),
            Self::EmptyName
            | Self::InvalidManualSection
            | Self::TldrManualSection { .. }
            | Self::InvalidSource
            | Self::ConflictingSourceSelectors
            | Self::EmptyMarkdownPath
            | Self::UnsupportedInputFormat { .. }
            | Self::EmptySelection
            | Self::EmptySelector
            | Self::EmptyEntry
            | Self::Markdown { .. }
            | Self::EmptyMarkdown { .. }
            | Self::Registry { .. }
            | Self::TldrNotFound { .. }
            | Self::Tldr { .. }
            | Self::NoReadableContent { .. } => None,
        }
    }
}

impl fmt::Display for ManualLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { name, detail } => {
                write!(formatter, "could not load manual '{name}': {detail}")
            }
            Self::Parse { name, detail } => write!(
                formatter,
                "could not load manual '{name}': manual source: {detail}"
            ),
            Self::Empty {
                name,
                path,
                diagnostics,
            } => {
                write!(
                    formatter,
                    "could not load manual '{name}': libmandoc parsed {} but produced no readable sections",
                    path.display()
                )?;
                if !diagnostics.is_empty() {
                    write!(formatter, "; diagnostics: {}", diagnostics.join("; "))?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ManualLoadError {}

impl fmt::Display for QueryExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(error) => error.fmt(formatter),
            Self::Projection(error) => error.fmt(formatter),
            Self::Search(error) => error.fmt(formatter),
        }
    }
}

impl Error for QueryExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Query(error) => Some(error),
            Self::Projection(error) => Some(error),
            Self::Search(error) => Some(error),
        }
    }
}

/// Query the local man database and optional offline tldr caches.
///
/// # Errors
///
/// Returns [`QueryError`] for invalid input or when neither source can produce
/// readable content.
pub fn resolve_query(request: &QueryRequest) -> Result<ResolvedContent, QueryError> {
    resolve_query_with_policy(request, QueryPolicy::default())
}

/// Query with an explicit input-resolution policy.
///
/// # Errors
///
/// Returns [`QueryError`] under the same conditions as [`resolve_query`].
pub fn resolve_query_with_policy(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<ResolvedContent, QueryError> {
    let resolver = DocumentResolver::from_system();
    resolver.resolve(request, policy)
}

/// Load and materialize the view encoded in one native request.
///
/// # Errors
///
/// Returns a typed loading, projection, or search failure.
pub fn execute_query(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<QueryViewResult, QueryExecutionError> {
    let resolver = DocumentResolver::from_system();
    resolver.execute(request, policy)
}

/// Materialize one view from an already loaded query.
///
/// # Errors
///
/// Returns a typed projection or search failure.
pub fn project_query_view(
    query: ResolvedContent,
    view: &QueryView,
) -> Result<QueryViewResult, QueryExecutionError> {
    match view {
        QueryView::Full {} => Ok(QueryViewResult::Full(Box::new(query))),
        QueryView::Outline { detail } => build_outline_with_detail(&query, *detail)
            .map(QueryViewResult::Outline)
            .map_err(QueryExecutionError::Projection),
        QueryView::Excerpt { selectors } => select_excerpt(&query, selectors)
            .map(QueryViewResult::Excerpt)
            .map_err(QueryExecutionError::Projection),
        QueryView::Explain { entry } => select_explanation(&query, entry)
            .map(QueryViewResult::Excerpt)
            .map_err(QueryExecutionError::Projection),
        QueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        } => search_query(
            &query,
            &SearchQuery {
                pattern: pattern.clone(),
                syntax: *syntax,
                case: *case,
                scope: *scope,
                word: *word,
                context_lines: *context_lines,
                limit: *limit,
                offset: *offset,
            },
        )
        .map(QueryViewResult::Search)
        .map_err(QueryExecutionError::Search),
    }
}

/// Validate all request and policy invariants before local I/O.
///
/// # Errors
///
/// Returns the exact invalid input constraint.
pub fn validate_query_request(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<(), QueryError> {
    match &request.input {
        QueryInput::Document {
            selector,
            source,
            manual_section,
        } => {
            if selector.trim().is_empty() {
                return Err(QueryError::EmptyName);
            }
            if source
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(QueryError::InvalidSource);
            }
            if manual_section
                .as_deref()
                .is_some_and(|value| !crate::is_manual_section(value.trim()))
            {
                return Err(QueryError::InvalidManualSection);
            }
            if policy == QueryPolicy::TldrOnly
                && let Some(section) = manual_section.as_deref()
                && !crate::is_command_manual_section(section.trim())
            {
                return Err(QueryError::TldrManualSection {
                    section: section.trim().to_owned(),
                });
            }
            if source.is_some() && (manual_section.is_some() || policy == QueryPolicy::ManualOnly) {
                return Err(QueryError::ConflictingSourceSelectors);
            }
        }
        QueryInput::File { path, .. } => {
            if path.trim().is_empty() {
                return Err(QueryError::EmptyMarkdownPath);
            }
            if policy != QueryPolicy::Combined {
                return Err(QueryError::Markdown {
                    path: path.trim().to_owned(),
                    detail: "content-only policies do not apply to direct input".to_owned(),
                });
            }
        }
    }
    match &request.view {
        QueryView::Excerpt { selectors } => {
            if selectors.is_empty() {
                return Err(QueryError::EmptySelection);
            }
            if selectors.iter().any(|selector| selector.trim().is_empty()) {
                return Err(QueryError::EmptySelector);
            }
        }
        QueryView::Explain { entry } if entry.trim().is_empty() => {
            return Err(QueryError::EmptyEntry);
        }
        QueryView::Search {
            pattern,
            syntax,
            case,
            scope,
            word,
            context_lines,
            limit,
            offset,
        } => validate_search_query(&SearchQuery {
            pattern: pattern.clone(),
            syntax: *syntax,
            case: *case,
            scope: *scope,
            word: *word,
            context_lines: *context_lines,
            limit: *limit,
            offset: *offset,
        })
        .map_err(QueryError::InvalidSearch)?,
        QueryView::Full {} | QueryView::Outline { .. } | QueryView::Explain { .. } => {}
    }
    Ok(())
}

trait QueryHost {
    fn name_candidates(&self, name: &str) -> Vec<String>;
    fn locate_registered_document(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Option<RegisteredSelection>, String>;
    fn locate_registered_document_groups(
        &self,
        candidates: &[String],
        source: Option<&str>,
        phase: RegisteredLookupPhase,
    ) -> Result<Vec<RegisteredSelectionGroup>, String>;
    fn locate_registered_address(
        &self,
        address: &DocumentAddress,
    ) -> Result<Option<RegisteredSelection>, String>;
    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String>;
    fn parse_manual(&self, page: &ManualPage) -> Result<Document, String>;
    fn parse_manual_input(&self, path: &Path) -> Result<Document, String>;
    fn read_tldr(&self, name: &str) -> Result<Option<TldrDocument>, String>;
    fn read_markdown(&self, path: &Path) -> Result<String, String>;
}

#[derive(Clone, Copy)]
enum RegisteredLookupPhase {
    BeforeBuiltin,
    AfterBuiltin,
}

#[derive(Clone)]
struct RegisteredSelection {
    path: PathBuf,
    address: DocumentAddress,
}

struct RegisteredSelectionGroup {
    documents: Vec<RegisteredSelection>,
}

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

struct LoadedManual {
    document: Document,
    address: DocumentAddress,
}

/// One explicit local document-environment snapshot.
pub struct DocumentResolver {
    registered: OnceLock<Result<RegisteredDocumentIndex, SourceConfigError>>,
    manual_roots: Vec<PathBuf>,
    manuals: OnceLock<ManualIndex>,
    available: OnceLock<Vec<crate::catalog::AvailableDocument>>,
}

impl DocumentResolver {
    /// Capture the native manual index and lazily snapshot Markdown registration.
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

/// Read at most `limit` bytes of UTF-8, rejecting anything larger.
///
/// The reader is bounded directly instead of trusting a reported length: a pipe
/// or character device such as `/dev/zero` reports no size yet streams without
/// end, so only capping the byte count keeps the read finite.
fn read_capped_utf8(reader: impl Read, limit: u64) -> Result<String, String> {
    read_capped_utf8_io(reader, limit).map_err(|error| error.to_string())
}

/// Read bounded UTF-8 while preserving failures from the underlying reader.
pub(crate) fn read_capped_utf8_io(reader: impl Read, limit: u64) -> io::Result<String> {
    crate::bounded::read_utf8(reader, limit, "Markdown document")
}

fn query_with(
    request: &QueryRequest,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    match &request.input {
        QueryInput::Document {
            selector,
            source,
            manual_section,
        } => query_named_document(
            selector,
            source.as_deref(),
            manual_section.as_deref(),
            policy,
            host,
        ),
        QueryInput::File { path, format } => query_input_file(path, *format, policy, host),
    }
}

fn query_input_file(
    requested_path: &str,
    format: InputFormat,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(QueryError::EmptyMarkdownPath);
    }
    let format = match format {
        InputFormat::Auto => {
            detect_input_format(path).ok_or_else(|| QueryError::UnsupportedInputFormat {
                path: path.to_owned(),
            })?
        }
        format => format,
    };
    match format {
        InputFormat::Markdown => query_markdown_file(path, policy, host),
        InputFormat::Roff => {
            if policy != QueryPolicy::Combined {
                return Err(QueryError::ConflictingSourceSelectors);
            }
            let document = host.parse_manual_input(Path::new(path)).map_err(|detail| {
                QueryError::Manual(ManualLoadError::Parse {
                    name: path.to_owned(),
                    detail,
                })
            })?;
            if document.sections.is_empty() && document.blocks.is_empty() {
                return Err(QueryError::NoReadableContent {
                    name: path.to_owned(),
                });
            }
            let label = document
                .meta
                .names
                .first()
                .cloned()
                .or_else(|| document.meta.title.clone())
                .unwrap_or_else(|| input_file_label(path));
            Ok(ResolvedContent {
                label,
                address: None,
                document: Some(document),
                tldr: None,
            })
        }
        InputFormat::Auto => unreachable!("auto input was resolved above"),
    }
}

fn detect_input_format(path: &str) -> Option<InputFormat> {
    let mut name = Path::new(path).file_name()?.to_str()?.to_ascii_lowercase();
    let mut compressed = false;
    if Path::new(&name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| matches!(extension, "gz" | "zst"))
    {
        name = Path::new(&name).file_stem()?.to_str()?.to_owned();
        compressed = true;
    }
    let extension = Path::new(&name).extension()?.to_str()?;
    if matches!(extension, "md" | "markdown") {
        return (!compressed).then_some(InputFormat::Markdown);
    }
    if matches!(extension, "roff" | "man" | "mdoc") {
        return Some(InputFormat::Roff);
    }
    crate::is_manual_section(extension).then_some(InputFormat::Roff)
}

fn input_file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(path)
        .to_owned()
}

fn query_markdown_file(
    requested_path: &str,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(QueryError::EmptyMarkdownPath);
    }
    if policy != QueryPolicy::Combined {
        return Err(QueryError::Markdown {
            path: path.to_owned(),
            detail: "content-only policies do not apply to direct input".to_owned(),
        });
    }
    let source = host
        .read_markdown(Path::new(path))
        .map_err(|detail| QueryError::Markdown {
            path: path.to_owned(),
            detail,
        })?;
    query_markdown_text(&source, Some(path.to_owned()))
}

/// Parse in-memory Markdown for the direct `mant -` command.
///
/// This helper intentionally sits outside [`QueryRequest`]: public protocol
/// requests reference local files and never embed arbitrary document content.
///
/// # Errors
///
/// Returns [`QueryError::EmptyMarkdown`] when parsing yields no visible blocks
/// or sections.
pub fn query_markdown_text(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, QueryError> {
    let label = source_path.as_deref().map_or_else(
        || "stdin".to_owned(),
        |path| {
            Path::new(path)
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or(path)
                .to_owned()
        },
    );
    let error_path = source_path.clone().unwrap_or_else(|| "stdin".to_owned());
    let parsed = parse_markdown(source, source_path).map_err(|error| QueryError::Markdown {
        path: error_path,
        detail: error.to_string(),
    })?;
    let document_is_empty =
        parsed.document.blocks.is_empty() && parsed.document.sections.is_empty();
    if document_is_empty && parsed.tldr.is_none() {
        return Err(QueryError::EmptyMarkdown {
            label: label.clone(),
        });
    }
    Ok(ResolvedContent {
        address: None,
        label,
        document: (!document_is_empty).then_some(parsed.document),
        tldr: parsed.tldr,
    })
}

/// Parse one bounded roff stream without consulting MANPATH or following `.so`.
///
/// # Errors
///
/// Returns a native parse error or an empty-document error.
pub fn query_roff_bytes(source: &[u8]) -> Result<ResolvedContent, QueryError> {
    if u64::try_from(source.len()).unwrap_or(u64::MAX) > crate::MAX_MANUAL_BYTES {
        return Err(QueryError::Manual(ManualLoadError::Parse {
            name: "stdin".to_owned(),
            detail: format!(
                "roff input exceeds the {}-byte limit",
                crate::MAX_MANUAL_BYTES
            ),
        }));
    }
    let document = parse_manual_bytes(Path::new("stdin"), source).map_err(|error| {
        QueryError::Manual(ManualLoadError::Parse {
            name: "stdin".to_owned(),
            detail: error.to_string(),
        })
    })?;
    if document.sections.is_empty() && document.blocks.is_empty() {
        return Err(QueryError::NoReadableContent {
            name: "stdin".to_owned(),
        });
    }
    let label = document
        .meta
        .names
        .first()
        .cloned()
        .or_else(|| document.meta.title.clone())
        .unwrap_or_else(|| "stdin".to_owned());
    Ok(ResolvedContent {
        address: None,
        label,
        document: Some(document),
        tldr: None,
    })
}

fn query_named_document(
    name: &str,
    requested_source: Option<&str>,
    requested_manual_section: Option<&str>,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(QueryError::EmptyName);
    }
    if let Some(address) = parse_catalog_address(name) {
        if requested_source.is_some() || requested_manual_section.is_some() {
            return Err(QueryError::ConflictingSourceSelectors);
        }
        return query_catalog_address(name, &address, policy, host);
    }
    let section = requested_manual_section.map(str::trim);
    if section.is_some_and(|section| !crate::is_manual_section(section)) {
        return Err(QueryError::InvalidManualSection);
    }
    let section = section.map(ToOwned::to_owned);
    let source = requested_source.map(str::trim);
    if source.is_some_and(str::is_empty) {
        return Err(QueryError::InvalidSource);
    }
    let plan = policy.named_resolution_plan(section.is_some());
    if source.is_some() && (section.is_some() || plan.document == FullDocumentMode::NativeManual) {
        return Err(QueryError::ConflictingSourceSelectors);
    }
    let candidates = host.name_candidates(name);

    if plan.quick_reference == QuickReferenceMode::Only {
        if let Some(section) = section.as_deref()
            && !crate::is_command_manual_section(section)
        {
            return Err(QueryError::TldrManualSection {
                section: section.to_owned(),
            });
        }
        return query_tldr_only(name, &candidates, source, host);
    }

    // Personal documents and positive-priority sources form the preferred
    // registration phase. Explicit source selection always wins regardless of
    // its configured rank. Non-positive sources are consulted only after the
    // priority-zero native-manual phase fails.
    if plan.document == FullDocumentMode::Priority {
        let registered = host
            .locate_registered_document(&candidates, source, RegisteredLookupPhase::BeforeBuiltin)
            .map_err(|detail| QueryError::Registry { detail })?;
        if let Some(registered) = registered {
            return query_registered_document(name, &registered, host);
        }
        if source.is_some() {
            return Err(QueryError::NoReadableContent {
                name: name.to_owned(),
            });
        }
    }

    let mut manual = load_manual(name, &candidates, section.as_deref(), host);

    // A malformed page may omit its own section metadata. Preserve the
    // requested section so labels stay `name(N)`.
    if let (Ok(manual), Some(section)) = (&mut manual, section.as_deref())
        && manual.document.meta.manual_section.is_none()
    {
        manual.document.meta.manual_section = Some(section.to_owned());
    }

    let tldr = match plan.quick_reference {
        QuickReferenceMode::AttachToCommandManual => match &manual {
            Ok(manual) if manual_accepts_tldr(manual) => host.read_tldr(name).ok().flatten(),
            Err(_)
                if section
                    .as_deref()
                    .is_none_or(crate::is_command_manual_section) =>
            {
                host.read_tldr(name).ok().flatten()
            }
            Ok(_) | Err(_) => None,
        },
        QuickReferenceMode::Exclude => None,
        QuickReferenceMode::Only => unreachable!("tldr-only queries returned before manual I/O"),
    };

    match plan.document {
        FullDocumentMode::Priority => {
            finish_unqualified_manual(name, &candidates, manual, tldr, host)
        }
        FullDocumentMode::NativeManual => finish_selected_manual(name, manual, tldr),
        FullDocumentMode::None => unreachable!("tldr-only queries returned before manual I/O"),
    }
}

fn manual_accepts_tldr(manual: &LoadedManual) -> bool {
    let DocumentAddress::Manual { manual_section, .. } = &manual.address else {
        return false;
    };
    crate::is_command_manual_section(manual_section)
}

fn query_catalog_address(
    selector: &str,
    address: &DocumentAddress,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    match address {
        DocumentAddress::Markdown { .. } if policy == QueryPolicy::TldrOnly => {
            let registered = host
                .locate_registered_address(address)
                .map_err(|detail| QueryError::Registry { detail })?
                .ok_or_else(|| QueryError::TldrNotFound {
                    topic: selector.to_owned(),
                })?;
            query_registered_tldr(selector, &registered, host)?.ok_or_else(|| {
                QueryError::TldrNotFound {
                    topic: selector.to_owned(),
                }
            })
        }
        DocumentAddress::Markdown { .. } if policy == QueryPolicy::ManualOnly => {
            Err(QueryError::ConflictingSourceSelectors)
        }
        DocumentAddress::Markdown { .. } => {
            let registered = host
                .locate_registered_address(address)
                .map_err(|detail| QueryError::Registry { detail })?
                .ok_or_else(|| QueryError::NoReadableContent {
                    name: selector.to_owned(),
                })?;
            query_registered_document(selector, &registered, host)
        }
        DocumentAddress::Manual {
            name,
            manual_section,
        } => query_named_document(name, None, Some(manual_section), policy, host),
    }
}

fn query_tldr_only(
    name: &str,
    candidates: &[String],
    source: Option<&str>,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let before = host
        .locate_registered_document_groups(candidates, source, RegisteredLookupPhase::BeforeBuiltin)
        .map_err(|detail| QueryError::Registry { detail })?;
    if let Some(tldr) = first_registered_tldr(name, before, host)? {
        return Ok(tldr);
    }
    if source.is_some() {
        return Err(QueryError::TldrNotFound {
            topic: name.to_owned(),
        });
    }

    if let Some(tldr) = host.read_tldr(name).map_err(|detail| QueryError::Tldr {
        topic: name.to_owned(),
        detail,
    })? {
        return Ok(ResolvedContent {
            address: None,
            label: name.to_owned(),
            document: None,
            tldr: Some(tldr),
        });
    }

    let after = host
        .locate_registered_document_groups(candidates, None, RegisteredLookupPhase::AfterBuiltin)
        .map_err(|detail| QueryError::Registry { detail })?;
    first_registered_tldr(name, after, host)?.ok_or_else(|| QueryError::TldrNotFound {
        topic: name.to_owned(),
    })
}

fn first_registered_tldr(
    name: &str,
    groups: Vec<RegisteredSelectionGroup>,
    host: &dyn QueryHost,
) -> Result<Option<ResolvedContent>, QueryError> {
    for group in groups {
        let mut matches = Vec::new();
        for registered in group.documents {
            if let Some(tldr) = query_registered_tldr(name, &registered, host)? {
                matches.push(tldr);
            }
        }
        match matches.len() {
            0 => {}
            1 => return Ok(matches.pop()),
            _ => {
                let choices = matches
                    .iter()
                    .filter_map(|candidate| candidate.address.as_ref())
                    .map(DocumentAddress::catalog_path)
                    .collect::<Vec<_>>()
                    .join("', '");
                return Err(QueryError::Registry {
                    detail: format!(
                        "tldr selector '{name}' is ambiguous at one document priority: '{choices}'"
                    ),
                });
            }
        }
    }
    Ok(None)
}

fn query_registered_tldr(
    name: &str,
    registered: &RegisteredSelection,
    host: &dyn QueryHost,
) -> Result<Option<ResolvedContent>, QueryError> {
    let resolved = query_registered_document(name, registered, host)?;
    let Some(tldr) = resolved.tldr else {
        return Ok(None);
    };
    Ok(Some(ResolvedContent {
        address: resolved.address,
        label: resolved.label,
        document: None,
        tldr: Some(tldr),
    }))
}

fn finish_selected_manual(
    name: &str,
    manual: Result<LoadedManual, ManualLoadError>,
    tldr: Option<TldrDocument>,
) -> Result<ResolvedContent, QueryError> {
    match manual {
        Ok(manual) => Ok(ResolvedContent {
            address: Some(manual.address),
            label: name.to_owned(),
            document: Some(manual.document),
            tldr,
        }),
        Err(error) if tldr.is_some() => Err(QueryError::ManualWithTldr {
            error,
            topic: name.to_owned(),
        }),
        Err(error) => Err(QueryError::Manual(error)),
    }
}

fn finish_unqualified_manual(
    name: &str,
    candidates: &[String],
    manual: Result<LoadedManual, ManualLoadError>,
    tldr: Option<TldrDocument>,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    match manual {
        Ok(manual) => Ok(ResolvedContent {
            address: Some(manual.address),
            label: name.to_owned(),
            document: Some(manual.document),
            tldr,
        }),
        Err(error) => {
            let registered = host
                .locate_registered_document(candidates, None, RegisteredLookupPhase::AfterBuiltin)
                .map_err(|detail| QueryError::Registry { detail })?;
            if let Some(registered) = registered {
                query_registered_document(name, &registered, host)
            } else if tldr.is_some() {
                Err(QueryError::ManualWithTldr {
                    error,
                    topic: name.to_owned(),
                })
            } else {
                Err(QueryError::Manual(error))
            }
        }
    }
}

fn parse_catalog_address(selector: &str) -> Option<DocumentAddress> {
    if let Some(path) = selector.strip_prefix("documents/")
        && !path.is_empty()
    {
        return Some(DocumentAddress::Markdown {
            path: path.to_owned(),
            origin: MarkdownOrigin::Documents,
        });
    }
    if let Some(rest) = selector.strip_prefix("sources/") {
        let (source, path) = rest.split_once('/')?;
        if !source.is_empty() && !path.is_empty() {
            return Some(DocumentAddress::Markdown {
                path: path.to_owned(),
                origin: MarkdownOrigin::Source {
                    name: source.to_owned(),
                },
            });
        }
    }
    if let Some(rest) = selector.strip_prefix("manual/") {
        let (manual_section, name) = rest.split_once('/')?;
        if !manual_section.is_empty() && !name.is_empty() && !name.contains('/') {
            return Some(DocumentAddress::Manual {
                name: name.to_owned(),
                manual_section: manual_section.to_owned(),
            });
        }
    }
    None
}

fn query_registered_document(
    name: &str,
    registered: &RegisteredSelection,
    host: &dyn QueryHost,
) -> Result<ResolvedContent, QueryError> {
    let path = &registered.path;
    let source_path = path.to_string_lossy().into_owned();
    let source = host
        .read_markdown(path)
        .map_err(|detail| QueryError::Markdown {
            path: source_path.clone(),
            detail,
        })?;
    let mut query = query_markdown_text(&source, Some(source_path))?;
    name.clone_into(&mut query.label);
    query.address = Some(registered.address.clone());
    Ok(query)
}

fn load_manual(
    requested_name: &str,
    candidates: &[String],
    section: Option<&str>,
    host: &dyn QueryHost,
) -> Result<LoadedManual, ManualLoadError> {
    let mut first_locate_error = None;
    let mut located = None;
    for candidate in candidates {
        let request = ManualRequest::new(candidate, section.map(ToOwned::to_owned));
        match host.locate_manual(&request) {
            Ok(page) => {
                located = Some(page);
                break;
            }
            Err(error) => {
                first_locate_error.get_or_insert(error);
            }
        }
    }
    let Some(page) = located else {
        let error =
            first_locate_error.unwrap_or_else(|| "no name candidates were available".to_owned());
        return Err(ManualLoadError::NotFound {
            name: requested_name.to_owned(),
            detail: error,
        });
    };

    let source_path = page.path.clone();
    let address = DocumentAddress::Manual {
        name: page.name.clone(),
        manual_section: page.section.clone(),
    };
    let document = host
        .parse_manual(&page)
        .map_err(|detail| ManualLoadError::Parse {
            name: requested_name.to_owned(),
            detail,
        })?;
    if document.sections.is_empty() {
        let diagnostics = document
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let location = diagnostic.source.map_or_else(String::new, |source| {
                    format!(" at {}:{}", source.line, source.column)
                });
                format!("{:?}{location}: {}", diagnostic.level, diagnostic.message)
            })
            .collect::<Vec<_>>();
        return Err(ManualLoadError::Empty {
            name: requested_name.to_owned(),
            path: source_path,
            diagnostics,
        });
    }
    Ok(LoadedManual { document, address })
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use mant_ir::{
        Diagnostic, DiagnosticLevel, Document, DocumentMeta, DocumentSource, Section, SourceFormat,
        TldrDocument, TldrOrigin,
    };
    use mant_protocol::{
        DocumentAddress, InputFormat, MarkdownOrigin, QueryInput, QueryRequest, QueryView,
        RequestSchema,
    };
    use mant_sources::BUILTIN_CONTENT_PRIORITY;

    use crate::{ManualPage, ManualRequest};

    use super::{
        MAX_MARKDOWN_BYTES, QueryError, QueryHost, QueryPolicy, RegisteredLookupPhase,
        RegisteredSelection, RegisteredSelectionGroup, query_markdown_text, query_with,
        read_capped_utf8, read_capped_utf8_io,
    };

    #[derive(Clone)]
    struct StubHost {
        name_candidates: Option<Vec<String>>,
        registered_document: Option<PathBuf>,
        registered_name: Option<String>,
        registered_source_priority: Option<i32>,
        locate: Result<ManualPage, String>,
        manual_name: Option<String>,
        direct: Result<Document, String>,
        tldr: Result<Option<TldrDocument>, String>,
        markdown: Result<String, String>,
        calls: std::sync::Arc<Mutex<Vec<&'static str>>>,
    }

    impl QueryHost for StubHost {
        fn name_candidates(&self, name: &str) -> Vec<String> {
            self.name_candidates
                .clone()
                .unwrap_or_else(|| vec![name.to_owned()])
        }

        fn locate_registered_document(
            &self,
            candidates: &[String],
            source: Option<&str>,
            phase: RegisteredLookupPhase,
        ) -> Result<Option<RegisteredSelection>, String> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(if source.is_some() {
                    "source"
                } else {
                    match phase {
                        RegisteredLookupPhase::BeforeBuiltin => "name",
                        RegisteredLookupPhase::AfterBuiltin => "fallback",
                    }
                });
            if source.is_none()
                && match phase {
                    RegisteredLookupPhase::BeforeBuiltin => self
                        .registered_source_priority
                        .is_some_and(|priority| priority <= BUILTIN_CONTENT_PRIORITY),
                    RegisteredLookupPhase::AfterBuiltin => self
                        .registered_source_priority
                        .is_none_or(|priority| priority > BUILTIN_CONTENT_PRIORITY),
                }
            {
                return Ok(None);
            }
            if self
                .registered_name
                .as_deref()
                .is_some_and(|registered_name| {
                    !candidates
                        .iter()
                        .any(|candidate| candidate == registered_name)
                })
            {
                return Ok(None);
            }
            Ok(self
                .registered_document
                .clone()
                .map(|path| RegisteredSelection {
                    path,
                    address: DocumentAddress::Markdown {
                        path: self
                            .registered_name
                            .clone()
                            .unwrap_or_else(|| candidates[0].clone()),
                        origin: source.map_or_else(
                            || {
                                self.registered_source_priority.map_or(
                                    MarkdownOrigin::Documents,
                                    |_| MarkdownOrigin::Source {
                                        name: "team".to_owned(),
                                    },
                                )
                            },
                            |name| MarkdownOrigin::Source {
                                name: name.to_owned(),
                            },
                        ),
                    },
                }))
        }

        fn locate_registered_document_groups(
            &self,
            candidates: &[String],
            source: Option<&str>,
            phase: RegisteredLookupPhase,
        ) -> Result<Vec<RegisteredSelectionGroup>, String> {
            self.locate_registered_document(candidates, source, phase)
                .map(|selection| {
                    selection
                        .map(|value| {
                            vec![RegisteredSelectionGroup {
                                documents: vec![value],
                            }]
                        })
                        .unwrap_or_default()
                })
        }

        fn locate_registered_address(
            &self,
            address: &DocumentAddress,
        ) -> Result<Option<RegisteredSelection>, String> {
            self.calls.lock().expect("calls lock").push("address");
            Ok(self
                .registered_document
                .clone()
                .map(|path| RegisteredSelection {
                    path,
                    address: address.clone(),
                }))
        }

        fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String> {
            self.calls.lock().expect("calls lock").push("locate");
            if self
                .manual_name
                .as_deref()
                .is_some_and(|manual_name| manual_name != request.name)
            {
                return Err("source not found".to_owned());
            }
            self.locate.clone()
        }

        fn parse_manual(&self, _page: &ManualPage) -> Result<Document, String> {
            self.calls.lock().expect("calls lock").push("parse");
            self.direct.clone()
        }

        fn parse_manual_input(&self, _path: &Path) -> Result<Document, String> {
            self.calls.lock().expect("calls lock").push("manual-input");
            self.direct.clone()
        }

        fn read_tldr(&self, _name: &str) -> Result<Option<TldrDocument>, String> {
            self.calls.lock().expect("calls lock").push("tldr");
            self.tldr.clone()
        }

        fn read_markdown(&self, _path: &Path) -> Result<String, String> {
            self.calls.lock().expect("calls lock").push("markdown");
            self.markdown.clone()
        }
    }

    fn document(format: SourceFormat, unsupported: bool, readable: bool) -> Document {
        Document {
            parser: None,
            source: DocumentSource { format, path: None },
            meta: DocumentMeta::default(),
            diagnostics: unsupported
                .then_some(Diagnostic {
                    level: DiagnosticLevel::Unsupported,
                    code: None,
                    message: "unsupported request".to_owned(),
                    source: None,
                })
                .into_iter()
                .collect(),
            blocks: Vec::new(),
            sections: readable
                .then_some(Section {
                    id: "name-1".to_owned().into(),
                    title: "NAME".to_owned(),
                    spacing_before_lines: 0,
                    blocks: Vec::new(),
                    children: Vec::new(),
                    source: None,
                })
                .into_iter()
                .collect(),
        }
    }

    fn tldr() -> TldrDocument {
        TldrDocument {
            title: "tool".to_owned(),
            description: vec!["quick reference".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/pages/common/tool.md".to_owned(),
            origin: TldrOrigin::TldrPages,
        }
    }

    fn embedded_tldr_markdown() -> String {
        "\
<!-- mant:tldr:start -->
# tool

> Source-owned quick reference.

- Run the tool:

`tool`
<!-- mant:tldr:end -->

# Tool

Full documentation.
"
        .to_owned()
    }

    fn host(direct: Result<Document, String>) -> StubHost {
        StubHost {
            name_candidates: None,
            registered_document: None,
            registered_name: None,
            registered_source_priority: None,
            locate: Ok(ManualPage {
                name: "tool".to_owned(),
                section: "1".to_owned(),
                path: PathBuf::from("/man/tool.1"),
                manual_root: PathBuf::from("/man"),
            }),
            manual_name: None,
            direct,
            tldr: Ok(None),
            markdown: Err("Markdown unavailable".to_owned()),
            calls: std::sync::Arc::default(),
        }
    }

    fn request() -> QueryRequest {
        QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: " tool ".to_owned(),
                source: None,
                manual_section: None,
            },
            view: QueryView::Full {},
        }
    }

    #[test]
    fn ordinary_manual_uses_the_native_parser() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

        assert_eq!(result.label, "tool");
        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "locate", "parse", "tldr"]
        );
    }

    #[test]
    fn ordinary_command_manuals_can_attach_cached_tldr() {
        for section in ["1", "1p", "8", "8x"] {
            let mut host = host(Ok(document(SourceFormat::Man, false, true)));
            host.locate.as_mut().expect("manual page").section = section.to_owned();
            host.tldr = Ok(Some(tldr()));

            let result = query_with(&request(), QueryPolicy::default(), &host)
                .expect("command manual query");

            assert_eq!(result.tldr.expect("attached tldr").title, "tool");
            assert_eq!(
                *host.calls.lock().expect("calls lock"),
                ["name", "locate", "parse", "tldr"]
            );
        }
    }

    #[test]
    fn non_command_manuals_do_not_attach_or_probe_cached_tldr() {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.locate.as_mut().expect("manual page").section = "5".to_owned();
        host.tldr = Ok(Some(tldr()));

        let result =
            query_with(&request(), QueryPolicy::default(), &host).expect("file format manual");

        assert!(result.tldr.is_none());
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "locate", "parse"]
        );
    }

    #[test]
    fn requested_manual_section_backfills_metadata_the_parser_left_empty() {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.locate.as_mut().expect("manual page").section = "3".to_owned();
        host.tldr = Ok(Some(tldr()));
        let request = QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "tool".to_owned(),
                source: None,
                manual_section: Some("3".to_owned()),
            },
            view: QueryView::Full {},
        };

        let result = query_with(&request, QueryPolicy::default(), &host).expect("query");
        assert_eq!(
            result.address,
            Some(DocumentAddress::Manual {
                name: "tool".to_owned(),
                manual_section: "3".to_owned(),
            })
        );
        assert_eq!(
            result
                .document
                .as_ref()
                .expect("manual")
                .meta
                .manual_section
                .as_deref(),
            Some("3"),
            "requested section must label output when the parser omits it"
        );
        assert!(
            result.tldr.is_none(),
            "a non-command manual category cannot inherit a tldr page"
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["locate", "parse"],
            "an explicit non-command section bypasses Markdown and tldr lookup"
        );
    }

    #[test]
    fn requested_command_section_keeps_the_combined_tldr_facet() {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.tldr = Ok(Some(tldr()));
        let request = QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "tool".to_owned(),
                source: None,
                manual_section: Some("1".to_owned()),
            },
            view: QueryView::Full {},
        };

        let result = query_with(&request, QueryPolicy::Combined, &host)
            .expect("section-qualified combined query");

        assert_eq!(result.tldr.expect("attached tldr").title, "tool");
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["locate", "parse", "tldr"]
        );
    }

    #[test]
    fn tldr_only_accepts_command_sections_and_rejects_other_categories() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.tldr = Ok(Some(tldr()));
        let request_for = |section: &str| QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "tool".to_owned(),
                source: None,
                manual_section: Some(section.to_owned()),
            },
            view: QueryView::Full {},
        };

        let result = query_with(&request_for("1"), QueryPolicy::TldrOnly, &host)
            .expect("section 1 identifies a command topic");
        assert_eq!(result.tldr.expect("tldr").title, "tool");

        assert_eq!(
            query_with(&request_for("5"), QueryPolicy::TldrOnly, &host),
            Err(QueryError::TldrManualSection {
                section: "5".to_owned(),
            })
        );
    }

    #[test]
    fn explicit_source_reads_only_registered_markdown() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/documents/tool.md"));
        host.markdown = Ok("# Tool\n\nSource body.\n".to_owned());
        let request = QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "tool".to_owned(),
                source: Some("team".to_owned()),
                manual_section: None,
            },
            view: QueryView::Full {},
        };
        let result = query_with(&request, QueryPolicy::default(), &host).expect("source query");
        assert_eq!(
            result.address,
            Some(DocumentAddress::Markdown {
                path: "tool".to_owned(),
                origin: MarkdownOrigin::Source {
                    name: "team".to_owned(),
                },
            })
        );
        assert_eq!(
            result.document.expect("Markdown").meta.title.as_deref(),
            Some("Tool")
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["source", "markdown"]
        );
    }

    #[test]
    fn canonical_catalog_paths_resolve_exact_addresses() {
        let mut markdown = host(Err("manual must not be read".to_owned()));
        markdown.registered_document = Some(PathBuf::from("/documents/en/tool.md"));
        markdown.markdown = Ok("# Tool\n\nBody.\n".to_owned());
        let request = QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "documents/en/tool".to_owned(),
                source: None,
                manual_section: None,
            },
            view: QueryView::Full {},
        };
        let result = query_with(&request, QueryPolicy::default(), &markdown).expect("canonical");
        assert_eq!(
            result.address,
            Some(DocumentAddress::Markdown {
                path: "en/tool".to_owned(),
                origin: MarkdownOrigin::Documents,
            })
        );
        assert_eq!(
            *markdown.calls.lock().expect("calls"),
            ["address", "markdown"]
        );

        let manual = host(Ok(document(SourceFormat::Man, false, true)));
        let request = QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "manual/1/tool".to_owned(),
                source: None,
                manual_section: None,
            },
            view: QueryView::Full {},
        };
        let result = query_with(&request, QueryPolicy::default(), &manual).expect("manual path");
        assert_eq!(
            result.address,
            Some(DocumentAddress::Manual {
                name: "tool".to_owned(),
                manual_section: "1".to_owned(),
            })
        );
        assert_eq!(
            *manual.calls.lock().expect("calls"),
            ["locate", "parse", "tldr"]
        );
    }

    #[test]
    fn complete_direct_document_survives_an_unsupported_finding() {
        let host = host(Ok(document(SourceFormat::Man, true, true)));
        let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "locate", "parse", "tldr"]
        );
    }

    #[test]
    fn manual_only_bypasses_registered_markdown() {
        let mut host = host(Ok(document(SourceFormat::Man, true, true)));
        host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
        host.markdown = Ok("# Registered".to_owned());
        host.tldr = Ok(Some(tldr()));
        let result =
            query_with(&request(), QueryPolicy::ManualOnly, &host).expect("manual-only query");

        assert_eq!(
            result.document.as_ref().expect("manual").source.format,
            SourceFormat::Man
        );
        assert!(result.tldr.is_none(), "manual-only must not attach tldr");
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["locate", "parse"],
            "manual-only lookup must not inspect Markdown or tldr namespaces"
        );
    }

    #[test]
    fn manual_only_failure_is_not_hidden_by_tldr() {
        let mut host = host(Ok(document(SourceFormat::Man, true, false)));
        host.tldr = Ok(Some(tldr()));

        let error = query_with(&request(), QueryPolicy::ManualOnly, &host)
            .expect_err("an optional tldr page must not hide native parser failure");

        let QueryError::Manual(detail) = error else {
            panic!("expected the native parser diagnostic");
        };
        assert!(detail.to_string().contains("/man/tool.1"));
        assert!(
            detail
                .to_string()
                .contains("Unsupported: unsupported request")
        );
        assert_eq!(*host.calls.lock().expect("calls lock"), ["locate", "parse"]);
    }

    #[test]
    fn requested_manual_section_failure_is_not_hidden_by_tldr() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("section not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let request = QueryRequest {
            schema: RequestSchema::V0Dot8,
            input: QueryInput::Document {
                selector: "tool".to_owned(),
                source: None,
                manual_section: Some("7".to_owned()),
            },
            view: QueryView::Full {},
        };

        let error = query_with(&request, QueryPolicy::default(), &host)
            .expect_err("an explicit section must require a native manual");

        assert!(matches!(&error, QueryError::Manual(_)));
        assert!(error.to_string().contains("section not found"));
        assert_eq!(*host.calls.lock().expect("calls lock"), ["locate"]);
    }

    #[test]
    fn truncated_unsupported_document_is_an_error_by_default() {
        let host = host(Ok(document(SourceFormat::Man, true, false)));

        let QueryError::Manual(detail) = query_with(&request(), QueryPolicy::default(), &host)
            .expect_err("empty-section document must error by default")
        else {
            panic!("expected Manual error");
        };
        assert!(detail.to_string().contains("produced no readable sections"));
    }

    #[test]
    fn readable_best_effort_document_survives_parser_findings() {
        let host = host(Ok(document(SourceFormat::Mdoc, true, true)));
        let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");
        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Mdoc
        );
    }

    #[test]
    fn ordinary_query_reports_a_tldr_hint_after_total_document_failure() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let error = query_with(&request(), QueryPolicy::default(), &host)
            .expect_err("ordinary query must require a full document");

        assert!(matches!(error, QueryError::ManualWithTldr { .. }));
        assert_eq!(
            error.to_string(),
            "could not load manual 'tool': source not found\nhint: a tldr entry is available; run `mant tool --tldr`"
        );
    }

    #[test]
    fn explicit_tldr_policy_survives_total_manual_failure() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let result =
            query_with(&request(), QueryPolicy::TldrOnly, &host).expect("explicit tldr-only query");

        assert!(result.document.is_none());
        assert_eq!(result.tldr.expect("tldr").title, "tool");
    }

    #[test]
    fn positive_source_embedded_tldr_precedes_the_builtin_cache() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(1);
        host.markdown = Ok(embedded_tldr_markdown());
        host.tldr = Ok(Some(tldr()));

        let result = query_with(&request(), QueryPolicy::TldrOnly, &host)
            .expect("positive-priority embedded tldr");

        assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
        assert_eq!(
            result.address,
            Some(DocumentAddress::Markdown {
                path: "tool".to_owned(),
                origin: MarkdownOrigin::Source {
                    name: "team".to_owned(),
                },
            })
        );
        assert_eq!(*host.calls.lock().expect("calls"), ["name", "markdown"]);
    }

    #[test]
    fn builtin_tldr_cache_wins_a_zero_priority_tie() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(0);
        host.markdown = Ok(embedded_tldr_markdown());
        host.tldr = Ok(Some(tldr()));

        let result =
            query_with(&request(), QueryPolicy::TldrOnly, &host).expect("builtin tldr cache");

        assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::TldrPages);
        assert_eq!(*host.calls.lock().expect("calls"), ["name", "tldr"]);
    }

    #[test]
    fn tldr_lookup_skips_markdown_without_an_embedded_quick_reference() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(10);
        host.markdown = Ok("# Tool\n\nFull documentation only.\n".to_owned());
        host.tldr = Ok(Some(tldr()));

        let result = query_with(&request(), QueryPolicy::TldrOnly, &host)
            .expect("cached tldr after empty Markdown candidate");

        assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::TldrPages);
        assert_eq!(
            *host.calls.lock().expect("calls"),
            ["name", "markdown", "tldr"]
        );
    }

    #[test]
    fn negative_source_embedded_tldr_is_the_final_fallback() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(-1);
        host.markdown = Ok(embedded_tldr_markdown());

        let result = query_with(&request(), QueryPolicy::TldrOnly, &host)
            .expect("negative-priority embedded tldr");

        assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
        assert_eq!(
            *host.calls.lock().expect("calls"),
            ["name", "tldr", "fallback", "markdown"]
        );
    }

    #[test]
    fn explicit_source_limits_tldr_lookup_to_that_source() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(-1);
        host.markdown = Ok(embedded_tldr_markdown());
        host.tldr = Ok(Some(tldr()));
        let mut request = request();
        let QueryInput::Document { source, .. } = &mut request.input else {
            unreachable!("document request")
        };
        *source = Some("team".to_owned());

        let result =
            query_with(&request, QueryPolicy::TldrOnly, &host).expect("source-owned embedded tldr");

        assert_eq!(result.tldr.expect("tldr").origin, TldrOrigin::Embedded);
        assert_eq!(*host.calls.lock().expect("calls"), ["source", "markdown"]);
    }

    #[test]
    fn reports_both_manual_paths_when_no_content_exists() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        let error = query_with(&request(), QueryPolicy::default(), &host)
            .expect_err("empty query must fail");
        assert_eq!(
            error.to_string(),
            "could not load manual 'tool': source not found"
        );
    }

    #[test]
    fn validates_before_touching_host_state() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        assert_eq!(
            query_with(
                &QueryRequest {
                    schema: RequestSchema::V0Dot8,
                    input: QueryInput::Document {
                        selector: " ".to_owned(),
                        source: None,
                        manual_section: None,
                    },
                    view: QueryView::Full {},
                },
                QueryPolicy::default(),
                &host
            ),
            Err(QueryError::EmptyName)
        );
        assert!(host.calls.lock().expect("calls lock").is_empty());
    }

    #[test]
    fn registered_markdown_shadows_an_unqualified_manual_name() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
        host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());

        let result = query_with(&request(), QueryPolicy::default(), &host)
            .expect("registered Markdown name");

        assert_eq!(result.label, "tool");
        assert!(result.tldr.is_none());
        let document = result.document.expect("registered document");
        assert_eq!(document.source.format, SourceFormat::Markdown);
        assert_eq!(document.source.path.as_deref(), Some("/data/mant/tool.md"));
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "markdown"],
            "a registered name must not consult man or external tldr caches"
        );
    }

    #[test]
    fn positive_source_priority_shadows_a_native_manual() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(1);
        host.markdown = Ok("# Team tool\n\nConfigured documentation.\n".to_owned());

        let result = query_with(&request(), QueryPolicy::default(), &host)
            .expect("positive-priority Markdown");

        assert_eq!(
            result.document.expect("document").source.format,
            SourceFormat::Markdown
        );
        assert_eq!(*host.calls.lock().expect("calls"), ["name", "markdown"]);
    }

    #[test]
    fn native_manual_wins_a_zero_priority_tie() {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(0);
        host.markdown = Ok("# Team tool\n\nConfigured documentation.\n".to_owned());

        let result = query_with(&request(), QueryPolicy::default(), &host).expect("native manual");

        assert_eq!(
            result.document.expect("document").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls"),
            ["name", "locate", "parse", "tldr"]
        );
    }

    #[test]
    fn non_positive_source_priority_falls_back_when_the_manual_is_unavailable() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.locate = Err("source not found".to_owned());
        host.registered_document = Some(PathBuf::from("/sources/team/tool.md"));
        host.registered_source_priority = Some(-1);
        host.markdown = Ok("# Team tool\n\nConfigured documentation.\n".to_owned());

        let result =
            query_with(&request(), QueryPolicy::default(), &host).expect("Markdown fallback");

        assert_eq!(
            result.document.expect("document").source.format,
            SourceFormat::Markdown
        );
        assert_eq!(
            *host.calls.lock().expect("calls"),
            ["name", "locate", "tldr", "fallback", "markdown"]
        );
    }

    #[test]
    fn windows_suffix_fallback_can_resolve_registered_markdown() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.name_candidates = Some(vec!["tool".to_owned(), "tool.EXE".to_owned()]);
        host.registered_name = Some("tool.EXE".to_owned());
        host.registered_document = Some(PathBuf::from("/data/mant/tool.exe.md"));
        host.markdown = Ok("# Tool executable\n\nWindows command documentation.\n".to_owned());

        let result = query_with(&request(), QueryPolicy::default(), &host)
            .expect("registered executable document");

        assert_eq!(result.label, "tool");
        assert_eq!(
            result.document.expect("document").source.path.as_deref(),
            Some("/data/mant/tool.exe.md")
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "markdown"]
        );
    }

    #[test]
    fn windows_suffix_fallback_can_resolve_a_native_manual() {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.name_candidates = Some(vec!["tool".to_owned(), "tool.EXE".to_owned()]);
        host.manual_name = Some("tool.EXE".to_owned());
        host.locate = Ok(ManualPage {
            name: "tool.exe".to_owned(),
            section: "1".to_owned(),
            path: PathBuf::from("/man/tool.exe.1"),
            manual_root: PathBuf::from("/man"),
        });

        let result = query_with(&request(), QueryPolicy::default(), &host)
            .expect("native executable manual");

        assert_eq!(result.label, "tool");
        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "locate", "locate", "parse", "tldr"]
        );
    }

    #[test]
    fn exact_names_win_before_windows_suffix_fallback() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.name_candidates = Some(vec!["tool".to_owned(), "tool.EXE".to_owned()]);
        host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
        host.markdown = Ok("# Exact tool\n\nExact-name documentation.\n".to_owned());

        let result = query_with(&request(), QueryPolicy::default(), &host)
            .expect("exact registered document");

        assert_eq!(
            result.document.expect("document").source.path.as_deref(),
            Some("/data/mant/tool.md")
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "markdown"]
        );
    }

    #[test]
    fn markdown_files_bypass_manual_and_tldr_sources() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());
        let result = query_with(
            &QueryRequest {
                schema: RequestSchema::V0Dot8,
                input: QueryInput::File {
                    path: "docs/tool.md".to_owned(),
                    format: InputFormat::Markdown,
                },
                view: QueryView::Full {},
            },
            QueryPolicy::default(),
            &host,
        )
        .expect("Markdown query");

        assert_eq!(result.label, "tool.md");
        assert!(result.tldr.is_none());
        let document = result.document.expect("document");
        assert_eq!(document.source.format, SourceFormat::Markdown);
        assert_eq!(document.source.path.as_deref(), Some("docs/tool.md"));
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["markdown"],
            "Markdown must not consult man or tldr"
        );
    }

    #[test]
    fn in_memory_markdown_is_available_without_a_protocol_content_field() {
        let result = query_markdown_text("# Piped\n\nBody.\n", None).expect("stdin Markdown query");

        assert_eq!(result.label, "stdin");
        assert!(result.tldr.is_none());
        let document = result.document.expect("document");
        assert_eq!(document.meta.title.as_deref(), Some("Piped"));
        assert_eq!(document.source.path, None);
    }

    #[test]
    fn leading_tldr_directives_are_independent_from_the_markdown_document() {
        let source = "\
<!-- mant:tldr:start -->
# demo

> Concise embedded help.

- Run the demo:

`demo {{path}}`
<!-- mant:tldr:end -->

# Demo

Document overview.

## Options

- `--help`: Show help.
";
        let result =
            query_markdown_text(source, Some("docs/demo.md".to_owned())).expect("Markdown query");

        let tldr = result.tldr.expect("embedded tldr");
        assert_eq!(tldr.title, "demo");
        assert_eq!(tldr.origin, TldrOrigin::Embedded);
        assert_eq!(tldr.source_path, "docs/demo.md");
        assert_eq!(tldr.examples[0].command, "demo {{path}}");

        let document = result.document.expect("document body");
        assert_eq!(document.meta.title.as_deref(), Some("Demo"));
        assert_eq!(document.sections[0].title, "Options");
        assert!(
            document
                .blocks
                .iter()
                .any(|block| matches!(block, mant_ir::Block::Paragraph { .. }))
        );
        assert!(
            document
                .diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.contains("mant:tldr"))
        );
    }

    #[test]
    fn malformed_leading_tldr_directives_report_the_source_path() {
        let error = query_markdown_text(
            "<!-- mant:tldr:start -->\n# demo\n\n- Run:\n\n`demo`\n",
            Some("docs/broken.md".to_owned()),
        )
        .expect_err("unterminated directive");

        assert_eq!(
            error.to_string(),
            "could not load Markdown document 'docs/broken.md': top-level <!-- mant:tldr:start --> marker is missing its <!-- mant:tldr:end --> marker"
        );
    }

    #[test]
    fn capped_read_accepts_input_up_to_the_limit() {
        let source = "abcd";
        assert_eq!(
            read_capped_utf8(source.as_bytes(), source.len() as u64).expect("within limit"),
            source
        );
    }

    #[test]
    fn capped_read_rejects_input_past_the_limit_without_buffering_it_whole() {
        // An unbounded stream (modelled by io::repeat) must fail fast on the
        // limit rather than read forever, matching the /dev/zero guard.
        let error = read_capped_utf8(io::repeat(b'a'), 8).expect_err("over limit");
        assert!(error.contains("exceeds the 8-byte limit"), "{error}");
    }

    #[test]
    fn capped_read_rejects_non_utf8_input() {
        let error =
            read_capped_utf8(&[0xff, 0xfe][..], MAX_MARKDOWN_BYTES).expect_err("invalid UTF-8");
        assert!(error.contains("must be UTF-8"), "{error}");
    }

    #[test]
    fn capped_io_read_preserves_the_underlying_error_kind() {
        struct PermissionDeniedReader;

        impl io::Read for PermissionDeniedReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "reader denied access",
                ))
            }
        }

        let error = read_capped_utf8_io(PermissionDeniedReader, MAX_MARKDOWN_BYTES)
            .expect_err("reader failure is preserved");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "reader denied access");
    }
}
