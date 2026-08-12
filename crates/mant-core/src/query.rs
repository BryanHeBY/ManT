//! Composes local manuals and cached tldr content into one versioned query.

use std::{
    error::Error,
    ffi::OsStr,
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::OnceLock,
};

use mant_ast::{
    MantDocument, QueryBundle, QueryExcerpt, QueryInput, QueryOutline, QueryRequest, QuerySchema,
    QuerySearch, QueryView, SearchQuery, TldrDocument,
};
use mant_sources::{RegisteredDocumentIndex, SourceConfigError};

use crate::{
    ManualIndex, ManualPage, ManualRequest, ProjectionError, SearchError,
    build_outline_with_detail, discover_manual_roots, executable::query_name_candidates,
    locate_manual_source_in, parse_manual_page, parse_markdown, read_cached_tldr_page,
    search_query, select_excerpt, select_explanation, validate_search_query,
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
    EmptyName,
    InvalidSection,
    InvalidSource,
    ConflictingSourceSelectors,
    EmptyMarkdownPath,
    EmptySelection,
    EmptySelector,
    EmptyEntry,
    InvalidSearch(SearchError),
    Markdown { path: String, detail: String },
    EmptyMarkdown { label: String },
    Registry { detail: String },
    Manual(ManualLoadError),
    NoReadableContent { name: String },
}

/// Native-manual resolution or lowering failed after candidate selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualLoadError {
    NotFound {
        name: String,
        detail: String,
    },
    Parse {
        name: String,
        detail: String,
    },
    Empty {
        name: String,
        path: PathBuf,
        diagnostics: Vec<String>,
    },
}

/// Materialized result of the view carried by a [`QueryRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryViewResult {
    Full(QueryBundle),
    Outline(QueryOutline),
    Excerpt(QueryExcerpt),
    Search(QuerySearch),
}

/// A valid request could not be loaded or projected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExecutionError {
    Query(QueryError),
    Projection(ProjectionError),
    Search(SearchError),
}

/// Input-resolution policy kept outside the serialized request contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryPolicy {
    /// Bypass registered Markdown and require a readable native manual.
    pub manual_only: bool,
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("name must not be empty"),
            Self::InvalidSection => formatter.write_str("manual section must not be empty"),
            Self::InvalidSource => formatter.write_str("document source must not be empty"),
            Self::ConflictingSourceSelectors => formatter.write_str(
                "document source cannot be combined with a manual section or manual-only policy",
            ),
            Self::EmptyMarkdownPath => formatter.write_str("Markdown path must not be empty"),
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
            Self::Manual(error) => Some(error),
            Self::EmptyName
            | Self::InvalidSection
            | Self::InvalidSource
            | Self::ConflictingSourceSelectors
            | Self::EmptyMarkdownPath
            | Self::EmptySelection
            | Self::EmptySelector
            | Self::EmptyEntry
            | Self::Markdown { .. }
            | Self::EmptyMarkdown { .. }
            | Self::Registry { .. }
            | Self::NoReadableContent { .. } => None,
        }
    }
}

impl fmt::Display for ManualLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { name, detail } | Self::Parse { name, detail } => {
                write!(
                    formatter,
                    "could not load manual '{name}': manual source: {detail}"
                )
            }
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
pub fn resolve_query(request: &QueryRequest) -> Result<QueryBundle, QueryError> {
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
) -> Result<QueryBundle, QueryError> {
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
    query: QueryBundle,
    view: &QueryView,
) -> Result<QueryViewResult, QueryExecutionError> {
    match view {
        QueryView::Full {} => Ok(QueryViewResult::Full(query)),
        QueryView::Outline { detail } => build_outline_with_detail(&query, *detail)
            .map(QueryViewResult::Outline)
            .map_err(QueryExecutionError::Projection),
        QueryView::Excerpt { nodes } => select_excerpt(&query, nodes)
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
            name,
            source,
            section,
        } => {
            if name.trim().is_empty() {
                return Err(QueryError::EmptyName);
            }
            if source
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(QueryError::InvalidSource);
            }
            if section
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(QueryError::InvalidSection);
            }
            if source.is_some() && (section.is_some() || policy.manual_only) {
                return Err(QueryError::ConflictingSourceSelectors);
            }
        }
        QueryInput::MarkdownFile { path } => {
            if path.trim().is_empty() {
                return Err(QueryError::EmptyMarkdownPath);
            }
            if policy.manual_only {
                return Err(QueryError::Markdown {
                    path: path.trim().to_owned(),
                    detail: "the manual-only policy does not apply to Markdown input".to_owned(),
                });
            }
        }
    }
    match &request.view {
        QueryView::Excerpt { nodes } => {
            if nodes.is_empty() {
                return Err(QueryError::EmptySelection);
            }
            if nodes.iter().any(|node| node.trim().is_empty()) {
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
    ) -> Result<Option<PathBuf>, String>;
    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String>;
    fn parse_manual(&self, page: &ManualPage) -> Result<MantDocument, String>;
    fn read_tldr(&self, name: &str) -> Result<Option<TldrDocument>, String>;
    fn read_markdown(&self, path: &Path) -> Result<String, String>;
}

/// One explicit local document-environment snapshot.
pub struct DocumentResolver {
    registered: OnceLock<Result<RegisteredDocumentIndex, SourceConfigError>>,
    manual_roots: Vec<PathBuf>,
    manuals: OnceLock<ManualIndex>,
}

impl DocumentResolver {
    /// Capture the native manual index and lazily snapshot Markdown registration.
    #[must_use]
    pub fn from_system() -> Self {
        Self {
            registered: OnceLock::new(),
            manual_roots: discover_manual_roots(),
            manuals: OnceLock::new(),
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
    ) -> Result<QueryBundle, QueryError> {
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
}

impl QueryHost for DocumentResolver {
    fn name_candidates(&self, name: &str) -> Vec<String> {
        query_name_candidates(name)
    }

    fn locate_registered_document(
        &self,
        candidates: &[String],
        source: Option<&str>,
    ) -> Result<Option<PathBuf>, String> {
        let index = self
            .registered
            .get_or_init(RegisteredDocumentIndex::load)
            .as_ref()
            .map_err(ToString::to_string)?;
        index
            .find(candidates, source)
            .map(|registered| registered.map(|registered| registered.path.clone()))
            .map_err(|error| error.to_string())
    }

    fn locate_manual(&self, request: &ManualRequest) -> Result<ManualPage, String> {
        let manuals = self
            .manuals
            .get_or_init(|| ManualIndex::from_roots(self.manual_roots.clone()));
        locate_manual_source_in(request, manuals).map_err(|error| error.to_string())
    }

    fn parse_manual(&self, page: &ManualPage) -> Result<MantDocument, String> {
        parse_manual_page(page).map_err(|error| error.to_string())
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
) -> Result<QueryBundle, QueryError> {
    match &request.input {
        QueryInput::Document {
            name,
            source,
            section,
        } => query_named_document(name, source.as_deref(), section.as_deref(), policy, host),
        QueryInput::MarkdownFile { path } => query_markdown_file(path, policy, host),
    }
}

fn query_markdown_file(
    requested_path: &str,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<QueryBundle, QueryError> {
    let path = requested_path.trim();
    if path.is_empty() {
        return Err(QueryError::EmptyMarkdownPath);
    }
    if policy.manual_only {
        return Err(QueryError::Markdown {
            path: path.to_owned(),
            detail: "the manual-only policy does not apply to Markdown input".to_owned(),
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
) -> Result<QueryBundle, QueryError> {
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
    Ok(QueryBundle {
        schema: QuerySchema::V6,
        label,
        document: (!document_is_empty).then_some(parsed.document),
        tldr: parsed.tldr,
    })
}

fn query_named_document(
    name: &str,
    requested_source: Option<&str>,
    requested_section: Option<&str>,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<QueryBundle, QueryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(QueryError::EmptyName);
    }
    let section = requested_section.map(str::trim);
    if section.is_some_and(str::is_empty) {
        return Err(QueryError::InvalidSection);
    }
    let section = section.map(ToOwned::to_owned);
    let source = requested_source.map(str::trim);
    if source.is_some_and(str::is_empty) {
        return Err(QueryError::InvalidSource);
    }
    if source.is_some() && (section.is_some() || policy.manual_only) {
        return Err(QueryError::ConflictingSourceSelectors);
    }
    let require_manual = policy.manual_only || section.is_some();
    let candidates = host.name_candidates(name);

    // An unqualified name first consults one snapshot of the platform-native
    // registration namespace. Section selectors and the explicit manual-only
    // policy bypass Markdown name discovery.
    if section.is_none() && !policy.manual_only {
        let registered = host
            .locate_registered_document(&candidates, source)
            .map_err(|detail| QueryError::Registry { detail })?;
        if let Some(path) = registered {
            return query_registered_document(name, &path, host);
        }
        if source.is_some() {
            return Err(QueryError::NoReadableContent {
                name: name.to_owned(),
            });
        }
    }

    // Explicit manual selection is exclusive: --manual and --section must not
    // appear to resolve to tldr just because the quick reference is rendered
    // before the requested page. Unqualified queries retain tldr as an
    // optional augmentation and never update it during a read.
    let tldr = if require_manual {
        None
    } else {
        host.read_tldr(name).ok().flatten()
    };
    let mut manual = load_manual(name, &candidates, section.as_deref(), host);

    // A malformed page may omit its own section metadata. Preserve the
    // requested section so labels stay `name(N)`.
    if let (Ok(document), Some(section)) = (&mut manual, section.as_deref())
        && document.meta.section.is_none()
    {
        document.meta.section = Some(section.to_owned());
    }

    // An explicit manual request must not degrade into an apparently
    // successful tldr-only response.
    if require_manual {
        return match manual {
            Ok(manual) => Ok(QueryBundle {
                schema: QuerySchema::V6,
                label: name.to_owned(),
                document: Some(manual),
                tldr,
            }),
            Err(error) => Err(QueryError::Manual(error)),
        };
    }

    match manual {
        Ok(manual) => Ok(QueryBundle {
            schema: QuerySchema::V6,
            label: name.to_owned(),
            document: Some(manual),
            tldr,
        }),
        Err(_) if tldr.is_some() => Ok(QueryBundle {
            schema: QuerySchema::V6,
            label: name.to_owned(),
            document: None,
            tldr,
        }),
        Err(error) => Err(QueryError::Manual(error)),
    }
}

fn query_registered_document(
    name: &str,
    path: &Path,
    host: &dyn QueryHost,
) -> Result<QueryBundle, QueryError> {
    let source_path = path.to_string_lossy().into_owned();
    let source = host
        .read_markdown(path)
        .map_err(|detail| QueryError::Markdown {
            path: source_path.clone(),
            detail,
        })?;
    let mut query = query_markdown_text(&source, Some(source_path))?;
    name.clone_into(&mut query.label);
    Ok(query)
}

fn load_manual(
    requested_name: &str,
    candidates: &[String],
    section: Option<&str>,
    host: &dyn QueryHost,
) -> Result<MantDocument, ManualLoadError> {
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
    Ok(document)
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use mant_ast::{
        Diagnostic, DiagnosticLevel, DocumentMeta, DocumentSchema, DocumentSource, MantDocument,
        Producer, QueryInput, QueryRequest, QueryView, RequestSchema, Section, SourceFormat,
        TldrDocument, TldrOrigin,
    };

    use crate::{ManualPage, ManualRequest};

    use super::{
        MAX_MARKDOWN_BYTES, QueryError, QueryHost, QueryPolicy, query_markdown_text, query_with,
        read_capped_utf8, read_capped_utf8_io,
    };

    #[derive(Clone)]
    struct StubHost {
        name_candidates: Option<Vec<String>>,
        registered_document: Option<PathBuf>,
        registered_name: Option<String>,
        locate: Result<ManualPage, String>,
        manual_name: Option<String>,
        direct: Result<MantDocument, String>,
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
        ) -> Result<Option<PathBuf>, String> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(if source.is_some() { "source" } else { "name" });
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
            Ok(self.registered_document.clone())
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

        fn parse_manual(&self, _page: &ManualPage) -> Result<MantDocument, String> {
            self.calls.lock().expect("calls lock").push("parse");
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

    fn document(format: SourceFormat, unsupported: bool, readable: bool) -> MantDocument {
        MantDocument {
            schema: DocumentSchema::V6,
            producer: Producer {
                name: "test".to_owned(),
                version: "1".to_owned(),
                engine: None,
            },
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
                    id: "name-1".to_owned(),
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

    fn host(direct: Result<MantDocument, String>) -> StubHost {
        StubHost {
            name_candidates: None,
            registered_document: None,
            registered_name: None,
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
            schema: RequestSchema::V6,
            input: QueryInput::Document {
                name: " tool ".to_owned(),
                source: None,
                section: None,
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
            ["name", "tldr", "locate", "parse"]
        );
    }

    #[test]
    fn requested_section_backfills_metadata_the_parser_left_empty() {
        let mut host = host(Ok(document(SourceFormat::Man, false, true)));
        host.tldr = Ok(Some(tldr()));
        let request = QueryRequest {
            schema: RequestSchema::V6,
            input: QueryInput::Document {
                name: "tool".to_owned(),
                source: None,
                section: Some("3".to_owned()),
            },
            view: QueryView::Full {},
        };

        let result = query_with(&request, QueryPolicy::default(), &host).expect("query");
        assert_eq!(
            result
                .document
                .as_ref()
                .expect("manual")
                .meta
                .section
                .as_deref(),
            Some("3"),
            "requested section must label output when the parser omits it"
        );
        assert!(result.tldr.is_none(), "an explicit section is manual-only");
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["locate", "parse"],
            "an explicit manual section bypasses Markdown and tldr"
        );
    }

    #[test]
    fn explicit_source_reads_only_registered_markdown() {
        let mut host = host(Err("manual must not be read".to_owned()));
        host.registered_document = Some(PathBuf::from("/documents/tool.md"));
        host.markdown = Ok("# Tool\n\nSource body.\n".to_owned());
        let request = QueryRequest {
            schema: RequestSchema::V6,
            input: QueryInput::Document {
                name: "tool".to_owned(),
                source: Some("team".to_owned()),
                section: None,
            },
            view: QueryView::Full {},
        };
        let result = query_with(&request, QueryPolicy::default(), &host).expect("source query");
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
    fn complete_direct_document_survives_an_unsupported_finding() {
        let host = host(Ok(document(SourceFormat::Man, true, true)));
        let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["name", "tldr", "locate", "parse"]
        );
    }

    #[test]
    fn manual_only_bypasses_registered_markdown() {
        let mut host = host(Ok(document(SourceFormat::Man, true, true)));
        host.registered_document = Some(PathBuf::from("/data/mant/tool.md"));
        host.markdown = Ok("# Registered".to_owned());
        host.tldr = Ok(Some(tldr()));
        let result = query_with(&request(), QueryPolicy { manual_only: true }, &host)
            .expect("manual-only query");

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

        let error = query_with(&request(), QueryPolicy { manual_only: true }, &host)
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
    fn requested_section_failure_is_not_hidden_by_tldr() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("section not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let request = QueryRequest {
            schema: RequestSchema::V6,
            input: QueryInput::Document {
                name: "tool".to_owned(),
                source: None,
                section: Some("7".to_owned()),
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
    fn cached_tldr_survives_total_manual_failure() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let result =
            query_with(&request(), QueryPolicy::default(), &host).expect("tldr-only query");

        assert!(result.document.is_none());
        assert_eq!(result.tldr.expect("tldr").title, "tool");
    }

    #[test]
    fn reports_both_manual_paths_when_no_content_exists() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        let error = query_with(&request(), QueryPolicy::default(), &host)
            .expect_err("empty query must fail");
        assert_eq!(
            error.to_string(),
            "could not load manual 'tool': manual source: source not found"
        );
    }

    #[test]
    fn validates_before_touching_host_state() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        assert_eq!(
            query_with(
                &QueryRequest {
                    schema: RequestSchema::V6,
                    input: QueryInput::Document {
                        name: " ".to_owned(),
                        source: None,
                        section: None,
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
            ["name", "tldr", "locate", "locate", "parse"]
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
                schema: RequestSchema::V6,
                input: QueryInput::MarkdownFile {
                    path: "docs/tool.md".to_owned(),
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
                .any(|block| matches!(block, mant_ast::Block::Paragraph { .. }))
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
