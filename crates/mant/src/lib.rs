//! Public process boundary for `ManT`'s native document CLI.
//!
//! `mant` is both an interactive reader and an agent-friendly command with a
//! versioned stdio boundary. Standard output is reserved for the requested
//! document; diagnostics go to standard error.

mod arguments;
mod mcp;

use std::io::{self, IsTerminal, Read, Write};

use mant_ast::{
    ExcerptSelection, QueryBundle, QueryInput, QueryRequest, QueryView, SearchQuery, SourceFormat,
    TldrCacheUpdate,
};
use mant_core::{ProjectionError, QueryError, QueryPolicy, SearchError};
use mant_sources::DocumentSourcesUpdate;
use serde::Serialize;

use arguments::{Command, QueryFormat, QueryPresentation, QuerySource, SchemaContract};

// ── Stable process protocol ────────────────────────────────────────────────

/// Exact stdio protocol exposed to external process clients.
pub const CLI_PROTOCOL_VERSION: &str = "mant.cli/v5";

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
    presentation: QueryPresentation,
    pretty: bool,
    manual_only: bool,
    explain: bool,
    preserve_anchors: bool,
}

/// Terminal capabilities consulted only by the OS process entry point.
///
/// The injectable [`run`] boundary intentionally remains deterministic and
/// treats `Auto` as conventional Markdown output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalCapabilities {
    input: bool,
    output: bool,
}

// ── Host boundary ─────────────────────────────────────────────────────────

trait CliHost {
    fn query(&self, request: &QueryRequest, policy: QueryPolicy) -> Result<QueryBundle, Failure>;
    fn query_markdown(&self, source: &str) -> Result<QueryBundle, Failure>;
    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure>;
    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure>;
}

struct SystemHost;

impl CliHost for SystemHost {
    fn query(&self, request: &QueryRequest, policy: QueryPolicy) -> Result<QueryBundle, Failure> {
        mant_core::query_with_policy(request, policy).map_err(|error| match error {
            QueryError::EmptyName
            | QueryError::InvalidSection
            | QueryError::InvalidSource
            | QueryError::ConflictingSourceSelectors
            | QueryError::EmptyMarkdownPath => Failure::usage(error),
            _ => Failure::operational(error),
        })
    }

    fn query_markdown(&self, source: &str) -> Result<QueryBundle, Failure> {
        mant_core::query_markdown_text(source, None).map_err(Failure::operational)
    }

    fn update_tldr(&self) -> Result<TldrCacheUpdate, Failure> {
        mant_core::update_tldr_cache().map_err(Failure::operational)
    }

    fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure> {
        mant_sources::update_document_sources().map_err(Failure::operational)
    }
}

// ── Process execution ─────────────────────────────────────────────────────

/// Run one CLI invocation using explicit streams and return its exit status.
///
/// Keeping the process streams injectable makes malformed protocol requests
/// testable without consulting host manual sources or a tldr client.
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
    let mut command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_argument_error(&error, &mut io::stderr().lock()),
    };

    if matches!(command, Command::Mcp) {
        return mcp::run_stdio().await;
    }

    if let Err(error) = resolve_process_presentation(
        &mut command,
        TerminalCapabilities {
            input: io::stdin().is_terminal(),
            output: io::stdout().is_terminal(),
        },
    ) {
        return report_failure(&error, &mut io::stderr().lock());
    }

    if matches!(
        command,
        Command::Query {
            presentation: QueryPresentation::Interactive,
            ..
        }
    ) {
        return run_interactive(command, &mut io::stderr().lock(), &SystemHost);
    }

    run_command(
        command,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
        &SystemHost,
    )
}

/// Resolve terminal-sensitive defaults without coupling argument parsing to
/// operating-system streams.
fn resolve_process_presentation(
    command: &mut Command,
    terminal: TerminalCapabilities,
) -> Result<(), Failure> {
    let Command::Query { presentation, .. } = command else {
        return Ok(());
    };
    match *presentation {
        QueryPresentation::Auto if terminal.input && terminal.output => {
            *presentation = QueryPresentation::Interactive;
        }
        QueryPresentation::Auto => {
            *presentation = QueryPresentation::Output(QueryFormat::Markdown);
        }
        QueryPresentation::Interactive if !terminal.input || !terminal.output => {
            return Err(Failure::usage(
                "interactive view requires an input and output terminal; omit --ui or select --format",
            ));
        }
        QueryPresentation::Interactive | QueryPresentation::Output(_) => {}
    }
    Ok(())
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
    if matches!(
        command,
        Command::Query {
            presentation: QueryPresentation::Interactive,
            ..
        }
    ) {
        return report_failure(
            &Failure::usage("interactive mode requires the native terminal process boundary"),
            diagnostics,
        );
    }

    let (rendered, success_status) = match command {
        Command::UpdateDocs { pretty } => {
            let update = match host.update_docs() {
                Ok(update) => update,
                Err(error) => return report_failure(&error, diagnostics),
            };
            let status = u8::from(update.has_failures());
            let rendered = match render_json(&update, pretty) {
                Ok(rendered) => rendered,
                Err(error) => return report_failure(&error, diagnostics),
            };
            (rendered, status)
        }
        command => match execute(command, input, host) {
            Ok(rendered) => (rendered, 0),
            Err(error) => return report_failure(&error, diagnostics),
        },
    };

    match write_output(output, &rendered) {
        Ok(()) => success_status,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => success_status,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
}

fn execute(command: Command, input: &mut dyn Read, host: &dyn CliHost) -> Result<String, Failure> {
    match command {
        Command::Help(help) => Ok(help),
        Command::ProtocolVersion { pretty } => render_json(
            &ProtocolDescription {
                protocol: CLI_PROTOCOL_VERSION,
                native_api_version: mant_core::native_api_version(),
                request_schema: "mant.request/v5",
                query_schema: "mant.query/v4",
                document_schema: "mant.document/v4",
                outline_schema: "mant.outline/v4",
                excerpt_schema: "mant.excerpt/v4",
                search_schema: "mant.search/v4",
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
        Command::UpdateDocs { .. } => {
            unreachable!("document updates are dispatched before normal execution")
        }
        Command::UpdateTldr { pretty } => {
            let update = host.update_tldr()?;
            mant_core::render_update_json(&update, pretty).map_err(Failure::operational)
        }
        Command::Query {
            source,
            presentation,
            pretty,
            manual_only,
            explain,
            preserve_anchors,
        } => execute_query(
            QueryExecution {
                source,
                presentation,
                pretty,
                manual_only,
                explain,
                preserve_anchors,
            },
            input,
            host,
        ),
    }
}

/// Load one manual query and render the projection encoded in its request.
fn execute_query(
    command: QueryExecution,
    input: &mut dyn Read,
    host: &dyn CliHost,
) -> Result<String, Failure> {
    let policy = QueryPolicy {
        manual_only: command.manual_only,
    };
    let (query, view) = match command.source {
        QuerySource::MarkdownStdin { view } => {
            validate_markdown_policy(policy)?;
            let source = read_utf8_input(input, mant_core::MAX_MARKDOWN_BYTES, "Markdown input")?;
            (host.query_markdown(&source)?, view)
        }
        source => {
            let request = read_query_request(source, input)?;
            validate_query_request(&request)?;
            if matches!(request.input, QueryInput::MarkdownFile { .. }) {
                validate_markdown_policy(policy)?;
            }
            let view = request.view.clone();
            (host.query(&request, policy)?, view)
        }
    };
    let format = match command.presentation {
        QueryPresentation::Auto => QueryFormat::Markdown,
        QueryPresentation::Output(format) => format,
        QueryPresentation::Interactive => {
            return Err(Failure::usage(
                "interactive mode requires the native terminal process boundary",
            ));
        }
    };
    render_query_view(
        &query,
        view,
        format,
        command.pretty,
        command.explain,
        command.preserve_anchors,
    )
}

/// Load one full query and hand the normalized document directly to Ratatui.
fn run_interactive(command: Command, diagnostics: &mut dyn Write, host: &dyn CliHost) -> u8 {
    let Command::Query {
        source,
        presentation: QueryPresentation::Interactive,
        manual_only,
        ..
    } = command
    else {
        return report_failure(
            &Failure::usage("interactive mode requires a document query"),
            diagnostics,
        );
    };
    let QuerySource::Arguments(request) = source else {
        return report_failure(
            &Failure::usage("interactive mode requires a document name or Markdown path"),
            diagnostics,
        );
    };
    if !matches!(request.view, QueryView::Full {}) {
        return report_failure(
            &Failure::usage("interactive mode requires the complete document view"),
            diagnostics,
        );
    }
    let policy = QueryPolicy { manual_only };
    if matches!(request.input, QueryInput::MarkdownFile { .. })
        && let Err(error) = validate_markdown_policy(policy)
    {
        return report_failure(&error, diagnostics);
    }
    let query = match host.query(&request, policy) {
        Ok(query) => query,
        Err(error) => return report_failure(&error, diagnostics),
    };
    match mant_ui::run(&query) {
        Ok(()) => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics),
    }
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
        [ExcerptSelection::DocumentEntry { .. }]
    ) {
        return Ok(());
    }
    Err(Failure::usage(
        "--explain requires one option, command, or environment variable; use --node for sections",
    ))
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
        QueryFormat::Man => {
            let Some(document) = query.document.as_ref() else {
                return Err(Failure::operational(
                    "manual page is unavailable; --format man cannot render tldr-only content",
                ));
            };
            if document.source.format == SourceFormat::Markdown {
                return Err(Failure::usage(
                    "--format man applies only to roff manual pages",
                ));
            }
            Ok(mant_core::render_query_man(query))
        }
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
    match source {
        QuerySource::Arguments(request) => return Ok(request),
        QuerySource::StdinJson => {}
        QuerySource::MarkdownStdin { .. } => {
            unreachable!("Markdown stdin is consumed before protocol request decoding");
        }
    }

    let request = read_utf8_input(input, MAX_REQUEST_BYTES, "request JSON")?;
    serde_json::from_str(&request)
        .map_err(|error| Failure::usage(format!("invalid query request JSON: {error}")))
}

fn read_utf8_input(input: &mut dyn Read, limit: u64, label: &str) -> Result<String, Failure> {
    let mut bytes = Vec::new();
    input
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Failure::usage(format!("cannot read {label}: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(Failure::usage(format!(
            "{label} exceeds the {limit}-byte limit"
        )));
    }
    String::from_utf8(bytes).map_err(|_| Failure::usage(format!("{label} must be UTF-8")))
}

fn validate_markdown_policy(policy: QueryPolicy) -> Result<(), Failure> {
    if policy.manual_only {
        return Err(Failure::usage(
            "the manual-only policy does not apply to Markdown input",
        ));
    }
    Ok(())
}

fn validate_query_request(request: &QueryRequest) -> Result<(), Failure> {
    match &request.input {
        QueryInput::Document {
            name,
            source,
            section,
        } => {
            if name.trim().is_empty() {
                return Err(Failure::usage("document name must not be empty"));
            }
            if source
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(Failure::usage("document source must not be empty"));
            }
            if source.is_some() && section.is_some() {
                return Err(Failure::usage(
                    "document source and manual section cannot be combined",
                ));
            }
            if section
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(Failure::usage("manual section must not be empty"));
            }
        }
        QueryInput::MarkdownFile { path } => {
            if path.trim().is_empty() {
                return Err(Failure::usage("Markdown path must not be empty"));
            }
        }
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

    use mant_sources::{DocumentSourcesUpdate, DocumentSourcesUpdateSchema};

    use mant_ast::{
        Block, DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta, DocumentSchema,
        DocumentSource, Inline, LayoutHint, MantDocument, Producer, QueryBundle, QueryInput,
        QueryRequest, QuerySchema, Section, SourceFormat, TldrCacheAction, TldrCacheUpdate,
        TldrDocument, TldrOrigin,
    };

    use super::{
        CLI_PROTOCOL_VERSION, CliHost, Failure, QueryPolicy, TerminalCapabilities,
        arguments::{self, Command, QueryFormat, QueryPresentation},
        resolve_process_presentation, run_with_host,
    };

    struct FakeHost {
        query_calls: Cell<usize>,
        update_calls: Cell<usize>,
        last_policy: Cell<QueryPolicy>,
        document: Option<MantDocument>,
        tldr: Option<TldrDocument>,
    }

    #[test]
    fn terminal_capabilities_resolve_only_automatic_full_queries() {
        let mut terminal_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
        resolve_process_presentation(
            &mut terminal_query,
            TerminalCapabilities {
                input: true,
                output: true,
            },
        )
        .expect("terminal query");
        assert!(matches!(
            terminal_query,
            Command::Query {
                presentation: QueryPresentation::Interactive,
                ..
            }
        ));

        let mut redirected_query = arguments::parse(&["git".to_owned()]).expect("automatic query");
        resolve_process_presentation(
            &mut redirected_query,
            TerminalCapabilities {
                input: true,
                output: false,
            },
        )
        .expect("redirected query");
        assert!(matches!(
            redirected_query,
            Command::Query {
                presentation: QueryPresentation::Output(QueryFormat::Markdown),
                ..
            }
        ));

        let mut outline =
            arguments::parse(&["git".to_owned(), "--outline".to_owned()]).expect("outline query");
        resolve_process_presentation(
            &mut outline,
            TerminalCapabilities {
                input: true,
                output: true,
            },
        )
        .expect("outline remains non-interactive");
        assert!(matches!(
            outline,
            Command::Query {
                presentation: QueryPresentation::Output(QueryFormat::Text),
                ..
            }
        ));
    }

    #[test]
    fn explicit_interactive_queries_require_both_terminal_streams() {
        for terminal in [
            TerminalCapabilities {
                input: false,
                output: true,
            },
            TerminalCapabilities {
                input: true,
                output: false,
            },
        ] {
            let mut command =
                arguments::parse(&["git".to_owned(), "--ui".to_owned()]).expect("UI query");
            let error = resolve_process_presentation(&mut command, terminal)
                .expect_err("incomplete terminal must fail");
            assert!(error.message.contains("interactive view requires"));
        }
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                query_calls: Cell::new(0),
                update_calls: Cell::new(0),
                last_policy: Cell::new(QueryPolicy::default()),
                document: None,
                tldr: None,
            }
        }

        fn with_manual() -> Self {
            Self {
                document: Some(manual()),
                ..Self::new()
            }
        }

        fn with_manual_and_tldr() -> Self {
            Self {
                document: Some(manual()),
                tldr: Some(tldr()),
                ..Self::new()
            }
        }

        fn with_tldr() -> Self {
            Self {
                tldr: Some(tldr()),
                ..Self::new()
            }
        }

        fn with_explainable_manual() -> Self {
            Self {
                document: Some(explainable_manual()),
                ..Self::new()
            }
        }
    }

    impl CliHost for FakeHost {
        fn query(
            &self,
            request: &QueryRequest,
            policy: QueryPolicy,
        ) -> Result<QueryBundle, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            self.last_policy.set(policy);
            let label = match &request.input {
                QueryInput::Document { name, .. } => name.trim().to_owned(),
                QueryInput::MarkdownFile { path } => path.clone(),
            };
            Ok(QueryBundle {
                schema: QuerySchema::V4,
                label,
                document: self.document.clone(),
                tldr: self.tldr.clone(),
            })
        }

        fn query_markdown(&self, _source: &str) -> Result<QueryBundle, Failure> {
            self.query_calls.set(self.query_calls.get() + 1);
            Ok(QueryBundle {
                schema: QuerySchema::V4,
                label: "stdin".to_owned(),
                document: self.document.clone(),
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

        fn update_docs(&self) -> Result<DocumentSourcesUpdate, Failure> {
            Ok(DocumentSourcesUpdate {
                schema: DocumentSourcesUpdateSchema::V1,
                config: "/data/mant/sources.toml".to_owned(),
                sources: Vec::new(),
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
            schema: DocumentSchema::V4,
            producer: Producer {
                name: "test".to_owned(),
                version: "1".to_owned(),
                engine: None,
            },
            source: DocumentSource {
                format: SourceFormat::Man,
                path: Some("/man/demo.1".to_owned()),
            },
            meta: DocumentMeta {
                section: Some("1".to_owned()),
                ..DocumentMeta::default()
            },
            diagnostics: Vec::new(),
            blocks: Vec::new(),
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
            origin: TldrOrigin::TldrPages,
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
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git","section":"1"},"view":{"kind":"full"}}"#,
            &host,
        );

        assert_eq!(status, 0);
        assert_eq!(output, "{\"schema\":\"mant.query/v4\",\"label\":\"git\"}\n");
        assert!(diagnostics.is_empty());
        assert_eq!(host.query_calls.get(), 1);
    }

    #[test]
    fn malformed_or_extended_requests_fail_before_querying_the_host() {
        for input in [
            br"not-json".as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"full"},"futureField":true}"#.as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"   "},"view":{"kind":"full"}}"#.as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"excerpt","nodes":[]}}"#.as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"search","pattern":"","limit":10}}"#.as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"search","pattern":"git","limit":0}}"#.as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"search","pattern":"git","contextLines":101}}"#.as_slice(),
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"git"},"view":{"kind":"search","pattern":"[","syntax":"regex"}}"#.as_slice(),
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
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"demo"},"view":{"kind":"outline","detail":"sections"}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let outline: serde_json::Value = serde_json::from_str(&output).expect("outline JSON");
        assert_eq!(outline["schema"], "mant.outline/v4");
        assert_eq!(outline["detail"], "sections");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(
            &["--request-json", "--format", "json", "--compact"],
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"demo"},"view":{"kind":"excerpt","nodes":["2.1"]}}"#,
            &host,
        );
        assert_eq!(status, 0);
        let excerpt: serde_json::Value = serde_json::from_str(&output).expect("excerpt JSON");
        assert_eq!(excerpt["schema"], "mant.excerpt/v4");
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
        assert_eq!(value["schema"], "mant.excerpt/v4");
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
    fn man_format_rejects_a_tldr_only_result() {
        let host = FakeHost::with_tldr();
        let (status, output, diagnostics) = invoke(&["demo", "--format", "man"], b"", &host);

        assert_eq!(status, 1);
        assert!(output.is_empty());
        assert_eq!(
            diagnostics,
            "mant: manual page is unavailable; --format man cannot render tldr-only content\n"
        );
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
        assert_eq!(value["schema"], "mant.excerpt/v4");
        assert_eq!(value["selections"][0]["kind"], "document-entry");
        assert_eq!(value["selections"][0]["id"], "exclude");
        assert!(diagnostics.is_empty());

        let (status, output, diagnostics) = invoke(&["demo", "--explain=2"], b"", &host);
        assert_eq!(status, 2);
        assert!(output.is_empty());
        assert!(diagnostics.contains("--explain requires one option"));
    }

    #[test]
    fn manual_option_reaches_the_resolution_policy_without_stderr_noise() {
        let host = FakeHost::with_manual();
        let (status, output, diagnostics) = invoke(&["demo", "--outline", "--manual"], b"", &host);

        assert_eq!(status, 0);
        assert!(output.contains("[name-1] NAME"));
        assert!(diagnostics.is_empty());
        assert_eq!(host.last_policy.get(), QueryPolicy { manual_only: true });
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
        assert_eq!(value["schema"], "mant.search/v4");
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
            br#"{"schema":"mant.request/v5","input":{"kind":"document","name":"demo"},"view":{"kind":"search","pattern":"options","limit":10}}"#,
            &host,
        );

        assert_eq!(status, 0);
        let value: serde_json::Value = serde_json::from_str(&output).expect("search JSON");
        assert_eq!(value["schema"], "mant.search/v4");
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
        assert_eq!(value["nativeApiVersion"], "5");
        assert_eq!(value["requestSchema"], "mant.request/v5");
        assert_eq!(value["outlineSchema"], "mant.outline/v4");
        assert_eq!(value["excerptSchema"], "mant.excerpt/v4");
        assert_eq!(value["searchSchema"], "mant.search/v4");
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
        assert!(output.contains("mant.request/v5"));
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
