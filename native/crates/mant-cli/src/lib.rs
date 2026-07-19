//! Public process boundary for Mant's native document CLI.
//!
//! `mant-cli` is both an agent-friendly command and the versioned stdio
//! backend used by the interactive TypeScript application. Standard output is
//! reserved for the requested document; diagnostics go to standard error.

mod arguments;

use std::io::{self, Read, Write};

use mant_ast::{QueryBundle, QueryRequest, TldrCacheUpdate};
use mant_core::{ProjectionError, QueryError};
use serde::Serialize;

use arguments::{Command, HELP, QueryFormat, QuerySource, QueryView, UsageError};

// ── Stable process protocol ────────────────────────────────────────────────

/// Exact stdio protocol understood by the TypeScript client.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v1";

const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolDescription<'a> {
    protocol: &'a str,
    native_api_version: &'a str,
    query_schema: &'a str,
    document_schema: &'a str,
    outline_schema: &'a str,
    excerpt_schema: &'a str,
}

// ── Host boundary ─────────────────────────────────────────────────────────

trait CliHost {
    fn query(&self, request: &QueryRequest) -> Result<QueryBundle, Failure>;
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
}

struct SystemHost;

impl CliHost for SystemHost {
    fn query(&self, request: &QueryRequest) -> Result<QueryBundle, Failure> {
        mant_core::query(request).map_err(|error| match error {
            QueryError::EmptyTopic | QueryError::InvalidSection => Failure::usage(error),
            _ => Failure::operational(error),
        })
    }

    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
        mant_core::update_tldr_cache().map_err(Failure::operational)
    }
}

// ── Process execution ─────────────────────────────────────────────────────

/// Run one CLI invocation using explicit streams and return its exit status.
///
/// Keeping the process streams injectable makes malformed protocol requests
/// testable without consulting the host man database or tldr client.
pub fn run(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> u8 {
    run_with_host(arguments, input, output, diagnostics, &SystemHost)
}

fn run_with_host(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> u8 {
    let command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_failure(&error.into(), diagnostics),
    };

    let rendered = match execute(command, input, host) {
        Ok(rendered) => rendered,
        Err(error) => return report_failure(&error, diagnostics),
    };

    match write_output(output, &rendered) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
}

fn execute(command: Command, input: &mut dyn Read, host: &dyn CliHost) -> Result<String, Failure> {
    match command {
        Command::Help => Ok(HELP.to_owned()),
        Command::ProtocolVersion { pretty } => render_json(
            &ProtocolDescription {
                protocol: CLI_PROTOCOL_VERSION,
                native_api_version: mant_core::native_api_version(),
                query_schema: "mant.query/v1",
                document_schema: "mant.document/v1",
                outline_schema: "mant.outline/v1",
                excerpt_schema: "mant.excerpt/v1",
            },
            pretty,
        ),
        Command::UpdateTldr { pretty } => {
            let update = host.update_tldr()?;
            mant_core::render_update_json(&update, pretty).map_err(Failure::operational)
        }
        Command::Query {
            source,
            view,
            format,
            pretty,
        } => {
            let request = read_query_request(source, input)?;
            validate_query_request(&request)?;
            let query = host.query(&request)?;
            match view {
                QueryView::Full => render_full_query(&query, format, pretty),
                QueryView::Outline => {
                    let outline = mant_core::build_outline(&query).map_err(projection_failure)?;
                    match format {
                        QueryFormat::Markdown => Ok(mant_core::render_outline_markdown(&outline)),
                        QueryFormat::Text => Ok(mant_core::render_outline_text(&outline)),
                        QueryFormat::Json => mant_core::render_outline_json(&outline, pretty)
                            .map_err(Failure::operational),
                    }
                }
                QueryView::Excerpt(selectors) => {
                    let excerpt = mant_core::select_excerpt(&query, &selectors)
                        .map_err(projection_failure)?;
                    match format {
                        QueryFormat::Markdown => Ok(mant_core::render_excerpt_markdown(&excerpt)),
                        QueryFormat::Text => Ok(mant_core::render_excerpt_text(&excerpt)),
                        QueryFormat::Json => mant_core::render_excerpt_json(&excerpt, pretty)
                            .map_err(Failure::operational),
                    }
                }
            }
        }
    }
}

fn render_full_query(
    query: &QueryBundle,
    format: QueryFormat,
    pretty: bool,
) -> Result<String, Failure> {
    match format {
        QueryFormat::Markdown => Ok(mant_core::render_markdown(query)),
        QueryFormat::Text => Ok(mant_core::render_query_text(query)),
        QueryFormat::Json => {
            mant_core::render_query_json(query, pretty).map_err(Failure::operational)
        }
    }
}

fn projection_failure(error: ProjectionError) -> Failure {
    match error {
        ProjectionError::MissingManual { .. } => Failure::operational(error),
        ProjectionError::EmptySelection
        | ProjectionError::EmptySelector
        | ProjectionError::UnknownSelector { .. } => Failure::usage(error),
    }
}

fn read_query_request(source: QuerySource, input: &mut dyn Read) -> Result<QueryRequest, Failure> {
    if let QuerySource::Arguments(request) = source {
        return Ok(request);
    }

    let mut bytes = Vec::new();
    input
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::usage(format!("cannot read request JSON: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REQUEST_BYTES {
        return Err(Failure::usage(format!(
            "request JSON exceeds the {MAX_REQUEST_BYTES}-byte limit"
        )));
    }
    let request =
        std::str::from_utf8(&bytes).map_err(|_| Failure::usage("request JSON must be UTF-8"))?;
    serde_json::from_str(request)
        .map_err(|error| Failure::usage(format!("invalid query request JSON: {error}")))
}

fn validate_query_request(request: &QueryRequest) -> Result<(), Failure> {
    if request.topic.trim().is_empty() {
        return Err(Failure::usage("manual topic must not be empty"));
    }
    if request
        .section
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(Failure::usage("manual section must not be empty"));
    }
    Ok(())
}

fn render_json(value: &impl Serialize, pretty: bool) -> Result<String, Failure> {
    if pretty {
        serde_json::to_string_pretty(value).map_err(Failure::operational)
    } else {
        serde_json::to_string(value).map_err(Failure::operational)
    }
}

fn write_output(output: &mut dyn Write, rendered: &str) -> io::Result<()> {
    output.write_all(rendered.as_bytes())?;
    if !rendered.ends_with('\n') {
        output.write_all(b"\n")?;
    }
    output.flush()
}

// ── Concise error presentation ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Usage,
    Operational,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Failure {
    kind: FailureKind,
    message: String,
}

impl Failure {
    fn usage(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Usage,
            message: message.to_string(),
        }
    }

    fn operational(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Operational,
            message: message.to_string(),
        }
    }
}

impl From<UsageError> for Failure {
    fn from(error: UsageError) -> Self {
        Self::usage(error.0)
    }
}

fn report_failure(error: &Failure, diagnostics: &mut dyn Write) -> u8 {
    let _ = writeln!(diagnostics, "mant-cli: {}", error.message);
    if error.kind == FailureKind::Usage {
        let _ = writeln!(diagnostics, "Try 'mant-cli --help' for more information.");
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use mant_ast::{
        Block, DocumentMeta, DocumentSchema, DocumentSource, Inline, LayoutHint, MantDocument,
        Producer, QueryBundle, QueryRequest, QuerySchema, Section, SourceFormat, TldrCacheAction,
        TldrCacheUpdate,
    };

    use super::{CLI_PROTOCOL_VERSION, CliHost, Failure, run_with_host};

    struct FakeHost {
        query_calls: Cell<usize>,
        update_calls: Cell<usize>,
        manual: Option<MantDocument>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                query_calls: Cell::new(0),
                update_calls: Cell::new(0),
                manual: None,
            }
        }

        fn with_manual() -> Self {
            Self {
                manual: Some(manual()),
                ..Self::new()
            }
        }
    }

    impl CliHost for FakeHost {
        fn query(&self, request: &QueryRequest) -> Result<QueryBundle, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            Ok(QueryBundle {
                schema: QuerySchema::V1,
                topic: request.topic.trim().to_owned(),
                section: request.section.clone(),
                manual: self.manual.clone(),
                tldr: None,
            })
        }

        fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
            self.update_calls.set(self.update_calls.get() + 1);
            Ok(TldrCacheUpdate {
                action: TldrCacheAction::Updated,
                cache_dir: Some("/cache/tldr".to_owned()),
                client: None,
                output: None,
                revision: Some("abc123".to_owned()),
            })
        }
    }

    fn invoke(arguments: &[&str], input: &[u8], host: &FakeHost) -> (u8, String, String) {
        let arguments = arguments
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut input = input;
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();
        let status = run_with_host(&arguments, &mut input, &mut output, &mut diagnostics, host);
        (
            status,
            String::from_utf8(output).expect("UTF-8 output"),
            String::from_utf8(diagnostics).expect("UTF-8 diagnostics"),
        )
    }

    fn manual() -> MantDocument {
        MantDocument {
            schema: DocumentSchema::V1,
            producer: Producer {
                name: "test".to_owned(),
                version: "1".to_owned(),
                engine: None,
            },
            source: DocumentSource {
                format: SourceFormat::Man,
                path: Some("/man/demo.1".to_owned()),
                renderer: None,
            },
            meta: DocumentMeta {
                section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            diagnostics: Vec::new(),
            sections: vec![
                section("name-1", "NAME", "demo - a test", Vec::new()),
                section(
                    "options-2",
                    "OPTIONS",
                    "all options",
                    vec![section(
                        "common-3",
                        "Common options",
                        "common details",
                        Vec::new(),
                    )],
                ),
            ],
        }
    }

    fn section(id: &str, title: &str, text: &str, children: Vec<Section>) -> Section {
        Section {
            id: id.to_owned(),
            title: title.to_owned(),
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: text.to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
            children,
            source: None,
        }
    }

    #[test]
    fn stdin_protocol_emits_only_compact_query_json() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--json", "--compact"],
            br#"{"topic":"git","section":"1"}"#,
            &host,
        );

        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"schema\":\"mant.query/v1\",\"topic\":\"git\",\"section\":\"1\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 1);
    }

    #[test]
    fn malformed_or_extended_requests_fail_before_querying_the_host() {
        for input in [
            br"not-json".as_slice(),
            br#"{"topic":"git","renderer":"html"}"#.as_slice(),
            br#"{"topic":"   "}"#.as_slice(),
        ] {
            let host = FakeHost::new();
            let (status, output, diagnostics) =
                invoke(&["--request-json", "--json", "--compact"], input, &host);
            assert_eq!(status, 2);
            assert!(output.is_empty());
            assert!(diagnostics.starts_with("mant-cli: "));
            assert_eq!(host.query_calls.get(), 0);
        }
    }

    #[test]
    fn direct_queries_render_outlines_and_selected_nodes_in_requested_formats() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(&["demo", "--outline"], b"", &host);
        assert_eq!(status, 0);
        assert!(output.contains("├─ 1 [name-1] NAME"));
        assert!(output.contains("└─ 2 [options-2] OPTIONS"));
        assert!(output.contains("└─ 2.1 [common-3] Common options"));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--node", "2.1", "--json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(value["schema"], "mant.excerpt/v1");
        assert_eq!(value["selections"][0]["path"], "2.1");
        assert_eq!(value["selections"][0]["section"]["title"], "Common options");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn unknown_nodes_are_concise_usage_failures() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(&["demo", "--node", "9", "--text"], b"", &host);

        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("manual 'demo' has no outline node '9'"));
        assert!(diagnostics.contains("mant-cli demo --outline"));
    }

    #[test]
    fn update_and_protocol_results_are_stable_json_documents() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["update", "tldr", "--compact"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"action\":\"updated\",\"cacheDir\":\"/cache/tldr\",\"revision\":\"abc123\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.update_calls.get(), 1);

        let (status, output, diagnostics) = invoke(&["protocol-version", "--compact"], b"", &host);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("protocol JSON");
        assert_eq!(value["protocol"], CLI_PROTOCOL_VERSION);
        assert_eq!(value["nativeApiVersion"], "1");
        assert_eq!(value["outlineSchema"], "mant.outline/v1");
        assert_eq!(value["excerptSchema"], "mant.excerpt/v1");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn usage_errors_are_concise_and_never_trigger_side_effects() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--unknown"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert_eq!(
            diagnostics,
            "mant-cli: unknown option '--unknown'\nTry 'mant-cli --help' for more information.\n"
        );
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);
    }
}
