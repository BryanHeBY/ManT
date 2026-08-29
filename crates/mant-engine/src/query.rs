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
    CatalogQuery, DocumentCatalog, EntryProjection, InputFormat, MAX_SEMANTIC_ENTRY_BYTES,
    QueryExcerpt, QueryInput, QueryOutline, QueryRequest, QuerySearch, QueryView, ScopeTextError,
    SearchCase, SearchQuery, SearchScope, SearchSyntax, validate_scope_text,
};
use mant_sources::{RegisteredDocumentIndex, RegisteredDocumentOrigin, SourceConfigError};

use crate::{
    ManualIndex, ManualPage, ManualRequest, ProjectionError, SearchError, discover_manual_roots,
    executable::query_name_candidates, locate_manual_source_in, parse_manual_bytes,
    parse_manual_page, parse_manual_source, parse_markdown, read_cached_tldr_page, search_query,
    select_excerpt, select_explanation, validate_search_query,
};

mod input;
mod named;

use input::query_with;
pub use input::{query_markdown_text, query_roff_bytes};
use named::query_named_document;

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
    /// A role-filtered outline contained no kinds or exceeded the closed kind family.
    InvalidEntryKinds,
    /// An explanation entry name was empty.
    EmptyEntry,
    /// A node or semantic-entry selector violated the bounded request contract.
    InvalidViewSelector {
        /// User-facing field name.
        field: &'static str,
        /// Precise bound or character violation.
        error: ScopeTextError,
    },
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
            Self::InvalidEntryKinds => {
                formatter.write_str("outline entry kinds must contain between 1 and 9 values")
            }
            Self::EmptyEntry => formatter.write_str("semantic entry must not be empty"),
            Self::InvalidViewSelector { field, error } => {
                write!(formatter, "{field} {}", view_selector_error_message(*error))
            }
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
            | Self::InvalidEntryKinds
            | Self::EmptyEntry
            | Self::InvalidViewSelector { .. }
            | Self::Markdown { .. }
            | Self::EmptyMarkdown { .. }
            | Self::Registry { .. }
            | Self::TldrNotFound { .. }
            | Self::Tldr { .. }
            | Self::NoReadableContent { .. } => None,
        }
    }
}

fn view_selector_error_message(error: ScopeTextError) -> String {
    match error {
        ScopeTextError::Empty => "must not be empty".to_owned(),
        ScopeTextError::ControlCharacter => "must not contain control characters".to_owned(),
        ScopeTextError::TooLong { maximum } => {
            format!("must not exceed {maximum} bytes")
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
        QueryView::Outline { entries, root } => {
            { crate::projection::build_outline_projection(&query, entries.clone(), root.clone()) }
                .map(QueryViewResult::Outline)
                .map_err(QueryExecutionError::Projection)
        }
        QueryView::Excerpt { selectors } => select_excerpt(&query, selectors)
            .map(QueryViewResult::Excerpt)
            .map_err(QueryExecutionError::Projection),
        QueryView::Explain { entry } => select_explanation_with_text_hint(&query, entry)
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

pub(crate) fn select_explanation_with_text_hint(
    query: &ResolvedContent,
    entry: &str,
) -> Result<QueryExcerpt, ProjectionError> {
    match select_explanation(query, entry) {
        Err(ProjectionError::UnknownSelector { document, selector }) => {
            let probe = SearchQuery {
                pattern: selector.clone(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Insensitive,
                scope: SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 1,
                offset: 0,
            };
            if let Some(found) = search_query(query, &probe)
                .ok()
                .and_then(|result| result.matches.into_iter().next())
            {
                let line = found
                    .occurrences
                    .first()
                    .map_or(1, |occurrence| occurrence.markdown.start_line);
                return Err(ProjectionError::SelectorFoundOnlyInText {
                    document,
                    selector,
                    path: found.outline.path().to_owned(),
                    title: found.outline.title().to_owned(),
                    line,
                });
            }
            Err(ProjectionError::UnknownSelector { document, selector })
        }
        result => result,
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
    validate_query_view(&request.view)
}

fn validate_query_view(view: &QueryView) -> Result<(), QueryError> {
    match view {
        QueryView::Excerpt { selectors } => {
            if selectors.is_empty() {
                return Err(QueryError::EmptySelection);
            }
            for selector in selectors {
                validate_scope_text(selector, MAX_SEMANTIC_ENTRY_BYTES).map_err(|error| {
                    if error == ScopeTextError::Empty {
                        QueryError::EmptySelector
                    } else {
                        QueryError::InvalidViewSelector {
                            field: "outline node",
                            error,
                        }
                    }
                })?;
            }
        }
        QueryView::Explain { entry } => {
            validate_scope_text(entry, MAX_SEMANTIC_ENTRY_BYTES).map_err(|error| {
                if error == ScopeTextError::Empty {
                    QueryError::EmptyEntry
                } else {
                    QueryError::InvalidViewSelector {
                        field: "semantic entry",
                        error,
                    }
                }
            })?;
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
        QueryView::Outline { entries, root } => {
            if let Some(selector) = root {
                validate_scope_text(selector, MAX_SEMANTIC_ENTRY_BYTES).map_err(|error| {
                    if error == ScopeTextError::Empty {
                        QueryError::EmptySelector
                    } else {
                        QueryError::InvalidViewSelector {
                            field: "outline root",
                            error,
                        }
                    }
                })?;
            }
            if let EntryProjection::Kinds { kinds } = entries
                && (kinds.is_empty() || kinds.len() > 9)
            {
                return Err(QueryError::InvalidEntryKinds);
            }
        }
        QueryView::Full {} => {}
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

#[cfg(test)]
mod tests;
