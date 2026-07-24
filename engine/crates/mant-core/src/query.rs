//! Composes local manuals and cached tldr content into one versioned query.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt, fs,
    path::{Path, PathBuf},
};

use mant_ast::{MantDocument, QueryBundle, QueryInput, QueryRequest, QuerySchema, TldrDocument};

use crate::{
    CommandRunner, ManualRequest, SystemCommandRunner, locate_manual_source, parse_groff_html,
    parse_manual_source, parse_markdown, read_cached_tldr_page, source::push_section_filter,
};

/// A query cannot produce either authoritative manual content or a quick reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    EmptyTopic,
    InvalidSection,
    EmptyMarkdownPath,
    Markdown { path: String, detail: String },
    EmptyMarkdown { label: String },
    Manual { topic: String, detail: String },
    NoReadableContent { topic: String },
}

/// Host execution policy kept outside the serialized request contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueryPolicy {
    /// Request direct libmandoc output for parser diagnostics.
    ///
    /// Libmandoc is the default backend. This diagnostic policy additionally
    /// rejects a tldr-only response when direct parsing cannot provide a
    /// readable manual.
    pub force_libmandoc: bool,
    /// Use `man -Thtml` + groff HTML parser instead of libmandoc.
    /// This code path has not been comprehensively tested.
    pub force_groff: bool,
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTopic => formatter.write_str("manual topic must not be empty"),
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
            Self::NoReadableContent { topic } => {
                write!(
                    formatter,
                    "no readable manual content was found for '{topic}'"
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

/// Query with an explicit host policy such as native-parser-only diagnostics.
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
    fn locate_manual(&self, request: &ManualRequest) -> Result<PathBuf, String>;
    fn parse_manual(&self, path: &Path) -> Result<MantDocument, String>;
    fn render_groff(
        &self,
        request: &ManualRequest,
        source_path: Option<&Path>,
    ) -> Result<MantDocument, String>;
    fn read_tldr(&self, topic: &str) -> Result<Option<TldrDocument>, String>;
    fn read_markdown(&self, path: &Path) -> Result<String, String>;
}

struct SystemQueryHost;

impl QueryHost for SystemQueryHost {
    fn locate_manual(&self, request: &ManualRequest) -> Result<PathBuf, String> {
        locate_manual_source(request).map_err(|error| error.to_string())
    }

    fn parse_manual(&self, path: &Path) -> Result<MantDocument, String> {
        parse_manual_source(path).map_err(|error| error.to_string())
    }

    fn render_groff(
        &self,
        request: &ManualRequest,
        source_path: Option<&Path>,
    ) -> Result<MantDocument, String> {
        render_groff_document_with(request, source_path, &SystemCommandRunner)
    }

    fn read_tldr(&self, topic: &str) -> Result<Option<TldrDocument>, String> {
        read_cached_tldr_page(topic).map_err(|error| error.to_string())
    }

    fn read_markdown(&self, path: &Path) -> Result<String, String> {
        fs::read_to_string(path).map_err(|error| error.to_string())
    }
}

fn query_with(
    request: &QueryRequest,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<QueryBundle, QueryError> {
    match &request.input {
        QueryInput::Manual { topic, section } => {
            query_manual(topic, section.as_deref(), policy, host)
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
    if policy.force_libmandoc || policy.force_groff {
        return Err(QueryError::Markdown {
            path: path.to_owned(),
            detail: "manual renderer policies do not apply to Markdown input".to_owned(),
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
    let document = parse_markdown(source, source_path);
    if document.blocks.is_empty() && document.sections.is_empty() {
        return Err(QueryError::EmptyMarkdown {
            label: label.clone(),
        });
    }
    Ok(QueryBundle {
        schema: QuerySchema::V3,
        label,
        document: Some(document),
        tldr: None,
    })
}

fn query_manual(
    topic: &str,
    requested_section: Option<&str>,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<QueryBundle, QueryError> {
    let topic = topic.trim();
    if topic.is_empty() {
        return Err(QueryError::EmptyTopic);
    }
    let section = requested_section.map(str::trim);
    if section.is_some_and(str::is_empty) {
        return Err(QueryError::InvalidSection);
    }
    let section = section.map(ToOwned::to_owned);
    let manual_request = ManualRequest::new(topic, section.clone());

    // A malformed or unreadable community cache must never hide a valid man
    // page. It is an optional augmentation and is never updated during query.
    let tldr = host.read_tldr(topic).ok().flatten();
    let manual = load_manual(&manual_request, policy, host);

    // Force-libmandoc mode is an explicit parser diagnostic request.
    // A tldr page may augment a successful manual, but must not turn a
    // failed native parse into an apparently successful tldr-only response.
    if policy.force_libmandoc || policy.force_groff {
        return match manual {
            Ok(Some(manual)) => Ok(QueryBundle {
                schema: QuerySchema::V3,
                label: topic.to_owned(),
                document: Some(manual),
                tldr,
            }),
            Ok(None) => Err(QueryError::NoReadableContent {
                topic: topic.to_owned(),
            }),
            Err(detail) => Err(QueryError::Manual {
                topic: topic.to_owned(),
                detail,
            }),
        };
    }

    match manual {
        Ok(Some(manual)) => Ok(QueryBundle {
            schema: QuerySchema::V3,
            label: topic.to_owned(),
            document: Some(manual),
            tldr,
        }),
        Ok(None) | Err(_) if tldr.is_some() => Ok(QueryBundle {
            schema: QuerySchema::V3,
            label: topic.to_owned(),
            document: None,
            tldr,
        }),
        Ok(None) => Err(QueryError::NoReadableContent {
            topic: topic.to_owned(),
        }),
        Err(detail) => Err(QueryError::Manual {
            topic: topic.to_owned(),
            detail,
        }),
    }
}

fn load_manual(
    request: &ManualRequest,
    policy: QueryPolicy,
    host: &dyn QueryHost,
) -> Result<Option<MantDocument>, String> {
    // The groff compatibility path needs the located source only as document
    // provenance. Do not parse it with libmandoc first: this switch is used to
    // isolate renderer differences and must not pay for or depend on native
    // lowering.
    if policy.force_groff {
        let source_path = host.locate_manual(request).ok();
        return match host.render_groff(request, source_path.as_deref()) {
            Ok(fallback) if !fallback.sections.is_empty() => Ok(Some(fallback)),
            Ok(_) => Ok(None),
            Err(error) => Err(error),
        };
    }

    let located = host.locate_manual(request);
    let (source_path, direct) = match located {
        Ok(path) => {
            let direct = host.parse_manual(&path);
            (Some(path), direct)
        }
        Err(error) => (None, Err(error)),
    };

    // Default (and --force-libmandoc): libmandoc only.
    let document = direct.map_err(|error| {
        format!(
            "could not load manual '{}': source/libmandoc: {error}",
            request.topic
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
            request.topic,
        ));
    }
    Ok(Some(document))
}

fn render_groff_document_with(
    request: &ManualRequest,
    source_path: Option<&Path>,
    runner: &impl CommandRunner,
) -> Result<MantDocument, String> {
    let mut arguments = vec![OsString::from("-Thtml")];
    if let Some(section) = request.section.as_deref() {
        // Label the section with portable `-S`. A bare section operand collides
        // with the `--` terminator below on man-db (the terminator is parsed as
        // the page name), while lowercase `-s` is unavailable in BSD man.
        push_section_filter(&mut arguments, section);
    }
    // Terminate option parsing so a topic beginning with '-' stays a
    // positional operand rather than an option to man.
    arguments.push(OsString::from("--"));
    arguments.push(OsString::from(&request.topic));
    let output = runner
        .run(OsStr::new("man"), &arguments)
        .map_err(|error| format!("cannot run 'man -Thtml': {error}"))?;
    if output.exit_code != 0 {
        let detail = first_nonempty_line(&output.stderr)
            .unwrap_or_else(|| format!("man -Thtml failed with code {}", output.exit_code));
        return Err(detail);
    }
    let html = String::from_utf8_lossy(&output.stdout);
    if html.trim().is_empty() {
        return Err(format!("man produced no HTML for '{}'", request.topic));
    }
    Ok(parse_groff_html(
        &html,
        source_path.map(|path| path.to_string_lossy().into_owned()),
    ))
}

fn first_nonempty_line(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::{OsStr, OsString},
        io,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use mant_ast::{
        Diagnostic, DiagnosticLevel, DocumentMeta, DocumentSchema, DocumentSource, MantDocument,
        Producer, QueryInput, QueryRequest, QueryView, RequestSchema, Section, SourceFormat,
        TldrDocument,
    };

    use crate::{CommandOutput, CommandRunner, ManualRequest};

    use super::{
        QueryError, QueryHost, QueryPolicy, query_markdown_text, query_with,
        render_groff_document_with,
    };

    #[derive(Clone)]
    struct StubHost {
        locate: Result<PathBuf, String>,
        direct: Result<MantDocument, String>,
        fallback: Result<MantDocument, String>,
        tldr: Result<Option<TldrDocument>, String>,
        markdown: Result<String, String>,
        calls: std::sync::Arc<Mutex<Vec<&'static str>>>,
    }

    impl QueryHost for StubHost {
        fn locate_manual(&self, _request: &ManualRequest) -> Result<PathBuf, String> {
            self.calls.lock().expect("calls lock").push("locate");
            self.locate.clone()
        }

        fn parse_manual(&self, _path: &Path) -> Result<MantDocument, String> {
            self.calls.lock().expect("calls lock").push("parse");
            self.direct.clone()
        }

        fn render_groff(
            &self,
            _request: &ManualRequest,
            _source_path: Option<&Path>,
        ) -> Result<MantDocument, String> {
            self.calls.lock().expect("calls lock").push("groff");
            self.fallback.clone()
        }

        fn read_tldr(&self, _topic: &str) -> Result<Option<TldrDocument>, String> {
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
            schema: DocumentSchema::V3,
            producer: Producer {
                name: "test".to_owned(),
                version: "1".to_owned(),
                engine: None,
            },
            source: DocumentSource {
                format,
                path: None,
                renderer: None,
            },
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
                    role: None,
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
        }
    }

    fn host(direct: Result<MantDocument, String>) -> StubHost {
        StubHost {
            locate: Ok(PathBuf::from("/man/tool.1")),
            direct,
            fallback: Err("fallback unavailable".to_owned()),
            tldr: Ok(None),
            markdown: Err("Markdown unavailable".to_owned()),
            calls: std::sync::Arc::default(),
        }
    }

    fn request() -> QueryRequest {
        QueryRequest {
            schema: RequestSchema::V3,
            input: QueryInput::Manual {
                topic: " tool ".to_owned(),
                section: None,
            },
            view: QueryView::Full {},
        }
    }

    #[test]
    fn ordinary_direct_document_does_not_start_groff() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

        assert_eq!(result.label, "tool");
        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"]
        );
    }

    /// With the old groff-fallback architecture this test verified that an
    /// unsupported diagnostic did not trigger an unnecessary groff call.
    /// Now libmandoc is the sole default backend so groff is never called.
    #[test]
    fn complete_direct_document_survives_an_unsupported_finding() {
        let mut host = host(Ok(document(SourceFormat::Man, true, true)));
        host.fallback = Ok(document(SourceFormat::GroffHtml, false, true));
        let result = query_with(&request(), QueryPolicy::default(), &host).expect("query");

        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"]
        );
    }

    #[test]
    fn forced_libmandoc_never_starts_groff() {
        let mut host = host(Ok(document(SourceFormat::Man, true, true)));
        host.fallback = Ok(document(SourceFormat::GroffHtml, false, true));
        let result = query_with(
            &request(),
            QueryPolicy {
                force_libmandoc: true,
                force_groff: false,
            },
            &host,
        )
        .expect("forced native query");

        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"]
        );
    }

    #[test]
    fn forced_groff_never_starts_libmandoc() {
        let mut host = host(Err("libmandoc must not run".to_owned()));
        host.fallback = Ok(document(SourceFormat::GroffHtml, false, true));
        let result = query_with(
            &request(),
            QueryPolicy {
                force_libmandoc: false,
                force_groff: true,
            },
            &host,
        )
        .expect("forced groff query");

        assert_eq!(
            result.document.expect("manual").source.format,
            SourceFormat::GroffHtml
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "groff"]
        );
    }

    #[test]
    fn forced_libmandoc_failure_is_not_hidden_by_tldr() {
        let mut host = host(Ok(document(SourceFormat::Man, true, false)));
        host.tldr = Ok(Some(tldr()));
        host.fallback = Ok(document(SourceFormat::GroffHtml, false, true));

        let error = query_with(
            &request(),
            QueryPolicy {
                force_libmandoc: true,
                force_groff: false,
            },
            &host,
        )
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

    /// With the old groff-fallback architecture this test verified that a
    /// truncated native document fell back to groff. Now libmandoc is the
    /// default and an empty-sections document is an error.
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
    fn failed_groff_retains_readable_best_effort_document() {
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
                    schema: RequestSchema::V3,
                    input: QueryInput::Manual {
                        topic: " ".to_owned(),
                        section: None,
                    },
                    view: QueryView::Full {},
                },
                QueryPolicy::default(),
                &host
            ),
            Err(QueryError::EmptyTopic)
        );
        assert!(host.calls.lock().expect("calls lock").is_empty());
    }

    #[test]
    fn markdown_files_bypass_manual_and_tldr_sources() {
        let mut host = host(Err("manual parser must not run".to_owned()));
        host.markdown = Ok("# Tool\n\n## Options\n\n- `--help`: Show help.\n".to_owned());
        let result = query_with(
            &QueryRequest {
                schema: RequestSchema::V3,
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

    struct StubRunner {
        output: CommandOutput,
        calls: Mutex<Vec<(OsString, Vec<OsString>)>>,
    }

    impl CommandRunner for StubRunner {
        fn run(&self, program: &OsStr, arguments: &[OsString]) -> io::Result<CommandOutput> {
            self.calls
                .lock()
                .expect("runner calls lock")
                .push((program.to_owned(), arguments.to_vec()));
            Ok(self.output.clone())
        }
    }

    #[test]
    fn groff_renderer_passes_section_and_preserves_source_identity() {
        let runner = StubRunner {
            output: CommandOutput {
                stdout: b"<body><h2>NAME</h2><p>tool</p></body>".to_vec(),
                stderr: Vec::new(),
                exit_code: 0,
            },
            calls: Mutex::new(Vec::new()),
        };
        let document = render_groff_document_with(
            &ManualRequest::new("tool", Some("1".to_owned())),
            Some(Path::new("/man/tool.1.gz")),
            &runner,
        )
        .expect("groff document");

        assert_eq!(document.source.path.as_deref(), Some("/man/tool.1.gz"));
        assert_eq!(
            *runner.calls.lock().expect("runner calls lock"),
            [(
                OsString::from("man"),
                ["-Thtml", "-S", "1", "--", "tool"]
                    .map(OsString::from)
                    .to_vec()
            )]
        );
    }
}
