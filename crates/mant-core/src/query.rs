//! Composes local manuals and cached tldr content into one versioned query.

use std::{
    error::Error,
    ffi::OsStr,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
};

use mant_ast::{MantDocument, QueryBundle, QueryInput, QueryRequest, QuerySchema, TldrDocument};

use crate::{ManualRequest, find_registered_document, parse_markdown, read_cached_tldr_page};

#[cfg(unix)]
use crate::{locate_manual_source, parse_manual_source};

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
    EmptyMarkdownPath,
    Markdown { path: String, detail: String },
    EmptyMarkdown { label: String },
    Manual { name: String, detail: String },
    NoReadableContent { name: String },
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
            Self::EmptyMarkdownPath => formatter.write_str("Markdown path must not be empty"),
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
            Self::Manual { detail, .. } => formatter.write_str(detail),
            Self::NoReadableContent { name } => {
                write!(
                    formatter,
                    "no readable document content was found for '{name}'"
                )
            }
        }
    }
}

impl Error for QueryError {}

/// Query the local man database and optional offline tldr caches.
///
/// # Errors
///
/// Returns [`QueryError`] for invalid input or when neither source can produce
/// readable content.
pub fn query(request: &QueryRequest) -> Result<QueryBundle, QueryError> {
    query_with(request, QueryPolicy::default(), &SystemQueryHost)
}

/// Query with an explicit input-resolution policy.
///
/// # Errors
///
/// Returns [`QueryError`] under the same conditions as [`query`].
pub fn query_with_policy(
    request: &QueryRequest,
    policy: QueryPolicy,
) -> Result<QueryBundle, QueryError> {
    query_with(request, policy, &SystemQueryHost)
}

trait QueryHost {
    fn locate_registered_document(&self, name: &str) -> Option<PathBuf>;
    fn locate_manual(&self, request: &ManualRequest) -> Result<PathBuf, String>;
    fn parse_manual(&self, path: &Path) -> Result<MantDocument, String>;
    fn read_tldr(&self, name: &str) -> Result<Option<TldrDocument>, String>;
    fn read_markdown(&self, path: &Path) -> Result<String, String>;
}

struct SystemQueryHost;

impl QueryHost for SystemQueryHost {
    fn locate_registered_document(&self, name: &str) -> Option<PathBuf> {
        find_registered_document(name).map(|registered| registered.path)
    }

    fn locate_manual(&self, request: &ManualRequest) -> Result<PathBuf, String> {
        #[cfg(unix)]
        {
            locate_manual_source(request).map_err(|error| error.to_string())
        }
        #[cfg(not(unix))]
        {
            Err(format!(
                "native manual pages are unavailable on this platform; register a Markdown document named '{}'",
                request.name
            ))
        }
    }

    fn parse_manual(&self, path: &Path) -> Result<MantDocument, String> {
        #[cfg(unix)]
        {
            parse_manual_source(path).map_err(|error| error.to_string())
        }
        #[cfg(not(unix))]
        {
            Err(format!(
                "native manual parsing is unavailable on this platform: {}",
                path.display()
            ))
        }
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
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("Markdown document exceeds the {limit}-byte limit"));
    }
    String::from_utf8(bytes).map_err(|_| "Markdown document must be UTF-8".to_owned())
}

fn query_with(
    request: &QueryRequest,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<QueryBundle, QueryError> {
    match &request.input {
        QueryInput::Document { name, section } => {
            query_manual(name, section.as_deref(), policy, host)
        }
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
        schema: QuerySchema::V4,
        label,
        document: (!document_is_empty).then_some(parsed.document),
        tldr: parsed.tldr,
    })
}

fn query_manual(
    name: &str,
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
    let require_manual = policy.manual_only || section.is_some();

    // An unqualified name first consults the platform-native registration
    // namespace. Section selectors and the explicit manual-only policy bypass
    // Markdown name discovery.
    if section.is_none()
        && !policy.manual_only
        && let Some(path) = host.locate_registered_document(name)
    {
        return query_registered_document(name, &path, host);
    }

    let manual_request = ManualRequest::new(name, section.clone());

    // A malformed or unreadable community cache must never hide a valid man
    // page. It is an optional augmentation and is never updated during query.
    let tldr = host.read_tldr(name).ok().flatten();
    let mut manual = load_manual(&manual_request, host);

    // A malformed page may omit its own section metadata. Preserve the
    // requested section so labels stay `name(N)`.
    if let (Ok(Some(document)), Some(section)) = (&mut manual, section.as_deref())
        && document.meta.section.is_none()
    {
        document.meta.section = Some(section.to_owned());
    }

    // An explicit manual request may include tldr beside a successful manual,
    // but must not degrade into an apparently successful tldr-only response.
    if require_manual {
        return match manual {
            Ok(Some(manual)) => Ok(QueryBundle {
                schema: QuerySchema::V4,
                label: name.to_owned(),
                document: Some(manual),
                tldr,
            }),
            Ok(None) => Err(QueryError::NoReadableContent {
                name: name.to_owned(),
            }),
            Err(detail) => Err(QueryError::Manual {
                name: name.to_owned(),
                detail,
            }),
        };
    }

    match manual {
        Ok(Some(manual)) => Ok(QueryBundle {
            schema: QuerySchema::V4,
            label: name.to_owned(),
            document: Some(manual),
            tldr,
        }),
        Ok(None) | Err(_) if tldr.is_some() => Ok(QueryBundle {
            schema: QuerySchema::V4,
            label: name.to_owned(),
            document: None,
            tldr,
        }),
        Ok(None) => Err(QueryError::NoReadableContent {
            name: name.to_owned(),
        }),
        Err(detail) => Err(QueryError::Manual {
            name: name.to_owned(),
            detail,
        }),
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
    request: &ManualRequest,
    host: &dyn QueryHost,
) -> Result<Option<MantDocument>, String> {
    let located = host.locate_manual(request);
    let (source_path, direct) = match located {
        Ok(path) => {
            let direct = host.parse_manual(&path);
            (Some(path), direct)
        }
        Err(error) => (None, Err(error)),
    };

    let document = direct.map_err(|error| {
        format!(
            "could not load manual '{}': source/libmandoc: {error}",
            request.name
        )
    })?;
    if document.sections.is_empty() {
        let path = source_path.as_deref().map_or_else(
            || "<unknown source>".to_owned(),
            |path| path.display().to_string(),
        );
        let diagnostics = document
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let location = diagnostic.source.map_or_else(String::new, |source| {
                    format!(" at {}:{}", source.line, source.column)
                });
                format!("{:?}{location}: {}", diagnostic.level, diagnostic.message)
            })
            .collect::<Vec<_>>()
            .join("; ");
        let detail = if diagnostics.is_empty() {
            String::new()
        } else {
            format!("; diagnostics: {diagnostics}")
        };
        return Err(format!(
            "could not load manual '{}': libmandoc parsed {path} but produced no readable sections{detail}",
            request.name,
        ));
    }
    Ok(Some(document))
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

    use crate::ManualRequest;

    use super::{
        MAX_MARKDOWN_BYTES, QueryError, QueryHost, QueryPolicy, query_markdown_text, query_with,
        read_capped_utf8,
    };

    #[derive(Clone)]
    struct StubHost {
        registered_document: Option<PathBuf>,
        locate: Result<PathBuf, String>,
        direct: Result<MantDocument, String>,
        tldr: Result<Option<TldrDocument>, String>,
        markdown: Result<String, String>,
        calls: std::sync::Arc<Mutex<Vec<&'static str>>>,
    }

    impl QueryHost for StubHost {
        fn locate_registered_document(&self, _name: &str) -> Option<PathBuf> {
            self.calls.lock().expect("calls lock").push("name");
            self.registered_document.clone()
        }

        fn locate_manual(&self, _request: &ManualRequest) -> Result<PathBuf, String> {
            self.calls.lock().expect("calls lock").push("locate");
            self.locate.clone()
        }

        fn parse_manual(&self, _path: &Path) -> Result<MantDocument, String> {
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
            schema: DocumentSchema::V4,
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
            registered_document: None,
            locate: Ok(PathBuf::from("/man/tool.1")),
            direct,
            tldr: Ok(None),
            markdown: Err("Markdown unavailable".to_owned()),
            calls: std::sync::Arc::default(),
        }
    }

    fn request() -> QueryRequest {
        QueryRequest {
            schema: RequestSchema::V4,
            input: QueryInput::Document {
                name: " tool ".to_owned(),
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
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        let request = QueryRequest {
            schema: RequestSchema::V4,
            input: QueryInput::Document {
                name: "tool".to_owned(),
                section: Some("3".to_owned()),
            },
            view: QueryView::Full {},
        };

        let result = query_with(&request, QueryPolicy::default(), &host).expect("query");
        assert_eq!(
            result.document.expect("manual").meta.section.as_deref(),
            Some("3"),
            "requested section must label output when the parser omits it"
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"],
            "an explicit manual section bypasses registered Markdown"
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
        let result = query_with(&request(), QueryPolicy { manual_only: true }, &host)
            .expect("manual-only query");

        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"],
            "manual-only lookup must not inspect the registered-document namespace"
        );
    }

    #[test]
    fn manual_only_failure_is_not_hidden_by_tldr() {
        let mut host = host(Ok(document(SourceFormat::Man, true, false)));
        host.tldr = Ok(Some(tldr()));

        let error = query_with(&request(), QueryPolicy { manual_only: true }, &host)
            .expect_err("an optional tldr page must not hide native parser failure");

        let QueryError::Manual { detail, .. } = error else {
            panic!("expected the native parser diagnostic");
        };
        assert!(detail.contains("/man/tool.1"));
        assert!(detail.contains("Unsupported: unsupported request"));
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"]
        );
    }

    #[test]
    fn requested_section_failure_is_not_hidden_by_tldr() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("section not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let request = QueryRequest {
            schema: RequestSchema::V4,
            input: QueryInput::Document {
                name: "tool".to_owned(),
                section: Some("7".to_owned()),
            },
            view: QueryView::Full {},
        };

        let error = query_with(&request, QueryPolicy::default(), &host)
            .expect_err("an explicit section must require a native manual");

        assert!(matches!(&error, QueryError::Manual { .. }));
        assert!(error.to_string().contains("section not found"));
        assert_eq!(*host.calls.lock().expect("calls lock"), ["tldr", "locate"]);
    }

    #[test]
    fn truncated_unsupported_document_is_an_error_by_default() {
        let host = host(Ok(document(SourceFormat::Man, true, false)));

        let QueryError::Manual { detail, .. } =
            query_with(&request(), QueryPolicy::default(), &host)
                .expect_err("empty-section document must error by default")
        else {
            panic!("expected Manual error");
        };
        assert!(detail.contains("produced no readable sections"));
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
            "could not load manual 'tool': source/libmandoc: source not found"
        );
    }

    #[test]
    fn validates_before_touching_host_state() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        assert_eq!(
            query_with(
                &QueryRequest {
                    schema: RequestSchema::V4,
                    input: QueryInput::Document {
                        name: " ".to_owned(),
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
    fn markdown_files_bypass_manual_and_tldr_sources() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());
        let result = query_with(
            &QueryRequest {
                schema: RequestSchema::V4,
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
}
