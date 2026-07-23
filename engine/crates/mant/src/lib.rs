//! Public process boundary for `ManT`'s native document CLI.
//!
//! `mant` is both an agent-friendly command and the versioned stdio
//! backend used by the interactive TypeScript application. Standard output is
//! reserved for the requested document; diagnostics go to standard error.

mod arguments;
mod mcp;

use std::io::{self, Read, Write};

use mant_ast::{
    Diagnostic, ExcerptSelection, QueryBundle, QueryRequest, QueryView, SearchQuery, SourceFormat,
    TldrCacheUpdate,
};
use mant_core::{ProjectionError, QueryError, QueryPolicy, SearchError};
use serde::Serialize;

use arguments::{Command, QueryFormat, QuerySource, SchemaContract};

// ── Stable process protocol ────────────────────────────────────────────────

/// Exact stdio protocol understood by the TypeScript client.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v2";

const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolDescription<'a> {
    protocol: &'a str,
    native_api_version: &'a str,
    request_schema: &'a str,
    query_schema: &'a str,
    document_schema: &'a str,
    outline_schema: &'a str,
    excerpt_schema: &'a str,
    search_schema: &'a str,
}

/// Normalized fields of a conventional CLI document query.
#[allow(clippy::struct_excessive_bools)]
struct QueryExecution {
    source: QuerySource,
    format: QueryFormat,
    pretty: bool,
    force_libmandoc: bool,
    force_groff: bool,
    explain: bool,
    preserve_anchors: bool,
}

// ── Host boundary ─────────────────────────────────────────────────────────

trait CliHost {
    fn query(&self, request: &QueryRequest, policy: QueryPolicy) -> Result<QueryBundle, Failure>;
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
}

struct SystemHost;

impl CliHost for SystemHost {
    fn query(&self, request: &QueryRequest, policy: QueryPolicy) -> Result<QueryBundle, Failure> {
        mant_core::query_with_policy(request, policy).map_err(|error| match error {
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

/// Run one native-process invocation, including the long-lived MCP mode.
///
/// The conventional CLI keeps injectable streams through [`run`], while MCP
/// owns operating-system stdio because the protocol reserves it exclusively
/// for newline-delimited JSON-RPC messages.
pub async fn run_process(arguments: &[String]) -> u8 {
    let command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_argument_error(&error, &mut io::stderr().lock()),
    };

    if matches!(command, Command::Mcp) {
        return mcp::run_stdio().await;
    }

    run_command(
        command,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
        &SystemHost,
    )
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
        Err(error) => return report_argument_error(&error, diagnostics),
    };

    run_command(command, input, output, diagnostics, host)
}

fn run_command(
    command: Command,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> u8 {
    if matches!(command, Command::Mcp) {
        return report_failure(
            &Failure::usage("MCP mode must be launched through the native process entry point"),
            diagnostics,
        );
    }

    let rendered = match execute(command, input, diagnostics, host) {
        Ok(rendered) => rendered,
        Err(error) => return report_failure(&error, diagnostics),
    };

    match write_output(output, &rendered) {
        Ok(()) => 0,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
}

fn execute(
    command: Command,
    input: &mut dyn Read,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    match command {
        Command::Help(help) => Ok(help),
        Command::ProtocolVersion { pretty } => render_json(
            &ProtocolDescription {
                protocol: CLI_PROTOCOL_VERSION,
                native_api_version: mant_core::native_api_version(),
                request_schema: "mant.request/v2",
                query_schema: "mant.query/v2",
                document_schema: "mant.document/v2",
                outline_schema: "mant.outline/v2",
                excerpt_schema: "mant.excerpt/v2",
                search_schema: "mant.search/v1",
            },
            pretty,
        ),
        Command::Schema { contract, pretty } => match contract {
            SchemaContract::Request => render_json(&mant_ast::query_request_json_schema(), pretty),
            SchemaContract::Query => render_json(&mant_ast::query_bundle_json_schema(), pretty),
            SchemaContract::Outline => render_json(&mant_ast::query_outline_json_schema(), pretty),
            SchemaContract::Excerpt => render_json(&mant_ast::query_excerpt_json_schema(), pretty),
            SchemaContract::Search => render_json(&mant_ast::query_search_json_schema(), pretty),
            SchemaContract::All => render_json(&mant_ast::query_json_schema_catalog(), pretty),
        },
        Command::Mcp => unreachable!("MCP mode is dispatched before normal CLI execution"),
        Command::UpdateTldr { pretty } => {
            let update = host.update_tldr()?;
            mant_core::render_update_json(&update, pretty).map_err(Failure::operational)
        }
        Command::Query {
            source,
            format,
            pretty,
            force_libmandoc,
            force_groff,
            explain,
            preserve_anchors,
        } => execute_query(
            QueryExecution {
                source,
                format,
                pretty,
                force_libmandoc,
                force_groff,
                explain,
                preserve_anchors,
            },
            input,
            diagnostics,
            host,
        ),
    }
}

/// Load one manual query and render the projection encoded in its request.
fn execute_query(
    command: QueryExecution,
    input: &mut dyn Read,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    let request = read_query_request(command.source, input)?;
    validate_query_request(&request)?;
    let view = request.view.clone();
    let query = host.query(
        &request,
        QueryPolicy {
            force_libmandoc: command.force_libmandoc,
            force_groff: command.force_groff,
        },
    )?;
    if command.force_libmandoc || command.force_groff {
        report_manual_diagnostics(&query, diagnostics)?;
    }
    render_query_view(
        &query,
        view,
        command.format,
        command.pretty,
        command.explain,
        command.preserve_anchors,
    )
}

/// Render one already-loaded projection without re-reading local source data.
fn render_query_view(
    query: &QueryBundle,
    view: QueryView,
    format: QueryFormat,
    pretty: bool,
    explain: bool,
    preserve_anchors: bool,
) -> Result<String, Failure> {
    match view {
        QueryView::Full { .. } => render_full_query(query, format, pretty, preserve_anchors),
        QueryView::Outline { detail } => {
            let outline =
                mant_core::build_outline_with_detail(query, detail).map_err(projection_failure)?;
            match format {
                QueryFormat::Markdown => Ok(mant_core::render_outline_markdown(&outline)),
                QueryFormat::Text | QueryFormat::Man => {
                    Ok(mant_core::render_outline_text(&outline))
                }
                QueryFormat::Json => {
                    mant_core::render_outline_json(&outline, pretty).map_err(Failure::operational)
                }
            }
        }
        QueryView::Excerpt { nodes } => {
            let excerpt = mant_core::select_excerpt(query, &nodes).map_err(projection_failure)?;
            if explain {
                validate_explanation(&excerpt)?;
            }
            match format {
                QueryFormat::Markdown => Ok(mant_core::render_excerpt_markdown_with_options(
                    &excerpt,
                    mant_core::MarkdownOptions { preserve_anchors },
                )),
                QueryFormat::Text | QueryFormat::Man => {
                    Ok(mant_core::render_excerpt_text(&excerpt))
                }
                QueryFormat::Json => {
                    mant_core::render_excerpt_json(&excerpt, pretty).map_err(Failure::operational)
                }
            }
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
        } => {
            let search = mant_core::search_query(
                query,
                &SearchQuery {
                    pattern,
                    syntax,
                    case,
                    scope,
                    word,
                    context_lines,
                    limit,
                    offset,
                },
            )
            .map_err(search_failure)?;
            match format {
                QueryFormat::Markdown => Ok(mant_core::render_search_markdown(&search)),
                QueryFormat::Text | QueryFormat::Man => Ok(mant_core::render_search_text(&search)),
                QueryFormat::Json => {
                    mant_core::render_search_json(&search, pretty).map_err(Failure::operational)
                }
            }
        }
    }
}

/// Keep `--explain` focused on one semantic definition while reusing the
/// versioned excerpt response used by `--node` and stdin requests.
fn validate_explanation(excerpt: &mant_ast::QueryExcerpt) -> Result<(), Failure> {
    if matches!(
        excerpt.selections.as_slice(),
        [ExcerptSelection::ManualEntry { .. }]
    ) {
        return Ok(());
    }
    Err(Failure::usage(
        "--explain requires one option, command, or environment variable; use --node for sections",
    ))
}

fn report_manual_diagnostics(query: &QueryBundle, output: &mut dyn Write) -> Result<(), Failure> {
    let Some(manual) = &query.manual else {
        return Ok(());
    };
    let engine = match manual.source.format {
        SourceFormat::GroffHtml => "groff HTML",
        SourceFormat::Man | SourceFormat::Mdoc | SourceFormat::MandocHtml => "libmandoc",
    };
    for diagnostic in &manual.diagnostics {
        writeln!(output, "mant: {engine} {}", format_diagnostic(diagnostic))
            .map_err(Failure::operational)?;
    }
    Ok(())
}

fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    let location = diagnostic.source.map_or_else(String::new, |source| {
        format!(" at {}:{}", source.line, source.column)
    });
    format!("{:?}{location}: {}", diagnostic.level, diagnostic.message)
}

fn render_full_query(
    query: &QueryBundle,
    format: QueryFormat,
    pretty: bool,
    preserve_anchors: bool,
) -> Result<String, Failure> {
    match format {
        QueryFormat::Markdown => Ok(mant_core::render_markdown_with_options(
            query,
            mant_core::MarkdownOptions { preserve_anchors },
        )),
        QueryFormat::Text => Ok(mant_core::render_query_text(query)),
        QueryFormat::Man => Ok(mant_core::render_query_man(query)),
        QueryFormat::Json => {
            mant_core::render_query_json(query, pretty).map_err(Failure::operational)
        }
    }
}

fn projection_failure(error: ProjectionError) -> Failure {
    match error {
        ProjectionError::MissingContent { .. } => Failure::operational(error),
        ProjectionError::EmptySelection
        | ProjectionError::EmptySelector
        | ProjectionError::UnknownSelector { .. } => Failure::usage(error),
    }
}

fn search_failure(error: SearchError) -> Failure {
    Failure::usage(error)
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
    if let QueryView::Excerpt { nodes } = &request.view {
        if nodes.is_empty() {
            return Err(Failure::usage("at least one outline node is required"));
        }
        if nodes.iter().any(|node| node.trim().is_empty()) {
            return Err(Failure::usage("outline node must not be empty"));
        }
    }
    if let QueryView::Search {
        pattern,
        syntax,
        case,
        scope,
        word,
        context_lines,
        limit,
        offset,
    } = &request.view
    {
        mant_core::validate_search_query(&SearchQuery {
            pattern: pattern.clone(),
            syntax: *syntax,
            case: *case,
            scope: *scope,
            word: *word,
            context_lines: *context_lines,
            limit: *limit,
            offset: *offset,
        })
        .map_err(search_failure)?;
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

fn report_failure(error: &Failure, diagnostics: &mut dyn Write) -> u8 {
    let _ = writeln!(diagnostics, "mant: {}", error.message);
    if error.kind == FailureKind::Usage {
        let _ = writeln!(diagnostics, "Try 'mant --help' for more information.");
        2
    } else {
        1
    }
}

/** Preserve clap's actionable usage and suggestion text on the injected stream. */
fn report_argument_error(error: &clap::Error, diagnostics: &mut dyn Write) -> u8 {
    let rendered = error.to_string();
    let _ = diagnostics.write_all(rendered.as_bytes());
    if !rendered.ends_with('\n') {
        let _ = diagnostics.write_all(b"\n");
    }
    2
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use mant_ast::{
        Block, DefinitionIdentity, DefinitionItem, DefinitionRole, Diagnostic, DiagnosticLevel,
        DocumentMeta, DocumentSchema, DocumentSource, Inline, LayoutHint, MantDocument, Producer,
        QueryBundle, QueryRequest, QuerySchema, Section, SourceFormat, SourceSpan, TldrCacheAction,
        TldrCacheUpdate, TldrDocument,
    };

    use super::{CLI_PROTOCOL_VERSION, CliHost, Failure, QueryPolicy, run_with_host};

    struct FakeHost {
        query_calls: Cell<usize>,
        update_calls: Cell<usize>,
        manual: Option<MantDocument>,
        tldr: Option<TldrDocument>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                query_calls: Cell::new(0),
                update_calls: Cell::new(0),
                manual: None,
                tldr: None,
            }
        }

        fn with_manual() -> Self {
            Self {
                manual: Some(manual()),
                ..Self::new()
            }
        }

        fn with_manual_and_tldr() -> Self {
            Self {
                manual: Some(manual()),
                tldr: Some(tldr()),
                ..Self::new()
            }
        }

        fn with_explainable_manual() -> Self {
            Self {
                manual: Some(explainable_manual()),
                ..Self::new()
            }
        }
    }

    impl CliHost for FakeHost {
        fn query(
            &self,
            request: &QueryRequest,
            _policy: QueryPolicy,
        ) -> Result<QueryBundle, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            Ok(QueryBundle {
                schema: QuerySchema::V2,
                topic: request.topic.trim().to_owned(),
                section: request.section.clone(),
                manual: self.manual.clone(),
                tldr: self.tldr.clone(),
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
            schema: DocumentSchema::V2,
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

    fn explainable_manual() -> MantDocument {
        let mut manual = manual();
        let options = manual
            .sections
            .iter_mut()
            .find(|section| section.id == "options-2")
            .expect("options section");
        options.blocks.push(Block::DefinitionList {
            items: vec![DefinitionItem {
                inline_term: false,
                identity: Some(DefinitionIdentity {
                    id: "exclude".to_owned(),
                    role: DefinitionRole::Option,
                    names: vec!["--exclude".to_owned()],
                }),
                terms: vec![vec![Inline::Text {
                    value: "--exclude=PATTERN".to_owned(),
                }]],
                description: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "Exclude matching files from the archive.".to_owned(),
                    }],
                    layout: LayoutHint::default(),
                    source: None,
                }],
                spacing_before_lines: None,
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        });
        manual
    }

    fn tldr() -> TldrDocument {
        TldrDocument {
            title: "demo".to_owned(),
            description: vec!["A small demonstration.".to_owned()],
            more_information: None,
            examples: Vec::new(),
            platform: "common".to_owned(),
            language: "en".to_owned(),
            source_path: "/cache/tldr/pages/common/demo.md".to_owned(),
        }
    }

    fn section(id: &str, title: &str, text: &str, children: Vec<Section>) -> Section {
        Section {
            id: id.to_owned(),
            title: title.to_owned(),
            spacing_before_lines: 0,
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
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v2","topic":"git","section":"1","view":{"kind":"full"}}"#,
            &host,
        );

        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"schema\":\"mant.query/v2\",\"topic\":\"git\",\"section\":\"1\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 1);
    }

    #[test]
    fn malformed_or_extended_requests_fail_before_querying_the_host() {
        for input in [
            br"not-json".as_slice(),
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"full"},"renderer":"html"}"#.as_slice(),
            br#"{"schema":"mant.request/v2","topic":"   ","view":{"kind":"full"}}"#.as_slice(),
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"excerpt","nodes":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"search","pattern":"","limit":10}}"#.as_slice(),
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"search","pattern":"git","limit":0}}"#.as_slice(),
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"search","pattern":"git","contextLines":101}}"#.as_slice(),
            br#"{"schema":"mant.request/v2","topic":"git","view":{"kind":"search","pattern":"[","syntax":"regex"}}"#.as_slice(),
        ] {
            let host = FakeHost::new();
            let (status, output, diagnostics) = invoke(
                &["--request-json", "--format", "json", "--compact"],
                input,
                &host,
            );
            assert_eq!(status, 2);
            assert!(output.is_empty());
            assert!(diagnostics.starts_with("mant: "));
            assert_eq!(host.query_calls.get(), 0);
        }
    }

    #[test]
    fn stdin_requests_select_outline_and_excerpt_projections() {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v2","topic":"demo","view":{"kind":"outline","detail":"sections"}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
        assert_eq!(outline["schema"], "mant.outline/v2");
        assert_eq!(outline["detail"], "sections");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v2","topic":"demo","view":{"kind":"excerpt","nodes":["2.1"]}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(excerpt["schema"], "mant.excerpt/v2");
        assert_eq!(excerpt["selections"][0]["path"], "2.1");
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 2);
    }

    #[test]
    fn direct_queries_render_outlines_and_selected_nodes_in_requested_formats() {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(&["demo", "--outline"], b"", &host);
        assert_eq!(status, 0);
        assert!(output.contains("├─ 0 [tldr] TLDR QUICK REFERENCE"));
        assert!(output.contains("├─ 1 [name-1] NAME"));
        assert!(output.contains("└─ 2 [options-2] OPTIONS"));
        assert!(output.contains("└─ 2.1 [common-3] Common options"));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--node", "2.1", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(value["schema"], "mant.excerpt/v2");
        assert_eq!(value["selections"][0]["path"], "2.1");
        assert_eq!(value["selections"][0]["section"]["title"], "Common options");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["demo", "--node", "0", "--format", "json", "--compact"],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("tldr excerpt JSON");
        assert_eq!(value["selections"][0]["kind"], "tldr");
        assert_eq!(value["selections"][0]["path"], "0");
        assert_eq!(value["selections"][0]["document"]["title"], "demo");
        assert!(value.get("producer").is_none());
        assert!(value.get("diagnostics").is_none());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn markdown_is_clean_by_default_and_preserves_anchors_on_request() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(&["demo"], b"", &host);
        assert_eq!(status, 0);
        assert!(!output.contains("<a "));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--preserve-anchors"], b"", &host);
        assert_eq!(status, 0);
        assert!(output.contains("<a id=\"name-1\"></a>"));
        assert!(output.contains("<a id=\"options-2\"></a>"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn explains_one_semantic_entry_without_changing_the_excerpt_contract() {
        let host = FakeHost::with_explainable_manual();
        let (status, output, diagnostics) = invoke(&["demo", "--explain", "--exclude"], b"", &host);

        assert_eq!(status, 0);
        assert!(output.contains("Outline `2/o1`: OPTIONS → --exclude"));
        assert!(output.contains("--exclude=PATTERN"));
        assert!(output.contains("Exclude matching files from the archive."));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &[
                "demo",
                "--explain=--exclude",
                "--format",
                "json",
                "--compact",
            ],
            b"",
            &host,
        );
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(value["schema"], "mant.excerpt/v2");
        assert_eq!(value["selections"][0]["kind"], "manual-entry");
        assert_eq!(value["selections"][0]["id"], "exclude");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--explain=2"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("--explain requires one option"));
    }

    #[test]
    fn forced_queries_print_native_findings_on_stderr() {
        let mut host = FakeHost::with_manual();
        host.manual
            .as_mut()
            .expect("manual")
            .diagnostics
            .push(Diagnostic {
                level: DiagnosticLevel::Unsupported,
                code: None,
                message: "unsupported roff request: xx".to_owned(),
                source: Some(SourceSpan {
                    line: 42,
                    column: 3,
                    end_line: None,
                    end_column: None,
                }),
            });

        let (status, output, diagnostics) =
            invoke(&["demo", "--outline", "--force-libmandoc"], b"", &host);

        assert_eq!(status, 0);
        assert!(output.contains("[name-1] NAME"));
        assert_eq!(
            diagnostics,
            "mant: libmandoc Unsupported at 42:3: unsupported roff request: xx\n"
        );
    }

    #[test]
    fn forced_groff_labels_renderer_findings_on_stderr() {
        let mut host = FakeHost::with_manual();
        let manual = host.manual.as_mut().expect("manual");
        manual.source.format = SourceFormat::GroffHtml;
        manual.diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: None,
            message: "renderer warning".to_owned(),
            source: None,
        });

        let (status, _output, diagnostics) =
            invoke(&["demo", "--outline", "--force-groff"], b"", &host);

        assert_eq!(status, 0);
        assert_eq!(diagnostics, "mant: groff HTML Warning: renderer warning\n");
    }

    #[test]
    fn searches_report_markdown_coordinates_and_reusable_outline_nodes() {
        let host = FakeHost::with_manual_and_tldr();
        let (status, output, diagnostics) = invoke(
            &[
                "demo",
                "--search",
                "common details",
                "--format",
                "json",
                "--compact",
            ],
            b"",
            &host,
        );

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
        assert_eq!(value["schema"], "mant.search/v1");
        assert_eq!(value["total"], 1);
        assert_eq!(value["matches"][0]["node"]["path"], "2.1");
        assert_eq!(value["matches"][0]["section"]["id"], "common-3");
        assert!(value["matches"][0]["markdown"]["startLine"].as_u64() > Some(1));
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--grep", "missing"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(output, "No matches for \"missing\" in demo(1).\n");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn stdin_search_requests_use_the_same_projection_contract() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v2","topic":"demo","view":{"kind":"search","pattern":"options","limit":10}}"#,
            &host,
        );

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
        assert_eq!(value["schema"], "mant.search/v1");
        assert_eq!(value["query"]["syntax"], "literal");
        assert_eq!(value["query"]["scope"], "visible");
        assert!(
            value["matches"]
                .as_array()
                .is_some_and(|matches| !matches.is_empty())
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn unknown_nodes_are_concise_usage_failures() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) =
            invoke(&["demo", "--node", "9", "--format", "text"], b"", &host);

        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("document 'demo' has no outline node '9'"));
        assert!(diagnostics.contains("mant demo --outline"));
    }

    #[test]
    fn update_and_protocol_results_are_stable_json_documents() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--update-tldr", "--compact"], b"", &host);
        assert_eq!(status, 0);
        assert_eq!(
            output,
            "{\"action\":\"updated\",\"cacheDir\":\"/cache/tldr\",\"revision\":\"abc123\"}\n"
        );
        assert!(diagnostics.is_empty());
        assert_eq!(host.update_calls.get(), 1);

        let (status, output, diagnostics) =
            invoke(&["--protocol-version", "--compact"], b"", &host);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("protocol JSON");
        assert_eq!(value["protocol"], CLI_PROTOCOL_VERSION);
        assert_eq!(value["nativeApiVersion"], "2");
        assert_eq!(value["requestSchema"], "mant.request/v2");
        assert_eq!(value["outlineSchema"], "mant.outline/v2");
        assert_eq!(value["excerptSchema"], "mant.excerpt/v2");
        assert_eq!(value["searchSchema"], "mant.search/v1");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn usage_errors_are_concise_and_never_trigger_side_effects() {
        let host = FakeHost::new();
        let (status, output, diagnostics) = invoke(&["--unknown"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.starts_with("error: unexpected argument '--unknown'"));
        assert!(diagnostics.contains("Usage: mant"));
        assert!(diagnostics.contains("For more information, try '--help'."));
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);
    }

    #[test]
    fn generated_schemas_are_json_only_and_side_effect_free() {
        let host = FakeHost::new();
        let (status, output, diagnostics) =
            invoke(&["--schema", "request", "--compact"], b"", &host);

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("request schema");
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["additionalProperties"], false);
        assert!(output.contains("mant.request/v2"));
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);

        let (status, output, diagnostics) = invoke(&["--schema", "all"], b"", &host);
        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("schema catalog");
        assert!(value["request"].is_object());
        assert!(value["query"].is_object());
        assert!(value["outline"].is_object());
        assert!(value["excerpt"].is_object());
        assert!(value["search"].is_object());
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 0);
        assert_eq!(host.update_calls.get(), 0);
    }
}
