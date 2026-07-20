//! Composes local manuals and cached tldr content into one versioned query.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
};

use mant_ast::{
    DiagnosticLevel, MantDocument, QueryBundle, QueryRequest, QuerySchema, TldrDocument,
};

use crate::{
    CommandRunner, ManualRequest, SystemCommandRunner, locate_manual_source, parse_groff_html,
    parse_manual_source, read_cached_tldr_page,
};

/// A query cannot produce either authoritative manual content or a quick reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    EmptyTopic,
    InvalidSection,
    Manual { topic: String, detail: String },
    NoReadableContent { topic: String },
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTopic => formatter.write_str("manual topic must not be empty"),
            Self::InvalidSection => formatter.write_str("manual section must not be empty"),
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
    query_with(request, &SystemQueryHost)
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
}

fn query_with(request: &QueryRequest, host: &dyn QueryHost) -> Result<QueryBundle, QueryError> {
    let topic = request.topic.trim();
    if topic.is_empty() {
        return Err(QueryError::EmptyTopic);
    }
    let section = request.section.as_deref().map(str::trim);
    if section.is_some_and(str::is_empty) {
        return Err(QueryError::InvalidSection);
    }
    let section = section.map(ToOwned::to_owned);
    let manual_request = ManualRequest::new(topic, section.clone());

    // A malformed or unreadable community cache must never hide a valid man
    // page. It is an optional augmentation and is never updated during query.
    let tldr = host.read_tldr(topic).ok().flatten();
    let manual = load_manual(&manual_request, host);

    match manual {
        Ok(Some(manual)) => Ok(QueryBundle {
            schema: QuerySchema::V2,
            topic: topic.to_owned(),
            section,
            manual: Some(manual),
            tldr,
        }),
        Ok(None) | Err(_) if tldr.is_some() => Ok(QueryBundle {
            schema: QuerySchema::V2,
            topic: topic.to_owned(),
            section,
            manual: None,
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

    if let Ok(document) = &direct {
        let readable = !document.sections.is_empty();
        if readable && !has_unsupported_diagnostics(document) {
            return Ok(Some(document.clone()));
        }

        match host.render_groff(request, source_path.as_deref()) {
            Ok(fallback) if !fallback.sections.is_empty() => return Ok(Some(fallback)),
            Ok(_) | Err(_) if readable => return Ok(Some(document.clone())),
            Ok(_) => return Ok(None),
            Err(fallback_error) => {
                return Err(format!(
                    "could not load manual '{}': libmandoc produced no readable sections; man/groff: {fallback_error}",
                    request.topic
                ));
            }
        }
    }

    let direct_error = direct.expect_err("the successful branch returns above");
    match host.render_groff(request, source_path.as_deref()) {
        Ok(fallback) if !fallback.sections.is_empty() => Ok(Some(fallback)),
        Ok(_) => Ok(None),
        Err(fallback_error) => Err(format!(
            "could not load manual '{}': source/libmandoc: {direct_error}; man/groff: {fallback_error}",
            request.topic
        )),
    }
}

fn has_unsupported_diagnostics(document: &MantDocument) -> bool {
    document
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.level == DiagnosticLevel::Unsupported)
}

fn render_groff_document_with(
    request: &ManualRequest,
    source_path: Option<&Path>,
    runner: &impl CommandRunner,
) -> Result<MantDocument, String> {
    let mut arguments = vec![OsString::from("-Thtml")];
    if let Some(section) = request.section.as_deref() {
        arguments.push(OsString::from(section));
    }
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
        Producer, QueryRequest, QueryView, RequestSchema, Section, SourceFormat, TldrDocument,
    };

    use crate::{CommandOutput, CommandRunner, ManualRequest};

    use super::{QueryError, QueryHost, query_with, render_groff_document_with};

    #[derive(Clone)]
    struct StubHost {
        locate: Result<PathBuf, String>,
        direct: Result<MantDocument, String>,
        fallback: Result<MantDocument, String>,
        tldr: Result<Option<TldrDocument>, String>,
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
    }

    fn document(format: SourceFormat, unsupported: bool, readable: bool) -> MantDocument {
        MantDocument {
            schema: DocumentSchema::V2,
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
        }
    }

    fn host(direct: Result<MantDocument, String>) -> StubHost {
        StubHost {
            locate: Ok(PathBuf::from("/man/tool.1")),
            direct,
            fallback: Err("fallback unavailable".to_owned()),
            tldr: Ok(None),
            calls: std::sync::Arc::default(),
        }
    }

    fn request() -> QueryRequest {
        QueryRequest {
            schema: RequestSchema::V2,
            topic: " tool ".to_owned(),
            section: None,
            view: QueryView::Full {},
        }
    }

    #[test]
    fn ordinary_direct_document_does_not_start_groff() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        let result = query_with(&request(), &host).expect("query");

        assert_eq!(result.topic, "tool");
        assert_eq!(
            result.manual.expect("manual").source.format,
            SourceFormat::Man
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse"]
        );
    }

    #[test]
    fn unsupported_direct_document_prefers_readable_groff() {
        let mut host = host(Ok(document(SourceFormat::Man, true, true)));
        host.fallback = Ok(document(SourceFormat::GroffHtml, false, true));
        let result = query_with(&request(), &host).expect("query");

        assert_eq!(
            result.manual.expect("manual").source.format,
            SourceFormat::GroffHtml
        );
        assert_eq!(
            *host.calls.lock().expect("calls lock"),
            ["tldr", "locate", "parse", "groff"]
        );
    }

    #[test]
    fn failed_groff_retains_readable_best_effort_document() {
        let host = host(Ok(document(SourceFormat::Mdoc, true, true)));
        let result = query_with(&request(), &host).expect("query");
        assert_eq!(
            result.manual.expect("manual").source.format,
            SourceFormat::Mdoc
        );
    }

    #[test]
    fn cached_tldr_survives_total_manual_failure() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        host.tldr = Ok(Some(tldr()));
        let result = query_with(&request(), &host).expect("tldr-only query");

        assert!(result.manual.is_none());
        assert_eq!(result.tldr.expect("tldr").title, "tool");
    }

    #[test]
    fn reports_both_manual_paths_when_no_content_exists() {
        let mut host = host(Err("libmandoc failed".to_owned()));
        host.locate = Err("source not found".to_owned());
        let error = query_with(&request(), &host).expect_err("empty query must fail");
        assert_eq!(
            error.to_string(),
            "could not load manual 'tool': source/libmandoc: source not found; man/groff: fallback unavailable"
        );
    }

    #[test]
    fn validates_before_touching_host_state() {
        let host = host(Ok(document(SourceFormat::Man, false, true)));
        assert_eq!(
            query_with(
                &QueryRequest {
                    schema: RequestSchema::V2,
                    topic: " ".to_owned(),
                    section: None,
                    view: QueryView::Full {},
                },
                &host
            ),
            Err(QueryError::EmptyTopic)
        );
        assert!(host.calls.lock().expect("calls lock").is_empty());
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
                ["-Thtml", "1", "tool"].map(OsString::from).to_vec()
            )]
        );
    }
}
