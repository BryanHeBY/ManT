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
    CatalogQuery, DocumentCatalog, EntryProjection, InputFormat, MAX_DOCUMENT_SELECTOR_CHARS,
    MAX_NODE_SELECTORS, MAX_SEMANTIC_ENTRY_CHARS, MAX_SOURCE_SELECTOR_CHARS, QueryExcerpt,
    QueryInput, QueryOutline, QueryRequest, QuerySearch, QueryView, ScopeTextError, SearchQuery,
    validate_scope_text,
};
use mant_sources::{RegisteredDocumentIndex, RegisteredDocumentOrigin, SourceConfigError};

use crate::{
    ManualIndex, ManualPage, ManualRequest, ProjectionError, SearchError, discover_manual_roots,
    executable::query_name_candidates, locate_manual_source_in, parse_manual_bytes,
    parse_manual_page, parse_manual_source, read_cached_tldr_page, search_query, select_excerpt,
    validate_search_query,
};

mod execution;
mod input;
mod named;
mod resolver;
mod validation;
pub use execution::project_query_view;
pub use resolver::DocumentResolver;
pub use validation::validate_query_request;

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
    /// Excerpt projection exceeded the closed selector-count bound.
    TooManySelections {
        /// Maximum selectors accepted by one focused request.
        maximum: usize,
    },
    /// An excerpt selector was empty.
    EmptySelector,
    /// An explicit local selector violated its path/ID grammar or byte limit.
    InvalidContentSelector,
    /// Reference inventory policy violated its closed bounds.
    InvalidReferenceProjection(&'static str),
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
    /// Explanation configuration failed validation or had no readable content.
    InvalidExplanation(crate::ExplanationError),
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
    /// Independent semantic evidence; multiple and zero owners are normal.
    Explanation(mant_protocol::QueryExplanation),
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
            Self::TooManySelections { maximum } => {
                write!(
                    formatter,
                    "outline nodes must not contain more than {maximum} values"
                )
            }
            Self::EmptySelector => formatter.write_str("outline node must not be empty"),
            Self::InvalidContentSelector => mant_protocol::InvalidContentSelector.fmt(formatter),
            Self::InvalidReferenceProjection(reason) => formatter.write_str(reason),
            Self::InvalidEntryKinds => {
                formatter.write_str("outline entry kinds must contain between 1 and 9 values")
            }
            Self::EmptyEntry => formatter.write_str("semantic entry must not be empty"),
            Self::InvalidViewSelector { field, error } => {
                write!(formatter, "{field} {}", view_selector_error_message(*error))
            }
            Self::InvalidSearch(error) => error.fmt(formatter),
            Self::InvalidExplanation(error) => error.fmt(formatter),
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
            Self::InvalidExplanation(error) => Some(error),
            Self::Manual(error) | Self::ManualWithTldr { error, .. } => Some(error),
            Self::EmptyName
            | Self::InvalidManualSection
            | Self::TldrManualSection { .. }
            | Self::InvalidSource
            | Self::ConflictingSourceSelectors
            | Self::EmptyMarkdownPath
            | Self::UnsupportedInputFormat { .. }
            | Self::EmptySelection
            | Self::TooManySelections { .. }
            | Self::EmptySelector
            | Self::InvalidContentSelector
            | Self::InvalidReferenceProjection(_)
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
            format!("must not exceed {maximum} Unicode scalar values")
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

struct LoadedManual {
    document: Document,
    address: DocumentAddress,
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
